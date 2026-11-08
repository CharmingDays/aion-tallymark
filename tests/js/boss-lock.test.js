const test = require('node:test');
const assert = require('node:assert/strict');
const lock = require('../../ui/js/boss-lock.js');

test('the chip counts whole seconds up and is empty when unlocked', () => {
  assert.equal(lock.chipText(null), '');
  assert.equal(lock.chipText({ remaining_ms: 10000 }), '10s');
  assert.equal(lock.chipText({ remaining_ms: 7001 }), '8s');
  assert.equal(lock.chipText({ remaining_ms: 1 }), '1s');
  assert.equal(lock.chipText({ remaining_ms: -50 }), '0s');
  assert.equal(lock.chipText({ remaining_ms: 0, engaged: true }), 'On');
});

test('the chip title names the boss and a fight behind the lock', () => {
  assert.match(lock.chipTitle({ boss: 'Fediv', pending_damage: 0 }), /Holding Fediv .*Click to unlock/);
  assert.doesNotMatch(lock.chipTitle({ boss: 'Fediv', pending_damage: 0 }), /recording/);
  assert.match(lock.chipTitle({ boss: 'Fediv', pending_damage: 5 }), /recording behind it/);
  assert.match(lock.chipTitle({ boss: 'Fediv', pending_damage: 0, engaged: true }), /Showing Fediv while you fight it\. Click to unlock/);
  assert.match(lock.chipTitle({ boss: 'Fediv', pending_damage: 9, engaged: true }), /still recorded/);
});

test('defeated needs a single target', () => {
  assert.equal(lock.defeated({ target_id: 5, target_defeated: true }), true);
  assert.equal(lock.defeated({ target_id: null, target_defeated: true }), false);
  assert.equal(lock.defeated({ target_id: 5, target_defeated: false }), false);
  assert.equal(lock.defeated(null), false);
});

test('lock seconds are whole numbers from 0 to 60', () => {
  assert.deepEqual(lock.parseLockSecs('10'), { ok: true, value: 10 });
  assert.deepEqual(lock.parseLockSecs(' 0 '), { ok: true, value: 0 });
  assert.deepEqual(lock.parseLockSecs(60), { ok: true, value: 60 });
  for (const bad of ['61', '-1', '2.5', '', 'ten', null]) assert.equal(lock.parseLockSecs(bad).ok, false, bad);
});

test('boss damage accepts commas and k/m suffixes', () => {
  assert.deepEqual(lock.parseBossDamage('1500000'), { ok: true, value: 1500000 });
  assert.deepEqual(lock.parseBossDamage('1,500,000'), { ok: true, value: 1500000 });
  assert.deepEqual(lock.parseBossDamage('1.5m'), { ok: true, value: 1500000 });
  assert.deepEqual(lock.parseBossDamage('2500K'), { ok: true, value: 2500000 });
  for (const bad of ['', '999', '-5', 'lots', '1.5b', '0']) assert.equal(lock.parseBossDamage(bad).ok, false, bad);
  assert.equal(lock.formatDamage(1500000), '1,500,000');
  assert.equal(lock.formatDamage(lock.DEFAULT_BOSS_DAMAGE), '1,500,000');
});
