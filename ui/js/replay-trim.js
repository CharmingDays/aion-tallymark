// Trims the idle stretch before the first saved hit so a replay starts about
// LEAD_MS before the action. A session's started_at is the first hit on any
// target, but only hits on the counted targets are saved, so the first saved
// hit can come seconds later. The file is never changed; playback uses a copy.
(function (root) {
  'use strict';

  const LEAD_MS = 1500;
  const TAIL_MS = 1500;

  /** Returns { session, offset, span }: a shallow copy whose event and heal
   * times are shifted by -offset, and the replay length measured from the new 0:00.
   * Heals before the new 0:00 are kept at 0:00 so the totals stay whole. */
  function trim(session, lead = LEAD_MS, tail = TAIL_MS) {
    const events = Array.isArray(session.events) ? session.events : [];
    const recorded = Math.max(0, (session.ended_at || 0) - (session.started_at || 0));
    if (!events.length) return { session, offset: 0, span: Math.max(1, recorded) };
    const first = events[0][0];
    const last = events[events.length - 1][0];
    const offset = Math.max(0, first - lead);
    const end = Math.min(Math.max(recorded, last), last + tail);
    const shifted = offset
      ? events.map((e) => { const c = e.slice(); c[0] = e[0] - offset; return c; })
      : events;
    const heals = Array.isArray(session.heals) ? session.heals : [];
    const healsShifted = offset && heals.length
      ? heals.map((h) => { const c = h.slice(); c[0] = Math.max(0, h[0] - offset); return c; })
      : heals;
    // Stagger shares and break times move with the hits (older files have none).
    const shift = (t) => Math.max(0, t - offset);
    const stagger = Array.isArray(session.stagger) ? session.stagger : null;
    const breaks = Array.isArray(session.stagger_breaks) ? session.stagger_breaks : null;
    const extra = {};
    if (stagger) extra.stagger = offset ? stagger.map((r) => { const c = r.slice(); c[0] = shift(r[0]); return c; }) : stagger;
    if (breaks) extra.stagger_breaks = offset ? breaks.map(shift) : breaks;
    return {
      session: Object.assign({}, session, { events: shifted, heals: healsShifted }, extra),
      offset,
      span: Math.max(1, end - offset),
    };
  }

  const api = { LEAD_MS, TAIL_MS, trim };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.replayTrim = api;
})(typeof window !== 'undefined' ? window : globalThis);