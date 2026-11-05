// Run with: node --test tests/js/heal-share.test.js
const test = require('node:test');
const assert = require('node:assert');
const heal = require('../../ui/js/heal-share.js');

const short = (n) => `${(n / 1e6).toFixed(2)}M`;

test('the view falls back to damage', () => {
  assert.strictEqual(heal.view('heal'), 'heal');
  assert.strictEqual(heal.view(null), 'damage');
  assert.strictEqual(heal.view('junk'), 'damage');
});

test('healer rows keep only healers, largest first, scaled to the top healer', () => {
  const rows = heal.healerRows([
    { id: 1, name: 'You', heal: 420000 },
    { id: 2, name: 'Seraphine', heal: 1480000 },
    { id: 3, name: 'Kael', heal: 0 },
  ]);
  assert.deepStrictEqual(rows.map((r) => [r.name, r.width]), [['Seraphine', 100], ['You', 28.4]]);
  assert.deepStrictEqual(heal.healerRows(undefined), []);
});

test('the heal badge is hidden at zero', () => {
  assert.strictEqual(heal.badge(420000, short), '+0.42M');
  assert.strictEqual(heal.badge(0, short), '');
});

test('the live heal feed keeps heal rows for the healer, capped', () => {
  const recent = [
    { heal: true, actor_id: 1 }, { heal: false, actor_id: 1 }, { heal: true, actor_id: 2 }, { heal: true, actor_id: 1 },
  ];
  assert.strictEqual(heal.feed(recent).length, 3);
  assert.strictEqual(heal.feed(recent, 1).length, 2);
  assert.strictEqual(heal.feed(recent, null, 1).length, 1);
});

test('sessions without heals are the empty state', () => {
  assert.strictEqual(heal.hasHeals({ schema_version: 1, events: [] }), false);
  assert.strictEqual(heal.hasHeals({ heals: [] }), false);
  assert.strictEqual(heal.hasHeals({ heals: [[0, 0, 5, 0, 1]] }), true);
});

test('advance counts heals up to the playhead and starts over when rewound', () => {
  const heals = [[100, 0, 400, 0, 7], [200, 1, 300, heal.FLAG_HOT, 8], [300, 0, 100, heal.FLAG_HOT, 7], [900, 1, 50, 0, 8]];
  let s = heal.advance(null, heals, 2, 300);
  assert.strictEqual(s.total, 800);
  assert.deepStrictEqual([...s.heal], [500, 300]);
  assert.strictEqual(s.skills[0].size, 2, 'direct and HoT rows stay apart');
  assert.strictEqual(s.skills[1].get(17).dot, true);
  s = heal.advance(s, heals, 2, 1000);
  assert.strictEqual(s.total, 850);
  s = heal.advance(s, heals, 2, 150);
  assert.strictEqual(s.total, 400);
});

test('rates bucket the healing per second', () => {
  assert.deepStrictEqual(heal.rates([[0, 0, 1000, 0, 1], [1500, 0, 500, 0, 1]], 2000, 2), [1000, 500]);
});
