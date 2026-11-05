//! Decode combat events out of framed game packets.
//!
//! The field order of a `04 38` damage record follows the layout documented
//! by A2Tools DPS Meter. Only combat fields are kept. Chat, inventories, and
//! other packet types are ignored.

use crate::varint::{read_varint, try_read_varint};

/// Player skills are 8 digits. A 7-digit skill is an NPC hitting a player.
const NPC_SKILL: std::ops::RangeInclusive<i32> = 1_000_000..=9_999_999;
/// A spirit and its owner naming each other. The amount is not damage.
const SPIRIT_TO_OWNER: std::ops::RangeInclusive<i32> = 16_990_000..=16_999_999;
const OWNER_TO_SPIRIT: std::ops::RangeInclusive<i32> = 16_770_000..=16_779_999;
const COMPACT_SKILL: i64 = 99_745_942;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hit {
    pub actor: i32,
    pub target: i32,
    pub skill: i32,
    /// Main component, after additional hits have been split out.
    pub damage: i32,
    pub multi_hits: i32,
    pub multi_damage: i32,
    pub hit_type: i32,
    pub crit: bool,
    pub back: bool,
    pub front: bool,
    pub block: bool,
    pub parry: bool,
    pub perfect: bool,
    pub double: bool,
    pub perfect_block: bool,
    pub dot: bool,
    /// An NPC skill: the player took this damage, nobody dealt meter DPS.
    pub taken: bool,
    pub heal: i32,
    pub miss: bool,
    pub resist: bool,
}

impl Hit {
    pub fn total(&self) -> i32 {
        self.damage.saturating_add(self.multi_damage)
    }

