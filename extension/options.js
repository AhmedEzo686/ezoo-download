const DEFAULTS = { port: 9614, intercept: true, minSizeKB: 0, bubbleSize: "normal" };

function saved() {
  const el = document.getElementById("saved");
  el.textContent = "Saved ✔";
  setTimeout(() => (el.textContent = ""), 1500);
}

async function init() {
  const opts = Object.assign({}, DEFAULTS, await chrome.storage.local.get(DEFAULTS));
  const intercept = document.getElementById("intercept");
  const minSize = document.getElementById("minSize");
  const port = document.getElementById("port");
  intercept.checked = opts.intercept;
  minSize.value = opts.minSizeKB;
  port.value = opts.port;
  const size = document.getElementById("bubbleSize");
  size.value = ["small", "normal", "large"].includes(opts.bubbleSize) ? opts.bubbleSize : "normal";
  size.addEventListener("change", () => chrome.storage.local.set({ bubbleSize: size.value }).then(saved));
  intercept.addEventListener("change", () => chrome.storage.local.set({ intercept: intercept.checked }).then(saved));
  minSize.addEventListener("change", () =>
    chrome.storage.local.set({ minSizeKB: Math.max(0, Number(minSize.value) || 0) }).then(saved)
  );
  port.addEventListener("change", () => {
    const p = Number(port.value);
    if (p >= 1024 && p <= 65535) chrome.storage.local.set({ port: p }).then(saved);
  });
}

init();
