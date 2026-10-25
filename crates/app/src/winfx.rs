//! Things Slint has no setting for, done with Windows itself:
//!
//! * window opacity (Options > General > Window opacity)
//! * every window opens in the middle of the screen; only the main window
//!   can be maximized (the others are resized from their edges)
//! * the progress of a download on its window's taskbar button
//! * dragging a finished file out of the list onto the desktop / a folder

use slint::ComponentHandle;
use std::cell::Cell;

thread_local! {
    // (window handle, percent) applied last
    static APPLIED: Cell<(isize, u32)> = const { Cell::new((0, 100)) };
}

/// The Win32 handle of a shown window.
#[cfg(windows)]
fn hwnd_of(w: &slint::Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = w.window_handle();
    let wh = handle.window_handle().ok()?;
    match wh.as_raw() {
        RawWindowHandle::Win32(raw) => Some(raw.hwnd.get()),
        _ => None,
    }
}

/// Make `w` `pct` % solid (50..=100). Called again every second for the main
/// window: it does nothing when nothing changed.
pub(crate) fn set_opacity(w: &slint::Window, pct: u32) {
    let pct = pct.clamp(50, 100);
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
        };
        let Some(id) = hwnd_of(w) else { return };
        let hwnd = id as windows_sys::Win32::Foundation::HWND;
        // SAFETY: hwnd is the live window of `w`; only its extended style and
        // its alpha are changed
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let layered = ex & WS_EX_LAYERED as isize != 0;
            // the window may have got its style back from the toolkit
            if APPLIED.with(|a| a.get()) == (id, pct) && layered == (pct < 100) {
                return;
            }
            if pct >= 100 {
                if layered {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED as isize));
                }
            } else {
                if !layered {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
                }
                SetLayeredWindowAttributes(hwnd, 0, (pct * 255 / 100) as u8, LWA_ALPHA);
            }
        }
        APPLIED.with(|a| a.set((id, pct)));
    }
    #[cfg(not(windows))]
    {
        let _ = w;
        APPLIED.with(|a| a.set((0, pct)));
    }
}

/// Where a window of `size` goes to be in the middle of a screen at `screen`
/// (position) with `screen_size`; never above or left of the screen.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn centered(screen: (i32, i32), screen_size: (u32, u32), size: (u32, u32)) -> (i32, i32) {
    let x = screen.0 + (screen_size.0 as i32 - size.0 as i32).max(0) / 2;
    let y = screen.1 + (screen_size.1 as i32 - size.1 as i32).max(0) / 2;
    (x, y)
}

#[cfg(windows)]
fn place_now(w: &slint::Window, maximizable: bool) -> bool {
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|win: &winit::window::Window| place_winit(win, maximizable)).is_some()
}

#[cfg(windows)]
fn place_winit(win: &slint::winit_030::winit::window::Window, maximizable: bool) {
    use slint::winit_030::winit;
    use winit::window::WindowButtons;
    if maximizable {
        // the main window: its maximize button (full screen)
        win.set_resizable(true);
        win.set_enabled_buttons(WindowButtons::all());
    } else {
        win.set_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE);
        if win.is_maximized() {
            win.set_maximized(false);
        }
    }
    let monitor = win.current_monitor().or_else(|| win.primary_monitor());
    if let Some(m) = monitor {
        let (p, s, o) = (m.position(), m.size(), win.outer_size());
        let (x, y) = centered((p.x, p.y), (s.width, s.height), (o.width, o.height));
        win.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
    }
}

#[cfg(not(windows))]
fn place_now(_w: &slint::Window, _maximizable: bool) -> bool {
    true
}

/// A window was just opened: put it in the middle of the screen
/// and, except for the main window, take away its maximize button.
pub(crate) fn place<C: ComponentHandle + 'static>(weak: slint::Weak<C>, maximizable: bool) {
    if let Some(c) = weak.upgrade() {
        if place_now(c.window(), maximizable) {
            return;
        }
    }
    // the system window is made a moment after show()
    slint::Timer::single_shot(std::time::Duration::from_millis(30), move || {
        if let Some(c) = weak.upgrade() {
            place_now(c.window(), maximizable);
        }
    });
}

/// What the taskbar button of a window shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TaskbarProgress {
    Off,
    /// size unknown: a moving bar
    Busy,
    Normal(u64, u64),
    Paused(u64, u64),
    Error(u64, u64),
}

#[cfg(windows)]
mod taskbar {
    use std::ffi::c_void;
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};

    // ITaskbarList3 (shobjidl.h), in the order of its methods
    #[repr(C)]
    struct Vtbl {
        _query_interface: usize,
        _add_ref: usize,
        _release: usize,
        hr_init: unsafe extern "system" fn(*mut c_void) -> i32,
        _add_tab: usize,
        _delete_tab: usize,
        _activate_tab: usize,
        _set_active_alt: usize,
        _mark_fullscreen_window: usize,
        set_progress_value: unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> i32,
        set_progress_state: unsafe extern "system" fn(*mut c_void, HWND, i32) -> i32,
    }

    const CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);
    const IID_ITASKBAR_LIST3: GUID = GUID::from_u128(0xea1afb91_9e28_4b86_90e9_9e9f8a5eefaf);
    pub const NOPROGRESS: i32 = 0;
    pub const INDETERMINATE: i32 = 1;
    pub const NORMAL: i32 = 2;
    pub const ERROR: i32 = 4;
    pub const PAUSED: i32 = 8;

    thread_local! {
        // made once on the UI thread and kept for the life of the program
        static LIST: usize = unsafe {
            let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as _);
            let mut p: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(&CLSID_TASKBAR_LIST, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ITASKBAR_LIST3, &mut p);
            if hr < 0 || p.is_null() {
                0
            } else {
                let vt = *(p as *const *const Vtbl);
                ((*vt).hr_init)(p);
                p as usize
            }
        };
    }

    pub fn set(hwnd: isize, state: i32, value: Option<(u64, u64)>) {
        LIST.with(|&p| {
            if p == 0 {
                return;
            }
            let p = p as *mut c_void;
            // SAFETY: p is the ITaskbarList3 made above (never released);
            // hwnd is a live window of this program
            unsafe {
                let vt = *(p as *const *const Vtbl);
                ((*vt).set_progress_state)(p, hwnd as HWND, state);
                if let Some((done, total)) = value {
                    ((*vt).set_progress_value)(p, hwnd as HWND, done, total.max(1));
                }
            }
        });
    }
}

