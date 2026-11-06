//! The right-click menu of the download list as a real Windows menu: it
//! floats on its own (never cut by the edge of the program's window), in
//! the dark or light look of the program.

/// One line of the menu. `id` 0 = a separator line.
#[derive(Debug, Clone, Default)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Item {
    pub id: u32,
    pub text: String,
    pub enabled: bool,
    pub checked: bool,
    /// the main choice (shown in bold, as Windows does)
    pub bold: bool,
    /// a list of its own beside it
    pub sub: Vec<Item>,
}

impl Item {
    pub fn new(id: u32, text: impl Into<String>, enabled: bool) -> Item {
        Item { id, text: text.into(), enabled, ..Default::default() }
    }
    pub fn separator() -> Item {
        Item::default()
    }
}

/// Windows has a menu of its own: the Slint one is left for other systems.
pub(crate) fn available() -> bool {
    cfg!(windows)
}

/// Show `items` where the mouse is; gives the id clicked (None: closed).
#[cfg(windows)]
pub(crate) fn popup(win: &slint::Window, items: &[Item], dark: bool, rtl: bool) -> Option<u32> {
    use windows_sys::Win32::Foundation::{HWND, POINT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow, SetMenuDefaultItem, TrackPopupMenuEx,
        TPM_LAYOUTRTL, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTALIGN, TPM_RIGHTBUTTON, WM_NULL,
    };
    let hwnd = crate::winfx::hwnd(win)? as HWND;
    set_dark_menus(dark);
    // SAFETY: the menu is made, shown and destroyed here; hwnd is the live
    // main window (it owns the menu while it is open)
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        fill(menu, items);
        if let Some(main) = items.iter().find(|i| i.bold && i.id != 0) {
            SetMenuDefaultItem(menu, main.id, 0);
        }
        let mut at = POINT { x: 0, y: 0 };
        GetCursorPos(&mut at);
        // without this the menu stays open when the user clicks elsewhere
        SetForegroundWindow(hwnd);
        let side = if rtl { TPM_RIGHTALIGN | TPM_LAYOUTRTL } else { TPM_LEFTALIGN };
        let id = TrackPopupMenuEx(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | side, at.x, at.y, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        (id > 0).then_some(id as u32)
    }
}

#[cfg(not(windows))]
pub(crate) fn popup(_win: &slint::Window, _items: &[Item], _dark: bool, _rtl: bool) -> Option<u32> {
    None
}

#[cfg(windows)]
unsafe fn fill(menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU, items: &[Item]) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    };
    for it in items {
        if it.id == 0 && it.sub.is_empty() {
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            continue;
        }
        // "&" would underline the next letter: written as it is
        let text: Vec<u16> = it.text.replace('&', "&&").encode_utf16().chain(Some(0)).collect();
        let mut flags = MF_STRING;
        if !it.enabled {
            flags |= MF_GRAYED;
        }
        if it.checked {
            flags |= MF_CHECKED;
        }
        if it.sub.is_empty() {
            AppendMenuW(menu, flags, it.id as usize, text.as_ptr());
        } else {
            let sub = CreatePopupMenu();
            fill(sub, &it.sub);
            AppendMenuW(menu, flags | MF_POPUP, sub as usize, text.as_ptr());
        }
    }
}

/// The menus of the program dark (or light) like the program itself:
/// Windows 10 1903 and newer can do it (uxtheme's unnamed functions 135
/// and 136); older ones keep their usual light menu.
#[cfg(windows)]
fn set_dark_menus(dark: bool) {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
    type FlushMenuThemes = unsafe extern "system" fn();
    thread_local! {
        static LAST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    }
    if LAST.with(|l| l.get()) == Some(dark) {
        return;
    }
    LAST.with(|l| l.set(Some(dark)));
    let name: Vec<u16> = "uxtheme.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: uxtheme is a system library; the two functions are looked up
    // by number and only called when Windows has them
    unsafe {
        let lib = LoadLibraryW(name.as_ptr());
        if lib.is_null() {
            return;
        }
        let set = GetProcAddress(lib, 135usize as *const u8);
        let flush = GetProcAddress(lib, 136usize as *const u8);
        if let (Some(set), Some(flush)) = (set, flush) {
            let set: SetPreferredAppMode = std::mem::transmute(set);
            let flush: FlushMenuThemes = std::mem::transmute(flush);
            // 2 = always dark, 3 = always light
            set(if dark { 2 } else { 3 });
            flush();
        }
    }
}
