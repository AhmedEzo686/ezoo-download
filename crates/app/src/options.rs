//! The Options window (all its tabs).

use crate::open_window;
use crate::*;
use swiftget_engine::config::{ServerLimit, SiteLogin, DEFAULT_CAPTURE_EXTS};
use crate::i18n::{t, tf};

/// Lists edited inside the Options window before OK is pressed.
#[derive(Default)]
pub(crate) struct Lists {
    servers: Vec<ServerLimit>,
    logins: Vec<SiteLogin>,
}

const KEYS: [&str; 4] = ["alt", "ctrl", "shift", "none"];

fn key_index(k: &str) -> i32 {
    KEYS.iter().position(|x| *x == k).unwrap_or(3) as i32
}

pub(crate) fn open(ui: &Ui, tab: i32) {
    let Some(d) = ui.settings_dlg.upgrade() else { return };
    let s = ui.ctx.mgr.settings();
    {
        let mut l = ui.options_lists.borrow_mut();
        l.servers = s.server_limits.clone();
        l.logins = s.logins.clone();
    }
    fill(ui, &d, &s);
    d.set_tab(tab);
    d.set_message(SharedString::new());
    open_window!(d);
}

fn show_lists(ui: &Ui, d: &SettingsDialog) {
    let l = ui.options_lists.borrow();
    let servers: Vec<PairRow> = l
        .servers
        .iter()
        .map(|x| PairRow { a: x.host.clone().into(), b: x.connections.to_string().into(), c: SharedString::new() })
        .collect();
    d.set_server_rows(ModelRc::new(VecModel::from(servers)));
    let logins: Vec<PairRow> = l
        .logins
        .iter()
        .map(|x| PairRow { a: x.site.clone().into(), b: x.user.clone().into(), c: SharedString::new() })
        .collect();
    d.set_login_rows(ModelRc::new(VecModel::from(logins)));
}

fn fill(ui: &Ui, d: &SettingsDialog, s: &Settings) {
    // General
    d.set_language(crate::i18n::index_of(crate::i18n::effective(&s.language)) as i32);
    d.set_theme(s.theme as i32);
    d.set_window_opacity(s.opacity.clamp(50, 100) as f32);
    d.set_text_size(s.font_scale.clamp(80, 150) as f32);
    d.set_font_index(crate::FONTS.iter().position(|f| *f == s.font_family).unwrap_or(0) as i32);
    d.set_bg_image(s.background_image.clone().into());
    d.set_bg_strength(s.background_strength.clamp(10, 80) as f32);
    d.set_start_with_windows(autostart::is_enabled());
    d.set_close_to_tray(s.close_to_tray);
    d.set_clipboard(s.clipboard_monitor);
    d.set_auto_clipboard(s.auto_start_clipboard);
    d.set_double_click(s.double_click.min(1) as i32);
    d.set_show_tips(s.show_tips);
    d.set_port(s.browser_port as i32);
    d.set_skip_key(key_index(&s.skip_key));
    d.set_force_key(key_index(&s.force_key));
    // File types
    d.set_capture_exts(s.capture_exts.clone().into());
    d.set_skip_sites(s.skip_sites.clone().into());
    d.set_skip_addresses(s.skip_addresses.join("\n").into());
    d.set_panel_enabled(s.panel_enabled);
    d.set_panel_skip_sites(s.panel_skip_sites.clone().into());
    d.set_panel_min_video(s.panel_min_video_kb.min(100_000) as i32);
    d.set_panel_min_audio(s.panel_min_audio_kb.min(100_000) as i32);
    // Save to
    d.set_download_dir(s.download_dir.display().to_string().into());
    d.set_category_folders(s.category_folders);
    d.set_temp_dir(s.temp_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default().into());
    // Downloads
    d.set_show_start_dialog(s.show_start_dialog);
    d.set_start_while_info(s.start_while_info);
    d.set_complete_dialog(s.show_complete_dialog);
    d.set_progress_mode(s.progress_mode.min(2) as i32);
    d.set_duplicate_action(s.duplicate_action.min(3) as i32);
    d.set_set_file_date(s.set_file_date);
    d.set_virus_scan(s.virus_scan);
    d.set_virus_program(s.virus_program.clone().into());
    d.set_virus_args(s.virus_args.clone().into());
    d.set_user_agent(s.user_agent.clone().into());
    // Connection
    d.set_connection_type(s.connection_type.min(2) as i32);
    d.set_connections(s.connections() as i32);
    d.set_max_active(s.max_active.min(100) as i32);
    d.set_retries(s.retries.min(100) as i32);
    d.set_server_current(-1);
    d.set_server_host(SharedString::new());
    d.set_quota_on(s.quota_on);
    d.set_quota_mb(s.quota_mb.clamp(1, 10_000_000) as i32);
    d.set_quota_hours(s.quota_hours.clamp(1, 720) as i32);
    d.set_quota_used(
        if s.quota_on {
            tf("Downloaded in the current period: {}", &[&fmt_size(ui.ctx.mgr.quota_used())])
        } else {
            t("When the limit is reached the downloads stop, and go on by themselves later.")
        }
        .into(),
    );
    // Proxy
    d.set_proxy_mode(s.proxy_mode.min(2) as i32);
    d.set_proxy_host(s.proxy_host.clone().into());
    d.set_proxy_port(s.proxy_port.max(1) as i32);
    d.set_proxy_user(s.proxy_user.clone().into());
    d.set_proxy_password(s.proxy_password.clone().into());
    d.set_proxy_bypass(s.proxy_bypass.clone().into());
    // Logins
    d.set_login_current(-1);
    d.set_login_site(SharedString::new());
    d.set_login_user(SharedString::new());
    d.set_login_password(SharedString::new());
    // Sounds
    d.set_sounds_on(s.sounds_on);
    d.set_sound_complete(s.sound_complete.clone().into());
    d.set_sound_failed(s.sound_failed.clone().into());
    d.set_sound_qstart(s.sound_queue_start.clone().into());
    d.set_sound_qdone(s.sound_queue_done.clone().into());
    // Speed limiter
    d.set_limiter_on(s.speed_limit_on);
    d.set_speed_limit(s.speed_limit_kb.clamp(1, 1_000_000) as i32);
    d.set_limiter_startup(s.limiter_on_startup);
    show_lists(ui, d);
}

