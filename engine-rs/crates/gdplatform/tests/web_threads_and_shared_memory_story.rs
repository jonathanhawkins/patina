//! pat-s3rrv.217 — the web threading story, told from one runtime fact.
//!
//! Godot ships a `thread_support` web variant that only works under
//! cross-origin isolation (COOP/COEP). With it, `OS.has_feature("threads")` is
//! true and the audio driver feeds the worklet through a `SharedArrayBuffer`
//! ring; without it the engine runs single-threaded and the worklet asks the
//! main thread for each block.
//!
//! 🔑 **Isolation is a RUNTIME fact, not a build flag.** The same wasm bundle is
//! isolated or not depending on the headers the server happens to send, so
//! anything decided at compile time is wrong in both directions. Every
//! assertion here flips on that one input.

use gdplatform::export::web_audio_worklet_glue;
use gdplatform::os::{has_feature, is_cross_origin_isolated, set_cross_origin_isolated};
use gdplatform::thread::{PoolMode, WorkerThreadPool};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// The feature tag
// ---------------------------------------------------------------------------

/// The capability round-trips, and it starts off.
///
/// ⚠️ Default false is the safe direction: a page that never reports isolation
/// must not be told it has threads it cannot create.
#[test]
fn the_isolation_capability_defaults_off_and_round_trips() {
    set_cross_origin_isolated(false);
    assert!(!is_cross_origin_isolated());
    set_cross_origin_isolated(true);
    assert!(is_cross_origin_isolated());
    set_cross_origin_isolated(false);
    assert!(!is_cross_origin_isolated());
}

/// On a native host `has_feature("threads")` is true regardless of isolation —
/// the flag only ever decides the web answer.
///
/// 🔑 This is the test that keeps the change honest on the platform the suite
/// actually runs on. `has_feature` now reads a mutable global, and the failure
/// mode of getting that wrong is a native build reporting no threads.
#[test]
fn native_reports_threads_whatever_the_isolation_flag_says() {
    set_cross_origin_isolated(false);
    assert!(
        has_feature("threads"),
        "a native host has threads even with the web isolation flag off"
    );
    set_cross_origin_isolated(true);
    assert!(has_feature("threads"));
    set_cross_origin_isolated(false);
}

// ---------------------------------------------------------------------------
// The cooperative scheduler
// ---------------------------------------------------------------------------

