// Number and text formatting shared by the overlay and the detailed meter.
(function () {
  // Only UNKNOWN (an unnamed player) gets the faint "unknown" style.
  // MONSTER (an unnamed non-player target) is drawn like any other name.
  const UNKNOWN = 'Unknown';
  const MONSTER = 'Monster';
  const BOSS = 'Boss';

  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));

  /** 12.4M, 3.12M, 121.6K, 940. */
  function short(n) {
    n = Math.max(0, Math.round(Number(n) || 0));
    if (n >= 999_950_000) return (n / 1e9).toFixed(2) + 'B';
    if (n >= 9_995_000) return (n / 1e6).toFixed(1) + 'M';
    if (n >= 999_950) return (n / 1e6).toFixed(2) + 'M';
    if (n >= 1000) return (n / 1000).toFixed(1) + 'K';
    return String(n);
  }

  /** 1,234,567 */
  const full = (n) => Math.round(Number(n) || 0).toLocaleString('en-US');

  /** 01:42, or 1:02:03 past an hour. */
  function clock(ms) {
    const s = Math.max(0, Math.floor((Number(ms) || 0) / 1000));
    const h = Math.floor(s / 3600);
    const mm = String(Math.floor((s % 3600) / 60)).padStart(2, '0');
    const ss = String(s % 60).padStart(2, '0');
    return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
  }

  /** 0.254 -> 25% (whole) or 25.4% (one decimal). */
  const pct = (ratio, digits = 0) => `${(Math.max(0, Number(ratio) || 0) * 100).toFixed(digits)}%`;

  /** Crit rate, or a dash when there are no hits to count. */
  const rate = (part, whole, digits = 1) => (whole > 0 ? pct(part / whole, digits) : '—');

  /**
   * The fight view shared by both pages. "ended" is the retention window after
   * a kill or after the fight went quiet: the totals stay until the reset delay
   * (the engine's 30 second default) runs out.
   */
  function fightState(data) {
    const m = data.meter;
    const resetSecs = data.settings ? data.settings.reset_after_secs : 30;
    const hasFight = m.status !== 'idle' && m.encounter_damage > 0;
    const ended = hasFight && (m.status === 'paused' || m.target_hp === 0);
    const resetIn = m.reset_in_ms == null ? resetSecs : Math.ceil(m.reset_in_ms / 1000);
    const extra = m.targets_shown > 1 ? ` +${m.targets_shown - 1}` : '';
    return {
      hasFight,
      ended,
      live: hasFight && !ended && m.status === 'combat',
      resetSecs,
      resetIn,
      target: hasFight ? m.target_label + extra : 'Waiting for combat',
      time: m.status === 'paused' ? m.active_ms : m.elapsed_ms,
    };
  }

  /** The opt-in target debug log, as a one-line note, or '' when it is off. */
  function debugNote(capture) {
    const d = capture && capture.target_debug;
    if (!d) return '';
    if (d.error) return `Target debug log failed: ${d.error} (${d.path})`;
    const state = d.full ? 'full, stopped' : `${d.records} records`;
    return `Target debug log on \u00b7 ${state} \u00b7 ${d.path}`;
  }

  window.fmt = { UNKNOWN, MONSTER, BOSS, esc, short, full, clock, pct, rate, fightState, debugNote };
})();
