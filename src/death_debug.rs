//! Opt-in log of what the game sends around a monster's death, so kill
//! detection can be checked against real fights before anything uses it.
//!
//! Off unless `AION2DPS_DEBUG_DEATHS=1` is set (independent of
//! `AION2DPS_DEBUG_SPAWNS`). When on, these records are appended as JSON
//! lines to `%LOCALAPPDATA%\aion2dps\death-debug.jsonl`:
//!
//! - `despawn`: every `<len> 42 36 <id> 00 <flag>` with any flag. `decode`
//!   still only treats flag `01` as a despawn; this is a separate read.
//! - `despawn_41`: the same short shape under `41 36`. A real `41 36` spawn
//!   always carries a flag, a u16 and a 24-bit type after the id, so a frame
//!   that ends exactly at `00 <flag>` after the id cannot be one.
//! - `hp_8d`: the live HP record `8D <id> 02 01 00 <u32 le hp> 00 00 00 00`.
//! - `hp_1b92`: the `1B 92 <id> <current> <max>` HP record `decode` reads.
//!
//! HP lines are only written for entities that took damage or are bosses,
//! at most one per entity per second, plus always the first drop to 0.
//! Embedded `FF FF` bundles inside a packet are opened and scanned too.
//! It only reads packets the meter already received; nothing is sent.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::decode::Event;
use crate::varint::read_varint;

pub const ENV_VAR: &str = "AION2DPS_DEBUG_DEATHS";
const FILE_NAME: &str = "death-debug.jsonl";
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_HEX_BYTES: usize = 512;
pub const HP_INTERVAL_MS: i64 = 1_000;
const MAX_TRACKED: usize = 50_000;
const MAX_DEPTH: usize = 4;

#[derive(Debug, Clone, Default)]
struct Tracked {
    monster_type: Option<u32>,
    summon: bool,
    name: Option<String>,
    hits: u32,
    last_hit_ms: Option<i64>,
    last_hit_damage: i32,
    last_hit_actor: i32,
    hp: Option<i64>,
    max_hp: Option<i64>,
    last_hp_log_ms: Option<i64>,
    last_nonzero_hp_ms: Option<i64>,
    zero_logged: bool,
}

pub struct DeathDebug {
    out: Box<dyn Write + Send>,
    written: u64,
    cap: u64,
    entities: HashMap<i32, Tracked>,
}

pub fn default_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("aion2dps").join(FILE_NAME)
}

pub fn env_on(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// A short despawn-shaped record: `<len> <op> 36 <id> 00 <flag>` and nothing after.
pub fn despawn_shape(packet: &[u8]) -> Option<(u8, i32, u8)> {
    let len = read_varint(packet, 0);
    if len.length <= 0 {
        return None;
    }
    let at = len.length as usize;
    let op = *packet.get(at)?;
    if !matches!(op, 0x41 | 0x42) || packet.get(at + 1) != Some(&0x36) {
        return None;
    }
    let id = read_varint(packet, at + 2);
    if id.length <= 0 || !(100..=9_999_999).contains(&id.value) {
        return None;
    }
    let rest = packet.get(at + 2 + id.length as usize..)?;
    match rest {
        [0x00, flag] => Some((op, id.value, *flag)),
        _ => None,
    }
}

/// Every `8D <id> 02 01 00 <u32 le> 00 00 00 00` in `data` (A2Tools layout).
pub fn hp_8d_records(data: &[u8]) -> Vec<(i32, u32)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        if data[i] != 0x8D {
            i += 1;
            continue;
        }
        let id = read_varint(data, i + 1);
        if id.length <= 0 || !(100..=9_999_999).contains(&id.value) {
            i += 1;
            continue;
        }
        let d = i + 1 + id.length as usize;
        if d + 11 <= data.len()
            && data[d..d + 3] == [0x02, 0x01, 0x00]
            && data[d + 7..d + 11] == [0, 0, 0, 0]
        {
            let hp = u32::from_le_bytes([data[d + 3], data[d + 4], data[d + 5], data[d + 6]]);
            out.push((id.value, hp));
            i = d + 11;
            continue;
        }
        i += 1;
    }
    out
}

