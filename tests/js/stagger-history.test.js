const test = require('node:test');
const assert = require('node:assert/strict');
const sh = require('../../ui/js/stagger-history.js');

const full = (n) => Math.round(n).toLocaleString('en-US');
const clock = (ms) => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}`;

test('older sessions have no stagger', () => {
  assert.equal(sh.has({ events: [] }), false);
  assert.equal(sh.has(null), false);
  assert.equal(sh.has({ stagger: [], stagger_breaks: [] }), false);
  assert.equal(sh.has({ stagger: [[0, 0, 5, 100]] }), true);
  assert.equal(sh.has({ stagger_breaks: [41000] }), true);
});

test('totals follow the playhead and start over when it goes back', () => {
  const shares = [[100, 0, 5, 100], [200, 1, 3, 200], [200, 0, 2.5, 100], [900, 1, 4, 300]];
  let acc = sh.advance(null, shares, 2, 200);
  assert.equal(acc.total, 10.5);
  assert.deepEqual([...acc.actor], [7.5, 3]);
  assert.equal(acc.skills[0].get(100), 7.5);
  acc = sh.advance(acc, shares, 2, 1000);
  assert.equal(acc.total, 14.5);
  assert.equal(acc.skills[1].get(300), 4);
  acc = sh.advance(acc, shares, 2, 150);
  assert.equal(acc.total, 5);
  assert.equal(sh.advance(null, undefined, 2, 1000).total, 0);
  assert.equal(sh.advance(null, [[0, 9, 5, 1]], 2, 1000).total, 0, 'an actor outside the list is skipped');
});

test('breaks up to the playhead and their text', () => {
  assert.deepEqual(sh.breaksUntil([41000, 108000], 60000), [41000]);
  assert.deepEqual(sh.breaksUntil(undefined, 60000), []);
  assert.equal(sh.breaksText([], clock), 'None yet');
  assert.equal(sh.breaksText([41000, 108000], clock), '2 at 0:41, 1:48');
  assert.equal(sh.points(7.5, full), '~8');
  assert.equal(sh.points(0, full), '');
});
