// Node checks for ui/js/player-colors.js (run via tests/player_colors.rs).
const assert = require('assert');
const pc = require('../ui/js/player-colors.js');

assert.strictEqual(pc.YOU_KEY, 'aion2dps.history.youColor');
assert.ok(pc.isHex(pc.YOU_DEFAULT));
assert.ok(pc.PALETTE.every((c) => pc.isHex(c)));
assert.ok(!pc.PALETTE.includes(pc.YOU_DEFAULT), 'default you colour stays off the other-player palette');
assert.ok(!pc.PALETTE.includes('#93c5fd') && !pc.PALETTE.includes('#fbbf24') && !pc.PALETTE.includes('#fda4af'),
  'near-duplicate palette colours were removed');
assert.strictEqual(pc.PALETTE.length, 9);


const actors = ['Aria', 'Borin', 'Cyra', 'Dax', 'You'];
const you = 4;
const colors = pc.colorsFor(actors, you, '#4ade80');
assert.strictEqual(colors[you], '#4ade80');
assert.strictEqual(new Set(colors).size, colors.length, 'names get distinct colours while palette lasts');
assert.ok(!colors.slice(0, you).includes('#4ade80'), 'others avoid your colour');

const again = pc.colorsFor(actors, you, '#4ade80');
assert.deepStrictEqual(again, colors, 'same names keep the same colours');

const unknown = pc.colorsFor(['Unknown', 'Aria'], null, '#4ade80');
assert.strictEqual(unknown[0], pc.UNKNOWN_COLOR);

const custom = pc.colorsFor(actors, you, '#ff00aa');
assert.strictEqual(custom[you], '#ff00aa');
assert.ok(!custom.slice(0, you).includes('#ff00aa'));

// Collision stepping still yields unique slots for many names
const many = Array.from({ length: pc.PALETTE.length }, (_, i) => `P${i}`);
const manyColors = pc.colorsFor(many, null, '#4ade80');
assert.strictEqual(new Set(manyColors).size, many.length);

console.log('player_colors_node: ok');
