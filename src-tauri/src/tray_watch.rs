//! System tray wait mode and AION 2 process watcher.
//!
//! When "Open automatically when Aion 2 is detected" is on, closing the overlay
//! hides the windows and keeps the process alive in the tray. A lightweight
//! poll of process names (no game interaction) shows the overlay again only on
//! the transition from not-running to running.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use aion2dps::game_process;
use aion2dps::state::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "aion2dps-tray";
const POLL: Duration = Duration::from_secs(2);

/// Shared flags for tray wait mode.
#[derive(Default)]
pub struct TrayWatch {
    /// True after the user closed the overlay into the tray; cleared when shown.
    waiting: AtomicBool,
}

impl TrayWatch {
    pub fn waiting(&self) -> bool {
        self.waiting.load(Ordering::Relaxed)
    }

    pub fn set_waiting(&self, on: bool) {
        self.waiting.store(on, Ordering::Relaxed);
    }
}

/// Hide the overlay (and meter/history if open) and keep a tray icon with Quit.
pub fn hide_to_tray(app: &AppHandle) -> tauri::Result<()> {
    let tray = app.state::<TrayWatch>();
    tray.set_waiting(true);
    ensure_tray(app)?;
    for label in ["overlay", "meter", "history"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    Ok(())
}

// The overlay is shown again by main's `reveal_overlay` (tray menu, click,
// game start, boss).

/// Show without taking focus from the game (also restores a minimized window).
#[cfg(windows)]
pub fn show_no_activate(window: &tauri::WebviewWindow) -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    }
    const SW_SHOWNOACTIVATE: i32 = 4;
    match window.hwnd() {
        Ok(hwnd) => {
            // Restores a minimized window too, and leaves focus on the game.
            unsafe { ShowWindow(hwnd.0 as isize, SW_SHOWNOACTIVATE) };
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
pub fn show_no_activate(_window: &tauri::WebviewWindow) -> bool {
    false
}

fn ensure_tray(app: &AppHandle) -> tauri::Result<()> {
    if app.tray_by_id(TRAY_ID).is_some() {
        return Ok(());
    }
    let show = MenuItem::with_id(app, "show", "Show overlay", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("aion2dps")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                // build_overlay is wired from main via a custom event.
                let _ = app.emit("tray-show", ());
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tray.app_handle().emit("tray-show", ());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Poll process names. Opens the overlay only on not-running → running while waiting.
pub fn start_watcher(app: AppHandle, state: Arc<AppState>) {
    thread::Builder::new()
        .name("aion2dps-game-watch".into())
        .spawn(move || {
            let mut was_running = game_process::aion2_running();
            loop {
                thread::sleep(POLL);
                let running = game_process::aion2_running();
                let should_open = state.open_when_aion2()
                    && app.state::<TrayWatch>().waiting()
                    && !was_running
                    && running;
                if should_open {
                    let _ = app.emit("tray-show", ());
                }
                was_running = running;
            }
        })
        .expect("game watch thread");
}
