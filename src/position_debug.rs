//! Opt-in log for finding the local player's position on the wire, so cube
//! alerts can later show distance and direction. Nothing here feeds alerts.
//!
//! Off unless `AION2DPS_DEBUG_POSITION=1`. When on, JSON lines go to
//! `%LOCALAPPDATA%\aion2dps\position-debug.jsonl`:
//!
//! - `player`: every server packet whose body holds the local player's id as
//!   a varint (or a candidate opcode about the player), with where the id sits
//!   and every f32 triple that looks like world coordinates.
//! - `self_spawn`: the `33 36` self spawn, which carries the spawn position.
//! - `cube`: hidden-cube spawn positions (`34 36`), in the same raw units, so
//!   both coordinate systems can be compared.
//! - `outbound`: frames from the client to the locked server port (short hex
//!   only), to see whether movement the client sends is readable at all.
//!
//! Bounded by size and rows. It only reads packets the meter already received.
use crate::hidden_cubes::Observation;
use crate::varint::try_read_varint;
use serde_json::{json, Value};
use std::{fs::OpenOptions, io::Write, path::PathBuf};

pub const ENV_VAR: &str = "AION2DPS_DEBUG_POSITION";
const FILE_NAME: &str = "position-debug.jsonl";
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_ROWS: u32 = 400_000;
const MAX_HEX: usize = 256;
const MAX_OUT_HEX: usize = 96;
const MAX_TRIPLES: usize = 16;

/// Opcodes that carried a position next to the player's id in earlier logs
/// (hits 02 38 / 03 38 / 1C 38, skill use 2A 38 / 2B 38, self spawn 33 36)
/// and the movement family other entities use (1A..30 37).
pub const CANDIDATES: &[[u8; 2]] = &[
    [0x02, 0x38], [0x03, 0x38], [0x1C, 0x38], [0x2A, 0x38], [0x2B, 0x38], [0x33, 0x36],
    [0x1A, 0x37], [0x1B, 0x37], [0x1C, 0x37], [0x1E, 0x37], [0x1F, 0x37], [0x29, 0x37],
    [0x2F, 0x37], [0x30, 0x37],
];

pub struct PositionDebug {
    out: Box<dyn Write + Send>,
    written: u64,
    rows: u32,
}

pub fn default_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("aion2dps")
        .join(FILE_NAME)
}

/// Where a frame came from, relative to the locked game server port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Server,
    Client,
    Unknown,
}

impl PositionDebug {
    pub fn from_env() -> Option<Self> {
        if std::env::var(ENV_VAR).ok().as_deref() != Some("1") {
            return None;
        }
        let path = default_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok()?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
        let written = file.metadata().ok()?.len();
        tracing::info!("position debug log: {}", path.display());
        Some(Self { out: Box::new(file), written, rows: 0 })
    }

    pub fn with_writer(out: Box<dyn Write + Send>) -> Self {
        Self { out, written: 0, rows: 0 }
    }

    pub fn full(&self) -> bool {
        self.written >= MAX_FILE_BYTES || self.rows >= MAX_ROWS
    }

    /// `packet` is one complete frame (length prefix included). `player` is
    /// the local player's id once the server identified it.
    pub fn observe(
        &mut self,
        packet: &[u8],
        found: &[Observation],
        player: Option<i32>,
        direction: Direction,
        server_identified: bool,
        now: i64,
    ) {
        if self.full() {
            return;
        }
        let mut at = 0;
        if try_read_varint(packet, &mut at).is_none() {
            return;
        }
        let body = &packet[at..];
        if direction == Direction::Client {
            let shown = &body[..body.len().min(MAX_OUT_HEX)];
            self.write(json!({"ts_ms":now,"kind":"outbound","len":packet.len(),
                "hex":hex(shown),"triples":triples(body)}));
            return;
        }
        let Some(op) = body.get(0..2) else { return };
        let opcode = format!("{:02x}{:02x}", op[0], op[1]);
        let mut id_at = 2;
        let entity = try_read_varint(body, &mut id_at);
        for o in found {
            if let Observation::Position { id, position } = o {
                let template = found.iter().find_map(|f| match f {
                    Observation::Spawn { id: s, template } if s == id => Some(*template),
                    _ => None,
                });
                self.write(json!({"ts_ms":now,"kind":"cube","opcode":opcode,"entity_id":id,
                    "template":template,"position_m":position,
                    "raw":[position.east * 100.0, position.north * 100.0, position.elevation * 100.0]}));
            }
        }
        if !server_identified {
            return;
        }
        let self_spawn = op == [0x33, 0x36];
        let id_at_offsets = player.map(|p| varint_offsets(body, p)).unwrap_or_default();
        if !self_spawn && id_at_offsets.is_empty() {
            return;
        }
        let candidate = CANDIDATES.iter().any(|c| c[..] == *op);
        let shown = &body[..body.len().min(MAX_HEX)];
        self.write(json!({"ts_ms":now,"kind":if self_spawn {"self_spawn"} else {"player"},
            "opcode":opcode,"entity_id":entity,"player_id":player,
            "is_player":entity.is_some() && entity == player,"candidate":candidate,
            "player_id_at":id_at_offsets,"triples":triples(body),
            "len":packet.len(),"hex":hex(shown),"truncated":body.len() > shown.len()}));
    }

