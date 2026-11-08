// Ezoo Download: before the page's own scripts (document_start).
// Some players (Google Drive's) take every press inside their area for
// themselves, before anything under it hears it: a click on the "Download
// this video" button played the video or made it full screen, and the
// button never opened. Presses on Ezoo Download's own buttons are kept from
// the page here (this runs first) and given to the buttons only.
(() => {
  if (globalThis.__ezooEarly) return;
  globalThis.__ezooEarly = true;
  // host element -> its (closed) shadow root, filled in by content.js
  const roots = (globalThis.__ezooRoots = globalThis.__ezooRoots || new WeakMap());
  const KEEP = ["pointerdown", "pointerup", "mousedown", "mouseup", "click", "dblclick", "auxclick", "contextmenu"];
  const GIVE = new Set(["mousedown", "mouseup", "click", "dblclick"]);
  const replayed = new WeakSet();
  const route = (e) => {
    if (replayed.has(e)) return;
    const root = e.target && roots.get(e.target);
    if (!root || typeof e.clientX !== "number") return;
    const inner = root.elementFromPoint(e.clientX, e.clientY);
    // the add-in's own text boxes and lists work as usual
    if (inner && /^(INPUT|SELECT|TEXTAREA|OPTION|LABEL)$/.test(inner.tagName)) return;
    e.stopImmediatePropagation();
    if (!inner || !GIVE.has(e.type)) return;
    // the same press, heard only inside the add-in's own buttons
    const copy = new MouseEvent(e.type, {
      bubbles: true,
      cancelable: true,
      composed: false,
      clientX: e.clientX,
      clientY: e.clientY,
      screenX: e.screenX,
      screenY: e.screenY,
      button: e.button,
      buttons: e.buttons,
      detail: e.detail,
      ctrlKey: e.ctrlKey,
      shiftKey: e.shiftKey,
      altKey: e.altKey,
      metaKey: e.metaKey,
    });
    replayed.add(copy);
    if (!inner.dispatchEvent(copy)) e.preventDefault();
  };
  for (const t of KEEP) addEventListener(t, route, true);
})();
