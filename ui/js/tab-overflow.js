// Fits the session history's roster tabs on one line. Primary tabs always
// stay; others that do not fit move, from the end, into a "More" menu. No DOM,
// so it can be tested.
(function (root) {
  'use strict';

  /**
   * items: [{ id, width, primary, hidden }] in display order. available: px
   * for the tab bar; more: the More button's width; gap: px between tabs.
   * Returns { visible: [id], overflow: [id] }, both in display order.
   */
  function layout(items, available, more, gap = 0) {
    const shown = items.filter((t) => !t.hidden);
    const width = (list) => list.reduce((w, t, i) => w + t.width + (i ? gap : 0), 0);
    if (width(shown) <= available) return { visible: shown.map((t) => t.id), overflow: [] };
    const budget = available - more - gap;
    const keep = new Set(shown.filter((t) => t.primary).map((t) => t.id));
    let used = width(shown.filter((t) => keep.has(t.id)));
    for (const t of shown) {
      if (keep.has(t.id)) continue;
      const next = used + (keep.size ? gap : 0) + t.width;
      if (next > budget) break;
      keep.add(t.id);
      used = next;
    }
    return {
      visible: shown.filter((t) => keep.has(t.id)).map((t) => t.id),
      overflow: shown.filter((t) => !keep.has(t.id)).map((t) => t.id),
    };
  }

  /** The More button's text: the active tab's name when it sits in the menu. */
  function moreLabel(active, overflow, names) {
    return overflow.includes(active) ? (names[active] || 'More') : 'More';
  }

  /** Next menu item for arrow keys, wrapping around. */
  function step(index, count, delta) {
    if (count <= 0) return -1;
    return (((index + delta) % count) + count) % count;
  }

  const api = { layout, moreLabel, step };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.tabOverflow = api;
})(typeof window !== 'undefined' ? window : globalThis);
