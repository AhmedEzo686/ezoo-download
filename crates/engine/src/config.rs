//! User settings, stored as JSON in the app data folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 9614;
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AfterDone {
    #[default]
    Nothing,
    Exit,
    Sleep,
    Shutdown,
    Hibernate,
}

impl AfterDone {
    /// Order of every "when done" list: nothing, exit, sleep, shut down, hibernate.
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => AfterDone::Exit,
            2 => AfterDone::Sleep,
            3 => AfterDone::Shutdown,
            4 => AfterDone::Hibernate,
            _ => AfterDone::Nothing,
        }
    }
    pub fn index(self) -> i32 {
        match self {
            AfterDone::Nothing => 0,
            AfterDone::Exit => 1,
            AfterDone::Sleep => 2,
            AfterDone::Shutdown => 3,
            AfterDone::Hibernate => 4,
        }
    }
    /// Needs the countdown window (the computer goes off / to sleep).
    pub fn is_power(self) -> bool {
        matches!(self, AfterDone::Sleep | AfterDone::Shutdown | AfterDone::Hibernate)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    /// "HH:MM", 24h local time
    pub start: String,
    /// "HH:MM" or empty for no stop time
    pub stop: String,
    /// Monday .. Sunday
    pub days: [bool; 7],
    pub when_done: AfterDone,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            enabled: false,
            start: "02:00".into(),
            stop: String::new(),
            days: [true; 7],
            when_done: AfterDone::Nothing,
        }
    }
}

/// When a queue starts and stops by itself (the queue's "Schedule" tab).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSchedule {
    /// start the queue automatically
    pub start_enabled: bool,
    /// "HH:MM"
    pub start_time: String,
    /// false: daily on `days`; true: once on `once_date`
    pub once: bool,
    /// "YYYY-MM-DD"
    pub once_date: String,
    /// Monday .. Sunday
    pub days: [bool; 7],
    /// start the queue when Ezoo Download starts
    pub on_startup: bool,
    pub stop_enabled: bool,
    pub stop_time: String,
}

impl Default for QueueSchedule {
    fn default() -> Self {
        QueueSchedule {
            start_enabled: false,
            start_time: "02:00".into(),
            once: false,
            once_date: String::new(),
            days: [true; 7],
            on_startup: false,
            stop_enabled: false,
            stop_time: "08:00".into(),
        }
    }
}

/// A download queue: files added to it download one after another (or a few
/// at a time), optionally all into the same folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueDef {
    pub id: u64,
    pub name: String,
    /// files downloading at the same time in this queue (0 = no limit)
    pub max_parallel: u32,
    /// save every file of this queue here (None = category folders)
    pub folder: Option<PathBuf>,
    #[serde(default)]
    pub schedule: QueueSchedule,
    /// a failed file is tried again this many times
    #[serde(default)]
    pub retries: u32,
    /// when every file of the queue is done
    #[serde(default)]
    pub after_done: AfterDone,
    /// open each file when it finishes
    #[serde(default)]
    pub open_when_done: bool,
}

impl QueueDef {
    pub fn new(id: u64, name: &str, max_parallel: u32, folder: Option<PathBuf>) -> QueueDef {
        QueueDef {
            id,
            name: name.to_string(),
            max_parallel,
            folder,
            schedule: QueueSchedule::default(),
            retries: 0,
            after_done: AfterDone::Nothing,
            open_when_done: false,
        }
    }
}

/// A file category of the sidebar: which file types (and sites) go to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryDef {
    pub name: String,
    /// extensions without the dot
    pub exts: Vec<String>,
    /// files from these sites go here too ("*.example.com" allowed)
    #[serde(default)]
    pub sites: Vec<String>,
    /// shipped with the app (can't be deleted, only edited)
    #[serde(default)]
    pub builtin: bool,
}

