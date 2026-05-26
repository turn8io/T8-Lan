mod commands;
mod network;
mod settings;
mod ssid_watcher;
mod switching;
mod tray;
mod util;
mod window;

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

static LAST_MOVE_SAVE: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

pub fn run() {
    tauri::Builder::default()
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

            // Periodic tray tooltip updater: "T8-Lan — DHCP — 192.168.x.x".
            let tip_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(3));
                loop {
                    tick.tick().await;
                    let h = tip_handle.clone();
                    if let Ok(text) =
                        tokio::task::spawn_blocking(move || switching::tooltip_for_selected(&h))
                            .await
                    {
                        tray::update_tooltip(&tip_handle, &text);
                    }
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
        .run(tauri::generate_context!())
        .expect("error while running T8-Lan");
}
