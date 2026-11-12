//! aion2dps desktop app: the always-on-top overlay, the detailed meter and the
//! session history windows, on top of the passive packet engine in the
//! `aion2dps` crate.
//!
//! Closing the overlay keeps the app in the system tray when "Minimize to
//! tray when overlay is closed" is on (the default). A process-name poll
//! (no game interaction) shows the overlay again when `AION2.exe` starts, if
//! "Open automatically when Aion 2 is detected" is on.
//!
//! Event alerts fire in the meter and overlay pages and are shown in a small
//! toast window over the game: the top center of the primary monitor unless
//! Config > Toasts & pop-ups puts that kind elsewhere (one window per place).
//! It is opened when a toast arrives, lets clicks through except on a toast,
//! never takes focus, and closes when its last toast is gone.
//!
//! With overlay click-through on (default), the compact overlay body also lets
//! clicks through to the game; the top bar and a thin resize edge stay live.
//!
//! The same engine state also backs the localhost page, so the detailed meter
//! keeps working in a normal browser at http://127.0.0.1:18732/.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_log;
mod tray_watch;
mod updater;
mod startup;

use std::collections::{HashMap, HashSet};
use std::env;
use std::process::Command;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use aion2dps::alerts::{self, Alerts, Position};
use aion2dps::combat::TargetMode;
use aion2dps::demo::{self, now_ms};
use aion2dps::monster_names::{self, MonsterName};
use aion2dps::sessions::{self, Session, Summary};
use aion2dps::state::{AppState, Live};
use aion2dps::target_debug::{self, DebugInfo, TargetDebug};
use aion2dps::toast_style::{self, ToastStyle};
use aion2dps::window_state::{self, Rect};
use aion2dps::{capture, web};
use tauri::{
    AppHandle, Emitter, Listener, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, RunEvent, State,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

const OVERLAY: &str = "overlay";
const METER: &str = "meter";
const HISTORY: &str = "history";
/// Tells the history window that a session was saved or deleted.
const SESSIONS_CHANGED: &str = "sessions";
/// Tells every window that the event alerts changed.
const ALERTS_CHANGED: &str = "alerts";
/// Tells the toast window that toasts are waiting in `take_toasts`.
const TOAST_WAKE: &str = "toast-wake";
/// Tells toast windows to drop a toast that moved to another window (`keep`).
const TOAST_DROP: &str = "toast-drop";
/// The toast window: logical width and distance from the screen edge.
const TOAST_WIDTH: f64 = 520.0;
const TOAST_MARGIN: f64 = 36.0;
/// The meter and the overlay both fire each alert; the second copy is dropped.
const TOAST_DEDUP: Duration = Duration::from_secs(5);
/// The overlay's logical minimum size; a restored size never goes below it.
const OVERLAY_MIN: (f64, f64) = aion2dps::overlay_layout::MIN_SIZE;
/// The overlay's rect is written this long after the last move or resize.
const OVERLAY_SAVE_DELAY: Duration = Duration::from_millis(500);
/// How often a combat snapshot is pushed to the windows as a `live` event.
const PUSH_EVERY: Duration = Duration::from_millis(200);

struct Args {
    port: u16,
    demo: bool,
    no_overlay: bool,
    browser: bool,
    debug_targets: bool,
    start_hidden: bool,
}

impl Args {
    fn parse() -> Self {
        let mut args = Self {
            port: web::DEFAULT_PORT,
            demo: false,
            no_overlay: false,
            browser: false,
            debug_targets: false,
            start_hidden: false,
        };
        let mut iter = env::args().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--start-hidden" => args.start_hidden = true,
                "--demo" => args.demo = true,
                "--no-overlay" => args.no_overlay = true,
                "--browser" => args.browser = true,
                target_debug::FLAG => args.debug_targets = true,
                "--port" => {
                    args.port = iter
                        .next()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(web::DEFAULT_PORT);
                }
                "--help" | "-h" => {
                    println!(
                        "aion2dps\n\n  --port <n>        localhost port (default {})\n  --demo            show sample combat instead of capturing\n  --no-overlay      do not open the always-on-top overlay; open the detailed meter window\n                    instead, or nothing but the browser page when used with --browser\n  --browser         open the detailed page in the default browser\n  --debug-targets   log raw packets around unnamed targets to {}\n                    (same as setting {}=1)\n",
                        web::DEFAULT_PORT,
                        target_debug::default_path().display(),
                        target_debug::ENV_VAR
                    );
                    std::process::exit(0);
                }
                other => eprintln!("ignoring unknown argument {other} (try --help)"),
            }
        }
        args
    }

    /// No app window at all: the browser page is the only view.
    fn windowless(&self) -> bool {
        self.no_overlay && self.browser
    }
}

/// Address of the localhost page, for the "open in browser" action.
struct Dashboard(String);

#[tauri::command]
fn live(state: State<'_, Arc<AppState>>) -> Live {
    state.live(now_ms())
}

#[tauri::command]
fn reset(state: State<'_, Arc<AppState>>) {
    state.reset(now_ms());
}

#[tauri::command]
fn set_mode(state: State<'_, Arc<AppState>>, mode: String) -> Result<(), String> {
    let mode = TargetMode::parse(&mode).ok_or_else(|| format!("unknown target mode {mode}"))?;
    state.set_mode(mode);
    Ok(())
}

#[tauri::command]
fn set_reset_secs(state: State<'_, Arc<AppState>>, secs: u64) -> Result<(), String> {
    if state.set_reset_after_secs(secs) {
        Ok(())
    } else {
        Err("enter a whole number of seconds from 5 to 3600".into())
    }
}

/// Either value may be left out.
#[tauri::command]
fn set_boss_settings(
    state: State<'_, Arc<AppState>>,
    lock_secs: Option<u64>,
    damage_threshold: Option<i64>,
    engage_lock: Option<bool>,
) -> Result<(), String> {
    state.set_boss_settings(lock_secs, damage_threshold, engage_lock, now_ms())
}

