//! The pages must keep showing names. Entity ids may be stored for selection,
//! but they are not the text a player reads. Also pins the overlay window
//! behaviour that is easy to lose in a redesign.

const METER_JS: &str = include_str!("../ui/js/meter.js");
const METER_HTML: &str = include_str!("../ui/index.html");
const OVERLAY_JS: &str = include_str!("../ui/js/overlay.js");
const OVERLAY_HTML: &str = include_str!("../ui/overlay.html");
const OVERLAY_CSS: &str = include_str!("../ui/css/overlay.css");
const THEME_CSS: &str = include_str!("../ui/css/theme.css");
const FORMAT_JS: &str = include_str!("../ui/js/format.js");
const NAMING_JS: &str = include_str!("../ui/js/naming.js");
const BRIDGE_JS: &str = include_str!("../ui/js/bridge.js");
const SESSION_SAVE_JS: &str = include_str!("../ui/js/session-save.js");
const HISTORY_HTML: &str = include_str!("../ui/history.html");
const HISTORY_JS: &str = include_str!("../ui/js/history.js");
const HISTORY_CSS: &str = include_str!("../ui/css/history.css");
const METER_CSS: &str = include_str!("../ui/css/meter.css");
const WEB_RS: &str = include_str!("../src/web.rs");
const APP_MAIN: &str = include_str!("../src-tauri/src/main.rs");
const TAURI_CONF: &str = include_str!("../src-tauri/tauri.conf.json");
const CAPABILITIES: &str = include_str!("../src-tauri/capabilities/default.json");

#[test]
fn pages_show_names_instead_of_user_ids() {
    let player = line_containing(METER_JS, "class=\"name\"");
    assert!(
        player.contains("${esc(p.name)}"),
        "the player row must show the character name"
    );
    assert!(!player.contains("p.id"), "the player row must not show the user id");

    let skill = line_containing(METER_JS, "esc(s.name)");
    assert!(skill.contains("${esc(s.name)}"));

    let recent = line_containing(METER_JS, "esc(r.actor)");
    assert!(recent.contains("${esc(r.actor)}"));
    assert!(recent.contains("${esc(r.skill_name)}"));
    assert!(!recent.contains("actor_id"));

    let history = line_containing(METER_JS, "esc(row.name)");
    assert!(history.contains("${esc(row.name)}"));
    assert!(!history.contains("row.id"));

    let overlay_name = line_containing(OVERLAY_JS, "esc(p.name)");
    assert!(overlay_name.contains("class=\"name\">${esc(p.name)}"));
    assert!(!overlay_name.contains("p.id"));
    assert!(FORMAT_JS.contains("m.target_label"));
    assert!(!OVERLAY_JS.contains("#{"));
}

#[test]
fn the_overlay_window_stays_transparent_frameless_and_on_top() {
    assert!(
        OVERLAY_CSS.contains("html, body { margin: 0; height: 100%; background: transparent;"),
        "the overlay page background must stay clear"
    );
    for setting in [
        ".decorations(false)",
        ".transparent(true)",
        ".always_on_top(true)",
        ".resizable(true)",
        ".skip_taskbar(false)",
    ] {
        assert!(APP_MAIN.contains(setting), "overlay window needs {setting}");
    }
    assert!(APP_MAIN.contains("\"overlay.html\""), "the app loads its own overlay page");
    assert!(
        !METER_HTML.contains("overlay.html"),
        "the detailed meter does not open the overlay page"
    );
    assert!(
        !OVERLAY_CSS.contains("app-region"),
        "a css drag region paints a solid square on the transparent window"
    );
    assert!(OVERLAY_JS.contains("bridge.startDrag()"));
    assert!(OVERLAY_JS.contains("bridge.startResize("));
    assert!(OVERLAY_HTML.contains("id=\"hit-top\""));
    assert!(OVERLAY_HTML.contains("id=\"drag-grip\""));
    assert!(OVERLAY_JS.contains("bridge.overlayPointer()"));
    assert!(OVERLAY_JS.contains("bridge.overlayHit("));
    assert!(APP_MAIN.contains("fn overlay_pointer("));
    assert!(APP_MAIN.contains("fn overlay_hit("));
    assert!(APP_MAIN.contains("set_ignore_cursor_events"));
    assert!(METER_HTML.contains("id=\"overlay-click-through\""));
    assert!(BRIDGE_JS.contains("set_overlay_click_through"));
    assert!(TAURI_CONF.contains("\"withGlobalTauri\": true"));
}

