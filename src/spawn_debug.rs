//! Opt-in log of summon spawn packets, so the summon owner layouts in
//! `decode` can be checked against real fights.
//!
//! Off unless `AION2DPS_DEBUG_SPAWNS=1` is set. When on, every `40/41 36`
//! summon spawn the meter already captured is appended as one JSON line to
//! `%LOCALAPPDATA%\aion2dps\spawn-debug.jsonl`, with its raw bytes and the
//! owner fields `decode` read from it. It only sees packets the meter
//! already received; nothing is ever sent to the game.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde_json::json;

use crate::decode::Event;

pub const ENV_VAR: &str = "AION2DPS_DEBUG_SPAWNS";
const FILE_NAME: &str = "spawn-debug.jsonl";
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_HEX_BYTES: usize = 4_096;

pub struct SpawnDebug {
    file: File,
    written: u64,
}

/// `%LOCALAPPDATA%\aion2dps\spawn-debug.jsonl`, or the temp directory.
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("aion2dps").join(FILE_NAME)
}

fn env_on(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

impl SpawnDebug {
    /// `None` unless the environment variable asks for the log.
    pub fn from_env() -> Option<Self> {
        if !std::env::var(ENV_VAR).is_ok_and(|value| env_on(&value)) {
            return None;
        }
        let path = default_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                let written = file.metadata().map(|m| m.len()).unwrap_or(0);
                tracing::info!("spawn debug log: {}", path.display());
                Some(Self { file, written })
            }
            Err(err) => {
                tracing::warn!("spawn debug log unavailable at {}: {err}", path.display());
                None
            }
        }
    }

    /// One line per summon spawn in this packet. Stops at the size cap.
    pub fn observe(&mut self, packet: &[u8], events: &[Event], now_ms: i64) {
        for event in events {
            let Event::SummonSpawn {
                summon,
                kind,
                owner,
                owner_name,
                cast_owner,
            } = event
            else {
                continue;
            };
            if self.written >= MAX_FILE_BYTES {
                return;
            }
            let shown = &packet[..packet.len().min(MAX_HEX_BYTES)];
            let hex: String = shown.iter().map(|b| format!("{b:02x}")).collect();
            let line = json!({
                "ts_ms": now_ms,
                "summon": summon,
                "kind": format!("{kind:02x}"),
                "owner": owner,
                "owner_name": owner_name,
                "cast_owner": cast_owner,
                "len": packet.len(),
                "hex": hex,
            })
            .to_string();
            if writeln!(self.file, "{line}").is_ok() {
                self.written += line.len() as u64 + 1;
            }
        }
    }
}
