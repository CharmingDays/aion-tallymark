//! Saved fights. Each session is one JSON file in
//! `%LOCALAPPDATA%\aion2dps\sessions\`, named `<UTC start>-<target>.json`.
//! The file name without `.json` is the session id.
//!
//! A session keeps the totals the meter showed when it was saved, plus every
//! damage hit on the counted targets, so the fight can be replayed. Hits are
//! compact arrays `[t_ms, actor, damage, flags, skill]`: `t_ms` is the offset
//! from the first hit, `actor` an index into `actors`, and `flags` has
//! [`FLAG_CRIT`] and [`FLAG_DOT`]. A fight with more than
//! [`crate::combat::SESSION_EVENT_LIMIT`] hits keeps its totals but only the
//! first hits, and is saved with `truncated: true`.
//!
//! `skill_names` names each skill id in `events`, so a replay can label hits
//! without the skill table. Files saved before it existed get the names
//! filled in when they are read.
//!
//! Schema 2 adds healing: `heals` are `[t_ms, actor, amount, flags, skill]`
//! with [`FLAG_HOT`] for heal-over-time ticks, plus heal totals per player and
//! for the group. Schema 1 files still load and simply have no healing.
//!
//! An optional `name` is a user-given label shown in history. Missing or empty
//! falls back to `target.label`. Renaming never changes the file name (the id).

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage;

/// 3 added `target.defeated` and `target.died_ms`; older files load with
/// both unset.
pub const SCHEMA_VERSION: u32 = 3;
/// The oldest schema that still loads.
pub const MIN_SCHEMA_VERSION: u32 = 1;
pub const FOLDER: &str = "sessions";
/// `--demo` keeps its sessions apart from real ones.
pub const DEMO_FOLDER: &str = "sessions-demo";
/// The history lists at most this many files, newest first.
pub const LIST_LIMIT: usize = 500;
pub const MAX_ID_CHARS: usize = 96;
/// Cap on a custom session name (characters, not bytes).
pub const MAX_NAME_CHARS: usize = 60;
const MAX_SLUG_CHARS: usize = 48;

pub const FLAG_CRIT: u8 = 1;
pub const FLAG_DOT: u8 = 2;
/// A heal-over-time tick, in `heals`.
pub const FLAG_HOT: u8 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub schema_version: u32,
    /// Milliseconds since the Unix epoch.
    pub started_at: i64,
    pub ended_at: i64,
    /// Time spent dealing damage, the same clock the meter's DPS uses.
    pub duration_ms: i64,
    pub target: Target,
    pub totals: Totals,
    pub players: Vec<Player>,
    pub actors: Vec<String>,
    pub events: Vec<Event>,
    pub truncated: bool,
    /// Skill id to English name, for the ids in `events`.
    #[serde(default)]
    pub skill_names: BTreeMap<i32, String>,
    /// Index into `actors` of the local player, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub you: Option<u32>,
    /// Heal ticks, schema 2. Empty for older files.
    #[serde(default)]
    pub heals: Vec<Heal>,
    /// User-given label. Missing or empty falls back to [`Target::label`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Stagger breaks on the target, ms from `started_at`. Empty for older files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stagger_breaks: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub label: String,
    pub boss: bool,
    pub dummy: bool,
    /// The target died in this fight.
    #[serde(default)]
    pub defeated: bool,
    /// When it died, ms from `started_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub died_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    pub total_damage: i64,
    pub party_dps: i64,
    #[serde(default)]
    pub total_heal: i64,
    #[serde(default)]
    pub group_hps: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub total: i64,
    pub dps: i64,
    /// Percent of the group's damage.
    pub share: f64,
    pub crit_pct: f64,
    #[serde(default)]
    pub heal: i64,
    #[serde(default)]
    pub hps: i64,
    /// Estimated stagger points. 0 (left out) for older files.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub stagger: f64,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

/// `[t_ms, actor index, damage, flags, skill id]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event(pub i64, pub u32, pub i64, pub u8, pub i32);

/// `[t_ms, actor index, healed, flags, skill id]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heal(pub i64, pub u32, pub i64, pub u8, pub i32);

fn supported(version: u32) -> bool {
    (MIN_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&version)
}

