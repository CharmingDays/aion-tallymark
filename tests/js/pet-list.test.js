const test = require('node:test');
const assert = require('node:assert/strict');
const pl = require('../../ui/js/pet-list.js');

const row = (id, name, family, level, souls, needed, seen = true) => ({ id, name, family, level, souls, needed, seen, source: null });
const rows = [
  row(1, 'Klaw', 'Cogni', 0, 0, 5, false),
  row(2, 'Slink', 'Fera', 0, 4, 5),
  row(3, 'Papis', 'Fera', 3, 0, null),
  row(4, 'Skyray', 'Fera', 1, 20, 25),
];

test('closest to unlock: fewest missing souls first, max level last', () => {
  assert.deepEqual(pl.sortRows(rows, 'close').map((r) => r.id), [2, 1, 4, 3]);
});

test('status and filters', () => {
  assert.equal(pl.status(rows[0], false), 'unknown');
  assert.equal(pl.status(rows[0], true), 'locked');
  assert.equal(pl.statusText(rows[2], true), 'Level 3 (max)');
  assert.equal(pl.progressText(rows[1]), '4 / 5');
  assert.deepEqual(pl.filterRows(rows, { query: 'fera', show: 'progress' }, true).map((r) => r.id), [2, 4]);
  assert.deepEqual(pl.filterRows(rows, { family: 'Cogni' }, true).map((r) => r.id), [1]);
});
