// Run with: node --test tests/js/alerts-schedule.test.js
const test = require('node:test');
const assert = require('node:assert');
const s = require('../../ui/js/alerts-schedule.js');

const at = (h, m, sec = 0) => new Date(2026, 10, 3, h, m, sec).getTime();
const boss = { id: 'wb', name: 'World Boss', time: '20:00', enabled: true };

test('times parse as minutes after midnight', () => {
  assert.strictEqual(s.parseTime('09:05'), 545);
  assert.strictEqual(s.parseTime('9:05'), 545);
  for (const bad of ['24:00', '12:60', '1200', '', 'ab:cd']) assert.strictEqual(s.parseTime(bad), null, bad);
});

test('a daily alert is next today, or tomorrow once it passed', () => {
  assert.strictEqual(s.nextAt(boss, at(19, 0)), at(20, 0));
  assert.strictEqual(s.nextAt(boss, at(20, 0)), at(20, 0) + 86_400_000);
});

test('a repeating alert steps from its time', () => {
  const every = { ...boss, time: '00:15', every_mins: 30 };
  assert.strictEqual(s.nextAt(every, at(10, 0)), at(10, 15));
  assert.strictEqual(s.nextAt(every, at(10, 15)), at(10, 45));
});

test('each lead time, 30 s and live fire once, in order', () => {
  const settings = { enabled: true, lead_mins: [5, 1], alerts: [boss] };
  const fired = s.due(settings, at(19, 50), at(20, 0, 1));
  assert.deepStrictEqual(fired.map((f) => f.stage), ['upcoming', 'upcoming', 'imminent', 'live']);
  assert.deepStrictEqual(fired.map((f) => f.at), [at(19, 55), at(19, 59), at(19, 59, 30), at(20, 0)]);
  assert.strictEqual(s.due(settings, at(19, 55), at(19, 56)).length, 0, 'the 5 min toast fired at exactly from');
});

test('nothing fires when alerts are off or the alert is disabled', () => {
  assert.strictEqual(s.due({ enabled: false, lead_mins: [5], alerts: [boss] }, at(19, 0), at(21, 0)).length, 0);
  const off = { enabled: true, lead_mins: [5], alerts: [{ ...boss, enabled: false }] };
  assert.strictEqual(s.due(off, at(19, 0), at(21, 0)).length, 0);
});

test('countdowns read like the design', () => {
  assert.strictEqual(s.countdown(300_000), '5:00');
  assert.strictEqual(s.countdown(29_001), '0:30');
  assert.strictEqual(s.countdown(3_720_000), '1:02:00');
});

const on = (d, h, m) => new Date(2026, 10, d, h, m).getTime(); // Nov 2026; Nov 3 is a Tuesday
const ev = (repeat, extra = {}) => ({ id: 'x', name: 'X', time: '20:00', start_date: '2026-11-03', enabled: true, repeat, ...extra });

test('does not repeat: once at the start, then never', () => {
  const once = ev({ type: 'none' });
  assert.strictEqual(s.nextAt(once, on(3, 10, 0)), on(3, 20, 0));
  assert.strictEqual(s.nextAt(once, on(3, 20, 0)), null);
});

test('minutes and hours step from the start date and time', () => {
  assert.strictEqual(s.nextAt(ev({ type: 'minutes', every: 45 }), on(1, 0, 0)), on(3, 20, 0), 'nothing before the start');
  assert.strictEqual(s.nextAt(ev({ type: 'minutes', every: 45 }), on(3, 20, 0)), on(3, 20, 45));
  assert.strictEqual(s.nextAt(ev({ type: 'minutes', every: 45 }), on(3, 21, 31)), on(3, 22, 15));
  assert.strictEqual(s.nextAt(ev({ type: 'hours', every: 2 }), on(4, 1, 0)), on(4, 2, 0));
});

test('hours + minutes combine into one step', () => {
  const r = ev({ type: 'hours_minutes', hours: 1, minutes: 30 });
  assert.strictEqual(s.nextAt(r, on(3, 20, 0)), on(3, 21, 30));
  assert.strictEqual(s.nextAt(r, on(3, 22, 59)), on(3, 23, 0));
  assert.strictEqual(s.nextAt(ev({ type: 'hours_minutes', hours: 0, minutes: 20 }), on(3, 20, 5)), on(3, 20, 20));
  assert.strictEqual(s.nextAt(ev({ type: 'hours_minutes', hours: 3, minutes: 0 }), on(3, 20, 5)), on(3, 23, 0));
});

test('daily every N days at the time', () => {
  const every3 = ev({ type: 'daily', every: 3 }, { time: '06:00' });
  assert.strictEqual(s.nextAt(every3, on(2, 12, 0)), on(3, 6, 0));
  assert.strictEqual(s.nextAt(every3, on(3, 6, 0)), on(6, 6, 0));
  assert.strictEqual(s.nextAt(every3, on(7, 0, 0)), on(9, 6, 0));
  assert.strictEqual(s.nextAt(ev({ type: 'daily', every: 1 }), on(5, 21, 0)), on(6, 20, 0));
});

