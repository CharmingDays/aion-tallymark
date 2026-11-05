// Run with: node --test tests/js/replay-trim.test.js
const test = require('node:test');
const assert = require('node:assert');
const { trim } = require('../../ui/js/replay-trim.js');

const s = (first, last, recorded) => ({
  started_at: 1000, ended_at: 1000 + recorded,
  events: [[first, 0, 10, 0, 1], [last, 0, 20, 0, 1]],
});

test('a late first hit starts the replay 1.5s before it', () => {
  const r = trim(s(55699, 278492, 278492));
  assert.strictEqual(r.offset, 54199);
  assert.deepStrictEqual(r.session.events.map((e) => e[0]), [1500, 224293]);
  assert.strictEqual(r.span, 224293);
});

test('an early first hit is left alone', () => {
  const r = trim(s(800, 5000, 5000));
  assert.strictEqual(r.offset, 0);
  assert.strictEqual(r.session.events[0][0], 800);
  assert.strictEqual(r.span, 5000);
});

test('idle time after the last hit is cut to 1.5s', () => {
  const r = trim(s(0, 4000, 20000));
  assert.strictEqual(r.span, 5500);
});

test('the original session is not changed', () => {
  const orig = s(9000, 10000, 10000);
  const r = trim(orig);
  assert.strictEqual(orig.events[0][0], 9000);
  assert.notStrictEqual(r.session, orig);
});

test('a session with no hits keeps its recorded length', () => {
  const r = trim({ started_at: 0, ended_at: 3000, events: [] });
  assert.deepStrictEqual([r.offset, r.span], [0, 3000]);
});
test('heals are shifted with the hits and never go before 0:00', () => {
  const session = Object.assign(s(55699, 278492, 278492), { heals: [[50000, 0, 10, 0, 1], [60000, 0, 20, 2, 1]] });
  const r = trim(session);
  assert.deepStrictEqual(r.session.heals.map((h) => h[0]), [0, 60000 - 54199]);
  assert.strictEqual(session.heals[1][0], 60000, 'the original is untouched');
});

test('a session without heals gets an empty heal list', () => {
  assert.deepStrictEqual(trim(s(55699, 278492, 278492)).session.heals, []);
});
