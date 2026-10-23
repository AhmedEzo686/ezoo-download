// Floating "Download this video" button, shown when the background worker
// detects a video/audio file or HLS stream on the page.
(() => {
  if (window.top !== window) return;

  let host = null;
  let root = null;
  let items = [];
  let open = false;
  let dismissed = false;
  // Options > File Types > "Download panel for web players"
  let panel = { enabled: true, skipSites: [] };
  const hostMatches = (host, p) => {
    p = String(p).replace(/^https?:\/\//i, "").split("/")[0].toLowerCase();
    if (!p) return false;
    if (p.includes("*")) return new RegExp("^" + p.replace(/[.]/g, "\\.").replace(/\*/g, ".*") + "$").test(host);
    return host === p || host.endsWith("." + p);
  };
  const panelAllowed = () =>
    panel.enabled !== false && !(panel.skipSites || []).some((s) => hostMatches(location.hostname.toLowerCase(), s));

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

  const cleanTitle = () =>
    (document.title || "video").replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").trim().slice(0, 120) || "video";

  // The video the user is watching: a playing one first, else the biggest
  // one on screen.
  function biggestVideo() {
    let best = null;
    let score = 0;
    for (const v of document.querySelectorAll("video")) {
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

  // Pages that are one video: their own address is the best link for it.
  const VIDEO_PAGE = [
    /youtube\.com\/(watch|shorts\/|live\/)/i,
    /tiktok\.com\/@[^/]+\/(video|photo)\//i,
    /instagram\.com\/(reels?|p|tv)\//i,
    /facebook\.com\/(.+\/videos\/|watch|reel\/|share\/[vr]\/)/i,
    /fb\.watch\//i,
    /(twitter|x)\.com\/[^/]+\/status\//i,
    /vimeo\.com\/\d+/i,
    /dailymotion\.com\/video\//i,
    /reddit\.com\/r\/[^/]+\/comments\//i,
    /twitch\.tv\/(videos\/|[^/]+\/clip\/)/i,
    /pinterest\.[a-z.]+\/pin\//i,
    /snapchat\.com\/(spotlight|add)\//i,
    /linkedin\.com\/(posts|feed\/update)\//i,
  ];
  const isVideoPage = (u) => VIDEO_PAGE.some((r) => r.test(u));

  // In feeds (Facebook, Instagram, X...) the page shows many videos: look for
  // the post / reel link around the video being watched.
  function linkFor(video) {
    if (isVideoPage(location.href)) return location.href;
    let el = video;
    for (let depth = 0; el && depth < 25; depth++, el = el.parentElement) {
      const links = el.querySelectorAll ? el.querySelectorAll("a[href]") : [];
      for (const a of links) {
        if (isVideoPage(a.href)) return a.href.split("#")[0];
      }
      if (el.tagName === "ARTICLE") break;
    }
    return location.href;
  }

  let lastPage = "";
  function watchVideos() {
    const v = biggestVideo();
    if (!v) return;
    // ignore tiny muted background loops that never play for long
    if (v.paused && !(v.duration > 3) && !v.currentSrc) return;
    const page = linkFor(v);
    if (page !== lastPage) {
      lastPage = page;
      chrome.runtime.sendMessage({ type: "swiftget-video-seen", page }).catch?.(() => {});
    }
  }

  function place() {
    if (!host) return;
    const v = biggestVideo();
    if (v) {
      const r = v.getBoundingClientRect();
      if (r.bottom > 0 && r.top < innerHeight) {
        host.style.position = "fixed";
        host.style.top = `${Math.max(8, r.top + 8)}px`;
        host.style.left = `${Math.max(8, Math.min(r.right - 240, innerWidth - 240))}px`;
        host.style.right = "auto";
        return;
      }
    }
    host.style.position = "fixed";
    host.style.top = "12px";
    host.style.right = "12px";
    host.style.left = "auto";
  }

  function ensureUI() {
    if (host) return;
    host = document.createElement("div");
    host.style.zIndex = "2147483647";
    host.style.all = "initial";
    host.style.position = "fixed";
    host.style.zIndex = "2147483647";
    root = host.attachShadow({ mode: "closed" });
    root.innerHTML = `
      <style>
        :host { all: initial; }
        .wrap { font: 12px/1.3 "Segoe UI", system-ui, sans-serif; color: #1f2733; width: 230px; }
        .btn { display: flex; align-items: center; gap: 7px; color: #fff; border: 1px solid #0f4fb5;
               background: linear-gradient(#3f8cf5, #1d63d6); border-radius: 4px; padding: 5px 8px;
               cursor: pointer; box-shadow: 0 2px 8px rgba(0,0,0,.35); font: inherit; font-weight: 600;
               width: 100%; opacity: .88; transition: opacity .15s; }
        .btn:hover { opacity: 1; background: linear-gradient(#4b96fa, #1a5bc7); }
        .ico { width: 16px; height: 16px; flex: none; }
        .caret { font-size: 9px; opacity: .9; }
        .x { margin-left: auto; opacity: .8; padding: 0 2px; }
        .x:hover { opacity: 1; }
        .list { margin-top: 6px; background: #fff; border-radius: 8px; box-shadow: 0 6px 20px rgba(0,0,0,.25);
                max-height: min(340px, 60vh); overflow-y: auto; overflow-x: hidden; overscroll-behavior: contain;
                scrollbar-width: thin; scrollbar-color: #b6c0ce transparent; }
        .list::-webkit-scrollbar { width: 8px; }
        .list::-webkit-scrollbar-track { background: transparent; }
        .list::-webkit-scrollbar-thumb { background: #b6c0ce; border-radius: 4px; }
        .list::-webkit-scrollbar-thumb:hover { background: #9da9ba; }
        .item { padding: 8px 10px; cursor: pointer; border-bottom: 1px solid #eef1f5; }
        .item:last-child { border-bottom: 0; }
        .item:hover { background: #eef4fe; }
        .t { font-weight: 600; }
        .s { color: #6b7686; font-size: 11px; word-break: break-all; }
        .msg { padding: 8px 10px; color: #1a9e5a; }
        .off { cursor: default; opacity: .7; }
        .off:hover { background: #fff; }
        .err { color: #d1342f; }
      </style>
      <div class="wrap">
        <button class="btn" id="main"><svg class="ico" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.5" fill="#fff"/><path d="M6.6 3.5h2.8v4.6h2.3L8 12 4.3 8.1h2.3z" fill="#1d63d6"/></svg><span id="label">Download this video</span><span class="caret">▼</span><span class="x" id="close" title="Hide">✕</span></button>
        <div class="list" id="list" hidden></div>
      </div>`;
    root.getElementById("main").addEventListener("click", (e) => {
      if (e.target.id === "close") {
        dismissed = true;
        host.remove();
        host = null;
        return;
      }
      if (items.length === 1 && !items[0].disabled && !items[0].yt) {
        download(items[0]);
      } else {
        open = !open;
        render();
      }
    });
    document.documentElement.appendChild(host);
    place();
  }

  function render() {
    if (!root) return;
    const label = root.getElementById("label");
    const usable = items.filter((i) => !i.disabled).length;
    label.textContent = usable > 1 ? `Download this video (${usable})` : "Download this video";
    const list = root.getElementById("list");
    list.hidden = !open;
    list.innerHTML = "";
    items.forEach((m) => {
      const div = document.createElement("div");
      div.className = "item" + (m.disabled ? " off" : "");
      const t = document.createElement("div");
      t.className = "t";
      t.textContent = m.label;
      const s = document.createElement("div");
      s.className = "s";
      const sub = m.sub || "";
      s.textContent = sub.length > 90 ? sub.slice(0, 90) + "…" : sub;
      div.append(t, s);
      if (!m.disabled) div.addEventListener("click", () => download(m));
      list.appendChild(div);
    });
  }

  function flash(text, isError) {
    const list = root.getElementById("list");
    list.hidden = false;
    list.innerHTML = "";
    const d = document.createElement("div");
    d.className = "msg" + (isError ? " err" : "");
    d.textContent = text;
    list.appendChild(d);
    setTimeout(() => {
      open = false;
      render();
    }, 2500);
  }

  function download(m) {
    const fileName = `${cleanTitle()}.${m.ext || "mp4"}`;
    chrome.runtime.sendMessage({ type: "swiftget-download", item: m, title: document.title, fileName }, (res) => {
      if (chrome.runtime.lastError || !res || !res.ok) {
        flash("Ezoo Download is not running — open the app and try again.", true);
      } else {
        flash("Sent to Ezoo Download ✔", false);
      }
    });
  }

  function update(newItems) {
    items = newItems || [];
    if (!items.length) {
      // new page / new video: remove the button and forget "hidden"
      if (host) host.remove();
      host = root = null;
      dismissed = false;
      open = false;
      return;
    }
    if (dismissed || !panelAllowed()) return;
    ensureUI();
    render();
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
      sendResponse(collectLinks(!!msg.selection));
      return true;
    }
    if (msg.type === "swiftget-open-panel") {
      dismissed = false;
      open = true;
      if (items.length) {
        ensureUI();
        render();
      }
    }
    return false;
  });
  chrome.runtime.sendMessage({ type: "swiftget-config" }, (res) => {
    if (!chrome.runtime.lastError && res) {
      panel = res;
      if (!panelAllowed() && host) {
        host.remove();
        host = root = null;
      }
    }
  });
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

  addEventListener("scroll", place, { passive: true });
  addEventListener("resize", place, { passive: true });
  setInterval(place, 1500);
  setInterval(watchVideos, 1500);
  document.addEventListener("play", () => setTimeout(watchVideos, 300), true);
  addEventListener("popstate", () => setTimeout(watchVideos, 500));
})();
