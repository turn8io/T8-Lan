use crate::settings::{self, Settings};
use crate::switching;
use std::sync::Mutex;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub const TRAY_ID: &str = "main";

/// Laatst gezette online-stip, zodat we het icoon alleen vervangen als de status wisselt
/// (elke `set_icon` laat het tray-icoon even knipperen).
static LAST_BADGE: Mutex<Option<bool>> = Mutex::new(None);

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
        // Windows kapt tooltips af op 127 tekens; netjes zelf inkorten.
        let text: String = text.chars().take(120).collect();
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Teken een groene (online) of rode (offline) stip rechtsonder op het tray-icoon. Zo is
/// de internetstatus altijd zichtbaar in de taakbalk, zonder te hoeven hoveren.
pub fn set_online_badge(app: &AppHandle, online: bool) {
    if let Ok(mut last) = LAST_BADGE.lock() {
        if *last == Some(online) {
            return;
        }
        *last = Some(online);
    }
    let Some(base) = app.default_window_icon() else {
        return;
    };
    let (w, h) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();
    if rgba.len() != (w as usize) * (h as usize) * 4 {
        return;
    }
    let color = if online { (74, 222, 128) } else { (239, 68, 68) };
    draw_badge(&mut rgba, w, h, color);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(Image::new(&rgba, w, h)));
    }
}

/// Gevulde cirkel met donkere rand, rechtsonder, ~21% van de icoonmaat (≈3-4 px op een
/// 16 px tray-icoon). Eenvoudige anti-aliasing op afstand tot de rand.
fn draw_badge(rgba: &mut [u8], w: u32, h: u32, (r, g, b): (u8, u8, u8)) {
    let size = w.min(h) as f32;
    let radius = (size * 0.21).max(2.5);
    let ring = radius + (size * 0.06).max(1.0);
    let cx = w as f32 - ring - 0.5;
    let cy = h as f32 - ring - 0.5;
    let coverage = |d: f32| (d + 0.5).clamp(0.0, 1.0);

    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > ring + 0.5 {
                continue;
            }
            let (col, alpha) = if dist <= radius + 0.5 {
                ((r, g, b), coverage(radius - dist))
            } else {
                ((16u8, 16u8, 18u8), coverage(ring - dist))
            };
            // Rand en vulling overlappen: binnen de vulling de rand eerst meenemen.
            let ring_alpha = if dist <= radius + 0.5 { coverage(ring - dist) } else { 0.0 };
            let i = ((y * w + x) * 4) as usize;
            if ring_alpha > 0.0 {
                blend(&mut rgba[i..i + 4], (16, 16, 18), ring_alpha);
            }
            blend(&mut rgba[i..i + 4], col, alpha);
        }
    }
}

fn blend(px: &mut [u8], (r, g, b): (u8, u8, u8), alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let mix = |dst: u8, src: u8| ((src as f32) * alpha + (dst as f32) * (1.0 - alpha)).round() as u8;
    // Tegen een (deels) transparante achtergrond moet de stip zelf dekkend zijn.
    let dst_a = px[3] as f32 / 255.0;
    let out_a = alpha + dst_a * (1.0 - alpha);
    if out_a <= 0.0 {
        return;
    }
    let comp = |dst: u8, src: u8| {
        (((src as f32) * alpha + (dst as f32) * dst_a * (1.0 - alpha)) / out_a).round() as u8
    };
    if dst_a >= 0.999 {
        px[0] = mix(px[0], r);
        px[1] = mix(px[1], g);
        px[2] = mix(px[2], b);
    } else {
        px[0] = comp(px[0], r);
        px[1] = comp(px[1], g);
        px[2] = comp(px[2], b);
    }
    px[3] = (out_a * 255.0).round() as u8;
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
                    Some(l) if !l.is_empty() => format!("Set Static {} - {}", recent.ip, l),
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
                update_tooltip(app, "T8-Lan: no adapter selected");
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

/// Toon het hoofdvenster. Een verborgen venster opent altijd in de compacte standaardmaat
/// op de onthouden positie (of bij de tray-klik); een al zichtbaar venster krijgt alleen
/// focus, zodat een handmatig vergroot venster niet terugspringt.
pub fn show_main(app: &AppHandle, click_at: Option<(f64, f64)>) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        if !visible {
            let saved = settings::load(app).ok().and_then(|s| s.window);
            crate::window::reset_size(&window);
            match (saved, click_at) {
                // Returning user: restore the remembered position.
                (Some(pos), _) => crate::window::position_initial(&window, Some(&pos)),
                // First time: open near the tray click.
                (None, Some((x, y))) => crate::window::position_near_point(&window, x, y),
                (None, None) => {}
            }
        }
        let _ = window.show();
        let _ = window.set_focus();
    }
}
