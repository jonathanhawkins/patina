//! pat-sadi6: Acceptance gate for atomic scene save (temp-write + rename).
//!
//! Acceptance: a SIGKILL injected mid-save leaves either the previous scene
//! or the new scene on disk and never a partial or corrupt file, across 100
//! iterations.
//!
//! Strategy: re-spawn the current test binary as a child running the ignored
//! helper `__atomic_save_child`, which calls `gdeditor::editor_server::atomic_write`
//! after an env-controlled delay. The parent SIGKILLs the child at varying
//! delays so the kill races every phase of write/sync/rename. After each
//! kill we read the target path and require it to be byte-identical to
//! either the seeded "old" content or the requested "new" content.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use gdeditor::editor_server::atomic_write;

const OLD_CONTENT: &str = "[gd_scene format=3]\n[node name=\"Old\" type=\"Node\"]\n";
const NEW_CONTENT: &str = "[gd_scene format=3]\n[node name=\"New\" type=\"Node2D\"]\nposition = Vector2(1, 2)\n";

const ENV_PATH: &str = "PATINA_ATOMIC_SAVE_PATH";
const ENV_CONTENT: &str = "PATINA_ATOMIC_SAVE_CONTENT";
const ENV_DELAY_NS: &str = "PATINA_ATOMIC_SAVE_DELAY_NS";

/// Child entry point invoked by the parent test via `current_exe()` with
/// `--ignored --exact __atomic_save_child`. Reads the target path and the
/// new content from the environment, optionally sleeps so the kill can
/// land mid-write, then performs the atomic write.
#[test]
#[ignore]
fn __atomic_save_child() {
    let path = std::env::var(ENV_PATH).expect("PATINA_ATOMIC_SAVE_PATH must be set");
    let content = std::env::var(ENV_CONTENT).expect("PATINA_ATOMIC_SAVE_CONTENT must be set");
    if let Ok(delay_ns) = std::env::var(ENV_DELAY_NS) {
        if let Ok(ns) = delay_ns.parse::<u64>() {
            if ns > 0 {
                std::thread::sleep(Duration::from_nanos(ns));
            }
        }
    }
    atomic_write(Path::new(&path), content.as_bytes()).expect("atomic_write succeeds");
}

#[test]
fn atomic_write_replaces_target_atomically_without_kill() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("scene.tscn");
    std::fs::write(&path, OLD_CONTENT).expect("seed OLD");

    atomic_write(&path, NEW_CONTENT.as_bytes()).expect("atomic_write succeeds");
    let observed = std::fs::read_to_string(&path).expect("read target");
    assert_eq!(observed, NEW_CONTENT);

    // The atomic write must not leave any sibling temp files behind on success.
    let leftovers: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains(".atomic.") && n.ends_with(".tmp"))
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "atomic_write leaked temp files: {leftovers:?}"
    );
}

#[test]
fn atomic_write_preserves_old_content_when_write_fails() {
    // If the temp write fails (parent dir missing), the target must be
    // untouched, not partially overwritten.
    let dir = tempfile::tempdir().expect("tempdir");
    let bad_path = dir.path().join("does-not-exist").join("scene.tscn");
    let err = atomic_write(&bad_path, NEW_CONTENT.as_bytes()).unwrap_err();
    assert!(
        err.kind() == std::io::ErrorKind::NotFound
            || err.kind() == std::io::ErrorKind::Other,
        "expected NotFound for missing parent, got {err:?}"
    );

    // Seeded sibling target must not be disturbed by a failure in another path.
    let seeded = dir.path().join("scene.tscn");
    std::fs::write(&seeded, OLD_CONTENT).unwrap();
    let _ = atomic_write(&bad_path, NEW_CONTENT.as_bytes());
    assert_eq!(std::fs::read_to_string(&seeded).unwrap(), OLD_CONTENT);
}

/// Core acceptance: 100 iterations, each spawning a child that performs an
/// atomic_write of NEW_CONTENT over a seeded OLD_CONTENT target. The parent
/// SIGKILLs the child at a varying delay so the kill races all phases of
/// the save (open, write, fsync, rename). The target file must be byte-equal
/// to OLD_CONTENT or NEW_CONTENT after every iteration — never partial.
#[test]
fn atomic_save_under_sigkill_keeps_old_or_new() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("scene.tscn");
    std::fs::write(&path, OLD_CONTENT).expect("seed OLD");

    let test_bin = std::env::current_exe().expect("current_exe");

    for iteration in 0..100u32 {
        // Spread the in-child delay across 0..2ms so SIGKILL can land before
        // the write starts, between write and fsync, between fsync and rename,
        // and after the rename.
        let delay_ns = (iteration as u64 * 19_991) % 2_000_000;

        let mut child = Command::new(&test_bin)
            .arg("--ignored")
            .arg("--exact")
            .arg("--nocapture")
            .arg("__atomic_save_child")
            .env(ENV_PATH, &path)
            .env(ENV_CONTENT, NEW_CONTENT)
            .env(ENV_DELAY_NS, delay_ns.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn atomic_save child");

        // Race the kill against the child's internal delay so we hit
        // every phase of the save over the 100 iterations.
        let parent_wait_us = (iteration as u64 * 23) % 1_500;
        if parent_wait_us > 0 {
            std::thread::sleep(Duration::from_micros(parent_wait_us));
        }
        // std::process::Child::kill() sends SIGKILL on Unix.
        let _ = child.kill();
        let _ = child.wait();

        let observed = std::fs::read(&path).expect("read scene after kill");
        let observed_str = String::from_utf8_lossy(&observed);
        assert!(
            observed_str == OLD_CONTENT || observed_str == NEW_CONTENT,
            "iteration {iteration}: target must be exactly OLD or NEW after SIGKILL, \
             never partial; got {} bytes: {:?}",
            observed.len(),
            observed_str
        );

        // If this iteration committed the new content, reseed OLD so the next
        // iteration still has a well-defined "previous scene" to fall back to.
        if observed_str == NEW_CONTENT {
            std::fs::write(&path, OLD_CONTENT).expect("reseed OLD between iterations");
        }
    }

    // Final sanity check: the target ends in a known-good state.
    let final_observed = std::fs::read_to_string(&path).expect("read final scene");
    assert!(final_observed == OLD_CONTENT || final_observed == NEW_CONTENT);
}
