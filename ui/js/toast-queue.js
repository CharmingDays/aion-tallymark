// The toast queue: order, turns and countdown, with no DOM, so it can be
// tested. notify.js keeps the elements and calls in here.
//
// Rules:
// - First come, first served: a new toast goes to the bottom of the queue.
// - Same time: toasts that arrive within BATCH_MS (250 ms) of the first toast
//   of a group, and before any of that group has started counting, are one
//   batch. A batch is sorted shortest display time first; "until dismissed"
//   (sticky) counts as the longest; ties keep arrival order. (Rust hands
//   toasts to the toast window one wake at a time, so "same JS call" alone
//   would miss alerts fired in the same scheduler pass.)
// - A toast with the same key as a queued one (the upcoming -> imminent -> live
//   stages of one event, or a snooze) replaces it in place: same spot in the
//   queue, fresh full display time. If it was on top it stays on top.
// - Only one toast counts down: the first *eligible* one among the two visible
//   (top and the dimmed one below). Sticky toasts and imminent toasts before
//   event start + 2 s are not eligible, so a sticky top lets the toast below it
//   run. If neither visible toast is eligible nothing counts; that is not a
//   deadlock: imminent ones become eligible 2 s after their event starts (or
//   are replaced by the live toast), and sticky ones wait for the close button,
//   which they always have (compact: click). Waiting toasts keep their full
//   time and bar.
// - Hover pauses the counting toast while the pointer is on it.
// - Self-timed toasts (pet soul progress) keep their own clock: they count
//   down whenever they are queued, whatever else is showing, and never take
//   the shared countdown turn. There is at most one pet toast: a soul for
//   another pet takes it over (petTarget).
(function (root) {
  const BATCH_MS = 250;
  const IMMINENT_HOLD_MS = 2000;

  const weight = (t) => (t.sticky ? Infinity : t.hideTotal);

  /** True when `t` may count down at `now`. */
  function eligible(t, now) {
    if (t.sticky || t.selfTimed) return false;
    if (t.kind === 'imminent' && t.eventAt != null && now < t.eventAt + IMMINENT_HOLD_MS) return false;
    return true;
  }

  /** Index of the toast counting down now, or -1. Only visible ones (0, 1). */
  function counter(list, now) {
    for (let i = 0; i < Math.min(2, list.length); i += 1) if (eligible(list[i], now)) return i;
    return -1;
  }

  /**
   * Add `t` (with hideTotal, hideLeft, key, kind, sticky, eventAt) at `now`.
   * Returns `{ list, replaced }`; `replaced` is the old toast of that key or null.
   */
  function add(list, t, now) {
    t.arrivedAt = now;
    t.started = false;
    const at = list.findIndex((o) => o.key === t.key);
    if (at >= 0) {
      const next = list.slice();
      const replaced = next[at];
      t.hovered = replaced.hovered;
      next[at] = t;
      return { list: next, replaced };
    }
    const next = list.concat([t]);
    // The batch: the trailing run of unstarted toasts arrived within BATCH_MS of its first.
    let start = next.length - 1;
    while (start > 0) {
      const o = next[start - 1];
      if (o.started || now - o.arrivedAt > BATCH_MS) break;
      start -= 1;
    }
    while (start < next.length - 1 && now - next[start].arrivedAt > BATCH_MS) start += 1;
    const batch = next.slice(start).map((o, i) => ({ o, i }));
    batch.sort((a, b) => (weight(a.o) - weight(b.o)) || (a.i - b.i));
    return { list: next.slice(0, start).concat(batch.map((b) => b.o)), replaced: null };
  }

  /** Advance `step` ms at `now`. Returns the toasts whose time ran out (already removed from the result). */
  function tick(list, now, step) {
    const expired = [];
    list.forEach((o) => {
      if (!o.selfTimed) return;
      o.started = true;
      if (!o.hovered) o.hideLeft -= step;
      if (o.hideLeft <= 0) expired.push(o);
    });
    const i = counter(list, now);
    if (i >= 0) {
      const t = list[i];
      t.started = true;
      if (!t.hovered) t.hideLeft -= step;
      if (t.hideLeft <= 0) expired.push(t);
    }
    if (!expired.length) return { list, expired };
    return { list: list.filter((o) => !expired.includes(o)), expired };
  }

  /** The pet toast a pet update goes to: the one showing (any pet), or null for a new one. */
  function petTarget(list, key) {
    return list.find((o) => o.kind === 'pet' && o.key === key) || list.find((o) => o.kind === 'pet') || null;
  }

  const api = { BATCH_MS, IMMINENT_HOLD_MS, eligible, counter, add, tick, petTarget };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.toastQueue = api;
})(typeof window !== 'undefined' ? window : globalThis);
