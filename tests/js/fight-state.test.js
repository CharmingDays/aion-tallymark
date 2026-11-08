const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const context = { window: {} };
vm.runInNewContext(fs.readFileSync(require.resolve('../../ui/js/format.js'), 'utf8'), context);
const { fightState } = context.window.fmt;

function data(overrides = {}) {
  return {
    settings: { reset_after_secs: 10 },
    meter: {
      status: 'paused', encounter_damage: 1600000, active_ms: 5000,
      elapsed_ms: 40000, target_hp: null, target_label: 'Boss', targets_shown: 1,
      reset_in_ms: null, ...overrides,
    },
  };
}

test('defeated boss countdown uses the boss hold, not the regular idle reset', () => {
  const f = fightState(data({ lock: { engaged: false, secs: 35, remaining_ms: 24001 } }));
  assert.equal(f.resetSecs, 35);
  assert.equal(f.resetIn, 25);
  assert.equal(f.bossHeld, true);
  assert.equal(f.ended, true);
});

test('boss death shows its hold immediately without waiting for paused status or HP ratio', () => {
  const f = fightState(data({ status: 'combat', lock: { engaged: false, secs: 35, remaining_ms: 35000 } }));
  assert.equal(f.ended, true);
  assert.equal(f.live, false);
  assert.equal(f.resetIn, 35);
});

test('engaged bosses still use the actual idle countdown, not a zero-second death hold', () => {
  const f = fightState(data({ reset_in_ms: 6300, lock: { engaged: true, secs: 35, remaining_ms: 0 } }));
  assert.equal(f.bossHeld, false);
  assert.equal(f.resetSecs, 10);
  assert.equal(f.resetIn, 7);
});

test('ordinary fights retain their idle countdown and default', () => {
  assert.equal(fightState(data({ reset_in_ms: 1100 })).resetIn, 2);
  assert.equal(fightState(data()).resetIn, 10);
});

test('boss hold countdown never goes negative at expiry', () => {
  assert.equal(fightState(data({ lock: { engaged: false, secs: 35, remaining_ms: -1 } })).resetIn, 0);
});
