// Pet tracker rows: status, filters and sorting for the Pets tab. No DOM, so
// it can be tested.
(function (root) {
  'use strict';

  const FAMILIES = ['Cogni', 'Fera', 'Natura', 'Varian', 'Special'];

  /** Souls still missing for the next step, or null at the top level. */
  function remaining(row) {
    return row.needed == null ? null : Math.max(0, row.needed - row.souls);
  }

  /** 'max', 'summoned', 'locked' (not summoned) or 'unknown' before any pet list. */
  function status(row, hasList) {
    if (row.needed == null) return 'max';
    if (row.level > 0) return 'summoned';
    return hasList || row.seen ? 'locked' : 'unknown';
  }

  function statusText(row, hasList) {
    switch (status(row, hasList)) {
      case 'max': return `Level ${row.level} (max)`;
      case 'summoned': return `Level ${row.level}`;
      case 'locked': return 'Not summoned';
      default: return 'Unknown';
    }
  }

  /** "3 / 5" for the next step; "Max" at the top. */
  function progressText(row) {
    return row.needed == null ? 'Max' : `${row.souls} / ${row.needed}`;
  }

  function progressShare(row) {
    if (row.needed == null) return 1;
    return row.needed > 0 ? Math.min(1, row.souls / row.needed) : 0;
  }

  /** filter: { query, family ('' = all), show ('' | 'locked' | 'summoned' | 'max' | 'progress') } */
  function filterRows(rows, filter, hasList) {
    const q = String(filter.query || '').trim().toLowerCase();
    return rows.filter((r) => {
      if (filter.family && r.family !== filter.family) return false;
      if (q && !r.name.toLowerCase().includes(q) && !r.family.toLowerCase().includes(q)
        && !String(r.source || '').toLowerCase().includes(q) && String(r.id) !== q) return false;
      const s = status(r, hasList);
      switch (filter.show) {
        case 'locked': return r.level === 0;
        case 'summoned': return r.level > 0;
        case 'max': return s === 'max';
        case 'progress': return r.souls > 0 && s !== 'max';
        default: return true;
      }
    });
  }

  const byName = (a, b) => a.name.localeCompare(b.name) || a.id - b.id;

  /** sort: 'close' (closest to the next unlock), 'name', 'family', 'level'. */
  function sortRows(rows, sort) {
    const list = rows.slice();
    if (sort === 'name') return list.sort(byName);
    if (sort === 'family') return list.sort((a, b) => a.family.localeCompare(b.family) || byName(a, b));
    if (sort === 'level') return list.sort((a, b) => b.level - a.level || b.souls - a.souls || byName(a, b));
    // Closest first: fewest souls missing; top-level pets last.
    return list.sort((a, b) => {
      const ra = remaining(a);
      const rb = remaining(b);
      if ((ra == null) !== (rb == null)) return ra == null ? 1 : -1;
      return (ra || 0) - (rb || 0) || progressShare(b) - progressShare(a) || a.level - b.level || byName(a, b);
    });
  }

  const api = { FAMILIES, remaining, status, statusText, progressText, progressShare, filterRows, sortRows };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.petList = api;
})(typeof window !== 'undefined' ? window : globalThis);
