//! Remembers where the overlay window was and how big it was, so it reopens
//! in the same place. Only the overlay is saved; every other window opens as
//! before. Kept in `%LOCALAPPDATA%\aion2dps\window-state.json`.
//!
//! Everything here is in physical pixels (what Windows reports for window
//! and monitor rectangles), so multi-monitor setups with negative
//! coordinates and mixed DPI need no conversion. The only logical value is
//! the window's minimum size, scaled by the target monitor's DPI.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::storage;

pub const FILE_NAME: &str = "window-state.json";
/// The only window whose position and size are remembered.
pub const OVERLAY_LABEL: &str = "overlay";
/// Windows parks minimized windows at (-32000, -32000).
const MINIMIZED: i32 = -32000;
/// How much of the window's top strip must be on a monitor to count as
/// reachable: this wide (or the whole width when narrower) and this tall.
const VISIBLE_WIDTH: i64 = 100;
const VISIBLE_HEIGHT: i64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A monitor's physical rectangle and its DPI scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub rect: Rect,
    pub scale: f64,
}

pub fn default_path() -> PathBuf {
    storage::folder().join(FILE_NAME)
}

/// Whether this window's rectangle is saved at all.
pub fn is_persisted(label: &str) -> bool {
    label == OVERLAY_LABEL
}

/// A rectangle worth saving: not the minimized sentinel, not empty.
pub fn is_savable(rect: &Rect) -> bool {
    rect.x > MINIMIZED && rect.y > MINIMIZED && rect.width > 0 && rect.height > 0
}

/// The saved overlay rectangle, or `None` when the file is missing, corrupt
/// or holds something unusable.
pub fn load(path: &Path) -> Option<Rect> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let o = value.get(OVERLAY_LABEL)?;
    let int = |key: &str| o.get(key).and_then(Value::as_i64);
    let rect = Rect {
        x: i32::try_from(int("x")?).ok()?,
        y: i32::try_from(int("y")?).ok()?,
        width: u32::try_from(int("width")?).ok()?,
        height: u32::try_from(int("height")?).ok()?,
    };
    is_savable(&rect).then_some(rect)
}

/// Save the overlay rectangle. Labels other than the overlay and unsavable
/// rectangles (minimized) are ignored and leave the file as it was.
pub fn save(path: &Path, label: &str, rect: &Rect) -> std::io::Result<bool> {
    if !is_persisted(label) || !is_savable(rect) {
        return Ok(false);
    }
    let value = json!({ OVERLAY_LABEL: {
        "x": rect.x, "y": rect.y, "width": rect.width, "height": rect.height,
    }});
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    storage::write_atomic(path, text.as_bytes())?;
    Ok(true)
}

fn overlap(a0: i64, a1: i64, b0: i64, b1: i64) -> i64 {
    (a1.min(b1) - a0.max(b0)).max(0)
}

fn right(r: &Rect) -> i64 {
    r.x as i64 + r.width as i64
}

fn bottom(r: &Rect) -> i64 {
    r.y as i64 + r.height as i64
}

/// Whether enough of the window's top strip (where it is dragged) is on
/// this monitor.
fn top_strip_visible(rect: &Rect, monitor: &Rect) -> bool {
    let strip_h = VISIBLE_HEIGHT.min(rect.height as i64);
    let need_w = VISIBLE_WIDTH.min(rect.width as i64);
    let w = overlap(rect.x as i64, right(rect), monitor.x as i64, right(monitor));
    let h = overlap(rect.y as i64, rect.y as i64 + strip_h, monitor.y as i64, bottom(monitor));
    w >= need_w && h >= strip_h
}

fn distance_sq(rect: &Rect, monitor: &Rect) -> i64 {
    let cx = rect.x as i64 + rect.width as i64 / 2;
    let cy = rect.y as i64 + rect.height as i64 / 2;
    let dx = (monitor.x as i64 - cx).max(0).max(cx - right(monitor));
    let dy = (monitor.y as i64 - cy).max(0).max(cy - bottom(monitor));
    dx * dx + dy * dy
}