/// A list entry: the session without its players and hits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    #[serde(default)]
    pub id: String,
    pub schema_version: u32,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_ms: i64,
    pub target: Target,
    pub totals: Totals,
    pub truncated: bool,
    /// User-given label. Missing or empty falls back to [`Target::label`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Summary {
    pub fn of(id: String, session: &Session) -> Self {
        Self {
            id,
            schema_version: session.schema_version,
            started_at: session.started_at,
            ended_at: session.ended_at,
            duration_ms: session.duration_ms,
            target: session.target.clone(),
            totals: session.totals.clone(),
            truncated: session.truncated,
            name: session.name.clone(),
        }
    }

    /// The label shown in the history list.
    pub fn label(&self) -> &str {
        display_name(self.name.as_deref(), &self.target.label)
    }
}

impl Session {
    /// The label shown for this session.
    pub fn label(&self) -> &str {
        display_name(self.name.as_deref(), &self.target.label)
    }
}

/// Trim, drop control characters, and cap length. Empty means "use the default".
pub fn normalize_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.chars().take(MAX_NAME_CHARS).collect())
}

fn display_name<'a>(name: Option<&'a str>, fallback: &'a str) -> &'a str {
    name.filter(|n| !n.is_empty()).unwrap_or(fallback)
}

/// Why a session action failed. The text is for the user and never holds a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    BadId,
    NotFound,
    Damaged,
    NothingToSave,
    NoFolder,
    Io,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BadId => "invalid session id",
            Self::NotFound => "no such session",
            Self::Damaged => "the session file is damaged",
            Self::NothingToSave => "no damage to save yet",
            Self::NoFolder => "sessions are not saved in this mode",
            Self::Io => "could not read or write the sessions folder",
        })
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::InvalidData => Self::Damaged,
            _ => Self::Io,
        }
    }
}

pub fn default_dir() -> PathBuf {
    storage::folder().join(FOLDER)
}

pub fn demo_dir() -> PathBuf {
    storage::folder().join(DEMO_FOLDER)
}

/// 1 to 96 of `0-9 A-Z a-z _ -`, so an id can never leave the folder, and
/// not a Windows device name.
pub fn valid_id(id: &str) -> bool {
    let reserved = |id: &str| {
        let upper = id.to_ascii_uppercase();
        matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((upper.starts_with("COM") || upper.starts_with("LPT"))
                && upper.len() == 4
                && upper.as_bytes()[3].is_ascii_digit())
    };
    (1..=MAX_ID_CHARS).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !reserved(id)
}

fn file(dir: &Path, id: &str) -> Result<PathBuf, Error> {
    if valid_id(id) {
        Ok(dir.join(format!("{id}.json")))
    } else {
        Err(Error::BadId)
    }
}

/// Write the session under a new id and return the id.
pub fn save(dir: &Path, session: &Session) -> Result<String, Error> {
    let base = format!("{}-{}", stamp(session.started_at), slug(&session.target.label));
    let id = (1..1000)
        .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
        .find(|id| !dir.join(format!("{id}.json")).exists())
        .ok_or(Error::Io)?;
    let bytes = serde_json::to_vec(session).map_err(|_| Error::Io)?;
    storage::write_atomic(&file(dir, &id)?, &bytes).map_err(|err| {
        tracing::warn!("could not save session {id} in {}: {err}", dir.display());
        Error::Io
    })?;
    Ok(id)
}

/// Newest first. Only the newest [`LIST_LIMIT`] files are read, and a file
/// that is not a session is skipped.
pub fn list(dir: &Path) -> Vec<Summary> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let id = name.strip_suffix(".json")?;
            valid_id(id).then(|| id.to_string())
        })
        .collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids.truncate(LIST_LIMIT);
    let mut list: Vec<Summary> = ids
        .into_iter()
        .filter_map(|id| {
            let bytes = std::fs::read(dir.join(format!("{id}.json"))).ok()?;
            let summary = serde_json::from_slice::<Summary>(&bytes)
                .ok()
                .filter(|summary| supported(summary.schema_version))?;
            Some(Summary { id, ..summary })
        })
        .collect();
    list.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| b.id.cmp(&a.id)));
    list
}

pub fn load(dir: &Path, id: &str) -> Result<Session, Error> {
    let mut session = parse(&std::fs::read(file(dir, id)?)?)?;
    fill_skill_names(&mut session);
    Ok(session)
}

/// The file as saved, once it is known to be a session. A file saved
/// without skill names gets them added.
pub fn read(dir: &Path, id: &str) -> Result<Vec<u8>, Error> {
    let bytes = std::fs::read(file(dir, id)?)?;
    let mut session = parse(&bytes)?;
    if fill_skill_names(&mut session) {
        return serde_json::to_vec(&session).map_err(|_| Error::Io);
    }
    Ok(bytes)
}

