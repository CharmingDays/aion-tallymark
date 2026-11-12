// Config window schema: every option is one entry here, and config-panel.js
// draws the sidebar, cards, rows, search, modified dots and Undo from it.
// Adding an option = a field in StoredSettings / SettingsBody (Rust) plus one
// entry below. Pure: no DOM, so tests/js/config-schema.test.js can load it.
// Toasts & pop-ups rows are made per toast kind from toast-style.js
// (`store: 'toast'`, id `toast.<kind>.<field>`); Config shows one kind at a time.
(function (root) {
  'use strict';

  const styles = root.toastStyle || (typeof require === 'function' ? require('./toast-style.js') : null);
  /** Rows that hold no value of their own (a button, the place map). */
  const PLAIN = ['action', 'spot'];
  /** A toast's place: changed together by dragging it. */
  const SPOT_KEYS = ['position', 'x_pct', 'y_pct', 'offset_x', 'offset_y'];

  const SECTIONS = [
    { id: 'general', label: 'General', blurb: 'App start-up, tray behaviour and how you appear in the meter.' },
    { id: 'overlay', label: 'Overlay', blurb: 'The compact always-on-top window.' },
    { id: 'meter', label: 'Meter', blurb: 'When a fight resets and how bosses hold the view.' },
    { id: 'alerts', label: 'Alerts & Notifications', blurb: 'Event alert toasts, sound and position, plus the pet progress toast.' },
    { id: 'toasts', label: 'Toasts & pop-ups', blurb: 'How each pop-up looks and where it shows. Pick a type, then change its size, place, text and colours.' },
    { id: 'exploration', label: 'Exploration', blurb: 'Passive alerts for things found while exploring.' },
    { id: 'pets', label: 'Pets', blurb: 'Pet soul progress. These settings are also listed under Alerts & Notifications.' },
    { id: 'achievements', label: 'Achievements', blurb: 'Nothing to set yet.', soon: true },
    { id: 'advanced', label: 'Advanced', blurb: 'Local debug logs for finding new packet fields. They only write what the meter already read, to files on this PC.' },
  ];

  /** UTC-12 .. UTC+14 by the hour, as minutes. */
  const OFFSETS = Array.from({ length: 27 }, (_, i) => (i - 12) * 60);
  const offsetLabel = (m) => `UTC${m < 0 ? '\u2212' : '+'}${Math.abs(m / 60)}`;

  const debugLog = (id, env, what, file) => ({
    id, section: 'advanced', card: 'Debug logs', type: 'toggle', default: false, env,
    label: `${what} debug log`,
    desc: `Write ${file} in %LOCALAPPDATA%\\aion2dps. Same as AION2DPS_DEBUG_${env.toUpperCase()}=1. Applies at once.`,
    keywords: ['debug', 'log', 'env', 'jsonl', env],
  });

  const SETTINGS = [
    { id: 'minimize_to_tray', section: 'general', card: 'App', type: 'toggle', default: true, appOnly: true,
      label: 'Minimize to tray when overlay is closed',
      desc: 'Left-click the overlay close button to hide aion2dps to the tray. Turn this off to quit instead. Right-click always quits.',
      keywords: ['tray', 'close', 'quit', 'minimize', 'exit'] },
    { id: 'open_when_aion2', section: 'general', card: 'App', type: 'toggle', default: false, appOnly: true,
      label: 'Open automatically when Aion 2 is detected',
      desc: 'If the overlay is hidden in the tray, it reopens when the game starts.',
      keywords: ['tray', 'launch', 'start', 'game'] },
    { id: 'start_with_windows', section: 'general', card: 'App', type: 'toggle', default: false, appOnly: true,
      label: 'Start with Windows',
      desc: 'Runs as administrator at sign-in (scheduled task) and starts in the tray.',
      keywords: ['boot', 'login', 'sign-in', 'autostart', 'task'],
      savedNote: () => '\u201cStart with Windows\u201d updated the scheduled task.' },
    { id: 'pop_up_on_boss', section: 'general', card: 'App', type: 'toggle', default: true, appOnly: true,
      label: 'Pop up when boss detected',
      desc: 'Restore and focus the overlay once when a new boss appears while minimized or in the tray.',
      keywords: ['tray', 'focus', 'restore', 'minimized'] },
    { id: 'check_updates_on_startup', section: 'general', card: 'App', type: 'toggle', default: false, store: 'local', appOnly: true,
      label: 'Check for updates on startup',
      desc: 'Ask GitHub once when the detailed meter opens. If a newer version is out, the version pill beside the aion2dps title turns blue with a dot; nothing is downloaded or installed until you click Update now.',
      keywords: ['update', 'version', 'github', 'release', 'upgrade', 'new'] },
    { id: 'you_color', section: 'general', card: 'Appearance', type: 'color', default: '#4ade80', store: 'local',
      label: 'Your color', desc: 'Colour of your name in the overlay and in session history.',
      keywords: ['colour', 'name', 'you', 'history'] },

    { id: 'overlay_click_through', section: 'overlay', card: 'Overlay', type: 'toggle', default: true, appOnly: true,
      label: 'Click-through',
      desc: 'Clicks on the overlay body pass through to the game. The top bar and a thin resize edge stay clickable.',
      keywords: ['mouse', 'click', 'pass', 'game'] },
    { id: 'overlay_opacity', section: 'overlay', card: 'Overlay', type: 'slider', default: 100, min: 20, max: 100, step: 10, unit: '%', store: 'local',
      label: 'Opacity', desc: 'How see-through the overlay is. The \u2212 and + keys on the overlay change it too.',
      keywords: ['transparency', 'alpha', 'see-through', 'fade'] },

    { id: 'reset_after_secs', section: 'meter', card: 'Fight', type: 'stepper', default: 30, min: 5, max: 3600, step: 5, unit: 's',
      label: 'Reset damage after', desc: 'Start a new fight after this long without a hit (5\u20133600 s).',
      keywords: ['reset', 'idle', 'timeout', 'encounter', 'combat'] },
    { id: 'boss_engage_lock', section: 'meter', card: 'Boss lock', type: 'toggle', default: true,
      label: 'Lock on boss while fighting',
      desc: 'Keep showing a boss while you fight it, so hitting other monsters doesn\u2019t switch the view.',
      keywords: ['boss', 'lock', 'target', 'view'] },
    { id: 'boss_lock_secs', section: 'meter', card: 'Boss lock', type: 'stepper', default: 10, min: 0, max: 60, step: 1, unit: 's',
      label: 'Lock after boss dies', desc: 'Keep showing a boss this long after it dies, so you can save it. 0 turns it off.',
      keywords: ['boss', 'lock', 'death', 'save'] },
    { id: 'boss_damage_threshold', section: 'meter', card: 'Boss lock', type: 'damage', default: 1500000, advanced: true,
      label: 'Boss damage threshold', desc: 'A target that takes this much damage counts as a boss. Like 1,500,000 or 1.5m.',
      keywords: ['boss', 'damage', 'threshold', 'hp'] },

    { id: 'alert_enabled', section: 'alerts', card: 'Event alerts', type: 'toggle', default: true, store: 'alerts', key: 'enabled',
      label: 'Event alerts', desc: 'Warn me before scheduled events start.', keywords: ['event', 'timer', 'schedule', 'notification'] },
    { id: 'alert_lead', section: 'alerts', card: 'Event alerts', type: 'multi', default: [5], store: 'alerts', key: 'lead_mins',
      options: [[1, '1 min'], [5, '5 min'], [10, '10 min']], dependsOn: 'alert_enabled',
      label: 'Lead time', desc: 'How early to show the toast. Pick one or more.', keywords: ['early', 'before', 'minutes'] },
    { id: 'alert_sound', section: 'alerts', card: 'Event alerts', type: 'toggle', default: false, store: 'alerts', key: 'sound', dependsOn: 'alert_enabled',
      label: 'Sound', desc: 'Play a short chime with event toasts.', keywords: ['chime', 'audio', 'beep'] },
    { id: 'alert_position', section: 'alerts', card: 'Event alerts', type: 'segmented', default: 'top', store: 'alerts', key: 'position',
      options: [['top', 'Top'], ['bottom', 'Bottom']], dependsOn: 'alert_enabled',
      label: 'Position', desc: 'Where event toasts appear on screen.', keywords: ['top', 'bottom', 'screen', 'place'] },
    { id: 'alert_events', section: 'alerts', card: 'Event alerts', type: 'action', action: 'events', button: 'Manage events',
      label: 'Event list', desc: 'Built-in timers and your own events, repeat rules and per-event duration.', keywords: ['event', 'timer', 'repeat', 'custom'] },
    { id: 'alert_server_clock', section: 'alerts', card: 'Event alerts', type: 'select', default: 540, store: 'alerts', key: 'server_utc_offset_mins', advanced: true,
      options: OFFSETS.map((m) => [m, offsetLabel(m)]),
      label: 'Game server clock', desc: 'For (server) events. Unconfirmed for Global servers: check an in-game time.',
      keywords: ['server', 'clock', 'utc', 'timezone', 'offset'] },
    { id: 'pet_toasts', section: 'alerts', card: 'Pet progress toast', type: 'toggle', default: true, appOnly: true, alsoIn: ['pets', 'toasts'], toastOnly: ['pet'],
      label: 'Pet progress toast', desc: 'Show a toast when a pet soul is collected. Further drops update it in place.',
      keywords: ['pet', 'soul', 'notification', 'toast'] },
    { id: 'pet_toast_secs', section: 'alerts', card: 'Pet progress toast', type: 'stepper', default: 60, min: 10, max: 300, step: 5, unit: 's',
      appOnly: true, alsoIn: ['pets', 'toasts'], toastOnly: ['pet'], dependsOn: 'pet_toasts',
      label: 'Toast duration', desc: 'How long the toast stays after your last soul for that pet (10\u2013300 s).',
      keywords: ['pet', 'soul', 'timer', 'notification', 'toast', 'duration'] },

    { id: 'hidden_cube_alerts', section: 'exploration', card: 'Hidden cubes', type: 'toggle', default: true, alsoIn: ['toasts'], toastOnly: ['cube'],
      label: 'Hidden cube alerts', desc: 'Only confirmed cube types alert. Unknown distance is hidden.',
      keywords: ['cube', 'treasure', 'explore', 'hidden'] },

    debugLog('debug_pets', 'pets', 'Pets', 'pet-debug.jsonl'),
    debugLog('debug_position', 'position', 'Position', 'position-debug.jsonl'),
    debugLog('debug_cubes', 'cubes', 'Cubes', 'cube-discovery.jsonl'),
    ...toastDefs(),
  ];

  /** Rows for one toast kind's style, in the order Config shows them. */
  function toastRows(k) {
    const L = styles.LIMITS;
    const def = styles.defaults(k.id);
    const on = k.event ? `toast.${k.id}.enabled` : k.id === 'pet' ? 'pet_toasts' : 'hidden_cube_alerts';
    const places = styles.POSITIONS.map(([v, label]) => [v, v === 'auto' ? (k.id === 'cube' ? 'Top center (default)' : label) : label]);
    const fields = k.fields.map((f) => `{${f}}`).join(' ');
    const rows = [];
    if (k.event) {
      rows.push({ key: 'enabled', card: 'Show', type: 'toggle', label: 'Show this pop-up', desc: `${k.help} Off skips this stage for every event.`, keywords: ['on', 'off', 'hide', 'disable'] });
      rows.push({ key: 'events', card: 'Show', type: 'action', action: 'events', button: 'Manage events',
        label: 'How long it stays', desc: 'Each event sets its own toast duration (8 s by default, or until dismissed).',
        keywords: ['duration', 'time', 'seconds', 'event'] });
    }
    rows.push({ key: 'sound', card: 'Show', type: 'segmented',
      options: k.event ? [['default', 'Like Event alerts'], ['on', 'Always'], ['off', 'Never']] : [['default', 'Off'], ['on', 'Chime']],
      label: 'Sound', desc: k.event ? 'Follow Event alerts › Sound, or always or never chime for this type.' : 'Play a short chime when it shows.',
      keywords: ['chime', 'audio', 'beep'] });
    rows.push(
      { key: 'scale', card: 'Size & place', type: 'slider', min: L.scale[0], max: L.scale[1], step: 10, unit: '%',
        label: 'Size', desc: 'Scale the whole pop-up (70–150%).', keywords: ['scale', 'zoom', 'big', 'small'] },
      { key: 'position', card: 'Size & place', type: 'select', options: places, appOnly: true,
        label: 'Screen location', desc: k.id === 'cube' ? 'Where on the monitor it shows. Custom (under Advanced) is anywhere.' : 'Where on the monitor it shows. Auto follows Event alerts › Position; Custom (under Advanced) is anywhere.',
        keywords: ['position', 'place', 'corner', 'top', 'bottom', 'left', 'right', 'center'] },
      { key: 'monitor', card: 'Size & place', type: 'select', appOnly: true,
        options: [[0, 'Primary'], ...Array.from({ length: L.monitor }, (_, i) => [i + 1, `Monitor ${i + 1}`])],
        label: 'Monitor', desc: 'Which screen. A monitor that is not connected falls back to the primary one.', keywords: ['screen', 'display', 'monitor'] },
      { key: 'spot', card: 'Size & place', type: 'spot', appOnly: true, advanced: true,
        label: 'Place anywhere', desc: 'Drag the pop-up on the map, or Place on screen to drag a sample on the real monitor (drop saves, Esc cancels). Sets Screen location to Custom.',
        keywords: ['custom', 'anywhere', 'drag', 'position', 'place', 'move', 'map'] },
      { key: 'x_pct', card: 'Size & place', type: 'slider', min: L.pct[0], max: L.pct[1], step: 1, unit: '%', appOnly: true, advanced: true,
        label: 'Across (custom)', desc: '0% is the left edge, 100% the right edge, so it always fits. Changing it picks Custom.', keywords: ['custom', 'x', 'horizontal', 'anywhere', 'position'] },
      { key: 'y_pct', card: 'Size & place', type: 'slider', min: L.pct[0], max: L.pct[1], step: 1, unit: '%', appOnly: true, advanced: true,
        label: 'Down (custom)', desc: '0% is the top edge, 100% the bottom edge. Changing it picks Custom.', keywords: ['custom', 'y', 'vertical', 'anywhere', 'position'] },
      { key: 'offset_x', card: 'Size & place', type: 'stepper', min: -L.offset, max: L.offset, step: 10, unit: 'px', appOnly: true, advanced: true,
        label: 'Nudge sideways', desc: 'Fine-tune right (+) or left (\u2212) in pixels from its spot; it stays on the monitor.', keywords: ['offset', 'x', 'horizontal', 'move'] },
      { key: 'offset_y', card: 'Size & place', type: 'stepper', min: -L.offset, max: L.offset, step: 10, unit: 'px', appOnly: true, advanced: true,
        label: 'Nudge up or down', desc: 'Fine-tune down (+) or up (\u2212) in pixels from its spot; it stays on the monitor.', keywords: ['offset', 'y', 'vertical', 'move'] },
      { key: 'title', card: 'Text', type: 'text', max: L.title, placeholder: k.title,
        label: 'Title', desc: `Blank keeps the built-in text. Placeholders: ${fields}.`, keywords: ['template', 'wording', 'name', 'heading'] },
      { key: 'message', card: 'Text', type: 'text', max: L.message, placeholder: k.message,
        label: 'Message', desc: `Blank keeps the built-in text. Placeholders: ${fields}.`, keywords: ['template', 'wording', 'line', 'body'] },
      { key: 'font_px', card: 'Text', type: 'stepper', min: L.font[0], max: L.font[1], step: 1, unit: 'px',
        label: 'Text size', desc: `Size of the main line (${def.font_px} px by default).`, keywords: ['font', 'size', 'text', 'big'] },
      { key: 'accent', card: 'Colors', type: 'tcolor', fallback: k.id === 'live' ? '#4ade80' : k.id === 'imminent' ? '#fbbf24' : k.id === 'cube' ? '#22d3ee' : '#5b9dff',
        label: 'Accent colour', desc: k.id === 'pet' ? 'Edge, icon and bar. Default uses the pet family colour.' : 'Edge, icon and countdown bar.', keywords: ['color', 'colour', 'tone', 'border', 'edge'] },
      { key: 'background', card: 'Colors', type: 'tcolor', fallback: k.id === 'cube' ? '#101d25' : '#161a21', dark: true,
        label: 'Background colour', desc: 'The pop-up’s fill.', keywords: ['color', 'colour', 'fill', 'panel'] },
      { key: 'text', card: 'Colors', type: 'tcolor', fallback: '#e6e9ef', light: true,
        label: 'Text colour', desc: 'Main text; the second line is a little dimmer.', keywords: ['color', 'colour', 'font'] },
      { key: 'bg_opacity', card: 'Colors', type: 'slider', min: L.opacity[0], max: 100, step: 5, unit: '%',
        label: 'Background opacity', desc: 'See the game through the pop-up (20–100%).', keywords: ['transparency', 'alpha', 'see-through'] },
      { key: 'show_icon', card: 'Colors', type: 'toggle',
        label: 'Show icon', desc: k.id === 'pet' ? 'The pet portrait.' : k.id === 'cube' ? 'The cube symbol.' : 'The coloured dot.', keywords: ['icon', 'image', 'dot', 'portrait'] },
    );
    return rows.map((r) => ({
      ...r, id: `toast.${k.id}.${r.key}`, fullLabel: `${k.label} \u203a ${r.label}`, section: 'toasts', store: PLAIN.includes(r.type) ? undefined : 'toast', toastKind: k.id, group: `${k.id}:${r.card}`,
      default: PLAIN.includes(r.type) ? undefined : def[r.key], dependsOn: r.key === 'enabled' ? undefined : on,
      keywords: [...(r.keywords || []), 'toast', 'pop-up', 'notification', k.short.toLowerCase()],
    }));
  }

  function toastDefs() {
    return styles ? styles.KINDS.flatMap(toastRows) : [];
  }

  /** The Toasts & pop-ups rows for one kind (its style and the settings mirrored there). */
  const forToastKind = (list, kind) => list.filter((d) => (d.toastKind ? d.toastKind === kind : !d.toastOnly || d.toastOnly.includes(kind)));

  const byId = (id) => SETTINGS.find((d) => d.id === id) || null;
  const editable = (d) => !PLAIN.includes(d.type);

  /** The settings this build shows: app-only ones are left out in a browser. */
  const visible = (inApp) => SETTINGS.filter((d) => inApp || !d.appOnly);

  /** Settings drawn in `section`, mirrored ones included. */
  const inSection = (defs, section) => defs.filter((d) => d.section === section || (d.alsoIn || []).includes(section));

  /** Cards of `defs` in order: [{ card, section, defs }]. */
  function cards(defs, section) {
    const out = [];
    for (const d of defs) {
      const home = section && d.section !== section ? section : d.section;
      const group = d.group || d.card;
      let c = out.find((x) => x.group === group && x.section === d.section);
      if (!c) out.push(c = { card: d.card, group, section: d.section, home, toastKind: d.toastKind, defs: [] });
      c.defs.push(d);
    }
    return out;
  }

  const norm = (v) => (Array.isArray(v) ? JSON.stringify([...v].sort((a, b) => a - b)) : JSON.stringify(v));
  const same = (a, b) => norm(a) === norm(b);
  const isModified = (d, value) => editable(d) && value !== undefined && !same(value, d.default);

  /** Where `query` matched `d`: 'label', 'desc', 'keyword', or null. */
  function match(d, query) {
    const q = String(query || '').trim().toLowerCase();
    if (!q) return null;
    if (d.label.toLowerCase().includes(q)) return 'label';
    if (d.desc.toLowerCase().includes(q)) return 'desc';
    if ((d.keywords || []).some((k) => k.toLowerCase().includes(q))) return 'keyword';
    return null;
  }

  /** Matches across every section, grouped by card; counts per section (mirrors count too). */
  function search(defs, query) {
    const hits = defs.filter((d) => match(d, query));
    const counts = {};
    for (const s of SECTIONS) counts[s.id] = inSection(hits, s.id).length;
    return { total: hits.length, counts, cards: cards(hits) };
  }

  /** A typed value checked against the entry's limits. */
  function parse(d, raw, parseDamage) {
    if (d.type === 'damage') {
      const r = parseDamage ? parseDamage(raw) : { ok: Number.isFinite(Number(raw)), value: Number(raw), error: 'Enter a number.' };
      return r.ok ? { ok: true, value: r.value } : { ok: false, error: r.error };
    }
    if (d.type === 'stepper' || d.type === 'slider') {
      const t = String(raw).trim();
      const n = Number(t);
      if (!/^-?\d+$/.test(t) || n < d.min || n > d.max) return { ok: false, error: `${d.label}: use ${d.min} to ${d.max}${d.unit ? ` ${d.unit}` : ''}.` };
      return { ok: true, value: n };
    }
    if (d.type === 'text') {
      const t = String(raw ?? '').trim();
      if (t.length > d.max || /[\u0000-\u001f]/.test(t)) return { ok: false, error: `${d.label}: up to ${d.max} characters on one line.` };
      return { ok: true, value: t };
    }
    if (d.type === 'tcolor') {
      const t = String(raw ?? '').trim().toLowerCase();
      if (t && !/^#[0-9a-f]{6}$/.test(t)) return { ok: false, error: `${d.label}: pick a colour or Default.` };
      return { ok: true, value: t };
    }
    return { ok: true, value: raw };
  }

  /** One step up (+1) or down (-1), kept inside the limits. */
  const stepValue = (d, value, dir) => Math.min(d.max, Math.max(d.min, (Number(value) || 0) + dir * (d.step || 1)));

  /** "60 s", "On", "1, 5 min", "Top". */
  function describe(d, v, formatDamage) {
    if (v === undefined || v === null) return '\u2014';
    switch (d.type) {
      case 'toggle': return v ? 'On' : 'Off';
      case 'stepper': case 'slider': return d.unit === '%' ? `${v}%` : `${v} ${d.unit || ''}`.trim();
      case 'damage': return formatDamage ? formatDamage(v) : String(v);
      case 'multi': return v.length ? `${[...v].sort((a, b) => a - b).join(', ')} min` : 'none';
      case 'text': return v ? `\u201c${v}\u201d` : 'built-in text';
      case 'tcolor': return v || 'Default';
      case 'segmented': case 'select': { const o = (d.options || []).find(([x]) => same(x, v)); return o ? o[1] : String(v); }
      default: return String(v);
    }
  }

  /** "Saved · Toast duration changed 60 → 90 s." */
  function changeText(changes, formatDamage) {
    const kind = changes.length > 1 && changes[0].def.toastKind;
    if (kind && changes.every((c) => c.def.toastKind === kind && SPOT_KEYS.includes(c.def.key))) {
      return `${styles.kindOf(kind).label} \u203a moved to a new spot.`;
    }
    if (changes.length !== 1) return `${changes.length} settings changed.`;
    const { def, before, after } = changes[0];
    const label = def.fullLabel || def.label;
    if (def.type === 'toggle') return `${label} turned ${after ? 'on' : 'off'}.`;
    return `${label} changed ${describe(def, before, formatDamage)} \u2192 ${describe(def, after, formatDamage)}.`;
  }

  /** Undo: a stack of change groups, each [{ id, before, after }]. */
  function createHistory(limit = 50) {
    const stack = [];
    return {
      push(group) { if (group.length) { stack.push(group); if (stack.length > limit) stack.shift(); } },
      pop: () => stack.pop() || null,
      size: () => stack.length,
      clear() { stack.length = 0; },
    };
  }

  const api = { SECTIONS, SETTINGS, OFFSETS, offsetLabel, byId, editable, visible, inSection, forToastKind, cards, same, isModified, match, search, parse, stepValue, describe, changeText, createHistory };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.configSchema = api;
})(typeof window !== 'undefined' ? window : globalThis);
