//! How each kind of pop-up looks and where it shows: event alert toasts
//! (upcoming, starting soon, live), the pet soul progress toast and the
//! hidden cube alert. Set in Config > Toasts & pop-ups and saved in
//! settings.json. The look is applied by the pages; the desktop shell reads
//! the place (monitor, screen position, offset) and size to move the windows.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Every kind, in Config order.
pub const KINDS: &[&str] = &["upcoming", "imminent", "live", "pet", "cube"];
/// `auto` follows Event alerts > Position (top or bottom center).
pub const POSITIONS: &[&str] = &[
    "auto", "top-left", "top", "top-right", "left", "center", "right", "bottom-left", "bottom", "bottom-right",
];
pub const SOUNDS: &[&str] = &["default", "on", "off"];
pub const SCALE: (u32, u32) = (70, 150);
pub const OFFSET: i32 = 1000;
pub const MAX_MONITOR: u32 = 6;
pub const FONT: (u32, u32) = (10, 32);
pub const OPACITY: (u32, u32) = (20, 100);
pub const MAX_TITLE: usize = 80;
pub const MAX_MESSAGE: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastStyle {
    /// Event kinds only; the pet and cube switches are their own settings.
    pub enabled: bool,
    /// Percent of the normal size.
    pub scale: u32,
    pub position: String,
    /// Logical pixels added after placing; + is right / down.
    pub offset_x: i32,
    pub offset_y: i32,
    /// 0 is the primary monitor, 1.. the nth monitor Windows lists.
    pub monitor: u32,
    /// Templates with `{placeholders}`; empty keeps the built-in text.
    pub title: String,
    pub message: String,
    /// Main text size in px.
    pub font_px: u32,
    /// `#rrggbb`, or empty for the built-in colour.
    pub accent: String,
    pub background: String,
    pub text: String,
    /// Background opacity in percent.
    pub bg_opacity: u32,
    pub show_icon: bool,
    /// `default` follows Event alerts > Sound for events and is off for the others.
    pub sound: String,
}

impl ToastStyle {
    pub fn default_for(kind: &str) -> Self {
        Self {
            enabled: true,
            scale: 100,
            position: "auto".into(),
            offset_x: 0,
            offset_y: 0,
            monitor: 0,
            title: String::new(),
            message: String::new(),
            font_px: match kind {
                "pet" => 14,
                "cube" => 23,
                _ => 13,
            },
            accent: String::new(),
            background: String::new(),
            text: String::new(),
            bg_opacity: 100,
            show_icon: true,
            sound: "default".into(),
        }
    }

    /// A sentence for the user when a value is out of range.
    pub fn check(&self) -> Result<(), String> {
        let hex = |s: &str| s.is_empty() || (s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit()));
        if !(SCALE.0..=SCALE.1).contains(&self.scale) {
            return Err(format!("size must be {} to {}%", SCALE.0, SCALE.1));
        }
        if !POSITIONS.contains(&self.position.as_str()) {
            return Err("unknown screen position".into());
        }
        if self.offset_x.abs() > OFFSET || self.offset_y.abs() > OFFSET {
            return Err(format!("offset must be -{OFFSET} to {OFFSET} px"));
        }
        if self.monitor > MAX_MONITOR {
            return Err("unknown monitor".into());
        }
        if self.title.chars().count() > MAX_TITLE || self.message.chars().count() > MAX_MESSAGE {
            return Err(format!("title is up to {MAX_TITLE} characters, message up to {MAX_MESSAGE}"));
        }
        if self.title.chars().chain(self.message.chars()).any(char::is_control) {
            return Err("text can't hold line breaks or control characters".into());
        }
        if !(FONT.0..=FONT.1).contains(&self.font_px) {
            return Err(format!("text size must be {} to {} px", FONT.0, FONT.1));
        }
        if !hex(&self.accent) || !hex(&self.background) || !hex(&self.text) {
            return Err("colours are #rrggbb".into());
        }
        if !(OPACITY.0..=OPACITY.1).contains(&self.bg_opacity) {
            return Err(format!("background opacity must be {} to {}%", OPACITY.0, OPACITY.1));
        }
        if !SOUNDS.contains(&self.sound.as_str()) {
            return Err("unknown sound choice".into());
        }
        Ok(())
    }
}

/// The style of every kind.
pub type ToastStyles = BTreeMap<String, ToastStyle>;

pub fn defaults() -> ToastStyles {
    KINDS.iter().map(|k| ((*k).to_string(), ToastStyle::default_for(k))).collect()
}

