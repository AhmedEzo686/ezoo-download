//! Icon next to the clock (Windows): the app keeps running there when its
//! window is closed, like other download managers.

#[cfg_attr(not(windows), allow(dead_code))]
pub enum TrayAction {
    Show,
    AddUrl,
    ResumeAll,
    StopAll,
    ToggleLimiter,
    Scheduler,
    Options,
    About,
    Exit,
}

/// What was clicked on the small icon of a hidden download window.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DownloadAction {
    Show,
    Pause,
}

/// 32 x 32 RGBA picture for a hidden download: a blue "E" with a green
/// bar under it showing how far it is (`None`: size unknown, no bar).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn download_icon(progress: Option<f32>) -> Vec<u8> {
    const N: usize = 32;
    let mut px = vec![0u8; N * N * 4];
    let mut put = |x: usize, y: usize, c: [u8; 4]| {
        let i = (y * N + x) * 4;
        px[i..i + 4].copy_from_slice(&c);
    };
    let blue = [0x1f, 0x6f, 0xeb, 0xff];
    let white = [0xff, 0xff, 0xff, 0xff];
    for y in 0..N {
        for x in 0..N {
            // a square with round corners (radius 6)
            let (dx, dy) = (x.min(N - 1 - x), y.min(N - 1 - y));
            let corner = dx < 6 && dy < 6 && {
                let (cx, cy) = (6.0 - dx as f32 - 0.5, 6.0 - dy as f32 - 0.5);
                cx * cx + cy * cy > 36.0
            };
            if !corner {
                put(x, y, blue);
            }
        }
    }
    let rect = |put: &mut dyn FnMut(usize, usize, [u8; 4]), x0: usize, y0: usize, x1: usize, y1: usize, c: [u8; 4]| {
        for y in y0..y1 {
            for x in x0..x1 {
                put(x, y, c);
            }
        }
    };
    // the letter
    rect(&mut put, 9, 4, 13, 22, white);
    rect(&mut put, 9, 4, 23, 8, white);
    rect(&mut put, 9, 11, 21, 15, white);
    rect(&mut put, 9, 18, 23, 22, white);
    if let Some(p) = progress {
        let p = p.clamp(0.0, 1.0);
        rect(&mut put, 3, 24, 29, 30, [0x0b, 0x2a, 0x5c, 0xff]);
        let end = 4 + ((24.0 * p).round() as usize);
        rect(&mut put, 4, 25, end, 29, [0x3d, 0xdc, 0x5b, 0xff]);
    }
    px
}

#[cfg(windows)]
mod imp {
    use super::{DownloadAction, TrayAction};
    use std::collections::HashMap;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    const MAIN_ID: &str = "ezoo-main";
    const DL_PREFIX: &str = "ezoo-dl-";

    /// The icon of one hidden download window.
    struct Mini {
        icon: TrayIcon,
        show: MenuId,
        pause: MenuId,
        /// what it shows now, to change it only when needed
        step: Option<u32>,
        tip: String,
    }

