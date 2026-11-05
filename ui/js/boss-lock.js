// Boss lock and boss settings: the "Locked 8s" chip text, the Defeated badge
// and the checks on the Boss settings fields. No DOM, so it can be tested.
//
// While a boss holds the view, the meter's snapshot carries
// `lock: { remaining_ms, secs, boss, pending_damage, engaged }`. `engaged` is
// a boss still being fought (no countdown); otherwise it just died and the
// countdown runs. Fights recorded behind the lock show once it ends.
(function (root) {
  const DEFAULT_LOCK_SECS = 10;
  const MAX_LOCK_SECS = 60;
  const DEFAULT_BOSS_DAMAGE = 1500000;
  const MIN_BOSS_DAMAGE = 1000;
  const MAX_BOSS_DAMAGE = 1e12;

  /** "Locked 8s", rounded up so it never reads 0s while still locked. */
  function chipText(lock) {
    if (!lock) return '';
    if (lock.engaged) return 'Locked on boss';
    return `Locked ${Math.max(0, Math.ceil((lock.remaining_ms || 0) / 1000))}s`;
  }

  function chipTitle(lock) {
    if (!lock) return '';
    if (lock.engaged) {
      const others = lock.pending_damage > 0 ? ' Other monsters are still recorded.' : '';
      return `Showing ${lock.boss || 'the boss'} while you fight it.${others} Click to unlock now.`;
    }
    const behind = lock.pending_damage > 0 ? ' A new fight is recording behind it.' : '';
    return `Holding ${lock.boss || 'the boss'} so you can save it.${behind} Click to unlock now.`;
  }

  /** Only for one target: All targets has no single one to call defeated. */
  function defeated(meter) {
    return Boolean(meter && meter.target_id != null && meter.target_defeated);
  }

  /** Whole seconds, 0 (off) to 60. */
  function parseLockSecs(text) {
    const t = String(text == null ? '' : text).trim();
    if (!/^\d+$/.test(t)) return { ok: false, error: `Use 0 to ${MAX_LOCK_SECS}.` };
    const value = Number(t);
    if (value > MAX_LOCK_SECS) return { ok: false, error: `Use 0 to ${MAX_LOCK_SECS}.` };
    return { ok: true, value };
  }

  /** "1500000", "1,500,000", "1.5m" or "1500k". */
  function parseBossDamage(text) {
    const t = String(text == null ? '' : text).trim().toLowerCase().replace(/[,_\s]/g, '');
    const m = /^(\d+(?:\.\d+)?)([km]?)$/.exec(t);
    const error = `Use ${formatDamage(MIN_BOSS_DAMAGE)} or more, like 1,500,000 or 1.5m.`;
    if (!m) return { ok: false, error };
    const value = Math.round(Number(m[1]) * (m[2] === 'm' ? 1e6 : m[2] === 'k' ? 1e3 : 1));
    if (!Number.isFinite(value) || value < MIN_BOSS_DAMAGE || value > MAX_BOSS_DAMAGE) return { ok: false, error };
    return { ok: true, value };
  }

  function formatDamage(value) {
    return String(Math.round(value)).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  }

  const api = {
    DEFAULT_LOCK_SECS, MAX_LOCK_SECS, DEFAULT_BOSS_DAMAGE, MIN_BOSS_DAMAGE,
    chipText, chipTitle, defeated, parseLockSecs, parseBossDamage, formatDamage,
  };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.bossLock = api;
})(typeof window !== 'undefined' ? window : globalThis);
