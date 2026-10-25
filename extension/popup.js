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
  document.getElementById("settings").addEventListener("click", () => {
    chrome.runtime.openOptionsPage();
    window.close();
  });

  // size of the "Download this video" button on pages (also in the settings page)
  const sizes = [...document.querySelectorAll("#sizes button")];
  const mark = (v) => sizes.forEach((b) => b.classList.toggle("on", b.dataset.size === v));
  chrome.storage.local.get({ bubbleSize: "normal" }, (o) => mark(["small", "normal", "large"].includes(o.bubbleSize) ? o.bubbleSize : "normal"));
  sizes.forEach((b) =>
    b.addEventListener("click", () => {
      chrome.storage.local.set({ bubbleSize: b.dataset.size });
      mark(b.dataset.size);
    })
  );

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
    box.replaceChildren();
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
