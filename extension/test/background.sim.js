// Runs background.js with a fake `chrome` API and a fake SwiftGet app.
// node extension/test/background.sim.js
const fs = require("fs"), path = require("path"), vm = require("vm"), assert = require("assert");
const dir = path.join(__dirname, "..");
const listeners = {}; const on = (n) => ({ addListener: (f) => (listeners[n] = f) });
const session = {}; const localStore = {}; let reloads = 0; let pingExt = null; const sent = []; const tabMsgs = []; let tabUrl = "https://www.youtube.com/watch?v=AAAAAAAAAAA";
const chrome = {
  storage: {
    onChanged: on("store"),
    local: { get: async (d) => (typeof d === "string" ? (d in localStore ? { [d]: localStore[d] } : {}) : { ...d, ...localStore }), set: async (o) => Object.assign(localStore, o) },
    session: {
      get: async (k) => { const ks = Array.isArray(k) ? k : [k]; const o = {}; for (const x of ks) if (x in session) o[x] = JSON.parse(JSON.stringify(session[x])); return o; },
      set: async (o) => Object.assign(session, JSON.parse(JSON.stringify(o))),
      remove: async (k) => { for (const x of [].concat(k)) delete session[x]; },
    },
  },
  downloads: { onCreated: on("dl"), cancel: async () => {}, erase: async () => {}, download: (o) => browserDl.push(o.url) },
  webRequest: { onHeadersReceived: on("headers"), onBeforeRequest: on("before") },
  tabs: { onUpdated: on("upd"), onRemoved: on("rm"), get: async (id) => ({ id, url: tabUrl }), query: async () => [{ id: 7 }],
          sendMessage: async (id, m) => tabMsgs.push(m) },
  action: { setBadgeBackgroundColor: () => {}, setBadgeText: async () => {} },
  runtime: { onInstalled: on("inst"), onStartup: on("start"), onMessage: on("msg"), getManifest: () => ({ version: "1.0.4" }), reload: () => { reloads++; } },
  contextMenus: { removeAll: () => {}, create: () => {}, onClicked: on("ctx") },
  cookies: { getAll: async () => [] },
};
const ytCalls = []; const browserDl = []; let appDown = false;
const fetch = async (url, opts = {}) => {
  if (url.endsWith("/ping") && pingExt) return { ok: true, status: 200, json: async () => ({ ext: pingExt }) };
  if (url.endsWith("/add")) { if (appDown) throw new TypeError("Failed to fetch"); sent.push(JSON.parse(opts.body)); }
  if (url.endsWith("/resolve")) {
    ytCalls.push(JSON.parse(opts.body));
    await new Promise((r) => setTimeout(r, 30)); // yt-dlp takes a moment
    return { ok: true, status: 200, json: async () => ({ ok: true, title: "Real: Title", options: [
      { key: "1080p|MP4", label: "1080p · MP4", sub: "94 MB", quality: "1080p", videoUrl: "https://gv/137", audioUrl: "https://gv/140", ext: "mp4", size: 94e6, headers: [["User-Agent", "YtDlpUA"]] },
      { key: "audio|M4A", label: "Audio only · M4A 130k", sub: "4 MB", quality: "audio", videoUrl: "https://gv/140", audioUrl: null, ext: "m4a", size: 4e6, headers: [] } ] }) };
  }
  return { ok: true, status: 200 };
};
const ctx = { chrome, fetch, AbortController, setTimeout, clearTimeout, navigator: { userAgent: "TestUA" }, console, URL,
  importScripts: (...f) => f.forEach((x) => vm.runInContext(fs.readFileSync(path.join(dir, x), "utf8"), ctx)) };
vm.createContext(ctx);
vm.runInContext(fs.readFileSync(path.join(dir, "background.js"), "utf8"), ctx);

const gv = (itag, mime, clen, dur, r) => `https://rr1---sn-x.googlevideo.com/videoplayback?expire=9&itag=${itag}&mime=${encodeURIComponent(mime)}&clen=${clen}&dur=${dur}&pot=T&range=${r}&rn=1`;
const tick = () => new Promise((r) => setTimeout(r, 20));

