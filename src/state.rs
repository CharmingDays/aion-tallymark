//! Shared meter state. The capture worker writes it. The localhost pages read it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::alerts::{self, Alerts};
use crate::combat::{
    Combat, SettingsView, Snapshot, TargetMode, UnitHistory, DEFAULT_BOSS_DAMAGE,
    DEFAULT_BOSS_LOCK_SECS, DEFAULT_RESET_SECS, MAX_BOSS_DAMAGE, MAX_BOSS_LOCK_SECS,
    MIN_BOSS_DAMAGE,
};
use crate::monster_names::{self, MonsterName};
use crate::sessions::{self, Session, Summary};
use crate::target_debug::DebugInfo;

#[derive(Debug, Clone, Serialize)]
pub struct CaptureInfo {
    pub running: bool,
    pub demo: bool,
    /// This build has no path that reads another process's memory.
    pub memory_access: bool,
    pub devices: Vec<String>,
    pub segments: u64,
    pub frames: u64,
    pub hits: u64,
    pub drops: u64,
    pub locked_port: Option<u16>,
    pub error: Option<String>,
    /// Set only while the opt-in target debug log is on.
    pub target_debug: Option<DebugInfo>,
}

impl Default for CaptureInfo {
    fn default() -> Self {
        Self {
            running: false,
            demo: false,
            memory_access: false,
            devices: Vec::new(),
            segments: 0,
            frames: 0,
            hits: 0,
            drops: 0,
            locked_port: None,
            error: None,
            target_debug: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Live {
    pub meter: Snapshot,
    pub capture: CaptureInfo,
    pub settings: SettingsView,
    pub history: Vec<UnitHistory>,
    pub cube_alerts: Vec<crate::hidden_cubes::CubeAlert>,
    /// Changes whenever the pet tracker does; the Pets tab refetches then.
    pub pets_version: u64,
}

/// The opt-in local debug logs (pets, position, cubes). Each one only writes
/// what the meter already read from captured packets to a file on this PC.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DebugLogs {
    pub pets: bool,
    pub position: bool,
    pub cubes: bool,
}

impl DebugLogs {
    /// The logs turned on by `AION2DPS_DEBUG_PETS` / `_POSITION` / `_CUBES`.
    pub fn from_env() -> Self {
        let on = |name: &str| std::env::var(name).ok().as_deref() == Some("1");
        Self {
            pets: on("AION2DPS_DEBUG_PETS"),
            position: on(crate::position_debug::ENV_VAR),
            cubes: crate::cube_discovery::env_mode().is_some(),
        }
    }
}

pub struct AppState {
    combat: Mutex<Combat>,
    cubes: Mutex<crate::hidden_cubes::Detector>,
    pets: Mutex<crate::pets::Pets>,
    hidden_cube_alerts: AtomicBool,
    pet_toasts: AtomicBool,
    pet_toast_secs: AtomicU64,
    /// Pet toasts waiting for the desktop shell.
    pet_toast_queue: Mutex<Vec<crate::pets::PetToast>>,
    /// Debug logs switched on in Config (saved), and those set by env vars.
    debug_pets: AtomicBool,
    debug_position: AtomicBool,
    debug_cubes: AtomicBool,
    debug_env: DebugLogs,
    capture: Mutex<CaptureInfo>,
    /// When on, closing the overlay keeps the app in the tray and reopens it
    /// when the AION 2 client starts. Desktop shell only; combat ignores it.
    open_when_aion2: AtomicBool,
    start_with_windows: AtomicBool,
    pop_up_on_boss: AtomicBool,
    /// When on, the overlay body lets clicks through to the game; the top bar
    /// and resize edges stay interactive. Desktop shell only.
    overlay_click_through: AtomicBool,
    /// Where monster names are saved. `None` keeps them in memory only.
    names_path: Option<PathBuf>,
    /// One save at a time, so the file always holds the latest table.
    names_io: Mutex<()>,
    /// Where sessions are saved. `None` cannot save them.
    sessions_dir: Option<PathBuf>,
    /// One session save at a time, so two saves never pick the same id.
    sessions_io: Mutex<()>,
    /// Where event alerts are saved. `None` keeps them in memory only.
    alerts_path: Option<PathBuf>,
    alerts: Mutex<Alerts>,
}

impl AppState {
    pub fn new() -> Self {
        Self::with_storage(
            Some(monster_names::default_path()),
            Some(sessions::default_dir()),
        )
        .with_alerts(Some(alerts::default_path()))
        .with_pets(Some(crate::pets::default_path()))
    }

    /// Load the last known pet progress from `path` and save changes back to it.
    pub fn with_pets(self, path: Option<PathBuf>) -> Self {
        *self.pets.lock().unwrap_or_else(|e| e.into_inner()) = crate::pets::Pets::load(path);
        self
    }

    pub fn with_pets_state<T>(&self, f: impl FnOnce(&mut crate::pets::Pets) -> T) -> T {
        f(&mut self.pets.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn pets(&self) -> crate::pets::PetsView {
        self.with_pets_state(|pets| pets.view())
    }

    /// Apply a pet packet and queue its toasts when pet toasts are on.
    pub fn apply_pet_packet(&self, packet: crate::pets::PetPacket, now: i64) {
        let toasts = self.with_pets_state(|pets| pets.apply(packet, now));
        if !toasts.is_empty() && self.pet_toasts() {
            let mut queue = self.pet_toast_queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.extend(toasts);
            // Nothing is taking them (no desktop shell): keep only the latest.
            let extra = queue.len().saturating_sub(64);
            queue.drain(..extra);
        }
    }

    /// Pet toasts since the last call, oldest first.
    pub fn take_pet_toasts(&self) -> Vec<crate::pets::PetToast> {
        std::mem::take(&mut *self.pet_toast_queue.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Load monster names from `path` and save changes back to it. Sessions
    /// are not saved.
    pub fn with_monster_names(names_path: Option<PathBuf>) -> Self {
        Self::with_storage(names_path, None)
    }

    /// Monster names as in [`Self::with_monster_names`], and sessions saved
    /// in `sessions_dir`.
    pub fn with_storage(names_path: Option<PathBuf>, sessions_dir: Option<PathBuf>) -> Self {
        let mut combat = Combat::new();
        let stored = load_settings();
        stored.apply(&mut combat);
        if let Some(path) = &names_path {
            combat.set_monster_names(monster_names::load(path));
        }
        Self {
            combat: Mutex::new(combat),
            cubes: Mutex::new(crate::hidden_cubes::Detector::default()),
            pets: Mutex::new(crate::pets::Pets::load(None)),
            hidden_cube_alerts: AtomicBool::new(stored.hidden_cube_alerts),
            pet_toasts: AtomicBool::new(stored.pet_toasts),
            pet_toast_secs: AtomicU64::new(clamp_toast_secs(stored.pet_toast_secs)),
            pet_toast_queue: Mutex::new(Vec::new()),
            debug_pets: AtomicBool::new(stored.debug_pets),
            debug_position: AtomicBool::new(stored.debug_position),
            debug_cubes: AtomicBool::new(stored.debug_cubes),
            debug_env: DebugLogs::from_env(),
            capture: Mutex::new(CaptureInfo::default()),
            open_when_aion2: AtomicBool::new(stored.open_when_aion2),
            start_with_windows: AtomicBool::new(stored.start_with_windows),
            pop_up_on_boss: AtomicBool::new(stored.pop_up_on_boss),
            overlay_click_through: AtomicBool::new(stored.overlay_click_through),
            names_path,
            names_io: Mutex::new(()),
            sessions_dir,
            sessions_io: Mutex::new(()),
            alerts_path: None,
            alerts: Mutex::new(Alerts::default()),
        }
    }

    /// Load event alerts from `path` and save changes back to it.
    pub fn with_alerts(mut self, path: Option<PathBuf>) -> Self {
        if let Some(path) = &path {
            let mut loaded = alerts::load(path);
            if alerts::seed_defaults(&mut loaded) {
                match alerts::validate(loaded.clone()) {
                    Ok(valid) => {
                        if let Err(err) = alerts::save(path, &valid) {
                            tracing::warn!("could not save alerts to {}: {err}", path.display());
                        }
                        loaded = valid;
                    }
                    Err(err) => tracing::warn!("built-in timers did not validate: {err}"),
                }
            }
            self.alerts = Mutex::new(loaded);
        }
        self.alerts_path = path;
        self
    }

    pub fn alerts(&self) -> Alerts {
        self.alerts.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Replace the alert settings and list. The error is a sentence for the user.
    pub fn set_alerts(&self, alerts: Alerts) -> Result<Alerts, String> {
        let mut current = self.alerts.lock().unwrap_or_else(|e| e.into_inner());
        self.store_alerts(&mut current, alerts)
    }

    fn store_alerts(&self, current: &mut Alerts, mut alerts: Alerts) -> Result<Alerts, String> {
        // A page that does not know the field must not make the built-in
        // timers come back on the next launch.
        alerts.defaults_version = alerts.defaults_version.max(current.defaults_version);
        let alerts = alerts::validate(alerts)?;
        if let Some(path) = &self.alerts_path {
            alerts::save(path, &alerts).map_err(|err| {
                tracing::warn!("could not save alerts to {}: {err}", path.display());
                "could not save the alerts file".to_string()
            })?;
        }
        *current = alerts.clone();
        Ok(alerts)
    }

    /// "Restore default timers": add back only the missing built-in ones.
    pub fn restore_default_alerts(&self) -> Result<alerts::Restored, String> {
        let mut current = self.alerts.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = current.clone();
        let restored = alerts::restore_defaults(&mut next);
        let alerts = if restored > 0 { self.store_alerts(&mut current, next)? } else { current.clone() };
        Ok(alerts::Restored { restored, alerts })
    }

    /// Save the open fight as a new session.
    pub fn save_session(&self, now: i64) -> Result<Summary, sessions::Error> {
        let dir = self.sessions_dir()?;
        let session = self
            .with_combat(|combat| {
                combat.tick(now);
                combat.session(now)
            })
            .ok_or(sessions::Error::NothingToSave)?;
        let _saving = self.sessions_io.lock().unwrap_or_else(|e| e.into_inner());
        let id = sessions::save(dir, &session)?;
        Ok(Summary::of(id, &session))
    }

    pub fn sessions(&self) -> Vec<Summary> {
        self.sessions_dir
            .as_deref()
            .map(sessions::list)
            .unwrap_or_default()
    }

    pub fn session(&self, id: &str) -> Result<Session, sessions::Error> {
        sessions::load(self.sessions_dir()?, id)
    }

    /// The saved file, checked to be a session.
    pub fn session_file(&self, id: &str) -> Result<Vec<u8>, sessions::Error> {
        sessions::read(self.sessions_dir()?, id)
    }

    pub fn delete_session(&self, id: &str) -> Result<(), sessions::Error> {
        sessions::delete(self.sessions_dir()?, id)
    }

    /// Set or clear a saved session's custom name. Empty clears it.
    pub fn rename_session(&self, id: &str, name: &str) -> Result<Summary, sessions::Error> {
        let dir = self.sessions_dir()?;
        let _io = self.sessions_io.lock().unwrap_or_else(|e| e.into_inner());
        sessions::rename(dir, id, name)
    }

    fn sessions_dir(&self) -> Result<&Path, sessions::Error> {
        self.sessions_dir
            .as_deref()
            .ok_or(sessions::Error::NoFolder)
    }

    pub fn monster_names(&self) -> Vec<MonsterName> {
        self.with_combat(|combat| monster_names::list(combat.monster_names()))
    }

    /// Name a monster type. An empty name clears it. The error is a sentence
    /// for the user.
    pub fn set_monster_name(&self, monster_type: u32, name: &str) -> Result<(), String> {
        if !monster_names::valid_type(monster_type) {
            return Err("unknown monster type".into());
        }
        let name = monster_names::clean_name(name)?;
        let _saving = self.names_io.lock().unwrap_or_else(|e| e.into_inner());
        let names = self.with_combat(|combat| {
            combat.set_monster_name(monster_type, name);
            combat.monster_names().clone()
        });
        let Some(path) = &self.names_path else {
            return Ok(());
        };
        monster_names::save(path, &names)
            .map_err(|err| format!("could not save {} ({err})", path.display()))
    }

    pub fn live(&self, now: i64) -> Live {
        let (meter, mut settings, history) = {
            let mut combat = self.combat.lock().unwrap_or_else(|e| e.into_inner());
            combat.tick(now);
            (combat.snapshot(now), combat.settings(), combat.history())
        };
        settings.open_when_aion2 = self.open_when_aion2();
        settings.start_with_windows = self.start_with_windows();
        settings.pop_up_on_boss = self.pop_up_on_boss();
        settings.overlay_click_through = self.overlay_click_through();
        settings.hidden_cube_alerts = self.hidden_cube_alerts();
        settings.pet_toasts = self.pet_toasts();
        settings.pet_toast_secs = self.pet_toast_secs();
        settings.debug_pets = self.debug_pets.load(Ordering::Relaxed);
        settings.debug_position = self.debug_position.load(Ordering::Relaxed);
        settings.debug_cubes = self.debug_cubes.load(Ordering::Relaxed);
        settings.debug_env = self.debug_env;
        Live {
            cube_alerts: self.with_cubes(|cubes| {
                if self.hidden_cube_alerts() {
                    let mut list = cubes.alerts(now);
                    for a in list.iter_mut() {
                        if let Some(by) = a.opened_by_id {
                            a.opened_by = self.with_combat(|c| c.display_name(by));
                        }
                    }
                    list
                } else {
                    cubes.clear_alerts();
                    Vec::new()
                }
            }),
            meter,
            settings,
            history,
            pets_version: self.with_pets_state(|pets| pets.version()),
            capture: self
                .capture
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        }
    }

    pub fn with_combat<T>(&self, f: impl FnOnce(&mut Combat) -> T) -> T {
        let mut combat = self.combat.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut combat)
    }

    pub fn with_capture<T>(&self, f: impl FnOnce(&mut CaptureInfo) -> T) -> T {
        let mut capture = self.capture.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut capture)
    }

    pub fn set_mode(&self, mode: TargetMode) {
        self.with_combat(|combat| combat.set_mode(mode));
    }

    pub fn reset(&self, now: i64) {
        self.with_combat(|combat| combat.reset(now));
    }

    pub fn set_reset_after_secs(&self, secs: u64) -> bool {
        let saved = self.with_combat(|combat| combat.set_reset_after_secs(secs));
        if saved {
            self.store_settings();
        }
        saved
    }

    /// Either value may be left out. Both are checked before either applies.
    pub fn set_boss_settings(
        &self,
        lock_secs: Option<u64>,
        damage_threshold: Option<i64>,
        engage_lock: Option<bool>,
        now: i64,
    ) -> Result<(), String> {
        if lock_secs.is_some_and(|secs| secs > MAX_BOSS_LOCK_SECS) {
            return Err(format!("enter a lock of 0 to {MAX_BOSS_LOCK_SECS} seconds"));
        }
        if damage_threshold.is_some_and(|d| !(MIN_BOSS_DAMAGE..=MAX_BOSS_DAMAGE).contains(&d)) {
            return Err(format!("enter a boss damage of at least {MIN_BOSS_DAMAGE}"));
        }
        self.with_combat(|combat| {
            if let Some(secs) = lock_secs {
                combat.set_boss_lock_secs(secs, now);
            }
            if let Some(damage) = damage_threshold {
                combat.set_boss_damage_threshold(damage);
            }
            if let Some(on) = engage_lock {
                combat.set_boss_engage_lock(on);
            }
        });
        self.store_settings();
        Ok(())
    }

    /// End a boss lock early. False when there was none.
    pub fn unlock(&self) -> bool {
        self.with_combat(|combat| combat.unlock())
    }

    /// Whether the desktop shell should wait in the tray for AION 2.
    pub fn open_when_aion2(&self) -> bool {
        self.open_when_aion2.load(Ordering::Relaxed)
    }

    /// Persist the tray auto-open preference. Default is off.
    pub fn set_open_when_aion2(&self, on: bool) {
        self.open_when_aion2.store(on, Ordering::Relaxed);
        self.store_settings();
    }

    pub fn start_with_windows(&self) -> bool {
        self.start_with_windows.load(Ordering::Relaxed)
    }

    /// Called by the desktop shell only after Task Scheduler succeeds.
    pub fn set_start_with_windows(&self, on: bool) {
        self.start_with_windows.store(on, Ordering::Relaxed);
        self.store_settings();
    }

    pub fn pop_up_on_boss(&self) -> bool {
        self.pop_up_on_boss.load(Ordering::Relaxed)
    }

    pub fn set_pop_up_on_boss(&self, on: bool) {
        self.pop_up_on_boss.store(on, Ordering::Relaxed);
        self.store_settings();
    }

    /// Whether the overlay body should ignore the mouse (top bar stays live).
    pub fn overlay_click_through(&self) -> bool {
        self.overlay_click_through.load(Ordering::Relaxed)
    }

    /// Persist overlay click-through. Default is on.
    pub fn set_overlay_click_through(&self, on: bool) {
        self.overlay_click_through.store(on, Ordering::Relaxed);
        self.store_settings();
    }

    pub fn with_cubes<T>(&self, f: impl FnOnce(&mut crate::hidden_cubes::Detector) -> T) -> T {
        f(&mut self.cubes.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn hidden_cube_alerts(&self) -> bool {
        self.hidden_cube_alerts.load(Ordering::Relaxed)
    }
    pub fn set_hidden_cube_alerts(&self, on: bool) {
        self.hidden_cube_alerts.store(on, Ordering::Relaxed);
        self.with_cubes(|cubes| cubes.clear_alerts());
        self.store_settings();
    }

    pub fn pet_toasts(&self) -> bool {
        self.pet_toasts.load(Ordering::Relaxed)
    }

    pub fn pet_toast_secs(&self) -> u64 {
        self.pet_toast_secs.load(Ordering::Relaxed)
    }

    /// Either may be left out. An out-of-range time is an error and nothing changes.
    pub fn set_pet_toasts(&self, on: Option<bool>, secs: Option<u64>) -> Result<(), String> {
        use crate::pets::{MAX_TOAST_SECS, MIN_TOAST_SECS};
        if secs.is_some_and(|s| !(MIN_TOAST_SECS..=MAX_TOAST_SECS).contains(&s)) {
            return Err(format!("enter {MIN_TOAST_SECS} to {MAX_TOAST_SECS} seconds"));
        }
        if let Some(on) = on {
            self.pet_toasts.store(on, Ordering::Relaxed);
            if !on {
                self.take_pet_toasts();
            }
        }
        if let Some(secs) = secs {
            self.pet_toast_secs.store(secs, Ordering::Relaxed);
        }
        self.store_settings();
        Ok(())
    }

    /// The debug logs that should be writing now: switched on in Config or
    /// by an env var. The decode thread opens or closes them when this changes.
    pub fn debug_logs(&self) -> DebugLogs {
        DebugLogs {
            pets: self.debug_env.pets || self.debug_pets.load(Ordering::Relaxed),
            position: self.debug_env.position || self.debug_position.load(Ordering::Relaxed),
            cubes: self.debug_env.cubes || self.debug_cubes.load(Ordering::Relaxed),
        }
    }

    /// Any may be left out. Saved, and applied without a restart.
    pub fn set_debug_logs(&self, pets: Option<bool>, position: Option<bool>, cubes: Option<bool>) {
        if let Some(on) = pets {
            self.debug_pets.store(on, Ordering::Relaxed);
        }
        if let Some(on) = position {
            self.debug_position.store(on, Ordering::Relaxed);
        }
        if let Some(on) = cubes {
            self.debug_cubes.store(on, Ordering::Relaxed);
        }
        self.store_settings();
    }

    fn store_settings(&self) {
        let view = self.with_combat(|combat| combat.settings());
        store_settings(&StoredSettings {
            reset_after_secs: view.reset_after_secs,
            boss_lock_secs: view.boss_lock_secs,
            boss_damage_threshold: view.boss_damage_threshold,
            boss_engage_lock: view.boss_engage_lock,
            open_when_aion2: self.open_when_aion2(),
            start_with_windows: self.start_with_windows(),
            pop_up_on_boss: self.pop_up_on_boss(),
            overlay_click_through: self.overlay_click_through(),
            hidden_cube_alerts: self.hidden_cube_alerts(),
            pet_toasts: self.pet_toasts(),
            pet_toast_secs: self.pet_toast_secs(),
            debug_pets: self.debug_pets.load(Ordering::Relaxed),
            debug_position: self.debug_position.load(Ordering::Relaxed),
            debug_cubes: self.debug_cubes.load(Ordering::Relaxed),
        });
    }

    pub fn clear_history(&self) {
        self.with_combat(|combat| combat.clear_history());
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// `%APPDATA%\aion2dps\settings.json`. Every field has a default, so a
/// file from an older version (only `reset_after_secs`) still loads.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct StoredSettings {
    reset_after_secs: u64,
    boss_lock_secs: u64,
    boss_damage_threshold: i64,
    boss_engage_lock: bool,
    /// Default off: the app does not wait in the tray for the game.
    open_when_aion2: bool,
    start_with_windows: bool,
    pop_up_on_boss: bool,
    /// Default on: overlay body is click-through.
    overlay_click_through: bool,
    hidden_cube_alerts: bool,
    /// Default on, 60 s after the last soul.
    pet_toasts: bool,
    pet_toast_secs: u64,
    /// Default off: the local debug logs (env vars still turn them on).
    debug_pets: bool,
    debug_position: bool,
    debug_cubes: bool,
}

fn clamp_toast_secs(secs: u64) -> u64 {
    secs.clamp(crate::pets::MIN_TOAST_SECS, crate::pets::MAX_TOAST_SECS)
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            reset_after_secs: DEFAULT_RESET_SECS,
            boss_lock_secs: DEFAULT_BOSS_LOCK_SECS,
            boss_damage_threshold: DEFAULT_BOSS_DAMAGE,
            boss_engage_lock: true,
            open_when_aion2: false,
            start_with_windows: false,
            pop_up_on_boss: true,
            overlay_click_through: true,
            hidden_cube_alerts: true,
            pet_toasts: true,
            pet_toast_secs: crate::pets::DEFAULT_TOAST_SECS,
            debug_pets: false,
            debug_position: false,
            debug_cubes: false,
        }
    }
}

impl From<SettingsView> for StoredSettings {
    fn from(view: SettingsView) -> Self {
        Self {
            reset_after_secs: view.reset_after_secs,
            boss_lock_secs: view.boss_lock_secs,
            boss_damage_threshold: view.boss_damage_threshold,
            boss_engage_lock: view.boss_engage_lock,
            open_when_aion2: view.open_when_aion2,
            start_with_windows: view.start_with_windows,
            pop_up_on_boss: view.pop_up_on_boss,
            overlay_click_through: view.overlay_click_through,
            hidden_cube_alerts: view.hidden_cube_alerts,
            pet_toasts: view.pet_toasts,
            pet_toast_secs: view.pet_toast_secs,
            debug_pets: view.debug_pets,
            debug_position: view.debug_position,
            debug_cubes: view.debug_cubes,
        }
    }
}

impl StoredSettings {
    /// A value out of range keeps the default.
    fn apply(&self, combat: &mut Combat) {
        let _ = combat.set_reset_after_secs(self.reset_after_secs);
        let _ = combat.set_boss_lock_secs(self.boss_lock_secs, 0);
        let _ = combat.set_boss_damage_threshold(self.boss_damage_threshold);
        combat.set_boss_engage_lock(self.boss_engage_lock);
    }
}

/// Tests never touch the real `%APPDATA%` settings file.
#[cfg(test)]
fn settings_path() -> Option<PathBuf> {
    None
}

#[cfg(not(test))]
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("aion2dps").join("settings.json"))
}

fn load_settings() -> StoredSettings {
    settings_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn store_settings(settings: &StoredSettings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(settings) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_setting_defaults_on_and_roundtrips_live_without_enabling_demo_alerts() {
        let state = AppState::with_storage(None, None);
        assert!(state.live(0).settings.hidden_cube_alerts);
        state.set_hidden_cube_alerts(false);
        assert!(!state.live(0).settings.hidden_cube_alerts);
        assert!(state.live(0).cube_alerts.is_empty());
        state.set_reset_after_secs(40);
        assert!(!state.hidden_cube_alerts());
        state.set_hidden_cube_alerts(true);
        assert!(state.live(0).cube_alerts.is_empty());
    }

    #[test]
    fn hidden_cube_preference_is_backward_compatible_and_serializable() {
        let mut stored: StoredSettings = serde_json::from_str("{}").unwrap();
        assert!(stored.hidden_cube_alerts);
        stored.hidden_cube_alerts = false;
        let back: StoredSettings = serde_json::from_str(&serde_json::to_string(&stored).unwrap()).unwrap();
        assert!(!back.hidden_cube_alerts);
    }

    #[test]
    fn a_settings_file_keeps_only_the_reset_delay() {
        let stored: StoredSettings = serde_json::from_str(
            r#"{"reset_after_secs":12,"transparency":70,"font_px":18,"show_dps":false}"#,
        )
        .expect("settings");
        assert_eq!(stored.reset_after_secs, 12);
        assert_eq!(stored.boss_lock_secs, DEFAULT_BOSS_LOCK_SECS);
        assert_eq!(stored.boss_damage_threshold, DEFAULT_BOSS_DAMAGE);
        assert!(!stored.open_when_aion2);
        assert!(!stored.start_with_windows);
        assert!(stored.pop_up_on_boss);
        assert!(stored.overlay_click_through);
    }

    #[test]
    fn boss_settings_round_trip_and_bad_values_keep_defaults() {
        let mut combat = Combat::new();
        combat.set_boss_lock_secs(25, 0);
        combat.set_boss_damage_threshold(2_000_000);
        let text = serde_json::to_string(&StoredSettings::from(combat.settings())).unwrap();
        let back: StoredSettings = serde_json::from_str(&text).unwrap();
        let mut fresh = Combat::new();
        back.apply(&mut fresh);
        assert_eq!(fresh.settings().boss_lock_secs, 25);
        assert_eq!(fresh.settings().boss_damage_threshold, 2_000_000);

        let bad: StoredSettings =
            serde_json::from_str(r#"{"boss_lock_secs":999,"boss_damage_threshold":-5}"#).unwrap();
        let mut fresh = Combat::new();
        bad.apply(&mut fresh);
        assert_eq!(fresh.settings().boss_lock_secs, DEFAULT_BOSS_LOCK_SECS);
        assert_eq!(fresh.settings().boss_damage_threshold, DEFAULT_BOSS_DAMAGE);
        assert_eq!(serde_json::from_str::<StoredSettings>("{}").unwrap(), StoredSettings::default());
    }

    #[test]
    fn boss_settings_are_checked_before_they_apply() {
        let state = AppState::with_storage(None, None);
        let before = state.with_combat(|c| c.settings().boss_lock_secs);
        assert!(state.set_boss_settings(Some(61), None, None, 0).is_err());
        assert!(state.set_boss_settings(None, Some(0), Some(false), 0).is_err());
        assert!(state.set_boss_settings(Some(5), Some(0), None, 0).is_err());
        assert_eq!(state.with_combat(|c| c.settings().boss_lock_secs), before);
        assert!(!state.unlock());
    }

    #[test]
    fn desktop_startup_defaults_and_preferences_survive_other_saves() {
        let state = AppState::with_storage(None, None);
        assert!(!state.start_with_windows());
        assert!(state.pop_up_on_boss());
        state.set_start_with_windows(true);
        state.set_pop_up_on_boss(false);
        state.set_reset_after_secs(40);
        let settings = state.live(0).settings;
        assert!(settings.start_with_windows);
        assert!(!settings.pop_up_on_boss);
        let stored = StoredSettings::from(settings);
        let decoded: StoredSettings = serde_json::from_str(&serde_json::to_string(&stored).unwrap()).unwrap();
        assert_eq!(stored, decoded);
        assert!(decoded.start_with_windows);
        assert!(!decoded.pop_up_on_boss);
    }

    #[test]
    fn open_when_aion2_defaults_off_and_survives_other_saves() {
        let state = AppState::with_storage(None, None);
        assert!(!state.open_when_aion2());
        assert!(!state.live(0).settings.open_when_aion2);
        state.set_open_when_aion2(true);
        assert!(state.open_when_aion2());
        assert!(state.live(0).settings.open_when_aion2);
        assert!(state.set_reset_after_secs(40));
        assert!(state.open_when_aion2(), "saving the reset delay must keep the tray option");
        state.set_open_when_aion2(false);
        assert!(!state.open_when_aion2());
    }

    #[test]
    fn overlay_click_through_defaults_on_and_survives_other_saves() {
        let state = AppState::with_storage(None, None);
        assert!(state.overlay_click_through());
        assert!(state.live(0).settings.overlay_click_through);
        state.set_overlay_click_through(false);
        assert!(!state.overlay_click_through());
        assert!(!state.live(0).settings.overlay_click_through);
        assert!(state.set_reset_after_secs(40));
        assert!(!state.overlay_click_through(), "saving the reset delay must keep click-through");
        state.set_overlay_click_through(true);
        assert!(state.overlay_click_through());
    }

    #[test]
    fn a_monster_name_is_saved_and_loaded_on_the_next_start() {
        let dir = std::env::temp_dir().join(format!("aion2dps-state-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(monster_names::FILE_NAME);

        let state = AppState::with_monster_names(Some(path.clone()));
        assert!(state.monster_names().is_empty(), "no prefilled names");
        state.set_monster_name(0x200b81, "  Rock Golem ").unwrap();
        assert!(state.set_monster_name(0x200b81, &"x".repeat(41)).is_err());
        assert!(state.set_monster_name(0, "Nope").is_err());
        assert!(state.set_monster_name(0x200c77, "Bad\u{1b}").is_err());

        let again = AppState::with_monster_names(Some(path.clone()));
        let names = again.monster_names();
        assert_eq!(names.len(), 1);
        assert_eq!((names[0].monster_type, names[0].name.as_str()), (0x200b81, "Rock Golem"));

        again.set_monster_name(0x200b81, "").unwrap();
        assert!(AppState::with_monster_names(Some(path)).monster_names().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
