//! Pet soul tracker.
//!
//! Reads three server packets the game already receives and never sends
//! anything back:
//! - `00 90` at login: the summoned pets and their levels. After the opcode
//!   come 4 bytes (unknown), a count, then `count` entries of 12 bytes:
//!   pet id (u32 LE), the same id again, level (u32 LE). Bytes after the list
//!   are not read yet.
//! - `0d 90` when a soul drops: a count, then `count` pairs of pet id and
//!   value (u32 LE each). For a pet that is not summoned the value is +1 per
//!   soul; for a summoned pet it reads as the souls toward the next level.
//! - `0e 90` when a pet is summoned or levels up: the same layout as `0d 90`,
//!   with the soul total that completed the step.
//!
//! Pet names and families come from the bundled `data/pets.json`.
use crate::varint::try_read_varint;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub const FILE_NAME: &str = "pets.json";
/// Highest pet level the game has.
pub const MAX_LEVEL: u8 = 3;

#[derive(Debug, Deserialize)]
struct PetData {
    summon: u32,
    level_up: Vec<u32>,
    pets: Vec<PetInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct PetInfo {
    id: u32,
    name: String,
    family: String,
    #[serde(default)]
    source: Option<String>,
}

fn data() -> &'static PetData {
    static DATA: OnceLock<PetData> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../data/pets.json")).expect("pet data")
    })
}

fn info(id: u32) -> Option<&'static PetInfo> {
    let pets = &data().pets;
    pets.binary_search_by_key(&id, |p| p.id).ok().map(|i| &pets[i])
}

/// Souls for the next step from `level` (0 = summon), or `None` at the top.
pub fn souls_needed(level: u8) -> Option<u32> {
    let data = data();
    match level {
        0 => Some(data.summon),
        l if l < MAX_LEVEL => data.level_up.get(usize::from(l) - 1).copied(),
        _ => None,
    }
}

