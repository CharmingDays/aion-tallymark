//! Passive AION 2 damage meter.
//!
//! Combat numbers come from the server-to-client packets the game already
//! receives. This crate never opens the game process and never reads its memory.
//!
//! The framing rules and the `04 38` damage layout are derived from
//! [A2Tools DPS Meter](https://github.com/taengu/A2Tools-DPS-Meter)
//! (GPL-3.0-or-later, Copyright (C) taengu and contributors). This program is
//! not that project: it has no log upload, no account client, and its own UI.

pub mod alerts;
pub mod capture;
pub mod combat;
pub mod decode;
pub mod demo;
pub mod framing;
pub mod game_process;
pub mod monster_names;
pub mod reassembly;
pub mod sessions;
pub mod skills;
pub mod death_debug;
pub mod spawn_debug;
pub mod state;
pub mod storage;
pub mod window_state;
pub mod target_debug;
pub mod varint;
pub mod web;

pub use combat::{Combat, Snapshot, TargetMode};
pub use state::AppState;
