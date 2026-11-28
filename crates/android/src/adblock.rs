//! The browser's ad blocking: the filter lists people use everywhere
//! (EasyList, uBlock Origin's lists, Peter Lowe's, the Arabic list...), read
//! by Brave's engine (the `adblock` crate).
//!
//! * requests of the pages: blocked, or answered with a harmless stand-in
//!   ("redirect"), so the page doesn't see that something is missing
//! * what is left on the page: hidden (CSS), and the lists' page fixes
//!   (scriptlets) run before the page's own scripts
//!
//! The lists are fetched by the app and kept up to date (`update`); they are
//! never part of the app itself. Each list belongs to a group the user turns
//! on or off: ads, trackers, cookie notices, annoyances.

use adblock::lists::ParseOptions;
use adblock::request::Request;
use adblock::resources::{PermissionMask, Resource};
use adblock::{Engine, FilterSet};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

/// (group, file, address). Files named `ubo-...` are uBlock Origin's own
/// lists: they may use its trusted page fixes.
fn lists() -> Vec<(&'static str, String, String)> {
    const UBO: &str = "https://ublockorigin.github.io/uAssets/filters";
    let mut v: Vec<(&'static str, String, String)> = vec![
        ("ads", "easylist".into(), "https://easylist.to/easylist/easylist.txt".into()),
        ("ads", "ubo-filters".into(), format!("{UBO}/filters.txt")),
        ("ads", "ubo-general".into(), format!("{UBO}/filters-general.txt")),
        ("ads", "ubo-mobile".into(), format!("{UBO}/filters-mobile.txt")),
        ("ads", "ubo-badware".into(), format!("{UBO}/badware.txt")),
        ("ads", "ubo-abuse".into(), format!("{UBO}/resource-abuse.txt")),
        ("ads", "ubo-quick-fixes".into(), format!("{UBO}/quick-fixes.txt")),
        ("ads", "ubo-unbreak".into(), format!("{UBO}/unbreak.txt")),
        ("ads", "adguard-mobile".into(), "https://filters.adtidy.org/extension/ublock/filters/11.txt".into()),
        ("ads", "liste-ar".into(), "https://easylist-downloads.adblockplus.org/Liste_AR.txt".into()),
        (
            "ads",
            "peter-lowe".into(),
            "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblockplus&showintro=0&mimetype=plaintext".into(),
        ),
        ("privacy", "easyprivacy".into(), "https://easylist.to/easylist/easyprivacy.txt".into()),
        ("privacy", "ubo-privacy".into(), format!("{UBO}/privacy.txt")),
        ("cookies", "ubo-cookies".into(), format!("{UBO}/annoyances-cookies.txt")),
        ("cookies", "easylist-cookie".into(), "https://secure.fanboy.co.nz/fanboy-cookiemonster_ubo.txt".into()),
        ("annoyances", "ubo-annoyances".into(), format!("{UBO}/annoyances-others.txt")),
        ("annoyances", "fanboy-mobile".into(), "https://secure.fanboy.co.nz/fanboy-mobile-notifications.txt".into()),
    ];
    // uBlock Origin keeps a list for each year's new filters
    for y in 2020..=chrono_year() {
        v.push(("ads", format!("ubo-filters-{y}"), format!("{UBO}/filters-{y}.txt")));
    }
    v
}

fn chrono_year() -> u32 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    1970 + (secs / 31_556_952) as u32
}

struct Blocker {
    engine: Engine,
    /// the exceptions of the last pages (for their generic hiding)
    exceptions: Mutex<Vec<(String, HashSet<String>)>>,
}

static ENGINE: RwLock<Option<Blocker>> = RwLock::new(None);

fn list_dir(dir: &Path) -> PathBuf {
    dir.join("lists")
}

/// Fetch every list (all groups: turning one on needs no wait). A list
/// that can't be fetched keeps its last copy. Returns (fetched, failed).
pub async fn update(client: &swiftget_engine::reqwest::Client, dir: &Path) -> (usize, usize) {
    let out = list_dir(dir);
    let _ = std::fs::create_dir_all(&out);
    let mut ok = 0;
    let mut failed = 0;
    // all at once (a dozen small files)
    let mut set = tokio::task::JoinSet::new();
    for (group, name, url) in lists() {
        let file = out.join(format!("{group}--{name}.txt"));
        let client = client.clone();
        set.spawn(async move {
            let r = client.get(url.as_str()).timeout(Duration::from_secs(60)).send().await;
            let text = match r {
                Ok(r) if r.status().is_success() => r.text().await.ok(),
                _ => None,
            };
            match text {
                // a list, not an error page
                Some(t) if t.len() > 200 && !t.trim_start().starts_with('<') => {
                    let tmp = file.with_extension("part");
                    std::fs::write(&tmp, t).and_then(|_| std::fs::rename(&tmp, &file)).is_ok()
                }
                _ => false,
            }
        });
    }
    while let Some(r) = set.join_next().await {
        if r.unwrap_or(false) {
            ok += 1;
        } else {
            failed += 1;
        }
    }
    (ok, failed)
}

