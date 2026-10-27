// YouTube stream logic (no browser APIs here, so it can be tested with Node).
//
// The YouTube player fetches video and audio from *.googlevideo.com/videoplayback
// URLs. Those URLs are already "unlocked" by the player (signature, n-param,
// PO token), so we can reuse them. Each URL carries:
//   itag  -> which quality/format this is
//   clen  -> full size of the stream in bytes
//   dur   -> duration in seconds (lets us tell the video apart from ads)
//   range -> the small piece the player wanted; we remove it to get the whole file

const YT_ITAGS = {
  // video + audio in one file (old progressive formats)
  18: { kind: "av", h: 360, ext: "mp4" },
  22: { kind: "av", h: 720, ext: "mp4" },
  // H.264 (mp4)
  160: { kind: "v", h: 144, ext: "mp4" },
  133: { kind: "v", h: 240, ext: "mp4" },
  134: { kind: "v", h: 360, ext: "mp4" },
  135: { kind: "v", h: 480, ext: "mp4" },
  136: { kind: "v", h: 720, ext: "mp4" },
  137: { kind: "v", h: 1080, ext: "mp4" },
  298: { kind: "v", h: 720, fps: 60, ext: "mp4" },
  299: { kind: "v", h: 1080, fps: 60, ext: "mp4" },
  264: { kind: "v", h: 1440, ext: "mp4" },
  266: { kind: "v", h: 2160, ext: "mp4" },
  // AV1 (mp4)
  394: { kind: "v", h: 144, ext: "mp4", codec: "AV1" },
  395: { kind: "v", h: 240, ext: "mp4", codec: "AV1" },
  396: { kind: "v", h: 360, ext: "mp4", codec: "AV1" },
  397: { kind: "v", h: 480, ext: "mp4", codec: "AV1" },
  398: { kind: "v", h: 720, ext: "mp4", codec: "AV1" },
  399: { kind: "v", h: 1080, ext: "mp4", codec: "AV1" },
  400: { kind: "v", h: 1440, ext: "mp4", codec: "AV1" },
  401: { kind: "v", h: 2160, ext: "mp4", codec: "AV1" },
  // VP9 (webm)
  278: { kind: "v", h: 144, ext: "webm" },
  242: { kind: "v", h: 240, ext: "webm" },
  243: { kind: "v", h: 360, ext: "webm" },
  244: { kind: "v", h: 480, ext: "webm" },
  247: { kind: "v", h: 720, ext: "webm" },
  248: { kind: "v", h: 1080, ext: "webm" },
  271: { kind: "v", h: 1440, ext: "webm" },
  313: { kind: "v", h: 2160, ext: "webm" },
  302: { kind: "v", h: 720, fps: 60, ext: "webm" },
  303: { kind: "v", h: 1080, fps: 60, ext: "webm" },
  308: { kind: "v", h: 1440, fps: 60, ext: "webm" },
  315: { kind: "v", h: 2160, fps: 60, ext: "webm" },
  // audio only
  139: { kind: "a", abr: 48, ext: "m4a" },
  140: { kind: "a", abr: 128, ext: "m4a" },
  141: { kind: "a", abr: 256, ext: "m4a" },
  249: { kind: "a", abr: 50, ext: "webm" },
  250: { kind: "a", abr: 70, ext: "webm" },
  251: { kind: "a", abr: 160, ext: "webm" },
};

// Parameters that only describe the player's current small request.
const PIECE_PARAMS = ["range", "rn", "rbuf", "ump", "srfvp", "alr"];

/** Turn a captured videoplayback URL into a stream description, or null. */
function parseStream(rawUrl) {
  let u;
  try {
    u = new URL(rawUrl);
  } catch {
    return null;
  }
  if (!/(^|\.)googlevideo\.com$/i.test(u.hostname) || u.pathname !== "/videoplayback") return null;
  const p = u.searchParams;
  // New streaming protocol: one POST carries many pieces, no reusable URL.
  if (p.get("sabr") === "1") return { sabr: true };
  const itag = Number(p.get("itag"));
  if (!itag) return null;
  const mime = (p.get("mime") || "").toLowerCase();
  let info = YT_ITAGS[itag];
  if (!info) {
    if (mime.startsWith("audio/")) info = { kind: "a", abr: 0, ext: mime.includes("webm") ? "webm" : "m4a" };
    else if (mime.startsWith("video/")) info = { kind: "v", h: 0, ext: mime.includes("webm") ? "webm" : "mp4" };
    else return null;
  }
  for (const k of PIECE_PARAMS) p.delete(k);
  // videos can have several sound tracks: "xtags" says which language and
  // whether it is the original one or a translated ("dubbed") one
  const xtags = (p.get("xtags") || "").toLowerCase();
  const lang = (/(?:^|:)lang=([\w-]+)/.exec(xtags) || [])[1] || "";
  const acont = (/(?:^|:)acont=([\w-]+)/.exec(xtags) || [])[1] || "";
  return {
    lang,
    original: acont === "original",
    dubbed: acont.startsWith("dubbed") || acont === "descriptive",
    itag,
    url: u.toString(),
    mime,
    size: Number(p.get("clen")) || 0,
    dur: Math.round(Number(p.get("dur")) || 0),
    ...info,
  };
}

/** Video id of a YouTube page URL (watch?v=, /shorts/, /embed/, youtu.be). */
function videoIdOf(pageUrl) {
  try {
    const u = new URL(pageUrl);
    if (u.hostname === "youtu.be") return u.pathname.slice(1) || null;
    if (u.searchParams.get("v")) return u.searchParams.get("v");
    const m = /^\/(shorts|embed|live)\/([\w-]{6,})/.exec(u.pathname);
    return m ? m[2] : null;
  } catch {
    return null;
  }
}

