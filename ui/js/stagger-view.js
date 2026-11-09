// Stagger gauge for the detailed meter. No DOM, so it can be tested.
//
// The snapshot carries `stagger: { max, remaining, fill, frozen, broken,
// breaks, break_ms, estimated, unattributed }` for the current target when the
// server sent its gauge (`05 E0`). Per-player and per-skill points are
// estimates: the server sends only the gauge, and each drop is shared out over
// the hits that landed since the previous update.
(function (root) {
  /** Whether there is anything to show. */
  function has(stagger) {
    return !!stagger && (stagger.max > 0 || stagger.breaks > 0);
  }

  /** Bar width in percent, 0 to 100. A broken gauge reads full. */
  function fillPct(stagger) {
    if (!stagger || !(stagger.max > 0)) return stagger && stagger.broken ? 100 : 0;
    if (stagger.broken) return 100;
    const pct = (stagger.fill / stagger.max) * 100;
    return Math.max(0, Math.min(100, Math.round(pct * 10) / 10));
  }

  /** "Broken", "Frozen", or "". */
  function stateText(stagger) {
    if (!stagger) return '';
    if (stagger.broken) return 'Broken';
    if (stagger.frozen) return 'Frozen';
    return '';
  }

  /** "640 / 1,200". */
  function gaugeText(stagger, full) {
    if (!stagger || !(stagger.max > 0)) return '';
    const fill = stagger.broken ? stagger.max : stagger.fill;
    return `${full(fill)} / ${full(stagger.max)}`;
  }

  /** "~1,098" for an estimate, "" for none. */
  function points(value, full) {
    if (!(value > 0)) return '';
    return `~${full(Math.round(value))}`;
  }

  /** "Breaks at 0:41, 1:48" from ms since the fight start. */
  function breaksTitle(stagger, clock) {
    if (!stagger || !stagger.breaks) return 'No stagger break yet';
    const at = (stagger.break_ms || []).map((ms) => clock(ms));
    return at.length ? `Breaks at ${at.join(', ')}` : `${stagger.breaks} breaks`;
  }

  const api = { has, fillPct, stateText, gaugeText, points, breaksTitle };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.staggerView = api;
})(typeof window !== 'undefined' ? window : globalThis);
