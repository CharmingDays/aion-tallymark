use aion2dps::{
    decode::Event,
    framing::{encode_bundle, encode_frame, packets_in},
    hidden_cubes::*,
    varint::encode_varint,
};
fn profile() -> Protocol {
    Protocol {
        templates: vec![Template {
            id: 0x123456,
            name: "Synthetic cube".into(),
            verified: true,
        }],
        ..Protocol::default()
    }
}
fn packet(op: [u8; 2], id: i32, tail: &[u8]) -> Vec<u8> {
    let mut body = op.to_vec();
    body.extend(encode_varint(id as u32));
    body.extend(tail);
    encode_frame(&body)
}
fn spawn(id: i32, kind: u32) -> Vec<u8> {
    let b = kind.to_le_bytes();
    packet([0x40, 0x36], id, &[4, 0x20, 0, b[0], b[1], b[2], 0, 0, 2])
}
fn pos(id: i32, x: f32, y: f32, z: f32) -> Vec<u8> {
    let mut tail = Vec::new();
    for n in [x, y, z] {
        tail.extend(n.to_le_bytes());
    }
    packet([0x2a, 0x38], id, &tail)
}
fn feed(d: &mut Detector, p: &[u8], now: i64) {
    d.observe_packet(p, &aion2dps::decode::parse_packet(p), now);
}
fn layout() -> PositionLayout {
    PositionLayout {
        verified: true,
        offsets: [0, 4, 8],
        units_to_meters: 1.0,
    }
}

