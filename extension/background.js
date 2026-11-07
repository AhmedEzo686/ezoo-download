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

// which add-in asks (its version and browser): the program tells the user
// when the Firefox add-on is older than the program and must be updated
// (Firefox keeps its own copy of the add-on)
const IS_FIREFOX = typeof navigator !== "undefined" && /Firefox\//.test(navigator.userAgent || "");
const ADDIN_HEADERS = { "X-Ezoo-Ext": chrome.runtime.getManifest().version, "X-Ezoo-Browser": IS_FIREFOX ? "firefox" : "chromium" };

async function plainFetch(url, opts = {}, ms = 1500) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), ms);
  try {
    if (/^http:\/\/127\.0\.0\.1:/.test(url)) opts = { ...opts, headers: Object.assign({}, ADDIN_HEADERS, opts.headers || {}) };
    return await fetch(url, { ...opts, signal: ctrl.signal });
  } finally {
    clearTimeout(timer);
  }
}

// ---------------------------------------------------------------- the program's key
// The program listens on two ports: the known one (Options, 9614) answers
// only "are you there" and pairing; the real one is chosen at random on
// this computer. Every request to it is signed with the program's secret
// key (the key itself is never sent, and a signed request can't be sent
// again). Loaded from the program's folder, the add-in finds the key and the
// port there (token.json); otherwise (Firefox, another folder) it asks for
// them once and the user says yes in a window of the program.
let link = null; // { token, port }
const FRONT_PATHS = new Set(["/ping", "/pair", "/show", "/quit"]);

async function fromFolder() {
  try {
    const r = await fetch(chrome.runtime.getURL("token.json"), { cache: "no-store" });
    if (!r.ok) return { has: false };
    const j = await r.json();
    return { has: true, token: j.token || "", port: Number(j.port) || 0 };
  } catch {
    return { has: false };
  }
}

/** The key and the real port, or null. */
async function programLink(fresh) {
  if (link && !fresh) return link;
  const f = await fromFolder();
  if (f.has && f.token && f.port) return (link = { token: f.token, port: f.port, folder: true });
  const { appToken, appPort } = await chrome.storage.local.get(["appToken", "appPort"]);
  if (appToken && appPort) return (link = { token: appToken, port: Number(appPort), folder: false });
  link = null;
  return null;
}

let pairing = null;
async function askForKey(frontPort) {
  if (!pairing) {
    pairing = (async () => {
      try {
        // the user has up to two minutes to answer in the program
        const r = await plainFetch(apiUrl(frontPort, "/pair"), { method: "POST", headers: { "X-SwiftGet": "1" } }, 125000);
        if (!r.ok) return null;
        const j = await r.json();
        if (j.token && j.port) {
          await chrome.storage.local.set({ appToken: j.token, appPort: j.port });
          link = { token: j.token, port: Number(j.port), folder: false };
          return link;
        }
        return null;
      } catch {
        return null;
      } finally {
        setTimeout(() => (pairing = null), 1000);
      }
    })();
  }
  return pairing;
}