/// End a boss lock early.
#[tauri::command]
fn unlock(state: State<'_, Arc<AppState>>) {
    state.unlock();
}

#[tauri::command]
fn set_open_when_aion2(state: State<'_, Arc<AppState>>, on: bool) {
    state.set_open_when_aion2(on);
}

#[tauri::command]
fn set_minimize_to_tray(state: State<'_, Arc<AppState>>, on: bool) {
    state.set_minimize_to_tray(on);
}

#[tauri::command]
fn set_start_with_windows(state: State<'_, Arc<AppState>>, on: bool) -> Result<(), String> {
    // Do not persist an enabled preference if Windows rejected the task.
    startup::configure(on)?;
    state.set_start_with_windows(on);
    Ok(())
}

#[tauri::command]
fn set_pop_up_on_boss(state: State<'_, Arc<AppState>>, on: bool) {
    state.set_pop_up_on_boss(on);
}

#[tauri::command]
fn set_overlay_click_through(state: State<'_, Arc<AppState>>, on: bool) {
    state.set_overlay_click_through(on);
}

/// Either value may be left out.
#[tauri::command]
fn set_pet_toasts(state: State<'_, Arc<AppState>>, on: Option<bool>, secs: Option<u64>) -> Result<(), String> {
    state.set_pet_toasts(on, secs)
}

/// Any may be left out. The logs open or close without a restart.
#[tauri::command]
fn set_debug_logs(
    state: State<'_, Arc<AppState>>,
    pets: Option<bool>,
    position: Option<bool>,
    cubes: Option<bool>,
) {
    state.set_debug_logs(pets, position, cubes);
}

#[tauri::command]
fn set_hidden_cube_alerts(state: State<'_, Arc<AppState>>, on: bool) {
    state.set_hidden_cube_alerts(on);
}

/// The mouse pointer in the overlay's own CSS pixels (same idea as toast_pointer).
#[tauri::command]
fn overlay_pointer(window: WebviewWindow) -> Option<(f64, f64)> {
    let cursor = window.cursor_position().ok()?;
    let origin = window.inner_position().ok()?;
    let scale = window.scale_factor().ok()?;
    Some(((cursor.x - f64::from(origin.x)) / scale, (cursor.y - f64::from(origin.y)) / scale))
}

/// Capture clicks on the overlay (`true`) or let them reach the game (`false`).
#[tauri::command]
fn overlay_hit(window: WebviewWindow, capture: bool) {
    let _ = window.set_ignore_cursor_events(!capture);
}

#[tauri::command]
fn clear_history(state: State<'_, Arc<AppState>>) {
    state.clear_history();
}

#[tauri::command]
fn pets(state: State<'_, Arc<AppState>>) -> aion2dps::pets::PetsView {
    state.pets()
}

#[tauri::command]
fn monster_names(state: State<'_, Arc<AppState>>) -> Vec<MonsterName> {
    state.monster_names()
}

/// An empty name clears it.
#[tauri::command]
fn set_monster_name(
    state: State<'_, Arc<AppState>>,
    monster_type: u32,
    name: String,
) -> Result<(), String> {
    state.set_monster_name(monster_type, &name)
}

// The session commands are async so file work stays off the main thread.

#[tauri::command]
async fn save_session(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<Summary, String> {
    let summary = state.save_session(now_ms()).map_err(|err| err.to_string())?;
    let _ = app.emit(SESSIONS_CHANGED, ());
    Ok(summary)
}

#[tauri::command]
async fn list_sessions(state: State<'_, Arc<AppState>>) -> Result<Vec<Summary>, String> {
    Ok(state.sessions())
}

#[tauri::command]
async fn get_session(state: State<'_, Arc<AppState>>, id: String) -> Result<Session, String> {
    state.session(&id).map_err(|err| err.to_string())
}

#[tauri::command]
async fn delete_session(app: AppHandle, state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.delete_session(&id).map_err(|err| err.to_string())?;
    let _ = app.emit(SESSIONS_CHANGED, ());
    Ok(())
}

#[tauri::command]
async fn rename_session(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
    name: String,
) -> Result<Summary, String> {
    let summary = state.rename_session(&id, &name).map_err(|err| err.to_string())?;
    let _ = app.emit(SESSIONS_CHANGED, ());
    Ok(summary)
}

/// Toasts on their way to the screen toast windows. Toasts that go to the
/// same place (monitor, screen position, offset; Config > Toasts & pop-ups)
/// share one window, a "lane"; another place gets its own window.
#[derive(Default)]
struct Toasts {
    lanes: Mutex<HashMap<String, Lane>>,
    seq: AtomicU32,
    recent: Mutex<HashMap<String, Instant>>,
}

#[derive(Default)]
struct Lane {
    pending: Vec<serde_json::Value>,
    /// Label of the open window. Each window gets a new label, so one still
    /// closing never blocks the next.
    window: Option<String>,
    /// Where the lane's toasts go, from the newest toast's style.
    style: Option<ToastStyle>,
    /// The window's logical size, as the page last measured it.
    size: (f64, f64),
}

impl Toasts {
    fn lane_of<'a>(lanes: &'a mut HashMap<String, Lane>, label: &str) -> Option<&'a mut Lane> {
        lanes.values_mut().find(|lane| lane.window.as_deref() == Some(label))
    }
}

/// `top` or `bottom`: what a toast's `auto` position follows.
fn alerts_place(app: &AppHandle) -> &'static str {
    if app.state::<Arc<AppState>>().alerts().position == Position::Bottom { "bottom" } else { "top" }
}

