//! pat-07tlx: Acceptance gate for the first audio test harness.
//!
//! Acceptance: the deliverable is broken into measurable evidence with
//! tests, docs, or oracle-backed artifacts. The harness lives in
//! `engine-rs/tests/audio_deterministic_harness_test.rs`, with the audio
//! implementation in the `gdaudio` crate. This test guards the harness +
//! gdaudio anchors so a regression — harness deleted, audio crate gutted —
//! fails under the bead's marker.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn audio_harness_test_anchor_exists() {
    let p = repo_root().join("engine-rs/tests/audio_deterministic_harness_test.rs");
    assert!(
        p.exists(),
        "audio test harness must exist at {}",
        p.display()
    );
}

#[test]
fn gdaudio_crate_publishes_core_modules() {
    let crate_src = repo_root().join("engine-rs/crates/gdaudio/src");
    for module in [
        "lib.rs",
        "server.rs",
        "bus.rs",
        "stream.rs",
        "mixer.rs",
        "spatial.rs",
        "decode.rs",
        "wav.rs",
    ] {
        let p = crate_src.join(module);
        assert!(
            p.exists(),
            "gdaudio crate must publish module '{module}' at {}",
            p.display()
        );
    }
}

#[test]
fn audio_evidence_test_set_is_present() {
    for f in [
        "engine-rs/tests/audio_smoke_test.rs",
        "engine-rs/tests/audio_runtime_broad_test.rs",
        "engine-rs/tests/audio_import_pipeline_test.rs",
    ] {
        let p = repo_root().join(f);
        assert!(
            p.exists(),
            "audio evidence test must exist: {}",
            p.display()
        );
    }
}
