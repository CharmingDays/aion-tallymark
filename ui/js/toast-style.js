// Toast styles: how each kind of pop-up looks (Config > Toasts & pop-ups).
// The same defaults and limits as src/toast_style.rs. Pure helpers used by
// notify.js (event and pet toasts), the cube window and the Config preview.
(function (root) {
  'use strict';

  const KINDS = [
    { id: 'upcoming', label: 'Event: upcoming', short: 'Upcoming', event: true,
      title: '{event}', message: '{message}', fields: ['event', 'message', 'left'],
      help: 'Shown at the lead time before an event.' },
    { id: 'imminent', label: 'Event: starting soon', short: 'Starting soon', event: true,
      title: 'Starting in {left}', message: 'Get ready \u2014 {event}', fields: ['event', 'message', 'left'],
      help: 'The last 30 seconds before an event starts.' },
    { id: 'live', label: 'Event: live', short: 'Live', event: true,
      title: '{event} is live now', message: '{message}', fields: ['event', 'message'],
      help: 'When an event starts.' },
    { id: 'pet', label: 'Pet progress', short: 'Pet', event: false,
      title: '{pet}', message: '{status}', fields: ['pet', 'family', 'souls', 'needed', 'count', 'gained', 'next', 'level', 'status'],
      help: 'Soul drops, summons and level ups. One at a time; another pet takes it over.' },
    { id: 'cube', label: 'Hidden cube', short: 'Cube', event: false,
      title: '{title}', message: '{subline}', fields: ['title', 'type', 'distance', 'subline'],
      help: 'A hidden cube nearby, out of range or opened.' },
  ];

  const POSITIONS = [
    ['auto', 'Same as Event alerts position'], ['top-left', 'Top left'], ['top', 'Top center'], ['top-right', 'Top right'],
    ['left', 'Middle left'], ['center', 'Center'], ['right', 'Middle right'],
    ['bottom-left', 'Bottom left'], ['bottom', 'Bottom center'], ['bottom-right', 'Bottom right'],
  ];
  const LIMITS = { scale: [70, 150], offset: 1000, monitor: 6, font: [10, 32], opacity: [20, 100], title: 80, message: 160 };
  const FONT = { upcoming: 13, imminent: 13, live: 13, pet: 14, cube: 23 };
  /** Built-in backgrounds, for opacity without a custom colour. */
  const BACKGROUND = { cube: '#101d25' };
  const PANEL = '#161a21';

  function defaults(kind) {
    return {
      enabled: true, scale: 100, position: 'auto', offset_x: 0, offset_y: 0, monitor: 0,
      title: '', message: '', font_px: FONT[kind] || 13,
      accent: '', background: '', text: '', bg_opacity: 100, show_icon: true, sound: 'default',
    };
  }

  /** The style of `kind` from `styles` (the live map, or one style), over the defaults. */
  function of(styles, kind) {
    const base = defaults(kind);
    const s = styles && (styles[kind] || (styles.scale != null ? styles : null));
    return s ? { ...base, ...s } : base;
  }

  const kindOf = (id) => KINDS.find((k) => k.id === id) || KINDS[0];

  /** `{name}` placeholders from `fields`; unknown ones stay as typed. */
  const fill = (template, fields) => String(template || '').replace(/\{(\w+)\}/g, (m, k) => (fields && fields[k] != null ? String(fields[k]) : m));

  const isHex = (c) => typeof c === 'string' && /^#[0-9a-f]{6}$/i.test(c);
  function rgba(hex, alpha) {
    const n = parseInt(hex.slice(1), 16);
    return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
  }

  /**
   * Put `style` on a toast element: accent, background and its opacity, text
   * colour, size (`zoom`, when `scale` is wanted), main text size and icon.
   */
  function apply(el, style, kind, { scale = true } = {}) {
    const s = style || defaults(kind);
    const props = el.style;
    if (isHex(s.accent)) {
      props.setProperty('--tone', s.accent);
      props.setProperty('--tone-bg', rgba(s.accent, 0.16));
      props.setProperty('--fam', s.accent);
      props.setProperty('--cube-accent', s.accent);
    } else {
      ['--tone', '--tone-bg', '--fam', '--cube-accent'].forEach((p) => props.removeProperty(p));
    }
    const alpha = Math.max(LIMITS.opacity[0], Math.min(100, Number(s.bg_opacity) || 100)) / 100;
    if (isHex(s.background) || alpha < 1) props.background = rgba(isHex(s.background) ? s.background : (BACKGROUND[kind] || PANEL), alpha);
    else props.removeProperty('background');
    if (isHex(s.text)) {
      props.color = s.text;
      props.setProperty('--t-muted', rgba(s.text, 0.72));
    } else {
      props.removeProperty('color');
      props.removeProperty('--t-muted');
    }
    if (Number(s.font_px) && Number(s.font_px) !== (FONT[kind] || 13)) props.setProperty('--t-font', `${s.font_px}px`);
    else props.removeProperty('--t-font');
    props.zoom = scale && Number(s.scale) && Number(s.scale) !== 100 ? String(Number(s.scale) / 100) : '';
    el.classList.toggle('no-icon', s.show_icon === false);
  }

  /** Whether this toast plays the chime; `alertsSound` is Event alerts > Sound. */
  const soundOn = (style, alertsSound) => style.sound === 'on' || (style.sound === 'default' && Boolean(alertsSound));

  /** A short two-note chime. Several windows may fire the same toast, so only one plays it. */
  let audio = null;
  function chime(key) {
    try {
      if (typeof localStorage !== 'undefined') {
        if (localStorage.getItem('aion2dps-chime') === key) return;
        localStorage.setItem('aion2dps-chime', key);
      }
    } catch (_) { /* no storage: play anyway */ }
    try {
      audio = audio || new (root.AudioContext || root.webkitAudioContext)();
      if (audio.state === 'suspended') audio.resume();
      const t = audio.currentTime;
      [[880, 0], [1320, 0.12]].forEach(([freq, delay]) => {
        const osc = audio.createOscillator();
        const gain = audio.createGain();
        osc.frequency.value = freq;
        gain.gain.setValueAtTime(0.0001, t + delay);
        gain.gain.exponentialRampToValueAtTime(0.12, t + delay + 0.02);
        gain.gain.exponentialRampToValueAtTime(0.0001, t + delay + 0.18);
        osc.connect(gain).connect(audio.destination);
        osc.start(t + delay);
        osc.stop(t + delay + 0.2);
      });
    } catch (_) { /* no audio device */ }
  }

  /** The styles the pages last saw in a live snapshot (meter, overlay). */
  let current = null;
  const remember = (styles) => { if (styles) current = styles; };
  const styleFor = (spec) => of(spec && spec.style ? spec.style : current, spec && spec.kind);

  const api = { KINDS, POSITIONS, LIMITS, FONT, defaults, of, kindOf, fill, isHex, rgba, apply, soundOn, chime, remember, styleFor };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.toastStyle = api;
})(typeof window !== 'undefined' ? window : globalThis);