(async () => {
  // player loads an ad, then the real video in many pieces
  listeners.before({ tabId: 7, url: gv(136, "video/mp4", 5e6, "15.0", "0-1000") });
  await tick();
  for (const r of ["0-1000", "1001-2000", "2001-3000"]) {
    listeners.before({ tabId: 7, url: gv(137, "video/mp4", 150e6, "300.5", r) }); await tick();
    listeners.before({ tabId: 7, url: gv(140, "audio/mp4", 4e6, "300.5", r) }); await tick();
  }
  const last = tabMsgs[tabMsgs.length - 1];
  assert.strictEqual(last.type, "swiftget-media");
  assert.strictEqual(last.items[0].label, "1080p Full HD · MP4");
  assert.strictEqual(last.items[0].yt.tag, "Full HD");
  console.log("button shows:", last.items.map((i) => i.label).join(" | "));

  // user clicks 1080p on the page
  const res = await new Promise((resolve) =>
    listeners.msg({ type: "swiftget-download", item: last.items[0], title: "(2) Cool video - YouTube" }, { tab: { id: 7, url: tabUrl } }, resolve));
  assert.strictEqual(res.ok, true);
  const req = sent[0];
  assert(req.url.includes("itag=137") && !req.url.includes("range="));
  assert(req.audioUrl.includes("itag=140"));
  assert.strictEqual(req.fileName, "Cool video [1080p].mp4");
  assert.strictEqual(req.userAgent, "TestUA");
  console.log("sent to app:", req.fileName, "| audio:", !!req.audioUrl);

  // navigating to another video clears the list
  tabUrl = "https://www.youtube.com/watch?v=BBBBBBBBBBB";
  await listeners.upd(7, { url: tabUrl }, { id: 7, url: tabUrl }); await tick();
  const afterNav = tabMsgs[tabMsgs.length - 1].items;
  assert(!afterNav.some((i) => i.label === "1080p · MP4" && i.yt && i.yt.videoUrl.includes("itag=137")), "old video's items gone");

  // ---- SABR video: no usable stream URL, the app (yt-dlp) gives the qualities
  listeners.before({ tabId: 7, url: "https://rr1---sn-x.googlevideo.com/videoplayback?expire=9&sabr=1&ump=1" });
  await listeners.upd(7, { url: tabUrl, status: "complete" }, { id: 7, url: tabUrl });
  await new Promise((r) => setTimeout(r, 150));
  assert.strictEqual(ytCalls.length, 1, "asked the app once");
  assert.strictEqual(ytCalls[0].url, "https://www.youtube.com/watch?v=BBBBBBBBBBB");
  assert(Array.isArray(ytCalls[0].cookieList), "sends the site's cookies as a list");
  const labels = tabMsgs.map((m) => (m.items[0] || {}).label);
  assert(labels.includes("Getting video qualities…"), "shows progress while resolving");
  const final = tabMsgs[tabMsgs.length - 1].items;
  assert.strictEqual(final[0].label, "1080p · MP4");
  const res2 = await new Promise((resolve) =>
    listeners.msg({ type: "swiftget-download", item: final[0], title: "ignored - YouTube" }, { tab: { id: 7, url: tabUrl } }, resolve));
  assert.strictEqual(res2.ok, true);
  const req2 = sent[sent.length - 1];
  assert.strictEqual(req2.fileName, "Real_ Title [1080p].mp4");
  assert.strictEqual(req2.sourcePage, "https://www.youtube.com/watch?v=BBBBBBBBBBB");
  assert.strictEqual(req2.sourceKey, "1080p|MP4");
  assert.strictEqual(req2.audioUrl, "https://gv/140");
  assert.strictEqual(req2.headers[0][1], "YtDlpUA");
  console.log("SABR video via app:", req2.fileName, "| key:", req2.sourceKey);

  // ---- any other site: a reel in an Instagram feed
  tabUrl = "https://www.instagram.com/";
  await listeners.upd(8, { url: tabUrl, status: "loading" }, { id: 8, url: tabUrl });
  tabMsgs.length = 0;
  listeners.msg({ type: "swiftget-video-seen", page: "https://www.instagram.com/reel/C1abcDEF/" }, { tab: { id: 8, url: tabUrl } }, () => {});
  await new Promise((r) => setTimeout(r, 150));
  const igCall = ytCalls[ytCalls.length - 1];
  assert.strictEqual(igCall.url, "https://www.instagram.com/reel/C1abcDEF/");
  const igItems = tabMsgs[tabMsgs.length - 1].items;
  assert.strictEqual(igItems[0].label, "1080p · MP4");
  const res3 = await new Promise((resolve) =>
    listeners.msg({ type: "swiftget-download", item: igItems[0], title: "x" }, { tab: { id: 8, url: tabUrl } }, resolve));
  assert.strictEqual(res3.ok, true);
  const req3 = sent[sent.length - 1];
  assert.strictEqual(req3.sourcePage, "https://www.instagram.com/reel/C1abcDEF/");
  assert.strictEqual(req3.referrer, "https://www.instagram.com/reel/C1abcDEF/");
  console.log("Instagram reel via app:", req3.fileName);
  // browser downloads: only new ones, each once, no loop when the app fails
  const now = new Date().toISOString();
  const dl = async (o) => { await listeners.dl({ id: Math.random(), state: "in_progress", startTime: now, ...o }); await tick(); };
  let before = sent.length;
  await dl({ url: "https://example.com/old.pdf", startTime: new Date(Date.now() - 3600e3).toISOString() });
  await dl({ url: "https://example.com/done.pdf", state: "complete" });
  await dl({ url: "https://example.com/ext.pdf", byExtensionId: "other" });
  assert.strictEqual(sent.length, before, "restored / finished / extension downloads are left alone");
  await dl({ url: "https://example.com/new.pdf" });
  await dl({ url: "https://example.com/new.pdf" });
  assert.strictEqual(sent.length, before + 1, "a new download goes to the app once");
  appDown = true;
  await dl({ url: "https://example.com/fail.pdf" });
  assert.deepStrictEqual(browserDl, ["https://example.com/fail.pdf"], "given back to the browser once");
  await dl({ url: "https://example.com/fail.pdf", byExtensionId: "self" });
  await dl({ url: "https://example.com/fail.pdf" }); // same address right after: the browser keeps it
  assert.strictEqual(browserDl.length, 1, "no loop");
  appDown = false;
  console.log("browser downloads: restored ones ignored, no repeats, no loop");
  // add-in reloads itself once when the program brings a newer one
  pingExt = "1.0.5"; delete session.versionCheckAt;
  await ctx.followAppVersion(); await tick();
  assert.strictEqual(reloads, 1, "reloads for the new version");
  delete session.versionCheckAt;
  await ctx.followAppVersion(); await tick();
  assert.strictEqual(reloads, 1, "only once per version");
  pingExt = "1.0.4"; delete session.versionCheckAt;
  await ctx.followAppVersion();
  assert.strictEqual(reloads, 1, "same version: nothing");
  pingExt = null;
  console.log("self update: reloads once when the program has a newer add-in");
  // a player inside a frame (Dailymotion's "geo.dailymotion.com/player/…"):
  // the page around it is looked up, its qualities are offered
  {
    const page = "https://www.dailymotion.com/video/xac78kk";
    const n = ytCalls.length;
    const res = await new Promise((resolve) =>
      listeners.msg({ type: "swiftget-resolve-video", page: "", src: "", frame: true }, { tab: { id: 21, url: page }, frameId: 4 }, resolve));
    assert(res.items && res.items.length >= 2, "qualities of the page: " + JSON.stringify(res));
    assert.strictEqual(ytCalls[n].url, page, "the page, not the player frame");
    assert(!ytCalls.slice(n).some((c) => /geo\.dailymotion\.com/.test(c.url)));
    console.log("player in a frame: uses the page's qualities");
  }
  // the on / off switch of the add-in's window
  {
    localStore.enabled = false;
    listeners.store({ enabled: { newValue: false } }, "local");
    const before = sent.length;
    await dl({ url: "https://example.com/while-off.zip" });
    assert.strictEqual(sent.length, before, "off: the browser keeps its downloads");
    const cfg = await new Promise((r) => listeners.msg({ type: "swiftget-config" }, { tab: { id: 7, url: tabUrl } }, r));
    assert.strictEqual(cfg.enabled, false, "off: no buttons on videos");
    const n = tabMsgs.length;
    await ctx.pushToTab(7);
    assert.strictEqual(tabMsgs.slice(n).pop().items.length, 0, "off: nothing offered on the page");
    localStore.enabled = true;
    await dl({ url: "https://example.com/while-on.zip" });
    assert.strictEqual(sent.length, before + 1, "on again: back to the app");
    console.log("on / off switch: off leaves everything to the browser");
  }
  console.log("background simulation: all checks passed");
})().catch((e) => { console.error(e); process.exit(1); });