    fn blank() -> Self {
        Self {
            actor: 0,
            target: 0,
            skill: 0,
            damage: 0,
            multi_hits: 0,
            multi_damage: 0,
            hit_type: 0,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Hit(Hit),
    /// `(summon id, owner id)` from a spirit link record.
    Link {
        summon: i32,
        owner: i32,
    },
    Name {
        id: i32,
        name: String,
        is_self: bool,
        /// A masked spawn. A later guess must not replace it.
        trusted: bool,
    },
    /// `4A 36` names the local player’s entity and nothing else.
    You {
        id: i32,
    },
    Hp {
        id: i32,
        current: i32,
        max: i32,
    },
    /// `8D <id> 02 01 00 <u32 hp>`: current HP of any entity, monsters
    /// included. Reaching 0 is the earliest death signal.
    LiveHp {
        id: i32,
        hp: u32,
    },
    /// `42 36 <id> 00 <flag>` with a flag other than 1. Flag 3 follows a
    /// kill; flag 7 is cleanup. Flag 1 stays [`Event::Despawn`].
    Removed {
        id: i32,
        flag: u8,
    },
    /// `41 36` spawns an NPC. The type is shared by every monster of one
    /// kind; the id belongs to this spawn only.
    Spawn {
        id: i32,
        monster_type: u32,
    },
    /// `42 36 <id> 00 01` removes an entity.
    Despawn {
        id: i32,
    },
    /// A `40/41 36` spawn of a summon, spirit, or skill effect. The id is
    /// new (or reused), so any old link on it is stale. Combat links it in
    /// this order: `owner`, then the player named `owner_name`, then
    /// `cast_owner`.
    SummonSpawn {
        summon: i32,
        /// The kind byte, the low byte of the spawn mask.
        kind: u8,
        /// The summon's own parent key (kind `0x5F` only).
        owner: Option<i32>,
        /// The character name the spawn carries: its owner's or caster's.
        owner_name: Option<String>,
        /// A spirit's buff-block caster, or a ground spell's caster.
        cast_owner: Option<i32>,
    },
}

/// Everything this packet contributes. Top-level bundles are already opened.
/// A self record can also sit in a bundle carried inside a normal packet.
pub fn parse_packet(packet: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let op = opcode(packet);
    let hits = if op == Some([0x04, 0x38]) {
        parse_damage(packet, false, false)
    } else if op == Some([0x05, 0x38]) {
        parse_dot(packet).into_iter().collect()
    } else {
        parse_damage(packet, true, true)
    };
    for hit in hits {
        if let Some((summon, owner)) = classify_link(hit.skill, hit.actor, hit.target) {
            events.push(Event::Link { summon, owner });
        } else {
            events.push(Event::Hit(hit));
        }
    }
    events.extend(identity_in(packet, 0));
    if let Some(hp) = parse_hp(packet) {
        events.push(hp);
    }
    events
}

/// Names, plus which entity is you. `depth` counts bundles already opened.
fn identity_in(packet: &[u8], depth: usize) -> Vec<Event> {
    let mut events = parse_names(packet);
    if let Some(id) = you_packet(packet) {
        events.push(Event::You { id });
    }
    if let Some((id, monster_type)) = npc_spawn(packet) {
        events.push(Event::Spawn { id, monster_type });
    }
    match despawn(packet) {
        Some((id, 1)) => events.push(Event::Despawn { id }),
        Some((id, flag)) => events.push(Event::Removed { id, flag }),
        None => {}
    }
    for (id, hp) in crate::death_debug::hp_8d_records(packet) {
        events.push(Event::LiveHp { id, hp });
    }
    if let Some(summon) = summon_spawn(packet) {
        events.push(summon);
    }
    if depth < crate::framing::MAX_BUNDLE_DEPTH {
        events.extend(embedded_identity(packet, depth));
    }
    events
}

/// `<len> 4A 36 <entity id>`. The server sends this about the local player only.
fn you_packet(packet: &[u8]) -> Option<i32> {
    let length = read_varint(packet, 0);
    if length.length <= 0 {
        return None;
    }
    let offset = length.length as usize;
    if packet.get(offset) != Some(&0x4A) || packet.get(offset + 1) != Some(&0x36) {
        return None;
    }
    let id = read_varint(packet, offset + 2);
    (id.length > 0 && (100..=9_999_999).contains(&id.value)).then_some(id.value)
}

/// `<len> 41 36 <npc record>`.
fn npc_spawn(packet: &[u8]) -> Option<(i32, u32)> {
    let at = opcode_at(packet)?;
    if packet.get(at..at + 2)? != [0x41, 0x36] {
        return None;
    }
    npc_record_at(packet, at + 2)
}

/// `<id varint> <flag> <u16 le 0x20|0x2C> <type u24 le> <00 00|00 40> 02`.
/// Any flag byte is accepted; seen so far are 04, 05, 0C, 0D, 0F.
fn npc_record_at(data: &[u8], at: usize) -> Option<(i32, u32)> {
    let id = read_varint(data, at);
    if id.length <= 0 || !(100..=9_999_999).contains(&id.value) {
        return None;
    }
    let start = at + id.length as usize;
    let f = data.get(start..start + 9)?;
    let shape = matches!(u16::from_le_bytes([f[1], f[2]]), 0x20 | 0x2C)
        && f[6] == 0x00
        && matches!(f[7], 0x00 | 0x40)
        && f[8] == 0x02;
    let monster_type = u32::from_le_bytes([f[3], f[4], f[5], 0]);
    (shape && monster_type != 0).then_some((id.value, monster_type))
}

// Summon spawns. The layouts below are read after A2Tools DPS Meter
// (github.com/taengu/A2Tools-DPS-Meter, `parse_summon_spawn_at` and friends)
// and RATmeter (github.com/Kuroukihime/AIon2-Dps-Meter,
// `MobPacketProcessor`), both GPL-3.0, re-written for this decoder. They
// come from those projects' captures and still need checking against ours.

/// A player's summon (pets, Divine Aura and the like).
const KIND_SUMMON: u8 = 0x5F;
/// A skill effect attached to the skill's target. Its parent is never the
/// owner; only its name (the caster) is usable.
const KIND_EFFECT: u8 = 0x1C;
/// The mask bit that says a summon carries its parent key.
const MASK_PARENT: u32 = 0x0010;
/// What precedes a spirit's owner on its buff block.
const SPIRIT_ANCHOR: [u8; 8] = [0x80, 0x75, 0xD5, 0x2A, 0xBB, 0x03, 0x00, 0x00];
const SPIRIT_SCAN: usize = 120;
const CASTER_SCAN: usize = 240;

fn is_summon_kind(kind: u8) -> bool {
    matches!(kind, KIND_SUMMON | KIND_EFFECT | 0x1F | 0x1D | 0x5D)
}

/// `<len> 40|41 36 <id varint> <mask, low byte = kind> ...`, for the kinds
/// in [`is_summon_kind`]. Monster summons are told apart in `Combat`, which
/// knows which ids are monsters.
fn summon_spawn(packet: &[u8]) -> Option<Event> {
    let at = opcode_at(packet)?;
    if !matches!(*packet.get(at)?, 0x40 | 0x41) || *packet.get(at + 1)? != 0x36 {
        return None;
    }
    let id = read_varint(packet, at + 2);
    if id.length <= 0 || !(100..=9_999_999).contains(&id.value) {
        return None;
    }
    let summon = id.value;
    let offset = at + 2 + id.length as usize;
    let kind = *packet.get(offset)?;
    if !is_summon_kind(kind) {
        return None;
    }
    let byte = |i: usize| packet.get(offset + i).copied().unwrap_or(0);
    let mask = u32::from_le_bytes([byte(0), byte(1), byte(2), byte(3)]);
    let named = spawn_inline_name(packet, offset);
    let after_name = named.as_ref().map_or(offset + 5, |(_, end)| *end);
    let owner = if kind == KIND_SUMMON && mask & MASK_PARENT != 0 {
        parent_key(packet, after_name, summon)
    } else {
        None
    };
    let cast_owner = match kind {
        KIND_EFFECT => None,
        _ => spirit_owner(packet, offset, summon).or_else(|| {
            matches!(kind, KIND_SUMMON | 0x1F)
                .then(|| ground_caster(packet, offset, summon))
                .flatten()
        }),
    };
    Some(Event::SummonSpawn {
        summon,
        kind,
        owner,
        owner_name: named.map(|(name, _)| name),
        cast_owner,
    })
}

/// The inline name gate is mask byte 4 (u32 mask) or 2 (older u16 mask):
/// `<gate, bit 0> <len u8> <utf-8 name>`. The whole field must be a name,
/// which is what makes trying two positions safe.
fn spawn_inline_name(packet: &[u8], offset: usize) -> Option<(String, usize)> {
    [4usize, 2].into_iter().find_map(|gate| {
        if packet.get(offset + gate)? & 0x01 == 0 {
            return None;
        }
        let len = *packet.get(offset + gate + 1)? as usize;
        let start = offset + gate + 2;
        let end = start + len;
        if !(1..=48).contains(&len) || end > packet.len() {
            return None;
        }
        Some((exact_name(&packet[start..end])?, end))
    })
}

/// The summon's parent key, found by the owner block right after it:
/// `06 <parent u32> <legion u32> 00 00 <server u16> <len u8> <legion name>`.
/// The `06` and five more fields must all line up.
fn parent_key(packet: &[u8], from: usize, summon: i32) -> Option<i32> {
    (from.max(1)..packet.len()).find_map(|at| {
        if packet[at - 1] != 0x06 {
            return None;
        }
        let f = packet.get(at..at + 13)?;
        let parent = u32::from_le_bytes([f[0], f[1], f[2], f[3]]);
        let server = u16::from_le_bytes([f[10], f[11]]);
        let len = f[12] as usize;
        let ok = (100..=9_999_999).contains(&parent)
            && parent as i32 != summon
            && f[8] == 0
            && f[9] == 0
            && (1..=9_999).contains(&server)
            && len <= 40
            && packet
                .get(at + 13..at + 13 + len)
                .is_some_and(|name| std::str::from_utf8(name).is_ok());
        ok.then_some(parent as i32)
    })
}

/// A spirit's owner: the varint after [`SPIRIT_ANCHOR`] on its buff block.
fn spirit_owner(packet: &[u8], offset: usize, summon: i32) -> Option<i32> {
    let end = packet.len().min(offset + SPIRIT_SCAN + SPIRIT_ANCHOR.len());
    let found = packet.get(offset..end)?.windows(SPIRIT_ANCHOR.len()).position(|w| w == SPIRIT_ANCHOR)?;
    let owner = read_varint(packet, offset + found + SPIRIT_ANCHOR.len());
    (owner.length > 0 && (100..=9_999_999).contains(&owner.value) && owner.value != summon)
        .then_some(owner.value)
}

/// A ground spell's caster (Cold Storm, Bittercold Wind): the u32 after the
/// `07 02 06` or `07 02 01` that follows the spawn position.
fn ground_caster(packet: &[u8], offset: usize, summon: i32) -> Option<i32> {
    let end = packet.len().min(offset + CASTER_SCAN);
    let found = packet
        .get(offset..end)?
        .windows(3)
        .position(|w| w[0] == 0x07 && w[1] == 0x02 && matches!(w[2], 0x06 | 0x01))?;
    let at = offset + found + 3;
    let caster = i32::from_le_bytes(packet.get(at..at + 4)?.try_into().ok()?);
    ((100..=9_999_999).contains(&caster) && caster != summon).then_some(caster)
}

/// `<len> 42 36 <id> 00 <flag>`, and nothing after it.
fn despawn(packet: &[u8]) -> Option<(i32, u8)> {
    let at = opcode_at(packet)?;
    if packet.get(at..at + 2)? != [0x42, 0x36] {
        return None;
    }
    let id = read_varint(packet, at + 2);
    if id.length <= 0 || !(100..=9_999_999).contains(&id.value) {
        return None;
    }
    match packet[at + 2 + id.length as usize..] {
        [0x00, flag] => Some((id.value, flag)),
        _ => None,
    }
}

fn opcode_at(packet: &[u8]) -> Option<usize> {
    let length = read_varint(packet, 0);
    (length.length > 0).then_some(length.length as usize)
}

/// A bundle hidden inside a packet: `<varint len> FF FF <u32> <lz4>`.
/// The stream framer never sees it, and that is where your self record goes
/// once you are already in a zone.
fn embedded_identity(packet: &[u8], depth: usize) -> Vec<Event> {
    let mut events = Vec::new();
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
        if let Some((end, plain)) = opened {
            events.extend(identity_in_buffer(&plain, depth + 1));
            i = end;
        } else {
            i += 1;
        }
    }
    events
}

fn identity_in_buffer(buffer: &[u8], depth: usize) -> Vec<Event> {
    let walked = crate::framing::walk_inner(buffer);
    let mut events = Vec::new();
    for frame in &walked.frames {
        match frame.kind {
            crate::framing::FrameKind::Packet => {
                events.extend(identity_in(frame.bytes(buffer), depth));
            }
            crate::framing::FrameKind::Bundle => {
                if depth < crate::framing::MAX_BUNDLE_DEPTH {
                    if let Some(plain) = crate::framing::decompress_bundle(frame.payload(buffer)) {
                        events.extend(identity_in_buffer(&plain, depth + 1));
                    }
                }
            }
        }
    }
    events
}

fn opcode(packet: &[u8]) -> Option<[u8; 2]> {
    let length = read_varint(packet, 0);
    if length.length <= 0 {
        return None;
    }
    let at = length.length as usize;
    Some([*packet.get(at)?, *packet.get(at + 1)?])
}

fn parse_damage(packet: &[u8], allow_embedded: bool, trusted_only: bool) -> Vec<Hit> {
    let length = read_varint(packet, 0);
    if length.length < 0 {
        return Vec::new();
    }
    let mut offset = length.length as usize;
    if offset + 1 >= packet.len() {
        return Vec::new();
    }
    if packet[offset] != 0x04 || packet[offset + 1] != 0x38 {
        if allow_embedded {
            return embedded_damage(packet);
        }
        return Vec::new();
    }
    offset += 2;

    let mut hits = Vec::new();
    while offset < packet.len() {
        let mut chained = false;
        if offset + 1 < packet.len() && packet[offset] == 0x01 && packet[offset + 1] == 0x00 {
            offset += 2;
            chained = true;
        }
        if !hits.is_empty() && !chained {
            break;
        }

        let Some(hit) = read_one_hit(packet, &mut offset, trusted_only) else {
            break;
        };
        hits.push(hit);
    }
    hits
}

fn classify_link(skill: i32, actor: i32, target: i32) -> Option<(i32, i32)> {
    if SPIRIT_TO_OWNER.contains(&skill) {
        Some((actor, target))
    } else if OWNER_TO_SPIRIT.contains(&skill) {
        Some((target, actor))
    } else {
        None
    }
}

/// Pull `04 38` records out of a packet whose own opcode is something else.
fn embedded_damage(packet: &[u8]) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut at = 0;
    while at + 1 < packet.len() {
        if packet[at] != 0x04 || packet[at + 1] != 0x38 {
            at += 1;
            continue;
        }
        // Two-byte length so the opcode sits where a framed packet's opcode sits.
        let mut headless = vec![0xFF, 0x01];
        headless.extend_from_slice(&packet[at..]);
        let found = parse_damage(&headless, false, true);
        if found.is_empty() {
            at += 1;
        } else {
            hits.extend(found);
            at += 2;
        }
    }
    hits
}

