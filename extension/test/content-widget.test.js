// Behavior tests for the content-script widget, using a small DOM test double.
// No browser profile, cookies, network traffic, or third-party packages.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '..', 'content.js'), 'utf8');

function fixture({ url = 'https://example.test/watch', items = formats(), appRunning = true } = {}) {
  const events = new Map(), intervals = new Map(), timeouts = [], messages = [], storageListeners = [], runtimeListeners = [];
  let document;
  const addGlobal = (type, fn) => { if (!events.has(type)) events.set(type, new Set()); events.get(type).add(fn); };
  const emitGlobal = (type, e = {}) => { for (const fn of [...(events.get(type) || [])]) fn(event(e)); };
  function event(data) { return { button: 0, detail: 1, preventDefault() { this.defaultPrevented = true; }, stopPropagation() {}, composedPath() { return []; }, ...data }; }
  function matches(node, selector) {
    const enabled = selector.includes(':not(:disabled)');
    selector = selector.replace(':not(:disabled)', '');
    if (enabled && node.disabled) return false;
    if (selector === '*') return true;
    if (selector.startsWith('.')) return node.classList.contains(selector.slice(1));
    if (selector.startsWith('#')) return node.id === selector.slice(1);
    return node.tagName.toLowerCase() === selector.toLowerCase();
  }
  class Element {
    constructor(tag) {
      this.tagName = tag.toUpperCase(); this.children = []; this.attributes = {}; this.parentNode = null;
      this.style = { setProperty(k, v) { this[k] = v; } }; this.hidden = false; this.disabled = false; this.scrollTop = 0;
      this.listeners = new Map(); this._text = ''; this.tabIndex = -1;
      this.classList = {
        contains: (c) => (this.attributes.class || '').split(/\s+/).includes(c),
        add: (c) => { if (!this.classList.contains(c)) this.attributes.class = ((this.attributes.class || '') + ' ' + c).trim(); },
        remove: (c) => { this.attributes.class = (this.attributes.class || '').split(/\s+/).filter(x => x !== c).join(' '); },
        toggle: (c, v) => { const want = v === undefined ? !this.classList.contains(c) : v; want ? this.classList.add(c) : this.classList.remove(c); return want; },
      };
    }
    get id() { return this.attributes.id; }
    setAttribute(k, v) { this.attributes[k] = String(v); }
    getAttribute(k) { return this.attributes[k] ?? null; }
    set textContent(v) { this._text = String(v); this.replaceChildren(); }
    get textContent() { return this._text + this.children.map(x => x.textContent).join(''); }
    get isConnected() { return this === document.documentElement || !!this.parentNode?.isConnected; }
    get scrollHeight() { return 320; }
    append(...nodes) { for (const n of nodes) this.appendChild(n); }
    appendChild(node) { node.remove(); this.children.push(node); node.parentNode = this; return node; }
    remove() { if (this.parentNode) this.parentNode.children = this.parentNode.children.filter(n => n !== this); this.parentNode = null; }
    replaceChildren(...nodes) { for (const n of this.children) n.parentNode = null; this.children = []; this.append(...nodes); }
    contains(node) { return node === this || this.children.some(c => c.contains(node)); }
    querySelectorAll(selector) {
      const selectors = selector.split(',').map(x => x.trim()); const found = [];
      const walk = (node) => { for (const child of node.children) { if (selectors.some(s => matches(child, s))) found.push(child); walk(child); } };
      walk(this); return found;
    }
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
    getElementById(id) { return this.querySelector('#' + id); }
    addEventListener(type, fn) { if (!this.listeners.has(type)) this.listeners.set(type, []); this.listeners.get(type).push(fn); }
    emit(type, data = {}) { const e = event({ target: this, ...data }); for (const f of this.listeners.get(type) || []) f(e); return e; }
    click() { if (!this.disabled) this.emit('click'); }
    focus() {
      let node = this; while (node && !node.host) node = node.parentNode;
      if (node) { node.activeElement = this; document.activeElement = node.host; }
    }
    attachShadow() { const root = new Element('shadowroot'); root.host = this; this._shadow = root; return root; }
    closest() { return null; }
    getBoundingClientRect() {
      if (this.tagName === 'VIDEO') return { left: 100, top: 80, right: 740, bottom: 440, width: 640, height: 360 };
      let node = this; while (node.parentNode) node = node.parentNode;
      const host = node.host || this; const z = Number(host.style['--z']) || 1;
      const left = parseFloat(host.style.left) || 500, top = parseFloat(host.style.top) || 88;
      const width = this.classList.contains('list') ? Math.min(340, parseFloat(this.style.maxWidth) || 340) * z : 220 * z;
      const height = this.classList.contains('list') ? Math.min(320, parseFloat(this.style.maxHeight) || 320) * z : 34 * z;
      return { left, top, right: left + width, bottom: top + height, width, height };
    }
  }
  document = new Element('document'); document.documentElement = new Element('html'); document.documentElement.parentNode = document;
  document.title = 'A safe <video> title'; document.fullscreenElement = null;
  document.createElement = tag => new Element(tag); document.createElementNS = (_, tag) => new Element(tag);
  document.querySelectorAll = selector => document.documentElement.querySelectorAll(selector);
  const video = new Element('video'); Object.assign(video, { duration: 60, paused: false, ended: false, currentSrc: 'https://cdn.example.test/clip.mp4', src: '', tabIndex: 0 }); document.documentElement.appendChild(video);
  const chrome = {
    storage: { local: { get(defaults, cb) { cb(defaults); } }, onChanged: { addListener(fn) { storageListeners.push(fn); } } },
    runtime: {
      lastError: null,
      onMessage: { addListener(fn) { runtimeListeners.push(fn); } },
      sendMessage(message, cb) {
        messages.push(message);
        const response = message.type === 'swiftget-config' ? { enabled: true, skipSites: [] }
          : message.type === 'swiftget-media-request' ? items
          : message.type === 'swiftget-resolve-video' ? { items }
          : { ok: appRunning };
        cb?.(response); return Promise.resolve(response);
      },
    },
  };
  const ctx = { document, chrome, location: new URL(url), innerWidth: 1000, innerHeight: 720, URL, console,
    addEventListener: addGlobal, removeEventListener(type, fn) { events.get(type)?.delete(fn); },
    setTimeout(fn) { timeouts.push(fn); return timeouts.length; }, clearTimeout() {},
    setInterval(fn, ms) { if (!intervals.has(ms)) intervals.set(ms, []); intervals.get(ms).push(fn); return fn; },
    clearInterval(fn) { for (const [ms, list] of intervals) intervals.set(ms, list.filter(f => f !== fn)); },
    getSelection() { return null; },
  };
  ctx.window = ctx; ctx.top = ctx;
  vm.runInNewContext(source, ctx, { filename: 'content.js' });
  const hosts = () => document.documentElement.children.filter(x => x._shadow);
  const root = () => hosts()[0]?._shadow;
  return { ctx, video, document, messages, events, root, hosts,
    send(message) { for (const fn of runtimeListeners) fn(message, {}, () => {}); },
    change(change) { for (const fn of storageListeners) fn(change, 'local'); },
    tick(ms) { for (const fn of intervals.get(ms) || []) fn(); },
    flush() { while (timeouts.length) timeouts.shift()(); },
    global: emitGlobal,
  };
}
function formats() {
  return [
    { label: '1080p Full HD · MP4', ext: 'mp4', yt: { key: '1080p|MP4', quality: '1080p', ext: 'mp4', tag: 'Full HD', codec: 'AV1', size: 30000000, duration: 60, title: 'A safe <video> title' } },
    { label: 'Audio · M4A', ext: 'm4a', yt: { key: 'audio|M4A', quality: 'audio', ext: 'm4a', size: 4000000, duration: 60 } },
    { label: 'Unavailable format', sub: 'Not available', disabled: true },
  ];
}