/// Logical origin and size of monitor `n` (0 = primary, 1.. = nth listed).
fn monitor_rect(app: &AppHandle, n: u32) -> Option<((f64, f64), (f64, f64))> {
    let nth = if n > 0 { app.available_monitors().ok().and_then(|m| m.into_iter().nth(n as usize - 1)) } else { None };
    let monitor = match nth {
        Some(m) => m,
        None => app.primary_monitor().ok()??,
    };
    let scale = monitor.scale_factor();
    let pos = monitor.position().to_logical::<f64>(scale);
    let size = monitor.size().to_logical::<f64>(scale);
    Some(((pos.x, pos.y), (size.width, size.height)))
}

/// Top-left corner, in logical pixels, for a window of `size` in `style`'s place.
fn styled_origin(app: &AppHandle, style: &ToastStyle, fallback: &str, size: (f64, f64), margin: f64) -> (f64, f64) {
    let Some((origin, screen)) = monitor_rect(app, style.monitor) else {
        return (0.0, margin);
    };
    toast_style::place(style, fallback, origin, screen, size, margin)
}

/// Queue a toast for its lane and open that lane's window, or wake it.
fn deliver(app: &AppHandle, toasts: &Toasts, mut toast: serde_json::Value) -> tauri::Result<()> {
    let kind = toast["kind"].as_str().unwrap_or("upcoming").to_string();
    let style = app.state::<Arc<AppState>>().toast_style(&kind);
    let fallback = alerts_place(app);
    let place = toast_style::resolved_position(&style, fallback).to_string();
    let lane_key = format!("{}|{}|{}|{}|{}|{}", style.monitor, place, style.x_pct, style.y_pct, style.offset_x, style.offset_y);
    toast["style"] = serde_json::to_value(&style).unwrap_or_default();
    toast["place"] = place.into();
    let key = toast["key"].clone();
    let width = TOAST_WIDTH * f64::from(style.scale) / 100.0;
    let (open, label) = {
        let mut lanes = toasts.lanes.lock().unwrap();
        let lane = lanes.entry(lane_key).or_default();
        lane.pending.push(toast);
        lane.style = Some(style.clone());
        if lane.window.as_ref().is_some_and(|label| app.get_webview_window(label).is_some()) {
            (true, lane.window.clone().unwrap_or_default())
        } else {
            let label = format!("toast-{}", toasts.seq.fetch_add(1, Ordering::Relaxed));
            lane.window = Some(label.clone());
            lane.size = (width, 80.0);
            (false, label)
        }
    };
    // The same toast in another lane (an upcoming alert that turns imminent
    // somewhere else) leaves that one.
    let _ = app.emit(TOAST_DROP, serde_json::json!({ "key": key, "keep": label }));
    if open {
        return app.emit(TOAST_WAKE, ());
    }
    let (x, y) = styled_origin(app, &style, fallback, (width, 80.0), TOAST_MARGIN);
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("toast.html".into()))
        .title("aion2dps alert")
        .inner_size(width, 80.0)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .focused(false)
        .build()?;
    let _ = window.set_ignore_cursor_events(true);
    Ok(())
}

/// Async so the toast window is not built on the IPC thread.
#[tauri::command]
async fn show_toast(app: AppHandle, toasts: State<'_, Toasts>, toast: serde_json::Value) -> Result<(), String> {
    let id = format!("{}:{}", toast["key"], toast["kind"]);
    {
        let mut recent = toasts.recent.lock().unwrap();
        let now = Instant::now();
        recent.retain(|_, at| now.duration_since(*at) < TOAST_DEDUP);
        if recent.contains_key(&id) {
            return Ok(());
        }
        recent.insert(id, now);
    }
    deliver(&app, &toasts, toast).map_err(|err| err.to_string())
}

/// Pet portraits being downloaded or that failed this run, so each is tried once.
#[derive(Default)]
struct PetIcons(Mutex<HashSet<u32>>);

/// Send pet soul toasts to the toast window. One toast per pet (`pet:<id>`):
/// the window updates it in place. The portrait comes from the local cache;
/// a missing one is downloaded in the background and sent as an update.
fn deliver_pet_toasts(app: &AppHandle, state: &AppState) {
    let pending = state.take_pet_toasts();
    if pending.is_empty() {
        return;
    }
    let hide_ms = state.pet_toast_secs() * 1000;
    let toasts = app.state::<Toasts>();
    for pet in pending {
        let id = pet.id;
        let key = format!("pet:{id}");
        let icon = aion2dps::pets::cached_icon(id);
        if icon.is_none() && app.state::<PetIcons>().0.lock().unwrap().insert(id) {
            let app = app.clone();
            let key = key.clone();
            thread::spawn(move || {
                if aion2dps::pets::fetch_icon(id) {
                    if let Some(icon) = aion2dps::pets::cached_icon(id) {
                        let update = serde_json::json!({ "key": key, "kind": "pet", "iconOnly": true, "icon": icon });
                        let _ = deliver(&app, &app.state::<Toasts>(), update);
                    }
                    app.state::<PetIcons>().0.lock().unwrap().remove(&id);
                }
            });
        }
        let toast = serde_json::json!({ "key": key, "kind": "pet", "hideMs": hide_ms, "pet": pet, "icon": icon });
        let _ = deliver(app, &toasts, toast);
    }
}

/// Show the toast again after `ms`, even if its window has closed by then.
#[tauri::command]
fn snooze_toast(app: AppHandle, toast: serde_json::Value, ms: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(ms.min(3_600_000))).await;
        let toasts = app.state::<Toasts>();
        let _ = deliver(&app, &toasts, toast);
    });
}

#[tauri::command]
fn take_toasts(window: WebviewWindow, toasts: State<'_, Toasts>) -> Vec<serde_json::Value> {
    let mut lanes = toasts.lanes.lock().unwrap();
    Toasts::lane_of(&mut lanes, window.label()).map(|lane| std::mem::take(&mut lane.pending)).unwrap_or_default()
}

