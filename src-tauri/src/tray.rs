use crate::settings::{self, Settings};
use crate::switching;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub const TRAY_ID: &str = "main";

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let initial_settings = settings::load(app).unwrap_or_default();
    let menu = build_menu(app, &initial_settings)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().unwrap())
        .icon_as_template(false)
        .tooltip("T8-Lan")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                show_main(tray.app_handle(), Some((position.x, position.y)));
            }
        })
        .build(app)?;

    Ok(())
}

pub fn rebuild(app: &AppHandle) -> tauri::Result<()> {
    let settings = settings::load(app).unwrap_or_default();
    let menu = build_menu(app, &settings)?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

pub fn update_tooltip(app: &AppHandle, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

fn build_menu(app: &AppHandle, settings: &Settings) -> tauri::Result<Menu<tauri::Wry>> {
    let dhcp_item = MenuItem::with_id(app, "dhcp", "Switch to DHCP", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let open_item = MenuItem::with_id(app, "open", "Open settings", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&dhcp_item, &separator];

    let mut recent_items: Vec<MenuItem<tauri::Wry>> = Vec::new();
    if let Some(adapter_ref) = &settings.selected_adapter {
        if let Some(recents) = settings.recent_ips_by_adapter.get(&adapter_ref.friendly_name) {
            for recent in recents.iter().take(5) {
                let label = match &recent.label {
                    Some(l) if !l.is_empty() => format!("Set Static {} — {}", recent.ip, l),
                    _ => format!("Set Static {}", recent.ip),
                };
                let id = format!("recent::{}", recent.ip);
                recent_items.push(MenuItem::with_id(app, &id, label, true, None::<&str>)?);
            }
        }
    }
    for r in &recent_items {
        items.push(r);
    }
    if !recent_items.is_empty() {
        items.push(&separator);
    }

    items.push(&open_item);
    items.push(&separator);
    items.push(&quit_item);

    Menu::with_items(app, &items)
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id.as_ref().to_string();
    match id.as_str() {
        "open" => show_main(app, None),
        "quit" => app.exit(0),
        "dhcp" => {
            if let Some(name) = switching::selected_adapter_name(app) {
                let app = app.clone();
                std::thread::spawn(move || switching::do_dhcp(&app, &name, "tray"));
            } else {
                update_tooltip(app, "T8-Lan — no adapter selected");
            }
        }
        s if s.starts_with("recent::") => {
            let ip = s.trim_start_matches("recent::").to_string();
            if let Some(name) = switching::selected_adapter_name(app) {
                let app = app.clone();
                std::thread::spawn(move || switching::do_static(&app, &name, &ip, "tray"));
            }
        }
        _ => {}
    }
}

fn show_main(app: &AppHandle, click_at: Option<(f64, f64)>) {
    if let Some(window) = app.get_webview_window("main") {
        let saved = settings::load(app).ok().and_then(|s| s.window);
        match (saved, click_at) {
            // Returning user: restore the remembered position.
            (Some(pos), _) => crate::window::position_initial(&window, Some(&pos)),
            // First time: open near the tray click.
            (None, Some((x, y))) => crate::window::position_near_point(&window, x, y),
            (None, None) => {}
        }
        let _ = window.show();
        let _ = window.set_focus();
    }
}
