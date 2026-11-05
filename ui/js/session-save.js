// The Save session button, shared by the overlay and the detailed meter.
// Disabled with no damage; green "saved" for 2 s after a save, red on failure.
(function () {
  const SAVED_MS = 2000;

  /**
   * `button` holds an `.idle` and a `.done` look. `onSaved(summary)` gets the
   * saved session's list entry.
   */
  function create(button, { save, onSaved }) {
    const title = button.title;
    let canSave = false;
    let busy = false;
    let timer = 0;

    const paint = () => { button.disabled = busy || !canSave; };

    function settle(cls, message) {
      button.classList.add(cls);
      button.title = message;
      clearTimeout(timer);
      timer = setTimeout(() => {
        button.classList.remove('saved', 'failed');
        button.title = title;
        busy = false;
        paint();
      }, SAVED_MS);
    }

    button.addEventListener('click', async () => {
      if (button.disabled) return;
      busy = true;
      paint();
      try {
        const summary = await save();
        settle('saved', 'Saved');
        if (onSaved) onSaved(summary);
      } catch (err) {
        settle('failed', `Could not save: ${(err && err.message) || err}`);
      }
    });

    return {
      update(hasDamage) {
        canSave = hasDamage;
        paint();
      },
    };
  }

  window.sessionSave = { create, SAVED_MS };
})();
