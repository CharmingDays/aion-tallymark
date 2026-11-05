//! Opt-in capture of raw packets around unnamed targets, so monster name,
//! type, and grade fields can be found by fighting a known target.
//!
//! Off unless `--debug-targets` or `AION2DPS_DEBUG_TARGETS=1` is given. When
//! off, the capture worker holds `None` and nothing here runs. When on, it
//! keeps a bounded ring of recent packets and appends JSON lines to
//! `%LOCALAPPDATA%\aion2dps\target-debug.jsonl`. It only sees the packets the
//! meter already captured; it never touches the game process.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use crate::decode::{Event, Hit};
use crate::varint::{encode_varint, read_varint};

pub const ENV_VAR: &str = "AION2DPS_DEBUG_TARGETS";
pub const FLAG: &str = "--debug-targets";
const FILE_NAME: &str = "target-debug.jsonl";
const OLD_FILE_NAME: &str = "target-debug.old.jsonl";

const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const RING_PACKETS: usize = 2_000;
const RING_BYTES: usize = 8 * 1024 * 1024;
const CONTEXT_PACKETS: usize = 3;
/// Hex dumps longer than this are cut; the full length is still recorded.
const MAX_HEX_BYTES: usize = 4_096;
const HP_RECORDS_PER_TARGET: u32 = 20;
const MAX_TRACKED_TARGETS: usize = 50_000;

/// What the pages show about the debug log.
#[derive(Debug, Clone, Serialize)]
pub struct DebugInfo {
    pub path: String,
    pub records: u64,
    pub bytes: u64,
    pub full: bool,
    pub error: Option<String>,
}

/// The flag or the environment variable asks for the debug log.
pub fn requested(flag: bool) -> bool {
    flag || std::env::var(ENV_VAR).is_ok_and(|value| env_on(&value))
}

fn env_on(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// `%LOCALAPPDATA%\aion2dps\target-debug.jsonl`, or the temp directory when
/// that variable is missing.
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("aion2dps").join(FILE_NAME)
}

struct RingPacket {
    at_ms: i64,
    damage: bool,
    bytes: Vec<u8>,
}

pub struct TargetDebug {
    path: PathBuf,
    file: File,
    bytes: u64,
    records: u64,
    full: bool,
    error: Option<String>,
    ring: VecDeque<RingPacket>,
    ring_bytes: usize,
    /// Targets whose surrounding packets were already written.
    context_logged: HashSet<i32>,
    hp_logged: HashMap<i32, u32>,
}

