//! Wiring checks: is a completion's acceptance test proof that the change is
//! *hooked up*, or only proof that some code exists?
//!
//! The systemic failure in this repo is real, tested library code that nothing
//! ever calls. A worker adds `foo()`, adds a test that calls `foo()` directly,
//! the test passes, the bead closes, and no running game ever reaches `foo()`.
//! The 2026-10-30 port audit found 93 UNWIRED and 19 STUB features among beads
//! that had closed green. Two checks close that gap:
//!
//! 1. [`check_entry_point`] (cheap, before the build): when a bead carries a
//!    `Wiring requirement:` line, its acceptance test must live in an
//!    integration-test file (`tests/`) that drives the engine through a public
//!    entry point (a `.tscn`/project fixture, the runner, MainLoop, the editor
//!    server, the web runtime), not through a library function.
//! 2. [`red_check`] (after a green verify): copy the tree to an isolated
//!    snapshot, put the completion's NON-test files back to their HEAD
//!    contents, and re-run the acceptance test. It must FAIL there. A test that
//!    passes with the change removed does not exercise the change.
//!
//! Both checks are deliberately one-sided: anything they cannot decide
//! (unlocatable test, timeout, a file already committed by an auto-checkpoint,
//! no tests ran) is `Inconclusive` and does NOT block the bead. A false
//! rejection costs a worker a verify attempt, and ~2 failed verifies park a
//! bead, so only a clean, positive "your test passed without your change"
//! result rejects.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Marker that opts a bead into the entry-point rule. Port-audit beads and
/// planner/editor-parity beads written after 2026-10-30 carry it.
pub const WIRING_MARKER: &str = "wiring requirement:";

/// Substrings that show a test file drives the engine the way a game or the
/// editor does. Any one is enough; the list is intentionally broad because a
/// false rejection is worse than a lenient pass here — the red check is the
/// stronger gate.
pub const ENTRY_POINT_MARKERS: &[&str] = &[
    "patina_runner",
    "patina_web",
    "MainLoop",
    "main_loop",
    "from_tscn",
    ".tscn",
    "project.godot",
    "res://",
    "EditorServer",
    "editor_server",
    "EDITOR_HTML",
    "editor_html",
    "bootstrap",
    "attach_script",
    "GDScriptNodeInstance",
];

/// Whether a bead's text opts it into the entry-point rule.
pub fn requires_wiring(description: &str) -> bool {
    description.to_lowercase().contains(WIRING_MARKER)
}

