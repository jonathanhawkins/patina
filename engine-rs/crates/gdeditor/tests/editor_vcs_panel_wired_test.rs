//! pat-nzgdu.101: served Version Control panel wired to the /api/vcs routes.
//!
//! Patina already had a complete server-side VCS implementation (vcs.rs +
//! /api/vcs/status|stage|unstage|diff|log) but nothing in the served editor
//! called it. This guards two things:
//!  1. Static — EDITOR_HTML now has a Version Control tab that references the
//!     status + stage endpoints and defines the panel wiring.
//!  2. Functional — a stage request moves a file from the unstaged area to the
//!     staged area, exercised against the same `vcs.rs` functions the routes
//!     call (`query_git_status` / `stage_file`), on a throwaway git repo so the
//!     round-trip is deterministic and side-effect-free.

use gdeditor::editor_ui::EDITOR_HTML;
use gdeditor::vcs::{query_git_status, stage_file, ChangeArea};
use std::path::Path;
use std::process::Command;

// ---------------------------------------------------------------------------
// (1) Static wiring guard.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_the_version_control_panel() {
    assert!(
        EDITOR_HTML.contains("/api/vcs/status"),
        "EDITOR_HTML must GET /api/vcs/status to list changed files"
    );
    assert!(
        EDITOR_HTML.contains("/api/vcs/stage"),
        "EDITOR_HTML must POST /api/vcs/stage to stage a file"
    );
    assert!(
        EDITOR_HTML.contains("data-tab=\"vcs\""),
        "EDITOR_HTML must have a Version Control bottom-panel tab"
    );
    assert!(
        EDITOR_HTML.contains("id=\"vcs-panel\""),
        "EDITOR_HTML must contain the Version Control panel markup"
    );
    assert!(
        EDITOR_HTML.contains("refreshVcsPanel"),
        "EDITOR_HTML must wire the VCS status fetch/render"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional stage round-trip (vcs.rs — what the routes call).
// ---------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("`git {args:?}` failed to spawn: {e}"));
    assert!(
        out.status.success(),
        "`git {args:?}` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn vcs_stage_moves_file_from_unstaged_to_staged() {
    // Throwaway repo (fixed path is fine: nextest runs each test in its own
    // process, and we clear any stale state first).
    let dir = std::env::temp_dir().join("patina_vcs_panel_wired_test_repo");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    git(&dir, &["init"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "Patina Test"]);

    // Track and commit a file, then modify it so it becomes an unstaged change.
    std::fs::write(dir.join("game.gd"), "extends Node\n").unwrap();
    git(&dir, &["add", "game.gd"]);
    git(&dir, &["commit", "-m", "init", "--no-gpg-sign"]);
    std::fs::write(dir.join("game.gd"), "extends Node\nfunc _ready():\n\tpass\n").unwrap();

    // Before staging: game.gd is an unstaged modification, not staged.
    let before = query_git_status(&dir);
    assert!(before.is_git_repo, "temp dir should be a git repo");
    assert!(
        before
            .files
            .iter()
            .any(|f| f.path.ends_with("game.gd") && f.area == ChangeArea::Unstaged),
        "game.gd should be an unstaged change before staging; files: {:?}",
        before.files
    );
    assert!(
        !before
            .files
            .iter()
            .any(|f| f.path.ends_with("game.gd") && f.area == ChangeArea::Staged),
        "game.gd must not be staged yet; files: {:?}",
        before.files
    );

    // Stage it — this is exactly what POST /api/vcs/stage invokes.
    stage_file(&dir, "game.gd").expect("stage_file should succeed");

    // After staging: game.gd is now in the staged area.
    let after = query_git_status(&dir);
    assert!(
        after
            .files
            .iter()
            .any(|f| f.path.ends_with("game.gd") && f.area == ChangeArea::Staged),
        "game.gd should be staged after stage_file; files: {:?}",
        after.files
    );

    let _ = std::fs::remove_dir_all(&dir);
}
