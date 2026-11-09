// Session history: the saved sessions, a summary, damage over time with a
// playhead, the players table and an ability feed as they stood at the
// playhead. A session opens paused at the start. Same page in the app's
// history window and at http://127.0.0.1:18732/history.
(function () {
  const { esc, short, full, clock, pct, rate, UNKNOWN } = window.fmt;
  const bridge = window.meterBridge;
  const $ = (id) => document.getElementById(id);

  /** Hit flag bits in a saved event `[t_ms, actor, damage, flags, skill]`. */
  const FLAG_CRIT = 1;
  const FLAG_DOT = 2;
  /** The feed keeps the newest hits only. */
  const FEED_ROWS = 60;
  const NEW_MS = 600;
  const MAX_BUCKETS = 120;
  const CHART_W = 1000;
  const CHART_H = 200;
  const CHART_TOP = 12;

  let summaries = [];
  let selectedId = null;
  let loadedId = null;
  let session = null;
  let rawSession = null;
  let loading = 0;
  let span = 1;
  let playhead = 0;
  let playing = false;
  let speed = 1;
  let lastFrame = 0;
  let frameId = 0;
  /** Running totals up to `acc.t`, so playback only adds the new hits. */
  let acc = null;
  /** Actor index the "Select player" picks follow; rows open to show abilities. */
  let selectedActor = null;
  const expanded = new Set();
  let feedFilter = 'all';
  /** Event count the feed last showed, so it is redrawn only when that changes. */
  let feedShown = -1;
  /** "players" or "share"; the Ability share tab's scope and sort. */
  let tab = 'players';
  let shareScope = 'all';
  /** Colour per actor index; local player colour comes from Config (same localStorage key). */
  let youColor = playerColors.readYouColor();
  let colors = [];
  const pc = (i) => colors[i] || playerColors.UNKNOWN_COLOR;
  /** Which Select player list is open: 'share', 'feed' or null. */
  let pickFor = null;
  let shareSort = 'damage';
  /** Event count and options the share table last showed. */
  let shareShown = '';
  /** Whether the "Other (N)" row is open to list the abilities inside it. */
  let otherOpen = false;
  /** Heal running totals (heal-share.js), whether the session has heals, and the chart series shown. */
  const heals = window.healShare;
  let hacc = null;
  let hasHeals = false;
  const series = { damage: true, heal: true };
  let healScope = 'all';
  let healOtherOpen = false;
  let healShown = '';
  /** Stagger running totals (stagger-history.js) and whether the session has any. */
  const sh = window.staggerHistory;
  let sacc = null;
  let staggerShown = '';
  const tabOverflow = window.tabOverflow;
  const TAB_NAMES = { players: 'Players', share: 'Ability share', heal: 'Healing', stagger: 'Stagger' };
  /** px kept for the roster sub line beside the tabs. */
  const SUB_MIN = 150;
  let hasStagger = false;
  const COLS = '<col class="c-rank"><col><col class="c-num"><col class="c-num"><col class="c-pct"><col class="c-pct">';

  if (bridge.inApp) document.body.classList.add('in-app');

  const kind = (target) => (target.dummy ? 'Dummy' : target.boss ? 'Boss' : '');
  const when = (ms) => new Date(ms).toLocaleString([], {
    month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  });
  /** Custom name when set, otherwise the target label. */
  const displayName = (s) => (s && s.name && String(s.name).trim()) || (s && s.target && s.target.label) || '';
  const MAX_NAME = 60;
  const PENCIL = '<svg viewBox="0 0 16 16" aria-hidden="true"><path fill="currentColor" d="M11.7 1.3a1.5 1.5 0 0 1 2.1 2.1l-.7.7-2.1-2.1.7-.7zm-1.4 1.4 2.1 2.1-7.4 7.4H3v-2.1l7.3-7.4z"/></svg>';
  let renamingId = null;

  function note(message) {
    $('action-note').textContent = message;
    $('action-note').title = message;
  }

  async function refresh() {
    try {
      summaries = await bridge.listSessions();
    } catch (err) {
      note(`Could not list sessions: ${(err && err.message) || err}`);
      return;
    }
    if (!summaries.some((s) => s.id === selectedId)) selectedId = summaries.length ? summaries[0].id : null;
    renderList();
    if (selectedId == null) show(null, null);
    else if (selectedId !== loadedId) select(selectedId);
  }

  function renderList() {
    $('no-sessions').hidden = summaries.length > 0;
    $('count').textContent = summaries.length ? String(summaries.length) : '';
    $('sessions').innerHTML = summaries.map((s) => {
      const badge = (kind(s.target) ? `<span class="target-badge">${kind(s.target)}</span>` : '')
        + (s.target.defeated ? '<span class="target-badge defeated">Defeated</span>' : '');
      const label = displayName(s);
      const custom = Boolean(s.name && String(s.name).trim());
      const tip = custom ? `Renamed \u00b7 target ${s.target.label}` : s.target.label;
      if (renamingId === s.id) {
        return `<li class="session-item sel editing" data-id="${esc(s.id)}">
          <div class="session">
            <span class="s-name"><input class="s-edit" type="text" maxlength="${MAX_NAME}" value="${esc(label)}" aria-label="Session name"></span>
            <span class="s-meta"><span>${esc(when(s.started_at))}</span><span>${clock(s.duration_ms)} \u00b7 ${short(s.totals.total_damage)}</span></span>
          </div>
        </li>`;
      }
      return `<li class="session-item${s.id === selectedId ? ' sel' : ''}" data-id="${esc(s.id)}">
        <button type="button" class="session" data-id="${esc(s.id)}">
          <span class="s-name"><span class="s-label" title="${esc(tip)}">${esc(label)}</span>${badge}</span>
          <span class="s-meta"><span>${esc(when(s.started_at))}</span><span>${clock(s.duration_ms)} \u00b7 ${short(s.totals.total_damage)}</span></span>
        </button>
        <button type="button" class="s-rename" data-rename="${esc(s.id)}" title="Rename session" aria-label="Rename session">${PENCIL}</button>
      </li>`;
    }).join('');
    if (renamingId) {
      const input = $('sessions').querySelector('.s-edit');
      if (input) {
        input.focus();
        input.select();
      }
    }
  }

  async function select(id) {
    selectedId = id;
    renderList();
    const token = ++loading;
    try {
      const loaded = await bridge.getSession(id);
      if (token === loading) show(id, loaded);
    } catch (err) {
      if (token !== loading) return;
      show(null, null);
      note(`Could not open the session: ${(err && err.message) || err}`);
    }
  }

  function show(id, loaded) {
    pause();
    loadedId = id;
    session = loaded;
    rawSession = loaded;
    acc = null;
    hacc = null;
    sacc = null;
    note('');
    $('detail').hidden = !session;
    $('feed-card').hidden = !session;
    $('placeholder').hidden = Boolean(session);
    $('export').disabled = !session;
    $('delete').disabled = !session;
    if (!session) {
      $('when').textContent = '';
      return;
    }
    // Start ~1.5s before the first saved hit; the export keeps the file as is.
    const trimmed = window.replayTrim.trim(session);
    session = trimmed.session;
    span = trimmed.span;
    $('scrub').max = String(span);
    $('t-end').textContent = clock(span);
    $('s-length').textContent = `/ ${clock(span)}`;
    selectedActor = null;
    paintColors();
    resetScopes();
    expanded.clear();
    otherOpen = false;
    healOtherOpen = false;
    feedShown = -1;
    shareShown = '';
    healShown = '';
    hasHeals = heals.hasHeals(session);
    series.damage = true;
    series.heal = hasHeals;
    paintSeries();
    $('filter-heals').dataset.off = hasHeals ? '' : '1';
    layoutFeedBar();
    hasStagger = sh.has(session);
    $('players-table').classList.toggle('has-stagger', hasStagger);
    $('players-cols').innerHTML = `${COLS}${hasStagger ? '<col class="c-stag">' : ''}<col class="c-share">`;
    $('s-stagger-stat').hidden = !hasStagger;
    $('s-stagger-total-stat').hidden = !hasStagger;
    staggerShown = '';
    document.querySelector('#tabs > [data-tab="stagger"]').dataset.off = hasStagger ? '' : '1';
    if (!hasStagger && tab === 'stagger') setTab('players');
    layoutTabs();
    renderSummary();
    renderChart();
    seek(0);
  }

  function renderSummary() {
    const s = session;
    const label = displayName(s);
    $('s-target').textContent = label;
    $('s-target').title = (s.name && String(s.name).trim() && s.target.label !== label)
      ? `${label} \u00b7 target ${s.target.label}`
      : label;
    $('s-badge').textContent = kind(s.target);
    $('s-badge').hidden = !kind(s.target);
    $('s-defeated').hidden = !s.target.defeated;
    $('truncated').hidden = !s.truncated;
  }

  /** The summary at the playhead; dashes until the first hit. */
  function renderLive(activeMs, rows) {
    const any = acc.total > 0;
    $('s-elapsed').textContent = clock(playhead);
    $('s-total').textContent = any ? short(acc.total) : '—';
    $('s-total').title = any ? full(acc.total) : '';
    $('s-dps').textContent = any ? `${short((acc.total * 1000) / activeMs)}/s` : '—';
    const healed = hacc && hacc.total > 0;
    $('s-heal').textContent = healed ? short(hacc.total) : '—';
    $('s-heal').title = healed ? full(hacc.total) : '';
    $('s-hps').textContent = healed ? `${short((hacc.total * 1000) / activeMs)}/s` : '—';
    $('s-top').textContent = rows.length ? `${rows[0].name} \u00b7 ${short((rows[0].damage * 1000) / activeMs)}/s` : '—';
    if (hasStagger) {
      const all = session.stagger_breaks || [];
      const seen = sh.breaksUntil(all, playhead);
      $('s-breaks').textContent = sh.breaksText(seen, clock);
      $('s-breaks').title = all.length ? `${all.length} in the fight, at ${all.map((ms) => clock(ms)).join(', ')}` : 'No stagger break in this fight';
      $('s-stagger').textContent = sh.points(sacc ? sacc.total : 0, full) || '—';
    }
  }

  /** A dashed mark on the chart for each stagger break. */
  function renderBreakMarks() {
    const breaks = hasStagger ? session.stagger_breaks || [] : [];
    $('break-marks').innerHTML = breaks.map((ms) => {
      const x = ((Math.min(span, ms) / span) * CHART_W).toFixed(1);
      return `<line class="break-mark" data-t="${ms}" x1="${x}" y1="0" x2="${x}" y2="${CHART_H}"><title>Stagger break at ${clock(ms)}</title></line>`;
    }).join('');
  }

  function renderState() {
    const at = playhead <= 0 ? 'paused at start' : playhead >= span ? 'replay finished' : `paused at ${clock(playhead)}`;
    $('when').textContent = `\u00b7 ${when(session.started_at)} \u00b7 ${playing ? `replaying ${speed}x` : at}`;
    $('pb-state').textContent = playing ? 'Playing' : 'Paused';
    $('pb-state').classList.toggle('on', playing);
    $('feed-dot').classList.toggle('on', playing);
    $('chart').classList.toggle('dim', !playing && playhead <= 0);
  }

  /** Group DPS per time bucket, as a line and a filled area. */
  function renderChart() {
    const buckets = Math.max(1, Math.min(MAX_BUCKETS, Math.ceil(span / 1000)));
    const size = span / buckets;
    const sums = new Float64Array(buckets);
    for (const event of session.events) sums[Math.min(buckets - 1, Math.floor(event[0] / size))] += event[2];
    const rates = Array.from(sums, (damage) => (damage * 1000) / size);
    const healRates = hasHeals ? heals.rates(session.heals, span, buckets) : [];
    const peak = Math.max(1, ...rates, ...healRates);
    const y = (value) => (CHART_H - (value / peak) * (CHART_H - CHART_TOP)).toFixed(1);
    const points = rates.map((value, i) => `${(((i + 0.5) / buckets) * CHART_W).toFixed(1)} ${y(value)}`);
    const line = `M0 ${y(rates[0])} L${points.join(' L')} L${CHART_W} ${y(rates[buckets - 1])}`;
    const area = `${line} L${CHART_W} ${CHART_H} L0 ${CHART_H} Z`;
    $('line').setAttribute('d', line);
    $('line-played').setAttribute('d', line);
    $('area').setAttribute('d', area);
    $('area-played').setAttribute('d', area);
    let healLine = '';
    let healArea = '';
    if (healRates.length) {
      const hp = healRates.map((value, i) => `${(((i + 0.5) / buckets) * CHART_W).toFixed(1)} ${y(value)}`);
      healLine = `M0 ${y(healRates[0])} L${hp.join(' L')} L${CHART_W} ${y(healRates[buckets - 1])}`;
      healArea = `${healLine} L${CHART_W} ${CHART_H} L0 ${CHART_H} Z`;
    }
    $('heal-line').setAttribute('d', healLine);
    $('heal-area').setAttribute('d', healArea);
    $('peak').textContent = `Group DPS \u00b7 peak ${short(Math.max(1, ...rates))}/s`;
    renderBreakMarks();
  }

  function advance(t) {
    if (!acc || t < acc.t) {
      const n = session.actors.length;
      acc = {
        t: 0, cursor: 0, total: 0, hits: 0, crits: 0,
        damage: new Float64Array(n), actorHits: new Uint32Array(n), actorCrits: new Uint32Array(n),
        skills: Array.from({ length: n }, () => new Map()),
      };
    }
    const events = session.events;
    while (acc.cursor < events.length && events[acc.cursor][0] <= t) {
      const [, actor, damage, flags, skill] = events[acc.cursor++];
      const crit = flags & FLAG_CRIT ? 1 : 0;
      const key = skill * 2 + (flags & FLAG_DOT ? 1 : 0);
      let row = acc.skills[actor].get(key);
      if (!row) acc.skills[actor].set(key, (row = { skill, dot: Boolean(flags & FLAG_DOT), damage: 0, hits: 0, crits: 0 }));
      row.damage += damage;
      row.hits += 1;
      row.crits += crit;
      acc.damage[actor] += damage;
      acc.actorHits[actor] += 1;
      acc.actorCrits[actor] += crit;
      acc.total += damage;
      acc.hits += 1;
      acc.crits += crit;
    }
    acc.t = t;
  }

  function renderAt() {
    advance(playhead);
    hacc = heals.advance(hacc, session.heals, session.actors.length, playhead);
    sacc = sh.advance(sacc, session.stagger, session.actors.length, playhead);
    document.querySelectorAll('#break-marks .break-mark').forEach((mark) => {
      mark.classList.toggle('passed', Number(mark.dataset.t) <= playhead);
    });
    $('scrub').value = String(playhead);
    $('t-now').textContent = clock(playhead);
    const x = ((playhead / span) * CHART_W).toFixed(1);
    $('playhead').setAttribute('x1', x);
    $('playhead').setAttribute('x2', x);
    $('played-rect').setAttribute('width', x);
    renderPlayers();
    renderShare();
    renderHeal();
    renderStagger();
    renderFeed();
    renderState();
  }

  /** Stagger tab: estimated stagger per player and per ability, and the breaks, up to the playhead. */
  function renderStagger(force) {
    if (tab !== 'stagger' || !hasStagger) return;
    const all = session.stagger_breaks || [];
    const seen = sh.breaksUntil(all, playhead);
    const key = `${sacc ? sacc.cursor : 0}|${seen.length}|${Math.floor(playhead / 1000)}`;
    if (!force && key === staggerShown) return;
    staggerShown = key;
    const total = sacc ? sacc.total : 0;
    $('roster-sub').textContent = `Group ${sh.points(total, full) || '—'} · ${seen.length} break${seen.length === 1 ? '' : 's'} · at ${clock(playhead)}`;
    const rows = sh.byPlayer(sacc, session.actors);
    const top = rows.length ? rows[0].points : 1;
    $('stagger-rows').innerHTML = rows.length ? rows.map((p) => `<tr style="--pc:${pc(p.i)}" class="player${p.name === UNKNOWN ? ' unknown' : ''}">
        <td><span class="name pname">${esc(p.name)}</span></td>
        <td class="dmg stag-pts" title="Estimated stagger points">${sh.points(p.points, full)}</td>
        <td class="stag-pct">${pct(p.points / total, 0)}</td>
        <td><span class="share stag-bar"><i style="width:${Math.round((p.points / top) * 1000) / 10}%"></i></span></td>
      </tr>`).join('') : '<tr class="empty-row"><td colspan="4">No stagger yet at this point.</td></tr>';
    $('break-list').innerHTML = all.length
      ? all.map((ms, n) => `<li class="${ms <= playhead ? 'passed' : ''}"><span class="dim">Break ${n + 1}</span> <span class="when">${clock(ms)}</span></li>`).join('')
      : '<li class="dim">No stagger break in this fight.</li>';
    const maps = sh.skillMaps(sacc);
    const merged = abilityShare.merge(maps, maps.map((_, i) => i), session.skill_names);
    if (total <= 0) { $('stagger-skill-rows').innerHTML = '<tr class="empty-row"><td colspan="4">No stagger yet at this point.</td></tr>'; return; }
    const { shown, rest } = abilityShare.split(merged, 'damage');
    const line = (r) => `<tr>
        <td><span class="name">${esc(r.name || skillName(r.skill))}</span></td>
        <td class="dmg stag-pts">${sh.points(r.damage, full)}</td>
        <td class="dim">${pct(r.damage / total, 0)}</td>
        <td>${shareBar(r, total)}</td>
      </tr>`;
    const out = shown.map(line);
    if (rest.length) {
      const o = rest.reduce((t, r) => t + r.damage, 0);
      out.push(`<tr class="other-flat"><td><span class="name dim">Other (${rest.length})</span></td><td class="dmg stag-pts">${sh.points(o, full)}</td><td class="dim">${pct(o / total, 0)}</td><td><span class="share faint"><i style="width:${Math.round((o / total) * 1000) / 10}%"></i></span></td></tr>`);
    }
    $('stagger-skill-rows').innerHTML = out.join('');
  }

  /**
   * Keeps a row of buttons on one line: the ones that do not fit go into a
   * More menu. `key` is the data attribute naming each button (tab, filter);
   * `active()` the current one; `pick(id)` runs a menu choice.
   */
  function overflowBar({ bar, key, more, button, label, menu, active, pick, room, names, extraClass }) {
    let overflow = [];
    const buttons = () => [...bar.querySelectorAll(`:scope > button[data-${key}]`)];
    const ctl = {
      layout() {
        const list = buttons();
        const skip = new Set(list.filter((b) => b.dataset.off === '1').map((b) => b.dataset[key]));
        list.forEach((b) => { b.hidden = skip.has(b.dataset[key]); });
        more.hidden = false;
        const gap = parseFloat(getComputedStyle(bar).columnGap) || 0;
        const items = list.map((b) => ({ id: b.dataset[key], width: b.offsetWidth, primary: b.dataset.primary === '1', hidden: skip.has(b.dataset[key]) }));
        const res = tabOverflow.layout(items, room(), button.offsetWidth || 64, gap);
        list.forEach((b) => { if (!skip.has(b.dataset[key])) b.hidden = !res.visible.includes(b.dataset[key]); });
        overflow = res.overflow;
        more.hidden = overflow.length === 0;
        if (more.hidden) ctl.close(false);
        menu.innerHTML = overflow.map((id) => `<li role="none"><button type="button" role="menuitem" data-${key}="${id}" class="${(extraClass && extraClass(id)) || ''}" tabindex="-1">${esc(names()[id] || id)}</button></li>`).join('');
        ctl.paint();
      },
      paint() {
        const now = active();
        label.textContent = tabOverflow.moreLabel(now, overflow, names());
        button.classList.toggle('on', overflow.includes(now));
        menu.querySelectorAll('button').forEach((b) => b.classList.toggle('on', b.dataset[key] === now));
      },
      open() {
        menu.hidden = false;
        button.setAttribute('aria-expanded', 'true');
        const items = [...menu.querySelectorAll('button')];
        const at = Math.max(0, items.findIndex((b) => b.dataset[key] === active()));
        if (items[at]) items[at].focus();
      },
      close(focus) {
        if (menu.hidden) return;
        menu.hidden = true;
        button.setAttribute('aria-expanded', 'false');
        if (focus) button.focus();
      },
      button,
    };
    button.addEventListener('click', () => (menu.hidden ? ctl.open() : ctl.close(true)));
    button.addEventListener('keydown', (event) => {
      if (event.key === 'ArrowDown') { event.preventDefault(); ctl.open(); }
    });
    menu.addEventListener('click', (event) => {
      const b = event.target.closest(`button[data-${key}]`);
      if (!b) return;
      ctl.close(true);
      pick(b.dataset[key]);
    });
    menu.addEventListener('keydown', (event) => {
      const items = [...menu.querySelectorAll('button')];
      const at = items.indexOf(document.activeElement);
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); ctl.close(true); }
      else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const next = tabOverflow.step(at, items.length, event.key === 'ArrowDown' ? 1 : -1);
        if (next >= 0) items[next].focus();
      } else if (event.key === 'Tab') ctl.close(false);
    });
    document.addEventListener('pointerdown', (event) => {
      if (!more.contains(event.target)) ctl.close(false);
    });
    window.addEventListener('blur', () => ctl.close(false));
    if (window.ResizeObserver) new ResizeObserver(() => ctl.layout()).observe(bar.parentElement);
    else window.addEventListener('resize', () => ctl.layout());
    return ctl;
  }

  let tabsBar = null;
  let feedBar = null;
  const layoutTabs = () => { if (tabsBar) tabsBar.layout(); };
  const layoutFeedBar = () => { if (feedBar) feedBar.layout(); };

  /** Legend chips show or hide each chart series. Healing stays off without heals. */
  function paintSeries() {
    document.querySelectorAll('#legend .chip').forEach((b) => {
      const on = series[b.dataset.series];
      b.classList.toggle('on', on);
      b.setAttribute('aria-pressed', String(on));
      b.disabled = b.dataset.series === 'heal' && !hasHeals;
    });
    $('chart').classList.toggle('no-damage', !series.damage);
    $('chart').classList.toggle('no-heal', !series.heal);
  }

  /** Healing tab: a heal meter and heal by ability up to the playhead. */
  function renderHeal(force) {
    if (tab !== 'heal') return;
    const who = healScope === 'player' ? selectedActor : null;
    const key = `${hacc.cursor}|${healScope}|${who}|${healOtherOpen}|${Math.floor(playhead / 1000)}`;
    if (!force && key === healShown) return;
    healShown = key;
    healScopeLabel();
    const activeMs = Math.max(1000, session.duration_ms * (playhead / span));
    $('heal-empty').hidden = hasHeals;
    $('heal-grid').hidden = !hasHeals;
    $('heal-controls').hidden = !hasHeals;
    if (!hasHeals) { $('roster-sub').textContent = ''; return; }
    $('roster-sub').textContent = `Total ${hacc.total > 0 ? short(hacc.total) : '—'} \u00b7 ${short((hacc.total * 1000) / activeMs)} HPS \u00b7 at ${clock(playhead)}`;
    const rows = session.actors
      .map((name, i) => ({ i, name, heal: hacc.heal[i] }))
      .filter((p) => p.heal > 0)
      .sort((a, b) => b.heal - a.heal || a.name.localeCompare(b.name));
    const top = rows.length ? rows[0].heal : 1;
    $('heal-rows').innerHTML = rows.length ? rows.map((p) => `<tr style="--pc:${pc(p.i)}" class="player${p.name === UNKNOWN ? ' unknown' : ''}">
        <td><span class="name pname">${esc(p.name)}</span></td>
        <td class="dmg" title="${full(p.heal)}">${short(p.heal)}</td>
        <td class="dim">${short((p.heal * 1000) / activeMs)}</td>
        <td class="heal-pct">${pct(p.heal / hacc.total, 0)}</td>
        <td><span class="share heal-bar"><i style="width:${Math.round((p.heal / top) * 1000) / 10}%"></i></span></td>
      </tr>`).join('') : '<tr class="empty-row"><td colspan="5">No healing yet at this point.</td></tr>';
    const actors = who != null ? [who] : healScope === 'player' ? [] : hacc.skills.map((_, i) => i);
    const merged = abilityShare.merge(hacc.skills, actors, session.skill_names);
    const total = merged.reduce((t, r) => t + r.damage, 0);
    if (total <= 0) { $('heal-skill-rows').innerHTML = '<tr class="empty-row"><td colspan="4">No healing yet at this point.</td></tr>'; return; }
    const { shown, rest } = abilityShare.split(merged, 'damage');
    const line = (r, cls) => {
      const name = r.name || skillName(r.skill);
      const hot = r.dotDamage > 0 && r.dotDamage >= r.damage;
      return `<tr class="${cls}">
        <td><span class="name">${esc(name)}${hot ? '<span class="tag hot">HoT</span>' : ''}</span></td>
        <td class="dmg" title="${full(r.damage)}">${short(r.damage)}</td>
        <td class="dim">${pct(r.damage / total, 0)}</td>
        <td>${shareBar(r, total)}</td>
      </tr>`;
    };
    const out = shown.map((r) => line(r, ''));
    if (rest.length) {
      const o = rest.reduce((t, r) => t + r.damage, 0);
      out.push(`<tr class="other${healOtherOpen ? ' open' : ''}" id="heal-other" tabindex="0" role="button" aria-expanded="${healOtherOpen}">
        <td><span class="name">Other (${rest.length})</span><span class="chev" aria-hidden="true"></span></td>
        <td class="dmg" title="${full(o)}">${short(o)}</td>
        <td class="dim">${pct(o / total, 0)}</td>
        <td><span class="share faint"><i style="width:${Math.round((o / total) * 1000) / 10}%"></i></span></td>
      </tr>`);
      if (healOtherOpen) out.push(...rest.map((r) => line(r, 'sub')));
    }
    $('heal-skill-rows').innerHTML = out.join('');
  }

  function healScopeLabel() {
    const b = $('heal-scope-player');
    const name = selectedActor != null && session ? session.actors[selectedActor] : null;
    b.textContent = name != null ? name : 'Select player';
    b.classList.toggle('picked', name != null);
  }

  /**
   * Abilities and their part of the damage up to the playhead, for all
   * players or the selected one, from the same running totals as the
   * players table. Redrawn only when the hits or options change.
   */
  function renderShare(force) {
    if (tab !== 'share') return;
    const who = shareScope === 'player' ? selectedActor : null;
    const key = `${acc.cursor}|${shareScope}|${who}|${shareSort}|${Math.floor(playhead / 1000)}`;
    if (!force && key === shareShown) return;
    shareShown = key;
    scopeLabel();
    const actors = shareScope === 'player' ? (who == null ? [] : [who]) : acc.skills.map((_, i) => i);
    const merged = abilityShare.merge(acc.skills, actors, session.skill_names);
    const total = merged.reduce((t, r) => t + r.damage, 0);
    $('roster-sub').textContent = `Total damage ${total > 0 ? short(total) : '—'} \u00b7 at ${clock(playhead)}`;
    const empty = total <= 0;
    $('share-empty').hidden = !empty;
    $('share-rows').closest('table').hidden = empty;
    if (empty) { $('share-rows').innerHTML = ''; return; }
    const { shown, rest } = abilityShare.split(merged, shareSort);
    const pctOf = (d) => Math.round((d / total) * 1000) / 10;
    const line = (r, rank, cls) => {
      const name = r.name || skillName(r.skill);
      const allDot = r.dotDamage > 0 && r.dotDamage >= r.damage;
      const tag = allDot ? ' <em>(DoT)</em>' : r.dotDamage > 0 ? ' <em class="incl-dot">incl. DoT</em>' : '';
      const tip = [r.dotDamage > 0 && !allDot ? `DoT ${short(r.dotDamage)} of ${short(r.damage)}` : '', r.variants > 1 ? `${r.variants} skill ids` : ''].filter(Boolean).join('\n');
      return `<tr class="${cls}">
        <td class="rank">${rank}</td>
        <td><div class="who">${icon(r.skill, name)}<span class="name"${tip ? ` title="${esc(tip)}"` : ''}>${esc(name)}${tag}</span></div></td>
        <td class="dmg" title="${full(r.damage)}">${short(r.damage)}</td>
        <td class="dim">${r.hits}</td>
        <td class="dim">${rate(r.crits, r.hits, 0)}</td>
        <td class="share-pct">${pctOf(r.damage).toFixed(1)}%</td>
        <td>${shareBar(r, total)}</td>
      </tr>`;
    };
    const rows = shown.map((r, i) => line(r, i + 1, i === 0 ? 'top' : ''));
    if (rest.length) {
      const o = rest.reduce((t, r) => ({ damage: t.damage + r.damage, hits: t.hits + r.hits, crits: t.crits + r.crits }), { damage: 0, hits: 0, crits: 0 });
      rows.push(`<tr class="other${otherOpen ? ' open' : ''}" id="share-other" tabindex="0" role="button" aria-expanded="${otherOpen}" aria-controls="share-other-list" title="${otherOpen ? 'Hide' : 'Show'} the abilities in Other">
        <td class="rank">${shown.length + 1}</td>
        <td><div class="who"><span class="ab-icon other-icon">…</span><span class="name">Other (${rest.length})</span><span class="chev" aria-hidden="true"></span></div></td>
        <td class="dmg" title="${full(o.damage)}">${short(o.damage)}</td>
        <td class="dim">${o.hits}</td>
        <td class="dim">${rate(o.crits, o.hits, 0)}</td>
        <td class="share-pct">${pctOf(o.damage).toFixed(1)}%</td>
        <td><span class="share faint"><i style="width:${pctOf(o.damage)}%"></i></span></td>
      </tr>`);
      if (otherOpen) {
        rows.push(`<tr class="other-list" id="share-other-list"><td colspan="7"><div class="other-scroll"><table class="players-table share-table">
          <colgroup><col class="c-rank"><col><col class="c-num"><col class="c-pct"><col class="c-pct"><col class="c-pct"><col class="c-share"></colgroup>
          <tbody>${rest.map((r, i) => line(r, shown.length + 1 + i, 'sub')).join('')}</tbody></table></div></td></tr>`);
      }
    }
    $('share-rows').innerHTML = rows.join('');
  }

  /** An ability's share bar, split into one segment per player in that player's colour. */
  function shareBar(r, total) {
    const segs = playerColors.segments(r.by, (r.damage / total) * 100);
    const tip = segs.map((s) => `${session.actors[s.actor] || UNKNOWN}: ${short(s.damage)} (${s.pct.toFixed(1)}%)`).join('\n');
    return `<span class="share split" title="${esc(tip)}">${segs.map((s) => `<i style="width:${Math.round(s.width * 100) / 100}%;--pc:${pc(s.actor)}"></i>`).join('')}</span>`;
  }

  /** Recolour players for the open session, e.g. after you pick a new colour. */
  function paintColors() {
    colors = session ? playerColors.colorsFor(session.actors, session.you == null ? null : session.you, youColor) : [];
  }

  function refreshYouColor() {
    youColor = playerColors.readYouColor();
    if (!session) return;
    paintColors();
    renderPlayers();
    renderShare(true);
    renderHeal(true);
    renderStagger(true);
    renderFeed(true);
  }

  function setTab(next) {
    tab = next;
    document.querySelectorAll('#tabs > button[data-tab]').forEach((b) => b.classList.toggle('on', b.dataset.tab === tab));
    $('players-view').hidden = tab !== 'players';
    $('roster-hint').hidden = tab !== 'players';
    $('share-view').hidden = tab !== 'share';
    $('share-controls').hidden = tab !== 'share';
    $('heal-view').hidden = tab !== 'heal';
    $('heal-controls').hidden = tab !== 'heal' || !hasHeals;
    $('stagger-view').hidden = tab !== 'stagger';
    layoutTabs();
    if (!session) return;
    if (tab === 'share') renderShare(true);
    else if (tab === 'heal') renderHeal(true);
    else if (tab === 'stagger') renderStagger(true);
    else renderPlayers();
  }

  const skillName = (skill) => (session.skill_names && session.skill_names[skill]) || `Skill ${skill}`;
  const stamp = (ms) => `${clock(ms)}.${Math.floor((ms % 1000) / 100)}`;
  /** A stand-in for the skill icon: its first letter on a colour from the id. */
  const icon = (skill, name) => `<span class="ab-icon" style="--h:${(skill * 47) % 360}">${esc(name.charAt(0).toUpperCase())}</span>`;

  function toggleOther() {
    otherOpen = !otherOpen;
    renderShare(true);
    const row = $('share-other');
    if (row) row.focus();
  }

  /** The Select player button names the chosen player, else asks for one. */
  function scopeLabel() {
    const b = $('scope-player');
    const name = selectedActor != null && session ? session.actors[selectedActor] : null;
    b.textContent = name != null ? name : 'Select player';
    b.classList.toggle('picked', name != null);
    b.title = name != null ? `Abilities of ${name}. Click to pick another player.` : 'Pick a player';
  }

  /** The feed's Select player button names the chosen player, like the share scope. */
  function feedLabel() {
    const b = $('filter-player');
    const name = selectedActor != null && session ? session.actors[selectedActor] : null;
    b.textContent = name != null ? name : 'Select player';
    b.classList.toggle('picked', name != null);
    b.title = name != null ? `Hits by ${name}. Click to pick another player.` : 'Pick a player';
    if (feedBar) feedBar.layout();
  }

  /** A new session starts on All players in both the share scope and the feed. */
  function resetScopes() {
    shareScope = 'all';
    healScope = 'all';
    feedFilter = 'all';
    document.querySelectorAll('#share-scope > button, #heal-scope > button').forEach((b) => b.classList.toggle('on', b.dataset.scope === 'all'));
    document.querySelectorAll('#feed-filters > button').forEach((b) => b.classList.toggle('on', b.dataset.filter === 'all'));
    closePicker(false);
    scopeLabel();
    feedLabel();
  }

  /** Each Select player button and the list it opens. */
  const PICKERS = { share: ['scope-player', 'player-pick'], feed: ['filter-player', 'feed-pick'], heal: ['heal-scope-player', 'heal-pick'] };

  /** The player picker under Select player: everyone in the session, by damage at the playhead. */
  function openPicker(which) {
    if (!session) return;
    closePicker(false);
    pickFor = which;
    const list = $(PICKERS[which][1]);
    const rows = session.actors
      .map((name, i) => ({ i, name, damage: acc.damage[i] || 0 }))
      .sort((a, b) => b.damage - a.damage || a.name.localeCompare(b.name));
    list.innerHTML = rows.length
      ? rows.map((p) => `<li role="option" tabindex="-1" data-actor="${p.i}" aria-selected="${p.i === selectedActor}" class="${p.i === selectedActor ? 'sel' : ''}" style="--pc:${pc(p.i)}">
          <span class="pick-dot" aria-hidden="true"></span><span class="pick-name">${esc(p.name)}</span><span class="pick-dmg" title="${full(p.damage)}">${short(p.damage)}</span></li>`).join('')
      : '<li class="pick-empty">No players in this session.</li>';
    list.hidden = false;
    $(PICKERS[which][0]).setAttribute('aria-expanded', 'true');
    const first = list.querySelector('li.sel') || list.querySelector('li[role="option"]');
    if (first) first.focus();
  }

  function closePicker(refocus) {
    if (!pickFor) return;
    const [button, list] = PICKERS[pickFor];
    pickFor = null;
    $(list).hidden = true;
    $(button).setAttribute('aria-expanded', 'false');
    if (refocus) ($(button).hidden && button === 'filter-player' ? $('feed-more-btn') : $(button)).focus();
  }

  /** Picking a player is the same selection the Players tab and the feed filter use. */
  function pickPlayer(i) {
    selectedActor = i;
    if (pickFor === 'feed') {
      feedFilter = 'player';
      document.querySelectorAll('#feed-filters > button').forEach((b) => b.classList.toggle('on', b.dataset.filter === 'player'));
      if (feedBar) feedBar.paint();
    } else if (pickFor === 'heal') {
      healScope = 'player';
      document.querySelectorAll('#heal-scope > button').forEach((b) => b.classList.toggle('on', b.dataset.scope === 'player'));
    } else {
      shareScope = 'player';
      document.querySelectorAll('#share-scope > button').forEach((b) => b.classList.toggle('on', b.dataset.scope === 'player'));
    }
    closePicker(true);
    renderPlayers();
    renderShare(true);
    renderHeal(true);
    renderFeed(true);
  }

  function feedPlayer() {
    if (selectedActor != null) return selectedActor;
    let best = null;
    acc.damage.forEach((d, i) => { if (d > 0 && (best == null || d > acc.damage[best])) best = i; });
    return best;
  }

  /** Newest hits and heals first, up to the playhead. Redrawn only when it changes. */
  function renderFeed(force) {
    const cursor = acc.cursor;
    const hcursor = hacc ? hacc.cursor : 0;
    const shownKey = `${cursor}|${hcursor}`;
    if (!force && shownKey === feedShown) return;
    feedShown = shownKey;
    // While playing, hits from the last 600 ms of real time stay highlighted.
    const fresh = playing ? playhead - NEW_MS * speed : Infinity;
    const who = feedFilter === 'player' ? feedPlayer() : null;
    feedLabel();
    $('feed-sub').textContent = `at ${clock(playhead)} \u00b7 ${speed}x`;
    const rows = [];
    const target = session.target.label;
    const healList = hasHeals && feedFilter !== 'crits' && feedFilter !== 'damage' ? session.heals : [];
    let h = healList.length ? hcursor - 1 : -1;
    let i = feedFilter === 'heals' ? -1 : cursor - 1;
    while ((i >= 0 || h >= 0) && rows.length < FEED_ROWS) {
      if (h >= 0 && (i < 0 || healList[h][0] >= session.events[i][0])) {
        const [t, actor, amount, flags, skill] = healList[h--];
        if (feedFilter === 'player' && actor !== who) continue;
        const name = skillName(skill);
        const hot = flags & heals.FLAG_HOT;
        rows.push(`<li class="heal${t >= fresh ? ' new' : ''}">
        <span class="ab-time">${stamp(t)}</span>${icon(skill, name)}
        <span class="ab-text"><span class="ab-name">${esc(name)}${hot ? ' \u00b7 tick' : ''}</span><span class="ab-who"><span class="pname" style="--pc:${pc(actor)}">${esc(session.actors[actor] || UNKNOWN)}</span></span></span>
        <span class="ab-heal-tag">HEAL</span><span class="ab-dmg heal" title="${full(amount)}">+${full(amount)}</span>
      </li>`);
        continue;
      }
      const [t, actor, damage, flags, skill] = session.events[i--];
      if (feedFilter === 'crits' && !(flags & FLAG_CRIT)) continue;
      if (feedFilter === 'player' && actor !== who) continue;
      const crit = flags & FLAG_CRIT;
      const dot = flags & FLAG_DOT;
      const name = skillName(skill);
      const cls = [crit ? 'crit' : '', dot ? 'dot' : '', t >= fresh ? 'new' : ''].filter(Boolean).join(' ');
      rows.push(`<li class="${cls}">
        <span class="ab-time">${stamp(t)}</span>${icon(skill, name)}
        <span class="ab-text"><span class="ab-name">${esc(name)}${dot ? ' <em>(tick)</em>' : ''}${crit ? '<span class="ab-crit">CRIT</span>' : ''}</span><span class="ab-who"><span class="pname" style="--pc:${pc(actor)}">${esc(session.actors[actor] || UNKNOWN)}</span> → ${esc(target)}</span></span>
        <span class="ab-dmg" title="${full(damage)}">${short(damage)}</span>
      </li>`);
    }
    $('feed').innerHTML = rows.join('');
    const empty = $('feed-empty');
    empty.hidden = rows.length > 0;
    empty.innerHTML = cursor === 0 && hcursor === 0
      ? '<strong>Press play to replay this session</strong><span>Abilities will appear here as they happen.</span>'
      : '<strong>No hits match</strong><span>Nothing for this filter up to the playhead.</span>';
  }

  /** One player's abilities up to the playhead, under their row. */
  function abilityRows(i, damage) {
    const list = [...acc.skills[i].values()].sort((a, b) => b.damage - a.damage);
    const top = Math.max(1, list.length ? list[0].damage : 1);
    const staggerOf = (r) => (r.dot || !sacc ? 0 : sacc.skills[i].get(r.skill) || 0);
    return `<tr class="ab-head"><td></td><td>ABILITY</td><td>DAMAGE</td><td>HITS</td><td>% OF PLAYER</td><td>CRIT %</td><td class="stag">STAGGER ~</td><td></td></tr>`
      + list.map((r) => {
        const name = skillName(r.skill);
        return `<tr class="ab-row">
        <td></td>
        <td><div class="who">${icon(r.skill, name)}<span class="name">${esc(name)}${r.dot ? ' <em>(DoT)</em>' : ''}</span></div></td>
        <td class="dmg" title="${full(r.damage)}">${short(r.damage)}</td>
        <td class="dim">${r.hits}</td>
        <td class="dim">${pct(r.damage / damage, 1)}</td>
        <td class="dim">${rate(r.crits, r.hits)}</td>
        <td class="dim stag" title="Estimated stagger points">${sh.points(staggerOf(r), full) || '\u2014'}</td>
        <td><span class="share"><i style="width:${Math.round((r.damage / top) * 1000) / 10}%"></i></span></td>
      </tr>`;
      }).join('');
  }

  /** DPS uses the saved fight clock, scaled to the playhead. */
  function renderPlayers() {
    const body = $('players');
    const activeMs = Math.max(1000, session.duration_ms * (playhead / span));
    const rows = session.actors
      .map((name, i) => ({ i, name, damage: acc.damage[i], hits: acc.actorHits[i], crits: acc.actorCrits[i] }))
      .filter((p) => p.damage > 0)
      .sort((a, b) => b.damage - a.damage || a.name.localeCompare(b.name));
    renderLive(activeMs, rows);
    $('group-row').hidden = !rows.length;
    const sub = tab === 'players' ? $('roster-sub') : { textContent: '' };
    if (!rows.length) {
      sub.textContent = `At ${clock(playhead)}`;
      body.innerHTML = `<tr class="empty-row"><td colspan="${hasStagger ? 8 : 7}">No damage yet at this point.</td></tr>`;
      return;
    }
    sub.textContent = `${rows.length} ${rows.length === 1 ? 'player' : 'players'} \u00b7 at ${clock(playhead)}`;
    const top = Math.max(1, rows[0].damage);
    body.innerHTML = rows.map((p, i) => `<tr data-actor="${p.i}" style="--pc:${pc(p.i)}" class="player${p.name === UNKNOWN ? ' unknown' : ''}${p.i === selectedActor ? ' sel' : ''}${expanded.has(p.i) ? ' open' : ''}">
        <td class="rank">${i + 1}</td>
        <td><div class="who"><span class="caret" aria-hidden="true"></span><span class="avatar"></span><span class="name">${esc(p.name)}</span></div></td>
        <td class="dmg" title="${full(p.damage)}">${short(p.damage)}</td>
        <td class="dim">${short((p.damage * 1000) / activeMs)}</td>
        <td class="dim">${pct(p.damage / acc.total, 1)}</td>
        <td class="dim">${rate(p.crits, p.hits)}</td>
        <td class="dim stag" title="Estimated stagger points">${sh.points(sacc ? sacc.actor[p.i] : 0, full) || '\u2014'}</td>
        <td><span class="share"><i style="width:${Math.round((p.damage / top) * 1000) / 10}%"></i></span></td>
      </tr>${expanded.has(p.i) ? abilityRows(p.i, p.damage) : ''}`).join('');
    $('foot-total').textContent = short(acc.total);
    $('foot-dps').textContent = short((acc.total * 1000) / activeMs);
    $('foot-crit').textContent = rate(acc.crits, acc.hits);
    $('foot-stagger').textContent = hasStagger ? sh.points(sacc ? sacc.total : 0, full) || '\u2014' : '';
  }

  function seek(t) {
    playhead = Math.min(span, Math.max(0, Number(t) || 0));
    renderAt();
  }

  function paintPlay() {
    $('play').classList.toggle('playing', playing);
    $('play').title = playing ? 'Pause' : 'Play';
  }

  function play() {
    if (!session) return;
    if (playhead >= span) seek(0);
    playing = true;
    lastFrame = performance.now();
    cancelAnimationFrame(frameId);
    frameId = requestAnimationFrame(frame);
    paintPlay();
    renderState();
  }

  function pause() {
    playing = false;
    cancelAnimationFrame(frameId);
    paintPlay();
    if (session) renderState();
  }

  /** At the end, playback stops and the button is back to Play. */
  function frame(now) {
    if (!playing) return;
    playhead = Math.min(span, playhead + (now - lastFrame) * speed);
    lastFrame = now;
    renderAt();
    if (playhead >= span) pause();
    else frameId = requestAnimationFrame(frame);
  }

  function beginRename(id) {
    if (!id || renamingId === id) return;
    renamingId = id;
    if (selectedId !== id) selectedId = id;
    renderList();
  }

  function cancelRename() {
    if (!renamingId) return;
    renamingId = null;
    renderList();
  }

  async function commitRename(input) {
    if (!renamingId || !input) return;
    const id = renamingId;
    const next = input.value;
    renamingId = null;
    const current = summaries.find((s) => s.id === id);
    if (current && next.trim() === (current.name || '').trim()) {
      renderList();
      return;
    }
    // Empty clears back to the target label.
    try {
      const summary = await bridge.renameSession(id, next);
      const at = summaries.findIndex((s) => s.id === id);
      if (at >= 0) summaries[at] = { ...summaries[at], ...summary };
      if (session && loadedId === id) {
        session.name = summary.name || null;
        if (rawSession) rawSession.name = summary.name || null;
        renderSummary();
      }
      note('');
    } catch (err) {
      note(`Could not rename: ${(err && err.message) || err}`);
    }
    renderList();
    if (selectedId === id && loadedId !== id) select(id);
  }

  $('sessions').addEventListener('click', (event) => {
    const renameBtn = event.target.closest('button[data-rename]');
    if (renameBtn) {
      event.preventDefault();
      event.stopPropagation();
      beginRename(renameBtn.dataset.rename);
      return;
    }
    if (renamingId) return;
    const item = event.target.closest('button.session[data-id]');
    if (item && item.dataset.id !== loadedId) select(item.dataset.id);
  });
  $('sessions').addEventListener('dblclick', (event) => {
    const label = event.target.closest('.s-label');
    if (!label) return;
    const row = label.closest('[data-id]');
    if (row) beginRename(row.dataset.id);
  });
  $('sessions').addEventListener('keydown', (event) => {
    const input = event.target.closest('.s-edit');
    if (!input) return;
    if (event.key === 'Enter') {
      event.preventDefault();
      commitRename(input);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      cancelRename();
    }
  });
  $('sessions').addEventListener('focusout', (event) => {
    const input = event.target.closest('.s-edit');
    if (!input || !renamingId) return;
    // Let focus move inside the editor without saving.
    const next = event.relatedTarget;
    if (next && input.closest('.session-item') && input.closest('.session-item').contains(next)) return;
    commitRename(input);
  });
  $('play').addEventListener('click', () => (playing ? pause() : play()));
  $('scrub').addEventListener('input', () => seek($('scrub').value));
  $('chart').addEventListener('click', (event) => {
    if (!session) return;
    const box = $('chart').getBoundingClientRect();
    seek(((event.clientX - box.left) / box.width) * span);
  });
  document.querySelectorAll('#speeds button').forEach((b) => b.addEventListener('click', () => {
    speed = Number(b.dataset.speed);
    document.querySelectorAll('#speeds button').forEach((other) => other.classList.toggle('on', other === b));
    if (session) { renderFeed(true); renderState(); }
  }));
  $('players').addEventListener('click', (event) => {
    const row = event.target.closest('tr[data-actor]');
    if (!row) return;
    const i = Number(row.dataset.actor);
    if (expanded.has(i)) expanded.delete(i);
    else expanded.add(i);
    selectedActor = i;
    renderPlayers();
    renderFeed(true);
    if (tab === 'share') renderShare(true);
    else scopeLabel();
    feedLabel();
  });
  document.querySelectorAll('#tabs > button[data-tab]').forEach((b) => b.addEventListener('click', () => setTab(b.dataset.tab)));
  tabsBar = overflowBar({
    bar: $('tabs'), key: 'tab', more: $('tab-more'), button: $('more-btn'), label: $('more-label'), menu: $('more-menu'),
    active: () => tab, pick: (id) => setTab(id), names: () => TAB_NAMES,
    // The sort/scope controls wrap to their own line; keep room for a short sub line.
    room: () => Math.max(0, $('tabs').parentElement.clientWidth - SUB_MIN - (parseFloat(getComputedStyle($('tabs').parentElement).columnGap) || 0) - 2),
    extraClass: (id) => (id === 'heal' ? 'heal-tab' : ''),
  });
  feedBar = overflowBar({
    bar: $('feed-filters'), key: 'filter', more: $('feed-more'), button: $('feed-more-btn'), label: $('feed-more-label'), menu: $('feed-more-menu'),
    active: () => feedFilter,
    pick: (id) => {
      if (id === 'player') { openPicker('feed'); return; }
      const b = document.querySelector(`#feed-filters > button[data-filter="${id}"]`);
      if (b) b.click();
      feedBar.paint();
    },
    names: () => ({ all: 'All', player: $('filter-player').textContent, crits: 'Crits only', damage: 'Damage', heals: 'Heals' }),
    room: () => {
      const bar = $('feed-filters');
      const cs = getComputedStyle(bar);
      const pad = parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight) + parseFloat(cs.borderLeftWidth) + parseFloat(cs.borderRightWidth);
      return Math.max(0, bar.parentElement.clientWidth - pad - 1);
    },
  });
  layoutTabs();
  layoutFeedBar();
  $('share-rows').addEventListener('click', (event) => {
    if (event.target.closest('#share-other')) toggleOther();
  });
  $('share-rows').addEventListener('keydown', (event) => {
    if ((event.key === 'Enter' || event.key === ' ') && event.target.id === 'share-other') {
      event.preventDefault();
      toggleOther();
    }
  });
  const toggleHealOther = () => {
    healOtherOpen = !healOtherOpen;
    renderHeal(true);
    const row = $('heal-other');
    if (row) row.focus();
  };
  $('heal-skill-rows').addEventListener('click', (event) => {
    if (event.target.closest('#heal-other')) toggleHealOther();
  });
  $('heal-skill-rows').addEventListener('keydown', (event) => {
    if ((event.key === 'Enter' || event.key === ' ') && event.target.id === 'heal-other') {
      event.preventDefault();
      toggleHealOther();
    }
  });
  $('heal-scope').querySelector('[data-scope="all"]').addEventListener('click', (event) => {
    closePicker(false);
    healScope = 'all';
    document.querySelectorAll('#heal-scope > button').forEach((other) => other.classList.toggle('on', other === event.currentTarget));
    if (session) renderHeal(true);
  });
  document.querySelectorAll('#legend .chip').forEach((b) => b.addEventListener('click', (event) => {
    event.stopPropagation();
    series[b.dataset.series] = !series[b.dataset.series];
    paintSeries();
  }));
  $('share-scope').querySelector('[data-scope="all"]').addEventListener('click', (event) => {
    closePicker(false);
    shareScope = 'all';
    document.querySelectorAll('#share-scope > button').forEach((other) => other.classList.toggle('on', other === event.currentTarget));
    if (session) renderShare(true);
  });
  Object.entries(PICKERS).forEach(([which, [button, list]]) => {
    $(button).addEventListener('click', () => {
      if (pickFor === which) closePicker(true);
      else openPicker(which);
    });
    $(button).addEventListener('keydown', (event) => {
      if (event.key === 'ArrowDown' && pickFor !== which) { event.preventDefault(); openPicker(which); }
    });
    $(list).addEventListener('click', (event) => {
      const li = event.target.closest('li[data-actor]');
      if (li) pickPlayer(Number(li.dataset.actor));
    });
    $(list).addEventListener('keydown', onPickKey);
  });
  function onPickKey(event) {
    const items = [...event.currentTarget.querySelectorAll('li[role="option"]')];
    const at = items.indexOf(document.activeElement);
    if (event.key === 'Escape') { event.preventDefault(); closePicker(true); }
    else if (event.key === 'Tab') closePicker(false);
    else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const next = items[Math.min(items.length - 1, Math.max(0, at + (event.key === 'ArrowDown' ? 1 : -1)))];
      if (next) next.focus();
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      const next = event.key === 'Home' ? items[0] : items[items.length - 1];
      if (next) next.focus();
    } else if ((event.key === 'Enter' || event.key === ' ') && at >= 0) {
      event.preventDefault();
      pickPlayer(Number(items[at].dataset.actor));
    }
  }
  document.addEventListener('mousedown', (event) => {
    if (!event.target.closest('.pick-list, #scope-player, #filter-player, #heal-scope-player, #feed-more')) closePicker(false);
  });
  document.querySelectorAll('#share-sort button').forEach((b) => b.addEventListener('click', () => {
    shareSort = b.dataset.sort;
    document.querySelectorAll('#share-sort button').forEach((other) => other.classList.toggle('on', other === b));
    if (session) renderShare(true);
  }));
  document.querySelectorAll('#feed-filters > button:not(#filter-player)').forEach((b) => b.addEventListener('click', () => {
    closePicker(false);
    feedFilter = b.dataset.filter;
    document.querySelectorAll('#feed-filters > button').forEach((other) => other.classList.toggle('on', other === b));
    if (feedBar) feedBar.paint();
    if (session) renderFeed(true);
  }));
  window.addEventListener('storage', (event) => {
    if (event.key === playerColors.YOU_KEY) refreshYouColor();
  });
  window.addEventListener('keydown', (event) => {
    if (event.code !== 'Space' || !session || event.target.closest('input, button, textarea, [role="option"]')) return;
    event.preventDefault();
    if (playing) pause();
    else play();
  });
  $('export').addEventListener('click', () => {
    if (session) bridge.exportSession(loadedId, rawSession);
  });
  $('delete').addEventListener('click', async () => {
    if (!session) return;
    const label = displayName(session);
    if (!confirm(`Delete the saved session "${label}" from ${when(session.started_at)}? This cannot be undone.`)) return;
    const index = summaries.findIndex((s) => s.id === loadedId);
    try {
      await bridge.deleteSession(loadedId);
    } catch (err) {
      note(`Could not delete: ${(err && err.message) || err}`);
      return;
    }
    const next = summaries[index + 1] || summaries[index - 1];
    selectedId = next ? next.id : null;
    show(null, null);
    refresh();
  });

  bridge.onSessionsChanged(refresh);
  refresh();
})();
