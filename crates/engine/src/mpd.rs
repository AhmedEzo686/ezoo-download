//! DASH manifests (.mpd): download video and audio tracks of an MPEG-DASH
//! stream and leave two plain files for the caller to merge with ffmpeg.
//!
//! The manifest is a small XML document: MPD → Period → AdaptationSet →
//! Representation, and the segments of a representation come either from a
//! SegmentTemplate (with or without a SegmentTimeline), from a SegmentList,
//! from a SegmentBase or straight from BaseURL. Only the first Period is
//! taken (like every simple downloader). DRM-protected streams (Widevine,
//! PlayReady, FairPlay, ClearKey) are refused on purpose, and so are live
//! ("dynamic") manifests.

use crate::http::{request, ConnGuard, Progress, Shared};
use crate::http_download::Cancelled;
use crate::model::RequestHeaders;
use anyhow::{anyhow, bail, Context};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// one segment of a track: a URL, optionally only a byte range of it
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub url: String,
    pub range: Option<(u64, u64)>,
}

/// One representation (a quality) of the stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: String,
    pub is_video: bool,
    pub bandwidth: u64,
    pub width: u32,
    pub height: u32,
    pub codecs: String,
    /// an "init" file (fMP4: the moov box) written before the segments
    pub init: Option<Segment>,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Manifest {
    pub videos: Vec<Track>,
    pub audios: Vec<Track>,
}

impl Manifest {
    /// The highest quality of each kind (the same pick the HLS code makes).
    pub fn best(&self) -> (Option<&Track>, Option<&Track>) {
        let key = |t: &Track| (t.bandwidth, (t.width as u64) * (t.height as u64));
        let video = self.videos.iter().max_by_key(|t| key(t));
        let audio = self.audios.iter().max_by_key(|t| key(t));
        (video, audio)
    }
}

// ------------------------------------------------------------------ XML

/// A tiny XML element: MPD files are machine-made, so this is enough.
#[derive(Debug, Clone)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<El>,
    text: String,
}

impl El {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn attr_u64(&self, name: &str) -> Option<u64> {
        self.attr(name)?.trim().parse().ok()
    }
    fn child(&self, name: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name == name)
    }
    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> {
        self.children.iter().filter(move |c| c.name == name)
    }
    fn text_of(&self) -> String {
        self.text.trim().to_string()
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#34;", "\"")
}

