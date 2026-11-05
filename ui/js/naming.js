// Naming the current target's monster type, shared by the overlay and the
// detailed meter. Only a target whose spawn record was seen has a type.
(function () {
  const { MONSTER, UNKNOWN, BOSS } = window.fmt;
  const HINT = 'Start the meter before the target spawns';
  const HINT_MS = 3000;
  const MAX_CHARS = 40;
  const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

  const typeHex = (type) => `#${Number(type).toString(16).padStart(6, '0')}`;

  /**
   * `label` shows the target and `badge` its Boss or Dummy tag. `save(type,
   * name)` resolves once the name is stored; an empty name clears it, and the
   * label falls back to the bundled name. `focus()` brings the window forward
   * before the editor takes the keyboard.
   */
  function create({ label, badge, save, focus }) {
    let target = { type: null, label: '', user: null, bundled: null, kind: '', hint: false };
    let editor = null;
    let hint = null;
    let hintTimer = 0;

    function update(data, f) {
      const m = data.meter;
      const single = f.hasFight && m.target_id != null;
      const typed = single && m.target_type != null;
      target = {
        type: typed ? m.target_type : null,
        label: m.target_label,
        user: typed ? m.target_user_name : null,
        bundled: typed ? m.target_bundled_name : null,
        kind: !single ? '' : m.target_dummy ? 'Dummy' : m.target_boss ? 'Boss' : '',
        hint: single && m.target_type == null && (m.target_label === MONSTER || m.target_label === BOSS),
      };
      label.classList.toggle('nameable', target.type != null);
      label.title = target.type != null ? 'Click to name this monster' : target.hint ? HINT : '';
      paintBadge();
    }

    /** Hidden while the editor or the hint takes the label's place. */
    function paintBadge() {
      if (!badge) return;
      badge.textContent = target.kind;
      badge.hidden = !target.kind || Boolean(editor) || Boolean(hint);
    }

    const clickable = () => !editor && (target.type != null || target.hint);

    function click() {
      if (editor) return;
      if (target.type != null) open(target);
      else if (target.hint) showHint();
    }

    function showHint() {
      clearTimeout(hintTimer);
      if (!hint) {
        hint = document.createElement('span');
        hint.className = 'name-hint';
        hint.textContent = HINT;
        label.after(hint);
      }
      label.hidden = true;
      paintBadge();
      hintTimer = setTimeout(hideHint, HINT_MS);
    }

    function hideHint() {
      clearTimeout(hintTimer);
      if (hint) hint.remove();
      hint = null;
      if (!editor) label.hidden = false;
      paintBadge();
    }

    /** The input holds the user's name only; the bundled one is the placeholder. */
    function startValue({ label: current, user, bundled }) {
      if (user != null) return user;
      if (bundled != null || current === MONSTER || current === BOSS || current === UNKNOWN) return '';
      return current;
    }

    function open(current) {
      hideHint();
      const { type } = current;
      const root = document.createElement('span');
      root.className = 'name-edit';
      const input = document.createElement('input');
      input.type = 'text';
      input.maxLength = MAX_CHARS;
      input.spellcheck = false;
      input.autocomplete = 'off';
      input.placeholder = current.bundled || 'Name this monster';
      input.setAttribute('aria-label', 'Monster name');
      input.value = startValue(current);
      const tag = document.createElement('span');
      tag.className = 'type-hint';
      tag.textContent = typeHex(type);
      root.append(input, tag);
      label.hidden = true;
      label.after(root);
      editor = { root, input, type, saving: false };
      paintBadge();

      input.addEventListener('keydown', (event) => {
        // Keys typed into the name are not page shortcuts.
        event.stopPropagation();
        if (event.key === 'Enter') {
          event.preventDefault();
          commit();
        } else if (event.key === 'Escape') {
          event.preventDefault();
          close();
        }
      });
      input.addEventListener('input', () => fail(''));
      input.addEventListener('blur', () => {
        if (editor && !editor.saving) close();
      });
      input.focus();
      input.select();
      Promise.resolve(focus && focus()).catch(() => {}).then(() => {
        if (editor && editor.input === input && document.activeElement !== input) input.focus();
      });
    }

    function fail(message) {
      if (!editor) return;
      editor.root.classList.toggle('bad', Boolean(message));
      editor.root.title = message;
    }

    function commit() {
      const { input, type } = editor;
      const name = input.value.trim();
      if ([...name].length > MAX_CHARS || CONTROL.test(name)) {
        fail(`Use up to ${MAX_CHARS} characters, without control characters.`);
        return;
      }
      editor.saving = true;
      Promise.resolve(save(type, name)).then(close, (err) => {
        if (!editor) return;
        editor.saving = false;
        fail(String((err && err.message) || err || 'Could not save the name.'));
        input.focus();
      });
    }

    function close() {
      if (!editor) return;
      const { root } = editor;
      editor = null;
      root.remove();
      label.hidden = false;
      paintBadge();
    }

    return { update, click, clickable, editing: () => Boolean(editor) };
  }

  window.targetNaming = { create, HINT };
})();
