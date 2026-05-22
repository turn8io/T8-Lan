use crate::network::{adapter, ip, ping, wifi, AdapterInfo, CurrentStatus, StatusState};
use crate::settings::{self, Settings};
use crate::switching::{self, PreviousConfig, UndoState};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use tauri_plugin_opener::OpenerExt;
use tokio::time::{interval, Duration, MissedTickBehavior};

pub struct PingController {
    task: crate::util::AbortableTask,
}

impl PingController {
    pub fn new() -> Self {
        Self {
            task: crate::util::AbortableTask::new(),
        }
    }
}

#[tauri::command]
pub fn get_adapters() -> Result<Vec<AdapterInfo>, String> {
    adapter::list_adapters()
}

#[tauri::command]
pub fn get_current_status(adapter_name: Option<String>) -> Result<CurrentStatus, String> {
    let adapters = adapter::list_adapters()?;
    let selected = match adapter_name {
        Some(name) => adapters.into_iter().find(|a| a.friendly_name == name),
        None => adapters
            .iter()
            .find(|a| a.is_up && a.current_ip.is_some())
            .cloned()
            .or_else(|| adapter::list_adapters().ok().and_then(|v| v.into_iter().next())),
    };

    let (state, message) = match &selected {
        Some(a) if !a.is_up => (StatusState::Error, "Adapter is down".into()),
        Some(a) if a.current_ip.is_none() && a.is_dhcp => {
            (StatusState::WaitingDhcp, "Wacht op DHCP lease...".into())
        }
        Some(a) if a.current_ip.is_none() => (StatusState::Error, "Geen IP toegekend".into()),
        Some(a) => (
            StatusState::Idle,
            if a.is_dhcp { "DHCP".into() } else { "Statisch".into() },
        ),
        None => (StatusState::Error, "Geen adapter gevonden".into()),
    };

    Ok(CurrentStatus {
        adapter: selected,
        state,
        message,
    })
}

#[tauri::command]
pub async fn switch_to_dhcp(app: AppHandle, adapter_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || switching::do_dhcp(&app, &adapter_name, "ui"))
        .await
        .map_err(|e| format!("switch worker faalt: {e}"))
}

#[tauri::command]
pub async fn switch_to_static(
    app: AppHandle,
    adapter_name: String,
    ip_addr: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || switching::do_static(&app, &adapter_name, &ip_addr, "ui"))
        .await
        .map_err(|e| format!("switch worker faalt: {e}"))
}

#[tauri::command]
pub fn check_ip_conflict(ip: String) -> Result<Option<String>, String> {
    ip::check_ip_conflict(&ip, 300)
}

#[tauri::command]
pub async fn scan_devices(app: AppHandle, subnet_base: String) -> Result<(), String> {
    use crate::network::scan::ScanEvent;
    tokio::task::spawn_blocking(move || {
        let emitter = app.clone();
        // Gevonden apparaten → "scan-device"; voortgang van de sweep → "scan-progress".
        let result = crate::network::scan::scan_devices(&subnet_base, move |ev| match ev {
            ScanEvent::Device(device) => {
                let _ = emitter.emit("scan-device", device);
            }
            ScanEvent::Progress { done, total } => {
                let _ = emitter.emit("scan-progress", (done, total));
            }
        });
        // Sein einde-scan (ook bij fout), zodat de UI de status kan afronden.
        let _ = app.emit("scan-done", ());
        result
    })
    .await
    .map_err(|e| format!("spawn_blocking faalt: {e}"))?
}

#[tauri::command]
pub fn list_wifi_networks() -> Result<Vec<String>, String> {
    wifi::list_available_networks()
}

#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<Settings, String> {
    settings::load(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    settings::save(&app, &settings)?;
    let _ = crate::tray::rebuild(&app);
    Ok(())
}

#[tauri::command]
pub fn has_undo(state: tauri::State<'_, Arc<UndoState>>) -> Option<PreviousConfig> {
    state.prev.lock().ok().and_then(|g| g.clone())
}

#[tauri::command]
pub fn has_redo(state: tauri::State<'_, Arc<UndoState>>) -> Option<PreviousConfig> {
    state.next.lock().ok().and_then(|g| g.clone())
}

/// Apply a captured config to its adapter (used by both undo and redo).
fn apply_config(app: &AppHandle, cfg: &PreviousConfig) -> Result<(), String> {
    crate::tray::update_tooltip(
        app,
        &format!("T8-Lan — herstelt config op {}...", cfg.adapter_name),
    );
    if cfg.was_dhcp {
        crate::network::ip::set_dhcp(&cfg.adapter_name)?;
        let _ = crate::network::dns::set_dhcp_dns(&cfg.adapter_name);
    } else if let (Some(ip_addr), Some(subnet), Some(gateway)) =
        (cfg.ip.clone(), cfg.subnet.clone(), cfg.gateway.clone())
    {
        crate::network::ip::set_static(&cfg.adapter_name, &ip_addr, &subnet, &gateway)?;
    } else {
        return Err("Config onvolledig — kan niet herstellen".into());
    }
    Ok(())
}