/// The names of the skills in `events`.
pub fn skill_names(events: &[Event]) -> BTreeMap<i32, String> {
    skill_names_with(events, &[])
}

/// The names of the skills in `events` and `heals`.
pub fn skill_names_with(events: &[Event], heals: &[Heal]) -> BTreeMap<i32, String> {
    let mut names = BTreeMap::new();
    for skill in events.iter().map(|e| e.4).chain(heals.iter().map(|h| h.4)) {
        names
            .entry(skill)
            .or_insert_with(|| crate::skills::skill_name(skill));
    }
    names
}

/// Name the skills of a session saved without names. True when it changed.
fn fill_skill_names(session: &mut Session) -> bool {
    if !session.skill_names.is_empty() || (session.events.is_empty() && session.heals.is_empty()) {
        return false;
    }
    session.skill_names = skill_names_with(&session.events, &session.heals);
    true
}

pub fn delete(dir: &Path, id: &str) -> Result<(), Error> {
    Ok(std::fs::remove_file(file(dir, id)?)?)
}

/// Set or clear the custom name. The file id stays the same. An empty name
/// removes the custom label so the target name shows again.
pub fn rename(dir: &Path, id: &str, name: &str) -> Result<Summary, Error> {
    let path = file(dir, id)?;
    let mut session = parse(&std::fs::read(&path)?)?;
    session.name = normalize_name(name);
    let bytes = serde_json::to_vec(&session).map_err(|_| Error::Io)?;
    storage::write_atomic(&path, &bytes).map_err(|err| {
        tracing::warn!("could not rename session {id} in {}: {err}", dir.display());
        Error::Io
    })?;
    Ok(Summary::of(id.to_string(), &session))
}

fn parse(bytes: &[u8]) -> Result<Session, Error> {
    serde_json::from_slice::<Session>(bytes)
        .ok()
        .filter(|session| supported(session.schema_version))
        .ok_or(Error::Damaged)
}

/// `20261102-231200` in UTC.
fn stamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, time) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (year, month, day) = civil_date(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        time / 3600,
        time % 3600 / 60,
        time % 60
    )
}