#[test]
fn bundled_table_is_inert_no_guessed_ids_or_coordinates() {
    let p = Protocol::bundled();
    assert!(p.templates.iter().all(|t| t.id == 1_200_039));
    assert!(p.position.is_none());
    assert!(p.spawn_position.is_none());
    assert!(p.map.is_none());
    let mut d = Detector::default();
    feed(&mut d, &spawn(1234, 0x123456), 0);
    assert!(d.alerts(0).is_empty());
}
#[test]
fn only_confirmed_template_alerts_and_duplicate_spawn_is_suppressed() {
    let mut d = Detector::new(profile());
    feed(&mut d, &spawn(1234, 999), 0);
    assert!(d.alerts(0).is_empty());
    feed(&mut d, &spawn(1234, 0x123456), 10);
    feed(&mut d, &spawn(1234, 0x123456), 20);
    let a = d.alerts(20);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].status, "detected");
    assert_eq!(a[0].distance_m, None);
}
#[test]
fn unverified_table_entries_do_not_alert() {
    let mut p = profile();
    p.templates[0].verified = false;
    let mut d = Detector::new(p);
    feed(&mut d, &spawn(1234, 0x123456), 0);
    assert!(d.alerts(0).is_empty());
}
#[test]
fn removal_is_only_for_tracked_cube_and_deduplicates() {
    let mut d = Detector::new(profile());
    feed(&mut d, &spawn(1234, 0x123456), 0);
    for id in [999, 1234, 1234] {
        feed(&mut d, &packet([0x21, 0x8d], id, &[0, 1]), 20);
    }
    let a = d.alerts(20);
    assert_eq!(a.len(), 2);
    assert_eq!(a[1].status, "gone");
    assert_eq!(a[1].entity_id, 1234);
}
#[test]
fn malformed_and_embedded_marker_bytes_do_not_alert() {
    let p = profile();
    let good = spawn(1234, 0x123456);
    for end in 0..good.len() {
        assert!(observations(&good[..end], &p).is_empty());
    }
    assert!(observations(&packet([4, 0x38], 1000, &good), &p).is_empty());
    let mut ambiguous = vec![4, 0x56, 0x34, 0x12, 0, 0, 2];
    ambiguous.extend([0x56, 0x34, 0x12, 0, 0, 2]);
    assert!(observations(&packet([0x40, 0x36], 1234, &ambiguous), &p).is_empty());
    assert!(observations(&packet([0x21, 0x8d], 1234, &[0]), &p).is_empty());
}
#[test]
fn synthetic_calibrated_positions_produce_distance_then_go_last_known() {
    let mut p = profile();
    p.position = Some(layout());
    let mut d = Detector::new(p);
    d.observe_packet(
        &packet([0x4a, 0x36], 1000, &[]),
        &[Event::You { id: 1000 }],
        0,
    );
    feed(&mut d, &spawn(1234, 0x123456), 0);
    feed(&mut d, &pos(1234, 3., 4., 0.), 0);
    feed(&mut d, &pos(1000, 0., 0., 0.), 0);
    let a = d.alerts(1);
    assert_eq!(a[0].distance_m, Some(5));
    assert_eq!(a[0].direction, None, "no compass until the axes are confirmed");
    assert!(!a[0].distance_stale);
    // No new fix: the last distance stays, marked as last known.
    let late = d.alerts(DISTANCE_STALE_MS + 1);
    assert_eq!(late[0].distance_m, Some(5));
    assert!(late[0].distance_stale);
    assert_eq!(late[0].distance_age_ms, Some(DISTANCE_STALE_MS + 1));
    feed(&mut d, &pos(1000, 0., 0., 0.), 20_000);
    assert!(!d.alerts(20_000)[0].distance_stale);
}
#[test]
fn unknown_or_nonfinite_positions_are_never_used() {
    let p = profile();
    assert!(observations(&pos(1000, 3., 4., 0.), &p).is_empty());
    let mut p = profile();
    p.position = Some(layout());
    assert!(observations(&pos(1000, f32::NAN, 4., 0.), &p).is_empty());
    p.position.as_mut().unwrap().verified = false;
    assert!(observations(&pos(1000, 3., 4., 0.), &p).is_empty());
}
#[test]
fn map_change_self_spawn_and_flow_reset_clear_alerts_and_positions() {
    let mut p = profile();
    p.map = Some(MapLayout {
        verified: true,
        opcode: [0x01, 0x37],
        id_offset: 2,
    });
    let mut d = Detector::new(p);
    feed(&mut d, &spawn(1234, 0x123456), 0);
    assert_eq!(d.alerts(0).len(), 1);
    feed(&mut d, &encode_frame(&[1, 0x37, 5, 0, 0, 0]), 0);
    assert!(d.alerts(0).is_empty());
    feed(&mut d, &spawn(1234, 0x123456), 0);
    feed(&mut d, &encode_frame(&[1, 0x37, 5, 0, 0, 0]), 0);
    assert_eq!(d.alerts(0).len(), 1);
    feed(&mut d, &encode_frame(&[1, 0x37, 6, 0, 0, 0]), 0);
    assert!(d.alerts(0).is_empty());
    feed(&mut d, &spawn(1234, 0x123456), 0);
    feed(&mut d, &packet([0x33, 0x36], 1000, &[]), 0);
    assert!(d.alerts(0).is_empty());
    feed(&mut d, &spawn(1234, 0x123456), 0);
    d.reset_world();
    assert!(d.alerts(0).is_empty());
}
#[test]
fn alert_ttl_and_id_reuse_do_not_leave_stale_coordinates() {
    let mut d = Detector::new(profile());
    feed(&mut d, &spawn(1234, 0x123456), 0);
    feed(&mut d, &spawn(1234, 999), 1);
    assert_eq!(d.relative(1234, 1), None);
    assert_eq!(d.alerts(2).last().unwrap().status, "gone");
    assert!(d.alerts(ALERT_MS + 1).is_empty());
    feed(&mut d, &spawn(1234, 0x123456), ALERT_MS + 1);
    assert_eq!(d.alerts(ALERT_MS + 1).len(), 1);
}
#[test]
fn compressed_frames_use_existing_listen_only_framer() {
    let bundle = encode_bundle(&spawn(1234, 0x123456));
    let (frames, _) = packets_in(&bundle);
    let mut d = Detector::new(profile());
    for p in frames {
        feed(&mut d, &p, 0);
    }
    assert_eq!(d.alerts(0).len(), 1);
}
#[test]
fn calibrated_spawn_position_can_supply_stationary_cube_coordinates() {
    let mut p = profile();
    p.position = Some(layout());
    p.spawn_position = Some(PositionLayout {
        verified: true,
        offsets: [9, 13, 17],
        units_to_meters: 1.0,
    });
    let mut d = Detector::new(p);
    d.observe_packet(
        &packet([0x4a, 0x36], 1000, &[]),
        &[Event::You { id: 1000 }],
        0,
    );
    feed(&mut d, &pos(1000, 0., 0., 0.), 0);
    let mut tail = vec![4, 0x20, 0, 0x56, 0x34, 0x12, 0, 0, 2];
    for n in [3f32, 4f32, 0f32] {
        tail.extend(n.to_le_bytes());
    }
    feed(&mut d, &packet([0x40, 0x36], 1234, &tail), 0);
    assert_eq!(d.alerts(0)[0].distance_m, Some(5));
}
fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}
// Captured 2026-11-09 (local player 5034 / 3511 / 11478), framed again here.
const CAST_11: &str = "2a38aa27011143e7000000c8000000000000004e856dc1a1010000aa270100b04619478261f8c5318ed946";
const CAST_13: &str = "2a38aa2701133c032b0000d0070000000000008c1f6dc1a1010000aa27014d04000000ed3607473ef4d8c60911b446";
const CAST_2B_13: &str = "2b38b71b1340f7f816091027000000000000647e78c0a1010000b71b017fb2e80002c56f82466bb28b467c4d1c47";
const CAST_2B_11: &str = "2b38d65911471fc6eb02ffffffffffffffff8075d52abb030000d65901028b566cc4de6566440018cdc6";
fn round(p: Position) -> [i64; 3] {
    [p.east, p.north, p.elevation].map(|v| (v * 100.0).round() as i64)
}
#[test]
fn skill_use_packets_give_the_casters_position_in_both_layouts() {
    let (id, p) = cast_position(&encode_frame(&hex(CAST_11))).unwrap();
    assert_eq!((id, round(p)), (5034, [39239, -7948, 27847]));
    let (id, p) = cast_position(&encode_frame(&hex(CAST_13))).unwrap();
    assert_eq!((id, round(p)), (5034, [34615, -27770, 23049]));
    let (id, p) = cast_position(&encode_frame(&hex(CAST_2B_13))).unwrap();
    assert_eq!((id, round(p)), (3511, [16696, 17881, 40013]));
    let (id, p) = cast_position(&encode_frame(&hex(CAST_2B_11))).unwrap();
    assert_eq!((id, round(p)), (11478, [-945, 922, -26252]));
    // Truncated, unframed or other opcodes are ignored.
    let short = hex(CAST_11);
    assert!(cast_position(&encode_frame(&short[..short.len() - 4])).is_none());
    assert!(cast_position(&hex(CAST_11)).is_none());
    let mut other = hex(CAST_11);
    other[1] = 0x37;
    assert!(cast_position(&encode_frame(&other)).is_none());
}
fn cube_at(d: &mut Detector, east: f64, north: f64, elevation: f64) {
    feed(d, &spawn(1234, 0x123456), 0);
    d.apply(
        Observation::Position { id: 1234, position: Position { east, north, elevation } },
        0,
    );
}
/// A 2A 38 cast by `caster` at `x` metres east of the origin.
fn cast_at(caster: i32, x: f32) -> Vec<u8> {
    let mut body = vec![0x2A, 0x38];
    body.extend(encode_varint(caster as u32));
    body.extend([1, 0x11, 0x43, 0xE7, 0, 0, 0, 0xC8, 0, 0, 0, 0, 0, 0, 0, 0x4E, 0x85, 0x6D, 0xC1, 0xA1, 1, 0, 0]);
    body.extend(encode_varint(caster as u32));
    body.extend([1, 0]);
    for n in [x * 100.0, 0.0, 0.0] {
        body.extend(n.to_le_bytes());
    }
    encode_frame(&body)
}
#[test]
fn own_casts_update_distance_with_hysteresis_and_others_are_ignored() {
    let mut d = Detector::new(profile());
    d.observe_packet(&packet([0x4a, 0x36], 5034, &[]), &[Event::You { id: 5034 }], 0);
    cube_at(&mut d, 0., 0., 0.);
    assert_eq!(d.alerts(0)[0].distance_m, None, "no player fix yet");
    feed(&mut d, &cast_at(5034, 100.), 1_000);
    let a = &d.alerts(1_000)[0];
    assert_eq!((a.distance_m, a.trend), (Some(100), None));
    // Someone else's cast never moves the player.
    feed(&mut d, &cast_at(7777, 10.), 1_500);
    assert_eq!(d.alerts(1_500)[0].distance_m, Some(100));
    // 3 m closer: below the step, no trend yet.
    feed(&mut d, &cast_at(5034, 97.), 2_000);
    let a = &d.alerts(2_000)[0];
    assert_eq!((a.distance_m, a.trend), (Some(97), None));
    // 5 m from the anchor (100): closer.
    feed(&mut d, &cast_at(5034, 95.), 3_000);
    assert_eq!(d.alerts(3_000)[0].trend, Some("closer"));
    // Jitter back by 3 m keeps "closer".
    feed(&mut d, &cast_at(5034, 98.), 4_000);
    assert_eq!(d.alerts(4_000)[0].trend, Some("closer"));
    // 4 m past the anchor (95): farther.
    feed(&mut d, &cast_at(5034, 99.), 5_000);
    let a = &d.alerts(5_000)[0];
    assert_eq!((a.distance_m, a.trend, a.distance_stale), (Some(99), Some("farther"), false));
    assert!(d.alerts(5_000 + DISTANCE_STALE_MS + 1)[0].distance_stale);
}
#[test]
fn distance_is_three_dimensional() {
    let mut d = Detector::new(profile());
    d.observe_packet(&packet([0x4a, 0x36], 5034, &[]), &[Event::You { id: 5034 }], 0);
    cube_at(&mut d, 0., 0., 40.);
    feed(&mut d, &cast_at(5034, 30.), 0);
    assert_eq!(d.alerts(0)[0].distance_m, Some(50));
}

