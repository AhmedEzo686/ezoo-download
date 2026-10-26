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
  const panelAllowed = () =>
    switchedOn && panel.enabled !== false && !(panel.skipSites || []).some((s) => hostMatches(location.hostname.toLowerCase(), s));

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
  const isNetflixWatch = () => /(^|\.)netflix\.com$/i.test(location.hostname) && /^\/watch(?:\/|$)/i.test(location.pathname || "");
  const NETFLIX_NOTICE = "Netflix playback is not supported by this downloader. Use Netflix’s official app on a supported device for offline viewing.";

  // In feeds (Facebook, Instagram, X...) the page shows many videos: look for
  // the post / reel link around the video being watched.
  function linkFor(video) {
    if (isVideoPage(location.href)) return location.href;
    return ownLink(video) || location.href;
  }

  // the post / video link next to this very video ("" = none found)
  function ownLink(video) {
    let el = video;
    for (let depth = 0; el && depth < 25; depth++) {
      const links = el.querySelectorAll ? el.querySelectorAll("a[href]") : [];
      for (const a of links) {
        if (isVideoPage(a.href)) return a.href.split("#")[0];
      }
      if (el.tagName === "ARTICLE") break;
      // out of a shadow root: on to the element that holds it
      el = el.parentElement || (el.parentNode && el.parentNode.host) || null;
    }
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
      chrome.runtime.sendMessage({ type: "swiftget-video-seen", page, frame: !TOP }).catch?.(() => {});
    }
  }

  // ---------------------------------------------------------------- bubbles
  // One "Download" bubble on every video of the page. The main video (the
  // one playing / the biggest) uses what the add-in found for the page; any
  // other video is looked up when its bubble is clicked. A bubble can be
  // dragged anywhere on its video and stays there.
  const bubbles = new Map(); // video -> bubble
  function removeAll() {
    for (const b of bubbles.values()) releaseBubble(b);
    bubbles.clear();
  }
  // size of the button: the slider in the add-in's menu (50% - 200%)
  const OLD_SIZES = { small: 0.85, normal: 1, large: 1.25 };
  const clampZoom = (z) => Math.min(2, Math.max(0.5, Number(z) || 1));
  let zoom = 1;
  const applyZoom = () => {
    for (const b of bubbles.values()) {
      b.host.style.setProperty("--z", String(zoom));
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
    chrome.storage.local.get({ bubbleScale: null, bubbleSize: "normal" }, (o) => {
      zoom = clampZoom(o && o.bubbleScale != null ? o.bubbleScale : OLD_SIZES[o && o.bubbleSize]);
      applyZoom();
    });
    chrome.storage.onChanged.addListener((ch) => {
      if (ch.bubbleScale) zoom = clampZoom(ch.bubbleScale.newValue);
      else if (ch.bubbleSize) zoom = clampZoom(OLD_SIZES[ch.bubbleSize.newValue]);
      else return;
      applyZoom();
    });
  } catch (_) {
    // storage not reachable (add-in reloading): default size
  }
  const STYLE = `
    :host { all: initial; --ez-surface: #fff; --ez-group: #f5f5f7; --ez-text: #1d1d1f; --ez-secondary: #5a5a60;
      --ez-line: #d2d2d7; --ez-accent: #007aff; --ez-link: #0063cc; --ez-hover: #edf4ff;
      --ez-success: #248a3d; --ez-error: #d70015; --ez-shadow: rgba(0,0,0,.18); }
    @media (prefers-color-scheme: dark) {
      :host { --ez-surface: #2c2c2e; --ez-group: #1c1c1e; --ez-text: #f5f5f7; --ez-secondary: #a1a1a6;
        --ez-line: #48484a; --ez-accent: #0a84ff; --ez-link: #64b0ff; --ez-hover: #353d4a;
        --ez-success: #30d158; --ez-error: #ff6961; --ez-shadow: rgba(0,0,0,.42); }
    }
    *, *::before, *::after { box-sizing: border-box; }
    [hidden] { display: none !important; }
    .wrap { position: relative; display: inline-flex; color-scheme: light dark; zoom: var(--z, 1);
      font: max(13px, calc(11px / var(--z, 1)))/1.4 -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif; color: var(--ez-text); }
    button { appearance: none; -webkit-appearance: none; font: inherit; color: inherit; margin: 0; cursor: pointer; }
    button:focus-visible { outline: 2px solid var(--ez-accent); outline-offset: -3px; }
    .btn { display: inline-flex; align-items: center; min-height: max(34px, calc(28px / var(--z, 1))); background: var(--ez-surface);
      border: 1px solid var(--ez-line); border-radius: 11px; box-shadow: 0 2px 8px var(--ez-shadow);
      user-select: none; white-space: nowrap; overflow: hidden; }
    .btn.open { border-color: var(--ez-accent); }
    .part { display: inline-flex; align-items: center; gap: 7px; padding: 7px 10px; min-height: max(34px, calc(28px / var(--z, 1)));
      background: transparent; border: 0; font-weight: 600; cursor: grab; }
    .part:hover, .side:hover { background: var(--ez-group); }
    .btn.drag .part { cursor: grabbing; }
    .ico { width: max(16px, calc(12px / var(--z, 1))); height: max(16px, calc(12px / var(--z, 1))); flex: none; stroke: currentColor; fill: none; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
    .download-symbol { color: var(--ez-link); }
    .caret { width: 12px; height: 12px; color: var(--ez-secondary); }
    .count { min-width: 18px; padding: 0 4px; border-radius: 5px; background: var(--ez-group); color: var(--ez-secondary);
      font-size: max(11px, calc(10px / var(--z, 1))); font-weight: 600; text-align: center; font-variant-numeric: tabular-nums; }
    .side { display: inline-flex; align-items: center; justify-content: center; min-width: max(28px, calc(24px / var(--z, 1))); height: max(28px, calc(24px / var(--z, 1)));
      background: transparent; border: 0; border-radius: 7px; color: var(--ez-secondary); }
    .sides { display: inline-flex; gap: 1px; align-items: center; padding: 0 3px; border-left: 1px solid var(--ez-line); }
    .spin { width: 16px; text-align: center; color: var(--ez-link); font-size: 17px; line-height: 16px; }
    .list { position: absolute; right: 0; top: calc(100% + 8px); width: max(340px, calc(250px / var(--z, 1))); max-width: calc(100vw - 16px);
      padding: 6px; max-height: 440px; overflow: auto; overscroll-behavior: contain;
      background: var(--ez-surface); color: var(--ez-text); border: 1px solid var(--ez-line); border-radius: 14px;
      box-shadow: 0 12px 30px -7px var(--ez-shadow); scrollbar-width: thin; scrollbar-color: var(--ez-line) transparent; }
    .list.above { top: auto; bottom: calc(100% + 8px); }
    .list.align-start { left: 0; right: auto; }
    .head { display: flex; align-items: start; gap: 10px; padding: 9px 8px 11px; border-bottom: 1px solid var(--ez-line); margin-bottom: 4px; }
    .heading-body { min-width: 0; flex: 1; }
    .heading { font-size: max(14px, calc(12px / var(--z, 1))); font-weight: 650; line-height: 1.4; }
    .video-title { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
      margin-top: 3px; font-size: max(12px, calc(11px / var(--z, 1))); line-height: 1.4; color: var(--ez-secondary); overflow-wrap: anywhere; }
    .group { margin: 0; }
    .group-title { margin: 0; padding: 10px 9px 5px; color: var(--ez-secondary); font-size: max(11px, calc(10px / var(--z, 1))); font-weight: 600;
      text-transform: uppercase; letter-spacing: .04em; }
    .item { display: flex; align-items: center; gap: 10px; width: 100%; min-height: max(56px, calc(40px / var(--z, 1))); padding: 9px 8px; text-align: left;
      border: 0; border-radius: 8px; background: transparent; }
    .item:hover:not(:disabled), .item:focus-visible { background: var(--ez-hover); }
    .format-symbol { display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px;
      border-radius: 8px; background: var(--ez-group); color: var(--ez-secondary); flex: none; }
    .body { display: block; flex: 1; min-width: 0; }
    .t { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; font-size: max(13px, calc(11px / var(--z, 1))); font-weight: 600; line-height: 1.4; }
    .s { display: block; color: var(--ez-secondary); font-size: max(12px, calc(11px / var(--z, 1))); margin-top: 3px; line-height: 1.4; overflow-wrap: anywhere; }
    .tag, .codec { display: inline-block; border: 1px solid var(--ez-line); border-radius: 4px; padding: 0 4px;
      font-size: max(10px, calc(10px / var(--z, 1))); font-weight: 600; line-height: 15px; color: var(--ez-secondary); white-space: nowrap; }
    .item > .accessory { color: var(--ez-link); width: 15px; height: 15px; }
    .all { color: var(--ez-link); font-weight: 600; }
    .item:disabled { cursor: default; }
    .item:disabled .format-symbol { opacity: .65; }
    .item:disabled .body { color: var(--ez-secondary); }
    .msg { display: flex; gap: 10px; padding: 14px 10px; font-size: max(13px, calc(11px / var(--z, 1))); line-height: 1.45; }
    .msg > .ico { margin-top: 2px; color: var(--ez-success); }
    .msg.err > .ico { color: var(--ez-error); }
    .msg.protected > .ico { color: var(--ez-secondary); }
    .msg.pending > .ico { color: var(--ez-link); }
    .msg-body { flex: 1; min-width: 0; overflow-wrap: anywhere; }
    .help { padding: 11px 10px; margin: 4px; border-radius: 8px; color: var(--ez-secondary); background: var(--ez-group); font-size: max(12px, calc(11px / var(--z, 1))); }
    .act { display: block; margin-top: 10px; min-height: 32px; border: 0; border-radius: 7px; padding: 6px 11px;
      background: #0063cc; color: #fff; font-weight: 600; }
    .act:hover { background: #0056b3; }
    @media (prefers-color-scheme: dark) { .act { background: #006bd6; } .act:hover { background: #005fbe; } }
    @media (prefers-contrast: more) { :host { --ez-line: var(--ez-secondary); } .btn, .list { border-width: 2px; } }
    @media (forced-colors: active) {
      .btn, .list { background: Canvas; color: CanvasText; border-color: CanvasText; box-shadow: none; }
      .item, .part, .side, .tag, .codec { color: CanvasText; }
      .item:focus-visible, button:focus-visible { outline-color: Highlight; }
      .act { background: Highlight; color: HighlightText; }
    }
    @media (prefers-reduced-motion: reduce) { *, *::before, *::after { animation: none !important; transition: none !important; } }
  `;

  function isMain(video) {
    return TOP && video === biggestVideo();
  }

  function directSrc(video) {
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

  const ICON_PATHS = {
    download: "M8 2v8M4.5 6.5 8 10l3.5-3.5M3 11v2.5h10V11",
    info: "M8 1.5a6.5 6.5 0 1 1 0 13 6.5 6.5 0 0 1 0-13M8 7v4M8 4.5v.1",
    close: "m4 4 8 8M12 4l-8 8",
    chevron: "m4 6 4 4 4-4",
    video: "M2 4h8v8H2zM10 6l4-2v8l-4-2",
    audio: "M6 11V3l7-1v8M6 4l7-1M6 11c0 1.1-1.1 2-2.5 2S1 12.3 1 11.5 2.1 10 3.5 10 6 10.2 6 11m7-1c0 1.1-1.1 2-2.5 2S8 11.3 8 10.5 9.1 9 10.5 9s2.5.2 2.5 1",
    check: "m3 8 3 3 7-7",
    warning: "M8 1.5 15 14H1zM8 6v3.5M8 12v.1",
    collection: "M5 2h9v9M2 5h9v9H2z",
    lock: "M3 7h10v7H3zM5 7V4a3 3 0 0 1 6 0v3M8 10v1.5",
  };
  function symbol(kind, className = "ico") {
    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("class", className);
    svg.setAttribute("viewBox", "0 0 16 16");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
    const path = document.createElementNS(ns, "path");
    path.setAttribute("d", ICON_PATHS[kind] || ICON_PATHS.download);
    svg.append(path);
    return svg;
  }

  function focusChoice(b, which = 0) {
    const choices = [...b.root.querySelectorAll(".item:not(:disabled), .act")];
    if (!choices.length) return false;
    const target = choices[Math.max(0, Math.min(which, choices.length - 1))];
    target.focus({ preventScroll: true });
    const box = b.root.getElementById("list");
    const r = target.getBoundingClientRect(), br = box.getBoundingClientRect();
    if (r.top < br.top + 6) box.scrollTop -= (br.top + 6 - r.top) / zoom;
    else if (r.bottom > br.bottom - 6) box.scrollTop += (r.bottom - br.bottom + 6) / zoom;
    return true;
  }

  function closeOne(b, restoreFocus = false) {
    b.open = false;
    b.help = false;
    b.focusMenu = false;
    renderOne(b);
    if (restoreFocus && !b.dismissed) b.root.getElementById("grab").focus({ preventScroll: true });
  }

  function releaseBubble(b) {
    b.cleanup?.();
    b.host.remove();
  }

  function makeBubble(video) {
    const host = document.createElement("div");
    host.style.all = "initial";
    host.style.position = "fixed";
    host.style.zIndex = "2147483647";
    host.style.colorScheme = "light dark";
    host.style.setProperty("--z", String(zoom));
    const root = host.attachShadow({ mode: "closed" });
    const list = el("section", { class: "list", id: "list", role: "dialog", "aria-label": "Video download options", "aria-modal": "false" });
    list.hidden = true;
    const spin = el("span", { class: "spin", id: "spin", "aria-hidden": "true", text: "…" });
    spin.hidden = true;
    root.append(
      el("style", { text: STYLE }),
      el("div", { class: "wrap" },
        el("div", { class: "btn", id: "main", role: "toolbar", "aria-label": "Ezoo video downloads" },
          el("button", { type: "button", class: "part", id: "grab", "aria-haspopup": "dialog", "aria-controls": "list", "aria-expanded": "false", title: "Choose a video format. Drag to reposition." },
            symbol("download", "ico download-symbol"), spin, el("span", { id: "label", text: "Download" }), el("span", { class: "count", id: "count", "aria-hidden": "true" }), symbol("chevron", "ico caret")),
          el("span", { class: "sides" },
            el("button", { type: "button", class: "side", id: "help", title: "Widget help", "aria-label": "Widget help" }, symbol("info")),
            el("button", { type: "button", class: "side", id: "close", title: "Hide for this video", "aria-label": "Hide download widget for this video" }, symbol("close")))
        ), list)
    );
    const b = { host, root, video, open: false, own: null, loading: false, dx: null, dy: null, help: false, focusMenu: false };
    const grab = root.getElementById("grab");
    // Native button activation works for mouse, keyboard and assistive tech.
    // Dragging suppresses only the click generated by that same gesture.
    let down = null, suppressClick = false;
    grab.addEventListener("mousedown", (e) => {
      if (e.button !== 0) return;
      const hr = host.getBoundingClientRect();
      down = { x: e.clientX, y: e.clientY, ox: e.clientX - hr.left, oy: e.clientY - hr.top, moved: false };
      suppressClick = false;
      grab.focus({ preventScroll: true });
      e.preventDefault();
    });
    const move = (e) => {
      if (!down) return;
      if (!down.moved && Math.abs(e.clientX - down.x) + Math.abs(e.clientY - down.y) < 5) return;
      down.moved = true;
      root.getElementById("main").classList.add("drag");
      const r = video.getBoundingClientRect();
      b.dx = e.clientX - down.ox - r.left;
      b.dy = e.clientY - down.oy - r.top;
      placeOne(b);
    };
    const up = () => {
      if (!down) return;
      suppressClick = down.moved;
      down = null;
      root.getElementById("main").classList.remove("drag");
      if (suppressClick) setTimeout(() => { suppressClick = false; }, 0);
    };
    addEventListener("mousemove", move);
    addEventListener("mouseup", up);
    b.cleanup = () => { down = null; removeEventListener("mousemove", move); removeEventListener("mouseup", up); };
    grab.addEventListener("click", (e) => {
      if (suppressClick && e.detail !== 0) return;
      clicked(b, e.detail === 0);
    });
    root.getElementById("help").addEventListener("click", (e) => {
      for (const other of bubbles.values()) if (other !== b && other.open) closeOne(other);
      b.help = !b.help;
      b.open = true;
      b.focusMenu = e.detail === 0;
      renderOne(b);
    });
    root.getElementById("close").addEventListener("click", () => {
      b.dismissed = true;
      b.open = false;
      host.remove();
      if (video.tabIndex >= 0) video.focus({ preventScroll: true });
    });
    root.addEventListener("keydown", (e) => {
      if (!b.open) return;
      if (e.key === "Escape") {
        e.preventDefault(); e.stopPropagation(); closeOne(b, true); return;
      }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
      const choices = [...root.querySelectorAll(".item:not(:disabled), .act")];
      if (!choices.length) return;
      const current = choices.indexOf(root.activeElement);
      const next = e.key === "Home" ? 0 : e.key === "End" ? choices.length - 1 : e.key === "ArrowDown" ? (current + 1) % choices.length : (current < 0 ? choices.length - 1 : (current + choices.length - 1) % choices.length);
      e.preventDefault(); e.stopPropagation(); focusChoice(b, next);
    });
    root.addEventListener("focusout", (e) => {
      // relatedTarget is null when a list update removes its focused item;
      // do not mistake that update for the user leaving the widget.
      if (b.open && e.relatedTarget && !root.contains(e.relatedTarget)) closeOne(b);
    });
    layer().appendChild(host);
    bubbles.set(video, b);
    renderOne(b);
    placeOne(b);
    return b;
  }

  function itemsOf(b) {
    if (b.own) return b.own;
    return isMain(b.video) ? items : [];
  }

  function isProtectedVideo(b, knownItems) {
    return isNetflixWatch() || (knownItems || itemsOf(b)).some((m) => m.protected || m.type === "protected");
  }

  function lookUp(b) {
    if (isProtectedVideo(b)) { b.loading = false; b.open = true; renderOne(b); return; }
    // another video on the page (or nothing found for it yet): look it up now
    b.loading = true;
    b.open = true;
    renderOne(b, "Looking for the video qualities…");
    chrome.runtime.sendMessage(
      // its own post / video link, or its own file; the page only when
      // neither is there
      {
        type: "swiftget-resolve-video",
        // in a frame that is only a player: the add-in uses the page around it
        page: ownLink(b.video) || (directSrc(b.video) || !TOP ? "" : location.href),
        src: directSrc(b.video),
        frame: !TOP,
      },
      (res) => {
        b.loading = false;
        if (chrome.runtime.lastError || !res) return notRunning(b, () => lookUp(b));
        if (res.error) {
          if (/not running/i.test(res.error)) return notRunning(b, () => lookUp(b));
          return renderOne(b, res.error, true);
        }
        b.own = res.items || [];
        renderOne(b);
      }
    );
  }

  function clicked(b, keyboard = false) {
    for (const other of bubbles.values()) if (other !== b && other.open) closeOne(other);
    b.focusMenu = keyboard;
    const list = itemsOf(b);
    if (!isProtectedVideo(b, list) && !list.length && !b.loading) return lookUp(b);
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
              renderOne(b, "Ezoo Download did not start. Open the app from Applications or your app launcher, then try again.", true);
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
      flashOne(b, "Playlist opened in Ezoo Download.", false);
    });
  }

  function renderOne(b, note, isError, action) {
    const list = itemsOf(b);
    const usable = list.filter((i) => !i.disabled);
    const busy = b.loading || (!usable.length && list.some((i) => i.disabled && /Getting/.test(i.label)));
    const trigger = b.root.getElementById("grab");
    const box = b.root.getElementById("list");
    const oldFocus = b.root.activeElement;
    const focusIndex = [...box.querySelectorAll(".item:not(:disabled), .act")].indexOf(oldFocus);
    const focusKey = oldFocus?.getAttribute?.("data-choice");
    const focusClose = oldFocus?.classList?.contains("popover-close");
    const protectedVideo = isProtectedVideo(b, list);
    b.root.getElementById("label").textContent = protectedVideo ? "Protected video" : busy ? "Finding formats…" : "Download";
    const count = b.root.getElementById("count");
    count.textContent = String(usable.length);
    count.hidden = protectedVideo || busy || usable.length <= 1;
    b.root.getElementById("spin").hidden = protectedVideo || !busy;
    const mainSymbol = b.root.querySelector(".download-symbol");
    mainSymbol.style.display = !protectedVideo && busy ? "none" : "";
    mainSymbol.querySelector("path").setAttribute("d", protectedVideo ? ICON_PATHS.lock : ICON_PATHS.download);
    trigger.setAttribute("aria-expanded", String(b.open));
    trigger.setAttribute("aria-busy", String(!protectedVideo && busy));
    trigger.setAttribute("aria-label", protectedVideo ? "Netflix protected video information" : busy ? "Finding video download formats" : `Choose a video download format${usable.length ? `, ${usable.length} available` : ""}`);
    b.root.getElementById("main").classList.toggle("open", b.open);
    box.hidden = !b.open;
    box.replaceChildren();
    // Closed controls do not need to rebuild their hidden option lists.
    if (!b.open) return;
    const title = (list.find((m) => m.yt && m.yt.title) || {}).yt;
    const heading = el("div", { class: "head" },
      el("div", { class: "heading-body" }, el("div", { class: "heading", text: protectedVideo ? "Protected Video" : "Download Video" }),
        el("div", { class: "video-title", title: (title && title.title) || cleanTitle(), text: (title && title.title) || cleanTitle() })),
      el("button", { type: "button", class: "side popover-close", title: "Close options", "aria-label": "Close download options" }, symbol("close")));
    heading.querySelector("button").addEventListener("click", () => closeOne(b, true));
    box.appendChild(heading);
    if (b.help) {
      box.appendChild(el("div", { class: "help", text: "Choose a format to send it to Ezoo Download. Drag the Download button to move it. Hide removes it for this video; use the browser extension menu to show it again or change its size. Keyboard: Tab to the controls, use arrow keys to choose, and Escape to close." }));
    }
    const finish = () => {
      placeOne(b);
      if (focusClose) heading.querySelector("button").focus({ preventScroll: true });
      else if (b.focusMenu || focusIndex >= 0) {
        const choices = [...box.querySelectorAll(".item:not(:disabled), .act")];
        const same = focusKey ? choices.findIndex((row) => row.getAttribute("data-choice") === focusKey) : -1;
        if (focusChoice(b, same >= 0 ? same : focusIndex >= 0 ? focusIndex : 0)) b.focusMenu = false;
        else trigger.focus({ preventScroll: true });
      }
    };
    if (protectedVideo) {
      box.appendChild(el("div", { class: "msg protected", role: "status" }, symbol("lock"), el("div", { class: "msg-body", text: NETFLIX_NOTICE })));
      finish();
      return;
    }
    if (note) {
      const body = el("div", { class: "msg-body", text: note });
      if (action) {
        const button = el("button", { type: "button", class: "act", text: action.text });
        button.addEventListener("click", action.run);
        body.appendChild(button);
      }
      box.appendChild(el("div", { class: "msg" + (isError ? " err" : busy ? " pending" : ""), role: "status", "aria-live": "polite" }, symbol(isError ? "warning" : busy ? "info" : "check"), body));
      finish();
      return;
    }
    if (!list.length) {
      box.appendChild(el("div", { class: "msg pending", role: "status", "aria-live": "polite" }, symbol("info"), el("div", { class: "msg-body", text: "Finding available video formats…" })));
      finish();
      return;
    }
    const all = downloadAllOf(b, usable);
    if (all) {
      const row = el("button", { type: "button", class: "item", "aria-label": all.text },
        el("span", { class: "format-symbol" }, symbol("collection")), el("span", { class: "body" }, el("span", { class: "t all", text: all.text })), symbol("download", "ico accessory"));
      row.addEventListener("click", all.run);
      box.appendChild(row);
    }
    const isAudio = (m) => (m.yt && (m.yt.quality === "audio" || m.yt.quality === "mp3" || /^audio\|/.test(m.yt.key || ""))) || /^audio\//i.test(m.label || "") || /^(?:mp3|m4a|aac|ogg|opus|flac|wav)$/i.test(m.ext || "");
    for (const audio of [false, true]) {
      const options = list.filter((m) => isAudio(m) === audio);
      if (!options.length) continue;
      const group = el("section", { class: "group", "aria-label": audio ? "Audio formats" : "Video formats" }, el("h3", { class: "group-title", text: audio ? "Audio" : "Video" }));
      for (const m of options) {
        const d = describe(m);
        const main = el("span", { class: "t" }, el("span", { text: d.main }));
        if (d.tag) main.appendChild(el("span", { class: "tag", text: d.tag }));
        if (d.codec) main.appendChild(el("span", { class: "codec", text: d.codec }));
        const sub = d.sub.length > 160 ? d.sub.slice(0, 160) + "…" : d.sub;
        const body = el("span", { class: "body" }, main);
        if (sub) body.appendChild(el("span", { class: "s", text: sub }));
        const row = el("button", { type: "button", class: "item", "data-choice": m.yt?.key || m.url || m.label || "", "aria-label": [d.main, d.tag, d.codec, d.sub].filter(Boolean).join(", ") }, el("span", { class: "format-symbol" }, symbol(audio ? "audio" : "video")), body);
        row.disabled = !!m.disabled;
        row.setAttribute("aria-disabled", String(!!m.disabled));
        if (!m.disabled) {
          row.appendChild(symbol("download", "ico accessory"));
          row.addEventListener("click", () => download(b, m));
        }
        group.appendChild(row);
      }
      box.appendChild(group);
    }
    finish();
  }

  function placeOne(b) {
    const fs = document.fullscreenElement || document.webkitFullscreenElement;
    const r = b.video.getBoundingClientRect();
    const onScreen =
      r.bottom > 0 && r.top < innerHeight && r.width >= 200 && r.height >= 100 && b.video.isConnected &&
      !(fs && fs.tagName === "VIDEO") && (!fs || fs.contains(b.video) || fs.shadowRoot);
    b.host.style.display = onScreen ? "block" : "none";
    if (!onScreen) return;
    if (b.host.parentNode !== layer()) layer().appendChild(b.host);
    // its real size (the user can make the button smaller or bigger)
    const w = b.host.getBoundingClientRect().width || 170;
    const h = b.root.getElementById("main").getBoundingClientRect().height || 22;
    let x = b.dx == null ? r.right - w - 8 : r.left + b.dx;
    let y = b.dy == null ? r.top + 8 : r.top + b.dy;
    x = Math.max(r.left, Math.min(x, r.right - 60));
    y = Math.max(r.top, Math.min(y, r.bottom - h));
    b.host.style.left = `${Math.max(4, Math.min(x, innerWidth - w))}px`;
    b.host.style.top = `${Math.max(4, Math.min(y, innerHeight - h - 4))}px`;
    if (b.open) {
      const box = b.root.getElementById("list");
      box.style.maxWidth = `${Math.max(140, (innerWidth - 16) / zoom)}px`;
      const hr = b.host.getBoundingClientRect();
      const below = innerHeight - hr.bottom - 12, above = hr.top - 12;
      const aboveMenu = below < Math.min(box.scrollHeight * zoom, 300) && above > below;
      box.classList.toggle("above", aboveMenu);
      box.style.maxHeight = `${Math.max(48 / zoom, Math.min(440, (aboveMenu ? above : below) / zoom))}px`;
      box.classList.toggle("align-start", hr.right - box.getBoundingClientRect().width < 8);
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
        releaseBubble(b);
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
    if (isProtectedVideo(b) || m.protected || m.type === "protected") { b.open = true; renderOne(b); return; }
    const fileName = `${cleanTitle()}.${m.ext || "mp4"}`;
    chrome.runtime.sendMessage(
      { type: "swiftget-download", item: m, title: document.title, fileName, referrer: location.href },
      (res) => {
        if (chrome.runtime.lastError || !res || !res.ok) notRunning(b, () => download(b, m));
        else flashOne(b, "Sent to Ezoo Download.", false);
      }
    );
  }

  function update(newItems) {
    if (!TOP) return; // frames look up their own videos
    items = newItems || [];
    if (!items.length) {
      // new page / new video: forget the bubbles and "hidden"
      for (const b of bubbles.values()) releaseBubble(b);
      bubbles.clear();
      return;
    }
    if (!panelAllowed()) return;
    place();
    for (const b of bubbles.values()) if (!b.own && !b.loading) renderOne(b);
  }

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
          closeOne(b);
        }
      }
    },
    true
  );
  addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    for (const b of bubbles.values()) {
      if (b.open) {
        closeOne(b, !!b.root.activeElement);
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
    for (const a of document.querySelectorAll("a[href]")) if (inSel(a)) add(a.href, a.innerText || a.title);
    for (const el of document.querySelectorAll("img[src], video[src], audio[src], source[src], embed[src]")) {
      if (inSel(el)) add(el.src, el.alt || el.title || "");
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
      for (const other of bubbles.values()) if (other !== b && other.open) closeOne(other);
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
        for (const b of bubbles.values()) releaseBubble(b);
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
