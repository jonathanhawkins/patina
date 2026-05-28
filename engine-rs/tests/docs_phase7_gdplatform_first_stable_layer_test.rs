//! pat-4zmrb: Acceptance gate for the `gdplatform` first stable layer.
//!
//! Acceptance: windowing, input, and timing responsibilities are documented
//! for the stable layer and backed by focused gdplatform or integration tests.
//! Documentation lives in `docs/PLATFORM_STABLE_LAYER.md`; implementation
//! lives in `engine-rs/crates/gdplatform/src/` (window, display, backend,
//! input, time); integration coverage is in the sibling
//! `platform_first_stable_layer_test.rs` plus windowing/input/timing
//! evidence tests. This test guards each anchor under the bead's marker.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn stable_layer_doc_documents_three_responsibilities() {
    let p = repo_root().join("docs/PLATFORM_STABLE_LAYER.md");
    let body = std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!(
            "PLATFORM_STABLE_LAYER.md must exist at {}: {e}",
            p.display()
        )
    });
    let lower = body.to_lowercase();
    for needle in ["windowing", "input", "timing"] {
        assert!(
            lower.contains(needle),
            "stable-layer doc must document '{needle}' responsibility"
        );
    }
}

#[test]
fn gdplatform_publishes_first_stable_layer_modules() {
    let crate_src = repo_root().join("engine-rs/crates/gdplatform/src");
    for module in ["backend.rs", "display.rs", "input.rs", "time.rs"] {
        let p = crate_src.join(module);
        assert!(
            p.exists(),
            "gdplatform stable-layer module must exist: {}",
            p.display()
        );
    }
}

#[test]
fn integration_test_anchors_present() {
    for f in [
        "engine-rs/tests/platform_first_stable_layer_test.rs",
        "engine-rs/tests/window_lifecycle_test.rs",
        "engine-rs/tests/input_action_coverage_test.rs",
        "engine-rs/tests/time_singleton_parity_test.rs",
    ] {
        let p = repo_root().join(f);
        assert!(
            p.exists(),
            "stable-layer evidence test must exist: {}",
            p.display()
        );
    }
}
