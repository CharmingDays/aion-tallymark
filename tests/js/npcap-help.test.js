// Run with: node --test tests/js/npcap-help.test.js
const test = require('node:test');
const assert = require('node:assert');
const npcap = require('../../ui/js/npcap-help.js');

test('the button points at the Npcap site', () => {
  assert.strictEqual(npcap.URL, 'https://npcap.com');
  assert.strictEqual(npcap.BUTTON, 'Get Npcap');
});

test('only Npcap errors show the button', () => {
  assert.ok(npcap.needsNpcap("Npcap was not found. aion2dps needs Npcap for live capture."));
  assert.ok(npcap.needsNpcap('Could not open eth0: denied. Install Npcap with WinPcap API-compatible mode and run as Administrator.'));
  assert.ok(!npcap.needsNpcap('Lost the local meter.'));
  assert.ok(!npcap.needsNpcap(''));
  assert.ok(!npcap.needsNpcap(null));
  assert.ok(!npcap.needsNpcap(undefined));
});
test('only a declined admin-only prompt shows Retry', () => {
  assert.ok(npcap.adminOnly({ npcap_admin_only: true, error: 'x' }));
  assert.ok(!npcap.adminOnly({ npcap_admin_only: false, error: 'Npcap was not found.' }));
  assert.ok(!npcap.adminOnly(null));
  assert.ok(!npcap.adminOnly(undefined));
  assert.ok(npcap.ADMIN_ONLY_FIX.includes('npcap.com'));
  assert.ok(npcap.ADMIN_ONLY_FIX.includes('Restrict Npcap driver\u2019s access to Administrators only'));
});
