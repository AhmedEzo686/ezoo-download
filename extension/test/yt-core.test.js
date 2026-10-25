// node extension/test/yt-core.test.js
const assert = require("assert");
const { parseStream, videoIdOf, buildOptions, cleanYoutubeTitle } = require("../yt-core.js");

const base = "https://rr3---sn-abc.googlevideo.com/videoplayback?expire=1&id=o-AB&pot=TOKEN&n=xyz&sig=S";
const url = (itag, mime, clen, dur, extra = "") =>
  `${base}&itag=${itag}&mime=${encodeURIComponent(mime)}&clen=${clen}&dur=${dur}&range=0-65535&rn=3&rbuf=0${extra}`;

// parsing strips the piece parameters and keeps the unlock tokens
const v = parseStream(url(137, "video/mp4", 150000000, "212.3"));
assert.strictEqual(v.itag, 137);
assert.strictEqual(v.h, 1080);
assert.strictEqual(v.size, 150000000);
assert.strictEqual(v.dur, 212);
assert(!v.url.includes("range="), "range removed");
assert(!v.url.includes("rn="), "rn removed");
assert(v.url.includes("pot=TOKEN") && v.url.includes("sig=S") && v.url.includes("n=xyz"));
assert.deepStrictEqual(parseStream(base + "&itag=137&sabr=1"), { sabr: true });
assert.strictEqual(parseStream("https://example.com/videoplayback?itag=1"), null);

// ids
assert.strictEqual(videoIdOf("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10"), "dQw4w9WgXcQ");
assert.strictEqual(videoIdOf("https://www.youtube.com/shorts/abcDEF12345"), "abcDEF12345");
assert.strictEqual(videoIdOf("https://youtu.be/xyz123"), "xyz123");

// options: ad (dur 15) ignored, mp4 paired with m4a, webm with webm audio
const streams = {};
for (const s of [
  url(137, "video/mp4", 150e6, "212.3"),
  url(136, "video/mp4", 70e6, "212.3"),
  url(248, "video/webm", 120e6, "212.3"),
  url(140, "audio/mp4", 3.4e6, "212.3"),
  url(251, "audio/webm", 3.6e6, "212.3"),
  url(137, "video/mp4", 9e6, "15.0"), // ad
  url(140, "audio/mp4", 0.2e6, "15.0"), // ad audio
].map(parseStream)) streams[`${s.itag}_${s.dur}`] = s;

const opts = buildOptions(streams);
assert.strictEqual(opts[0].quality, "1080p");
assert(opts[0].videoUrl.includes("itag=137") && opts[0].videoUrl.includes("dur=212"));
assert(opts[0].audioUrl.includes("itag=140"));
const webm = opts.find((o) => o.ext === "webm");
assert(webm.audioUrl.includes("itag=251"));
const audioOnly = opts[opts.length - 2];
assert.strictEqual(audioOnly.quality, "audio");
assert.strictEqual(audioOnly.audioUrl, null);
// the MP3 choice: same sound, the app makes the MP3 file
const mp3 = opts[opts.length - 1];
assert.strictEqual(mp3.key, "audio|MP3");
assert.strictEqual(mp3.ext, "mp3");
assert.strictEqual(mp3.videoUrl, audioOnly.videoUrl);
assert(mp3.size > 0 && mp3.duration === 212);
// the resolution in plain words
assert.strictEqual(opts[0].tag, "Full HD");
assert(opts[0].label.startsWith("1080p Full HD"));
const { qualityTag } = require("../yt-core.js");
assert.deepStrictEqual([4320, 2160, 1440, 1080, 720, 480, 0].map(qualityTag), ["8K", "4K", "2K", "Full HD", "HD", "SD", ""]);
assert(opts.every((o) => !o.videoUrl.includes("dur=15")), "ad streams must be ignored");

// no audio captured yet -> only progressive or nothing
assert.deepStrictEqual(buildOptions({ a: parseStream(url(137, "video/mp4", 1, "10")) }), []);

// several sound tracks: the original language wins over a translated one
{
  const tracks = {};
  for (const s of [
    url(137, "video/mp4", 9, "50"),
    url(140, "audio/mp4", 5, "50", "&xtags=acont%3Ddubbed-auto%3Alang%3Den-US"),
    url(140, "audio/mp4", 5, "50", "&xtags=acont%3Doriginal%3Alang%3Dar"),
  ].map(parseStream)) tracks[`${s.itag}_${s.dur}_${s.lang}`] = s;
  const o = buildOptions(tracks);
  assert(o[0].audioUrl.includes("lang%3Dar"), "original Arabic track, not the dubbed one");
}

assert.strictEqual(cleanYoutubeTitle("(3) My video: part 1 - YouTube"), "My video_ part 1");
console.log("yt-core: all tests passed;", opts.map((o) => o.label).join(" | "));