/// Fit the toast window to its toasts and keep it in its place.
#[tauri::command]
fn toast_layout(app: AppHandle, window: WebviewWindow, toasts: State<'_, Toasts>, height: f64, width: Option<f64>) {
    let height = height.clamp(20.0, 900.0);
    let (style, size) = {
        let mut lanes = toasts.lanes.lock().unwrap();
        let Some(lane) = Toasts::lane_of(&mut lanes, window.label()) else { return };
        let width = width.unwrap_or(lane.size.0).clamp(120.0, 1600.0);
        lane.size = (width, height);
        (lane.style.clone().unwrap_or_else(|| ToastStyle::default_for("upcoming")), lane.size)
    };
    let (x, y) = styled_origin(&app, &style, alerts_place(&app), size, TOAST_MARGIN);
    let _ = window.set_size(LogicalSize::new(size.0, size.1));
    let _ = window.set_position(LogicalPosition::new(x, y));
}

/// The mouse pointer in the window's own CSS pixels.
#[tauri::command]
fn toast_pointer(window: WebviewWindow) -> Option<(f64, f64)> {
    let cursor = window.cursor_position().ok()?;
    let origin = window.inner_position().ok()?;
    let scale = window.scale_factor().ok()?;
    Some(((cursor.x - f64::from(origin.x)) / scale, (cursor.y - f64::from(origin.y)) / scale))
}

/// Take clicks while the pointer is on a toast; let them through otherwise.
#[tauri::command]
fn toast_hit(window: WebviewWindow, over: bool) {
    let _ = window.set_ignore_cursor_events(!over);
}

/// The last toast is gone: close the window unless more are waiting.
#[tauri::command]
fn toast_done(window: WebviewWindow, toasts: State<'_, Toasts>) {
    let mut lanes = toasts.lanes.lock().unwrap();
    if let Some(lane) = Toasts::lane_of(&mut lanes, window.label()) {
        if !lane.pending.is_empty() {
            let _ = window.emit(TOAST_WAKE, ());
            return;
        }
        lane.window = None;
    }
    drop(lanes);
    let _ = window.close();
}

/// Config > Toasts & pop-ups. New toasts use it; the cube window moves when it next shows.
#[tauri::command]
fn set_toast_style(state: State<'_, Arc<AppState>>, kind: String, patch: serde_json::Value) -> Result<(), String> {
    state.set_toast_style(&kind, &patch)
}

const PLACER: &str = "toast-placer";

/// Config's "Place on screen": a see-through window over one monitor with a
/// draggable sample of one toast type. It only reads this PC's screen; nothing
/// is sent to the game. Config saves the spot (so Undo works).
#[derive(Default)]
struct Placer {
    seq: AtomicU64,
    now: Mutex<Option<Placing>>,
}

struct Placing {
    seq: u64,
    info: serde_json::Value,
    screen: (f64, f64),
    size: (f64, f64),
    reply: tokio::sync::oneshot::Sender<Option<serde_json::Value>>,
}

/// About the logical size a toast of `kind` takes at its style's size (the
/// event and pet windows fit their content).
fn toast_size(kind: &str, style: &ToastStyle) -> (f64, f64) {
    let k = f64::from(style.scale) / 100.0;
    match kind {
        "cube" => (CUBE_SIZE.0 * k, CUBE_SIZE.1 * k),
        "pet" => (TOAST_WIDTH * k, 84.0 * k),
        _ => (TOAST_WIDTH * k, 64.0 * k),
    }
}

/// End placing `seq` (any when `None`): the custom fields for a drop at `at`,
/// or `None` for Esc or a closed window.
fn finish_placing(app: &AppHandle, seq: Option<u64>, at: Option<(f64, f64)>) {
    let placing = {
        let placer = app.state::<Placer>();
        let mut now = placer.now.lock().unwrap();
        if now.as_ref().is_some_and(|p| seq.map_or(true, |s| s == p.seq)) { now.take() } else { None }
    };
    if let Some(p) = placing {
        let _ = p.reply.send(at.map(|at| toast_style::custom_fields(at, p.screen, p.size)));
        if let Some(window) = app.get_webview_window(PLACER) {
            let _ = window.close();
        }
    }
}

/// Show the placer for `kind` on its monitor; resolves with the fields to save
/// once the sample is dropped, or `None` when cancelled.
#[tauri::command]
async fn place_toast(app: AppHandle, kind: String) -> Result<Option<serde_json::Value>, String> {
    if !toast_style::KINDS.contains(&kind.as_str()) {
        return Err("unknown toast type".into());
    }
    let style = app.state::<Arc<AppState>>().toast_style(&kind);
    let (origin, screen) = monitor_rect(&app, style.monitor).ok_or("no monitor found")?;
    let size = toast_size(&kind, &style);
    let (fallback, margin) = if kind == "cube" { ("top", 8.0) } else { (alerts_place(&app), TOAST_MARGIN) };
    let at = toast_style::place(&style, fallback, origin, screen, size, margin);
    finish_placing(&app, None, None);
    if let Some(old) = app.get_webview_window(PLACER) {
        let _ = old.destroy();
    }
    let (reply, answer) = tokio::sync::oneshot::channel();
    let seq = {
        let placer = app.state::<Placer>();
        let seq = placer.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let info = serde_json::json!({ "kind": kind, "style": style, "size": [size.0, size.1], "at": [at.0 - origin.0, at.1 - origin.1] });
        *placer.now.lock().unwrap() = Some(Placing { seq, info, screen, size, reply });
        seq
    };
    let window = WebviewWindowBuilder::new(&app, PLACER, WebviewUrl::App("placer.html".into()))
        .title("aion2dps - place pop-up")
        .inner_size(screen.0, screen.1)
        .position(origin.0, origin.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(true)
        .build()
        .map_err(|err| err.to_string())?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            finish_placing(&handle, Some(seq), None);
        }
    });
    let _ = window.set_focus();
    Ok(answer.await.ok().flatten())
}