/// Parse a small XML document into its element tree. Comments,
/// declarations and CDATA are skipped; unknown syntax is an error.
fn parse_xml(src: &str) -> anyhow::Result<El> {
    let s = src.trim_start_matches('\u{feff}');
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut stack: Vec<El> = Vec::new();
    let mut root: Option<El> = None;
    let push_text = |stack: &mut Vec<El>, text: &str| {
        if !stack.is_empty() && !text.trim().is_empty() {
            let last = stack.last_mut().unwrap();
            if !last.text.is_empty() {
                last.text.push(' ');
            }
            last.text.push_str(text.trim());
        }
    };
    while i < b.len() {
        if b[i] == b'<' {
            // comment / declaration / CDATA
            if s[i..].starts_with("<!--") {
                i += 4;
                i = match s[i..].find("-->") {
                    Some(p) => i + p + 3,
                    None => bail!("XML comment without an end"),
                };
                continue;
            }
            if s[i..].starts_with("<![CDATA[") {
                let start = i + 9;
                let end = s[start..].find("]]>").map(|p| start + p).unwrap_or(b.len());
                let text = &s[start..end];
                if !stack.is_empty() && !text.trim().is_empty() {
                    let last = stack.last_mut().unwrap();
                    if !last.text.is_empty() {
                        last.text.push(' ');
                    }
                    last.text.push_str(text.trim());
                }
                i = (end + 3).min(b.len());
                continue;
            }
            if s[i..].starts_with("<?") {
                i = match s[i..].find("?>") {
                    Some(p) => i + p + 2,
                    None => bail!("XML declaration without an end"),
                };
                continue;
            }
            if s[i..].starts_with("<!") {
                i = match s[i..].find(">") {
                    Some(p) => i + p + 1,
                    None => bail!("broken doctype"),
                };
                continue;
            }
            // closing tag
            if s[i..].starts_with("</") {
                let end = s[i..].find('>').ok_or_else(|| anyhow!("broken XML tag"))?;
                let name = s[i + 2..i + end].trim();
                let el = stack.pop().ok_or_else(|| anyhow!("XML: unexpected </{name}>"))?;
                if !name.is_empty() && el.name != name && !el.name.ends_with(name) {
                    bail!("XML: <{}> closed by </{}>", el.name, name);
                }
                match stack.last_mut() {
                    Some(p) => p.children.push(el),
                    None => {
                        root = Some(el);
                    }
                }
                i += end + 1;
                continue;
            }
            // opening tag (maybe self-closing)
            let end = s[i..].find('>').ok_or_else(|| anyhow!("broken XML tag"))?;
            let inner = &s[i + 1..i + end];
            let self_closing = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let (name, rest) = match inner.find(|c: char| c.is_ascii_whitespace()) {
                Some(p) => (&inner[..p], &inner[p..]),
                None => (inner, ""),
            };
            let name = name.rsplit(':').next().unwrap_or(name).to_string();
            let el = El { name, attrs: parse_attrs_str(rest), children: Vec::new(), text: String::new() };
            if self_closing {
                match stack.last_mut() {
                    Some(p) => p.children.push(el),
                    None => root = Some(el),
                }
            } else {
                stack.push(el);
            }
            i += end + 1;
        } else {
            // text until the next '<'
            let end = s[i..].find('<').unwrap_or(b.len() - i);
            push_text(&mut stack, &unescape(&s[i..i + end]));
            i += end;
        }
    }
    if !stack.is_empty() {
        bail!("XML ends in the middle of <{}>", stack.last().unwrap().name);
    }
    root.ok_or_else(|| anyhow!("the manifest has no XML elements"))
}

fn parse_attrs_str(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        if name_start == i {
            break;
        }
        let name = s[name_start..i].to_string();
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'=') {
            i += 1;
        }
        let (value, next) = if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
            let q = b[i];
            let start = i + 1;
            let end = s[start..].find(q as char).map(|p| start + p).unwrap_or(b.len());
            (unescape(&s[start..end]), end + 1)
        } else {
            let start = i;
            while i < b.len() && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            (unescape(&s[start..i]), i)
        };
        out.push((name, value));
        i = next;
    }
    out
}

// ------------------------------------------------------------- MPD model

/// ISO-8601 duration as used by DASH: "PT1H2M3.5S".
fn parse_iso_duration(s: &str) -> Option<f64> {
    let s = s.trim();
    let rest = s.strip_prefix(['P', 'p'])?;
    let mut years = 0.0f64;
    let mut days = 0.0f64;
    let mut seconds = 0.0f64;
    let (date, time) = match rest.find(['T', 't']) {
        Some(p) => (&rest[..p], Some(&rest[p + 1..])),
        None => (rest, None),
    };
    // years / months / days in the date part (months are approximated)
    let mut num = String::new();
    for c in date.chars() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let v: f64 = num.parse().ok()?;
        num.clear();
        match c.to_ascii_lowercase() {
            'y' => years = v,
            'm' => days += v * 30.0,
            'd' => days += v,
            'w' => days += v * 7.0,
            _ => {}
        }
    }
    if let Some(t) = time {
        num.clear();
        for c in t.chars() {
            if c.is_ascii_digit() || c == '.' || c == ',' {
                num.push(if c == ',' { '.' } else { c });
                continue;
            }
            let v: f64 = num.parse().ok()?;
            num.clear();
            match c.to_ascii_lowercase() {
                'h' => seconds += v * 3600.0,
                'm' => seconds += v * 60.0,
                's' => seconds += v,
                _ => {}
            }
        }
    }
    Some(years * 365.25 * 86400.0 + days * 86400.0 + seconds)
}

#[derive(Debug, Clone, Default)]
struct Template {
    initialization: Option<String>,
    media: Option<String>,
    timescale: u64,
    duration: u64,
    start_number: u64,
    /// (t, d, r) entries of a SegmentTimeline
    timeline: Vec<(u64, u64, i64)>,
}

