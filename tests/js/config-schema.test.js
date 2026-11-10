const test = require('node:test');
const assert = require('node:assert/strict');
const s = require('../../ui/js/config-schema.js');

test('every setting has a known section, a card and a default', () => {
  const ids = new Set();
  for (const d of s.SETTINGS) {
    assert.ok(s.SECTIONS.some((x) => x.id === d.section), d.id);
    assert.ok(d.card && d.label && d.desc, d.id);
    if (s.editable(d)) assert.notEqual(d.default, undefined, d.id);
    assert.ok(!ids.has(d.id), `duplicate ${d.id}`);
    ids.add(d.id);
  }
});

test('the pet toast duration keeps 10 to 300 s, default 60, and shows under Pets too', () => {
  const d = s.byId('pet_toast_secs');
  assert.deepEqual([d.min, d.max, d.default], [10, 300, 60]);
  assert.equal(s.parse(d, '9').ok, false);
  assert.equal(s.parse(d, '301').ok, false);
  assert.deepEqual(s.parse(d, '90'), { ok: true, value: 90 });
  assert.equal(s.stepValue(d, 300, 1), 300);
  assert.ok(s.inSection(s.visible(true), 'pets').some((x) => x.id === 'pet_toast_secs'));
});

test('new options: overlay opacity and the three debug log switches', () => {
  const o = s.byId('overlay_opacity');
  assert.deepEqual([o.section, o.min, o.max, o.default], ['overlay', 20, 100, 100]);
  for (const [id, env] of [['debug_pets', 'pets'], ['debug_position', 'position'], ['debug_cubes', 'cubes']]) {
    const d = s.byId(id);
    assert.equal(d.section, 'advanced');
    assert.equal(d.default, false);
    assert.equal(d.env, env);
  }
});

test('app-only settings are left out in a browser', () => {
  const ids = s.visible(false).map((d) => d.id);
  assert.ok(!ids.includes('start_with_windows'));
  assert.ok(ids.includes('reset_after_secs'));
});

test('search "toast" finds the alert rows, the Pets mirrors and every toast style row', () => {
  const defs = s.visible(true);
  const res = s.search(defs, 'toast');
  const styled = defs.filter((d) => d.section === 'toasts').length;
  assert.equal(res.counts.alerts, 5);
  assert.equal(res.counts.pets, 2);
  assert.equal(res.counts.general, 0);
  assert.equal(res.counts.toasts, styled + 2);
  assert.equal(s.match(s.byId('alert_sound'), 'toast'), 'desc');
});

test('Toasts & pop-ups: one set of rows per type, with checked limits', () => {
  const defs = s.inSection(s.visible(true), 'toasts');
  for (const kind of ['upcoming', 'imminent', 'live', 'pet', 'cube']) {
    const rows = s.forToastKind(defs, kind);
    for (const key of ['sound', 'scale', 'position', 'monitor', 'offset_x', 'offset_y', 'title', 'message', 'font_px', 'accent', 'background', 'text', 'bg_opacity', 'show_icon']) {
      assert.ok(rows.some((d) => d.id === `toast.${kind}.${key}`), `${kind}.${key}`);
    }
    assert.ok(rows.every((d) => !d.toastKind || d.toastKind === kind), kind);
  }
  assert.ok(s.forToastKind(defs, 'pet').some((d) => d.id === 'pet_toast_secs'));
  assert.ok(!s.forToastKind(defs, 'live').some((d) => d.id === 'pet_toast_secs'));
  assert.ok(s.forToastKind(defs, 'cube').some((d) => d.id === 'hidden_cube_alerts'));
  const off = s.byId('toast.live.offset_x');
  assert.deepEqual(s.parse(off, '-40'), { ok: true, value: -40 });
  assert.equal(s.parse(off, '-1010').ok, false);
  assert.equal(s.parse(s.byId('toast.pet.scale'), '160').ok, false);
  assert.equal(s.parse(s.byId('toast.pet.title'), 'x'.repeat(81)).ok, false);
  assert.deepEqual(s.parse(s.byId('toast.pet.accent'), ''), { ok: true, value: '' });
  assert.equal(s.parse(s.byId('toast.pet.accent'), 'red').ok, false);
  assert.equal(s.changeText([{ def: s.byId('toast.pet.scale'), before: 100, after: 120 }]), 'Pet progress \u203a Size changed 100% \u2192 120%.');
  const x = s.byId('toast.cube.x_pct');
  assert.deepEqual([x.min, x.max, x.advanced, s.editable(s.byId('toast.cube.spot'))], [0, 100, true, false]);
  assert.ok(s.byId('toast.cube.position').options.some(([v]) => v === 'custom'));
  assert.equal(s.changeText([{ def: x, before: 50, after: 80 }, { def: s.byId('toast.cube.position'), before: 'auto', after: 'custom' }]), 'Hidden cube \u203a moved to a new spot.');
  const cards = s.cards(s.forToastKind(defs, 'upcoming'), 'toasts').map((c) => c.card);
  assert.deepEqual(cards, ['Show', 'Size & place', 'Text', 'Colors']);
});

test('modified compares with the default, lists in any order', () => {
  assert.equal(s.isModified(s.byId('pet_toast_secs'), 60), false);
  assert.equal(s.isModified(s.byId('pet_toast_secs'), 90), true);
  assert.equal(s.isModified(s.byId('alert_lead'), [5]), false);
  assert.equal(s.isModified(s.byId('alert_lead'), [5, 1]), true);
  assert.equal(s.same([1, 5], [5, 1]), true);
});

test('saved text says what changed', () => {
  const d = s.byId('pet_toast_secs');
  assert.equal(s.changeText([{ def: d, before: 60, after: 90 }]), 'Toast duration changed 60 s \u2192 90 s.');
  assert.equal(s.changeText([{ def: s.byId('pet_toasts'), before: true, after: false }]), 'Pet progress toast turned off.');
});

test('undo history returns the last change group first', () => {
  const h = s.createHistory(2);
  h.push([{ id: 'a', before: 1, after: 2 }]);
  h.push([]);
  h.push([{ id: 'b', before: 3, after: 4 }]);
  h.push([{ id: 'c', before: 5, after: 6 }]);
  assert.equal(h.size(), 2);
  assert.equal(h.pop()[0].id, 'c');
  assert.equal(h.pop()[0].id, 'b');
  assert.equal(h.pop(), null);
});