fn read(ui: &Ui, d: &SettingsDialog, mut s: Settings) -> Result<Settings, String> {
    let dir = d.get_download_dir().trim().to_string();
    if dir.is_empty() {
        return Err(t("Choose a download folder (Save To)"));
    }
    if d.get_proxy_mode() == 2 && d.get_proxy_host().trim().is_empty() {
        return Err(t("Type the proxy address (Proxy / Server)"));
    }
    // General
    s.language = crate::i18n::LANGS[(d.get_language().max(0) as usize).min(crate::i18n::LANGS.len() - 1)].0.into();
    s.theme = d.get_theme().clamp(0, 8) as u32;
    s.opacity = (d.get_window_opacity().round() as u32).clamp(50, 100);
    s.font_scale = (d.get_text_size().round() as u32).clamp(80, 150);
    s.font_family = crate::FONTS.get(d.get_font_index().max(0) as usize).copied().unwrap_or("").to_string();
    s.background_image = d.get_bg_image().to_string();
    s.background_strength = (d.get_bg_strength().round() as u32).clamp(10, 80);
    s.close_to_tray = d.get_close_to_tray();
    s.clipboard_monitor = d.get_clipboard();
    s.auto_start_clipboard = d.get_auto_clipboard();
    s.double_click = d.get_double_click().clamp(0, 1) as u32;
    s.show_tips = d.get_show_tips();
    s.browser_port = d.get_port().clamp(1024, 65535) as u16;
    s.skip_key = KEYS[d.get_skip_key().clamp(0, 3) as usize].into();
    s.force_key = KEYS[d.get_force_key().clamp(0, 3) as usize].into();
    if d.get_start_with_windows() != autostart::is_enabled() {
        autostart::set(d.get_start_with_windows());
    }
    // File types
    s.capture_exts = d.get_capture_exts().split_whitespace().collect::<Vec<_>>().join(" ");
    s.skip_sites = d.get_skip_sites().trim().to_string();
    s.skip_addresses = d
        .get_skip_addresses()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    s.panel_enabled = d.get_panel_enabled();
    s.panel_skip_sites = d.get_panel_skip_sites().trim().to_string();
    s.panel_min_video_kb = d.get_panel_min_video().max(0) as u64;
    s.panel_min_audio_kb = d.get_panel_min_audio().max(0) as u64;
    // Save to
    s.download_dir = PathBuf::from(dir);
    s.category_folders = d.get_category_folders();
    let temp = d.get_temp_dir().trim().to_string();
    s.temp_dir = (!temp.is_empty()).then(|| PathBuf::from(temp));
    // Downloads
    s.show_start_dialog = d.get_show_start_dialog();
    s.start_while_info = d.get_start_while_info();
    s.show_complete_dialog = d.get_complete_dialog();
    s.progress_mode = d.get_progress_mode().clamp(0, 2) as u32;
    s.show_progress_window = s.progress_mode != 2;
    s.duplicate_action = d.get_duplicate_action().clamp(0, 3) as u32;
    s.set_file_date = d.get_set_file_date();
    s.virus_scan = d.get_virus_scan();
    s.virus_program = d.get_virus_program().trim().to_string();
    s.virus_args = d.get_virus_args().trim().to_string();
    let ua = d.get_user_agent().trim().to_string();
    if !ua.is_empty() {
        s.user_agent = ua;
    }
    // Connection
    s.connection_type = d.get_connection_type().clamp(0, 2) as u32;
    s.connections = d.get_connections().clamp(1, 32) as u32;
    s.max_active = d.get_max_active().clamp(0, 100) as u32;
    s.retries = d.get_retries().max(0) as u32;
    s.quota_on = d.get_quota_on();
    s.quota_mb = d.get_quota_mb().max(1) as u64;
    s.quota_hours = d.get_quota_hours().max(1) as u32;
    // Proxy
    s.proxy_mode = d.get_proxy_mode().clamp(0, 2) as u32;
    s.proxy_host = d.get_proxy_host().trim().to_string();
    s.proxy_port = d.get_proxy_port().clamp(1, 65535) as u16;
    s.proxy_user = d.get_proxy_user().trim().to_string();
    s.proxy_password = d.get_proxy_password().to_string();
    s.proxy_bypass = d.get_proxy_bypass().trim().to_string();
    // Lists
    {
        let l = ui.options_lists.borrow();
        s.server_limits = l.servers.clone();
        s.logins = l.logins.clone();
    }
    // Sounds
    s.sounds_on = d.get_sounds_on();
    s.sound_complete = d.get_sound_complete().trim().to_string();
    s.sound_failed = d.get_sound_failed().trim().to_string();
    s.sound_queue_start = d.get_sound_qstart().trim().to_string();
    s.sound_queue_done = d.get_sound_qdone().trim().to_string();
    // Speed limiter
    s.speed_limit_on = d.get_limiter_on();
    s.speed_limit_kb = d.get_speed_limit().max(1) as u64;
    s.limiter_on_startup = d.get_limiter_startup();
    Ok(s)
}