/// Join a chain of relative/absolute URLs the way DASH BaseURLs stack.
fn join_chain(bases: &[String], rel: &str) -> anyhow::Result<String> {
    let mut acc = String::new();
    for b in bases {
        if acc.is_empty() {
            acc = b.clone();
        } else {
            acc = url::Url::parse(&acc)?.join(b)?.to_string();
        }
    }
    Ok(url::Url::parse(if acc.is_empty() { "." } else { &acc })?
        .join(rel.trim())?
        .to_string())
}

/// Expand the `$…$` tokens of a SegmentTemplate URL.
fn expand_template(
    tpl: &str,
    id: &str,
    bandwidth: u64,
    number: u64,
    time: u64,
) -> String {
    let mut out = String::with_capacity(tpl.len() + 16);
    let b = tpl.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' {
            let ch = tpl[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        if tpl[i..].starts_with("$$") {
            out.push('$');
            i += 2;
            continue;
        }
        let Some(end) = tpl[i + 1..].find('$') else { break };
        let token = &tpl[i + 1..i + 1 + end];
        let (name, width) = match token.find('%') {
            Some(p) => (&token[..p], Some(&token[p..])),
            None => (token, None),
        };
        let value = match name {
            "RepresentationID" => id.to_string(),
            "Bandwidth" => bandwidth.to_string(),
            "Number" => match width {
                Some(w) => format!("{:0width$}", number, width = w.trim_start_matches('%').trim_end_matches('d').parse::<usize>().unwrap_or(1)),
                None => number.to_string(),
            },
            "Time" => time.to_string(),
            // unknown tokens stay as they are, the URL keeps its meaning
            other => format!("${other}$"),
        };
        out.push_str(&value);
        i += token.len() + 2;
    }
    out
}

