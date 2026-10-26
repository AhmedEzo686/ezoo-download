// Popup/options behavior contracts with a minimal DOM, without a browser profile.
// node extension/test/ui-contract.test.js
const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vm = require("vm");
const extension = path.join(__dirname, "..");

class Element {
  constructor(tag, all) {
    this.tagName = tag; this.children = []; this.dataset = {}; this.attributes = {}; this.listeners = {}; this.style = {};
    this.className = ""; this.textContent = ""; this.checked = false; this.disabled = false; this.hidden = false; this._value = ""; all.push(this);
    this.classList = {
      toggle: (name, force) => { const names = new Set(this.className.split(/\s+/).filter(Boolean)); const add = force === undefined ? !names.has(name) : force; add ? names.add(name) : names.delete(name); this.className = [...names].join(" "); },
      remove: (name) => { this.className = this.className.split(/\s+/).filter((n) => n !== name).join(" "); },
    };
  }
  set value(v) { this._value = String(v); }
  get value() { return this._value; }
  set innerHTML(_) { throw new Error("User-facing HTML must use safe text nodes"); }
  setAttribute(name, value) { this.attributes[name] = String(value); if (name === "class") this.className = value; }
  addEventListener(name, handler) { (this.listeners[name] ||= []).push(handler); }
  append(...nodes) { this.children.push(...nodes); }
  appendChild(node) { this.children.push(node); return node; }
  replaceChildren(...nodes) { this.children = [...nodes]; }
  async fire(name) { if (this.disabled) return; for (const handler of this.listeners[name] || []) await handler({ target: this }); }
}

async function page(name, { store = {}, items = [], online = true, preparedPort = undefined, title = "Calm: <Coast>?", url = "https://example.test/watch" } = {}) {
  const all = [], ids = {}, messages = [], writes = [];
  const html = fs.readFileSync(path.join(extension, `${name}.html`), "utf8");
  for (const match of html.matchAll(/id="([^"]+)"/g)) ids[match[1]] = new Element("existing", all);
  const document = { body: new Element("body", all), getElementById: (id) => { assert(ids[id], `missing HTML id ${id}`); return ids[id]; }, createElement: (tag) => new Element(tag, all), createElementNS: (_, tag) => new Element(tag, all), querySelectorAll: (selector) => { assert.strictEqual(selector, "[data-download]"); return all.filter((e) => e.dataset.download); } };
  let opened = 0, closed = 0;
  const chrome = {
    storage: { local: { get: async (defaults) => ({ ...defaults, ...store }), set: async (values) => { writes.push(values); Object.assign(store, values); } } },
    tabs: { query: async () => [{ id: 7, title, url }] },
    runtime: { getURL: (part) => `chrome-extension://synthetic/${part}`, openOptionsPage: () => { opened++; }, sendMessage: (message, callback) => { messages.push(message); queueMicrotask(() => callback(message.type === "swiftget-get-media" ? items : { ok: online })); } },
  };
  const sandbox = { document, chrome, window: { close: () => { closed++; } }, console, AbortController, fetch: async () => ({ ok: preparedPort !== undefined, text: async () => JSON.stringify({ port: preparedPort }) }), setTimeout: (...args) => { const t = setTimeout(...args); t.unref(); return t; }, clearTimeout };
  await vm.runInNewContext(fs.readFileSync(path.join(extension, `${name}.js`), "utf8"), sandbox);
  return { ids, all, messages, writes, document, store, opened: () => opened, closed: () => closed };
}

