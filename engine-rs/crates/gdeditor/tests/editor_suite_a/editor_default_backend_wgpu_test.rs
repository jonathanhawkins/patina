//! pat-ism62: Proves the default `cargo build -p gdeditor` selects the wgpu
//! viewport backend (and therefore links wgpu via `gdrender3d/gpu`). The
//! software rasterizer is opt-in via `--no-default-features --features
//! software-render`.

#[test]
fn editor_default_backend_wgpu_test() {
    // Exactly one of the two backend features must be active.
    let gpu = cfg!(feature = "gpu-render");
    let software = cfg!(feature = "software-render");
    assert!(
        gpu ^ software,
        "exactly one of gpu-render / software-render must be enabled (gpu={gpu}, software={software})"
    );

    let backend = gdeditor::viewport_backend();
    if gpu {
        assert_eq!(
            backend, "wgpu",
            "default editor build must report the wgpu backend"
        );
    } else {
        assert_eq!(
            backend, "software",
            "software-render feature must report the software backend"
        );
    }
}

/// Compiled-in evidence that the wgpu crate is actually linked when the
/// default `gpu-render` feature is on. Pulls a symbol from `wgpu` through
/// `gdrender3d` so the linker will fail if the dep chain regresses.
#[cfg(feature = "gpu-render")]
#[test]
fn editor_default_backend_links_wgpu_symbol() {
    // `gdrender3d::wgpu_pipeline::GpuVertex::layout()` returns a
    // `wgpu::VertexBufferLayout`. Calling it forces wgpu into the link.
    let layout = gdrender3d::wgpu_pipeline::GpuVertex::layout();
    assert!(
        layout.array_stride > 0,
        "wgpu vertex layout must have a non-zero stride"
    );
}