/// Packets inside `FF FF` bundles embedded in `packet`, with their depth.
fn embedded_packets(packet: &[u8], depth: usize, out: &mut Vec<(Vec<u8>, usize)>) {
    if depth >= MAX_DEPTH {
        return;
    }
    let mut i = 1;
    while i + 8 < packet.len() {
        if packet[i] != 0xFF || packet[i + 1] != 0xFF {
            i += 1;
            continue;
        }
        let opened = (1..=3usize).rev().find_map(|width| {
            let at = i.checked_sub(width)?;
            let len = read_varint(packet, at);
            if len.length != width as i32 {
                return None;
            }
            let size = crate::framing::frame_size(len.value, len.length)?;
            let end = at + size;
            let plain = crate::framing::decompress_bundle(packet.get(i..end)?)?;
            Some((end, plain))
        });
        let Some((end, plain)) = opened else {
            i += 1;
            continue;
        };
        let walked = crate::framing::walk_inner(&plain);
        for frame in &walked.frames {
            if let crate::framing::FrameKind::Packet = frame.kind {
                let bytes = frame.bytes(&plain).to_vec();
                embedded_packets(&bytes, depth + 1, out);
                out.push((bytes, depth + 1));
            }
        }
        i = end;
    }
}

/// UTC `YYYY-MM-DDTHH:MM:SS.mmmZ` for epoch milliseconds.
pub fn wall_clock(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        tod / 3600,
        (tod / 60) % 60,
        tod % 60
    )
}

