# Aion Tallymark

A passive, listen-only DPS meter for AION 2 with a compact overlay, a detailed meter and session replay.

> Unofficial fan project. Not affiliated with, endorsed by or sponsored by NCSOFT. AION is a trademark of NCSOFT Corporation.

<!-- Screenshots: none yet. Add real captures to docs/ and link them here. -->

## Features

- **Compact overlay**: transparent, always-on-top, resizable; remembers its position, size and opacity. Save and reset buttons in the header.
- **Detailed meter**: players table (damage, DPS, share, crit %), damage by skill, recent hits and finished pulls. Opens as an app window or at `http://127.0.0.1:18732/` in any browser.
- **Session history and replay**: save a fight, then replay it at 1x, 2x or 4x with a damage-over-time chart. Export or delete sessions.
- **Heals and ability share** views.
- **Event timers**: default AION 2 event timers you can edit, switch off or delete, with a "Restore default timers" button. Local, server or UTC clock.
- **Boss detection**: from the bundled NPC table, or once a target takes 1,500,000 player damage (adjustable).
- **Kill detection**: a *Defeated* badge when a monster dies.
- **Boss lock**: the view stays on the boss while you fight it and for 10 s after it dies (adjustable, 0 to 60 s), so adds don't replace it. Other hits are still recorded.
- **Toasts** for timers and saves, stacked two at a time.

## Passive and listen-only

The app reads copies of the game packets your PC already receives, through Npcap. It never sends anything to the game or its servers, never opens the game process and never reads game memory. Nothing is uploaded; all data stays on your PC.

## Requirements