/// The placer page: the type, its style, the sample's size and where it is now.
#[tauri::command]
fn placer_info(placer: State<'_, Placer>) -> Option<serde_json::Value> {
    placer.now.lock().unwrap().as_ref().map(|p| p.info.clone())
}

/// The sample was dropped with its top-left at `at` (window px), or cancelled.
#[tauri::command]
fn placer_done(app: AppHandle, at: Option<(f64, f64)>) {
    finish_placing(&app, None, at.filter(|(x, y)| x.is_finite() && y.is_finite()));
}

/// Config's "Send test toast" for the hidden cube: the cube window shows a
/// sample alert for a few seconds. Nothing is read from or sent to the game.
#[tauri::command]
fn test_cube_toast(app: AppHandle) {
    let _ = app.emit_to(CUBE, "cube-test", ());
}

#[tauri::command]
async fn get_alerts(state: State<'_, Arc<AppState>>) -> Result<Alerts, String> {
    Ok(state.alerts())
}

#[tauri::command]
async fn set_alerts(app: AppHandle, state: State<'_, Arc<AppState>>, alerts: Alerts) -> Result<Alerts, String> {
    let saved = state.set_alerts(alerts)?;
    let _ = app.emit(ALERTS_CHANGED, ());
    Ok(saved)
}

#[tauri::command]
async fn restore_default_alerts(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<alerts::Restored, String> {
    let done = state.restore_default_alerts()?;
    if done.restored > 0 {
        let _ = app.emit(ALERTS_CHANGED, ());
    }
    Ok(done)
}

/// Async so the new window is not built on the IPC thread, which deadlocks on Windows.
#[tauri::command]
async fn open_meter(app: AppHandle) -> Result<(), String> {
    show_meter(&app).map_err(|err| err.to_string())
}

#[tauri::command]
async fn open_history(app: AppHandle) -> Result<(), String> {
    show_page(&app, HISTORY, "history.html", "aion2dps - session history").map_err(|err| err.to_string())
}

#[tauri::command]
fn open_in_browser(dashboard: State<'_, Dashboard>) {
    open_browser(&dashboard.0);
}

/// Full quit, same path as the tray Quit item. The overlay close button
/// uses this on right-click so left-click can still hide to the tray.
#[tauri::command]
fn quit_app(app: AppHandle) {
    tracing::info!(target: "overlay", "quitting from the overlay close button (right-click)");
    app.exit(0);
}

/// "Check for updates": the latest GitHub release against this build.
#[tauri::command]
async fn check_update(app: AppHandle, fresh: bool) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || updater::check(&app, fresh).map(|c| c.json()))
        .await
        .map_err(|err| err.to_string())?
}

/// "Update now": download and verify the installer from the last check, start it, quit.
#[tauri::command]
async fn install_update(app: AppHandle, version: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || updater::install(&app, &version))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The release page link in the update dialog. The URL is fixed here.
#[tauri::command]
fn open_release_page() {
    open_browser(updater::RELEASES);
}

/// Retry in the Npcap admin-only notice: open capture again, which brings the
/// Windows permission prompt back. Nothing is sent to the game.
#[tauri::command]
fn retry_capture(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    aion2dps::capture::retry(state.inner().clone())
}

/// The "Get Npcap" button. The URL is fixed here, never taken from the page.
#[tauri::command]
fn open_npcap_page() {
    open_browser(aion2dps::capture::NPCAP_URL);
}

const CUBE: &str = "cube";
const CUBE_SIZE: (f64, f64) = (460.0, 140.0);

/// The cube window's size and top-left corner, per Config > Toasts & pop-ups
/// (`auto` is the top center).
fn cube_place(app: &AppHandle) -> ((f64, f64), (f64, f64)) {
    let style = app.state::<Arc<AppState>>().toast_style("cube");
    let k = f64::from(style.scale) / 100.0;
    let size = (CUBE_SIZE.0 * k, CUBE_SIZE.1 * k);
    (size, styled_origin(app, &style, "top", size, 8.0))
}

fn cube_origin(app: &AppHandle) -> (f64, f64) {
    cube_place(app).1
}

