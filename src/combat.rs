//! Fight totals. DPS uses time the fight was actually dealing damage:
//! gaps of 7 seconds pause the clock. A target dying does not end the fight.
//! The fight ends after a configurable silence ([`ENCOUNTER_IDLE_MS`] by
//! default), counted from the last hit or the last death, whichever is later.
//! Then the damage dealt to each unit is stored and the meter starts over.
//!
//! Each damage hit of the open fight is also kept in memory, up to
//! [`SESSION_EVENT_LIMIT`], so the fight can be saved as a session.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::decode::{Event, Hit};
use crate::monster_names::{self, Names};
use crate::sessions::{self, Session};

/// Silence after the last hit or target death before the fight is archived.
pub const ENCOUNTER_IDLE_MS: i64 = 30_000;
pub const DEFAULT_RESET_SECS: u64 = (ENCOUNTER_IDLE_MS / 1000) as u64;
pub const MIN_RESET_SECS: u64 = 5;
pub const MAX_RESET_SECS: u64 = 3_600;
pub const DEFAULT_BOSS_LOCK_SECS: u64 = 10;
pub const MAX_BOSS_LOCK_SECS: u64 = 60;
/// Damage to one target that makes it a boss, whatever the npc table says.
pub const DEFAULT_BOSS_DAMAGE: i64 = 1_500_000;
pub const MIN_BOSS_DAMAGE: i64 = 1_000;
pub const MAX_BOSS_DAMAGE: i64 = 1_000_000_000_000;
/// A flag-3 removal this soon after a hit, with HP never at 0, is cleanup
/// (out of range, zone change), not a kill.
pub const KILL_GUARD_MS: i64 = 1_500;
const PAUSE_MS: i64 = 7_000;
const RECENT_LIMIT: usize = 40;
const HISTORY_LIMIT: usize = 60;
/// Unnamed player, or an unnamed entity that acted like one.
pub const UNKNOWN: &str = "Unknown";
/// Unnamed entity that was only ever hit, or that dealt NPC damage.
pub const MONSTER: &str = "Monster";
/// The label of an unnamed non-player target flagged as a boss.
pub const BOSS: &str = "Boss";
/// Ids seen acting as players. Cleared rather than grown without bound.
const PLAYER_ID_LIMIT: usize = 50_000;
/// Summons waiting for their owner's name to be seen.
const PENDING_OWNER_LIMIT: usize = 4_096;
/// Summon-to-owner links followed before giving up.
const LINK_HOPS: usize = 16;
/// Spawned entities whose monster type is kept. When full, only the ones
/// in the current fight are kept.
const SPAWN_TYPE_LIMIT: usize = 50_000;
/// Damage hits kept per fight for a saved session (about 6 MB). Past this the
/// totals keep counting, hits are no longer kept, and the session is marked
/// truncated.
pub const SESSION_EVENT_LIMIT: usize = 200_000;
/// A single heal above this is a misread packet, not a heal, and is dropped.
pub const MAX_HEAL_TICK: i32 = 5_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetMode {
    All,
    Current,
    Boss,
}

impl TargetMode {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "all" => Some(Self::All),
            "current" => Some(Self::Current),
            "boss" => Some(Self::Boss),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Skill {
    damage: i64,
    hits: i32,
    crits: i32,
    backs: i32,
    fronts: i32,
    parries: i32,
    blocks: i32,
    perfects: i32,
    doubles: i32,
    multi_hits: i32,
    /// Estimated stagger points, see [`Combat::attribute_stagger`].
    stagger: f64,
}

impl Skill {
    fn add(&mut self, other: &Skill) {
        self.stagger += other.stagger;
        self.damage += other.damage;
        self.hits += other.hits;
        self.crits += other.crits;
        self.backs += other.backs;
        self.fronts += other.fronts;
        self.parries += other.parries;
        self.blocks += other.blocks;
        self.perfects += other.perfects;
        self.doubles += other.doubles;
        self.multi_hits += other.multi_hits;
    }
}

#[derive(Debug, Clone, Default)]
struct Actor {
    skills: HashMap<(i32, bool), Skill>,
    damage: i64,
    hits: i32,
    crits: i32,
    backs: i32,
    /// Estimated stagger points on this target.
    stagger: f64,
}

impl Actor {
    fn add_stagger(&mut self, skill: i32, points: f64) {
        self.skills.entry((skill, false)).or_default().stagger += points;
        self.stagger += points;
    }

    fn add_hit(&mut self, hit: &Hit) {
        let skill_id = crate::skills::canonical_skill_id(hit.skill);
        let skill = self.skills.entry((skill_id, hit.dot)).or_default();
        let amount = i64::from(hit.total());
        skill.damage += amount;
        skill.hits += 1;
        skill.crits += i32::from(hit.crit);
        skill.backs += i32::from(hit.back);
        skill.fronts += i32::from(hit.front);
        skill.parries += i32::from(hit.parry);
        skill.blocks += i32::from(hit.block);
        skill.perfects += i32::from(hit.perfect);
        skill.doubles += i32::from(hit.double);
        if hit.multi_hits > 0 {
            skill.multi_hits += hit.multi_hits;
        }
        self.damage += amount;
        self.hits += 1;
        self.crits += i32::from(hit.crit);
        self.backs += i32::from(hit.back);
    }

