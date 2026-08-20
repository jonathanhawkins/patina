//! pat-s3rrv.131 — `randomize()` is safe on the web target.
//!
//! `Rng::randomize` seeded itself from `std::time::SystemTime::now()`, which
//! **panics on `wasm32-unknown-unknown`** — that target has no wall clock. This
//! was not theoretical: `gdcore` is a dependency of `patina-web`, and
//! `randomize()` is the global reseed entry, so it is among the first things a
//! web build touches.
//!
//! ⚠️ These tests run on the HOST, so they cannot execute the wasm branch. They
//! pin the two halves that are checkable here: the behavioural contract the web
//! branch has to satisfy, and — structurally — that no ungated clock call has
//! crept back into the module. The wasm branch itself is gated by the new
//! `wasm32` CI job, which is the only thing that can compile it.

use gdcore::rng::{randomize, Rng};

/// The contract `randomize()` promises, and the one the counter-based web
/// entropy is built to satisfy: a different seed every call.
#[test]
fn successive_randomize_calls_produce_distinct_seeds() {
    let mut rng = Rng::with_seed(1);
    let a = rng.randomize();
    let b = rng.randomize();
    let c = rng.randomize();

    assert_ne!(a, b, "two reseeds must not collide");
    assert_ne!(b, c);
    assert_ne!(a, c);
}

/// ⚠️ The acceptance asks for a NON-ZERO seed specifically. The mixing can in
/// principle cancel to zero, so that is guaranteed rather than assumed.
#[test]
fn a_randomized_seed_is_never_zero() {
    let mut rng = Rng::with_seed(0);
    for _ in 0..64 {
        assert_ne!(rng.randomize(), 0, "a reseed must never yield a zero seed");
    }
}

/// The global entry point — the one a script reaches through
/// `@GlobalScope.randomize()` — works and also varies.
#[test]
fn the_global_randomize_varies() {
    let a = randomize();
    let b = randomize();
    assert_ne!(a, b);
    assert_ne!(a, 0);
    assert_ne!(b, 0);
}

/// 🔑 The structural guard: **every** `SystemTime::now()` call in `rng.rs` must
/// sit behind a `not(target_arch = "wasm32")` gate.
///
/// ⚠️ A behavioural test on the host can never catch a regression here — the
/// native branch is exactly the one that works. Someone adding a second,
/// ungated clock call would reintroduce the wasm panic with every host test
/// still green, and only this scan or an actual wasm build would see it.
#[test]
fn no_ungated_wall_clock_call_remains_in_the_module() {
    const SRC: &str = include_str!("../src/rng.rs");

    // Scan CODE lines only; the module's own doc comments name the API.
    let code: Vec<&str> = SRC
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.starts_with("//") && !l.starts_with("/*") && !l.starts_with('*'))
        .collect();

    let mut checked = 0usize;
    for (i, line) in code.iter().enumerate() {
        if !line.contains("SystemTime::now") {
            continue;
        }
        checked += 1;
        // The gate is the nearest preceding attribute line.
        let gated = code[..i]
            .iter()
            .rev()
            .take(3)
            .any(|l| l.contains("cfg(not(target_arch = \"wasm32\"))"));
        assert!(
            gated,
            "an ungated SystemTime::now() would panic on wasm32: {line}"
        );
    }

    assert_eq!(
        checked, 1,
        "expected exactly one clock call site to guard; if this changed, the \
         new one needs the same gate"
    );
}