    fn write(&mut self, line: Value) {
        let line = line.to_string();
        let size = line.len() as u64 + 1;
        if self.written + size > MAX_FILE_BYTES || self.rows >= MAX_ROWS {
            self.written = MAX_FILE_BYTES;
            return;
        }
        if writeln!(self.out, "{line}").is_ok() {
            self.written += size;
            self.rows += 1;
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn encode_varint(mut value: u32) -> Vec<u8> {
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

/// Offsets (from the opcode) where `id` appears as a varint, after the opcode.
pub fn varint_offsets(body: &[u8], id: i32) -> Vec<usize> {
    if id <= 0 {
        return vec![];
    }
    let needle = encode_varint(id as u32);
    (2..body.len().saturating_sub(needle.len() - 1))
        .filter(|&i| body[i..].starts_with(&needle))
        // Not the tail of a longer varint.
        .filter(|&i| body[i - 1] & 0x80 == 0 || i == 2)
        .take(8)
        .collect()
}

/// f32 LE triples that could be world coordinates (the game uses centimetres;
/// cube spawns are tens of thousands of units). Offsets are from the opcode.
pub fn triples(body: &[u8]) -> Vec<Value> {
    let f = |i: usize| f32::from_le_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]);
    let mut out = Vec::new();
    let mut i = 2;
    while i + 12 <= body.len() && out.len() < MAX_TRIPLES {
        let (x, y, z) = (f(i), f(i + 4), f(i + 8));
        let plausible = [x, y, z].iter().all(|v| v.is_finite() && v.abs() <= 1.0e6)
            && x.abs() >= 50.0
            && y.abs() >= 50.0
            && z.abs() <= 2.0e5
            && !(x == y && y == z);
        if plausible {
            out.push(json!({"at":i,"v":[x, y, z]}));
            i += 12;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hidden_cubes::Position;
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
    fn rows(sink: &Sink) -> Vec<Value> {
        String::from_utf8(sink.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    fn frame(body: &[u8]) -> Vec<u8> {
        crate::framing::encode_frame(body)
    }
    fn floats(v: [f32; 3]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    #[test]
    fn logs_packets_with_the_player_id_and_their_coordinate_triples() {
        let sink = Sink::default();
        let mut log = PositionDebug::with_writer(Box::new(sink.clone()));
        // 2A 38 <3511> 01 .. <3511> 01 00 <east> <north> <elev>
        let mut body = vec![0x2A, 0x38, 0xB7, 0x1B, 0x01, 0x11, 0xA1, 0x01, 0x00, 0x00, 0xB7, 0x1B, 0x01, 0x00];
        body.extend(floats([7191.5, -11134.0, 53115.0]));
        log.observe(&frame(&body), &[], Some(3511), Direction::Server, true, 5);
        // Another entity, no player id: skipped.
        log.observe(&frame(&[0x1C, 0x37, 0x95, 0xBC, 0x01, 0x04, 0, 0, 0, 0]), &[], Some(3511), Direction::Server, true, 6);
        // Before the server is identified: skipped.
        log.observe(&frame(&body), &[], Some(3511), Direction::Server, false, 7);
        let rows = rows(&sink);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r["kind"], "player");
        assert_eq!(r["opcode"], "2a38");
        assert_eq!(r["is_player"], true);
        assert_eq!(r["candidate"], true);
        assert_eq!(r["player_id_at"], json!([2, 10]));
        assert_eq!(r["triples"][0]["at"], 14);
        assert_eq!(r["triples"][0]["v"], json!([7191.5, -11134.0, 53115.0]));
    }

    #[test]
    fn logs_cube_positions_self_spawn_and_short_outbound_frames() {
        let sink = Sink::default();
        let mut log = PositionDebug::with_writer(Box::new(sink.clone()));
        let found = [
            Observation::Spawn { id: 17376, template: 1200860 },
            Observation::Position { id: 17376, position: Position { east: -1052.04, north: -865.44, elevation: 201.27 } },
        ];
        log.observe(&frame(&[0x34, 0x36, 0xE0, 0x87, 0x01, 0, 0]), &found, None, Direction::Server, true, 1);
        log.observe(&frame(&[0x33, 0x36, 0xA2, 0x56, 0]), &[], None, Direction::Server, true, 2);
        log.observe(&frame(&[0x99; 300]), &[], Some(1), Direction::Client, false, 3);
        let rows = rows(&sink);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["kind"], "cube");
        assert_eq!(rows[0]["template"], 1200860);
        assert_eq!(rows[1]["kind"], "self_spawn");
        assert_eq!(rows[2]["kind"], "outbound");
        assert_eq!(rows[2]["hex"].as_str().unwrap().len(), MAX_OUT_HEX * 2);
    }

    #[test]
    fn stops_at_the_size_cap() {
        let sink = Sink::default();
        let mut log = PositionDebug::with_writer(Box::new(sink.clone()));
        log.written = MAX_FILE_BYTES - 10;
        log.observe(&frame(&[0x33, 0x36, 0xA2, 0x56, 0]), &[], None, Direction::Server, true, 2);
        assert!(rows(&sink).is_empty());
        assert!(log.full());
    }

    #[test]
    fn varint_offsets_skip_the_tail_of_a_longer_varint() {
        // 0x1B alone is id 27; inside B7 1B it is part of 3511.
        assert_eq!(varint_offsets(&[0x2A, 0x38, 0xB7, 0x1B, 0x1B], 27), vec![4]);
        assert_eq!(varint_offsets(&[0x2A, 0x38, 0xB7, 0x1B], 3511), vec![2]);
    }
}