/// `patch` (any subset of the fields) over `base`, checked. Unknown fields are errors.
pub fn merged(base: &ToastStyle, patch: &Value) -> Result<ToastStyle, String> {
    let Value::Object(patch) = patch else { return Err("expected an object".into()) };
    let mut value = serde_json::to_value(base).map_err(|e| e.to_string())?;
    let Value::Object(fields) = &mut value else { unreachable!() };
    for (k, v) in patch {
        if !fields.contains_key(k) {
            return Err(format!("unknown toast setting {k}"));
        }
        fields.insert(k.clone(), v.clone());
    }
    let style: ToastStyle = serde_json::from_value(value).map_err(|_| "a value has the wrong type".to_string())?;
    style.check()?;
    Ok(style)
}

/// Saved styles over the defaults. A missing field keeps its default and a
/// kind that does not check out is reset, so old or hand-edited files load.
pub fn load(saved: &Value) -> ToastStyles {
    let mut out = defaults();
    let Value::Object(saved) = saved else { return out };
    for (kind, style) in out.iter_mut() {
        let Some(Value::Object(fields)) = saved.get(kind) else { continue };
        let names = serde_json::to_value(&*style).unwrap_or(Value::Null);
        let known: Map<String, Value> = fields
            .iter()
            .filter(|(k, _)| names.get(k.as_str()).is_some())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Ok(next) = merged(style, &Value::Object(known)) {
            *style = next;
        }
    }
    out
}

/// Where a toast `size` (logical px) goes on a monitor at `origin` / `screen`:
/// the top-left corner, kept on that monitor. `auto` is `fallback` (top or bottom).
pub fn place(style: &ToastStyle, fallback: &str, origin: (f64, f64), screen: (f64, f64), size: (f64, f64), margin: f64) -> (f64, f64) {
    let pos = if style.position == "auto" { fallback } else { style.position.as_str() };
    let (col, row) = match pos {
        "top-left" => (0, 0),
        "top" => (1, 0),
        "top-right" => (2, 0),
        "left" => (0, 1),
        "center" => (1, 1),
        "right" => (2, 1),
        "bottom-left" => (0, 2),
        "bottom" => (1, 2),
        "bottom-right" => (2, 2),
        _ => (1, 0),
    };
    let x = match col {
        0 => origin.0 + margin,
        2 => origin.0 + screen.0 - size.0 - margin,
        _ => origin.0 + ((screen.0 - size.0) / 2.0).max(0.0),
    } + f64::from(style.offset_x);
    // Bottom keeps clear of the taskbar.
    let y = match row {
        0 => origin.1 + margin,
        2 => origin.1 + screen.1 - size.1 - margin - 40.0,
        _ => origin.1 + ((screen.1 - size.1) / 2.0).max(0.0),
    } + f64::from(style.offset_y);
    let max_x = (origin.0 + screen.0 - size.0).max(origin.0);
    let max_y = (origin.1 + screen.1 - size.1).max(origin.1);
    (x.clamp(origin.0, max_x), y.clamp(origin.1, max_y))
}

/// The screen position `auto` resolves to, for grouping toasts in one window.
pub fn resolved_position<'a>(style: &'a ToastStyle, fallback: &'a str) -> &'a str {
    if style.position == "auto" { fallback } else { &style.position }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patches_are_checked_and_old_files_load() {
        let base = ToastStyle::default_for("pet");
        assert_eq!(merged(&base, &json!({"scale": 120})).unwrap().scale, 120);
        assert!(merged(&base, &json!({"scale": 200})).is_err());
        assert!(merged(&base, &json!({"accent": "red"})).is_err());
        assert!(merged(&base, &json!({"nope": 1})).is_err());
        let styles = load(&json!({"pet": {"scale": 130, "extra": true}, "cube": {"scale": 999}}));
        assert_eq!(styles["pet"].scale, 130);
        assert_eq!(styles["cube"], ToastStyle::default_for("cube"));
        assert_eq!(load(&Value::Null), defaults());
    }

    #[test]
    fn placing_stays_on_the_monitor() {
        let mut s = ToastStyle::default_for("upcoming");
        assert_eq!(place(&s, "top", (0.0, 0.0), (1920.0, 1080.0), (520.0, 80.0), 36.0), (700.0, 36.0));
        s.position = "bottom-right".into();
        s.offset_x = 500;
        let (x, y) = place(&s, "top", (0.0, 0.0), (1920.0, 1080.0), (520.0, 80.0), 36.0);
        assert_eq!((x, y), (1400.0, 1080.0 - 80.0 - 36.0 - 40.0));
    }
}
