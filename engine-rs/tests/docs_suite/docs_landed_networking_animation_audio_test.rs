//! pat-273bn: docs must not contradict landed networking / animation / audio / post-process.

use std::fs;
use std::path::PathBuf;

fn migration_guide() -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/migration-guide.md"))
        .expect("read migration-guide")
}

#[test]
fn migration_guide_does_not_list_networking_as_unsupported() {
    let g = migration_guide();
    assert!(
        !g.contains("✗ Networking/multiplayer"),
        "migration-guide must not list networking as fully unsupported"
    );
    assert!(
        g.contains("HTTP") || g.contains("WebSocket") || g.contains("TcpMultiplayer"),
        "migration-guide should acknowledge HTTP/WebSocket/Tcp multiplayer"
    );
    // Stale absolute denials that contradicted landed HTTP/WS/Tcp multiplayer.
    assert!(
        !g.contains("No `MultiplayerPeer` / ENet / WebSocket transport"),
        "migration-guide must not claim no MultiplayerPeer/WebSocket transport"
    );
    assert!(
        !g.contains("No `HTTPRequest` node"),
        "migration-guide must not claim HTTPRequest is missing"
    );
    // Limitations table must not list HTTPRequest under "Not Yet Supported".
    for line in g.lines() {
        if line.contains("| Networking |") && line.contains("HTTPRequest") {
            assert!(
                line.contains("implemented") || line.contains("TcpMultiplayerPeer"),
                "Networking unsupported row must not list bare HTTPRequest as missing: {line}"
            );
        }
    }
}

#[test]
fn migration_guide_does_not_claim_no_animation_tree() {
    let g = migration_guide();
    assert!(
        !g.contains("No blend trees or `AnimationTree`"),
        "AnimationTree exists in gdscene"
    );
    assert!(
        !g.contains("no blend trees or state machines"),
        "animation partial claim must not deny blend trees wholesale"
    );
}

#[test]
fn migration_guide_does_not_call_tween_a_stub_only() {
    let g = migration_guide();
    // Allow "Partial" but not a table row of pure Stub for Tween.
    for line in g.lines() {
        if line.contains("`Tween`") && line.contains('|') {
            assert!(
                !line.contains("| Stub |"),
                "Tween must not be Stub-only: {line}"
            );
        }
    }
}

#[test]
fn migration_guide_worldenvironment_mentions_post_or_tonemap() {
    let g = migration_guide();
    let mut found = false;
    for line in g.lines() {
        if line.contains("`WorldEnvironment`") {
            found = true;
            assert!(
                line.contains("tonemap")
                    || line.contains("glow")
                    || line.contains("fog")
                    || line.contains("post"),
                "WorldEnvironment row should reflect tonemap/glow/fog: {line}"
            );
        }
    }
    assert!(found, "WorldEnvironment row must exist");
}

#[test]
fn compat_dashboard_audio_not_stub_only() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../COMPAT_DASHBOARD.md");
    let d = fs::read_to_string(path).expect("COMPAT_DASHBOARD");
    assert!(
        !d.contains("| Audio | Claimed |") || !d.contains("Stub only"),
        "Audio must not be documented as stub-only claimed"
    );
}

/// pat-u2e5p: FileSystem visual-fidelity exit criteria stay ticked when tests exist.
#[test]
fn visual_fidelity_exit_filesystem_criteria_are_checked() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../prd/EDITOR_PARITY_VISUAL_FIDELITY_EXIT.md");
    let d = fs::read_to_string(path).expect("read visual fidelity exit");
    for key in [
        "fs-vf-favorites-header",
        "fs-vf-toolbar-chrome",
        "fs-vf-chrome-wired",
    ] {
        let mut found = false;
        for line in d.lines() {
            if line.contains(key) {
                found = true;
                assert!(
                    line.trim_start().starts_with("- [x]"),
                    "exit criterion {key} must be checked off (test already lands): {line}"
                );
            }
        }
        assert!(found, "exit criterion {key} must appear in the exit doc");
    }
}

/// pat-f5o9v: migration-guide must not deny the landed GDScript interpreter.
#[test]
fn migration_guide_does_not_claim_no_gdscript_interpreter() {
    let g = migration_guide();
    assert!(
        !g.contains("No GDScript interpreter"),
        "migration-guide must not claim there is no GDScript interpreter"
    );
    assert!(
        !g.contains("No GDScript -- logic is written in Rust"),
        "migration-guide must not claim No GDScript wholesale"
    );
    assert!(
        !g.contains("GDScript execution (logic must be written in Rust)"),
        "migration-guide must not claim GDScript execution is impossible"
    );
    // Positive acknowledgment of the interop/interpreter path.
    assert!(
        g.contains("gdscript-interop")
            || g.contains("Partial GDScript")
            || g.contains("GDScript interpreter"),
        "migration-guide should mention the GDScript interpreter / gdscript-interop"
    );
}
