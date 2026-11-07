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

/// The Win32 handle of a shown window (for the native menu).
#[cfg(windows)]
pub(crate) fn hwnd(w: &slint::Window) -> Option<isize> {
    hwnd_of(w)
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

/// Where every window is made, and where a window waits before it is shown
/// again: far from every screen. Windows puts a new window on the screen
/// before its first picture is drawn (white for a moment, in its own corner
/// on the left, then it jumped to the middle). Away from the screen nobody
/// sees that; `place` brings the window in once it is drawn.
#[cfg(windows)]
const AWAY: (i32, i32) = (-30000, -30000);

/// The window attributes hook of the backend (main): windows are made away.
#[cfg(windows)]
pub(crate) fn made_away(
    a: slint::winit_030::winit::window::WindowAttributes,
) -> slint::winit_030::winit::window::WindowAttributes {
    a.with_position(slint::winit_030::winit::dpi::PhysicalPosition::new(AWAY.0, AWAY.1))
}

/// Ctrl+C / V / X / A / Z / Y work with every keyboard language: with an
/// Arabic (Russian…) keyboard Windows says the key is "ؤ", not "c", and the
/// text boxes only know the Latin letters. The key's place on the keyboard
/// says which one it is.
#[cfg(windows)]
pub(crate) fn latin_shortcuts(w: &slint::Window) {
    use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
    use winit::event::{ElementState, WindowEvent};
    use winit::keyboard::{Key, KeyCode, PhysicalKey};
    let mut ctrl = false;
    let mut alt = false;
    w.on_winit_window_event(move |win, ev| {
        match ev {
            WindowEvent::ModifiersChanged(m) => {
                ctrl = m.state().control_key();
                alt = m.state().alt_key();
            }
            WindowEvent::KeyboardInput { event, .. } if ctrl && !alt => {
                let foreign = matches!(&event.logical_key, Key::Character(c) if !c.is_ascii());
                let latin = match event.physical_key {
                    PhysicalKey::Code(KeyCode::KeyC) => "c",
                    PhysicalKey::Code(KeyCode::KeyV) => "v",
                    PhysicalKey::Code(KeyCode::KeyX) => "x",
                    PhysicalKey::Code(KeyCode::KeyA) => "a",
                    PhysicalKey::Code(KeyCode::KeyZ) => "z",
                    PhysicalKey::Code(KeyCode::KeyY) => "y",
                    _ => "",
                };
                if foreign && !latin.is_empty() {
                    let text: slint::SharedString = latin.into();
                    let _ = win.dispatch_event_with_result(if event.state == ElementState::Pressed {
                        slint::platform::WindowEvent::KeyPressed { text }
                    } else {
                        slint::platform::WindowEvent::KeyReleased { text }
                    });
                    return EventResult::PreventDefault;
                }
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

#[cfg(not(windows))]
pub(crate) fn latin_shortcuts(_w: &slint::Window) {}

/// A hidden window that is shown again waits away from the screen too.
#[cfg(windows)]
pub(crate) fn send_away(w: &slint::Window) {
    latin_shortcuts(w);
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|win: &winit::window::Window| {
        if !win.is_maximized() && win.is_visible() != Some(true) {
            win.set_outer_position(winit::dpi::PhysicalPosition::new(AWAY.0, AWAY.1));
        }
    });
}

#[cfg(not(windows))]
pub(crate) fn send_away(_w: &slint::Window) {}

#[cfg(windows)]
fn has_system_window(w: &slint::Window) -> bool {
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|_: &winit::window::Window| ()).is_some()
}

#[cfg(not(windows))]
fn has_system_window(_w: &slint::Window) -> bool {
    true
}

#[cfg(windows)]
fn place_now(w: &slint::Window, maximizable: bool) {
    use slint::winit_030::{winit, WinitWindowAccessor};
    w.with_winit_window(|win: &winit::window::Window| place_winit(win, maximizable));
}

#[cfg(windows)]
fn place_winit(win: &slint::winit_030::winit::window::Window, maximizable: bool) {
    use slint::winit_030::winit;
    use winit::window::WindowButtons;
    if !win.is_decorated() {
        // a window drawn without the Windows frame: its buttons are not
        // touched (that gives it back a hidden Windows title bar)
    } else if maximizable {
        // the main window: its maximize button (full screen)
        win.set_resizable(true);
        win.set_enabled_buttons(WindowButtons::all());
    } else {
        win.set_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE);
        if win.is_maximized() {
            win.set_maximized(false);
        }
    }
    if win.is_maximized() {
        // it fills its screen already
        return;
    }
    // the screen of the mouse (where the user is working), else the main one
    let monitor = cursor_monitor(win).or_else(|| win.primary_monitor()).or_else(|| win.current_monitor());
    if let Some(m) = monitor {
        let (p, s, o) = (m.position(), m.size(), win.outer_size());
        let (x, y) = centered((p.x, p.y), (s.width, s.height), (o.width, o.height));
        win.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
    }
}

/// The screen the mouse pointer is on.
#[cfg(windows)]
fn cursor_monitor(win: &slint::winit_030::winit::window::Window) -> Option<slint::winit_030::winit::monitor::MonitorHandle> {
    let mut pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    // SAFETY: writes the pointer position into `pt`
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt) } == 0 {
        return None;
    }
    win.available_monitors().find(|m| {
        let (p, s) = (m.position(), m.size());
        pt.x >= p.x && pt.x < p.x + s.width as i32 && pt.y >= p.y && pt.y < p.y + s.height as i32
    })
}

#[cfg(not(windows))]
fn place_now(_w: &slint::Window, _maximizable: bool) {}

/// Time for the first picture of a window that was just put on its place
/// away from the screen.
const FIRST_PICTURE: std::time::Duration = std::time::Duration::from_millis(70);

/// A window was just opened (away from the screen): once it is drawn it
/// comes to the middle of the screen and, except for the main window,
/// loses its maximize button.
pub(crate) fn place<C>(weak: slint::Weak<C>, maximizable: bool)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    place_then(weak, maximizable, || {});
}

