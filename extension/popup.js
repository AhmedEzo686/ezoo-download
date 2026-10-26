// The popup is a small, system-appearance downloads surface. User-provided
// labels and page titles are always text nodes, never HTML.
const OLD_SIZES = { small: 85, normal: 100, large: 125 };
let integrationEnabled = true;

function icon(kind) {
  const paths = {
    download: "M12 3v11m-4-4 4 4 4-4M5 15v4a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4",
    video: "M4 6h10a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2Zm12 3 5-3v12l-5-3",
    audio: "M9 17V5l11-2v12M9 8l11-2M9 17c0 1.4-1.6 2.5-3.5 2.5S2 18.4 2 17s1.6-2.5 3.5-2.5S9 15.6 9 17Zm11-2c0 1.4-1.6 2.5-3.5 2.5S13 16.4 13 15s1.6-2.5 3.5-2.5S20 13.6 20 15Z",
    playlist: "M4 6h11M4 12h8M4 18h6m9-7v10m-4-4 4 4 4-4",
    check: "m5 12 4 4L19 6",
    lock: "M7 10h10a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2Zm1 0V7a4 4 0 0 1 8 0v3m-4 4v3",
  };
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", "glyph");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", paths[kind] || paths.video);
  svg.appendChild(path);
  return svg;
}

function request(message) {
  return new Promise((resolve) => {
    try {
      chrome.runtime.sendMessage(message, (response) => {
        resolve(chrome.runtime.lastError ? null : response);
      });
    } catch {
      resolve(null);
    }
  });
}

function updateDownloadControls() {
  for (const button of document.querySelectorAll("[data-download]")) {
    button.disabled = !integrationEnabled || button.dataset.unavailable === "true" || button.dataset.pending === "true" || button.dataset.sent === "true";
  }
  document.getElementById("scale").disabled = !integrationEnabled;
  document.getElementById("scaleReset").disabled = !integrationEnabled;
}

function showEnabled(value) {
  integrationEnabled = value;
  document.getElementById("enabled").checked = value;
  document.body.classList.toggle("off", !value);
  document.getElementById("onText").textContent = value ? "Browser Integration" : "Integration Paused";
  document.getElementById("onHint").textContent = value ? "Download and video tools are available" : "Downloads stay in your browser";
  updateDownloadControls();
}

function showScale(value) {
  const percent = Number.isFinite(value) ? Math.max(50, Math.min(200, Math.round(value))) : 100;
  document.getElementById("scale").value = String(percent);
  document.getElementById("scaleText").textContent = `${percent}%`;
  return percent;
}

function showConnection(connected) {
  const status = document.getElementById("status");
  status.textContent = connected ? "Connected" : "App Not Running";
  status.classList.toggle("on", connected);
  status.classList.toggle("offline", !connected);
  document.getElementById("connectionHint").hidden = connected;
  document.getElementById("openApp").classList.toggle("secondary-action", connected);
}

function protectedRow(item) {
  const row = document.createElement("div");
  row.className = "media-row media-notice";
  const symbol = document.createElement("div");
  symbol.className = "media-symbol";
  symbol.appendChild(icon("lock"));
  const info = document.createElement("div");
  info.className = "media-info";
  const title = document.createElement("div");
  title.className = "media-title";
  title.textContent = item.label || "Protected Video";
  const detail = document.createElement("small");
  detail.textContent = item.sub || "Protected playback can’t be saved by this downloader.";
  const help = document.createElement("a");
  help.className = "notice-link";
  help.href = "https://help.netflix.com/en/node/54816";
  help.target = "_blank";
  help.rel = "noopener noreferrer";
  help.textContent = "Netflix Download Help";
  info.append(title, detail, help);
  row.append(symbol, info);
  return row;
}