pub fn default_categories() -> Vec<CategoryDef> {
    let c = |name: &str, exts: &str| CategoryDef {
        name: name.into(),
        exts: exts.split_whitespace().map(String::from).collect(),
        sites: Vec::new(),
        builtin: true,
    };
    vec![
        c("Compressed", "zip rar 7z tar gz tgz bz2 xz zst iso img cab arj lzh ace"),
        c("Documents", "pdf doc docx xls xlsx ppt pptx txt rtf odt ods odp epub csv md pps"),
        c("Music", "mp3 wav flac aac ogg m4a wma opus mid mka ra"),
        c("Programs", "exe msi msix appx apk xapk dmg deb rpm bat jar"),
        c("Video", "mp4 mkv avi mov wmv flv webm m4v ts 3gp mpg mpeg vob rm rmvb asf ogv"),
        c("Images", "jpg jpeg png gif webp bmp svg tif tiff heic ico psd"),
    ]
}

/// Extensions the browser extension hands to Ezoo Download (Options > File Types).
pub const DEFAULT_CAPTURE_EXTS: &str = "3GP 7Z AAC ACE AIF APK ARJ ASF AVI BIN BZ2 DEB DMG EXE GZ GZIP IMG ISO LZH M4A M4V MKA MKV MOV MP3 MP4 MPA MPE MPEG MPG MSI MSU OGG OGV PDF PLJ PPS PPT PPTX QT R0* R1* RA RAR RM RMVB RPM SEA SIT SITX TAR TIF TIFF TGZ WAV WEBM WMA WMV XAPK XZ Z ZIP ZST DOC DOCX XLS XLSX FLAC OPUS";

/// Server with its own connection count (Options > Connection > Exceptions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerLimit {
    pub host: String,
    pub connections: u32,
}

/// Login for a site (Options > Site Logins).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteLogin {
    /// "example.com" or "example.com/members"; "*" allowed
    pub site: String,
    pub user: String,
    /// encrypted on the disk (secret.rs)
    #[serde(with = "crate::secret::string")]
    pub password: String,
}

pub const MAIN_QUEUE: u64 = 1;

/// Bump when defaults change in a way old settings files should pick up.
const SETTINGS_VERSION: u32 = 4;

/// Columns of the download list that can be shown or hidden (View > Columns).
pub const ALL_COLUMNS: [&str; 11] =
    ["q", "size", "status", "eta", "speed", "last", "desc", "added", "saveto", "referer", "parent"];
/// Width a column starts with (pixels).
pub fn default_column_width(key: &str) -> u32 {
    match key {
        "name" => 280,
        "q" => 30,
        "size" => 82,
        "status" => 110,
        "eta" => 80,
        "speed" => 92,
        "last" => 140,
        "desc" => 170,
        "added" => 140,
        _ => 190,
    }
}