    thread_local! {
        static HIDDEN: std::cell::RefCell<HashMap<u64, Mini>> = std::cell::RefCell::new(HashMap::new());
        static PENDING: std::cell::RefCell<Vec<(u64, DownloadAction)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn icon_for(progress: Option<f32>) -> Option<Icon> {
        Icon::from_rgba(super::download_icon(progress), 32, 32).ok()
    }

    /// A small icon next to the clock for a download whose window was hidden.
    pub fn show_download(id: u64, tip: &str, progress: Option<f32>, labels: (&str, &str)) -> bool {
        if HIDDEN.with(|h| h.borrow().contains_key(&id)) {
            return true;
        }
        let menu = Menu::new();
        let show = MenuItem::new(labels.0, true, None);
        let pause = MenuItem::new(labels.1, true, None);
        if menu.append(&show).is_err() || menu.append(&pause).is_err() {
            return false;
        }
        let Some(icon) = icon_for(progress) else { return false };
        let built = TrayIconBuilder::new()
            .with_id(format!("{DL_PREFIX}{id}"))
            .with_tooltip(super::short_tip(tip))
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build();
        let Ok(icon) = built else { return false };
        let mini = Mini {
            icon,
            show: show.id().clone(),
            pause: pause.id().clone(),
            step: progress.map(super::step_of),
            tip: tip.to_string(),
        };
        HIDDEN.with(|h| h.borrow_mut().insert(id, mini));
        true
    }

    /// New numbers for its icon and its hint.
    pub fn update_download(id: u64, tip: &str, progress: Option<f32>) {
        HIDDEN.with(|h| {
            if let Some(m) = h.borrow_mut().get_mut(&id) {
                let step = progress.map(super::step_of);
                if step != m.step {
                    m.step = step;
                    let _ = m.icon.set_icon(icon_for(progress));
                }
                if m.tip != tip {
                    m.tip = tip.to_string();
                    let _ = m.icon.set_tooltip(Some(super::short_tip(tip)));
                }
            }
        });
    }

    /// Its window is back (or it is done): the icon goes.
    pub fn remove_download(id: u64) {
        HIDDEN.with(|h| h.borrow_mut().remove(&id));
    }

    /// Clicks on the icons of hidden downloads since the last call.
    pub fn take_download_actions() -> Vec<(u64, DownloadAction)> {
        PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()))
    }

    pub struct Tray {
        _icon: TrayIcon,
        limiter: CheckMenuItem,
        ids: Vec<(MenuId, fn() -> TrayAction)>,
    }

    impl Tray {
        pub fn new() -> Option<Tray> {
            let menu = Menu::new();
            let show = MenuItem::new(crate::i18n::t("Restore Ezoo Download"), true, None);
            let add = MenuItem::new(crate::i18n::t("Add new download…"), true, None);
            let resume = MenuItem::new(crate::i18n::t("Resume all downloads"), true, None);
            let stop = MenuItem::new(crate::i18n::t("Stop all downloads"), true, None);
            let limiter = CheckMenuItem::new(crate::i18n::t("Speed Limiter"), true, false, None);
            let sched = MenuItem::new(crate::i18n::t("Scheduler…"), true, None);
            let options = MenuItem::new(crate::i18n::t("Options…"), true, None);
            let about = MenuItem::new(crate::i18n::t("About"), true, None);
            let exit = MenuItem::new(crate::i18n::t("Exit"), true, None);
            menu.append(&show).ok()?;
            menu.append(&PredefinedMenuItem::separator()).ok()?;
            menu.append(&add).ok()?;
            menu.append(&resume).ok()?;
            menu.append(&stop).ok()?;
            menu.append(&PredefinedMenuItem::separator()).ok()?;
            menu.append(&limiter).ok()?;
            menu.append(&sched).ok()?;
            menu.append(&options).ok()?;
            menu.append(&PredefinedMenuItem::separator()).ok()?;
            menu.append(&about).ok()?;
            menu.append(&exit).ok()?;
            let icon = Icon::from_rgba(include_bytes!("../ui/tray32.rgba").to_vec(), 32, 32).ok()?;
            let tray = TrayIconBuilder::new()
                .with_id(MAIN_ID)
                .with_tooltip("Ezoo Download")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                // the menu on a right click only; a double click opens the window
                .with_menu_on_left_click(false)
                .build()
                .ok()?;
            let ids: Vec<(MenuId, fn() -> TrayAction)> = vec![
                (show.id().clone(), || TrayAction::Show),
                (add.id().clone(), || TrayAction::AddUrl),
                (resume.id().clone(), || TrayAction::ResumeAll),
                (stop.id().clone(), || TrayAction::StopAll),
                (limiter.id().clone(), || TrayAction::ToggleLimiter),
                (sched.id().clone(), || TrayAction::Scheduler),
                (options.id().clone(), || TrayAction::Options),
                (about.id().clone(), || TrayAction::About),
                (exit.id().clone(), || TrayAction::Exit),
            ];
            Some(Tray { _icon: tray, limiter, ids })
        }

        /// Clicks since the last call (those on hidden downloads go to
        /// [`take_download_actions`]).
        pub fn poll(&self) -> Vec<TrayAction> {
            let mut out = Vec::new();
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                let id = ev.id().0.clone();
                if let Some(dl) = id.strip_prefix(DL_PREFIX).and_then(|n| n.parse::<u64>().ok()) {
                    // one click on a hidden download's icon: its window
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        PENDING.with(|p| p.borrow_mut().push((dl, DownloadAction::Show)));
                    }
                    continue;
                }
                // left double click: the window (the menu is on the right button)
                if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = ev {
                    out.push(TrayAction::Show);
                }
            }
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                if let Some((_, make)) = self.ids.iter().find(|(id, _)| *id == ev.id) {
                    out.push(make());
                    continue;
                }
                let hit = HIDDEN.with(|h| {
                    h.borrow().iter().find_map(|(dl, m)| {
                        if ev.id == m.show {
                            Some((*dl, DownloadAction::Show))
                        } else if ev.id == m.pause {
                            Some((*dl, DownloadAction::Pause))
                        } else {
                            None
                        }
                    })
                });
                if let Some(hit) = hit {
                    PENDING.with(|p| p.borrow_mut().push(hit));
                }
            }
            out
        }

        /// Keep the check mark of "Speed Limiter" in step with the app.
        pub fn set_limiter(&self, on: bool) {
            if self.limiter.is_checked() != on {
                self.limiter.set_checked(on);
            }
        }
    }
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
mod imp {
    use super::{DownloadAction, TrayAction};
    pub fn show_download(_id: u64, _tip: &str, _progress: Option<f32>, _labels: (&str, &str)) -> bool {
        false
    }
    pub fn update_download(_id: u64, _tip: &str, _progress: Option<f32>) {}
    pub fn remove_download(_id: u64) {}
    pub fn take_download_actions() -> Vec<(u64, DownloadAction)> {
        Vec::new()
    }
    pub struct Tray;
    impl Tray {
        pub fn new() -> Option<Tray> {
            None
        }
        pub fn poll(&self) -> Vec<TrayAction> {
            Vec::new()
        }
        pub fn set_limiter(&self, _on: bool) {}
    }
}

