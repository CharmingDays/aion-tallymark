// When event alerts fire. Pure functions, shared by the toast scheduler in
// notify.js, the alert editor and the node tests in tests/js. An alert is
// `{ id, name, message, time: "HH:MM", start_date?: "YYYY-MM-DD", repeat,
// lead_mins?, duration_secs?, enabled }` in local time. The first occurrence
// is `start_date` at `time`, and `repeat` is one of
// `{type:'none'}`, `{type:'minutes', every}`, `{type:'hours', every}`,
// `{type:'daily', every}` (days, at `time`), `{type:'weekly', days}` (0 = Mon,
// at `time`) or `{type:'hours_minutes', hours, minutes}`.
// Older alerts have no `repeat`: `every_mins` meant every that many minutes
// in step with `time`, nothing meant daily. Without `start_date` the steps
// keep that old meaning: in step with `time` on any day.
// `clock` says which clock `time` and `start_date` are in: missing or
// 'local' is this PC, 'server' is the game server at the settings'
// `server_utc_offset_mins` (default UTC+9), 'utc' is UTC. Times returned are
// always real instants (ms), so the pages show them in local time.
(function (root) {
  const MINUTE = 60_000;
  const DAY = 24 * 60 * MINUTE;
  /** The amber "starting in 0:30" toast. */
  const IMMINENT_MS = 30_000;

  /** "HH:MM" as minutes after midnight, or null. */
  function parseTime(text) {
    const m = /^(\d{1,2}):(\d{2})$/.exec(String(text || '').trim());
    if (!m) return null;
    const h = Number(m[1]);
    const min = Number(m[2]);
    return h < 24 && min < 60 ? h * 60 + min : null;
  }

  // Calendar math in this PC's zone, or in UTC for fixed-offset clocks
  // (the time is shifted by the offset first, then shifted back).
  const LOCAL = {
    atDay(ms, minutes) { const d = new Date(ms); d.setHours(0, 0, 0, 0); d.setMinutes(minutes); return d.getTime(); },
    onDay: ([y, mo, d], days, minutes) => new Date(y, mo, d + days, Math.floor(minutes / 60), minutes % 60).getTime(),
    dayOf: (ms) => { const t = new Date(ms); return [t.getFullYear(), t.getMonth(), t.getDate()]; },
    weekday: (ms) => (new Date(ms).getDay() + 6) % 7,
  };
  const UTC = {
    atDay: (ms, minutes) => Math.floor(ms / DAY) * DAY + minutes * MINUTE,
    onDay: ([y, mo, d], days, minutes) => Date.UTC(y, mo, d + days, Math.floor(minutes / 60), minutes % 60),
    dayOf: (ms) => { const t = new Date(ms); return [t.getUTCFullYear(), t.getUTCMonth(), t.getUTCDate()]; },
    weekday: (ms) => (new Date(ms).getUTCDay() + 6) % 7,
  };
  const DEFAULT_SERVER_OFFSET_MINS = 540;

  /** Minutes east of UTC the alert's clock runs at, or null for local time. */
  function offsetOf(alert, settings) {
    if (alert.clock === 'utc') return 0;
    if (alert.clock === 'server') {
      const off = settings && Number.isFinite(Number(settings.server_utc_offset_mins)) ? Number(settings.server_utc_offset_mins) : DEFAULT_SERVER_OFFSET_MINS;
      return off;
    }
    return null;
  }

  /** "UTC+9", "UTC-5:30", "UTC". */
  function offsetLabel(mins) {
    const m = Number(mins) || 0;
    if (!m) return 'UTC';
    const a = Math.abs(m);
    return `UTC${m < 0 ? '-' : '+'}${Math.floor(a / 60)}${a % 60 ? `:${String(a % 60).padStart(2, '0')}` : ''}`;
  }

  const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  /** "YYYY-MM-DD" as [y, monthIndex, d], or null. */
  function parseDate(text) {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(text || '').trim());
    if (!m) return null;
    const [y, mo, d] = [Number(m[1]), Number(m[2]) - 1, Number(m[3])];
    const check = new Date(y, mo, d);
    return check.getFullYear() === y && check.getMonth() === mo && check.getDate() === d ? [y, mo, d] : null;
  }

  /** The alert's repeat, with older alerts read the old way. */
  function repeatOf(alert) {
    if (alert.repeat && alert.repeat.type) return alert.repeat;
    const every = Number(alert.every_mins) || 0;
    return every > 0 ? { type: 'minutes', every } : { type: 'daily', every: 1 };
  }

  /** The step in minutes for the fixed-step repeats, else 0. */
  function stepMins(r) {
    if (r.type === 'minutes') return Number(r.every) || 0;
    if (r.type === 'hours') return (Number(r.every) || 0) * 60;
    if (r.type === 'hours_minutes') return (Number(r.hours) || 0) * 60 + (Number(r.minutes) || 0);
    return 0;
  }

  /** Whole local days from `a` to `b`, both `[y, mo, d]`. */
  const daysBetween = (a, b) => Math.round((Date.UTC(b[0], b[1], b[2]) - Date.UTC(a[0], a[1], a[2])) / DAY);

  /**
   * The first time the event happens strictly after `after` (ms), or null.
   * `settings` gives the server clock for `clock: 'server'` alerts.
   */
  function nextAt(alert, after, settings) {
    const off = offsetOf(alert, settings);
    if (off == null) return nextIn(LOCAL, alert, after);
    const shift = off * MINUTE;
    const at = nextIn(UTC, alert, after + shift);
    return at == null ? null : at - shift;
  }

  function nextIn(cal, alert, after) {
    const { atDay, onDay, dayOf } = cal;
    const minutes = parseTime(alert.time);
    if (minutes == null) return null;
    const r = repeatOf(alert);
    const startDay = parseDate(alert.start_date);
    const start = startDay ? onDay(startDay, 0, minutes) : null;
    const step = stepMins(r);
    if (r.type === 'none') {
      const at = start != null ? start : atDay(after, minutes);
      return at > after ? at : null;
    }
    if (step > 0) {
      const base = start != null ? start : atDay(after, minutes);
      if (start != null && after < start) return start;
      const ms = step * MINUTE;
      return base + (Math.floor((after - base) / ms) + 1) * ms;
    }
    if (r.type === 'daily') {
      const every = Math.max(1, Number(r.every) || 1);
      const anchor = startDay || dayOf(after);
      if (start != null && after < start) return start;
      const diff = daysBetween(anchor, dayOf(after));
      let n = Math.floor(diff / every) * every;
      let at = onDay(anchor, n, minutes);
      while (at <= after) { n += every; at = onDay(anchor, n, minutes); }
      return at;
    }
    if (r.type === 'weekly') {
      const days = (r.days || []).map(Number);
      if (!days.length) return null;
      const from = start != null && after < start ? startDay : dayOf(after);
      for (let i = 0; i <= 7; i += 1) {
        const at = onDay(from, i, minutes);
        const weekday = cal.weekday(at);
        if (at > after && (start == null || at >= start) && days.includes(weekday)) return at;
      }
      return null;
    }
    return null;
  }

  const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
  const hm = (h, m) => [h ? `${h}h` : '', m ? `${m}m` : ''].filter(Boolean).join(' ');

  /** " (server)" or " (UTC)" after a time not in local time. */
  const clockNote = (alert) => (alert.clock === 'server' ? ' (server)' : alert.clock === 'utc' ? ' (UTC)' : '');

  /** Short repeat text for the event list: "Every 1h 30m", "Weekly on Sat 21:00 (server)". */
  function repeatShort(alert) {
    return repeatShortBare(alert) + clockNote(alert);
  }

  function repeatShortBare(alert) {
    const r = repeatOf(alert);
    const step = stepMins(r);
    if (r.type === 'none') return 'Does not repeat';
    if (r.type === 'minutes') return `Every ${r.every} min`;
    if (r.type === 'hours') return `Every ${plural(Number(r.every), 'hour', 'hours')}`;
    if (r.type === 'hours_minutes') return `Every ${hm(Math.floor(step / 60), step % 60)}`;
    if (r.type === 'daily') return Number(r.every) === 1 ? `Daily at ${alert.time}` : `Every ${r.every} days at ${alert.time}`;
    if (r.type === 'weekly') return `Weekly on ${(r.days || []).map((d) => WEEKDAYS[d]).join(', ')} ${alert.time}`;
    return '';
  }

  /** The line under the repeat control: "Repeats every 1h 30m". */
  function repeatSummary(alert) {
    const r = repeatOf(alert);
    const step = stepMins(r);
    if (r.type === 'none') return 'Fires once at the start time';
    if (r.type === 'daily') return Number(r.every) === 1 ? `Repeats every day at ${alert.time}` : `Repeats every ${r.every} days at ${alert.time}`;
    if (r.type === 'weekly') return `Repeats ${(r.days || []).map((d) => WEEKDAYS[d]).join(', ')} at ${alert.time}`;
    return `Repeats every ${hm(Math.floor(step / 60), step % 60)}`;
  }

  /** What is wrong with a repeat, or '' when it is fine. */
  function repeatError(r) {
    const whole = (v) => Number.isInteger(Number(v)) && String(v).trim() !== '';
    if (r.type === 'minutes' || r.type === 'hours' || r.type === 'daily') {
      return whole(r.every) && Number(r.every) >= 1 ? '' : 'Value must be 1 or more';
    }
    if (r.type === 'hours_minutes') {
      if (!whole(r.hours) || !whole(r.minutes) || r.hours < 0 || r.minutes < 0 || r.minutes > 59) return 'Use whole numbers, minutes 0 to 59';
      return r.hours * 60 + Number(r.minutes) >= 1 ? '' : 'Interval must be 1 minute or more';
    }
    if (r.type === 'weekly') return (r.days || []).length ? '' : 'Pick at least one day';
    return '';
  }

  /**
   * The toasts due in `(from, to]`: one per lead time, a 30 s "imminent" one
   * and a "live" one at the event. Each is `{ alert, stage, at, eventAt, lead }`
   * with stage `upcoming`, `imminent` or `live`, oldest first.
   */
  function due(settings, from, to) {
    if (!settings || !settings.enabled || !(to > from)) return [];
    const globalLeads = settings.lead_mins || [];
    const out = [];
    for (const alert of settings.alerts || []) {
      if (alert.enabled === false) continue;
      const leads = (Array.isArray(alert.lead_mins) ? alert.lead_mins : globalLeads).map(Number).filter((m) => m > 0);
      const reach = Math.max(IMMINENT_MS, ...leads.map((m) => m * MINUTE));
      let eventAt = nextAt(alert, from, settings);
      for (let guard = 0; eventAt != null && eventAt - reach <= to && guard < 50; guard += 1) {
        const stages = leads.map((lead) => ({ stage: 'upcoming', at: eventAt - lead * MINUTE, lead }));
        // A lead under 30 s would only repeat the imminent toast.
        stages.push({ stage: 'imminent', at: eventAt - IMMINENT_MS, lead: 0.5 });
        stages.push({ stage: 'live', at: eventAt, lead: 0 });
        for (const s of stages) if (s.at > from && s.at <= to) out.push({ alert, eventAt, ...s });
        eventAt = nextAt(alert, eventAt, settings);
      }
    }
    return out.sort((a, b) => a.at - b.at || b.lead - a.lead);
  }

  /** 5:00, 0:30, 1:02:00 */
  function countdown(ms) {
    const s = Math.max(0, Math.ceil(ms / 1000));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const ss = String(s % 60).padStart(2, '0');
    return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
  }

  const api = { MINUTE, IMMINENT_MS, WEEKDAYS, DEFAULT_SERVER_OFFSET_MINS, offsetOf, offsetLabel, parseTime, parseDate, repeatOf, nextAt, due, countdown, repeatShort, repeatSummary, repeatError };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.alertSchedule = api;
})(typeof window !== 'undefined' ? window : globalThis);