/// Split a completion's free-form `files_changed` into candidate paths.
///
/// Workers write it as a newline list, a comma list, a markdown bullet list or
/// prose, so this keeps any token that looks like a path and strips the
/// decoration around it.
pub fn parse_files_changed(files_changed: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in files_changed.split(|c: char| c == '\n' || c == ',' || c.is_whitespace()) {
        let t = raw
            .trim()
            .trim_start_matches("- ")
            .trim_matches(|c: char| c == '`' || c == '"' || c == '\'' || c == '*' || c == '(' || c == ')')
            .trim_end_matches(':')
            .trim_start_matches("./");
        if t.is_empty() || t.starts_with('-') || t.contains("://") {
            continue;
        }
        let looks_like_path = t.contains('/') || t.rsplit_once('.').is_some_and(|(_, ext)| {
            !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric())
        });
        if looks_like_path && !out.iter().any(|p: &String| p == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// Whether a path is test code or a test input, which the red check keeps.
pub fn is_test_path(path: &str) -> bool {
    let p = path.replace('\\', "/");
    let file = p.rsplit('/').next().unwrap_or(&p);
    p.contains("/tests/")
        || p.starts_with("tests/")
        || p.contains("/benches/")
        || p.contains("/fixtures/")
        || p.starts_with("fixtures/")
        || p.contains("/testdata/")
        || file.ends_with("_test.rs")
        || file.ends_with("_tests.rs")
        || file == "tests.rs"
}

/// Resolve a reported path to one relative to the repo root.
///
/// Workers report paths relative to the repo root (`engine-rs/crates/..`) or
/// to the engine workspace (`crates/..`). A path that exists in neither the
/// working tree nor HEAD under either spelling is dropped.
pub fn resolve_repo_path(root: &Path, path: &str) -> Option<String> {
    let candidates = [path.to_string(), format!("engine-rs/{path}")];
    for c in &candidates {
        if root.join(c).exists() || git_head_blob(root, c).is_some() {
            return Some(c.clone());
        }
    }
    None
}

fn git_head_blob(root: &Path, rel: &str) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .args(["show", &format!("HEAD:{rel}")])
        .current_dir(root)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

// ---------------------------------------------------------------------------
// 1. Entry-point rule
// ---------------------------------------------------------------------------

/// Locate the file defining `fn <test_name>`: first among the completion's own
/// files, then across every `tests/` directory under `engine-rs`.
fn locate_test_file(root: &Path, test_name: &str, files: &[String]) -> Option<PathBuf> {
    let leaf = test_name.rsplit("::").next().unwrap_or(test_name);
    let needle = format!("fn {leaf}(");
    for f in files {
        let p = root.join(f);
        if p.extension().is_some_and(|e| e == "rs") {
            if let Ok(src) = std::fs::read_to_string(&p) {
                if src.contains(&needle) {
                    return Some(p);
                }
            }
        }
    }
    // Fall back to a grep over the engine. `-l` and a fixed string keep this
    // fast; a miss is Inconclusive, never a rejection.
    let out = Command::new("grep")
        .args(["-rlF", "--include=*.rs", &needle, "engine-rs/crates", "engine-rs/tests"])
        .current_dir(root)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let hits: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    // Prefer an integration-test location when the name is defined twice.
    hits.iter()
        .find(|h| is_test_path(h))
        .or_else(|| hits.first())
        .map(|h| root.join(h))
}

/// The test file's source, plus every helper under the nearest
/// `tests/support/` when the file uses one: group modules often boot the engine
/// through a shared helper (AGENTS.md: "Shared helpers live in
/// `tests/support/`"), so the entry point may only appear there.
fn test_source_with_support(path: &Path) -> String {
    let mut src = std::fs::read_to_string(path).unwrap_or_default();
    if !src.contains("support") {
        return src;
    }
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.file_name().is_some_and(|n| n == "tests") {
            if let Ok(entries) = std::fs::read_dir(d.join("support")) {
                for e in entries.flatten() {
                    if e.path().extension().is_some_and(|x| x == "rs") {
                        src.push_str(&std::fs::read_to_string(e.path()).unwrap_or_default());
                    }
                }
            }
            break;
        }
        dir = d.parent();
    }
    src
}

/// Outcome of the entry-point rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryPointVerdict {
    /// The bead does not opt in, or names no test.
    NotApplicable,
    Ok,
    /// Could not decide (test file not found). Does not block.
    Inconclusive(String),
    /// A rejection with a worker-facing reason.
    Rejected(String),
}

/// Apply the entry-point rule to every test the bead demands by name.
pub fn check_entry_point(
    root: &Path,
    description: &str,
    demanded_tests: &[String],
    files_changed: &str,
) -> EntryPointVerdict {
    if !requires_wiring(description) || demanded_tests.is_empty() {
        return EntryPointVerdict::NotApplicable;
    }
    let files: Vec<String> = parse_files_changed(files_changed)
        .iter()
        .filter_map(|f| resolve_repo_path(root, f))
        .collect();
    let mut inconclusive = Vec::new();
    for name in demanded_tests {
        let Some(path) = locate_test_file(root, name, &files) else {
            inconclusive.push(format!("could not locate `fn {name}`"));
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        if !is_test_path(&rel) {
            return EntryPointVerdict::Rejected(format!(
                "wiring rule: the acceptance test `{name}` is defined in `{rel}`, a library \
                 source file. This bead has a Wiring requirement, so its acceptance test must be \
                 an integration test (under a `tests/` directory) that drives the feature the way \
                 a game does, not a unit test that calls the implementation directly. Move or \
                 rewrite it under `tests/`, entering through a scene/project fixture, the runner, \
                 MainLoop, the editor server or the web runtime."
            ));
        }
        let src = test_source_with_support(&path);
        if !ENTRY_POINT_MARKERS.iter().any(|m| src.contains(m)) {
            return EntryPointVerdict::Rejected(format!(
                "wiring rule: the acceptance test `{name}` (in `{rel}`) never enters the engine \
                 through a public entry point. This bead has a Wiring requirement, so the test \
                 must load a scene or project (`.tscn`, `res://`, `project.godot`, \
                 `PackedScene::from_tscn`), boot through `patina_runner`/`MainLoop`, attach a \
                 GDScript, or drive the editor server / web runtime, and assert what a game or \
                 the editor would observe. A test that calls the new function directly proves \
                 the code exists, not that anything reaches it. Accepted markers: {}",
                ENTRY_POINT_MARKERS.join(", ")
            ));
        }
    }
    if inconclusive.is_empty() {
        EntryPointVerdict::Ok
    } else {
        EntryPointVerdict::Inconclusive(inconclusive.join("; "))
    }
}

// ---------------------------------------------------------------------------
// 2. Red check
// ---------------------------------------------------------------------------

/// Outcome of the red check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedVerdict {
    /// Nothing to revert (test-only or docs-only change) or disabled.
    Skipped(String),
    /// With the change reverted, the acceptance test failed or no longer
    /// compiled: it depends on the change.
    Detected(String),
    /// With the change reverted, the acceptance test still passed.
    NotDetected(String),
    /// Could not decide; does not block.
    Inconclusive(String),
}

