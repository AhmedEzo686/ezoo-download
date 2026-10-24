//! Window opacity (Options > General > Window opacity, View > Theme):
//! Windows makes the whole main window see-through with a "layered" style.

use std::cell::Cell;

thread_local! {
    // (window handle, percent) applied last, so the timer does nothing most of the time
    static APPLIED: Cell<(isize, u32)> = const { Cell::new((0, 100)) };
}

/// Make `w` `pct` % solid (50..=100). Call it again when the window was
/// shown again; it does nothing when nothing changed.
pub(crate) fn set_opacity(w: &slint::Window, pct: u32) {
    let pct = pct.clamp(50, 100);
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
        };
        let handle = w.window_handle();
        let Ok(wh) = handle.window_handle() else { return };
        let RawWindowHandle::Win32(raw) = wh.as_raw() else { return };
        let id = raw.hwnd.get();
        if APPLIED.with(|a| a.get()) == (id, pct) {
            return;
        }
        let hwnd = id as windows_sys::Win32::Foundation::HWND;
        // SAFETY: hwnd is the live window of `w`; only its extended style and
        // its alpha are changed
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if pct >= 100 {
                if ex & WS_EX_LAYERED as isize != 0 {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED as isize));
                }
            } else {
                if ex & WS_EX_LAYERED as isize == 0 {
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
