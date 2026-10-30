// Ezoo Download browser integration (Manifest V3 service worker)
//  1. takes over browser downloads and hands them to the Ezoo Download app
//     together with cookies + referrer, so links behind a login still work
//  2. watches network responses for video/audio and HLS streams and offers
//     a "Download this video" button on the page
//  3. Videos on web pages: the app finds the qualities with yt-dlp,
//     downloads video + audio with many connections and joins them
//  4. adds "Download with Ezoo Download", "Download all links with Ezoo
//     Download" and "Download video with Ezoo Download" to the right-click menu
//  5. follows the app's Options: file types to take, sites to leave alone,
//     the keys that skip / force Ezoo Download, the video panel

// Chrome / Edge / Opera load yt-core.js here; Firefox lists it in its manifest
if (typeof importScripts === "function") importScripts("yt-core.js");

// enabled: the on / off switch of the add-in's window (off = the browser
// keeps its downloads and no buttons are shown on videos)
const DEFAULTS = { port: 9614, intercept: true, minSizeKB: 0, enabled: true };

async function getOpts() {
  return Object.assign({}, DEFAULTS, await chrome.storage.local.get(DEFAULTS));
}

function apiUrl(port, path) {
  return `http://127.0.0.1:${port}${path}`;
}

async function fetchWithTimeout(url, opts = {}, ms = 1500) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), ms);
  try {
    return await fetch(url, { ...opts, signal: ctrl.signal });
  } finally {
    clearTimeout(timer);
  }
}

// ---------------------------------------------------------------- self update
// The add-in is loaded from the program's own folder. When the program
// updates itself it brings newer add-in files along; the browser only reads
// them after a reload, so the add-in reloads itself once per new version.
async function followAppVersion() {
  const last = (await chrome.storage.session.get("versionCheckAt")).versionCheckAt || 0;
  if (Date.now() - last < 60000) return;
  await chrome.storage.session.set({ versionCheckAt: Date.now() });
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(apiUrl(port, "/ping"), { headers: { "X-SwiftGet": "1" } }, 800);
    if (!r.ok || typeof r.json !== "function") return;
    const want = (await r.json()).ext;
    const have = chrome.runtime.getManifest().version;
    if (!want || want === have) return;
    // only once per version: if the files on disk did not change (the add-in
    // was loaded from somewhere else) it must not keep reloading
    const done = (await chrome.storage.local.get("reloadedFor")).reloadedFor;
    if (done === want) return;
    await chrome.storage.local.set({ reloadedFor: want });
    chrome.runtime.reload();
  } catch {
    /* the program is not running */
  }
}
followAppVersion();

async function appAlive() {
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(apiUrl(port, "/ping"), { headers: { "X-SwiftGet": "1" } }, 800);
    if (r.ok) followAppVersion();
    return r.ok;
  } catch {
    return false;
  }
}

async function cookieHeader(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

/** The site's cookies for the app, only used if the site asks for a login. */
async function cookieList(url) {
  try {
    const list = await chrome.cookies.getAll({ url });
    return list.map((c) => ({
      domain: c.domain,
      name: c.name,
      value: c.value,
      path: c.path || "/",
      secure: !!c.secure,
      expirationDate: c.expirationDate || null,
    }));
  } catch {
    return [];
  }
}

function isYoutube(url) {
  try {
    return /(^|\.)youtube\.com$|^youtu\.be$/i.test(new URL(url).hostname);
  } catch {
    return false;
  }
}

async function sendToApp({ url, audioUrl, referrer, fileName, headers, sourcePage, sourceKey }) {
  const { port } = await getOpts();
  const body = {
    url,
    audioUrl: audioUrl || null,
    headers: headers || null,
    sourcePage: sourcePage || null,
    sourceKey: sourceKey || null,
    referrer: referrer || null,
    cookies: await cookieHeader(url),
    userAgent: navigator.userAgent,
    fileName: fileName || null,
  };
  const r = await fetchWithTimeout(
    apiUrl(port, "/add"),
    {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-SwiftGet": "1" },
      body: JSON.stringify(body),
    },
    3000
  );
  if (!r.ok) throw new Error(`Ezoo Download answered HTTP ${r.status}`);
}

// ---------------------------------------------------------------- app options

let appConfig = null;
let configAt = 0;

/** Options > File Types / General of the app (cached for a minute). */
async function getConfig() {
  if (appConfig && Date.now() - configAt < 60000) return appConfig;
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(apiUrl(port, "/config"), { headers: { "X-SwiftGet": "1" } }, 1000);
    if (r.ok) {
      appConfig = await r.json();
      configAt = Date.now();
    }
  } catch {
    /* app not running or older version: keep the last one */
  }
  return appConfig;
}

/** "*" matches anything, "?" one character (case-insensitive). */
function wildcard(pattern, text) {
  const re = new RegExp(
    "^" + pattern.toLowerCase().replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$"
  );
  return re.test(text.toLowerCase());
}

