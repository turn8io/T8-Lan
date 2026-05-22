use crate::settings::WindowPos;
use tauri::{PhysicalPosition, WebviewWindow};

const EDGE_PADDING: i32 = 12;
const ASSUMED_TASKBAR_HEIGHT: i32 = 48;

pub fn apply_vibrancy(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        use window_vibrancy::{apply_acrylic, apply_mica};

        if apply_mica(window, Some(true)).is_err() {
            let _ = apply_acrylic(window, Some((20, 20, 24, 200)));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
    }
}

pub fn position_initial(window: &WebviewWindow, saved: Option<&WindowPos>) {
    if let Some(pos) = saved {
        if is_within_any_monitor(window, pos) {
            // Only restore position — size is fixed by the window config (non-resizable).
            // Restoring a stale saved size would override the configured dimensions.
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
    let Ok(window_size) = window.outer_size() else {
        return;
    };

    let monitor_size = monitor.size();
    let scale = monitor.scale_factor();

    let work_bottom =
        monitor_size.height as i32 - (ASSUMED_TASKBAR_HEIGHT as f64 * scale) as i32;
    let x = monitor_size.width as i32 - window_size.width as i32 - EDGE_PADDING;
    let y = work_bottom - window_size.height as i32 - EDGE_PADDING;

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Position the window just above a clicked point (e.g. the tray icon, which
/// may be in the taskbar OR in the overflow flyout higher up the screen).
/// The window's bottom edge sits above the click so the content — including the
/// IP input near the top — is immediately visible and reachable.
pub fn position_near_point(window: &WebviewWindow, click_x: f64, click_y: f64) {
    let Ok(window_size) = window.outer_size() else {
        return;
    };
    let cx = click_x as i32;
    let cy = click_y as i32;

    // Find the monitor that contains the click (handles multi-monitor + the
    // overflow flyout, which can be on any screen). Fall back to primary.
    let (mon_x, mon_y, mon_w, mon_h) = monitor_containing(window, cx, cy);

    let w = window_size.width as i32;
    let h = window_size.height as i32;
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
