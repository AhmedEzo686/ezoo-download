//! Event-driven platform appearance. AppKit supplies semantic colors and the
//! system font on macOS; other platforms keep the same accessible role palette.
use slint::Color;
use std::cell::Cell;

pub(crate) fn index(value: &str) -> i32 {
    match value { "light" => 1, "dark" => 2, _ => 0 }
}

pub(crate) fn preference(index: i32) -> &'static str {
    match index { 1 => "light", 2 => "dark", _ => "system" }
}

thread_local! { static SYSTEM_DARK: Cell<bool> = const { Cell::new(false) }; }

#[derive(Clone)]
pub(crate) struct Palette {
    pub dark: bool,
    pub window: Color,
    pub panel: Color,
    pub secondary_surface: Color,
    pub sidebar: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub line: Color,
    pub selection: Color,
    pub hover: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub input: Color,
    pub bar: Color,
    pub font_family: String,
}

fn rgb(value: u32) -> Color { Color::from_rgb_u8((value >> 16) as u8, (value >> 8) as u8, value as u8) }

fn mix(foreground: Color, background: Color, amount: f64) -> Color {
    let channel = |a: u8, b: u8| (a as f64 * amount + b as f64 * (1.0 - amount)).round() as u8;
    Color::from_rgb_u8(channel(foreground.red(), background.red()), channel(foreground.green(), background.green()), channel(foreground.blue(), background.blue()))
}

fn luminance(color: Color) -> f64 {
    let linear = |c: u8| { let c = c as f64 / 255.0; if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } };
    0.2126 * linear(color.red()) + 0.7152 * linear(color.green()) + 0.0722 * linear(color.blue())
}

fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Native secondary labels and status colors can be translucent or intended
/// for larger symbols. Keep small custom text readable on our opaque surface.
fn readable(color: Color, background: Color) -> Color {
    if contrast(color, background) >= 4.5 { return color; }
    let target = if luminance(background) > 0.45 { rgb(0x000000) } else { rgb(0xffffff) };
    (1..=100).map(|step| mix(target, color, step as f64 / 100.0))
        .find(|candidate| contrast(*candidate, background) >= 4.5).unwrap_or(target)
}

impl Palette {
    fn ensure_readable(&mut self) {
        let surfaces = [self.panel, self.window, self.sidebar, self.secondary_surface, self.selection, self.hover];
        let target = if self.dark { rgb(0xffffff) } else { rgb(0x000000) };
        for color in [&mut self.text, &mut self.muted, &mut self.accent_text, &mut self.ok, &mut self.warn, &mut self.err] {
            let original = *color;
            *color = (0..=100).map(|step| mix(target, original, step as f64 / 100.0))
                .find(|candidate| surfaces.iter().all(|surface| contrast(*candidate, *surface) >= 4.5)).unwrap_or(target);
        }
    }
}

fn fallback(dark: bool) -> Palette {
    let window = rgb(if dark { 0x1c1c1e } else { 0xf5f5f7 });
    let panel = rgb(if dark { 0x2c2c2e } else { 0xffffff });
    let text = rgb(if dark { 0xf5f5f7 } else { 0x1d1d1f });
    let accent = rgb(if dark { 0x0a84ff } else { 0x007aff });
    let mut palette = Palette {
        dark, window, panel, sidebar: window,
        secondary_surface: mix(text, panel, if dark { 0.045 } else { 0.025 }),
        text, muted: rgb(if dark { 0xa1a1a6 } else { 0x5a5a60 }),
        accent, accent_text: readable(accent, panel),
        line: rgb(if dark { 0x48484a } else { 0xd2d2d7 }),
        selection: mix(accent, panel, if dark { 0.24 } else { 0.13 }),
        hover: mix(text, panel, 0.055),
        ok: readable(rgb(if dark { 0x30d158 } else { 0x248a3d }), panel),
        warn: readable(rgb(if dark { 0xff9f0a } else { 0xb25000 }), panel),
        err: readable(rgb(if dark { 0xff453a } else { 0xd70015 }), panel),
        input: panel, bar: mix(text, panel, 0.12), font_family: String::new(),
    };
    palette.ensure_readable();
    palette
}

pub(crate) fn palette(mode: i32) -> Palette {
    #[cfg(target_os = "macos")]
    if let Some(palette) = mac::palette(mode) { return palette; }
    let dark = match mode { 1 => false, 2 => true, _ => SYSTEM_DARK.with(Cell::get) };
    fallback(dark)
}

/// Winit delivers theme changes without a polling timer. Deferring our update
/// lets AppKit/Slint finish applying the native event before colors are read.
pub(crate) fn watch_window(window: &slint::Window, changed: fn()) {
    use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
    if let Some(Some(theme)) = window.with_winit_window(|window| window.theme()) {
        SYSTEM_DARK.with(|dark| dark.set(theme == winit::window::Theme::Dark));
    }
    window.on_winit_window_event(move |window, event| {
        if let winit::event::WindowEvent::ThemeChanged(theme) = event {
            SYSTEM_DARK.with(|dark| dark.set(*theme == winit::window::Theme::Dark));
            let _ = slint::invoke_from_event_loop(changed);
        } else if matches!(event, winit::event::WindowEvent::Focused(true)) {
            // The native window may not exist yet when the listener is first
            // attached. Its first focus supplies the initial non-Mac theme.
            if let Some(Some(theme)) = window.with_winit_window(|window| window.theme()) {
                let value = theme == winit::window::Theme::Dark;
                if SYSTEM_DARK.with(|dark| dark.replace(value)) != value {
                    let _ = slint::invoke_from_event_loop(changed);
                }
            }
        }
        EventResult::Propagate
    });
}