{
  const f = fixture(); const root = f.root(); const trigger = root.getElementById('grab'); const box = root.getElementById('list');
  assert.equal(trigger.tagName, 'BUTTON'); assert.equal(trigger.getAttribute('aria-haspopup'), 'dialog');
  assert.equal(box.children.length, 0, 'closed widget does not build hidden format rows');
  trigger.emit('click', { detail: 0 });
  assert.equal(trigger.getAttribute('aria-expanded'), 'true'); assert.equal(box.getAttribute('role'), 'dialog');
  assert.deepEqual(root.querySelectorAll('h3').map(n => n.textContent), ['Video', 'Audio']);
  assert(root.querySelectorAll('.item').every(n => n.tagName === 'BUTTON'));
  assert.equal(root.querySelectorAll('.item').filter(n => n.disabled).length, 1);
  assert.equal(root.activeElement.getAttribute('data-choice'), '1080p|MP4');
  root.emit('keydown', { key: 'ArrowDown' }); assert.equal(root.activeElement.getAttribute('data-choice'), 'audio|M4A');
  const updated = [{ label: '4K · WEBM', ext: 'webm', yt: { key: '2160p|WEBM', quality: '2160p', ext: 'webm' } }, ...formats()];
  f.send({ type: 'swiftget-media', items: updated });
  assert.equal(root.activeElement.getAttribute('data-choice'), 'audio|M4A', 'async updates preserve the focused format');
  root.emit('keydown', { key: 'Home' }); assert.equal(root.activeElement.getAttribute('data-choice'), '2160p|WEBM');
  root.emit('keydown', { key: 'End' }); assert.equal(root.activeElement.getAttribute('data-choice'), 'audio|M4A');
  root.emit('keydown', { key: 'Escape' }); assert.equal(trigger.getAttribute('aria-expanded'), 'false'); assert.equal(root.activeElement, trigger);
  trigger.emit('click', { detail: 0 }); root.querySelectorAll('.item:not(:disabled)')[0].click();
  const sent = f.messages.find(m => m.type === 'swiftget-download'); assert.equal(sent.item.yt.key, '2160p|WEBM');
  assert.equal(sent.referrer, f.ctx.location.href); assert.equal(sent.title, f.document.title);
  assert(root.getElementById('list').textContent.includes('Sent to Ezoo Download.'));
  assert.equal(root.querySelectorAll('img').length, 0, 'page titles are plain text, not markup');
}
{
  const f = fixture(); const root = f.root(), trigger = root.getElementById('grab');
  trigger.emit('mousedown', { clientX: 600, clientY: 100 });
  f.global('mousemove', { clientX: 450, clientY: 180 }); f.global('mouseup'); trigger.emit('click', { detail: 1 });
  assert.equal(trigger.getAttribute('aria-expanded'), 'false', 'a drag does not also open the format picker');
  f.flush(); trigger.emit('click', { detail: 1 }); assert.equal(trigger.getAttribute('aria-expanded'), 'true');
  root.getElementById('close').click(); assert.equal(f.hosts().length, 0);
  f.send({ type: 'swiftget-open-panel' }); assert.equal(f.hosts().length, 1, 'context menu can restore a hidden widget');
  assert.equal(f.events.get('mousemove').size, 1);
  f.video.remove(); f.tick(700);
  assert.equal(f.hosts().length, 0); assert.equal(f.events.get('mousemove').size, 0); assert.equal(f.events.get('mouseup').size, 0);
}
{
  const f = fixture(); f.change({ enabled: { newValue: false } });
  assert.equal(f.hosts().length, 0); assert.equal(f.events.get('mousemove').size, 0, 'disabling releases per-video handlers');
}
{
  const f = fixture({ url: 'https://www.netflix.com/watch/12345' }); const root = f.root();
  assert.equal(root.getElementById('label').textContent, 'Protected video'); root.getElementById('grab').click();
  assert(root.getElementById('list').textContent.includes('Netflix playback is not supported'));
  assert.equal(root.querySelectorAll('.item').length, 0);
  f.send({ type: 'swiftget-open-panel' });
  assert.equal(f.messages.filter(m => /resolve-video|download|playlist/.test(m.type)).length, 0, 'protected playback is not offered as a fragment download');
}
{
  const f = fixture({ url: 'https://www.netflix.com/title/12345' }); f.root().getElementById('grab').click();
  assert(f.root().querySelectorAll('.item').length > 0, 'Netflix public/general pages are not blanket-blocked');
}
{
  const f = fixture({ url: 'https://player.example.test/embed', items: [{ protected: true, disabled: true, type: 'protected', label: 'Protected video', url: null }] });
  f.root().getElementById('grab').click();
  assert.equal(f.root().getElementById('label').textContent, 'Protected video', 'a backend-protected embed also shows the explanation');
  assert.equal(f.root().querySelectorAll('.item').length, 0);
  assert.equal(f.messages.filter(m => /resolve-video|download|playlist/.test(m.type)).length, 0);
}
for (const url of ['https://notnetflix.com/watch/12345', 'https://netflix.com.example.test/watch/12345', 'https://www.netflix.com/watching/12345']) {
  const f = fixture({ url }); f.root().getElementById('grab').click();
  assert(f.root().querySelectorAll('.item').length > 0, 'Netflix guard requires its real hostname and watch path: ' + url);
}
console.log('content widget: keyboard navigation, focus, messages, drag/hide cleanup and protected-video guidance passed');
