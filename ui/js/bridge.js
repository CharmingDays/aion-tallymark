// Where the pages get their data. Inside the desktop app, snapshots arrive as
// Tauri `live` events and actions are Tauri commands. In a normal browser the
// same JSON comes from the localhost server at /api/live.
(function () {
  const tauri = window.__TAURI__;
  const inApp = Boolean(tauri && tauri.core && tauri.event);
  const POLL_MS = 250;

  function subscribe(render, onError) {
    const fail = onError || (() => {});
    if (inApp) {
      tauri.core.invoke('live').then(render).catch(fail);
      tauri.event.listen('live', (event) => render(event.payload)).catch(fail);
      return;
    }
    const tick = async () => {
      try {
        const res = await fetch('/api/live', { cache: 'no-store' });
        render(await res.json());
      } catch (err) {
        fail(err);
      }
    };
    tick();
    setInterval(tick, POLL_MS);
  }

  /** Resolves with the JSON answer, or undefined when there is none. */
  const send = (method, path, body, extraHeaders) => fetch(path, {
    method,
    cache: 'no-store',
    headers: { ...(body ? { 'content-type': 'application/json' } : {}), ...extraHeaders },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (res) => {
    const text = await res.text();
    if (!res.ok) throw new Error(text || `HTTP ${res.status}`);
    return text ? JSON.parse(text) : undefined;
  });
  const post = (path, body, extraHeaders) => send('POST', path, body, extraHeaders);
  const get = (path) => send('GET', path);

  // The server refuses saved-data writes without this header.
  const WRITE = { 'x-aion2dps': '1' };

  const sessionPath = (id) => `/api/sessions/${encodeURIComponent(id)}`;

  function download(href, name) {
    const link = document.createElement('a');
    link.href = href;
    link.download = name;
    document.body.append(link);
    link.click();
    link.remove();
  }

  /** The browser downloads the server's file; the app writes the loaded session. */
  function exportSession(id, session) {
    if (!inApp) {
      download(`${sessionPath(id)}/export`, `${id}.json`);
      return;
    }
    const url = URL.createObjectURL(new Blob([JSON.stringify(session)], { type: 'application/json' }));
    download(url, `${id}.json`);
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  }

  /** Saves and deletes in the app are announced; a browser tab rechecks on focus. */
  function onSessionsChanged(callback) {
    if (inApp) tauri.event.listen('sessions', () => callback()).catch(() => {});
    window.addEventListener('focus', () => callback());
  }

  /** Alert changes in the app are announced; a browser tab rechecks on focus and every 30 s. */
  function onAlertsChanged(callback) {
    if (inApp) tauri.event.listen('alerts', () => callback()).catch(() => {});
    else setInterval(callback, 30_000);
    window.addEventListener('focus', () => callback());
  }

  const currentWindow = () => (inApp && tauri.window ? tauri.window.getCurrentWindow() : null);

  window.meterBridge = {
    inApp,
    subscribe,
    reset: () => (inApp ? tauri.core.invoke('reset') : post('/api/reset')),
    setMode: (mode) => (inApp ? tauri.core.invoke('set_mode', { mode }) : post('/api/mode', { mode })),
    setResetSecs: (secs) => (inApp
      ? tauri.core.invoke('set_reset_secs', { secs })
      : post('/api/settings', { reset_after_secs: secs })),
    /** Either may be undefined to leave it as it is. */
    setBossSettings: ({ lockSecs, damageThreshold, engageLock }) => (inApp
      ? tauri.core.invoke('set_boss_settings', { lockSecs, damageThreshold, engageLock })
      : post('/api/settings', {
        boss_lock_secs: lockSecs,
        boss_damage_threshold: damageThreshold,
        boss_engage_lock: engageLock,
      })),
    setOpenWhenAion2: (on) => (inApp
      ? tauri.core.invoke('set_open_when_aion2', { on })
      : post('/api/settings', { open_when_aion2: on })),
    unlock: () => (inApp ? tauri.core.invoke('unlock') : post('/api/unlock')),
    clearHistory: () => (inApp ? tauri.core.invoke('clear_history') : post('/api/history/clear')),
    setMonsterName: (type, name) => (inApp
      ? tauri.core.invoke('set_monster_name', { monsterType: type, name })
      : post('/api/monster-name', { type, name }, WRITE)),
    saveSession: () => (inApp ? tauri.core.invoke('save_session') : post('/api/session/save', null, WRITE)),
    listSessions: () => (inApp ? tauri.core.invoke('list_sessions') : get('/api/sessions')),
    getSession: (id) => (inApp ? tauri.core.invoke('get_session', { id }) : get(sessionPath(id))),
    deleteSession: (id) => (inApp
      ? tauri.core.invoke('delete_session', { id })
      : send('DELETE', sessionPath(id), null, WRITE)),
    exportSession,
    onSessionsChanged,
    getAlerts: () => (inApp ? tauri.core.invoke('get_alerts') : get('/api/alerts')),
    setAlerts: (alerts) => (inApp ? tauri.core.invoke('set_alerts', { alerts }) : send('PUT', '/api/alerts', alerts, WRITE)),
    restoreDefaultAlerts: () => (inApp ? tauri.core.invoke('restore_default_alerts') : post('/api/alerts/restore-defaults', undefined, WRITE)),
    onAlertsChanged,
    /** In the app, alert toasts go to their own window at the top of the screen. */
    showToast: (toast) => tauri.core.invoke('show_toast', { toast }),
    openMeter: () => (inApp ? tauri.core.invoke('open_meter') : Promise.resolve(window.open('/', '_blank'))),
    openHistory: () => (inApp ? tauri.core.invoke('open_history') : Promise.resolve(window.open('/history', '_blank'))),
    openInBrowser: () => (inApp ? tauri.core.invoke('open_in_browser') : Promise.resolve()),
    // Opens the Npcap download page in the default browser. Nothing is sent to the game.
    openNpcap: () => (inApp ? tauri.core.invoke('open_npcap_page') : Promise.resolve(window.open('https://npcap.com', '_blank', 'noopener'))),
    startDrag: () => { const w = currentWindow(); return w ? w.startDragging() : Promise.resolve(); },
    startResize: (direction) => { const w = currentWindow(); return w ? w.startResizeDragging(direction) : Promise.resolve(); },
    closeWindow: () => { const w = currentWindow(); return w ? w.close() : Promise.resolve(); },
    focusWindow: () => { const w = currentWindow(); return w ? w.setFocus() : Promise.resolve(); },
  };
})();