fn read_one_hit(packet: &[u8], offset: &mut usize, trusted_only: bool) -> Option<Hit> {
    let target = try_read_varint(packet, offset)?;
    if !(100..=9_999_999).contains(&target) {
        return None;
    }
    let switch = try_read_varint(packet, offset)?;
    let layout = switch & 0x0F;
    let no_value = matches!(layout, 0 | 2);
    if !(4..=7).contains(&layout) && !no_value {
        return None;
    }
    try_read_varint(packet, offset)?;
    let actor = try_read_varint(packet, offset)?;
    if !(100..=9_999_999).contains(&actor) {
        return None;
    }
    if *offset + 4 > packet.len() {
        return None;
    }
    let mut skill = i64::from(u32::from_le_bytes([
        packet[*offset],
        packet[*offset + 1],
        packet[*offset + 2],
        packet[*offset + 3],
    ]));
    *offset += 4;
    if (3_000_000..=3_099_999).contains(&skill) {
        skill = skill * 10 + 1;
    }
    if !(1..=299_999_999).contains(&skill) {
        return None;
    }
    let npc = NPC_SKILL.contains(&(skill as i32));
    if *offset < packet.len() {
        *offset += 1;
    }
    let hit_type = try_read_varint(packet, offset)?;

    if no_value {
        let mut hit = Hit::blank();
        hit.actor = actor;
        hit.target = target;
        hit.skill = skill as i32;
        hit.hit_type = hit_type;
        hit.miss = hit_type == 1;
        hit.resist = hit_type == 6;
        if hit.miss || hit.resist {
            return Some(hit);
        }
        return None;
    }

    let fixed = match layout {
        5 => 12,
        6 => 10,
        7 => 14,
        _ => 8,
    };
    let mut crit = hit_type == 3;
    let mut back = false;
    let mut front = false;
    let mut block = false;
    let mut parry = false;
    let mut perfect = false;
    let mut double = false;
    let mut perfect_block = false;
    let mut extra = 0usize;
    if layout & 0x02 != 0 {
        let plotter = read_plotter(packet, *offset)?;
        block = plotter.flags & 0x01 != 0;
        parry = plotter.flags & 0x02 != 0;
        perfect = plotter.flags & 0x04 != 0;
        double = plotter.flags & 0x08 != 0;
        perfect_block = plotter.flags & 0x40 != 0;
        back = plotter.angle == Some(0x01);
        front = plotter.angle == Some(0x02);
        extra = plotter.hp_len.saturating_sub(1);
        let _ = plotter.flags & 0x10;
        let _ = plotter.flags & 0x20;
    }
    if hit_type == 3 {
        crit = true;
    }
    *offset += fixed + extra;
    if *offset >= packet.len() {
        return None;
    }

    let mut first = try_read_varint(packet, offset)?;
    let mut second = try_read_varint(packet, offset)?;
    // A leading zero is padding in front of the power scalar, and the damage
    // is the varint after that scalar.
    if first == 0 {
        if let Some(third) = try_read_varint(packet, offset) {
            first = second;
            second = third;
        }
    }
    let _power_scalar = first;
    let mut damage = second;
    if damage < 0 || damage > 99_999_999 {
        return None;
    }

    let strict = if matches!(layout, 4 | 6) && skill != COMPACT_SKILL {
        parse_hit_tail(packet, *offset, layout, switch, damage)
    } else {
        None
    };

    if strict.is_none() && (switch & 0x30) == 0x30 && *offset < packet.len() {
        try_read_varint(packet, offset);
    }
    let (multi_hits, multi_damage) = if let Some((end, _field, count, extra_damage)) = strict {
        *offset = end;
        (count, extra_damage)
    } else {
        read_loose_multi(packet, offset, damage)
    };

    if multi_hits > 0 && multi_damage > 0 && damage > multi_damage {
        damage -= multi_damage;
    }

    let mut heal = 0;
    if *offset + 1 < packet.len() && packet[*offset] == 0x03 && packet[*offset + 1] == 0x00 {
        *offset += 2;
        if let Some(amount) = try_read_varint(packet, offset) {
            if (1..10_000_000).contains(&amount) {
                heal = amount;
            }
        }
    }

    if trusted_only && !trusted_shape(actor, target, hit_type, damage + multi_damage, skill as i32)
    {
        return None;
    }

    let mut hit = Hit::blank();
    hit.actor = actor;
    hit.target = target;
    hit.skill = skill as i32;
    hit.damage = damage.max(0);
    hit.multi_hits = multi_hits;
    hit.multi_damage = multi_damage;
    hit.hit_type = hit_type;
    hit.crit = crit;
    hit.back = back;
    hit.front = front;
    hit.block = block;
    hit.parry = parry;
    hit.perfect = perfect;
    hit.double = double;
    hit.perfect_block = perfect_block;
    hit.taken = npc;
    hit.heal = heal;
    // A self-cast with a value is a heal, not damage dealt.
    if actor == target && hit.total() > 1 {
        hit.heal = hit.heal.max(hit.total());
        hit.damage = 0;
        hit.multi_damage = 0;
        hit.multi_hits = 0;
        hit.taken = false;
    }
    Some(hit)
}

fn trusted_shape(actor: i32, target: i32, hit_type: i32, damage: i32, skill: i32) -> bool {
    if actor == target || !(1..=3).contains(&hit_type) || damage <= 0 {
        return false;
    }
    (10_000_000..=29_999_999).contains(&skill) || (30_000_000..=30_999_999).contains(&skill)
}

struct Plotter {
    flags: u8,
    hp_len: usize,
    angle: Option<u8>,
}

fn read_plotter(packet: &[u8], offset: usize) -> Option<Plotter> {
    let flags = *packet.get(offset)?;
    let mut at = offset + 1;
    let _hp = try_read_varint(packet, &mut at)?;
    Some(Plotter {
        flags,
        hp_len: at - offset - 1,
        angle: packet.get(at).copied(),
    })
}

