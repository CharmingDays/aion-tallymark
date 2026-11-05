// Groups the Ability share tab's rows. A skill's DoT ticks and direct hits
// are one row, and variants of one skill (ids in the same 10000 block whose
// name, up to " - ", is the same, e.g. "Dimensional Control" and
// "Dimensional Control - Delayed Damage") fold into the plain skill's row.
// Different skills that share an id block (spirit attacks) stay apart.
(function (root) {
  'use strict';

  /** Rows past this many fold into one "Other (N)" row. */
  const SHARE_ROWS = 12;

  const stem = (name) => {
    const i = name.indexOf(' - ');
    return i > 0 ? name.slice(0, i) : name;
  };

  /** The row a skill id belongs to: block + name stem, or the id when unnamed. */
  function groupKey(skill, names) {
    const name = names && names[skill];
    return name ? `${Math.floor(skill / 10000)}|${stem(name)}` : `#${skill}`;
  }

  /**
   * skillMaps: per actor, a Map of { skill, dot, damage, hits, crits };
   * actors: the actor indexes to include. Returns rows with
   * { key, skill, name, damage, hits, crits, dotDamage, variants, by: [[actor, damage]] }.
   */
  function merge(skillMaps, actors, names) {
    const rows = new Map();
    for (const a of actors) {
      const m = skillMaps[a];
      if (!m) continue;
      for (const r of m.values()) {
        const key = groupKey(r.skill, names);
        let g = rows.get(key);
        if (!g) {
          const n = names && names[r.skill];
          rows.set(key, (g = { key, skill: r.skill, name: n ? stem(n) : null, damage: 0, hits: 0, crits: 0, dotDamage: 0, ids: new Set(), per: new Map() }));
        }
        if (r.skill < g.skill) g.skill = r.skill;
        g.ids.add(r.skill);
        g.damage += r.damage;
        g.hits += r.hits;
        g.crits += r.crits;
        if (r.dot) g.dotDamage += r.damage;
        g.per.set(a, (g.per.get(a) || 0) + r.damage);
      }
    }
    return [...rows.values()].map((g) => ({
      key: g.key, skill: g.skill, name: g.name, damage: g.damage, hits: g.hits, crits: g.crits,
      dotDamage: g.dotDamage, variants: g.ids.size, by: [...g.per],
    }));
  }

  const critRate = (r) => (r.hits ? r.crits / r.hits : 0);
  const SORTS = {
    damage: (a, b) => b.damage - a.damage,
    hits: (a, b) => b.hits - a.hits || b.damage - a.damage,
    crit: (a, b) => critRate(b) - critRate(a) || b.damage - a.damage,
  };

  /** Sorted rows with damage, split into the shown ones and the rest ("Other"). */
  function split(rows, sort = 'damage', cutoff = SHARE_ROWS) {
    const all = rows.filter((r) => r.damage > 0).sort(SORTS[sort] || SORTS.damage);
    const n = all.length > cutoff + 1 ? cutoff : all.length;
    return { shown: all.slice(0, n), rest: all.slice(n) };
  }

  const api = { SHARE_ROWS, stem, groupKey, merge, split };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.abilityShare = api;
})(typeof window !== 'undefined' ? window : globalThis);
