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

  const ICON_CLOSE = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>';

  /** A short two-note chime. Several windows may fire the same toast, so only one plays it. */
  let audio = null;
  function chime(key) {
    try {
      const last = localStorage.getItem('aion2dps-chime');
      if (last === key) return;
      localStorage.setItem('aion2dps-chime', key);
    } catch (_) { /* no storage: play anyway */ }
    try {
      audio = audio || new (window.AudioContext || window.webkitAudioContext)();
      if (audio.state === 'suspended') audio.resume();
      const t = audio.currentTime;
      [[880, 0], [1320, 0.12]].forEach(([freq, delay]) => {
        const osc = audio.createOscillator();
        const gain = audio.createGain();
        osc.frequency.value = freq;
        gain.gain.setValueAtTime(0.0001, t + delay);
        gain.gain.exponentialRampToValueAtTime(0.12, t + delay + 0.02);
        gain.gain.exponentialRampToValueAtTime(0.0001, t + delay + 0.18);
        osc.connect(gain).connect(audio.destination);
        osc.start(t + delay);
        osc.stop(t + delay + 0.2);
      });
    } catch (_) { /* no audio device */ }
  }

  function createHost({ compact = false, onChange = null } = {}) {
    const host = document.createElement('div');
    host.className = `notify-host top${compact ? ' compact' : ''}`;
    host.setAttribute('aria-live', 'polite');
    document.body.append(host);

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

    function build(t) {
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
        el.querySelector('.t-title').textContent = fill(t.title, t, now);
        const msg = fill(t.message, t, now);
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
      const hideTotal = Number(spec.hideMs) > 0 ? Number(spec.hideMs) : HIDE_MS;
      const t = { ...spec, sticky: Boolean(spec.sticky), hovered: false, hideTotal, hideLeft: hideTotal };
      t.el = build(t);
      const res = queue.add(toasts, t, now);
      toasts = res.list;
      if (res.replaced) res.replaced.el.replaceWith(t.el);
      else host.append(t.el);
      paint(now);
      return t;
    }

    let last = Date.now();
    setInterval(() => {
      const now = Date.now();
      const step = now - last;
      last = now;
      // Only the counting toast loses time; see toast-queue.js for the rules.
      queue.tick(toasts, now, step).expired.forEach(dismiss);
      paint(now);
    }, TICK_MS);

    return {
      show,
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
    if (stage === 'live') return { key, kind: 'live', title: `${alert.name} is live now`, message, eventAt, ...stay };
    if (stage === 'imminent') {
      return { key, kind: 'imminent', title: 'Starting in {left}', message: message ? `Get ready — ${alert.name}: ${message}` : `Get ready — ${alert.name}`, eventAt, ...stay };
    }
    return { key, kind: 'upcoming', title: alert.name, message: message || 'starts in {left}', eventAt, snooze: true, ...stay };
  }

  /** Sample toasts for the Preview button. The names are placeholders. */
  function preview(host) {
    const now = Date.now();
    host.show({ key: 'preview-upcoming', kind: 'upcoming', title: 'World Boss', message: 'Sample alert, spawns in {left}', eventAt: now + 5 * MINUTE, snooze: true });
    setTimeout(() => host.show({ key: 'preview-imminent', kind: 'imminent', title: 'Starting in {left}', message: 'Get ready — Sample event', eventAt: Date.now() + IMMINENT_MS }), 1200);
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
      host.show(spec);
      if (settings && settings.sound) chime(`${spec.key}:${spec.kind}`);
    }

    host.onSnooze = (t) => {
      const at = Date.now() + SNOOZE_MS;
      if (t.eventAt == null || t.eventAt - at <= IMMINENT_MS) return;
      const { key, kind, title, message, eventAt, snooze, hideMs, sticky } = t;
      setTimeout(() => host.show({ key, kind, title, message, eventAt, snooze, hideMs, sticky }), SNOOZE_MS);
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
        if (settings && settings.sound) chime(`preview:${Date.now()}`);
      },
    };
  }

  window.notify = { createHost, screenHost, hostFor, startAlerts, toastFor, durationOf, HIDE_MS, SNOOZE_MS };
})();