impl TargetDebug {
    /// Open (append) the log at `path`, creating its folder. A log already at
    /// the size cap is moved to `target-debug.old.jsonl` first.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::metadata(&path).is_ok_and(|meta| meta.len() >= MAX_FILE_BYTES) {
            let old = path.with_file_name(OLD_FILE_NAME);
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&path, &old)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let bytes = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let mut debug = Self {
            path,
            file,
            bytes,
            records: 0,
            full: false,
            error: None,
            ring: VecDeque::new(),
            ring_bytes: 0,
            context_logged: HashSet::new(),
            hp_logged: HashMap::new(),
        };
        debug.write(json!({
            "type": "session",
            "ts_ms": crate::demo::now_ms(),
            "version": env!("CARGO_PKG_VERSION"),
            "note": "hit = damage on a target with no known name; context = recent non-damage packets containing the target id as a varint, plus monster_type from the target's 41 36 spawn (null if not seen); hp = 1B 92 HP record for a logged target",
        }));
        Ok(debug)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn info(&self) -> DebugInfo {
        DebugInfo {
            path: self.path.display().to_string(),
            records: self.records,
            bytes: self.bytes,
            full: self.full,
            error: self.error.clone(),
        }
    }

    /// Look at one decoded packet. `knows_name` says whether the meter has a
    /// name for an entity, and `monster_type` gives its spawn type, both after
    /// this packet's own events were applied.
    pub fn observe(
        &mut self,
        packet: &[u8],
        events: &[Event],
        knows_name: impl Fn(i32) -> bool,
        monster_type: impl Fn(i32) -> Option<u32>,
        now_ms: i64,
    ) {
        let mut damage = is_damage_opcode(packet);
        for event in events {
            match event {
                Event::Hit(hit) => {
                    damage = true;
                    if !hit.taken && hit.total() > 0 && !knows_name(hit.target) {
                        self.log_hit(hit, monster_type(hit.target), packet, now_ms);
                    }
                }
                Event::Hp { id, current, max } => {
                    self.log_hp(*id, *current, *max, packet, now_ms, &knows_name);
                }
                _ => {}
            }
        }
        self.remember(packet, damage, now_ms);
    }

    fn log_hit(&mut self, hit: &Hit, monster_type: Option<u32>, packet: &[u8], now_ms: i64) {
        if self.full {
            return;
        }
        let (hex, truncated) = hex_of(packet);
        self.write(json!({
            "type": "hit",
            "ts_ms": now_ms,
            "target": hit.target,
            "actor": hit.actor,
            "skill": hit.skill,
            "amount": hit.total(),
            "dot": hit.dot,
            "opcode": opcode_hex(packet),
            "len": packet.len(),
            "hex": hex,
            "truncated": truncated,
        }));
        if self.context_logged.contains(&hit.target) {
            return;
        }
        if self.context_logged.len() >= MAX_TRACKED_TARGETS {
            self.context_logged.clear();
            self.hp_logged.clear();
        }
        self.context_logged.insert(hit.target);
        let packets = self.context_for(hit.target, now_ms);
        self.write(json!({
            "type": "context",
            "ts_ms": now_ms,
            "target": hit.target,
            "monster_type": monster_type,
            "varint": hex_of(&encode_varint(hit.target as u32)).0,
            "ring_packets": self.ring.len(),
            "packets": packets,
        }));
    }

    /// Up to [`CONTEXT_PACKETS`] ring packets carrying the target id, one per
    /// opcode, newest first. When more opcodes match, the oldest match takes
    /// the last slot: it is the likeliest spawn record.
    fn context_for(&self, target: i32, now_ms: i64) -> Vec<Value> {
        let needle = encode_varint(target as u32);
        let mut picked: Vec<(&RingPacket, Vec<usize>)> = Vec::new();
        let mut opcodes: HashSet<Option<[u8; 2]>> = HashSet::new();
        let mut oldest: Option<(&RingPacket, Vec<usize>)> = None;
        for entry in self.ring.iter().rev().filter(|entry| !entry.damage) {
            let offsets = find_all(&entry.bytes, &needle);
            if offsets.is_empty() {
                continue;
            }
            let op = opcode(&entry.bytes);
            if opcodes.insert(op) {
                if picked.len() < CONTEXT_PACKETS {
                    picked.push((entry, offsets));
                } else {
                    oldest = Some((entry, offsets));
                }
            }
        }
        if let Some(spawn) = oldest {
            picked.truncate(CONTEXT_PACKETS - 1);
            picked.push(spawn);
        }
        picked
            .into_iter()
            .map(|(entry, offsets)| {
                let (hex, truncated) = hex_of(&entry.bytes);
                json!({
                    "age_ms": now_ms.saturating_sub(entry.at_ms),
                    "opcode": opcode_hex(&entry.bytes),
                    "len": entry.bytes.len(),
                    "id_offsets": offsets,
                    "hex": hex,
                    "truncated": truncated,
                })
            })
            .collect()
    }

    fn log_hp(
        &mut self,
        id: i32,
        current: i32,
        max: i32,
        packet: &[u8],
        now_ms: i64,
        knows_name: &impl Fn(i32) -> bool,
    ) {
        if self.full || !self.context_logged.contains(&id) || knows_name(id) {
            return;
        }
        let count = self.hp_logged.entry(id).or_insert(0);
        if *count >= HP_RECORDS_PER_TARGET {
            return;
        }
        *count += 1;
        let (hex, truncated) = hex_of(packet);
        self.write(json!({
            "type": "hp",
            "ts_ms": now_ms,
            "target": id,
            "current": current,
            "max": max,
            "opcode": opcode_hex(packet),
            "len": packet.len(),
            "hex": hex,
            "truncated": truncated,
        }));
    }

    fn remember(&mut self, packet: &[u8], damage: bool, now_ms: i64) {
        self.ring_bytes += packet.len();
        self.ring.push_back(RingPacket {
            at_ms: now_ms,
            damage,
            bytes: packet.to_vec(),
        });
        while self.ring.len() > RING_PACKETS || self.ring_bytes > RING_BYTES {
            let Some(old) = self.ring.pop_front() else {
                break;
            };
            self.ring_bytes -= old.bytes.len();
        }
    }

    fn write(&mut self, record: Value) {
        if self.full {
            return;
        }
        let mut line = record.to_string();
        line.push('\n');
        if self.bytes + line.len() as u64 > MAX_FILE_BYTES {
            self.full = true;
            let note = format!(
                "{}\n",
                json!({"type": "cap", "ts_ms": crate::demo::now_ms(), "bytes": self.bytes})
            );
            let _ = self.file.write_all(note.as_bytes());
            return;
        }
        match self.file.write_all(line.as_bytes()) {
            Ok(()) => {
                self.bytes += line.len() as u64;
                self.records += 1;
            }
            Err(err) => {
                self.error = Some(err.to_string());
            }
        }
    }
}

fn opcode(packet: &[u8]) -> Option<[u8; 2]> {
    let length = read_varint(packet, 0);
    if length.length <= 0 {
        return None;
    }
    let at = length.length as usize;
    Some([*packet.get(at)?, *packet.get(at + 1)?])
}

fn opcode_hex(packet: &[u8]) -> Option<String> {
    opcode(packet).map(|op| hex_of(&op).0)
}

fn is_damage_opcode(packet: &[u8]) -> bool {
    matches!(opcode(packet), Some([0x04 | 0x05, 0x38]))
}

