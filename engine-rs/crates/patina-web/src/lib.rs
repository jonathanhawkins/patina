//! `patina-web` — the WebAssembly runtime entry point for the Patina engine.
//!
//! This crate is the `wasm32-unknown-unknown` compilation root: it is built as a
//! `cdylib` and instantiated in the browser via `wasm-bindgen`. It links the
//! platform-agnostic engine core (currently `gdcore` + `gdvariant`) so the
//! scene/script data model is available in the browser. Heavier subsystems that
//! depend on native-only crates (`gdaudio` -> `cpal`, `gdrender3d` -> native
//! `wgpu` backends) are cfg-gated and brought online by the follow-up web beads
//! (pat-u3l8c.110-.113).
//!
//! All browser-only bindings live behind `cfg(target_arch = "wasm32")`, so the
//! crate also builds and unit-tests on the host as an `rlib`. The monotonic
//! clock the engine uses on the web is provided by `gdplatform::os` via
//! `web_time::Instant` (`performance.now()` on wasm) — see that crate.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

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
}
