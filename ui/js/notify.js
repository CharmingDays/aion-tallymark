// Event toasts. In the desktop app they show in their own small window at the
// top center of the screen, over the game (toast.html); in a browser they
// show inside the page (the overlay as a compact pill, the meter as a full
// banner). A toast slides in at the top center (or the
// bottom, per the alert settings), hides itself after its event's
// notification duration (8 s by default; "until dismissed" never) unless the
// pointer is on it, and stacks first come, first served: the oldest on top, the
// next dimmed below, the rest folded into a "+N more" chip. Only one toast
// counts down at a time (the top one, or the dimmed one when the top is
// "until dismissed" or an imminent one waiting for its start), so each gets
// its full time on top. Toasts arriving together (within 250 ms) go shortest
// time first. A toast with the same key replaces the old one in place. The
// rules live in toast-queue.js.
//
// Kinds: `upcoming` (blue, "in 5:00" chip, optional Snooze), `imminent`
// (amber, the last 30 s, a countdown line along the bottom, stays until the
// event starts, whatever the duration, unless dismissed) and `live` (green, LIVE chip).
//
// `pet` toasts (Figma "Toast - Pet soul progress") show one pet's soul
// progress. There is one per pet (key `pet:<id>`): another soul updates its
// counter and bar in place and restarts its own timer (no hide and re-show),
// a summon or level up switches it to the completed look, and a soul for a
// different pet takes over the one pet toast (never two at once). They keep their own clock
// (see toast-queue.js), hiding `hideMs` after the last soul.
//
// Each kind has a style (Config > Toasts & pop-ups, toast-style.js): on/off,
// size, colours, opacity, icon, sound and title/message templates. The app adds
// it to each toast as `spec.style`; a browser page uses the last live snapshot.
(function () {
  const { esc } = window.fmt;
  const { countdown, due, IMMINENT_MS, MINUTE } = window.alertSchedule;
  const queue = window.toastQueue;
  const bridge = window.meterBridge;

  const HIDE_MS = 8000;
  const LEAVE_MS = 160;
  const TICK_MS = 250;
  const SNOOZE_MS = MINUTE;
  /** Missed time (a sleeping PC) older than this does not fire late toasts. */
  const CATCH_UP_MS = 5000;

  const ICON_PAW = '<svg viewBox="0 0 20 20" aria-hidden="true"><ellipse cx="5" cy="8" rx="1.8" ry="2.3"/><ellipse cx="8.5" cy="4.8" rx="1.8" ry="2.3"/><ellipse cx="12.5" cy="4.8" rx="1.8" ry="2.3"/><ellipse cx="16" cy="8" rx="1.8" ry="2.3"/><path d="M10.5 9.5c2.6 0 5 2.6 5 4.9 0 1.6-1.3 2.3-2.6 2.1-1-.2-1.6-.6-2.4-.6s-1.4.4-2.4.6c-1.3.2-2.6-.5-2.6-2.1 0-2.3 2.4-4.9 5-4.9z"/></svg>';
  const ICON_CHECK = '<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 6.3l2.3 2.3 4.7-5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  const FAMILIES = ['cogni', 'fera', 'natura', 'varian', 'special'];

  /** The pet toast's lines for `p` (a PetToast) and the souls gained since it showed. */
  function petLines(p, gainedTotal, prevNeeded) {
    const next = p.needed == null ? 'max level' : `${p.needed - p.souls} more to ${p.level === 0 ? 'summon' : `level ${p.level + 1}`}`;
    if (p.stepped) {
      const what = p.level <= 1 ? 'Summoned!' : `Level ${p.level}!`;
      const after = p.needed == null ? 'max level' : `next: level ${p.level + 1} (${p.souls} / ${p.needed})`;
      return { done: true, status: `${what} \u00b7 ${after}`, count: prevNeeded ? `${prevNeeded} / ${prevNeeded}` : 'Done', share: 1 };
    }
    return {
      done: false,
      status: `Soul collected +${gainedTotal} \u00b7 ${next}`,
      count: p.needed == null ? 'Max' : `${p.souls} / ${p.needed}`,
      share: p.needed ? Math.min(1, p.souls / p.needed) : 1,
    };
  }

  const ICON_CLOSE = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>';

  const styles = window.toastStyle;
  const chime = styles.chime;

  /**
   * `container`: show the toasts inside it (the Config preview), not over the page.
   * `scale`: size toasts by their style (the toast window sizes itself instead).
   * `silent`: never chime (the preview redraws often).
   */
  function createHost({ compact = false, onChange = null, container = null, scale = true, silent = false } = {}) {
    const host = document.createElement('div');
    host.className = `notify-host top${compact ? ' compact' : ''}${container ? ' inline' : ''}`;
    host.setAttribute('aria-live', 'polite');
    (container || document.body).append(host);

    /** Queue order, top first. Each: { key, kind, title, message, eventAt, snooze, hideLeft, el, hovered } */
    let toasts = [];
    let onSnooze = null;

    function chip(t, now) {
      if (t.kind === 'live') return 'LIVE';
      if (t.eventAt == null) return '';
      const left = countdown(t.eventAt - now);
      return t.kind === 'upcoming' ? `in ${left}` : left;
    }

    /** `{text}` with the live countdown filled in where `{left}` stands. */
    const fill = (text, t, now) => String(text || '').replace('{left}', t.eventAt == null ? '' : countdown(t.eventAt - now));

    /** An event toast's title and message: its style's templates, or the built-in text. */
    function eventText(t, now) {
      const s = t.styleNow;
      const left = t.eventAt == null ? '' : countdown(t.eventAt - now);
      const fields = { event: t.event || t.title, message: t.note || '', left };
      return {
        title: s.title ? styles.fill(s.title, fields) : fill(t.title, t, now),
        message: s.message ? styles.fill(s.message, fields) : fill(t.message, t, now),
      };
    }

    /** Put the kind's style on a toast's element. */
    function dress(t) {
      t.styleNow = styles.styleFor(t);
      styles.apply(t.el, t.styleNow, t.kind, { scale });
    }

    function buildPet(t) {
      const el = document.createElement('div');
      el.className = 'toast t-pet entering';
      el.innerHTML = `<span class="tp-img"><img alt="" hidden>${ICON_PAW}<span class="tp-done">${ICON_CHECK}</span></span>
        <span class="t-text tp-text"><span class="tp-head"><b class="tp-name"></b><span class="tp-fam"></span><span class="tp-count"></span></span>
        <span class="tp-status"></span><span class="tp-prog"><i></i></span></span>
        ${compact ? '' : `<button type="button" class="t-close" title="Dismiss">${ICON_CLOSE}</button>`}
        <i class="t-bar"></i><span class="t-more" hidden></span>`;
      const img = el.querySelector('img');
      img.addEventListener('error', () => { img.hidden = true; el.classList.remove('has-img'); });
      el.addEventListener('mouseenter', () => { t.hovered = true; });
      el.addEventListener('mouseleave', () => { t.hovered = false; });
      el.addEventListener('click', (event) => {
        if (compact || event.target.closest('.t-close')) dismiss(t);
      });
      el.addEventListener('animationend', (event) => {
        if (event.target === el) el.classList.remove('entering');
      });
      return el;
    }

    /** Fill a pet toast from `t.pet`; `bump` flashes the counter on an update. */
    function paintPet(t, bump) {
      const el = t.el;
      const p = t.pet || {};
      const lines = petLines(p, t.gainedTotal || 0, t.prevNeeded);
      const fam = String(p.family || '').toLowerCase();
      el.classList.toggle('done', lines.done);
      FAMILIES.forEach((f) => el.classList.toggle(`fam-${f}`, f === fam));
      const s = t.styleNow || styles.defaults('pet');
      const fields = {
        pet: p.name || 'Pet', family: p.family || '', level: p.level == null ? '' : p.level,
        souls: p.souls == null ? '' : p.souls, needed: p.needed == null ? 'max' : p.needed,
        count: lines.count, gained: t.gainedTotal || 0, status: lines.status,
        next: p.needed == null ? 'max level' : `${p.needed - p.souls} more`,
      };
      el.querySelector('.tp-name').textContent = s.title ? styles.fill(s.title, fields) : fields.pet;
      el.querySelector('.tp-fam').textContent = p.family || '';
      el.querySelector('.tp-count').textContent = lines.count;
      el.querySelector('.tp-status').textContent = s.message ? styles.fill(s.message, fields) : lines.status;
      el.querySelector('.tp-prog > i').style.width = `${lines.share * 100}%`;
      const img = el.querySelector('img');
      if (t.icon && img.getAttribute('src') !== t.icon) {
        img.src = t.icon;
        img.hidden = false;
        el.classList.add('has-img');
      }
      if (bump) {
        const count = el.querySelector('.tp-count');
        count.classList.remove('bump');
        void count.offsetWidth;
        count.classList.add('bump');
      }
    }

    /** Take in a pet update; true when it went to a toast already showing. */
    function updatePet(spec, now) {
      if (spec.iconOnly) {
        const t = toasts.find((o) => o.key === spec.key);
        if (t && spec.icon) { t.icon = spec.icon; paintPet(t, false); }
        return true;
      }
      // At most one pet toast: another pet's soul takes it over in place.
      const t = queue.petTarget(toasts, spec.key);
      if (!t) return false;
      const switched = t.key !== spec.key;
      if (switched) {
        t.key = spec.key;
        t.pet = null;
        t.icon = null;
        t.gainedTotal = 0;
        t.prevNeeded = null;
        t.el.querySelector('img').hidden = true;
        t.el.classList.remove('has-img');
      }
      const before = t.pet;
      if (spec.pet && spec.pet.stepped) t.prevNeeded = before && !before.stepped ? before.needed : t.prevNeeded;
      else if (before && before.stepped) t.gainedTotal = 0;
      t.pet = spec.pet;
      t.gainedTotal = (t.gainedTotal || 0) + ((spec.pet && spec.pet.gained) || 0);
      if (spec.icon) t.icon = spec.icon;
      if (spec.style) t.style = spec.style;
      dress(t);
      if (!silent && styles.soundOn(t.styleNow, false)) chime(`pet:${Date.now()}`);
      t.hideTotal = Number(spec.hideMs) > 0 ? Number(spec.hideMs) : t.hideTotal;
      t.hideLeft = t.hideTotal;
      paintPet(t, true);
      paint(now);
      return true;
    }

    function build(t) {
      if (t.kind === 'pet') return buildPet(t);
      const el = document.createElement('div');
      el.className = `toast t-${t.kind} entering`;
      el.innerHTML = `<span class="t-dot"></span>
        <span class="t-text"><b class="t-title"></b><span class="t-msg"></span></span>
        ${t.snooze ? '<button type="button" class="t-act">Snooze</button>' : ''}
        <span class="t-chip"></span>
        ${compact ? '' : `<button type="button" class="t-close" title="Dismiss">${ICON_CLOSE}</button>`}
        <i class="t-bar"></i><span class="t-more" hidden></span>`;
      el.addEventListener('mouseenter', () => { t.hovered = true; });
      el.addEventListener('mouseleave', () => { t.hovered = false; });
      el.addEventListener('click', (event) => {
        if (event.target.closest('.t-act')) {
          dismiss(t);
          if (onSnooze) onSnooze(t);
        } else if (compact || event.target.closest('.t-close')) {
          dismiss(t);
        }
      });
      el.addEventListener('animationend', () => el.classList.remove('entering'));
      return el;
    }

    function paint(now) {
      // Keep the elements in queue order; only move them when the order changed.
      const live = [...host.children].filter((el) => !el.classList.contains('leaving'));
      if (live.length !== toasts.length || live.some((el, i) => el !== toasts[i].el)) toasts.forEach((t) => host.append(t.el));
      toasts.forEach((t, i) => {
        const el = t.el;
        el.classList.toggle('older', i > 0);
        el.hidden = i > 1;
        if (t.kind === 'pet') {
          el.querySelector('.t-bar').style.width = `${Math.max(0, Math.min(1, t.hideLeft / t.hideTotal)) * 100}%`;
          const more = el.querySelector('.t-more');
          more.hidden = !(i === 1 && toasts.length > 2);
          if (!more.hidden) more.textContent = `+${toasts.length - 2} more`;
          return;
        }
        const text = eventText(t, now);
        el.querySelector('.t-title').textContent = text.title;
        const msg = text.message;
        el.querySelector('.t-msg').textContent = msg ? (compact ? ` ${msg}` : ` — ${msg}`) : '';
        el.querySelector('.t-chip').textContent = chip(t, now);
        const bar = el.querySelector('.t-bar');
        const ratio = t.kind === 'imminent' && t.eventAt != null && now < t.eventAt
          ? (t.eventAt - now) / IMMINENT_MS
          : t.sticky ? 0 : t.hideLeft / t.hideTotal;
        bar.style.width = `${Math.max(0, Math.min(1, ratio)) * 100}%`;
        const more = el.querySelector('.t-more');
        const extra = toasts.length - 2;
        more.hidden = !(i === 1 && extra > 0);
        if (!more.hidden) more.textContent = `+${extra} more`;
      });
      host.hidden = !toasts.length;
      if (onChange) onChange(toasts.length);
    }

    function dismiss(t) {
      toasts = toasts.filter((other) => other !== t);
      t.el.classList.add('leaving');
      setTimeout(() => t.el.remove(), LEAVE_MS);
      paint(Date.now());
    }

    /**
     * Show a toast at the bottom of the queue (toasts arriving together are
     * sorted shortest first). One with the same key replaces the old one in place.
     */
    function show(spec) {
      const now = Date.now();
      if (spec.kind === 'pet' && updatePet(spec, now)) return null;
      if (spec.iconOnly) return null;
      const hideTotal = Number(spec.hideMs) > 0 ? Number(spec.hideMs) : HIDE_MS;
      const t = { ...spec, sticky: Boolean(spec.sticky), hovered: false, hideTotal, hideLeft: hideTotal };
      if (t.kind === 'pet') {
        t.selfTimed = true;
        t.gainedTotal = (t.pet && t.pet.gained) || 0;
        t.prevNeeded = null;
      }
      t.el = build(t);
      dress(t);
      if (t.kind === 'pet') {
        paintPet(t, false);
        if (!silent && styles.soundOn(t.styleNow, false)) chime(`pet:${now}`);
      }
      const res = queue.add(toasts, t, now);
      toasts = res.list;
      if (res.replaced) res.replaced.el.replaceWith(t.el);
      else host.append(t.el);
      paint(now);
      return t;
    }

    let last = Date.now();
    const timer = setInterval(() => {
      const now = Date.now();
      const step = now - last;
      last = now;
      // Only the counting toast loses time; see toast-queue.js for the rules.
      queue.tick(toasts, now, step).expired.forEach(dismiss);
      paint(now);
    }, TICK_MS);

    return {
      show,
      /** Take away the toast with `key` at once (another toast window shows it now). */
      drop(key) {
        const t = toasts.find((o) => o.key === key);
        if (t) dismiss(t);
      },
      /** Clear the toasts and stop (the Config preview, when it redraws). */
      destroy() {
        clearInterval(timer);
        toasts = [];
        host.remove();
      },
      setPosition(position) {
        host.classList.toggle('top', position !== 'bottom');
        host.classList.toggle('bottom', position === 'bottom');
      },
      set onSnooze(fn) { onSnooze = fn; },
    };
  }

  /** Sends toasts to the app's screen toast window, which handles snooze. */
  function screenHost() {
    return {
      show: (spec) => { bridge.showToast(spec).catch(() => {}); },
      setPosition() {},
      set onSnooze(_) {},
    };
  }

  /** In the app the screen window; in a browser a host inside this page. */
  const hostFor = (options) => (bridge.inApp ? screenHost() : createHost(options));

  /** How long an alert's toasts stay: `{ hideMs }`, or `{ sticky }` for until dismissed. */
  function durationOf(alert) {
    const secs = alert.duration_secs;
    if (secs == null) return { hideMs: HIDE_MS };
    return Number(secs) === 0 ? { sticky: true } : { hideMs: Number(secs) * 1000 };
  }

  /** The toast for one due stage of an alert. */
  function toastFor(item) {
    const { alert, stage, eventAt } = item;
    const key = `${alert.id}@${eventAt}`;
    const message = alert.message || '';
    const stay = durationOf(alert);
    const names = { event: alert.name, note: message };
    if (stage === 'live') return { key, kind: 'live', title: `${alert.name} is live now`, message, eventAt, ...names, ...stay };
    if (stage === 'imminent') {
      return { key, kind: 'imminent', title: 'Starting in {left}', message: message ? `Get ready — ${alert.name}: ${message}` : `Get ready — ${alert.name}`, eventAt, ...names, ...stay };
    }
    return { key, kind: 'upcoming', title: alert.name, message: message || 'starts in {left}', eventAt, snooze: true, ...names, ...stay };
  }

  /** Sample toasts for the Preview button. The names are placeholders. */
  function preview(host) {
    const now = Date.now();
    host.show(sample('upcoming', now));
    setTimeout(() => host.show(sample('imminent', Date.now())), 1200);
  }

  /** A sample toast of `kind` (Config preview and Send test toast). The names are placeholders. */
  function sample(kind, now = Date.now(), extra = {}) {
    if (kind === 'pet') {
      return { key: 'pet:test', kind: 'pet', hideMs: 60000, ...extra,
        pet: { id: 0, name: 'Sample pet', family: 'Fera', level: 0, max_level: 5, souls: 3, needed: 5, gained: 1, stepped: false } };
    }
    const base = { event: 'World Boss', note: 'Sample alert', ...extra };
    if (kind === 'live') return { key: 'preview-live', kind, title: 'World Boss is live now', message: 'Sample alert', eventAt: now, ...base };
    if (kind === 'imminent') return { key: 'preview-imminent', kind, title: 'Starting in {left}', message: 'Get ready — World Boss: Sample alert', eventAt: now + IMMINENT_MS, ...base };
    return { key: 'preview-upcoming', kind: 'upcoming', title: 'World Boss', message: 'Sample alert, spawns in {left}', eventAt: now + 5 * MINUTE, snooze: true, ...base };
  }

  /**
   * Watch the saved alerts and show their toasts on `host`. Returns
   * `{ settings(), reload(), preview() }`.
   */
  function startAlerts(host) {
    let settings = null;
    let lastTick = Date.now();

    async function reload() {
      try {
        settings = await bridge.getAlerts();
        host.setPosition(settings.position);
      } catch (_) { /* server not up yet; the next change or focus retries */ }
      return settings;
    }

    function fire(spec) {
      const style = styles.styleFor(spec);
      if (!style.enabled) return;
      host.show(spec);
      if (styles.soundOn(style, settings && settings.sound)) chime(`${spec.key}:${spec.kind}`);
    }

    host.onSnooze = (t) => {
      const at = Date.now() + SNOOZE_MS;
      if (t.eventAt == null || t.eventAt - at <= IMMINENT_MS) return;
      const { key, kind, title, message, event, note, eventAt, snooze, hideMs, sticky } = t;
      setTimeout(() => host.show({ key, kind, title, message, event, note, eventAt, snooze, hideMs, sticky }), SNOOZE_MS);
    };

    setInterval(() => {
      const now = Date.now();
      const from = Math.max(lastTick, now - CATCH_UP_MS);
      lastTick = now;
      if (settings) due(settings, from, now).forEach((item) => fire(toastFor(item)));
    }, 1000);

    bridge.onAlertsChanged(reload);
    reload();
    return {
      settings: () => settings,
      reload,
      preview: () => {
        preview(host);
        if (styles.soundOn(styles.styleFor({ kind: 'upcoming' }), settings && settings.sound)) chime(`preview:${Date.now()}`);
      },
      sound: () => Boolean(settings && settings.sound),
    };
  }

  window.notify = { createHost, screenHost, hostFor, startAlerts, toastFor, durationOf, sample, HIDE_MS, SNOOZE_MS };
})();