fn hex_of(bytes: &[u8]) -> (String, bool) {
    let shown = &bytes[..bytes.len().min(MAX_HEX_BYTES)];
    let mut out = String::with_capacity(shown.len() * 2);
    for byte in shown {
        out.push_str(&format!("{byte:02x}"));
    }
    (out, shown.len() < bytes.len())
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::encode_frame;

    fn temp_log(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aion2dps-target-debug-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join(FILE_NAME)
    }

    fn lines(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn hit_on(target: i32) -> Hit {
        Hit {
            actor: 1234,
            target,
            skill: 12_090_243,
            damage: 500,
            multi_hits: 0,
            multi_damage: 0,
            hit_type: 2,
            crit: false,
            back: false,
            front: false,
            block: false,
            parry: false,
            perfect: false,
            double: false,
            perfect_block: false,
            dot: false,
            taken: false,
            heal: 0,
            miss: false,
            resist: false,
        }
    }

    #[test]
    fn the_environment_value_turns_it_on() {
        assert!(env_on("1"));
        assert!(env_on(" TRUE "));
        assert!(!env_on("0"));
        assert!(!env_on(""));
        assert!(requested(true));
    }

    #[test]
    fn an_unnamed_hit_logs_the_spawn_packet_once_and_later_hp() {
        let path = temp_log("spawn");
        let mut debug = TargetDebug::open(&path).unwrap();
        let target = 90_331;

        let mut spawn = vec![0x40, 0x36];
        spawn.extend(encode_varint(target as u32));
        spawn.extend([0xAA, 0xBB, 0xCC]);
        let spawn = encode_frame(&spawn);
        debug.observe(&spawn, &[], |_| false, |_| None, 1_000);

        let unrelated = encode_frame(&[0x12, 0x36, 0x01, 0x02]);
        debug.observe(&unrelated, &[], |_| false, |_| None, 1_100);

        let mut damage = vec![0x04, 0x38];
        damage.extend(encode_varint(target as u32));
        let damage = encode_frame(&damage);
        let hit = Event::Hit(hit_on(target));
        let typed = |id: i32| (id == target).then_some(0x200b81);
        debug.observe(&damage, &[hit.clone()], |_| false, typed, 2_000);
        debug.observe(&damage, &[hit], |_| false, typed, 2_100);

        let hp = Event::Hp {
            id: target,
            current: 10,
            max: 100,
        };
        debug.observe(&encode_frame(&[0x1B, 0x92]), &[hp], |_| false, |_| None, 2_200);

        let records = lines(&path);
        let kinds: Vec<&str> = records.iter().map(|r| r["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["session", "hit", "context", "hit", "hp"]);
        assert_eq!(records[1]["target"], target);
        assert_eq!(records[1]["actor"], 1234);
        assert_eq!(records[1]["amount"], 500);
        assert_eq!(records[1]["opcode"], "0438");
        assert_eq!(records[2]["monster_type"], 0x200b81);
        let packets = records[2]["packets"].as_array().unwrap();
        assert_eq!(packets.len(), 1, "damage packets and unrelated packets are left out");
        assert_eq!(packets[0]["opcode"], "4036");
        assert_eq!(packets[0]["len"], spawn.len());
        assert_eq!(packets[0]["age_ms"], 1_000);
        assert_eq!(packets[0]["id_offsets"], json!([3]));
        assert_eq!(packets[0]["hex"], hex_of(&spawn).0);
        assert_eq!(debug.info().records, 5);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_named_target_is_not_logged() {
        let path = temp_log("named");
        let mut debug = TargetDebug::open(&path).unwrap();
        let damage = encode_frame(&[0x04, 0x38, 0x00]);
        debug.observe(&damage, &[Event::Hit(hit_on(500))], |_| true, |_| None, 0);
        assert_eq!(lines(&path).len(), 1, "only the session line");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn context_keeps_three_opcodes_with_the_oldest_last() {
        let path = temp_log("context");
        let mut debug = TargetDebug::open(&path).unwrap();
        let target = 4_321;
        for (i, op) in [0x40u8, 0x41, 0x42, 0x43, 0x43].iter().enumerate() {
            let mut body = vec![*op, 0x36];
            body.extend(encode_varint(target as u32));
            debug.observe(&encode_frame(&body), &[], |_| false, |_| None, i as i64);
        }
        let damage = encode_frame(&[0x04, 0x38, 0x00]);
        debug.observe(&damage, &[Event::Hit(hit_on(target))], |_| false, |_| None, 10);
        let records = lines(&path);
        let ops: Vec<&str> = records[2]["packets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["opcode"].as_str().unwrap())
            .collect();
        assert_eq!(ops, ["4336", "4236", "4036"]);
        assert!(records[2]["monster_type"].is_null(), "no spawn type seen");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_ring_stays_bounded() {
        let path = temp_log("ring");
        let mut debug = TargetDebug::open(&path).unwrap();
        let packet = encode_frame(&[0x12, 0x36, 0x00]);
        for i in 0..(RING_PACKETS + 50) {
            debug.observe(&packet, &[], |_| false, |_| None, i as i64);
        }
        assert_eq!(debug.ring.len(), RING_PACKETS);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
