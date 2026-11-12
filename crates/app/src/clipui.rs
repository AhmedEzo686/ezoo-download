//! "Download a part of a video": the window where an address, the quality,
//! from / to and the kind of cut are chosen, with a look at the part before
//! it is downloaded. The browser add-in sends parts too (`add_part`).

use crate::i18n::{t, tf};
use crate::open_window;
use crate::*;
use std::cell::RefCell;
use swiftget_engine::ytdlp::{self, YtOption};
use swiftget_engine::{clock, parse_clock, Clip, ClipSource};
use tokio_util::sync::CancellationToken;

/// The longest preview (seconds): the first minute of the part.
const PREVIEW_MAX: f64 = 60.0;

#[derive(Default)]
struct State {
    page: String,
    title: String,
    options: Vec<YtOption>,
    preview: Option<CancellationToken>,
}

thread_local! {
    static WIN: RefCell<Option<ClipDialog>> = const { RefCell::new(None) };
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn is_web(u: &str) -> bool {
    let u = u.trim();
    u.starts_with("https://") || u.starts_with("http://")
}

/// The name of the part's file: the video's title and the kind of file of
/// the choice (sound alone in a .webm is an Opus sound: .ogg).
pub(crate) fn part_name(title: &str, ext: &str, key: &str) -> String {
    let ext = if key == ytdlp::MP3_KEY {
        "mp3"
    } else if key.starts_with("audio|") && matches!(ext, "webm" | "opus") {
        "ogg"
    } else if key.starts_with("audio|") {
        "m4a"
    } else {
        "mp4"
    };
    let stem = filename::sanitize(title.trim());
    let stem = if stem.is_empty() { "video".to_string() } else { stem };
    format!("{stem}.{ext}")
}

/// A part sent by the browser add-in: straight to the list.
pub(crate) fn add_part(ui: &Ui, req: NewDownload) {
    let id = ui.ctx.mgr.add(req);
    show_progress_for_start(ui, id);
    refresh(ui);
}

/// Open the window (with `url` already looked up, when given).
pub(crate) fn open(ui: &Ui, url: Option<String>) {
    let made = WIN.with(|w| w.borrow().is_some());
    if !made {
        let Ok(d) = ClipDialog::new() else { return };
        wire(ui, &d);
        WIN.with(|w| *w.borrow_mut() = Some(d));
    }
    WIN.with(|w| {
        let w = w.borrow();
        let Some(d) = w.as_ref() else { return };
        crate::set_theme_of(d.global::<Theme>(), &crate::Look::of(&ui.ctx.mgr.settings()));
        let was_open = d.window().is_visible();
        match url.filter(|u| is_web(u)) {
            Some(u) => {
                d.set_url(u.into());
                d.invoke_fetch();
            }
            None if !was_open && d.get_url().is_empty() => {
                // an address in the clipboard is the likely one
                let clip = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
                if is_web(&clip) && !clip.contains(char::is_whitespace) {
                    d.set_url(clip.trim().into());
                }
            }
            None => {}
        }
        open_window!(d);
        d.window().set_minimized(false);
    });
}

fn with_win(f: impl FnOnce(&ClipDialog)) {
    WIN.with(|w| {
        if let Some(d) = w.borrow().as_ref() {
            f(d);
        }
    });
}

/// From / to as they are now, or what is wrong with them.
fn chosen(d: &ClipDialog) -> Result<Clip, String> {
    let (a, b) = (d.get_from() as f64, d.get_to() as f64);
    let dur = d.get_duration() as f64;
    if dur > 0.0 && a >= dur {
        return Err(t("The start is after the end of the video."));
    }
    if b <= a {
        return Err(t("The end must come after the start."));
    }
    let b = if dur > 0.0 { b.min(dur) } else { b };
    Clip::new(a, b, d.get_cut() == 1).ok_or_else(|| t("The part must be at least half a second long."))
}

fn show_length(d: &ClipDialog) {
    let len = (d.get_to() - d.get_from()).max(0.0) as f64;
    d.set_length_text(if len > 0.0 { clock(len, ':').into() } else { "—".into() });
}

fn wire(ui: &Ui, d: &ClipDialog) {
    let w = d.as_weak();
    d.on_close_window(move || {
        if let Some(d) = w.upgrade() {
            let _ = d.hide();
        }
        STATE.with(|s| {
            if let Some(c) = s.borrow_mut().preview.take() {
                c.cancel();
            }
        });
    });
    let w = d.as_weak();
    d.on_fetch(move || {
        let Some(d) = w.upgrade() else { return };
        let url = d.get_url().trim().to_string();
        if !is_web(&url) {
            d.set_message(t("Paste the address of the video (https://…).").into());
            return;
        }
        d.set_message(SharedString::new());
        d.set_busy(true);
        d.set_ready(false);
        d.set_video_title(SharedString::new());
        d.set_status(t("Looking at the video… (a few seconds)").into());
        let w2 = d.as_weak();
        std::thread::spawn(move || {
            let result = ytdlp::resolve_for(&url, ytdlp::remembered_cookies(&url).as_deref(), None);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(d) = w2.upgrade() else { return };
                d.set_busy(false);
                match result {
                    Ok(info) => {
                        let options: Vec<YtOption> = info.options.into_iter().filter(|o| is_web(&o.video_url)).collect();
                        if options.is_empty() {
                            d.set_status(t("No video was found at this address.").into());
                            return;
                        }
                        let labels: Vec<SharedString> = options
                            .iter()
                            .map(|o| if o.sub.is_empty() { o.label.clone().into() } else { format!("{} — {}", o.label, o.sub).into() })
                            .collect();
                        let dur = options.iter().map(|o| o.duration).max().unwrap_or(0) as f32;
                        d.set_qualities(ModelRc::new(VecModel::from(labels)));
                        d.set_quality(0);
                        d.set_duration(dur);
                        d.set_from(0.0);
                        let to = if dur > 0.0 { dur.min(30.0) } else { 0.0 };
                        d.set_to(to);
                        d.set_from_text(clock(0.0, ':').into());
                        d.set_to_text(if to > 0.0 { clock(to as f64, ':').into() } else { SharedString::new() });
                        show_length(&d);
                        d.set_video_title(info.title.clone().into());
                        d.set_status(if dur > 0.0 {
                            tf("Length of the video: {}", &[&clock(dur as f64, ':')]).into()
                        } else {
                            t("The length of this video is not known: type the times.").into()
                        });
                        d.set_ready(true);
                        STATE.with(|s| {
                            let mut s = s.borrow_mut();
                            s.page = url.clone();
                            s.title = info.title;
                            s.options = options;
                        });
                    }
                    Err(e) => d.set_status(tf("Could not get the video: {}", &[&crate::err_text(&format!("{e:#}"))]).into()),
                }
            });
        });
    });
    // the sliders: the times follow; the end stays after the start
    let w = d.as_weak();
    d.on_slid(move || {
        let Some(d) = w.upgrade() else { return };
        let dur = d.get_duration();
        if d.get_to() <= d.get_from() {
            d.set_to((d.get_from() + 1.0).min(dur.max(d.get_from() + 1.0)));
        }
        d.set_from_text(clock(d.get_from() as f64, ':').into());
        d.set_to_text(clock(d.get_to() as f64, ':').into());
        d.set_message(SharedString::new());
        show_length(&d);
    });
    let w = d.as_weak();
    d.on_from_typed(move |text| {
        let Some(d) = w.upgrade() else { return };
        if let Some(v) = parse_clock(&text) {
            let dur = d.get_duration();
            d.set_from(if dur > 0.0 { (v as f32).min(dur) } else { v as f32 });
            d.set_message(SharedString::new());
            show_length(&d);
        }
    });
    let w = d.as_weak();
    d.on_to_typed(move |text| {
        let Some(d) = w.upgrade() else { return };
        if let Some(v) = parse_clock(&text) {
            let dur = d.get_duration();
            d.set_to(if dur > 0.0 { (v as f32).min(dur) } else { v as f32 });
            d.set_message(SharedString::new());
            show_length(&d);
        }
    });
    // a look at the part: the part alone, quick, in a small quality
    let (w, mgr) = (d.as_weak(), ui.ctx.mgr.clone());
    d.on_preview(move || {
        let Some(d) = w.upgrade() else { return };
        let c = match chosen(&d) {
            Ok(c) => c,
            Err(m) => return d.set_message(m.into()),
        };
        let picked = d.get_quality().max(0) as usize;
        let Some((opt, sound_only)) = STATE.with(|s| {
            let s = s.borrow();
            let mine = s.options.get(picked)?.clone();
            if mine.key.starts_with("audio|") {
                return Some((mine, true));
            }
            // the smallest picture is enough to see the part (and quick)
            let small = s
                .options
                .iter()
                .filter(|o| !o.key.starts_with("audio|"))
                .min_by_key(|o| {
                    let h: u32 = o.quality.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(9999);
                    if h < 200 { 10_000 + h } else { h }
                })
                .cloned()
                .unwrap_or(mine);
            Some((small, false))
        }) else {
            return;
        };
        let c = Clip { end: c.end.min(c.start + PREVIEW_MAX), ..c };
        let mut headers = RequestHeaders::default();
        ytdlp::apply_headers(&mut headers, &opt.headers);
        let src = ClipSource { url: opt.video_url.clone(), audio_url: opt.audio_url.clone(), headers, hls: false };
        d.set_previewing(true);
        d.set_message(SharedString::new());
        let w2 = d.as_weak();
        let token = mgr.preview_part(src, c, sound_only, move |r| {
            let _ = slint::invoke_from_event_loop(move || {
                let Some(d) = w2.upgrade() else { return };
                d.set_previewing(false);
                match r {
                    Ok(file) => {
                        if open::that_detached(&file).is_err() {
                            d.set_message(t("No program on this computer plays this kind of file.").into());
                        }
                    }
                    Err(e) if e.contains("cancelled") || e.contains("stopped") => {}
                    Err(e) => d.set_message(tf("Could not prepare the preview: {}", &[&crate::err_text(&e)]).into()),
                }
            });
        });
        STATE.with(|s| {
            if let Some(old) = s.borrow_mut().preview.replace(token) {
                old.cancel();
            }
        });
    });
    let (w, u) = (d.as_weak(), ui.clone());
    d.on_download(move || {
        let Some(d) = w.upgrade() else { return };
        let c = match chosen(&d) {
            Ok(c) => c,
            Err(m) => return d.set_message(m.into()),
        };
        let picked = d.get_quality().max(0) as usize;
        let Some((opt, page, title)) = STATE.with(|s| {
            let s = s.borrow();
            Some((s.options.get(picked)?.clone(), s.page.clone(), s.title.clone()))
        }) else {
            return;
        };
        let mut headers = RequestHeaders::default();
        ytdlp::apply_headers(&mut headers, &opt.headers);
        let req = NewDownload {
            url: opt.video_url.clone(),
            file_name: Some(part_name(&title, &opt.ext, &opt.key)),
            headers,
            audio_url: opt.audio_url.clone(),
            source_page: Some(page),
            source_key: Some(opt.key.clone()),
            clip: Some(c),
            start: true,
            ..Default::default()
        };
        add_part(&u, req);
        d.set_message(SharedString::new());
        d.set_status(tf("The part {} was added to the list.", &[&c.label()]).into());
    });
}

/// The window is closed by the program (it quits).
#[allow(dead_code)]
pub(crate) fn close() {
    with_win(|d| {
        let _ = d.hide();
    });
}

#[cfg(test)]
mod tests {
    use super::part_name;

    #[test]
    fn names_of_parts() {
        assert!(part_name("My: video", "webm", "1080p|WEBM").ends_with(".mp4"));
        assert_eq!(part_name("Song", "m4a", "audio|M4A"), "Song.m4a");
        assert_eq!(part_name("Song", "webm", "audio|WEBM"), "Song.ogg");
        assert_eq!(part_name("Song", "m4a", swiftget_engine::ytdlp::MP3_KEY), "Song.mp3");
        assert!(part_name("  ", "mp4", "720p|MP4").ends_with(".mp4"));
    }
}