pub const DEFAULT_COLUMNS: [&str; 7] = ["q", "size", "status", "eta", "speed", "last", "desc"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// missing in old files -> 0, so they get migrated
    #[serde(default)]
    pub version: u32,
    pub download_dir: PathBuf,
    /// Save into Downloads\Video, Downloads\Music ...
    pub category_folders: bool,
    /// Connections per download
    pub connections: u32,
    /// Downloads running at the same time (0 = no limit)
    pub max_active: u32,
    /// KB/s, 0 = unlimited
    pub speed_limit_kb: u64,
    pub retries: u32,
    pub user_agent: String,
    pub clipboard_monitor: bool,
    pub show_complete_dialog: bool,
    pub browser_port: u16,
    /// A connection only splits a part when both halves are at least this big
    pub min_split_kb: u64,
    pub schedule: Schedule,
    /// progress window for each download
    pub show_progress_window: bool,
    pub queues: Vec<QueueDef>,
    /// 0 Light, 1 Dark, 2 Ocean, 3 Midnight, 4 Forest, 5 Black, 6 Sunset,
    /// 7 Teal night, 8 Rose
    pub theme: u32,
    /// how solid the main window is, 50..=100 (%)
    pub opacity: u32,
    /// size of all texts, 80..=150 (%)
    pub font_scale: u32,
    /// font of all windows ("" = Windows' own, Segoe UI)
    pub font_family: String,
    /// a picture behind the main window ("" = none, the theme's colors)
    pub background_image: String,
    /// how much of the picture shows through the panels, 10..=80 (%)
    pub background_strength: u32,
    /// version that ran last time: after an update "What's new" is shown once
    pub last_version: String,
    /// speed limiter switched on (the limit itself is speed_limit_kb)
    pub speed_limit_on: bool,
    /// "remember this path for this category" from the download dialog
    pub category_dirs: std::collections::BTreeMap<String, PathBuf>,
    pub hide_categories: bool,
    /// list sorting: "name", "size", "status", "date"; and direction
    pub sort_by: String,
    pub sort_desc: bool,
    /// closing the window keeps the app running next to the clock
    pub close_to_tray: bool,
    /// "start with Windows" was set up once (first run)
    pub autostart_set: bool,

    // ---- v4: everything of the classic Options window
    /// sidebar categories (General is implicit, always last)
    pub categories: Vec<CategoryDef>,
    /// start downloading links copied to the clipboard without asking
    pub auto_start_clipboard: bool,
    /// double click on a download: 0 open file / progress, 1 properties
    pub double_click: u32,
    /// speed limiter is switched on every time the app starts
    pub limiter_on_startup: bool,
    /// "Tip of the day" at startup
    pub show_tips: bool,
    pub tip_index: u32,
    /// browser: extensions handed to us, space separated ("R0*" style patterns)
    pub capture_exts: String,
    /// browser: never take downloads from these sites (space separated, * allowed)
    pub skip_sites: String,
    /// browser: never take these addresses (* allowed)
    pub skip_addresses: Vec<String>,
    /// browser: key that lets the browser download by itself ("alt", "ctrl", "shift", "none")
    pub skip_key: String,
    /// browser: key that forces Ezoo Download to take any link
    pub force_key: String,
    /// "Download this video" panel on web players
    pub panel_enabled: bool,
    pub panel_skip_sites: String,
    pub panel_min_video_kb: u64,
    pub panel_min_audio_kb: u64,
    /// .sgpart files go here while downloading (None = next to the file)
    pub temp_dir: Option<PathBuf>,
    /// "Download File Info" window for downloads from the browser / clipboard
    pub show_start_dialog: bool,
    /// progress window: 0 normal, 1 minimized, 2 don't show
    pub progress_mode: u32,
    /// address already in the list: 0 ask, 1 add with a number, 2 replace the old one, 3 show the old one
    pub duplicate_action: u32,
    /// set the file date to the server's "Last-Modified"
    pub set_file_date: bool,
    /// scan every finished file with this program ("[File]" = the file)
    pub virus_scan: bool,
    pub virus_program: String,
    pub virus_args: String,
    /// Options > Connection type: 0 low, 1 medium, 2 high speed
    pub connection_type: u32,
    pub server_limits: Vec<ServerLimit>,
    /// download no more than quota_mb every quota_hours
    pub quota_on: bool,
    pub quota_mb: u64,
    pub quota_hours: u32,
    pub quota_warn: bool,
    /// proxy: 0 none, 1 system settings, 2 manual
    pub proxy_mode: u32,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub proxy_user: String,
    /// encrypted on the disk (secret.rs)
    #[serde(with = "crate::secret::string")]
    pub proxy_password: String,
    /// don't use the proxy for these addresses (space separated)
    pub proxy_bypass: String,
    pub logins: Vec<SiteLogin>,
    /// sounds
    pub sounds_on: bool,
    pub sound_complete: String,
    pub sound_failed: String,
    pub sound_queue_start: String,
    pub sound_queue_done: String,
    /// visible list columns (see ALL_COLUMNS) and toolbar look
    pub columns: Vec<String>,
    /// order of the list's columns (dragged in the header), "name" included
    pub column_order: Vec<String>,
    /// width of each column in pixels (dragged in the header)
    pub column_widths: std::collections::BTreeMap<String, u32>,
    pub toolbar_small: bool,
    /// options-on-completion: close running programs when shutting down
    pub force_shutdown: bool,
    /// "Add Ezoo Download to your browser" was shown once
    pub extension_prompted: bool,
    /// start downloading at once while "Download File Info" is shown
    pub start_while_info: bool,
    /// "en", "ar", or "" = the language of Windows
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        let download_dir = directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        Settings {
            version: SETTINGS_VERSION,
            download_dir,
            category_folders: true,
            connections: 32,
            max_active: 0,
            speed_limit_kb: 0,
            retries: 10,
            user_agent: DEFAULT_USER_AGENT.into(),
            clipboard_monitor: true,
            show_complete_dialog: true,
            browser_port: DEFAULT_PORT,
            min_split_kb: 256,
            schedule: Schedule::default(),
            show_progress_window: true,
            theme: 0,
            opacity: 100,
            font_scale: 100,
            font_family: String::new(),
            background_image: String::new(),
            background_strength: 45,
            last_version: String::new(),
            speed_limit_on: false,
            category_dirs: Default::default(),
            hide_categories: false,
            sort_by: "date".into(),
            sort_desc: true,
            close_to_tray: true,
            autostart_set: false,
            queues: vec![QueueDef::new(MAIN_QUEUE, "Main download queue", 1, None)],
            categories: default_categories(),
            auto_start_clipboard: false,
            double_click: 0,
            limiter_on_startup: false,
            show_tips: true,
            tip_index: 0,
            capture_exts: DEFAULT_CAPTURE_EXTS.into(),
            skip_sites: String::new(),
            skip_addresses: Vec::new(),
            skip_key: "alt".into(),
            force_key: "none".into(),
            panel_enabled: true,
            panel_skip_sites: String::new(),
            panel_min_video_kb: 0,
            panel_min_audio_kb: 0,
            temp_dir: None,
            show_start_dialog: true,
            progress_mode: 0,
            duplicate_action: 0,
            set_file_date: false,
            virus_scan: false,
            virus_program: String::new(),
            virus_args: "[File]".into(),
            connection_type: 2,
            server_limits: Vec::new(),
            quota_on: false,
            quota_mb: 2000,
            quota_hours: 24,
            quota_warn: true,
            proxy_mode: 1,
            proxy_host: String::new(),
            proxy_port: 8080,
            proxy_user: String::new(),
            proxy_password: String::new(),
            proxy_bypass: "localhost 127.0.0.1".into(),
            logins: Vec::new(),
            sounds_on: false,
            sound_complete: default_sound("tada.wav"),
            sound_failed: default_sound("Windows Critical Stop.wav"),
            sound_queue_start: default_sound("Windows Notify System Generic.wav"),
            sound_queue_done: default_sound("chimes.wav"),
            columns: DEFAULT_COLUMNS.iter().map(|c| c.to_string()).collect(),
            column_order: Vec::new(),
            column_widths: Default::default(),
            toolbar_small: false,
            force_shutdown: false,
            extension_prompted: false,
            start_while_info: true,
            language: String::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let mut s: Settings = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if s.version < 2 {
            // v2: full speed by default (32 connections, no limit on downloads)
            s.connections = 32;
            s.max_active = 0;
            s.min_split_kb = 256;
            s.show_progress_window = true;
        }
        if s.version < 3 {
            // v3: the speed limit got an on/off switch
            s.speed_limit_on = s.speed_limit_kb > 0;
            if s.sort_by.is_empty() {
                s.sort_by = "date".into();
                s.sort_desc = true;
            }
        }
        if s.version < 4 {
            // v4: progress window has 3 modes; the old global scheduler
            // becomes the main queue's schedule
            s.progress_mode = if s.show_progress_window { 0 } else { 2 };
            let old = s.schedule.clone();
            if let Some(q) = s.queues.iter_mut().find(|q| q.id == MAIN_QUEUE) {
                if q.name == "Main queue" {
                    q.name = "Main download queue".into();
                }
                if old.enabled {
                    q.schedule.start_enabled = true;
                    q.schedule.start_time = old.start.clone();
                    q.schedule.days = old.days;
                    q.schedule.stop_enabled = !old.stop.trim().is_empty();
                    if q.schedule.stop_enabled {
                        q.schedule.stop_time = old.stop.clone();
                    }
                    q.after_done = old.when_done;
                }
            }
            s.schedule.enabled = false;
        }
        s.version = SETTINGS_VERSION;
        if !s.queues.iter().any(|q| q.id == MAIN_QUEUE) {
            s.queues.insert(0, QueueDef::new(MAIN_QUEUE, "Main download queue", 1, None));
        }
        if s.categories.is_empty() {
            s.categories = default_categories();
        }
        if s.columns.is_empty() {
            s.columns = DEFAULT_COLUMNS.iter().map(|c| c.to_string()).collect();
        }
        if s.limiter_on_startup {
            s.speed_limit_on = true;
        }
        s
    }

    /// Category of a file: by site first, then by extension; "General" otherwise.
    pub fn category_of(&self, file_name: &str, url: &str) -> String {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
            .unwrap_or_default();
        if !host.is_empty() {
            for c in &self.categories {
                if c.sites.iter().any(|s| host_matches(&host, s)) {
                    return c.name.clone();
                }
            }
        }
        let ext = crate::filename::extension(file_name);
        if !ext.is_empty() {
            for c in &self.categories {
                if c.exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                    return c.name.clone();
                }
            }
        }
        "General".into()
    }

    /// Names of every category, General last.
    pub fn category_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.categories.iter().map(|c| c.name.clone()).collect();
        v.push("General".into());
        v
    }

    /// Connections allowed for this address (server exceptions).
    pub fn connections_for(&self, url: &str, wanted: u32) -> u32 {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
            .unwrap_or_default();
        self.server_limits
            .iter()
            .find(|l| host_matches(&host, &l.host))
            .map(|l| wanted.min(l.connections.max(1)))
            .unwrap_or(wanted)
    }

    /// Saved login for this address (Options > Site Logins).
    pub fn login_for(&self, url: &str) -> Option<(String, String)> {
        let u = url::Url::parse(url).ok()?;
        let host = u.host_str()?.to_ascii_lowercase();
        let path = u.path().to_string();
        self.logins
            .iter()
            .filter(|l| !l.user.is_empty())
            .find(|l| {
                let site = l.site.trim().trim_start_matches("http://").trim_start_matches("https://");
                let (h, p) = site.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((site, String::new()));
                host_matches(&host, h) && (p.is_empty() || path.starts_with(&p))
            })
            .map(|l| (l.user.clone(), l.password.clone()))
    }

    pub fn column_visible(&self, key: &str) -> bool {
        self.columns.iter().any(|c| c == key)
    }

    /// The visible columns of the download list, in their order, with their
    /// widths: "name" always, the others as View > Columns says.
    pub fn column_layout(&self) -> Vec<(String, u32)> {
        let mut order: Vec<String> = self
            .column_order
            .iter()
            .filter(|k| k.as_str() == "name" || ALL_COLUMNS.contains(&k.as_str()))
            .cloned()
            .collect();
        order.dedup();
        for k in std::iter::once("name").chain(ALL_COLUMNS) {
            if !order.iter().any(|o| o == k) {
                order.push(k.to_string());
            }
        }
        order
            .into_iter()
            .filter(|k| k == "name" || self.column_visible(k))
            .map(|k| {
                let w = self.column_widths.get(&k).copied().unwrap_or_else(|| default_column_width(&k));
                (k, w.clamp(36, 2000))
            })
            .collect()
    }

    pub fn queue(&self, id: u64) -> Option<&QueueDef> {
        self.queues.iter().find(|q| q.id == id)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        crate::store::write_json_atomic(path, self)
    }

    /// bytes per second for the limiter (0 = full speed)
    pub fn effective_limit(&self) -> u64 {
        if self.speed_limit_on {
            self.speed_limit_kb * 1024
        } else {
            0
        }
    }

    pub fn connections(&self) -> u32 {
        self.connections.clamp(1, 32)
    }

    pub fn min_split(&self) -> u64 {
        self.min_split_kb.max(32) * 1024
    }

    /// Folder for a new download of the given category. "General"
    /// files go straight into the download folder.
    /// (A folder set in the category's properties wins.)
    pub fn dir_for(&self, category: &str) -> PathBuf {
        if let Some(p) = self.category_dirs.get(category).filter(|p| !p.as_os_str().is_empty()) {
            return p.clone();
        }
        if self.category_folders && category != "General" {
            self.download_dir.join(category)
        } else {
            self.download_dir.clone()
        }
    }

    /// Create the download folder and one sub-folder per category, so they
    /// are there in Windows from the start.
    pub fn create_folders(&self) {
        let _ = std::fs::create_dir_all(&self.download_dir);
        if self.category_folders {
            for c in &self.categories {
                if !self.category_dirs.contains_key(&c.name) {
                    let _ = std::fs::create_dir_all(self.download_dir.join(&c.name));
                }
            }
        }
    }
}