/// `(end, layout-4 field, additional hits, their damage)`.
fn parse_hit_tail(
    packet: &[u8],
    offset: usize,
    layout: i32,
    switch: i32,
    value: i32,
) -> Option<(usize, i32, i32, i32)> {
    parse_hit_tail_as(packet, offset, layout, switch, value).or_else(|| {
        if layout == 4 {
            parse_hit_tail_as(packet, offset, 6, switch, value)
        } else {
            None
        }
    })
}

fn parse_hit_tail_as(
    packet: &[u8],
    mut offset: usize,
    layout: i32,
    switch: i32,
    value: i32,
) -> Option<(usize, i32, i32, i32)> {
    let mut field = 0;
    if layout == 4 {
        field = try_read_varint(packet, &mut offset)?;
        if !(1..=25).contains(&field) {
            return None;
        }
    }
    let (mut count, mut damage) = (0, 0i64);
    if switch & 0x20 != 0 {
        count = try_read_varint(packet, &mut offset)?;
        if !(1..=25).contains(&count) {
            return None;
        }
        for _ in 0..count {
            let hit = try_read_varint(packet, &mut offset)?;
            if hit < 0 {
                return None;
            }
            damage += i64::from(hit);
        }
        if damage >= i64::from(value) {
            return None;
        }
    }
    if switch & 0x40 != 0 {
        let absorbed = try_read_varint(packet, &mut offset)?;
        if !(1..=8).contains(&absorbed) {
            return None;
        }
        offset = offset.checked_add(4 * absorbed as usize)?;
        if offset > packet.len() {
            return None;
        }
    }
    let rest = packet.get(offset..).unwrap_or_default();
    let clean = rest.is_empty()
        || (rest.len() >= 2 && rest[1] == 0x00 && (1..=7).contains(&rest[0]))
        || rest.starts_with(&[0x04, 0x38]);
    clean.then_some((offset, field, count, damage as i32))
}

/// Older multi-hit tail, used when the record does not end on the strict shape.
fn read_loose_multi(packet: &[u8], offset: &mut usize, damage: i32) -> (i32, i32) {
    if *offset >= packet.len() {
        return (0, 0);
    }
    let saved = *offset;
    let marker = |at: usize| {
        at + 1 < packet.len() && packet[at + 1] == 0x00 && (1..=7).contains(&packet[at])
    };
    if marker(*offset) {
        return (0, 0);
    }
    let Some(peek) = try_read_varint(packet, offset) else {
        return (0, 0);
    };
    let hit_count = if (0..=25).contains(&peek) {
        peek
    } else if !marker(*offset) {
        match try_read_varint(packet, offset) {
            Some(actual) if (0..=25).contains(&actual) => actual,
            _ => {
                *offset = saved;
                return (0, 0);
            }
        }
    } else {
        0
    };
    if hit_count <= 0 {
        return (0, 0);
    }
    let cap = damage.max(500_000);
    let mut count = 0;
    let mut total = 0;
    let mut first = None;
    let mut same = true;
    while count < hit_count.min(25) && *offset < packet.len() {
        if marker(*offset) || packet.get(*offset..*offset + 2) == Some(&[0x04, 0x38]) {
            break;
        }
        let Some(value) = try_read_varint(packet, offset) else {
            break;
        };
        if value > cap || value < 50 {
            return (0, 0);
        }
        match first {
            None => first = Some(value),
            Some(prev) if prev != value => same = false,
            _ => {}
        }
        total += value;
        count += 1;
    }
    if hit_count > count && count == 1 && same {
        if let Some(value) = first {
            return (hit_count, value * hit_count);
        }
    }
    (count, total)
}

fn parse_dot(packet: &[u8]) -> Option<Hit> {
    let length = read_varint(packet, 0);
    if length.length < 0 {
        return None;
    }
    let mut offset = length.length as usize;
    if packet.get(offset) != Some(&0x05) || packet.get(offset + 1) != Some(&0x38) {
        return None;
    }
    offset += 2;
    let target = try_read_varint(packet, &mut offset)?;
    let effect = *packet.get(offset)? as i32;
    offset += 1;
    let damage_effect = matches!(effect, 0x02 | 0x0A | 0x06 | 0x0E);
    let heal_effect = matches!(effect, 0x01 | 0x09 | 0x0B);
    if !damage_effect && !heal_effect {
        return None;
    }
    let actor = try_read_varint(packet, &mut offset)?;
    if !(100..=9_999_999).contains(&actor) || !(100..=9_999_999).contains(&target) {
        return None;
    }
    if damage_effect && actor == target {
        return None;
    }
    try_read_varint(packet, &mut offset)?;
    if offset + 4 > packet.len() {
        return None;
    }
    let skill = i32::from_le_bytes([
        packet[offset],
        packet[offset + 1],
        packet[offset + 2],
        packet[offset + 3],
    ]) / 100;
    offset += 4;
    if !(1..=299_999_999).contains(&skill) {
        return None;
    }
    let amount = try_read_varint(packet, &mut offset)?;
    if !(1..=99_999_999).contains(&amount) {
        return None;
    }
    // A shield id can follow the amount. It is not more damage.
    let mut hit = Hit::blank();
    hit.actor = actor;
    hit.target = target;
    hit.skill = skill;
    hit.dot = true;
    if heal_effect {
        hit.heal = amount;
    } else {
        hit.damage = amount;
        hit.taken = NPC_SKILL.contains(&skill);
    }
    Some(hit)
}

fn parse_hp(packet: &[u8]) -> Option<Event> {
    let length = read_varint(packet, 0);
    if length.length <= 0 {
        return None;
    }
    let mut offset = length.length as usize;
    if packet.get(offset) != Some(&0x1B) || packet.get(offset + 1) != Some(&0x92) {
        return None;
    }
    offset += 2;
    let id = try_read_varint(packet, &mut offset)?;
    if !(100..=9_999_999).contains(&id) {
        return None;
    }
    let current = try_read_varint(packet, &mut offset)?;
    let max = try_read_varint(packet, &mut offset)?;
    if !(1..=50_000_000).contains(&max) || current < 0 || current > max {
        return None;
    }
    Some(Event::Hp { id, current, max })
}

fn parse_names(packet: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    // Spawn records first, so a later guess cannot overwrite them.
    events.extend(masked_names(packet));
    events.extend(ownership_records(packet));
    events.extend(anchored_names(packet));
    events.extend(nearby_names(packet));
    events.extend(block_names(packet));
    events.extend(terminator_names(packet));
    events.extend(loot_names(packet));
    events
}

fn push_name(events: &mut Vec<Event>, id: i32, name: String, is_self: bool, trusted: bool) {
    events.push(Event::Name {
        id,
        name,
        is_self,
        trusted,
    });
}

/// `33 36` is you. `44 36` and `45 36` are other players. `40 36` and
/// `41 36` use the same layout but are not confirmed as player spawns, so
/// their names are guesses.
/// `<id varint> <mask1 u32> <mask2 u8> [bit 0] <len u8> <utf-8 name>`.
fn masked_names(packet: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut i = 0;
    while i + 1 < packet.len() {
        let is_self = packet[i] == 0x33 && packet[i + 1] == 0x36;
        let is_player = matches!(packet[i], 0x44 | 0x45) && packet[i + 1] == 0x36;
        let is_other = matches!(packet[i], 0x40 | 0x41) && packet[i + 1] == 0x36;
        if !is_self && !is_player && !is_other {
            i += 1;
            continue;
        }
        // A typed NPC spawn has no name; its type bytes only look like one.
        if packet[i] == 0x41 && npc_record_at(packet, i + 2).is_some() {
            i += 2;
            continue;
        }
        // A summon's inline name is its owner's (or caster's), not its own.
        // `summon_spawn` passes it on as `owner_name`.
        if is_other && summon_kind_at(packet, i + 2) {
            i += 2;
            continue;
        }
        let Some((id, name, end)) = masked_at(packet, i + 2) else {
            i += 1;
            continue;
        };
        // A self record carries your server id after the name. Without it,
        // the bytes are another record that only looks like one.
        let you = is_self && server_follows(packet, end);
        push_name(&mut events, id, name, you, !is_other);
        i = end;
    }
    events
}

