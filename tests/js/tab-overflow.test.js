const test = require('node:test');
const assert = require('node:assert/strict');
const to = require('../../ui/js/tab-overflow.js');

const tabs = [
  { id: 'players', width: 60, primary: true },
  { id: 'share', width: 100 },
  { id: 'heal', width: 60, primary: true },
  { id: 'stagger', width: 60 },
];

test('everything fits: no More menu', () => {
  assert.deepEqual(to.layout(tabs, 400, 50, 10), { visible: ['players', 'share', 'heal', 'stagger'], overflow: [] });
});

test('tabs that do not fit move into More, primaries stay', () => {
  // 60+10+60 primaries = 130; budget 300-50-10 = 240; share 240 fits exactly, stagger does not.
  assert.deepEqual(to.layout(tabs, 300, 50, 10), { visible: ['players', 'share', 'heal'], overflow: ['stagger'] });
  assert.deepEqual(to.layout(tabs, 200, 50, 10), { visible: ['players', 'heal'], overflow: ['share', 'stagger'] });
  assert.deepEqual(to.layout(tabs, 10, 50, 10), { visible: ['players', 'heal'], overflow: ['share', 'stagger'] });
});

test('tabs keep their order: after one overflows, the later ones do too', () => {
  assert.deepEqual(to.layout(tabs, 250, 50, 10), { visible: ['players', 'heal'], overflow: ['share', 'stagger'] });
  const small = [tabs[0], { id: 'share', width: 200 }, tabs[2], { id: 'stagger', width: 20 }];
  assert.deepEqual(to.layout(small, 240, 50, 10).visible, ['players', 'heal']);
});

test('hidden tabs are left out', () => {
  const noStagger = tabs.map((t) => (t.id === 'stagger' ? { ...t, hidden: true } : t));
  assert.deepEqual(to.layout(noStagger, 240, 50, 10), { visible: ['players', 'share', 'heal'], overflow: [] });
});

test('More shows the active tab name and arrow keys wrap', () => {
  const names = { share: 'Ability share', stagger: 'Stagger' };
  assert.equal(to.moreLabel('stagger', ['share', 'stagger'], names), 'Stagger');
  assert.equal(to.moreLabel('players', ['share', 'stagger'], names), 'More');
  assert.equal(to.step(0, 3, -1), 2);
  assert.equal(to.step(2, 3, 1), 0);
  assert.equal(to.step(0, 0, 1), -1);
});