/// Classify one acceptance-test run in the reverted snapshot.
pub fn classify_red_run(exit_ok: bool, output: &str) -> RedVerdict {
    let lower = output.to_lowercase();
    if exit_ok {
        return match crate::verifier::count_tests_run(output) {
            Some(0) => RedVerdict::Inconclusive("0 tests ran in the reverted snapshot".into()),
            _ => RedVerdict::NotDetected("acceptance test passed with the change reverted".into()),
        };
    }
    if lower.contains("error[e") || lower.contains("could not compile") {
        return RedVerdict::Detected("does not compile without the change".into());
    }
    // A test binary that dies (stack overflow, abort, segfault) without the
    // change also proves the test depends on it.
    if lower.contains("test result: failed")
        || lower.contains("panicked at")
        || lower.contains("fatal runtime error")
        || lower.contains("sigabrt")
        || lower.contains("sigsegv")
        || lower.contains("error: test failed")
        || output.contains("ABORT [")
        || output.contains("SIGSEGV [")
        || lower.contains("tests failed")
        || output.contains("FAIL [")
    {
        return RedVerdict::Detected("fails without the change".into());
    }
    RedVerdict::Inconclusive(format!(
        "unrecognised failure in the reverted snapshot: {}",
        crate::verifier::tail_chars(output, 300)
    ))
}

/// Directories never copied into the snapshot: build outputs, VCS metadata and
/// package caches. The snapshot builds into its own target dir.
const SNAPSHOT_EXCLUDES: &[&str] = &[
    ".git",
    "target",
    "target-*",
    "node_modules",
    ".next",
    ".orchestrator/redcheck",
    "*.trash-*",
    ".shadertest-target",
    // Read-only reference source (vendored Godot). Symlinked into the
    // snapshot instead of copied: tests may read its doc XML, nothing edits it.
    "/upstream",
];

