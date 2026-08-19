//! The `DisplayServer` backend seam (pat-0q2kx.38).
//!
//! Godot's `DisplayServer` is a platform singleton: the web build binds clipboard
//! to `navigator.clipboard`, TTS to `SpeechSynthesis`, and window size to the
//! canvas (`platform/web/display_server_web.cpp`), while `has_feature` answers
//! truthfully per platform.
//!
//! Patina's script-side `DisplayServer.*` calls resolved entirely inside the
//! interpreter: window size was a hardcoded 1920x1080, the clipboard was a
//! process-local `thread_local` cell, TTS appended to a vector, and `has_feature`
//! always answered `false`. Nothing errored — a game asking for its window size
//! simply got a number unrelated to the window it was running in, and every
//! capability query said "unsupported" even on a platform that supported it.
//!
//! This module is the seam those calls now route through. The trait lives in
//! `gdcore` rather than `gdplatform` because `gdscript-interop` — where the
//! script dispatch lives — depends on `gdcore` but NOT on `gdplatform`; putting
//! it here lets the interpreter reach it without inverting the crate graph, and
//! `gdplatform` (which does depend on `gdcore`) can still implement richer
//! backends on top.
//!
//! Install a backend once at boot with [`install_display_backend`]; script calls
//! read it via [`display_backend`]. With nothing installed the interpreter keeps
//! its previous headless answers, so existing behaviour is unchanged.

use std::sync::{Arc, Mutex, OnceLock};

/// `DisplayServer.Feature` values, from `doc/classes/DisplayServer.xml`.
/// Only the ones a backend is likely to answer are named here.
pub mod feature {
    pub const GLOBAL_MENU: i64 = 0;
    pub const SUBWINDOWS: i64 = 1;
    pub const TOUCHSCREEN: i64 = 2;
    pub const MOUSE: i64 = 3;
    pub const MOUSE_WARP: i64 = 4;
    pub const CLIPBOARD: i64 = 5;
    pub const VIRTUAL_KEYBOARD: i64 = 6;
    pub const CURSOR_SHAPE: i64 = 7;
    pub const CUSTOM_CURSOR_SHAPE: i64 = 8;
    pub const NATIVE_DIALOG: i64 = 9;
    pub const IME: i64 = 10;
    pub const WINDOW_TRANSPARENCY: i64 = 11;
    pub const HIDPI: i64 = 12;
    pub const ICON: i64 = 13;
    pub const NATIVE_ICON: i64 = 14;
    pub const ORIENTATION: i64 = 15;
    pub const SWAP_BUFFERS: i64 = 16;
    pub const CLIPBOARD_PRIMARY: i64 = 18;
    pub const TEXT_TO_SPEECH: i64 = 19;
}

/// A platform's `DisplayServer` implementation.
///
/// Every method has a default so a backend only overrides what it genuinely
/// supports — a backend that forgets a method reports the honest headless
/// answer rather than a plausible-looking lie. `&self` throughout, so a backend
/// with mutable state (a clipboard, a TTS log) carries its own interior
/// mutability and can be shared across threads.
pub trait DisplayBackend: Send + Sync {
    /// Window size in pixels.
    fn window_size(&self) -> (i64, i64);

    /// Screen size in pixels; defaults to the window size.
    fn screen_size(&self) -> (i64, i64) {
        self.window_size()
    }

    fn screen_dpi(&self) -> i64 {
        96
    }

    fn screen_scale(&self) -> f64 {
        1.0
    }

    fn screen_count(&self) -> i64 {
        1
    }

    fn clipboard_get(&self) -> String {
        String::new()
    }

    fn clipboard_set(&self, _text: &str) {}

    fn clipboard_has(&self) -> bool {
        !self.clipboard_get().is_empty()
    }

    fn tts_speak(&self, _text: &str) {}

    fn tts_stop(&self) {}

    fn tts_voices(&self) -> Vec<String> {
        Vec::new()
    }

    fn tts_is_speaking(&self) -> bool {
        false
    }

    /// The display driver's name (Godot's `DisplayServer.get_name()`):
    /// `Windows`, `macOS`, `X11`, `Wayland`, `Android`, `iOS`, `web`, or
    /// `headless`. Defaults to the honest answer for a backend that has not
    /// said otherwise.
    fn display_name(&self) -> String {
        "headless".to_string()
    }

    /// Current pointer mode (Godot's `DisplayServer.mouse_get_mode()`), as the
    /// `MouseMode` enum's integer: 0 VISIBLE, 1 HIDDEN, 2 CAPTURED,
    /// 3 CONFINED, 4 CONFINED_HIDDEN.
    fn mouse_mode(&self) -> i64 {
        0
    }

