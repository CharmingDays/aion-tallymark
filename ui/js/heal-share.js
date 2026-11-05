// Healing helpers shared by the overlay, the detailed meter and the replay.
// Pure functions only, so they can be tested with node.
(function (root) {
  'use strict';

  const VIEW_DAMAGE = 'damage';
  const VIEW_HEAL = 'heal';
  /** HoT flag bit in a saved heal `[t_ms, actor, amount, flags, skill]`. */
  const FLAG_HOT = 2;
  /** The meter's heal feed shows at most this many rows. */
  const FEED_ROWS = 50;

  /** A stored view name, falling back to Damage. */
  const view = (v) => (v === VIEW_HEAL ? VIEW_HEAL : VIEW_DAMAGE);

  /** Live healers with healing, largest first, each with `width` (% of the top healer). */
  function healerRows(healers) {
    const rows = (Array.isArray(healers) ? healers : []).filter((h) => h && h.heal > 0)
      .sort((a, b) => b.heal - a.heal || String(a.name).localeCompare(String(b.name)));
    const top = rows.length ? rows[0].heal : 1;
    return rows.map((h) => Object.assign({}, h, { width: Math.round((h.heal / top) * 1000) / 10 }));
  }

  /** The small "+0.42M" after a name in Damage view; empty when the player did not heal. */
  const badge = (heal, short) => (heal > 0 ? `+${short(heal)}` : '');

  /** Heal rows of a live feed, newest first as sent, optionally one healer's only. */
  function feed(recent, healer, limit = FEED_ROWS) {
    return (Array.isArray(recent) ? recent : [])
      .filter((r) => r.heal && (healer == null || r.actor_id === healer))
      .slice(0, limit);
  }

  /** Whether a saved session has heal data (schema 2 with at least one heal). */
  const hasHeals = (session) => Boolean(session && Array.isArray(session.heals) && session.heals.length);

  /**
   * Running heal totals up to `t` over `heals` (sorted by time). Pass the
   * previous state to continue from it; a smaller `t` starts over.
   * `skills` is per actor a Map in the shape ability-share.js merges
   * ({ skill, dot, damage, hits, crits }, with damage = healed, dot = HoT).
   */
  function advance(state, heals, actors, t) {
    let s = state;
    if (!s || t < s.t) {
      s = { t: 0, cursor: 0, total: 0, heal: new Float64Array(actors), skills: Array.from({ length: actors }, () => new Map()) };
    }
    const list = Array.isArray(heals) ? heals : [];
    while (s.cursor < list.length && list[s.cursor][0] <= t) {
      const [, actor, amount, flags, skill] = list[s.cursor++];
      if (actor >= actors) continue;
      const hot = Boolean(flags & FLAG_HOT);
      const key = skill * 2 + (hot ? 1 : 0);
      let row = s.skills[actor].get(key);
      if (!row) s.skills[actor].set(key, (row = { skill, dot: hot, damage: 0, hits: 0, crits: 0 }));
      row.damage += amount;
      row.hits += 1;
      s.heal[actor] += amount;
      s.total += amount;
    }
    s.t = t;
    return s;
  }

  /** Healing per time bucket as a per-second rate, for the chart's second series. */
  function rates(heals, span, buckets) {
    const n = Math.max(1, buckets);
    const size = Math.max(1, span) / n;
    const sums = new Float64Array(n);
    for (const h of Array.isArray(heals) ? heals : []) sums[Math.min(n - 1, Math.max(0, Math.floor(h[0] / size)))] += h[2];
    return Array.from(sums, (v) => (v * 1000) / size);
  }

  const api = { VIEW_DAMAGE, VIEW_HEAL, FLAG_HOT, FEED_ROWS, view, healerRows, badge, feed, hasHeals, advance, rates };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.healShare = api;
})(typeof window !== 'undefined' ? window : globalThis);
