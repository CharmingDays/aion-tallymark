//! Opt-in, bounded local discovery log of selected world-entity opcodes only.
//! Raw spawn bytes can include nearby names. No chat opcodes or full stream dumps.
use crate::hidden_cubes::{Detector, Observation};
use crate::varint::try_read_varint;
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
};
pub struct CubeDiscovery {
    file: File,
    written: u64,
    rows: u32,
    /// AION2DPS_DEBUG_CUBES=2 logs every framed opcode (local file only).
    wide: bool,
}
const CAP: u64 = 20 * 1024 * 1024;
pub fn default_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("aion2dps")
        .join("cube-discovery.jsonl")
}
impl CubeDiscovery {
    pub fn from_env() -> Option<Self> {
        let wide = match std::env::var("AION2DPS_DEBUG_CUBES").ok().as_deref() {
            Some("1") => false,
            Some("2") => true,
            _ => return None,
        };
        let path = default_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok()?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        let written = file.metadata().ok()?.len();
        tracing::info!(
            "cube discovery (unverified wire fields): {}",
            path.display()
        );
        Some(Self {
            file,
            written,
            rows: 0,
            wide,
        })
    }
    pub fn observe(
        &mut self,
        packet: &[u8],
        found: &[Observation],
        detector: &Detector,
        now: i64,
        server_identified: bool,
    ) {
        let (cap, max_rows) = if self.wide { (CAP * 10, 500_000) } else { (CAP, 50_000) };
        if self.written >= cap || self.rows >= max_rows {
            return;
        }
        let mut at = 0;
        if try_read_varint(packet, &mut at).is_none() {
            return;
        }
        let Some(op) = packet.get(at..at + 2) else {
            return;
        };
        if !self.wide && !matches!(
            op,
            [0x40, 0x36] | [0x21, 0x8D] | [0x42, 0x36] | [0x2A, 0x38] | [0x33, 0x36]
        ) {
            return;
        }
        let mut id_at = at + 2;
        let id = try_read_varint(packet, &mut id_at);
        let templates: Vec<u32> = found
            .iter()
            .filter_map(|o| match o {
                Observation::Spawn { template, .. } => Some(*template),
                _ => None,
            })
            .collect();
        // Offsets are retained so a labelled approach/collect/leave run can
        // calibrate coordinates; these bytes are NOT used as trusted positions.
        let shown = &packet[at..packet.len().min(at + 512)];
        let hex: String = shown.iter().map(|b| format!("{b:02x}")).collect();
        let relative = server_identified
            .then(|| id.and_then(|id| detector.relative(id, now)))
            .flatten();
        let player_position = server_identified
            .then(|| detector.player_position(now))
            .flatten();
        let line = json!({"ts_ms":now, "opcode":format!("{:02x}{:02x}", op[0],op[1]),
            "entity_id":id,"template_candidates":templates,"wire_verified":false,
            "server_identified":server_identified,"player_entity_id":detector.player_id(),"player_position":player_position,
            "distance_m":relative.map(|r| r.0),"direction":relative.map(|r| r.1),
            "observations":found,"opcode_hex":hex,"len":packet.len(),
            "truncated":packet.len()-at > shown.len()})
        .to_string();
        if self.written + line.len() as u64 + 1 > cap {
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
    #[test]
    fn discovery_skips_other_opcodes_and_respects_size_cap() {
        let path = std::env::temp_dir().join(format!(
            "aion2dps-cube-discovery-test-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut log = CubeDiscovery {
            file,
            written: 0,
            rows: 0,
            wide: false,
        };
        let detector = Detector::default();
        log.observe(
            &crate::framing::encode_frame(&[4, 0x38, 0xE8, 7]),
            &[],
            &detector,
            0,
            false,
        );
        assert_eq!(log.rows, 0);
        let position = crate::framing::encode_frame(&[0x2A, 0x38, 0xE8, 7, 0, 0, 0, 0]);
        log.observe(&position, &[], &detector, 0, false);
        assert_eq!(log.rows, 1);
        let text = std::fs::read_to_string(&path).unwrap();
        let row: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(row["entity_id"], 1000);
        assert_eq!(row["wire_verified"], false);
        assert!(row["distance_m"].is_null());
        log.written = CAP;
        log.observe(&position, &[], &detector, 1, false);
        assert_eq!(log.rows, 1);
        drop(log);
        std::fs::remove_file(path).unwrap();
    }
}