function mediaRow({ label, detail, kind, unavailable, message, playlist }) {
  const row = document.createElement("div");
  row.className = "media-row";
  const symbol = document.createElement("div");
  symbol.className = "media-symbol";
  symbol.appendChild(icon(kind));
  const info = document.createElement("div");
  info.className = "media-info";
  const title = document.createElement("div");
  title.className = "media-title";
  title.textContent = label;
  info.appendChild(title);
  if (detail) {
    const secondary = document.createElement("small");
    secondary.textContent = detail;
    info.appendChild(secondary);
  }
  const error = document.createElement("p");
  error.className = "row-error";
  error.setAttribute("role", "status");
  error.hidden = true;
  info.appendChild(error);
  const button = document.createElement("button");
  button.type = "button";
  button.className = "download-button";
  button.dataset.download = "true";
  button.dataset.unavailable = String(!!unavailable);
  const actionLabel = playlist ? "Download All" : "Download";
  button.setAttribute("aria-label", `${actionLabel}: ${label}`);
  const buttonLabel = document.createElement("span");
  buttonLabel.textContent = unavailable ? "Unavailable" : actionLabel;
  button.append(icon("download"), buttonLabel);
  button.addEventListener("click", async () => {
    if (button.disabled) return;
    button.dataset.pending = "true";
    buttonLabel.textContent = "Sending…";
    button.setAttribute("aria-busy", "true");
    error.hidden = true;
    updateDownloadControls();
    const result = await request(message);
    button.dataset.pending = "false";
    button.setAttribute("aria-busy", "false");
    if (result && result.ok) {
      button.dataset.sent = "true";
      buttonLabel.textContent = playlist ? "Opened" : "Sent";
      button.replaceChildren(icon("check"), buttonLabel);
    } else {
      buttonLabel.textContent = "Try Again";
      error.textContent = "Open Ezoo Download, then try again.";
      error.hidden = false;
      showConnection(false);
    }
    updateDownloadControls();
  });
  row.append(symbol, info, button);
  return row;
}

async function loadMedia() {
  let tab;
  try { [tab] = await chrome.tabs.query({ active: true, currentWindow: true }); } catch { return; }
  if (!tab) return;
  const items = await request({ type: "swiftget-get-media", tabId: tab.id });
  if (!Array.isArray(items) || !items.length) return;
  const box = document.getElementById("media");
  box.replaceChildren();
  const title = (tab.title || "video").replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").slice(0, 120);
  if (/youtube\.com/i.test(tab.url || "") && /[?&]list=/.test(tab.url || "")) {
    box.appendChild(mediaRow({
      label: "This Playlist", detail: "Choose videos in the app", kind: "playlist", playlist: true,
      message: { type: "swiftget-playlist", url: tab.url },
    }));
  }
  for (const item of items) {
    if (item.protected || item.type === "protected") {
      box.appendChild(protectedRow(item));
      continue;
    }
    box.appendChild(mediaRow({
      label: item.label || "Video", detail: item.sub || "", kind: item.ext && /^(mp3|m4a|aac|ogg|opus|flac|wav)$/i.test(item.ext) ? "audio" : "video", unavailable: item.disabled,
      message: { type: "swiftget-download", item, title: tab.title, referrer: tab.url, fileName: `${title}.${item.ext || "mp4"}` },
    }));
  }
  const formats = items.filter((item) => !item.protected && item.type !== "protected").length;
  document.getElementById("mediaCount").textContent = formats ? `${formats} ${formats === 1 ? "format" : "formats"}` : "";
  updateDownloadControls();
}

async function init() {
  document.getElementById("settings").addEventListener("click", () => {
    chrome.runtime.openOptionsPage();
    window.close();
  });
  document.getElementById("openApp").addEventListener("click", () => setTimeout(() => window.close(), 300));
  const enabled = document.getElementById("enabled");
  enabled.addEventListener("change", async () => {
    const previous = integrationEnabled;
    showEnabled(enabled.checked);
    try { await chrome.storage.local.set({ enabled: integrationEnabled }); }
    catch { showEnabled(previous); }
  });
  const saveScale = async (value) => {
    const percent = showScale(value);
    try { await chrome.storage.local.set({ bubbleScale: percent / 100 }); } catch { /* keep the UI usable if the browser is closing */ }
  };
  document.getElementById("scale").addEventListener("input", (event) => saveScale(Number(event.target.value)));
  document.getElementById("scaleReset").addEventListener("click", () => saveScale(100));
  try {
    const options = await chrome.storage.local.get({ enabled: true, bubbleScale: null, bubbleSize: "normal" });
    showEnabled(options.enabled !== false);
    showScale(options.bubbleScale != null ? Number(options.bubbleScale) * 100 : OLD_SIZES[options.bubbleSize] || 100);
  } catch { showEnabled(false); }
  const connection = request({ type: "swiftget-status" }).then((response) => showConnection(!!(response && response.ok)));
  await Promise.all([loadMedia(), connection]);
}

init();
