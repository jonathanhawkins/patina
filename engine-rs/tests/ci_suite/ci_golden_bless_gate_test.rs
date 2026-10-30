//! pat-21oab.2: no test may rewrite a committed golden fixture unless the
//! operator explicitly asked for it.
//!
//! The bug this guards against: `physics_integration_test` generated a physics
//! trace, wrote it over `fixtures/golden/physics/*.json`, then regenerated and
//! compared the two *fresh* runs. The committed golden played no part in the
//! assertion, so the test could not fail no matter how far the engine drifted —
//! and a full workspace run left seven golden files modified in the working
//! tree, ready for a careless `git commit -a` to bake the drift in as the new
//! expectation.
//!
//! Acceptance: a full workspace run leaves `fixtures/` clean. Rather than shell
//! out to git (which would only notice the problem after it happened, and only
//! on a clean tree), this asserts the property that makes it true: every write
//! into `fixtures/golden/` sits behind a bless gate.

#[path = "../support/golden_bless.rs"]
mod golden_bless;

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // engine-rs/ -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Env vars that authorise rewriting a golden.
///
/// `PATINA_BLESS` is the canonical spelling (and the one
/// `tests/support/golden_bless.rs` documents); the other two predate it and
/// still gate their own files.
const BLESS_GATES: [&str; 4] = [
    "golden_bless::",
    "PATINA_BLESS",
    "GENERATE_GOLDENS",
    "REGENERATE_SCENE_GOLDENS",
];

/// Every `.rs` file under the engine's test trees.
fn test_sources(root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![root.join("engine-rs/tests")];
    let crates = root.join("engine-rs/crates");
    if let Ok(entries) = std::fs::read_dir(&crates) {
        for entry in entries.flatten() {
            let tests = entry.path().join("tests");
            if tests.is_dir() {
                roots.push(tests);
            }
        }
    }

    let mut out = Vec::new();
    let mut stack = roots;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Blank out `//` line comments and the bodies of string literals, so a path
/// mentioned in prose does not read as code.
///
/// Doc comments in these files talk about `fixtures/golden/...` constantly, and
/// a raw substring scan would flag every one of them.
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        // Line comment: keep the newline so line structure survives.
        if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment.
        if c == '/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        // String literal: keep the quotes (so `fs::write(` shape is intact)
        // but drop the contents.
        if c == '"' {
            out.push('"');
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            out.push('"');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// No test file that touches `fixtures/golden/` writes to disk without a bless
/// gate somewhere in it.
#[test]
fn no_test_writes_goldens_without_bless() {
    let root = repo_root();
    let sources = test_sources(&root);
    assert!(
        sources.len() > 100,
        "expected to scan the whole engine test tree, found only {} files — \
         the directory walk is broken, not the repo",
        sources.len()
    );

    let mut offenders = Vec::new();
    let mut gated = Vec::new();

    for path in &sources {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        // Only files that actually name the committed golden tree can dirty it.
        if !src.contains("fixtures/golden") {
            continue;
        }
        let code = strip_comments(&src);
        let writes = code.contains("fs::write(") || code.contains("File::create(");
        if !writes {
            continue;
        }
        let rel = path.strip_prefix(&root).unwrap_or(path).to_path_buf();
        if BLESS_GATES.iter().any(|gate| src.contains(gate)) {
            gated.push(rel);
        } else {
            offenders.push(rel);
        }
    }

    assert!(
        offenders.is_empty(),
        "these test files write into fixtures/golden/ with no bless gate, so a \
         plain test run rewrites committed expectations:\n{}\n\n\
         Route the write through tests/support/golden_bless.rs \
         (`write_or_compare_golden`), which compares by default and writes only \
         under PATINA_BLESS=1.",
        offenders
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // A green result above is worthless if the scan found nothing to classify:
    // these are the files known to write goldens, and the scan must still see
    // them. Without this, deleting `fixtures/golden` from every doc comment
    // would "fix" the test.
    assert!(
        gated.len() >= 5,
        "expected at least 5 gated golden writers, found {}: {:?} — \
         the scan is no longer finding the files it is meant to police",
        gated.len(),
        gated
    );
}

/// The two files that carried the bug name the shared helper, not a raw write.
///
/// The file-level scan above would also pass if someone added one gated write
/// and left an ungated one beside it. These two files are where that already
/// happened, so they get an exact rule: no raw writes at all.
#[test]
fn golden_writers_that_regressed_use_the_shared_helper() {
    let root = repo_root();
    for rel in [
        "engine-rs/tests/physics_suite/physics_integration_test.rs",
        "engine-rs/tests/render_suite/render_benchmark_fixtures_test.rs",
    ] {
        let path = root.join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        assert!(
            src.contains("golden_bless"),
            "{rel} must go through tests/support/golden_bless.rs"
        );

        let code = strip_comments(&src);
        // `render_benchmark_fixtures_test` keeps one `fs::write`, inside an
        // `if golden_bless::bless_enabled()` branch, because its baseline is
        // timing data that cannot be compared byte-for-byte. Allow exactly that
        // shape by requiring the gate to appear before any write.
        if let Some(write_at) = code.find("fs::write(") {
            let gate_at = code
                .find("bless_enabled()")
                .unwrap_or_else(|| panic!("{rel} has an fs::write with no bless_enabled() guard"));
            assert!(
                gate_at < write_at,
                "{rel}: the fs::write at byte {write_at} is not guarded — \
                 bless_enabled() first appears at byte {gate_at}"
            );
        }
    }
}

/// The helper itself: compare when not blessing, write when blessing, and in
/// neither case silently accept a mismatch.
///
/// Exercised through a temp file rather than a real fixture, and it branches on
/// the ambient bless state instead of mutating the environment — a
/// `set_var` here would be visible to every other test in the binary.
#[test]
fn write_or_compare_golden_honours_the_gate() {
    let dir = std::env::temp_dir().join(format!(
        "patina-golden-bless-gate-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let golden = dir.join("trace.json");
    std::fs::write(&golden, "recorded\n").unwrap();

    // Identical content is accepted either way.
    golden_bless::write_or_compare_golden(&golden, "recorded\n", "gate-test");

    let mismatch = std::panic::catch_unwind(|| {
        golden_bless::write_or_compare_golden(&golden, "drifted\n", "gate-test");
    });
    let after = std::fs::read_to_string(&golden).unwrap();

    if golden_bless::bless_enabled() {
        assert!(
            mismatch.is_ok(),
            "blessing must accept new content, not panic"
        );
        assert_eq!(after, "drifted\n", "blessing must rewrite the golden");
    } else {
        assert!(
            mismatch.is_err(),
            "a drifted trace must fail the test, not be written over the golden"
        );
        assert_eq!(
            after, "recorded\n",
            "the committed golden must be untouched when not blessing"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
