//! Whether the overlay should receive the mouse at a given point.
//!
//! Used when "Overlay click-through" is on: the body lets clicks reach the
//! game, while the top bar (buttons / drag) and a thin edge strip (resize)
//! keep capturing the pointer. Pure geometry so it can be unit-tested.

/// CSS pixels of border kept interactive for the frameless resize grips.
pub const EDGE_MARGIN_PX: f64 = 8.0;

/// True when the overlay window should capture the cursor (not ignore it).
///
/// `local_x` / `local_y` are in the same units as `width` / `height` /
/// `top_bar` / `edge` (CSS pixels of the overlay webview).
/// `holding` keeps capture while a drag or resize started on an interactive
/// region so the pointer can leave the bar without dropping the gesture.
pub fn receives_cursor(
    local_x: f64,
    local_y: f64,
    width: f64,
    height: f64,
    top_bar: f64,
    edge: f64,
    click_through: bool,
    holding: bool,
) -> bool {
    if !click_through || holding {
        return true;
    }
    if width <= 0.0 || height <= 0.0 {
        return true;
    }
    if local_x < 0.0 || local_y < 0.0 || local_x >= width || local_y >= height {
        // Outside the window: ignore so re-entry over the body stays click-through.
        return false;
    }
    let top = top_bar.max(0.0).min(height);
    if local_y < top {
        return true;
    }
    let m = edge.max(0.0);
    local_x < m || local_x >= width - m || local_y >= height - m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_through_off_always_captures() {
        assert!(receives_cursor(50.0, 200.0, 360.0, 400.0, 48.0, 8.0, false, false));
    }

    #[test]
    fn top_bar_captures_and_body_does_not() {
        assert!(receives_cursor(100.0, 10.0, 360.0, 400.0, 48.0, 8.0, true, false));
        assert!(receives_cursor(100.0, 47.9, 360.0, 400.0, 48.0, 8.0, true, false));
        assert!(!receives_cursor(100.0, 48.0, 360.0, 400.0, 48.0, 8.0, true, false));
        assert!(!receives_cursor(180.0, 200.0, 360.0, 400.0, 48.0, 8.0, true, false));
    }

    #[test]
    fn edge_strip_stays_interactive_for_resize() {
        assert!(receives_cursor(2.0, 200.0, 360.0, 400.0, 48.0, 8.0, true, false));
        assert!(receives_cursor(355.0, 200.0, 360.0, 400.0, 48.0, 8.0, true, false));
        assert!(receives_cursor(180.0, 395.0, 360.0, 400.0, 48.0, 8.0, true, false));
        assert!(!receives_cursor(20.0, 200.0, 360.0, 400.0, 48.0, 8.0, true, false));
    }

    #[test]
    fn holding_keeps_capture_outside_the_bar() {
        assert!(receives_cursor(180.0, 200.0, 360.0, 400.0, 48.0, 8.0, true, true));
    }

    #[test]
    fn outside_the_window_does_not_capture_when_click_through() {
        assert!(!receives_cursor(-1.0, 10.0, 360.0, 400.0, 48.0, 8.0, true, false));
        assert!(!receives_cursor(10.0, 500.0, 360.0, 400.0, 48.0, 8.0, true, false));
    }

    #[test]
    fn top_bar_is_clamped_to_the_window_height() {
        assert!(receives_cursor(10.0, 10.0, 360.0, 40.0, 999.0, 8.0, true, false));
    }
}
