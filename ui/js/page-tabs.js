// Detailed meter page tabs (Meter, Pets). Tabs that do not fit move into a
// More menu, so the bar never scrolls sideways.
(function () {
  const $ = (id) => document.getElementById(id);
  const to = window.tabOverflow;
  const KEY = 'aion2dps.meter.page';
  const bar = $('page-tabs');
  const more = $('page-more');
  const button = $('page-more-btn');
  const label = $('page-more-label');
  const menu = $('page-more-menu');
  const tabs = () => [...bar.querySelectorAll(':scope > button[data-page]')];
  const names = () => Object.fromEntries(tabs().map((b) => [b.dataset.page, b.textContent]));
  let overflow = [];
  let page = localStorage.getItem(KEY) || 'meter';
  if (!tabs().some((b) => b.dataset.page === page)) page = 'meter';

  function paint() {
    tabs().forEach((b) => {
      b.classList.toggle('on', b.dataset.page === page);
      b.setAttribute('aria-selected', String(b.dataset.page === page));
    });
    document.querySelectorAll('[data-page-view]').forEach((el) => { el.hidden = el.dataset.pageView !== page; });
    label.textContent = to.moreLabel(page, overflow, names());
    button.classList.toggle('on', overflow.includes(page));
    menu.querySelectorAll('button').forEach((b) => b.classList.toggle('on', b.dataset.page === page));
    document.body.dataset.page = page;
    if (window.petsTab) window.petsTab.show(page === 'pets');
  }

  function layout() {
    const list = tabs();
    list.forEach((b) => { b.hidden = false; });
    more.hidden = false;
    const gap = parseFloat(getComputedStyle(bar).columnGap) || 0;
    const items = list.map((b) => ({ id: b.dataset.page, width: b.offsetWidth, primary: b.dataset.primary === '1' }));
    const res = to.layout(items, bar.clientWidth, button.offsetWidth || 64, gap);
    list.forEach((b) => { b.hidden = !res.visible.includes(b.dataset.page); });
    overflow = res.overflow;
    more.hidden = overflow.length === 0;
    if (more.hidden) close(false);
    const n = names();
    menu.innerHTML = overflow.map((id) => `<li role="none"><button type="button" role="menuitem" data-page="${id}" tabindex="-1">${window.fmt.esc(n[id] || id)}</button></li>`).join('');
    paint();
  }

  function pick(id) {
    page = id;
    localStorage.setItem(KEY, page);
    paint();
  }

  function open() {
    menu.hidden = false;
    button.setAttribute('aria-expanded', 'true');
    const items = [...menu.querySelectorAll('button')];
    const at = Math.max(0, items.findIndex((b) => b.dataset.page === page));
    if (items[at]) items[at].focus();
  }

  function close(focus) {
    if (menu.hidden) return;
    menu.hidden = true;
    button.setAttribute('aria-expanded', 'false');
    if (focus) button.focus();
  }

  tabs().forEach((b) => b.addEventListener('click', () => pick(b.dataset.page)));
  button.addEventListener('click', () => (menu.hidden ? open() : close(true)));
  button.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowDown') { event.preventDefault(); open(); }
  });
  menu.addEventListener('click', (event) => {
    const b = event.target.closest('button[data-page]');
    if (!b) return;
    close(true);
    pick(b.dataset.page);
  });
  menu.addEventListener('keydown', (event) => {
    const items = [...menu.querySelectorAll('button')];
    const at = items.indexOf(document.activeElement);
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(true); }
    else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const next = to.step(at, items.length, event.key === 'ArrowDown' ? 1 : -1);
      if (next >= 0) items[next].focus();
    } else if (event.key === 'Tab') close(false);
  });
  document.addEventListener('pointerdown', (event) => { if (!more.contains(event.target)) close(false); });
  window.addEventListener('blur', () => close(false));
  if (window.ResizeObserver) new ResizeObserver(layout).observe(bar);
  else window.addEventListener('resize', layout);
  layout();
})();
