const DEFAULTS = { port: 9614, intercept: true, minSizeKB: 0 };

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

async function init() {
  const opts = Object.assign({}, DEFAULTS, await chrome.storage.local.get(DEFAULTS));
  const intercept = document.getElementById("intercept");
  const minSize = document.getElementById("minSize");
  const port = document.getElementById("port");
  intercept.checked = opts.intercept;
  minSize.value = opts.minSizeKB;
  port.value = opts.port;
  intercept.addEventListener("change", () => chrome.storage.local.set({ intercept: intercept.checked }));
  minSize.addEventListener("change", () => chrome.storage.local.set({ minSizeKB: Math.max(0, Number(minSize.value) || 0) }));
  port.addEventListener("change", () => {
    const p = Number(port.value);
    if (p >= 1024 && p <= 65535) chrome.storage.local.set({ port: p });
  });

  chrome.runtime.sendMessage({ type: "swiftget-status" }, (res) => {
    const el = document.getElementById("status");
    const ok = res && res.ok;
    el.textContent = ok ? "connected" : "app not running";
    el.classList.toggle("on", !!ok);
  });

  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) return;
  chrome.runtime.sendMessage({ type: "swiftget-get-media", tabId: tab.id }, (items) => {
    if (!items || !items.length) return;
    const box = document.getElementById("media");
    box.innerHTML = "";
    const title = (tab.title || "video").replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").slice(0, 120);
    items.forEach((m) => {
      const row = document.createElement("div");
      row.className = "media";
      const info = document.createElement("div");
      info.textContent = m.label;
      const small = document.createElement("small");
      small.textContent = m.sub || "";
      info.appendChild(small);
      const btn = document.createElement("button");
      btn.textContent = "Download";
      btn.disabled = !!m.disabled;
      btn.addEventListener("click", () => {
        chrome.runtime.sendMessage(
          { type: "swiftget-download", item: m, title: tab.title, referrer: tab.url, fileName: `${title}.${m.ext || "mp4"}` },
          (res) => {
            btn.textContent = res && res.ok ? "Sent ✔" : "App not running";
          }
        );
      });
      row.append(info, btn);
      box.appendChild(row);
    });
  });
}

init();