/// Fit a saved rectangle to the monitors present now.
///
/// - The monitor is the one holding the top strip, else the one nearest the
///   window's center, else `primary` (index into `monitors`), else the first.
/// - The size is at least `min_logical` times that monitor's scale and at
///   most the monitor's size.
/// - If the top strip is still reachable on some monitor, the position is
///   kept (a window may hang partly off an edge, or span two monitors).
///   Otherwise the window is moved fully inside the chosen monitor.
///
/// Returns `None` when there are no monitors (use the defaults).
pub fn fit(rect: Rect, monitors: &[Monitor], primary: Option<usize>, min_logical: (f64, f64)) -> Option<Rect> {
    if monitors.is_empty() {
        return None;
    }
    let holding = monitors.iter().position(|m| top_strip_visible(&rect, &m.rect));
    let index = holding
        .or_else(|| (0..monitors.len()).min_by_key(|&i| distance_sq(&rect, &monitors[i].rect)))
        .or(primary.filter(|&i| i < monitors.len()))
        .unwrap_or(0);
    let m = monitors[index];
    let scale = if m.scale.is_finite() && m.scale > 0.0 { m.scale } else { 1.0 };
    let min_w = (min_logical.0 * scale).ceil() as u32;
    let min_h = (min_logical.1 * scale).ceil() as u32;
    let width = rect.width.max(min_w).min(m.rect.width.max(1));
    let height = rect.height.max(min_h).min(m.rect.height.max(1));
    let mut out = Rect { width, height, ..rect };
    if !monitors.iter().any(|mon| top_strip_visible(&out, &mon.rect)) {
        let max_x = right(&m.rect) - width as i64;
        let max_y = bottom(&m.rect) - height as i64;
        out.x = (rect.x as i64).clamp(m.rect.x as i64, max_x.max(m.rect.x as i64)) as i32;
        out.y = (rect.y as i64).clamp(m.rect.y as i64, max_y.max(m.rect.y as i64)) as i32;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: (f64, f64) = (220.0, 150.0);

    fn mon(x: i32, y: i32, w: u32, h: u32, scale: f64) -> Monitor {
        Monitor { rect: Rect { x, y, width: w, height: h }, scale }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aion2dps-ws-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join(FILE_NAME)
    }

    #[test]
    fn round_trip() {
        let path = temp("rt");
        let rect = Rect { x: -1500, y: 40, width: 400, height: 300 };
        assert!(save(&path, OVERLAY_LABEL, &rect).unwrap());
        assert_eq!(load(&path), Some(rect));
    }

    #[test]
    fn missing_or_corrupt_file_falls_back() {
        let path = temp("bad");
        assert_eq!(load(&path), None);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for text in ["", "{not json", "[]", r#"{"overlay":{"x":"a"}}"#, r#"{"overlay":{"x":1,"y":2,"width":-5,"height":3}}"#] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(load(&path), None, "{text}");
        }
    }

    #[test]
    fn only_the_overlay_is_saved() {
        let path = temp("label");
        let rect = Rect { x: 1, y: 2, width: 300, height: 200 };
        for label in ["meter", "history", "toast-0"] {
            assert!(!save(&path, label, &rect).unwrap());
            assert!(!is_persisted(label));
        }
        assert!(!path.exists());
    }

    #[test]
    fn minimized_sentinel_is_ignored() {
        let path = temp("min");
        let good = Rect { x: 10, y: 10, width: 300, height: 200 };
        save(&path, OVERLAY_LABEL, &good).unwrap();
        let parked = Rect { x: -32000, y: -32000, width: 160, height: 28 };
        assert!(!save(&path, OVERLAY_LABEL, &parked).unwrap());
        assert_eq!(load(&path), Some(good));
    }

    #[test]
    fn on_screen_rect_is_kept() {
        let monitors = [mon(0, 0, 1920, 1080, 1.0), mon(-2560, -200, 2560, 1440, 1.0)];
        let rect = Rect { x: -1000, y: -100, width: 400, height: 300 };
        assert_eq!(fit(rect, &monitors, Some(0), MIN), Some(rect));
        // Hanging partly off the right edge is fine while the strip shows.
        let edge = Rect { x: 1700, y: 500, width: 400, height: 300 };
        assert_eq!(fit(edge, &monitors, Some(0), MIN), Some(edge));
    }

    #[test]
    fn off_screen_rect_is_clamped_to_nearest_monitor() {
        // Saved on a left monitor that is now unplugged.
        let monitors = [mon(0, 0, 1920, 1080, 1.0)];
        let rect = Rect { x: -2000, y: 300, width: 400, height: 300 };
        assert_eq!(fit(rect, &monitors, Some(0), MIN), Some(Rect { x: 0, y: 300, width: 400, height: 300 }));
        // Title strip above the top of the screen.
        let above = Rect { x: 500, y: -280, width: 400, height: 300 };
        assert_eq!(fit(above, &monitors, Some(0), MIN).unwrap().y, 0);
        // Far below and right, after a resolution drop.
        let far = Rect { x: 3000, y: 2000, width: 400, height: 300 };
        assert_eq!(fit(far, &monitors, Some(0), MIN), Some(Rect { x: 1520, y: 780, width: 400, height: 300 }));
    }

    #[test]
    fn size_is_held_between_min_and_monitor() {
        let monitors = [mon(0, 0, 1920, 1080, 1.5)];
        let tiny = Rect { x: 100, y: 100, width: 50, height: 40 };
        let out = fit(tiny, &monitors, Some(0), MIN).unwrap();
        assert_eq!((out.width, out.height), (330, 225));
        let huge = Rect { x: 0, y: 0, width: 5000, height: 4000 };
        let out = fit(huge, &monitors, Some(0), MIN).unwrap();
        assert_eq!((out.width, out.height), (1920, 1080));
    }

    #[test]
    fn no_monitors_means_defaults() {
        assert_eq!(fit(Rect { x: 0, y: 0, width: 300, height: 200 }, &[], None, MIN), None);
    }
}
