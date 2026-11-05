// Config panel: the meter's settings in a floating panel styled like Alerts.
// meter.js binds the fields and the single Save button; closing discards unsaved edits.
(() => {
  const panel = document.getElementById('config-panel');
  const button = document.getElementById('open-config');
  const alertsBtn = document.getElementById('open-alerts');
  if (!panel || !button) return;
  const alertsPanel = () => document.querySelector('.alerts-panel:not(.config-panel)');
  const close = () => {
    panel.hidden = true;
    button.classList.remove('on');
    document.dispatchEvent(new Event('config:closed'));
  };
  const open = () => {
    const other = alertsPanel();
    if (other && !other.hidden) { const x = other.querySelector('[data-close]'); if (x) x.click(); }
    panel.hidden = false;
    button.classList.add('on');
  };
  button.addEventListener('click', () => (panel.hidden ? open() : close()));
  panel.querySelector('[data-close]').addEventListener('click', close);
  // Opening Alerts closes Config.
  if (alertsBtn) alertsBtn.addEventListener('click', () => { if (!panel.hidden) close(); }, true);
  window.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !panel.hidden) close();
  });
})();
