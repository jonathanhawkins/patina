//! Integration tests for VCS git status display feature (pat-1zlel).
//!
//! Tests the full VCS integration: git status queries, branch info,
//! file diff retrieval, commit log, staging/unstaging, and discard.

use std::fs;
use std::process::Command;

use gdeditor::vcs::{
    query_git_status, get_file_diff, get_file_diff_staged, get_commit_log,
    stage_file, unstage_file, discard_changes,
    FileChangeStatus, ChangeArea,
};

/// Helper to create a temporary git repo.
fn make_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    let p = dir.path();
    run_git(p, &["init"]);
    run_git(p, &["config", "user.email", "test@test.com"]);
    run_git(p, &["config", "user.name", "Test"]);
    fs::write(p.join("main.gd"), "extends Node\n").unwrap();
    run_git(p, &["add", "main.gd"]);
    run_git(p, &["commit", "-m", "init"]);
    dir
}

fn run_git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git command");
    assert!(out.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
}

// ---------------------------------------------------------------------------
// Status detection
// ---------------------------------------------------------------------------

#[test]
fn vcs_status_clean_repo() {
    let repo = make_repo();
    let status = query_git_status(repo.path());
    assert!(status.is_git_repo);
    assert!(status.files.is_empty());
    assert!(status.branch.is_some());
}

#[test]
fn vcs_status_modified_files_detected() {
    let repo = make_repo();
    fs::write(repo.path().join("main.gd"), "extends Node2D\nfunc _ready(): pass\n").unwrap();

    let status = query_git_status(repo.path());
    let modified: Vec<_> = status.files.iter()
        .filter(|f| f.status == FileChangeStatus::Modified)
        .collect();
    assert!(!modified.is_empty(), "should detect modified main.gd");
    assert!(modified.iter().any(|f| f.path == "main.gd"));
}

#[test]
fn vcs_status_untracked_files_detected() {
    let repo = make_repo();
    fs::write(repo.path().join("enemy.gd"), "extends CharacterBody2D\n").unwrap();
    fs::write(repo.path().join("player.gd"), "extends CharacterBody2D\n").unwrap();

    let status = query_git_status(repo.path());
    let untracked: Vec<_> = status.files.iter()
        .filter(|f| f.status == FileChangeStatus::Untracked)
        .collect();
    assert!(untracked.len() >= 2, "should detect at least 2 untracked files, got {}", untracked.len());
}

#[test]
fn vcs_status_deleted_file_detected() {
    let repo = make_repo();
    fs::remove_file(repo.path().join("main.gd")).unwrap();

    let status = query_git_status(repo.path());
    let deleted: Vec<_> = status.files.iter()
        .filter(|f| f.status == FileChangeStatus::Deleted)
        .collect();
    assert!(!deleted.is_empty(), "should detect deleted main.gd");
}

#[test]
fn vcs_status_staged_vs_unstaged() {
    let repo = make_repo();

    // Create and stage a new file.
    fs::write(repo.path().join("staged.gd"), "extends Sprite2D\n").unwrap();
    run_git(repo.path(), &["add", "staged.gd"]);

    // Modify an existing file without staging.
    fs::write(repo.path().join("main.gd"), "# modified\n").unwrap();

    let status = query_git_status(repo.path());

    let has_staged = status.files.iter().any(|f|
        f.path == "staged.gd" && f.area == ChangeArea::Staged
    );
    let has_unstaged = status.files.iter().any(|f|
        f.path == "main.gd" && f.area == ChangeArea::Unstaged
    );

    assert!(has_staged, "staged.gd should be staged");
    assert!(has_unstaged, "main.gd should be unstaged");
}

// ---------------------------------------------------------------------------
// Branch info
// ---------------------------------------------------------------------------

