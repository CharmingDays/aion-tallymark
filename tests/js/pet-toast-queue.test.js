const test = require('node:test');
const assert = require('node:assert/strict');
const q = require('../../ui/js/toast-queue.js');

const toast = (key, extra = {}) => ({ key, kind: 'live', hideTotal: 8000, hideLeft: 8000, ...extra });

test('a pet toast keeps its own clock beside the counting alert', () => {
  let list = [];
  list = q.add(list, toast('alert'), 0).list;
  list = q.add(list, toast('pet:1104', { kind: 'pet', selfTimed: true, hideTotal: 60000, hideLeft: 60000 }), 1000).list;
  const res = q.tick(list, 2000, 1000);
  assert.equal(res.list[0].hideLeft, 7000, 'the alert counts');
  assert.equal(res.list[1].hideLeft, 59000, 'the pet counts too');
  assert.equal(q.counter(res.list, 2000), 0);
  const gone = q.tick([toast('pet:1', { kind: 'pet', selfTimed: true, hideTotal: 500, hideLeft: 500 })], 0, 600);
  assert.equal(gone.expired.length, 1);
  assert.equal(gone.list.length, 0);
});
