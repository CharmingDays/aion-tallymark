// Shared, pure alert presentation logic; safe to exercise without the game.
(function (root) {
  const TTL = { detected: 12000, gone: 6000 };
  const key = (a) => `${a.ts_ms}:${a.sequence}`;
  function select(alerts, dismissed, now) {
    // A removal supersedes the corresponding detection, even if dismissed.
    const latest = new Map();
    for (const a of alerts || []) {
      if (!(a.status in TTL) || !Number.isFinite(a.ts_ms)) continue;
      if (!latest.has(a.entity_id) || latest.get(a.entity_id).sequence < a.sequence) latest.set(a.entity_id, a);
    }
    return [...latest.values()].filter((a) => !dismissed.has(key(a)) && now >= a.ts_ms && now - a.ts_ms < TTL[a.status])
      .sort((a, b) => b.sequence - a.sequence)[0] || null;
  }
  function text(a) {
    const type = a.cube_type || 'Hidden Cube';
    if (a.status === 'gone') return { title: 'Cube no longer nearby', subline: `${type} \u00b7 Out of range` };
    const distance = Number.isFinite(a.distance_m) && a.distance_m >= 0 && /^(N|NE|E|SE|S|SW|W|NW)$/.test(a.direction || '')
      ? ` \u00b7 ${Math.round(a.distance_m)}m ${a.direction}` : '';
    return { title: 'Hidden Cube nearby', subline: `${type}${distance}` };
  }
  function create(element, timers = { set: setTimeout, clear: clearTimeout }) {
    const dismissed = new Set();
    let shown = null;
    let expiry = null;
    element.querySelector('button').addEventListener('click', (event) => {
      event.stopPropagation();
      timers.clear(expiry);
      if (shown) dismissed.add(key(shown));
      if (dismissed.size > 64) dismissed.delete(dismissed.values().next().value);
      element.hidden = true;
    });
    return {
      update(data, now = Date.now()) {
        timers.clear(expiry);
        shown = data.settings && data.settings.hidden_cube_alerts === false ? null : select(data.cube_alerts, dismissed, now);
        element.hidden = !shown;
        if (!shown) return;
        const currentKey = key(shown);
        expiry = timers.set(() => {
          if (shown && key(shown) === currentKey) element.hidden = true;
        }, Math.max(0, TTL[shown.status] - (now - shown.ts_ms)));
        if (expiry && expiry.unref) expiry.unref(); // Node tests must not keep a timer alive.
        const label = text(shown);
        element.classList.toggle('gone', shown.status === 'gone');
        element.querySelector('[data-cube-title]').textContent = label.title;
        element.querySelector('[data-cube-subline]').textContent = label.subline;
      },
      hit(x, y) {
        if (element.hidden) return false;
        const r = element.querySelector('button').getBoundingClientRect();
        return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
      },
    };
  }
  const api = { select, text, key, create };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.cubeAlert = api;
})(typeof window !== 'undefined' ? window : globalThis);