/// Days since 1970-01-01 as a calendar date (Howard Hinnant's `civil_from_days`).
fn civil_date(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// Lowercase ASCII letters and digits joined by `-`, or `session`.
fn slug(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if out.len() >= MAX_SLUG_CHARS {
            break;
        }
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "session".into()
    } else {
        out.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aion2dps-sessions-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample(started_at: i64, label: &str) -> Session {
        Session {
            schema_version: SCHEMA_VERSION,
            started_at,
            ended_at: started_at + 12_000,
            duration_ms: 12_000,
            target: Target {
                label: label.into(),
                boss: true,
                dummy: false,
                defeated: true,
                died_ms: Some(11_500),
            },
            totals: Totals {
                total_damage: 3_000,
                party_dps: 250,
                total_heal: 900,
                group_hps: 75,
            },
            players: vec![Player {
                name: "Aria".into(),
                total: 3_000,
                dps: 250,
                share: 100.0,
                crit_pct: 50.0,
                heal: 900,
                hps: 75,
                stagger: 0.0,
            }],
            actors: vec!["Aria".into()],
            events: vec![Event(0, 0, 1_000, FLAG_CRIT, 100), Event(12_000, 0, 2_000, FLAG_DOT, 200)],
            truncated: false,
            skill_names: BTreeMap::from([(100, "Strike".into()), (200, "Bleed".into())]),
            you: None,
            heals: vec![Heal(500, 0, 400, 0, 300), Heal(1_500, 0, 500, FLAG_HOT, 300)],
            name: None,
            stagger_breaks: Vec::new(),
        }
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_session_saved_without_skill_names_gets_them_when_read() {
        let dir = temp_dir("names");
        let mut session = sample(1_700_000_000_000, "Old File");
        session.skill_names.clear();
        let id = save(&dir, &session).unwrap();
        let expected = skill_names_with(&session.events, &session.heals);
        assert_eq!(load(&dir, &id).unwrap().skill_names, expected);
        let served: Session = serde_json::from_slice(&read(&dir, &id).unwrap()).unwrap();
        assert_eq!(served.skill_names, expected);
    }

    #[test]
    fn a_saved_session_loads_back_the_same() {
        let dir = temp_dir("roundtrip");
        let session = sample(1_700_000_000_000, "Fediv Wraith");
        let id = save(&dir, &session).unwrap();
        assert_eq!(id, "20231114-221320-fediv-wraith");
        assert_eq!(load(&dir, &id).unwrap(), session);
        assert_eq!(serde_json::from_slice::<Session>(&read(&dir, &id).unwrap()).unwrap(), session);

        let text = std::fs::read_to_string(dir.join(format!("{id}.json"))).unwrap();
        assert!(text.contains(r#""events":[[0,0,1000,1,100],[12000,0,2000,2,200]]"#), "{text}");
        assert!(text.contains(r#""schema_version":3"#));
        assert!(text.contains(r#""heals":[[500,0,400,0,300],[1500,0,500,2,300]]"#), "{text}");

        let listed = list(&dir);
        assert_eq!(listed, vec![Summary::of(id.clone(), &session)]);
        delete(&dir, &id).unwrap();
        assert_eq!(load(&dir, &id), Err(Error::NotFound));
        assert_eq!(delete(&dir, &id), Err(Error::NotFound));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_leaves_no_temp_file_and_never_overwrites() {
        let dir = temp_dir("atomic");
        let session = sample(1_700_000_000_000, "Boss");
        let first = save(&dir, &session).unwrap();
        let second = save(&dir, &session).unwrap();
        assert_ne!(first, second);
        assert_eq!(second, format!("{first}-2"));
        assert_eq!(files(&dir), [format!("{first}-2.json"), format!("{first}.json")]);
        assert!(!storage::temp_path(&dir.join(format!("{first}.json"))).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ids_cannot_leave_the_folder() {
        for good in ["20231114-221320-boss", "a", "A_b-9", &"x".repeat(MAX_ID_CHARS)] {
            assert!(valid_id(good), "{good}");
        }
        for bad in [
            "",
            "..",
            "../x",
            "..\\x",
            "a/b",
            "a\\b",
            "a.json",
            "C:x",
            "%2e%2e",
            "a b",
            "a\0",
            "cafÃ©",
            "nul",
            "COM1",
            &"x".repeat(MAX_ID_CHARS + 1),
        ] {
            assert!(!valid_id(bad), "{bad:?}");
        }

        let dir = temp_dir("traversal");
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::write(dir.join("secret.json"), "{}").unwrap();
        let inner = dir.join("inner");
        for id in ["../secret", "..\\secret", "..", "secret.json"] {
            assert_eq!(load(&inner, id), Err(Error::BadId));
            assert_eq!(read(&inner, id), Err(Error::BadId));
            assert_eq!(delete(&inner, id), Err(Error::BadId));
        }
        assert!(dir.join("secret.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_skips_files_that_are_not_sessions() {
        let dir = temp_dir("corrupt");
        let older = save(&dir, &sample(1_700_000_000_000, "Older")).unwrap();
        let newer = save(&dir, &sample(1_700_000_100_000, "Newer")).unwrap();
        std::fs::write(dir.join("20991231-000000-broken.json"), "{ not json").unwrap();
        std::fs::write(dir.join("20991231-000001-empty.json"), "").unwrap();
        let mut future = serde_json::to_value(sample(1_700_000_200_000, "Future")).unwrap();
        future["schema_version"] = (SCHEMA_VERSION + 1).into();
        std::fs::write(dir.join("20991231-000002-future.json"), future.to_string()).unwrap();
        std::fs::write(dir.join("not an id.json"), "{}").unwrap();
        std::fs::write(dir.join("notes.txt"), "hi").unwrap();

        let ids: Vec<String> = list(&dir).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, [newer, older]);
        assert_eq!(load(&dir, "20991231-000000-broken"), Err(Error::Damaged));
        assert_eq!(read(&dir, "20991231-000002-future"), Err(Error::Damaged));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_reads_at_most_the_newest_500_files() {
        let dir = temp_dir("limit");
        std::fs::create_dir_all(&dir).unwrap();
        let text = serde_json::to_string(&sample(0, "x")).unwrap();
        for n in 0..LIST_LIMIT + 3 {
            std::fs::write(dir.join(format!("{n:04}.json")), &text).unwrap();
        }
        let listed = list(&dir);
        assert_eq!(listed.len(), LIST_LIMIT);
        assert_eq!(listed[0].id, format!("{:04}", LIST_LIMIT + 2));
        assert!(listed.iter().all(|s| s.id.as_str() >= "0003"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_schema_2_file_without_death_fields_still_loads() {
        let dir = temp_dir("schema2");
        std::fs::create_dir_all(&dir).unwrap();
        let mut old = serde_json::to_value(sample(1_700_000_000_000, "Old")).unwrap();
        old["schema_version"] = 2.into();
        old["target"].as_object_mut().unwrap().remove("defeated");
        old["target"].as_object_mut().unwrap().remove("died_ms");
        std::fs::write(dir.join("20231114-221320-old.json"), old.to_string()).unwrap();
        let session = load(&dir, "20231114-221320-old").unwrap();
        assert_eq!((session.target.defeated, session.target.died_ms), (false, None));
        assert!(!list(&dir)[0].target.defeated);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_schema_1_file_without_healing_still_loads() {
        let dir = temp_dir("schema1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut old = serde_json::to_value(sample(1_700_000_000_000, "Old")).unwrap();
        old["schema_version"] = 1.into();
        old.as_object_mut().unwrap().remove("heals");
        old["totals"].as_object_mut().unwrap().remove("total_heal");
        old["totals"].as_object_mut().unwrap().remove("group_hps");
        for player in old["players"].as_array_mut().unwrap() {
            player.as_object_mut().unwrap().remove("heal");
            player.as_object_mut().unwrap().remove("hps");
        }
        std::fs::write(dir.join("20231114-221320-old.json"), old.to_string()).unwrap();
        let session = load(&dir, "20231114-221320-old").unwrap();
        assert_eq!(session.schema_version, 1);
        assert!(session.heals.is_empty());
        assert_eq!((session.totals.total_heal, session.players[0].heal), (0, 0));
        assert_eq!(list(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_lists_nothing() {
        assert!(list(&temp_dir("missing")).is_empty());
    }

    #[test]
    fn rename_sets_clears_and_caps_the_name() {
        let dir = temp_dir("rename");
        let session = sample(1_700_000_000_000, "Fediv Wraith");
        let id = save(&dir, &session).unwrap();

        let summary = rename(&dir, &id, "  My clear  ").unwrap();
        assert_eq!(summary.name.as_deref(), Some("My clear"));
        assert_eq!(summary.label(), "My clear");
        assert_eq!(load(&dir, &id).unwrap().name.as_deref(), Some("My clear"));
        assert_eq!(list(&dir)[0].label(), "My clear");

        let cleared = rename(&dir, &id, "   ").unwrap();
        assert_eq!(cleared.name, None);
        assert_eq!(cleared.label(), "Fediv Wraith");
        let loaded = load(&dir, &id).unwrap();
        assert_eq!(loaded.name, None);
        assert_eq!(loaded.label(), "Fediv Wraith");

        let long = "x".repeat(MAX_NAME_CHARS + 20);
        let capped = rename(&dir, &id, &long).unwrap();
        assert_eq!(capped.name.as_ref().unwrap().chars().count(), MAX_NAME_CHARS);
        assert_eq!(normalize_name("  a\u{0007}b  c  ").as_deref(), Some("ab c"));
        assert_eq!(normalize_name(""), None);
        assert_eq!(rename(&dir, "nope", "x"), Err(Error::NotFound));
        assert_eq!(rename(&dir, "../x", "x"), Err(Error::BadId));
        // File id is unchanged.
        assert_eq!(files(&dir), [format!("{id}.json")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_session_without_a_name_still_loads() {
        let dir = temp_dir("old-name");
        std::fs::create_dir_all(&dir).unwrap();
        let mut old = serde_json::to_value(sample(1_700_000_000_000, "Old Target")).unwrap();
        old.as_object_mut().unwrap().remove("name");
        std::fs::write(dir.join("20231114-221320-old-target.json"), old.to_string()).unwrap();
        let session = load(&dir, "20231114-221320-old-target").unwrap();
        assert_eq!(session.name, None);
        assert_eq!(session.label(), "Old Target");
        assert_eq!(list(&dir)[0].label(), "Old Target");
        let renamed = rename(&dir, "20231114-221320-old-target", "Custom").unwrap();
        assert_eq!(renamed.label(), "Custom");
        assert_eq!(load(&dir, "20231114-221320-old-target").unwrap().label(), "Custom");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_are_a_utc_stamp_and_a_safe_target() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(951_782_400_000), "20000229-000000");
        assert_eq!(stamp(1_700_000_000_999), "20231114-221320");
        assert_eq!(slug("Fediv Wraith"), "fediv-wraith");
        assert_eq!(slug("  ../..\\Boss: Lv.50!  "), "boss-lv-50");
        assert_eq!(slug("ê°€ê³ ì¼"), "session");
        assert_eq!(slug(""), "session");
        assert!(slug(&"ab ".repeat(100)).len() <= MAX_SLUG_CHARS);
        let id = format!("{}-{}", stamp(i64::MAX / 4), slug(&"x".repeat(500)));
        assert!(valid_id(&format!("{id}-999")), "{id}");
    }
}