#[test]
fn embedded_bundle_spawn_is_opened() {
    let spawn = aion2dps::framing::encode_frame(&[0x40, 0x36, 0xE8, 0x07, 1, 2, 3]);
    let bundle = aion2dps::framing::encode_bundle(&spawn);
    let mut outer_body = vec![0x99, 0x37, 0x05];
    outer_body.extend_from_slice(&bundle);
    let outer = aion2dps::framing::encode_frame(&outer_body);
    let inner = aion2dps::hidden_cubes::embedded_packets(&outer);
    assert!(inner.iter().any(|p| p == &spawn), "spawn not found: {inner:?}");
}

#[test]
fn verified_cube_object_appears_and_disappears() {
    use aion2dps::hidden_cubes::{observations, Detector, Observation, Protocol};
    let hex = "3436f1ce020400a74f12000a6b1448d19d8d43009f1c470000000000507a430101017c73a9b5a101000000000000000000001a2646f6070000";
    let pkt: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    let pkt = encode_frame(&pkt);
    let p = Protocol::bundled();
    let obs = observations(&pkt, &p);
    assert!(obs.contains(&Observation::Spawn { id: 42865, template: 1_200_039 }), "{obs:?}");
    let mut d = Detector::new(p.clone());
    for o in obs { d.apply(o, 1000); }
    assert_eq!(d.alerts(1000)[0].status, "detected");
    let gone = encode_frame(&[0x35, 0x36, 0xf1, 0xce, 0x02, 0x01, 0x01, 0x00]);
    for o in observations(&gone, &p) { d.apply(o, 2000); }
    assert!(d.alerts(2000).iter().any(|a| a.status == "gone"));
}

#[test]
fn opened_cube_reports_who_opened_it() {
    use aion2dps::hidden_cubes::{observations, Detector, Observation, Protocol};
    let p = Protocol::bundled();
    let mut d = Detector::new(p.clone());
    d.apply(Observation::Spawn { id: 42865, template: 1_200_039 }, 0);
    for o in observations(&encode_frame(&[0x3A, 0x36, 0xf1, 0xce, 0x02, 0xf0, 0x7e, 0x01, 0, 0, 0]), &p) { d.apply(o, 10); }
    for o in observations(&encode_frame(&[0x35, 0x36, 0xf1, 0xce, 0x02, 0x04, 0x03, 0x00]), &p) { d.apply(o, 20); }
    let alerts = d.alerts(20);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].status, "opened");
    assert_eq!(alerts[0].opened_by_id, Some(16240));
}
