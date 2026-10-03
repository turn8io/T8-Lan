pub mod autostart;
mod commands;
mod network;
mod settings;
pub mod single_instance;
mod ssid_watcher;
mod switching;
mod tray;
mod util;
mod window;

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

static LAST_MOVE_SAVE: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

/// Zoveel opeenvolgende gemiste pings voordat de tray op "offline" springt. Eén gemiste
/// ping komt op 4G of een drukke klantrouter geregeld voor en is nog geen storing.
const OFFLINE_AFTER_FAILS: u8 = 2;

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        commands::handle_shortcut(app, shortcut);
                    }
                })
                .build(),
        )
        .manage(Arc::new(commands::PingController::new()))
        .manage(Arc::new(switching::UndoState::new()))
        .manage(Arc::new(ssid_watcher::SsidWatcher::new()))
        .manage(Arc::new(network::dhcp::DhcpServer::new()))
        .manage(Arc::new(network::hotspot::Hotspot::new()))
        .invoke_handler(tauri::generate_handler![
            commands::get_adapters,
            commands::get_current_status,
            commands::switch_to_dhcp,
            commands::switch_to_static,
            commands::check_ip_conflict,
            commands::scan_devices,
            commands::list_wifi_networks,
            commands::load_settings,
            commands::save_settings,
            commands::save_window_position,
            commands::has_undo,
            commands::has_redo,
            commands::undo_last_switch,
            commands::redo_last_switch,
            commands::update_hotkeys,
            commands::ssid_watcher_set_enabled,
            commands::ping_start,
            commands::ping_stop,
            commands::dns_ping,
            commands::open_external,
            commands::get_app_version,
            commands::show_main_window,
            commands::set_autostart,
            commands::autostart_registered,
            commands::dhcp_start,
            commands::dhcp_stop,
            commands::dhcp_status,
            commands::hotspot_start,
            commands::hotspot_stop,
        ])
        .setup(|app| {
            let main_window = app
                .get_webview_window("main")
                .expect("main window should exist");

            let stored_settings = settings::load(app.handle()).unwrap_or_default();
            window::position_initial(&main_window, stored_settings.window.as_ref());

            commands::apply_hotkeys(app.handle(), &stored_settings);

            if !stored_settings.ssid_rules.is_empty() {
                let watcher = app.state::<Arc<ssid_watcher::SsidWatcher>>().inner().clone();
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    ssid_watcher::start(handle, watcher).await;
                });
            }

            tray::build(app.handle())?;

            // Een tweede start (Startmenu, dubbele taak) seint dit proces: venster tonen.
            single_instance::watch_show_requests(app.handle().clone());

            // Autostart-taak (her)registreren zodat pad en instellingen altijd kloppen.
            // Alleen in release-builds: een dev-exe hoort niet in de Taakplanner.
            if !cfg!(debug_assertions) {
                let enabled = stored_settings.autostart;
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(e) = autostart::apply(enabled) {
                        eprintln!("autostart: {e}");
                    }
                });
            }

            // Tray-status elke 3 s: tooltip "DHCP 192.168.x.x - online" en een groene/rode
            // stip op het tray-icoon. Met hysterese tegen flapperen.
            let tip_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(3));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut fails: u8 = 0;
                loop {
                    tick.tick().await;
                    let h = tip_handle.clone();
                    let Ok((text, ping_ok)) =
                        tokio::task::spawn_blocking(move || switching::tray_status(&h)).await
                    else {
                        continue;
                    };
                    fails = if ping_ok { 0 } else { fails.saturating_add(1) };
                    let online = fails < OFFLINE_AFTER_FAILS;
                    let state = if online { "online" } else { "offline" };
                    tray::update_tooltip(&tip_handle, &format!("T8-Lan: {text} - {state}"));
                    tray::set_online_badge(&tip_handle, online);
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = commands::save_window_position(window.app_handle().clone());
                let _ = window.hide();
            }
            WindowEvent::Moved(_) => {
                // Save the position as the window is dragged, throttled so we
                // don't thrash the settings file during a drag.
                if let Ok(mut last) = LAST_MOVE_SAVE.lock() {
                    if last.elapsed().as_millis() >= 350 {
                        *last = Instant::now();
                        let _ = commands::save_window_position(window.app_handle().clone());
                    }
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building T8-Lan");

    app.run(|app, event| {
        // Bij afsluiten (tray "Quit") de DHCP-server netjes stoppen, zodat de adapter
        // niet op 192.168.8.8 blijft hangen.
        if let RunEvent::ExitRequested { .. } = event {
            if let Some(server) = app.try_state::<Arc<network::dhcp::DhcpServer>>() {
                if server.is_running() {
                    let _ = server.stop(app);
                }
            }
            if let Some(hotspot) = app.try_state::<Arc<network::hotspot::Hotspot>>() {
                hotspot.stop();
            }
        }
    });
}
