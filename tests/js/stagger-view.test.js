const test = require('node:test');
const assert = require('node:assert/strict');
const sv = require('../../ui/js/stagger-view.js');

const full = (n) => Math.round(n).toLocaleString('en-US');
const clock = (ms) => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}`;
const gauge = (over) => ({ max: 1200, remaining: 1200, fill: 0, frozen: false, broken: false, breaks: 0, break_ms: [], estimated: 0, unattributed: 0, ...over });

test('nothing to show without a gauge', () => {
  assert.equal(sv.has(null), false);
  assert.equal(sv.has(undefined), false);
  assert.equal(sv.has(gauge({ max: 0 })), false);
  assert.equal(sv.has(gauge({})), true);
  assert.equal(sv.has(gauge({ max: 0, breaks: 1 })), true);
});

test('the bar fills as the gauge counts down and reads full when broken', () => {
  assert.equal(sv.fillPct(gauge({ remaining: 900, fill: 300 })), 25);
  assert.equal(sv.fillPct(gauge({ remaining: 0, fill: 1200 })), 100);
  assert.equal(sv.fillPct(gauge({ remaining: 1200, fill: 1200, broken: true })), 100);
  assert.equal(sv.fillPct(null), 0);
  assert.equal(sv.gaugeText(gauge({ remaining: 560, fill: 640 }), full), '640 / 1,200');
  assert.equal(sv.gaugeText(gauge({ broken: true }), full), '1,200 / 1,200');
  assert.equal(sv.gaugeText(gauge({ max: 0 }), full), '');
});

test('state, estimates and break times', () => {
  assert.equal(sv.stateText(gauge({})), '');
  assert.equal(sv.stateText(gauge({ frozen: true })), 'Frozen');
  assert.equal(sv.stateText(gauge({ frozen: true, broken: true })), 'Broken');
  assert.equal(sv.points(1097.6, full), '~1,098');
  assert.equal(sv.points(0, full), '');
  assert.equal(sv.points(undefined, full), '');
  assert.equal(sv.breaksTitle(gauge({}), clock), 'No stagger break yet');
  assert.equal(sv.breaksTitle(gauge({ breaks: 2, break_ms: [41000, 108000] }), clock), 'Breaks at 0:41, 1:48');
});