function hostMatches(host, pattern) {
  const p = pattern.replace(/^https?:\/\//i, "").split("/")[0].toLowerCase();
  if (!p) return false;
  if (p.includes("*")) return wildcard(p, host);
  return host === p || host.endsWith("." + p);
}

function extensionOf(name) {
  const clean = (name || "").split(/[?#]/)[0];
  const last = clean.split(/[\\/]/).pop() || "";
  const dot = last.lastIndexOf(".");
  return dot > 0 ? last.slice(dot + 1).toLowerCase() : "";
}

/** Should Ezoo Download take this browser download? (Options > File Types rules) */
function wanted(cfg, url, fileName, mime, keys) {
  if (!cfg) return true; // old app: take everything like before
  if (cfg.skipKey && cfg.skipKey !== "none" && keys[cfg.skipKey]) return false;
  if (cfg.forceKey && cfg.forceKey !== "none" && keys[cfg.forceKey]) return true;
  let host = "";
  try {
    host = new URL(url).hostname.toLowerCase();
  } catch {
    return false;
  }
  if ((cfg.skipSites || []).some((s) => hostMatches(host, s))) return false;
  if ((cfg.skipAddresses || []).some((a) => wildcard(a, url))) return false;
  const exts = cfg.captureExts || [];
  if (!exts.length) return true;
  let ext = extensionOf(fileName) || extensionOf(new URL(url).pathname);
  // script addresses (download.php?id=…) say nothing about the file
  if (/^(php\d?|aspx?|jsp|cgi|pl|cfm|do|action|html?)$/.test(ext) && !extensionOf(fileName)) ext = "";
  if (!ext) {
    // no extension (download.php?id=…): go by the type the server sends
    return !!mime && !/^(text\/html|image\/|text\/plain)/i.test(mime);
  }
  if (exts.some((e) => wildcard(e, ext))) return true;
  // an extension not in the list, but the server says it is a file to
  // download (not a page, picture or text the browser shows)
  return !!mime && /^(application\/(octet-stream|x-|zip|vnd\.|pdf|java-archive|gzip)|video\/|audio\/)/i.test(mime) && !/^(html?|xhtml|php\d?|aspx?|jsp|json|xml|js|css|txt)$/.test(ext);
}

// keys held when the user clicked the link (sent by the page)
let lastKeys = { alt: false, ctrl: false, shift: false, at: 0 };
function currentKeys() {
  return Date.now() - lastKeys.at < 4000 ? lastKeys : { alt: false, ctrl: false, shift: false };
}

function baseName(path) {
  if (!path) return null;
  const name = path.split(/[\\/]/).pop();
  return name || null;
}

// ---------------------------------------------------------------- 1. downloads

// Remembered in session storage, not in memory: the browser stops this
// service worker whenever it likes, and a lost "let the browser keep it"
// mark made the same file come back over and over.
const BYPASS_KEY = "bypassUrls";
const RECENT_KEY = "recentSends";

async function takeBypass(url) {
  const list = (await chrome.storage.session.get(BYPASS_KEY))[BYPASS_KEY] || [];
  const now = Date.now();
  const fresh = list.filter((b) => now - b.at < 120000);
  const hit = fresh.some((b) => b.url === url);
  await chrome.storage.session.set({ [BYPASS_KEY]: fresh.filter((b) => b.url !== url) });
  return hit;
}

async function addBypass(url) {
  const list = (await chrome.storage.session.get(BYPASS_KEY))[BYPASS_KEY] || [];
  list.push({ url, at: Date.now() });
  await chrome.storage.session.set({ [BYPASS_KEY]: list.slice(-50) });
}

/** The same address handed to the app a moment ago (pages and the browser can fire twice). */
async function sentRecently(url) {
  const list = (await chrome.storage.session.get(RECENT_KEY))[RECENT_KEY] || [];
  const now = Date.now();
  const fresh = list.filter((r) => now - r.at < 15000);
  const hit = fresh.some((r) => r.url === url);
  if (!hit) fresh.push({ url, at: now });
  await chrome.storage.session.set({ [RECENT_KEY]: fresh.slice(-50) });
  return hit;
}

/** The app's "Download with the browser" button: leave this one to the browser. */
async function appSaysBrowser(urls) {
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(
      apiUrl(port, "/browser"),
      { method: "POST", headers: { "Content-Type": "application/json", "X-SwiftGet": "1" }, body: JSON.stringify({ urls }) },
      800
    );
    if (!r.ok || typeof r.json !== "function") return false;
    return !!(await r.json()).browser;
  } catch {
    return false;
  }
}

/** Only downloads the user starts now: not ones the browser restores or resumes at start-up. */
function isNewDownload(item) {
  if (item.state && item.state !== "in_progress") return false;
  // ours (given back to the browser) and other extensions' downloads stay in the browser
  if (item.byExtensionId) return false;
  const started = Date.parse(item.startTime || "");
  if (!Number.isNaN(started) && Date.now() - started > 30000) return false;
  return true;
}

/** Should Ezoo Download take this browser download? */
async function shouldTake(item) {
  const opts = await getOpts();
  if (!opts.enabled || !opts.intercept) return false;
  const url = item.finalUrl || item.url;
  if (!url || !/^https?:/i.test(url)) return false; // blob:, data:, file: stay in the browser
  if (!isNewDownload(item)) return false;
  if (await takeBypass(url)) return false;
  // already handed to the app by the click on its link: the browser's copy goes
  if ((await justTaken(item.url)) || (await justTaken(url))) return true;
  if (opts.minSizeKB > 0 && item.totalBytes > 0 && item.totalBytes < opts.minSizeKB * 1024) return false;
  if (!(await appAlive())) return false; // app not running: let the browser handle it
  const cfg = await getConfig();
  if (!wanted(cfg, url, baseName(item.filename), item.mime, currentKeys())) return false;
  if (await appSaysBrowser([item.url, url])) return false; // the user chose the browser for it
  return true;
}

/** Take it from the browser and give it to the app. */
async function takeOver(item) {
  const url = item.finalUrl || item.url;
  try {
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch (_) {
    /* already finished or gone */
  }
  if (await sentRecently(url)) return;
  try {
    await sendToApp({ url, referrer: item.referrer, fileName: baseName(item.filename) });
  } catch (e) {
    // the app may have got it anyway (it was only slow to answer): never
    // hand it to the browser as well, that showed the file twice
    if (e && e.name === "AbortError") return;
    // the app could not be reached: give the download back to the browser, once
    await addBypass(url);
    chrome.downloads.download({ url });
  }
}

/** A file link the page caught a click on: to the app, or back to the page. */
async function takeLink(url, referrer) {
  if (!/^https?:/i.test(url || "")) throw new Error("not a web address");
  const opts = await getOpts();
  if (!opts.enabled || !opts.intercept) throw new Error("sending is off");
  if (!(await appAlive())) throw new Error("Ezoo Download is not running");
  if (await appSaysBrowser([url])) throw new Error("the browser keeps it");
  // the same link clicked twice in a row: once is enough
  if (!(await sentRecently(url))) await sendToApp({ url, referrer });
  // the browser may still start it (a second handler of the page): not again
  await addBypassTaken(url);
  return true;
}

// links just handed over from a click: if the browser starts them anyway, they are dropped
const TAKEN_KEY = "takenLinks";
async function addBypassTaken(url) {
  const list = (await chrome.storage.session.get(TAKEN_KEY))[TAKEN_KEY] || [];
  list.push({ url, at: Date.now() });
  await chrome.storage.session.set({ [TAKEN_KEY]: list.slice(-50) });
}
async function justTaken(url) {
  const list = (await chrome.storage.session.get(TAKEN_KEY))[TAKEN_KEY] || [];
  return list.some((l) => l.url === url && Date.now() - l.at < 15000);
}

// Chrome, Edge, Brave, Opera: decided before the browser asks where to
// save it, so its own "Save as" window never shows up for files the app
// takes. Firefox has no such moment: there it is decided when it starts.
const DECIDE_EARLY = !!(chrome.downloads.onDeterminingFilename && chrome.downloads.onDeterminingFilename.addListener);
if (DECIDE_EARLY) {
  chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
    let answered = false;
    const keep = () => {
      if (!answered) {
        answered = true;
        try {
          suggest();
        } catch (_) {
          /* the download is gone */
        }
      }
    };
    // never leave the browser waiting long
    const guard = setTimeout(keep, 4000);
    shouldTake(item)
      .then((take) => {
        clearTimeout(guard);
        if (take && !answered) {
          answered = true;
          return takeOver(item);
        }
        keep();
      })
      .catch(() => {
        clearTimeout(guard);
        keep();
      });
    return true; // answered later
  });
}

chrome.downloads.onCreated.addListener(async (item) => {
  if (DECIDE_EARLY) return;
  if (await shouldTake(item)) await takeOver(item);
});

// ---------------------------------------------------------------- per-tab state
// media_<tab>: generic files found on the page
// yt_<tab>:    { videoId, sabr, streams: { "<itag>_<dur>": stream }, resolved... }
// res_<tab>:   any other site: { page, resolving, resolved, resolveError }

async function readTab(tabId) {
  const keys = [`media_${tabId}`, `yt_${tabId}`, `res_${tabId}`];
  const r = await chrome.storage.session.get(keys);
  return { media: r[keys[0]] || [], yt: r[keys[1]] || null, res: r[keys[2]] || null };
}

function optionItems(resolved, source) {
  return resolved.options.map((o) => ({
    label: o.label,
    sub: o.sub,
    yt: { ...o, title: resolved.title, source },
  }));
}

/** Everything the page button and the popup show, in one list. */
async function itemsForTab(tabId) {
  const { media, yt, res } = await readTab(tabId);
  const items = [];
  if (res && !yt) {
    if (res.resolved && res.resolved.options.length) {
      items.push(...optionItems(res.resolved, res.page));
    } else if (res.resolving) {
      items.push({ label: "Getting video qualities…", sub: "This takes a few seconds", disabled: true });
    } else if (res.resolveError && !media.length) {
      // the page itself is no video page the app knows: the streams the
      // player opens show up here as soon as it plays
      const unknown = /unsupported url|no video formats|unable to extract/i.test(res.resolveError);
      items.push(
        unknown
          ? { label: "Play the video for a moment", sub: "Its qualities appear here as soon as it plays", disabled: true }
          : { label: "Can't get this video", sub: res.resolveError, disabled: true }
      );
    }
  }
  if (yt) {
    if (yt.resolved && yt.resolved.options.length) {
      // qualities found by the app (yt-dlp): works with every stream format
      items.push(...optionItems(yt.resolved, `https://www.youtube.com/watch?v=${yt.videoId}`));
    } else {
      // fallback: streams captured from the player itself
      for (const o of buildOptions(yt.streams, appConfig && appConfig.mp3Kbps)) items.push({ label: o.label, sub: o.sub, yt: o });
      if (!items.length) {
        if (yt.resolving) {
          items.push({ label: "Getting video qualities…", sub: "This takes a few seconds", disabled: true });
        } else if (yt.resolveError) {
          items.push({ label: "Can't get this video", sub: yt.resolveError, disabled: true });
        } else if (yt.sabr) {
          items.push({
            label: "This video needs Ezoo Download",
            sub: "Open Ezoo Download (with yt-dlp.exe next to it) and reload the page",
            disabled: true,
          });
        }
      }
    }
  }
  const resolvedFound = items.some((i) => i.yt);
  let hlsAsked = false;
  for (const m of media) {
    // when the app already found the real qualities, raw streams are noise
    if (resolvedFound && /HLS|DASH|video/i.test(m.type)) continue;
    if (m.dash) {
      // its qualities are read by the app when it is clicked
      items.push({ label: "Video stream (DASH) · choose a quality", sub: m.url, resolve: m.url, ext: "mp4" });
      continue;
    }
    if (m.type === "HLS stream") {
      // a quality list of a stream whose qualities are listed already
      if (m.child) continue;
      if (m.master && !m.master.separateAudio) {
        // every quality of the stream, read from its playlist
        for (const v of m.master.variants) {
          items.push({ label: `${hlsLabel(v)} · MP4 · stream`, sub: v.url, url: v.url, ext: "ts" });
        }
        continue;
      }
      if (m.master) {
        // picture and sound come in separate lists: the app puts them together
        items.push({
          label: `${m.master.variants.map(hlsLabel).slice(0, 4).join(" / ")} · choose a quality`,
          sub: m.url,
          resolve: m.url,
          ext: "mp4",
        });
        continue;
      }
      // saved as an MP4 file; the quality first when the address tells it
      items.push({ label: `${qualityOf(m.url) || "Video"} · MP4 · stream`, sub: m.url, url: m.url, ext: "ts" });
      if (!hlsAsked && !m.media) {
        hlsAsked = true;
        items.push({ label: "Other qualities of this stream…", sub: m.url, resolve: m.url, ext: "mp4" });
      }
      continue;
    }
    const q = qualityOf(m.url);
    items.push({
      label: `${q ? q + " · " : ""}${(m.ext || "").toUpperCase() || m.type}${m.size ? " · " + fmtSize(m.size) : ""}`,
      sub: m.url,
      url: m.url,
      ext: m.ext,
    });
  }
  return items;
}

/** "720p" when a stream's address says which quality it is. */
function qualityOf(url) {
  let path = "";
  try {
    path = new URL(url).pathname;
  } catch {
    return "";
  }
  const m = /(?:^|[^0-9])(4320|2160|1440|1080|720|576|540|480|360|240|144)p?(?=[^0-9]|$)/i.exec(path);
  return m ? `${m[1]}p` : "";
}

async function pushToTab(tabId) {
  const { enabled } = await getOpts();
  if (!enabled) {
    // switched off: no buttons on the page, "off" on the add-in's icon
    chrome.action.setBadgeBackgroundColor({ color: "#8a94a3" });
    chrome.action.setBadgeText({ tabId, text: "off" }).catch(() => {});
    chrome.tabs.sendMessage(tabId, { type: "swiftget-media", items: [] }).catch(() => {});
    return;
  }
  const items = await itemsForTab(tabId);
  const count = items.filter((i) => !i.disabled).length;
  chrome.action.setBadgeBackgroundColor({ color: "#1f6feb" });
  chrome.action.setBadgeText({ tabId, text: count ? String(count) : "" }).catch(() => {});
  chrome.tabs.sendMessage(tabId, { type: "swiftget-media", items }).catch(() => {});
}

async function clearTab(tabId) {
  await chrome.storage.session.remove([`media_${tabId}`, `yt_${tabId}`, `res_${tabId}`]);
  chrome.action.setBadgeText({ tabId, text: "" }).catch(() => {});
  chrome.tabs.sendMessage(tabId, { type: "swiftget-media", items: [] }).catch(() => {});
}

// ---------------------------------------------------------------- 2. media sniffer

const MEDIA_TYPE = /^(video|audio)\//i;
const HLS_TYPE = /mpegurl/i;
const DASH_TYPE = /dash\+xml/i;

function headerMap(list) {
  const out = {};
  for (const h of list || []) out[h.name.toLowerCase()] = h.value || "";
  return out;
}

async function addMedia(tabId, item) {
  const key = `media_${tabId}`;
  const current = (await chrome.storage.session.get(key))[key] || [];
  if (current.some((m) => m.url === item.url)) return;
  current.push(item);
  while (current.length > 15) current.shift();
  await chrome.storage.session.set({ [key]: current });
  pushToTab(tabId);
  if (item.type === "HLS stream") readHls(tabId, item.url).catch(() => {});
}

/**
 * An HLS stream: its playlist says which qualities it has (a "master" list
 * of one list per quality). Read once, so the button shows "1080p", "720p"...
 * instead of anonymous streams.
 */
function parseHlsMaster(text, base) {
  if (!/#EXT-X-STREAM-INF/i.test(text)) return null;
  const lines = text.split(/\r?\n/).map((l) => l.trim());
  const variants = [];
  // separate sound lists: a quality list alone would be without sound
  const separateAudio = lines.some((l) => /^#EXT-X-MEDIA:.*TYPE=AUDIO.*URI=/i.test(l));
  for (let i = 0; i < lines.length; i++) {
    if (!/^#EXT-X-STREAM-INF:/i.test(lines[i])) continue;
    let uri = "";
    for (let j = i + 1; j < lines.length; j++) {
      if (lines[j] && !lines[j].startsWith("#")) {
        uri = lines[j];
        break;
      }
    }
    if (!uri) continue;
    let url = "";
    try {
      url = new URL(uri, base).toString();
    } catch {
      continue;
    }
    const res = /RESOLUTION=(\d+)x(\d+)/i.exec(lines[i]);
    const bw = /[^-]BANDWIDTH=(\d+)/i.exec("," + lines[i]);
    variants.push({ url, height: res ? Number(res[2]) : 0, width: res ? Number(res[1]) : 0, bw: bw ? Number(bw[1]) : 0 });
  }
  if (!variants.length) return null;
  // one entry per quality, the best first
  variants.sort((a, b) => b.height - a.height || b.bw - a.bw);
  const seen = new Set();
  const unique = variants.filter((v) => {
    const k = v.height ? `h${v.height}` : `b${v.bw}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
  return { variants: unique, separateAudio };
}

async function readHls(tabId, url) {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), 8000);
  let text = "";
  try {
    const r = await fetch(url, { credentials: "include", signal: ctl.signal });
    if (!r.ok) return;
    text = (await r.text()).slice(0, 512 * 1024);
  } finally {
    clearTimeout(timer);
  }
  if (!/^\s*#EXTM3U/.test(text)) return;
  const master = parseHlsMaster(text, url);
  const key = `media_${tabId}`;
  const current = (await chrome.storage.session.get(key))[key] || [];
  const me = current.find((m) => m.url === url);
  if (!me) return;
  if (master) {
    me.master = master;
    // the quality lists the player opened by itself: shown under the master
    const inside = new Set(master.variants.map((v) => v.url));
    for (const m of current) if (m !== me && inside.has(m.url)) m.child = true;
  } else {
    me.media = true;
    if (current.some((m) => m.master && m.master.variants.some((v) => v.url === url))) me.child = true;
  }
  await chrome.storage.session.set({ [key]: current });
  pushToTab(tabId);
}

/** "1080p" (or the bit rate when the list does not say the size). */
function hlsLabel(v) {
  if (v.height) return `${v.height}p`;
  if (v.bw) return `${Math.round(v.bw / 1000)} kbps`;
  return "Video";
}

chrome.webRequest.onHeadersReceived.addListener(
  (d) => {
    if (d.tabId < 0 || d.statusCode < 200 || d.statusCode >= 300) return;
    const url = d.url;
    if (!/^https?:/i.test(url)) return;
    if (/googlevideo\.com/i.test(url)) return; // YouTube has its own handler below
    const h = headerMap(d.responseHeaders);
    const type = (h["content-type"] || "").toLowerCase();
    const isHls = HLS_TYPE.test(type) || /\.m3u8(\?|$)/i.test(url);
    // DASH: a list of the parts of a video in several qualities (.mpd)
    const isDash = DASH_TYPE.test(type) || /\.mpd(\?|$)/i.test(url);
    const isMedia = MEDIA_TYPE.test(type) && !type.includes("mp2t"); // skip HLS chunks
    if (!isHls && !isDash && !isMedia) return;
    if (isDash) {
      addMedia(d.tabId, { url, type: "DASH stream", size: 0, ext: "mp4", dash: true });
      return;
    }
    // byte-range pieces of a bigger stream can't be saved as a file
    if (/[?&](range|bytestart)=/i.test(url)) return;

    let size = 0;
    const cr = /\/(\d+)\s*$/.exec(h["content-range"] || "");
    if (cr) size = Number(cr[1]);
    else if (h["content-length"]) size = Number(h["content-length"]);
    if (isMedia && size > 0 && size < 300 * 1024) return; // tiny clips, ads, sounds

    addMedia(d.tabId, {
      url,
      type: isHls ? "HLS stream" : type.split(";")[0],
      size,
      ext: isHls ? "ts" : (type.split("/")[1] || "mp4").split(";")[0].replace("x-matroska", "mkv").replace("mpeg", "mp3"),
    });
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

// ---------------------------------------------------------------- 3. YouTube

// The player asks for many small pieces of the same stream; we only need to
// store each stream once per tab.
const seenStreams = new Map(); // tabId -> Set("<itag>_<dur>")

async function addYoutubeStream(tabId, stream) {
  const key = `yt_${tabId}`;
  const tab = await chrome.tabs.get(tabId).catch(() => null);
  const videoId = tab ? videoIdOf(tab.url) : null;
  const state = (await chrome.storage.session.get(key))[key] || { videoId, sabr: false, streams: {} };
  if (videoId && state.videoId && state.videoId !== videoId) {
    // the user moved to another video
    state.videoId = videoId;
    state.streams = {};
    state.sabr = false;
    seenStreams.delete(tabId);
  }
  state.videoId = state.videoId || videoId;
  if (stream.sabr) {
    if (state.sabr) return;
    state.sabr = true;
  } else {
    const sk = `${stream.itag}_${stream.dur}${stream.lang ? "_" + stream.lang : ""}`;
    // keep the newest URL of each stream (the old one may expire)
    state.streams[sk] = stream;
    const ids = Object.keys(state.streams);
    if (ids.length > 60) delete state.streams[ids[0]];
  }
  await chrome.storage.session.set({ [key]: state });
  pushToTab(tabId);
}

chrome.webRequest.onBeforeRequest.addListener(
  (d) => {
    if (d.tabId < 0) return;
    const stream = parseStream(d.url);
    if (!stream) return;
    const sk = stream.sabr ? "sabr" : `${stream.itag}_${stream.dur}${stream.lang ? "_" + stream.lang : ""}`;
    let seen = seenStreams.get(d.tabId);
    if (!seen) seenStreams.set(d.tabId, (seen = new Set()));
    if (seen.has(sk)) return;
    seen.add(sk);
    addYoutubeStream(d.tabId, stream);
  },
  { urls: ["*://*.googlevideo.com/videoplayback*"] }
);

async function downloadVideo(option, pageTitle, pageUrl) {
  const title = cleanYoutubeTitle(option.title || pageTitle);
  const name = option.quality === "mp3" ? `${title}.mp3` : `${title} [${option.quality}].${option.ext}`;
  await sendToApp({
    url: option.videoUrl,
    audioUrl: option.audioUrl,
    referrer: option.source && !isYoutube(option.source) ? option.source : pageUrl || "https://www.youtube.com/",
    fileName: name,
    headers: option.headers,
    sourcePage: option.source,
    sourceKey: option.key,
  });
}

async function sendPlaylist(url) {
  if (!/^https?:/i.test(url || "")) throw new Error("bad address");
  const { port } = await getOpts();
  const r = await fetchWithTimeout(
    apiUrl(port, "/playlist"),
    { method: "POST", headers: { "Content-Type": "application/json", "X-SwiftGet": "1" }, body: JSON.stringify({ url }) },
    5000
  );
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
}

// The on / off switch: every tab shows it at once.
chrome.storage.onChanged.addListener((ch, area) => {
  if (area !== "local" || !ch.enabled) return;
  chrome.action.setBadgeText({ text: ch.enabled.newValue === false ? "off" : "" }).catch(() => {});
  chrome.tabs.query({}).then((tabs) => tabs.forEach((t) => t.id != null && pushToTab(t.id))).catch(() => {});
});

// Ask the app (yt-dlp) for the qualities of a video page.
const resolving = new Map(); // tabId -> page being resolved

async function askApp(page, referer) {
  const { port } = await getOpts();
  const r = await fetchWithTimeout(
    apiUrl(port, "/resolve"),
    {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-SwiftGet": "1" },
      body: JSON.stringify({ url: page, cookieList: await cookieList(referer || page), referer: referer || null }),
    },
    90000
  );
  const data = await r.json();
  return data.ok
    ? { resolved: { title: data.title, site: data.site, options: data.options } }
    : { resolveError: data.error || "unknown error" };
}

async function patchYt(tabId, videoId, change) {
  const key = `yt_${tabId}`;
  let state = (await chrome.storage.session.get(key))[key];
  if (!state || state.videoId !== videoId) state = { videoId, sabr: false, streams: {} };
  Object.assign(state, change);
  await chrome.storage.session.set({ [key]: state });
  pushToTab(tabId);
}

async function resolveYoutube(tabId, pageUrl) {
  const videoId = videoIdOf(pageUrl);
  if (!videoId || !/(^|\.)youtube\.com$/i.test(new URL(pageUrl).hostname)) return;
  const key = `yt_${tabId}`;
  const state = (await chrome.storage.session.get(key))[key];
  if (state && state.videoId === videoId && (state.resolved || state.resolveError)) return;
  if (resolving.get(tabId) === videoId) return;
  resolving.set(tabId, videoId);
  await patchYt(tabId, videoId, { resolving: true, resolveError: null });
  let change;
  try {
    change = await askApp(`https://www.youtube.com/watch?v=${videoId}`);
  } catch (e) {
    change = { resolveError: (await appAlive()) ? String((e && e.message) || e) : "Ezoo Download is not running" };
  } finally {
    if (resolving.get(tabId) === videoId) resolving.delete(tabId);
  }
  // the user may have moved on to another video meanwhile
  const now = await chrome.tabs.get(tabId).catch(() => null);
  if (now && videoIdOf(now.url) === videoId) await patchYt(tabId, videoId, { resolving: false, ...change });
}

/**
 * Any other site: the page (or the post / reel / video link next to the
 * video the user is watching) is handed to yt-dlp, which knows Facebook,
 * Instagram, TikTok, X, Vimeo, Dailymotion and ~1800 more sites.
 */
async function resolvePage(tabId, page, embedded = false) {
  // YouTube's own pages have their own way (resolveYoutube); a YouTube
  // player embedded in another site goes this way
  if (!page || !/^https?:/i.test(page) || (isYoutube(page) && !embedded)) return;
  const key = `res_${tabId}`;
  const state = (await chrome.storage.session.get(key))[key];
  if (state && state.page === page && (state.resolved || state.resolving || state.resolveError)) return;
  if (resolving.get(tabId) === page) return;
  resolving.set(tabId, page);
  await chrome.storage.session.set({ [key]: { page, resolving: true } });
  pushToTab(tabId);
  let change;
  try {
    change = await askApp(page);
  } catch (e) {
    change = { resolveError: (await appAlive()) ? String((e && e.message) || e) : "Ezoo Download is not running" };
  } finally {
    if (resolving.get(tabId) === page) resolving.delete(tabId);
  }
  const now = (await chrome.storage.session.get(key))[key];
  if (now && now.page === page) {
    await chrome.storage.session.set({ [key]: { page, resolving: false, ...change } });
    pushToTab(tabId);
  }
}

/** The bubble of a video that is not the page's main one was clicked. */
async function resolveOneVideo(msg, tab) {
  // a DASH stream of the page: its own qualities
  if (msg.resolve && /^https?:/i.test(msg.resolve)) {
    try {
      const r = await askApp(msg.resolve, tab && tab.url);
      if (r.resolved && r.resolved.options.length) return { items: optionItems(r.resolved, msg.resolve) };
      return { error: r.resolveError || "Can't get this video" };
    } catch (e) {
      return { error: (await appAlive()) ? String((e && e.message) || e) : "Ezoo Download is not running" };
    }
  }
  // a player in a frame (Dailymotion, news sites...): what was found for the
  // whole page (its qualities, its streams) is what this player plays
  if (msg.frame && tab) {
    if (!msg.page && !isYoutube(tab.url)) {
      await resolvePage(tab.id, tab.url).catch(() => {});
      // it may have been started already (the player was seen): wait for it
      for (let i = 0; i < 180; i++) {
        const { res } = await readTab(tab.id);
        if (!res || !res.resolving) break;
        await new Promise((r) => setTimeout(r, 500));
      }
    }
    const found = (await itemsForTab(tab.id)).filter((i) => !i.disabled);
    if (found.length) return { items: found };
  }
  const items = [];
  if (msg.feed && !msg.page) {
    // Facebook feed and no link next to this video: the page's own streams,
    // else how to get the video's own page
    const found = tab ? (await itemsForTab(tab.id)).filter((i) => !i.disabled && !i.yt) : [];
    if (found.length) return { items: found };
    return { error: "Open the video first (click the time next to the name above the video), then press the button again." };
  }
  if (msg.src && /^https?:/i.test(msg.src)) {
    items.push({ label: "Video file", sub: msg.src, url: msg.src, ext: (/\.(\w{2,4})(\?|$)/.exec(msg.src) || [])[1] || "mp4" });
  }
  const page = msg.page || (msg.src && !msg.frame ? "" : tab && tab.url);
  let problem = "";
  if (page && /^https?:/i.test(page)) {
    try {
      const r = await askApp(page);
      if (r.resolved && r.resolved.options.length) items.unshift(...optionItems(r.resolved, page));
      else problem = r.resolveError || "Can't get this video";
    } catch (e) {
      if (!(await appAlive())) return { error: "Ezoo Download is not running" };
      problem = String((e && e.message) || e);
    }
  }
  if (items.length) return { items };
  // the page is nothing the app knows (a player of its own): the streams the
  // player opened are this video
  const found = tab ? (await itemsForTab(tab.id)).filter((i) => !i.disabled && !i.yt) : [];
  if (found.length) return { items: found };
  if (/unsupported url/i.test(problem)) return { error: "Play the video for a moment, then press the button again." };
  return { error: problem || "Can't get this video" };
}

// ---------------------------------------------------------------- tab lifecycle

chrome.tabs.onUpdated.addListener(async (tabId, info, tab) => {
  if (!info.url) return;
  const ytKey = `yt_${tabId}`;
  const { [ytKey]: yt } = await chrome.storage.session.get(ytKey);
  const newId = videoIdOf(info.url);
  if (yt && newId && yt.videoId === newId) return; // same video (e.g. timestamp change)
  seenStreams.delete(tabId);
  const resKey = `res_${tabId}`;
  const { [resKey]: res } = await chrome.storage.session.get(resKey);
  if (info.status === "loading" || yt || res) await clearTab(tabId);
  if (newId && isYoutube(info.url)) resolveYoutube(tabId, info.url);
});
chrome.tabs.onRemoved.addListener((tabId) => {
  seenStreams.delete(tabId);
  chrome.storage.session.remove([`media_${tabId}`, `yt_${tabId}`, `res_${tabId}`]);
});

// ---------------------------------------------------------------- 4. context menu

function makeMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "swiftget-link", title: "Download with Ezoo Download", contexts: ["link"] });
    chrome.contextMenus.create({
      id: "swiftget-media",
      title: "Download this media with Ezoo Download",
      contexts: ["video", "audio", "image"],
    });
    chrome.contextMenus.create({
      id: "swiftget-video",
      title: "Download video with Ezoo Download",
      contexts: ["page", "video", "frame"],
    });
    chrome.contextMenus.create({
      id: "swiftget-all",
      title: "Download all links with Ezoo Download",
      contexts: ["page", "frame", "link", "image"],
    });
    chrome.contextMenus.create({
      id: "swiftget-selected",
      title: "Download selected links with Ezoo Download",
      contexts: ["selection"],
    });
  });
}
chrome.runtime.onInstalled.addListener(makeMenus);
chrome.runtime.onStartup.addListener(makeMenus);

