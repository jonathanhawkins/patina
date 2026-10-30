//! `patina-web` — the WebAssembly runtime entry point for the Patina engine.
//!
//! This crate is the `wasm32-unknown-unknown` compilation root: it is built as a
//! `cdylib` and instantiated in the browser via `wasm-bindgen`. It links the
//! platform-agnostic engine core (currently `gdcore` + `gdvariant`) so the
//! scene/script data model is available in the browser, the engine's audio
//! stack (`gdaudio`, see the `audio` module), plus — on wasm only —
//! `gdrender3d` for canvas rendering (see the `canvas` module). Subsystems that
//! still depend on native-only crates (`gdscene` -> `std::fs`) are cfg-gated
//! and brought online by the remaining web beads (pat-u3l8c.111, .113).
//!
//! All browser-only bindings live behind `cfg(target_arch = "wasm32")`, so the
//! crate also builds and unit-tests on the host as an `rlib`. The monotonic
//! clock the engine uses on the web is provided by `gdplatform::os` via
//! `web_time::Instant` (`performance.now()` on wasm) — see that crate.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Canvas rendering: binds a `<canvas>` to a wgpu surface (pat-u3l8c.110).
/// Browser-only — the surface target and `web_sys::window()` exist only on
/// wasm. The sizing math it relies on ([`canvas_backing_size`]) lives here in
/// the crate root so it is testable on the host.
#[cfg(target_arch = "wasm32")]
pub mod canvas;

/// The engine handle that drives a `MainLoop` from animation-frame timestamps
/// (pat-s3rrv.40).
pub mod engine;

/// Browser event → engine input translation (pat-s3rrv.41).
pub mod browser_input;

/// The browser end of the frame loop and the input listeners
/// (pat-u3l8c.111): registers `requestAnimationFrame`, subscribes the DOM
/// events, polls `navigator.getGamepads()`, and feeds it all into
/// [`engine::WebEngine`].
///
/// Browser-only — everything it decides lives in [`browser_input`] and
/// [`engine`], which are host-tested; this module is the JS plumbing.
#[cfg(target_arch = "wasm32")]
pub mod browser_runtime;

/// The web audio backend: the engine's `AudioServer` mixing into the ring an
/// `AudioWorkletProcessor` drains (pat-u3l8c.112).
///
/// Portable on purpose — the mixer -> resample -> ring -> block chain is
/// exercised by native `cargo test`, and only the `AudioContext` half needs a
/// browser (see [`web_audio`]).
pub mod audio;

/// Browser bindings for [`audio`]: creates the `AudioContext`, emits the
/// worklet source, and serves the worklet's pulls (pat-u3l8c.112).
#[cfg(target_arch = "wasm32")]
pub mod web_audio;

/// The web asset VFS: `res://` out of a fetched `.pck`, `user://` in the
/// browser's persistent storage (pat-u3l8c.113).
///
/// Portable on purpose, like [`audio`]: the pack mount and the write-through
/// `user://` store are exercised by native `cargo test`, and only the `fetch`
/// and the IndexedDB puts need a browser (see [`web_vfs`]).
pub mod vfs;

/// Browser bindings for [`vfs`]: the handle the page mounts the fetched pack
/// through and drains `user://` writes from (pat-u3l8c.113).
#[cfg(target_arch = "wasm32")]
pub mod web_vfs;

/// Largest backing-store dimension we will ask a browser for, in physical
/// pixels. Matches wgpu's default `max_texture_dimension_2d`, which is the
/// ceiling a swapchain texture has to fit under; a huge CSS box on a 3x
/// display would otherwise ask for a surface no adapter can allocate.
pub const MAX_BACKING_DIMENSION: u32 = 8192;

/// Physical-pixel size a canvas' backing store (and therefore the swapchain)
/// should use for a CSS box of `css_width` x `css_height` at
/// `device_pixel_ratio`.
///
/// Keeping the backing store at `css * dpr` is what makes the render sharp on
/// a HiDPI display, where one CSS pixel is 2 or 3 device pixels. Three edges
/// are handled because all three occur in real pages:
///
/// - a not-yet-laid-out or `display:none` canvas reports a 0-sized box, and
///   wgpu panics on a zero-sized swapchain, so the result is clamped to 1;
/// - a browser can report a nonsensical `devicePixelRatio` (0, NaN) — treated
///   as 1.0 rather than collapsing the surface;
/// - the result is capped at [`MAX_BACKING_DIMENSION`] so a large box on a
///   high-DPR display cannot demand an unallocatable texture.
pub fn canvas_backing_size(css_width: u32, css_height: u32, device_pixel_ratio: f64) -> (u32, u32) {
    let dpr = if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio
    } else {
        1.0
    };
    let scale = |css: u32| -> u32 {
        let scaled = (f64::from(css) * dpr).round();
        // `as u32` saturates at 0 and u32::MAX, so an absurd product cannot
        // wrap; the explicit clamp then bounds it to what an adapter can hold.
        (scaled as u32).clamp(1, MAX_BACKING_DIMENSION)
    };
    (scale(css_width), scale(css_height))
}

/// wasm-bindgen entry point, invoked automatically when the module is
/// instantiated in the browser. Installs a panic hook that forwards Rust panics
/// to the browser console; the JS host then drives the engine through the
/// (forthcoming) exported API.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn __patina_web_start() {
    console_error_panic_hook::set_once();
}

