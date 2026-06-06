//! pat-ism62: The default editor viewport renderer is wgpu.
//!
//! Acceptance (this test): a default `cargo build -p gdeditor` selects the
//! wgpu viewport backend; the software path is gated behind
//! `--features software-render`. The GPU CI pixel-match against a wgpu
//! golden is enforced by a separate render-gate; this unit-level test only
//! validates that the *backend selector* defaults to wgpu so that any code
//! path branching on it picks the GPU implementation by default.

#[test]
fn editor_default_backend_wgpu_test() {
    // gdeditor declares `default = ["gpu-render"]` and gates the software
    // rasterizer behind the `software-render` feature; this assertion
    // catches anyone toggling either of those defaults by accident.
    assert_eq!(
        gdeditor::viewport_backend(),
        "wgpu",
        "default editor build must select the wgpu viewport backend; the \
         software rasterizer is opt-in via --features software-render"
    );
}