/// The time of the newest list (seconds since 1970), 0 when there are none.
pub fn lists_time(dir: &Path) -> u64 {
    std::fs::read_dir(list_dir(dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .max()
        .unwrap_or(0)
}

/// Build (or reload from its saved copy) the engine of the groups that are
/// on. Returns the number of lists read.
pub fn load(dir: &Path, groups: &[String]) -> usize {
    let mut files: Vec<PathBuf> = std::fs::read_dir(list_dir(dir))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            groups.iter().any(|g| n.starts_with(&format!("{g}--")))
        })
        .collect();
    files.sort();
    // the saved engine fits these very lists (names, times, sizes) and
    // this very engine version
    let mut key = String::from(env!("CARGO_PKG_VERSION"));
    for f in &files {
        if let Ok(m) = std::fs::metadata(f) {
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            key.push_str(&format!("|{}:{}:{}", f.display(), t, m.len()));
        }
    }
    let hash = {
        let d = ring_like_hash(key.as_bytes());
        format!("{d:016x}")
    };
    let saved = dir.join(format!("engine-{hash}.dat"));
    let mut engine = Engine::default();
    let mut from_saved = false;
    if let Ok(bytes) = std::fs::read(&saved) {
        from_saved = engine.deserialize(&bytes).is_ok();
    }
    if !from_saved {
        let mut set = FilterSet::new(false);
        for f in &files {
            let Ok(text) = std::fs::read_to_string(f) else { continue };
            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let trusted = name.contains("--ubo-");
            let opts = ParseOptions {
                permissions: if trusted { PermissionMask::from_bits(1) } else { PermissionMask::default() },
                ..Default::default()
            };
            set.add_filter_list(text, opts);
        }
        engine = Engine::new_with_filter_set(set);
        // older saved engines go; this one is kept
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("engine-") && n.ends_with(".dat") {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let _ = std::fs::write(&saved, engine.serialize());
    }
    // the stand-ins and page fixes (resources.json, made by the app)
    if let Ok(text) = std::fs::read_to_string(dir.join("resources.json")) {
        if let Ok(res) = serde_json::from_str::<Vec<Resource>>(&text) {
            engine.use_resources(res);
        }
    }
    *ENGINE.write().unwrap_or_else(|p| p.into_inner()) = Some(Blocker { engine, exceptions: Mutex::new(Vec::new()) });
    files.len()
}

/// FNV-1a: a short name for the saved engine.
fn ring_like_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn unload() {
    *ENGINE.write().unwrap_or_else(|p| p.into_inner()) = None;
}

/// A request of a page: "" (let it go), "B" (block it) or "R" + a data:
/// address (answer with this harmless stand-in instead).
pub fn check(url: &str, source: &str, kind: &str) -> String {
    let guard = ENGINE.read().unwrap_or_else(|p| p.into_inner());
    let Some(b) = guard.as_ref() else { return String::new() };
    let Ok(req) = Request::new(url, if source.is_empty() { url } else { source }, kind, "GET") else {
        return String::new();
    };
    let r = b.engine.check_network_request(&req);
    if !r.should_block() {
        return String::new();
    }
    match r.redirect {
        Some(redirect) => format!("R{redirect}"),
        None => "B".into(),
    }
}

/// What a page gets at its start: {css, script, generichide}.
pub fn cosmetic(url: &str) -> serde_json::Value {
    let guard = ENGINE.read().unwrap_or_else(|p| p.into_inner());
    let Some(b) = guard.as_ref() else { return serde_json::json!({}) };
    let r = b.engine.url_cosmetic_resources(url);
    {
        let mut ex = b.exceptions.lock().unwrap_or_else(|p| p.into_inner());
        ex.retain(|(u, _)| u != url);
        ex.push((url.to_string(), r.exceptions.clone()));
        if ex.len() > 30 {
            ex.remove(0);
        }
    }
    let css = hide_css(r.hide_selectors.iter().map(String::as_str));
    serde_json::json!({
        "css": css,
        "script": r.injected_script,
        "generichide": r.generichide,
    })
}

/// The generic hiding for the classes and ids found on a page: CSS.
pub fn generic(url: &str, classes: &[String], ids: &[String]) -> String {
    let guard = ENGINE.read().unwrap_or_else(|p| p.into_inner());
    let Some(b) = guard.as_ref() else { return String::new() };
    let ex = {
        let list = b.exceptions.lock().unwrap_or_else(|p| p.into_inner());
        list.iter().find(|(u, _)| u == url).map(|(_, e)| e.clone()).unwrap_or_default()
    };
    let sels = b.engine.hidden_class_id_selectors(classes, ids, &ex);
    hide_css(sels.iter().map(String::as_str))
}

/// Selectors into "a,b{display:none!important}", in groups (one bad
/// selector spoils only its own group).
fn hide_css<'a>(sels: impl Iterator<Item = &'a str>) -> String {
    let all: Vec<&str> = sels.collect();
    let mut out = String::new();
    for chunk in all.chunks(20) {
        out.push_str(&chunk.join(","));
        out.push_str("{display:none!important}\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EZOO_AB_TEST=<folder with lists/ and resources.json>
    #[test]
    fn real_lists() {
        let Some(dir) = std::env::var_os("EZOO_AB_TEST") else { return };
        let dir = PathBuf::from(dir);
        let t = std::time::Instant::now();
        let n = load(&dir, &["ads".into(), "privacy".into()]);
        eprintln!("lists {n} in {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let n = load(&dir, &["ads".into(), "privacy".into()]);
        eprintln!("again (saved) {n} in {:?}", t.elapsed());
        let gpt = check("https://securepubads.g.doubleclick.net/tag/js/gpt.js", "https://www.example.com/", "script");
        eprintln!("gpt: {}", &gpt[..gpt.len().min(80)]);
        assert!(!gpt.is_empty());
        assert_eq!(check("https://www.example.com/app.js", "https://www.example.com/", "script"), "");
        let c = cosmetic("https://www.youtube.com/watch?v=x");
        let script = c["script"].as_str().unwrap_or("");
        eprintln!("youtube script {} bytes, css {} bytes", script.len(), c["css"].as_str().unwrap_or("").len());
        assert!(script.len() > 100);
        let g = generic("https://www.youtube.com/watch?v=x", &["ad-banner".into(), "adsbygoogle".into()], &["ad_box".into()]);
        eprintln!("generic: {g}");
    }
}

