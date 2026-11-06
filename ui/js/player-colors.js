// Player colours for the session history replay: one distinct colour per
// player name, with the local player ("you") on their own chosen colour.
(function (root) {
  'use strict';

  const YOU_DEFAULT = '#4ade80';
  /** Kept as history.youColor for backward compatibility. */
  const YOU_KEY = 'aion2dps.history.youColor';
  const PALETTE = ['#60a5fa', '#f472b6', '#facc15', '#a78bfa', '#fb923c', '#22d3ee', '#f87171', '#2dd4bf', '#e879f9', '#fbbf24', '#93c5fd', '#fda4af'];
  const PRESETS = ['#4ade80', '#60a5fa', '#f472b6', '#facc15', '#a78bfa', '#fb923c', '#22d3ee', '#f87171'];
  const UNKNOWN_COLOR = '#6b7280';

  /** A small stable string hash (FNV-1a). */
  function hash(s) {
    let h = 0x811c9dc5;
    for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193) >>> 0;
    return h;
  }

  const isHex = (c) => typeof c === 'string' && /^#[0-9a-f]{6}$/i.test(c);

  function readYouColor() {
    try {
      const c = localStorage.getItem(YOU_KEY);
      return isHex(c) ? c.toLowerCase() : YOU_DEFAULT;
    } catch (_) {
      return YOU_DEFAULT;
    }
  }

  function writeYouColor(c) {
    if (!isHex(c)) return readYouColor();
    const next = c.toLowerCase();
    try { localStorage.setItem(YOU_KEY, next); } catch (_) { /* private mode */ }
    return next;
  }

  /**
   * Colours for `actors` (names, by index). Each name starts at a hashed
   * palette slot, so the same name keeps its colour; names are placed in
   * sorted order and step past taken slots so nobody shares a colour until
   * the palette runs out. `you` is the local player's index or null.
   */
  function colorsFor(actors, you, youColor) {
    const mine = isHex(youColor) ? youColor.toLowerCase() : YOU_DEFAULT;
    const free = PALETTE.filter((c) => c !== mine);
    const out = new Array(actors.length);
    const taken = new Set();
    const order = actors.map((name, i) => ({ name: String(name), i })).sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : a.i - b.i));
    for (const { name, i } of order) {
      if (i === you) { out[i] = mine; continue; }
      if (name === 'Unknown') { out[i] = UNKNOWN_COLOR; continue; }
      let slot = hash(name) % free.length;
      if (taken.size < free.length) while (taken.has(slot)) slot = (slot + 1) % free.length;
      taken.add(slot);
      out[i] = free[slot];
    }
    return out;
  }

  /**
   * Split a share bar of `width` percent into per-player segments, each
   * proportional to that player's damage. `parts` is `[actor, damage]`
   * pairs; segments come back largest first with `width` and `pct` (of the
   * ability's damage), and their widths sum to `width`.
   */
  function segments(parts, width) {
    const rows = parts.filter(([, d]) => d > 0).sort((a, b) => b[1] - a[1] || a[0] - b[0]);
    const sum = rows.reduce((s, [, d]) => s + d, 0);
    if (sum <= 0) return [];
    return rows.map(([actor, damage]) => ({ actor, damage, pct: (damage / sum) * 100, width: (damage / sum) * width }));
  }

  const api = { YOU_DEFAULT, YOU_KEY, PALETTE, PRESETS, UNKNOWN_COLOR, isHex, readYouColor, writeYouColor, colorsFor, segments, hash };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.playerColors = api;
})(typeof window !== 'undefined' ? window : globalThis);