/// Hidden top-center window for the hidden-cube alert; the page shows it.
fn build_cube_window(app: &AppHandle) -> tauri::Result<()> {
    let (x, y) = cube_origin(app);
    WebviewWindowBuilder::new(app, CUBE, WebviewUrl::App("cube.html".into()))
        .title("aion2dps hidden cube")
        .inner_size(CUBE_SIZE.0, CUBE_SIZE.1)
        .position(x, y)
        .visible(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .focused(false)
        .build()?;
    Ok(())
}

#[tauri::command]
fn cube_visible(app: AppHandle, show: bool) {
    let Some(window) = app.get_webview_window(CUBE) else { return };
    if show {
        let ((w, h), (x, y)) = cube_place(&app);
        let _ = window.set_size(LogicalSize::new(w, h));
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
        let _ = window.show();
        let _ = window.set_always_on_top(true);
    } else {
        let _ = window.hide();
    }
}

fn build_overlay(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("overlay.html".into()))
        .title("aion2dps")
        .inner_size(aion2dps::overlay_layout::DEFAULT_SIZE.0, aion2dps::overlay_layout::DEFAULT_SIZE.1)
        .min_inner_size(OVERLAY_MIN.0, OVERLAY_MIN.1)
        // Hidden until the saved position and size are applied, so it does
        // not appear at the default spot and then jump.
        .visible(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .resizable(true)
        // Stays in the taskbar so Alt-Tab can focus it for the -/+ keys.
        .skip_taskbar(false)
        .focused(true)
        .build()?;
    restore_overlay_rect(app, &window);
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

/// Put the overlay where it was last closed, fitted to the monitors present
/// now. Missing or corrupt state leaves the default size and position.
fn restore_overlay_rect(app: &AppHandle, window: &WebviewWindow) {
    let Some(saved) = window_state::load(&window_state::default_path()) else {
        return;
    };
    if let Some(rect) = fit_to_monitors(app, saved) {
        place_overlay(window, rect);
    }
}

/// `rect` (physical px) fitted to the monitors present now; `None` without monitors.
fn fit_to_monitors(app: &AppHandle, rect: Rect) -> Option<Rect> {
    let monitors: Vec<window_state::Monitor> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| window_state::Monitor {
            rect: Rect {
                x: m.position().x,
                y: m.position().y,
                width: m.size().width,
                height: m.size().height,
            },
            scale: m.scale_factor(),
        })
        .collect();
    let primary = app.primary_monitor().ok().flatten().and_then(|p| {
        monitors
            .iter()
            .position(|m| m.rect.x == p.position().x && m.rect.y == p.position().y)
    });
    window_state::fit(rect, &monitors, primary, OVERLAY_MIN)
}

fn place_overlay(window: &WebviewWindow, rect: Rect) {
    let size = PhysicalSize::new(rect.width, rect.height);
    // Size again after the move: moving onto a monitor with another DPI
    // makes Windows rescale the window.
    note("overlay size", window.set_size(size));
    note("overlay position", window.set_position(PhysicalPosition::new(rect.x, rect.y)));
    note("overlay size", window.set_size(size));
}

/// Log a failed window call to app.log instead of dropping it.
fn note(what: &str, result: tauri::Result<()>) {
    if let Err(err) = result {
        tracing::warn!(target: "overlay", "{what} failed: {err}");
    }
}

/// Overlay pings: the page answers each with its number, so a stuck or dead
/// page (nothing drawn in a transparent window) can be rebuilt.
#[derive(Default)]
struct OverlayPing {
    sent: AtomicU64,
    answered: AtomicU64,
}

const PING_WAIT: Duration = Duration::from_secs(3);

/// Show the overlay again (tray click, Show overlay, the game starting, or a
/// boss when `quiet`): back on a monitor that is there now, on top, redrawn,
/// with a fresh snapshot; rebuilt if its page does not answer.
fn reveal_overlay(app: &AppHandle, quiet: bool) {
    app.state::<tray_watch::TrayWatch>().set_waiting(false);
    let Some(window) = app.get_webview_window(OVERLAY) else {
        match build_overlay(app) {
            Ok(()) => tracing::info!(target: "overlay", quiet, "overlay built"),
            Err(err) => tracing::warn!(target: "overlay", "could not open the overlay: {err}"),
        }
        return;
    };
    match (window.outer_position(), window.inner_size()) {
        (Ok(pos), Ok(size)) => {
            let now = Rect { x: pos.x, y: pos.y, width: size.width, height: size.height };
            if let Some(fit) = fit_to_monitors(app, now).filter(|fit| *fit != now) {
                tracing::info!(target: "overlay", "moved the overlay back on screen: {now:?} -> {fit:?}");
                place_overlay(&window, fit);
            }
        }
        _ => tracing::warn!(target: "overlay", "could not read the overlay's position"),
    }
    // Re-assert topmost: it can be lost while hidden (display sleep, full-screen games).
    note("overlay always on top", window.set_always_on_top(false));
    note("overlay always on top", window.set_always_on_top(true));
    if quiet {
        if !tray_watch::show_no_activate(&window) {
            note("overlay show", window.show());
        }
    } else {
        note("overlay unminimize", window.unminimize());
        note("overlay show", window.show());
        note("overlay focus", window.set_focus());
    }
    // A 1 px size nudge makes WebView2 draw again after a long time hidden.
    if let Ok(size) = window.inner_size() {
        note("overlay repaint", window.set_size(PhysicalSize::new(size.width + 1, size.height)));
        note("overlay repaint", window.set_size(size));
    }
    send_live(app, OVERLAY);
    let visible = window.is_visible().unwrap_or(false);
    tracing::info!(target: "overlay", quiet, visible, "overlay shown");
    watch_overlay(app.clone());
}

/// One fresh snapshot to the window `label` (hidden windows get none).
fn send_live(app: &AppHandle, label: &str) {
    let live = app.state::<Arc<AppState>>().live(now_ms());
    if let Err(err) = app.emit_to(label, "live", live) {
        tracing::warn!(target: "overlay", "could not send a snapshot to {label}: {err}");
    }
}

/// Ping the overlay page; rebuild the window when it has not answered in PING_WAIT.
fn watch_overlay(app: AppHandle) {
    let n = app.state::<OverlayPing>().sent.fetch_add(1, Ordering::SeqCst) + 1;
    if let Err(err) = app.emit_to(OVERLAY, "overlay-ping", n) {
        tracing::warn!(target: "overlay", "could not ping the overlay: {err}");
    }
    thread::spawn(move || {
        thread::sleep(PING_WAIT);
        let ping = app.state::<OverlayPing>();
        if ping.answered.load(Ordering::SeqCst) >= n || ping.sent.load(Ordering::SeqCst) != n {
            return;
        }
        tracing::warn!(target: "overlay", "the overlay page did not answer in {PING_WAIT:?}; rebuilding it");
        if let Some(window) = app.get_webview_window(OVERLAY) {
            save_overlay_rect(&app);
            note("overlay destroy", window.destroy());
        }
        for _ in 0..40 {
            if app.get_webview_window(OVERLAY).is_none() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        match build_overlay(&app) {
            Ok(()) => tracing::info!(target: "overlay", "overlay rebuilt"),
            Err(err) => tracing::warn!(target: "overlay", "could not rebuild the overlay: {err}"),
        }
    });
}

/// The overlay page's answer to `overlay-ping`.
#[tauri::command]
fn overlay_pong(ping: State<'_, OverlayPing>, n: u64) {
    ping.answered.fetch_max(n, Ordering::SeqCst);
}

/// Write the overlay's current rect, unless it is minimized or maximized.
fn save_overlay_rect(app: &AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY) else {
        return;
    };
    // Hidden in the tray, Windows may move it (monitors sleeping or changing):
    // keep the spot it had when it was last visible.
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    if window.is_minimized().unwrap_or(true) || window.is_maximized().unwrap_or(true) {
        return;
    }
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    let rect = Rect { x: pos.x, y: pos.y, width: size.width, height: size.height };
    if let Err(err) = window_state::save(&window_state::default_path(), OVERLAY, &rect) {
        tracing::warn!("could not save the overlay position: {err}");
    }
}

/// Bumped on every overlay move or resize; a delayed save only runs if no
/// newer one came in meanwhile.
#[derive(Default)]
struct OverlaySaves(Arc<AtomicU64>);

fn schedule_overlay_save(app: &AppHandle) {
    let counter = app.state::<OverlaySaves>().0.clone();
    let mine = counter.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(OVERLAY_SAVE_DELAY);
        if counter.load(Ordering::SeqCst) == mine {
            save_overlay_rect(&app);
        }
    });
}

