use crate::settings::WindowPos;
use tauri::{LogicalSize, PhysicalPosition, WebviewWindow};

const EDGE_PADDING: i32 = 12;
const ASSUMED_TASKBAR_HEIGHT: i32 = 48;

/// Standaardafmeting (logische pixels) waarin het venster altijd opent. Het venster is
/// daarna vrij te vergroten/verkleinen, maar elke keer dat het (opnieuw) getoond wordt
/// begint het compact.
pub const DEFAULT_WIDTH: f64 = 240.0;
pub const DEFAULT_HEIGHT: f64 = 350.0;

/// Zet het venster terug op de compacte standaardmaat.
pub fn reset_size(window: &WebviewWindow) {
    let _ = window.set_size(LogicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT));
}

/// Standaardmaat in fysieke pixels voor het scherm waar het venster staat. We rekenen
/// hiermee i.p.v. `outer_size()`, omdat een net gedane `set_size` op Windows pas even
/// later in `outer_size()` zichtbaar is.
fn default_physical_size(window: &WebviewWindow) -> (i32, i32) {
    let scale = window.scale_factor().unwrap_or(1.0);
    (
        (DEFAULT_WIDTH * scale).round() as i32,
        (DEFAULT_HEIGHT * scale).round() as i32,
    )
}

/// Opstart-/toonpositie: altijd eerst de compacte maat, dan de onthouden positie (als
/// die nog op een scherm ligt), anders rechtsonder.
pub fn position_initial(window: &WebviewWindow, saved: Option<&WindowPos>) {
    reset_size(window);
    if let Some(pos) = saved {
        if is_within_any_monitor(window, pos) {
            // Alleen de positie herstellen; de maat is net bewust teruggezet.
            let _ = window.set_position(PhysicalPosition::new(pos.x, pos.y));
            return;
        }
    }
    position_default_bottom_right(window);
}

pub fn position_default_bottom_right(window: &WebviewWindow) {
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return;
    };
    let (w, h) = default_physical_size(window);

    let monitor_size = monitor.size();
    let scale = monitor.scale_factor();

    let work_bottom =
        monitor_size.height as i32 - (ASSUMED_TASKBAR_HEIGHT as f64 * scale) as i32;
    let x = monitor_size.width as i32 - w - EDGE_PADDING;
    let y = work_bottom - h - EDGE_PADDING;

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Position the window just above a clicked point (e.g. the tray icon, which
/// may be in the taskbar OR in the overflow flyout higher up the screen).
/// The window's bottom edge sits above the click so the content — including the
/// IP input near the top — is immediately visible and reachable.
pub fn position_near_point(window: &WebviewWindow, click_x: f64, click_y: f64) {
    let (w, h) = default_physical_size(window);
    let cx = click_x as i32;
    let cy = click_y as i32;

    // Find the monitor that contains the click (handles multi-monitor + the
    // overflow flyout, which can be on any screen). Fall back to primary.
    let (mon_x, mon_y, mon_w, mon_h) = monitor_containing(window, cx, cy);

    let edge = 4; // flush against the right edge

    // Flush to the monitor's right edge.
    let x = mon_x + mon_w - w - edge;

    // Above the click (tray / flyout), a touch higher.
    let mut y = cy - h - edge - 10;
    if y < mon_y + edge {
        y = cy + edge;
    }
    y = y.clamp(mon_y + edge, mon_y + mon_h - h - edge);

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

fn monitor_containing(window: &WebviewWindow, x: i32, y: i32) -> (i32, i32, i32, i32) {
    if let Ok(monitors) = window.available_monitors() {
        for m in &monitors {
            let p = m.position();
            let s = m.size();
            let right = p.x + s.width as i32;
            let bottom = p.y + s.height as i32;
            if x >= p.x && x < right && y >= p.y && y < bottom {
                return (p.x, p.y, s.width as i32, s.height as i32);
            }
        }
    }
    match window.primary_monitor() {
        Ok(Some(m)) => {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width as i32, s.height as i32)
        }
        _ => (0, 0, 1920, 1080),
    }
}

fn is_within_any_monitor(window: &WebviewWindow, pos: &WindowPos) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    for m in monitors {
        let m_pos = m.position();
        let m_size = m.size();
        let m_right = m_pos.x + m_size.width as i32;
        let m_bottom = m_pos.y + m_size.height as i32;

        let visible_left_edge = pos.x + 40;
        let visible_top_edge = pos.y + 20;
        if visible_left_edge >= m_pos.x
            && visible_left_edge < m_right
            && visible_top_edge >= m_pos.y
            && visible_top_edge < m_bottom
        {
            return true;
        }
    }
    false
}