const enc = new TextEncoder();
const hex = (buf) => [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
async function signHeaders(token, method, path) {
  const time = String(Date.now());
  const nonce = hex(crypto.getRandomValues(new Uint8Array(16)));
  const key = await crypto.subtle.importKey("raw", enc.encode(token), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = hex(await crypto.subtle.sign("HMAC", key, enc.encode(`${method}\n${path}\n${time}\n${nonce}`)));
  return { "X-Ezoo-Time": time, "X-Ezoo-Nonce": nonce, "X-Ezoo-Sig": sig };
}

/** The real port moved (it was taken at the program's start): ask where. */
async function whereNow(frontPort, l) {
  try {
    const r = await plainFetch(apiUrl(frontPort, "/where"), { headers: { "X-SwiftGet": "1", ...(await signHeaders(l.token, "GET", "/where")) } }, 1500);
    if (!r.ok) return null;
    const port = Number((await r.json()).port) || 0;
    if (!port) return null;
    if (!l.folder) await chrome.storage.local.set({ appPort: port });
    return (link = { ...l, port });
  } catch {
    return null;
  }
}

// how the connection to the program is (for the add-in's window)
let linkState = "unknown"; // "ok" | "nokey" | "down"

/** A request; to the program it goes to the real port, signed. */
async function fetchWithTimeout(url, opts = {}, ms = 1500) {
  const m = /^http:\/\/127\.0\.0\.1:(\d+)(\/[^?#]*)/.exec(url);
  if (!m) return plainFetch(url, opts, ms);
  const frontPort = Number(m[1]);
  const path = m[2];
  if (FRONT_PATHS.has(path)) return plainFetch(url, opts, ms);
  const method = (opts.method || "GET").toUpperCase();
  const send = async (l) => {
    const headers = Object.assign({}, opts.headers || {}, await signHeaders(l.token, method, path));
    return plainFetch(`http://127.0.0.1:${l.port}${path}`, { ...opts, headers }, ms);
  };
  let l = await programLink();
  if (!l) {
    // from the program's folder, the key may only be a moment late
    const f = await fromFolder();
    if (f.has) {
      await new Promise((r) => setTimeout(r, 1500));
      l = await programLink(true);
    }
    if (!l && !f.has) l = await askForKey(frontPort);
    if (!l) {
      linkState = "nokey";
      throw new TypeError("not connected to Ezoo Download");
    }
  }
  let r;
  try {
    r = await send(l);
  } catch (e) {
    if (e && e.name === "AbortError") throw e;
    // the real port did not answer: it may have moved
    const again = (await programLink(true)) || l;
    const moved = again.port !== l.port ? again : await whereNow(frontPort, again);
    if (!moved) {
      linkState = "down";
      throw e;
    }
    r = await send(moved);
  }
  if (r.status === 401) {
    // a new key (the program was set up again): read it again, else ask
    const f = await fromFolder();
    let again = await programLink(true);
    if (!again || (again.token === l.token && !f.has)) {
      await chrome.storage.local.remove(["appToken", "appPort"]);
      again = f.has ? null : await askForKey(frontPort);
    }
    if (again && again.token !== l.token) r = await send(again);
  }
  linkState = r.status === 401 ? "nokey" : "ok";
  return r;
}

/** The add-in's window: "Connect again" (forget the key and the port, look again). */
async function reconnect() {
  link = null;
  linkState = "unknown";
  const f = await fromFolder();
  if (!f.has) await chrome.storage.local.remove(["appToken", "appPort"]);
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(apiUrl(port, "/config"), { headers: { "X-SwiftGet": "1" } }, 3000);
    return r.ok;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------- a clean start
// After every update (and every reload) the add-in forgets what the old
// version kept: as if it was removed and added again. The user's choices
// (button style, size...) and a key given by pairing stay.
const OLD_KEYS = ["loadedStamp", "reloadedFor", "reloadedForStamp", "seenRepair"];
chrome.runtime.onInstalled.addListener(async (d) => {
  if (d.reason !== "install" && d.reason !== "update") return;
  try {
    await chrome.storage.session.clear();
    await chrome.storage.local.remove(OLD_KEYS.filter((k) => k !== "seenRepair"));
  } catch {
    /* nothing kept */
  }
  link = null;
  await rememberLoadedFiles();
});

// ---------------------------------------------------------------- self update
// The add-in is loaded from the program's own folder. When the program
// updates itself it brings newer add-in files along; the browser only reads
// them after a reload, so the add-in reloads itself once per new version.
//
// The version alone is not enough (a new setup of the same version brings
// changed files too): the files on the disk are compared with the files the
// browser loaded, and when they differ the add-in reloads (once for them).
const OWN_FILES = ["manifest.json", "background.js", "content.js", "yt-core.js", "popup.html", "popup.js", "options.html", "options.js"];

async function filesStamp() {
  let all = "";
  for (const name of OWN_FILES) {
    try {
      const r = await fetch(chrome.runtime.getURL(name), { cache: "no-store" });
      all += `${name}\n${await r.text()}\n`;
    } catch {
      all += `${name}\n-\n`;
    }
  }
  const sum = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(all));
  return [...new Uint8Array(sum)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

// what the browser loaded: the files as they are when it (re)loads the add-in
async function rememberLoadedFiles() {
  try {
    await chrome.storage.local.set({ loadedStamp: await filesStamp() });
  } catch {
    /* nothing to compare with: the version check still works */
  }
}
chrome.runtime.onStartup.addListener(rememberLoadedFiles);

async function followOwnFiles() {
  const { loadedStamp, reloadedForStamp } = await chrome.storage.local.get(["loadedStamp", "reloadedForStamp"]);
  if (!loadedStamp) return false;
  const now = await filesStamp();
  if (now === loadedStamp || now === reloadedForStamp) return false;
  await chrome.storage.local.set({ reloadedForStamp: now });
  chrome.runtime.reload();
  return true;
}

async function followAppVersion() {
  const last = (await chrome.storage.session.get("versionCheckAt")).versionCheckAt || 0;
  if (Date.now() - last < 60000) return;
  await chrome.storage.session.set({ versionCheckAt: Date.now() });
  try {
    if (await followOwnFiles()) return;
  } catch {
    /* fetch / crypto missing: the version check below */
  }
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(apiUrl(port, "/ping"), { headers: { "X-SwiftGet": "1" } }, 800);
    if (!r.ok || typeof r.json !== "function") return;
    const ping = await r.json();
    // "Repair the browser add-in" in the program: start again, clean
    const repair = Number(ping.repair) || 0;
    if (repair) {
      const { seenRepair } = await chrome.storage.local.get("seenRepair");
      if (seenRepair !== repair) {
        await chrome.storage.local.set({ seenRepair: repair });
        if (seenRepair !== undefined) {
          await chrome.storage.session.clear();
          chrome.runtime.reload();
          return;
        }
      }
    }
    const want = ping.ext;
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

// "are you there": on the known port, else on the program's own port (the
// known one may be kept by another program or by Windows); a busy computer
// may take a moment, so it is asked twice before the program is "not running"
async function appAlive() {
  const { port } = await getOpts();
  const ask = async (p, ms) => {
    try {
      const r = await plainFetch(apiUrl(p, "/ping"), { headers: { "X-SwiftGet": "1" } }, ms);
      return r.ok;
    } catch {
      return false;
    }
  };
  for (const ms of [1500, 4000]) {
    if (await ask(port, ms)) {
      followAppVersion();
      return true;
    }
    const l = await programLink(true);
    if (l && l.port && l.port !== port && (await ask(l.port, ms))) return true;
  }
  return false;
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

// ---------------------------------------------------------------- protected videos
// A video protected by its site (DRM): no program can download it. The
// message says so plainly, in the browser's language.
const DRM_TEXTS = {
  ar: "🔒 الفيديو ده محمي من صاحب الموقع (DRM) ومينفعش يتحمّل بأي برنامج تحميل. تقدر تتفرج عليه من الموقع بس.",
  fr: "🔒 Cette vidéo est protégée par son site (DRM) : aucun programme ne peut la télécharger. Elle se regarde seulement sur le site.",
  es: "🔒 Este vídeo está protegido por su sitio (DRM): ningún programa puede descargarlo. Solo se puede ver en el sitio.",
  de: "🔒 Dieses Video ist von seiner Seite geschützt (DRM): Kein Programm kann es herunterladen. Es lässt sich nur auf der Seite ansehen.",
  pt: "🔒 Este vídeo é protegido pelo site (DRM): nenhum programa consegue baixá-lo. Ele só pode ser assistido no site.",
  tr: "🔒 Bu video sitesi tarafından korunuyor (DRM): hiçbir program indiremez. Yalnızca sitede izlenebilir.",
  ru: "🔒 Это видео защищено сайтом (DRM): ни одна программа не может его скачать. Его можно смотреть только на сайте.",
  en: "🔒 This video is protected by its site (DRM): no download program can save it. It can only be watched on the site.",
};
// Telegram Web plays its videos through its own page (a "stream" address
// that exists only inside that page): no program can download it from
// outside, Telegram's own download button can.
const TELEGRAM_TEXTS = {
  ar: "الفيديو ده في تيليجرام ويب مينفعش يتحمّل من برا الصفحة. نزّله من زرار التحميل بتاع تيليجرام نفسه (⋮ ← تنزيل).",
  fr: "Cette vidéo de Telegram Web ne peut pas être téléchargée hors de la page. Utilisez le bouton de téléchargement de Telegram (⋮ → Télécharger).",
  es: "Este vídeo de Telegram Web no se puede descargar fuera de la página. Usa el botón de descarga de Telegram (⋮ → Descargar).",
  de: "Dieses Video in Telegram Web lässt sich nicht außerhalb der Seite laden. Nutze den Download-Knopf von Telegram (⋮ → Herunterladen).",
  pt: "Este vídeo do Telegram Web não pode ser baixado fora da página. Use o botão de download do próprio Telegram (⋮ → Baixar).",
  tr: "Telegram Web'deki bu video sayfanın dışından indirilemez. Telegram'ın kendi indirme düğmesini kullanın (⋮ → İndir).",
  ru: "Это видео в Telegram Web нельзя скачать вне страницы. Используйте кнопку загрузки самого Telegram (⋮ → Скачать).",
  en: "This Telegram Web video can't be downloaded outside the page. Use Telegram's own download button (⋮ → Download).",
};
const isTelegramStream = (url) => /^https:\/\/web\.telegram\.org\/(?:[a-z]\/)?stream\//i.test(url || "");
function telegramText() {
  const lang = String((navigator.language || "en")).slice(0, 2).toLowerCase();
  return TELEGRAM_TEXTS[lang] || TELEGRAM_TEXTS.en;
}
const isDrm = (text) => /\bDRM\b|drm protected|widevine|playready/i.test(text || "");
function drmText() {
  const lang = String((navigator.language || "en")).slice(0, 2).toLowerCase();
  return DRM_TEXTS[lang] || DRM_TEXTS.en;
}

// ---------------------------------------------------------------- the browser's own request
// What the browser sent for a file a moment ago (some sites let only exactly
// that request in: the right Accept-Language, Sec-Fetch-*, client hints...).
// The download is handed over with the same headers. Kept in memory for two
// minutes: the download starts right after its request.
const SENT_HEADERS = new Map();
const KEEP_HEADERS = /^(accept-language|sec-fetch-(site|mode|dest|user)|sec-ch-ua(-mobile|-platform)?|origin|referer|user-agent|cookie|x-requested-with)$/i;
function rememberRequest(d) {
  if (!d.requestHeaders || !/^https?:/i.test(d.url)) return;
  const kept = d.requestHeaders.filter((h) => KEEP_HEADERS.test(h.name) && typeof h.value === "string");
  if (!kept.length) return;
  SENT_HEADERS.set(d.url, { at: Date.now(), headers: kept });
  if (SENT_HEADERS.size > 300) {
    const old = Date.now() - 120000;
    for (const [k, v] of SENT_HEADERS) if (v.at < old || SENT_HEADERS.size > 300) SENT_HEADERS.delete(k);
  }
}
try {
  // Chrome / Edge / Brave need "extraHeaders" to show Cookie and Referer
  const extra = chrome.webRequest.OnSendHeadersOptions && chrome.webRequest.OnSendHeadersOptions.EXTRA_HEADERS;
  chrome.webRequest.onSendHeaders.addListener(
    rememberRequest,
    { urls: ["<all_urls>"], types: ["main_frame", "sub_frame", "media", "object", "other", "xmlhttprequest"] },
    extra ? ["requestHeaders", extra] : ["requestHeaders"]
  );
} catch (_) {
  /* no request headers here: the cookies and the page still go along */
}

/** What the browser sent for `url` (or the first of `urls` it knows). */
function browserRequest(...urls) {
  for (const u of urls) {
    const r = u && SENT_HEADERS.get(u);
    if (r && Date.now() - r.at < 120000) return r.headers;
  }
  return null;
}

async function sendToApp({ url, audioUrl, referrer, fileName, headers, sourcePage, sourceKey, alsoUrl, clip }) {
  const { port } = await getOpts();
  // the browser's own request for this file: its headers go along (the app
  // keeps its own Accept / Range); its cookie is the one the site really got
  const sent = headers ? null : browserRequest(url, alsoUrl);
  const sentCookie = sent && sent.find((h) => /^cookie$/i.test(h.name));
  const sentReferer = sent && sent.find((h) => /^referer$/i.test(h.name));
  const sentUa = sent && sent.find((h) => /^user-agent$/i.test(h.name));
  const passOn = sent ? sent.filter((h) => !/^(cookie|referer|user-agent)$/i.test(h.name)).map((h) => [h.name, h.value]) : null;
  const body = {
    url,
    audioUrl: audioUrl || null,
    headers: headers || (passOn && passOn.length ? passOn : null),
    sourcePage: sourcePage || null,
    sourceKey: sourceKey || null,
    referrer: referrer || (sentReferer && sentReferer.value) || null,
    cookies: (sentCookie && sentCookie.value) || (await cookieHeader(url)),
    userAgent: (sentUa && sentUa.value) || navigator.userAgent,
    fileName: fileName || null,
    clip: clip || null,
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
  // a .torrent file: only when Ezoo Download is the main torrent program
  const isTorrent = extensionOf(fileName) === "torrent" || extensionOf(new URL(url).pathname) === "torrent" || /bittorrent/i.test(mime || "");
  if (isTorrent) return cfg.torrents !== false;
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
  // Telegram's own download of a video: only its page can give the file
  if (isTelegramStream(url) || isTelegramStream(item.url)) return false;
  if (!isNewDownload(item)) return false;
  if (await takeBypass(url)) return false;
  // the browser's PDF viewer saving the PDF it shows: the browser asks where
  // to save it anyway (that window can't be stopped), so it keeps it rather
  // than two windows asking at once
  if (await shownInPdfViewer(item)) return false;
  // already handed to the app by the click on its link: the browser's copy goes
  if ((await justTaken(item.url)) || (await justTaken(url))) return true;
  if (opts.minSizeKB > 0 && item.totalBytes > 0 && item.totalBytes < opts.minSizeKB * 1024) return false;
  if (!(await appAlive())) return false; // app not running: let the browser handle it
  const cfg = await getConfig();
  if (!wanted(cfg, url, baseName(item.filename), item.mime, currentKeys())) return false;
  if (await appSaysBrowser([item.url, url])) return false; // the user chose the browser for it
  // a link that works once only (the browser just used it) or only with
  // the page's own sign-in (ChatGPT, iLovePDF...): it stays in the browser
  if (!(await appCanFetch(url, item))) return false;
  return true;
}

/** Can the program get this file with the browser's sign-in? (a short look; when it can't tell, yes) */
async function appCanFetch(url, item) {
  const { port } = await getOpts();
  try {
    const sent = browserRequest(url, item.url);
    const pick = (n) => sent && sent.find((h) => h.name.toLowerCase() === n);
    const passOn = sent ? sent.filter((h) => !/^(cookie|referer|user-agent|range)$/i.test(h.name)).map((h) => [h.name, h.value]) : null;
    const body = {
      url,
      referrer: item.referrer || (pick("referer") && pick("referer").value) || null,
      cookies: (pick("cookie") && pick("cookie").value) || (await cookieHeader(url)),
      userAgent: (pick("user-agent") && pick("user-agent").value) || navigator.userAgent,
      headers: passOn && passOn.length ? passOn : null,
    };
    const r = await fetchWithTimeout(
      apiUrl(port, "/check"),
      { method: "POST", headers: { "Content-Type": "application/json", "X-SwiftGet": "1" }, body: JSON.stringify(body) },
      3200
    );
    if (!r.ok || typeof r.json !== "function") return true;
    return (await r.json()).ok !== false;
  } catch {
    return true;
  }
}

/** A PDF that a tab shows in the browser's own viewer right now. */
async function shownInPdfViewer(item) {
  const pdf = /pdf/i.test(item.mime || "") || /\.pdf($|[?#])/i.test(item.finalUrl || item.url || "");
  if (!pdf) return false;
  try {
    const tabs = await chrome.tabs.query({});
    const same = (a, b) => !!a && !!b && a.split("#")[0] === b.split("#")[0];
    return tabs.some((t) => same(t.url, item.url) || same(t.url, item.finalUrl));
  } catch {
    return false;
  }
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
    await sendToApp({ url, alsoUrl: item.url, referrer: item.referrer, fileName: baseName(item.filename) });
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
    const guard = setTimeout(keep, 7000);
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
        isDrm(res.resolveError)
          ? { label: drmText(), sub: "", disabled: true, drm: true }
          : unknown
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
    if (isTelegramStream(url)) return; // only Telegram's own page can play it
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

// a part of a video as the page sent it: two times in seconds, the end
// after the start (else nothing)
function partOf(c) {
  if (!c || typeof c !== "object") return null;
  const start = Number(c.start);
  const end = Number(c.end);
  if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end <= start || end > 7 * 24 * 3600) return null;
  return { start, end, exact: c.exact === true };
}

async function downloadVideo(option, pageTitle, pageUrl, clip) {
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
    clip,
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
  if (isTelegramStream(msg.src) || isTelegramStream(msg.resolve)) return { error: telegramText() };
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
  if (isDrm(problem)) return { error: drmText(), drm: true };
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
      // a part only from the "✂ Part" tab (the whole video never carries one)
      const clip = msg.part ? partOf(msg.clip) : null;
      if (msg.part && !clip) return reply(Promise.reject(new Error("the part is not right")), sendResponse);
      if (item.yt) return reply(downloadVideo(item.yt, msg.title, referrer, clip), sendResponse);
      return reply(sendToApp({ url: item.url, referrer, fileName: msg.fileName, clip }), sendResponse);
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
        getOpts().then(async (o) => {
          if (!o.enabled || !o.intercept) throw new Error("sending is off");
          // Options > Torrents: Ezoo Download is not the main torrent program
          const cfg = await getConfig();
          if (cfg && cfg.torrents === false) throw new Error("torrents are not taken");
          return sendToApp({ url: msg.url, referrer: sender.tab ? sender.tab.url : "" });
        }),
        sendResponse
      );
    case "swiftget-playlist":
      // "Download all" on a playlist page: the app's playlist window
      return reply(sendPlaylist(msg.url), sendResponse);
    case "swiftget-status":
      (async () => {
        const ok = await appAlive();
        if (!ok) return sendResponse({ ok: false, state: "down" });
        // the program runs: is the add-in connected to it (its key)?
        const { port } = await getOpts();
        let linked = false;
        try {
          linked = (await fetchWithTimeout(apiUrl(port, "/config"), { headers: { "X-SwiftGet": "1" } }, 2500)).ok;
        } catch {
          linked = false;
        }
        sendResponse({ ok: linked, state: linked ? "ok" : "nokey" });
      })();
      return true;
    case "swiftget-reconnect":
      reconnect().then((ok) => sendResponse({ ok }));
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