(async () => {
  const unsafeLabel = '<img src="https://untrusted.test/x" onerror="alert(1)">';
  let p = await page("popup", { items: [{ label: unsafeLabel, ext: "mp4", url: "https://media.test/video", sub: "84 MB" }, { label: "Unavailable format", disabled: true }] });
  assert.strictEqual(p.ids.status.textContent, "Connected");
  assert(p.ids.openApp.className.includes("secondary-action"));
  const buttons = p.document.querySelectorAll("[data-download]");
  assert.strictEqual(buttons.length, 2); assert(!buttons[0].disabled); assert(buttons[1].disabled);
  assert(p.all.some((n) => n.className === "media-title" && n.textContent === unsafeLabel), "media labels stay literal text");
  await buttons[0].fire("click");
  const download = p.messages.find((m) => m.type === "swiftget-download");
  assert(download); assert.strictEqual(download.item.url, "https://media.test/video"); assert.strictEqual(download.referrer, "https://example.test/watch");
  assert.strictEqual(download.fileName, "Calm_ _Coast__.mp4"); assert.strictEqual(buttons[0].dataset.sent, "true"); assert(buttons[0].disabled);
  const count = p.messages.length; await buttons[0].fire("click"); assert.strictEqual(p.messages.length, count, "sent actions do not submit twice");
  p.ids.enabled.checked = false; await p.ids.enabled.fire("change"); assert.strictEqual(p.store.enabled, false); assert(p.ids.scale.disabled); assert(buttons.every((b) => b.disabled));
  p.ids.enabled.checked = true; await p.ids.enabled.fire("change"); assert(buttons[1].disabled, "unavailable rows stay disabled after resume");
  await p.ids.settings.fire("click"); assert.strictEqual(p.opened(), 1); assert.strictEqual(p.closed(), 1);

  p = await page("popup", { online: false, items: [{ label: "720p · MP4", url: "https://media.test/v" }] });
  assert.strictEqual(p.ids.status.textContent, "App Not Running"); assert(!p.ids.connectionHint.hidden);
  const offlineButton = p.document.querySelectorAll("[data-download]")[0]; await offlineButton.fire("click"); assert(!offlineButton.disabled); assert.strictEqual(offlineButton.dataset.pending, "false"); assert(p.all.some((n) => n.className === "row-error" && !n.hidden));

  p = await page("popup", { url: "https://www.youtube.com/watch?v=x&list=synthetic", items: [{ label: "Audio", ext: "m4a" }], store: { enabled: false, bubbleSize: "large" } });
  assert.strictEqual(p.ids.scaleText.textContent, "125%"); assert(p.document.querySelectorAll("[data-download]").every((b) => b.disabled));
  p.ids.enabled.checked = true; await p.ids.enabled.fire("change"); await p.document.querySelectorAll("[data-download]")[0].fire("click"); assert(p.messages.some((m) => m.type === "swiftget-playlist"));
  await p.ids.scaleReset.fire("click"); assert.strictEqual(p.store.bubbleScale, 1);

  p = await page("popup", { items: [{ type: "protected", protected: true, disabled: true, label: "Protected Video", sub: "Use official downloads." }] });
  assert.strictEqual(p.document.querySelectorAll("[data-download]").length, 0); assert.strictEqual(p.ids.mediaCount.textContent, "");
  assert(p.all.some((n) => n.href === "https://help.netflix.com/en/node/54816")); assert(!p.messages.some((m) => m.type === "swiftget-download"));

  let o = await page("options", { preparedPort: 19614, store: { bubbleSize: "small" } });
  assert.strictEqual(o.ids.port.value, "19614"); assert.strictEqual(o.ids.bubbleScaleText.textContent, "85%");
  o.ids.port.value = "65536"; await o.ids.port.fire("change"); assert.strictEqual(o.writes.length, 0); assert(!o.ids["port-error"].hidden);
  o.ids.port.value = "9614.5"; await o.ids.port.fire("change"); assert.strictEqual(o.writes.length, 0);
  o.ids.port.value = "19719"; await o.ids.port.fire("change"); assert.strictEqual(o.store.port, 19719); assert(o.ids["port-error"].hidden);
  o.ids.minSize.value = "-1"; await o.ids.minSize.fire("change"); assert(!o.ids["minimum-error"].hidden);
  o.ids.minSize.value = "0.5"; await o.ids.minSize.fire("change"); assert.strictEqual(o.store.minSizeKB, .5);
  o.ids.intercept.checked = false; await o.ids.intercept.fire("change"); assert.strictEqual(o.store.intercept, false);
  o.ids.bubbleScale.value = "150"; await o.ids.bubbleScale.fire("input"); assert.strictEqual(o.store.bubbleScale, 1.5); assert.strictEqual(o.ids.bubbleScaleText.textContent, "150%");
  await o.ids.bubbleScaleReset.fire("click"); assert.strictEqual(o.store.bubbleScale, 1);
  o = await page("options", { preparedPort: 19614, store: { port: 19719 } }); assert.strictEqual(o.ids.port.value, "19719", "explicit port overrides prepared default");
  o = await page("options", { preparedPort: 999999 }); assert.strictEqual(o.ids.port.value, "9614", "invalid prepared metadata falls back");
  console.log("extension UI contracts: download/playlist messages, safe labels, paused/offline/protected states, scale migration, settings validation and prepared-port precedence passed");
})().catch((error) => { console.error(error); process.exitCode = 1; });