fn summon_kind_at(data: &[u8], at: usize) -> bool {
    let id = read_varint(data, at);
    id.length > 0
        && data
            .get(at + id.length as usize)
            .is_some_and(|kind| is_summon_kind(*kind))
}

fn masked_at(data: &[u8], at: usize) -> Option<(i32, String, usize)> {
    let id_info = read_varint(data, at);
    if id_info.length <= 0 || !(1..=9_999_999).contains(&id_info.value) {
        return None;
    }
    let mask2 = at + id_info.length as usize + 4;
    if data.get(mask2).copied().unwrap_or(0) & 0x01 == 0 {
        return None;
    }
    let name_len = *data.get(mask2 + 1)? as usize;
    let start = mask2 + 2;
    let end = start + name_len;
    if !(1..=48).contains(&name_len) || end > data.len() {
        return None;
    }
    let name = exact_name(&data[start..end])?;
    Some((id_info.value, name, end))
}

/// Your server id, the u16 after your name. Real servers sit in this band.
fn server_follows(data: &[u8], at: usize) -> bool {
    let Some(bytes) = data.get(at..at + 2) else {
        return false;
    };
    let server = u16::from_le_bytes([bytes[0], bytes[1]]);
    (1000..3000).contains(&server)
}

/// `04 8D <summon> <4 bytes> <owner> <meta varint> <len> <owner name>`.
/// Links the summon to its owner, so its damage is not an unnamed row.
fn ownership_records(packet: &[u8]) -> Vec<Event> {
    let length = read_varint(packet, 0);
    if length.length < 0 {
        return Vec::new();
    }
    let mut pos = length.length as usize;
    if packet.get(pos) != Some(&0x04) || packet.get(pos + 1) != Some(&0x8D) {
        return Vec::new();
    }
    pos += 2;
    let summon = read_varint(packet, pos);
    if summon.length <= 0 || !(100..=9_999_999).contains(&summon.value) {
        return Vec::new();
    }
    pos += summon.length as usize + 4;
    let owner = read_varint(packet, pos);
    if owner.length <= 0
        || !(100..=9_999_999).contains(&owner.value)
        || owner.value == summon.value
    {
        return Vec::new();
    }
    pos += owner.length as usize;
    let meta = read_varint(packet, pos);
    if meta.length <= 0 {
        return Vec::new();
    }
    pos += meta.length as usize;
    let Some(name) = name_field(packet, pos, false) else {
        return Vec::new();
    };
    vec![
        Event::Link {
            summon: summon.value,
            owner: owner.value,
        },
        Event::Name {
            id: owner.value,
            name,
            is_self: false,
            trusted: true,
        },
    ]
}

/// `E0/E2 07 <len> <name>`, with the entity id ending on the marker.
fn anchored_names(packet: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut i = 0;
    while i + 2 < packet.len() {
        if !matches!(packet[i], 0xE0 | 0xE2) || packet[i + 1] != 0x07 {
            i += 1;
            continue;
        }
        let Some(name) = name_field(packet, i + 2, false) else {
            i += 1;
            continue;
        };
        let end = i + 3 + packet[i + 2] as usize;
        let Some(id) = varint_ending_at(packet, i) else {
            i += 1;
            continue;
        };
        push_name(&mut events, id, name, false, false);
        i = end;
    }
    events
}

/// `36 <id varint>` then, within 64 bytes, `07 <len> <name>`.
/// Spawn opcodes are read by `masked_names` instead.
fn nearby_names(packet: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut i = 0;
    while i < packet.len() {
        if packet[i] != 0x36 || (i > 0 && matches!(packet[i - 1], 0x33 | 0x40 | 0x41 | 0x44 | 0x45))
        {
            i += 1;
            continue;
        }
        let id_info = read_varint(packet, i + 1);
        if id_info.length <= 0 || !(100..=9_999_999).contains(&id_info.value) {
            i += 1;
            continue;
        }
        let end = i + 1 + id_info.length as usize;
        let window = end..packet.len().min(end + 64);
        if let Some(at) = packet[window.clone()].iter().position(|b| *b == 0x07) {
            let at = window.start + at;
            if let Some(name) = name_field(packet, at + 1, false) {
                push_name(&mut events, id_info.value, name, false, false);
                i = at + 2 + packet[at + 1] as usize;
                continue;
            }
        }
        i += 1;
    }
    events
}

/// `0F 1D 37 <id>` then `00 00 <len> <name>` before the block ends.
fn block_names(packet: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut i = 0;
    while i + 3 < packet.len() {
        if packet[i] != 0x0F || packet[i + 1] != 0x1D || packet[i + 2] != 0x37 {
            i += 1;
            continue;
        }
        let id_info = read_varint(packet, i + 3);
        if id_info.length <= 0 || !(100..=9_999_999).contains(&id_info.value) {
            i += 1;
            continue;
        }
        let mut scan = i + 3 + id_info.length as usize;
        let stop = packet.len().min(scan + 500);
        while scan + 2 < stop {
            if matches!(packet[scan], 0x06 | 0x0E)
                && packet[scan + 1] == 0x00
                && packet[scan + 2] == 0x36
            {
                break;
            }
            if packet[scan] == 0x00 && packet.get(scan + 1) == Some(&0x00) {
                if let Some(name) = name_field(packet, scan + 2, false) {
                    push_name(&mut events, id_info.value, name, false, false);
                    break;
                }
            }
            scan += 1;
        }
        i += 1;
    }
    events
}

/// `04/00 4C <id>`, then the name sitting in front of a `06/0E 00 36` tail.
fn terminator_names(packet: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut i = 0;
    while i + 1 < packet.len() {
        if !matches!(packet[i], 0x04 | 0x00) || packet[i + 1] != 0x4C {
            i += 1;
            continue;
        }
        let id_info = read_varint(packet, i + 2);
        if id_info.length <= 0 || !(100..=9_999_999).contains(&id_info.value) {
            i += 1;
            continue;
        }
        let from = i + 2 + id_info.length as usize;
        let stop = packet.len().saturating_sub(2).min(i + 2 + 128);
        let mut scan = from;
        while scan < stop {
            if matches!(packet[scan], 0x06 | 0x0E)
                && packet.get(scan + 1) == Some(&0x00)
                && packet.get(scan + 2) == Some(&0x36)
            {
                if let Some(name) = name_before(packet, scan, from) {
                    push_name(&mut events, id_info.value, name, false, false);
                }
                break;
            }
            scan += 1;
        }
        i += 1;
    }
    events
}

fn name_before(packet: &[u8], before: usize, min: usize) -> Option<String> {
    for len in 2..=36usize {
        for gap in 0..=1usize {
            let len_at = before.checked_sub(gap + len + 1)?;
            if len_at < min || packet.get(len_at).copied() != Some(len as u8) {
                continue;
            }
            if let Some(name) = name_field(packet, len_at, false) {
                return Some(name);
            }
        }
    }
    None
}

/// `<id varint> <F0..FF> <03|A3> <len> <name>`.
fn loot_names(packet: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut i = 0;
    while i + 2 < packet.len() {
        let marker = packet[i];
        let next = packet[i + 1];
        if !(0xF0..=0xFF).contains(&marker) || !matches!(next, 0x03 | 0xA3) {
            i += 1;
            continue;
        }
        let Some(id) = varint_ending_at(packet, i) else {
            i += 1;
            continue;
        };
        let Some(name) = name_field(packet, i + 2, false) else {
            i += 1;
            continue;
        };
        let end = i + 3 + packet[i + 2] as usize;
        push_name(&mut events, id, name, false, false);
        i = end;
    }
    events
}

