//! pat-21oab.2: the single gate every test must pass a committed golden
//! fixture through before rewriting it.
//!
//! A test that writes its own expected values can never fail. Several golden
//! tests used to do exactly that: generate a trace, write it over
//! `fixtures/golden/...`, then regenerate and compare the two *fresh* runs.
//! That only ever proved the engine is deterministic — it could not notice
//! that the engine no longer agrees with the recorded trace, and it left the
//! working tree dirty so a careless commit baked the new behaviour in as the
//! new "expectation".
//!
//! The rule here: a golden is **read-only** unless the operator explicitly
//! asks for a re-bless.
//!
//! ```text
//! cargo nextest run -p patina-engine --test physics_suite          # compares
//! PATINA_BLESS=1 cargo nextest run -p patina-engine --test physics_suite  # rewrites
//! ```
//!
//! `GENERATE_GOLDENS` is accepted as well: it is the pre-existing spelling in
//! `deterministic_physics_trace_golden_test` and
//! `physics_playground_extended_golden_test`, and splitting the repo across two
//! env vars would mean an operator who blesses with one gets a dirty tree from
//! the other. `REGENERATE_SCENE_GOLDENS` stays as the scene-suite's own
//! narrower switch.
//!
//! Included with `#[path = "../support/golden_bless.rs"] mod golden_bless;`
//! from a suite subdirectory, or `#[path = "support/golden_bless.rs"]` from
//! `tests/`.

#![allow(dead_code)] // each including suite uses a different subset

use std::path::Path;

/// Environment variables that authorise rewriting a committed golden.
///
/// Kept public so the CI gate test can assert the source scan and the runtime
/// gate agree on the spelling instead of hardcoding it twice.
pub const BLESS_VARS: [&str; 2] = ["PATINA_BLESS", "GENERATE_GOLDENS"];

/// True when the operator asked for goldens to be regenerated.
///
/// Presence is what counts, not the value: `PATINA_BLESS=0` still blesses,
/// matching how `GENERATE_GOLDENS` has always behaved (`var().is_err()`).
pub fn bless_enabled() -> bool {
    BLESS_VARS.iter().any(|v| std::env::var_os(v).is_some())
}

/// Compare `actual` against the golden committed at `path`, or rewrite it when
/// blessing.
///
/// Returns `true` if the golden was rewritten.
///
/// Panics when the goldens disagree and blessing is off — that failure is the
/// whole point of the gate. The message carries the first differing line and
/// the bless command, because the useful next step is almost always to look at
/// the diff rather than to re-run.
pub fn write_or_compare_golden(path: &Path, actual: &str, label: &str) -> bool {
    if bless_enabled() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("{label}: cannot create {}: {e}", parent.display()));
        }
        std::fs::write(path, actual)
            .unwrap_or_else(|e| panic!("{label}: cannot write {}: {e}", path.display()));
        eprintln!("[bless] rewrote {}", path.display());
        return true;
    }

    let expected = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "{label}: no committed golden at {} ({e}).\n\
             Generate it once with:  PATINA_BLESS=1 cargo nextest run -p patina-engine …",
            path.display()
        )
    });

    if expected.trim_end() == actual.trim_end() {
        return false;
    }

    panic!(
        "{label}: engine output no longer matches the committed golden\n\
         golden: {}\n\
         {}\n\
         If the NEW behaviour is the correct one, re-bless with\n\
         \x20 PATINA_BLESS=1 cargo nextest run -p patina-engine …\n\
         and justify the change against the Godot oracle in the commit message.\n\
         Do NOT bless a trace you have not checked: the previous version of\n\
         these tests rewrote the golden silently, which is what pat-21oab.2 fixed.",
        path.display(),
        first_difference(&expected, actual),
    );
}

/// The first line where two golden texts diverge, formatted for a panic
/// message.
///
/// A whole-file diff in a panic is unreadable; the first differing line is
/// almost always enough to recognise which quantity moved.
fn first_difference(expected: &str, actual: &str) -> String {
    let mut e = expected.lines();
    let mut a = actual.lines();
    let mut line = 0usize;
    loop {
        line += 1;
        match (e.next(), a.next()) {
            (Some(le), Some(la)) if le == la => continue,
            (Some(le), Some(la)) => {
                return format!("first difference at line {line}:\n  golden: {le}\n  actual: {la}")
            }
            (Some(le), None) => {
                return format!("actual ends at line {line}; golden still has: {le}")
            }
            (None, Some(la)) => {
                return format!("golden ends at line {line}; actual still has: {la}")
            }
            (None, None) => return "files differ only in trailing whitespace".to_string(),
        }
    }
}