fn default_sound(name: &str) -> String {
    if cfg!(windows) {
        format!("C:\\Windows\\Media\\{name}")
    } else {
        String::new()
    }
}

/// "example.com" matches example.com and www.example.com; "*.x.com" and
/// "*" patterns work too.
pub fn host_matches(host: &str, pattern: &str) -> bool {
    let p = pattern.trim().trim_start_matches("http://").trim_start_matches("https://");
    let p = p.split('/').next().unwrap_or("").to_ascii_lowercase();
    if p.is_empty() {
        return false;
    }
    if p.contains('*') {
        return wildcard_match(&p, host);
    }
    host == p || host.ends_with(&format!(".{p}"))
}

/// `*` matches any run of characters, `?` one character (case-insensitive).
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Parse "HH:MM" to (hour, minute).
pub fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.example.com", "dl.example.com"));
        assert!(!wildcard_match("*.example.com", "example.org"));
        assert!(wildcard_match("r0*", "r01"));
        assert!(host_matches("www.site.com", "site.com"));
        assert!(!host_matches("othersite.com", "site.com"));
    }

    #[test]
    fn categories_and_logins() {
        let mut s = Settings::default();
        assert_eq!(s.category_of("a.ZIP", "https://example.com/get?id=1"), "Compressed");
        assert_eq!(s.category_of("noext", "https://example.com/"), "General");
        s.categories.push(CategoryDef {
            name: "Courses".into(),
            exts: vec![],
            sites: vec!["courses.example.com".into()],
            builtin: false,
        });
        assert_eq!(s.category_of("v.mp4", "https://www.courses.example.com/v.mp4"), "Courses");
        s.logins.push(SiteLogin { site: "example.com/members".into(), user: "u".into(), password: "p".into() });
        assert!(s.login_for("https://example.com/members/a.pdf").is_some());
        assert!(s.login_for("https://example.com/public/a.pdf").is_none());
        s.server_limits.push(ServerLimit { host: "slow.example.com".into(), connections: 2 });
        assert_eq!(s.connections_for("https://a.slow.example.com/x", 32), 2);
        assert_eq!(s.connections_for("https://fast.example.com/x", 32), 32);
    }
}

#[cfg(test)]
mod column_tests {
    use super::*;

    #[test]
    fn columns_keep_their_order_and_width() {
        let mut s = Settings::default();
        let keys = |s: &Settings| s.column_layout().into_iter().map(|c| c.0).collect::<Vec<_>>();
        // new settings: the name first, then the default columns
        assert_eq!(keys(&s)[0], "name");
        assert!(keys(&s).contains(&"size".to_string()));
        s.column_order = vec!["size".into(), "name".into(), "bogus".into()];
        s.column_widths.insert("size".into(), 5);
        s.column_widths.insert("name".into(), 400);
        let l = s.column_layout();
        assert_eq!(l[0], ("size".to_string(), 36)); // never narrower than 36
        assert_eq!(l[1], ("name".to_string(), 400));
        assert!(!keys(&s).contains(&"bogus".to_string()));
        // hidden in View > Columns: not shown, order kept for later
        s.columns.retain(|c| c != "size");
        assert_eq!(keys(&s)[0], "name");
    }
}
