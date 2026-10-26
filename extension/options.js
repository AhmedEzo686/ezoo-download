const DEFAULTS = { port: 9614, intercept: true, minSizeKB: 0, bubbleSize: "normal", bubbleScale: null };
const OLD_SIZES = { small: 85, normal: 100, large: 125 };
let savedTimer;

// Use the setup assistant's prepared port as the default, without replacing
// an explicit connection setting the user already saved.
async function preparedPort() {
  if (typeof chrome.runtime.getURL !== "function") return DEFAULTS.port;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 800);
  try {
    const response = await fetch(chrome.runtime.getURL("app-connection.json"), { signal: controller.signal });
    if (!response.ok) return DEFAULTS.port;
    const text = await response.text();
    if (text.length > 1024) return DEFAULTS.port;
    const value = JSON.parse(text).port;
    return Number.isInteger(value) && value > 0 && value <= 65535 ? value : DEFAULTS.port;
  } catch { return DEFAULTS.port; }
  finally { clearTimeout(timeout); }
}

function saved(message = "Saved", error = false) {
  const element = document.getElementById("saved");
  clearTimeout(savedTimer);
  element.textContent = message;
  element.classList.toggle("error", error);
  savedTimer = setTimeout(() => { element.textContent = "Changes save automatically."; element.classList.remove("error"); }, 1800);
}

async function save(values) {
  try { await chrome.storage.local.set(values); saved(); return true; }
  catch { saved("Couldn’t save changes. Try again.", true); return false; }
}

function showScale(value) {
  const percent = Number.isFinite(value) ? Math.max(50, Math.min(200, Math.round(value))) : 100;
  document.getElementById("bubbleScale").value = String(percent);
  document.getElementById("bubbleScaleText").textContent = `${percent}%`;
  document.getElementById("buttonPreview").style.transform = `scale(${percent / 100})`;
  return percent;
}

function invalid(input, errorId, value) {
  input.setAttribute("aria-invalid", String(value));
  document.getElementById(errorId).hidden = !value;
  return value;
}

async function init() {
  let options;
  try {
    const defaults = { ...DEFAULTS, port: await preparedPort() };
    options = Object.assign({}, defaults, await chrome.storage.local.get(defaults));
  }
  catch { options = { ...DEFAULTS }; saved("Couldn’t load settings. Try opening this page again.", true); }
  const intercept = document.getElementById("intercept");
  const minSize = document.getElementById("minSize");
  const port = document.getElementById("port");
  intercept.checked = options.intercept;
  minSize.value = options.minSizeKB;
  port.value = options.port;
  showScale(options.bubbleScale != null ? Number(options.bubbleScale) * 100 : OLD_SIZES[options.bubbleSize] || 100);
  const scale = document.getElementById("bubbleScale");
  scale.addEventListener("input", async () => {
    const percent = showScale(Number(scale.value));
    await save({ bubbleScale: percent / 100 });
  });
  document.getElementById("bubbleScaleReset").addEventListener("click", async () => {
    showScale(100);
    await save({ bubbleScale: 1 });
  });
  intercept.addEventListener("change", () => save({ intercept: intercept.checked }));
  minSize.addEventListener("change", async () => {
    const value = Number(minSize.value);
    if (invalid(minSize, "minimum-error", !Number.isFinite(value) || value < 0 || minSize.value.trim() === "")) return;
    await save({ minSizeKB: value });
  });
  port.addEventListener("change", async () => {
    const value = Number(port.value);
    if (invalid(port, "port-error", !Number.isInteger(value) || value < 1024 || value > 65535 || port.value.trim() === "")) return;
    await save({ port: value });
  });
}

init();
