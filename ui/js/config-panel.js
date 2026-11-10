// Config window, from the Figma "Config redesign" frames: a sidebar of
// sections, search across every section, and schema-driven rows in cards
// (js/config-schema.js). Every change saves at once; the footer says what was
// saved with a small green "Saved", and Undo puts the last change back.
// meter.js feeds it the live settings (update) and the alerts notifier (attach).
(() => {
  const panel = document.getElementById('config-panel');
  const button = document.getElementById('open-config');
  const alertsBtn = document.getElementById('open-alerts');
  if (!panel || !button) return;
  const schema = window.configSchema;
  const bridge = window.meterBridge;
  const bossLock = window.bossLock;
  const pcApi = window.playerColors;
  const { esc } = window.fmt;
  const $ = (sel) => panel.querySelector(sel);

  const OPACITY_KEY = 'aion2dps.opacity';
  const SEEN_KEY = 'aion2dps.config.seen';
  /** Settings that show a NEW badge until Config has been closed once after seeing them. */
  const FRESH = ['overlay_opacity', 'debug_pets', 'debug_position', 'debug_cubes', 'pet_toasts', 'pet_toast_secs'];
  const SAVED_MS = 2400;
  const STATUS_MS = 6000;
  const ENV_NAME = { pets: 'AION2DPS_DEBUG_PETS', position: 'AION2DPS_DEBUG_POSITION', cubes: 'AION2DPS_DEBUG_CUBES' };

  const ICONS = {
    general: '<path d="M2.5 4.5h6M11.5 4.5h2M2.5 8h2M7.5 8h6M2.5 11.5h7M12.5 11.5h1"/><circle cx="10" cy="4.5" r="1.5"/><circle cx="6" cy="8" r="1.5"/><circle cx="11" cy="11.5" r="1.5"/>',
    overlay: '<path d="M8 2.5l5.5 3L8 8.5l-5.5-3z"/><path d="M2.5 8.2L8 11.2l5.5-3M2.5 10.8L8 13.8l5.5-3"/>',
    meter: '<path d="M2.5 2.5v11h11"/><path d="M5.5 11V8M8.5 11V5.5M11.5 11V7"/>',
    alerts: '<path d="M8 2.2a3.8 3.8 0 0 0-3.8 3.8v2.6L3 10.8h10l-1.2-2.2V6A3.8 3.8 0 0 0 8 2.2zM6.6 12.8a1.5 1.5 0 0 0 2.8 0"/>',
    exploration: '<circle cx="8" cy="8" r="5.5"/><path d="M10.3 5.7L9 9 5.7 10.3 7 7z"/>',
    pets: '<circle cx="4.5" cy="6" r="1.3"/><circle cx="7" cy="3.8" r="1.3"/><circle cx="10" cy="4.3" r="1.3"/><circle cx="12" cy="7" r="1.3"/><path d="M5.5 12.5c0-2 1.5-3.8 3.3-3.8s2.7 1.8 2.7 3c0 1.3-1.3 1.4-3 1.1-1.5-.3-3 .9-3-.3z"/>',
    achievements: '<path d="M5 2.5h6v3a3 3 0 0 1-6 0zM5 3.5H2.8a2 2 0 0 0 2.4 3M11 3.5h2.2a2 2 0 0 1-2.4 3M8 8.5v3M5.5 13.5h5M6.5 11.5h3"/>',
    advanced: '<path d="M9.8 2.7a3.2 3.2 0 0 0-3.9 4.1L2.6 10.1a1.1 1.1 0 0 0 0 1.6l1.7 1.7a1.1 1.1 0 0 0 1.6 0l3.3-3.3a3.2 3.2 0 0 0 4.1-3.9l-1.9 1.9-1.9-.4-.4-1.9z"/>',
  };
  const icon = (id) => `<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">${ICONS[id] || ''}</svg>`;
  const RESET_ICON = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 8a4.5 4.5 0 1 0 1.4-3.3M3.5 2.8v2.4h2.4" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  const CHECK = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>';

  const defs = schema.visible(bridge.inApp);
  const vals = {};
  let env = {};
  let alertSettings = null;
  let notifier = null;
  let current = 'general';
  let query = '';
  const openAdvanced = new Set();
  const busy = new Set();
  const history = schema.createHistory();
  let flash = { ids: new Set(), until: 0 };
  let statusTimer = null;
  let flashTimer = null;
  let seen = readSeen();

  function readSeen() {
    try { return new Set(JSON.parse(localStorage.getItem(SEEN_KEY) || '[]')); } catch (_) { return new Set(); }
  }
  const isFresh = (d) => FRESH.includes(d.id) && !seen.has(d.id);

  function readOpacity() {
    const n = Number(localStorage.getItem(OPACITY_KEY));
    if (!Number.isFinite(n) || n <= 0) return 100;
    return Math.min(100, Math.max(20, Math.round(n * 10) * 10));
  }

  function readLocal() {
    vals.you_color = pcApi.readYouColor();
    vals.overlay_opacity = readOpacity();
  }

  function readAlerts(s) {
    if (!s) return;
    alertSettings = s;
    for (const d of defs) if (d.store === 'alerts' && !busy.has(d.id)) vals[d.id] = s[d.key] ?? d.default;
  }

  /** The live settings from the meter; returns true when something changed. */
  function readSettings(s) {
    let changed = false;
    for (const d of defs) {
      if (d.store || !schema.editable(d) || busy.has(d.id) || !(d.id in s)) continue;
      if (!schema.same(vals[d.id], s[d.id])) { vals[d.id] = s[d.id]; changed = true; }
    }
    const nextEnv = s.debug_env || {};
    if (JSON.stringify(nextEnv) !== JSON.stringify(env)) { env = nextEnv; changed = true; }
    return changed;
  }

  async function loadAlerts() {
    try { readAlerts(notifier ? await notifier.reload() : await bridge.getAlerts()); } catch (_) { /* row shows defaults */ }
  }

  // ---- saving -------------------------------------------------------------

  async function write(d, v) {
    if (d.id === 'you_color') { pcApi.writeYouColor(v); return; }
    if (d.id === 'overlay_opacity') { localStorage.setItem(OPACITY_KEY, (v / 100).toFixed(1)); return; }
    if (d.store === 'alerts') {
      if (!alertSettings) throw new Error('Alerts are not loaded yet.');
      const next = JSON.parse(JSON.stringify(alertSettings));
      next[d.key] = v;
      readAlerts(await bridge.setAlerts(next));
      if (notifier) notifier.reload();
      return;
    }
    switch (d.id) {
      case 'reset_after_secs': return bridge.setResetSecs(v);
      case 'boss_lock_secs': return bridge.setBossSettings({ lockSecs: v });
      case 'boss_damage_threshold': return bridge.setBossSettings({ damageThreshold: v });
      case 'boss_engage_lock': return bridge.setBossSettings({ engageLock: v });
      case 'open_when_aion2': return bridge.setOpenWhenAion2(v);
      case 'start_with_windows': return bridge.setStartWithWindows(v);
      case 'pop_up_on_boss': return bridge.setPopUpOnBoss(v);
      case 'overlay_click_through': return bridge.setOverlayClickThrough(v);
      case 'hidden_cube_alerts': return bridge.setHiddenCubeAlerts(v);
      case 'pet_toasts': return bridge.setPetToasts({ on: v });
      case 'pet_toast_secs': return bridge.setPetToasts({ secs: v });
      case 'debug_pets': return bridge.setDebugLogs({ pets: v });
      case 'debug_position': return bridge.setDebugLogs({ position: v });
      case 'debug_cubes': return bridge.setDebugLogs({ cubes: v });
      default: throw new Error(`Unknown setting ${d.id}`);
    }
  }

  const errText = (d, err) => {
    const m = String((err && err.message) || err || '').trim();
    return `${d.label}: ${m || 'could not save.'}`;
  };

  /** Saves each [{ d, v }] in turn; `undo` does not add to the Undo history. */
  async function apply(list, undo) {
    const todo = list.filter(({ d, v }) => !schema.same(vals[d.id], v));
    if (!todo.length) return;
    const done = [];
    let error = '';
    for (const { d, v } of todo) {
      const before = vals[d.id];
      busy.add(d.id);
      vals[d.id] = v;
      setStatus('Saving\u2026', 'busy');
      render();
      try {
        await write(d, v);
        done.push({ def: d, before, after: v });
      } catch (err) {
        vals[d.id] = before;
        error = errText(d, err);
      } finally {
        busy.delete(d.id);
      }
    }
    if (done.length && !undo) history.push(done.map((c) => ({ id: c.def.id, before: c.before, after: c.after })));
    if (error) setStatus(error, 'bad');
    else if (done.length) saved(done, undo);
    render();
  }

  function saved(done, undo) {
    clearTimeout(flashTimer);
    flash = { ids: new Set(done.map((c) => c.def.id)), until: Date.now() + SAVED_MS };
    flashTimer = setTimeout(() => { flash = { ids: new Set(), until: 0 }; paintHeadSaved(); }, SAVED_MS);
    const note = done.length === 1 && done[0].def.savedNote && !undo ? ` ${done[0].def.savedNote(done[0].after)}` : '';
    const text = `${undo ? 'Undone \u00b7 ' : ''}${schema.changeText(done, bossLock.formatDamage)}${note}`;
    setStatus(text, 'ok');
    paintHeadSaved();
  }

  function setStatus(text, kind) {
    clearTimeout(statusTimer);
    const el = $('#cfg-status');
    el.className = `cfg-status ${kind || ''}`;
    if (kind === 'ok') {
      el.innerHTML = `<span class="cfg-ok">${CHECK}Saved</span><span class="cfg-status-text">\u00b7 ${esc(text)}</span>`;
      statusTimer = setTimeout(() => setStatus('All changes saved', 'idle'), STATUS_MS);
    } else if (kind === 'idle') {
      el.innerHTML = `${CHECK}<span class="cfg-status-text">${esc(text)}</span>`;
    } else {
      el.textContent = text;
    }
    paintFoot();
  }

  /** The small green "Saved" by the title, fading out. */
  function paintHeadSaved() {
    const el = $('#cfg-saved-head');
    const on = flash.until > Date.now();
    el.hidden = !on;
    if (on) { el.classList.remove('fade'); void el.offsetWidth; el.classList.add('fade'); }
  }

  function paintFoot() {
    $('#cfg-undo').disabled = history.size() === 0 || busy.size > 0;
    $('#cfg-undo').hidden = Boolean(query);
  }

  function undo() {
    const group = history.pop();
    if (!group) return;
    apply(group.slice().reverse().map((c) => ({ d: schema.byId(c.id), v: c.before })), true);
  }

  const resetList = (list) => list.filter((d) => schema.isModified(d, vals[d.id]) && !lockedByEnv(d)).map((d) => ({ d, v: d.default }));

  // ---- drawing ------------------------------------------------------------

  const lockedByEnv = (d) => Boolean(d.env && env[d.env]);
  const disabled = (d) => lockedByEnv(d) || (d.dependsOn && vals[d.dependsOn] === false);
  const sectionOf = (id) => schema.SECTIONS.find((s) => s.id === id);

  function mark(text) {
    const q = query.trim();
    const t = String(text);
    const i = q ? t.toLowerCase().indexOf(q.toLowerCase()) : -1;
    if (i < 0) return esc(t);
    return `${esc(t.slice(0, i))}<mark>${esc(t.slice(i, i + q.length))}</mark>${esc(t.slice(i + q.length))}`;
  }

  function control(d) {
    const v = vals[d.id];
    const off = disabled(d) ? ' disabled' : '';
    switch (d.type) {
      case 'toggle':
        return `<button type="button" class="switch${v || lockedByEnv(d) ? ' on' : ''}" role="switch" aria-checked="${Boolean(v || lockedByEnv(d))}" aria-label="${esc(d.label)}" data-act="toggle"${off}></button>`;
      case 'stepper':
        return `<span class="cfg-stepper"><button type="button" data-act="dec" aria-label="Less"${off}>\u2212</button><input data-act="num" inputmode="numeric" value="${esc(v ?? d.default)}" aria-label="${esc(d.label)}"${off}><button type="button" data-act="inc" aria-label="More"${off}>+</button></span><span class="cfg-unit">${esc(d.unit || '')}</span>`;
      case 'damage':
        return `<input class="cfg-input" data-act="num" inputmode="numeric" value="${esc(bossLock.formatDamage(v ?? d.default))}" aria-label="${esc(d.label)}"${off}>`;
      case 'slider':
        return `<input type="range" class="cfg-range" data-act="range" min="${d.min}" max="${d.max}" step="${d.step}" value="${esc(v ?? d.default)}" aria-label="${esc(d.label)}"${off}><span class="cfg-unit cfg-range-val">${esc(schema.describe(d, v ?? d.default))}</span>`;
      case 'segmented':
      case 'multi':
        return `<span class="cfg-seg">${d.options.map(([x, label]) => {
          const on = d.type === 'multi' ? (v || []).includes(x) : schema.same(v, x);
          return `<button type="button" class="${on ? 'on' : ''}" data-act="${d.type}" data-v="${esc(JSON.stringify(x))}"${off}>${esc(label)}</button>`;
        }).join('')}</span>`;
      case 'select':
        return `<select class="cfg-select" data-act="select" aria-label="${esc(d.label)}"${off}>${d.options.map(([x, label]) => `<option value="${esc(JSON.stringify(x))}"${schema.same(v, x) ? ' selected' : ''}>${esc(label)}</option>`).join('')}</select>`;
      case 'color':
        return `<span class="cfg-swatches">${pcApi.PRESETS.map((c) => `<button type="button" data-act="swatch" data-v="${c}" style="--c:${c}" class="${c === v ? 'on' : ''}" aria-label="${c}"></button>`).join('')}<input type="color" data-act="color" value="${esc(v || d.default)}" aria-label="Custom colour"></span>`;
      case 'action':
        return `<button type="button" class="btn small cfg-action" data-act="${d.action}">${esc(d.button)} <span aria-hidden="true">\u203a</span></button>`;
      default: return '';
    }
  }

  function row(d, where) {
    const mod = schema.isModified(d, vals[d.id]);
    const badges = [];
    if (isFresh(d)) badges.push('<span class="cfg-badge new">NEW</span>');
    if (query && (d.alsoIn || []).length) badges.push(`<span class="cfg-badge">ALSO IN ${esc(d.alsoIn.map((s) => sectionOf(s).label).join(', ').toUpperCase())}</span>`);
    if (where === 'desc') badges.push('<span class="cfg-badge">IN DESCRIPTION</span>');
    if (where === 'keyword') badges.push('<span class="cfg-badge">KEYWORD</span>');
    if (lockedByEnv(d)) badges.push('<span class="cfg-badge env">ENV</span>');
    const savedChip = flash.ids.has(d.id) && flash.until > Date.now() ? '<span class="cfg-saved-chip">Saved</span>' : '';
    const desc = lockedByEnv(d) ? `${d.desc} On now because ${ENV_NAME[d.env]} is set.` : d.desc;
    const reset = mod && !lockedByEnv(d)
      ? `<button type="button" class="cfg-reset" data-act="reset" title="Reset to ${esc(schema.describe(d, d.default, bossLock.formatDamage))}">${RESET_ICON}</button>`
      : '<span class="cfg-reset-gap"></span>';
    const wide = ['stepper', 'damage', 'slider', 'segmented', 'multi', 'select', 'color'].includes(d.type) ? ' wide-ctl' : '';
    return `<div class="cfg-row${mod ? ' mod' : ''}${disabled(d) && !lockedByEnv(d) ? ' dim' : ''}${wide}" data-id="${d.id}">
      <div class="cfg-text"><div class="cfg-label">${mod ? '<i class="cfg-dot" title="Changed from default"></i>' : ''}<span>${mark(d.label)}</span>${badges.join('')}${savedChip}</div>
      <div class="cfg-desc">${query ? mark(desc) : esc(desc)}</div></div>
      <div class="cfg-ctl">${control(d)}${schema.editable(d) ? reset : ''}</div></div>`;
  }

  function card(c, section) {
    const rows = c.defs.filter((d) => query || !d.advanced).map((d) => row(d, query ? schema.match(d, query) : null));
    const adv = query ? [] : c.defs.filter((d) => d.advanced);
    const key = `${c.section}:${c.card}`;
    if (adv.length) {
      const open = openAdvanced.has(key);
      const extra = adv.length === 1 ? adv[0].label : `${adv.length} settings`;
      rows.push(`<button type="button" class="cfg-fold${open ? ' open' : ''}" data-fold="${esc(key)}"><span class="cfg-chev" aria-hidden="true">\u203a</span>Advanced <span class="cfg-count">${adv.length}</span><span class="cfg-fold-hint">${esc(extra)}</span></button>`);
      if (open) adv.forEach((d) => rows.push(row(d, null)));
    }
    const title = query ? `${sectionOf(c.section).label} \u203a ${c.card}` : c.card;
    const fresh = !query && c.defs.some(isFresh) && c.defs.every((d) => FRESH.includes(d.id)) ? '<span class="cfg-badge new">NEW</span>' : '';
    let aside = '';
    if (!query && c.section !== section) aside = `Also listed under ${sectionOf(c.section).label}`;
    else if (!query && c.defs.some((d) => (d.alsoIn || []).length)) aside = `Also listed under ${c.defs.find((d) => d.alsoIn).alsoIn.map((s) => sectionOf(s).label).join(', ')}`;
    return `<div class="cfg-card"><div class="cfg-card-head"><span>${esc(title.toUpperCase())}</span>${fresh}${aside ? `<span class="cfg-aside">${esc(aside)}</span>` : ''}</div>${rows.join('')}</div>`;
  }

  function renderContent() {
    const box = $('#cfg-content');
    let html;
    if (query.trim()) {
      const res = schema.search(defs, query);
      html = `<div class="cfg-sec-head"><div><h3>${res.total} result${res.total === 1 ? '' : 's'} for \u201c${esc(query.trim())}\u201d</h3>
        <p>Matches labels, descriptions and keywords across every section. Sections with no match are dimmed in the sidebar.</p></div></div>
        ${res.cards.map((c) => card(c, null)).join('') || '<div class="cfg-empty">No setting matches. Try another word.</div>'}`;
    } else {
      const sec = sectionOf(current);
      const list = schema.inSection(defs, current);
      const canReset = resetList(list).length > 0;
      html = `<div class="cfg-sec-head"><div><h3>${esc(sec.label)}</h3><p>${esc(sec.blurb)}</p></div>
        ${list.length ? `<button type="button" class="btn small cfg-reset-sec" data-act="reset-section"${canReset ? '' : ' disabled'}>${RESET_ICON}Reset section</button>` : ''}</div>
        ${list.length ? schema.cards(list, current).map((c) => card(c, current)).join('') : `<div class="cfg-empty">${sec.soon ? 'Achievements are coming soon. Their settings will show up here.' : 'Nothing to set here in the browser. Open the app for these settings.'}</div>`}`;
    }
    box.innerHTML = html;
  }

  function renderNav() {
    const res = query.trim() ? schema.search(defs, query) : null;
    $('#cfg-nav-list').innerHTML = schema.SECTIONS.map((s) => {
      const list = schema.inSection(defs, s.id);
      const count = res ? res.counts[s.id] : list.length;
      const mod = list.some((d) => schema.isModified(d, vals[d.id]));
      const dim = res ? count === 0 : s.soon || !list.length;
      const on = !res && s.id === current;
      const tail = !res && (s.soon || !list.length) ? '<span class="cfg-soon">SOON</span>' : `<span class="cfg-count${res && count ? ' hit' : ''}">${count}</span>`;
      return `<button type="button" class="cfg-nav-item${on ? ' on' : ''}${dim ? ' dim' : ''}" data-section="${s.id}" title="${esc(s.label)}"${on ? ' aria-current="page"' : ''}>
        ${icon(s.id)}<span class="cfg-nav-label">${esc(s.label)}</span>${mod && !res ? '<i class="cfg-dot"></i>' : ''}${tail}</button>`;
    }).join('');
  }

  function render() {
    if (panel.hidden) return;
    renderNav();
    renderContent();
    paintFoot();
    const n = query.trim() ? schema.search(defs, query).total : 0;
    $('#cfg-search-count').textContent = query.trim() ? `${n} result${n === 1 ? '' : 's'}` : '';
    $('#cfg-search-clear').hidden = !query;
    $('#cfg-kbd').hidden = Boolean(query);
    const st = $('#cfg-status').classList;
    if (query && !st.contains('bad') && !st.contains('busy') && !st.contains('ok')) {
      $('#cfg-status').className = 'cfg-status idle';
      $('#cfg-status').innerHTML = `${CHECK}<span class="cfg-status-text">Esc clears the search \u00b7 Enter jumps to the first result in its section.</span>`;
    }
  }

  // ---- events -------------------------------------------------------------

  function rowDef(el) {
    const r = el.closest('.cfg-row');
    return r ? schema.byId(r.dataset.id) : null;
  }

  function commitNumber(input) {
    const d = rowDef(input);
    if (!d) return;
    const r = schema.parse(d, input.value, bossLock.parseBossDamage);
    input.classList.toggle('bad', !r.ok);
    if (!r.ok) { setStatus(r.error, 'bad'); return; }
    apply([{ d, v: r.value }]);
  }

  panel.addEventListener('click', (event) => {
    const nav = event.target.closest('[data-section]');
    if (nav) {
      current = nav.dataset.section;
      if (query) { query = ''; $('#cfg-search').value = ''; setStatus('All changes saved', 'idle'); }
      render();
      $('#cfg-content').scrollTop = 0;
      return;
    }
    const fold = event.target.closest('[data-fold]');
    if (fold) {
      const k = fold.dataset.fold;
      if (openAdvanced.has(k)) openAdvanced.delete(k); else openAdvanced.add(k);
      render();
      return;
    }
    const act = event.target.closest('[data-act]');
    if (!act || act.disabled) return;
    const d = rowDef(act);
    switch (act.dataset.act) {
      case 'toggle': apply([{ d, v: !vals[d.id] }]); break;
      case 'inc': case 'dec': {
        const input = act.parentElement.querySelector('input');
        const base = schema.parse(d, input.value).ok ? Number(input.value) : vals[d.id];
        apply([{ d, v: schema.stepValue(d, base, act.dataset.act === 'inc' ? 1 : -1) }]);
        break;
      }
      case 'segmented': apply([{ d, v: JSON.parse(act.dataset.v) }]); break;
      case 'multi': {
        const x = JSON.parse(act.dataset.v);
        const cur = vals[d.id] || [];
        apply([{ d, v: cur.includes(x) ? cur.filter((y) => y !== x) : [...cur, x].sort((a, b) => a - b) }]);
        break;
      }
      case 'swatch': apply([{ d, v: act.dataset.v }]); break;
      case 'reset': apply([{ d, v: d.default }]); break;
      case 'reset-section': apply(resetList(schema.inSection(defs, current))); break;
      case 'events': manageEvents(); break;
      default: break;
    }
  });
  panel.addEventListener('change', (event) => {
    const el = event.target;
    const d = rowDef(el);
    if (!d) return;
    if (el.dataset.act === 'num') commitNumber(el);
    else if (el.dataset.act === 'range') apply([{ d, v: Number(el.value) }]);
    else if (el.dataset.act === 'select') apply([{ d, v: JSON.parse(el.value) }]);
    else if (el.dataset.act === 'color' && pcApi.isHex(el.value)) apply([{ d, v: el.value.toLowerCase() }]);
  });
  panel.addEventListener('input', (event) => {
    const el = event.target;
    if (el.dataset.act === 'range') {
      const d = rowDef(el);
      el.parentElement.querySelector('.cfg-range-val').textContent = schema.describe(d, Number(el.value));
    }
  });
  panel.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && event.target.dataset && event.target.dataset.act === 'num') { event.preventDefault(); event.target.blur(); }
  });

  const search = $('#cfg-search');
  search.addEventListener('input', () => { query = search.value; render(); });
  search.addEventListener('keydown', (event) => {
    if (event.key === 'Enter') {
      const res = schema.search(defs, query);
      const first = res.cards[0];
      if (!first) return;
      const id = first.defs[0].id;
      current = first.section;
      if (first.defs[0].advanced) openAdvanced.add(`${first.section}:${first.card}`);
      query = '';
      search.value = '';
      setStatus('All changes saved', 'idle');
      render();
      const r = $(`.cfg-row[data-id="${id}"]`);
      if (r) { r.scrollIntoView({ block: 'center' }); r.classList.add('found'); }
    }
  });
  $('#cfg-search-clear').addEventListener('click', () => { query = ''; search.value = ''; setStatus('All changes saved', 'idle'); render(); search.focus(); });
  $('#cfg-undo').addEventListener('click', undo);
  $('#cfg-done').addEventListener('click', () => close());
  $('#cfg-reset-all').addEventListener('click', () => {
    const list = resetList(defs);
    if (!list.length) { setStatus('Everything is already at its default.', 'idle'); return; }
    if (confirm(`Reset ${list.length} setting${list.length === 1 ? '' : 's'} to their defaults? Undo can put them back.`)) apply(list);
  });

  function manageEvents() {
    close();
    if (alertsBtn) alertsBtn.click();
  }

  // ---- open / close -------------------------------------------------------

  const alertsPanel = () => document.querySelector('.alerts-panel:not(.config-panel)');
  function close() {
    if (panel.hidden) return;
    panel.hidden = true;
    button.classList.remove('on');
    FRESH.forEach((id) => seen.add(id));
    try { localStorage.setItem(SEEN_KEY, JSON.stringify([...seen])); } catch (_) { /* private mode */ }
  }
  async function open() {
    const other = alertsPanel();
    if (other && !other.hidden) { const x = other.querySelector('[data-close]'); if (x) x.click(); }
    readLocal();
    panel.hidden = false;
    button.classList.add('on');
    setStatus('All changes saved', 'idle');
    render();
    await loadAlerts();
    render();
  }
  button.addEventListener('click', () => (panel.hidden ? open() : close()));
  $('[data-close]').addEventListener('click', close);
  if (alertsBtn) alertsBtn.addEventListener('click', () => { if (!panel.hidden) close(); }, true);
  window.addEventListener('keydown', (event) => {
    if (panel.hidden) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') { event.preventDefault(); search.focus(); search.select(); return; }
    if (event.key === 'Escape') {
      if (query) { query = ''; search.value = ''; setStatus('All changes saved', 'idle'); render(); } else close();
    }
  });
  // The overlay's -/+ keys change opacity in another window.
  window.addEventListener('storage', (event) => {
    if (event.key !== OPACITY_KEY && event.key !== null) return;
    readLocal();
    render();
  });
  // Narrow window: icon rail and stacked controls (Figma "Config / Narrow").
  if (window.ResizeObserver) new ResizeObserver(() => panel.classList.toggle('narrow', panel.clientWidth < 560)).observe(panel);

  readLocal();
  window.configPanel = {
    /** Live settings from each snapshot. */
    update(settings) {
      if (!settings) return;
      const changed = readSettings(settings);
      const typing = panel.contains(document.activeElement) && document.activeElement.matches('input, select');
      if (changed && !panel.hidden && !typing) render();
    },
    /** The alerts notifier, so alert setting changes reload its schedule. */
    attach(n) { notifier = n; },
  };
})();