/// The overlay is the main window: closing it hides to the tray or quits,
/// depending on "Minimize to tray when overlay is closed". Other windows
/// only close themselves. Right-click the overlay close button, or Quit
/// on the tray icon, always exits.
fn is_overlay(label: &str) -> bool {
    label == OVERLAY
}

/// Stay alive in the tray and keep a tray icon so the overlay can be shown again.
fn close_overlay_to_tray(app: &AppHandle, api: &tauri::CloseRequestApi) {
    api.prevent_close();
    save_overlay_rect(app);
    if let Err(err) = tray_watch::hide_to_tray(app) {
        tracing::warn!("could not hide to the tray: {err}");
        app.exit(0);
    }
}
fn show_meter(app: &AppHandle) -> tauri::Result<()> {
    show_page(app, METER, "index.html", "aion2dps - detailed meter")
}

/// Focus the window with this label, or open it on `page`.
fn show_page(app: &AppHandle, label: &str, page: &str, title: &str) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.unminimize();
        window.show()?;
        // It got no snapshots while hidden.
        send_live(app, label);
        return window.set_focus();
    }
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
        .title(title)
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 560.0)
        .background_color(tauri::window::Color(15, 17, 21, 255))
        .center()
        .build()?;
    Ok(())
}

/// Windows that get no snapshots while hidden. The cube window always does:
/// it shows itself from them.
const SKIP_WHEN_HIDDEN: [&str; 3] = [OVERLAY, METER, HISTORY];

/// Push a snapshot to every window. The payload is the same JSON as `/api/live`.
fn push_snapshots(app: AppHandle, state: Arc<AppState>, windowless: bool) {
    thread::Builder::new()
        .name("aion2dps-push".into())
        .spawn(move || {
            let mut popup = aion2dps::boss_popup::BossPopup::default();
            loop {
                let live = state.live(now_ms());
                let keys = state.with_combat(|combat| combat.boss_detection_keys());
                // Use the same restore action as the tray. Never steal focus
                // when either normal app window is already visible.
                let hidden = app.state::<tray_watch::TrayWatch>().waiting()
                    || [OVERLAY, METER].iter().all(|label| {
                        app.get_webview_window(label).is_none_or(|w| {
                            !w.is_visible().unwrap_or(true) || w.is_minimized().unwrap_or(false)
                        })
                    });
                if popup.observe(&keys, !windowless && state.pop_up_on_boss(), hidden) {
                    let _ = app.emit("boss-show", ());
                }
                // The UI only: `live()` above still ticks the fight, history and
                // boss detection whatever is shown. Hidden overlay / meter /
                // history windows skip it and get a fresh one when shown.
                let payload = serde_json::to_value(&live).unwrap_or_default();
                for (label, window) in app.webview_windows() {
                    if SKIP_WHEN_HIDDEN.contains(&label.as_str()) && !window.is_visible().unwrap_or(true) {
                        continue;
                    }
                    let _ = app.emit_to(label.as_str(), "live", &payload);
                }
                if windowless {
                    state.take_pet_toasts();
                } else {
                    deliver_pet_toasts(&app, &state);
                }
                thread::sleep(PUSH_EVERY);
            }
        })
        .expect("snapshot thread");
}

fn open_browser(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flash from the GUI app.
        let _ = Command::new("cmd")
            .args(["/c", "start", "", url])
            .creation_flags(0x0800_0000)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

/// The release exe has no console, so the path also goes to the capture
/// status the pages show.
fn open_target_debug(state: &AppState, flag: bool) -> Option<TargetDebug> {
    if !target_debug::requested(flag) {
        return None;
    }
    let path = target_debug::default_path();
    match TargetDebug::open(&path) {
        Ok(log) => {
            println!("target debug    {}", log.path().display());
            tracing::info!("target debug log: {}", log.path().display());
            let info = log.info();
            state.with_capture(|capture| capture.target_debug = Some(info));
            Some(log)
        }
        Err(err) => {
            eprintln!("target debug    could not open {} ({err})", path.display());
            tracing::warn!("target debug log unavailable at {}: {err}", path.display());
            state.with_capture(|capture| {
                capture.target_debug = Some(DebugInfo {
                    path: path.display().to_string(),
                    records: 0,
                    bytes: 0,
                    full: false,
                    error: Some(err.to_string()),
                })
            });
            None
        }
    }
}

/// Console logging as before, plus warnings, errors, overlay show results and update steps
/// in a size-capped `%LOCALAPPDATA%\aion2dps\app.log`.
fn init_logging() {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};
    let console = fmt::layer().with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")));
    let path = window_state::default_path().with_file_name("app.log");
    let file = match app_log::RotatingLog::open(&path) {
        Ok(log) => Some(fmt::layer().with_ansi(false).with_writer(Mutex::new(log)).with_filter(EnvFilter::new("warn,overlay=info,update=info"))),
        Err(err) => {
            eprintln!("could not open {}: {err}", path.display());
            None
        }
    };
    tracing_subscriber::registry().with(console).with(file).init();
}