- Windows 10 or 11 (64-bit).
- [WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (built into Windows 11; the installer downloads it if missing).
- [Npcap](https://npcap.com/) installed with **"Install Npcap in WinPcap API-compatible Mode"** checked. Without Npcap the app starts but can only run `--demo`; it shows a notice with a link.
- Run the app **as Administrator** for live capture.

## Install

Get the files from the Releases page.

- **Installer** (`aion2dps_<version>_x64-setup.exe`, recommended): installs for the current user, adds Start menu shortcuts, installs WebView2 if needed and reminds you about Npcap.
- **Portable exe** (`aion2dps.exe`): runs from anywhere with no install. You still need WebView2 and Npcap.

**Updates** are manual for now: download the new installer and run it over the old install. Your settings, timers and sessions are kept. There is no auto-update.

## Usage

- Start the game, then start the app as Administrator. Start it before a boss spawns so names and bosses are recognised.
- The **overlay** opens first. Drag to move, resize from the edges, `-`/`+` for opacity. Closing the overlay quits the app.
- Open the **detailed meter** from the overlay's hover buttons, or browse to `http://127.0.0.1:18732/` (`/overlay` gives the compact view, handy as a stream browser source; `/history` gives session history).
- **Demo mode**: `aion2dps.exe --demo` shows sample combat with no game or Npcap needed.

## Settings and data

| What | Where |
| --- | --- |
| Settings (opacity, boss lock, threshold, reset delay) | `%APPDATA%\aion2dps\settings.json` |
| Saved sessions | `%LOCALAPPDATA%\aion2dps\sessions\` |
| Event timers | `%LOCALAPPDATA%\aion2dps\alerts.json` |
| Your monster names | `%LOCALAPPDATA%\aion2dps\monster-names.json` |
| Debug logs (only when turned on) | `%LOCALAPPDATA%\aion2dps\death-debug.jsonl`, `target-debug.jsonl` |

Set `AION2DPS_DEBUG_DEATHS=1` before starting the app to log what the game sends when monsters die (see [Debugging monster deaths](#debugging-monster-deaths)). `AION2DPS_DEBUG_TARGETS=1` or `--debug-targets` logs unnamed targets. Both only read packets.

## Building from source

Prerequisites: [Rust](https://rustup.rs/) 1.77+, Node.js and npm (only for the Tauri CLI), WebView2, and Npcap for live capture.

```text
npm install                 # Tauri CLI only; the UI has no build step
npm run demo                # dev app with sample combat
npm run dev                 # dev app with live capture (elevated terminal)
npm run build               # release exe + NSIS installer
```

- Exe: `src-tauri/target/release/aion2dps.exe`
- Installer: `src-tauri/target/release/bundle/nsis/aion2dps_<version>_x64-setup.exe`

Without Node: `cargo run --manifest-path src-tauri/Cargo.toml -- --demo`.

Tests:

```text
cargo test                          # engine, page checks, no-memory-access check
node --test tests/js/*.test.js     # UI logic modules
```

## Disclaimer

This is an unofficial tool, provided as is with no warranty. It is passive and read-only, but third-party tools may still be against a game's terms of service. Check the game's rules yourself; you use it at your own risk.

## Acknowledgements

- [A2Tools DPS Meter](https://github.com/taengu/A2Tools-DPS-Meter) (GPL-3.0): packet framing and damage-record layout, the English NPC names and skill names. See [docs/third-party.md](docs/third-party.md).
- [AION2-DPS-Meter](https://github.com/taengu/AION2-DPS-Meter) (MIT), which A2Tools is partly derived from.
- [Inter](https://rsms.me/inter/) font (SIL Open Font License).
- Built with [Tauri](https://tauri.app/).
- Pet names and game reference data from [Aion2Data](https://aion2data.com) (unofficial fan reference; game data © NCSOFT).

## License

Copyright (C) 2026 Shiro

GPL-3.0-or-later. See [LICENSE](LICENSE). Third-party notices are in [docs/third-party.md](docs/third-party.md); the Inter font licence is in `ui/fonts/Inter-LICENSE.txt`.

## Details

### Windows

**Overlay** (360 x 400 by default): transparent, frameless, always on top.

- Drag anywhere on the panel to move it. Resize it from the edges and corners, and the whole layout scales with the window.
- Click (without moving) on a typed target's name to name its monster type; a press that moves still drags the window. A small *Boss* or *Dummy* badge after the name comes from the bundled names.
- `-` and `+` (or the footer buttons) change opacity. The level is remembered.
- Hover to show the buttons that open the detailed meter and close the overlay. Closing the overlay quits aion2dps and closes the detailed meter and history windows too. With **Open automatically when Aion 2 is detected** on (detailed meter → App), closing the overlay keeps aion2dps in the system tray instead; the overlay opens again when `AION2.exe` starts (only on that start, so closing it while the game is already running does not reopen it). Right-click the tray icon for Show overlay or Quit.
- It stays in the taskbar, so Alt-Tab focuses it like any other window.

**Detailed meter**: a top bar with the target, fight time, group total damage and group DPS; a players table (rank, name, total damage, DPS, % of group, crit %, share bar); a **Damage by skill** panel for the selected player; recent hits; finished pulls; and the reset-delay setting. The same page is served at `http://127.0.0.1:18732/` for any browser, and `/overlay` serves the compact view (useful as a stream browser source). The server listens on `127.0.0.1` only.

Inside the app, the windows get a combat snapshot every 200 ms as a Tauri `live` event, and actions (reset, target mode, reset delay, clear history) are Tauri commands. The browser page polls `/api/live` and posts to `/api/...`. Both use the same JSON snapshot.

### Fight lifecycle

A fight's clock pauses after 7 seconds without a player hit. A target dying does not end the fight: the totals stay for 30 seconds after the last hit or death (the reset delay, adjustable from 5 to 3600 seconds on the detailed page), damage inside that window continues the same fight, and only then does the meter return to 0. While that window runs, both views show **Fight ended - resets in Ns** and the overlay dims its rows. Reset is on the detailed page. Target modes are Current (the live target plus the ones killed in this fight), Boss (the target with the most player damage), and All targets.

**Kills and bosses.** A monster counts as killed when the game removes it (`42 36 <id> 00 03`) or its live HP (`8D`) reaches 0 and it then disappears, within 1.5 s of the last hit; a *Defeated* badge then shows in both views and in saved sessions. A target is a boss when the bundled names mark it or once it has taken 1,500,000 player damage (it stays a boss for that fight).

**Boss lock.** When a boss dies, both views hold its fight for 10 seconds (a *Locked 8s* chip next to the name) so you can save it; Save during the lock saves the boss fight. Damage on new targets during the lock is still recorded and shows when the lock ends; click the chip to end it early. Both values are under *Boss* on the detailed page (lock 0 to 60 s, 0 turns it off; threshold 1,000 or more, `1.5m` works) and are kept in `%APPDATA%\aion2dps\settings.json`.

**Lock on boss while fighting.** As soon as a boss you are fighting is detected (bundled boss flag, or the damage threshold crossed) both views stay on it with a *Locked on boss* chip (no countdown), so hitting other monsters does not switch the target. Those hits are still recorded in the same fight: they show under *All targets*, go to history when the fight ends, and the chip tooltip says they are being recorded. Engaged means: flagged a boss, took damage in this fight, not dead, hit within the idle window (30 s by default) of the fight's latest hit, and not unlocked by hand. It ends when the boss dies (then the normal *Locked Ns* countdown runs, if it is above 0), when the boss is left alone for the idle window, when the fight idles out (archived to history as usual), on Reset fight, or by clicking the chip (that boss is not held again until the next fight). A false death (HP comes back) keeps it. *Boss* mode shows the engaged boss too; *All targets* is unchanged and shows no chip. The checkbox under *Boss* turns it off (default on, applies at once). The overlay has a small reset button that does the same as *Reset fight*.

### What a number is

Hits are `04 38` records (and damage-over-time `05 38` records) from the server. Crit, back, perfect, double, and parry come from the hit itself; Crit % in the table and the per-skill breakdown use those counts. Summon damage is added to the owner when a spirit-link record is seen. Player names come from the spawn and self records in that same stream, and monster names from the monster type in the spawn record (see Naming monsters); a player whose name has not been seen yet shows as *Unknown*, and any other unnamed target (one that never dealt player damage or took monster damage) shows as *Monster*. An entity id is never shown as a name. Skill names come from the English table in A2Tools; a skill with no entry stays as its id.

### Naming monsters

Monster names are not in the packets, but each monster's `41 36` spawn record carries a monster **type** that is the same for every monster of that kind. A label for a typed monster is, in order: your name for the type, the bundled English name for the type, then *Monster*. The same order is used for the target, recent hits and finished pulls.

**Bundled names.** The app ships an English table of about 9,800 types (`data/npc-names-en.json`, from A2Tools, see `docs/third-party.md`). It is compiled into the program and read once on first use; nothing is downloaded. Its placeholder entries (`???` and `None`) are not used as names. The table also marks bosses and training dummies, shown as a small *Boss* or *Dummy* badge next to the target name.

**Your names.** You can name any type, or replace a bundled name:

- Click the target name in the detailed meter or the overlay, type a name, and press Enter. Esc cancels; an empty name clears yours and the bundled name (shown greyed in the empty editor) comes back. The small `#xxxxxx` in the editor is the type. Names are 1 to 40 characters.
- Only a target whose spawn was seen has a type. A target that spawned before the meter started shows a hint instead: *Start the meter before the target spawns*.
- A type name, yours or bundled, wins over any name read from packets. A target with no type keeps the *Monster* and *Unknown* labels.
- The table is saved to `%LOCALAPPDATA%\aion2dps\monster-names.json` as a JSON object of decimal type to name, for example `{"2100097": "Rock Golem"}`. A missing or unreadable file starts an empty table (an unreadable one is copied to `monster-names.json.bad` first). `--demo` uses `monster-names-demo.json` instead, so sample names never touch the real table.
- In a browser: `GET /api/monster-names` lists `[{"type": n, "name": "..."}]`, and `POST /api/monster-name` with `{"type": n, "name": "..."}` sets one. The POST needs `Content-Type: application/json` and the header `X-Aion2dps: 1`, so another web page cannot change your names.

### Saving sessions

**Save session** (top bar of the detailed meter, or the small download icon after the overlay timer) writes the open fight to a file. It is disabled until there is damage, and turns green with *Saved* for 2 seconds; the overlay footer shows *Session saved · mm:ss*. A session holds what the meter shows at that moment: the counted targets of the current target mode, and the players on them.

**History** (detailed meter top bar) opens the session history window, also served at `http://127.0.0.1:18732/history`. Pick a session on the left to see its summary, a damage-over-time chart and the players table. Play, pause, drag the scrubber or click the chart to replay the fight at 1x, 2x or 4x; the table shows each player's totals up to the playhead. **Export** downloads the JSON file, and **Delete session** removes it after a confirmation.

Files are saved to `%LOCALAPPDATA%\aion2dps\sessions\<UTC start>-<target>.json`, for example `20261102-231200-fediv-wraith.json` (`sessions-demo` with `--demo`). The file name without `.json` is the session id. The history lists the newest 500 files and skips files it cannot read. Each file is written to a temporary file and renamed, so a crash never leaves half a session.

Schema (`schema_version: 1`):

- `started_at`, `ended_at`: first and last hit, in milliseconds since the Unix epoch. `duration_ms`: the fight clock the meter uses for DPS (pauses of 7 s or more are not counted).
- `target`: `{label, boss, dummy, defeated, died_ms}`, the label as the meter shows it; `died_ms` is ms from the start, only when it died.
- `totals`: `{total_damage, party_dps}`.
- `players`: `[{name, total, dps, share, crit_pct}]`, by damage; `share` and `crit_pct` are percentages.
- `actors`: player names; `events` refer to them by index.
- `events`: one compact array per hit, `[t_ms, actor_idx, damage, flags, skill]`: `t_ms` from the first hit, `flags` is a bit set (1 = crit, 2 = damage over time), `skill` the skill id.
- `truncated`: the meter keeps at most 200,000 hits per fight. Past that it keeps counting the totals but stops recording hits, and the replay covers only the first 200,000.

In a browser: `POST /api/session/save` saves, `GET /api/sessions` lists, `GET /api/sessions/{id}` reads one, `GET /api/sessions/{id}/export` downloads it, and `DELETE /api/sessions/{id}` deletes it. The POST and DELETE need the header `X-Aion2dps: 1` (and `Content-Type: application/json` if they have a body), like the monster name POST. An id is 1 to 96 of `0-9 A-Z a-z _ -`, so it cannot point outside the folder, and no response contains a file path.

TODO: the round class icon next to each name is a placeholder. The decoder does not read a player's class from the packets yet.

### Options

```text
aion2dps --demo                  sample combat instead of capturing
aion2dps --port 18740            localhost port (default 18732)
aion2dps --browser               also open the detailed page in the default browser
aion2dps --no-overlay            no overlay; open the detailed meter window instead
aion2dps --no-overlay --browser  no app window at all; the browser page only
aion2dps --debug-targets         log raw packets around unnamed targets (see below)
```

### Debugging monster deaths

To check what the game sends when a monster or boss dies (step 1 of kill detection), set `AION2DPS_DEBUG_DEATHS=1` before starting the app (`set AION2DPS_DEBUG_DEATHS=1` in cmd, `$env:AION2DPS_DEBUG_DEATHS = "1"` in PowerShell), start the meter before the boss spawns, and kill it. Lines go to `%LOCALAPPDATA%\aion2dps\death-debug.jsonl`: every `42 36 <id> 00 <flag>` removal with any flag, the short `41 36 <id> 00 <flag>` shape, and HP records (`8D` live HP and `1B 92`) for damaged or boss entities, at most one per entity per second plus the first 0. Each line has the time, the raw hex (512 bytes max) and what the meter knows about the entity. Logging stops at about 20 MB. It only reads; nothing is sent to the game and combat is unchanged. Summarize it with:

```
powershell -ExecutionPolicy Bypass -File scripts\summarize-death-debug.ps1
```

or send the file.

### Debugging unnamed targets

Monsters show as *Monster* when their type is not in the bundled names and you have not named it (see Naming monsters), because the decoder only knows where player names sit in the packets. To help find the monster name, type and grade fields, turn on the target debug log, then fight a target whose name you know:

```text
aion2dps --debug-targets
```

or set the environment variable before starting the app (useful with a shortcut or the installer build):

```text
set AION2DPS_DEBUG_TARGETS=1          (cmd)
$env:AION2DPS_DEBUG_TARGETS = "1"     (PowerShell)
```

The log is written to `%LOCALAPPDATA%\aion2dps\target-debug.jsonl` (for example `C:\Users\<you>\AppData\Local\aion2dps\target-debug.jsonl`). The folder is created if needed. The release exe has no console, so the path is also shown as a note at the bottom of the overlay and in the status bar of the detailed meter, and it is in `/api/live` under `capture.target_debug`.

Each line is one JSON object:

- `session`: the app started with the log on.
- `hit`: a damage hit on a target with no known name: time, target id, actor id, skill id, amount, and the hex of the packet that carried it.
- `context`: once per target, its `monster_type` from the `41 36` spawn record (or `null` if the spawn was not seen), and up to 3 recent non-damage packets (any opcode) that contain the target id encoded as a varint, with their opcode, length, the offsets of the id, and their hex. The meter keeps the last 2000 packets in memory so a spawn packet seen before the first hit can still be found. One packet per opcode is kept; when more opcodes match, the oldest match (most likely the spawn) takes the last slot.
- `hp`: later `1B 92` HP updates for a target already logged (up to 20 per target).
- `cap`: the file reached about 20 MB and logging stopped. On the next start a full file is moved to `target-debug.old.jsonl`.

Packets longer than 4096 bytes are cut in the hex dump (`truncated: true`, with the full `len`). The log only contains packets the meter already captures; nothing is read from the game. Without the flag or variable, none of this runs.

### Layout

```text
src/              engine library (crate `aion2dps`): capture, framing, reassembly, decode,
                  combat, state, skills, demo, the localhost server (web.rs), the
                  monster name table (monster_names.rs), saved sessions
                  (sessions.rs), and the opt-in unnamed-target packet log
                  (target_debug.rs)
src-tauri/        desktop app (crate `aion2dps-app`, binary `aion2dps`), Tauri v2
ui/               frontend, plain HTML/CSS/JS with no build step
  overlay.html    compact overlay
  index.html      detailed meter (app window and http://127.0.0.1:18732/)
  history.html    saved sessions and replay (app window and /history)
  css/theme.css   design tokens; Inter is bundled in ui/fonts (no CDN)
  js/bridge.js    data source: Tauri events in the app, /api/live polling in a browser
data/             English skill names and English monster names (npc-names-en.json)
docs/design/      reference renders of the UI design
docs/third-party.md  source and license of bundled third-party data
tests/            page and no-memory-access checks
```

`src-tauri` is its own Cargo workspace, so the engine builds and tests without any Tauri crate.

### Design

Reference renders of the overlay are in `docs/design/figma-overlay.png` (live) and `docs/design/figma-overlay-idle.png` (fight ended). Colors, radii and spacing are the tokens in `ui/css/theme.css`. The overlay is designed at 360 px wide and the detailed meter at 1440 x 900.