#[test]
fn minus_and_plus_change_overlay_opacity_and_the_layout_scales() {
    assert!(OVERLAY_HTML.contains("id=\"less\""));
    assert!(OVERLAY_HTML.contains("id=\"more\""));
    assert!(OVERLAY_JS.contains("event.key === '-'"));
    assert!(OVERLAY_JS.contains("event.key === '+'"));
    assert!(OVERLAY_JS.contains("--overlay-alpha"));
    assert!(OVERLAY_JS.contains("innerWidth / DESIGN_WIDTH"));
    assert!(OVERLAY_JS.contains("documentElement.style.fontSize"));
    assert!(
        !METER_HTML.contains("id=\"less\""),
        "opacity is changed on the overlay, not the detailed page"
    );
}

#[test]
fn both_pages_show_group_and_player_damage_totals() {
    assert!(OVERLAY_HTML.contains("Group total damage"));
    assert!(OVERLAY_JS.contains("short(m.encounter_damage)"), "overlay group total");
    assert!(OVERLAY_JS.contains("class=\"dmg\">${short(p.damage)}"), "overlay player total");
    assert!(OVERLAY_JS.contains("short(p.dps)") && OVERLAY_JS.contains("p.pct"));

    assert!(METER_HTML.contains("Group total damage"), "detailed page header total");
    assert!(METER_HTML.contains("id=\"group-row\""), "detailed page total row");
    assert!(METER_JS.contains("short(m.encounter_damage)"));
    assert!(METER_JS.contains("${short(p.damage)}"), "detailed page player total");
    for heading in [
        "RANK",
        "NAME",
        "TOTAL DAMAGE",
        "DPS",
        "% OF GROUP",
        "CRIT %",
        "SHARE",
    ] {
        assert!(
            METER_HTML.contains(&format!("<th>{heading}</th>")),
            "missing column {heading}"
        );
    }
}

#[test]
fn the_design_tokens_and_local_font_are_used() {
    for token in [
        "#0F1115", "#161A21", "#1E232C", "#2A303B", "#E6E9EF", "#8A93A3", "#5A6272", "#5B9DFF",
        "#4ADE80", "#F87171",
    ] {
        assert!(THEME_CSS.contains(token), "missing color {token}");
    }
    assert!(THEME_CSS.contains("../fonts/Inter-Variable.woff2"));
    for page in [METER_HTML, OVERLAY_HTML, HISTORY_HTML] {
        assert!(!page.contains("https://"), "pages load nothing from the network");
    }
}

#[test]
fn the_idle_countdown_follows_the_engine_reset_delay() {
    assert!(OVERLAY_HTML.contains("Fight ended - resets in"));
    assert!(FORMAT_JS.contains("m.reset_in_ms"));
    assert!(FORMAT_JS.contains("reset_after_secs"));
    assert!(OVERLAY_JS.contains("Damage kept ${f.resetSecs}s after kill"));
    assert!(OVERLAY_JS.contains("Idle reset in ${f.resetSecs}s"));
}

#[test]
fn an_unnamed_monster_is_not_styled_as_an_unknown_player() {
    assert!(FORMAT_JS.contains("const UNKNOWN = 'Unknown';"));
    assert!(FORMAT_JS.contains("const MONSTER = 'Monster';"));
    let meter_class = line_containing(METER_JS, "const nameClass");
    assert!(meter_class.contains("name === UNKNOWN ? 'unknown'"));
    assert!(!meter_class.contains("MONSTER"));
    let overlay_class = line_containing(OVERLAY_JS, "cls.push('unknown')");
    assert!(overlay_class.contains("p.name === UNKNOWN"));
    assert!(!overlay_class.contains("MONSTER"));
}

#[test]
fn both_pages_show_the_target_debug_log_when_it_is_on() {
    assert!(FORMAT_JS.contains("capture.target_debug"));
    assert!(METER_HTML.contains("id=\"debug-note\""));
    assert!(OVERLAY_HTML.contains("id=\"debug-note\""));
    assert!(METER_JS.contains("debugNote(c)"));
    assert!(OVERLAY_JS.contains("debugNote(data.capture)"));
    assert!(APP_MAIN.contains("target_debug::FLAG"));
    assert!(APP_MAIN.contains("capture.target_debug = Some(info)"));
}