    /// Requests a pointer mode. A backend with no pointer ignores it, which is
    /// why the default is a no-op rather than an error: Godot's own headless
    /// display server accepts the call and reports VISIBLE forever.
    fn set_mouse_mode(&self, _mode: i64) {}

    /// Pointer position in screen coordinates
    /// (`DisplayServer.mouse_get_position()`).
    fn mouse_position(&self) -> (f64, f64) {
        (0.0, 0.0)
    }

    /// Whether the OS exposes a light/dark preference at all
    /// (`DisplayServer.is_dark_mode_supported()`).
    ///
    /// ⚠️ Distinct from [`Self::is_dark_mode`]: Godot returns `false` from
    /// `is_dark_mode()` both when the system is light AND when it cannot tell,
    /// so a caller needs the supported flag to distinguish them.
    fn is_dark_mode_supported(&self) -> bool {
        false
    }

    /// Whether the OS is currently in dark mode.
    fn is_dark_mode(&self) -> bool {
        false
    }

    /// Refresh rate of the primary screen, in Hz.
    ///
    /// ⚠️ Godot returns **-1.0** when the rate cannot be determined
    /// (`DisplayServer.xml:1797`), NOT a plausible 60.0. A backend that
    /// invented 60 would have callers computing frame budgets from a number
    /// nobody measured — the sentinel is the honest answer and callers check
    /// for it.
    fn screen_refresh_rate(&self) -> f64 {
        -1.0
    }

    /// Position of the window's client area on the screen, in pixels.
    fn window_position(&self) -> (i64, i64) {
        (0, 0)
    }

    /// Whether touch events are available (Android/iOS, or detected on web).
    fn touchscreen_available(&self) -> bool {
        false
    }

    /// Whether OK and Cancel are swapped in dialogs — Godot enables this on
    /// Windows to follow platform convention (`DisplayServer.xml:910`).
    fn swap_cancel_ok(&self) -> bool {
        false
    }

    /// The default cursor shape (`CursorShape`; 0 is `CURSOR_ARROW`).
    fn cursor_shape(&self) -> i64 {
        0
    }

    /// Requests a default cursor shape. A backend with no cursor ignores it.
    fn set_cursor_shape(&self, _shape: i64) {}

    /// V-Sync mode for the main window (`VSyncMode`: 0 DISABLED, 1 ENABLED,
    /// 2 ADAPTIVE, 3 MAILBOX).
    ///
    /// Defaults to **1**, matching Godot's `display/window/vsync/vsync_mode`
    /// project-setting default rather than "off" — a backend that cannot
    /// present still reports the mode the engine believes is in force.
    fn vsync_mode(&self) -> i64 {
        1
    }

    /// Requests a V-Sync mode; a backend with no swapchain ignores it.
    fn set_vsync_mode(&self, _mode: i64) {}

    /// Screen orientation (`ScreenOrientation`; 0 is `SCREEN_LANDSCAPE`).
    ///
    /// ⚠️ Godot answers `SCREEN_LANDSCAPE` even for an invalid screen
    /// (`DisplayServer.xml:1762`), so 0 is the documented fallback rather than
    /// an error value.
    fn screen_orientation(&self) -> i64 {
        0
    }

    /// Requests a screen orientation; desktop backends ignore it.
    fn set_screen_orientation(&self, _orientation: i64) {}

    /// The unobscured display area interactive controls should stay inside —
    /// Godot's `get_display_safe_area()`, which is where notches and rounded
    /// corners get subtracted on mobile.
    ///
    /// Defaults to the WHOLE screen, derived from [`Self::screen_size`], so a
    /// backend with no cutouts is automatically correct and one that reports a
    /// size never has to restate it. A zero rect would tell every caller there
    /// is nowhere safe to draw.
    fn display_safe_area(&self) -> (i64, i64, i64, i64) {
        let (w, h) = self.screen_size();
        (0, 0, w, h)
    }

    /// Maps a physical (US QWERTY) keycode onto the active keyboard layout.
    ///
    /// ⚠️ The default is the IDENTITY, matching Godot's base implementation —
    /// the method is only implemented on Linux, macOS and Windows, and a
    /// platform with no layout information hands the physical code straight
    /// back. Returning 0 would map every physical key to `KEY_NONE` and break
    /// every key-remapping UI that round-trips through this.
    fn keycode_from_physical(&self, keycode: i64) -> i64 {
        keycode
    }

    /// Bounding rectangles of display cutouts / notches, empty on a device with
    /// none.
    ///
    /// ⚠️ Godot types these as **`Rect2`** (floats), even though the closely
    /// related `get_display_safe_area()` is a `Rect2i`. The asymmetry is real;
    /// assuming they match is the easy mistake.
    fn display_cutouts(&self) -> Vec<(f32, f32, f32, f32)> {
        Vec::new()
    }