/// Pet ids in the game are four digits.
fn plausible_id(id: u32) -> bool {
    (1000..2000).contains(&id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PetPacket {
    /// `00 90`: summoned pets with their level.
    List(Vec<(u32, u8)>),
    /// `0d 90`: soul drops.
    Souls(Vec<(u32, u32)>),
    /// `0e 90`: a pet summoned or levelled.
    Stepped(Vec<(u32, u32)>),
}

/// The opcode of a framed packet when it is one of the pet opcodes.
pub fn pet_opcode(packet: &[u8]) -> Option<[u8; 2]> {
    let mut at = 0;
    try_read_varint(packet, &mut at)?;
    match packet.get(at..at + 2)? {
        [a @ (0x00 | 0x0D | 0x0E), 0x90] => Some([*a, 0x90]),
        _ => None,
    }
}

/// A framed packet (length varint first). Anything that does not match the
/// layout exactly is ignored.
pub fn parse(packet: &[u8]) -> Option<PetPacket> {
    let mut at = 0;
    try_read_varint(packet, &mut at)?;
    let op = packet.get(at..at + 2)?;
    let rest = &packet[at + 2..];
    match op {
        [0x00, 0x90] => parse_list(rest).map(PetPacket::List),
        [0x0D, 0x90] => parse_pairs(rest).map(PetPacket::Souls),
        [0x0E, 0x90] => parse_pairs(rest).map(PetPacket::Stepped),
        _ => None,
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn parse_list(rest: &[u8]) -> Option<Vec<(u32, u8)>> {
    let mut at = 4;
    let count = usize::try_from(try_read_varint(rest, &mut at)?).ok()?;
    if count == 0 || count > 1000 || rest.len() < at + count * 12 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let p = at + i * 12;
        let (id, again, level) = (u32_at(rest, p)?, u32_at(rest, p + 4)?, u32_at(rest, p + 8)?);
        if id != again || !plausible_id(id) || !(1..=10).contains(&level) {
            return None;
        }
        out.push((id, level as u8));
    }
    Some(out)
}

fn parse_pairs(rest: &[u8]) -> Option<Vec<(u32, u32)>> {
    let (&count, tail) = rest.split_first()?;
    let count = usize::from(count);
    if count == 0 || count > 32 || tail.len() != count * 8 {
        return None;
    }
    tail.chunks_exact(8)
        .map(|c| {
            let (id, value) = (u32_at(c, 0)?, u32_at(c, 4)?);
            (plausible_id(id) && value <= 10_000).then_some((id, value))
        })
        .collect()
}

/// What the tracker knows about one pet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// 0 = not summoned.
    pub level: u8,
    /// Souls toward the next step.
    pub souls: u32,
    pub updated_ms: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    /// When the login pet list last arrived.
    #[serde(default)]
    list_ms: Option<i64>,
    #[serde(default)]
    pets: BTreeMap<u32, Progress>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PetRow {
    pub id: u32,
    pub name: String,
    pub family: String,
    pub source: Option<String>,
    pub level: u8,
    pub souls: u32,
    /// Souls for the next step; `None` at the top level.
    pub needed: Option<u32>,
    /// False when no packet has mentioned this pet yet.
    pub seen: bool,
    pub updated_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PetsView {
    pub version: u64,
    pub list_ms: Option<i64>,
    pub max_level: u8,
    pub summoned: usize,
    pub total: usize,
    pub pets: Vec<PetRow>,
}

#[derive(Debug, Default)]
pub struct Pets {
    saved: Saved,
    path: Option<PathBuf>,
    version: u64,
}

pub fn default_path() -> PathBuf {
    crate::storage::folder().join(FILE_NAME)
}

impl Pets {
    /// Load the last known state from `path` and save changes back to it.
    pub fn load(path: Option<PathBuf>) -> Self {
        let saved = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self { saved, path, version: 0 }
    }

    /// Bumps on every change, so pages refetch only then.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn progress(&self, id: u32) -> Option<&Progress> {
        self.saved.pets.get(&id)
    }

    pub fn apply(&mut self, packet: PetPacket, now: i64) {
        match packet {
            PetPacket::List(list) => {
                // The login list is the whole set of summoned pets.
                for (id, p) in self.saved.pets.iter_mut() {
                    if p.level > 0 && !list.iter().any(|(seen, _)| seen == id) {
                        p.level = 0;
                        p.updated_ms = now;
                    }
                }
                for (id, level) in list {
                    let p = self.saved.pets.entry(id).or_default();
                    if p.level != level {
                        if level > p.level {
                            p.souls = 0;
                        }
                        p.level = level.min(MAX_LEVEL);
                        p.updated_ms = now;
                    }
                }
                self.saved.list_ms = Some(now);
            }
            PetPacket::Souls(drops) => {
                for (id, value) in drops {
                    let p = self.saved.pets.entry(id).or_default();
                    // Unverified for summoned pets: the value read as a running
                    // count there (1, 2) and as +1 per soul before summoning.
                    p.souls = if p.level == 0 { p.souls.saturating_add(value) } else { value };
                    p.updated_ms = now;
                }
            }
            PetPacket::Stepped(steps) => {
                for (id, _) in steps {
                    let p = self.saved.pets.entry(id).or_default();
                    p.level = (p.level + 1).min(MAX_LEVEL);
                    p.souls = 0;
                    p.updated_ms = now;
                }
            }
        }
        self.version += 1;
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Ok(text) = serde_json::to_vec(&self.saved) {
            if let Err(err) = crate::storage::write_atomic(path, &text) {
                tracing::warn!("could not save pets to {}: {err}", path.display());
            }
        }
    }

    pub fn view(&self) -> PetsView {
        let mut rows: Vec<PetRow> = data()
            .pets
            .iter()
            .map(|info| self.row(info.id, Some(info)))
            .collect();
        // Pets the game sent that the bundled list does not have yet.
        for id in self.saved.pets.keys() {
            if info(*id).is_none() {
                rows.push(self.row(*id, None));
            }
        }
        PetsView {
            version: self.version,
            list_ms: self.saved.list_ms,
            max_level: MAX_LEVEL,
            summoned: rows.iter().filter(|r| r.level > 0).count(),
            total: rows.len(),
            pets: rows,
        }
    }

    fn row(&self, id: u32, info: Option<&PetInfo>) -> PetRow {
        let p = self.saved.pets.get(&id);
        let level = p.map_or(0, |p| p.level);
        PetRow {
            id,
            name: info.map_or_else(|| format!("Pet #{id}"), |i| i.name.clone()),
            family: info.map_or_else(|| "Unknown".into(), |i| i.family.clone()),
            source: info.and_then(|i| i.source.clone()),
            level,
            souls: p.map_or(0, |p| p.souls),
            needed: souls_needed(level),
            seen: p.is_some(),
            updated_ms: p.map(|p| p.updated_ms),
        }
    }
}

/// Opt-in (`AION2DPS_DEBUG_PETS=1`), bounded local log of the three pet
/// opcodes only, with the whole packet so the `00 90` tail can be read.
pub struct PetDebug {
    file: File,
    written: u64,
    rows: u32,
}

const DEBUG_CAP: u64 = 4 * 1024 * 1024;
const DEBUG_ROWS: u32 = 5_000;
/// Longest packet logged in full; the login list was about 3 KB.
const DEBUG_PACKET: usize = 16 * 1024;

pub fn debug_path() -> PathBuf {
    crate::storage::folder().join("pet-debug.jsonl")
}

impl PetDebug {
    pub fn from_env() -> Option<Self> {
        if std::env::var("AION2DPS_DEBUG_PETS").ok().as_deref() != Some("1") {
            return None;
        }
        Self::open(&debug_path())
    }

    fn open(path: &Path) -> Option<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok()?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path).ok()?;
        let written = file.metadata().ok()?.len();
        tracing::info!("pet debug log: {}", path.display());
        Some(Self { file, written, rows: 0 })
    }

    pub fn observe(&mut self, packet: &[u8], now: i64) {
        if self.written >= DEBUG_CAP || self.rows >= DEBUG_ROWS {
            return;
        }
        let Some(op) = pet_opcode(packet) else {
            return;
        };
        let mut at = 0;
        let _ = try_read_varint(packet, &mut at);
        let shown = &packet[at..packet.len().min(at + DEBUG_PACKET)];
        let hex: String = shown.iter().map(|b| format!("{b:02x}")).collect();
        let parsed = parse(packet).map(|p| format!("{p:?}"));
        let line = json!({"ts_ms": now, "opcode": format!("{:02x}{:02x}", op[0], op[1]),
            "len": packet.len(), "truncated": packet.len() - at > shown.len(),
            "parsed": parsed, "opcode_hex": hex})
        .to_string();
        if self.written + line.len() as u64 + 1 > DEBUG_CAP {
            return;
        }
        if writeln!(self.file, "{line}").is_ok() {
            self.written += line.len() as u64 + 1;
            self.rows += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(hex: &str) -> Vec<u8> {
        let body: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        crate::framing::encode_frame(&body)
    }

    #[test]
    fn soul_drop_and_summon_from_a_capture() {
        // Manduri Trebuchet Worker (1104): five drops, then summoned at 5.
        assert_eq!(parse(&frame("0d900150040000010000")), None, "short pair");
        let drop = frame("0d90015004000001000000");
        assert_eq!(parse(&drop), Some(PetPacket::Souls(vec![(1104, 1)])));
        let summoned = frame("0e90015004000005000000");
        assert_eq!(parse(&summoned), Some(PetPacket::Stepped(vec![(1104, 5)])));
        // Client-side bytes of the same length do not parse.
        assert_eq!(parse(&frame("0d908a6ce63f588f8f71")), None);

        let mut pets = Pets::load(None);
        for _ in 0..4 {
            pets.apply(parse(&drop).unwrap(), 1);
        }
        assert_eq!(pets.progress(1104).unwrap().souls, 4);
        pets.apply(parse(&summoned).unwrap(), 2);
        assert_eq!(pets.progress(1104).unwrap(), &Progress { level: 1, souls: 0, updated_ms: 2 });
        let row = pets.view().pets.into_iter().find(|r| r.id == 1104).unwrap();
        assert_eq!(row.name, "Manduri Trebuchet Worker");
        assert_eq!(row.family, "Cogni");
        assert_eq!(row.needed, Some(25));
    }

    #[test]
    fn login_list_sets_levels() {
        // Start of a real 00 90 list: 1002 lv1, 1003 lv1, 1011 lv2.
        let list = frame("00902a04000003ea030000ea03000001000000eb030000eb03000001000000f3030000f303000002000000ffff");
        assert_eq!(parse(&list), Some(PetPacket::List(vec![(1002, 1), (1003, 1), (1011, 2)])));
        assert_eq!(parse(&frame("00902a04000003ea030000eb03000001000000")), None);
        let mut pets = Pets::load(None);
        pets.apply(PetPacket::Souls(vec![(1004, 1)]), 1);
        pets.apply(parse(&list).unwrap(), 2);
        let view = pets.view();
        assert_eq!(view.summoned, 3);
        assert_eq!(view.list_ms, Some(2));
        assert_eq!(pets.progress(1004).unwrap().souls, 1, "unsummoned souls stay");
        assert_eq!(souls_needed(2), Some(75));
        assert_eq!(souls_needed(MAX_LEVEL), None);
    }

    #[test]
    fn bundled_data_is_sorted_and_named() {
        let pets = &data().pets;
        assert!(pets.len() > 200);
        assert!(pets.windows(2).all(|w| w[0].id < w[1].id));
        assert!(pets.iter().all(|p| !p.name.is_empty() && !p.family.is_empty()));
    }
}