#[test]
fn a_typed_target_can_be_named_from_both_pages() {
    for page in [METER_HTML, OVERLAY_HTML] {
        assert!(page.contains("<script src=\"js/naming.js\"></script>"));
    }
    for script in [METER_JS, OVERLAY_JS] {
        assert!(script.contains("targetNaming.create("));
        assert!(script.contains("bridge.setMonsterName(type, name)"));
        assert!(script.contains("naming.update(data, f)"));
    }
    assert!(NAMING_JS.contains("m.target_type != null"), "only a typed target opens the editor");
    assert!(NAMING_JS.contains("Start the meter before the target spawns"));
    assert!(NAMING_JS.contains("event.key === 'Enter'"));
    assert!(NAMING_JS.contains("event.key === 'Escape'"));
    assert!(NAMING_JS.contains("className = 'type-hint'"), "the type is shown inside the editor only");
    assert!(!METER_JS.contains("target_type") && !OVERLAY_JS.contains("target_type"));
    assert!(BRIDGE_JS.contains("'x-aion2dps': '1'"));
    assert!(BRIDGE_JS.contains("'/api/monster-name'"));
    assert!(APP_MAIN.contains("set_monster_name"));
    // Dragging from the name still moves the overlay once the press moves.
    assert!(OVERLAY_JS.contains("DRAG_SLOP_PX"));
    assert!(OVERLAY_JS.contains("naming.editing()"), "typing a name does not change opacity");
}

#[test]
fn both_pages_badge_a_boss_or_dummy_and_suggest_the_bundled_name() {
    for page in [METER_HTML, OVERLAY_HTML] {
        let target = line_containing(page, "id=\"target\"");
        assert!(target.contains("class=\"target\""));
        assert!(page.contains("<span class=\"target-badge\" id=\"target-badge\" hidden></span>"));
    }
    for script in [METER_JS, OVERLAY_JS] {
        assert!(script.contains("badge: $('target-badge')"));
    }
    assert!(NAMING_JS.contains("m.target_dummy ? 'Dummy' : m.target_boss ? 'Boss'"));
    assert!(NAMING_JS.contains("input.placeholder = current.bundled || 'Name this monster'"));
    assert!(NAMING_JS.contains("m.target_bundled_name"));
    assert!(NAMING_JS.contains("m.target_user_name"));
    assert!(
        OVERLAY_CSS.contains(".target-badge[hidden] { display: none; }"),
        "the badge hides while the editor is open"
    );
}

#[test]
fn both_pages_save_the_session_and_the_meter_opens_the_history() {
    let meter_save = line_containing(METER_HTML, "id=\"save-session\"");
    assert!(meter_save.contains("title=\"Save session\"") && meter_save.contains("disabled"));
    assert!(METER_HTML.contains(">Save session</span>") && METER_HTML.contains(">Saved</span>"));
    let after_dps = &METER_HTML[METER_HTML.find("id=\"dps\"").unwrap()..];
    assert!(after_dps.contains("id=\"save-session\""), "the save button comes after group DPS");
    assert!(METER_HTML.contains("id=\"open-history\""));

    let overlay_save = line_containing(OVERLAY_HTML, "id=\"save-session\"");
    assert!(overlay_save.contains("class=\"tool save"));
    assert!(overlay_save.contains("title=\"Save session\"") && overlay_save.contains("disabled"));
    let timer = OVERLAY_HTML.find("id=\"timer\"").unwrap();
    assert!(OVERLAY_HTML.find("id=\"save-session\"").unwrap() > timer, "save comes after the timer");
    assert!(!OVERLAY_HTML.contains("id=\"open-history\""), "history opens from the detailed meter only");
    assert!(!OVERLAY_JS.contains("openHistory"));
    let tags = &OVERLAY_HTML[OVERLAY_HTML.find("</header>").unwrap()..];
    assert!(tags.contains("id=\"lock-chip\"") && tags.contains("id=\"target-defeated\""), "boss tags sit below the header row");
    assert!(OVERLAY_JS.contains("`Session saved · ${clock(summary.duration_ms)}`"));

    for page in [METER_HTML, OVERLAY_HTML] {
        assert!(page.contains("<script src=\"js/session-save.js\"></script>"));
    }
    for script in [METER_JS, OVERLAY_JS] {
        assert!(script.contains("window.sessionSave.create($('save-session')"));
        assert!(script.contains("bridge.saveSession()"));
        assert!(script.contains("saving.update(m.encounter_damage > 0)"), "disabled with no damage");
    }
    assert!(METER_JS.contains("bridge.openHistory()"));
    assert!(SESSION_SAVE_JS.contains("const SAVED_MS = 2000;"));
    assert!(SESSION_SAVE_JS.contains("settle('saved'"));
    assert!(METER_CSS.contains(".save-btn:hover:not(:disabled) { border-color: var(--accent)"));
    assert!(METER_CSS.contains(".save-btn.saved, .save-btn.saved:hover { opacity: 1; border-color: var(--live); color: var(--live); }"));
    assert!(OVERLAY_CSS.contains(".tool.save.saved { opacity: 1; border-color: var(--live); color: var(--live); }"));
}