/// The same, and `then` once the window is on the screen.
pub(crate) fn place_then<C>(weak: slint::Weak<C>, maximizable: bool, then: impl FnOnce() + 'static)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    wait_for_picture(weak, maximizable, Box::new(then), 0, false);
}

fn wait_for_picture<C>(weak: slint::Weak<C>, maximizable: bool, then: Box<dyn FnOnce()>, tries: u32, seen: bool)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    // the system window is made a moment after show(): when it is there,
    // one more moment for its first picture
    let wait = if tries == 0 || seen { FIRST_PICTURE } else { std::time::Duration::from_millis(20) };
    slint::Timer::single_shot(wait, move || {
        let Some(c) = weak.upgrade() else { return };
        if !c.window().is_visible() {
            return;
        }
        let there = has_system_window(c.window());
        if there && (seen || tries == 0) {
            place_now(c.window(), maximizable);
            // on the screen now: drawn whole (Windows kept nothing of what
            // was drawn away from the screen; only changed parts were drawn
            // again and the rest stayed white)
            repaint_all(&c);
            c.window().request_redraw();
            then();
        } else if there {
            wait_for_picture(weak, maximizable, then, tries + 1, true);
        } else if tries < 250 {
            wait_for_picture(weak, maximizable, then, tries + 1, false);
        } else {
            then();
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

/// Draw all of `c` again at the next frame (not only what changed).
pub(crate) fn repaint_all<C>(c: &C)
where
    C: ComponentHandle,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    let t = c.global::<crate::Theme>();
    t.set_repaint(t.get_repaint().wrapping_add(1));
}

/// The windows are drawn by the processor, and only the parts that changed
/// are drawn again. When Windows throws away what a window showed (it was
/// hidden next to the clock, the screen slept, the size or the scale
/// changed...), the rest stayed black. So the whole window is drawn again
/// whenever that can have happened, and at most once a second whenever
/// Windows asks for a picture. `dropped`: files dropped on the window.
pub(crate) fn keep_drawn<C>(c: &C)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    repaint_all(c);
    hook_window(c, None);
}

/// The same, with files dropped on the window from Explorer.
pub(crate) fn keep_drawn_with_drop<C>(c: &C, dropped: impl Fn(std::path::PathBuf) + 'static)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    repaint_all(c);
    hook_window(c, Some(Box::new(dropped)));
}

/// `w.keep_drawn()` on any window (a reference to one too).
pub(crate) trait KeepDrawn {
    fn keep_drawn(&self);
}

impl<C> KeepDrawn for C
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    fn keep_drawn(&self) {
        keep_drawn(self);
    }
}

