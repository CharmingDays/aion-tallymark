//! Synthetic hits so the overlay and the dashboard can be opened without the game.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::decode::{Event, Hit};
use crate::state::AppState;

/// A spawned monster with a type, so it shows its bundled name (Fediv
/// Wraith, a boss) and can be renamed.
const TYPED_TARGET: i32 = 91;
pub const DEMO_MONSTER_TYPE: u32 = 2_090_175;
/// A named target with no spawn record, which cannot be named.
const UNTYPED_TARGET: i32 = 90;
/// How long the demo fights one target before switching.
const TARGET_MS: i64 = 20_000;

pub fn start(state: Arc<AppState>) {
    state.with_capture(|capture| {
        capture.running = true;
        capture.demo = true;
        capture.error = None;
        capture.devices = vec!["demo".to_string()];
    });
    state.with_combat(|combat| {
        combat.set_name(1, "You", true);
        combat.set_name(2, "Aria", false);
        combat.set_name(3, "Boros", false);
        combat.set_name(4, "Celine", false);
        combat.set_name(UNTYPED_TARGET, "Training Scarecrow", false);
        combat.apply(
            Event::Spawn {
                id: TYPED_TARGET,
                monster_type: DEMO_MONSTER_TYPE,
            },
            now_ms(),
        );
    });
    thread::Builder::new()
        .name("aion2dps-demo".into())
        .spawn(move || loop {
            let now = now_ms();
            let tick = (now / 180) as i32;
            let target = if (now / TARGET_MS) % 2 == 0 {
                TYPED_TARGET
            } else {
                UNTYPED_TARGET
            };
            let actor = 1 + (tick as u32 % 4) as i32;
            let skill = match actor {
                1 => 13_010_000 + (tick % 3) * 10,
                2 => 12_090_000,
                3 => 14_020_000,
                _ => 11_050_000,
            };
            let mut hit = Hit {
                actor,
                target,
                skill,
                damage: 800 + (tick % 17) * 137,
                multi_hits: if tick % 5 == 0 { 2 } else { 0 },
                multi_damage: if tick % 5 == 0 { 120 } else { 0 },
                hit_type: if tick % 3 == 0 { 3 } else { 2 },
                crit: tick % 3 == 0,
                back: tick % 4 == 0,
                front: false,
                block: false,
                parry: tick % 11 == 0,
                perfect: tick % 9 == 0,
                double: tick % 7 == 0,
                perfect_block: false,
                dot: tick % 6 == 0,
                taken: false,
                heal: if actor == 4 { 220 + (tick % 5) * 30 } else { 0 },
                miss: false,
                resist: false,
            };
            if tick % 13 == 0 {
                hit.taken = true;
                hit.actor = target;
                hit.target = 1;
                hit.skill = 1_609_010;
                hit.damage = 300 + tick % 200;
                hit.multi_hits = 0;
                hit.multi_damage = 0;
            }
            state.with_combat(|combat| combat.apply_hit(&hit, now));
            state.with_capture(|capture| {
                capture.hits = capture.hits.saturating_add(1);
                capture.frames = capture.frames.saturating_add(1);
                capture.segments = capture.segments.saturating_add(4);
            });
            if tick % 8 == 0 {
                let current = 1_000_000 - (tick as i32 % 1000) * 900;
                state.with_combat(|combat| {
                    combat.apply(
                        Event::Hp {
                            id: target,
                            current: current.max(1),
                            max: 1_000_000,
                        },
                        now,
                    );
                });
            }
            thread::sleep(Duration::from_millis(180));
        })
        .expect("demo thread");
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