test('weekly on several days at the time', () => {
  const wedSat = ev({ type: 'weekly', days: [2, 5] }, { time: '19:30' }); // Wed, Sat
  assert.strictEqual(s.nextAt(wedSat, on(3, 12, 0)), on(4, 19, 30));
  assert.strictEqual(s.nextAt(wedSat, on(4, 19, 30)), on(7, 19, 30));
  assert.strictEqual(s.nextAt(wedSat, on(7, 20, 0)), on(11, 19, 30));
  assert.strictEqual(s.nextAt(ev({ type: 'weekly', days: [1] }), on(3, 20, 0)), on(10, 20, 0), 'Tuesday after Tuesday');
  assert.strictEqual(s.nextAt(ev({ type: 'weekly', days: [] }), on(3, 0, 0)), null);
});

test('old alerts keep their meaning', () => {
  assert.strictEqual(s.repeatOf(boss).type, 'daily');
  assert.strictEqual(s.repeatOf({ ...boss, every_mins: 30 }).type, 'minutes');
});

test('per-event lead times replace the global ones', () => {
  const settings = { enabled: true, lead_mins: [5], alerts: [ev({ type: 'none' }, { lead_mins: [10] })] };
  const fired = s.due(settings, on(3, 19, 45), on(3, 20, 0));
  assert.deepStrictEqual(fired.map((f) => f.lead), [10, 0.5, 0]);
});

test('repeat summaries and errors read like the design', () => {
  assert.strictEqual(s.repeatSummary(ev({ type: 'hours_minutes', hours: 1, minutes: 30 })), 'Repeats every 1h 30m');
  assert.strictEqual(s.repeatSummary(ev({ type: 'weekly', days: [2, 5] }, { time: '19:30' })), 'Repeats Wed, Sat at 19:30');
  assert.strictEqual(s.repeatSummary(ev({ type: 'none' })), 'Fires once at the start time');
  assert.strictEqual(s.repeatError({ type: 'minutes', every: 0 }), 'Value must be 1 or more');
  assert.strictEqual(s.repeatError({ type: 'hours_minutes', hours: 0, minutes: 0 }), 'Interval must be 1 minute or more');
  assert.strictEqual(s.repeatError({ type: 'weekly', days: [] }), 'Pick at least one day');
  assert.strictEqual(s.repeatError({ type: 'daily', every: 2 }), '');
});

test('server and UTC clocks fire at the real instant, whatever the PC zone', () => {
  const u = (d, h, m) => Date.UTC(2026, 10, d, h, m);
  const settings = { server_utc_offset_mins: 540 };
  // Daily reset 07:00 UTC.
  const reset = { id: 'r', name: 'Reset', time: '07:00', clock: 'utc', repeat: { type: 'daily', every: 1 } };
  assert.strictEqual(s.nextAt(reset, u(4, 6, 0), settings), u(4, 7, 0));
  assert.strictEqual(s.nextAt(reset, u(4, 7, 0), settings), u(5, 7, 0));
  // Watcher Kaira every 3 h from 01:00 server (UTC+9) = 16:00 UTC, 19:00 UTC, ...
  const kaira = { id: 'k', name: 'Kaira', time: '01:00', clock: 'server', repeat: { type: 'hours', every: 3 } };
  assert.strictEqual(s.nextAt(kaira, u(4, 15, 0), settings), u(4, 16, 0));
  assert.strictEqual(s.nextAt(kaira, u(4, 16, 0), settings), u(4, 19, 0));
  assert.strictEqual(s.nextAt(kaira, u(4, 16, 0), { server_utc_offset_mins: -300 }), u(4, 18, 0), 'UTC-5: 01:00 = 06:00 UTC, steps of 3 h');
  // Weekly Fri, Sun 21:00 server: Wed Nov 4 2026 -> Fri Nov 6 21:00 KST = 12:00 UTC.
  const nahma = { id: 'n', name: 'Nahma', time: '21:00', clock: 'server', repeat: { type: 'weekly', days: [4, 6] } };
  assert.strictEqual(s.nextAt(nahma, u(4, 0, 0), settings), u(6, 12, 0));
  // Hourly at :30.
  const raid = { id: 'b', name: 'Raid', time: '00:30', clock: 'server', repeat: { type: 'hours', every: 1 } };
  assert.strictEqual(s.nextAt(raid, u(4, 10, 31), settings), u(4, 11, 30));
  // No settings: UTC+9.
  assert.strictEqual(s.nextAt(kaira, u(4, 15, 0)), u(4, 16, 0));
  assert.strictEqual(s.offsetLabel(540), 'UTC+9');
  assert.strictEqual(s.offsetLabel(-330), 'UTC-5:30');
  assert.strictEqual(s.repeatShort(nahma), 'Weekly on Fri, Sun 21:00 (server)');
});

test('due uses the server clock from the settings', () => {
  const u = (h, m) => Date.UTC(2026, 10, 4, h, m);
  const reset = { id: 'r', name: 'Reset', time: '07:00', clock: 'utc', repeat: { type: 'daily', every: 1 }, enabled: true };
  const fired = s.due({ enabled: true, lead_mins: [5], alerts: [reset] }, u(6, 50), u(7, 0));
  assert.deepStrictEqual(fired.map((f) => f.at), [u(6, 55), u(6, 59) + 30_000, u(7, 0)]);
});
