//! Monster type names: the user's own, and the English table bundled with
//! the app.
//!
//! A monster's type comes from its `41 36` spawn record and is the same for
//! every monster of that kind, so one name covers all of them. The user's
//! table is empty until the user names something. It is stored as a JSON
//! object of decimal type to name at `%LOCALAPPDATA%\aion2dps\monster-names.json`.
//! A user name wins over the bundled one for the same type.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;

use crate::storage;

/// A2Tools `src/data/i18n/npcs/en.json`, unmodified. See docs/third-party.md.
const BUNDLED_JSON: &str = include_str!("../data/npc-names-en.json");

pub const FILE_NAME: &str = "monster-names.json";
/// `--demo` keeps its names apart so sample targets never name real monsters.
pub const DEMO_FILE_NAME: &str = "monster-names-demo.json";
pub const MAX_NAME_CHARS: usize = 40;
/// Types are 24-bit.
pub const MAX_TYPE: u32 = 0xFF_FFFF;

pub type Names = BTreeMap<u32, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MonsterName {
    #[serde(rename = "type")]
    pub monster_type: u32,
    pub name: String,
}

pub fn list(names: &Names) -> Vec<MonsterName> {
    names
        .iter()
        .map(|(monster_type, name)| MonsterName {
            monster_type: *monster_type,
            name: name.clone(),
        })
        .collect()
}

/// One entry of the bundled table. `name` is `None` for the table's
/// placeholders (`???`, `None`), which are not names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundled {
    pub name: Option<String>,
    pub boss: bool,
    pub dummy: bool,
}

/// The bundled entry for a type. The table is parsed on first use.
pub fn bundled(monster_type: u32) -> Option<&'static Bundled> {
    static TABLE: OnceLock<HashMap<u32, Bundled>> = OnceLock::new();
    TABLE
        .get_or_init(|| parse_bundled(BUNDLED_JSON))
        .get(&monster_type)
}

/// `{"<type>": {"name": "...", "isBoss": bool, "isDummy": bool, ...}}`.
/// Other fields are dropped, and so are entries with neither a usable name
/// nor a flag.
fn parse_bundled(text: &str) -> HashMap<u32, Bundled> {
    let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text) else {
        tracing::warn!("the bundled monster names are not a JSON object");
        return HashMap::new();
    };
    map.into_iter()
        .filter_map(|(key, entry)| {
            let monster_type = key.trim().parse::<u32>().ok().filter(|t| valid_type(*t))?;
            let name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|name| usable_bundled_name(name))
                .map(str::to_string);
            let flag = |field: &str| entry.get(field).and_then(|v| v.as_bool()).unwrap_or(false);
            let bundled = Bundled {
                name,
                boss: flag("isBoss"),
                dummy: flag("isDummy"),
            };
            (bundled.name.is_some() || bundled.boss || bundled.dummy)
                .then_some((monster_type, bundled))
        })
        .collect()
}

fn usable_bundled_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(char::is_control)
        && !name.chars().all(|c| c.is_ascii_digit() || c == '?')
        && !name.eq_ignore_ascii_case("none")
}

/// `%LOCALAPPDATA%\aion2dps\monster-names.json`, or the temp directory when
/// that variable is missing.
pub fn default_path() -> PathBuf {
    storage::folder().join(FILE_NAME)
}

pub fn demo_path() -> PathBuf {
    storage::folder().join(DEMO_FILE_NAME)
}

pub fn valid_type(monster_type: u32) -> bool {
    (1..=MAX_TYPE).contains(&monster_type)
}

/// The name to store, or `None` to clear it. Trimmed, 1 to 40 characters,
/// no control characters.
pub fn clean_name(raw: &str) -> Result<Option<String>, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().any(char::is_control) {
        return Err("a name cannot contain control characters".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("a name is at most {MAX_NAME_CHARS} characters"));
    }
    Ok(Some(name.to_string()))
}

/// The saved table. A missing file is an empty table. An unreadable file is
/// copied to `<name>.bad` and treated as empty, so the next save does not
/// destroy it. Bad entries in a readable file are skipped.
pub fn load(path: &Path) -> Names {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            if err.kind() != io::ErrorKind::NotFound {
                tracing::warn!("could not read {}: {err}", path.display());
            }
            return Names::new();
        }
    };
    let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text) else {
        tracing::warn!("{} is not a JSON object; starting with no monster names", path.display());
        let _ = std::fs::copy(path, bad_copy(path));
        return Names::new();
    };
    map.into_iter()
        .filter_map(|(key, value)| {
            let monster_type = key.trim().parse::<u32>().ok().filter(|t| valid_type(*t))?;
            let name = clean_name(value.as_str()?).ok()??;
            Some((monster_type, name))
        })
        .collect()
}

/// Write the table next to `path` and rename it over the old file, so a crash
/// mid-write leaves either the old table or the new one.
pub fn save(path: &Path, names: &Names) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(names).map_err(io::Error::other)?;
    text.push('\n');
    storage::write_atomic(path, text.as_bytes())
}