fn main() {
    init_logging();

    let args = Args::parse();
    let (names_path, sessions_dir, alerts_path) = if args.demo {
        (monster_names::demo_path(), sessions::demo_dir(), alerts::demo_path())
    } else {
        (monster_names::default_path(), sessions::default_dir(), alerts::default_path())
    };
    let state = Arc::new(
        AppState::with_storage(Some(names_path), Some(sessions_dir)).with_alerts(Some(alerts_path)),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let port = runtime
        .block_on(web::serve(state.clone(), args.port))
        .unwrap_or_else(|err| {
            eprintln!("could not listen on 127.0.0.1:{} ({err})", args.port);
            std::process::exit(1);
        });

    let dashboard = format!("http://127.0.0.1:{port}/");
    println!("detailed meter  {dashboard}");
    if !args.no_overlay {
        println!("overlay window  this app");
    }
    println!("packets only - this program does not read game memory");

    if args.demo {
        if target_debug::requested(args.debug_targets) {
            println!("target debug    off in --demo (no packets to log)");
        }
        demo::start(state.clone());
    } else {
        let debug = open_target_debug(&state, args.debug_targets);
        capture::start(state.clone(), debug);
    }
    if args.browser {
        open_browser(&dashboard);
    }

    let windowless = args.windowless();
    let no_overlay = args.no_overlay;
    let start_hidden = args.start_hidden;
    let app = tauri::Builder::default()
        .manage(state.clone())
        .manage(Dashboard(dashboard))
        .manage(Toasts::default())
        .manage(Placer::default())
        .manage(OverlayPing::default())
        .manage(updater::Updater::default())
        .manage(PetIcons::default())
        .manage(OverlaySaves::default())
        .manage(tray_watch::TrayWatch::default())
        .invoke_handler(tauri::generate_handler![
            live,
            reset,
            set_mode,
            set_reset_secs,
            set_boss_settings,
            unlock,
            set_open_when_aion2,
            set_minimize_to_tray,
            set_start_with_windows,
            set_pop_up_on_boss,
            set_overlay_click_through,
            set_hidden_cube_alerts,
            set_pet_toasts,
            set_debug_logs,
            set_toast_style,
            test_cube_toast,
            place_toast,
            placer_info,
            placer_done,
            overlay_pong,
            cube_visible,
            overlay_pointer,
            overlay_hit,
            clear_history,
            monster_names,
            pets,
            set_monster_name,
            save_session,
            list_sessions,
            get_session,
            delete_session,
            rename_session,
            get_alerts,
            set_alerts,
            restore_default_alerts,
            show_toast,
            snooze_toast,
            take_toasts,
            toast_layout,
            toast_pointer,
            toast_hit,
            toast_done,
            open_meter,
            open_history,
            open_in_browser,
            quit_app,
            open_npcap_page,
            retry_capture,
            check_update,
            install_update,
            app_version,
            open_release_page
        ])
        .on_window_event(|window, event| {
            if window_state::is_persisted(window.label()) {
                match event {
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                        schedule_overlay_save(window.app_handle());
                    }
                    tauri::WindowEvent::CloseRequested { .. } => save_overlay_rect(window.app_handle()),
                    _ => {}
                }
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if is_overlay(window.label()) {
                    let app = window.app_handle();
                    if app.state::<Arc<AppState>>().minimize_to_tray() {
                        close_overlay_to_tray(app, api);
                    } else {
                        // Same full-quit path as quit_app and the tray Quit item.
                        tracing::info!(target: "overlay", "quitting because the overlay was closed");
                        app.exit(0);
                    }
                }
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            if start_hidden {
                tray_watch::hide_to_tray(&handle)?;
            } else if !no_overlay {
                if let Err(err) = build_overlay(&handle) {
                    tracing::warn!("overlay window is unavailable: {err}");
                    show_meter(&handle)?;
                }
            } else if !windowless {
                show_meter(&handle)?;
            }
            let boss_handle = handle.clone();
            handle.listen("boss-show", move |_| {
                reveal_overlay(&boss_handle, true);
            });
            let show_handle = handle.clone();
            handle.listen("tray-show", move |_| {
                reveal_overlay(&show_handle, false);
            });
            if let Err(err) = build_cube_window(&handle) {
                tracing::warn!("cube alert window is unavailable: {err}");
            }
            tray_watch::start_watcher(handle.clone(), state.clone());
            push_snapshots(handle, state, windowless);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("could not start the aion2dps window (is the WebView2 runtime installed?)");

    if windowless {
        println!("press Ctrl+C to stop");
    }
    app.run(move |app, event| {
        // Keep the localhost server alive for as long as the app runs.
        let _ = &runtime;
        if let RunEvent::ExitRequested { api, code, .. } = event {
            // Any way of quitting: keep the overlay's last rect.
            save_overlay_rect(app);
            if windowless && code.is_none() {
                api.prevent_exit();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_overlay_rect_is_remembered() {
        assert!(window_state::is_persisted(OVERLAY));
        for label in [METER, HISTORY, "toast-0"] {
            assert!(!window_state::is_persisted(label), "{label}");
        }
    }

    #[test]
    fn only_the_overlay_uses_the_close_setting() {
        assert!(is_overlay(OVERLAY));
        for label in [METER, HISTORY, "toast-0", "toast-7"] {
            assert!(!is_overlay(label), "{label}");
        }
    }
}
