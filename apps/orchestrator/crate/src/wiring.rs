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
//!    snapshot, put the completion's NON-test files back to their contents
//!    from when the bead was assigned (HEAD when no assignment was recorded),
//!    and re-run the acceptance test. It must FAIL there. A test that
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
/// stronger gate. Matched against code with comments stripped, so a doc
/// comment that merely mentions a scene does not count.
///
/// `project.godot` is deliberately absent: a data-layer test that writes and
/// re-parses a project file names it without entering the engine
/// (pat-mgixk.16). A test that boots a project does so through one of the
/// entry points below.
pub const ENTRY_POINT_MARKERS: &[&str] = &[
    "patina_runner",
    "patina_web",
    "MainLoop",
    "main_loop",
    "from_tscn",
    ".tscn",
    "PackedScene",
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
    git_blob(root, "HEAD", rel)
}

fn git_blob(root: &Path, rev: &str, rel: &str) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .args(["show", &format!("{rev}:{rel}")])
        .current_dir(root)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// Rust source with `//` and `/* */` comments removed and string, raw-string
/// and char literals kept verbatim, so `"res://x.tscn"` survives while a doc
/// comment that only mentions a scene does not.
pub fn strip_rust_comments(src: &str) -> String {
    let b = src.as_bytes();
    let n = b.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;
    let mut kept = 0;
    let flush = |out: &mut String, from: usize, to: usize| out.push_str(&src[from..to]);
    while i < n {
        if b[i] == b'/' && i + 1 < n && b[i + 1] == b'/' {
            flush(&mut out, kept, i);
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            kept = i;
        } else if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
            flush(&mut out, kept, i);
            let mut depth = 0usize;
            while i < n {
                if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if b[i] == b'*' && i + 1 < n && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
            kept = i;
        } else if b[i] == b'r' && (i + 1 < n && (b[i + 1] == b'"' || b[i + 1] == b'#'))
            && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
        {
            // Raw string r"..." / r#"..."#; `r#ident` (raw identifier) has no quote.
            let mut j = i + 1;
            while j < n && b[j] == b'#' {
                j += 1;
            }
            if j < n && b[j] == b'"' {
                let close = format!("\"{}", "#".repeat(j - i - 1));
                i = match src[j + 1..].find(&close) {
                    Some(off) => j + 1 + off + close.len(),
                    None => n,
                };
            } else {
                i = j;
            }
        } else if b[i] == b'"' {
            i += 1;
            while i < n && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(n);
        } else if b[i] == b'\'' {
            // A char literal ('x', '\n', '\'') or a lifetime ('a). Only skip
            // the former, so a quote inside it cannot open a string.
            if i + 2 < n && b[i + 1] == b'\\' {
                let mut k = i + 2;
                while k < n && b[k] != b'\'' {
                    k += 1;
                }
                i = (k + 1).min(n);
            } else if let Some(c) = src[i + 1..].chars().next() {
                let after = i + 1 + c.len_utf8();
                i = if after < n && b[after] == b'\'' { after + 1 } else { i + 1 };
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    flush(&mut out, kept, n);
    out
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
        let src = strip_rust_comments(&test_source_with_support(&path));
        if !ENTRY_POINT_MARKERS.iter().any(|m| src.contains(m)) {
            return EntryPointVerdict::Rejected(format!(
                "wiring rule: the acceptance test `{name}` (in `{rel}`) never enters the engine \
                 through a public entry point. This bead has a Wiring requirement, so the test \
                 must load a scene (`.tscn`, `res://`, `PackedScene::from_tscn`), boot \
                 through `patina_runner`/`MainLoop`, attach a GDScript, or drive the editor \
                 server (`EditorServerHandle::start` + HTTP) / web runtime, and assert what a \
                 game or the editor would observe. A test that calls the new function directly \
                 proves the code exists, not that anything reaches it. Markers are matched in \
                 code only, not in comments; writing or parsing a `project.godot` file by \
                 itself does not count. Accepted markers: {}",
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
    /// With the change reverted, the acceptance test FAILED (an assertion,
    /// panic or crash): it depends on the change's behaviour.
    Detected(String),
    /// With the change reverted, the acceptance test no longer COMPILED. That
    /// proves only that the test names a symbol the change added, not that any
    /// assertion depends on the new behaviour, so it is not a pass: a test that
    /// calls a new function and asserts nothing about it lands here too.
    /// Does not block (a new API legitimately breaks the build without it), but
    /// is logged as its own verdict rather than as `Detected`.
    CompileOnly(String),
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
        return RedVerdict::CompileOnly(
            "does not compile without the change — proves the test uses the change's API, \
             not that an assertion depends on its behaviour"
                .into(),
        );
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
    // Tracker state. Its SQLite WAL/SHM files come and go while the coordinator
    // writes, and rsync fails on a file that vanishes mid-copy.
    "/.beads",
    // Agent Mail's store: lock files there appear and vanish every send.
    "/.agent-mail-store",
    // Read-only reference source (vendored Godot). Symlinked into the
    // snapshot instead of copied: tests may read its doc XML, nothing edits it.
    "/upstream",
];

/// Whether an rsync failure is only files that vanished during the copy.
fn only_vanished_files(code: Option<i32>, stderr: &str) -> bool {
    match code {
        Some(24) => true,
        Some(23) => stderr
            .lines()
            .filter(|l| l.contains("error:") || l.contains("error in"))
            .all(|l| l.contains("No such file or directory") || l.contains("some files vanished") || l.contains("some files/attrs were not transferred")),
        _ => false,
    }
}

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
    // A live tree always has churn (lock files, editor swap files), and a
    // file that vanishes mid-copy is not a test input. rsync reports that as
    // 24, or as 23 with "No such file or directory" when it disappears between
    // the scan and the open.
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && !only_vanished_files(out.status.code(), &stderr) {
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

/// Where the commit HEAD pointed at when each bead was assigned is kept.
fn assignment_base_path(cache_dir: &Path, bead_id: &str) -> PathBuf {
    let safe: String = bead_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    cache_dir.join("assign-base").join(safe)
}

/// Record HEAD as the red-check baseline for `bead_id`, at assignment time.
///
/// An auto-checkpoint commits the whole tree every N closes, so by the time a
/// bead is verified HEAD often already holds part of its change, and reverting
/// to HEAD removes only the last delta. The commit HEAD pointed at when the
/// bead was handed out predates all of the worker's edits.
///
/// Re-sending the same bead to the same worker (a reprompt) keeps the first
/// record; a different worker starts a fresh one.
pub fn record_assignment_base(root: &Path, cache_dir: &Path, bead_id: &str, worker: &str) {
    let path = assignment_base_path(cache_dir, bead_id);
    if let Ok(prev) = std::fs::read_to_string(&path) {
        if prev.trim_end().split('\t').nth(1) == Some(worker) {
            return;
        }
    }
    let Ok(out) = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output() else {
        return;
    };
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || sha.is_empty() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, format!("{sha}\t{worker}\n"));
}

/// The recorded baseline commit for `bead_id`, if any and if it still exists.
pub fn assignment_base(root: &Path, cache_dir: &Path, bead_id: &str) -> Option<String> {
    let rec = std::fs::read_to_string(assignment_base_path(cache_dir, bead_id)).ok()?;
    let sha = rec.split('\t').next()?.trim().to_string();
    let ok = Command::new("git")
        .args(["cat-file", "-e", &format!("{sha}^{{commit}}")])
        .current_dir(root)
        .status()
        .ok()?
        .success();
    ok.then_some(sha)
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
/// Files go back to their contents at `baseline` (the commit recorded when the
/// bead was assigned, see [`record_assignment_base`]); with no baseline they go
/// back to HEAD, guarded against auto-checkpoint sweeps. Reverting to an older
/// commit can also undo a peer's change to the same file; if that breaks the
/// build the result is `Detected`, which never rejects correct work.
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
    baseline: Option<&str>,
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
    let mut code: Vec<&String> = files
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

    if let Some(base) = baseline {
        // A listed file identical to its assignment-time contents is not part
        // of this change; there is nothing to revert in it.
        code.retain(|f| std::fs::read(root.join(f)).ok() != git_blob(root, base, f));
        if code.is_empty() {
            return RedVerdict::Inconclusive(format!(
                "no listed code file differs from the assignment baseline {base}"
            ));
        }
    }
    // Without a baseline, HEAD is the reference and two guards apply.
    // A file whose working copy already equals HEAD cannot be reverted: an
    // auto-checkpoint committed it before this verify ran. Reverting the rest
    // would test a half-removed change, so bail out instead of guessing.
    let mut already_committed = Vec::new();
    let guard: &[&String] = if baseline.is_some() { &[] } else { &code };
    for f in guard {
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
    let swept: Vec<&str> = guard
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
    let reference = baseline.unwrap_or("HEAD");
    for f in &code {
        let dest = snap.join(f);
        match git_blob(root, reference, f) {
            Some(bytes) => {
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
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
    let mut compile_only = Vec::new();
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
                // Keep looking: another command may fail on behaviour.
                RedVerdict::CompileOnly(_) => compile_only.push(cmd.clone()),
                RedVerdict::Inconclusive(why) => inconclusive.push(why),
                RedVerdict::Skipped(why) => inconclusive.push(why),
            },
            Ok(None) => inconclusive.push(format!("timed out after {}s", timeout.as_secs())),
            Err(e) => inconclusive.push(format!("spawn failed: {e}")),
        }
    }
    if !not_detected.is_empty() && inconclusive.is_empty() {
        let reverted = code.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        let against = match baseline {
            Some(b) => format!("their contents when the bead was assigned ({})", &b[..b.len().min(10)]),
            None => "HEAD".to_string(),
        };
        return RedVerdict::NotDetected(format!(
            "red check: your acceptance test still PASSES with your non-test changes reverted \
             to {against} ({reverted}), so it does not exercise your change. A test that passes \
             without the fix proves nothing about it. Make the test drive the feature through \
             the path a game or the editor uses, so it fails without your change and passes \
             with it. Commands that passed on reverted code: {}",
            not_detected.join(" ; ")
        ));
    }
    if !compile_only.is_empty() && not_detected.is_empty() {
        return RedVerdict::CompileOnly(format!(
            "the acceptance test only fails to COMPILE with the change reverted ({}): that \
             proves it names the change's new API, not that an assertion depends on what the \
             change does. Name the assertion that fails at the old behaviour in the \
             completion's Red check: line",
            compile_only.join(" ; ")
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
    fn red_run_failure_is_detected_and_a_compile_error_is_compile_only() {
        assert!(matches!(
            classify_red_run(false, "thread 'x' panicked at src/a.rs:1\ntest result: FAILED."),
            RedVerdict::Detected(_)
        ));
        assert!(matches!(
            classify_red_run(false, "error[E0425]: cannot find function `foo`"),
            RedVerdict::CompileOnly(_)
        ));
        assert!(matches!(
            classify_red_run(false, "        FAIL [   0.010s] gdscene::foo bar"),
            RedVerdict::Detected(_)
        ));
    }

    /// A compile failure is not proof: `could not compile` alone, with no
    /// failing assertion, must never read as Detected — the red check used to
    /// pass any test that merely called a function the change added.
    #[test]
    fn a_compile_failure_without_a_failing_assertion_is_not_detected() {
        let out = "error[E0425]: cannot find function `prepare_play_frame` in module\n\
                   error: could not compile `gdeditor` (test \"x\") due to 1 previous error";
        match classify_red_run(false, out) {
            RedVerdict::CompileOnly(why) => assert!(why.contains("API"), "{why}"),
            other => panic!("a compile error must be CompileOnly, got {other:?}"),
        }
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
        let v = red_check(r, "engine-rs/crates/a/tests/t.rs", &["cargo test --test t".into()], &["t".into()], None, Duration::from_secs(5));
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
            None,
            Duration::from_secs(5),
        );
        assert!(matches!(v, RedVerdict::Inconclusive(ref m) if m.contains("already committed")), "{v:?}");
    }

    #[test]
    fn red_check_skips_beads_that_name_no_acceptance_test() {
        let dir = repo();
        let r = dir.path();
        std::fs::write(r.join("engine-rs/crates/a/src/lib.rs"), "pub fn v() -> i32 { 1 } // refactor\n").unwrap();
        let v = red_check(r, "engine-rs/crates/a/src/lib.rs", &["cargo test -p a".into()], &[], None, Duration::from_secs(5));
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
        let v = red_check(r, "engine-rs/crates/a/src/lib.rs", &["cargo test -p a --test t".into()], &["t".into()], None, Duration::from_secs(5));
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

        let v = red_check(r, files, &["cargo test -p a --test sees".into()], &["sees_change".into()], None, t);
        assert!(matches!(v, RedVerdict::Detected(_)), "{v:?}");

        let v = red_check(r, files, &["cargo test -p a --test blind".into()], &["blind".into()], None, t);
        assert!(matches!(v, RedVerdict::NotDetected(ref m) if m.contains("still PASSES")), "{v:?}");

        // The live tree is untouched by the check.
        assert_eq!(
            std::fs::read_to_string(r.join("engine-rs/crates/a/src/lib.rs")).unwrap(),
            "pub fn v() -> i32 { 2 }\n"
        );
    }
    /// The checkpoint-sweep case end to end: an auto-checkpoint committed the
    /// change mid-bead, so HEAD already returns 2. Against HEAD the check can
    /// only give up; against the assignment baseline it decides both ways.
    #[test]
    fn red_check_against_the_assignment_baseline_sees_through_a_checkpoint_sweep() {
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
        let cache = r.join(".codex/orchestrator");
        record_assignment_base(r, &cache, "pat-x.1", "Worker");
        let base = assignment_base(r, &cache, "pat-x.1").expect("baseline recorded");

        std::fs::write(r.join("engine-rs/crates/a/src/lib.rs"), "pub fn v() -> i32 { 2 }\n").unwrap();
        sh("git add -A && git commit -qm 'Checkpoint: auto after 10 closes (orchestrator)'");
        std::fs::write(r.join("engine-rs/crates/a/src/lib.rs"), "pub fn v() -> i32 { 2 } // tidy\n").unwrap();
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
        let sees = ["cargo test -p a --test sees".to_string()];
        let blind = ["cargo test -p a --test blind".to_string()];

        let v = red_check(r, files, &sees, &["sees_change".into()], None, t);
        assert!(matches!(v, RedVerdict::Inconclusive(ref m) if m.contains("auto-checkpoint")), "{v:?}");

        let v = red_check(r, files, &sees, &["sees_change".into()], Some(&base), t);
        assert!(matches!(v, RedVerdict::Detected(_)), "{v:?}");

        let v = red_check(r, files, &blind, &["blind".into()], Some(&base), t);
        assert!(
            matches!(v, RedVerdict::NotDetected(ref m) if m.contains("when the bead was assigned")),
            "{v:?}"
        );
    }

    #[test]
    fn assignment_base_keeps_the_first_record_for_the_same_worker() {
        let dir = repo();
        let r = dir.path();
        let cache = r.join(".codex/orchestrator");
        record_assignment_base(r, &cache, "pat-x.1", "Worker");
        let first = assignment_base(r, &cache, "pat-x.1").unwrap();
        let sh = |c: &str| {
            assert!(Command::new("bash").args(["-c", c]).current_dir(r).status().unwrap().success());
        };
        sh("git commit -q --allow-empty -m later");
        // A reprompt to the same worker must not move the baseline forward.
        record_assignment_base(r, &cache, "pat-x.1", "Worker");
        assert_eq!(assignment_base(r, &cache, "pat-x.1").unwrap(), first);
        // A reassignment to another worker starts from the current tree.
        record_assignment_base(r, &cache, "pat-x.1", "Other");
        assert_ne!(assignment_base(r, &cache, "pat-x.1").unwrap(), first);
        assert_eq!(assignment_base(r, &cache, "pat-none"), None);
        // A record naming a commit that no longer exists is ignored.
        std::fs::write(cache.join("assign-base/pat-x.1"), "0123456789abcdef0123456789abcdef01234567\tOther\n").unwrap();
        assert_eq!(assignment_base(r, &cache, "pat-x.1"), None);
    }

    #[test]
    fn a_file_vanishing_mid_copy_is_not_an_rsync_failure() {
        let vanished = "rsync(1): error: /r/.agent-mail-store/p/.archive.lock: open (2) in /r: No such file or directory\n\
                        rsync(1): error: some files/attrs were not transferred (see previous errors) (code 23) at main.c(1338)\n";
        assert!(only_vanished_files(Some(23), vanished));
        assert!(only_vanished_files(Some(24), ""));
        let denied = "rsync(1): error: /r/x: open: Permission denied\n";
        assert!(!only_vanished_files(Some(23), denied));
        assert!(!only_vanished_files(Some(11), ""));
    }

    #[test]
    fn snapshot_skips_tracker_state() {
        let dir = repo();
        let r = dir.path();
        std::fs::create_dir_all(r.join(".beads")).unwrap();
        std::fs::write(r.join(".beads/beads.db-wal"), "x").unwrap();
        let snap = r.join(".orchestrator/redcheck/tree");
        sync_snapshot(r, &snap).unwrap();
        assert!(snap.join("engine-rs/crates/a/src/lib.rs").exists());
        assert!(!snap.join(".beads").exists());
    }

    #[test]
    fn comment_stripping_keeps_string_literals() {
        let src = "// loads x.tscn\n/* res://a\n /* nested */ still */ let s = \"res://k.tscn\"; \
                   let r = r#\"a // \"not a comment\" b\"#; let c = '\"'; fn f<'a>(x: &'a str) {}\n\
                   let u = \"uid://abc\"; MainLoop::new(); // EditorServer\n";
        let out = strip_rust_comments(src);
        assert!(!out.contains("x.tscn"), "{out}");
        assert!(!out.contains("res://a"), "{out}");
        assert!(!out.contains("EditorServer"), "{out}");
        assert!(out.contains("\"res://k.tscn\""), "{out}");
        assert!(out.contains(r#"a // "not a comment" b"#), "{out}");
        assert!(out.contains("\"uid://abc\""), "{out}");
        assert!(out.contains("MainLoop::new()"), "{out}");
        assert!(out.contains("fn f<'a>(x: &'a str)"), "{out}");
    }

    /// pat-mgixk.16: a data-layer test whose only "entry point" was a doc
    /// comment mentioning project.godot passed the rule.
    #[test]
    fn entry_point_rule_ignores_markers_in_comments_and_bare_project_files() {
        let dir = repo();
        let r = dir.path();
        let desc = "**Wiring requirement:** served editor";
        std::fs::write(
            r.join("engine-rs/crates/a/tests/c.rs"),
            "//! Round-trips project.godot and x.tscn files.\n#[test]\nfn in_comment() { let _ = a::v(); }\n",
        )
        .unwrap();
        std::fs::write(
            r.join("engine-rs/crates/a/tests/p.rs"),
            "#[test]\nfn writes_project_file() { std::fs::write(\"project.godot\", \"\").ok(); }\n",
        )
        .unwrap();
        for (name, file) in [("in_comment", "c.rs"), ("writes_project_file", "p.rs")] {
            let v = check_entry_point(r, desc, &[name.into()], &format!("engine-rs/crates/a/tests/{file}"));
            assert!(matches!(v, EntryPointVerdict::Rejected(_)), "{name}: {v:?}");
        }
    }
}