/** The resolution in plain words: SD, HD, Full HD, 2K, 4K, 8K. */
function qualityTag(h) {
  if (!h) return "";
  if (h >= 4320) return "8K";
  if (h >= 2160) return "4K";
  if (h >= 1440) return "2K";
  if (h >= 1080) return "Full HD";
  if (h >= 720) return "HD";
  return "SD";
}

/** The "MP3" choice: the app downloads the sound and makes an MP3 file of it. */
const MP3_KEY = "audio|MP3";
const MP3_KBPS = 192;

function fmtSize(n) {
  if (!n) return "";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
}

/**
 * Build the download choices from all captured streams of a tab.
 * Ads are separate short streams, so we keep the group with the longest
 * duration (the actual video).
 */
function buildOptions(streams) {
  const list = Object.values(streams || {});
  if (!list.length) return [];
  const groups = new Map();
  for (const s of list) {
    const key = s.dur || 0;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(s);
  }
  const mainDur = [...groups.keys()].sort((a, b) => b - a || groups.get(b).length - groups.get(a).length)[0];
  const main = groups.get(mainDur);

  // the video's own language first, translated tracks last, then quality
  const trackRank = (s) => (s.original ? 2 : s.dubbed ? 0 : 1);
  const audios = main
    .filter((s) => s.kind === "a")
    .sort((a, b) => trackRank(b) - trackRank(a) || (b.abr || 0) - (a.abr || 0) || b.size - a.size);
  const bestAudio = (ext) => audios.find((a) => (ext === "webm" ? a.ext === "webm" : a.ext === "m4a"));

  const options = [];
  const seen = new Set();
  const videos = main
    .filter((s) => s.kind === "v" || s.kind === "av")
    .sort((a, b) => (b.h || 0) - (a.h || 0) || (b.fps || 30) - (a.fps || 30) || b.size - a.size);

  for (const v of videos) {
    const q = `${v.h ? v.h + "p" : "video"}${v.fps && v.fps > 30 ? v.fps : ""}`;
    const fmt = v.codec ? `${v.ext.toUpperCase()} ${v.codec}` : v.ext.toUpperCase();
    const tag = qualityTag(v.h);
    const named = tag ? `${q} ${tag}` : q;
    const kbpsOf = (size) => (size && v.dur ? Math.round((size * 8) / v.dur / 1000) : 0);
    const key = `${q}|${fmt}`;
    if (seen.has(key)) continue;
    seen.add(key);
    if (v.kind === "av") {
      options.push({
        label: `${named} · ${fmt}`,
        sub: `${fmtSize(v.size)} · video with sound`,
        quality: q,
        tag,
        codec: v.codec || "",
        duration: v.dur || 0,
        kbps: kbpsOf(v.size),
        videoUrl: v.url,
        audioUrl: null,
        ext: v.ext,
        size: v.size,
      });
      continue;
    }
    // same container when possible, otherwise put both into .mkv
    let audio = bestAudio(v.ext);
    let ext = v.ext;
    if (!audio && audios.length) {
      audio = audios[0];
      ext = "mkv";
    }
    if (!audio) continue; // the player has not loaded any sound yet
    options.push({
      label: `${named} · ${ext === "mkv" ? "MKV" : fmt}`,
      sub: fmtSize(v.size + audio.size),
      quality: q,
      tag,
      codec: v.codec || "",
      duration: v.dur || 0,
      kbps: kbpsOf(v.size + audio.size),
      videoUrl: v.url,
      audioUrl: audio.url,
      ext,
      size: v.size + audio.size,
    });
  }
  const audioOnly = bestAudio("m4a") || audios[0];
  if (audioOnly) {
    options.push({
      label: `Audio only · ${audioOnly.ext.toUpperCase()}${audioOnly.abr ? " " + audioOnly.abr + "k" : ""}`,
      sub: fmtSize(audioOnly.size),
      quality: "audio",
      duration: audioOnly.dur || 0,
      kbps: audioOnly.abr || 0,
      videoUrl: audioOnly.url,
      audioUrl: null,
      ext: audioOnly.ext,
      size: audioOnly.size,
    });
    // the same sound as an MP3 file (made by the app after the download)
    const mp3Size = audioOnly.dur ? Math.round((audioOnly.dur * MP3_KBPS * 1000) / 8) : 0;
    options.push({
      label: `Audio · MP3 ${MP3_KBPS}k`,
      sub: mp3Size ? `about ${fmtSize(mp3Size)}` : "",
      key: MP3_KEY,
      quality: "mp3",
      duration: audioOnly.dur || 0,
      kbps: MP3_KBPS,
      videoUrl: audioOnly.url,
      audioUrl: null,
      ext: "mp3",
      size: mp3Size,
    });
  }
  return options;
}

/** "Some title - YouTube" -> "Some title", safe for Windows file names. */
function cleanYoutubeTitle(title) {
  return (title || "video")
    .replace(/^\(\d+\)\s*/, "")
    .replace(/\s*-\s*YouTube\s*$/i, "")
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_")
    .trim()
    .slice(0, 120) || "video";
}

if (typeof module !== "undefined") {
  module.exports = { YT_ITAGS, parseStream, videoIdOf, buildOptions, cleanYoutubeTitle, fmtSize, qualityTag, MP3_KEY };
}
