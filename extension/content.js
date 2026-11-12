// Floating "Download this video" button, shown when the background worker
// detects a video/audio file or HLS stream on the page. Runs in every frame:
// videos inside frames (embedded players) and inside shadow DOM get their
// own button too.
(() => {
  if (window.__ezooDownload) return;
  window.__ezooDownload = true;
  const TOP = window.top === window;

  let items = [];
  // Options > File Types > "Download panel for web players"
  let panel = { enabled: true, skipSites: [] };
  const hostMatches = (host, p) => {
    p = String(p).replace(/^https?:\/\//i, "").split("/")[0].toLowerCase();
    if (!p) return false;
    if (p.includes("*")) return new RegExp("^" + p.replace(/[.]/g, "\\.").replace(/\*/g, ".*") + "$").test(host);
    return host === p || host.endsWith("." + p);
  };
  // the on / off switch of the add-in's window
  let switchedOn = true;
  // the video button in full screen too (the add-in's window); off: hidden there
  let showInFullscreen = false;
  const panelAllowed = () =>
    switchedOn &&
    !noButtonHere &&
    panel.enabled !== false &&
    !(panel.skipSites || []).some((s) => hostMatches(location.hostname.toLowerCase(), s));
  // a press of the user (or the same press passed on by early.js), never one a
  // page's script made up
  const userPress = (e) => !!e && (e.isTrusted || !!(globalThis.__ezooReplayed && globalThis.__ezooReplayed.has(e)));
  // "Don't show it on this site", and where the user put it on this site
  const SITE = location.hostname.toLowerCase();
  let noButtonHere = false;
  let sitePlace = null;
  try {
    chrome.storage.local.get({ noButtonSites: [], buttonPlaces: {} }, (o) => {
      noButtonHere = !!(o && (o.noButtonSites || []).includes(SITE));
      sitePlace = (o && o.buttonPlaces && o.buttonPlaces[SITE]) || null;
      if (noButtonHere) removeAll();
    });
  } catch (_) {
    /* storage not reachable: shown, at its usual place */
  }
  const PRIVATE = !!(chrome.extension && chrome.extension.inIncognitoContext);
  function keepPlace(p) {
    sitePlace = p;
    // never in a private window (it would keep the site's name)
    if (PRIVATE) return;
    try {
      chrome.storage.local.get({ buttonPlaces: {} }, (o) => {
        const all = (o && o.buttonPlaces) || {};
        delete all[SITE];
        all[SITE] = p;
        const sites = Object.keys(all);
        for (const s of sites.slice(0, Math.max(0, sites.length - 200))) delete all[s];
        chrome.storage.local.set({ buttonPlaces: all });
      });
    } catch (_) {
      /* not kept: it stays there on this page */
    }
  }
  function noButtonOnThisSite() {
    noButtonHere = true;
    removeAll();
    try {
      chrome.storage.local.get({ noButtonSites: [] }, (o) => {
        const list = ((o && o.noButtonSites) || []).filter((h) => h !== SITE);
        list.push(SITE);
        chrome.storage.local.set({ noButtonSites: list.slice(-500) });
      });
    } catch (_) {
      /* only for this page then */
    }
  }

  const fmtSize = (n) => {
    if (!n) return "";
    const u = ["B", "KB", "MB", "GB"];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) {
      n /= 1024;
      i++;
    }
    return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
  };
  const fmtLen = (s) => {
    s = Math.round(Number(s) || 0);
    if (!s) return "";
    const two = (n) => String(n).padStart(2, "0");
    return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${Math.floor(s / 60)}:${two(s % 60)}`;
  };
  // "1:02:05", "02:05", "125", "2:05.5" -> seconds (null when not a time)
  const parseClock = (text) => {
    const t = String(text || "").trim().replace(/[٠-٩]/g, (c) => String(c.charCodeAt(0) - 0x660)).replace(",", ".");
    const m = /^(?:(\d{1,2}):)?(?:(\d{1,3}):)?(\d{1,6})(?:\.(\d{1,3}))?$/.exec(t);
    if (!m) return null;
    const parts = t.split(".")[0].split(":").map(Number);
    if (parts.slice(1).some((n) => n >= 60)) return null;
    let secs = parts.reduce((a, n) => a * 60 + n, 0);
    if (m[4]) secs += Number("0." + m[4]);
    return secs;
  };
  const clock = (s) => {
    s = Math.max(0, Math.floor(Number(s) || 0));
    const two = (n) => String(n).padStart(2, "0");
    return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${two(Math.floor(s / 60))}:${two(s % 60)}`;
  };
  // the resolution in plain words (the app sends it; streams seen in the
  // player only have the height)
  const tagOf = (h) => (!h ? "" : h >= 4320 ? "8K" : h >= 2160 ? "4K" : h >= 1440 ? "2K" : h >= 1080 ? "Full HD" : h >= 720 ? "HD" : "SD");

  const cleanTitle = () =>
    (document.title || "video").replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").trim().slice(0, 120) || "video";

  // --------------------------------------------------------------- videos
  // Videos of the page, also those inside shadow DOM (players built as web
  // components). The deep search is slower, so it runs every few rounds.
  let shadowVideos = [];
  let deepRound = 0;
  function collectShadow(root, out, depth) {
    if (depth > 6) return;
    for (const el of root.querySelectorAll("*")) {
      const sr = el.shadowRoot;
      if (!sr) continue;
      out.push(...sr.querySelectorAll("video"));
      collectShadow(sr, out, depth + 1);
    }
  }
  function allVideos() {
    const list = [...document.querySelectorAll("video")];
    if (deepRound++ % 4 === 0) {
      const found = [];
      try {
        collectShadow(document, found, 0);
      } catch (_) {
        /* a strange page: keep what we have */
      }
      shadowVideos = found;
    }
    for (const v of shadowVideos) if (v.isConnected && !list.includes(v)) list.push(v);
    return list;
  }

  // The video the user is watching: a playing one first, else the biggest
  // one on screen.
  function biggestVideo() {
    let best = null;
    let score = 0;
    for (const v of allVideos()) {
      const r = v.getBoundingClientRect();
      if (r.width < 200 || r.height < 100) continue;
      const visible = Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0));
      if (visible < 60) continue;
      const s = r.width * visible * (!v.paused && !v.ended ? 4 : 1);
      if (s > score) {
        best = v;
        score = s;
      }
    }
    return best;
  }
  const playing = (v) => !v.paused && !v.ended && v.duration > 3;

  // Pages that are one video: their own address is the best link for it.
  const VIDEO_PAGE = [
    /youtube\.com\/(watch|shorts\/|live\/|embed\/)/i,
    /youtube-nocookie\.com\/embed\//i,
    /tiktok\.com\/@[^/]+\/(video|photo)\//i,
    /instagram\.com\/(reels?|p|tv)\//i,
    /facebook\.com\/(.+\/videos\/|watch|reel\/|share\/[vr]\/|plugins\/video)/i,
    /fb\.watch\//i,
    /(twitter|x)\.com\/[^/]+\/status\//i,
    /vimeo\.com\/\d+/i,
    /player\.vimeo\.com\/video\//i,
    // (not the "geo.dailymotion.com/player/…" frame: that is only the player)
    /dailymotion\.com\/(video\/|embed\/video\/)/i,
    /dai\.ly\//i,
    /reddit\.com\/r\/[^/]+\/comments\//i,
    /twitch\.tv\/(videos\/|[^/]+\/clip\/)/i,
    /clips\.twitch\.tv\//i,
    /pinterest\.[a-z.]+\/pin\//i,
    /snapchat\.com\/(spotlight|add)\//i,
    /linkedin\.com\/(posts|feed\/update)\//i,
  ];
  const isVideoPage = (u) => VIDEO_PAGE.some((r) => r.test(u));
  // Facebook feeds: the post a video is in is also a link the app can read
  // (the feed's own address - facebook.com/ - is not)
  const FB_POST = [
    /facebook\.com\/[^/?#]+\/posts\//i,
    /facebook\.com\/permalink\.php\?[^#]*story_fbid=/i,
    /facebook\.com\/story\.php\?[^#]*story_fbid=/i,
    /facebook\.com\/groups\/[^/]+\/(permalink|posts)\//i,
    /facebook\.com\/photo(\.php)?\/?\?[^#]*fbid=/i,
  ];
  const onFacebook = /(^|\.)facebook\.com$/i.test(location.hostname);
  const isPostLink = (u) => onFacebook && FB_POST.some((r) => r.test(u));
  // a Facebook feed (home, a page's wall, groups...): many videos, and its
  // own address is none of them
  const inFeed = () => onFacebook && !isVideoPage(location.href) && !isPostLink(location.href);
  // what is left of a Facebook link once its tracking part is cut off
  function cleanLink(u) {
    try {
      const x = new URL(u);
      for (const k of [...x.searchParams.keys()]) if (/^(__cft__|__tn__|_fb_noscript|ref|rdid|mibextid)/i.test(k)) x.searchParams.delete(k);
      x.hash = "";
      return x.toString();
    } catch {
      return u.split("#")[0];
    }
  }

  // In feeds (Facebook, Instagram, X...) the page shows many videos: look for
  // the post / reel link around the video being watched.
  function linkFor(video) {
    if (isVideoPage(location.href) || isPostLink(location.href)) return cleanLink(location.href);
    // a Facebook feed / home page is no video: nothing rather than the feed
    return ownLink(video) || (onFacebook ? "" : location.href);
  }

  // the post / video link next to this very video ("" = none found)
  function ownLink(video) {
    let el = video;
    let post = "";
    for (let depth = 0; el && depth < 25; depth++) {
      const links = el.querySelectorAll ? el.querySelectorAll("a[href]") : [];
      for (const a of links) {
        if (isVideoPage(a.href)) return cleanLink(a.href);
        if (!post && isPostLink(a.href)) post = cleanLink(a.href);
      }
      // the end of this post (Facebook marks its posts, it has no <article>)
      if (el.tagName === "ARTICLE" || (el.getAttribute && el.getAttribute("role") === "article")) break;
      // out of a shadow root: on to the element that holds it
      el = el.parentElement || (el.parentNode && el.parentNode.host) || null;
    }
    if (post) return post;
    // an embedded player: the frame's own address is the video
    if (!TOP && isVideoPage(location.href)) return location.href;
    return "";
  }

  let lastPage = "";
  function watchVideos() {
    const v = biggestVideo();
    if (!v) return;
    // ignore tiny muted background loops that never play for long
    if (v.paused && !(v.duration > 3) && !v.currentSrc) return;
    // in a frame (an embedded player): its own address when it is a video
    // page (YouTube / Vimeo embeds), otherwise the page around it (the
    // add-in knows that one)
    const page = TOP ? linkFor(v) : isVideoPage(location.href) ? location.href : "";
    if (page !== lastPage || (!TOP && !lastPage)) {
      if (!TOP && lastPage === "frame:" + page) return;
      lastPage = TOP ? page : "frame:" + page;
      chrome.runtime.sendMessage({ type: "swiftget-video-seen", page, frame: !TOP, feed: TOP && !page && inFeed() }).catch?.(() => {});
    }
  }

  // ---------------------------------------------------------------- bubbles
  // One "Download" bubble on every video of the page. The main video (the
  // one playing / the biggest) uses what the add-in found for the page; any
  // other video is looked up when its bubble is clicked. A bubble can be
  // dragged anywhere on its video and stays there.
  const bubbles = new Map(); // video -> bubble
  function removeAll() {
    for (const b of bubbles.values()) b.host.remove();
    bubbles.clear();
  }
  // size of the button: the slider in the add-in's menu (50% - 200%)
  const OLD_SIZES = { small: 0.85, normal: 1, large: 1.25 };
  // its look: the add-in's menu > "Button style"
  const STYLES = ["compact", "blue", "dark", "light", "green", "icon"];
  let look = "compact";
  const applyLook = () => {
    for (const b of bubbles.values()) {
      const w = b.root.querySelector(".wrap");
      if (w) w.className = "wrap " + look;
      placeOne(b);
    }
  };
  const clampZoom = (z) => Math.min(2, Math.max(0.5, Number(z) || 1));
  let zoom = 1;
  // how solid the button is while the mouse is not on it (the menu: 20% - 100%)
  const clampSolid = (o) => Math.min(1, Math.max(0.2, Number(o) || 0.9));
  let solid = 0.9;
  const applyZoom = () => {
    for (const b of bubbles.values()) {
      b.host.style.setProperty("--z", String(zoom));
      b.host.style.setProperty("--o", String(solid));
      placeOne(b);
    }
  };
  try {
    chrome.storage.local.get({ enabled: true }, (o) => {
      switchedOn = !o || o.enabled !== false;
      if (!switchedOn) removeAll();
    });
    chrome.storage.onChanged.addListener((ch) => {
      if (!ch.enabled) return;
      switchedOn = ch.enabled.newValue !== false;
      if (!switchedOn) removeAll();
      else chrome.runtime.sendMessage({ type: "swiftget-media-request" }, (res) => !chrome.runtime.lastError && update(res));
    });
  } catch (_) {
    // storage not reachable (add-in reloading): stays on
  }
  try {
    chrome.storage.local.get({ bubbleFullscreen: false }, (o) => (showInFullscreen = !!(o && o.bubbleFullscreen)));
    chrome.storage.onChanged.addListener((ch) => {
      if (ch.bubbleFullscreen) {
        showInFullscreen = !!ch.bubbleFullscreen.newValue;
        bubbles.forEach((b) => placeOne(b));
      }
    });
    chrome.storage.local.get({ bubbleScale: null, bubbleSize: "normal", bubbleStyle: "compact", bubbleOpacity: 0.9 }, (o) => {
      zoom = clampZoom(o && o.bubbleScale != null ? o.bubbleScale : OLD_SIZES[o && o.bubbleSize]);
      solid = clampSolid(o && o.bubbleOpacity);
      look = STYLES.includes(o && o.bubbleStyle) ? o.bubbleStyle : "compact";
      applyZoom();
      applyLook();
    });
    chrome.storage.onChanged.addListener((ch) => {
      if (ch.bubbleStyle) {
        look = STYLES.includes(ch.bubbleStyle.newValue) ? ch.bubbleStyle.newValue : "compact";
        applyLook();
        return;
      }
      if (ch.bubbleOpacity) solid = clampSolid(ch.bubbleOpacity.newValue);
      else if (ch.bubbleScale) zoom = clampZoom(ch.bubbleScale.newValue);
      else if (ch.bubbleSize) zoom = clampZoom(OLD_SIZES[ch.bubbleSize.newValue]);
      else return;
      applyZoom();
    });
  } catch (_) {
    // storage not reachable (add-in reloading): default size
  }
  const STYLE = `
    :host { all: initial; }
    .wrap { font: 11px/1.25 "Segoe UI", system-ui, sans-serif; color: #1f2733; display: inline-block; zoom: var(--z, 1); position: relative; }
    .btn { display: inline-flex; align-items: stretch; color: #fff; border: 1px solid #0f4fb5;
           background: linear-gradient(#3f8cf5, #1d63d6); border-radius: 3px;
           box-shadow: 0 1px 6px rgba(0,0,0,.35); font: inherit; font-weight: 600;
           opacity: var(--o, .9); transition: opacity .15s; user-select: none; white-space: nowrap; }
    .btn:hover, .btn.open { opacity: 1; }
    .btn.drag { opacity: 1; }
    .part { display: inline-flex; align-items: center; gap: 5px; padding: 3px 7px; cursor: default; }
    .part:hover { background: rgba(255,255,255,.12); }
    .btn.drag .part { cursor: grabbing; }
    .side { padding: 3px 6px; cursor: default; border-left: 1px solid rgba(255,255,255,.28); display: inline-flex; align-items: center; }
    .side:hover { background: rgba(255,255,255,.18); }
    .ico { width: 13px; height: 13px; flex: none; }
    .caret { font-size: 9px; opacity: .9; }
    .spin { width: 9px; height: 9px; border: 2px solid rgba(255,255,255,.4); border-top-color: #fff; border-radius: 50%;
            animation: r .8s linear infinite; flex: none; }
    @keyframes r { to { transform: rotate(360deg); } }
    /* the list floats under the button (the button never moves): to its
       left when the button is on the right of the video, else to its right,
       and above it when there is no room below */
    .list { position: absolute; top: 100%; right: 0; width: 330px; margin-top: 4px; background: #fff; border-radius: 6px;
            box-shadow: 0 6px 22px rgba(0,0,0,.28); overflow: hidden; max-height: 360px; overflow-y: auto; border: 1px solid #d5dbe4; }
    .wrap.toright .list { right: auto; left: 0; }
    .wrap.up .list { top: auto; bottom: 100%; margin-top: 0; margin-bottom: 4px; }
    .btn.locked { background: linear-gradient(#7b8794, #5b6573); border-color: #4a535f; }
    .head { padding: 7px 10px; background: #f4f6fa; border-bottom: 1px solid #e3e8ef; font-weight: 600; color: #334;
            overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .item { padding: 6px 10px; cursor: default; border-bottom: 1px solid #eef1f5; display: flex; gap: 6px; }
    .item:last-child { border-bottom: 0; }
    .item:hover { background: #e8f0fe; }
    .num { color: #6b7686; min-width: 16px; text-align: right; }
    .body { flex: 1; min-width: 0; }
    .t { font-weight: 600; display: flex; align-items: center; gap: 5px; flex-wrap: wrap; }
    .s { color: #6b7686; font-size: 10.5px; overflow-wrap: anywhere; }
    .tag { font-size: 9.5px; font-weight: 700; padding: 0 4px; border-radius: 3px; color: #fff; background: #7b8794; line-height: 14px; }
    .tag.HD { background: #1d8f5a; } .tag.FullHD { background: #1d63d6; } .tag.K2 { background: #7b3fd6; }
    .tag.K4 { background: #d6631d; } .tag.K8 { background: #c0262d; }
    .codec { font-size: 9.5px; color: #4a5566; border: 1px solid #c9d1dc; border-radius: 3px; padding: 0 3px; line-height: 13px; }
    .all { font-weight: 700; color: #1d63d6; }
    .msg { padding: 8px 10px; color: #1a9e5a; }
    .help { padding: 8px 10px; color: #334; background: #fffbe6; border-bottom: 1px solid #f0e2a8; }
    .act { margin-top: 6px; display: inline-block; background: #1d63d6; color: #fff; border-radius: 4px; padding: 3px 9px; cursor: default; }
    .act:hover { background: #1a56b8; }
    .off { cursor: default; opacity: .7; }
    .off:hover { background: #fff; }
    .err { color: #d1342f; }
    /* "Download a part" */
    .clip { padding: 8px 10px; border-bottom: 1px solid #e3e8ef; background: #fafbfd; }
    .clip .row { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; }
    .clip .row b { min-width: 34px; font-weight: 600; }
    .clip input[type=text] { width: 76px; font: inherit; padding: 3px 5px; border: 1px solid #c9d1dc; border-radius: 4px; color: #1f2733; background: #fff; }
    .clip input.bad { border-color: #d1342f; }
    .clip .now, .clip .btn2 { background: #e8f0fe; color: #1d63d6; border-radius: 4px; padding: 3px 8px; cursor: default; white-space: nowrap; }
    .clip .now:hover, .clip .btn2:hover { background: #d6e4fd; }
    .clip .len { color: #334; margin-bottom: 6px; }
    .clip label { display: flex; gap: 6px; align-items: flex-start; margin: 4px 0; cursor: default; }
    .clip label input { margin: 2px 0 0; flex: none; }
    .clip label small { display: block; color: #6b7686; font-size: 10.5px; }
    .clip .acts { display: flex; gap: 6px; margin-top: 6px; }
    .clip .pick { margin-top: 8px; font-weight: 600; color: #334; }
    .cliprow .t { color: #1d63d6; }
    /* "⬇ Whole video" / "✂ Part" (the part in orange) */
    .tabs { display: flex; border-bottom: 1px solid #e3e8ef; background: #f4f6fa; }
    .tab { flex: 1; text-align: center; padding: 7px 4px; cursor: default; color: #5b6575; font-weight: 600; border-bottom: 3px solid transparent; }
    .tab:hover { background: #eef2f8; }
    .tab.on { color: #1d63d6; border-bottom-color: #1d63d6; background: #fff; }
    .tab.part.on { color: #e8590c; border-bottom-color: #e8590c; }
    .cut { padding: 9px 12px 11px; background: #fff; }
    .cut .lead { color: #e8590c; font-weight: 700; margin-bottom: 8px; }
    .cut .times { display: flex; align-items: center; gap: 6px; margin-bottom: 8px; }
    .cut .times b { font-weight: 700; color: #1f2733; }
    .cut input[type=text] { width: 58px; font: inherit; text-align: center; padding: 3px 4px; border: 1px solid #c9d1dc; border-radius: 4px; color: #1f2733; background: #fff; }
    .cut input.bad { border-color: #d1342f; }
    .cut .now { color: #e8590c; cursor: default; white-space: nowrap; font-size: 10.5px; }
    .cut .now:hover { text-decoration: underline; }
    .cut .len { margin-left: auto; font-weight: 700; color: #1f2733; }
    .cut .len.err { color: #d1342f; }
    .cut .seg { display: inline-flex; border: 1px solid #e8590c; border-radius: 4px; overflow: hidden; margin-bottom: 8px; }
    .cut .seg span { padding: 3px 10px; color: #e8590c; cursor: default; background: #fff; }
    .cut .seg span + span { border-left: 1px solid #e8590c; }
    .cut .seg span.on { background: #e8590c; color: #fff; }
    .cut .qs { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 10px; }
    .cut .qs span { padding: 3px 9px; border: 1px solid #c9d1dc; border-radius: 4px; color: #1f2733; cursor: default; background: #fff; font-weight: 600; }
    .cut .qs span:hover { border-color: #e8590c; }
    .cut .qs span.on { border-color: #e8590c; background: #fff3eb; color: #e8590c; }
    .cut .acts { display: flex; gap: 8px; }
    .cut .pv { padding: 6px 10px; border-radius: 5px; background: #fff3eb; color: #e8590c; cursor: default; white-space: nowrap; }
    .cut .pv:hover { background: #ffe4d2; }
    .cut .go { flex: 1; text-align: center; padding: 6px 10px; border-radius: 5px; color: #fff; font-weight: 700; cursor: default;
               background: linear-gradient(#f57a2e, #e05a0c); box-shadow: 0 2px 6px rgba(224,90,12,.35); }
    .cut .go:hover { background: linear-gradient(#ff8a3d, #e8630f); }
    /* looks */
    .dark .btn { background: linear-gradient(#3a3f47, #23272e); border-color: #15181c; }
    .dark .btn:hover, .dark .btn.open { background: linear-gradient(#454b55, #2a2f37); }
    .dark .ico path { fill: #23272e; }
    .light .btn { background: linear-gradient(#ffffff, #e9eef5); border-color: #b8c3d1; color: #1f2733; }
    .light .btn:hover, .light .btn.open { background: linear-gradient(#ffffff, #dfe7f1); }
    .light .ico circle { fill: #1d63d6; }
    .light .ico path { fill: #fff; }
    .light .side { border-left-color: #c9d3df; }
    .light .spin { border-color: rgba(29,99,214,.3); border-top-color: #1d63d6; }
    .green .btn { background: linear-gradient(#35b36b, #1f8a4c); border-color: #176b3a; }
    .green .btn:hover, .green .btn.open { background: linear-gradient(#3cc276, #1d8048); }
    .green .ico path { fill: #1f8a4c; }
    .compact .btn { border-radius: 7px; background: linear-gradient(#2f72dc, #1e56b9); border-color: #163f8a; }
    .compact .btn:hover, .compact .btn.open { background: linear-gradient(#3a7deb, #1f5cc6); }
    .compact #label, .compact .caret, .compact #help { display: none; }
    .compact .part { padding: 4px 7px; }
    .compact .ico { width: 18px; height: 18px; }
    .compact .ico circle { fill: #e4e6ea; }
    .compact .ico path { fill: #1e56b9; }
    .compact #close { padding: 4px 8px; font-size: 13px; border-left-color: rgba(255,255,255,.3); }
    .icon .btn { border-radius: 50%; }
    .icon .part { padding: 5px; }
    .icon #label, .icon .caret, .icon .side { display: none; }
    .icon .ico { width: 16px; height: 16px; }`;

  function isMain(video) {
    // a player in a frame the page covers (below): it is the page's video
    return TOP && (video.tagName === "IFRAME" || video === biggestVideo());
  }

  function directSrc(video) {
    if (video.tagName === "IFRAME") return "";
    const src = video.currentSrc || video.src || "";
    return /^https?:/i.test(src) && !/\.m3u8|\.mpd/i.test(src) ? src : "";
  }

  // where the bubbles live: in full screen the player's own element (the
  // only thing the browser shows then)
  function layer() {
    const f = document.fullscreenElement || document.webkitFullscreenElement;
    return f && f.tagName !== "VIDEO" ? f : document.documentElement;
  }

  const el = (tag, props = {}, ...kids) => {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(props)) {
      if (k === "text") e.textContent = v;
      else e.setAttribute(k, v);
    }
    e.append(...kids);
    return e;
  };

  function makeBubble(video) {
    const host = document.createElement("div");
    host.style.all = "initial";
    host.style.position = "fixed";
    host.style.zIndex = "2147483647";
    host.style.setProperty("--z", String(zoom));
    host.style.setProperty("--o", String(solid));
    const root = host.attachShadow({ mode: "closed" });
    // early.js keeps the page's own player from taking presses on it
    (globalThis.__ezooRoots = globalThis.__ezooRoots || new WeakMap()).set(host, root);
    const svgNs = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(svgNs, "svg");
    svg.setAttribute("class", "ico");
    svg.setAttribute("viewBox", "0 0 16 16");
    const circle = document.createElementNS(svgNs, "circle");
    for (const [k, v] of [["cx", "8"], ["cy", "8"], ["r", "7.5"], ["fill", "#fff"]]) circle.setAttribute(k, v);
    const arrow = document.createElementNS(svgNs, "path");
    arrow.setAttribute("d", "M6.6 3.5h2.8v4.6h2.3L8 12 4.3 8.1h2.3z");
    arrow.setAttribute("fill", "#1d63d6");
    svg.append(circle, arrow);
    const list = el("div", { class: "list", id: "list" });
    list.hidden = true;
    const spin = el("span", { class: "spin", id: "spin" });
    spin.hidden = true;
    root.append(
      el("style", { text: STYLE }),
      el(
        "div",
        { class: "wrap " + look },
        el(
          "div",
          { class: "btn", id: "main" },
          el(
            "span",
            { class: "part", id: "grab", tabindex: "0", role: "button", "aria-haspopup": "true", "aria-label": "Download this video" },
            svg,
            spin,
            el("span", { id: "label", text: "Download this video" }),
            el("span", { class: "caret", text: "▼" })
          ),
          el("span", { class: "side", id: "help", title: "Help", text: "?", tabindex: "0", role: "button", "aria-label": "Help" }),
          el("span", { class: "side", id: "close", title: "Hide", text: "✕", tabindex: "0", role: "button", "aria-label": "Hide" })
        ),
        list
      )
    );
    const b = { host, root, video, open: false, own: null, loading: false, dx: null, dy: null, help: false, place: sitePlace, auto: null };
    const grab = root.getElementById("grab");
    // drag: move it anywhere on the video; a short press is a click
    let down = null;
    grab.addEventListener("mousedown", (e) => {
      if (e.button !== 0 || !userPress(e)) return;
      const hr = host.getBoundingClientRect();
      down = { x: e.clientX, y: e.clientY, ox: e.clientX - hr.left, oy: e.clientY - hr.top, moved: false };
      e.preventDefault();
    });
    addEventListener("mousemove", (e) => {
      if (!down) return;
      if (!down.moved && Math.abs(e.clientX - down.x) + Math.abs(e.clientY - down.y) < 5) return;
      down.moved = true;
      root.getElementById("main").classList.add("drag");
      const r = video.getBoundingClientRect();
      b.dx = e.clientX - down.ox - r.left;
      b.dy = e.clientY - down.oy - r.top;
      placeOne(b);
    });
    const up = (e) => {
      if (!down || !userPress(e)) return;
      const wasDrag = down.moved;
      down = null;
      root.getElementById("main").classList.remove("drag");
      if (!wasDrag) return clicked(b);
      // dropped: it goes to the nearest edge of the video, and stays there on
      // this site (its other videos, next time too)
      const r = video.getBoundingClientRect();
      const main = root.getElementById("main").getBoundingClientRect();
      const roomX = Math.max(1, r.width - main.width - 16);
      const roomY = Math.max(1, r.height - main.height - 16);
      const at = { x: Math.min(1, Math.max(0, (b.dx - 8) / roomX)), y: Math.min(1, Math.max(0, (b.dy - 8) / roomY)) };
      b.place = snapToEdge(at);
      b.dx = null;
      b.dy = null;
      keepPlace(b.place);
      placeOne(b);
    };
    addEventListener("mouseup", up);
    // heard here when the page's player keeps it from the window (early.js)
    root.addEventListener("mouseup", up);
    const press = (id, run) => {
      const x = root.getElementById(id);
      x.addEventListener("click", (e) => userPress(e) && run());
      x.addEventListener("keydown", (e) => {
        if ((e.key === "Enter" || e.key === " ") && e.isTrusted) {
          e.preventDefault();
          e.stopPropagation();
          run();
        }
      });
    };
    press("help", () => {
      b.help = !b.help;
      b.open = true;
      renderOne(b);
    });
    press("close", () => {
      b.dismissed = true;
      host.remove();
    });
    // the keyboard: Enter / Space opens it like a click
    grab.addEventListener("keydown", (e) => {
      if ((e.key === "Enter" || e.key === " ") && e.isTrusted) {
        e.preventDefault();
        e.stopPropagation();
        clicked(b);
      }
    });
    // one player for many videos (the previews of a home page, a feed): a
    // new video in it forgets what was found for the one before
    // (only when it really is another video: a site's own player changes
    // its blob: source and empties itself often for the same video)
    video.addEventListener("emptied", () => {
      if (stale(b)) forget(b);
    });
    video.addEventListener("loadstart", () => {
      if (stale(b)) forget(b);
    });
    layer().appendChild(host);
    bubbles.set(video, b);
    renderOne(b);
    placeOne(b);
    return b;
  }


  // which video a player shows now: its own link, else its own file (a
  // blob: source says nothing: the site makes a new one for the same video)
  function videoKey(video) {
    return ownLink(video) || directSrc(video) || "";
  }

  // what was found (or is being looked up) is for another video now
  function stale(b) {
    const key = videoKey(b.video);
    if (b.own) return b.ownFor !== key;
    if (b.loading) return b.asked !== key;
    return false;
  }

  // what was found belongs to another video of this player now
  function forget(b) {
    if (!b.own && !b.loading) return;
    b.own = null;
    b.ownFor = "";
    b.loading = false;
    b.open = false;
    renderOne(b);
  }

  function itemsOf(b) {
    if (b.own && b.ownFor !== videoKey(b.video)) {
      b.own = null;
      b.ownFor = "";
    }
    if (b.own) return b.own;
    // the page's own video only: a player that shows another video (a
    // preview on a home page) is looked up for that video
    if (!isMain(b.video)) return [];
    const mine = ownLink(b.video);
    return !mine || isVideoPage(location.href) || mine === cleanLink(location.href) ? items : [];
  }

  function lookUp(b, stream) {
    // another video on the page (or nothing found for it yet): look it up now
    b.loading = true;
    b.open = true;
    const asked = videoKey(b.video);
    b.asked = asked;
    renderOne(b, "Looking for the video qualities…");
    chrome.runtime.sendMessage(
      // its own post / video link, or its own file; the page only when
      // neither is there
      {
        type: "swiftget-resolve-video",
        resolve: stream || "",
        // in a frame that is only a player: the add-in uses the page around it
        page: ownLink(b.video) || (directSrc(b.video) || !TOP || inFeed() ? "" : location.href),
        feed: TOP && inFeed(),
        src: directSrc(b.video),
        frame: !TOP,
      },
      (res) => {
        b.loading = false;
        if (chrome.runtime.lastError || !res) return notRunning(b, () => lookUp(b, stream));
        if (res.error) {
          if (/not running/i.test(res.error)) return notRunning(b, () => lookUp(b, stream));
          return renderOne(b, res.error, true);
        }
        // the player went on to another video meanwhile: not its qualities
        if (videoKey(b.video) !== asked) {
          b.open = false;
          return renderOne(b);
        }
        b.own = res.items || [];
        b.ownFor = asked;
        renderOne(b);
      }
    );
  }

  function clicked(b) {
    const list = itemsOf(b);
    if (!list.length && !b.loading) return lookUp(b);
    b.help = false;
    b.open = !b.open;
    renderOne(b);
  }

  // The app is closed: offer to open it, then do what the user wanted.
  function notRunning(b, retry) {
    b.open = true;
    renderOne(b, "Ezoo Download is not running.", true, {
      text: "Open Ezoo Download",
      run: () => {
        // the program registers "ezoo-download:" when it is installed
        const a = document.createElement("a");
        a.href = "ezoo-download://open";
        a.style.display = "none";
        document.documentElement.appendChild(a);
        a.click();
        a.remove();
        renderOne(b, "Opening Ezoo Download…");
        let tries = 0;
        const wait = setInterval(() => {
          tries++;
          chrome.runtime.sendMessage({ type: "swiftget-status" }, (res) => {
            if (chrome.runtime.lastError) return;
            if (res && res.ok) {
              clearInterval(wait);
              retry();
            } else if (tries >= 25) {
              clearInterval(wait);
              renderOne(b, "Ezoo Download did not start. Open it from the Start menu and try again.", true);
            }
          });
        }, 1000);
      },
    });
  }

  function heightOf(q) {
    const m = /^(\d{3,4})p/.exec(q || "");
    return m ? Number(m[1]) : 0;
  }

  // One line of the menu, in plain words: "MP4 file · 1080p [Full HD] AV1",
  // under it "12:34 · 2,500 kbps · 245 MB".
  function describe(m) {
    const y = m.yt;
    if (!y) return { main: m.label, tag: "", codec: "", sub: m.sub || "" };
    const audio = y.quality === "audio" || y.quality === "mp3" || /^audio\|/.test(y.key || "");
    const ext = (y.ext || "mp4").toUpperCase();
    const q = audio ? "" : y.quality || "";
    const tag = audio ? "" : y.tag || tagOf(heightOf(q));
    const codec = audio ? "" : y.codec || (/\bAV1\b/.test(m.label || "") ? "AV1" : "");
    const kbps = y.kbps || (y.size && y.duration ? Math.round((y.size * 8) / y.duration / 1000) : 0);
    const sub = [
      fmtLen(y.duration),
      kbps ? `${kbps.toLocaleString("en")} kbps` : "",
      y.size ? (y.quality === "mp3" ? "about " : "") + fmtSize(y.size) : "",
      y.quality === "mp3" ? "made into MP3 after the download" : "",
      // some players play only the sound of an AV1 video
      codec === "AV1" ? "AV1: smaller file, needs a newer player" : "",
    ]
      .filter(Boolean)
      .join(" · ");
    const kind = audio ? `${ext} audio` : `${ext} file`;
    return { main: q ? `${kind} · ${q}` : kind, tag, codec, sub };
  }

  // "Download all": the whole playlist (YouTube list), or every file found
  function downloadAllOf(b, usable) {
    if (TOP && /youtube\.com/i.test(location.hostname) && /[?&]list=/.test(location.search)) {
      return { text: "Download all videos of this playlist", run: () => sendPlaylist(b) };
    }
    const files = usable.filter((m) => !m.yt && m.url);
    if (files.length >= 2) {
      return {
        text: `Download all (${files.length} files)`,
        run: () => files.forEach((m, i) => setTimeout(() => download(b, m), i * 300)),
      };
    }
    return null;
  }

  function sendPlaylist(b) {
    chrome.runtime.sendMessage({ type: "swiftget-playlist", url: location.href }, (res) => {
      if (chrome.runtime.lastError || !res || !res.ok) return notRunning(b, () => sendPlaylist(b));
      flashOne(b, "The playlist is open in Ezoo Download ✔", false);
    });
  }

  // the list may open up or down: decided again after every change of it
  function renderOne(b, note, isError, action) {
    renderBubble(b, note, isError, action);
    // a protected (DRM) video: the button says so with a lock
    const locked = /🔒/.test(note || "") || itemsOf(b).some((i) => i.drm);
    b.root.getElementById("main").classList.toggle("locked", locked);
    placeOne(b);
  }

  function renderBubble(b, note, isError, action) {
    const list = itemsOf(b);
    const usable = list.filter((i) => !i.disabled);
    const busy = b.loading || (!usable.length && list.some((i) => i.disabled && /Getting/.test(i.label)));
    b.root.getElementById("label").textContent = usable.length > 1 ? `Download this video (${usable.length})` : "Download this video";
    b.root.getElementById("spin").hidden = !busy;
    b.root.querySelector("svg").style.display = busy ? "none" : "";
    b.root.getElementById("main").classList.toggle("open", b.open);
    const box = b.root.getElementById("list");
    box.hidden = !b.open;
    box.replaceChildren();
    if (b.help) {
      box.appendChild(
        el("div", {
          class: "help",
          text: "Click the button (or press Enter on it) to choose a quality. Drag it to an edge of the video: it stays there on this site. ✕ hides it for this video. Its size can be changed in the Ezoo Download add-in menu.",
        })
      );
      const never = el("div", { class: "act", tabindex: "0", role: "button", text: "Don't show the button on this site" });
      never.addEventListener("click", (e) => userPress(e) && noButtonOnThisSite());
      never.addEventListener("keydown", (e) => e.key === "Enter" && e.isTrusted && noButtonOnThisSite());
      box.appendChild(never);
    }
    if (note) {
      // dir "auto": a note in Arabic reads right to left (its full stop at the end)
      const d = el("div", { class: "msg" + (isError ? " err" : ""), dir: "auto", text: note });
      if (action) {
        const a = el("div", { class: "act", text: action.text });
        a.addEventListener("click", action.run);
        d.append(el("br"), a);
      }
      box.appendChild(d);
      return;
    }
    if (!list.length) {
      box.appendChild(el("div", { class: "msg", text: "Looking for the video qualities…" }));
      return;
    }
    const title = (list.find((m) => m.yt && m.yt.title) || {}).yt;
    box.appendChild(el("div", { class: "head", dir: "auto", title: (title && title.title) || cleanTitle(), text: (title && title.title) || cleanTitle() }));
    // two tabs: the whole video (never a part), or a part of it
    const partOk = usable.some(canCut);
    const partTab = b.clipMode && partOk;
    if (partOk) {
      const tab = (on, text, part) => {
        const t = el("div", { class: "tab" + (part ? " part" : "") + (on ? " on" : ""), text });
        t.addEventListener("click", () => {
          if (b.clipMode === part) return;
          b.clipMode = part;
          if (part && !b.clip) {
            const v = b.video;
            const at = Math.floor(v.currentTime || 0);
            const dur = Number.isFinite(v.duration) ? v.duration : 0;
            b.clip = { from: at, to: dur ? Math.min(dur, at + 30) : at + 30, exact: false };
          }
          if (!part) stopPart(b);
          renderOne(b);
        });
        return t;
      };
      box.appendChild(el("div", { class: "tabs" }, tab(!partTab, "⬇ Whole video", false), tab(partTab, "✂ Part", true)));
    }
    if (partTab) {
      box.appendChild(clipPanel(b, usable));
    } else {
      const all = downloadAllOf(b, usable);
      if (all) {
        const row = el("div", { class: "item" }, el("span", { class: "num", text: "⇊" }), el("div", { class: "body" }, el("div", { class: "t all", text: all.text })));
        row.addEventListener("click", all.run);
        box.appendChild(row);
      }
    }
    if (partTab) return;
    let n = 0;
    list.forEach((m) => {
      const d = describe(m);
      const t = el("div", { class: "t" }, el("span", { text: d.main }));
      if (d.tag) t.append(el("span", { class: "tag " + d.tag.replace(/\s/g, "").replace(/^(\d)K$/, "K$1"), text: d.tag }));
      if (d.codec) t.append(el("span", { class: "codec", text: d.codec }));
      const sub = d.sub.length > 110 ? d.sub.slice(0, 110) + "…" : d.sub;
      const row = el(
        "div",
        { class: "item" + (m.disabled ? " off" : "") },
        el("span", { class: "num", text: m.disabled ? "" : `${++n}.` }),
        el("div", { class: "body" }, t, el("div", { class: "s", text: sub }))
      );
      if (!m.disabled) {
        row.setAttribute("tabindex", "0");
        row.setAttribute("role", "button");
        row.addEventListener("click", () => download(b, m));
        row.addEventListener("keydown", (e) => {
          if (e.key === "Enter" && e.isTrusted) {
            e.preventDefault();
            download(b, m);
          }
        });
      }
      box.appendChild(row);
    });
  }

  // a quality that can be cut: a video / sound the app downloads itself
  function canCut(m) {
    if (m.disabled || m.drm || m.resolve) return false;
    if (m.yt) return true;
    return !!m.url && /\.(mp4|m4v|webm|mkv|mov|m3u8|mp3|m4a)(\?|$)/i.test(m.url);
  }

  // keys typed in our boxes stay ours (not the player's shortcuts)
  function keepKeys(input) {
    for (const t of ["keydown", "keyup", "keypress"]) input.addEventListener(t, (e) => e.stopPropagation());
  }

  // a short name of a quality for the part ("1080p", "MP3")
  function chipOf(m) {
    const y = m.yt;
    if (y) {
      if (y.quality === "mp3") return "MP3";
      if (y.quality === "audio" || /^audio\|/.test(y.key || "")) return (y.ext || "M4A").toUpperCase();
      return (y.quality || (y.ext || "MP4").toUpperCase()) + (y.codec ? " " + y.codec : "");
    }
    const h = heightOf(m.label || "");
    return h ? `${h}p` : (m.ext || (m.url || "").split("?")[0].split(".").pop() || "file").toUpperCase();
  }

  // The "✂ Part" tab: From / To, quick or exact, the quality, a look at the
  // part on the page's own player, and its own button
  function clipPanel(b, usable) {
    const c = b.clip;
    const v = b.video;
    const dur = Number.isFinite(v.duration) ? v.duration : 0;
    const choices = usable.filter(canCut);
    if (!choices.some((m) => m === b.cutPick)) {
      b.cutPick = choices.find((m) => m.yt && m.yt.quality === "720p") || choices.find((m) => heightOf(m.label || "") === 720) || choices[0];
    }
    const len = el("span", { class: "len" });
    const ok = () => c.to > c.from && (!dur || c.from < dur);
    const showLen = () => {
      len.textContent = ok() ? clock(c.to - c.from) : "!";
      len.title = ok() ? "Length of the part" : "The end must come after the start.";
      len.classList.toggle("err", !ok());
    };
    const time = (label, key) => {
      const input = el("input", { type: "text", value: clock(c[key]), "aria-label": label });
      keepKeys(input);
      input.addEventListener("input", () => {
        const t = parseClock(input.value);
        input.classList.toggle("bad", t == null);
        if (t != null) {
          c[key] = dur ? Math.min(t, dur) : t;
          showLen();
        }
      });
      const now = el("span", { class: "now", title: "The time the video is at now", text: "⏱ Now" });
      now.addEventListener("click", () => {
        c[key] = Math.floor(v.currentTime || 0);
        input.value = clock(c[key]);
        input.classList.remove("bad");
        showLen();
      });
      return { label: el("b", { text: label }), input, now };
    };
    const from = time("From", "from");
    const to = time("To", "to");
    const seg = el("div", { class: "seg" });
    const showSeg = () => {
      seg.replaceChildren();
      for (const [exact, text, words] of [
        [false, "Quick cut", "Takes seconds, the quality stays as it is; it may start a second or two early."],
        [true, "Exact cut", "Starts on the very second you chose; the picture is made again, so it takes longer."],
      ]) {
        const k = el("span", { class: c.exact === exact ? "on" : "", text, title: words });
        k.addEventListener("click", () => {
          c.exact = exact;
          showSeg();
        });
        seg.appendChild(k);
      }
    };
    const qs = el("div", { class: "qs" });
    const showQs = () => {
      qs.replaceChildren();
      for (const m of choices) {
        const d = describe(m);
        const q = el("span", { class: m === b.cutPick ? "on" : "", text: chipOf(m), title: [d.main, d.sub].filter(Boolean).join(" · ") });
        q.addEventListener("click", () => {
          b.cutPick = m;
          showQs();
        });
        qs.appendChild(q);
      }
    };
    const preview = el("span", { class: "pv", text: "▶ Preview" });
    preview.addEventListener("click", () => playPart(b));
    const go = el("span", { class: "go", text: "✂ Download the part" });
    go.addEventListener("click", () => b.cutPick && downloadPart(b, b.cutPick));
    const box = el(
      "div",
      { class: "cut" },
      el("div", { class: "lead", text: "Only the part you choose is downloaded" }),
      el("div", { class: "times" }, from.label, from.input, to.label, to.input, to.now, len),
      seg,
      qs,
      el("div", { class: "acts" }, preview, go)
    );
    showLen();
    showSeg();
    showQs();
    return box;
  }

  // the page's own player plays the part (nothing is downloaded)
  function playPart(b) {
    const v = b.video;
    const c = b.clip;
    if (!(c.to > c.from)) return;
    stopPart(b);
    const stop = () => {
      if (v.currentTime >= c.to || v.currentTime < c.from - 1) {
        v.pause();
        stopPart(b);
      }
    };
    b.partWatch = stop;
    try {
      v.currentTime = c.from;
      v.addEventListener("timeupdate", stop);
      const p = v.play();
      if (p && p.catch) p.catch(() => {});
    } catch (_) {
      // a player that does not let us move it: nothing to show
    }
  }

  function stopPart(b) {
    if (b.partWatch) b.video.removeEventListener("timeupdate", b.partWatch);
    b.partWatch = null;
  }

  function downloadPart(b, m) {
    const c = b.clip;
    const dur = Number.isFinite(b.video.duration) ? b.video.duration : 0;
    if (!(c.to > c.from) || (dur && c.from >= dur)) return flashOne(b, "The end must come after the start.", true);
    const clip = { start: c.from, end: dur ? Math.min(c.to, dur) : c.to, exact: !!c.exact };
    const fileName = `${cleanTitle()}.${m.ext || "mp4"}`;
    stopPart(b);
    chrome.runtime.sendMessage(
      { type: "swiftget-download", item: m, title: document.title, fileName, referrer: location.href, clip, part: true },
      (res) => {
        if (chrome.runtime.lastError || !res || !res.ok) notRunning(b, () => downloadPart(b, m));
        else {
          b.clipMode = false;
          flashOne(b, `The part ${clock(clip.start)} → ${clock(clip.end)} was sent to Ezoo Download ✔`, false);
        }
      }
    );
  }

  // a free place (fractions of the video, after a drag) put on its nearest
  // edge; near a corner, in the corner
  function snapToEdge(at) {
    const d = { left: at.x, right: 1 - at.x, top: at.y, bottom: 1 - at.y };
    const edge = Object.keys(d).reduce((a, k) => (d[a] <= d[k] ? a : k));
    const c = (n) => (n < 0.08 ? 0 : n > 0.92 ? 1 : n);
    if (edge === "left") return { x: 0, y: c(at.y) };
    if (edge === "right") return { x: 1, y: c(at.y) };
    if (edge === "top") return { x: c(at.x), y: 0 };
    return { x: c(at.x), y: 1 };
  }

  // is one of the page's own buttons (much smaller than the video) under this spot?
  function onPageControl(b, left, top, w, h, r) {
    const area = (x) => Math.max(0, x.width) * Math.max(0, x.height);
    const small = (e) => area(e.getBoundingClientRect()) < area(r) / 5;
    const points = [[left + w / 2, top + h / 2], [left + 3, top + 3], [left + w - 3, top + 3]];
    for (const [px, py] of points) {
      let list = [];
      try {
        list = document.elementsFromPoint(px, py);
      } catch (_) {
        return false;
      }
      for (const e of list) {
        if (e === b.host) continue;
        if (e === b.video || e.contains(b.video)) break;
        const c = e.closest && e.closest("button, [role=button], a[href], input, select");
        if (c && !c.contains(b.video) && small(c)) return true;
      }
    }
    return false;
  }

  // where the button sits on its video: the user's place on this site, else
  // the top right corner, or the top left one when a button of the page's
  // own is there (kept a moment, so it does not jump about)
  function spotFor(b, r, w, h) {
    if (b.place) return b.place;
    const now = Date.now();
    if (b.auto && b.auto.v === b.video && now - b.auto.t < 600) return b.auto.at;
    let at = { x: 1, y: 0 };
    for (const c of [{ x: 1, y: 0 }, { x: 0, y: 0 }]) {
      const left = r.left + 8 + c.x * Math.max(0, r.width - w - 16);
      const top = r.top + 8 + c.y * Math.max(0, r.height - h - 16);
      if (!onPageControl(b, left, top, w, h, r)) {
        at = c;
        break;
      }
    }
    b.auto = { v: b.video, at, t: now };
    return at;
  }

  function placeOne(b) {
    const fs = document.fullscreenElement || document.webkitFullscreenElement;
    const r = b.video.getBoundingClientRect();
    // closed with its X: it stays closed (also after full screen, a new size…)
    // until the page shows another video; in full screen only when the user
    // asked for it (the add-in's window)
    const onScreen =
      !b.dismissed && (!fs || showInFullscreen) &&
      r.bottom > 0 && r.top < innerHeight && r.width >= 200 && r.height >= 100 && b.video.isConnected &&
      !(fs && fs.tagName === "VIDEO") && (!fs || fs.contains(b.video) || fs.shadowRoot);
    b.host.style.display = onScreen ? "block" : "none";
    if (!onScreen) return;
    if (b.host.parentNode !== layer()) layer().appendChild(b.host);
    // the size of the button itself (the user can make it smaller or bigger);
    // its list floats and never moves it
    const mainRect = b.root.getElementById("main").getBoundingClientRect();
    const w = mainRect.width || 62;
    const h = mainRect.height || 22;
    let x;
    let y;
    if (b.dx != null) {
      // being dragged
      x = r.left + b.dx;
      y = r.top + b.dy;
    } else {
      const at = spotFor(b, r, w, h);
      x = r.left + 8 + at.x * Math.max(0, r.width - w - 16);
      y = r.top + 8 + at.y * Math.max(0, r.height - h - 16);
    }
    x = Math.max(r.left, Math.min(x, r.right - 60));
    y = Math.max(r.top, Math.min(y, r.bottom - h));
    const left = Math.max(4, Math.min(x, innerWidth - w));
    const top = Math.max(4, Math.min(y, innerHeight - h - 4));
    b.host.style.left = `${left}px`;
    b.host.style.top = `${top}px`;
    // where the list opens: away from the near edge of the screen
    const wrap = b.root.querySelector(".wrap");
    if (wrap) {
      const listW = 330 * zoom;
      const listH = Math.min(380, (b.root.getElementById("list") || {}).scrollHeight || 380) * zoom;
      wrap.classList.toggle("toright", left + w - listW < 4);
      wrap.classList.toggle("up", top + h + listH > innerHeight - 4 && top - listH > 4);
    }
  }

  function place() {
    if (!panelAllowed()) return;
    const vids = allVideos().filter((v) => {
      const r = v.getBoundingClientRect();
      return r.width >= 200 && r.height >= 100;
    });
    // bubbles of videos that went away
    for (const [v, b] of bubbles) {
      if (!v.isConnected) {
        b.host.remove();
        bubbles.delete(v);
      }
    }
    // the main video once the add-in found media or it plays (then it shows
    // "looking for qualities"), any other one as soon as it is a real player
    for (const v of vids.slice(0, 12)) {
      let b = bubbles.get(v);
      if (!b) {
        const show = isMain(v) ? items.length || playing(v) : !(v.paused && !(v.duration > 3) && !directSrc(v));
        if (!show) continue;
        b = makeBubble(v);
      }
      if (!b.dismissed) placeOne(b);
    }
    // the video plays in a frame the page lays a see-through layer over
    // (Google Drive): the frame's own button can be seen but never pressed,
    // so this page shows one over the frame (with what was found for it)
    if (TOP && items.length && !vids.length) {
      for (const f of coveredPlayers()) {
        const b = bubbles.get(f) || makeBubble(f);
        if (!b.dismissed) placeOne(b);
      }
    }
  }

  // big frames whose top right corner (where the button is) the page covers
  function coveredPlayers() {
    const out = [];
    for (const f of document.querySelectorAll("iframe")) {
      const r = f.getBoundingClientRect();
      if (r.width < 300 || r.height < 170 || r.bottom < 60 || r.top > innerHeight - 60) continue;
      const x = Math.min(r.right - 40, innerWidth - 5);
      const y = Math.max(r.top + 16, 5);
      const top = document.elementFromPoint(x, y);
      if (top && top !== f && !(bubbles.get(f) && top === bubbles.get(f).host)) out.push(f);
      if (out.length >= 2) break;
    }
    return out;
  }

  function flashOne(b, text, isError) {
    b.open = true;
    renderOne(b, text, isError);
    setTimeout(() => {
      b.open = false;
      renderOne(b);
    }, 2500);
  }

  function download(b, m) {
    // a stream whose qualities are not known yet: ask the app for them
    if (m.resolve) return lookUp(b, m.resolve);
    const fileName = `${cleanTitle()}.${m.ext || "mp4"}`;
    chrome.runtime.sendMessage(
      { type: "swiftget-download", item: m, title: document.title, fileName, referrer: location.href },
      (res) => {
        if (chrome.runtime.lastError || !res || !res.ok) notRunning(b, () => download(b, m));
        else flashOne(b, "Sent to Ezoo Download ✔", false);
      }
    );
  }

  function update(newItems) {
    if (!TOP) return; // frames look up their own videos
    items = newItems || [];
    if (!items.length) {
      // new page / new video: forget the bubbles and "hidden"
      for (const b of bubbles.values()) b.host.remove();
      bubbles.clear();
      return;
    }
    if (!panelAllowed()) return;
    place();
    for (const b of bubbles.values()) if (!b.own && !b.loading) renderOne(b);
  }

  // File links (.zip, .exe, .mp4…): a click goes straight to Ezoo Download,
  // so the browser never starts its own download (and never asks where to
  // save it). The rules are the program's own (Options > File Types).
  const wildcard = (pattern, text) =>
    new RegExp(
      "^" + String(pattern).toLowerCase().replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$"
    ).test(String(text).toLowerCase());
  let linkRules = null;
  const refreshLinkRules = () =>
    chrome.runtime.sendMessage({ type: "swiftget-link-rules" }, (res) => {
      linkRules = chrome.runtime.lastError ? null : res || null;
    });
  refreshLinkRules();
  setInterval(refreshLinkRules, 60000);
  // shown by the browser itself, not downloaded: never taken from a click
  // pages and pictures open in the browser; a PDF too, unless PDF is one of
  // the file types the app takes (then a click on its link goes to the app:
  // once it is open in the browser's PDF viewer, the viewer's own download
  // button always asks where to save, and that can't be stopped)
  const VIEWED = /^(html?|xhtml|php\d?|aspx?|jsp|cgi|pl|cfm|do|action|json|xml|txt|css|js|jpe?g|png|gif|webp|svg|bmp|ico|avif)$/;
  const keyHeld = (e, k) => (k === "alt" ? e.altKey : k === "ctrl" ? e.ctrlKey : k === "shift" ? e.shiftKey : false);
  function wantedLink(e, href) {
    const r = linkRules;
    if (!r || !switchedOn) return false;
    let u;
    try {
      u = new URL(href);
    } catch {
      return false;
    }
    if (!/^https?:$/.test(u.protocol)) return false;
    if (r.skipKey !== "none" && keyHeld(e, r.skipKey)) return false;
    const forced = r.forceKey !== "none" && keyHeld(e, r.forceKey);
    // Ctrl / Shift / Meta + click: a new tab or window, unless that is the "take it" key
    if (!forced && (e.ctrlKey || e.shiftKey || e.metaKey || e.altKey)) return false;
    const host = u.hostname.toLowerCase();
    if ((r.skipSites || []).some((s) => hostMatches(host, s))) return false;
    if ((r.skipAddresses || []).some((a) => wildcard(a, href))) return false;
    const last = decodeURIComponent(u.pathname.split("/").pop() || "");
    const dot = last.lastIndexOf(".");
    const ext = dot > 0 ? last.slice(dot + 1).toLowerCase() : "";
    if (!ext || VIEWED.test(ext)) return false;
    if (ext === "pdf" && !(r.exts || []).some((x) => wildcard(x, ext))) return false;
    // a .torrent file opens in the program's torrent window
    if (forced || ext === "torrent") return true;
    return (r.exts || []).some((x) => wildcard(x, ext));
  }
  // Heard after the page's own click code (not before it, and the click is
  // not stopped): file sites often make their link work in that code (a
  // request saying "the download starts", a cookie); stopped, the link stayed
  // "403 Forbidden" for the program while the browser and other programs got
  // the file. Only the browser's own download is held back.
  addEventListener(
    "click",
    (e) => {
      if (e.button !== 0 || e.defaultPrevented) return;
      const a = e.composedPath ? e.composedPath().find((n) => n && n.tagName === "A" && n.href) : null;
      if (!a || a.dataset.ezooPass) return;
      const href = String(a.href);
      if (!wantedLink(e, href)) return;
      e.preventDefault();
      chrome.runtime.sendMessage({ type: "swiftget-link", url: href }, (res) => {
        // not taken (the app is closed, the user chose the browser): the browser's way
        if (chrome.runtime.lastError || !res || !res.ok) {
          const tmp = document.createElement("a");
          tmp.href = href;
          if (a.hasAttribute("download")) tmp.setAttribute("download", a.getAttribute("download") || "");
          tmp.dataset.ezooPass = "1";
          document.documentElement.appendChild(tmp);
          tmp.click();
          tmp.remove();
        }
      });
    },
    false
  );

  // magnet links of torrents go to Ezoo Download (Alt + click: to the browser)
  addEventListener(
    "click",
    (e) => {
      if (e.button !== 0 || e.altKey || e.defaultPrevented || !switchedOn) return;
      const a = e.composedPath ? e.composedPath().find((n) => n && n.tagName === "A" && n.href) : null;
      const href = a ? String(a.href) : "";
      if (!/^magnet:\?/i.test(href) || !/xt=urn:bt[im]h:/i.test(href)) return;
      e.preventDefault();
      e.stopPropagation();
      chrome.runtime.sendMessage({ type: "swiftget-magnet", url: href }, (res) => {
        // not taken (the app is closed, or sending is off): the browser's way
        if (chrome.runtime.lastError || !res || !res.ok) location.href = href;
      });
    },
    true
  );

  // a click anywhere else closes an open menu
  addEventListener(
    "mousedown",
    (e) => {
      const path = e.composedPath ? e.composedPath() : [];
      for (const b of bubbles.values()) {
        if (b.open && !path.includes(b.host)) {
          b.open = false;
          b.help = false;
          renderOne(b);
        }
      }
    },
    true
  );
  addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    for (const b of bubbles.values()) {
      if (b.open) {
        b.open = false;
        renderOne(b);
      }
    }
  });
  for (const ev of ["fullscreenchange", "webkitfullscreenchange"]) {
    document.addEventListener(ev, () => setTimeout(() => bubbles.forEach((b) => placeOne(b)), 50));
  }

  // every link of the page (or the selection) for "Download all links"
  function collectLinks(selectionOnly) {
    const sel = selectionOnly ? getSelection() : null;
    const inSel = (el) => !sel || (sel.rangeCount > 0 && sel.containsNode(el, true));
    const out = [];
    const seen = new Set();
    const add = (url, text) => {
      if (!url || !/^https?:/i.test(url)) return;
      url = url.split("#")[0];
      if (seen.has(url)) return;
      seen.add(url);
      out.push({ url, text: (text || "").replace(/\s+/g, " ").trim().slice(0, 200) });
    };
    // the page itself and the parts built as web components
    const roots = [document];
    try {
      for (const el of document.querySelectorAll("*")) if (el.shadowRoot) roots.push(el.shadowRoot);
    } catch (_) {
      /* a strange page */
    }
    for (const root of roots) {
      for (const a of root.querySelectorAll("a[href]")) if (inSel(a)) add(a.href, a.innerText || a.title);
      for (const el of root.querySelectorAll("img[src], video[src], audio[src], source[src], embed[src]")) {
        if (inSel(el)) add(el.src, el.alt || el.title || "");
      }
    }
    // addresses written as plain text (pages that show a list of links, a
    // note or a paste): what the user sees is what is collected
    const text = sel ? String(sel) : (document.body && document.body.innerText) || "";
    for (const m of text.slice(0, 2000000).matchAll(/\bhttps?:\/\/[^\s"'<>()\[\]{}]+/gi)) {
      add(m[0].replace(/[.,;:!?]+$/, ""), "");
      if (out.length > 5000) break;
    }
    return { page: location.href, links: out };
  }

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg.type === "swiftget-media") update(msg.items);
    if (msg.type === "swiftget-collect-links") {
      if (!TOP) return false; // the page itself answers
      sendResponse(collectLinks(!!msg.selection));
      return true;
    }
    if (msg.type === "swiftget-open-panel") {
      const v = biggestVideo();
      if (!v) return false;
      let b = bubbles.get(v);
      if (!b) b = makeBubble(v);
      b.dismissed = false;
      layer().appendChild(b.host);
      b.open = true;
      if (!itemsOf(b).length) lookUp(b);
      else renderOne(b);
      placeOne(b);
    }
    return false;
  });
  chrome.runtime.sendMessage({ type: "swiftget-config" }, (res) => {
    if (!chrome.runtime.lastError && res) {
      panel = res;
      if (!panelAllowed()) {
        for (const b of bubbles.values()) b.host.remove();
        bubbles.clear();
      }
    }
  });
  if (TOP) {
    // keys held while clicking a link: Alt = let the browser download it, etc.
    addEventListener(
      "mousedown",
      (e) => {
        chrome.runtime.sendMessage({ type: "swiftget-keys", alt: e.altKey, ctrl: e.ctrlKey, shift: e.shiftKey }).catch?.(() => {});
      },
      true
    );
    // media detected before this script loaded
    chrome.runtime.sendMessage({ type: "swiftget-media-request" }, (res) => {
      if (!chrome.runtime.lastError) update(res);
    });
  }

  addEventListener("scroll", place, { passive: true, capture: true });
  addEventListener("resize", place, { passive: true });
  setInterval(place, 700);
  setInterval(watchVideos, 1500);
  document.addEventListener("play", () => setTimeout(() => (watchVideos(), place()), 300), true);
  addEventListener("popstate", () => setTimeout(watchVideos, 500));
})();
