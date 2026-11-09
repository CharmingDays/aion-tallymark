//! Passive cube tracking. Unknown templates and uncalibrated coordinates never alert.
//! Wire hypotheses are deliberately separate from verified classification data.
use crate::{decode::Event, varint::try_read_varint};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

pub const PLAYER_FRESH_MS: i64 = 2_000;
/// A distance older than this is shown as "last known".
pub const DISTANCE_STALE_MS: i64 = 12_000;
/// Closer/farther only flips after the distance moved this far (metres).
pub const TREND_STEP_M: f64 = 4.0;
pub const ALERT_MS: i64 = 12_000;
const MAX_TRACKED: usize = 4096;

#[derive(Clone, Debug, Deserialize)]
pub struct Template {
    pub id: u32,
    pub name: String,
    pub verified: bool,
}
/// Offsets are relative to the byte immediately AFTER the entity varint.
/// Axes must be calibrated as east, north, elevation; units_to_meters is not guessed.
#[derive(Clone, Debug, Deserialize)]
pub struct PositionLayout {
    pub verified: bool,
    pub offsets: [usize; 3],
    pub units_to_meters: f64,
}
/// Offsets here are relative to the opcode, not the length prefix.
#[derive(Clone, Debug, Deserialize)]
pub struct MapLayout {
    pub verified: bool,
    pub opcode: [u8; 2],
    pub id_offset: usize,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Protocol {
    pub templates: Vec<Template>,
    pub position: Option<PositionLayout>,
    pub spawn_position: Option<PositionLayout>,
    pub map: Option<MapLayout>,
}
impl Protocol {
    pub fn bundled() -> Self {
        serde_json::from_str(include_str!("../data/hidden-cubes.json")).expect("cube protocol data")
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Position {
    pub east: f64,
    pub north: f64,
    pub elevation: f64,
}
impl Position {
    fn valid(self) -> bool {
        [self.east, self.north, self.elevation]
            .into_iter()
            .all(|n| n.is_finite() && n.abs() < 1_000_000.0)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Observation {
    Spawn { id: i32, template: u32 },
    Gone { id: i32 },
    Consumed { id: i32 },
    Opener { id: i32, by: i32 },
    Position { id: i32, position: Position },
    Map(u32),
    WorldBoundary,
}

/// Only reads complete frames already opened by the meter's framer. No raw
/// substring search for opcodes: arbitrary bytes in damage/chat are not spawns.
pub fn observations(packet: &[u8], protocol: &Protocol) -> Vec<Observation> {
    let mut at = 0;
    let Some(len) = try_read_varint(packet, &mut at) else {
        return vec![];
    };
    if crate::framing::frame_size(len, at as i32) != Some(packet.len()) {
        return vec![];
    }
    let Some(op) = packet.get(at..at + 2) else {
        return vec![];
    };
    let body = &packet[at..];
    let mut out = Vec::new();
    if let Some(map) = &protocol.map {
        if map.verified && op == map.opcode {
            if let Some(bytes) = body.get(map.id_offset..map.id_offset.saturating_add(4)) {
                out.push(Observation::Map(u32::from_le_bytes(
                    bytes.try_into().unwrap(),
                )));
            }
        }
    }
    // Self spawn is a conservative world boundary, not a claimed map opcode.
    if op == [0x33, 0x36] {
        out.push(Observation::WorldBoundary);
    }
    let mut pos = at + 2;
    let Some(id) = try_read_varint(packet, &mut pos) else {
        return out;
    };
    if !(100..=9_999_999).contains(&id) {
        return out;
    }
    match op {
        [0x40, 0x36] => {
            // Community template marker heuristic. Ambiguous markers are rejected.
            let candidates: Vec<_> = (pos + 3..packet.len().saturating_sub(2))
                .filter_map(|i| {
                    if packet[i] != 0 || packet[i + 1] & 0xBF != 0 || packet[i + 2] != 2 {
                        return None;
                    }
                    let template =
                        u32::from_le_bytes([packet[i - 3], packet[i - 2], packet[i - 1], 0]);
                    (template != 0).then_some(template)
                })
                .collect();
            if candidates.len() == 1 {
                out.push(Observation::Spawn {
                    id,
                    template: candidates[0],
                });
            }
        }
        // Interactable world objects (verified 2026-11-07 against a hidden cube):
        // 34 36 <id> <2 bytes> <LE24 template> 00 <f32 east> <f32 north> <f32 elev> (cm).
        [0x34, 0x36] => {
            if let Some(b) = packet.get(pos..pos + 18) {
                let template = u32::from_le_bytes([b[2], b[3], b[4], 0]);
                if template != 0 {
                    out.push(Observation::Spawn { id, template });
                    let f = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64 * 0.01;
                    let position = Position { east: f(6), north: f(10), elevation: f(14) };
                    if position.valid() {
                        out.push(Observation::Position { id, position });
                    }
                }
            }
        }
        // 3A 36 <object id> <actor id> ..: interaction finished by actor.
        [0x3A, 0x36] => {
            let mut at = pos;
            if let Some(by) = try_read_varint(packet, &mut at) {
                out.push(Observation::Opener { id, by });
            }
        }
        // 35 36 <id> <code> ..: 01 = left range, 04 = opened/consumed.
        [0x35, 0x36] => match packet.get(pos) {
            Some(1) => out.push(Observation::Gone { id }),
            Some(4) => out.push(Observation::Consumed { id }),
            _ => {}
        },
        // This exact suffix exists in local target-debug; meaning still needs
        // correlation against a known cube. Existing 42 36 removals also work.
        [0x21, 0x8D] | [0x42, 0x36] if packet.get(pos..) == Some(&[0, 1][..]) => {
            out.push(Observation::Gone { id });
        }
        [0x2A, 0x38] => {
            if let Some(layout) = &protocol.position {
                if layout.verified
                    && layout.units_to_meters.is_finite()
                    && layout.units_to_meters > 0.0
                {
                    if let Some(position) = read_position(packet, pos, layout) {
                        out.push(Observation::Position { id, position });
                    }
                }
            }
        }
        _ => {}
    }
    if op == [0x40, 0x36] && out.iter().any(|o| matches!(o, Observation::Spawn { .. })) {
        if let Some(layout) = protocol.spawn_position.as_ref().filter(|p| p.verified) {
            if let Some(position) = read_position(packet, pos, layout) {
                out.push(Observation::Position { id, position });
            }
        }
    }
    out
}
/// The caster's own position from a skill-use packet, verified against
/// captures on 2026-11-09 (it matched the self spawn and moved with the
/// player; cube spawns use the same centimetre units):
///
/// `2A 38 <caster> 01 <flag> ... <u64 ms> <caster> <tail> <f32 east> <f32 north> <f32 elev>`
/// `2B 38 <caster> <flag> ... <u64 ms> <caster> <tail> <f32 ×3>`
///
/// `flag` 0x11: `tail` is `<n> 00|02`; 0x13: `tail` is `01 <LE24> 00 <kind>`.
/// The position follows the SECOND copy of the caster id. Values in metres.
pub fn cast_position(packet: &[u8]) -> Option<(i32, Position)> {
    let mut at = 0;
    let len = try_read_varint(packet, &mut at)?;
    if crate::framing::frame_size(len, at as i32) != Some(packet.len()) {
        return None;
    }
    let op = packet.get(at..at + 2)?;
    let mut pos = at + 2;
    let caster = try_read_varint(packet, &mut pos)?;
    if !(1..=9_999_999).contains(&caster) {
        return None;
    }
    let flag = match op {
        [0x2A, 0x38] => {
            if *packet.get(pos)? != 1 {
                return None;
            }
            *packet.get(pos + 1)?
        }
        [0x2B, 0x38] => *packet.get(pos)?,
        _ => return None,
    };
    let skip = match flag {
        0x11 => 2,
        0x13 => 6,
        _ => return None,
    };
    let needle = encode_id(caster);
    let mut from = pos;
    while let Some(found) = packet
        .get(from..)
        .and_then(|rest| rest.windows(needle.len()).position(|w| w == needle.as_slice()))
    {
        let id_at = from + found;
        from = id_at + 1;
        // A real second copy is preceded by the u64 timestamp's zero high bytes.
        if id_at < 2 || packet[id_at - 2..id_at] != [0, 0] {
            continue;
        }
        let tail = id_at + needle.len();
        let ok = match flag {
            0x11 => matches!(packet.get(tail + 1), Some(0 | 2)),
            _ => packet.get(tail) == Some(&1) && packet.get(tail + 4) == Some(&0),
        };
        if !ok {
            continue;
        }
        let b = packet.get(tail + skip..tail + skip + 12)?;
        let f = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64 * 0.01;
        let position = Position { east: f(0), north: f(4), elevation: f(8) };
        return position.valid().then_some((caster, position));
    }
    None
}
fn encode_id(id: i32) -> Vec<u8> {
    let mut value = id as u32;
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}
fn read_position(packet: &[u8], base: usize, layout: &PositionLayout) -> Option<Position> {
    if !layout.verified || !layout.units_to_meters.is_finite() || layout.units_to_meters <= 0.0 {
        return None;
    }
    let values: Option<Vec<f64>> = layout
        .offsets
        .iter()
        .map(|offset| {
            let start = base.checked_add(*offset)?;
            let bytes = packet.get(start..start.checked_add(4)?)?;
            Some(f32::from_le_bytes(bytes.try_into().ok()?) as f64 * layout.units_to_meters)
        })
        .collect();
    let v = values?;
    let position = Position {
        east: v[0],
        north: v[1],
        elevation: v[2],
    };
    position.valid().then_some(position)
}

/// Packets carried in bundles embedded inside `packet` (`<varint len> FF FF <u32> <lz4>`),
/// opened recursively. The stream framer only opens top-level bundles; world
/// spawns usually arrive embedded, so cube discovery must look inside too.
pub fn embedded_packets(packet: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    embedded_into(packet, 0, &mut out);
    out
}
fn embedded_into(packet: &[u8], depth: usize, out: &mut Vec<Vec<u8>>) {
    if depth >= crate::framing::MAX_BUNDLE_DEPTH {
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
            let mut cursor = at;
            let len = try_read_varint(packet, &mut cursor)?;
            if cursor - at != width {
                return None;
            }
            let size = crate::framing::frame_size(len, width as i32)?;
            let end = at.checked_add(size)?;
            let plain = crate::framing::decompress_bundle(packet.get(i..end)?)?;
            Some((end, plain))
        });
        let Some((end, plain)) = opened else {
            i += 1;
            continue;
        };
        for frame in &crate::framing::walk_inner(&plain).frames {
            match frame.kind {
                crate::framing::FrameKind::Packet => {
                    let inner = frame.bytes(&plain).to_vec();
                    embedded_into(&inner, depth + 1, out);
                    out.push(inner);
                }
                crate::framing::FrameKind::Bundle => {
                    if let Some(p) = crate::framing::decompress_bundle(frame.payload(&plain)) {
                        for f in &crate::framing::walk_inner(&p).frames {
                            if matches!(f.kind, crate::framing::FrameKind::Packet) {
                                out.push(f.bytes(&p).to_vec());
                            }
                        }
                    }
                }
            }
        }
        i = end;
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct CubeAlert {
    pub sequence: u64,
    pub ts_ms: i64,
    pub status: &'static str,
    pub entity_id: i32,
    pub cube_type: String,
    pub distance_m: Option<u32>,
    /// Never set: compass direction waits until the game's axes are confirmed.
    pub direction: Option<&'static str>,
    /// "closer" / "farther" once the distance moved by TREND_STEP_M.
    pub trend: Option<&'static str>,
    /// Age of the player position behind `distance_m`.
    pub distance_age_ms: Option<i64>,
    /// True after DISTANCE_STALE_MS without a new position: "last known".
    pub distance_stale: bool,
    pub opened_by_id: Option<i32>,
    pub opened_by: Option<String>,
    pub opened_by_you: bool,
}
struct Tracked {
    template: u32,
    name: String,
    position: Option<Position>,
    opened_by: Option<i32>,
    range: Range,
}
/// Player-to-cube distance, recomputed on each new player fix.
#[derive(Clone, Copy, Debug, Default)]
struct Range {
    /// Metres and the player fix's time.
    last: Option<(f64, i64)>,
    /// Distance at the last trend change; moves only past TREND_STEP_M.
    anchor: Option<f64>,
    trend: Option<&'static str>,
}
impl Range {
    fn update(&mut self, meters: f64, fix_ts: i64) {
        self.last = Some((meters, fix_ts));
        match self.anchor {
            None => self.anchor = Some(meters),
            Some(anchor) if meters <= anchor - TREND_STEP_M => {
                self.trend = Some("closer");
                self.anchor = Some(meters);
            }
            Some(anchor) if meters >= anchor + TREND_STEP_M => {
                self.trend = Some("farther");
                self.anchor = Some(meters);
            }
            _ => {}
        }
    }
}
fn distance(a: Position, b: Position) -> f64 {
    let (dx, dy, dz) = (a.east - b.east, a.north - b.north, a.elevation - b.elevation);
    (dx * dx + dy * dy + dz * dz).sqrt()
}
pub struct Detector {
    pub protocol: Protocol,
    entities: HashMap<i32, Tracked>,
    positions: HashMap<i32, (Position, i64)>,
    player: Option<i32>,
    map: Option<u32>,
    sequence: u64,
    alerts: VecDeque<CubeAlert>,
}
impl Default for Detector {
    fn default() -> Self {
        Self::new(Protocol::bundled())
    }
}
impl Detector {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            entities: HashMap::new(),
            positions: HashMap::new(),
            player: None,
            map: None,
            sequence: 0,
            alerts: VecDeque::new(),
        }
    }
    /// A new TCP flow, self spawn, or confirmed map transition invalidates ALL
    /// positions and duplicate suppression. Never report old-map cubes as nearby.
    pub fn reset_world(&mut self) {
        self.entities.clear();
        self.positions.clear();
        self.alerts.clear();
        self.player = None;
    }
    pub fn observe_packet(
        &mut self,
        packet: &[u8],
        events: &[Event],
        now: i64,
    ) -> Vec<Observation> {
        let found = observations(packet, &self.protocol);
        for observation in found.iter().cloned() {
            self.apply(observation, now);
        }
        for event in events {
            match event {
                Event::You { id }
                | Event::Name {
                    id, is_self: true, ..
                } => {
                    if self.player.is_some_and(|old| old != *id) {
                        self.reset_world();
                    }
                    self.player = Some(*id);
                }
                Event::Despawn { id } | Event::Removed { id, .. } => {
                    self.apply(Observation::Gone { id: *id }, now)
                }
                _ => {}
            }
        }
        // Only the local player's own casts move the player position.
        if let Some((caster, position)) = cast_position(packet) {
            if Some(caster) == self.player {
                self.player_fix(position, now);
            }
        }
        found
    }
    pub fn apply(&mut self, event: Observation, now: i64) {
        match event {
            Observation::WorldBoundary => self.reset_world(),
            Observation::Map(map) => {
                if self.map != Some(map) {
                    self.reset_world();
                    self.map = Some(map);
                }
            }
            Observation::Position { id, position } => {
                if !position.valid() {
                    return;
                }
                if self.positions.len() >= MAX_TRACKED && !self.positions.contains_key(&id) {
                    if let Some(old) = self
                        .positions
                        .iter()
                        .filter(|(id, _)| Some(**id) != self.player)
                        .min_by_key(|(_, (_, ts))| *ts)
                        .map(|(id, _)| *id)
                    {
                        self.positions.remove(&old);
                    }
                }
                self.positions.insert(id, (position, now));
                if Some(id) == self.player {
                    self.player_fix(position, now);
                    return;
                }
                let player = self.player.and_then(|p| self.positions.get(&p)).copied();
                if let Some(cube) = self.entities.get_mut(&id) {
                    cube.position = Some(position);
                    if let Some((at, ts)) = player {
                        cube.range.update(distance(position, at), ts);
                    }
                }
            }
            Observation::Spawn { id, template } => {
                if self
                    .entities
                    .get(&id)
                    .is_some_and(|cube| cube.template == template)
                {
                    return;
                }
                // Reused entity IDs must lose old coordinates, including unknown replacements.
                if let Some(old) = self.entities.remove(&id) {
                    self.positions.remove(&id);
                    self.emit(id, old.name, "gone", now);
                }
                let Some(kind) = self
                    .protocol
                    .templates
                    .iter()
                    .find(|kind| kind.id == template && kind.verified)
                else {
                    return;
                };
                if self.entities.len() >= MAX_TRACKED {
                    return;
                }
                let name = kind.name.clone();
                let position = self
                    .positions
                    .get(&id)
                    .filter(|(_, ts)| fresh(now, *ts))
                    .map(|(p, _)| *p);
                let mut range = Range::default();
                if let (Some(cube), Some((at, ts))) =
                    (position, self.player.and_then(|p| self.positions.get(&p)).copied())
                {
                    range.update(distance(cube, at), ts);
                }
                self.entities.insert(
                    id,
                    Tracked {
                        template,
                        name: name.clone(),
                        position,
                        opened_by: None,
                        range,
                    },
                );
                self.emit(id, name, "detected", now);
            }
            Observation::Opener { id, by } => {
                if let Some(cube) = self.entities.get_mut(&id) {
                    cube.opened_by = Some(by);
                }
            }
            Observation::Consumed { id } => {
                self.positions.remove(&id);
                if let Some(cube) = self.entities.remove(&id) {
                    self.alerts.retain(|a| a.entity_id != id);
                    self.emit(id, cube.name, "opened", now);
                    if let Some(last) = self.alerts.back_mut() {
                        last.opened_by_id = cube.opened_by;
                        last.opened_by_you = cube.opened_by.is_some() && cube.opened_by == self.player;
                    }
                }
            }
            Observation::Gone { id } => {
                self.positions.remove(&id);
                if let Some(cube) = self.entities.remove(&id) {
                    self.emit(id, cube.name, "gone", now);
                }
            }
        }
    }
    fn emit(&mut self, id: i32, name: String, status: &'static str, now: i64) {
        self.sequence = self.sequence.saturating_add(1);
        self.alerts.push_back(CubeAlert {
            sequence: self.sequence,
            ts_ms: now,
            status,
            entity_id: id,
            cube_type: name,
            distance_m: None,
            direction: None,
            trend: None,
            distance_age_ms: None,
            distance_stale: false,
            opened_by_id: None,
            opened_by: None,
            opened_by_you: false,
        });
        while self.alerts.len() > 32 {
            self.alerts.pop_front();
        }
    }
    /// A new position for the local player: every tracked cube's distance
    /// is recomputed from it (3D, metres).
    fn player_fix(&mut self, position: Position, now: i64) {
        if let Some(player) = self.player {
            self.positions.insert(player, (position, now));
        }
        for cube in self.entities.values_mut() {
            if let Some(at) = cube.position {
                cube.range.update(distance(at, position), now);
            }
        }
    }
    /// Last known distance (rounded metres), trend, and the fix's age.
    pub fn relative(&self, id: i32, now: i64) -> Option<(u32, Option<&'static str>, i64)> {
        let range = &self.entities.get(&id)?.range;
        let (meters, ts) = range.last?;
        Some((meters.round() as u32, range.trend, now.saturating_sub(ts).max(0)))
    }
    pub fn player_position(&self, now: i64) -> Option<Position> {
        let (position, ts) = self.positions.get(&self.player?)?;
        (now >= *ts && now - *ts <= DISTANCE_STALE_MS).then_some(*position)
    }
    pub fn player_id(&self) -> Option<i32> {
        self.player
    }
    pub fn clear_alerts(&mut self) {
        self.alerts.clear();
    }
    pub fn alerts(&mut self, now: i64) -> Vec<CubeAlert> {
        let live = &self.entities;
        self.alerts.retain(|a| {
            now >= a.ts_ms
                && (now - a.ts_ms < ALERT_MS || (a.status == "detected" && live.contains_key(&a.entity_id)))
        });
        self.alerts
            .iter()
            .cloned()
            .map(|mut a| {
                if a.status == "detected" {
                    if let Some((distance, trend, age)) = self.relative(a.entity_id, now) {
                        a.distance_m = Some(distance);
                        a.trend = trend;
                        a.distance_age_ms = Some(age);
                        a.distance_stale = age > DISTANCE_STALE_MS;
                    }
                }
                a
            })
            .collect()
    }
}
fn fresh(now: i64, ts: i64) -> bool {
    now >= ts && now - ts <= PLAYER_FRESH_MS
}