    /// The IME composition string, empty when no composition is active.
    fn ime_text(&self) -> String {
        String::new()
    }

    /// IME selection as `(caret position, selection length)`.
    fn ime_selection(&self) -> (i64, i64) {
        (0, 0)
    }

    /// The OS theme accent colour as RGBA in 0..=1.
    ///
    /// ⚠️ Godot returns `Color(0, 0, 0, 0)` when the accent colour is unknown
    /// (`DisplayServer.xml:847`) — a fully transparent sentinel, NOT a
    /// plausible blue. Callers test the alpha to decide whether to use it.
    fn accent_color(&self) -> (f32, f32, f32, f32) {
        (0.0, 0.0, 0.0, 0.0)
    }

    /// Whether this backend really supports `feature` (a
    /// [`feature`]-module constant). Defaults to `false`, so an unimplemented
    /// capability is reported honestly.
    fn has_feature(&self, _feature: i64) -> bool {
        false
    }
}

/// The default backend: no real display, but window metrics sourced from the
/// project's configured viewport rather than invented.
///
/// This is what makes `DisplayServer.window_get_size()` agree with
/// `display/window/size/viewport_width` in `project.godot` instead of reporting
/// a fixed 1920x1080 that matches nothing.
pub struct HeadlessDisplayBackend {
    width: i64,
    height: i64,
    clipboard: Mutex<String>,
    tts_log: Mutex<Vec<String>>,
    /// Last mode requested through [`DisplayBackend::set_mouse_mode`]. A
    /// headless process has no pointer, but remembering the request is what
    /// makes `mouse_set_mode` / `mouse_get_mode` a faithful round-trip — Godot's
    /// headless server behaves the same way.
    mouse_mode: Mutex<i64>,
    /// Last shape requested through [`DisplayBackend::set_cursor_shape`], for
    /// the same reason as `mouse_mode`: a headless process has no cursor, but
    /// remembering the request keeps `cursor_set_shape` / `cursor_get_shape` a
    /// faithful round-trip.
    cursor_shape: Mutex<i64>,
    /// Last requested V-Sync mode; remembered so the setter/getter pair round
    /// trips even though a headless process presents nothing.
    vsync_mode: Mutex<i64>,
    /// Last requested screen orientation, remembered for the same reason.
    screen_orientation: Mutex<i64>,
}

impl HeadlessDisplayBackend {
    /// Builds a backend reporting the given viewport size.
    pub fn new(width: i64, height: i64) -> Self {
        Self {
            width,
            height,
            clipboard: Mutex::new(String::new()),
            tts_log: Mutex::new(Vec::new()),
            mouse_mode: Mutex::new(0),
            cursor_shape: Mutex::new(0),
            // Godot's project-setting default is ENABLED (1).
            vsync_mode: Mutex::new(1),
            screen_orientation: Mutex::new(0),
        }
    }