/// **The acceptance's middle clause.** Without threads, group tasks still all
/// complete — drained on the frame tick.
#[test]
fn cooperative_group_tasks_all_complete_on_the_frame_tick() {
    let pool = WorkerThreadPool::new_cooperative();
    assert_eq!(pool.mode(), PoolMode::Cooperative);

    let seen = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&seen);
    let group = pool.add_group_task(8, move |_index| {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    // 🔑 Nothing has run yet: the whole point of the cooperative path is that
    // submitting does NOT execute. If this were non-zero the work would be
    // running on the caller's thread at submit time, which would stall the
    // frame that queued it rather than the one that drains it.
    assert_eq!(
        seen.load(Ordering::SeqCst),
        0,
        "cooperative submission must queue, not run"
    );
    assert!(!pool.is_group_task_completed(group));

    pool.drain_pending();

    assert_eq!(seen.load(Ordering::SeqCst), 8, "every element must run");
    assert!(pool.is_group_task_completed(group));
    assert_eq!(pool.get_group_processed_element_count(group), 8);
}

/// 🔑 **The deadlock this design exists to avoid.** On a single-threaded build
/// `wait_for_group_task_completion` cannot wait — there is no other thread that
/// could make progress, so a wait loop would spin forever on exactly the build
/// that needs this path. It drains inline instead.
///
/// A test that only called `drain_pending` would never catch that; this is the
/// call a ported script actually makes.
#[test]
fn waiting_on_a_cooperative_group_drains_instead_of_hanging() {
    let pool = WorkerThreadPool::new_cooperative();
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&seen);
    let group = pool.add_group_task(4, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    // If this hangs, the test times out rather than failing — which is itself
    // the signal, and is why the assertion after it matters.
    pool.wait_for_group_task_completion(group);

    assert_eq!(seen.load(Ordering::SeqCst), 4);
    assert!(pool.is_group_task_completed(group));
}

/// Each element receives its own index, so a group task can actually partition
/// work rather than running the same thing N times.
#[test]
fn every_element_gets_its_own_index() {
    let pool = WorkerThreadPool::new_cooperative();
    let sum = Arc::new(AtomicUsize::new(0));
    let acc = Arc::clone(&sum);
    let group = pool.add_group_task(5, move |index| {
        acc.fetch_add(index, Ordering::SeqCst);
    });
    pool.wait_for_group_task_completion(group);
    // 0+1+2+3+4
    assert_eq!(
        sum.load(Ordering::SeqCst),
        10,
        "indices must be distinct; 0 would mean every element got the same one"
    );
}

/// The threaded pool runs the same group to completion, so a script sees the
/// same result on both builds.
#[test]
fn a_threaded_group_completes_too() {
    let pool = WorkerThreadPool::new();
    assert_eq!(pool.mode(), PoolMode::Threaded);
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&seen);
    let group = pool.add_group_task(6, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    pool.wait_for_group_task_completion(group);
    assert_eq!(seen.load(Ordering::SeqCst), 6);
    assert!(pool.is_group_task_completed(group));
}

/// An empty group is immediately complete rather than never completing.
#[test]
fn an_empty_group_is_already_done() {
    let pool = WorkerThreadPool::new_cooperative();
    let group = pool.add_group_task(0, |_| unreachable!("no elements to run"));
    assert!(pool.is_group_task_completed(group));
    pool.wait_for_group_task_completion(group);
}

// ---------------------------------------------------------------------------
// The audio worklet transport
// ---------------------------------------------------------------------------

/// **The acceptance's last clause.** The threaded export emits SAB-ring glue;
/// the plain one emits message-port glue.
#[test]
fn the_worklet_transport_follows_the_threaded_variant() {
    let threaded = web_audio_worklet_glue(true);
    assert!(
        threaded.contains("Atomics.load") && threaded.contains("SharedArrayBuffer ring"),
        "the threaded export must emit the shared-memory ring; got:\n{threaded}"
    );

    let plain = web_audio_worklet_glue(false);
    assert!(
        plain.contains("port.postMessage") && plain.contains("need-block"),
        "the single-threaded export must emit message-port block passing; \
         got:\n{plain}"
    );
}

/// ⚠️ **The discriminator, and the reason this is not merely a performance
/// choice.** `SharedArrayBuffer` and `Atomics` are ABSENT from the global object
/// without cross-origin isolation — not slower, absent — so glue referencing
/// them throws on load and the page gets no audio at all.
///
/// Emitting the ring unconditionally would break every export not served with
/// both COOP and COEP, which is the default for most static hosts.
#[test]
fn the_non_isolated_glue_never_references_shared_memory() {
    let plain = web_audio_worklet_glue(false);
    for forbidden in ["SharedArrayBuffer", "Atomics"] {
        assert!(
            !plain.contains(forbidden),
            "the non-isolated worklet must not reference `{forbidden}` — it does \
             not exist on that page, so the glue would throw on load and play \
             nothing. got:\n{plain}"
        );
    }
}

/// Both variants register the same processor name, so the loader does not have
/// to know which transport it got.
#[test]
fn both_transports_register_the_same_processor() {
    for glue in [web_audio_worklet_glue(true), web_audio_worklet_glue(false)] {
        assert!(
            glue.contains("registerProcessor('patina-audio'"),
            "every variant must register the same name; got:\n{glue}"
        );
    }
}

// ---------------------------------------------------------------------------
// Thread.set_thread_safety_checks_enabled
// ---------------------------------------------------------------------------

/// The checks are on by default, and toggling them round-trips.
#[test]
fn thread_safety_checks_default_on() {
    use gdplatform::thread::{is_thread_safety_checks_enabled, set_thread_safety_checks_enabled};
    assert!(
        is_thread_safety_checks_enabled(),
        "Godot's default, for every thread, is enabled"
    );
    set_thread_safety_checks_enabled(false);
    assert!(!is_thread_safety_checks_enabled());
    set_thread_safety_checks_enabled(true);
    assert!(is_thread_safety_checks_enabled());
}

/// 🔑 **The setting is PER-THREAD, and this is what a global flag gets wrong.**
///
/// Godot's docs say the checks are disabled "on the current thread", with every
/// thread defaulting to enabled. Backed by a process-wide `AtomicBool`, one
/// worker opting out would silently opt the main thread out too — turning a
/// deliberate, locally-scoped escape hatch into an engine-wide one, which is
/// precisely the opposite of what someone reaching for this wants.
#[test]
fn disabling_the_checks_does_not_leak_to_another_thread() {
    use gdplatform::thread::{is_thread_safety_checks_enabled, set_thread_safety_checks_enabled};

    set_thread_safety_checks_enabled(false);
    assert!(!is_thread_safety_checks_enabled(), "off on this thread");

    let other = std::thread::spawn(|| {
        // A freshly spawned thread starts enabled regardless of what the
        // spawning thread chose for itself.
        let inherited = is_thread_safety_checks_enabled();
        set_thread_safety_checks_enabled(false);
        inherited
    })
    .join()
    .expect("worker joins");

    assert!(
        other,
        "a new thread must start with the checks ENABLED — inheriting the \
         spawner's choice would spread an opt-out the caller scoped to itself"
    );
    assert!(
        !is_thread_safety_checks_enabled(),
        "and the worker turning its own off must not turn ours back on"
    );

    set_thread_safety_checks_enabled(true);
}
