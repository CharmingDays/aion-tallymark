// Run with: node --test tests/js/toast-queue.test.js
const test = require('node:test');
const assert = require('node:assert');
const q = require('../../ui/js/toast-queue.js');

const mk = (key, ms, extra = {}) => ({ key, kind: 'live', hideTotal: ms || 8000, hideLeft: ms || 8000, sticky: !ms, hovered: false, ...extra });

/** Run ticks of `step` from `from` to `to`; returns { list, gone: [[key, time]] }. */
function run(list, from, to, step = 250) {
  const gone = [];
  for (let now = from + step; now <= to; now += step) {
    const r = q.tick(list, now, step);
    list = r.list;
    r.expired.forEach((t) => gone.push([t.key, now]));
  }
  return { list, gone };
}

test('two 3 s toasts leave at about 3 s and 6 s', () => {
  let list = q.add([], mk('a', 3000), 0).list;
  list = q.add(list, mk('b', 3000), 0).list;
  const { gone } = run(list, 0, 7000);
  assert.deepStrictEqual(gone, [['a', 3000], ['b', 6000]]);
});

test('a waiting toast keeps its full time', () => {
  let list = q.add([], mk('a', 3000), 0).list;
  list = q.add(list, mk('b', 5000), 1000).list;
  ({ list } = run(list, 1000, 2500));
  assert.strictEqual(list[1].hideLeft, 5000);
  assert.ok(list[0].hideLeft < 3000);
});

test('a batch goes shortest first, sticky last, ties in arrival order', () => {
  let list = [];
  for (const [k, ms] of [['long', 10000], ['sticky', 0], ['short', 3000], ['mid', 5000], ['mid2', 5000]]) list = q.add(list, mk(k, ms), 100).list;
  assert.deepStrictEqual(list.map((t) => t.key), ['short', 'mid', 'mid2', 'long', 'sticky']);
});

test('toasts within 250 ms are one batch; later ones go to the bottom', () => {
  let list = q.add([], mk('a', 10000), 0).list;
  list = q.add(list, mk('b', 3000), 200).list;
  assert.deepStrictEqual(list.map((t) => t.key), ['b', 'a']);
  ({ list } = run(list, 200, 1000));
  list = q.add(list, mk('c', 1000), 1000).list;
  assert.deepStrictEqual(list.map((t) => t.key), ['b', 'a', 'c']);
});

test('a started toast is never re-sorted by a newcomer', () => {
  let list = q.add([], mk('a', 8000), 0).list;
  ({ list } = run(list, 0, 250));
  list = q.add(list, mk('b', 1000), 260).list;
  assert.deepStrictEqual(list.map((t) => t.key), ['a', 'b']);
});

test('same key replaces in place with full time and keeps its turn', () => {
  let list = q.add([], mk('ev', 4000, { kind: 'upcoming' }), 0).list;
  list = q.add(list, mk('other', 1000), 5000).list;
  ({ list } = run(list, 5000, 6000));
  const r = q.add(list, mk('ev', 4000, { kind: 'live' }), 6000);
  assert.strictEqual(r.replaced.kind, 'upcoming');
  assert.deepStrictEqual(r.list.map((t) => [t.key, t.kind]), [['ev', 'live'], ['other', 'live']]);
  assert.strictEqual(r.list[0].hideLeft, 4000);
});

test('sticky on top does not stall: the toast below counts', () => {
  let list = q.add([], mk('s', 0), 0).list;
  list = q.add(list, mk('b', 2000), 1000).list;
  list = q.add(list, mk('c', 2000), 1000).list;
  const { list: left, gone } = run(list, 1000, 6000);
  assert.deepStrictEqual(gone, [['b', 3000], ['c', 5000]]);
  assert.deepStrictEqual(left.map((t) => t.key), ['s']);
});

test('only visible toasts count; two sticky on top hold the rest until dismissed', () => {
  let list = [mk('s1', 0), mk('s2', 0)].reduce((l, t) => q.add(l, t, 0).list, []);
  list = q.add(list, mk('c', 1000), 1000).list;
  ({ list } = run(list, 1000, 5000));
  assert.strictEqual(list[2].hideLeft, 1000);
  list = list.filter((t) => t.key !== 's1');
  assert.strictEqual(q.counter(list, 5000), 1);
});

test('hover pauses the counting toast', () => {
  let list = q.add([], mk('a', 1000), 0).list;
  list[0].hovered = true;
  ({ list } = run(list, 0, 3000));
  assert.strictEqual(list[0].hideLeft, 1000);
  list[0].hovered = false;
  const { gone } = run(list, 3000, 5000);
  assert.deepStrictEqual(gone, [['a', 4000]]);
});

test('imminent waits for event start + 2 s while the next one counts', () => {
  let list = q.add([], mk('imm', 1000, { kind: 'imminent', eventAt: 30000 }), 0).list;
  list = q.add(list, mk('b', 2000), 500).list;
  const r = run(list, 500, 40000);
  assert.deepStrictEqual(r.gone, [['b', 2500], ['imm', 32750]]); // 1 s counted from start + 2 s, give or take one 250 ms tick);
});
