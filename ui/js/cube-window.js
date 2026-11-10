// Top-center cube alert window: shows itself only while an alert is up. Its
// look, size, place and texts follow Config > Toasts & pop-ups > Hidden cube;
// "Send test toast" there shows a sample here for a few seconds.
(function () {
  const el = document.getElementById('cube-alert');
  const styles = window.toastStyle;
  const TEST_MS = 6000;
  let style = styles.defaults('cube');
  let alertsSound = false;

  /** The style's colours only on the "nearby" look; out of range and opened keep theirs. */
  function dress(alert) {
    const plain = alert && alert.status !== 'detected';
    styles.apply(el, plain ? { ...style, accent: '', background: '' } : style, 'cube', { scale: false });
    document.documentElement.style.fontSize = `${(16 * style.scale) / 100}px`;
    if (Number(style.font_px) !== styles.FONT.cube) el.style.setProperty('--t-font', `${style.font_px / 16}rem`);
  }

  function label(text, alert) {
    dress(alert);
    const fields = {
      title: text.title, subline: text.subline, type: alert.cube_type || 'Hidden Cube',
      distance: typeof alert.distance_m === 'number' && Number.isFinite(alert.distance_m) ? `${Math.round(alert.distance_m)} m` : '',
    };
    return {
      title: style.title ? styles.fill(style.title, fields) : text.title,
      subline: style.message ? styles.fill(style.message, fields) : text.subline,
    };
  }

  function onShow(alert, fresh) {
    if (fresh && styles.soundOn(style, alertsSound)) styles.chime(`cube:${alert.ts_ms}:${alert.sequence}`);
  }

  const cubes = window.cubeAlert.create(el, undefined, { label, onShow });
  const tauri = window.__TAURI__;
  let visible = false;
  const sync = () => {
    const want = !el.hidden;
    if (want === visible || !tauri) return;
    visible = want;
    tauri.core.invoke('cube_visible', { show: want }).catch(() => {});
  };

  // Send test toast: a sample cube, on top of what the game shows, for TEST_MS.
  let test = null;
  function withTest(data) {
    if (!test || Date.now() > test.until) {
      test = null;
      return data;
    }
    return { ...data, settings: { ...(data.settings || {}), hidden_cube_alerts: true }, cube_alerts: [...(data.cube_alerts || []), test.alert] };
  }
  let last = { settings: {}, cube_alerts: [] };
  const render = () => { cubes.update(withTest(last)); sync(); };
  if (tauri) {
    tauri.event.listen('cube-test', () => {
      const now = Date.now();
      test = { until: now + TEST_MS, alert: { ts_ms: now, sequence: Number.MAX_SAFE_INTEGER, entity_id: -1, status: 'detected', cube_type: 'Hidden Cube (test)', distance_m: 42 } };
      render();
      setTimeout(render, TEST_MS + 50);
    });
  }

  el.querySelector('button').addEventListener('click', () => setTimeout(sync, 0));
  window.meterBridge.subscribe((data) => {
    last = data;
    style = styles.of(data.toast_styles, 'cube');
    alertsSound = Boolean(data.alerts_sound);
    render();
  });
  setInterval(sync, 500);
})();
