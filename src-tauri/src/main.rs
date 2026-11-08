//! aion2dps desktop app: the always-on-top overlay, the detailed meter and the
//! session history windows, on top of the passive packet engine in the
//! `aion2dps` crate.
//!
//! With "Open automatically when Aion 2 is detected" on, closing the overlay
//! keeps the app in the system tray. A process-name poll (no game interaction)
//! shows the overlay again when `AION2.exe` starts.
//!
//! Event alerts fire in the meter and overlay pages and are shown in a small
//! toast window at the top center of the primary monitor, over the game. It is
//! opened when a toast arrives, lets clicks through except on a toast, never
//! takes focus, and closes when its last toast is gone.
//!
//! With overlay click-through on (default), the compact overlay body also lets
//! clicks through to the game; the top bar and a thin resize edge stay live.
//!
//! The same engine state also backs the localhost page, so the detailed meter
//! keeps working in a normal browser at http://127.0.0.1:18732/.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray_watch;
mod startup;

use std::collections::HashMap;
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

/// Toasts on their way to the screen toast window.
#[derive(Default)]
struct Toasts {
    pending: Mutex<Vec<serde_json::Value>>,
    /// Label of the open toast window. Each window gets a new label, so one
    /// still closing never blocks the next.
    window: Mutex<Option<String>>,
    seq: AtomicU32,
    recent: Mutex<HashMap<String, Instant>>,
}

impl Toasts {
    fn current(&self, app: &AppHandle) -> Option<WebviewWindow> {
        let label = self.window.lock().unwrap().clone()?;
        app.get_webview_window(&label)
    }
}

/// Queue a toast and open the toast window, or wake it.
fn deliver(app: &AppHandle, toasts: &Toasts, toast: serde_json::Value) -> tauri::Result<()> {
    toasts.pending.lock().unwrap().push(toast);
    if let Some(window) = toasts.current(app) {
        return window.emit(TOAST_WAKE, ());
    }
    let label = format!("toast-{}", toasts.seq.fetch_add(1, Ordering::Relaxed));
    let (x, y) = toast_origin(app, 80.0);
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("toast.html".into()))
        .title("aion2dps alert")
        .inner_size(TOAST_WIDTH, 80.0)
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
    *toasts.window.lock().unwrap() = Some(label);
    Ok(())
}

/// Top-left corner, in logical pixels, for a toast window `height` tall:
/// centered on the primary monitor, at the top or bottom per the settings.
fn toast_origin(app: &AppHandle, height: f64) -> (f64, f64) {
    let Ok(Some(monitor)) = app.primary_monitor() else {
        return (0.0, TOAST_MARGIN);
    };
    let scale = monitor.scale_factor();
    let pos = monitor.position().to_logical::<f64>(scale);
    let size = monitor.size().to_logical::<f64>(scale);
    let x = pos.x + ((size.width - TOAST_WIDTH) / 2.0).max(0.0);
    let bottom = app.state::<Arc<AppState>>().alerts().position == Position::Bottom;
    let y = if bottom { pos.y + size.height - height - TOAST_MARGIN - 40.0 } else { pos.y + TOAST_MARGIN };
    (x, y)
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
fn take_toasts(toasts: State<'_, Toasts>) -> Vec<serde_json::Value> {
    std::mem::take(&mut *toasts.pending.lock().unwrap())
}

/// Fit the toast window to its toasts and keep it centered.
#[tauri::command]
fn toast_layout(app: AppHandle, window: WebviewWindow, height: f64) {
    let height = height.clamp(20.0, 400.0);
    let (x, y) = toast_origin(&app, height);
    let _ = window.set_size(LogicalSize::new(TOAST_WIDTH, height));
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
    if !toasts.pending.lock().unwrap().is_empty() {
        let _ = window.emit(TOAST_WAKE, ());
        return;
    }
    let mut current = toasts.window.lock().unwrap();
    if current.as_deref() == Some(window.label()) {
        *current = None;
    }
    drop(current);
    let _ = window.close();
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

/// The "Get Npcap" button. The URL is fixed here, never taken from the page.
#[tauri::command]
fn open_npcap_page() {
    open_browser(aion2dps::capture::NPCAP_URL);
}

const CUBE: &str = "cube";
const CUBE_SIZE: (f64, f64) = (460.0, 140.0);

fn cube_origin(app: &AppHandle) -> (f64, f64) {
    let Ok(Some(monitor)) = app.primary_monitor() else {
        return (0.0, TOAST_MARGIN);
    };
    let scale = monitor.scale_factor();
    let pos = monitor.position().to_logical::<f64>(scale);
    let size = monitor.size().to_logical::<f64>(scale);
    (pos.x + ((size.width - CUBE_SIZE.0) / 2.0).max(0.0), pos.y + 8.0)
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
        let (x, y) = cube_origin(&app);
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
    if let Some(rect) = window_state::fit(saved, &monitors, primary, OVERLAY_MIN) {
        let size = PhysicalSize::new(rect.width, rect.height);
        // Size again after the move: moving onto a monitor with another DPI
        // makes Windows rescale the window.
        let _ = window.set_size(size);
        let _ = window.set_position(PhysicalPosition::new(rect.x, rect.y));
        let _ = window.set_size(size);
    }
}

/// Write the overlay's current rect, unless it is minimized or maximized.
fn save_overlay_rect(app: &AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY) else {
        return;
    };
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

/// The overlay is the main window: closing it quits the whole app (or hides to
/// the tray when auto-open is on), so the meter, history and toast windows
/// close with it. Other windows only close themselves.
fn closing_quits_app(label: &str) -> bool {
    label == OVERLAY
}

/// Closing the overlay with auto-open on: stay alive in the tray instead of quitting.
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
                    let _ = app.emit("tray-show", ());
                }
                if !app.webview_windows().is_empty() {
                    let _ = app.emit("live", live);
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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
            set_start_with_windows,
            set_pop_up_on_boss,
            set_overlay_click_through,
            set_hidden_cube_alerts,
            cube_visible,
            overlay_pointer,
            overlay_hit,
            clear_history,
            monster_names,
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
            open_npcap_page
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
                if closing_quits_app(window.label()) {
                    let app = window.app_handle();
                    let state = app.state::<Arc<AppState>>();
                    if state.open_when_aion2() || state.pop_up_on_boss() {
                        // Stay in the tray for the enabled game/boss triggers.
                        close_overlay_to_tray(app, api);
                    } else {
                        // Exits the process: the capture, push and web server
                        // threads stop with it.
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
            let show_handle = handle.clone();
            handle.listen("tray-show", move |_| {
                tray_watch::reveal_overlay(&show_handle, build_overlay);
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
    fn only_the_overlay_quits_the_app() {
        assert!(closing_quits_app(OVERLAY));
        for label in [METER, HISTORY, "toast-0", "toast-7"] {
            assert!(!closing_quits_app(label), "{label}");
        }
    }
}