/// The look the Options window shows now (not saved yet).
fn look_of(d: &SettingsDialog) -> crate::Look {
    crate::Look {
        mode: d.get_theme().clamp(0, 8),
        scale: (d.get_text_size().round() as u32).clamp(80, 150) as f32 / 100.0,
        family: crate::FONTS.get(d.get_font_index().max(0) as usize).copied().unwrap_or("").to_string(),
        background: d.get_bg_image().to_string(),
        strength: (d.get_bg_strength().round() as u32).clamp(10, 80) as f32 / 100.0,
    }
}

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.settings_dlg.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_preview_look(move || {
            if let Some(d) = ui.settings_dlg.upgrade() {
                crate::apply_look(&ui, &look_of(&d));
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_browse_bg(move || {
            let Some(d) = ui.settings_dlg.upgrade() else { return };
            let pick = rfd::FileDialog::new().add_filter(t("Pictures"), &["png", "jpg", "jpeg"]).pick_file();
            if let Some(f) = pick {
                d.set_bg_image(f.display().to_string().into());
                crate::apply_look(&ui, &look_of(&d));
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_preview_opacity(move |v| {
            if let Some(m) = ui.ctx.main.upgrade() {
                crate::winfx::set_opacity(m.window(), v.round() as u32);
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_browse(move |which| {
            let Some(d) = w.upgrade() else { return };
            match which.as_str() {
                "download" => {
                    let start = PathBuf::from(d.get_download_dir().as_str());
                    if let Some(dir) = rfd::FileDialog::new().set_directory(start).pick_folder() {
                        d.set_download_dir(dir.display().to_string().into());
                    }
                }
                "temp" => {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        d.set_temp_dir(dir.display().to_string().into());
                    }
                }
                "virus" => {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Program", &["exe", "com", "bat", "cmd"]).pick_file() {
                        d.set_virus_program(f.display().to_string().into());
                    }
                }
                s if s.starts_with("sound:") => {
                    let mut dlg = rfd::FileDialog::new().add_filter("Sound", &["wav"]);
                    if cfg!(windows) {
                        dlg = dlg.set_directory(r"C:\Windows\Media");
                    }
                    if let Some(f) = dlg.pick_file() {
                        let p: SharedString = f.display().to_string().into();
                        match &s[6..] {
                            "complete" => d.set_sound_complete(p),
                            "failed" => d.set_sound_failed(p),
                            "qstart" => d.set_sound_qstart(p),
                            _ => d.set_sound_qdone(p),
                        }
                    }
                }
                _ => {}
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_play(move |which| {
            let Some(d) = w.upgrade() else { return };
            let p = match which.as_str() {
                "complete" => d.get_sound_complete(),
                "failed" => d.get_sound_failed(),
                "qstart" => d.get_sound_qstart(),
                _ => d.get_sound_qdone(),
            };
            sound::play(&p);
        });
    }
    {
        let mgr = ui.ctx.mgr.clone();
        d.on_reset_paths(move || {
            let mut s = mgr.settings();
            s.category_dirs.clear();
            let _ = mgr.set_settings(s);
            message(&t("The remembered category folders were cleared."));
        });
    }
    {
        let w = d.as_weak();
        d.on_default_exts(move || {
            if let Some(d) = w.upgrade() {
                d.set_capture_exts(DEFAULT_CAPTURE_EXTS.into());
            }
        });
    }
    {
        let ui2 = ui.clone();
        d.on_server_add(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let host = d.get_server_host().trim().trim_start_matches("https://").trim_start_matches("http://").to_string();
            let host = host.split('/').next().unwrap_or("").to_string();
            if host.is_empty() {
                d.set_message(t("Type the server, e.g. example.com").into());
                return;
            }
            {
                let mut l = ui2.options_lists.borrow_mut();
                let conns = d.get_server_conns().clamp(1, 32) as u32;
                match l.servers.iter_mut().find(|s| s.host.eq_ignore_ascii_case(&host)) {
                    Some(s) => s.connections = conns,
                    None => l.servers.push(ServerLimit { host, connections: conns }),
                }
            }
            d.set_server_host(SharedString::new());
            d.set_message(SharedString::new());
            show_lists(&ui2, &d);
        });
    }
    {
        let ui2 = ui.clone();
        d.on_server_remove(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let i = d.get_server_current();
            {
                let mut l = ui2.options_lists.borrow_mut();
                if i >= 0 && (i as usize) < l.servers.len() {
                    l.servers.remove(i as usize);
                }
            }
            d.set_server_current(-1);
            show_lists(&ui2, &d);
        });
    }
    {
        let ui2 = ui.clone();
        d.on_login_pick(move |i| {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            if let Some(l) = ui2.options_lists.borrow().logins.get(i.max(0) as usize) {
                d.set_login_site(l.site.clone().into());
                d.set_login_user(l.user.clone().into());
                d.set_login_password(l.password.clone().into());
            }
        });
    }
    {
        let ui2 = ui.clone();
        d.on_login_add(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let site = d.get_login_site().trim().to_string();
            let user = d.get_login_user().trim().to_string();
            if site.is_empty() || user.is_empty() {
                d.set_message(t("Type the site and the user name").into());
                return;
            }
            let password = d.get_login_password().to_string();
            {
                let mut l = ui2.options_lists.borrow_mut();
                match l.logins.iter_mut().find(|x| x.site.eq_ignore_ascii_case(&site)) {
                    Some(x) => {
                        x.user = user;
                        x.password = password;
                    }
                    None => l.logins.push(SiteLogin { site, user, password }),
                }
            }
            d.set_login_site(SharedString::new());
            d.set_login_user(SharedString::new());
            d.set_login_password(SharedString::new());
            d.set_login_current(-1);
            d.set_message(SharedString::new());
            show_lists(&ui2, &d);
        });
    }
    {
        let ui2 = ui.clone();
        d.on_login_remove(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let i = d.get_login_current();
            {
                let mut l = ui2.options_lists.borrow_mut();
                if i >= 0 && (i as usize) < l.logins.len() {
                    l.logins.remove(i as usize);
                }
            }
            d.set_login_current(-1);
            show_lists(&ui2, &d);
        });
    }
    {
        let ui2 = ui.clone();
        d.on_open_extension_folder(move || browser::open(&ui2));
    }
    {
        let ui2 = ui.clone();
        d.on_save(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let old = ui2.ctx.mgr.settings();
            match read(&ui2, &d, old.clone()) {
                Ok(s) => {
                    let port_changed = s.browser_port != old.browser_port;
                    let theme = s.theme as i32;
                    let view = s.clone();
                    if let Err(e) = ui2.ctx.mgr.set_settings(s) {
                        d.set_message(tf("Could not save: {}", &[&format!("{e:#}")]).into());
                        return;
                    }
                    apply_theme(&ui2, theme);
                    apply_view_settings(&ui2, &view);
                    set_language(&ui2, crate::i18n::effective(&view.language));
                    let _ = d.hide();
                    refresh(&ui2);
                    if port_changed {
                        message(&t("Restart Ezoo Download to use the new port. Set the same port in the browser extension."));
                    }
                }
                Err(msg) => d.set_message(msg.into()),
            }
        });
    }
    {
        let w = d.as_weak();
        let ui = ui.clone();
        d.on_cancel(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
            // the sliders only showed it: back to the saved look
            if let Some(m) = ui.ctx.main.upgrade() {
                crate::winfx::set_opacity(m.window(), ui.ctx.mgr.settings().opacity);
            }
            crate::apply_look(&ui, &crate::Look::of(&ui.ctx.mgr.settings()));
        });
    }
}