/// Version of the engine linked into this web runtime (the workspace version).
pub fn engine_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Smoke check that the linked engine core is usable from the web runtime: build
/// a couple of [`gdvariant::Variant`] values and confirm they behave. Runs on
/// both native (`cargo test`) and, once wired to an exported entry, in the
/// browser — proving the core actually links on `wasm32`.
pub fn core_linked() -> bool {
    use gdcore::Vector2;
    use gdvariant::Variant;
    let v = Vector2 { x: 3.0, y: 4.0 };
    let i = Variant::Int(7);
    matches!(i, Variant::Int(7)) && (v.x - 3.0).abs() < 1e-6 && (v.y - 4.0).abs() < 1e-6
}

/// Frames-per-second over a sliding one-second window (pat-u3l8c.111).
///
/// The acceptance for the web frame loop is a rate, so the rate has to be
/// *measurable* from the page rather than asserted. Dividing the frame count
/// by the total elapsed time would report a long-run average that hides a
/// stall; counting frames inside a window reports what the last second
/// actually did.
///
/// Target-independent: it takes the same `DOMHighResTimeStamp` the frame
/// callback gets, so the averaging is unit-tested on the host.
#[derive(Debug, Clone)]
pub struct FpsMeter {
    window_ms: f64,
    window_start_ms: Option<f64>,
    frames_in_window: u32,
    last_fps: f64,
}

impl FpsMeter {
    /// A meter averaging over `window_ms` milliseconds.
    ///
    /// A non-finite or non-positive window falls back to one second rather
    /// than dividing by zero on the first report.
    pub fn new(window_ms: f64) -> Self {
        let window_ms = if window_ms.is_finite() && window_ms > 0.0 {
            window_ms
        } else {
            1000.0
        };
        Self {
            window_ms,
            window_start_ms: None,
            frames_in_window: 0,
            last_fps: 0.0,
        }
    }

    /// Records a frame at `now_ms` and returns the current rate.
    ///
    /// The rate only changes once a full window has elapsed; before then the
    /// previous window's value stands (0.0 for the very first second), because
    /// extrapolating from a partial window makes the first reading wild.
    pub fn frame(&mut self, now_ms: f64) -> f64 {
        let start = match self.window_start_ms {
            Some(start) => start,
            None => {
                // The first frame only opens the window; it is the boundary,
                // not a sample inside it.
                self.window_start_ms = Some(now_ms);
                return self.last_fps;
            }
        };
        self.frames_in_window += 1;
        let span = now_ms - start;
        // A backwards clock (a stepped system time) would otherwise produce a
        // negative span and a negative rate.
        if span <= 0.0 {
            self.window_start_ms = Some(now_ms);
            self.frames_in_window = 0;
            return self.last_fps;
        }
        if span >= self.window_ms {
            self.last_fps = f64::from(self.frames_in_window) * 1000.0 / span;
            self.window_start_ms = Some(now_ms);
            self.frames_in_window = 0;
        }
        self.last_fps
    }

    /// The last completed window's rate.
    pub fn fps(&self) -> f64 {
        self.last_fps
    }
}

impl Default for FpsMeter {
    fn default() -> Self {
        Self::new(1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_version_is_nonempty() {
        assert!(!engine_version().is_empty());
    }

    #[test]
    fn core_is_linked() {
        assert!(core_linked());
    }

    #[test]
    fn backing_size_matches_css_box_at_1x() {
        assert_eq!(canvas_backing_size(800, 600, 1.0), (800, 600));
    }

    #[test]
    fn backing_size_scales_with_device_pixel_ratio() {
        assert_eq!(canvas_backing_size(800, 600, 2.0), (1600, 1200));
        assert_eq!(canvas_backing_size(800, 600, 1.5), (1200, 900));
    }

    #[test]
    fn backing_size_rounds_a_fractional_ratio() {
        // 801 * 1.25 = 1001.25 -> 1001; 601 * 1.25 = 751.25 -> 751.
        assert_eq!(canvas_backing_size(801, 601, 1.25), (1001, 751));
    }

    #[test]
    fn backing_size_never_reaches_zero() {
        // A `display:none` canvas reports a 0-sized CSS box, and wgpu panics on
        // a zero-sized swapchain.
        assert_eq!(canvas_backing_size(0, 0, 2.0), (1, 1));
        assert_eq!(canvas_backing_size(1, 1, 0.1), (1, 1));
    }

    #[test]
    fn nonsensical_device_pixel_ratio_falls_back_to_one() {
        assert_eq!(canvas_backing_size(640, 480, 0.0), (640, 480));
        assert_eq!(canvas_backing_size(640, 480, -2.0), (640, 480));
        assert_eq!(canvas_backing_size(640, 480, f64::NAN), (640, 480));
        assert_eq!(canvas_backing_size(640, 480, f64::INFINITY), (640, 480));
    }

    #[test]
    fn backing_size_is_capped_to_an_allocatable_texture() {
        let (w, h) = canvas_backing_size(6000, 4000, 3.0);
        assert_eq!((w, h), (MAX_BACKING_DIMENSION, MAX_BACKING_DIMENSION));
        // And an enormous CSS box cannot wrap the u32 conversion.
        let (w, h) = canvas_backing_size(u32::MAX, u32::MAX, 4.0);
        assert_eq!((w, h), (MAX_BACKING_DIMENSION, MAX_BACKING_DIMENSION));
    }
}
