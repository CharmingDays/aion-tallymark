// Top-center cube alert window: shows itself only while an alert is up.
(function () {
  const el = document.getElementById('cube-alert');
  const cubes = window.cubeAlert.create(el);
  const tauri = window.__TAURI__;
  let visible = false;
  const sync = () => {
    const want = !el.hidden;
    if (want === visible || !tauri) return;
    visible = want;
    tauri.core.invoke('cube_visible', { show: want }).catch(() => {});
  };
  el.querySelector('button').addEventListener('click', () => setTimeout(sync, 0));
  window.bridge.subscribe((data) => { cubes.update(data); sync(); });
  setInterval(sync, 500);
})();