/// Linux: a StatusNotifierItem (org.kde.StatusNotifierItem over D-Bus), shown
/// next to the clock by KDE, GNOME (with the AppIndicator extension), Xfce,
/// Cinnamon, Budgie... `Tray::new` gives None when no such host is there, and
/// then the program never hides itself in the tray.
#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::{DownloadAction, TrayAction};
    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::CheckmarkItem;
    use ksni::{Icon, MenuItem, ToolTip};
    use std::collections::HashMap;
    use std::sync::Mutex;

    const DL_PREFIX: &str = "ezoo-dl-";

    static MAIN_ACTIONS: Mutex<Vec<TrayAction>> = Mutex::new(Vec::new());
    static DL_ACTIONS: Mutex<Vec<(u64, DownloadAction)>> = Mutex::new(Vec::new());

    fn push(action: TrayAction) {
        MAIN_ACTIONS.lock().unwrap_or_else(|p| p.into_inner()).push(action);
    }

    fn push_dl(id: u64, action: DownloadAction) {
        DL_ACTIONS.lock().unwrap_or_else(|p| p.into_inner()).push((id, action));
    }

    /// The picture the program keeps as RGBA; D-Bus wants ARGB32.
    fn argb(rgba: Vec<u8>, width: i32, height: i32) -> Icon {
        let mut data = rgba;
        for px in data.chunks_exact_mut(4) {
            px.rotate_right(1);
        }
        Icon { width, height, data }
    }

    fn item(label: String, action: fn()) -> MenuItem<MainTray> {
        MenuItem::Standard(ksni::menu::StandardItem {
            label,
            activate: Box::new(move |_| action()),
            ..Default::default()
        })
    }

    pub struct MainTray {
        limiter: bool,
    }

    impl ksni::Tray for MainTray {
        // a left click on the icon opens the window, the menu is on the right button
        const MENU_ON_ACTIVATE: bool = false;

        fn id(&self) -> String {
            "ezoo-download".into()
        }

        fn title(&self) -> String {
            "Ezoo Download".into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            vec![argb(include_bytes!("../ui/tray32.rgba").to_vec(), 32, 32)]
        }

        fn tool_tip(&self) -> ToolTip {
            ToolTip {
                title: "Ezoo Download".into(),
                icon_pixmap: self.icon_pixmap(),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            push(TrayAction::Show);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            use crate::i18n::t;
            vec![
                item(t("Restore Ezoo Download"), || push(TrayAction::Show)),
                MenuItem::Separator,
                item(t("Add new download…"), || push(TrayAction::AddUrl)),
                item(t("Resume all downloads"), || push(TrayAction::ResumeAll)),
                item(t("Stop all downloads"), || push(TrayAction::StopAll)),
                MenuItem::Separator,
                MenuItem::Checkmark(CheckmarkItem {
                    label: t("Speed Limiter"),
                    checked: self.limiter,
                    activate: Box::new(|t: &mut MainTray| {
                        // the window does the real switch; the check mark follows it
                        push(TrayAction::ToggleLimiter);
                        t.limiter = !t.limiter;
                    }),
                    ..Default::default()
                }),
                item(t("Scheduler…"), || push(TrayAction::Scheduler)),
                item(t("Options…"), || push(TrayAction::Options)),
                MenuItem::Separator,
                item(t("About"), || push(TrayAction::About)),
                item(t("Exit"), || push(TrayAction::Exit)),
            ]
        }
    }

    pub struct Tray {
        handle: Handle<MainTray>,
    }

    impl Tray {
        pub fn new() -> Option<Tray> {
            // no StatusNotifier host (or no D-Bus): Err right away, and the
            // program keeps its window instead of hiding in nothing
            let handle = MainTray { limiter: false }.spawn().ok()?;
            Some(Tray { handle })
        }

        pub fn poll(&self) -> Vec<TrayAction> {
            std::mem::take(&mut *MAIN_ACTIONS.lock().unwrap_or_else(|p| p.into_inner()))
        }

        pub fn set_limiter(&self, on: bool) {
            let _ = self.handle.update(|t| t.limiter = on);
        }
    }

    /// The icon of one hidden download window.
    struct MiniTray {
        id: u64,
        tip: String,
        progress: Option<f32>,
        show_label: String,
        pause_label: String,
    }

    impl ksni::Tray for MiniTray {
        const MENU_ON_ACTIVATE: bool = false;

        fn id(&self) -> String {
            format!("{DL_PREFIX}{}", self.id)
        }

        fn title(&self) -> String {
            self.tip.lines().next().unwrap_or_default().to_string()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            vec![argb(super::download_icon(self.progress), 32, 32)]
        }

        fn tool_tip(&self) -> ToolTip {
            ToolTip {
                title: self.tip.clone(),
                icon_pixmap: self.icon_pixmap(),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            push_dl(self.id, DownloadAction::Show);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let id = self.id;
            vec![
                MenuItem::Standard(ksni::menu::StandardItem {
                    label: self.show_label.clone(),
                    activate: Box::new(move |_| push_dl(id, DownloadAction::Show)),
                    ..Default::default()
                }),
                MenuItem::Standard(ksni::menu::StandardItem {
                    label: self.pause_label.clone(),
                    activate: Box::new(move |_| push_dl(id, DownloadAction::Pause)),
                    ..Default::default()
                }),
            ]
        }
    }

    thread_local! {
        static HIDDEN: std::cell::RefCell<HashMap<u64, Handle<MiniTray>>> = std::cell::RefCell::new(HashMap::new());
    }

    /// A small icon next to the clock for a download whose window was hidden.
    pub fn show_download(id: u64, tip: &str, progress: Option<f32>, labels: (&str, &str)) -> bool {
        if HIDDEN.with(|h| h.borrow().contains_key(&id)) {
            return true;
        }
        let mini = MiniTray {
            id,
            tip: tip.to_string(),
            progress,
            show_label: labels.0.to_string(),
            pause_label: labels.1.to_string(),
        };
        match mini.spawn() {
            Ok(handle) => {
                HIDDEN.with(|h| h.borrow_mut().insert(id, handle));
                true
            }
            Err(_) => false,
        }
    }

    /// New numbers for its icon and its hint.
    pub fn update_download(id: u64, tip: &str, progress: Option<f32>) {
        HIDDEN.with(|h| {
            if let Some(handle) = h.borrow().get(&id) {
                let tip = tip.to_string();
                let _ = handle.update(move |m| {
                    m.tip = tip;
                    m.progress = progress;
                });
            }
        });
    }

    /// Its window is back (or it is done): the icon goes.
    pub fn remove_download(id: u64) {
        let handle = HIDDEN.with(|h| h.borrow_mut().remove(&id));
        if let Some(handle) = handle {
            // the D-Bus service shuts itself down; never wait for it here
            std::thread::spawn(move || {
                handle.shutdown().wait();
            });
        }
    }

    /// Clicks on the icons of hidden downloads since the last call.
    pub fn take_download_actions() -> Vec<(u64, DownloadAction)> {
        std::mem::take(&mut *DL_ACTIONS.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

pub use imp::{remove_download, show_download, take_download_actions, update_download, Tray};

/// Windows shows at most 127 characters in an icon's hint.
#[cfg_attr(not(windows), allow(dead_code))]
fn short_tip(tip: &str) -> String {
    if tip.chars().count() <= 120 {
        tip.to_string()
    } else {
        tip.chars().take(119).chain(Some('…')).collect()
    }
}

/// The bar changes in steps of 1/24 (its width in dots).
#[cfg_attr(not(windows), allow(dead_code))]
fn step_of(p: f32) -> u32 {
    (p.clamp(0.0, 1.0) * 24.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_icon_shows_the_progress() {
        let empty = download_icon(Some(0.0));
        let half = download_icon(Some(0.5));
        assert_eq!(empty.len(), 32 * 32 * 4);
        let green = |px: &[u8], x: usize| px[(27 * 32 + x) * 4 + 1] == 0xdc;
        assert!(!green(&empty, 10) && green(&half, 10) && !green(&half, 26));
        // corners are see-through, the letter is white
        assert_eq!(half[3], 0);
        assert_eq!(&half[(5 * 32 + 10) * 4..(5 * 32 + 10) * 4 + 4], &[255, 255, 255, 255]);
        assert_eq!(short_tip(&"x".repeat(300)).chars().count(), 120);
        assert_eq!(step_of(0.5), 12);
    }
}