    fn merge(&mut self, other: &Actor) {
        self.stagger += other.stagger;
        self.damage += other.damage;
        self.hits += other.hits;
        self.crits += other.crits;
        self.backs += other.backs;
        for (key, skill) in &other.skills {
            self.skills.entry(*key).or_default().add(skill);
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Target {
    actors: HashMap<i32, Actor>,
    damage: i64,
    /// Set when an HP record shows 0. Only a later HP above 0 clears it.
    died_ms: Option<i64>,
    /// Kept for the fight even after the entity despawns.
    monster_type: Option<u32>,
    last_hit_ms: i64,
    /// When an `8D` record showed HP 0.
    hp_zero_ms: Option<i64>,
    /// A flag-3 removal confirmed the kill.
    killed: bool,
    /// Removed by cleanup while alive. Not a kill.
    removed: bool,
    /// Crossed the boss damage threshold. Stays set for the fight.
    boss: bool,
    stagger: TargetStagger,
}

/// Stagger in this fight for one target. Points are estimates: the server
/// sends only the gauge, so each drop is shared out over the hits that
/// landed since the gauge's previous update.
#[derive(Debug, Clone, Default)]
struct TargetStagger {
    /// When the gauge broke.
    breaks: Vec<i64>,
    /// `(actor, raw skill, canonical skill)` of hits since the last gauge update.
    pending: Vec<(i32, i32, i32)>,
    /// Gauge drops with no hit to put them on.
    unattributed: f64,
    /// Every gauge drop seen in this fight.
    total: f64,
}

/// One hit's estimated share of a gauge drop, for a saved session.
#[derive(Debug, Clone, Copy)]
struct StaggerShare {
    at: i64,
    target: i32,
    actor: i32,
    skill: i32,
    points: f64,
}

/// The last gauge the server sent for an entity.
#[derive(Debug, Clone, Copy)]
struct Gauge {
    max: u32,
    remaining: u32,
    frozen: bool,
    /// Broke and not refilled yet.
    broken: bool,
}

/// Hits kept per target between two gauge updates.
const STAGGER_PENDING_LIMIT: usize = 64;
/// Gauges kept. When full, only the ones in the current fight are kept.
const GAUGE_LIMIT: usize = 4_096;
/// Distinct values remembered per skill.
const STAGGER_VALUES_PER_SKILL: usize = 32;
/// Skills with a learned stagger value.
const STAGGER_SKILL_LIMIT: usize = 4_096;
/// A second break record this soon after one is the same break.
const BREAK_DEDUPE_MS: i64 = 1_000;

impl Target {
    fn clear_death(&mut self) {
        self.died_ms = None;
        self.hp_zero_ms = None;
        self.killed = false;
        self.removed = false;
    }
}

/// What a `42 36 <id> 00 <flag>` removal means for a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    Killed,
    /// Cleanup of a live target.
    Removed,
    /// Not a monster we fought, or not a kill flag.
    Ignored,
}

/// The kill rule. Flag 3 on a damaged monster (not a summon or player) is a
/// kill, unless HP never reached 0 and the last hit was under
/// [`KILL_GUARD_MS`] ago. Flags 1 and 7 are never kills.
pub fn classify_removal(
    flag: u8,
    monster: bool,
    damage: i64,
    ms_since_last_hit: i64,
    hp_reached_zero: bool,
) -> Removal {
    if flag != 3 || !monster || damage <= 0 {
        return Removal::Ignored;
    }
    if !hp_reached_zero && ms_since_last_hit < KILL_GUARD_MS {
        return Removal::Removed;
    }
    Removal::Killed
}

#[derive(Debug, Clone)]
struct Recent {
    at: i64,
    actor: i32,
    actor_type: Option<u32>,
    skill: i32,
    amount: i32,
    crit: bool,
    dot: bool,
    taken: bool,
    heal: bool,
}

/// One heal of the open fight, for a saved session.
#[derive(Debug, Clone, Copy)]
struct HealEvent {
    at: i64,
    actor: i32,
    amount: i32,
    hot: bool,
    skill: i32,
}

/// One damage hit of the open fight, for a saved session.
#[derive(Debug, Clone, Copy)]
struct DamageEvent {
    at: i64,
    actor: i32,
    target: i32,
    amount: i32,
    crit: bool,
    dot: bool,
    skill: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillRow {
    pub id: i32,
    pub name: String,
    pub dot: bool,
    pub damage: i64,
    pub hits: i32,
    pub crits: i32,
    pub backs: i32,
    pub fronts: i32,
    pub parries: i32,
    pub blocks: i32,
    pub perfects: i32,
    pub doubles: i32,
    pub multi_hits: i32,
    /// Estimated stagger points.
    pub stagger: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerRow {
    pub id: i32,
    pub name: String,
    pub you: bool,
    pub damage: i64,
    pub dps: i64,
    pub pct: f64,
    pub hits: i32,
    pub crits: i32,
    pub backs: i32,
    pub taken: i64,
    pub heal: i64,
    pub misses: i32,
    pub resists: i32,
    /// Estimated stagger points on the counted targets.
    pub stagger: f64,
    pub skills: Vec<SkillRow>,
}

/// The current target's stagger gauge. Per-player points are estimates.
#[derive(Debug, Clone, Serialize)]
pub struct StaggerView {
    pub max: u32,
    pub remaining: u32,
    /// `max - remaining`: how full the bar is.
    pub fill: u32,
    /// State 3: the gauge does not move.
    pub frozen: bool,
    /// Broke and not refilled yet.
    pub broken: bool,
    pub breaks: usize,
    /// Break times, ms from the start of the fight.
    pub break_ms: Vec<i64>,
    /// Points shared out to players (estimated).
    pub estimated: f64,
    /// Points no hit could be matched to.
    pub unattributed: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealSkillRow {
    pub id: i32,
    pub name: String,
    /// Heal-over-time ticks.
    pub hot: bool,
    pub heal: i64,
    pub ticks: i32,
}

/// A player who healed in this fight, with summons folded into the owner.
#[derive(Debug, Clone, Serialize)]
pub struct HealerRow {
    pub id: i32,
    pub name: String,
    pub you: bool,
    pub heal: i64,
    pub hps: i64,
    /// Share of the group's healing, 0 to 1.
    pub pct: f64,
    pub skills: Vec<HealSkillRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentRow {
    pub ago_ms: i64,
    pub actor_id: i32,
    pub actor: String,
    pub you: bool,
    pub skill: i32,
    pub skill_name: String,
    pub amount: i32,
    pub crit: bool,
    pub dot: bool,
    pub taken: bool,
    pub heal: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnitHistory {
    pub ended_ms: i64,
    pub id: i32,
    pub name: String,
    pub damage: i64,
    pub dps: i64,
    pub duration_ms: i64,
    /// The current name for this type (the user's, else the bundled one)
    /// replaces `name` when history is read.
    #[serde(skip)]
    pub monster_type: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SettingsView {
    pub reset_after_secs: u64,
    /// 0 turns the post-death lock off.
    pub boss_lock_secs: u64,
    pub boss_damage_threshold: i64,
    /// Keep the view on a boss while it is being fought.
    pub boss_engage_lock: bool,
    /// Desktop shell: open the overlay when AION 2 starts (tray wait).
    /// Combat ignores this; AppState fills it for the live view.
    pub open_when_aion2: bool,
    pub start_with_windows: bool,
    pub pop_up_on_boss: bool,
    /// Desktop shell: overlay body click-through (top bar stays interactive).
    /// Combat ignores this; AppState fills it for the live view.
    pub overlay_click_through: bool,
    /// AppState-owned passive cube alert preference.
    pub hidden_cube_alerts: bool,
}

/// The live view is held on a boss that just died.
#[derive(Debug, Clone, Serialize)]
pub struct LockView {
    pub remaining_ms: i64,
    pub secs: u64,
    pub boss: String,
    /// Damage in the fight recorded behind the lock, not shown yet.
    pub pending_damage: i64,
    /// Held on a boss that is still being fought (no countdown yet).
    pub engaged: bool,
}

/// A frozen copy of the fight in which a boss died.
#[derive(Debug, Clone)]
struct Lock {
    view: Box<Combat>,
    boss: i32,
    died_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub status: &'static str,
    pub mode: TargetMode,
    pub elapsed_ms: i64,
    pub active_ms: i64,
    /// Damage the whole group dealt to the targets on screen.
    pub encounter_damage: i64,
    pub encounter_dps: i64,
    /// Time left before the fight is archived, while one is open.
    pub reset_in_ms: Option<i64>,
    pub target_id: Option<i32>,
    pub target_label: String,
    /// Monster type of the target, from its spawn record. Not a label.
    pub target_type: Option<u32>,
    /// The user's name for the target's type.
    pub target_user_name: Option<String>,
    /// The bundled name for the target's type, shown when the user has none.
    pub target_bundled_name: Option<String>,
    /// From the bundled table for the target's type.
    pub target_boss: bool,
    pub target_dummy: bool,
    /// The target died: HP 0, or a confirmed kill.
    pub target_defeated: bool,
    pub target_hp: Option<f64>,
    /// Targets counted in the totals. Current mode keeps the ones that died.
    pub targets_shown: usize,
    pub players: Vec<PlayerRow>,
    pub recent: Vec<RecentRow>,
    /// Healing in the whole fight, not only on the counted targets.
    pub encounter_heal: i64,
    pub encounter_hps: i64,
    /// By heal, largest first.
    pub healers: Vec<HealerRow>,
    /// Set while the view is held on a dead boss.
    pub lock: Option<LockView>,
    /// The target's stagger gauge, when the server sent one.
    pub stagger: Option<StaggerView>,
}

#[derive(Debug, Clone)]
pub struct Combat {
    targets: HashMap<i32, Target>,
    names: HashMap<i32, String>,
    /// Monster type per spawned entity. A new spawn on the same id replaces
    /// it; a despawn removes it.
    spawn_types: HashMap<i32, u32>,
    /// Names the user gave to monster types.
    monster_names: Names,
    links: HashMap<i32, i32>,
    /// Summons whose spawn named an owner not seen yet, by that name.
    pending_owners: HashMap<i32, String>,
    /// Entities that dealt player damage or took NPC damage.
    players: HashSet<i32>,
    hp: HashMap<i32, (i32, i32)>,
    taken: HashMap<i32, i64>,
    heal: HashMap<i32, i64>,
    /// Heal and ticks per (actor, skill, hot).
    heal_skills: HashMap<(i32, i32, bool), (i64, i32)>,
    heal_events: Vec<HealEvent>,
    misses: HashMap<i32, i32>,
    resists: HashMap<i32, i32>,
    recent: VecDeque<Recent>,
    self_id: Option<i32>,
    /// How many `4A 36` records in a row have named the same entity.
    you_streak: Option<(i32, u32)>,
    last_target: Option<i32>,
    mode: TargetMode,
    first_ms: Option<i64>,
    last_ms: Option<i64>,
    /// Latest death of a target in this fight.
    death_ms: Option<i64>,
    active_ms: i64,
    reset_after_ms: i64,
    history: VecDeque<UnitHistory>,
    events: Vec<DamageEvent>,
    /// Set once the fight had more hits than [`SESSION_EVENT_LIMIT`].
    events_truncated: bool,
    /// Ids that a summon spawn claimed. Never kills or bosses.
    summons: HashSet<i32>,
    boss_damage: i64,
    lock_secs: u64,
    /// A boss died in this fight: `(id, died at)`. Taken by [`Combat::apply`].
    boss_death: Option<(i32, i64)>,
    locked: Option<Box<Lock>>,
    /// Keep the view on a boss while it is being fought.
    engage_lock: bool,
    /// A boss the user unlocked by hand: not held again in this fight.
    released: Option<i32>,
    /// In the frozen view of a post-death lock: the boss it shows.
    pinned: Option<i32>,
    /// Latest stagger gauge per entity. Kept across fights.
    gauges: HashMap<i32, Gauge>,
    /// Gauge drops seen after exactly one hit: raw skill id to
    /// `(points, times seen)`. Kept across fights.
    stagger_learned: HashMap<i32, Vec<(u32, u32)>>,
    /// Every stagger share of the open fight, for a saved session.
    stagger_shares: Vec<StaggerShare>,
}

impl Default for Combat {
    fn default() -> Self {
        Self::new()
    }
}

impl Combat {
    pub fn new() -> Self {
        Self {
            targets: HashMap::new(),
            names: HashMap::new(),
            spawn_types: HashMap::new(),
            monster_names: Names::new(),
            links: HashMap::new(),
            pending_owners: HashMap::new(),
            players: HashSet::new(),
            hp: HashMap::new(),
            taken: HashMap::new(),
            heal: HashMap::new(),
            heal_skills: HashMap::new(),
            heal_events: Vec::new(),
            misses: HashMap::new(),
            resists: HashMap::new(),
            recent: VecDeque::new(),
            self_id: None,
            you_streak: None,
            last_target: None,
            mode: TargetMode::Current,
            first_ms: None,
            last_ms: None,
            death_ms: None,
            active_ms: 0,
            reset_after_ms: ENCOUNTER_IDLE_MS,
            history: VecDeque::new(),
            events: Vec::new(),
            events_truncated: false,
            summons: HashSet::new(),
            boss_damage: DEFAULT_BOSS_DAMAGE,
            lock_secs: DEFAULT_BOSS_LOCK_SECS,
            boss_death: None,
            locked: None,
            engage_lock: true,
            released: None,
            pinned: None,
            gauges: HashMap::new(),
            stagger_learned: HashMap::new(),
            stagger_shares: Vec::new(),
        }
    }

    pub fn set_mode(&mut self, mode: TargetMode) {
        self.mode = mode;
        if let Some(lock) = self.locked.as_mut() {
            lock.view.mode = mode;
        }
    }

    pub fn settings(&self) -> SettingsView {
        SettingsView {
            reset_after_secs: (self.reset_after_ms / 1000) as u64,
            boss_lock_secs: self.lock_secs,
            boss_damage_threshold: self.boss_damage,
            boss_engage_lock: self.engage_lock,
            open_when_aion2: false,
            start_with_windows: false,
            pop_up_on_boss: true,
            overlay_click_through: true,
            hidden_cube_alerts: true,
        }
    }

    /// 0 turns the lock off and releases one that is running.
    pub fn set_boss_lock_secs(&mut self, secs: u64, now: i64) -> bool {
        if secs > MAX_BOSS_LOCK_SECS {
            return false;
        }
        self.lock_secs = secs;
        self.expire_lock(now);
        true
    }

    pub fn set_boss_damage_threshold(&mut self, damage: i64) -> bool {
        if !(MIN_BOSS_DAMAGE..=MAX_BOSS_DAMAGE).contains(&damage) {
            return false;
        }
        self.boss_damage = damage;
        if let Some(lock) = self.locked.as_mut() {
            lock.view.boss_damage = damage;
        }
        true
    }

    /// Off: hitting another monster takes the view as before. Applies live.
    pub fn set_boss_engage_lock(&mut self, on: bool) {
        self.engage_lock = on;
        if let Some(lock) = self.locked.as_mut() {
            lock.view.engage_lock = on;
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    /// The live view is held on a boss that is still being fought.
    pub fn is_engaged(&self) -> bool {
        self.locked.is_none() && self.engaged_boss().is_some()
    }

    /// The boss the live fight is held on: flagged a boss (table or damage
    /// threshold), took damage in this fight, not dead, hit within the idle
    /// window of the fight's latest hit, and not unlocked by hand. The most
    /// damaged one wins.
    fn engaged_boss(&self) -> Option<i32> {
        if !self.engage_lock {
            return None;
        }
        let last = self.last_ms?;
        self.targets
            .iter()
            .filter(|(id, target)| {
                target.damage > 0
                    && target.died_ms.is_none()
                    && Some(**id) != self.released
                    && last.saturating_sub(target.last_hit_ms) < self.reset_after_ms
                    && self.target_is_boss(**id)
            })
            .max_by_key(|(id, target)| (target.damage, -**id))
            .map(|(id, _)| *id)
    }

    /// A boss the view is held on: the dead one in a frozen view, else the
    /// one being fought.
    fn held_target(&self) -> Option<i32> {
        self.pinned
            .filter(|id| self.targets.contains_key(id))
            .or_else(|| self.engaged_boss())
    }

    /// End the lock early. The view moves to whatever was recorded behind it.
    /// With no post-death lock, releases the boss being fought instead.
    pub fn unlock(&mut self) -> bool {
        match self.locked.take() {
            Some(lock) => {
                self.release(lock);
                true
            }
            None => match self.engaged_boss() {
                Some(id) => {
                    self.released = Some(id);
                    true
                }
                None => false,
            },
        }
    }

    fn lock_until(&self, lock: &Lock) -> i64 {
        lock.died_ms
            .saturating_add((self.lock_secs as i64).saturating_mul(1000))
    }

    fn expire_lock(&mut self, now: i64) {
        let expired = self
            .locked
            .as_ref()
            .is_some_and(|lock| self.lock_secs == 0 || now >= self.lock_until(lock));
        if expired {
            self.unlock();
        }
    }

    /// The frozen fight goes to history like any finished fight.
    fn release(&mut self, lock: Box<Lock>) {
        let view = lock.view;
        let end = view.last_ms.unwrap_or(lock.died_ms);
        let rows = view.archive_rows(end);
        self.push_history(rows);
    }

    /// Freeze the fight a boss died in. The live side starts empty and
    /// records whatever comes next behind the lock.
    fn begin_lock(&mut self, boss: i32, died_ms: i64) {
        if self.lock_secs == 0 || !self.targets.contains_key(&boss) {
            return;
        }
        // Another boss died in the fight behind the lock: the latest wins.
        if let Some(old) = self.locked.take() {
            self.release(old);
        }
        let mut view = self.clone();
        view.history.clear();
        view.locked = None;
        view.boss_death = None;
        view.pinned = Some(boss);
        view.reset_after_ms = i64::MAX / 4;
        self.clear_fight();
        self.locked = Some(Box::new(Lock {
            view: Box::new(view),
            boss,
            died_ms,
        }));
    }

    /// The locked boss came back (a stray HP 0). If nothing happened behind
    /// the lock, the fight carries on as if it had never been frozen.
    fn unfreeze(&mut self, lock: Box<Lock>) {
        if self.encounter_damage() > 0 {
            self.release(lock);
            return;
        }
        let history = std::mem::take(&mut self.history);
        let (reset, secs, damage) = (self.reset_after_ms, self.lock_secs, self.boss_damage);
        *self = *lock.view;
        self.history = history;
        self.reset_after_ms = reset;
        self.lock_secs = secs;
        self.boss_damage = damage;
        self.locked = None;
        self.boss_death = None;
        self.pinned = None;
    }

    pub fn set_reset_after_secs(&mut self, secs: u64) -> bool {
        if !(MIN_RESET_SECS..=MAX_RESET_SECS).contains(&secs) {
            return false;
        }
        self.reset_after_ms = (secs as i64).saturating_mul(1000);
        true
    }

    pub fn history(&self) -> Vec<UnitHistory> {
        self.history
            .iter()
            .map(|row| {
                let mut row = row.clone();
                if let Some(name) = row.monster_type.and_then(|t| self.type_name(t)) {
                    row.name = name.to_string();
                }
                row
            })
            .collect()
    }

    pub fn monster_names(&self) -> &Names {
        &self.monster_names
    }

    pub fn set_monster_names(&mut self, names: Names) {
        if let Some(lock) = self.locked.as_mut() {
            lock.view.monster_names = names.clone();
        }
        self.monster_names = names;
    }

    /// `None` clears the name.
    pub fn set_monster_name(&mut self, monster_type: u32, name: Option<String>) {
        if let Some(lock) = self.locked.as_mut() {
            lock.view.set_monster_name(monster_type, name.clone());
        }
        match name {
            Some(name) => self.monster_names.insert(monster_type, name),
            None => self.monster_names.remove(&monster_type),
        };
    }

    /// The type from the entity's spawn, or the one its target kept after it
    /// despawned. A summon shown under its owner has none.
    pub fn monster_type(&self, id: i32) -> Option<u32> {
        if self.links.contains_key(&id) {
            return None;
        }
        self.spawn_types
            .get(&id)
            .copied()
            .or_else(|| self.targets.get(&id).and_then(|target| target.monster_type))
    }

    fn note_spawn(&mut self, id: i32, monster_type: u32) {
        if self.spawn_types.len() >= SPAWN_TYPE_LIMIT && !self.spawn_types.contains_key(&id) {
            let targets = &self.targets;
            self.spawn_types.retain(|id, _| targets.contains_key(id));
        }
        self.spawn_types.insert(id, monster_type);
        // The id belongs to a new monster now, not to whoever had it before.
        self.names.remove(&id);
        self.players.remove(&id);
        self.summons.remove(&id);
        if let Some(target) = self.targets.get_mut(&id) {
            target.monster_type = Some(monster_type);
            // A respawn on the id: whatever died there is gone.
            target.clear_death();
        }
    }

    /// A player spawn on an id that was a monster.
    fn forget_spawn_type(&mut self, id: i32) {
        self.spawn_types.remove(&id);
        if let Some(target) = self.targets.get_mut(&id) {
            target.monster_type = None;
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Silence before the fight resets. A fight with a boss in it is held
    /// for at least the boss lock time, so a kill the packets never confirm
    /// still keeps the boss fight on screen as long as a confirmed one.
    fn idle_reset_ms(&self) -> i64 {
        let boss_fight = self.lock_secs > 0
            && self
                .targets
                .iter()
                .any(|(id, target)| target.damage > 0 && self.target_is_boss(*id));
        if boss_fight {
            self.reset_after_ms
                .max((self.lock_secs as i64).saturating_mul(1000))
        } else {
            self.reset_after_ms
        }
    }

    /// Archive the current fight once silence reaches the reset setting.
    /// The fight behind a lock idles out on its own clock.
    pub fn tick(&mut self, now: i64) {
        self.expire_lock(now);
        let Some(idle) = self.idle_since() else {
            return;
        };
        if now.saturating_sub(idle) >= self.idle_reset_ms() {
            self.finish(now);
        }
    }

    pub fn set_name(&mut self, id: i32, name: impl Into<String>, is_self: bool) {
        self.remember_name(id, name.into(), is_self, true);
    }

    /// `trusted` names come from a spawn record. A guessed name fills a gap
    /// and does not replace one. Rows look names up when drawn, so a name
    /// that arrives after the damage still lands on the existing row.
    fn remember_name(&mut self, id: i32, name: String, is_self: bool, trusted: bool) {
        if is_self {
            self.self_id = Some(id);
            self.you_streak = None;
        }
        if name.is_empty() {
            return;
        }
        if trusted {
            self.forget_reused_link(id, &name);
            self.forget_spawn_type(id);
        }
        if trusted || !self.names.contains_key(&id) {
            self.link_pending(id, &name);
            self.names.insert(id, name);
            self.backfill_history(id);
        }
    }

    /// A summon spawn reuses its id, so whatever that id was is gone. The
    /// owner is the parent key, else the one player with the spawn's name,
    /// else the spirit or spell caster. A monster is never an owner: a
    /// monster's summon names the player it targets.
    fn note_summon_spawn(
        &mut self,
        summon: i32,
        owner: Option<i32>,
        owner_name: Option<String>,
        cast_owner: Option<i32>,
    ) {
        self.links.remove(&summon);
        self.pending_owners.remove(&summon);
        self.names.remove(&summon);
        self.summons.insert(summon);
        if let Some(target) = self.targets.get_mut(&summon) {
            target.clear_death();
        }
        let is_monster = |combat: &Self, id: i32| combat.spawn_types.contains_key(&id);
        if owner.is_some_and(|id| is_monster(self, id))
            || cast_owner.is_some_and(|id| is_monster(self, id))
        {
            return;
        }
        if let Some(owner) = owner {
            if let Some(name) = owner_name {
                self.remember_name(owner, name, false, false);
            }
            self.link(summon, owner);
            return;
        }
        if let Some(name) = owner_name {
            match self.player_named(&name, summon) {
                Some(owner) => {
                    self.link(summon, owner);
                    return;
                }
                None if cast_owner.is_none() => {
                    if self.pending_owners.len() >= PENDING_OWNER_LIMIT {
                        self.pending_owners.clear();
                    }
                    self.pending_owners.insert(summon, name);
                    return;
                }
                None => {}
            }
        }
        if let Some(owner) = cast_owner {
            self.link(summon, owner);
        }
    }

    /// The one entity with this name that is not itself a summon.
    fn player_named(&self, name: &str, not: i32) -> Option<i32> {
        let mut found = self
            .names
            .iter()
            .filter(|(id, seen)| **id != not && seen.as_str() == name && !self.links.contains_key(id))
            .map(|(id, _)| *id);
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }

    /// Summons that spawned under this name before its owner was seen.
    fn link_pending(&mut self, id: i32, name: &str) {
        let waiting: Vec<i32> = self
            .pending_owners
            .iter()
            .filter(|(summon, seen)| **summon != id && seen.as_str() == name)
            .map(|(summon, _)| *summon)
            .collect();
        for summon in waiting {
            self.pending_owners.remove(&summon);
            self.link(summon, id);
        }
    }

    /// Never a loop: an owner that leads back to the summon is ignored.
    fn link(&mut self, summon: i32, owner: i32) {
        if summon != owner && self.owner_of(owner) != summon {
            self.links.insert(summon, owner);
        }
    }

    /// Follows summon to owner, so a summon's summon lands on the player.
    fn owner_of(&self, id: i32) -> i32 {
        let mut current = id;
        for _ in 0..LINK_HOPS {
            match self.links.get(&current) {
                Some(next) if *next != id => current = *next,
                _ => break,
            }
        }
        current
    }

    /// A spawn under its own character name means the id is a player now,
    /// not the summon it was linked as before the id was reused.
    fn forget_reused_link(&mut self, id: i32, name: &str) {
        let Some(owner) = self.links.get(&id) else {
            return;
        };
        if self.names.get(owner).map(String::as_str) != Some(name) {
            self.links.remove(&id);
        }
    }

    /// Pulls archived before the unit's name was seen.
    fn backfill_history(&mut self, id: i32) {
        let label = self.label(id);
        if is_fallback(&label) {
            return;
        }
        for row in self
            .history
            .iter_mut()
            .filter(|row| row.id == id && is_fallback(&row.name))
        {
            row.name = label.clone();
        }
    }

    pub fn reset(&mut self, now: i64) {
        self.unlock();
        self.finish(now);
    }

    /// While locked, hits on the frozen fight's targets go to it only; new
    /// targets go to the live fight behind the lock; everything else (names,
    /// spawns, HP, removals) goes to both.
    pub fn apply(&mut self, event: Event, now: i64) {
        self.expire_lock(now);
        if let Some(mut lock) = self.locked.take() {
            let hit = matches!(event, Event::Hit(_));
            let to_view = match &event {
                Event::Hit(h) => lock.view.targets.contains_key(&h.target),
                _ => true,
            };
            if to_view {
                lock.view.apply_event(event.clone(), now);
                // A later boss death in the frozen fight restarts the timer.
                if let Some((id, at)) = lock.view.boss_death.take() {
                    lock.boss = id;
                    lock.died_ms = at;
                    lock.view.pinned = Some(id);
                }
            }
            let alive = lock
                .view
                .targets
                .get(&lock.boss)
                .is_some_and(|target| target.died_ms.is_none());
            if alive {
                self.unfreeze(lock);
            } else {
                self.locked = Some(lock);
            }
            if hit && to_view {
                return;
            }
        }
        self.apply_event(event, now);
        if let Some((id, at)) = self.boss_death.take() {
            self.begin_lock(id, at);
        }
    }

    fn apply_event(&mut self, event: Event, now: i64) {
        match event {
            Event::Hit(hit) => self.apply_hit(&hit, now),
            Event::Link { summon, owner } => {
                self.links.insert(summon, owner);
            }
            Event::Name {
                id,
                name,
                is_self,
                trusted,
            } => self.remember_name(id, name, is_self, trusted),
            Event::You { id } => self.note_you(id),
            Event::Hp { id, current, max } => self.apply_hp(id, current, max, now),
            Event::Spawn { id, monster_type } => self.note_spawn(id, monster_type),
            Event::Despawn { id } => {
                self.spawn_types.remove(&id);
            }
            Event::LiveHp { id, hp } => self.apply_live_hp(id, hp, now),
            Event::Removed { id, flag } => self.apply_removed(id, flag, now),
            Event::Stagger {
                id,
                max,
                remaining,
                frozen,
            } => self.apply_stagger(id, max, remaining, frozen, now),
            Event::StaggerBreak { id } => self.apply_stagger_break(id, now),
            Event::SummonSpawn {
                summon,
                owner,
                owner_name,
                cast_owner,
                ..
            } => self.note_summon_spawn(summon, owner, owner_name, cast_owner),
        }
    }

    fn apply_hp(&mut self, id: i32, current: i32, max: i32, now: i64) {
        self.hp.insert(id, (current, max));
        let Some(target) = self.targets.get_mut(&id) else {
            return;
        };
        if current > 0 {
            target.died_ms = None;
        } else {
            self.mark_died(id, now);
        }
    }

    /// Not a summon and not a player: something a kill can count for.
    fn monster_like(&self, id: i32) -> bool {
        !self.summons.contains(&id) && !self.is_player(id)
    }

    fn mark_died(&mut self, id: i32, now: i64) {
        let Some(target) = self.targets.get_mut(&id) else {
            return;
        };
        if target.died_ms.is_some() {
            return;
        }
        target.died_ms = Some(now);
        self.death_ms = Some(self.death_ms.map_or(now, |prev| prev.max(now)));
        if self.target_is_boss(id) {
            self.boss_death = Some((id, now));
        }
    }

    /// `8D` HP. 0 is the death; a later HP above 0 before the kill is
    /// confirmed means it was not one.
    fn apply_live_hp(&mut self, id: i32, hp: u32, now: i64) {
        if !self.monster_like(id) {
            return;
        }
        let Some(target) = self.targets.get_mut(&id) else {
            return;
        };
        if target.damage <= 0 {
            return;
        }
        if hp > 0 {
            if target.hp_zero_ms.is_some() && !target.killed {
                target.hp_zero_ms = None;
                target.died_ms = None;
            }
            return;
        }
        if target.hp_zero_ms.is_none() {
            target.hp_zero_ms = Some(now);
        }
        self.mark_died(id, now);
    }

    fn apply_removed(&mut self, id: i32, flag: u8, now: i64) {
        let Some(target) = self.targets.get(&id) else {
            return;
        };
        let zero = target.hp_zero_ms.is_some();
        let monster = self.monster_like(id) && (zero || self.monster_type(id).is_some());
        let since = now.saturating_sub(target.last_hit_ms);
        match classify_removal(flag, monster, target.damage, since, zero) {
            Removal::Killed => {
                if let Some(target) = self.targets.get_mut(&id) {
                    target.killed = true;
                }
                self.mark_died(id, now);
            }
            Removal::Removed => {
                if let Some(target) = self.targets.get_mut(&id) {
                    target.removed = true;
                }
            }
            Removal::Ignored => {}
        }
    }

    /// `05 E0` gauge. A drop is shared out over the hits since the last update.
    fn apply_stagger(&mut self, id: i32, max: u32, remaining: u32, frozen: bool, now: i64) {
        if self.gauges.len() >= GAUGE_LIMIT && !self.gauges.contains_key(&id) {
            let targets = &self.targets;
            self.gauges.retain(|id, _| targets.contains_key(id));
            if self.gauges.len() >= GAUGE_LIMIT {
                self.gauges.clear();
            }
        }
        let prev = self.gauges.get(&id).filter(|g| !g.broken).map(|g| g.remaining);
        self.gauges.insert(
            id,
            Gauge {
                max,
                remaining,
                frozen,
                broken: false,
            },
        );
        match prev {
            Some(prev) if remaining < prev => self.attribute_stagger(id, prev - remaining, now),
            _ => {
                if let Some(target) = self.targets.get_mut(&id) {
                    target.stagger.pending.clear();
                }
            }
        }
    }

    /// `05 E0 <id> 00 03 02`. What was left on the gauge goes to the last hits.
    fn apply_stagger_break(&mut self, id: i32, now: i64) {
        let left = match self.gauges.get_mut(&id) {
            Some(gauge) if !gauge.broken => {
                let left = gauge.remaining;
                gauge.remaining = 0;
                gauge.broken = true;
                gauge.frozen = false;
                left
            }
            Some(_) => 0,
            None => {
                self.gauges.insert(
                    id,
                    Gauge {
                        max: 0,
                        remaining: 0,
                        frozen: false,
                        broken: true,
                    },
                );
                0
            }
        };
        if left > 0 {
            self.attribute_stagger(id, left, now);
        } else if let Some(target) = self.targets.get_mut(&id) {
            target.stagger.pending.clear();
        }
        if let Some(target) = self.targets.get_mut(&id) {
            let repeat = target
                .stagger
                .breaks
                .last()
                .is_some_and(|at| now.saturating_sub(*at) < BREAK_DEDUPE_MS);
            if !repeat {
                target.stagger.breaks.push(now);
            }
        }
    }

    /// The skill's most common single-hit drop.
    fn learned_stagger(&self, skill: i32) -> Option<f64> {
        self.stagger_learned
            .get(&skill)?
            .iter()
            .max_by_key(|(value, seen)| (*seen, std::cmp::Reverse(*value)))
            .map(|(value, _)| f64::from(*value))
    }

    fn learn_stagger(&mut self, skill: i32, points: u32) {
        if self.stagger_learned.len() >= STAGGER_SKILL_LIMIT
            && !self.stagger_learned.contains_key(&skill)
        {
            self.stagger_learned.clear();
        }
        let values = self.stagger_learned.entry(skill).or_default();
        if let Some((_, seen)) = values.iter_mut().find(|(value, _)| *value == points) {
            *seen = seen.saturating_add(1);
        } else if values.len() < STAGGER_VALUES_PER_SKILL {
            values.push((points, 1));
        }
    }

    /// One hit takes the whole drop and teaches its skill's value. Several
    /// share it by their skills' learned values (an unknown skill counts as
    /// the average known one), or evenly when none is known.
    fn attribute_stagger(&mut self, id: i32, drop: u32, now: i64) {
        let Some(target) = self.targets.get_mut(&id) else {
            return;
        };
        let pending = std::mem::take(&mut target.stagger.pending);
        let points = f64::from(drop);
        target.stagger.total += points;
        if pending.is_empty() {
            target.stagger.unattributed += points;
            return;
        }
        if let [(_, raw, _)] = pending.as_slice() {
            self.learn_stagger(*raw, drop);
        }
        let known: Vec<Option<f64>> = pending
            .iter()
            .map(|(_, raw, _)| self.learned_stagger(*raw))
            .collect();
        let found: Vec<f64> = known.iter().flatten().copied().collect();
        let fallback = if found.is_empty() {
            1.0
        } else {
            found.iter().sum::<f64>() / found.len() as f64
        };
        let weights: Vec<f64> = known
            .iter()
            .map(|w| w.unwrap_or(fallback).max(0.0))
            .collect();
        let sum: f64 = weights.iter().sum();
        let Some(target) = self.targets.get_mut(&id) else {
            return;
        };
        for ((actor, _, skill), weight) in pending.iter().zip(&weights) {
            let share = if sum > 0.0 {
                points * weight / sum
            } else {
                points / pending.len() as f64
            };
            target.actors.entry(*actor).or_default().add_stagger(*skill, share);
            if self.stagger_shares.len() < SESSION_EVENT_LIMIT {
                self.stagger_shares.push(StaggerShare {
                    at: now,
                    target: id,
                    actor: *actor,
                    skill: *skill,
                    points: share,
                });
            }
        }
    }

    fn stagger_view(&self, id: i32) -> Option<StaggerView> {
        let gauge = self.gauges.get(&id).copied();
        let target = self.targets.get(&id);
        let stagger = target.map(|t| &t.stagger);
        if gauge.is_none() && stagger.map_or(true, |s| s.breaks.is_empty()) {
            return None;
        }
        let gauge = gauge.unwrap_or(Gauge {
            max: 0,
            remaining: 0,
            frozen: false,
            broken: false,
        });
        let start = self.first_ms.unwrap_or(0);
        let total = stagger.map_or(0.0, |s| s.total);
        let unattributed = stagger.map_or(0.0, |s| s.unattributed);
        Some(StaggerView {
            max: gauge.max,
            remaining: gauge.remaining,
            fill: gauge.max.saturating_sub(gauge.remaining),
            frozen: gauge.frozen,
            broken: gauge.broken,
            breaks: stagger.map_or(0, |s| s.breaks.len()),
            break_ms: stagger
                .map(|s| s.breaks.iter().map(|at| at.saturating_sub(start).max(0)).collect())
                .unwrap_or_default(),
            estimated: round1(total - unattributed),
            unattributed: round1(unattributed),
        })
    }

    /// The npc table says boss, or the target took the threshold in damage.
    fn target_is_boss(&self, id: i32) -> bool {
        let Some(target) = self.targets.get(&id) else {
            return false;
        };
        target.boss
            || target.damage >= self.boss_damage
            || self
                .monster_type(id)
                .and_then(monster_names::bundled)
                .is_some_and(|b| b.boss)
    }

    /// Boss detections independent of the selected display mode. Include the
    /// held fight so a death lock does not cause the same boss to be detected twice.
    pub fn boss_detection_keys(&self) -> Vec<(i64, i32)> {
        let mut keys: Vec<_> = self.targets.keys()
            .filter(|id| self.target_is_boss(**id))
            .map(|id| (self.first_ms.unwrap_or(0), *id))
            .collect();
        if let Some(lock) = &self.locked {
            keys.extend(lock.view.boss_detection_keys());
        }
        keys
    }

    pub fn apply_hit(&mut self, hit: &Hit, now: i64) {
        if hit.miss || hit.resist {
            if self.actor_known(hit.actor) {
                if hit.miss {
                    *self.misses.entry(hit.actor).or_insert(0) += 1;
                }
                if hit.resist {
                    *self.resists.entry(hit.actor).or_insert(0) += 1;
                }
            }
            return;
        }

        // A DoT or HoT record naming a spawned monster as its source is not
        // player damage or healing (seen with player skill ids on bosses).
        if hit.dot && !hit.taken && self.is_npc(hit.actor) {
            return;
        }

        if hit.taken {
            if hit.total() > 0 {
                self.note_player(hit.target);
                *self.taken.entry(hit.target).or_insert(0) += i64::from(hit.total());
                self.note(now, hit, true, false);
            }
            return;
        }

        if plausible_heal(hit.heal) {
            // `05 38` names its source unreliably: a tick never makes a player.
            if !hit.dot {
                self.note_player(hit.actor);
            }
            self.record_heal(now, hit);
            if hit.total() == 0 {
                self.note(now, hit, false, true);
                return;
            }
        }

        if hit.total() <= 0 {
            return;
        }

        self.touch(now);
        if !hit.dot {
            self.note_player(hit.actor);
        }
        let spawned = self.spawn_types.get(&hit.target).copied();
        let target = self.targets.entry(hit.target).or_default();
        if spawned.is_some() {
            target.monster_type = spawned;
        }
        target.damage += i64::from(hit.total());
        target.last_hit_ms = now;
        if target.damage >= self.boss_damage {
            target.boss = true;
        }
        target.actors.entry(hit.actor).or_default().add_hit(hit);
        if !hit.dot && target.stagger.pending.len() < STAGGER_PENDING_LIMIT {
            let canonical = crate::skills::canonical_skill_id(hit.skill);
            target.stagger.pending.push((hit.actor, hit.skill, canonical));
        }
        // A late tick on a dead target does not take Current away from a live one.
        if target.died_ms.is_none() || self.last_target.is_none() {
            self.last_target = Some(hit.target);
        }
        self.record(now, hit);
        self.note(now, hit, false, false);
    }

    fn record_heal(&mut self, now: i64, hit: &Hit) {
        let skill = crate::skills::canonical_skill_id(hit.skill);
        *self.heal.entry(hit.actor).or_insert(0) += i64::from(hit.heal);
        let row = self.heal_skills.entry((hit.actor, skill, hit.dot)).or_insert((0, 0));
        row.0 += i64::from(hit.heal);
        row.1 += 1;
        if self.heal_events.len() < SESSION_EVENT_LIMIT {
            self.heal_events.push(HealEvent {
                at: now,
                actor: hit.actor,
                amount: hit.heal,
                hot: hit.dot,
                skill,
            });
        } else {
            self.events_truncated = true;
        }
    }

    /// Heals per player, summons folded into their owner, largest first.
    fn healers(&self, active_ms: i64) -> Vec<HealerRow> {
        let mut by_owner: HashMap<i32, HashMap<(i32, bool), (i64, i32)>> = HashMap::new();
        for ((actor, skill, hot), (heal, ticks)) in &self.heal_skills {
            let row = by_owner
                .entry(self.owner_of(*actor))
                .or_default()
                .entry((*skill, *hot))
                .or_insert((0, 0));
            row.0 += heal;
            row.1 += ticks;
        }
        let group: i64 = by_owner.values().flat_map(|skills| skills.values()).map(|r| r.0).sum();
        let mut rows: Vec<HealerRow> = by_owner
            .into_iter()
            .map(|(id, skills)| {
                let heal: i64 = skills.values().map(|r| r.0).sum();
                let mut skills: Vec<HealSkillRow> = skills
                    .into_iter()
                    .map(|((skill, hot), (heal, ticks))| HealSkillRow {
                        id: skill,
                        name: crate::skills::skill_name(skill),
                        hot,
                        heal,
                        ticks,
                    })
                    .collect();
                skills.sort_by(|a, b| b.heal.cmp(&a.heal).then(a.id.cmp(&b.id)));
                HealerRow {
                    id,
                    name: self.label(id),
                    you: self.self_id == Some(id),
                    heal,
                    hps: per_second(heal, active_ms),
                    pct: if group > 0 { heal as f64 / group as f64 } else { 0.0 },
                    skills,
                }
            })
            .collect();
        rows.sort_by(|a, b| b.heal.cmp(&a.heal).then(a.name.cmp(&b.name)));
        rows
    }

    fn record(&mut self, now: i64, hit: &Hit) {
        if self.events.len() >= SESSION_EVENT_LIMIT {
            self.events_truncated = true;
            return;
        }
        self.events.push(DamageEvent {
            at: now,
            actor: hit.actor,
            target: hit.target,
            amount: hit.total(),
            crit: hit.crit,
            dot: hit.dot,
            skill: crate::skills::canonical_skill_id(hit.skill),
        });
    }

    fn note_player(&mut self, id: i32) {
        if self.is_npc(id) {
            return;
        }
        if self.players.len() >= PLAYER_ID_LIMIT && !self.players.contains(&id) {
            self.players.clear();
        }
        self.players.insert(id);
    }

    fn actor_known(&self, id: i32) -> bool {
        self.targets
            .values()
            .any(|target| target.actors.contains_key(&id))
            || self.links.values().any(|owner| *owner == id)
    }

    /// New damage inside the idle window continues the same fight.
    fn touch(&mut self, now: i64) {
        if let (Some(last), Some(idle)) = (self.last_ms, self.idle_since()) {
            let gap = now.saturating_sub(last);
            if now.saturating_sub(idle) >= self.idle_reset_ms() {
                self.finish(now);
            } else if gap < PAUSE_MS {
                self.active_ms += gap;
            }
        }
        if self.first_ms.is_none() {
            self.first_ms = Some(now);
        }
        self.last_ms = Some(now);
    }

    fn finish(&mut self, now: i64) {
        self.archive(now);
        self.clear_fight();
    }

    fn archive(&mut self, now: i64) {
        let rows = self.archive_rows(now);
        self.push_history(rows);
    }

    fn archive_rows(&self, now: i64) -> Vec<UnitHistory> {
        let duration = self.shown_active(now);
        let mut rows: Vec<UnitHistory> = self
            .targets
            .iter()
            .filter(|(_, target)| target.damage > 0)
            .map(|(id, target)| UnitHistory {
                ended_ms: now,
                id: *id,
                name: self.label(*id),
                damage: target.damage,
                dps: per_second(target.damage, duration),
                duration_ms: duration,
                monster_type: self.monster_type(*id),
            })
            .collect();
        rows.sort_by(|a, b| b.damage.cmp(&a.damage));
        rows
    }

    fn push_history(&mut self, rows: Vec<UnitHistory>) {
        for row in rows.into_iter().rev() {
            self.history.push_front(row);
        }
        while self.history.len() > HISTORY_LIMIT {
            self.history.pop_back();
        }
    }

    fn clear_fight(&mut self) {
        self.targets.clear();
        self.taken.clear();
        self.heal.clear();
        self.heal_skills.clear();
        self.heal_events = Vec::new();
        self.misses.clear();
        self.resists.clear();
        self.recent.clear();
        self.last_target = None;
        self.first_ms = None;
        self.last_ms = None;
        self.death_ms = None;
        self.active_ms = 0;
        self.events = Vec::new();
        self.events_truncated = false;
        self.stagger_shares = Vec::new();
        self.released = None;
        self.pinned = None;
    }

    /// Start of the current silence: the last hit, or a later death.
    fn idle_since(&self) -> Option<i64> {
        let last = self.last_ms?;
        Some(self.death_ms.map_or(last, |died| died.max(last)))
    }

    fn note(&mut self, now: i64, hit: &Hit, taken: bool, heal: bool) {
        let actor_type = self.monster_type(hit.actor);
        self.recent.push_front(Recent {
            at: now,
            actor: hit.actor,
            actor_type,
            skill: crate::skills::canonical_skill_id(hit.skill),
            amount: if heal { hit.heal } else { hit.total() },
            crit: hit.crit,
            dot: hit.dot,
            taken,
            heal,
        });
        while self.recent.len() > RECENT_LIMIT {
            self.recent.pop_back();
        }
    }

    /// What the meter shows: the frozen boss fight while locked, else the
    /// live one.
    pub fn snapshot(&self, now: i64) -> Snapshot {
        let Some(lock) = &self.locked else {
            let mut snap = self.fight_snapshot(now);
            if self.mode != TargetMode::All {
                if let Some(boss) = self.engaged_boss() {
                    let boss_damage = self.targets.get(&boss).map_or(0, |t| t.damage);
                    snap.lock = Some(LockView {
                        remaining_ms: 0,
                        secs: self.lock_secs,
                        boss: self.label(boss),
                        pending_damage: (self.encounter_damage() - boss_damage).max(0),
                        engaged: true,
                    });
                }
            }
            return snap;
        };
        let mut snap = lock.view.fight_snapshot(now);
        snap.reset_in_ms = None;
        snap.lock = Some(LockView {
            remaining_ms: (self.lock_until(lock) - now).max(0),
            secs: self.lock_secs,
            boss: lock.view.label(lock.boss),
            pending_damage: self.encounter_damage(),
            engaged: false,
        });
        snap
    }

    fn fight_snapshot(&self, now: i64) -> Snapshot {
        let active_ms = self.shown_active(now);
        let status = match self.last_ms {
            Some(last) if now.saturating_sub(last) < PAUSE_MS => "combat",
            Some(_) if self.encounter_damage() > 0 => "paused",
            _ => "idle",
        };
        let selected = self.selected_targets();
        let mut merged: HashMap<i32, Actor> = HashMap::new();
        for id in &selected {
            let Some(target) = self.targets.get(id) else {
                continue;
            };
            for (actor_id, actor) in &target.actors {
                let owner = self.owner_of(*actor_id);
                merged.entry(owner).or_default().merge(actor);
            }
        }
        let encounter_damage: i64 = merged.values().map(|actor| actor.damage).sum();
        let encounter_dps = per_second(encounter_damage, active_ms);
        let mut players: Vec<PlayerRow> = merged
            .into_iter()
            .map(|(id, actor)| self.player_row(id, actor, encounter_damage, active_ms))
            .collect();
        players.sort_by(|a, b| b.damage.cmp(&a.damage).then(a.name.cmp(&b.name)));
        let healers = self.healers(active_ms);
        for player in &mut players {
            player.heal = healers.iter().find(|h| h.id == player.id).map_or(0, |h| h.heal);
        }
        let encounter_heal: i64 = healers.iter().map(|h| h.heal).sum();

        let target_id = match self.mode {
            TargetMode::All => None,
            TargetMode::Current => self.held_target().or(self.last_target),
            TargetMode::Boss => self.held_target().or_else(|| self.boss_target()),
        };
        let (target_hp, target_label) = match target_id {
            Some(id) => (self.hp_ratio(id), self.label(id)),
            None => (None, "All targets".to_string()),
        };
        let target_type = target_id.and_then(|id| self.monster_type(id));
        let bundled = target_type.and_then(monster_names::bundled);

        let recent = self
            .recent
            .iter()
            .map(|row| RecentRow {
                ago_ms: now.saturating_sub(row.at).max(0),
                actor_id: row.actor,
                actor: self.label_typed(row.actor, row.actor_type),
                you: self.self_id == Some(self.named_id(row.actor)),
                skill: row.skill,
                skill_name: crate::skills::skill_name(row.skill),
                amount: row.amount,
                crit: row.crit,
                dot: row.dot,
                taken: row.taken,
                heal: row.heal,
            })
            .collect();

        Snapshot {
            status,
            mode: self.mode,
            elapsed_ms: self
                .first_ms
                .map(|start| now.saturating_sub(start).max(0))
                .unwrap_or(0),
            active_ms,
            encounter_damage,
            encounter_dps,
            reset_in_ms: self
                .idle_since()
                .map(|idle| (idle + self.idle_reset_ms() - now).max(0)),
            target_id,
            target_label,
            target_type,
            target_user_name: target_type.and_then(|t| self.monster_names.get(&t).cloned()),
            target_bundled_name: bundled.and_then(|b| b.name.clone()),
            target_boss: target_id.is_some_and(|id| self.target_is_boss(id)),
            target_dummy: bundled.is_some_and(|b| b.dummy),
            target_defeated: target_id
                .and_then(|id| self.targets.get(&id))
                .is_some_and(|t| t.died_ms.is_some()),
            target_hp,
            targets_shown: selected.len(),
            players,
            recent,
            encounter_heal,
            encounter_hps: per_second(encounter_heal, active_ms),
            healers,
            lock: None,
            stagger: target_id.and_then(|id| self.stagger_view(id)),
        }
    }

    /// The open fight as a session: what the meter shows now for the counted
    /// targets, and their recorded hits. `None` before any damage.
    /// Saving while locked saves the boss fight.
    pub fn session(&self, now: i64) -> Option<Session> {
        match &self.locked {
            Some(lock) => lock.view.fight_session(now),
            None => self.fight_session(now),
        }
    }

    fn fight_session(&self, now: i64) -> Option<Session> {
        let started_at = self.first_ms?;
        let snap = self.fight_snapshot(now);
        if snap.encounter_damage <= 0 {
            return None;
        }
        let counted: HashSet<i32> = self.selected_targets().into_iter().collect();
        let mut actors: Vec<String> = snap.players.iter().map(|p| p.name.clone()).collect();
        let mut index: HashMap<i32, u32> = snap
            .players
            .iter()
            .enumerate()
            .map(|(i, player)| (player.id, i as u32))
            .collect();
        // A healer who dealt no damage to the counted targets still gets a row.
        let mut players: Vec<sessions::Player> = Vec::new();
        for healer in &snap.healers {
            if !index.contains_key(&healer.id) {
                index.insert(healer.id, actors.len() as u32);
                actors.push(healer.name.clone());
                players.push(sessions::Player {
                    name: healer.name.clone(),
                    total: 0,
                    dps: 0,
                    share: 0.0,
                    crit_pct: 0.0,
                    heal: healer.heal,
                    hps: healer.hps,
                    stagger: 0.0,
                });
            }
        }
        let heals: Vec<sessions::Heal> = self
            .heal_events
            .iter()
            .filter_map(|event| {
                let actor = *index.get(&self.named_id(event.actor))?;
                let flags = if event.hot { sessions::FLAG_HOT } else { 0 };
                let t_ms = event.at.saturating_sub(started_at).max(0);
                Some(sessions::Heal(t_ms, actor, i64::from(event.amount), flags, event.skill))
            })
            .collect();
        let events: Vec<sessions::Event> = self
            .events
            .iter()
            .filter(|event| counted.contains(&event.target))
            .filter_map(|event| {
                let actor = *index.get(&self.named_id(event.actor))?;
                let mut flags = 0;
                if event.crit {
                    flags |= sessions::FLAG_CRIT;
                }
                if event.dot {
                    flags |= sessions::FLAG_DOT;
                }
                let t_ms = event.at.saturating_sub(started_at).max(0);
                Some(sessions::Event(t_ms, actor, i64::from(event.amount), flags, event.skill))
            })
            .collect();
        let healer = |id: i32| snap.healers.iter().find(|h| h.id == id);
        let damage_players: Vec<sessions::Player> = snap
            .players
            .iter()
            .map(|player| sessions::Player {
                heal: healer(player.id).map_or(0, |h| h.heal),
                hps: healer(player.id).map_or(0, |h| h.hps),
                name: player.name.clone(),
                total: player.damage,
                dps: player.dps,
                stagger: player.stagger,
                share: round1(player.pct * 100.0),
                crit_pct: if player.hits > 0 {
                    round1(f64::from(player.crits) * 100.0 / f64::from(player.hits))
                } else {
                    0.0
                },
            })
            .collect();
        let players: Vec<sessions::Player> = damage_players.into_iter().chain(players).collect();
        let you = snap
            .players
            .iter()
            .map(|p| (p.id, p.you))
            .chain(snap.healers.iter().map(|h| (h.id, h.you)))
            .find(|(_, you)| *you)
            .and_then(|(id, _)| index.get(&id).copied());
        Some(Session {
            schema_version: sessions::SCHEMA_VERSION,
            started_at,
            ended_at: self.last_ms.unwrap_or(now),
            duration_ms: snap.active_ms,
            target: sessions::Target {
                label: snap.target_label,
                boss: snap.target_boss,
                dummy: snap.target_dummy,
                defeated: snap.target_defeated,
                died_ms: snap
                    .target_id
                    .and_then(|id| self.targets.get(&id))
                    .and_then(|t| t.died_ms)
                    .map(|at| at.saturating_sub(started_at).max(0)),
            },
            totals: sessions::Totals {
                total_damage: snap.encounter_damage,
                party_dps: snap.encounter_dps,
                total_heal: snap.encounter_heal,
                group_hps: snap.encounter_hps,
            },
            players,
            you,
            actors,
            skill_names: sessions::skill_names_with(&events, &heals),
            heals,
            events,
            truncated: self.events_truncated,
            name: None,
            stagger_breaks: snap
                .stagger
                .as_ref()
                .map(|stagger| stagger.break_ms.clone())
                .unwrap_or_default(),
            stagger: self
                .stagger_shares
                .iter()
                .filter(|share| counted.contains(&share.target))
                .filter_map(|share| {
                    let actor = *index.get(&self.named_id(share.actor))?;
                    let t_ms = share.at.saturating_sub(started_at).max(0);
                    let points = (share.points * 100.0).round() / 100.0;
                    Some(sessions::Stagger(t_ms, actor, points, share.skill))
                })
                .collect(),
        })
    }

    fn player_row(&self, id: i32, actor: Actor, encounter: i64, active_ms: i64) -> PlayerRow {
        let mut skills: Vec<SkillRow> = actor
            .skills
            .iter()
            .map(|((skill, dot), row)| SkillRow {
                id: *skill,
                name: crate::skills::skill_name(*skill),
                dot: *dot,
                damage: row.damage,
                hits: row.hits,
                crits: row.crits,
                backs: row.backs,
                fronts: row.fronts,
                parries: row.parries,
                blocks: row.blocks,
                perfects: row.perfects,
                doubles: row.doubles,
                multi_hits: row.multi_hits,
                stagger: round1(row.stagger),
            })
            .collect();
        skills.sort_by(|a, b| b.damage.cmp(&a.damage));
        PlayerRow {
            id,
            name: self.label(id),
            you: self.self_id == Some(id),
            damage: actor.damage,
            dps: per_second(actor.damage, active_ms),
            pct: if encounter > 0 {
                actor.damage as f64 / encounter as f64
            } else {
                0.0
            },
            hits: actor.hits,
            crits: actor.crits,
            backs: actor.backs,
            taken: self.taken.get(&id).copied().unwrap_or(0),
            heal: self.heal.get(&id).copied().unwrap_or(0),
            misses: self.misses.get(&id).copied().unwrap_or(0),
            resists: self.resists.get(&id).copied().unwrap_or(0),
            stagger: round1(actor.stagger),
            skills,
        }
    }

    /// Current is the live target plus every target that died in this fight,
    /// so a kill does not wipe the tally when the next pull starts.
    fn selected_targets(&self) -> Vec<i32> {
        match self.mode {
            TargetMode::All => self.targets.keys().copied().collect(),
            // A boss being fought is shown alone: other monsters stay
            // recorded. A frozen view keeps its dead targets as before.
            TargetMode::Current | TargetMode::Boss
                if self.pinned.is_none() && self.engaged_boss().is_some() =>
            {
                self.engaged_boss().into_iter().collect()
            }
            TargetMode::Boss if self.held_target().is_some() => {
                self.held_target().into_iter().collect()
            }
            TargetMode::Current => {
                let current = self.held_target().or(self.last_target);
                let mut ids: Vec<i32> = self
                    .targets
                    .iter()
                    .filter(|(id, target)| {
                        target.died_ms.is_some() && Some(**id) != current
                    })
                    .map(|(id, _)| *id)
                    .collect();
                ids.extend(current);
                ids
            }
            TargetMode::Boss => self.boss_target().into_iter().collect(),
        }
    }

    /// The boss that took the most damage, else whatever took the most.
    fn boss_target(&self) -> Option<i32> {
        let most = |ids: &mut dyn Iterator<Item = (&i32, &Target)>| {
            ids.max_by_key(|(id, target)| (target.damage, -**id)).map(|(id, _)| *id)
        };
        most(&mut self.targets.iter().filter(|(id, _)| self.target_is_boss(**id)))
            .or_else(|| most(&mut self.targets.iter()))
    }

    fn encounter_damage(&self) -> i64 {
        self.targets.values().map(|target| target.damage).sum()
    }

    fn shown_active(&self, now: i64) -> i64 {
        let Some(last) = self.last_ms else {
            return 0;
        };
        if now.saturating_sub(last) < PAUSE_MS {
            self.active_ms + now.saturating_sub(last)
        } else {
            self.active_ms
        }
    }

    /// `4A 36` is only about the local player. One stray record is not enough;
    /// three in a row for the same entity are.
    fn note_you(&mut self, id: i32) {
        if !(100..=9_999_999).contains(&id) {
            return;
        }
        if self.self_id == Some(id) {
            self.you_streak = None;
            return;
        }
        let streak = match self.you_streak {
            Some((prev, n)) if prev == id => n + 1,
            _ => 1,
        };
        if streak >= 3 {
            self.self_id = Some(id);
            self.you_streak = None;
        } else {
            self.you_streak = Some((id, streak));
        }
    }

    /// A summon is shown under its owner when that link is known.
    fn named_id(&self, id: i32) -> i32 {
        self.owner_of(id)
    }

    /// Whether a real name has been seen for this entity (or its owner).
    pub fn knows_name(&self, id: i32) -> bool {
        self.name_of(id).is_some()
    }

    /// Player display name for an entity id, when the meter has seen one.
    pub fn display_name(&self, id: i32) -> Option<String> {
        self.name_of(id).cloned()
    }

    fn name_of(&self, id: i32) -> Option<&String> {
        self.names
            .get(&self.named_id(id))
            .filter(|name| !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()))
    }

    /// A spawned monster (NPC spawn record or a kept monster type) that is
    /// not a summon. Never a player.
    fn is_npc(&self, id: i32) -> bool {
        !self.links.contains_key(&id)
            && !self.summons.contains(&id)
            && (self.spawn_types.contains_key(&id)
                || self.targets.get(&id).is_some_and(|t| t.monster_type.is_some()))
    }

    fn is_player(&self, id: i32) -> bool {
        if self.is_npc(id) {
            return false;
        }
        let owner = self.named_id(id);
        self.self_id == Some(owner)
            || self.players.contains(&owner)
            || self.players.contains(&id)
            || self.links.contains_key(&id)
            || self.links.values().any(|linked| *linked == owner)
    }

    /// The user's name for a monster type, else the bundled one.
    fn type_name(&self, monster_type: u32) -> Option<&str> {
        self.monster_names
            .get(&monster_type)
            .map(String::as_str)
            .or_else(|| monster_names::bundled(monster_type)?.name.as_deref())
    }

    /// The name shown for an entity: the user's name for its monster type,
    /// else the bundled name for that type, else its own name, else Unknown
    /// or Monster (Boss for a flagged boss). The entity id and the type are never the label.
    fn label(&self, id: i32) -> String {
        self.label_typed(id, None)
    }

    /// `kept` is a type remembered with the row, for an entity that may
    /// have despawned since.
    fn label_typed(&self, id: i32, kept: Option<u32>) -> String {
        let monster_type = if self.links.contains_key(&id) {
            None
        } else {
            kept.or_else(|| self.monster_type(id))
        };
        if let Some(name) = monster_type.and_then(|t| self.type_name(t)) {
            return name.to_string();
        }
        match self.name_of(id) {
            Some(name) => name.clone(),
            None if self.is_player(id) => UNKNOWN.to_string(),
            None if self.target_is_boss(id) => BOSS.to_string(),
            None => MONSTER.to_string(),
        }
    }

    fn hp_ratio(&self, id: i32) -> Option<f64> {
        let (current, max) = self.hp.get(&id).copied()?;
        if max <= 0 {
            return None;
        }
        Some((current as f64 / max as f64).clamp(0.0, 1.0))
    }
}

fn is_fallback(label: &str) -> bool {
    label == UNKNOWN || label == MONSTER || label == BOSS
}

/// A heal worth counting: positive and not above [`MAX_HEAL_TICK`].
fn plausible_heal(amount: i32) -> bool {
    amount > 0 && amount <= MAX_HEAL_TICK
}

fn per_second(amount: i64, active_ms: i64) -> i64 {
    if amount <= 0 {
        return 0;
    }
    amount.saturating_mul(1000) / active_ms.max(1000)
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(actor: i32, target: i32, skill: i32, damage: i32) -> Hit {
        Hit {
            actor,
            target,
            skill,
            damage,
            multi_hits: 0,
            multi_damage: 0,
            hit_type: 1,
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
    fn boss_popup_detection_is_independent_of_display_mode_and_encounter() {
        let mut combat = Combat::new();
        combat.set_boss_damage_threshold(1_500_000);
        combat.set_mode(TargetMode::All);
        combat.apply_hit(&hit(1, 9, 100, 1_500_001), 1000);
        assert!(combat.snapshot(1000).target_id.is_none());
        assert_eq!(combat.boss_detection_keys(), vec![(1000, 9)]);
        combat.reset(2000);
        assert!(combat.boss_detection_keys().is_empty());
        combat.apply_hit(&hit(1, 9, 100, 1_500_001), 3000);
        assert_eq!(combat.boss_detection_keys(), vec![(3000, 9)]);
    }

    #[test]
    fn dps_pauses_across_a_long_gap_inside_the_fight() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 1000), 0);
        combat.apply_hit(&hit(1, 9, 100, 1000), 4_000);
        let snap = combat.snapshot(4_000);
        assert_eq!(snap.encounter_damage, 2000);
        assert_eq!(snap.active_ms, 4_000);
        let paused = combat.snapshot(20_000);
        assert_eq!(paused.status, "paused");
        assert_eq!(paused.active_ms, 4_000);
        assert_eq!(paused.encounter_dps, 500);
    }

    #[test]
    fn silence_archives_each_unit_and_returns_damage_to_zero() {
        let mut combat = Combat::new();
        combat.set_name(9, "Training Scarecrow", false);
        combat.set_name(8, "Watcher", false);
        combat.apply_hit(&hit(1, 9, 100, 1000), 0);
        combat.apply_hit(&hit(1, 8, 100, 400), 1_000);
        combat.tick(31_000);
        let snap = combat.snapshot(31_000);
        assert_eq!(snap.status, "idle");
        assert_eq!(snap.encounter_damage, 0);
        let history = combat.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].name, "Training Scarecrow");
        assert_eq!(history[0].damage, 1000);
        assert_eq!(history[1].name, "Watcher");
        assert_eq!(history[1].damage, 400);
    }

    #[test]
    fn manual_reset_keeps_unit_history() {
        let mut combat = Combat::new();
        combat.set_name(9, "Training Scarecrow", false);
        combat.apply_hit(&hit(1, 9, 100, 2500), 0);
        combat.reset(5_000);
        assert_eq!(combat.snapshot(5_000).encounter_damage, 0);
        assert_eq!(combat.history()[0].damage, 2500);
        combat.clear_history();
        assert!(combat.history().is_empty());
    }

    #[test]
    fn reset_delay_is_configurable() {
        let mut combat = Combat::new();
        assert!(combat.set_reset_after_secs(10));
        assert!(!combat.set_reset_after_secs(1));
        combat.apply_hit(&hit(1, 9, 100, 800), 0);
        combat.tick(9_000);
        assert_eq!(combat.snapshot(9_000).encounter_damage, 800);
        combat.tick(10_000);
        assert_eq!(combat.snapshot(10_000).encounter_damage, 0);
        assert_eq!(combat.history()[0].damage, 800);
        assert_eq!(combat.settings().reset_after_secs, 10);
    }

    #[test]
    fn a_named_fight_is_shown_by_name_not_user_id() {
        let mut combat = Combat::new();
        combat.set_name(481_920, "Aria", true);
        combat.set_name(90_331, "Training Scarecrow", false);
        combat.apply_hit(&hit(481_920, 90_331, 12_090_243, 500), 0);
        let snap = combat.snapshot(1_000);

        assert_eq!(snap.target_label, "Training Scarecrow");
        assert_eq!(snap.players.len(), 1);
        assert_eq!(snap.players[0].name, "Aria");
        assert!(snap.players[0].you);
        assert!(snap.recent[0].you);
        assert_eq!(snap.players[0].skills[0].name, "Punishment");
        assert_eq!(snap.recent[0].actor, "Aria");
        assert_eq!(snap.recent[0].skill_name, "Punishment");

        combat.reset(2_000);
        let history_name = combat.history()[0].name.clone();
        assert_eq!(history_name, "Training Scarecrow");

        let shown = [
            snap.target_label.clone(),
            snap.players[0].name.clone(),
            snap.players[0].skills[0].name.clone(),
            snap.recent[0].actor.clone(),
            snap.recent[0].skill_name.clone(),
            history_name,
        ];
        for label in shown {
            assert!(
                label.chars().any(|c| c.is_alphabetic()),
                "expected a name, got {label}"
            );
            assert_ne!(label, "481920");
            assert_ne!(label, "90331");
            assert_ne!(label, "12090243");
            assert!(!label.starts_with('#'));
        }
    }

    #[test]
    fn an_unnamed_entity_is_not_shown_as_its_user_id() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(481_920, 90_331, 100, 10), 0);
        let snap = combat.snapshot(0);
        assert_eq!(snap.players[0].name, "Unknown");
        assert_eq!(snap.target_label, "Monster");
        assert!(!snap.players[0].name.contains("481920"));
        assert!(!snap.target_label.contains("90331"));
    }

    #[test]
    fn an_unnamed_target_that_acts_as_a_player_stays_unknown() {
        let mut combat = Combat::new();
        // 700 dealt player damage before, so as a PvP target it is a player.
        combat.apply_hit(&hit(700, 9, 100, 10), 0);
        combat.apply_hit(&hit(800, 700, 100, 10), 100);
        assert_eq!(combat.snapshot(100).target_label, "Unknown");

        // A player that only took monster damage is a player too.
        let mut taken = hit(9, 600, 1_609_010, 50);
        taken.taken = true;
        combat.apply_hit(&taken, 200);
        combat.apply_hit(&hit(800, 600, 100, 10), 300);
        let snap = combat.snapshot(300);
        assert_eq!(snap.target_label, "Unknown");
        assert_eq!(snap.recent[1].actor, "Monster", "the monster that hit 600");
    }

    #[test]
    fn your_unnamed_entity_is_never_called_a_monster() {
        let mut combat = Combat::new();
        combat.apply(Event::You { id: 420 }, 0);
        combat.apply(Event::You { id: 420 }, 0);
        combat.apply(Event::You { id: 420 }, 0);
        combat.apply_hit(&hit(800, 420, 100, 10), 0);
        assert_eq!(combat.snapshot(0).target_label, "Unknown");
    }

    #[test]
    fn a_name_replaces_the_monster_label_on_the_target() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 10), 0);
        assert!(!combat.knows_name(9));
        assert_eq!(combat.snapshot(0).target_label, "Monster");
        combat.set_name(9, "Watcher", false);
        assert!(combat.knows_name(9));
        assert_eq!(combat.snapshot(0).target_label, "Watcher");
    }

    #[test]
    fn summon_damage_is_shown_on_the_owner() {
        let mut combat = Combat::new();
        combat.apply(
            Event::Link {
                summon: 50,
                owner: 1,
            },
            0,
        );
        combat.set_name(1, "Aria", false);
        combat.apply_hit(&hit(1, 9, 100, 100), 0);
        combat.apply_hit(&hit(50, 9, 200, 40), 1_000);
        let snap = combat.snapshot(1_000);
        assert_eq!(snap.players.len(), 1);
        assert_eq!(snap.players[0].name, "Aria");
        assert_eq!(snap.players[0].damage, 140);
        assert_eq!(snap.recent[0].actor, "Aria");
    }

    #[test]
    fn a_guessed_name_does_not_replace_a_spawn_name() {
        let mut combat = Combat::new();
        combat.apply(
            Event::Name {
                id: 1,
                name: "Aria".into(),
                is_self: true,
                trusted: true,
            },
            0,
        );
        combat.apply(
            Event::Name {
                id: 1,
                name: "Nope".into(),
                is_self: false,
                trusted: false,
            },
            0,
        );
        combat.apply_hit(&hit(1, 9, 100, 10), 0);
        let snap = combat.snapshot(0);
        assert_eq!(snap.players[0].name, "Aria");
        assert!(snap.players[0].you);
    }

    #[test]
    fn a_guessed_name_fills_a_missing_one() {
        let mut combat = Combat::new();
        combat.apply(
            Event::Name {
                id: 2,
                name: "Boros".into(),
                is_self: false,
                trusted: false,
            },
            0,
        );
        combat.apply_hit(&hit(2, 9, 100, 10), 0);
        assert_eq!(combat.snapshot(0).players[0].name, "Boros");
        combat.apply(
            Event::Name {
                id: 2,
                name: "Celine".into(),
                is_self: false,
                trusted: true,
            },
            0,
        );
        assert_eq!(combat.snapshot(0).players[0].name, "Celine");
    }

    #[test]
    fn three_stats_records_mark_your_existing_name() {
        let mut combat = Combat::new();
        combat.set_name(420, "Naicha", false);
        combat.apply_hit(&hit(420, 9, 100, 10), 0);
        combat.apply(Event::You { id: 420 }, 0);
        combat.apply(Event::You { id: 420 }, 1);
        assert!(!combat.snapshot(1).players[0].you);
        combat.apply(Event::You { id: 420 }, 2);
        let snap = combat.snapshot(2);
        assert_eq!(snap.players[0].name, "Naicha");
        assert!(snap.players[0].you);
    }

    fn name(id: i32, name: &str, trusted: bool) -> Event {
        Event::Name {
            id,
            name: name.into(),
            is_self: false,
            trusted,
        }
    }

    fn hp(id: i32, current: i32) -> Event {
        Event::Hp {
            id,
            current,
            max: 10_000,
        }
    }

    #[test]
    fn a_name_that_arrives_after_the_damage_fills_the_existing_row() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(700, 9, 100, 300), 0);
        combat.apply_hit(&hit(700, 9, 100, 200), 500);
        let before = combat.snapshot(500);
        assert_eq!(before.players[0].name, "Unknown");
        assert_eq!(before.recent[0].actor, "Unknown");

        combat.apply(name(700, "Lumiel", false), 900);
        let after = combat.snapshot(1_000);
        assert_eq!(after.players.len(), 1);
        assert_eq!(after.players[0].name, "Lumiel");
        assert_eq!(after.players[0].damage, 500);
        assert!(after.recent.iter().all(|row| row.actor == "Lumiel"));
    }

    #[test]
    fn a_unit_named_after_its_pull_was_archived_is_renamed_in_history() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 900), 0);
        combat.reset(1_000);
        assert_eq!(combat.history()[0].name, "Monster");
        combat.apply(name(9, "Watcher", false), 2_000);
        assert_eq!(combat.history()[0].name, "Watcher");
    }

    #[test]
    fn a_summon_linked_after_its_damage_moves_onto_the_owner() {
        let mut combat = Combat::new();
        combat.set_name(1, "Aria", false);
        combat.apply_hit(&hit(1, 9, 100, 100), 0);
        combat.apply_hit(&hit(50, 9, 200, 40), 500);
        assert_eq!(combat.snapshot(500).players.len(), 2);

        combat.apply(Event::Link { summon: 50, owner: 1 }, 600);
        let snap = combat.snapshot(600);
        assert_eq!(snap.players.len(), 1);
        assert_eq!(snap.players[0].name, "Aria");
        assert_eq!(snap.players[0].damage, 140);
        assert!(snap.players.iter().all(|p| p.name != "Unknown"));
    }

    #[test]
    fn a_reused_summon_id_that_spawns_as_a_player_gets_its_own_row() {
        let mut combat = Combat::new();
        combat.set_name(1, "Aria", false);
        combat.apply(Event::Link { summon: 50, owner: 1 }, 0);
        combat.set_name(50, "Boros", false);
        combat.apply_hit(&hit(50, 9, 100, 70), 1_000);
        let snap = combat.snapshot(1_000);
        assert_eq!(snap.players.len(), 1);
        assert_eq!(snap.players[0].name, "Boros");
        assert_eq!(snap.players[0].id, 50);
    }

    #[test]
    fn a_summon_spawned_under_its_owners_name_stays_linked() {
        let mut combat = Combat::new();
        combat.set_name(1, "Aria", false);
        combat.apply(Event::Link { summon: 50, owner: 1 }, 0);
        combat.set_name(50, "Aria", false);
        combat.apply_hit(&hit(1, 9, 100, 100), 0);
        combat.apply_hit(&hit(50, 9, 200, 40), 0);
        let snap = combat.snapshot(0);
        assert_eq!(snap.players.len(), 1);
        assert_eq!(snap.players[0].damage, 140);
    }

    fn summon_spawn(summon: i32, owner: Option<i32>, name: Option<&str>, cast: Option<i32>) -> Event {
        Event::SummonSpawn {
            summon,
            kind: 0x5F,
            owner,
            owner_name: name.map(String::from),
            cast_owner: cast,
        }
    }

    fn owner_row(combat: &Combat) -> Vec<(String, i64)> {
        combat
            .snapshot(1_000)
            .players
            .into_iter()
            .map(|player| (player.name, player.damage))
            .collect()
    }

    #[test]
    fn a_summon_spawns_parent_key_puts_its_damage_on_the_owner() {
        let mut combat = Combat::new();
        combat.apply(name(1, "Aria", true), 0);
        combat.apply(summon_spawn(50, Some(1), Some("Aria"), None), 0);
        combat.apply_hit(&hit(1, 900, 11_000_000, 100), 100);
        combat.apply_hit(&hit(50, 900, 11_000_001, 40), 200);
        assert_eq!(owner_row(&combat), vec![("Aria".to_string(), 140)]);
    }

    #[test]
    fn a_summon_with_only_a_name_links_to_the_player_of_that_name() {
        let mut combat = Combat::new();
        combat.apply(name(1, "Aria", true), 0);
        combat.apply(summon_spawn(50, None, Some("Aria"), None), 0);
        combat.apply_hit(&hit(50, 900, 11_000_001, 40), 200);
        assert_eq!(owner_row(&combat), vec![("Aria".to_string(), 40)]);
    }

    #[test]
    fn a_summon_named_before_its_owner_is_linked_once_the_owner_appears() {
        let mut combat = Combat::new();
        combat.apply(summon_spawn(50, None, Some("Aria"), None), 0);
        combat.apply_hit(&hit(50, 900, 11_000_001, 40), 200);
        combat.apply(name(1, "Aria", true), 300);
        assert_eq!(owner_row(&combat), vec![("Aria".to_string(), 40)]);
    }

    #[test]
    fn a_name_two_players_share_links_nothing() {
        let mut combat = Combat::new();
        combat.apply(name(1, "Aria", true), 0);
        combat.apply(name(2, "Aria", true), 0);
        combat.apply(summon_spawn(50, None, Some("Aria"), None), 0);
        assert_eq!(combat.named_id(50), 50);
    }

    #[test]
    fn a_spirit_or_spell_caster_is_the_last_resort() {
        let mut combat = Combat::new();
        combat.apply(name(1, "Aria", true), 0);
        combat.apply(summon_spawn(50, None, None, Some(1)), 0);
        assert_eq!(combat.named_id(50), 1);
    }

    #[test]
    fn a_monster_is_never_a_summons_owner() {
        let mut combat = Combat::new();
        combat.apply(spawn(700, 2_000_123), 0);
        combat.apply(name(1, "Aria", true), 0);
        combat.apply(summon_spawn(50, Some(700), Some("Aria"), None), 0);
        assert_eq!(combat.named_id(50), 50);
        combat.apply(summon_spawn(51, None, Some("Aria"), Some(700)), 0);
        assert_eq!(combat.named_id(51), 51);
    }

    #[test]
    fn a_summon_spawn_on_a_reused_id_drops_the_old_link() {
        let mut combat = Combat::new();
        combat.apply(name(1, "Aria", true), 0);
        combat.apply(name(2, "Boros", true), 0);
        combat.apply(Event::Link { summon: 50, owner: 1 }, 0);
        combat.apply(summon_spawn(50, Some(2), None, None), 10);
        assert_eq!(combat.named_id(50), 2);
        combat.apply(summon_spawn(50, None, None, None), 20);
        assert_eq!(combat.named_id(50), 50);
    }

    #[test]
    fn a_summons_summon_lands_on_the_player() {
        let mut combat = Combat::new();
        combat.apply(name(1, "Aria", true), 0);
        combat.apply(summon_spawn(50, Some(1), None, None), 0);
        combat.apply(summon_spawn(60, Some(50), None, None), 0);
        assert_eq!(combat.named_id(60), 1);
    }

    #[test]
    fn links_never_loop() {
        let mut combat = Combat::new();
        combat.apply(Event::Link { summon: 50, owner: 60 }, 0);
        combat.apply(summon_spawn(60, Some(50), None, None), 0);
        assert_eq!(combat.named_id(60), 60);
        assert_eq!(combat.named_id(50), 60);
    }

    #[test]
    fn the_idle_window_is_thirty_seconds() {
        assert_eq!(ENCOUNTER_IDLE_MS, 30_000);
        assert_eq!(Combat::new().settings().reset_after_secs, 30);
    }

    #[test]
    fn a_kill_keeps_the_tally_for_the_idle_window() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply(hp(9, 0), 1_000);
        combat.tick(25_000);
        let snap = combat.snapshot(25_000);
        assert_eq!(snap.encounter_damage, 1_000);
        assert_eq!(snap.players[0].damage, 1_000);
        assert_eq!(snap.reset_in_ms, Some(6_000));
        assert!(combat.history().is_empty());
    }

    #[test]
    fn the_idle_window_counts_from_a_death_after_the_last_hit() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply(hp(9, 0), 4_000);
        combat.tick(30_000);
        assert_eq!(combat.snapshot(30_000).encounter_damage, 1_000);
        combat.tick(34_000);
        assert_eq!(combat.snapshot(34_000).encounter_damage, 0);
        assert_eq!(combat.history()[0].damage, 1_000);
    }

    #[test]
    fn damage_inside_the_idle_window_continues_the_same_encounter() {
        let mut combat = Combat::new();
        combat.set_name(8, "Watcher", false);
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply(hp(9, 0), 0);
        combat.tick(29_000);
        combat.apply_hit(&hit(1, 8, 100, 500), 29_000);
        let snap = combat.snapshot(29_000);
        assert_eq!(snap.elapsed_ms, 29_000);
        assert_eq!(snap.encounter_damage, 1_500);
        assert_eq!(snap.players[0].damage, 1_500);
        assert_eq!(snap.targets_shown, 2);
        assert_eq!(snap.target_label, "Watcher");
        assert!(combat.history().is_empty());
    }

    #[test]
    fn damage_after_the_idle_window_starts_a_new_encounter() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply(hp(9, 0), 0);
        combat.apply_hit(&hit(1, 8, 100, 500), 30_000);
        let snap = combat.snapshot(30_000);
        assert_eq!(snap.encounter_damage, 500);
        assert_eq!(snap.elapsed_ms, 0);
        assert_eq!(combat.history()[0].damage, 1_000);
    }

    #[test]
    fn current_mode_drops_a_live_target_you_switched_away_from() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply(hp(9, 4_000), 0);
        combat.apply_hit(&hit(1, 8, 100, 500), 1_000);
        let snap = combat.snapshot(1_000);
        assert_eq!(snap.encounter_damage, 500);
        assert_eq!(snap.targets_shown, 1);
    }

    #[test]
    fn a_late_tick_on_a_dead_target_does_not_take_current() {
        let mut combat = Combat::new();
        combat.set_name(8, "Watcher", false);
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply(hp(9, 0), 0);
        combat.apply_hit(&hit(1, 8, 100, 500), 1_000);
        combat.apply_hit(&hit(1, 9, 100, 50), 1_200);
        let snap = combat.snapshot(1_200);
        assert_eq!(snap.target_label, "Watcher");
        assert_eq!(snap.encounter_damage, 1_550);
    }

    fn spawn(id: i32, monster_type: u32) -> Event {
        Event::Spawn { id, monster_type }
    }

    /// Types not in the bundled table.
    const UNLISTED: u32 = 2_099_073;
    const UNLISTED_2: u32 = 2_099_074;
    const SEEKER: u32 = 2_100_343;
    const PUNCHING_BAG: u32 = 2_000_000;
    const BOSS: u32 = 2_000_412;

    #[test]
    fn a_typed_monster_is_shown_by_the_users_name_for_its_type() {
        let mut combat = Combat::new();
        combat.apply(spawn(19_107, UNLISTED), 0);
        combat.apply_hit(&hit(1, 19_107, 100, 10), 0);
        let snap = combat.snapshot(0);
        assert_eq!(snap.target_label, "Monster", "no name given yet");
        assert_eq!(snap.target_type, Some(UNLISTED));
        assert_eq!(snap.target_bundled_name, None);
        assert_eq!(snap.target_user_name, None);

        combat.set_monster_name(UNLISTED, Some("Rock Golem".into()));
        assert_eq!(combat.snapshot(0).target_label, "Rock Golem");
        assert_eq!(combat.snapshot(0).target_user_name.as_deref(), Some("Rock Golem"));

        // Another spawn of the same kind has the same name.
        combat.apply(spawn(36_358, UNLISTED), 10);
        combat.apply_hit(&hit(1, 36_358, 100, 10), 10);
        assert_eq!(combat.snapshot(10).target_label, "Rock Golem");

        combat.set_monster_name(UNLISTED, None);
        assert_eq!(combat.snapshot(10).target_label, "Monster");
    }

    #[test]
    fn the_users_type_name_wins_over_a_packet_name() {
        let mut combat = Combat::new();
        combat.apply(spawn(500, UNLISTED), 0);
        combat.apply(name(500, "Guess", false), 0);
        combat.apply_hit(&hit(1, 500, 100, 10), 0);
        assert_eq!(combat.snapshot(0).target_label, "Guess");
        combat.set_monster_name(UNLISTED, Some("Gargoyle".into()));
        assert_eq!(combat.snapshot(0).target_label, "Gargoyle");
    }

    #[test]
    fn a_typed_target_is_named_user_then_bundled_then_monster() {
        let mut combat = Combat::new();
        combat.apply(spawn(19_107, SEEKER), 0);
        combat.apply(name(19_107, "Guess", false), 0);
        combat.apply_hit(&hit(1, 19_107, 100, 10), 0);
        let snap = combat.snapshot(0);
        assert_eq!(snap.target_label, "Enhanced Krall Seeker", "bundled beats a packet name");
        assert_eq!(snap.target_bundled_name.as_deref(), Some("Enhanced Krall Seeker"));
        assert_eq!(snap.target_user_name, None);
        assert!(!snap.target_boss && !snap.target_dummy);

        combat.set_monster_name(SEEKER, Some("Seeker".into()));
        let snap = combat.snapshot(0);
        assert_eq!(snap.target_label, "Seeker", "the user's name beats the bundled one");
        assert_eq!(snap.target_user_name.as_deref(), Some("Seeker"));
        assert_eq!(snap.target_bundled_name.as_deref(), Some("Enhanced Krall Seeker"));

        combat.set_monster_name(SEEKER, None);
        assert_eq!(combat.snapshot(0).target_label, "Enhanced Krall Seeker", "clearing falls back");

        combat.apply(spawn(19_200, UNLISTED), 10);
        combat.apply_hit(&hit(1, 19_200, 100, 10), 10);
        let snap = combat.snapshot(10);
        assert_eq!(snap.target_label, "Monster", "a type missing from the table");
        assert_eq!(snap.target_bundled_name, None);
        for raw in ["19200", &UNLISTED.to_string(), &format!("{UNLISTED:x}")] {
            assert!(!snap.target_label.contains(raw));
        }
    }

    #[test]
    fn a_dummy_and_a_boss_are_flagged_from_the_bundled_table() {
        let mut combat = Combat::new();
        combat.apply(spawn(600, PUNCHING_BAG), 0);
        combat.apply_hit(&hit(1, 600, 100, 10), 0);
        let snap = combat.snapshot(0);
        assert_eq!(snap.target_label, "Punching Bag");
        assert!(snap.target_dummy);
        assert!(!snap.target_boss);

        combat.apply(spawn(601, BOSS), 10);
        combat.apply_hit(&hit(1, 601, 100, 10), 10);
        let snap = combat.snapshot(10);
        assert!(snap.target_boss);
        assert!(!snap.target_dummy);

        combat.set_mode(TargetMode::All);
        let snap = combat.snapshot(10);
        assert!(!snap.target_boss && !snap.target_dummy, "no single target");
    }

    #[test]
    fn a_bundled_monster_is_named_in_recent_hits_and_history() {
        let mut combat = Combat::new();
        combat.set_name(1, "Aria", true);
        combat.apply(spawn(700, SEEKER), 0);
        let mut taken = hit(700, 1, 1_609_010, 50);
        taken.taken = true;
        combat.apply_hit(&taken, 0);
        combat.apply_hit(&hit(1, 700, 100, 900), 0);
        let actor = |combat: &Combat| {
            let snap = combat.snapshot(0);
            snap.recent.into_iter().find(|row| row.actor_id == 700).unwrap().actor
        };
        assert_eq!(actor(&combat), "Enhanced Krall Seeker");

        combat.set_monster_name(SEEKER, Some("Seeker".into()));
        assert_eq!(actor(&combat), "Seeker");

        combat.reset(1_000);
        let monster = |combat: &Combat| {
            combat.history().into_iter().find(|row| row.id == 700).unwrap().name
        };
        assert_eq!(monster(&combat), "Seeker");
        combat.set_monster_name(SEEKER, None);
        assert_eq!(monster(&combat), "Enhanced Krall Seeker", "history falls back too");
    }

    #[test]
    fn an_untyped_target_keeps_the_old_labels() {
        let mut combat = Combat::new();
        combat.set_monster_name(UNLISTED, Some("Rock Golem".into()));
        combat.apply_hit(&hit(1, 9, 100, 10), 0);
        let snap = combat.snapshot(0);
        assert_eq!(snap.target_label, "Monster");
        assert_eq!(snap.target_type, None);
        assert_eq!(snap.players[0].name, "Unknown");
    }

    #[test]
    fn a_monster_that_hits_you_is_named_in_recent_hits() {
        let mut combat = Combat::new();
        combat.set_name(1, "Aria", true);
        combat.apply(spawn(700, UNLISTED), 0);
        let mut taken = hit(700, 1, 1_609_010, 50);
        taken.taken = true;
        combat.apply_hit(&taken, 0);
        assert_eq!(combat.snapshot(0).recent[0].actor, "Monster");
        combat.set_monster_name(UNLISTED, Some("Spriggan".into()));
        assert_eq!(combat.snapshot(0).recent[0].actor, "Spriggan");
    }

    #[test]
    fn a_despawned_target_keeps_its_type_for_the_fight_and_in_history() {
        let mut combat = Combat::new();
        combat.set_monster_name(0x200b68, Some("Wolf".into()));
        combat.apply(spawn(36_914, 0x200b68), 0);
        combat.apply_hit(&hit(1, 36_914, 100, 900), 0);
        combat.apply(Event::Despawn { id: 36_914 }, 500);
        assert!(!combat.spawn_types.contains_key(&36_914));
        let snap = combat.snapshot(500);
        assert_eq!(snap.target_label, "Wolf");
        assert_eq!(snap.target_type, Some(0x200b68));

        combat.reset(1_000);
        assert_eq!(combat.history()[0].name, "Wolf");
        combat.set_monster_name(0x200b68, Some("Grey Wolf".into()));
        assert_eq!(combat.history()[0].name, "Grey Wolf", "a later name reaches history");
        assert_eq!(combat.monster_type(36_914), None, "gone after the fight");
    }

    #[test]
    fn a_reused_id_takes_the_new_spawns_type() {
        let mut combat = Combat::new();
        combat.set_monster_name(UNLISTED, Some("Rock Golem".into()));
        combat.set_monster_name(UNLISTED_2, Some("Spriggan".into()));
        combat.apply(spawn(800, UNLISTED), 0);
        combat.apply(spawn(800, UNLISTED_2), 0);
        combat.apply_hit(&hit(1, 800, 100, 10), 0);
        assert_eq!(combat.snapshot(0).target_label, "Spriggan");

        // A player spawn on that id is a player, not the monster.
        combat.apply(name(800, "Celine", true), 10);
        let snap = combat.snapshot(10);
        assert_eq!(snap.target_label, "Celine");
        assert_eq!(snap.target_type, None);
    }

    #[test]
    fn a_monster_spawn_drops_the_previous_owners_name() {
        let mut combat = Combat::new();
        combat.apply(name(900, "Boros", true), 0);
        combat.apply(spawn(900, UNLISTED), 0);
        combat.apply_hit(&hit(1, 900, 100, 10), 0);
        assert_eq!(combat.snapshot(0).target_label, "Monster");
    }

    #[test]
    fn spawn_types_stay_bounded_and_keep_the_current_fight() {
        let mut combat = Combat::new();
        combat.apply(spawn(100, 0x200b81), 0);
        combat.apply_hit(&hit(1, 100, 100, 10), 0);
        for id in 0..SPAWN_TYPE_LIMIT as i32 {
            combat.apply(spawn(1_000 + id, 0x200c77), 0);
        }
        assert!(combat.spawn_types.len() <= SPAWN_TYPE_LIMIT);
        assert_eq!(combat.monster_type(100), Some(0x200b81));
    }

    #[test]
    fn a_target_with_hp_again_is_alive() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply(hp(9, 0), 0);
        combat.apply(hp(9, 10_000), 500);
        combat.apply_hit(&hit(1, 8, 100, 500), 1_000);
        assert_eq!(combat.snapshot(1_000).encounter_damage, 500);
    }

    #[test]
    fn a_session_keeps_the_counted_targets_hits_by_player() {
        let mut combat = Combat::new();
        combat.set_name(1, "Aria", true);
        combat.set_name(2, "Boros", false);
        combat.apply(Event::Link { summon: 50, owner: 1 }, 0);
        combat.apply(spawn(9, PUNCHING_BAG), 0);
        assert!(combat.session(0).is_none(), "nothing to save before damage");

        combat.apply_hit(&hit(2, 8, 100, 5_000), 0);
        let mut crit = hit(1, 9, 12_090_243, 600);
        crit.crit = true;
        combat.apply_hit(&crit, 1_000);
        combat.apply_hit(&hit(2, 9, 100, 300), 1_500);
        let mut dot = hit(50, 9, 100, 100);
        dot.dot = true;
        combat.apply_hit(&dot, 2_000);

        let session = combat.session(2_000).expect("a session");
        assert_eq!(session.schema_version, sessions::SCHEMA_VERSION);
        assert_eq!((session.started_at, session.ended_at, session.duration_ms), (0, 2_000, 2_000));
        assert_eq!(
            session.target,
            sessions::Target {
                label: "Punching Bag".into(),
                boss: false,
                dummy: true,
                defeated: false,
                died_ms: None,
            }
        );
        assert_eq!(session.totals.total_damage, 1_000, "only the current target counts");
        assert_eq!(session.totals.party_dps, 500);
        assert_eq!(session.actors, ["Aria", "Boros"]);
        let aria = &session.players[0];
        assert_eq!((aria.name.as_str(), aria.total, aria.dps), ("Aria", 700, 350));
        assert_eq!((aria.share, aria.crit_pct), (70.0, 50.0));
        assert_eq!((session.players[1].share, session.players[1].crit_pct), (30.0, 0.0));
        assert_eq!(
            session.events,
            [
                sessions::Event(1_000, 0, 600, sessions::FLAG_CRIT, 12_090_000),
                sessions::Event(1_500, 1, 300, 0, 100),
                sessions::Event(2_000, 0, 100, sessions::FLAG_DOT, 100),
            ],
            "the summon's hit is its owner's"
        );
        assert!(!session.truncated);

        combat.reset(3_000);
        assert!(combat.session(3_000).is_none(), "a reset starts a new session");
    }

    #[test]
    fn a_session_stops_keeping_hits_at_the_limit_but_keeps_the_totals() {
        let mut combat = Combat::new();
        let hits = SESSION_EVENT_LIMIT + 5;
        for _ in 0..hits {
            combat.apply_hit(&hit(1, 9, 100, 2), 0);
        }
        assert_eq!(combat.events.len(), SESSION_EVENT_LIMIT);
        let session = combat.session(0).unwrap();
        assert!(session.truncated);
        assert_eq!(session.events.len(), SESSION_EVENT_LIMIT);
        assert_eq!(session.totals.total_damage, 2 * hits as i64);
        assert_eq!(session.players[0].total, 2 * hits as i64);

        combat.reset(1_000);
        combat.apply_hit(&hit(1, 9, 100, 2), 2_000);
        let next = combat.session(2_000).unwrap();
        assert!(!next.truncated);
        assert_eq!(next.events.len(), 1);
    }

    fn heal(actor: i32, skill: i32, amount: i32, hot: bool) -> Hit {
        let mut h = hit(actor, actor, skill, 0);
        h.heal = amount;
        h.dot = hot;
        h
    }

    #[test]
    fn the_snapshot_lists_healers_with_summons_under_the_owner() {
        let mut combat = Combat::new();
        combat.set_name(1, "Aria", true);
        combat.set_name(2, "Boros", false);
        combat.apply(Event::Link { summon: 50, owner: 2 }, 0);
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply_hit(&heal(2, 300, 600, false), 500);
        combat.apply_hit(&heal(50, 300, 300, true), 1_000);
        combat.apply_hit(&heal(1, 400, 100, false), 2_000);
        let snap = combat.snapshot(2_000);
        assert_eq!((snap.encounter_heal, snap.encounter_hps), (1_000, 500));
        let names: Vec<&str> = snap.healers.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["Boros", "Aria"]);
        let boros = &snap.healers[0];
        assert_eq!((boros.heal, boros.hps, boros.pct), (900, 450, 0.9));
        assert_eq!(boros.skills.len(), 2, "direct and HoT rows stay apart");
        assert_eq!((boros.skills[0].hot, boros.skills[0].heal, boros.skills[0].ticks), (false, 600, 1));
        assert!(snap.healers[1].you);
        assert_eq!(snap.players[0].heal, 100);
        assert!(snap.recent.iter().any(|r| r.heal && r.amount == 300));
    }

    #[test]
    fn a_bogus_heal_tick_is_dropped() {
        let mut combat = Combat::new();
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply_hit(&heal(1, 300, MAX_HEAL_TICK + 1, false), 100);
        combat.apply_hit(&heal(1, 300, MAX_HEAL_TICK, false), 200);
        let snap = combat.snapshot(1_000);
        assert_eq!(snap.encounter_heal, i64::from(MAX_HEAL_TICK));
        assert_eq!(combat.heal_events.len(), 1);
        assert!(!plausible_heal(0) && !plausible_heal(-5) && plausible_heal(1));
    }

    #[test]
    fn a_session_keeps_heals_and_pure_healers() {
        let mut combat = Combat::new();
        combat.set_name(1, "Aria", true);
        combat.set_name(3, "Cleo", false);
        combat.apply_hit(&hit(1, 9, 100, 1_000), 0);
        combat.apply_hit(&heal(3, 300, 400, true), 1_000);
        combat.apply_hit(&heal(1, 400, 100, false), 2_000);
        let session = combat.session(2_000).unwrap();
        assert_eq!(session.actors, ["Aria", "Cleo"]);
        assert_eq!((session.totals.total_heal, session.totals.group_hps), (500, 250));
        assert_eq!((session.players[0].heal, session.players[1].heal, session.players[1].total), (100, 400, 0));
        assert_eq!(
            session.heals,
            [
                sessions::Heal(1_000, 1, 400, sessions::FLAG_HOT, 300),
                sessions::Heal(2_000, 0, 100, 0, 400),
            ]
        );
        assert_eq!(session.you, Some(0));
        assert!(session.skill_names.contains_key(&300));
        combat.reset(3_000);
        assert!(combat.heal_events.is_empty());
    }

    const BOSS_ID: i32 = 500;
    const ADD_ID: i32 = 600;

    fn spawned() -> Combat {
        let mut combat = Combat::new();
        combat.apply(Event::Spawn { id: BOSS_ID, monster_type: 4_242 }, 0);
        combat.apply(Event::Spawn { id: ADD_ID, monster_type: 4_243 }, 0);
        combat
    }

    fn boss_dies_at(combat: &mut Combat, at: i64) {
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_600_000)), at - 1_000);
        combat.apply(Event::LiveHp { id: BOSS_ID, hp: 0 }, at);
    }

    #[test]
    fn the_kill_rule() {
        use Removal::*;
        assert_eq!(classify_removal(3, true, 10, 5_000, true), Killed);
        assert_eq!(classify_removal(3, true, 10, 5_000, false), Killed);
        assert_eq!(classify_removal(3, true, 10, 200, true), Killed);
        assert_eq!(classify_removal(3, true, 10, 200, false), Removed);
        assert_eq!(classify_removal(1, true, 10, 5_000, true), Ignored);
        assert_eq!(classify_removal(7, true, 10, 5_000, true), Ignored);
        assert_eq!(classify_removal(3, false, 10, 5_000, true), Ignored);
        assert_eq!(classify_removal(3, true, 0, 5_000, true), Ignored);
    }

    #[test]
    fn hp_zero_then_flag_3_is_a_kill_and_defeats_the_target() {
        let mut combat = spawned();
        combat.set_boss_lock_secs(0, 0);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 500)), 1_000);
        assert!(!combat.snapshot(1_000).target_defeated);
        combat.apply(Event::LiveHp { id: ADD_ID, hp: 0 }, 2_000);
        assert!(combat.snapshot(2_000).target_defeated);
        assert_eq!(combat.targets[&ADD_ID].died_ms, Some(2_000));
        combat.apply(Event::Removed { id: ADD_ID, flag: 3 }, 6_000);
        assert!(combat.targets[&ADD_ID].killed);
        assert_eq!(combat.targets[&ADD_ID].died_ms, Some(2_000), "the HP 0 time stays");
        let session = combat.session(6_000).unwrap();
        assert!(session.target.defeated);
        assert_eq!(session.target.died_ms, Some(1_000));
    }

    #[test]
    fn flags_1_and_7_and_quick_cleanup_are_not_kills() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 500)), 1_000);
        combat.apply(Event::Removed { id: ADD_ID, flag: 7 }, 9_000);
        combat.apply(Event::Despawn { id: ADD_ID }, 9_000);
        assert!(!combat.snapshot(9_000).target_defeated);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 500)), 10_000);
        combat.apply(Event::Removed { id: ADD_ID, flag: 3 }, 10_500);
        let target = &combat.targets[&ADD_ID];
        assert!(target.removed && !target.killed && target.died_ms.is_none());
    }

    #[test]
    fn flag_3_long_after_the_last_hit_is_a_kill_without_hp() {
        let mut combat = spawned();
        combat.set_boss_lock_secs(0, 0);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 500)), 1_000);
        combat.apply(Event::Removed { id: ADD_ID, flag: 3 }, 3_000);
        assert!(combat.targets[&ADD_ID].killed);
        assert_eq!(combat.targets[&ADD_ID].died_ms, Some(3_000));
    }

    #[test]
    fn summons_and_players_are_never_killed() {
        let mut combat = Combat::new();
        combat.apply(
            Event::SummonSpawn { summon: 700, kind: 0, owner: None, owner_name: None, cast_owner: None },
            0,
        );
        combat.apply(Event::Hit(hit(1, 700, 100, 500)), 1_000);
        combat.apply(Event::LiveHp { id: 700, hp: 0 }, 2_000);
        combat.apply(Event::Removed { id: 700, flag: 3 }, 6_000);
        assert!(!combat.targets[&700].killed && combat.targets[&700].died_ms.is_none());
        combat.apply(Event::Hit(hit(2, 1, 100, 500)), 7_000);
        combat.apply(Event::Removed { id: 1, flag: 3 }, 9_000);
        assert!(!combat.targets[&1].killed);
    }

    #[test]
    fn unknown_class_needs_hp_zero_to_be_killed() {
        let mut combat = Combat::new();
        combat.set_boss_lock_secs(0, 0);
        combat.apply(Event::Hit(hit(1, 800, 100, 500)), 1_000);
        combat.apply(Event::Removed { id: 800, flag: 3 }, 5_000);
        assert!(!combat.targets[&800].killed);
        combat.apply(Event::Hit(hit(1, 801, 100, 500)), 1_000);
        combat.apply(Event::LiveHp { id: 801, hp: 0 }, 4_000);
        combat.apply(Event::Removed { id: 801, flag: 3 }, 5_000);
        assert!(combat.targets[&801].killed);
    }

    #[test]
    fn a_respawn_on_the_id_clears_the_death() {
        let mut combat = spawned();
        combat.set_boss_lock_secs(0, 0);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 500)), 1_000);
        combat.apply(Event::LiveHp { id: ADD_ID, hp: 0 }, 2_000);
        combat.apply(Event::Removed { id: ADD_ID, flag: 3 }, 3_000);
        combat.apply(Event::Spawn { id: ADD_ID, monster_type: 4_243 }, 4_000);
        let target = &combat.targets[&ADD_ID];
        assert!(!target.killed && target.died_ms.is_none() && target.hp_zero_ms.is_none());
    }

    #[test]
    fn damage_over_the_threshold_makes_a_boss_for_the_fight() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_499_999)), 1_000);
        assert!(!combat.snapshot(1_000).target_boss);
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1)), 1_100);
        assert!(combat.snapshot(1_100).target_boss);
        assert!(combat.set_boss_damage_threshold(10_000_000));
        assert!(combat.snapshot(1_100).target_boss, "once a boss, it stays one");
        // Boss mode picks the boss even when an add took more.
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 3_000_000)), 1_200);
        combat.set_mode(TargetMode::Boss);
        assert_eq!(combat.snapshot(1_200).target_id, Some(BOSS_ID));
        assert!(combat.session(1_200).unwrap().target.boss);
        assert!(!combat.set_boss_damage_threshold(0));
    }

    #[test]
    fn a_boss_death_locks_the_view_and_records_behind_it() {
        let mut combat = spawned();
        boss_dies_at(&mut combat, 10_000);
        let snap = combat.snapshot(12_000);
        let lock = snap.lock.clone().expect("locked");
        assert_eq!((lock.remaining_ms, lock.secs, lock.pending_damage), (8_000, 10, 0));
        assert_eq!(snap.reset_in_ms, None);

        // New combat during the lock: recorded, not shown.
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 700)), 13_000);
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 50)), 13_500);
        let snap = combat.snapshot(14_000);
        assert_eq!(snap.target_id, Some(BOSS_ID));
        assert!(snap.target_defeated && snap.target_boss);
        assert_eq!(snap.encounter_damage, 1_600_050, "a late tick lands on the boss");
        assert_eq!(snap.lock.unwrap().pending_damage, 700);
        // Saving during the lock saves the boss.
        let session = combat.session(14_000).unwrap();
        assert_eq!(session.totals.total_damage, 1_600_050);
        assert!(session.target.defeated);
        // Idle and auto-reset respect the lock.
        combat.tick(19_999);
        assert!(combat.is_locked());
        assert!(combat.history().is_empty());

        combat.tick(20_000);
        assert!(!combat.is_locked());
        let snap = combat.snapshot(20_000);
        assert_eq!((snap.target_id, snap.encounter_damage), (Some(ADD_ID), 700));
        assert!(snap.lock.is_none());
        assert_eq!(combat.history()[0].id, BOSS_ID);
    }

    #[test]
    fn a_long_boss_lock_outlives_the_short_idle_reset_and_saves_the_boss() {
        let mut combat = spawned();
        assert!(combat.set_reset_after_secs(10));
        assert!(combat.set_boss_lock_secs(35, 0));
        boss_dies_at(&mut combat, 10_000);
        // The new fight can idle out behind the held boss without releasing it.
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 700)), 11_000);
        combat.tick(21_000);
        let snap = combat.snapshot(21_000);
        assert_eq!(snap.target_id, Some(BOSS_ID));
        assert_eq!(snap.reset_in_ms, None);
        let lock = snap.lock.expect("boss still held past the 10 second idle reset");
        assert_eq!((lock.remaining_ms, lock.secs, lock.pending_damage), (24_000, 35, 0));
        let session = combat.session(21_000).expect("boss still available to save");
        assert_eq!(session.totals.total_damage, 1_600_000);
        assert!(session.target.boss && session.target.defeated);
        combat.tick(44_999);
        assert!(combat.is_locked());
        combat.tick(45_000);
        assert!(!combat.is_locked());
        assert!(combat.session(45_000).is_none());
        assert_eq!(combat.history()[0].id, BOSS_ID);
    }

    #[test]
    fn with_nothing_behind_the_lock_the_view_goes_idle() {
        let mut combat = spawned();
        boss_dies_at(&mut combat, 10_000);
        combat.tick(21_000);
        let snap = combat.snapshot(21_000);
        assert_eq!((snap.status, snap.encounter_damage), ("idle", 0));
        assert_eq!(combat.history().len(), 1);
    }

    #[test]
    fn the_lock_can_be_ended_early_turned_off_or_changed() {
        let mut combat = spawned();
        boss_dies_at(&mut combat, 10_000);
        assert!(combat.unlock());
        assert!(!combat.is_locked() && !combat.unlock());

        let mut combat = spawned();
        assert!(combat.set_boss_lock_secs(0, 0));
        boss_dies_at(&mut combat, 10_000);
        assert!(!combat.is_locked());

        let mut combat = spawned();
        boss_dies_at(&mut combat, 10_000);
        assert!(combat.set_boss_lock_secs(30, 11_000));
        assert_eq!(combat.snapshot(11_000).lock.unwrap().remaining_ms, 29_000);
        assert!(!combat.set_boss_lock_secs(61, 11_000));
        assert!(combat.set_boss_lock_secs(0, 11_000));
        assert!(!combat.is_locked());
        assert_eq!(combat.settings().boss_lock_secs, 0);
    }

    #[test]
    fn a_later_boss_death_restarts_the_timer() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 1_600_000)), 9_000);
        boss_dies_at(&mut combat, 10_000);
        combat.apply(Event::LiveHp { id: ADD_ID, hp: 0 }, 15_000);
        let snap = combat.snapshot(16_000);
        assert_eq!(snap.lock.unwrap().remaining_ms, 9_000);
        assert_eq!(snap.encounter_damage, 3_200_000, "still the same frozen fight");

        // A boss dying behind the lock takes the view over.
        let mut combat = spawned();
        boss_dies_at(&mut combat, 10_000);
        combat.apply(Event::Spawn { id: 900, monster_type: 4_244 }, 11_000);
        combat.apply(Event::Hit(hit(1, 900, 100, 2_000_000)), 12_000);
        combat.apply(Event::LiveHp { id: 900, hp: 0 }, 13_000);
        let snap = combat.snapshot(14_000);
        assert_eq!(snap.target_id, Some(900));
        assert_eq!(snap.lock.unwrap().remaining_ms, 9_000);
        assert_eq!(combat.history()[0].id, BOSS_ID, "the first boss was archived");
    }

    #[test]
    fn a_boss_that_comes_back_unfreezes_the_fight() {
        let mut combat = spawned();
        boss_dies_at(&mut combat, 10_000);
        combat.apply(Event::LiveHp { id: BOSS_ID, hp: 5_000 }, 11_000);
        assert!(!combat.is_locked());
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 10)), 11_500);
        let snap = combat.snapshot(11_500);
        assert_eq!(snap.encounter_damage, 1_600_010);
        assert!(!snap.target_defeated);
        assert!(combat.history().is_empty());
    }

    #[test]
    fn no_lock_once_the_boss_fight_already_ended() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_600_000)), 1_000);
        combat.tick(40_000);
        combat.apply(Event::LiveHp { id: BOSS_ID, hp: 0 }, 40_500);
        combat.apply(Event::Removed { id: BOSS_ID, flag: 3 }, 41_000);
        assert!(!combat.is_locked());
    }

    #[test]
    fn reset_and_mode_changes_during_the_lock() {
        let mut combat = spawned();
        boss_dies_at(&mut combat, 10_000);
        combat.set_mode(TargetMode::All);
        assert_eq!(combat.snapshot(11_000).mode, TargetMode::All);
        assert!(combat.snapshot(11_000).lock.is_some());
        combat.reset(12_000);
        assert!(!combat.is_locked());
        assert_eq!(combat.history().len(), 1);
    }

    #[test]
    fn an_unnamed_boss_is_labelled_boss() {
        // Unnamed, under the threshold: Monster.
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_499_999)), 1_000);
        let snap = combat.snapshot(1_000);
        assert_eq!(snap.target_label, "Monster");
        assert!(!snap.target_boss);
        // Crossing the threshold mid-fight flips the label live.
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1)), 1_100);
        let snap = combat.snapshot(1_100);
        assert_eq!(snap.target_label, "Boss");
        assert!(snap.target_boss);
        // A user-given name still wins over the fallback.
        combat.set_monster_name(4_242, Some("Big One".into()));
        assert_eq!(combat.snapshot(1_100).target_label, "Big One");
        combat.set_monster_name(4_242, None);
        assert_eq!(combat.snapshot(1_100).target_label, "Boss");
    }

    #[test]
    fn a_named_boss_keeps_its_name_and_players_stay_unknown() {
        let mut combat = Combat::new();
        combat.apply(spawn(601, BOSS), 10);
        combat.apply_hit(&hit(1, 601, 100, 2_000_000), 10);
        let snap = combat.snapshot(10);
        assert!(snap.target_boss);
        assert_ne!(snap.target_label, "Boss");
        assert_ne!(snap.target_label, "Monster");
        assert_eq!(snap.players[0].name, "Unknown", "player fallback unchanged");
    }

    #[test]
    fn an_engaged_boss_holds_the_view_while_adds_are_recorded() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_600_000)), 1_000);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 700)), 2_000);
        let snap = combat.snapshot(2_000);
        assert_eq!(snap.target_id, Some(BOSS_ID));
        assert_eq!(snap.encounter_damage, 1_600_000);
        let lock = snap.lock.expect("engaged");
        assert!(lock.engaged);
        assert_eq!((lock.remaining_ms, lock.pending_damage), (0, 700));
        assert!(combat.is_engaged() && !combat.is_locked());
        // All targets shows everything and no chip; Boss mode holds the boss.
        combat.set_mode(TargetMode::All);
        let snap = combat.snapshot(2_000);
        assert!(snap.lock.is_none());
        assert_eq!(snap.encounter_damage, 1_600_700);
        combat.set_mode(TargetMode::Boss);
        assert_eq!(combat.snapshot(2_000).target_id, Some(BOSS_ID));
        combat.set_mode(TargetMode::Current);
        // The idle end archives the whole fight, adds included, and frees the view.
        combat.tick(2_000 + ENCOUNTER_IDLE_MS);
        assert!(!combat.is_engaged());
        let ids: Vec<i32> = combat.history().iter().map(|r| r.id).collect();
        assert!(ids.contains(&BOSS_ID) && ids.contains(&ADD_ID));
        assert!(combat.snapshot(2_000 + ENCOUNTER_IDLE_MS).lock.is_none());
    }

    #[test]
    fn below_the_threshold_nothing_is_held() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_000)), 1_000);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 700)), 2_000);
        let snap = combat.snapshot(2_000);
        assert_eq!(snap.target_id, Some(ADD_ID));
        assert!(snap.lock.is_none() && !combat.is_engaged());
    }

    #[test]
    fn a_boss_left_alone_for_the_idle_window_is_let_go() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_600_000)), 1_000);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 7)), 20_000);
        assert!(combat.is_engaged());
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 7)), 31_500);
        assert!(!combat.is_engaged());
        assert_eq!(combat.snapshot(31_500).target_id, Some(ADD_ID));
    }

    #[test]
    fn engagement_ends_at_death_then_the_post_death_lock_then_the_live_fight() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_600_000)), 1_000);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 700)), 2_000);
        combat.apply(Event::LiveHp { id: BOSS_ID, hp: 0 }, 3_000);
        assert!(combat.is_locked() && !combat.is_engaged());
        let snap = combat.snapshot(4_000);
        assert_eq!(
            snap.target_id,
            Some(BOSS_ID),
            "the add hit last, the boss stays shown"
        );
        let lock = snap.lock.unwrap();
        assert!(!lock.engaged);
        assert_eq!(lock.remaining_ms, 9_000);
        // A new monster during the lock is recorded behind it.
        combat.apply(Event::Hit(hit(1, 700, 100, 300)), 5_000);
        assert_eq!(combat.snapshot(5_000).target_id, Some(BOSS_ID));
        combat.tick(13_000);
        let snap = combat.snapshot(13_000);
        assert!(snap.lock.is_none());
        assert_eq!((snap.target_id, snap.encounter_damage), (Some(700), 300));
    }

    #[test]
    fn unlocking_or_turning_it_off_lets_adds_take_the_view() {
        let mut combat = spawned();
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_600_000)), 1_000);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 700)), 2_000);
        assert!(combat.unlock());
        assert!(!combat.is_engaged());
        let snap = combat.snapshot(2_000);
        assert!(snap.lock.is_none());
        assert_eq!(snap.target_id, Some(ADD_ID));
        assert!(!combat.unlock(), "nothing left to unlock");
        // Reset starts a new fight, which engages again.
        combat.reset(3_000);
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_600_000)), 4_000);
        assert!(combat.is_engaged());
        combat.set_boss_engage_lock(false);
        assert!(!combat.is_engaged() && !combat.settings().boss_engage_lock);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 700)), 5_000);
        assert_eq!(combat.snapshot(5_000).target_id, Some(ADD_ID));
    }

    #[test]
    fn engaged_without_a_post_death_lock_and_through_a_false_death() {
        let mut combat = spawned();
        combat.set_boss_lock_secs(0, 0);
        combat.apply(Event::Hit(hit(1, BOSS_ID, 100, 1_600_000)), 1_000);
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 700)), 2_000);
        assert!(combat.is_engaged());
        combat.apply(Event::LiveHp { id: BOSS_ID, hp: 0 }, 2_500);
        assert!(!combat.is_engaged());
        combat.apply(
            Event::LiveHp {
                id: BOSS_ID,
                hp: 5_000,
            },
            2_600,
        );
        assert!(combat.is_engaged(), "HP back: still fighting it");
        assert_eq!(combat.snapshot(2_600).target_id, Some(BOSS_ID));
        combat.apply(Event::LiveHp { id: BOSS_ID, hp: 0 }, 3_000);
        combat.apply(
            Event::Removed {
                id: BOSS_ID,
                flag: 3,
            },
            3_100,
        );
        assert!(!combat.is_engaged() && !combat.is_locked());
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 1)), 3_200);
        assert_eq!(combat.snapshot(3_200).target_id, Some(ADD_ID));
    }

    #[test]
    fn a_false_death_behind_the_lock_resumes_the_engagement() {
        let mut combat = spawned();
        boss_dies_at(&mut combat, 10_000);
        assert!(combat.is_locked());
        combat.apply(
            Event::LiveHp {
                id: BOSS_ID,
                hp: 5_000,
            },
            11_000,
        );
        assert!(!combat.is_locked() && combat.is_engaged());
        combat.apply(Event::Hit(hit(1, ADD_ID, 100, 5)), 11_500);
        let snap = combat.snapshot(11_500);
        assert_eq!(snap.target_id, Some(BOSS_ID));
        assert!(snap.lock.unwrap().engaged);
    }

    fn packet(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    fn feed(combat: &mut Combat, bytes: &[u8], now: i64) {
        for event in crate::decode::parse_packet(bytes) {
            combat.apply(event, now);
        }
    }

    fn framed(body: &str) -> Vec<u8> {
        crate::framing::encode_frame(&packet(body))
    }

    /// The 2026-11-09 07:32 CST field boss 37138: its spawn has a `30 00`
    /// field, and `05 38` ticks name it as the source with player skill
    /// ids. It used to be taken for a player, so HP 0 and the flag-3
    /// removal were both ignored.
    #[test]
    fn the_field_boss_kill_from_the_capture_is_detected() {
        const BOSS: i32 = 37_138;
        let mut combat = Combat::new();
        combat.set_boss_lock_secs(35, 0);
        combat.set_name(979, "Modollove", false);
        combat.set_name(1769, "yiny", false);
        feed(
            &mut combat,
            &framed("413692a2020c30008c1b230000020a3a18c75ec6b8c600d958470098b24300fe01a0e7d904a0e7d904640000006400000000000000000000000000000000000000409c0000409c0000010000000000000000000000000000000000000006021101819698"),
            0,
        );
        assert_eq!(combat.monster_type(BOSS), Some(2_300_812));
        combat.apply_hit(&hit(979, BOSS, 12_350_000, 900_000), 1_000);
        // DoT damage and a HoT naming the boss as their source.
        feed(&mut combat, &framed("0538d3070a92a202ec04875ef147b40b812cb800"), 1_500);
        feed(&mut combat, &framed("0538e90d0b92a202df032ff5fe4bc30ec42cb18cc200"), 1_600);
        assert!(!combat.is_player(BOSS), "a spawned monster is never a player");
        combat.apply_hit(&hit(1769, BOSS, 12_350_000, 900_000), 2_000);
        assert_eq!(combat.snapshot(2_000).target_label, "Vakron");

        feed(&mut combat, &packet("14008d92a2020201000000000000000000"), 3_000);
        combat.apply_hit(&hit(979, BOSS, 12_350_000, 256), 3_100);
        feed(&mut combat, &packet("0b423692a2020003"), 3_500);

        assert!(combat.is_locked(), "the boss lock begins");
        let snap = combat.snapshot(3_500);
        assert_eq!(snap.target_id, Some(BOSS));
        assert!(snap.target_defeated);
        assert!(snap.lock.as_ref().is_some_and(|lock| !lock.engaged && lock.secs == 35));
        let session = combat.session(3_500).unwrap();
        assert!(session.target.defeated);
        assert_eq!(session.target.died_ms, Some(2_000));
    }

    #[test]
    fn a_monster_dot_on_a_player_still_counts_as_damage_taken() {
        let mut combat = Combat::new();
        combat.apply(Event::Spawn { id: 37_138, monster_type: 2_300_812 }, 0);
        let mut dot = hit(37_138, 979, 1_200_016, 500);
        dot.dot = true;
        dot.taken = true;
        combat.apply_hit(&dot, 0);
        assert_eq!(combat.taken.get(&979), Some(&500));
    }

    fn gauge(id: i32, remaining: u32) -> Event {
        Event::Stagger { id, max: 1_200, remaining, frozen: false }
    }

    fn stagger_of(snap: &Snapshot, name: &str) -> f64 {
        snap.players.iter().find(|p| p.name == name).map_or(0.0, |p| p.stagger)
    }

    #[test]
    fn a_gauge_drop_after_one_hit_goes_to_that_hit_and_teaches_the_skill() {
        let mut combat = spawned();
        combat.set_name(1, "Aria", false);
        combat.set_name(2, "Boros", false);
        combat.apply(gauge(BOSS_ID, 1_200), 0);
        combat.apply_hit(&hit(1, BOSS_ID, 11_170_450, 1_000), 100);
        combat.apply(gauge(BOSS_ID, 1_195), 100);
        combat.apply_hit(&hit(2, BOSS_ID, 12_040_240, 1_000), 200);
        combat.apply(gauge(BOSS_ID, 1_192), 200);
        assert_eq!(combat.learned_stagger(11_170_450), Some(5.0));
        assert_eq!(combat.learned_stagger(12_040_240), Some(3.0));

        // Two hits, one drop: shared 5 : 3 by the learned values.
        combat.apply_hit(&hit(1, BOSS_ID, 11_170_450, 1_000), 300);
        combat.apply_hit(&hit(2, BOSS_ID, 12_040_240, 1_000), 300);
        combat.apply(gauge(BOSS_ID, 1_176), 300);
        let snap = combat.snapshot(300);
        assert_eq!(stagger_of(&snap, "Aria"), 15.0);
        assert_eq!(stagger_of(&snap, "Boros"), 9.0);
        let aria = snap.players.iter().find(|p| p.name == "Aria").unwrap();
        assert_eq!(aria.skills[0].stagger, 15.0);
        let view = snap.stagger.unwrap();
        assert_eq!((view.max, view.remaining, view.fill), (1_200, 1_176, 24));
        assert_eq!((view.estimated, view.unattributed, view.breaks), (24.0, 0.0, 0));
    }

    #[test]
    fn unknown_skills_split_evenly_and_a_drop_with_no_hit_is_unattributed() {
        let mut combat = spawned();
        combat.set_name(1, "Aria", false);
        combat.set_name(2, "Boros", false);
        combat.apply_hit(&hit(1, BOSS_ID, 100, 1_000), 0);
        combat.apply(gauge(BOSS_ID, 1_200), 0);
        combat.apply(gauge(BOSS_ID, 1_190), 50);
        combat.apply_hit(&hit(1, BOSS_ID, 100, 1_000), 100);
        combat.apply_hit(&hit(2, BOSS_ID, 200, 1_000), 100);
        combat.apply(gauge(BOSS_ID, 1_180), 100);
        let snap = combat.snapshot(100);
        assert_eq!(stagger_of(&snap, "Aria"), 5.0);
        assert_eq!(stagger_of(&snap, "Boros"), 5.0);
        let view = snap.stagger.unwrap();
        assert_eq!((view.estimated, view.unattributed), (10.0, 10.0));
    }

    #[test]
    fn a_break_is_counted_and_the_refill_is_not_stagger() {
        let mut combat = spawned();
        combat.set_name(1, "Aria", false);
        combat.apply_hit(&hit(1, BOSS_ID, 100, 1_000), 0);
        combat.apply(gauge(BOSS_ID, 3), 0);
        combat.apply_hit(&hit(1, BOSS_ID, 100, 1_000), 1_000);
        combat.apply(Event::StaggerBreak { id: BOSS_ID }, 1_000);
        combat.apply(Event::StaggerBreak { id: BOSS_ID }, 1_200);
        let view = combat.snapshot(1_000).stagger.unwrap();
        assert!(view.broken);
        assert_eq!((view.breaks, view.break_ms.clone(), view.fill), (1, vec![1_000], 1_200));
        assert_eq!(stagger_of(&combat.snapshot(1_000), "Aria"), 3.0);

        combat.apply_hit(&hit(1, BOSS_ID, 100, 1_000), 3_000);
        combat.apply(gauge(BOSS_ID, 1_200), 6_000);
        let snap = combat.snapshot(6_000);
        let view = snap.stagger.unwrap();
        assert!(!view.broken);
        assert_eq!((view.remaining, view.estimated), (1_200, 3.0));
        combat.apply(Event::Stagger { id: BOSS_ID, max: 1_200, remaining: 1_200, frozen: true }, 7_000);
        assert!(combat.snapshot(7_000).stagger.unwrap().frozen);
        let session = combat.session(7_000).unwrap();
        assert_eq!(session.stagger_breaks, vec![1_000]);
        assert_eq!(session.stagger, vec![sessions::Stagger(1_000, 0, 3.0, 100)]);
        assert_eq!(session.players[0].stagger, 3.0);
    }

    #[test]
    fn no_gauge_means_no_stagger_view() {
        let mut combat = spawned();
        combat.apply_hit(&hit(1, BOSS_ID, 100, 1_000), 0);
        assert!(combat.snapshot(0).stagger.is_none());
        combat.apply(gauge(ADD_ID, 1_000), 0);
        assert!(combat.snapshot(0).stagger.is_none(), "another entity's gauge");
    }
}