fn sync_snapshot(root: &Path, snap: &Path) -> Result<(), String> {
    std::fs::create_dir_all(snap).map_err(|e| format!("mkdir snapshot: {e}"))?;
    let mut cmd = Command::new("rsync");
    cmd.arg("-a").arg("--delete");
    for ex in SNAPSHOT_EXCLUDES {
        cmd.arg(format!("--exclude={ex}"));
    }
    cmd.arg(format!("{}/", root.display()))
        .arg(format!("{}/", snap.display()));
    let out = cmd.output().map_err(|e| format!("rsync spawn: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "rsync failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let src_upstream = root.join("upstream");
    let snap_upstream = snap.join("upstream");
    if src_upstream.is_dir() {
        // A real directory here is a copy from before upstream/ was excluded.
        if snap_upstream.is_dir() && !snap_upstream.is_symlink() {
            let _ = std::fs::remove_dir_all(&snap_upstream);
        }
        if !snap_upstream.exists() {
            std::os::unix::fs::symlink(&src_upstream, &snap_upstream)
                .map_err(|e| format!("symlink upstream: {e}"))?;
        }
    }
    Ok(())
}

/// How far back an auto-checkpoint counts as possibly holding a bead's WIP.
const CHECKPOINT_SWEEP_WINDOW_SECS: i64 = 48 * 3600;

/// Whether the last commit touching `rel` is an orchestrator auto-checkpoint
/// made within [`CHECKPOINT_SWEEP_WINDOW_SECS`].
fn recently_swept_by_checkpoint(root: &Path, rel: &str) -> bool {
    let Ok(out) = Command::new("git")
        .args(["log", "-1", "--format=%ct%x09%s", "--", rel])
        .current_dir(root)
        .output()
    else {
        return false;
    };
    let line = String::from_utf8_lossy(&out.stdout);
    let Some((ts, subject)) = line.trim().split_once('\t') else {
        return false;
    };
    let Ok(ts) = ts.parse::<i64>() else {
        return false;
    };
    let now = chrono::Utc::now().timestamp();
    subject.starts_with("Checkpoint: auto") && now - ts < CHECKPOINT_SWEEP_WINDOW_SECS
}

/// Turn a verify command into one that runs in the snapshot's engine
/// workspace, off the shared build slot, with the red-check target dir.
pub fn snapshot_command(cmd: &str, target_dir: &Path) -> Option<String> {
    let t = cmd.trim();
    let args = if let Some(rest) = t.strip_prefix("cargo ") {
        rest
    } else if let Some(idx) = t.find("rust_task.sh ") {
        &t[idx + "rust_task.sh ".len()..]
    } else {
        return None;
    };
    Some(format!(
        "CARGO_TARGET_DIR='{}' nice -n 19 cargo {args}",
        target_dir.display()
    ))
}

/// Revert the completion's non-test files inside `snap` and re-run the
/// acceptance commands there.
///
/// Only beads that name an acceptance test (`acceptance_tests`, from the
/// `(test: \`name\`)` form) are checked. A refactor, perf or test-coverage
/// bead whose acceptance is "existing tests still pass" legitimately passes
/// with its change reverted and must not be rejected for it. When some
/// commands mention a named test, only those are re-run; otherwise (a command
/// names the binary, the bead names the function) every test command is.
pub fn red_check(
    root: &Path,
    files_changed: &str,
    commands: &[String],
    acceptance_tests: &[String],
    timeout: Duration,
) -> RedVerdict {
    if std::env::var("ORCH_RED_CHECK").is_ok_and(|v| v == "0") {
        return RedVerdict::Skipped("disabled by ORCH_RED_CHECK=0".into());
    }
    if acceptance_tests.is_empty() {
        return RedVerdict::Skipped("bead names no acceptance test".into());
    }
    let files: Vec<String> = parse_files_changed(files_changed)
        .iter()
        .filter_map(|f| resolve_repo_path(root, f))
        .collect();
    let code: Vec<&String> = files
        .iter()
        .filter(|f| !is_test_path(f))
        // Everything but prose and tracker state: logic also lives in .wgsl,
        // .gd, .tscn, .json, .js and similar, and leaving those in place would
        // keep the feature in the "reverted" snapshot.
        .filter(|f| {
            let l = f.to_lowercase();
            !(l.ends_with(".md")
                || l.ends_with(".txt")
                || l.ends_with(".jsonl")
                || l.starts_with(".beads/")
                || l.starts_with("docs/")
                || l.starts_with("prd/"))
        })
        .collect();
    if code.is_empty() {
        return RedVerdict::Skipped("no non-test code files to revert".into());
    }
    // Re-run exactly what the green verify ran: apply the verifier's own
    // broad-command rewrite (an unscoped `--workspace` / `-E` run becomes a
    // compile check there), then keep the commands that still run tests.
    // `--test <group> -E 'test(/^module::/)'`, the consolidated-binary form
    // AGENTS.md prescribes, survives the rewrite and is re-run.
    let runnable: Vec<String> = commands
        .iter()
        .map(|c| crate::verifier::rewrite_broad_command(c))
        .filter(|c| crate::verifier::is_test_run_command(c))
        .filter(|c| !c.to_lowercase().contains("--workspace"))
        .collect();
    let naming: Vec<String> = runnable
        .iter()
        .filter(|c| acceptance_tests.iter().any(|t| crate::verifier::command_mentions_test(c, t)))
        .cloned()
        .collect();
    let runnable = if naming.is_empty() { runnable } else { naming };
    if runnable.is_empty() {
        return RedVerdict::Skipped("no test-run command to re-run".into());
    }

    // A file whose working copy already equals HEAD cannot be reverted: an
    // auto-checkpoint committed it before this verify ran. Reverting the rest
    // would test a half-removed change, so bail out instead of guessing.
    let mut already_committed = Vec::new();
    for f in &code {
        let live = std::fs::read(root.join(f)).ok();
        let head = git_head_blob(root, f);
        if live.is_some() && live == head {
            already_committed.push(f.as_str());
        }
    }
    if !already_committed.is_empty() {
        return RedVerdict::Inconclusive(format!(
            "already committed at HEAD, cannot revert: {}",
            already_committed.join(", ")
        ));
    }
    // Partly committed: an auto-checkpoint swept an EARLIER version of this
    // WIP into HEAD and the worker kept editing. Reverting to HEAD would only
    // remove the last delta and leave most of the feature in place, so a
    // pass there would be a false rejection.
    let swept: Vec<&str> = code
        .iter()
        .filter(|f| recently_swept_by_checkpoint(root, f))
        .map(|f| f.as_str())
        .collect();
    if !swept.is_empty() {
        return RedVerdict::Inconclusive(format!(
            "touched by a recent auto-checkpoint, HEAD may hold part of this change: {}",
            swept.join(", ")
        ));
    }

    let snap = root.join(".orchestrator/redcheck/tree");
    let target = root.join("engine-rs/target-redcheck");
    if let Err(e) = sync_snapshot(root, &snap) {
        return RedVerdict::Inconclusive(e);
    }
    for f in &code {
        let dest = snap.join(f);
        match git_head_blob(root, f) {
            Some(bytes) => {
                if let Err(e) = std::fs::write(&dest, bytes) {
                    return RedVerdict::Inconclusive(format!("revert {f}: {e}"));
                }
            }
            // New in this change: it does not exist without the change.
            None => {
                let _ = std::fs::remove_file(&dest);
            }
        }
    }

    let workdir = snap.join("engine-rs");
    let mut inconclusive = Vec::new();
    let mut not_detected = Vec::new();
    for cmd in &runnable {
        let Some(snap_cmd) = snapshot_command(cmd, &target) else {
            inconclusive.push(format!("not a cargo command: {cmd}"));
            continue;
        };
        tracing::info!("[red-check] running in {}: {}", workdir.display(), snap_cmd);
        match crate::verifier::run_command_capture(&workdir, &snap_cmd, timeout) {
            Ok(Some((status, output))) => match classify_red_run(status.success(), &output) {
                RedVerdict::Detected(why) => return RedVerdict::Detected(format!("{why}: {cmd}")),
                RedVerdict::NotDetected(_) => not_detected.push(cmd.clone()),
                RedVerdict::Inconclusive(why) => inconclusive.push(why),
                RedVerdict::Skipped(why) => inconclusive.push(why),
            },
            Ok(None) => inconclusive.push(format!("timed out after {}s", timeout.as_secs())),
            Err(e) => inconclusive.push(format!("spawn failed: {e}")),
        }
    }
    if !not_detected.is_empty() && inconclusive.is_empty() {
        let reverted = code.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        return RedVerdict::NotDetected(format!(
            "red check: your acceptance test still PASSES with your non-test changes reverted \
             to HEAD ({reverted}), so it does not exercise your change. A test that passes \
             without the fix proves nothing about it. Make the test drive the feature through \
             the path a game or the editor uses, so it fails on HEAD and passes with your \
             change. Commands that passed on reverted code: {}",
            not_detected.join(" ; ")
        ));
    }
    RedVerdict::Inconclusive(inconclusive.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_newline_comma_bullet_and_backtick_lists() {
        let got = parse_files_changed(
            "- `engine-rs/crates/a/src/x.rs`\nengine-rs/crates/b/src/y.rs, crates/c/tests/z.rs\n(see notes)",
        );
        assert_eq!(
            got,
            vec![
                "engine-rs/crates/a/src/x.rs",
                "engine-rs/crates/b/src/y.rs",
                "crates/c/tests/z.rs"
            ]
        );
    }

    #[test]
    fn parse_skips_urls_flags_and_words() {
        let got = parse_files_changed("see https://x.y/z --release and also the fix");
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn test_paths_are_kept_and_code_paths_are_not() {
        assert!(is_test_path("engine-rs/crates/gdscene/tests/foo.rs"));
        assert!(is_test_path("engine-rs/tests/physics_suite/x_test.rs"));
        assert!(is_test_path("engine-rs/fixtures/scenes/a.tscn"));
        assert!(is_test_path("engine-rs/crates/gdscene/src/tests.rs"));
        assert!(!is_test_path("engine-rs/crates/gdscene/src/scene_tree.rs"));
        // "tests" as a substring of a name is not a test directory.
        assert!(!is_test_path("engine-rs/crates/gdscene/src/contests.rs"));
    }

    #[test]
    fn wiring_marker_is_case_insensitive() {
        assert!(requires_wiring("**Wiring requirement:** runner -> x"));
        assert!(!requires_wiring("Acceptance: cargo test"));
    }

    #[test]
    fn red_run_pass_is_not_detected() {
        let out = "test result: ok. 1 passed; 0 failed";
        assert!(matches!(classify_red_run(true, out), RedVerdict::NotDetected(_)));
    }

    #[test]
    fn red_run_zero_tests_is_inconclusive_not_a_rejection() {
        let out = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out";
        assert!(matches!(classify_red_run(true, out), RedVerdict::Inconclusive(_)));
    }

    #[test]
    fn red_run_failure_and_compile_error_are_detected() {
        assert!(matches!(
            classify_red_run(false, "thread 'x' panicked at src/a.rs:1\ntest result: FAILED."),
            RedVerdict::Detected(_)
        ));
        assert!(matches!(
            classify_red_run(false, "error[E0425]: cannot find function `foo`"),
            RedVerdict::Detected(_)
        ));
        assert!(matches!(
            classify_red_run(false, "        FAIL [   0.010s] gdscene::foo bar"),
            RedVerdict::Detected(_)
        ));
    }

    #[test]
    fn red_run_abort_is_detected() {
        let out = "thread 'x' has overflowed its stack\nfatal runtime error: stack overflow, aborting\nerror: test failed, to rerun pass `--lib`\nprocess didn't exit successfully (signal: 6, SIGABRT: process abort signal)";
        assert!(matches!(classify_red_run(false, out), RedVerdict::Detected(_)));
    }

    #[test]
    fn red_run_unknown_failure_is_inconclusive() {
        assert!(matches!(
            classify_red_run(false, "error: no such command: `nextest`"),
            RedVerdict::Inconclusive(_)
        ));
    }

    #[test]
    fn snapshot_command_unwraps_rust_task_and_sets_target_dir() {
        let t = Path::new("/r/engine-rs/target-redcheck");
        assert_eq!(
            snapshot_command("cargo nextest run -p gdscene --test foo", t).unwrap(),
            "CARGO_TARGET_DIR='/r/engine-rs/target-redcheck' nice -n 19 cargo nextest run -p gdscene --test foo"
        );
        assert_eq!(
            snapshot_command("./scripts/rust_task.sh test -p gdscene --test foo", t).unwrap(),
            "CARGO_TARGET_DIR='/r/engine-rs/target-redcheck' nice -n 19 cargo test -p gdscene --test foo"
        );
        assert!(snapshot_command("pnpm test", t).is_none());
    }

    /// A git repo with one committed library file and one test, used by the
    /// end-to-end red-check and entry-point tests below.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        let sh = |c: &str| {
            let ok = Command::new("bash").args(["-c", c]).current_dir(r).status().unwrap();
            assert!(ok.success(), "{c}");
        };
        sh("git init -q && git config user.email t@t && git config user.name t");
        std::fs::create_dir_all(r.join("engine-rs/crates/a/src")).unwrap();
        std::fs::create_dir_all(r.join("engine-rs/crates/a/tests")).unwrap();
        std::fs::write(r.join("engine-rs/crates/a/src/lib.rs"), "pub fn v() -> i32 { 1 }\n").unwrap();
        sh("git add -A && git commit -qm init");
        dir
    }

    #[test]
    fn entry_point_rule_rejects_a_direct_library_test_and_accepts_a_scene_test() {
        let dir = repo();
        let r = dir.path();
        let desc = "**Wiring requirement:** tscn -> runner";
        std::fs::write(
            r.join("engine-rs/crates/a/tests/direct.rs"),
            "#[test]\nfn direct_call() { assert_eq!(a::v(), 2); }\n",
        )
        .unwrap();
        let v = check_entry_point(r, desc, &["direct_call".into()], "engine-rs/crates/a/tests/direct.rs");
        assert!(matches!(v, EntryPointVerdict::Rejected(ref m) if m.contains("public entry point")), "{v:?}");

        std::fs::write(
            r.join("engine-rs/crates/a/tests/scene.rs"),
            "#[test]\nfn via_scene() { let _s = PackedScene::from_tscn(\"[gd_scene]\"); }\n",
        )
        .unwrap();
        let v = check_entry_point(r, desc, &["via_scene".into()], "engine-rs/crates/a/tests/scene.rs");
        assert_eq!(v, EntryPointVerdict::Ok);
    }

    #[test]
    fn entry_point_rule_sees_a_marker_in_a_tests_support_helper() {
        let dir = repo();
        let r = dir.path();
        std::fs::create_dir_all(r.join("engine-rs/tests/group_suite")).unwrap();
        std::fs::create_dir_all(r.join("engine-rs/tests/support")).unwrap();
        std::fs::write(
            r.join("engine-rs/tests/support/boot.rs"),
            "pub fn boot() { let _ = \"res://main.tscn\"; }\n",
        )
        .unwrap();
        std::fs::write(
            r.join("engine-rs/tests/group_suite/m.rs"),
            "use crate::support::boot;\n#[test]\nfn via_helper() { boot(); }\n",
        )
        .unwrap();
        let v = check_entry_point(
            r,
            "Wiring requirement: x",
            &["via_helper".into()],
            "engine-rs/tests/group_suite/m.rs",
        );
        assert_eq!(v, EntryPointVerdict::Ok);
    }

    #[test]
    fn red_check_keeps_grouped_binary_filters_and_drops_workspace_runs() {
        // A grouped `--test <group> -E` run survives the verifier rewrite;
        // `--workspace -E` becomes a compile check and is not re-run.
        let keep = crate::verifier::rewrite_broad_command(
            "cargo nextest run -p patina-engine --test physics_suite -E 'test(/^m::/)'",
        );
        assert!(crate::verifier::is_test_run_command(&keep) && keep.contains("--test physics_suite"));
        let drop = crate::verifier::rewrite_broad_command("cargo nextest run --workspace -E 'test(x)'");
        assert!(!crate::verifier::is_test_run_command(&drop), "{drop}");
    }

    #[test]
    fn entry_point_rule_rejects_a_unit_test_in_src() {
        let dir = repo();
        let r = dir.path();
        std::fs::write(
            r.join("engine-rs/crates/a/src/lib.rs"),
            "pub fn v() -> i32 { 2 }\n#[cfg(test)] mod t { #[test] fn in_src() { let _ = \"x.tscn\"; } }\n",
        )
        .unwrap();
        let v = check_entry_point(
            r,
            "Wiring requirement: x",
            &["in_src".into()],
            "engine-rs/crates/a/src/lib.rs",
        );
        assert!(matches!(v, EntryPointVerdict::Rejected(ref m) if m.contains("library source file")), "{v:?}");
    }

    #[test]
    fn entry_point_rule_only_applies_to_opted_in_beads_and_never_rejects_on_a_miss() {
        let dir = repo();
        let r = dir.path();
        assert_eq!(
            check_entry_point(r, "plain bead", &["x".into()], ""),
            EntryPointVerdict::NotApplicable
        );
        assert!(matches!(
            check_entry_point(r, "Wiring requirement: x", &["nowhere_to_be_found".into()], ""),
            EntryPointVerdict::Inconclusive(_)
        ));
    }

    #[test]
    fn red_check_skips_test_only_changes() {
        let dir = repo();
        let r = dir.path();
        std::fs::write(r.join("engine-rs/crates/a/tests/t.rs"), "#[test] fn t() {}\n").unwrap();
        let v = red_check(r, "engine-rs/crates/a/tests/t.rs", &["cargo test --test t".into()], &["t".into()], Duration::from_secs(5));
        assert!(matches!(v, RedVerdict::Skipped(_)), "{v:?}");
    }

    #[test]
    fn red_check_is_inconclusive_when_the_change_is_already_committed() {
        let dir = repo();
        let r = dir.path();
        // lib.rs is reported as changed but equals HEAD: an auto-checkpoint
        // swept it in. Reverting it would be a no-op, so the check must not
        // claim the test is blind.
        let v = red_check(
            r,
            "engine-rs/crates/a/src/lib.rs",
            &["cargo test --test t".into()],
            &["t".into()],
            Duration::from_secs(5),
        );
        assert!(matches!(v, RedVerdict::Inconclusive(ref m) if m.contains("already committed")), "{v:?}");
    }

    #[test]
    fn red_check_skips_beads_that_name_no_acceptance_test() {
        let dir = repo();
        let r = dir.path();
        std::fs::write(r.join("engine-rs/crates/a/src/lib.rs"), "pub fn v() -> i32 { 1 } // refactor\n").unwrap();
        let v = red_check(r, "engine-rs/crates/a/src/lib.rs", &["cargo test -p a".into()], &[], Duration::from_secs(5));
        assert!(matches!(v, RedVerdict::Skipped(ref m) if m.contains("no acceptance test")), "{v:?}");
    }

    #[test]
    fn red_check_is_inconclusive_when_a_checkpoint_swept_part_of_the_change() {
        let dir = repo();
        let r = dir.path();
        let sh = |c: &str| {
            assert!(Command::new("bash").args(["-c", c]).current_dir(r).status().unwrap().success());
        };
        std::fs::write(r.join("engine-rs/crates/a/src/lib.rs"), "pub fn v() -> i32 { 2 }\n").unwrap();
        sh("git add -A && git commit -qm 'Checkpoint: auto after 10 closes (orchestrator)'");
        // The worker's follow-up edit after the sweep.
        std::fs::write(r.join("engine-rs/crates/a/src/lib.rs"), "pub fn v() -> i32 { 2 } // tidy\n").unwrap();
        let v = red_check(r, "engine-rs/crates/a/src/lib.rs", &["cargo test -p a --test t".into()], &["t".into()], Duration::from_secs(5));
        assert!(matches!(v, RedVerdict::Inconclusive(ref m) if m.contains("auto-checkpoint")), "{v:?}");
    }

    /// End to end with a real cargo workspace: a test that reads the changed
    /// function is Detected; a test that ignores it is NotDetected.
    #[test]
    fn red_check_end_to_end_with_cargo() {
        if Command::new("cargo").arg("--version").output().is_err() {
            return;
        }
        let dir = repo();
        let r = dir.path();
        std::fs::write(
            r.join("engine-rs/Cargo.toml"),
            "[workspace]\nmembers=[\"crates/a\"]\nresolver=\"2\"\n",
        )
        .unwrap();
        std::fs::write(
            r.join("engine-rs/crates/a/Cargo.toml"),
            "[package]\nname=\"a\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let sh = |c: &str| {
            assert!(Command::new("bash").args(["-c", c]).current_dir(r).status().unwrap().success());
        };
        sh("git add -A && git commit -qm ws");
        // The change: v() now returns 2.
        std::fs::write(r.join("engine-rs/crates/a/src/lib.rs"), "pub fn v() -> i32 { 2 }\n").unwrap();
        std::fs::write(
            r.join("engine-rs/crates/a/tests/sees.rs"),
            "#[test]\nfn sees_change() { assert_eq!(a::v(), 2); }\n",
        )
        .unwrap();
        std::fs::write(
            r.join("engine-rs/crates/a/tests/blind.rs"),
            "#[test]\nfn blind() { assert_eq!(1 + 1, 2); }\n",
        )
        .unwrap();
        let files = "engine-rs/crates/a/src/lib.rs\nengine-rs/crates/a/tests/sees.rs\nengine-rs/crates/a/tests/blind.rs";
        let t = Duration::from_secs(300);

        let v = red_check(r, files, &["cargo test -p a --test sees".into()], &["sees_change".into()], t);
        assert!(matches!(v, RedVerdict::Detected(_)), "{v:?}");

        let v = red_check(r, files, &["cargo test -p a --test blind".into()], &["blind".into()], t);
        assert!(matches!(v, RedVerdict::NotDetected(ref m) if m.contains("still PASSES")), "{v:?}");

        // The live tree is untouched by the check.
        assert_eq!(
            std::fs::read_to_string(r.join("engine-rs/crates/a/src/lib.rs")).unwrap(),
            "pub fn v() -> i32 { 2 }\n"
        );
    }
}