fn bad_copy(path: &Path) -> PathBuf {
    storage::with_suffix(path, ".bad")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aion2dps-monster-names-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_bundled_table_has_names_and_flags() {
        let bag = bundled(2_000_000).expect("Punching Bag");
        assert_eq!(bag.name.as_deref(), Some("Punching Bag"));
        assert!(bag.dummy);
        assert!(!bag.boss);

        let seeker = bundled(2_100_343).expect("Enhanced Krall Seeker");
        assert_eq!(seeker.name.as_deref(), Some("Enhanced Krall Seeker"));
        assert!(!seeker.dummy && !seeker.boss);

        let boss = bundled(2_000_412).expect("a boss");
        assert!(boss.boss && !boss.dummy);
        let demo = bundled(crate::demo::DEMO_MONSTER_TYPE).expect("the demo target is bundled");
        assert!(demo.boss && demo.name.is_some());

        assert_eq!(bundled(2_099_073), None);
        assert_eq!(bundled(0), None);
    }

    #[test]
    fn bundled_placeholders_and_bad_entries_give_no_name() {
        let table = parse_bundled(
            r#"{
                "2100001": {"name": " Rock Golem ", "isBoss": true, "category": "x", "tier": "y"},
                "2100002": {"name": ""},
                "2100003": {"name": "123"},
                "2100004": {"name": "a\u0007b"},
                "2100005": {"isBoss": true},
                "2100006": "Plain",
                "2100007": {"name": "???", "isBoss": false},
                "2100008": {"name": "None", "isBoss": false, "isDummy": true},
                "abc": {"name": "Bad key"},
                "0": {"name": "Zero"},
                "16777216": {"name": "Too big"}
            }"#,
        );
        let mut types: Vec<u32> = table.keys().copied().collect();
        types.sort();
        assert_eq!(types, [2_100_001, 2_100_005, 2_100_008], "a flag is kept without a name");
        assert_eq!(
            table[&2_100_001],
            Bundled {
                name: Some("Rock Golem".into()),
                boss: true,
                dummy: false
            }
        );
        assert_eq!(table[&2_100_005].name, None);
        assert!(table[&2_100_005].boss);
        assert_eq!(table[&2_100_008].name, None);
        assert!(table[&2_100_008].dummy);
        assert!(parse_bundled("[1, 2]").is_empty());
    }

    #[test]
    fn the_bundled_table_never_names_a_type_with_a_placeholder() {
        let placeholder = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(BUNDLED_JSON)
            .unwrap()
            .into_iter()
            .find(|(_, entry)| entry["name"] == "???")
            .and_then(|(key, _)| key.parse::<u32>().ok())
            .expect("the source table has ??? entries");
        assert!(bundled(placeholder).map_or(true, |b| b.name.is_none()));
    }

    #[test]
    fn a_name_is_trimmed_and_an_empty_one_clears() {
        assert_eq!(clean_name("  Rock Golem \t"), Ok(Some("Rock Golem".into())));
        assert_eq!(clean_name(""), Ok(None));
        assert_eq!(clean_name("   "), Ok(None));
    }

    #[test]
    fn a_name_is_at_most_forty_characters() {
        assert!(clean_name(&"a".repeat(40)).is_ok());
        assert!(clean_name(&"a".repeat(41)).is_err());
        // Characters, not bytes.
        assert!(clean_name(&"가".repeat(40)).is_ok());
    }

    #[test]
    fn a_name_has_no_control_characters() {
        assert!(clean_name("Rock\nGolem").is_err());
        assert!(clean_name("Rock\u{7}Golem").is_err());
        assert!(clean_name("Rock\u{85}Golem").is_err());
    }

    #[test]
    fn a_type_is_24_bit_and_not_zero() {
        assert!(valid_type(0x200b81));
        assert!(valid_type(MAX_TYPE));
        assert!(!valid_type(0));
        assert!(!valid_type(MAX_TYPE + 1));
    }

    #[test]
    fn a_saved_table_loads_back_and_leaves_no_temp_file() {
        let dir = temp_dir("roundtrip");
        let path = dir.join(FILE_NAME);
        let mut names = Names::new();
        names.insert(0x200b81, "Rock Golem".into());
        names.insert(0x233fd6, "가고일".into());
        save(&path, &names).unwrap();
        assert_eq!(load(&path), names);
        assert!(!storage::temp_path(&path).exists());

        names.remove(&0x200b81);
        save(&path, &names).unwrap();
        assert_eq!(load(&path), names, "a save replaces the old file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_an_empty_table() {
        let dir = temp_dir("missing");
        assert!(load(&dir.join(FILE_NAME)).is_empty());
    }

    #[test]
    fn a_corrupt_file_is_kept_aside_and_read_as_empty() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load(&path).is_empty());
        assert_eq!(std::fs::read_to_string(bad_copy(&path)).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_entries_are_skipped() {
        let dir = temp_dir("entries");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(
            &path,
            r#"{"2100097":"Rock Golem","abc":"x","0":"zero","99999999":"big","2100098":7,"2100099":"  "}"#,
        )
        .unwrap();
        let names = load(&path);
        assert_eq!(names.len(), 1);
        assert_eq!(names[&2_100_097], "Rock Golem");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