/// Show `p` on the taskbar button of `w` (the window may be minimized).
pub(crate) fn taskbar_progress(w: &slint::Window, p: TaskbarProgress) {
    #[cfg(windows)]
    {
        use taskbar::*;
        let Some(hwnd) = hwnd_of(w) else { return };
        match p {
            TaskbarProgress::Off => set(hwnd, NOPROGRESS, None),
            TaskbarProgress::Busy => set(hwnd, INDETERMINATE, None),
            TaskbarProgress::Normal(d, t) => set(hwnd, NORMAL, Some((d, t))),
            TaskbarProgress::Paused(d, t) => set(hwnd, PAUSED, Some((d, t))),
            TaskbarProgress::Error(d, t) => set(hwnd, ERROR, Some((d, t))),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (w, p);
    }
}

/// Drag a finished file out of the program: the user drops it on the
/// desktop, a folder, a chat... (a copy; the file stays where it is, so the
/// download list stays right). Returns when the mouse is let go.
pub(crate) fn drag_file_out(w: &slint::Window, path: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::core::GUID;
        use windows_sys::Win32::System::Ole::{OleInitialize, DROPEFFECT_COPY, DROPEFFECT_LINK};
        use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
        use windows_sys::Win32::UI::Shell::{
            ILClone, ILCreateFromPathW, ILFindLastID, ILFree, ILRemoveLastID, SHCreateDataObject, SHDoDragDrop,
        };
        const IID_IDATA_OBJECT: GUID = GUID::from_u128(0x0000010e_0000_0000_c000_000000000046);

        let Some(hwnd) = hwnd_of(w) else { return };
        if !path.is_file() {
            return;
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is a NUL-terminated path that outlives the calls; the
        // item list and the data object are freed / released exactly once
        unsafe {
            let _ = OleInitialize(std::ptr::null_mut());
            let pidl = ILCreateFromPathW(wide.as_ptr());
            if pidl.is_null() {
                return;
            }
            // the folder + the file's own item inside it: what Explorer itself
            // hands out (a full path item alone gives an object nobody accepts)
            let parent = ILClone(pidl);
            if parent.is_null() {
                ILFree(pidl);
                return;
            }
            ILRemoveLastID(parent);
            let child = ILFindLastID(pidl) as *const ITEMIDLIST;
            let items: [*const ITEMIDLIST; 1] = [child];
            let mut obj: *mut c_void = std::ptr::null_mut();
            let hr = SHCreateDataObject(parent, 1, items.as_ptr(), std::ptr::null_mut(), &IID_IDATA_OBJECT, &mut obj);
            if hr >= 0 && !obj.is_null() {
                let mut effect = 0;
                // no drop source of our own: the shell's standard one; a copy
                // (or a shortcut), never a move, so the list stays right
                let _ = SHDoDragDrop(hwnd as _, obj as _, std::ptr::null_mut(), DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect);
                // IUnknown::Release is the third entry of every COM object's table
                let vt = *(obj as *const *const [usize; 3]);
                let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute((*vt)[2]);
                release(obj);
            }
            ILFree(parent);
            ILFree(pidl);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (w, path);
    }
}

/// Windows drawn without the Windows frame (their own title bar): move the
/// window while the mouse button is held on the title.
#[cfg(windows)]
pub(crate) fn drag_window(w: &slint::Window) {
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|win: &winit::window::Window| {
        let _ = win.drag_window();
    });
}

/// ...and make it bigger / smaller from its bottom right corner.
#[cfg(windows)]
pub(crate) fn drag_resize_window(w: &slint::Window) {
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|win: &winit::window::Window| {
        let _ = win.drag_resize_window(winit::window::ResizeDirection::SouthEast);
    });
}

#[cfg(not(windows))]
pub(crate) fn drag_window(_w: &slint::Window) {}

#[cfg(not(windows))]
pub(crate) fn drag_resize_window(_w: &slint::Window) {}

/// A file / folder picker that belongs to `w`: it stays on top of it and
/// `w` waits for it.
#[cfg(windows)]
pub(crate) fn on_top_of(dlg: rfd::FileDialog, w: &slint::Window) -> rfd::FileDialog {
    dlg.set_parent(&w.window_handle())
}

#[cfg(not(windows))]
pub(crate) fn on_top_of(dlg: rfd::FileDialog, _w: &slint::Window) -> rfd::FileDialog {
    dlg
}

#[cfg(test)]
mod tests {
    use super::centered;

    #[test]
    fn windows_open_in_the_middle() {
        assert_eq!(centered((0, 0), (1920, 1080), (800, 600)), (560, 240));
        // a second screen to the right
        assert_eq!(centered((1920, 0), (1280, 1024), (640, 480)), (2240, 272));
        // bigger than the screen: its top left corner stays on the screen
        assert_eq!(centered((0, 0), (1366, 768), (1600, 900)), (0, 0));
    }
}