fn varint_ending_at(data: &[u8], end: usize) -> Option<i32> {
    let start = end.saturating_sub(5);
    for at in start..end {
        let id = read_varint(data, at);
        if id.length > 0 && at + id.length as usize == end && (100..=9_999_999).contains(&id.value)
        {
            return Some(id.value);
        }
    }
    None
}

/// A length byte at `at`, then the name. `whole` demands the field be only
/// the name. Otherwise the leading letters and digits are the name, and a
/// guild tag or marker after them is ignored.
fn name_field(packet: &[u8], at: usize, whole: bool) -> Option<String> {
    let name_len = *packet.get(at)? as usize;
    if !(1..=48).contains(&name_len) || at + 1 + name_len > packet.len() {
        return None;
    }
    let field = &packet[at + 1..at + 1 + name_len];
    if whole {
        exact_name(field)
    } else {
        loose_name(field)
    }
}

/// The field is one character name: 1 to 12 letters and digits, one of them
/// a letter. A tutorial placeholder (`$` plus random letters) is not a name.
fn exact_name(field: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(field).ok()?;
    if is_placeholder(name) {
        return None;
    }
    let count = name.chars().count();
    let ok = (1..=12).contains(&count)
        && name.chars().all(|c| c.is_alphanumeric() && !c.is_control())
        && name.chars().any(char::is_alphabetic);
    ok.then(|| name.to_string())
}

/// Letters and digits up to the first other character. Two are required,
/// unless the name is a single CJK character. Digits alone are not a name.
fn loose_name(field: &[u8]) -> Option<String> {
    let raw = std::str::from_utf8(field).ok()?;
    let trimmed = raw.split('\0').next().unwrap_or("").trim();
        let mut name = String::new();
    for ch in trimmed.chars() {
        if !ch.is_alphanumeric() || ch.is_control() || ch == '\u{FFFD}' {
            break;
        }
        name.push(ch);
    }
    let count = name.chars().count();
    if name.is_empty()
        || !(1..=12).contains(&count)
        || !name.chars().any(char::is_alphabetic)
        || (count < 2 && !name.chars().any(is_cjk))
    {
        return None;
    }
    Some(name)
}