/// Take the config from `from`, capture the current config into `to` (so the move
/// stays reversible), then apply the taken config. Shared by undo and redo, which
/// only differ in which slot they pop from and the message when it's empty.
fn restore_step(
    app: &AppHandle,
    from: &Mutex<Option<PreviousConfig>>,
    to: &Mutex<Option<PreviousConfig>>,
    empty_msg: &str,
) -> Result<PreviousConfig, String> {
    let cfg = from
        .lock()
        .map_err(|e| format!("lock: {e}"))?
        .take()
        .ok_or_else(|| empty_msg.to_string())?;

    if let Some(cur) = switching::capture_current(&cfg.adapter_name) {
        if let Ok(mut g) = to.lock() {
            *g = Some(cur);
        }
    }

    apply_config(app, &cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn undo_last_switch(
    app: AppHandle,
    state: tauri::State<'_, Arc<UndoState>>,
) -> Result<PreviousConfig, String> {
    restore_step(
        &app,
        &state.prev,
        &state.next,
        "Geen vorige configuratie om te herstellen",
    )
}

#[tauri::command]
pub fn redo_last_switch(
    app: AppHandle,
    state: tauri::State<'_, Arc<UndoState>>,
) -> Result<PreviousConfig, String> {
    restore_step(
        &app,
        &state.next,
        &state.prev,
        "Niets om opnieuw uit te voeren",
    )
}

// ---- Global shortcuts ----

fn parse_shortcut(s: &str) -> Option<Shortcut> {
    Shortcut::from_str(s).ok()
}

/// Re-register the configured hotkeys. Unregisters everything first.
pub fn apply_hotkeys(app: &AppHandle, s: &Settings) {
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    if !s.global_hotkey_enabled {
        return;
    }
    if let Some(sc) = parse_shortcut(&s.hotkey_dhcp) {
        let _ = manager.register(sc);
    }
    if let Some(sc) = parse_shortcut(&s.hotkey_static) {
        let _ = manager.register(sc);
    }
}

/// Called from the global-shortcut handler: figure out which hotkey fired.
pub fn handle_shortcut(app: &AppHandle, fired: &Shortcut) {
    let s = settings::load(app).unwrap_or_default();
    let dhcp = parse_shortcut(&s.hotkey_dhcp);
    let stat = parse_shortcut(&s.hotkey_static);

    if dhcp.as_ref() == Some(fired) {
        if let Some(name) = switching::selected_adapter_name(app) {
            let app = app.clone();
            std::thread::spawn(move || switching::do_dhcp(&app, &name, "hotkey"));
        } else {
            let _ = app.emit(
                "switch-result",
                serde_json::json!({ "ok": false, "msg": "Geen adapter gekozen", "trigger": "hotkey" }),
            );
        }
    } else if stat.as_ref() == Some(fired) {
        if let Some((name, ip)) = switching::last_static_ip(app) {
            let app = app.clone();
            std::thread::spawn(move || switching::do_static(&app, &name, &ip, "hotkey"));
        } else {
            let _ = app.emit(
                "switch-result",
                serde_json::json!({ "ok": false, "msg": "Geen recent statisch IP", "trigger": "hotkey" }),
            );
        }
    }
}

#[tauri::command]
pub fn update_hotkeys(app: AppHandle, settings: Settings) -> Result<(), String> {
    settings::save(&app, &settings)?;
    apply_hotkeys(&app, &settings);
    Ok(())
}

// ---- SSID watcher ----

#[tauri::command]
pub async fn ssid_watcher_set_enabled(
    app: AppHandle,
    state: tauri::State<'_, Arc<crate::ssid_watcher::SsidWatcher>>,
    enabled: bool,
) -> Result<(), String> {
    let watcher = state.inner().clone();
    if enabled {
        crate::ssid_watcher::start(app, watcher).await;
    } else {
        crate::ssid_watcher::stop(watcher).await;
    }
    Ok(())
}

// ---- Ping ----

#[tauri::command]
pub async fn ping_start(
    app: AppHandle,
    state: tauri::State<'_, Arc<PingController>>,
    target: String,
) -> Result<(), String> {
    let app_handle = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut tick = interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let t = target.clone();
            let result = tokio::task::spawn_blocking(move || ping::ping_once(&t, 1000))
                .await
                .unwrap_or_else(|e| ping::PingResult {
                    target: String::new(),
                    timestamp_ms: 0,
                    rtt_ms: None,
                    error: Some(format!("worker panicked: {e}")),
                });
            if app_handle.emit("ping-result", result).is_err() {
                break;
            }
        }
    });
    state.task.replace(handle).await;
    Ok(())
}

#[tauri::command]
pub async fn dns_ping(target: String) -> Result<ping::PingResult, String> {
    tokio::task::spawn_blocking(move || ping::ping_once(&target, 800))
        .await
        .map_err(|e| format!("ping worker faalt: {e}"))
}

#[tauri::command]
pub async fn ping_stop(state: tauri::State<'_, Arc<PingController>>) -> Result<(), String> {
    state.task.abort().await;
    Ok(())
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_window_position(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("main window niet beschikbaar")?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;

    let mut s = settings::load(&app)?;
    s.window = Some(settings::WindowPos {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    });
    settings::save(&app, &s)
}