thread_local! {
    // the window (by its address) whose handler also takes dropped files:
    // it is not replaced by one without them
    static WITH_DROP: Cell<usize> = const { Cell::new(0) };
}

#[cfg(windows)]
fn hook_window<C>(c: &C, dropped: Option<Box<dyn Fn(std::path::PathBuf)>>)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
    use winit::event::WindowEvent;
    // one handler per window (a new one replaces the old): the one that
    // also takes dropped files stays
    let key = c.window() as *const slint::Window as usize;
    if dropped.is_some() {
        WITH_DROP.with(|w| w.set(key));
    } else if WITH_DROP.with(|w| w.get()) == key {
        return;
    }
    let weak = c.as_weak();
    let last = Cell::new(std::time::Instant::now());
    c.window().on_winit_window_event(move |_, ev| {
        let all = |weak: &slint::Weak<C>| {
            if let Some(c) = weak.upgrade() {
                repaint_all(&c);
            }
        };
        match ev {
            WindowEvent::DroppedFile(p) => {
                if let Some(f) = &dropped {
                    f(p.clone());
                }
            }
            // a window comes onto the screen from where it waited (see
            // `place`): Windows kept nothing of what was drawn there, and
            // only the parts that changed were drawn again, so it stayed
            // white around its buttons
            WindowEvent::Moved(_) if last.get().elapsed() > std::time::Duration::from_millis(150) => {
                last.set(std::time::Instant::now());
                all(&weak);
                if let Some(c) = weak.upgrade() {
                    c.window().request_redraw();
                }
            }
            WindowEvent::Resized(_)
            | WindowEvent::Focused(true)
            | WindowEvent::Occluded(false)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::ThemeChanged(_) => {
                last.set(std::time::Instant::now());
                all(&weak);
            }
            // Windows asks for a picture (or we did): the whole window now
            // and then, so nothing stays black for long
            WindowEvent::RedrawRequested if last.get().elapsed() > std::time::Duration::from_secs(1) => {
                last.set(std::time::Instant::now());
                all(&weak);
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

#[cfg(not(windows))]
fn hook_window<C>(c: &C, _dropped: Option<Box<dyn Fn(std::path::PathBuf)>>)
where
    C: ComponentHandle + 'static,
    for<'a> crate::Theme<'a>: slint::Global<'a, C>,
{
    let _ = c;
    let _ = &WITH_DROP;
}

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

/// The window of ours the user is working in (in front), if any.
#[cfg(windows)]
fn own_front_window() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain queries about the window in front; `pid` is a local
    let (hwnd, pid) = unsafe {
        let h = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        (h, pid)
    };
    (!hwnd.is_null() && pid == std::process::id()).then_some(hwnd as isize)
}

#[cfg(windows)]
struct OwnWindow(isize);

#[cfg(windows)]
impl raw_window_handle::HasWindowHandle for OwnWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{HandleError, RawWindowHandle, Win32WindowHandle, WindowHandle};
        let raw = Win32WindowHandle::new(std::num::NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?);
        // SAFETY: the handle is of a window of ours that is open now; the
        // message box using it is shown and closed before we go on
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(raw)) })
    }
}

#[cfg(windows)]
impl raw_window_handle::HasDisplayHandle for OwnWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::windows())
    }
}

/// A message box that belongs to the window in front: it opens on top of
/// it (never hidden behind, which looked like the program had frozen).
pub(crate) fn msg_box() -> rfd::MessageDialog {
    let d = rfd::MessageDialog::new();
    #[cfg(windows)]
    if let Some(h) = own_front_window() {
        return d.set_parent(&OwnWindow(h));
    }
    d
}

/// The same for file / folder pickers.
pub(crate) fn file_box() -> rfd::FileDialog {
    // the windows wait for the picker: no "does not answer" meanwhile (the
    // main timer clears this once the windows move again)
    crate::api::UI_WAITS.store(true, std::sync::atomic::Ordering::Relaxed);
    let d = rfd::FileDialog::new();
    #[cfg(windows)]
    if let Some(h) = own_front_window() {
        return d.set_parent(&OwnWindow(h));
    }
    d
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
