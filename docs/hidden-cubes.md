# Hidden cube discovery preview (not calibrated)

This feature is listen-only. It consumes copies of already received frames; it
never sends a packet, opens game memory, or probes the server. No cloud service
is involved. Nothing in the bundled data currently identifies a real cube.

## What was actually found locally

Read-only inspection of `%LOCALAPPDATA%\aion2dps\target-debug.jsonl` (1,533
records, including nested packet context) found one complete `2A 38` frame and
three complete `21 8D` frames. The three removals have `<entity varint> 00 01`.
No `40 36` was found in that log. `death-debug.jsonl` (2,649 records) has no
matching tags. These logs are selective combat diagnostics, not full captures:
absence does NOT prove an opcode is absent from the game.

The `2A 38` record is 49 bytes, contains an entity varint and possible float
coordinates, but there is no labelled player/cube position with which to verify
its offsets, axes, scale, or meaning. A float-looking byte sequence is NOT a
verified position. Neither a cube template ID nor a map ID layout is confirmed.

## Recognition and safety gates

* Existing TCP reassembly, length-varint framing and LZ4 expansion are reused.
  There is no new framing delimiter assumption.
* A complete top-level `40 36` frame supplies an entity ID. One unambiguous
  marker `00 [b & 0xBF == 0] 02` supplies a candidate LE24 template immediately
  before it. This is a community hypothesis, not a cube identifier by itself.
* ONLY a matching `verified: true` template in `data/hidden-cubes.json` can
  generate a nearby alert. The bundled table is intentionally empty. Never add
  all observed templates or label one a cube just because it spawned nearby.
* `21 8D <id> 00 01`, existing `42 36` removals, and decoded removal events remove
  only a tracked cube. They do not generate alerts for arbitrary entities.
* Duplicate spawns/removals are suppressed. Tracking, positions and alert history
  are bounded. New identified server flows, self spawns and confirmed map ID
  transitions reset world state. There is a calibrated map-layout hook, but no
  guessed map opcode: seamless map transitions without a self spawn/new flow
  still need live verification before enabling real templates.
* Classification uses the server direction identified by existing self/combat
  decoding, not every TCP stream. Discovery can record candidate frames before
  identity is known. Such records are explicitly marked unverified.

## Distance

Distance is computed, not an assumed field in the packet. Both a cube position
and the locally identified player's position are required. Calibrated
`spawn_position` offsets may supply the cube position from a spawn; calibrated
`position` offsets read `2A 38` updates. Both layouts default to null.

Offsets are relative to the byte immediately after the entity varint. Their
three little-endian f32 fields must be verified as east, north and elevation,
with `units_to_meters` established against an in-game distance. Invalid,
nonfinite or out-of-bounds coordinates are rejected. The distance is Euclidean
3D, with eight-way WORLD compass direction, NOT camera-relative direction.
Player coordinates expire after 2 seconds, even if no new packets arrive.
A cube is stationary until removed; fresh updates replace its stored position.
Unknown/stale distance is omitted entirely, never shown as `0m`.

## UI

Config -> Exploration -> **Hidden cube alerts** defaults on and participates in
the existing single Save button in both the desktop and browser meter. The
preview currently produces no real-game alerts until IDs are confirmed.

The overlay uses a prominent floating cyan bar for detected and amber for gone,
a cube icon, title, type and (only if known) meters/compass. It overlays rows
temporarily instead of permanently reducing the five-player layout. Detection
expires after 12 seconds and gone after 6. The X is explicitly included in the
33ms click-through cursor poll; the rest of the body still passes clicks through.
The event notification implementation is unchanged. Text is written with
textContent; middle dots are JS escapes or HTML entities.

## Exact live discovery procedure (requires a separately approved test build)

1. Do not use the existing v0.2.2 executable: it lacks this code. No app exe or
   installer was built as part of this change. After approving a test rebuild,
   close the running meter and launch THAT rebuilt executable from PowerShell:

   ```powershell
   $env:AION2DPS_DEBUG_CUBES = '1'
   & 'C:\Users\kowai\Documents\Github\aion2dps\src-tauri\target\release\aion2dps.exe'
   ```

2. Launch before logging into the character or changing zones so self identity
   is captured. Keep Hidden cube alerts on. This run is logging-only until the
   bundled template table is calibrated. Normal gameplay is all that is needed.
3. Pick a known UNCOLLECTED cube. Note local CT times for: far away; first visible;
   near it; collected; leaving; return. Record the cube type and map. Approach
   slowly with no other object interactions. Note nearby non-cube objects too.
4. At several known distances and directions, jump/dash or briefly enter combat
   and note the in-game distance/coordinates. Community claims those actions
   refresh player position; this is not yet verified here. Stop moving for more
   than 2 seconds as a stale-position check. Collect the cube, leave, and return.
5. Repeat with a second cube of the same type, then another type. Change map and
   return without collecting to test map reset and entity ID reuse.
6. Close the meter. The bounded (20 MiB, max 50,000 rows/run) local log is
   `%LOCALAPPDATA%\aion2dps\cube-discovery.jsonl`. It appends; preserve a prior log
   separately if already at the size cap. Only selected entity-opcode bytes are retained; spawn records may include
   nearby character names, so treat the log as private. No automatic upload is performed.
   Disable for the next launch with:

   ```powershell
   Remove-Item Env:AION2DPS_DEBUG_CUBES -ErrorAction SilentlyContinue
   ```

7. Correlate candidate templates across BOTH labelled cubes, confirm removals
   coincide with collection/range exit, and identify player/cube coordinate
   layouts and units against recorded values. Confirm map boundaries. Only then
   add verified template entries and calibrated layouts; rerun synthetic tests
   and obtain approval for another test build. Never enable a guessed template.
8. In the calibrated build, repeat approach/leave/re-enter/map-change. Expect one
   cyan alert per cube presence, amber on removal, no duplicate spam, no stale
   distance, and a working X while the rest of the overlay stays click-through.

## Public leads (approach only)

* a2meter `OPCODES.md`: spawn template marker and removal opcode hypotheses.
* https://github.com/benhighlanderr/Aion2-Hidden-Cube-Radar : `2A 38` position and
  intermittent player-update claims.
* https://github.com/Grachy/aion2t-dps-meter/releases/tag/v1.1.49 : duplicate,
  map-reset and stale-distance behaviour.

No external implementation was copied; these claims are not test fixtures for
real cubes. Synthetic test IDs/layouts are deliberately not in bundled data.

## Validation notes

`cargo test` includes the detector's synthetic packet tests, configuration
round-trip unit tests (isolated from real settings), existing framing tests and
passive-only source guard. Frontend tests are `node --test
 tests/js/cube-alert.test.js`. `cargo check --manifest-path src-tauri/Cargo.toml`
type-checks the desktop wiring without producing an app executable/installer.

The wider existing JS suite has an unrelated baseline failure in
`tests/js/boss-lock.test.js`: it expects `Locked 10s` while the unchanged compact
chip implementation returns `10s`. Neither that test nor its implementation was
changed in this feature.
