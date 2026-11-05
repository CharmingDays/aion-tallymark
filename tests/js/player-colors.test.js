// Run with: node --test tests/js/player-colors.test.js
const test = require('node:test');
const assert = require('node:assert');
const c = require('../../ui/js/player-colors.js');

test('you get green by default, or your saved colour', () => {
  assert.strictEqual(c.colorsFor(['Aria', 'Boros'], 1, null)[1], '#4ade80');
  assert.strictEqual(c.colorsFor(['Aria', 'Boros'], 0, '#F472B6')[0], '#f472b6');
  assert.strictEqual(c.colorsFor(['Aria'], 0, 'red')[0], '#4ade80');
});

test('players get distinct colours, never your colour', () => {
  const names = ['Aria', 'Boros', 'Cyra', 'Dax', 'Eli', 'Fen'];
  const got = c.colorsFor(names, 2, '#60a5fa');
  assert.strictEqual(new Set(got).size, names.length);
  got.forEach((col, i) => { if (i !== 2) assert.notStrictEqual(col, '#60a5fa'); });
});

test('a name keeps its colour whatever its index', () => {
  const a = c.colorsFor(['Aria', 'Boros'], null, null);
  const b = c.colorsFor(['Boros', 'Aria'], null, null);
  assert.strictEqual(a[0], b[1]);
  assert.strictEqual(a[1], b[0]);
});

test('Unknown is grey', () => {
  assert.strictEqual(c.colorsFor(['Unknown'], null, null)[0], c.UNKNOWN_COLOR);
});

test('segments split the bar by damage and sum to its width', () => {
  const s = c.segments([[0, 300], [1, 100], [2, 0]], 40);
  assert.deepStrictEqual(s.map((x) => x.actor), [0, 1]);
  assert.strictEqual(s[0].width, 30);
  assert.strictEqual(s[1].width, 10);
  assert.strictEqual(s[0].pct, 75);
  assert.deepStrictEqual(c.segments([[0, 0]], 10), []);
});
