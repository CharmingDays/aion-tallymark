// Npcap help shared by the overlay and the detailed meter: when capture fails
// because Npcap is missing, the pages show a "Get Npcap" button that opens the
// download page in the default browser. When Npcap is in admin-only mode and
// the Windows prompt was declined, they show a Retry notice instead (the app
// sets capture.npcap_admin_only). Pure functions only, tested with node.
(function (root) {
  'use strict';

  const URL = 'https://npcap.com';
  const BUTTON = 'Get Npcap';

  const RETRY = 'Retry';
  const RETRYING = 'Waiting for Windows\u2026';
  const ADMIN_ONLY_TITLE = 'Capture couldn\u2019t start';
  /** Overlay text: short, since the overlay is compact. */
  const ADMIN_ONLY_SHORT = 'Capture couldn\u2019t start: Npcap is in admin-only mode and the Windows permission prompt was declined.';
  /** Detailed meter banner text. */
  const ADMIN_ONLY_DETAIL = 'Npcap is installed in admin-only mode, and the Windows permission prompt (\u201cThe helper program for \u2018Admin-only Mode\u2019\u201d) was declined. Click Retry and choose Yes to start capturing.';
  const ADMIN_ONLY_FIX = 'To stop the prompt for good, reinstall Npcap from npcap.com and untick \u201cRestrict Npcap driver\u2019s access to Administrators only\u201d.';

  /** True when a capture error is about Npcap, so the button is worth showing. */
  function needsNpcap(error) {
    return typeof error === 'string' && /npcap/i.test(error);
  }

  /** True when capture failed on the Npcap admin-only prompt, so Retry is worth showing. */
  function adminOnly(capture) {
    return Boolean(capture && capture.npcap_admin_only);
  }

  const api = {
    URL, BUTTON, RETRY, RETRYING,
    ADMIN_ONLY_TITLE, ADMIN_ONLY_SHORT, ADMIN_ONLY_DETAIL, ADMIN_ONLY_FIX,
    needsNpcap, adminOnly,
  };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.npcapHelp = api;
})(typeof window !== 'undefined' ? window : globalThis);