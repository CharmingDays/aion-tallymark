// "Place on screen" (Config > Toasts & pop-ups > Advanced): a see-through
// window over one monitor with a sample of one pop-up type. Drag it, drop it
// to save the spot (Config saves it, so Undo works), Esc cancels. It never
// talks to the game.
(function () {
  const { invoke } = window.__TAURI__.core;
  const styles = window.toastStyle;
  const box = document.getElementById('pl-box');
  const where = document.getElementById('pl-where');
  let size = [520, 64];
  let at = [0, 0];
  let done = false;

  const finish = (spot) => {
    if (done) return;
    done = true;
    invoke('placer_done', { at: spot }).catch(() => {});
  };

  /** Keep the whole sample on this monitor. */
  function move(x, y) {
    at = [
      Math.round(Math.min(Math.max(0, x), Math.max(0, window.innerWidth - size[0]))),
      Math.round(Math.min(Math.max(0, y), Math.max(0, window.innerHeight - size[1]))),
    ];
    box.style.left = `${at[0]}px`;
    box.style.top = `${at[1]}px`;
    const room = [window.innerWidth - size[0], window.innerHeight - size[1]];
    const pct = (i) => (room[i] > 0 ? Math.round((at[i] / room[i]) * 100) : 0);
    where.textContent = `${pct(0)}% across \u00b7 ${pct(1)}% down`;
  }

  async function start() {
    const info = await invoke('placer_info');
    if (!info) { finish(null); return; }
    const kind = info.kind;
    const style = styles.of(info.style, kind);
    size = info.size;
    box.style.width = `${size[0]}px`;
    box.style.height = `${size[1]}px`;
    if (kind === 'cube') {
      const cube = document.createElement('div');
      cube.className = 'pl-cube';
      cube.textContent = style.title ? styles.fill(style.title, { title: 'Hidden Cube nearby', type: 'Hidden Cube', distance: '42 m', subline: '' }) : 'Hidden Cube nearby';
      box.prepend(cube);
      styles.apply(cube, { ...style, scale: 100 }, 'cube', { scale: false });
      cube.style.fontSize = `${(22 * style.scale) / 100}px`;
    } else {
      const host = window.notify.createHost({ container: box, silent: true });
      const extra = kind === 'pet' ? { hideMs: 3600000 } : { sticky: true };
      host.show({ ...window.notify.sample(kind, Date.now(), extra), style });
    }
    document.getElementById('pl-hint').innerHTML = `Drag the <b>${styles.kindOf(kind).label}</b> pop-up where you want it. <b>Drop</b> to save \u00b7 <b>Esc</b> cancels.`;
    move(info.at[0], info.at[1]);
    window.focus();
  }

  let grab = null;
  box.addEventListener('pointerdown', (event) => {
    grab = [event.clientX - at[0], event.clientY - at[1]];
    box.setPointerCapture(event.pointerId);
    box.classList.add('dragging');
  });
  box.addEventListener('pointermove', (event) => {
    if (grab) move(event.clientX - grab[0], event.clientY - grab[1]);
  });
  box.addEventListener('pointerup', () => {
    if (!grab) return;
    grab = null;
    box.classList.remove('dragging');
    finish(at);
  });
  window.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') finish(null);
    else if (event.key === 'Enter') finish(at);
  });
  start().catch(() => finish(null));
})();