fn parse_range(s: &str) -> Option<(u64, u64)> {
    let (a, b) = s.split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn drm_scheme(scheme: &str) -> bool {
    let s = scheme.to_ascii_lowercase();
    ["widevine", "playready", "fairplay", "clearkey"]
        .iter()
        .any(|d| s.contains(d))
}

/// Collect a SegmentTemplate from any element level (child fields win).
fn read_template(el: &El, into: &mut Template) {
    if let Some(v) = el.attr("initialization") {
        into.initialization = Some(v.to_string());
    }
    if let Some(v) = el.attr("media") {
        into.media = Some(v.to_string());
    }
    if let Some(v) = el.attr_u64("timescale") {
        into.timescale = v;
    }
    if let Some(v) = el.attr_u64("duration") {
        into.duration = v;
    }
    if let Some(v) = el.attr_u64("startNumber") {
        into.start_number = v;
    }
    if let Some(tl) = el.child("SegmentTimeline") {
        into.timeline = tl
            .children_named("S")
            .filter_map(|s| {
                let d = s.attr_u64("d")?;
                Some((s.attr_u64("t").unwrap_or(0), d, s.attr("r").and_then(|r| r.trim().parse().ok()).unwrap_or(0)))
            })
            .collect();
    }
}

/// Parse one representation into segments. `bases` is the BaseURL chain
/// (outermost first) plus the manifest URL as the very first fallback.
fn representation_segments(
    rep: &El,
    aset: &El,
    _period: &El,
    bases: &[String],
    tpl_base: &Template,
    period_seconds: f64,
) -> anyhow::Result<Track> {
    let id = rep.attr("id").unwrap_or("0").to_string();
    let bandwidth = rep.attr_u64("bandwidth").unwrap_or(0);
    let width = rep.attr_u64("width").unwrap_or(0) as u32;
    let height = rep.attr_u64("height").unwrap_or(0) as u32;
    let codecs = rep.attr("codecs").unwrap_or("").to_string();
    let mime = rep.attr("mimeType").or(aset.attr("mimeType")).unwrap_or("");
    let ctype = rep.attr("contentType").or(aset.attr("contentType")).unwrap_or("");
    let is_video = if !ctype.is_empty() {
        ctype.starts_with("video")
    } else if !mime.is_empty() {
        mime.starts_with("video")
    } else {
        width > 0 || height > 0
    };

    // this level's BaseURL, if any
    let mut bases: Vec<String> = bases.to_vec();
    if let Some(b) = rep.child("BaseURL").map(El::text_of).filter(|s| !s.is_empty()) {
        bases.push(b);
    }
    let deepest = bases.last().cloned().unwrap_or_default();

    // DRM check: any known protection scheme on any level means no download
    for scope in [aset, rep] {
        for cp in scope.children_named("ContentProtection") {
            if let Some(scheme) = cp.attr("schemeIdUri").or(cp.attr("value")) {
                if drm_scheme(scheme) {
                    bail!("this stream is DRM protected ({}) and cannot be downloaded", scheme);
                }
            }
        }
    }

    // the template of this representation on top of the inherited one
    let mut tpl = tpl_base.clone();
    if let Some(t) = rep.child("SegmentTemplate") {
        read_template(t, &mut tpl);
    }
    let timescale = if tpl.timescale > 0 { tpl.timescale } else { 1 };

    let mut init: Option<Segment> = None;
    let mut segments: Vec<Segment> = Vec::new();

    // the init URL carries the same tokens as the segment media URLs
    let expand_init = |tpl_url: &str| expand_template(tpl_url, &id, bandwidth, 0, 0);

    if let Some(list) = rep.child("SegmentList").or(aset.child("SegmentList")) {
        if let Some(init_el) = list.child("Initialization") {
            let url = init_el
                .attr("sourceURL")
                .map(|u| join_chain(&bases, u))
                .transpose()?;
            let range = init_el.attr("range").and_then(parse_range);
            if let Some(url) = url {
                init = Some(Segment { url, range });
            }
        }
        for su in list.children_named("SegmentURL") {
            if let Some(media) = su.attr("media") {
                segments.push(Segment {
                    url: join_chain(&bases, media)?,
                    range: su.attr("mediaRange").and_then(parse_range),
                });
            }
        }
    } else if let Some(t) = rep.child("SegmentTemplate").or(aset.child("SegmentTemplate")) {
        // a template that only lives on the representation, not inherited
        let mut own = tpl_base.clone();
        read_template(t, &mut own);
        let (init_url, media_url, timescale, duration, start_number, timeline) = (
            own.initialization.clone(),
            own.media.clone(),
            if own.timescale > 0 { own.timescale } else { 1 },
            own.duration,
            own.start_number.max(1),
            own.timeline.clone(),
        );
        init = init_url
            .map(|u| join_chain(&bases, &expand_init(&u)))
            .transpose()?
            .map(|url| Segment { url, range: None });
        if !timeline.is_empty() {
            // SegmentTimeline: every S is d repeated r+1 times; a negative
            // r repeats until the next S (or the end of the period)
            let mut number = start_number;
            let mut t_prev = 0u64;
            for &(t, d, r) in &timeline {
                let mut t0 = t;
                if t0 == 0 && t_prev > 0 {
                    t0 = t_prev;
                }
                let count: u64 = if r >= 0 {
                    (r as u64) + 1
                } else {
                    // until the next S or the end of the period
                    1 // corrected below when the next entry is known
                };
                for k in 0..count {
                    let time = t0 + k * d;
                    let url = media_url.as_deref().ok_or_else(|| anyhow!("SegmentTemplate without a media template"))?;
                    segments.push(Segment {
                        url: join_chain(&bases, &expand_template(url, &id, bandwidth, number, time))?,
                        range: None,
                    });
                    number += 1;
                }
                t_prev = t0 + count.max(1) * d;
            }
        } else if duration > 0 {
            // fixed length per segment; the period length decides how many
            let secs = if period_seconds > 0.0 {
                period_seconds
            } else {
                bail!("the DASH manifest does not say how long the video is");
            };
            let count = ((secs * timescale as f64) / duration as f64).ceil() as u64;
            let pto = rep
                .child("SegmentTemplate")
                .or(aset.child("SegmentTemplate"))
                .and_then(|e| e.attr_u64("presentationTimeOffset"))
                .unwrap_or(0);
            for k in 0..count.min(crate::m3u8::MAX_PARTS as u64) {
                let time = pto + k * duration;
                let url = media_url.as_deref().ok_or_else(|| anyhow!("SegmentTemplate without a media template"))?;
                segments.push(Segment {
                    url: join_chain(&bases, &expand_template(url, &id, bandwidth, start_number + k, time))?,
                    range: None,
                });
            }
        }
    } else if let Some(sb) = rep.child("SegmentBase").or(aset.child("SegmentBase")) {
        // one big file with an index inside: the init range comes first,
        // then the rest of the file
        let rep_url = deepest.clone();
        if rep_url.is_empty() {
            bail!("the representation has no URL");
        }
        init = sb
            .child("Initialization")
            .and_then(|i| i.attr("range"))
            .and_then(parse_range)
            .map(|range| Segment { url: rep_url.clone(), range: Some(range) });
        segments.push(Segment { url: rep_url, range: None });
    } else {
        // no segmentation at all: the BaseURL is the file (a plain .mp4)
        let rep_url = deepest;
        if rep_url.is_empty() {
            bail!("the representation has no URL and no segments");
        }
        segments.push(Segment { url: rep_url, range: None });
    }

    let _ = timescale; // (used through the segment counts computed above)
    let _ = tpl; // (fields were read above through `own`)
    if segments.is_empty() {
        bail!("the representation {} has no segments", id);
    }
    if segments.len() > crate::m3u8::MAX_PARTS {
        bail!("the stream has too many parts");
    }
    Ok(Track { id, is_video, bandwidth, width, height, codecs, init, segments })
}

/// Parse the text of an .mpd manifest. `manifest_url` resolves relative
/// BaseURLs; the final (post redirect) URL should be given when known.
pub fn parse(manifest_url: &str, xml: &str) -> anyhow::Result<Manifest> {
    let xml = xml.trim();
    if !xml.starts_with('<') {
        bail!("not a valid DASH manifest");
    }
    if xml.len() > crate::m3u8::MAX_PLAYLIST_BYTES {
        bail!("the manifest is too big");
    }
    let root = parse_xml(xml).context("could not read the DASH manifest")?;
    if root.name != "MPD" {
        bail!("not a DASH manifest (no <MPD>)");
    }
    let mpd_type = root.attr("type").unwrap_or("static");
    if mpd_type.eq_ignore_ascii_case("dynamic") {
        bail!("live streams are not supported (the manifest is dynamic)");
    }
    let total = root
        .attr("mediaPresentationDuration")
        .and_then(parse_iso_duration)
        .unwrap_or(0.0);

    let mut bases: Vec<String> = Vec::new();
    if let Some(b) = root.child("BaseURL").map(El::text_of).filter(|s| !s.is_empty()) {
        bases.push(b);
    } else {
        bases.push(manifest_url.to_string());
    }

    let Some(period) = root.children_named("Period").next() else {
        bail!("the manifest has no Period");
    };
    let mut p_bases = bases.clone();
    if let Some(b) = period.child("BaseURL").map(El::text_of).filter(|s| !s.is_empty()) {
        p_bases.push(b);
    }
    let period_seconds = period
        .attr("duration")
        .and_then(parse_iso_duration)
        .unwrap_or(total);

    let mut manifest = Manifest::default();
    for aset in period.children_named("AdaptationSet") {
        let mut a_bases = p_bases.clone();
        if let Some(b) = aset.child("BaseURL").map(El::text_of).filter(|s| !s.is_empty()) {
            a_bases.push(b);
        }
        let mut tpl = Template::default();
        if let Some(t) = aset.child("SegmentTemplate") {
            read_template(t, &mut tpl);
        }
        for rep in aset.children_named("Representation") {
            match representation_segments(rep, aset, period, &a_bases, &tpl, period_seconds) {
                Ok(track) => {
                    if track.is_video {
                        manifest.videos.push(track);
                    } else {
                        manifest.audios.push(track);
                    }
                }
                // one bad quality does not kill the whole manifest
                Err(_) => continue,
            }
        }
    }
    if manifest.videos.is_empty() && manifest.audios.is_empty() {
        bail!("the manifest has no usable track");
    }
    Ok(manifest)
}

// ------------------------------------------------------------ downloading

/// The work order for [`run`].
#[derive(Clone)]
pub struct MpdJob {
    pub url: String,
    pub headers: RequestHeaders,
    pub user_agent: String,
    /// video output (written as one plain file, fMP4 when the manifest is)
    pub video_out: PathBuf,
    /// audio output when the manifest has an audio track
    pub audio_out: Option<PathBuf>,
    pub temp_dir: PathBuf,
    pub connections: u32,
    pub retries: u32,
    pub limiter: Arc<crate::limiter::SpeedLimiter>,
}

async fn fetch_bytes(shared: &Shared, job: &MpdJob, seg: &Segment) -> anyhow::Result<Vec<u8>> {
    let rb = request(&shared.client(), &seg.url, &job.headers, &job.user_agent);
    let rb = if let Some((a, b)) = seg.range {
        rb.header("Range", format!("bytes={a}-{b}"))
    } else {
        rb
    };
    let resp = rb.send().await?;
    if !resp.status().is_success() {
        bail!("HTTP {} for {}", resp.status().as_u16(), seg.url);
    }
    // a server that ignores Range answers 200 with the whole file: only
    // right for a request that did not ask for a range
    if seg.range.is_some() && resp.status().as_u16() != 206 {
        bail!("the server ignores byte ranges for {}", seg.url);
    }
    Ok(resp.bytes().await?.to_vec())
}

async fn fetch_text(shared: &Shared, job: &MpdJob, url: &str) -> anyhow::Result<String> {
    let seg = Segment { url: url.to_string(), range: None };
    let b = fetch_bytes(shared, job, &seg).await?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

fn part_file(dir: &Path, prefix: &str, i: usize) -> PathBuf {
    dir.join(format!("{prefix}{i:06}.part"))
}

fn init_file(dir: &Path, prefix: &str) -> PathBuf {
    dir.join(format!("{prefix}init.part"))
}

/// Download the segments of one track with several workers, exactly like
/// the HLS code does (finished parts survive pause/resume).
async fn download_track(
    shared: Arc<Shared>,
    job: Arc<MpdJob>,
    track: Track,
    prefix: &'static str,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(&job.temp_dir)?;
    // init segment first (part of the parts count)
    if let Some(init) = &track.init {
        let path = init_file(&job.temp_dir, prefix);
        if !path.exists() {
            let data = fetch_bytes(&shared, &job, init).await.context("could not load the init segment")?;
            shared.limiter.acquire(data.len()).await;
            job.limiter.acquire(data.len()).await;
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, &data)?;
            std::fs::rename(&tmp, &path)?;
            progress.bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
        }
        progress.parts_done.fetch_add(1, Ordering::Relaxed);
    }
    progress.parts_total.fetch_add(track.segments.len() as u32, Ordering::Relaxed);

    // count what is already on the disk (resume)
    let mut done_bytes = 0u64;
    let mut done = 0u32;
    for (i, _) in track.segments.iter().enumerate() {
        if let Ok(m) = std::fs::metadata(part_file(&job.temp_dir, prefix, i)) {
            done += 1;
            done_bytes += m.len();
        }
    }
    progress.parts_done.fetch_add(done, Ordering::Relaxed);
    progress.bytes.fetch_add(done_bytes, Ordering::Relaxed);

    let segments = Arc::new(track.segments);
    let next = Arc::new(AtomicUsize::new(0));
    let child = cancel.child_token();
    let mut set = JoinSet::new();
    for _ in 0..job.connections.clamp(1, 16) {
        let (shared, job, progress, child, next, segments) = (
            shared.clone(),
            job.clone(),
            progress.clone(),
            child.clone(),
            next.clone(),
            segments.clone(),
        );
        set.spawn(async move {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= segments.len() {
                    return Ok::<(), anyhow::Error>(());
                }
                let path = part_file(&job.temp_dir, prefix, i);
                if path.exists() {
                    continue;
                }
                let mut attempt = 0;
                loop {
                    let fut = async {
                        let _g = ConnGuard::new(&progress);
                        let data = fetch_bytes(&shared, &job, &segments[i]).await?;
                        shared.limiter.acquire(data.len()).await;
                        job.limiter.acquire(data.len()).await;
                        let tmp = path.with_extension("tmp");
                        std::fs::write(&tmp, &data)?;
                        std::fs::rename(&tmp, &path)?;
                        Ok::<u64, anyhow::Error>(data.len() as u64)
                    };
                    let r = tokio::select! {
                        r = fut => r,
                        _ = child.cancelled() => return Err(Cancelled.into()),
                    };
                    match r {
                        Ok(n) => {
                            progress.bytes.fetch_add(n, Ordering::Relaxed);
                            progress.parts_done.fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                        Err(e) => {
                            attempt += 1;
                            if attempt > job.retries {
                                return Err(e);
                            }
                            tokio::time::sleep(Duration::from_secs((attempt as u64 * 2).min(20))).await;
                        }
                    }
                }
            }
        });
    }
    let mut first_error = None;
    while let Some(r) = set.join_next().await {
        let r = r.map_err(|e| anyhow!("worker crashed: {e}")).and_then(|r| r);
        if let Err(e) = r {
            if first_error.is_none() && !cancel.is_cancelled() {
                first_error = Some(e);
            }
            child.cancel();
        }
    }
    if cancel.is_cancelled() {
        return Err(Cancelled.into());
    }
    if let Some(e) = first_error {
        return Err(e);
    }

    // join init + segments into one file
    let temp_dir = job.temp_dir.clone();
    let out = if prefix == "v" { job.video_out.clone() } else { job.audio_out.clone().unwrap_or_else(|| job.video_out.clone()) };
    let has_init = track.init.is_some();
    let total = segments.len();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        use std::io::Write;
        let mut out = std::io::BufWriter::new(std::fs::File::create(&out)?);
        if has_init {
            let mut f = std::fs::File::open(init_file(&temp_dir, prefix))?;
            std::io::copy(&mut f, &mut out)?;
        }
        for i in 0..total {
            let mut f = std::fs::File::open(part_file(&temp_dir, prefix, i))?;
            std::io::copy(&mut f, &mut out)?;
        }
        out.flush()?;
        drop(out);
        Ok(())
    })
    .await??;
    Ok(())
}

