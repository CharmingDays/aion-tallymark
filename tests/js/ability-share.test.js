// Run with: node --test tests/js/ability-share.test.js
const test = require('node:test');
const assert = require('node:assert');
const { merge, split, groupKey } = require('../../ui/js/ability-share.js');

const names = {
  16330000: 'Dimensional Control',
  16330027: 'Dimensional Control - Delayed Damage',
  100014: 'Fire Spirit: Basic Attack',
  100024: 'Water Spirit: Basic Attack',
  16150000: 'Jointstrike: Corrode',
  16151400: 'Fire Spirit: Rage Burst',
};
const row = (skill, dot, damage, hits = 1, crits = 0) => ({ skill, dot, damage, hits, crits });
const map = (...rows) => new Map(rows.map((r) => [r.skill * 2 + (r.dot ? 1 : 0), r]));

test('DoT ticks and direct hits of a skill are one row', () => {
  const rows = merge([map(row(16150000, false, 100, 2, 1), row(16150000, true, 50, 5))], [0], names);
  assert.strictEqual(rows.length, 1);
  assert.deepStrictEqual([rows[0].damage, rows[0].hits, rows[0].crits, rows[0].dotDamage], [150, 7, 1, 50]);
});

test('a " - " variant folds into the plain skill', () => {
  const rows = merge([map(row(16330027, false, 30), row(16330000, false, 70))], [0], names);
  assert.strictEqual(rows.length, 1);
  assert.strictEqual(rows[0].skill, 16330000);
  assert.strictEqual(rows[0].name, 'Dimensional Control');
  assert.strictEqual(rows[0].variants, 2);
});

test('different skills in one id block stay apart', () => {
  assert.notStrictEqual(groupKey(100014, names), groupKey(100024, names));
  assert.notStrictEqual(groupKey(16150000, names), groupKey(16151400, names));
});

test('unnamed skills are never merged', () => {
  const rows = merge([map(row(5550001, false, 1), row(5550002, false, 1))], [0], {});
  assert.strictEqual(rows.length, 2);
});

test('player damage is summed per player for the split bar', () => {
  const rows = merge([map(row(16330000, false, 10)), map(row(16330027, false, 5), row(16330000, true, 5))], [0, 1], names);
  assert.deepStrictEqual(rows[0].by, [[0, 10], [1, 10]]);
});

test('only the chosen actors count', () => {
  const rows = merge([map(row(16330000, false, 10)), map(row(16330000, false, 99))], [1], names);
  assert.strictEqual(rows[0].damage, 99);
});

test('rows past the cutoff go to Other, but never just one', () => {
  const rows = Array.from({ length: 14 }, (_, i) => ({ damage: 100 - i, hits: i, crits: 0 }));
  let s = split(rows, 'damage', 12);
  assert.deepStrictEqual([s.shown.length, s.rest.length], [12, 2]);
  s = split(rows.slice(0, 13), 'damage', 12);
  assert.deepStrictEqual([s.shown.length, s.rest.length], [13, 0]);
});

test('sorting by hits and crit rate, zero-damage rows dropped', () => {
  const rows = [{ damage: 10, hits: 1, crits: 1 }, { damage: 20, hits: 5, crits: 1 }, { damage: 0, hits: 9, crits: 9 }];
  assert.strictEqual(split(rows, 'hits').shown[0].damage, 20);
  assert.strictEqual(split(rows, 'crit').shown[0].damage, 10);
  assert.strictEqual(split(rows, 'damage').shown.length, 2);
});