#[test]
fn the_history_window_lists_replays_and_deletes_sessions() {
    for sheet in ["css/theme.css", "css/meter.css", "css/history.css"] {
        assert!(HISTORY_HTML.contains(&format!("<link rel=\"stylesheet\" href=\"{sheet}\">")));
    }
    for script in ["format", "bridge", "history"] {
        assert!(HISTORY_HTML.contains(&format!("<script src=\"js/{script}.js\"></script>")));
    }
    assert!(HISTORY_HTML.contains("<header class=\"topbar\">"), "same chrome as the meter");
    assert!(HISTORY_HTML.contains("No saved sessions yet"));
    assert!(HISTORY_HTML.contains(">Export</button>"));
    assert!(HISTORY_HTML.contains("class=\"btn danger\" id=\"delete\""));
    assert!(HISTORY_HTML.contains(">Delete session</button>"));
    assert!(HISTORY_CSS.contains(".btn.danger { color: var(--danger); }"));
    assert!(HISTORY_CSS.contains(".session-item.sel"));
    assert!(HISTORY_CSS.contains(".s-rename"));
    assert!(HISTORY_CSS.contains(".s-edit"));
    for heading in ["RANK", "NAME", "TOTAL DAMAGE", "DPS", "% OF GROUP", "CRIT %", "SHARE"] {
        assert!(HISTORY_HTML.contains(&format!("<th>{heading}</th>")), "missing column {heading}");
    }
    for speed in ["1x", "2x", "4x"] {
        assert!(HISTORY_HTML.contains(&format!(">{speed}</button>")));
    }
    assert!(HISTORY_HTML.contains("id=\"scrub\"") && HISTORY_HTML.contains("id=\"playhead\""));
    assert!(HISTORY_JS.contains("requestAnimationFrame(frame)"));
    assert!(HISTORY_JS.contains("if (playhead >= span) pause();"), "playback stops at the end");
    assert!(HISTORY_JS.contains("if (!confirm("), "delete asks first");
    assert!(HISTORY_JS.contains("<span class=\"name\">${esc(p.name)}</span>"));
    assert!(HISTORY_JS.contains("const displayName = "));
    assert!(HISTORY_JS.contains("bridge.renameSession"));
    assert!(HISTORY_JS.contains("data-rename="));
    assert!(HISTORY_JS.contains("class=\"s-edit\""));

    assert!(BRIDGE_JS.contains("post('/api/session/save', null, WRITE)"));
    assert!(BRIDGE_JS.contains("send('DELETE', sessionPath(id), null, WRITE)"));
    assert!(BRIDGE_JS.contains("send('PATCH', sessionPath(id), { name }, WRITE)"));
    assert!(BRIDGE_JS.contains("get('/api/sessions')"));
    assert!(BRIDGE_JS.contains("`${sessionPath(id)}/export`"));
    for command in ["save_session", "list_sessions", "get_session", "delete_session", "rename_session", "open_history"] {
        assert!(BRIDGE_JS.contains(&format!("invoke('{command}'")), "bridge calls {command}");
        assert!(APP_MAIN.contains(&format!("fn {command}(")), "app has {command}");
    }
    assert!(BRIDGE_JS.contains("window.open('/history', '_blank')"));
    assert!(APP_MAIN.contains("\"history.html\""));
    assert!(APP_MAIN.contains("#![cfg_attr(not(debug_assertions), windows_subsystem = \"windows\")]"));
    assert!(CAPABILITIES.contains("\"windows\": [\"overlay\", \"meter\", \"history\", \"toast-*\"]"));
    assert!(WEB_RS.contains("\"/history\" => \"/history.html\""));
    for route in ["\"/api/session/save\"", "\"/api/sessions\"", "\"/api/sessions/:id\"", "\"/api/sessions/:id/export\""] {
        assert!(WEB_RS.contains(route), "missing route {route}");
    }
}

fn line_containing<'a>(page: &'a str, needle: &str) -> &'a str {
    page.lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle}"))
}