#[test]
fn vcs_branch_info_available() {
    let repo = make_repo();
    let status = query_git_status(repo.path());
    let branch = status.branch.as_ref().expect("branch info");
    assert!(!branch.name.is_empty());
    // New repo should have 0 ahead/behind (no upstream).
    assert_eq!(branch.ahead, 0);
    assert_eq!(branch.behind, 0);
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

#[test]
fn vcs_diff_shows_changes() {
    let repo = make_repo();
    fs::write(repo.path().join("main.gd"), "extends Node2D\nvar speed = 100\n").unwrap();

    let diff = get_file_diff(repo.path(), "main.gd");
    assert!(diff.contains("Node2D"), "diff should show new content");
    assert!(diff.contains("speed"), "diff should show added line");
}

#[test]
fn vcs_staged_diff() {
    let repo = make_repo();
    fs::write(repo.path().join("main.gd"), "extends Control\n").unwrap();
    run_git(repo.path(), &["add", "main.gd"]);

    let diff = get_file_diff_staged(repo.path(), "main.gd");
    assert!(diff.contains("Control"), "staged diff should show changes");
}

// ---------------------------------------------------------------------------
// Commit log
// ---------------------------------------------------------------------------

#[test]
fn vcs_commit_log_returns_history() {
    let repo = make_repo();

    // Make a second commit.
    fs::write(repo.path().join("extra.gd"), "extends Area2D\n").unwrap();
    run_git(repo.path(), &["add", "extra.gd"]);
    run_git(repo.path(), &["commit", "-m", "add extra"]);

    let log = get_commit_log(repo.path(), 10);
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].subject, "add extra");
    assert_eq!(log[1].subject, "init");
    assert!(log[0].timestamp > 0);
}

// ---------------------------------------------------------------------------
// Stage / unstage / discard
// ---------------------------------------------------------------------------

#[test]
fn vcs_stage_unstage_roundtrip() {
    let repo = make_repo();
    fs::write(repo.path().join("new.gd"), "extends Node\n").unwrap();

    // Stage.
    stage_file(repo.path(), "new.gd").expect("stage");
    let status = query_git_status(repo.path());
    assert!(status.files.iter().any(|f|
        f.path == "new.gd" && f.area == ChangeArea::Staged
    ));

    // Unstage.
    unstage_file(repo.path(), "new.gd").expect("unstage");
    let status = query_git_status(repo.path());
    assert!(!status.files.iter().any(|f|
        f.path == "new.gd" && f.area == ChangeArea::Staged
    ));
}

#[test]
fn vcs_discard_restores_original() {
    let repo = make_repo();
    let original = fs::read_to_string(repo.path().join("main.gd")).unwrap();

    fs::write(repo.path().join("main.gd"), "# completely rewritten\n").unwrap();
    discard_changes(repo.path(), "main.gd").expect("discard");

    let restored = fs::read_to_string(repo.path().join("main.gd")).unwrap();
    assert_eq!(restored, original);
}

// ---------------------------------------------------------------------------
// Summary counts
// ---------------------------------------------------------------------------

#[test]
fn vcs_summary_counts_match_file_list() {
    let repo = make_repo();
    fs::write(repo.path().join("main.gd"), "# changed\n").unwrap();
    fs::write(repo.path().join("a.txt"), "a").unwrap();
    fs::write(repo.path().join("b.txt"), "b").unwrap();

    let status = query_git_status(repo.path());

    // Verify summary counts match actual file list.
    let total_from_summary: usize = status.summary.values().sum();
    assert_eq!(
        total_from_summary,
        status.files.len(),
        "summary total should match file count"
    );
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn vcs_non_repo_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let status = query_git_status(dir.path());
    assert!(!status.is_git_repo);
    assert!(status.branch.is_none());
    assert!(status.files.is_empty());
    assert!(status.summary.is_empty());
}

#[test]
fn vcs_empty_diff_for_clean_file() {
    let repo = make_repo();
    let diff = get_file_diff(repo.path(), "main.gd");
    assert!(diff.is_empty(), "clean file should have empty diff");
}

#[test]
fn vcs_commit_log_empty_repo_still_works() {
    // We can't easily test a truly empty repo (no commits),
    // but we can test with max_count=0.
    let repo = make_repo();
    let log = get_commit_log(repo.path(), 0);
    assert!(log.is_empty());
}