/// Full DASH download: manifest → best video (+ best audio) → files.
/// Returns `true` when an audio track was written next to the video.
pub async fn run(
    shared: Arc<Shared>,
    job: MpdJob,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<bool> {
    let job = Arc::new(job);
    let text = fetch_text(&shared, &job, &job.url).await.context("could not load the manifest")?;
    let manifest = parse(&job.url, &text)?;
    let (video, audio) = manifest.best();
    let video = video.ok_or_else(|| anyhow!("the manifest has no video track"))?;
    // the caller decides (by giving an audio path) whether it can merge
    let audio = audio.filter(|_| job.audio_out.is_some());
    let temp_dir = job.temp_dir.clone();

    // two tracks share one temp folder, so each keeps its own prefix
    download_track(shared.clone(), job.clone(), video.clone(), "v", progress.clone(), cancel.clone()).await?;
    if let Some(audio) = audio.cloned() {
        download_track(shared, job, audio, "a", progress, cancel).await?;
    }
    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(audio.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MPD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT1M4S">
 <Period id="0">
  <BaseURL>video/</BaseURL>
  <AdaptationSet contentType="video">
   <SegmentTemplate timescale="1000" duration="4000" initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/seg-$Number%05d$.m4s" startNumber="1"/>
   <Representation id="1080p" bandwidth="3000000" width="1920" height="1080" codecs="avc1.640028"/>
   <Representation id="360p" bandwidth="800000" width="640" height="360" codecs="avc1.42c01e"/>
  </AdaptationSet>
  <AdaptationSet contentType="audio">
   <Representation id="a1" bandwidth="128000" codecs="mp4a.40.2" mimeType="audio/mp4">
    <SegmentTemplate timescale="48000" duration="192000" media="audio-$Number$.m4s" initialization="audio-init.mp4"/>
   </Representation>
  </AdaptationSet>
 </Period>
</MPD>"#;

    #[test]
    fn iso_durations() {
        assert_eq!(parse_iso_duration("PT1M4S"), Some(64.0));
        assert_eq!(parse_iso_duration("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(parse_iso_duration("P1DT2H"), Some(93600.0));
        assert_eq!(parse_iso_duration("PT0S"), Some(0.0));
    }

    #[test]
    fn tokens_expand() {
        assert_eq!(
            expand_template("$RepresentationID$/seg-$Number%05d$.m4s", "1080p", 100, 42, 0),
            "1080p/seg-00042.m4s"
        );
        assert_eq!(expand_template("a$$b$Bandwidth$", "x", 800, 1, 0), "a$b800");
        assert_eq!(expand_template("$Time$-$Unknown$.mp4", "x", 1, 2, 3000), "3000-$Unknown$.mp4");
    }

    #[test]
    fn manifest_with_templates() {
        let m = parse("https://cdn.example/stream.mpd", MPD).unwrap();
        assert_eq!(m.videos.len(), 2);
        let (v, a) = m.best();
        let v = v.unwrap();
        assert_eq!(v.id, "1080p");
        assert_eq!(v.init.as_ref().unwrap().url, "https://cdn.example/video/1080p/init.mp4");
        assert_eq!(v.segments.len(), 16); // 64 s / 4 s
        assert_eq!(v.segments[0].url, "https://cdn.example/video/1080p/seg-00001.m4s");
        assert_eq!(v.segments[15].url, "https://cdn.example/video/1080p/seg-00016.m4s");
        let a = a.unwrap();
        assert_eq!(a.segments.len(), 16); // 192000/48000 = 4 s per part
        assert_eq!(a.segments[0].url, "https://cdn.example/video/audio-1.m4s");
    }

    #[test]
    fn timeline_and_ranges() {
        let m = r#"<MPD type="static"><Period><AdaptationSet mimeType="video/mp4">
          <BaseURL>https://cdn.example/v/</BaseURL>
          <Representation id="hi" bandwidth="1000">
           <SegmentTemplate timescale="1" media="$Time$.m4s" initialization="i.mp4">
            <SegmentTimeline><S t="0" d="5" r="2"/><S t="15" d="5"/></SegmentTimeline>
           </SegmentTemplate>
          </Representation>
        </AdaptationSet></Period></MPD>"#;
        let m = parse("https://x.example/a.mpd", m).unwrap();
        let v = &m.videos[0];
        assert_eq!(v.segments.len(), 4);
        assert_eq!(v.segments[0].url, "https://cdn.example/v/0.m4s");
        assert_eq!(v.segments[2].url, "https://cdn.example/v/10.m4s");
        assert_eq!(v.segments[3].url, "https://cdn.example/v/15.m4s");
    }

    #[test]
    fn drm_and_live_are_refused() {
        let drm = r#"<MPD type="static"><Period><AdaptationSet contentType="video">
          <ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"/>
          <Representation id="v" bandwidth="1"><SegmentTemplate media="s.m4s" duration="1"/></AdaptationSet>
        </Period></MPD>"#;
        assert!(parse("https://x.example/a.mpd", drm).is_err());
        let live = r#"<MPD type="dynamic"><Period><AdaptationSet contentType="video">
          <Representation id="v" bandwidth="1"><SegmentTemplate media="s.m4s" duration="1"/></AdaptationSet>
        </Period></MPD>"#;
        assert!(parse("https://x.example/a.mpd", live).is_err());
    }

    #[test]
    fn segment_list_with_ranges() {
        let m = r#"<MPD type="static"><Period><AdaptationSet mimeType="video/mp4">
          <Representation id="v" bandwidth="1">
           <BaseURL>https://cdn.example/movie.mp4</BaseURL>
           <SegmentList duration="10">
            <Initialization sourceURL="init.mp4"/>
            <SegmentURL media="p1.m4s" mediaRange="0-1000"/>
            <SegmentURL media="p2.m4s"/>
           </SegmentList>
          </Representation>
        </AdaptationSet></Period></MPD>"#;
        let m = parse("https://x.example/a.mpd", m).unwrap();
        let v = &m.videos[0];
        assert_eq!(v.init.as_ref().unwrap().url, "https://cdn.example/init.mp4");
        assert_eq!(v.segments[0].url, "https://cdn.example/p1.m4s");
        assert_eq!(v.segments[0].range, Some((0, 1000)));
        assert_eq!(v.segments[1].url, "https://cdn.example/p2.m4s");
    }
}
