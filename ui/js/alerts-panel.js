// The Notifications panel on the detailed meter, from the Figma "Notification
// / Settings" frame: the Event alerts switch, lead time, sound, position and
// Preview, plus the user's own list of events (Figma "Alerts / Event list")
// with an Edit event / New event dialog ("Alerts / Edit event", "Alerts /
// Repeat options"). Every change is saved at once.
(function () {
  const { esc } = window.fmt;
  const bridge = window.meterBridge;
  const sched = window.alertSchedule;

  const LEADS = [1, 5, 10];
  const DURATIONS = [[5, '5s'], [8, '8s'], [15, '15s'], [30, '30s'], [0, 'Until dismissed']];
  const REPEAT_TYPES = [['none', 'Does not repeat'], ['minutes', 'Minutes'], ['hours', 'Hours'], ['daily', 'Daily'], ['weekly', 'Weekly'], ['hours_minutes', 'Hours + minutes']];
  const ICON_EDIT = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10.5 3.2l2.3 2.3-7 7H3.5v-2.3z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>';
  const ICON_TRASH = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round"/></svg>';

  const CLOCKS = [['local', 'This PC (local)'], ['server', 'Game server'], ['utc', 'UTC']];
  /** UTC-12 .. UTC+14 by the hour, as minutes. */
  const OFFSETS = Array.from({ length: 27 }, (_, i) => (i - 12) * 60);
  const RESTORE_CONFIRM = 'Restore default timers?\n\nThis adds back only the built-in timers you deleted. Built-in timers you still have, edited or not, are left as they are, and your own events are not touched.';

  const pad = (n) => String(n).padStart(2, '0');
  const isoDay = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

  /** "Today 20:00", "Tomorrow 06:00", "Sat 19:30", "Nov 30 23:00". */
  function whenText(ms) {
    if (ms == null) return 'never again';
    const d = new Date(ms);
    const time = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const days = Math.round((new Date(d).setHours(0, 0, 0, 0) - today.getTime()) / 86_400_000);
    if (days === 0) return `Today ${time}`;
    if (days === 1) return `Tomorrow ${time}`;
    if (days > 1 && days < 7) return `${sched.WEEKDAYS[(d.getDay() + 6) % 7]} ${time}`;
    return `${d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })} ${time}`;
  }

  /** `button` toggles the panel; `notifier` is what notify.startAlerts returned. */
  function create(button, notifier) {
    const panel = document.createElement('section');
    panel.className = 'alerts-panel';
    panel.hidden = true;
    panel.innerHTML = `
      <div class="ap-head"><h2>Notifications</h2><button type="button" class="t-close" data-close title="Close"><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg></button></div>
      <div class="ap-body">
        <div class="ap-row"><span class="ap-label"><span>Event alerts</span><span>Warn me before events start</span></span><button type="button" class="switch" data-key="enabled" aria-label="Event alerts"></button></div>
        <div class="ap-row"><span class="ap-label"><span>Lead time</span><span>How early to show the banner</span></span><div class="ap-chips" data-leads>${LEADS.map((m) => `<button type="button" data-lead="${m}">${m} min</button>`).join('')}</div></div>
        <div class="ap-row"><span class="ap-label"><span>Sound</span><span>Play a short chime</span></span><button type="button" class="switch" data-key="sound" aria-label="Sound"></button></div>
        <div class="ap-row"><span class="ap-label"><span>Position</span><span>Where banners appear</span></span><div class="ap-chips" data-position><button type="button" data-pos="top">Top</button><button type="button" data-pos="bottom">Bottom</button></div></div>
        <div class="ap-row" style="flex-direction:column;align-items:stretch">
          <span class="ap-label"><span>Events</span><span>Times are this PC's local time unless marked (server) or (UTC)</span></span>
          <ul class="ap-events" data-events></ul>
          <div class="ap-btns">
            <button type="button" class="btn small" data-restore title="Adds back only the built-in timers you deleted. Edited ones and your own events stay as they are.">Restore default timers</button>
            <button type="button" class="btn small primary" data-new>+ Add event</button>
          </div>
        </div>
        <div class="ap-row"><span class="ap-label"><span>Server clock</span><span>For (server) events. Unconfirmed for Global servers: check an in-game time</span></span><select class="ap-select" data-server>${OFFSETS.map((m) => `<option value="${m}">${sched.offsetLabel(m)}</option>`).join('')}</select></div>
        <div class="ap-foot"><span class="ap-note" data-note></span><button type="button" class="btn small primary" data-preview>Preview</button></div>
      </div>`;
    document.body.append(panel);
    const q = (sel) => panel.querySelector(sel);
    let settings = null;

    const note = (text, ok) => { const n = q('[data-note]'); n.textContent = text; n.classList.toggle('ok', Boolean(ok)); };

    function render() {
      if (!settings) return;
      panel.querySelectorAll('.switch').forEach((s) => s.classList.toggle('on', Boolean(settings[s.dataset.key])));
      panel.querySelectorAll('[data-lead]').forEach((b) => b.classList.toggle('on', settings.lead_mins.includes(Number(b.dataset.lead))));
      panel.querySelectorAll('[data-pos]').forEach((b) => b.classList.toggle('on', b.dataset.pos === settings.position));
      const server = q('[data-server]');
      const off = Number(settings.server_utc_offset_mins ?? sched.DEFAULT_SERVER_OFFSET_MINS);
      if (![...server.options].some((o) => Number(o.value) === off)) server.add(new Option(sched.offsetLabel(off), String(off)));
      server.value = String(off);
      const list = settings.alerts;
      const now = Date.now();
      q('[data-events]').innerHTML = list.length
        ? list.map((a) => `<li class="${a.enabled === false ? 'off' : ''}">
            <button type="button" class="switch${a.enabled === false ? '' : ' on'}" data-toggle="${esc(a.id)}" aria-label="Enable ${esc(a.name)}"></button>
            <span class="ev-text"><span class="ev-name" title="${esc(a.message || a.name)}">${esc(a.name)}</span>
            <span class="ev-when">${a.enabled === false ? 'Off' : `Next: ${esc(whenText(sched.nextAt(a, now, settings)))}`} \u00b7 ${esc(sched.repeatShort(a))}</span></span>
            <button type="button" class="ev-icon" data-edit="${esc(a.id)}" title="Edit">${ICON_EDIT}</button>
            <button type="button" class="ev-icon ev-del" data-del="${esc(a.id)}" title="Delete">${ICON_TRASH}</button></li>`).join('')
        : '<li class="ap-empty" style="background:none;padding:2px 0">No events yet. Add one below.</li>';
    }

    async function save(change) {
      const next = JSON.parse(JSON.stringify(settings));
      change(next);
      try {
        settings = await bridge.setAlerts(next);
        note('');
        notifier.reload();
      } catch (err) {
        note(`Could not save: ${(err && err.message) || err}`);
      }
      render();
    }

    async function open() {
      panel.hidden = false;
      button.classList.add('on');
      settings = (await notifier.reload()) || settings;
      if (!settings) note('Could not load the alerts.');
      render();
    }
    const close = () => { panel.hidden = true; button.classList.remove('on'); };

    button.addEventListener('click', () => (panel.hidden ? open() : close()));
    q('[data-close]').addEventListener('click', close);
    panel.addEventListener('click', (event) => {
      if (!settings) return;
      const toggle = event.target.closest('[data-toggle]');
      if (toggle) {
        save((s) => { s.alerts.forEach((a) => { if (a.id === toggle.dataset.toggle) a.enabled = a.enabled === false; }); });
        return;
      }
      const edit = event.target.closest('[data-edit]');
      if (edit) editor.open(settings.alerts.find((a) => a.id === edit.dataset.edit));
      if (event.target.closest('[data-new]')) editor.open(null);
      const sw = event.target.closest('.switch[data-key]');
      if (sw) save((s) => { s[sw.dataset.key] = !s[sw.dataset.key]; });
      const lead = event.target.closest('[data-lead]');
      if (lead) {
        const m = Number(lead.dataset.lead);
        save((s) => { s.lead_mins = s.lead_mins.includes(m) ? s.lead_mins.filter((x) => x !== m) : [...s.lead_mins, m]; });
      }
      const pos = event.target.closest('[data-pos]');
      if (pos) save((s) => { s.position = pos.dataset.pos; });
      const del = event.target.closest('[data-del]');
      if (del) {
        const a = settings.alerts.find((x) => x.id === del.dataset.del);
        if (a && confirm(`Delete the event "${a.name}"?`)) save((s) => { s.alerts = s.alerts.filter((x) => x.id !== a.id); });
      }
    });
    q('[data-preview]').addEventListener('click', () => notifier.preview());
    q('[data-server]').addEventListener('change', (event) => {
      const m = Number(event.target.value);
      if (settings) save((s) => { s.server_utc_offset_mins = m; });
    });
    q('[data-restore]').addEventListener('click', async () => {
      if (!settings || !confirm(RESTORE_CONFIRM)) return;
      try {
        const done = await bridge.restoreDefaultAlerts();
        settings = done.alerts;
        notifier.reload();
        note(done.restored ? `Restored ${done.restored} default timer${done.restored === 1 ? '' : 's'}` : 'All default timers already exist', true);
      } catch (err) {
        note(`Could not restore: ${(err && err.message) || err}`);
      }
      render();
    });
    const editor = createEditor({
      save: (alert, isNew) => save((s) => {
        if (isNew) s.alerts.push(alert);
        else s.alerts = s.alerts.map((a) => (a.id === alert.id ? alert : a));
      }).then(() => q('[data-note]').textContent),
      remove: (id) => save((s) => { s.alerts = s.alerts.filter((a) => a.id !== id); }).then(() => q('[data-note]').textContent),
      defaults: () => (settings ? settings.lead_mins : [5]),
      settings: () => settings,
    });
    window.addEventListener('keydown', (event) => {
      if (event.key !== 'Escape') return;
      if (editor.isOpen()) editor.close();
      else if (!panel.hidden) close();
    });
  }

  /**
   * The Edit event / New event dialog. `save(alert, isNew)` and `remove(id)`
   * resolve to an error message, or '' once saved.
   */
  function createEditor({ save, remove, defaults, settings }) {
    const back = document.createElement('div');
    back.className = 'ae-back';
    back.hidden = true;
    back.innerHTML = `
      <form class="ae" autocomplete="off" novalidate>
        <div class="ae-head"><h2 data-title>Edit event</h2><button type="button" class="t-close" data-cancel title="Close"><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg></button></div>
        <label class="ae-field"><span>Name</span><input name="name" maxlength="40" placeholder="World Boss"></label>
        <label class="ae-field"><span>Message</span><input name="message" maxlength="120" placeholder="Optional"><small>Shown under the name in the toast. Optional.</small></label>
        <div class="ae-field"><span>Start / first occurrence</span>
          <div class="ae-line"><input name="date" type="date"><input name="time" type="time"><select name="clock" title="Which clock the date and time are in">${CLOCKS.map(([v, l]) => `<option value="${v}">${l}</option>`).join('')}</select></div>
          <small data-next></small></div>
        <div class="ae-field"><span>Repeat</span>
          <div class="ae-line">
            <select name="type">${REPEAT_TYPES.map(([v, l]) => `<option value="${v}">${l}</option>`).join('')}</select>
            <span class="ae-rep" data-rep="every">Every <input name="every" type="number" min="1" step="1" class="num"> <span data-unit></span></span>
            <span class="ae-rep" data-rep="hm">Every <input name="hours" type="number" min="0" step="1" class="num"> h <input name="minutes" type="number" min="0" max="59" step="1" class="num"> m</span>
          </div>
          <div class="ae-line ae-rep" data-rep="weekly"><span class="ae-on">On</span><div class="ap-chips" data-days>${sched.WEEKDAYS.map((d, i) => `<button type="button" data-day="${i}">${d}</button>`).join('')}</div></div>
          <small data-summary></small></div>
        <div class="ae-field"><span>Alert me before</span>
          <div class="ap-chips" data-leads>${LEADS.map((m) => `<button type="button" data-lead="${m}">${m} min</button>`).join('')}</div></div>
        <div class="ae-field"><span>Notification duration</span>
          <div class="modes ae-modes" data-durs>${DURATIONS.map(([v, l]) => `<button type="button" data-dur="${v}">${l}</button>`).join('')}</div>
          <div class="ae-line"><span class="ae-on">Custom</span><input name="custom" type="number" min="1" max="600" step="1" class="num" placeholder="e.g. 12"> <span class="ae-muted">seconds (1–600)</span></div>
          <small data-dur-note>How long the toast stays on screen before it hides.</small></div>
        <div class="ae-foot">
          <button type="button" class="btn small ae-delete" data-delete>Delete</button>
          <span class="ae-err" data-err></span>
          <button type="button" class="btn small" data-cancel>Cancel</button>
          <button type="submit" class="btn small primary">Save</button>
        </div>
      </form>`;
    document.body.append(back);
    const form = back.querySelector('form');
    const q = (sel) => back.querySelector(sel);
    let editing = null;
    let days = [];
    let leads = [];
    /** Seconds, 0 for until dismissed. */
    let duration = 8;

    function draft() {
      const type = form.type.value;
      const repeat = { type };
      if (type === 'minutes' || type === 'hours' || type === 'daily') repeat.every = form.every.value === '' ? '' : Number(form.every.value);
      if (type === 'hours_minutes') {
        repeat.hours = form.hours.value === '' ? 0 : Number(form.hours.value);
        repeat.minutes = form.minutes.value === '' ? 0 : Number(form.minutes.value);
      }
      if (type === 'weekly') repeat.days = [...days].sort((a, b) => a - b);
      const clock = form.clock.value;
      return {
        // Keeps fields the form does not show, such as default_id.
        ...(editing || {}),
        id: editing ? editing.id : `a${Date.now().toString(36)}`,
        clock,
        name: form.name.value.trim(),
        message: form.message.value.trim(),
        time: form.time.value,
        start_date: form.date.value,
        repeat,
        lead_mins: [...leads].sort((a, b) => a - b),
        duration_secs: duration,
        enabled: editing ? editing.enabled !== false : true,
      };
    }

    function paint() {
      const a = draft();
      const type = a.repeat.type;
      back.querySelectorAll('[data-rep]').forEach((el) => {
        const which = el.dataset.rep;
        el.hidden = !((which === 'every' && ['minutes', 'hours', 'daily'].includes(type)) || (which === 'hm' && type === 'hours_minutes') || (which === 'weekly' && type === 'weekly'));
      });
      q('[data-unit]').textContent = type === 'minutes' ? 'minute(s)' : type === 'hours' ? 'hour(s)' : `day(s) at ${a.time || 'HH:MM'}`;
      back.querySelectorAll('[data-day]').forEach((b) => b.classList.toggle('on', days.includes(Number(b.dataset.day))));
      back.querySelectorAll('[data-lead]').forEach((b) => b.classList.toggle('on', leads.includes(Number(b.dataset.lead))));
      const preset = DURATIONS.some(([v]) => v === duration);
      back.querySelectorAll('[data-dur]').forEach((b) => b.classList.toggle('on', preset && Number(b.dataset.dur) === duration));
      if (preset && document.activeElement !== form.custom) form.custom.value = '';
      q('[data-dur-note]').textContent = duration === 0 ? 'Stays until you close it.' : `The toast stays ${duration} s, longer while the pointer is on it.`;
      const repErr = sched.repeatError(a.repeat);
      const summary = q('[data-summary]');
      summary.textContent = repErr || (a.time ? sched.repeatSummary(a) : 'Pick a start time');
      summary.classList.toggle('bad', Boolean(repErr));
      summary.classList.toggle('good', !repErr && Boolean(a.time));
      form.every.classList.toggle('bad', repErr === 'Value must be 1 or more');
      form.hours.classList.toggle('bad', type === 'hours_minutes' && Boolean(repErr));
      form.minutes.classList.toggle('bad', type === 'hours_minutes' && Boolean(repErr));
      q('[data-days]').classList.toggle('bad', repErr === 'Pick at least one day');
      const next = a.time && a.start_date && !repErr ? sched.nextAt(a, Date.now(), settings()) : null;
      q('[data-next]').textContent = a.time && a.start_date && !repErr ? (next == null ? 'Already passed: this event will not fire again.' : `Next fire: ${whenText(next)} (local time)`) : '';
      return repErr;
    }

    function open(alert) {
      editing = alert || null;
      const a = alert || {};
      const r = alert ? sched.repeatOf(alert) : { type: 'daily', every: 1 };
      q('[data-title]').textContent = alert ? 'Edit event' : 'New event';
      q('[data-delete]').hidden = !alert;
      form.name.value = a.name || '';
      form.message.value = a.message || '';
      form.time.value = a.time || '';
      // Older alerts have no start date: today stands in, which fires the same.
      form.date.value = a.start_date || isoDay(new Date());
      form.type.value = r.type;
      form.clock.value = a.clock || 'local';
      form.every.value = r.every != null ? r.every : 1;
      form.hours.value = r.type === 'hours_minutes' ? r.hours : 1;
      form.minutes.value = r.type === 'hours_minutes' ? r.minutes : 0;
      days = r.type === 'weekly' ? [...r.days] : [(new Date().getDay() + 6) % 7];
      leads = Array.isArray(a.lead_mins) ? [...a.lead_mins] : [...defaults()];
      duration = a.duration_secs == null ? 8 : Number(a.duration_secs);
      if (!DURATIONS.some(([v]) => v === duration)) form.custom.value = duration;
      q('[data-err]').textContent = '';
      form.name.classList.remove('bad');
      back.hidden = false;
      paint();
      form.name.focus();
    }
    const close = () => { back.hidden = true; editing = null; };

    form.addEventListener('input', (event) => {
      if (event.target === form.custom) {
        const v = Number(form.custom.value);
        if (Number.isInteger(v) && v >= 1 && v <= 600) duration = v;
        form.custom.classList.toggle('bad', form.custom.value !== '' && !(Number.isInteger(v) && v >= 1 && v <= 600));
      }
      paint();
    });
    form.addEventListener('change', paint);
    back.addEventListener('click', async (event) => {
      if (event.target === back || event.target.closest('[data-cancel]')) { close(); return; }
      const day = event.target.closest('[data-day]');
      if (day) {
        const d = Number(day.dataset.day);
        days = days.includes(d) ? days.filter((x) => x !== d) : [...days, d];
      }
      const lead = event.target.closest('[data-lead]');
      if (lead) {
        const m = Number(lead.dataset.lead);
        leads = leads.includes(m) ? leads.filter((x) => x !== m) : [...leads, m];
      }
      const dur = event.target.closest('[data-dur]');
      if (dur) {
        duration = Number(dur.dataset.dur);
        form.custom.value = '';
        form.custom.classList.remove('bad');
      }
      if (event.target.closest('[data-delete]') && editing && confirm(`Delete the event "${editing.name}"?`)) {
        const err = await remove(editing.id);
        if (err) q('[data-err]').textContent = err;
        else close();
        return;
      }
      paint();
    });
    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      const repErr = paint();
      const a = draft();
      form.name.classList.toggle('bad', !a.name);
      const custom = form.custom.value !== '' && form.custom.classList.contains('bad');
      const problem = !a.name ? 'Name the event' : !a.time ? 'Pick a start time' : !a.start_date ? 'Pick a start date' : repErr || (custom ? 'Duration must be 1 to 600 seconds' : '');
      q('[data-err]').textContent = problem;
      if (problem) return;
      const err = await save(a, !editing);
      if (err) q('[data-err]').textContent = err;
      else close();
    });

    return { open, close, isOpen: () => !back.hidden };
  }

  window.alertsPanel = { create };
})();
