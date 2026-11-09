// Compact always-on-top overlay. -/+ keys change opacity; the layout scales
// with the window; a press on the card moves it. A click that does not move on
// a typed target's name opens the name editor. With click-through on (default),
// the body lets clicks reach the game; the top bar and a thin edge for resizing
// stay interactive (same pattern as toasts).
(function () {
  const { esc, short, clock, fightState, debugNote, UNKNOWN } = window.fmt;
  const bridge = window.meterBridge;
  const $ = (id) => document.getElementById(id);
  const cubes = window.cubeAlert.create($('cube-alert'));

  // Match src/overlay_layout.rs (DESIGN_WIDTH / rem floor / row pitch).
  const DESIGN_WIDTH = 280;
  // Match overlay_layout::DEFAULT_SIZE.1 so rem stays 10px at the snug default.
  const DESIGN_HEIGHT = 180;
  const REM_FLOOR = 0.95; // 9.5px at design 10px rem
  const ROW_PITCH_REM = 1.9;
  const ROWS_PADDING_REM = 0.5;
  const MAX_ROWS = 12;

  const OPACITY_KEY = 'aion2dps.opacity';
  const VIEW_KEY = 'aion2dps.overlay.view';
  const heals = window.healShare;
  const playerColors = window.playerColors;
  const STEP = 0.1;
  const MIN_LEVEL = 0.2;
  const MAX_LEVEL = 1;
  /** Panel alpha at full opacity, from the design (#0F1115 at 88%). */
  const PANEL_ALPHA = 0.88;

  const HINT_MS = 1200;
  /** A press on the target label that moves this far drags the window instead. */
  const DRAG_SLOP_PX = 4;

  let level = readLevel();
  let view = heals.view(localStorage.getItem(VIEW_KEY));
  let latest = null;
  let hintUntil = 0;
  let press = null;
  let clickThrough = true;
  let capturing = true;
  let holding = false;
  let topBarPx = 48;

  if (bridge.inApp) document.body.classList.add('in-app');

  const naming = window.targetNaming.create({
    label: $('target'),
    badge: $('target-badge'),
    save: (type, name) => bridge.setMonsterName(type, name),
    focus: () => bridge.focusWindow(),
  });
  const saving = window.sessionSave.create($('save-session'), {
    save: () => bridge.saveSession(),
    onSaved: (summary) => {
      $('foot-note').textContent = `Session saved \u00b7 ${clock(summary.duration_ms)}`;
      hintUntil = Date.now() + window.sessionSave.SAVED_MS;
    },
  });

  function readLevel() {
    const n = Number(localStorage.getItem(OPACITY_KEY));
    if (!Number.isFinite(n) || n <= 0) return MAX_LEVEL;
    return Math.min(MAX_LEVEL, Math.max(MIN_LEVEL, Math.round(n / STEP) * STEP));
  }

  function remPx() {
    return parseFloat(getComputedStyle(document.documentElement).fontSize) || 10;
  }

  function scale() {
    const s = Math.min(innerWidth / DESIGN_WIDTH, innerHeight / DESIGN_HEIGHT);
    const px = 10 * Math.min(4, Math.max(REM_FLOOR, s));
    document.documentElement.style.fontSize = `${px.toFixed(2)}px`;
    document.body.classList.toggle('narrow-sm', innerWidth <= 260);
    document.body.classList.toggle('narrow-xs', innerWidth <= 240);
    document.body.classList.toggle('narrow-xxs', innerWidth <= 210);
  }

  function paintOpacity() {
    const card = $('card');
    card.style.setProperty('--overlay-alpha', (PANEL_ALPHA * level).toFixed(3));
    card.style.setProperty('--content-opacity', (0.4 + 0.6 * level).toFixed(3));
    localStorage.setItem(OPACITY_KEY, level.toFixed(1));
  }

  function stepOpacity(direction) {
    level = Math.min(MAX_LEVEL, Math.max(MIN_LEVEL, Math.round((level + direction * STEP) * 10) / 10));
    paintOpacity();
    $('foot-note').textContent = `Opacity ${Math.round(level * 100)}%`;
    hintUntil = Date.now() + HINT_MS;
  }

  function rowCapacity(box) {
    const rem = remPx();
    const room = box.clientHeight - ROWS_PADDING_REM * rem;
    return Math.max(1, Math.min(MAX_ROWS, Math.floor((room + 0.6 * rem) / (ROW_PITCH_REM * rem))));
  }

  const LOCK_ICON = '<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="3.5" y="7" width="9" height="6.5" rx="1.2" fill="currentColor"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" stroke-width="1.6"/></svg>';

  function paintBoss(m) {
    const lock = m.lock || null;
    const chip = $('lock-chip');
    chip.hidden = !lock;
    // Compact: a lock icon, plus the seconds left once the boss is dead.
    const secs = lock && !lock.engaged ? String(Math.max(0, Math.ceil((lock.remaining_ms || 0) / 1000))) : '';
    const key = lock ? `${lock.engaged}|${secs}` : '';
    if (chip.dataset.key !== key) {
      chip.dataset.key = key;
      chip.innerHTML = lock ? `${LOCK_ICON}${secs ? `<span>${secs}</span>` : ''}` : '';
    }
    chip.setAttribute('aria-label', window.bossLock.chipText(lock) ? `Boss lock ${window.bossLock.chipText(lock)}` : 'Boss lock');
    chip.title = window.bossLock.chipTitle(lock);
    $('target-defeated').hidden = !window.bossLock.defeated(m) || naming.editing();
  }

  function render(data) {
    latest = data;
    if (data.settings && typeof data.settings.overlay_click_through === 'boolean') {
      clickThrough = data.settings.overlay_click_through;
      if (!clickThrough) applyCapture(true);
    }
    const m = data.meter;
    const f = fightState(data);

    $('dot').className = f.live ? 'dot live' : 'dot';
    $('target').textContent = f.target;
    naming.update(data, f);
    paintBoss(m);
    $('timer').textContent = clock(f.time);
    $('banner').hidden = !f.ended;
    $('banner-secs').textContent = String(f.resetIn);
    const healing = view === heals.VIEW_HEAL;
    document.querySelectorAll('#view button').forEach((b) => b.classList.toggle('on', b.dataset.view === view));
    $('card').classList.toggle('heal-view', healing);
    $('total-label').textContent = healing ? 'Group healing' : 'Group total damage';
    $('dps-label').textContent = healing ? 'Group HPS' : 'Group DPS';
    $('total').textContent = healing ? short(m.encounter_heal || 0) : short(m.encounter_damage);
    $('dps').textContent = `${short(healing ? m.encounter_hps || 0 : m.encounter_dps)}/s`;
    saving.update(m.encounter_damage > 0);
    if (Date.now() >= hintUntil) {
      $('foot-note').textContent = healing
        ? 'Shields/absorbs not counted'
        : f.ended
          ? f.bossHeld ? `Boss locked for ${f.resetIn}s` : `Damage kept ${f.resetSecs}s after kill`
          : `Idle reset in ${f.resetSecs}s`;
    }

    const note = $('debug-note');
    note.textContent = debugNote(data.capture);
    note.title = note.textContent;
    note.hidden = !note.textContent;

    const box = $('rows');
    if (healing) {
      renderHeals(box, m, f, data);
      return;
    }
    const players = m.players;
    if (!players.length) {
      box.className = 'rows empty';
      emptyRows(box, data.capture.error, 'Waiting for combat');
      return;
    }
    box.className = f.ended ? 'rows dim' : 'rows';
    const shown = players.slice(0, rowCapacity(box));
    const you = players.find((p) => p.you);
    if (you && !shown.includes(you)) shown[shown.length - 1] = you;
    const top = Math.max(1, players[0].damage);
    const youIdx = players.findIndex((p) => p.you);
    const palette = playerColors.colorsFor(
      players.map((p) => p.name),
      youIdx >= 0 ? youIdx : null,
      playerColors.readYouColor(),
    );
    box.innerHTML = shown.map((p) => {
      const cls = ['row'];
      if (p.you) cls.push('you');
      if (p.name === UNKNOWN) cls.push('unknown');
      const width = Math.round((p.damage / top) * 1000) / 10;
      const pi = players.indexOf(p);
      const color = palette[pi] || playerColors.UNKNOWN_COLOR;
      return `<div class="${cls.join(' ')}" style="--pc:${color}">
        <span class="rank">${pi + 1}</span>
        <span class="name">${esc(p.name)}${p.heal > 0 ? `<b class="heal-badge">${heals.badge(p.heal, short)}</b>` : ''}</span>
        <span class="dmg">${short(p.damage)}</span>
        <span class="dps">${short(p.dps)}</span>
        <span class="pct">${Math.round(p.pct * 100)}%</span>
        <span class="bar"><i style="width:${width}%"></i></span>
      </div>`;
    }).join('');
  }

  function renderHeals(box, m, f, data) {
    const rows = heals.healerRows(m.healers);
    if (!rows.length) {
      box.className = 'rows empty';
      emptyRows(box, data.capture.error, 'No healing yet');
      return;
    }
    box.className = f.ended ? 'rows dim' : 'rows';
    const shown = rows.slice(0, rowCapacity(box));
    const you = rows.find((h) => h.you);
    if (you && !shown.includes(you)) shown[shown.length - 1] = you;
    const youIdx = rows.findIndex((h) => h.you);
    const palette = playerColors.colorsFor(
      rows.map((h) => h.name),
      youIdx >= 0 ? youIdx : null,
      playerColors.readYouColor(),
    );
    box.innerHTML = shown.map((h) => {
      const cls = ['row', 'heal'];
      if (h.you) cls.push('you');
      if (h.name === UNKNOWN) cls.push('unknown');
      const hi = rows.indexOf(h);
      const color = palette[hi] || playerColors.UNKNOWN_COLOR;
      return `<div class="${cls.join(' ')}" style="--pc:${color}">
        <span class="rank">${hi + 1}</span>
        <span class="name">${esc(h.name)}</span>
        <span class="dmg">${short(h.heal)}</span>
        <span class="dps">${short(h.hps)}</span>
        <span class="pct">${Math.round(h.pct * 100)}%</span>
        <span class="bar"><i style="width:${h.width}%"></i></span>
      </div>`;
    }).join('');
  }

  /** Empty-state text; a missing-Npcap error also gets a "Get Npcap" button. */
  function emptyRows(box, error, idle) {
    const text = error || idle;
    // Pushes arrive several times a second; keep the same button so a click lands.
    if (box.dataset.empty === text && box.querySelector('.get-npcap')) return;
    box.dataset.empty = text;
    box.textContent = text;
    if (!npcapHelp.needsNpcap(error)) return;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'get-npcap';
    btn.title = 'Open npcap.com in your browser';
    btn.textContent = npcapHelp.BUTTON;
    btn.addEventListener('click', () => bridge.openNpcap());
    box.append(btn);
  }

  function showError() {
    const box = $('rows');
    box.className = 'rows empty';
    box.textContent = 'Lost the local meter.';
  }

  $('card').addEventListener('mousedown', (event) => {
    if (event.button !== 0 || event.target.closest('button, .rz, .name-edit, .seg')) return;
    if (event.target.closest('#drag-grip')) { bridge.startDrag(); return; }
    if (event.target.closest('#target') && naming.clickable()) {
      press = { x: event.screenX, y: event.screenY };
      return;
    }
    bridge.startDrag();
  });
  window.addEventListener('mousemove', (event) => {
    if (!press) return;
    if (!(event.buttons & 1)) {
      press = null;
      return;
    }
    if (Math.hypot(event.screenX - press.x, event.screenY - press.y) < DRAG_SLOP_PX) return;
    press = null;
    bridge.startDrag();
  });
  window.addEventListener('mouseup', (event) => {
    if (!press || event.button !== 0) return;
    press = null;
    if (event.target.closest('#target')) naming.click();
  });
  document.querySelectorAll('.rz').forEach((zone) => zone.addEventListener('mousedown', (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    bridge.startResize(zone.dataset.dir);
  }));
  document.querySelectorAll('#view button').forEach((b) => b.addEventListener('click', () => {
    view = heals.view(b.dataset.view);
    localStorage.setItem(VIEW_KEY, view);
    if (latest) render(latest);
  }));
  $('close').addEventListener('click', () => bridge.closeWindow());
  $('lock-chip').addEventListener('click', () => bridge.unlock().catch(() => {}));
  // Same as Reset fight in the detailed meter: ends any boss lock and archives the fight.
  $('reset').addEventListener('click', () => bridge.reset().catch(showError));
  $('open-meter').addEventListener('click', () => bridge.openMeter());
  window.addEventListener('keydown', (event) => {
    if (event.ctrlKey || event.altKey || event.metaKey || naming.editing()) return;
    if (event.key === '-' || event.code === 'NumpadSubtract') stepOpacity(-1);
    else if (event.key === '+' || event.key === '=' || event.code === 'NumpadAdd') stepOpacity(1);
    else return;
    event.preventDefault();
  });
  window.addEventListener('resize', () => {
    scale();
    if (latest) render(latest);
  });


  // Click-through: poll the global cursor (ignore_cursor_events blocks DOM
  // mouseenter). Top bar + EDGE_PX border stay interactive; holding the mouse
  // keeps capture so a drag can leave the bar.
  const EDGE_PX = 8;
  const POINTER_MS = 33;

  function measureTopBar() {
    const el = $('hit-top');
    if (!el) return;
    topBarPx = Math.ceil(el.getBoundingClientRect().height) || topBarPx;
  }

  function wantsCapture(x, y) {
    const w = window.innerWidth;
    const h = window.innerHeight;
    if (!clickThrough || holding) return true;
    if (x < 0 || y < 0 || x >= w || y >= h) return false;
    if (y < topBarPx || cubes.hit(x, y)) return true;
    return x < EDGE_PX || x >= w - EDGE_PX || y >= h - EDGE_PX;
  }

  function applyCapture(next) {
    if (next === capturing) return;
    capturing = next;
    bridge.overlayHit(next).catch(() => {});
  }

  if (bridge.inApp) {
    window.addEventListener('mousedown', (event) => {
      if (event.button === 0) holding = true;
    });
    window.addEventListener('mouseup', () => { holding = false; });
    window.addEventListener('blur', () => { holding = false; });
    measureTopBar();
    setInterval(async () => {
      measureTopBar();
      if (!clickThrough) { applyCapture(true); return; }
      let p = null;
      try { p = await bridge.overlayPointer(); } catch (_) { return; }
      if (!p) { applyCapture(true); return; }
      applyCapture(wantsCapture(p[0], p[1]));
    }, POINTER_MS);
  }

  window.addEventListener('storage', (event) => {
    if (event.key === playerColors.YOU_KEY && latest) render(latest);
  });
  scale();
  paintOpacity();
  bridge.subscribe(render, showError);
  // Event alerts: at the top of the screen in the app, a compact pill here in
  // a browser. They are set up in the detailed meter.
  window.notify.startAlerts(window.notify.hostFor({ compact: true }));
})();