#[cfg(target_os = "macos")]
pub(crate) use mac::Observer;
#[cfg(not(target_os = "macos"))]
pub(crate) struct Observer;

pub(crate) fn watch_colors(changed: fn()) -> Option<Observer> {
    #[cfg(target_os = "macos")]
    { mac::watch(changed) }
    #[cfg(not(target_os = "macos"))]
    { let _ = changed; None }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use block2::RcBlock;
    use objc2::{rc::Retained, runtime::{NSObjectProtocol, ProtocolObject}, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace, NSFont, NSSystemColorsDidChangeNotification};
    use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSOperationQueue};
    use std::{cell::RefCell, ptr::NonNull};

    thread_local! { static APPLIED: Cell<i32> = const { Cell::new(-1) }; }

    fn color(value: Retained<NSColor>, background: Color) -> Color {
        let Some(value) = value.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else { return background };
        let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let opaque = Color::from_rgb_u8(channel(value.redComponent()), channel(value.greenComponent()), channel(value.blueComponent()));
        mix(opaque, background, value.alphaComponent().clamp(0.0, 1.0))
    }

    pub(super) fn palette(mode: i32) -> Option<Palette> {
        let main = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(main);
        // These constants are immutable AppKit-provided appearance names.
        let (aqua, dark_aqua) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        if APPLIED.with(|previous| previous.replace(mode)) != mode {
            let explicit = match mode { 1 => NSAppearance::appearanceNamed(aqua), 2 => NSAppearance::appearanceNamed(dark_aqua), _ => None };
            // App-only override. No system preference is written.
            app.setAppearance(explicit.as_deref());
        }
        let appearance = app.effectiveAppearance();
        let choices = NSArray::from_slice(&[dark_aqua, aqua]);
        let dark = appearance.bestMatchFromAppearancesWithNames(&choices).is_some_and(|name| &*name == dark_aqua);
        let result = RefCell::new(fallback(dark));
        let block = RcBlock::new(|| {
            let mut palette = result.borrow_mut();
            palette.window = color(NSColor::windowBackgroundColor(), palette.window);
            palette.panel = color(NSColor::controlBackgroundColor(), palette.panel);
            palette.sidebar = palette.window;
            palette.text = color(NSColor::labelColor(), palette.panel);
            palette.muted = readable(color(NSColor::secondaryLabelColor(), palette.panel), palette.panel);
            palette.accent = color(NSColor::controlAccentColor(), palette.accent);
            palette.accent_text = readable(palette.accent, palette.panel);
            palette.line = color(NSColor::separatorColor(), palette.panel);
            palette.input = color(NSColor::textBackgroundColor(), palette.panel);
            palette.secondary_surface = mix(palette.text, palette.panel, if dark { 0.045 } else { 0.025 });
            palette.selection = mix(color(NSColor::selectedContentBackgroundColor(), palette.accent), palette.panel, if dark { 0.24 } else { 0.13 });
            palette.hover = mix(palette.text, palette.panel, 0.055);
            palette.bar = mix(palette.text, palette.panel, 0.12);
            palette.ok = readable(color(NSColor::systemGreenColor(), palette.ok), palette.panel);
            palette.warn = readable(color(NSColor::systemOrangeColor(), palette.warn), palette.panel);
            palette.err = readable(color(NSColor::systemRedColor(), palette.err), palette.panel);
            let native_font = NSFont::systemFontOfSize(13.0).familyName().map(|name| name.to_string()).unwrap_or_default();
            // Fontique exposes Apple's hidden SF font under this concrete
            // family name, rather than AppKit's runtime-only UIFont alias.
            palette.font_family = if native_font.starts_with(".AppleSystemUIFont") { ".SF NS".into() } else { native_font };
            palette.ensure_readable();
        });
        appearance.performAsCurrentDrawingAppearance(&block);
        drop(block);
        Some(result.into_inner())
    }

    pub(crate) struct Observer {
        token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    }

    impl Drop for Observer {
        fn drop(&mut self) {
            // Token was returned by this same notification center.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver((*self.token).as_ref()); }
        }
    }

    pub(super) fn watch(changed: fn()) -> Option<Observer> {
        MainThreadMarker::new()?;
        let block = RcBlock::new(move |_: NonNull<NSNotification>| { let _ = slint::invoke_from_event_loop(changed); });
        // The block captures only a Send function pointer and posts to Slint;
        // the main operation queue serializes native notification delivery.
        let token = unsafe { NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(Some(NSSystemColorsDidChangeNotification), None, Some(&NSOperationQueue::mainQueue()), &block) };
        Some(Observer { token })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_appearances_keep_small_text_and_status_labels_readable() {
        for dark in [false, true] {
            let palette = fallback(dark);
            for color in [palette.text, palette.muted, palette.accent_text, palette.ok, palette.warn, palette.err] {
                for surface in [palette.panel, palette.window, palette.sidebar, palette.secondary_surface, palette.selection, palette.hover] {
                    assert!(contrast(color, surface) >= 4.5);
                }
            }
        }
    }

    #[test]
    fn low_contrast_system_colors_are_adjusted_without_losing_light_dark_direction() {
        assert!(luminance(readable(rgb(0xff9500), rgb(0xffffff))) < luminance(rgb(0xff9500)));
        assert!(luminance(readable(rgb(0x444444), rgb(0x2c2c2e))) > luminance(rgb(0x444444)));
        assert_eq!(index("unrecognized"), 0);
        assert_eq!(preference(index("dark")), "dark");
    }
}
