// Stagger in a saved session, for the history replay. No DOM, so it can be
// tested. `session.stagger` holds `[t_ms, actor index, points, skill id]`
// shares of each gauge drop (estimates) and `session.stagger_breaks` the
// break times in ms. Both are missing in files saved before stagger tracking.
(function (root) {
  'use strict';

  function list(session, key) {
    return session && Array.isArray(session[key]) ? session[key] : [];
  }

  /** Whether the session has any stagger data to show. */
  function has(session) {
    return list(session, 'stagger').length > 0 || list(session, 'stagger_breaks').length > 0;
  }

  /** Running stagger totals up to `t`, per actor and per (actor, skill).
   * Starts over when `t` goes back. */
  function advance(acc, shares, actors, t) {
    if (!acc || t < acc.t || acc.actors !== actors) {
      acc = {
        t: 0, cursor: 0, total: 0, actors,
        actor: new Float64Array(actors),
        skills: Array.from({ length: actors }, () => new Map()),
      };
    }
    const rows = Array.isArray(shares) ? shares : [];
    while (acc.cursor < rows.length && rows[acc.cursor][0] <= t) {
      const [, actor, points, skill] = rows[acc.cursor++];
      if (!(actor >= 0 && actor < actors) || !(points > 0)) continue;
      acc.actor[actor] += points;
      acc.skills[actor].set(skill, (acc.skills[actor].get(skill) || 0) + points);
      acc.total += points;
    }
    acc.t = t;
    return acc;
  }

  /** Break times at or before `t`. */
  function breaksUntil(breaks, t) {
    return (Array.isArray(breaks) ? breaks : []).filter((ms) => ms <= t);
  }

  /** "2 at 0:41, 1:48", "None yet". */
  function breaksText(breaks, clock) {
    if (!breaks.length) return 'None yet';
    return `${breaks.length} at ${breaks.map((ms) => clock(ms)).join(', ')}`;
  }

  /** "~1,098" for an estimate, "" for none. */
  function points(value, full) {
    if (!(value > 0)) return '';
    return `~${full(Math.round(value))}`;
  }

  /** Per-player rows { i, name, points }, by name: most stagger first, then A-Z.
   * Players with no stagger are left out. */
  function byPlayer(acc, names) {
    if (!acc) return [];
    return (names || [])
      .map((name, i) => ({ i, name, points: acc.actor[i] || 0 }))
      .filter((p) => p.points > 0)
      .sort((a, b) => b.points - a.points || a.name.localeCompare(b.name));
  }

  /** Per-actor skill maps shaped for abilityShare.merge (points as `damage`). */
  function skillMaps(acc) {
    if (!acc) return [];
    return acc.skills.map((m) => new Map([...m].map(([skill, pts]) => [skill, { skill, dot: false, damage: pts, hits: 0, crits: 0 }])));
  }

  const api = { has, advance, breaksUntil, breaksText, points, byPlayer, skillMaps };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.staggerHistory = api;
})(typeof window !== 'undefined' ? window : globalThis);
