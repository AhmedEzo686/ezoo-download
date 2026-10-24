// Floating "Download this video" button, shown when the background worker
// detects a video/audio file or HLS stream on the page.
(() => {
  if (window.top !== window) return;

  let items = [];
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
    return ownLink(video) || location.href;
  }

  // the post / video link next to this very video ("" = none found)
  function ownLink(video) {
    let el = video;
    for (let depth = 0; el && depth < 25; depth++, el = el.parentElement) {
      const links = el.querySelectorAll ? el.querySelectorAll("a[href]") : [];
      for (const a of links) {
        if (isVideoPage(a.href)) return a.href.split("#")[0];
      }
      if (el.tagName === "ARTICLE") break;
    }
    return "";
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

  // ---------------------------------------------------------------- bubbles
  // One "Download" bubble on every video of the page. The main video (the
  // one playing / the biggest) uses what the add-in found for the page; any
  // other video is looked up when its bubble is clicked. A bubble can be
  // dragged anywhere on its video and stays there.
  const bubbles = new Map(); // video -> bubble
  const STYLE = `
    :host { all: initial; }
    .wrap { font: 12px/1.3 "Segoe UI", system-ui, sans-serif; color: #1f2733; width: 230px; }
    .btn { display: flex; align-items: center; gap: 7px; color: #fff; border: 1px solid #0f4fb5;
           background: linear-gradient(#3f8cf5, #1d63d6); border-radius: 4px; padding: 5px 8px;
           cursor: grab; box-shadow: 0 2px 8px rgba(0,0,0,.35); font: inherit; font-weight: 600;
           width: 100%; opacity: .88; transition: opacity .15s; user-select: none; }
    .btn:hover { opacity: 1; background: linear-gradient(#4b96fa, #1a5bc7); }
    .btn.drag { cursor: grabbing; opacity: 1; }
    .ico { width: 16px; height: 16px; flex: none; }
    .caret { font-size: 9px; opacity: .9; }
    .x { margin-left: auto; opacity: .8; padding: 0 2px; cursor: pointer; }
    .x:hover { opacity: 1; }
    .list { margin-top: 6px; background: #fff; border-radius: 8px; box-shadow: 0 6px 20px rgba(0,0,0,.25);
            overflow: hidden; max-height: 320px; overflow-y: auto; }
    .item { padding: 8px 10px; cursor: pointer; border-bottom: 1px solid #eef1f5; }
    .item:last-child { border-bottom: 0; }
    .item:hover { background: #eef4fe; }
    .t { font-weight: 600; }
    .s { color: #6b7686; font-size: 11px; word-break: break-all; }
    .msg { padding: 8px 10px; color: #1a9e5a; }
    .off { cursor: default; opacity: .7; }
    .off:hover { background: #fff; }
    .err { color: #d1342f; }`;

  function isMain(video) {
    return video === biggestVideo();
  }

  function directSrc(video) {
    const src = video.currentSrc || video.src || "";
    return /^https?:/i.test(src) && !/\.m3u8|\.mpd/i.test(src) ? src : "";
  }

  function makeBubble(video) {
    const host = document.createElement("div");
    host.style.all = "initial";
    host.style.position = "fixed";
    host.style.zIndex = "2147483647";
    const root = host.attachShadow({ mode: "closed" });
    // built element by element (no HTML strings)
    const el = (tag, props = {}, ...kids) => {
      const e = document.createElement(tag);
      for (const [k, v] of Object.entries(props)) {
        if (k === "text") e.textContent = v;
        else e.setAttribute(k, v);
      }
      e.append(...kids);
      return e;
    };
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
    root.append(
      el("style", { text: STYLE }),
      el(
        "div",
        { class: "wrap" },
        el(
          "div",
          { class: "btn", id: "main" },
          svg,
          el("span", { id: "label", text: "Download this video" }),
          el("span", { class: "caret", text: "▼" }),
          el("span", { class: "x", id: "close", title: "Hide", text: "✕" })
        ),
        list
      )
    );
    const b = { host, root, video, open: false, own: null, loading: false, dx: null, dy: null };
    const btn = root.getElementById("main");
    // drag: move it anywhere on the video; a short press is a click
    let down = null;
    btn.addEventListener("mousedown", (e) => {
      if (e.button !== 0 || e.target.id === "close") return;
      const hr = host.getBoundingClientRect();
      down = { x: e.clientX, y: e.clientY, ox: e.clientX - hr.left, oy: e.clientY - hr.top, moved: false };
      e.preventDefault();
    });
    addEventListener("mousemove", (e) => {
      if (!down) return;
      if (!down.moved && Math.abs(e.clientX - down.x) + Math.abs(e.clientY - down.y) < 5) return;
      down.moved = true;
      btn.classList.add("drag");
      const r = video.getBoundingClientRect();
      b.dx = e.clientX - down.ox - r.left;
      b.dy = e.clientY - down.oy - r.top;
      placeOne(b);
    });
    addEventListener("mouseup", (e) => {
      if (!down) return;
      const wasDrag = down.moved;
      down = null;
      btn.classList.remove("drag");
      if (!wasDrag) clicked(b, e);
    });
    root.getElementById("close").addEventListener("click", () => {
      b.dismissed = true;
      host.remove();
    });
    document.documentElement.appendChild(host);
    bubbles.set(video, b);
    renderOne(b);
    placeOne(b);
    return b;
  }

  function itemsOf(b) {
    if (b.own) return b.own;
    return isMain(b.video) ? items : [];
  }

  function clicked(b) {
    const list = itemsOf(b);
    if (list.length === 1 && !list[0].disabled && !list[0].yt) {
      download(b, list[0]);
      return;
    }
    if (!list.length && !b.loading) {
      // another video on the page: look it up now
      b.loading = true;
      b.open = true;
      renderOne(b, "Looking for the video…");
      chrome.runtime.sendMessage(
        // its own post / video link, or its own file; the page only when
        // neither is there
        {
          type: "swiftget-resolve-video",
          page: ownLink(b.video) || (directSrc(b.video) ? "" : location.href),
          src: directSrc(b.video),
        },
        (res) => {
          b.loading = false;
          if (chrome.runtime.lastError || !res) return renderOne(b, "Ezoo Download is not running — open the app and try again.", true);
          if (res.error) return renderOne(b, res.error, true);
          b.own = res.items || [];
          renderOne(b);
        }
      );
      return;
    }
    b.open = !b.open;
    renderOne(b);
  }

  function renderOne(b, note, isError) {
    const list = itemsOf(b);
    const usable = list.filter((i) => !i.disabled).length;
    b.root.getElementById("label").textContent = usable > 1 ? `Download this video (${usable})` : "Download this video";
    const box = b.root.getElementById("list");
    box.hidden = !b.open;
    box.replaceChildren();
    if (note) {
      const d = document.createElement("div");
      d.className = "msg" + (isError ? " err" : "");
      d.textContent = note;
      box.appendChild(d);
      return;
    }
    list.forEach((m) => {
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
      if (!m.disabled) div.addEventListener("click", () => download(b, m));
      box.appendChild(div);
    });
  }

  function placeOne(b) {
    const r = b.video.getBoundingClientRect();
    const onScreen = r.bottom > 0 && r.top < innerHeight && r.width >= 200 && r.height >= 100 && b.video.isConnected;
    b.host.style.display = onScreen ? "block" : "none";
    if (!onScreen) return;
    const w = 236;
    let x = b.dx == null ? r.right - w - 8 : r.left + b.dx;
    let y = b.dy == null ? r.top + 8 : r.top + b.dy;
    x = Math.max(r.left, Math.min(x, r.right - 60));
    y = Math.max(r.top, Math.min(y, r.bottom - 24));
    b.host.style.left = `${Math.max(4, Math.min(x, innerWidth - w))}px`;
    b.host.style.top = `${Math.max(4, Math.min(y, innerHeight - 28))}px`;
  }

  function place() {
    if (!panelAllowed()) return;
    const vids = [...document.querySelectorAll("video")].filter((v) => {
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
    // only when there is something to offer: the main video once the add-in
    // found media, any other one as soon as it is a real player
    for (const v of vids.slice(0, 12)) {
      let b = bubbles.get(v);
      if (!b) {
        if (isMain(v) ? !items.length : v.paused && !(v.duration > 3) && !directSrc(v)) continue;
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
    const fileName = `${cleanTitle()}.${m.ext || "mp4"}`;
    chrome.runtime.sendMessage({ type: "swiftget-download", item: m, title: document.title, fileName }, (res) => {
      if (chrome.runtime.lastError || !res || !res.ok) {
        flashOne(b, "Ezoo Download is not running — open the app and try again.", true);
      } else {
        flashOne(b, "Sent to Ezoo Download ✔", false);
      }
    });
  }

  function update(newItems) {
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
      const v = biggestVideo();
      let b = v && bubbles.get(v);
      if (v && !b) b = makeBubble(v);
      if (b) {
        b.dismissed = false;
        document.documentElement.appendChild(b.host);
        b.open = true;
        renderOne(b);
        placeOne(b);
      }
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
  setInterval(place, 700);
  setInterval(watchVideos, 1500);
  document.addEventListener("play", () => setTimeout(watchVideos, 300), true);
  addEventListener("popstate", () => setTimeout(watchVideos, 500));
})();