fn hex_of(packet: &[u8]) -> String {
    packet[..packet.len().min(MAX_HEX_BYTES)]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl DeathDebug {
    pub fn from_env() -> Option<Self> {
        if !env_on(std::env::var(ENV_VAR).ok().as_deref()) {
            return None;
        }
        let path = default_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                let written = file.metadata().map(|m| m.len()).unwrap_or(0);
                tracing::info!("death debug log: {}", path.display());
                Some(Self::with_writer(Box::new(file), written, MAX_FILE_BYTES))
            }
            Err(err) => {
                tracing::warn!("death debug log unavailable at {}: {err}", path.display());
                None
            }
        }
    }

    pub fn with_writer(out: Box<dyn Write + Send>, written: u64, cap: u64) -> Self {
        Self { out, written, cap, entities: HashMap::new() }
    }

    fn entity(&mut self, id: i32) -> &mut Tracked {
        if self.entities.len() >= MAX_TRACKED && !self.entities.contains_key(&id) {
            self.entities.clear();
        }
        self.entities.entry(id).or_default()
    }

    fn write(&mut self, line: Value) {
        if self.written >= self.cap {
            return;
        }
        let line = line.to_string();
        if writeln!(self.out, "{line}").is_ok() {
            self.written += line.len() as u64 + 1;
        }
    }

    fn describe(&self, id: i32) -> Value {
        let Some(t) = self.entities.get(&id) else {
            return json!({ "class": "unknown", "took_damage": false });
        };
        let bundled = t.monster_type.and_then(crate::monster_names::bundled);
        let class = if t.monster_type.is_some() {
            "monster"
        } else if t.summon {
            "summon"
        } else if t.name.is_some() {
            "player"
        } else {
            "unknown"
        };
        json!({
            "class": class,
            "monster_type": t.monster_type,
            "monster_name": bundled.and_then(|b| b.name.clone()),
            "boss": bundled.map(|b| b.boss),
            "dummy": bundled.map(|b| b.dummy),
            "player_name": t.name,
            "took_damage": t.hits > 0,
            "hits": t.hits,
            "last_hit_ms": t.last_hit_ms,
            "last_hit_damage": t.last_hit_damage,
            "last_hit_actor": t.last_hit_actor,
            "last_hp": t.hp,
            "max_hp": t.max_hp,
            "last_nonzero_hp_ms": t.last_nonzero_hp_ms,
        })
    }

    fn is_boss(&self, id: i32) -> bool {
        self.entities
            .get(&id)
            .and_then(|t| t.monster_type)
            .and_then(crate::monster_names::bundled)
            .is_some_and(|b| b.boss)
    }

    /// Feed one decoded frame. Reads `events` only; never changes them.
    pub fn observe(&mut self, packet: &[u8], events: &[Event], now_ms: i64) {
        for event in events {
            match event {
                Event::Spawn { id, monster_type } => {
                    *self.entity(*id) = Tracked { monster_type: Some(*monster_type), ..Default::default() };
                }
                Event::SummonSpawn { summon, .. } => {
                    *self.entity(*summon) = Tracked { summon: true, ..Default::default() };
                }
                Event::Name { id, name, .. } => {
                    self.entity(*id).name = Some(name.clone());
                }
                Event::Hit(hit) if hit.total() > 0 && !hit.taken => {
                    let t = self.entity(hit.target);
                    t.hits = t.hits.saturating_add(1);
                    t.last_hit_ms = Some(now_ms);
                    t.last_hit_damage = hit.total();
                    t.last_hit_actor = hit.actor;
                }
                Event::Hp { id, current, max } => {
                    self.hp(*id, *current as i64, Some(*max as i64), "hp_1b92", packet, 0, now_ms);
                }
                _ => {}
            }
        }
        let mut packets = vec![(packet.to_vec(), 0usize)];
        embedded_packets(packet, 0, &mut packets);
        for (bytes, depth) in packets {
            if let Some((op, id, flag)) = despawn_shape(&bytes) {
                let kind = if op == 0x42 { "despawn" } else { "despawn_41" };
                let entity = self.describe(id);
                let since_hp = self
                    .entities
                    .get(&id)
                    .and_then(|t| t.last_nonzero_hp_ms)
                    .map(|ms| now_ms - ms);
                let since_hit = self.entities.get(&id).and_then(|t| t.last_hit_ms).map(|ms| now_ms - ms);
                self.write(json!({
                    "ts_ms": now_ms,
                    "wall": wall_clock(now_ms),
                    "kind": kind,
                    "id": id,
                    "flag": flag,
                    "depth": depth,
                    "ms_since_last_nonzero_hp": since_hp,
                    "ms_since_last_hit": since_hit,
                    "entity": entity,
                    "len": bytes.len(),
                    "hex": hex_of(&bytes),
                }));
                if op == 0x42 {
                    self.entities.remove(&id);
                }
            }
            for (id, hp) in hp_8d_records(&bytes) {
                self.hp(id, hp as i64, None, "hp_8d", &bytes, depth, now_ms);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn hp(&mut self, id: i32, current: i64, max: Option<i64>, kind: &str, packet: &[u8], depth: usize, now_ms: i64) {
        let boss = self.is_boss(id);
        let Some(t) = self.entities.get_mut(&id) else {
            return;
        };
        if t.hits == 0 && !boss {
            return;
        }
        if let Some(max) = max {
            t.max_hp = Some(max);
        }
        let first_zero = current == 0 && !t.zero_logged;
        let due = t.last_hp_log_ms.is_none_or(|last| now_ms - last >= HP_INTERVAL_MS);
        t.hp = Some(current);
        if current > 0 {
            t.last_nonzero_hp_ms = Some(now_ms);
            t.zero_logged = false;
        }
        if !(first_zero || due) {
            return;
        }
        t.last_hp_log_ms = Some(now_ms);
        if current == 0 {
            t.zero_logged = true;
        }
        let ratio = t.max_hp.filter(|m| *m > 0).map(|m| current as f64 / m as f64);
        let entity = self.describe(id);
        self.write(json!({
            "ts_ms": now_ms,
            "wall": wall_clock(now_ms),
            "kind": kind,
            "id": id,
            "hp": current,
            "max_hp": max,
            "ratio": ratio,
            "depth": depth,
            "entity": entity,
            "len": packet.len(),
            "hex": hex_of(packet),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Hit;
    use crate::framing::{encode_bundle, encode_frame};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Sink {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect()
        }
    }

    fn debug(cap: u64) -> (DeathDebug, Sink) {
        let sink = Sink::default();
        (DeathDebug::with_writer(Box::new(sink.clone()), 0, cap), sink)
    }

    fn varint(mut v: u32) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return out;
            }
            out.push(b | 0x80);
        }
    }

    fn despawn(op: u8, id: u32, flag: u8) -> Vec<u8> {
        let mut body = vec![op, 0x36];
        body.extend(varint(id));
        body.extend([0x00, flag]);
        encode_frame(&body)
    }

    fn hp8d(id: u32, hp: u32) -> Vec<u8> {
        let mut body = vec![0x99, 0x99, 0x8D];
        body.extend(varint(id));
        body.extend([0x02, 0x01, 0x00]);
        body.extend(hp.to_le_bytes());
        body.extend([0, 0, 0, 0]);
        encode_frame(&body)
    }

    fn hit(target: i32) -> Event {
        Event::Hit(Hit { actor: 5_000, target, skill: 11_000_000, damage: 100, ..Default::default() })
    }

    #[test]
    fn env_gating() {
        assert!(env_on(Some("1")) && env_on(Some(" TRUE ")) && env_on(Some("on")));
        assert!(!env_on(None) && !env_on(Some("0")) && !env_on(Some("")));
    }

    #[test]
    fn wall_clock_is_utc() {
        assert_eq!(wall_clock(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(wall_clock(1_793_806_200_123), "2026-11-04T15:30:00.123Z");
    }

    #[test]
    fn every_despawn_flag_is_logged_with_entity_info() {
        let (mut d, sink) = debug(MAX_FILE_BYTES);
        d.observe(&[], &[Event::Spawn { id: 19_069, monster_type: 1234 }, hit(19_069)], 1_000);
        for flag in [1u8, 3, 7] {
            d.observe(&despawn(0x42, 19_069 + flag as u32, flag), &[], 2_000);
        }
        d.observe(&despawn(0x42, 19_069, 3), &[], 2_500);
        let lines = sink.lines();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines.iter().map(|l| l["flag"].as_u64().unwrap()).collect::<Vec<_>>(), vec![1, 3, 7, 3]);
        let last = &lines[3];
        assert_eq!(last["kind"], "despawn");
        assert_eq!(last["entity"]["class"], "monster");
        assert_eq!(last["entity"]["monster_type"], 1234);
        assert_eq!(last["entity"]["took_damage"], true);
        assert_eq!(last["ms_since_last_hit"], 1_500);
        assert_eq!(last["hex"], "0b4236fd94010003");
        assert_eq!(last["wall"], "1970-01-01T00:00:02.500Z");
        assert_eq!(lines[0]["entity"]["class"], "unknown");
    }

    #[test]
    fn short_41_shape_only() {
        assert_eq!(despawn_shape(&despawn(0x41, 19_069, 3)), Some((0x41, 19_069, 3)));
        let mut spawn = vec![0x41, 0x36];
        spawn.extend(varint(19_069));
        spawn.extend([0x04, 0x20, 0x00, 0x01, 0x02, 0x03, 0x00, 0x00, 0x02]);
        assert_eq!(despawn_shape(&encode_frame(&spawn)), None);
        let mut longer = despawn(0x42, 19_069, 1);
        longer.push(0);
        assert_eq!(despawn_shape(&longer), None);
    }

    #[test]
    fn despawn_inside_an_embedded_bundle_is_found() {
        let (mut d, sink) = debug(MAX_FILE_BYTES);
        let inner = despawn(0x42, 20_000, 3);
        let mut body = vec![0x77, 0x77];
        body.extend(encode_bundle(&inner));
        d.observe(&encode_frame(&body), &[], 10);
        let lines = sink.lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["depth"], 1);
        assert_eq!(lines[0]["flag"], 3);
    }

    #[test]
    fn hp_only_for_damaged_entities_rate_limited_and_zero_always() {
        let (mut d, sink) = debug(MAX_FILE_BYTES);
        d.observe(&hp8d(30_000, 500), &[], 0); // never hit: skipped
        d.observe(&[], &[Event::Spawn { id: 30_000, monster_type: 1 }, hit(30_000)], 0);
        d.observe(&hp8d(30_000, 900), &[], 100); // logged
        d.observe(&hp8d(30_000, 800), &[], 500); // rate-limited
        d.observe(&hp8d(30_000, 0), &[], 600); // first zero: logged
        d.observe(&hp8d(30_000, 0), &[], 700); // repeat zero: skipped
        d.observe(&hp8d(30_000, 700), &[], 1_700); // due again
        let lines = sink.lines();
        let hps: Vec<i64> = lines.iter().map(|l| l["hp"].as_i64().unwrap()).collect();
        assert_eq!(hps, vec![900, 0, 700]);
        assert_eq!(lines[0]["kind"], "hp_8d");
        d.observe(&despawn(0x42, 30_000, 3), &[], 2_000);
        let last = sink.lines().pop().unwrap();
        assert_eq!(last["ms_since_last_nonzero_hp"], 300);
    }

    #[test]
    fn hp_1b92_carries_max_and_ratio() {
        let (mut d, sink) = debug(MAX_FILE_BYTES);
        d.observe(&[], &[hit(40_000)], 0);
        d.observe(&[0x01], &[Event::Hp { id: 40_000, current: 250, max: 1_000 }], 10);
        let line = &sink.lines()[0];
        assert_eq!(line["kind"], "hp_1b92");
        assert_eq!(line["max_hp"], 1_000);
        assert_eq!(line["ratio"], 0.25);
    }

    #[test]
    fn hex_is_capped_per_line() {
        let (mut d, sink) = debug(MAX_FILE_BYTES);
        d.observe(&[], &[hit(30_000)], 0);
        let mut big = vec![0x55; 4_000];
        big.extend([0x8D]);
        big.extend(varint(30_000));
        big.extend([0x02, 0x01, 0x00, 5, 0, 0, 0, 0, 0, 0, 0]);
        d.observe(&encode_frame(&big), &[], 1);
        let line = &sink.lines()[0];
        assert_eq!(line["hex"].as_str().unwrap().len(), MAX_HEX_BYTES * 2);
        assert!(line["len"].as_u64().unwrap() > 4_000);
    }

    #[test]
    fn file_cap_stops_writing() {
        let (mut d, sink) = debug(300);
        for i in 0..20 {
            d.observe(&despawn(0x42, 20_000 + i, 3), &[], i as i64);
        }
        let n = sink.lines().len();
        assert!((1..20).contains(&n), "{n}");
        assert!(sink.0.lock().unwrap().len() < 300 + 600);
    }
}
