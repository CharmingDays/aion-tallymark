// Detailed meter: the same page in the app's meter window and at http://127.0.0.1:18732/.
(function () {
  const { esc, short, full, clock, pct, rate, fightState, debugNote, UNKNOWN } = window.fmt;
  const bridge = window.meterBridge;
  const $ = (id) => document.getElementById(id);
  const RECENT_ROWS = 40;
  const VIEW_KEY = 'aion2dps.meter.view';
  const heals = window.healShare;
  const DAMAGE_HEADINGS = ['RANK', 'NAME', 'TOTAL DAMAGE', 'DPS', '% OF GROUP', 'CRIT %', 'SHARE'];
  const HEAL_HEADINGS = ['#', 'NAME', 'TOTAL HEAL', 'HPS', '% OF GROUP', '', 'SHARE'];

  let selected = null;
  // Config panel edits wait for its single Save button.
  let configDirty = false;
  let configSaving = false;
  const bossLock = window.bossLock;
  let latest = null;
  let view = heals.view(localStorage.getItem(VIEW_KEY));

  if (bridge.inApp) document.body.classList.add('in-app');

  const naming = window.targetNaming.create({
    label: $('target'),
    badge: $('target-badge'),
    save: (type, name) => bridge.setMonsterName(type, name),
  });
  const saving = window.sessionSave.create($('save-session'), { save: () => bridge.saveSession() });

  const nameClass = (name, you) => (you ? 'you' : name === UNKNOWN ? 'unknown' : '');

  function paintBoss(m) {
    const lock = m.lock || null;
    const chip = $('lock-chip');
    chip.hidden = !lock;
    chip.textContent = bossLock.chipText(lock);
    chip.title = bossLock.chipTitle(lock);
    $('target-defeated').hidden = !bossLock.defeated(m) || naming.editing();
  }

  function renderTop(data, f) {
    const m = data.meter;
    $('dot').className = f.live ? 'dot live' : 'dot';
    const targetEl = $('target');
    targetEl.textContent = f.target;
    naming.update(data, f);
    // Keep a tooltip for ellipsis when naming did not set a more specific title.
    if (!targetEl.title) targetEl.title = f.target;
    paintBoss(m);
    const state = $('state');
    state.textContent = f.live ? '· live' : f.hasFight ? '· paused' : '· idle';
    state.className = f.live ? 'state live' : 'state';
    state.hidden = f.ended;
    $('ended').hidden = !f.ended;
    $('ended-secs').textContent = String(f.resetIn);
    $('time').textContent = clock(f.time);
    const healing = view === heals.VIEW_HEAL;
    const total = healing ? m.encounter_heal || 0 : m.encounter_damage;
    const perSec = healing ? m.encounter_hps || 0 : m.encounter_dps;
    $('total-label').textContent = healing ? 'Group healing' : 'Group total damage';
    $('dps-label').textContent = healing ? 'Group HPS' : 'Group DPS';
    $('total').textContent = short(total);
    $('total').title = full(total);
    $('total').classList.toggle('heal-text', healing);
    $('dps').textContent = `${short(perSec)}/s`;
    $('dps').title = `${full(perSec)} per second`;
    $('dps').classList.toggle('heal-text', healing);
    saving.update(m.encounter_damage > 0);
    document.querySelectorAll('#modes button').forEach((b) => b.classList.toggle('on', b.dataset.mode === m.mode));
  }

  function renderPlayers(m) {
    const body = $('players');
    const players = m.players;
    $('group-row').hidden = !players.length;
    if (!players.length) {
      $('players-sub').textContent = 'No hits yet';
      body.innerHTML = '<tr class="empty-row"><td colspan="7">No hits yet. Damage is read from game packets, not from memory.</td></tr>';
      return;
    }
    const targets = m.targets_shown === 1 ? '1 target' : `${m.targets_shown} targets`;
    $('players-sub').textContent = `${players.length} ${players.length === 1 ? 'player' : 'players'} · ${targets}`;
    const top = Math.max(1, players[0].damage);
    body.innerHTML = players.map((p, i) => {
      const cls = [nameClass(p.name, p.you), p.id === selected ? 'sel' : ''].filter(Boolean).join(' ');
      return `<tr class="${cls}" data-id="${p.id}">
        <td class="rank">${i + 1}</td>
        <td><div class="who"><span class="avatar"></span><span class="name">${esc(p.name)}</span>${p.you ? '<span class="you-tag">YOU</span>' : ''}</div></td>
        <td class="dmg" title="${full(p.damage)}">${short(p.damage)}</td>
        <td class="dim">${short(p.dps)}</td>
        <td class="dim">${pct(p.pct, 1)}</td>
        <td class="dim">${rate(p.crits, p.hits)}</td>
        <td><span class="share"><i style="width:${Math.round((p.damage / top) * 1000) / 10}%"></i></span></td>
      </tr>`;
    }).join('');
    const hits = players.reduce((n, p) => n + p.hits, 0);
    const crits = players.reduce((n, p) => n + p.crits, 0);
    $('foot-total').textContent = short(m.encounter_damage);
    $('foot-dps').textContent = short(m.encounter_dps);
    $('foot-crit').textContent = rate(crits, hits);
  }

  function renderHealers(m) {
    const body = $('players');
    const rows = heals.healerRows(m.healers);
    $('group-row').hidden = !rows.length;
    if (!rows.length) {
      $('players-sub').textContent = 'No healing yet';
      body.innerHTML = '<tr class="empty-row"><td colspan="7">No healing recorded yet. Only players with healing are listed.</td></tr>';
      return;
    }
    $('players-sub').textContent = `${rows.length} ${rows.length === 1 ? 'healer' : 'healers'}`;
    body.innerHTML = rows.map((h, i) => {
      const cls = [nameClass(h.name, h.you), h.id === selected ? 'sel' : '', 'heal'].filter(Boolean).join(' ');
      return `<tr class="${cls}" data-id="${h.id}">
        <td class="rank">${i + 1}</td>
        <td><div class="who"><span class="avatar"></span><span class="name">${esc(h.name)}</span>${h.you ? '<span class="you-tag">YOU</span>' : ''}</div></td>
        <td class="dmg" title="${full(h.heal)}">${short(h.heal)}</td>
        <td class="dim">${short(h.hps)}/s</td>
        <td class="dim">${pct(h.pct, 1)}</td>
        <td></td>
        <td><span class="share heal-share"><i style="width:${h.width}%"></i></span></td>
      </tr>`;
    }).join('');
    $('foot-total').textContent = short(m.encounter_heal || 0);
    $('foot-dps').textContent = short(m.encounter_hps || 0);
    $('foot-crit').textContent = '';
  }

  function renderHealDetail(m, healer, name) {
    const who = $('skills-player');
    if (!healer) {
      who.textContent = name || 'Select a player';
      who.className = 'sub';
      ['h-total', 'h-hps', 'h-share'].forEach((id) => { $(id).textContent = '—'; });
      $('heal-skills').innerHTML = '<div class="empty">No healing recorded yet</div>';
      $('heal-feed-head').textContent = 'Heal feed';
      $('heal-feed').innerHTML = '';
      return;
    }
    who.textContent = healer.name;
    who.className = `sub ${nameClass(healer.name, healer.you)}`;
    $('h-total').textContent = short(healer.heal);
    $('h-total').title = full(healer.heal);
    $('h-hps').textContent = `${short(healer.hps)}/s`;
    $('h-share').textContent = pct(healer.pct, 0);
    const skills = healer.skills || [];
    const top = Math.max(1, skills.length ? skills[0].heal : 1);
    $('heal-skills').innerHTML = skills.length ? skills.map((s) => `
      <div class="skill heal-skill" title="Skill ${s.id}">
        <div class="skill-top">
          <span class="skill-name">${esc(s.name)}${s.hot ? '<span class="tag hot">HoT</span>' : ''}</span>
          <span class="skill-dmg">${short(s.heal)}</span>
          <span class="skill-pct">${healer.heal > 0 ? pct(s.heal / healer.heal, 0) : '—'}</span>
        </div>
        <div class="skill-bar heal-bar"><i style="width:${Math.round((s.heal / top) * 1000) / 10}%"></i></div>
      </div>`).join('') : '<div class="empty">No healing recorded yet</div>';
    $('heal-feed-head').textContent = `Heal feed (healer: ${healer.name})`;
    const rows = heals.feed(m.recent, healer.id);
    $('heal-feed').innerHTML = rows.length ? rows.map((r) => `
      <li><span class="tag heal-tag">HEAL</span><span class="feed-skill" title="Skill ${r.skill}">${esc(r.skill_name)}${r.dot ? ' · tick' : ''}</span><span class="heal-amount">+${full(r.amount)}</span></li>`).join('')
      : '<li class="empty">No healing recorded yet</li>';
  }

  function renderSkills(player) {
    const list = $('skills');
    const who = $('skills-player');
    if (!player) {
      who.textContent = 'Select a player';
      who.className = 'sub';
      ['t-damage', 't-hits', 't-crit', 't-back'].forEach((id) => { $(id).textContent = '—'; });
      $('extras').innerHTML = '';
      list.innerHTML = '<div class="empty">Select a player in the table.</div>';
      return;
    }
    who.textContent = player.name;
    who.className = `sub ${nameClass(player.name, player.you)}`;
    $('t-damage').textContent = short(player.damage);
    $('t-damage').title = full(player.damage);
    $('t-hits').textContent = full(player.hits);
    $('t-crit').textContent = rate(player.crits, player.hits);
    $('t-back').textContent = rate(player.backs, player.hits);
    $('extras').innerHTML = `<span>DPS <b>${short(player.dps)}</b></span><span>Heal <b>${short(player.heal)}</b></span><span>Taken <b>${short(player.taken)}</b></span><span>Miss <b>${player.misses}</b></span><span>Resist <b>${player.resists}</b></span>`;
    if (!player.skills.length) {
      list.innerHTML = '<div class="empty">No skills recorded.</div>';
      return;
    }
    const top = Math.max(1, player.skills[0].damage);
    list.innerHTML = player.skills.map((s) => `
      <div class="skill" title="Skill ${s.id}">
        <div class="skill-top">
          <span class="skill-name">${esc(s.name)}${s.dot ? '<span class="tag">DoT</span>' : ''}</span>
          <span class="skill-dmg">${short(s.damage)}</span>
          <span class="skill-pct">${player.damage > 0 ? pct(s.damage / player.damage, 1) : '—'}</span>
        </div>
        <div class="skill-bar"><i style="width:${Math.round((s.damage / top) * 1000) / 10}%"></i></div>
        <div class="skill-meta">${full(s.hits)} hits · crit ${rate(s.crits, s.hits)} · back ${rate(s.backs, s.hits)} · perfect ${rate(s.perfects, s.hits)} · double ${rate(s.doubles, s.hits)}</div>
      </div>`).join('');
  }

  function renderRecent(m) {
    const body = $('recent');
    const rows = m.recent.filter((r) => selected == null || r.actor_id === selected || r.taken).slice(0, RECENT_ROWS);
    if (!rows.length) {
      body.innerHTML = '<tr class="empty-row"><td colspan="4">—</td></tr>';
      return;
    }
    body.innerHTML = rows.map((r) => {
      const tags = [r.crit && '<span class="tag crit">crit</span>', r.dot && '<span class="tag">dot</span>', r.taken && '<span class="tag">taken</span>', r.heal && '<span class="tag">heal</span>'].filter(Boolean).join('');
      return `<tr><td class="${nameClass(r.actor, r.you)}">${esc(r.actor)}</td><td title="Skill ${r.skill}">${esc(r.skill_name)}</td><td>${full(r.amount)}</td><td>${tags}</td></tr>`;
    }).join('');
  }

  function renderHistory(pulls) {
    const body = $('history');
    if (!pulls.length) {
      body.innerHTML = '<tr class="empty-row"><td colspan="5">Finished pulls are kept here, one row per unit.</td></tr>';
      return;
    }
    body.innerHTML = pulls.map((row) => `<tr><td>${new Date(row.ended_ms).toLocaleTimeString()}</td><td class="${nameClass(row.name, false)}">${esc(row.name)}</td><td>${short(row.damage)}</td><td>${short(row.dps)}</td><td>${clock(row.duration_ms)}</td></tr>`).join('');
  }

  function renderCapture(c) {
    const note = $('debug-note');
    note.textContent = debugNote(c);
    note.title = note.textContent;
    note.hidden = !note.textContent;
    const cap = $('capture');
    $('get-npcap').hidden = !npcapHelp.needsNpcap(c.error);
    if (c.error) {
      cap.className = 'capture bad';
      cap.textContent = c.error;
      return;
    }
    cap.className = 'capture good';
    const port = c.locked_port == null ? 'looking for the game' : `server port ${c.locked_port}`;
    cap.textContent = `${c.running ? 'Capture on' : 'Capture off'} · ${c.demo ? 'demo data' : port} · ${full(c.hits)} hits · ${full(c.segments)} segments · ${full(c.drops)} dropped`;
  }

  function render(data) {
    latest = data;
    const m = data.meter;
    const f = fightState(data);
    if (!m.players.some((p) => p.id === selected)) {
      const you = m.players.find((p) => p.you);
      selected = you ? you.id : m.players.length ? m.players[0].id : null;
    }
    renderTop(data, f);
    const healing = view === heals.VIEW_HEAL;
    const healers = m.healers || [];
    if (healing && healers.length && !healers.some((h) => h.id === selected)) {
      const you = healers.find((h) => h.you);
      selected = (you || heals.healerRows(healers)[0]).id;
    }
    document.querySelectorAll('#view button, #detail-view button').forEach((b) => b.classList.toggle('on', b.dataset.view === view));
    $('players-headings').innerHTML = (healing ? HEAL_HEADINGS : DAMAGE_HEADINGS).map((h) => `<th>${h}</th>`).join('');
    $('detail-title').textContent = healing ? 'Healing' : 'Damage by skill';
    ['tiles', 'extras', 'skills'].forEach((id) => { $(id).hidden = healing; });
    ['heal-tiles', 'heal-detail'].forEach((id) => { $(id).hidden = !healing; });
    if (healing) renderHealers(m);
    else renderPlayers(m);
    const player = m.players.find((p) => p.id === selected);
    if (healing) renderHealDetail(m, healers.find((h) => h.id === selected), player && player.name);
    else renderSkills(player);
    renderRecent(m);
    renderHistory(data.history || []);
    renderCapture(data.capture);
    if (!configDirty && !configSaving && data.settings) loadConfig(data.settings);
  }

  function loadConfig(s) {
    if (s.reset_after_secs != null) $('reset-secs').value = s.reset_after_secs;
    if (s.boss_lock_secs != null) {
      $('boss-lock-secs').value = s.boss_lock_secs;
      $('boss-damage').value = bossLock.formatDamage(s.boss_damage_threshold);
    }
    if (typeof s.boss_engage_lock === 'boolean') $('boss-engage').checked = s.boss_engage_lock;
    if (typeof s.open_when_aion2 === 'boolean') $('open-when-aion2').checked = s.open_when_aion2;
    if (typeof s.overlay_click_through === 'boolean') $('overlay-click-through').checked = s.overlay_click_through;
  }

  function lost() {
    const cap = $('capture');
    cap.className = 'capture bad';
    cap.textContent = 'Lost the local meter.';
  }

  $('get-npcap').addEventListener('click', () => bridge.openNpcap());
  $('target').addEventListener('click', () => naming.click());
  $('players').addEventListener('click', (event) => {
    const row = event.target.closest('tr[data-id]');
    if (!row) return;
    selected = Number(row.dataset.id);
    if (latest) render(latest);
  });
  document.querySelectorAll('#view button, #detail-view button').forEach((b) => b.addEventListener('click', () => {
    view = heals.view(b.dataset.view);
    localStorage.setItem(VIEW_KEY, view);
    if (latest) render(latest);
  }));
  document.querySelectorAll('#modes button').forEach((b) => b.addEventListener('click', () => {
    bridge.setMode(b.dataset.mode).catch(lost);
  }));
  $('reset').addEventListener('click', () => bridge.reset().catch(lost));
  $('clear-history').addEventListener('click', () => bridge.clearHistory().catch(lost));
  $('open-history').addEventListener('click', () => bridge.openHistory());
  $('open-browser').addEventListener('click', () => bridge.openInBrowser());
  const saveBtn = $('config-save');
  const configNote = $('config-note');
  let savedTimer = null;
  const notes = ['settings-note', 'boss-note', 'app-note'];
  function setDirty(on) {
    configDirty = on;
    saveBtn.disabled = !on || configSaving;
    if (on) { clearTimeout(savedTimer); configNote.textContent = ''; configNote.className = 'cp-status'; }
  }
  ['reset-secs', 'boss-lock-secs', 'boss-damage'].forEach((id) => $(id).addEventListener('input', () => setDirty(true)));
  ['boss-engage', 'open-when-aion2', 'overlay-click-through'].forEach((id) => $(id).addEventListener('change', () => setDirty(true)));
  // Closing the panel discards unsaved edits; the next open shows the saved values.
  document.addEventListener('config:closed', () => {
    setDirty(false);
    notes.forEach((id) => { $(id).textContent = ''; });
    configNote.textContent = '';
    if (latest && latest.settings) loadConfig(latest.settings);
  });

  async function saveConfig() {
    if (!configDirty || configSaving) return;
    notes.forEach((id) => { $(id).textContent = ''; });
    const s = (latest && latest.settings) || {};
    const resetSecs = Number($('reset-secs').value);
    const lock = bossLock.parseLockSecs($('boss-lock-secs').value);
    if (!lock.ok) { $('boss-note').textContent = lock.error; return; }
    const damage = bossLock.parseBossDamage($('boss-damage').value);
    if (!damage.ok) { $('boss-note').textContent = damage.error; return; }
    const engage = $('boss-engage').checked;
    const openWhen = $('open-when-aion2').checked;
    const clickThrough = $('overlay-click-through').checked;
    configSaving = true;
    saveBtn.disabled = true;
    let failed = false;
    if (resetSecs !== s.reset_after_secs) {
      try { await bridge.setResetSecs(resetSecs); } catch (_) { failed = true; $('settings-note').textContent = 'Use 5 to 3600.'; }
    }
    if (lock.value !== s.boss_lock_secs || damage.value !== s.boss_damage_threshold || engage !== s.boss_engage_lock) {
      try {
        await bridge.setBossSettings({ lockSecs: lock.value, damageThreshold: damage.value, engageLock: engage });
        $('boss-damage').value = bossLock.formatDamage(damage.value);
      } catch (_) { failed = true; $('boss-note').textContent = 'Could not save.'; }
    }
    if (bridge.inApp && openWhen !== s.open_when_aion2) {
      try {
        await bridge.setOpenWhenAion2(openWhen);
        $('app-note').textContent = openWhen
          ? 'Close the overlay to wait in the tray for Aion 2.'
          : 'Closing the overlay quits the app.';
      } catch (_) { failed = true; $('app-note').textContent = 'Could not save.'; }
    }
    if (bridge.inApp && clickThrough !== s.overlay_click_through) {
      try {
        await bridge.setOverlayClickThrough(clickThrough);
      } catch (_) { failed = true; $('app-note').textContent = 'Could not save.'; }
    }
    configSaving = false;
    clearTimeout(savedTimer);
    if (failed) {
      configNote.textContent = 'Some settings could not be saved.';
      configNote.className = 'cp-status bad';
      setDirty(true);
      configNote.textContent = 'Some settings could not be saved.';
      configNote.className = 'cp-status bad';
    } else {
      setDirty(false);
      configNote.textContent = 'Saved';
      configNote.className = 'cp-status ok';
      savedTimer = setTimeout(() => { configNote.textContent = ''; }, 2500);
    }
  }
  saveBtn.addEventListener('click', saveConfig);
  // Enter in a field saves the whole panel too.
  ['settings', 'boss-settings', 'app-settings'].forEach((id) => $(id).addEventListener('submit', (event) => {
    event.preventDefault();
    saveConfig();
  }));
  $('lock-chip').addEventListener('click', () => bridge.unlock().catch(lost));

  bridge.subscribe(render, lost);
  const notifier = window.notify.startAlerts(window.notify.hostFor());
  window.alertsPanel.create($('open-alerts'), notifier);
})();
