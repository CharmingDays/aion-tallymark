// The screen toast window: a borderless, transparent, always-on-top window at
// the top center of the primary monitor that the app opens when an alert
// fires. It ignores the mouse except over a toast, so the game underneath
// keeps every click, and closes once its last toast is gone.
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
    const { key, kind, title, message, eventAt, snooze, hideMs, sticky } = t;
    invoke('snooze_toast', { toast: { key, kind, title, message, eventAt, snooze, hideMs, sticky }, ms: window.notify.SNOOZE_MS }).catch(() => {});
  };

  let lastHeight = 0;
  function layout() {
    const height = Math.ceil(document.querySelector('.notify-host').getBoundingClientRect().height);
    if (height && height !== lastHeight) {
      lastHeight = height;
      invoke('toast_layout', { height }).catch(() => {});
    }
  }

  async function take() {
    try {
      (await invoke('take_toasts')).forEach((spec) => host.show(spec));
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
  take();
})();
