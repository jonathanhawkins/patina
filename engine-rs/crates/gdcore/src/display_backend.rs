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
}

impl HeadlessDisplayBackend {
    /// Builds a backend reporting the given viewport size.
    pub fn new(width: i64, height: i64) -> Self {
        Self {
            width,
            height,
            clipboard: Mutex::new(String::new()),
            tts_log: Mutex::new(Vec::new()),
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
        self.clipboard
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
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
