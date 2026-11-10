// Pets tab in the detailed meter: summoned pets, levels and soul progress from
// game packets. The login pet list sets the start; soul drops update it live.
(function () {
  const { esc } = window.fmt;
  const bridge = window.meterBridge;
  const pl = window.petList;
  const $ = (id) => document.getElementById(id);
  const KEY = 'aion2dps.meter.pets';

  let view = null;
  let loadedVersion = -1;
  let wantedVersion = 0;
  let loading = false;
  let shown = false;
  const saved = (() => { try { return JSON.parse(localStorage.getItem(KEY)) || {}; } catch (_) { return {}; } })();
  const filter = { query: '', family: saved.family || '', show: saved.show || '', sort: saved.sort || 'close' };

  $('pets-family').innerHTML = ['<option value="">All families</option>']
    .concat(pl.FAMILIES.map((f) => `<option value="${f}">${f}</option>`)).join('');
  $('pets-family').value = filter.family;
  $('pets-show').value = filter.show;
  $('pets-sort').value = filter.sort;

  const remember = () => localStorage.setItem(KEY, JSON.stringify({ family: filter.family, show: filter.show, sort: filter.sort }));

  function when(ms) {
    if (!ms) return '';
    const d = new Date(ms);
    return d.toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
  }

  function paint() {
    if (!view) return;
    const hasList = view.list_ms != null;
    const rows = pl.sortRows(pl.filterRows(view.pets, filter, hasList), filter.sort);
    $('pets-sub').textContent = hasList
      ? `${view.summoned} of ${view.total} summoned \u00b7 list from ${when(view.list_ms)}`
      : `${view.total} pets \u00b7 waiting for your pet list`;
    $('pets-note').hidden = hasList;
    $('pets-count').textContent = rows.length === view.pets.length ? '' : `${rows.length} shown`;
    if (!rows.length) {
      $('pets-rows').innerHTML = '<tr class="empty-row"><td colspan="6">No pets match.</td></tr>';
      return;
    }
    $('pets-rows').innerHTML = rows.map((r) => {
      const s = pl.status(r, hasList);
      const share = Math.round(pl.progressShare(r) * 1000) / 10;
      const left = pl.remaining(r);
      const tip = r.needed == null ? 'Top level'
        : `${left} more soul${left === 1 ? '' : 's'} to ${r.level === 0 ? 'summon' : `level ${r.level + 1}`}${r.seen ? '' : ' (no souls seen yet)'}`;
      return `<tr class="pet ${s}${r.seen ? '' : ' unseen'}">`
        + `<td class="name" title="Pet ${r.id}">${esc(r.name)}</td>`
        + `<td><span class="fam fam-${esc(r.family.toLowerCase())}">${esc(r.family)}</span></td>`
        + `<td class="status">${esc(pl.statusText(r, hasList))}</td>`
        + `<td class="souls" title="${esc(tip)}"><span class="pet-bar"><i style="width:${share}%"></i></span><span class="num">${esc(pl.progressText(r))}</span></td>`
        + `<td class="src" title="${esc(r.source || '')}">${esc(r.source || '')}</td>`
        + `<td class="when">${r.updated_ms ? esc(when(r.updated_ms)) : ''}</td>`
        + '</tr>';
    }).join('');
  }

  async function load() {
    if (loading || !shown) return;
    loading = true;
    const want = wantedVersion;
    let ok = false;
    try {
      view = await bridge.getPets();
      loadedVersion = want;
      ok = true;
      paint();
    } catch (_) {
      $('pets-sub').textContent = 'Could not load pets';
    } finally {
      loading = false;
      // A change that arrived while loading.
      if (ok && shown && wantedVersion !== want) load();
    }
  }

  $('pets-search').addEventListener('input', (e) => { filter.query = e.target.value; paint(); });
  $('pets-family').addEventListener('change', (e) => { filter.family = e.target.value; remember(); paint(); });
  $('pets-show').addEventListener('change', (e) => { filter.show = e.target.value; remember(); paint(); });
  $('pets-sort').addEventListener('change', (e) => { filter.sort = e.target.value; remember(); paint(); });

  window.petsTab = {
    /** Called with every live snapshot; fetches the list only when it changed. */
    onLive(data) {
      wantedVersion = data && typeof data.pets_version === 'number' ? data.pets_version : wantedVersion;
      if (shown && (wantedVersion !== loadedVersion || !view)) load();
    },
    show(on) {
      shown = on;
      if (on) load();
    },
  };
})();