/** Every link of the page (or of the selection) to the app's "Download all links". */
async function sendAllLinks(tab, selection) {
  const res = await chrome.tabs.sendMessage(tab.id, { type: "swiftget-collect-links", selection }).catch(() => null);
  if (!res || !res.links || !res.links.length) return;
  const { port } = await getOpts();
  await fetchWithTimeout(
    apiUrl(port, "/links"),
    {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-SwiftGet": "1" },
      body: JSON.stringify({
        page: res.page || tab.url,
        links: res.links,
        cookies: await cookieHeader(res.page || tab.url),
        userAgent: navigator.userAgent,
      }),
    },
    5000
  );
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === "swiftget-all" || info.menuItemId === "swiftget-selected") {
    if (tab) sendAllLinks(tab, info.menuItemId === "swiftget-selected").catch((e) => console.warn("Ezoo Download:", e));
    return;
  }
  if (info.menuItemId === "swiftget-video") {
    if (!tab) return;
    // find the qualities and open the panel on the page
    if (isYoutube(tab.url)) resolveYoutube(tab.id, tab.url);
    else resolvePage(tab.id, info.frameUrl && info.frameUrl !== tab.url ? tab.url : tab.url);
    chrome.tabs.sendMessage(tab.id, { type: "swiftget-open-panel" }).catch(() => {});
    pushToTab(tab.id);
    return;
  }
  const url = info.linkUrl || info.srcUrl;
  if (!url || !/^https?:/i.test(url)) return;
  sendToApp({ url, referrer: info.pageUrl }).catch((e) => console.warn("Ezoo Download:", e));
});