    /// Everything spoken through this backend, in order — the headless stand-in
    /// for actually hearing it.
    pub fn spoken(&self) -> Vec<String> {
        self.tts_log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

impl Default for HeadlessDisplayBackend {
    fn default() -> Self {
        // Godot's own default viewport, used when a project sets none.
        Self::new(1152, 648)
    }
}

impl DisplayBackend for HeadlessDisplayBackend {
    fn window_size(&self) -> (i64, i64) {
        (self.width, self.height)
    }

    fn clipboard_get(&self) -> String {
        self.clipboard.lock().map(|c| c.clone()).unwrap_or_default()
    }

    fn clipboard_set(&self, text: &str) {
        if let Ok(mut c) = self.clipboard.lock() {
            *c = text.to_string();
        }
    }

    fn tts_speak(&self, text: &str) {
        if let Ok(mut l) = self.tts_log.lock() {
            l.push(text.to_string());
        }
    }

    fn tts_stop(&self) {
        if let Ok(mut l) = self.tts_log.lock() {
            l.clear();
        }
    }

    fn mouse_mode(&self) -> i64 {
        self.mouse_mode.lock().map(|m| *m).unwrap_or(0)
    }

    /// ⚠️ Out-of-range modes are ignored rather than stored. Godot's enum stops
    /// at `MOUSE_MODE_MAX` (5), and echoing back a mode the engine has no
    /// meaning for would make `mouse_get_mode()` a value the caller cannot act
    /// on — worse than reporting the mode that is actually in force.
    fn set_mouse_mode(&self, mode: i64) {
        if !(0..5).contains(&mode) {
            return;
        }
        if let Ok(mut m) = self.mouse_mode.lock() {
            *m = mode;
        }
    }

    fn cursor_shape(&self) -> i64 {
        self.cursor_shape.lock().map(|c| *c).unwrap_or(0)
    }

    /// ⚠️ Godot's `CursorShape` runs 0..=16 (`CURSOR_MAX` is 17). Out-of-range
    /// shapes are ignored for the same reason as mouse modes: echoing back a
    /// shape the engine cannot draw is worse than reporting the one in force.
    fn set_cursor_shape(&self, shape: i64) {
        if !(0..17).contains(&shape) {
            return;
        }
        if let Ok(mut c) = self.cursor_shape.lock() {
            *c = shape;
        }
    }

    fn vsync_mode(&self) -> i64 {
        self.vsync_mode.lock().map(|v| *v).unwrap_or(1)
    }

    /// `VSyncMode` runs 0..=3; anything else leaves the mode in force, for the
    /// same reason as the other enum setters here.
    fn set_vsync_mode(&self, mode: i64) {
        if !(0..4).contains(&mode) {
            return;
        }
        if let Ok(mut v) = self.vsync_mode.lock() {
            *v = mode;
        }
    }

    fn screen_orientation(&self) -> i64 {
        self.screen_orientation.lock().map(|o| *o).unwrap_or(0)
    }

    /// `ScreenOrientation` runs 0..=6 (`SCREEN_SENSOR` is the last).
    fn set_screen_orientation(&self, orientation: i64) {
        if !(0..7).contains(&orientation) {
            return;
        }
        if let Ok(mut o) = self.screen_orientation.lock() {
            *o = orientation;
        }
    }

    /// A headless process really does have a working in-process clipboard and
    /// TTS sink, and really has no mouse, screens or cursor — so it answers
    /// exactly those.
    fn has_feature(&self, feature: i64) -> bool {
        matches!(feature, feature::CLIPBOARD | feature::TEXT_TO_SPEECH)
    }
}

fn slot() -> &'static Mutex<Option<Arc<dyn DisplayBackend>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<dyn DisplayBackend>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Installs the process-wide display backend, replacing any previous one.
///
/// Called once at boot by the runner/editor. Replaceable rather than
/// write-once so a test can install a recording backend.
pub fn install_display_backend(backend: Arc<dyn DisplayBackend>) {
    if let Ok(mut s) = slot().lock() {
        *s = Some(backend);
    }
}

/// The installed backend, or `None` when nothing has been installed.
pub fn display_backend() -> Option<Arc<dyn DisplayBackend>> {
    slot().lock().ok().and_then(|s| s.clone())
}

/// Removes the installed backend. Mainly for tests that must not leak a backend
/// into another test in the same process.
pub fn clear_display_backend() {
    if let Ok(mut s) = slot().lock() {
        *s = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_backend_reports_configured_viewport() {
        let b = HeadlessDisplayBackend::new(1152, 648);
        assert_eq!(b.window_size(), (1152, 648));
        // Screen size defaults to the window size rather than a separate lie.
        assert_eq!(b.screen_size(), (1152, 648));
    }

    #[test]
    fn headless_backend_round_trips_the_clipboard() {
        let b = HeadlessDisplayBackend::default();
        assert!(!b.clipboard_has(), "a fresh clipboard is empty");
        b.clipboard_set("hello");
        assert_eq!(b.clipboard_get(), "hello");
        assert!(b.clipboard_has());
    }

    #[test]
    fn headless_backend_answers_features_honestly() {
        let b = HeadlessDisplayBackend::default();
        // It really does have these...
        assert!(b.has_feature(feature::CLIPBOARD));
        assert!(b.has_feature(feature::TEXT_TO_SPEECH));
        // ...and really does not have these. Answering `true` here would be the
        // same class of lie as the old always-`false`.
        assert!(!b.has_feature(feature::MOUSE));
        assert!(!b.has_feature(feature::TOUCHSCREEN));
        assert!(!b.has_feature(feature::SUBWINDOWS));
    }

    #[test]
    fn tts_speak_is_recorded_and_stop_clears_it() {
        let b = HeadlessDisplayBackend::default();
        b.tts_speak("one");
        b.tts_speak("two");
        assert_eq!(b.spoken(), vec!["one".to_string(), "two".to_string()]);
        b.tts_stop();
        assert!(b.spoken().is_empty());
    }

    #[test]
    fn install_and_clear_round_trip() {
        clear_display_backend();
        assert!(display_backend().is_none(), "starts uninstalled");
        install_display_backend(Arc::new(HeadlessDisplayBackend::new(800, 600)));
        assert_eq!(
            display_backend().expect("installed").window_size(),
            (800, 600)
        );
        clear_display_backend();
        assert!(display_backend().is_none());
    }
}
