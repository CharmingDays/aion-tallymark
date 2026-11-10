// The screen toast window: a borderless, transparent, always-on-top window at
// the top center of the primary monitor that the app opens when an alert
// fires. It ignores the mouse except over a toast, so the game underneath
// keeps every click, and closes once its last toast is gone. Each toast kind
// can have its own window (its style's place, monitor and size); a toast that
// moves to another window drops out of this one.
(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const POINTER_MS = 100;

  let shown = 0;
  let closing = false;
  const host = window.notify.createHost({
    onChange(count) {
      if (count) shown = count;
      layout();
      // Wait out the leave animation, then let the app close the window.
      if (!count && shown && !closing) {
        closing = true;
        setTimeout(() => {
          closing = false;
          if (!document.querySelector('.toast:not(.leaving)')) invoke('toast_done').catch(() => {});
        }, 300);
      }
    },
  });
  host.onSnooze = (t) => {
    const { key, kind, title, message, event, note, eventAt, snooze, hideMs, sticky } = t;
    invoke('snooze_toast', { toast: { key, kind, title, message, event, note, eventAt, snooze, hideMs, sticky }, ms: window.notify.SNOOZE_MS }).catch(() => {});
  };

  let lastHeight = 0;
  let lastWidth = 0;
  function layout() {
    const box = document.querySelector('.notify-host');
    const height = Math.ceil(box.getBoundingClientRect().height);
    // The widest toast (its style's size) and the host's side padding.
    const widest = Math.max(0, ...[...box.querySelectorAll('.toast:not([hidden])')].map((el) => el.getBoundingClientRect().width));
    const width = widest ? Math.ceil(widest) + 20 : lastWidth;
    if (height && (height !== lastHeight || width !== lastWidth)) {
      lastHeight = height;
      lastWidth = width;
      invoke('toast_layout', { height, width: width || null }).catch(() => {});
    }
  }

  /** Line the toasts up by where this window sits: left, center or right; newest last. */
  function align(place) {
    const box = document.querySelector('.notify-host');
    const p = String(place || 'top');
    box.classList.toggle('place-left', /left$/.test(p));
    box.classList.toggle('place-right', /right$/.test(p));
    host.setPosition(p.startsWith('bottom') ? 'bottom' : 'top');
  }

  async function take() {
    try {
      (await invoke('take_toasts')).forEach((spec) => {
        if (spec.place) align(spec.place);
        host.show(spec);
      });
      layout();
    } catch (_) { /* window closing */ }
  }

  // Clicks pass through to the game unless the pointer is on a toast.
  let over = false;
  setInterval(async () => {
    let now = false;
    try {
      const p = await invoke('toast_pointer');
      const el = p && document.elementFromPoint(p[0], p[1]);
      now = Boolean(el && el.closest('.toast'));
    } catch (_) { return; }
    if (now === over) return;
    over = now;
    invoke('toast_hit', { over }).catch(() => {});
    if (!over) document.querySelectorAll('.toast').forEach((el) => el.dispatchEvent(new MouseEvent('mouseleave')));
  }, POINTER_MS);

  listen('toast-wake', take);
  const me = window.__TAURI__.window.getCurrentWindow().label;
  listen('toast-drop', (event) => {
    const p = event.payload || {};
    if (p.keep !== me) host.drop(p.key);
  });
  take();
})();