fn is_placeholder(raw: &str) -> bool {
    raw.strip_prefix('$')
        .is_some_and(|rest| rest.chars().count() >= 4 && rest.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn is_cjk(ch: char) -> bool {
    let cp = ch as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
        || (0x1100..=0x11FF).contains(&cp)
        || (0x3040..=0x30FF).contains(&cp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    /// A framed `04 38` packet whose body is the record after the opcode,
    /// matching the feed used by the A2Tools parser tests.
    fn feed(record: &str) -> Vec<Hit> {
        let mut packet = vec![0x00, 0x04, 0x38];
        packet.extend(hex(record));
        assert!(packet.len() < 128, "test length prefix must be one byte");
        packet[0] = packet.len() as u8;
        parse_damage(&packet, false, false)
            .into_iter()
            .filter(|hit| classify_link(hit.skill, hit.actor, hit.target).is_none())
            .collect()
    }

    fn one(record: &str) -> Hit {
        let hits = feed(record);
        assert_eq!(hits.len(), 1, "{record} -> {hits:?}");
        hits.into_iter().next().unwrap()
    }

    #[test]
    fn layout_6_additional_hits_sum_to_the_shown_total() {
        let hit = one("b1ea013600f30a40c0f4007a038000010b199b5f01000000ac52a40d021818");
        assert_eq!(hit.total(), 1700);
        assert_eq!((hit.multi_hits, hit.multi_damage), (2, 48));
    }

    #[test]
    fn layout_4_keeps_one_additional_hit() {
        let hit = one("b1ea013400f30ae0b7f800cd028bd3276101000000ac52d330010159");
        assert_eq!(hit.total(), 6227);
        assert_eq!((hit.multi_hits, hit.multi_damage), (1, 89));
    }

    #[test]
    fn a_spirit_layout_4_record_has_no_leading_field() {
        let a = one("fe9e0224009e9b01c3f8f5000302792b156002000000ac52e10301060200");
        assert_eq!(a.total(), 481);
        assert_eq!((a.multi_hits, a.multi_damage), (1, 6));
        let b = one("fe9e0224009e9b01c18601001702a7a2980001000000ac52e401030303030100");
        assert_eq!(b.total(), 228);
        assert_eq!((b.multi_hits, b.multi_damage), (3, 9));
        let c = one("fe9e0204009e9b01c3f8f5000302792b156003000000ac52b7030300");
        assert_eq!(c.total(), 439);
        assert_eq!((c.multi_hits, c.multi_damage), (0, 0));
    }

    #[test]
    fn layout_6_without_additional_hits() {
        let hit = one("b1ea011600f30a40c0f40063028000010b199b5f01000000ac52d007");
        assert_eq!(hit.total(), 976);
        assert_eq!((hit.multi_hits, hit.multi_damage), (0, 0));
    }

    #[test]
    fn a_hit_into_a_shield_is_the_value_that_got_through() {
        let a = one("F3B7024600A17B680BC8001502000002D545244E02000000F462C50401C3414A070200");
        let b = one("F3B7024400A17B181DD2001A026B5D135201000000F462BD0201C3414A070100");
        assert_eq!(a.total(), 581);
        assert_eq!(b.total(), 317);
    }

    #[test]
    fn a_monster_skill_is_damage_taken_not_meter_dps() {
        let hit = one("D14B0600D5F3027E871800020202000243ED940901000000904EEA090100");
        assert!(hit.taken);
        assert_eq!(hit.total(), 1258);
    }

    fn names_of(packet: &[u8]) -> Vec<(i32, String, bool)> {
        parse_packet(packet)
            .into_iter()
            .filter_map(|event| match event {
                Event::Name {
                    id,
                    name,
                    is_self,
                    ..
                } => Some((id, name, is_self)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_anchored_record_binds_a_player_name() {
        let packet = [0xD2, 0x09, 0xE0, 0x07, 0x04, b'A', b'r', b'i', b'a'];
        assert_eq!(
            names_of(&packet),
            vec![(1234, "Aria".to_string(), false)]
        );
    }

    #[test]
    fn a_short_name_with_a_trailing_marker_is_kept() {
        // "BH." — the period is not part of the name.
        let packet = [0xD2, 0x09, 0xE0, 0x07, 0x03, b'B', b'H', b'.'];
        assert_eq!(names_of(&packet), vec![(1234, "BH".to_string(), false)]);
    }

    #[test]
    fn one_hangul_syllable_is_a_name() {
        let packet = [0xD2, 0x09, 0xE0, 0x07, 0x03, 0xEA, 0xB0, 0x80];
        assert_eq!(names_of(&packet), vec![(1234, "가".to_string(), false)]);
    }

    #[test]
    fn a_name_longer_than_twelve_characters_is_not_a_name() {
        let mut packet = vec![0xD2, 0x09, 0xE0, 0x07, 13];
        packet.extend(b"ABCDEFGHIJKLM");
        assert!(names_of(&packet).is_empty());
    }

    #[test]
    fn a_tutorial_placeholder_is_not_a_name() {
        let mut packet = vec![0x33, 0x36, 0xD2, 0x09, 0, 0, 0, 0, 0x01, 5];
        packet.extend(b"$Abcd");
        packet.extend([0xE8, 0x03]);
        assert!(names_of(&packet).is_empty());
    }

    #[test]
    fn your_own_record_is_marked_when_the_server_follows() {
        let mut packet = vec![0x33, 0x36, 0xD2, 0x09, 0, 0, 0, 0, 0x01, 4];
        packet.extend(b"Aria");
        packet.extend([0xE8, 0x03]); // server 1000
        assert_eq!(names_of(&packet), vec![(1234, "Aria".to_string(), true)]);

        packet.truncate(packet.len() - 2);
        assert_eq!(names_of(&packet), vec![(1234, "Aria".to_string(), false)]);
    }

    #[test]
    fn a_name_after_an_actor_marker_is_bound() {
        let packet = [0x36, 0xD2, 0x09, 0x00, 0x07, 0x02, b'M', b'7'];
        assert_eq!(names_of(&packet), vec![(1234, "M7".to_string(), false)]);
    }

    /// Naicha's self record: entity 14957, server 1304.
    fn naicha_self() -> Vec<u8> {
        hex("3336ed745e91c12837064e616963686118051e000000011c0000007f0100007f0100001c000000d002040000000000")
    }

    #[test]
    fn your_self_record_names_you() {
        let packet = crate::framing::encode_frame(&naicha_self());
        assert_eq!(
            names_of(&packet),
            vec![(14957, "Naicha".to_string(), true)]
        );
    }

    #[test]
    fn your_self_record_is_read_from_a_bundle_inside_another_packet() {
        let inner = crate::framing::encode_frame(&naicha_self());
        let bundle = crate::framing::encode_bundle(&inner);
        let mut body = vec![0x01, 0x99];
        body.extend_from_slice(&bundle);
        let outer = crate::framing::encode_frame(&body);
        assert!(names_of(&outer).contains(&(14957, "Naicha".to_string(), true)));
    }

    fn links_of(packet: &[u8]) -> Vec<(i32, i32)> {
        parse_packet(packet)
            .into_iter()
            .filter_map(|event| match event {
                Event::Link { summon, owner } => Some((summon, owner)),
                _ => None,
            })
            .collect()
    }

    /// Summon 1234 owned by 1000, who is named "Aria".
    fn ownership_body() -> Vec<u8> {
        let mut body = vec![0x04, 0x8D, 0xD2, 0x09, 0, 0, 0, 0, 0xE8, 0x07, 0x00, 4];
        body.extend(b"Aria");
        body
    }

    #[test]
    fn an_ownership_record_links_the_summon_and_names_the_owner() {
        let packet = crate::framing::encode_frame(&ownership_body());
        assert_eq!(links_of(&packet), vec![(1234, 1000)]);
        assert_eq!(names_of(&packet), vec![(1000, "Aria".to_string(), false)]);
    }

    #[test]
    fn an_ownership_record_inside_a_carried_bundle_links_the_summon() {
        let inner = crate::framing::encode_frame(&ownership_body());
        let bundle = crate::framing::encode_bundle(&inner);
        let mut body = vec![0x01, 0x99];
        body.extend_from_slice(&bundle);
        let outer = crate::framing::encode_frame(&body);
        assert!(links_of(&outer).contains(&(1234, 1000)));
        assert!(names_of(&outer).contains(&(1000, "Aria".to_string(), false)));
    }

    fn summons_of(packet: &[u8]) -> Vec<Event> {
        parse_packet(packet)
            .into_iter()
            .filter(|event| matches!(event, Event::SummonSpawn { .. }))
            .collect()
    }

    fn summon(kind: u8, owner: Option<i32>, name: Option<&str>, cast: Option<i32>) -> Event {
        Event::SummonSpawn {
            summon: 1234,
            kind,
            owner,
            owner_name: name.map(String::from),
            cast_owner: cast,
        }
    }

    /// Summon 1234 of kind 0x5F, carrying "Aria" and parent key 1000.
    fn pet_spawn() -> Vec<u8> {
        let mut body = vec![0x40, 0x36, 0xD2, 0x09, 0x5F, 0x00, 0x00, 0x00, 0x01, 4];
        body.extend(b"Aria");
        body.extend([0x20, 0x4E, 0x00, 0x00, 0x06]);
        body.extend(1000u32.to_le_bytes());
        body.extend([0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0xE9, 0x03, 0x03]);
        body.extend(b"Inn");
        crate::framing::encode_frame(&body)
    }

    #[test]
    fn a_summon_spawn_gives_its_parent_key_and_owner_name() {
        assert_eq!(summons_of(&pet_spawn()), vec![summon(0x5F, Some(1000), Some("Aria"), None)]);
    }

    #[test]
    fn a_summon_spawn_does_not_name_the_summon_after_its_owner() {
        assert!(!names_of(&pet_spawn()).iter().any(|(id, _, _)| *id == 1234));
    }

    #[test]
    fn a_summon_spawn_inside_a_carried_bundle_is_read() {
        let bundle = crate::framing::encode_bundle(&pet_spawn());
        let mut body = vec![0x01, 0x99];
        body.extend_from_slice(&bundle);
        let outer = crate::framing::encode_frame(&body);
        assert!(summons_of(&outer).contains(&summon(0x5F, Some(1000), Some("Aria"), None)));
    }

    #[test]
    fn a_parent_key_needs_the_whole_owner_block() {
        let mut packet = pet_spawn();
        let at = packet.len() - 6;
        packet[at] = 0x00;
        packet[at + 1] = 0x00; // server id 0
        assert_eq!(summons_of(&packet), vec![summon(0x5F, None, Some("Aria"), None)]);
    }

    #[test]
    fn a_spirit_spawn_gives_its_buff_block_owner() {
        let mut body = vec![0x40, 0x36, 0xD2, 0x09, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x11, 0x22];
        body.extend(SPIRIT_ANCHOR);
        body.extend([0xE8, 0x07]);
        let packet = crate::framing::encode_frame(&body);
        assert_eq!(summons_of(&packet), vec![summon(0x1D, None, None, Some(1000))]);
    }

    #[test]
    fn a_spirit_anchor_naming_the_spirit_itself_is_no_owner() {
        let mut body = vec![0x40, 0x36, 0xD2, 0x09, 0x1F, 0x00, 0x00, 0x00, 0x00];
        body.extend(SPIRIT_ANCHOR);
        body.extend([0xD2, 0x09]);
        let packet = crate::framing::encode_frame(&body);
        assert_eq!(summons_of(&packet), vec![summon(0x1F, None, None, None)]);
    }

    #[test]
    fn a_ground_spell_gives_its_caster() {
        let mut body = vec![0x41, 0x36, 0xD2, 0x09, 0x1F, 0x00, 0x00, 0x00, 0x00, 0xAA, 0xBB];
        body.extend([0x07, 0x02, 0x06]);
        body.extend(1000i32.to_le_bytes());
        let packet = crate::framing::encode_frame(&body);
        assert_eq!(summons_of(&packet), vec![summon(0x1F, None, None, Some(1000))]);
    }

    #[test]
    fn a_skill_effect_gives_only_the_casters_name() {
        let mut body = vec![0x40, 0x36, 0xD2, 0x09, 0x1C, 0x00, 0x00, 0x00, 0x01, 4];
        body.extend(b"Aria");
        body.extend([0x07, 0x02, 0x06]);
        body.extend(1000i32.to_le_bytes());
        let packet = crate::framing::encode_frame(&body);
        assert_eq!(summons_of(&packet), vec![summon(0x1C, None, Some("Aria"), None)]);
    }

    #[test]
    fn a_monster_spawn_kind_is_not_a_summon() {
        let packet = crate::framing::encode_frame(&[0x41, 0x36, 0xD2, 0x09, 0x04, 0x20, 0x00, 0x11, 0x22, 0x03, 0x00, 0x00, 0x02]);
        assert!(summons_of(&packet).is_empty());
    }

    #[test]
    fn a_40_36_spawn_record_gives_a_guessed_name() {
        let mut packet = vec![0x40, 0x36, 0xD2, 0x09, 0, 0, 0, 0, 0x01, 5];
        packet.extend(b"Boros");
        let events = parse_packet(&packet);
        assert!(events.contains(&Event::Name {
            id: 1234,
            name: "Boros".into(),
            is_self: false,
            trusted: false,
        }));
    }

    #[test]
    fn a_name_record_after_the_damage_still_names_the_dealer() {
        let mut combat = crate::combat::Combat::new();
        let hit = one("b1ea011600f30a40c0f40063028000010b199b5f01000000ac52d007");
        let actor = hit.actor;
        combat.apply_hit(&hit, 0);
        let before = combat.snapshot(0);
        assert_eq!(before.players[0].name, "Unknown");
        assert_eq!(before.target_label, "Monster", "an unnamed target is not a player");

        let mut record = vec![0x44, 0x36];
        record.extend(crate::varint::encode_varint(actor as u32));
        record.extend([0, 0, 0, 0, 0x01, 6]);
        record.extend(b"Naicha");
        for event in parse_packet(&crate::framing::encode_frame(&record)) {
            combat.apply(event, 100);
        }
        let after = combat.snapshot(100);
        assert_eq!(after.players[0].name, "Naicha");
        assert_eq!(after.target_label, "Monster");
    }

    #[test]
    fn a_stats_record_names_the_local_entity() {
        let packet = crate::framing::encode_frame(&[0x4A, 0x36, 0xED, 0x74]);
        let you: Vec<_> = parse_packet(&packet)
            .into_iter()
            .filter_map(|event| match event {
                Event::You { id } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(you, vec![14957]);
    }

    /// `41 36` spawn records copied from target-debug.jsonl.
    const SPAWN_19107: &str = "7d4136a39501042c00810b20000002e3c3c5c6adc1254600b89c4600d0b64200410194a70794a707640000006400000000000000000000000000000000000000640000006400000001000000000000000000000000000000000000000600010016000000204e0000010000000014783b9ea10100001ab5e19f00";
    const SPAWN_36914: &str = "6e4136b2a0020d2000680b20000002e518b7c6c0ea844600e295465fefa043e3e401b0bd10b0bd106400000064000000000000000000000000000000000000002c0100002c010000010000000000000000000000000000000000000006000201010016000000ec828a6b00";
    const SPAWN_17151: &str = "94014136ff85010d2000d63f230000021e84ccc6798bc64600e0abc462c0a1430ce601b4aa01b4aa016400000064000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000006011101bc060000ffffffffffffffff8075d52abb030000ff850104022f75cdc6d23dc74600a0acc4020100160000003bdc0d8000";
    const SPAWN_38756: &str = "98014136e4ae02052000770c20004002bf13e1c603516b46003e96468e20854356bd019c8a0494a707640000006400000000000000000000000000000000000000640000006400000001000000000000000000000000000000000000000106011301e13c2e0a10270000000000000d223d9ea1010000fe600db09f0401001eace2c6637065460000974601010017000000c6828ae400";
    const SPAWN_25569: &str = "b9014136e1c7010f2000d73f230000028e00d0c65324c146541cabc400300443005e018c608c60640000006400000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000602110181969800ffffffffffffffff8075d52abb030000e1c70101048e00d0c65324c146541cabc41102bc060000ffffffffffffffff8075d52abb030000e1c701048e00d0c65324c146541cabc4070201001600000086308bb400";
    const DESPAWN_19069: &str = "0b4236fd94010001";

    fn spawns_of(packet: &[u8]) -> Vec<(i32, u32)> {
        parse_packet(packet)
            .into_iter()
            .filter_map(|event| match event {
                Event::Spawn { id, monster_type } => Some((id, monster_type)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_npc_spawn_gives_the_entity_and_its_monster_type() {
        assert_eq!(spawns_of(&hex(SPAWN_19107)), vec![(19107, 0x200b81)]);
        assert_eq!(spawns_of(&hex(SPAWN_36914)), vec![(36914, 0x200b68)]);
        assert_eq!(spawns_of(&hex(SPAWN_17151)), vec![(17151, 0x233fd6)]);
        assert_eq!(spawns_of(&hex(SPAWN_25569)), vec![(25569, 0x233fd7)]);
    }

    #[test]
    fn an_npc_spawn_with_the_0040_suffix_and_a_two_byte_length_is_read() {
        let packet = hex(SPAWN_38756);
        assert_eq!(packet.len(), 150);
        assert_eq!(spawns_of(&packet), vec![(38756, 0x200c77)]);
    }

    #[test]
    fn a_typed_npc_spawn_never_gives_a_name() {
        for record in [SPAWN_19107, SPAWN_36914, SPAWN_17151, SPAWN_38756, SPAWN_25569] {
            assert!(names_of(&hex(record)).is_empty(), "{record}");
        }
    }

    #[test]
    fn a_42_36_record_despawns_the_entity() {
        let despawns: Vec<i32> = parse_packet(&hex(DESPAWN_19069))
            .into_iter()
            .filter_map(|event| match event {
                Event::Despawn { id } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(despawns, vec![19069]);
        let mut longer = hex(DESPAWN_19069);
        longer.push(0x00);
        assert!(!parse_packet(&longer)
            .iter()
            .any(|event| matches!(event, Event::Despawn { .. })));
    }

    #[test]
    fn other_despawn_flags_are_removals_not_despawns() {
        let mut kill = hex(DESPAWN_19069);
        *kill.last_mut().unwrap() = 0x03;
        let events = parse_packet(&kill);
        assert!(events.contains(&Event::Removed { id: 19069, flag: 3 }));
        assert!(!events.iter().any(|e| matches!(e, Event::Despawn { .. })));
        *kill.last_mut().unwrap() = 0x07;
        assert!(parse_packet(&kill).contains(&Event::Removed { id: 19069, flag: 7 }));
    }

    #[test]
    fn an_8d_record_is_live_hp() {
        let mut packet = vec![0x10, 0x8D];
        packet.extend([0xFD, 0x94, 0x01, 0x02, 0x01, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(parse_packet(&packet).contains(&Event::LiveHp { id: 19069, hp: 0 }));
    }

    #[test]
    fn a_41_36_record_without_the_npc_shape_is_not_a_spawn() {
        let mut packet = vec![0x41, 0x36, 0xD2, 0x09, 0, 0, 0, 0, 0x01, 5];
        packet.extend(b"Boros");
        let packet = crate::framing::encode_frame(&packet);
        assert!(spawns_of(&packet).is_empty());
        assert_eq!(names_of(&packet), vec![(1234, "Boros".to_string(), false)]);

        // SPAWN_19107 is `7d 41 36 <a3 95 01> 04 <2c 00> <81 0b 20> <00 00> 02`.
        let mut bad_field = hex(SPAWN_19107);
        bad_field[7] = 0x21;
        assert!(spawns_of(&bad_field).is_empty(), "the u16 field is 20 00 or 2c 00");

        let mut bad_marker = hex(SPAWN_19107);
        bad_marker[14] = 0x03;
        assert!(spawns_of(&bad_marker).is_empty(), "the record ends in 02");

        let mut bad_suffix = hex(SPAWN_19107);
        bad_suffix[13] = 0x41;
        assert!(spawns_of(&bad_suffix).is_empty(), "00 00 or 00 40 before the 02");

        let mut no_type = hex(SPAWN_19107);
        no_type[9..12].fill(0);
        assert!(spawns_of(&no_type).is_empty(), "a zero type is not a spawn");

        let mut short = hex(SPAWN_19107);
        short.truncate(13);
        short[0] = 13;
        assert!(spawns_of(&short).is_empty(), "a cut record is not a spawn");
    }

    #[test]
    fn an_npc_spawn_with_any_flag_byte_is_read() {
        for flag in [0x00, 0x01, 0x07, 0x2A, 0x80, 0xFF] {
            let mut packet = hex(SPAWN_19107);
            packet[6] = flag;
            assert_eq!(spawns_of(&packet), vec![(19107, 0x200b81)], "flag {flag:02x}");
            assert!(names_of(&packet).is_empty(), "flag {flag:02x}");
        }
    }
}