// ---------------------------------------------------------------- messages from page/popup

function reply(promise, sendResponse) {
  promise
    .then(() => sendResponse({ ok: true }))
    .catch((e) => sendResponse({ ok: false, error: String((e && e.message) || e) }));
  return true;
}

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  switch (msg.type) {
    case "swiftget-download": {
      const item = msg.item || {};
      // a video inside a frame (embedded player): that frame's page
      const referrer = sender.frameId && msg.referrer ? msg.referrer : sender.tab ? sender.tab.url : msg.referrer;
      if (item.yt) return reply(downloadVideo(item.yt, msg.title, referrer), sendResponse);
      return reply(sendToApp({ url: item.url, referrer, fileName: msg.fileName }), sendResponse);
    }
    case "swiftget-get-media":
      itemsForTab(msg.tabId).then(sendResponse);
      return true;
    case "swiftget-media-request":
      if (!sender.tab) return false;
      if (sender.tab.url && videoIdOf(sender.tab.url)) resolveYoutube(sender.tab.id, sender.tab.url);
      itemsForTab(sender.tab.id).then(sendResponse);
      return true;
    case "swiftget-video-seen":
      // the page shows a video: find its qualities
      if (!sender.tab) return false;
      getOpts().then((o) => {
        if (!o.enabled) return;
        if (isYoutube(sender.tab.url)) resolveYoutube(sender.tab.id, sender.tab.url);
        // a Facebook feed: its address is no video (the video's own post
        // link is looked up when its button is clicked)
        else if (!msg.feed) resolvePage(sender.tab.id, msg.page || sender.tab.url, !!msg.frame);
      });
      return false;
    case "swiftget-resolve-video":
      // one of several videos on the page: its own file, or its own link
      resolveOneVideo(msg, sender.tab).then(sendResponse);
      return true;
    case "swiftget-link-rules":
      // what a click on a file link in the page should do (decided in the
      // page at once, so the browser never starts its own download)
      Promise.all([getOpts(), getConfig()]).then(([o, cfg]) => {
        if (!o.enabled || !o.intercept || !cfg) return sendResponse(null);
        sendResponse({
          exts: cfg.captureExts || [],
          skipSites: cfg.skipSites || [],
          skipAddresses: cfg.skipAddresses || [],
          skipKey: cfg.skipKey || "none",
          forceKey: cfg.forceKey || "none",
        });
      });
      return true;
    case "swiftget-link":
      // a file link clicked in a page: straight to the app
      return reply(takeLink(msg.url, sender.tab ? sender.tab.url : ""), sendResponse);
    case "swiftget-magnet":
      // a torrent's magnet link clicked on a page
      return reply(
        getOpts().then((o) => {
          if (!o.enabled || !o.intercept) throw new Error("sending is off");
          return sendToApp({ url: msg.url, referrer: sender.tab ? sender.tab.url : "" });
        }),
        sendResponse
      );
    case "swiftget-playlist":
      // "Download all" on a playlist page: the app's playlist window
      return reply(sendPlaylist(msg.url), sendResponse);
    case "swiftget-status":
      appAlive().then((ok) => sendResponse({ ok }));
      return true;
    case "swiftget-keys":
      lastKeys = { alt: !!msg.alt, ctrl: !!msg.ctrl, shift: !!msg.shift, at: Date.now() };
      return false;
    case "swiftget-config":
      Promise.all([getConfig(), getOpts()]).then(([cfg, o]) => {
        const panel = (cfg && cfg.panel) || { enabled: true, skipSites: [] };
        sendResponse(o.enabled ? panel : { ...panel, enabled: false });
      });
      return true;
    default:
      return false;
  }
});
