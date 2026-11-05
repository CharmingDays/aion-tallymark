//! English skill names. The table is the one published with A2Tools DPS Meter
//! (GPL-3.0-or-later, Copyright (C) taengu and contributors).

use std::collections::HashMap;
use std::sync::OnceLock;

static SKILLS: OnceLock<HashMap<i32, String>> = OnceLock::new();

fn table() -> &'static HashMap<i32, String> {
    SKILLS.get_or_init(|| {
        let raw: HashMap<String, String> =
            serde_json::from_str(include_str!("../data/skills_en.json")).unwrap_or_default();
        raw.into_iter()
            .filter_map(|(id, name)| id.parse().ok().map(|id| (id, name)))
            .collect()
    })
}

/// The id this hit should be shown under. A live record carries a variant such
/// as 12090243; the table names the family, 12090000, "Punishment".
pub fn canonical_skill_id(raw: i32) -> i32 {
    if (30_000_000..=30_999_999).contains(&raw) {
        return raw;
    }
    let base = raw - raw.rem_euclid(10_000);
    let Some(base_name) = table().get(&base) else {
        return raw;
    };
    match table().get(&raw) {
        Some(raw_name) if raw_name != base_name => raw,
        _ => base,
    }
}

/// The skill's name, or the id when this table has no entry for it or its family.
pub fn skill_name(id: i32) -> String {
    let id = canonical_skill_id(id);
    table()
        .get(&id)
        .cloned()
        .unwrap_or_else(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_skill_uses_its_name() {
        assert_eq!(skill_name(12_090_000), "Punishment");
        assert_eq!(skill_name(14_020_000), "Snipe");
        assert_eq!(skill_name(11_050_000), "Crushing Wave");
    }

    #[test]
    fn an_unknown_skill_stays_an_id() {
        assert_eq!(skill_name(99_999_999), "99999999");
    }

    #[test]
    fn a_live_variant_uses_its_family_name() {
        assert_eq!(canonical_skill_id(12_090_243), 12_090_000);
        assert_eq!(skill_name(12_090_243), "Punishment");
        assert_eq!(skill_name(14_020_111), "Snipe");
        assert_eq!(canonical_skill_id(1_609_010), 1_609_010);
        assert_eq!(skill_name(1_609_010), "Commence Hunt");
    }
}
