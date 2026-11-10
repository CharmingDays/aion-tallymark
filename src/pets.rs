//! Pet soul tracker.
//!
//! Reads three server packets the game already receives and never sends
//! anything back:
//! - `00 90` at login: the whole pet state. After the opcode come 4 bytes
//!   (unknown), a count, then `count` entries of 12 bytes: pet id (u32 LE),
//!   the same id again, level (u32 LE), one per summoned pet. Right after
//!   that, a second count and `count` pairs of pet id and souls (u32 LE
//!   each): souls toward the next step, for summoned and unsummoned pets.
//!   A pet in neither list has no souls. The bytes after that are another
//!   structure and are not read.
//! - `0d 90` when a soul drops: a count, then `count` pairs of pet id and
//!   souls gained (u32 LE each).
//! - `0e 90` when a pet is summoned or levels up: the same layout as `0d 90`,
//!   with the soul total that completed the step.
//!
//! Pet names and families come from the bundled `data/pets.json`. Pet
//! portraits are not bundled: [`fetch_icon`] downloads one from the web page
//! named in that file into the app's folder the first time it is needed.
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
    /// Portrait address with `{icon}` for a pet's `icon`.
    #[serde(default)]
    icon_url: String,
    pets: Vec<PetInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct PetInfo {
    id: u32,
    name: String,
    family: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    icon: Option<String>,
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

/// Default, lowest and highest seconds a pet toast stays after the last soul.
pub const DEFAULT_TOAST_SECS: u64 = 60;
pub const MIN_TOAST_SECS: u64 = 10;
pub const MAX_TOAST_SECS: u64 = 300;

/// One pet toast: the pet's progress right after a soul drop (`gained`) or a
/// summon / level up (`stepped`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PetToast {
    pub id: u32,
    pub name: String,
    pub family: String,
    pub level: u8,
    pub max_level: u8,
    pub souls: u32,
    pub needed: Option<u32>,
    pub gained: u32,
    pub stepped: bool,
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
    /// `00 90`: summoned pets with their level, and souls toward the next
    /// step (`None` when the souls section did not parse).
    List {
        levels: Vec<(u32, u8)>,
        souls: Option<Vec<(u32, u32)>>,
    },
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
        [0x00, 0x90] => parse_list(rest).map(|(levels, souls)| PetPacket::List { levels, souls }),
        [0x0D, 0x90] => parse_pairs(rest).map(PetPacket::Souls),
        [0x0E, 0x90] => parse_pairs(rest).map(PetPacket::Stepped),
        _ => None,
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

type Lists = (Vec<(u32, u8)>, Option<Vec<(u32, u32)>>);

fn parse_list(rest: &[u8]) -> Option<Lists> {
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
    let souls = parse_souls(rest, at + count * 12);
    Some((out, souls))
}

/// The souls section right after the level list: a count, then pairs.
fn parse_souls(rest: &[u8], mut at: usize) -> Option<Vec<(u32, u32)>> {
    let count = usize::try_from(try_read_varint(rest, &mut at)?).ok()?;
    if count > 1000 || rest.len() < at + count * 8 {
        return None;
    }
    (0..count)
        .map(|i| {
            let p = at + i * 8;
            let (id, souls) = (u32_at(rest, p)?, u32_at(rest, p + 4)?);
            (plausible_id(id) && souls <= 10_000).then_some((id, souls))
        })
        .collect()
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
    /// False when nothing is known yet: no login list and no packet about it.
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

    /// Apply a packet. Returns one toast per soul drop or step.
    pub fn apply(&mut self, packet: PetPacket, now: i64) -> Vec<PetToast> {
        let mut toasts = Vec::new();
        match packet {
            PetPacket::List { levels, souls } => {
                // The login list is the whole state of the logged-in
                // character, so it replaces what was saved (also another
                // character's progress).
                let before = std::mem::take(&mut self.saved.pets);
                let mut next: BTreeMap<u32, Progress> = BTreeMap::new();
                for (id, level) in levels {
                    next.entry(id).or_default().level = level.min(MAX_LEVEL);
                }
                match souls {
                    Some(souls) => {
                        for (id, count) in souls {
                            next.entry(id).or_default().souls = count;
                        }
                    }
                    // Without the souls section keep the saved souls of pets
                    // whose level did not change.
                    None => {
                        for (id, old) in &before {
                            let level = next.get(id).map_or(0, |p| p.level);
                            if old.level == level && old.souls > 0 {
                                next.entry(*id).or_default().souls = old.souls;
                            }
                        }
                    }
                }
                for (id, p) in next.iter_mut() {
                    p.updated_ms = match before.get(id) {
                        Some(old) if old.level == p.level && old.souls == p.souls => old.updated_ms,
                        _ => now,
                    };
                }
                self.saved.pets = next;
                self.saved.list_ms = Some(now);
            }
            PetPacket::Souls(drops) => {
                for (id, gained) in drops {
                    let p = self.saved.pets.entry(id).or_default();
                    p.souls = p.souls.saturating_add(gained);
                    p.updated_ms = now;
                    toasts.push((id, gained, false));
                }
            }
            PetPacket::Stepped(steps) => {
                for (id, _) in steps {
                    let p = self.saved.pets.entry(id).or_default();
                    p.level = (p.level + 1).min(MAX_LEVEL);
                    p.souls = 0;
                    p.updated_ms = now;
                    toasts.push((id, 0, true));
                }
            }
        }
        self.version += 1;
        self.save();
        toasts
            .into_iter()
            .map(|(id, gained, stepped)| {
                let row = self.row(id, info(id));
                PetToast {
                    id,
                    name: row.name,
                    family: row.family,
                    level: row.level,
                    max_level: MAX_LEVEL,
                    souls: row.souls,
                    needed: row.needed,
                    gained,
                    stepped,
                }
            })
            .collect()
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
            seen: p.is_some() || self.saved.list_ms.is_some(),
            updated_ms: p.map(|p| p.updated_ms),
        }
    }
}

