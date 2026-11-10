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
  const COLS = '<col class="c-rank"><col><col class="c-num"><col class="c-num"><col class="c-pct"><col class="c-pct">';
  const sv = window.staggerView;

  let selected = null;
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
    state.textContent = f.live ? '\u00b7 live' : f.hasFight ? '\u00b7 paused' : '\u00b7 idle';
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

  function renderPlayers(m, stagger) {
    const body = $('players');
    const players = m.players;
    $('group-row').hidden = !players.length;
    if (!players.length) {
      $('players-sub').textContent = 'No hits yet';
      body.innerHTML = `<tr class="empty-row"><td colspan="${stagger ? 8 : 7}">No hits yet. Damage is read from game packets, not from memory.</td></tr>`;
      return;
    }
    const targets = m.targets_shown === 1 ? '1 target' : `${m.targets_shown} targets`;
    $('players-sub').textContent = `${players.length} ${players.length === 1 ? 'player' : 'players'} \u00b7 ${targets}`;
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
        <td class="dim stag" title="Estimated stagger points">${sv.points(p.stagger, full) || '\u2014'}</td>
        <td><span class="share"><i style="width:${Math.round((p.damage / top) * 1000) / 10}%"></i></span></td>
      </tr>`;
    }).join('');
    const hits = players.reduce((n, p) => n + p.hits, 0);
    const crits = players.reduce((n, p) => n + p.crits, 0);
    $('foot-total').textContent = short(m.encounter_damage);
    $('foot-dps').textContent = short(m.encounter_dps);
    $('foot-crit').textContent = rate(crits, hits);
    $('foot-stagger').textContent = stagger ? sv.points(stagger.estimated, full) || '\u2014' : '';
  }

  function renderStagger(stagger) {
    const box = $('stagger');
    box.hidden = !stagger;
    if (!stagger) return;
    $('stagger-bar').style.width = `${sv.fillPct(stagger)}%`;
    $('stagger-bar-wrap').classList.toggle('broken', !!stagger.broken);
    $('stagger-bar-wrap').classList.toggle('frozen', !stagger.broken && !!stagger.frozen);
    $('stagger-gauge').textContent = sv.gaugeText(stagger, full);
    const state = sv.stateText(stagger);
    const stateEl = $('stagger-state');
    stateEl.hidden = !state;
    stateEl.textContent = state;
    stateEl.className = `stagger-state ${state.toLowerCase()}`;
    stateEl.title = state === 'Frozen' ? 'The gauge is not moving right now.' : state === 'Broken' ? 'Stagger broke. The gauge refills shortly.' : '';
    $('stagger-break-count').textContent = String(stagger.breaks || 0);
    $('stagger-breaks').title = sv.breaksTitle(stagger, clock);
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
        <td class="stag"></td>
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
      <li><span class="tag heal-tag">HEAL</span><span class="feed-skill" title="Skill ${r.skill}">${esc(r.skill_name)}${r.dot ? ' \u00b7 tick' : ''}</span><span class="heal-amount">+${full(r.amount)}</span></li>`).join('')
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
    $('extras').innerHTML = `<span>DPS <b>${short(player.dps)}</b></span><span>Heal <b>${short(player.heal)}</b></span><span>Taken <b>${short(player.taken)}</b></span><span>Miss <b>${player.misses}</b></span><span>Resist <b>${player.resists}</b></span>${player.stagger > 0 ? `<span title="Estimated stagger points">Stagger <b>${sv.points(player.stagger, full)}</b></span>` : ''}`;
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
        <div class="skill-meta">${full(s.hits)} hits \u00b7 crit ${rate(s.crits, s.hits)} \u00b7 back ${rate(s.backs, s.hits)} \u00b7 perfect ${rate(s.perfects, s.hits)} \u00b7 double ${rate(s.doubles, s.hits)}${s.stagger > 0 ? ` \u00b7 <span class="stag-meta" title="Estimated stagger points">stagger ${sv.points(s.stagger, full)}</span>` : ''}</div>
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
    cap.textContent = `${c.running ? 'Capture on' : 'Capture off'} \u00b7 ${c.demo ? 'demo data' : port} \u00b7 ${full(c.hits)} hits \u00b7 ${full(c.segments)} segments \u00b7 ${full(c.drops)} dropped`;
  }

  function render(data) {
    latest = data;
    window.toastStyle.remember(data.toast_styles);
    if (window.petsTab) window.petsTab.onLive(data);
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
    const stagger = sv.has(m.stagger) ? m.stagger : null;
    const stagCol = !!stagger && !healing;
    const headings = (healing ? HEAL_HEADINGS : DAMAGE_HEADINGS).map((h) => `<th>${h}</th>`);
    headings.splice(6, 0, '<th class="stag" title="Estimated stagger points">STAGGER</th>');
    $('players-headings').innerHTML = headings.join('');
    $('players-cols').innerHTML = `${COLS}${stagCol ? '<col class="c-stag">' : ''}<col class="c-share">`;
    $('players-table').classList.toggle('has-stagger', stagCol);
    renderStagger(stagger);
    $('detail-title').textContent = healing ? 'Healing' : 'Damage by skill';
    ['tiles', 'extras', 'skills'].forEach((id) => { $(id).hidden = healing; });
    ['heal-tiles', 'heal-detail'].forEach((id) => { $(id).hidden = !healing; });
    if (healing) renderHealers(m);
    else renderPlayers(m, stagger);
    const player = m.players.find((p) => p.id === selected);
    if (healing) renderHealDetail(m, healers.find((h) => h.id === selected), player && player.name);
    else renderSkills(player);
    renderRecent(m);
    renderHistory(data.history || []);
    renderCapture(data.capture);
    if (data.settings) window.configPanel.update(data.settings, data.toast_styles);
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
  $('lock-chip').addEventListener('click', () => bridge.unlock().catch(lost));

  bridge.subscribe(render, lost);
  const notifier = window.notify.startAlerts(window.notify.hostFor());
  window.alertsPanel.create($('open-alerts'), notifier);
  window.configPanel.attach(notifier);
})();
