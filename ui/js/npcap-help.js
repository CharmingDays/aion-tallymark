// Npcap help shared by the overlay and the detailed meter: when capture fails
// because Npcap is missing, the pages show a "Get Npcap" button that opens the
// download page in the default browser. Pure functions only, tested with node.
(function (root) {
  'use strict';

  const URL = 'https://npcap.com';
  const BUTTON = 'Get Npcap';

  /** True when a capture error is about Npcap, so the button is worth showing. */
  function needsNpcap(error) {
    return typeof error === 'string' && /npcap/i.test(error);
  }

  const api = { URL, BUTTON, needsNpcap };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.npcapHelp = api;
})(typeof window !== 'undefined' ? window : globalThis);