/// A portrait name is letters, digits and `_` only, so it cannot leave the
/// icon folder or change the address.
fn icon_name(id: u32) -> Option<&'static str> {
    let icon = info(id)?.icon.as_deref()?;
    (!icon.is_empty() && icon.len() <= 80 && icon.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then_some(icon)
}

pub fn icon_dir() -> PathBuf {
    crate::storage::folder().join("pet-icons")
}

fn icon_path(id: u32) -> Option<PathBuf> {
    Some(icon_dir().join(format!("{}.webp", icon_name(id)?)))
}

/// The cached portrait of a pet as a `data:` URL, or `None` until fetched.
pub fn cached_icon(id: u32) -> Option<String> {
    let bytes = std::fs::read(icon_path(id)?).ok()?;
    is_webp(&bytes).then(|| format!("data:image/webp;base64,{}", base64(&bytes)))
}

fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

/// Download a pet's portrait into [`icon_dir`]. Blocking (up to about 10 s);
/// run it off the capture and UI threads. A web request to the data site
/// only, never anything to the game. True when the icon is cached now.
pub fn fetch_icon(id: u32) -> bool {
    let (Some(name), Some(path)) = (icon_name(id), icon_path(id)) else {
        return false;
    };
    if path.exists() {
        return true;
    }
    let url = data().icon_url.replace("{icon}", name);
    if !url.starts_with("https://") {
        return false;
    }
    if std::fs::create_dir_all(icon_dir()).is_err() {
        return false;
    }
    let temp = crate::storage::temp_path(&path);
    let mut curl = std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    curl.args(["-sSfL", "--max-time", "10", "--max-filesize", "2000000", "-o"])
        .arg(&temp)
        .arg(&url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        curl.creation_flags(CREATE_NO_WINDOW);
    }
    let ok = curl.status().is_ok_and(|s| s.success())
        && std::fs::read(&temp).is_ok_and(|b| is_webp(&b))
        && std::fs::rename(&temp, &path).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&temp);
    }
    ok
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
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

    /// The Config switch opens it like `AION2DPS_DEBUG_PETS=1`.
    pub fn open_default() -> Option<Self> {
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
        let toasts = pets.apply(parse(&summoned).unwrap(), 2);
        assert_eq!(pets.progress(1104).unwrap(), &Progress { level: 1, souls: 0, updated_ms: 2 });
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].stepped);
        assert_eq!((toasts[0].level, toasts[0].needed), (1, Some(25)));
        let row = pets.view().pets.into_iter().find(|r| r.id == 1104).unwrap();
        assert_eq!(row.name, "Manduri Trebuchet Worker");
        assert_eq!(row.family, "Cogni");
        assert_eq!(row.needed, Some(25));
    }

    #[test]
    fn captured_login_list_sets_levels_and_souls() {
        // A real 00 90 from a relog: 91 summoned pets, then 100 soul counts,
        // then the first bytes of the next structure.
        let hex = include_str!("../tests/data/pet-list-0728.hex").trim();
        let PetPacket::List { levels, souls } = parse(&frame(hex)).expect("login list") else {
            panic!("not a list");
        };
        assert_eq!(levels.len(), 91);
        let at = |level: u8| levels.iter().filter(|(_, l)| *l == level).count();
        assert_eq!((at(1), at(2), at(3)), (82, 7, 2));
        let souls = souls.expect("souls section");
        assert_eq!(souls.len(), 100);
        assert_eq!(souls.iter().filter(|(id, _)| !levels.iter().any(|(l, _)| l == id)).count(), 24);
        assert_eq!(souls.iter().find(|(id, _)| *id == 1006).map(|s| s.1), Some(12));

        let mut pets = Pets::load(None);
        pets.apply(PetPacket::Souls(vec![(1999, 3)]), 1);
        pets.apply(PetPacket::List { levels, souls: Some(souls) }, 2);
        assert!(pets.progress(1999).is_none(), "the login list replaces saved progress");
        let p1006 = pets.progress(1006).unwrap().clone();
        assert_eq!((p1006.level, p1006.souls), (1, 12));
        // Soul drops add, summoned or not (1006 got 1, 1, 2, 2, 1).
        for gained in [1, 1, 2, 2, 1] {
            let toast = pets.apply(PetPacket::Souls(vec![(1006, gained)]), 3).remove(0);
            assert_eq!((toast.name.as_str(), toast.gained, toast.stepped), ("Sparkie", gained, false));
        }
        assert_eq!(pets.progress(1006).unwrap().souls, 19);
        let view = pets.view();
        assert_eq!(view.summoned, 91);
        assert_eq!(view.list_ms, Some(2));
        assert!(view.pets.iter().all(|r| r.seen), "known once a list arrived");
        assert_eq!(souls_needed(2), Some(75));
        assert_eq!(souls_needed(MAX_LEVEL), None);
    }

    #[test]
    fn login_list_without_souls_section_keeps_levels() {
        // 1002 lv1, 1003 lv1, 1011 lv2 and a count that runs past the end.
        let list = frame("00902a04000003ea030000ea03000001000000eb030000eb03000001000000f3030000f30300000200000005");
        assert_eq!(
            parse(&list),
            Some(PetPacket::List { levels: vec![(1002, 1), (1003, 1), (1011, 2)], souls: None })
        );
        assert_eq!(parse(&frame("00902a04000003ea030000eb03000001000000")), None);
        let mut pets = Pets::load(None);
        pets.apply(PetPacket::Souls(vec![(1004, 1)]), 1);
        pets.apply(parse(&list).unwrap(), 2);
        assert_eq!(pets.view().summoned, 3);
        assert_eq!(pets.progress(1004).unwrap().souls, 1, "kept when souls are missing");
    }

    #[test]
    fn base64_and_icon_names() {
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(icon_name(1104), Some("UT_Vehicle_Portrait_ManduriRng_01"));
        assert!(data().icon_url.starts_with("https://") && data().icon_url.contains("{icon}"));
    }

    #[test]
    fn bundled_data_is_sorted_and_named() {
        let pets = &data().pets;
        assert!(pets.len() > 200);
        assert!(pets.windows(2).all(|w| w[0].id < w[1].id));
        assert!(pets.iter().all(|p| !p.name.is_empty() && !p.family.is_empty()));
    }
}
