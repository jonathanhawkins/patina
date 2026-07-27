//! pat-0q2kx.209 — `Thread` / `Mutex` / `Semaphore` / `WorkerThreadPool` are
//! reachable from GDScript.
//!
//! `gdplatform/src/thread.rs` has had real, tested `GodotThread`,
//! `GodotMutex`, `GodotSemaphore` and `WorkerThreadPool` all along — with
//! **zero call sites anywhere in the workspace**. None of the four classes was
//! registered in ClassDB and none was constructible: `Thread.new()` fell
//! through the `.new()` gate and errored.
//!
//! ## Why these run inline
//!
//! Every `gdplatform::thread` entry point requires `F: FnOnce() + Send +
//! 'static`. A GDScript `Callable` cannot satisfy that — it borrows the
//! interpreter, whose handle tables are `thread_local` and whose values are
//! `Rc`-based. Handing one to another OS thread is a soundness problem, not a
//! wiring gap. So the script-facing classes execute synchronously, which is
//! exactly what Godot itself does in a no-threads web export
//! (`core/object/worker_thread_pool.cpp`). The observable contract survives:
//! work submitted has finished by the time control returns.

use gdobject::class_db::class_exists;
use gdobject::register_3d_classes;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `method` on a fresh instance of `src` through the real VM dispatch.
fn call_method(src: &str, method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("call {method}() failed: {e:?}"))
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** All four classes constructible and behaving from a
/// script.
#[test]
fn threading_script_classes() {
    // -- Thread: start a Callable, read its value back ----------------------
    let thread_src = "\
extends Node

func check():
    var t = Thread.new()
    t.start(func(): return 42)
    if not t.is_started():
        return -1
    return t.wait_to_finish()
";
    assert_eq!(
        call_method(thread_src, "check"),
        Variant::Int(42),
        "Thread.start() must run the Callable and wait_to_finish() return its value",
    );

    // -- Mutex: lock / try_lock / unlock ------------------------------------
    //
    // Godot's Mutex is recursive, so try_lock() succeeds even while held by
    // this same thread.
    let mutex_src = "\
extends Node

func check():
    var m = Mutex.new()
    m.lock()
    var got = m.try_lock()
    m.unlock()
    m.unlock()
    return got
";
    assert_eq!(
        call_method(mutex_src, "check"),
        Variant::Bool(true),
        "a recursive Mutex must allow try_lock() from the holder",
    );

    // -- Semaphore: post / try_wait ----------------------------------------
    let sem_src = "\
extends Node

func check():
    var s = Semaphore.new()
    if s.try_wait():
        return \"try_wait succeeded on an empty semaphore\"
    s.post()
    if not s.try_wait():
        return \"try_wait failed after post\"
    if s.try_wait():
        return \"try_wait succeeded twice for one post\"
    return \"ok\"
";
    assert_eq!(
        call_method(sem_src, "check"),
        Variant::String("ok".to_string()),
        "post() must make exactly one try_wait() succeed",
    );

    // -- WorkerThreadPool: single task --------------------------------------
    let task_src = "\
extends Node

func check():
    var id = WorkerThreadPool.add_task(func(): return 7)
    if not WorkerThreadPool.is_task_completed(id):
        return -1
    WorkerThreadPool.wait_for_task_completion(id)
    return 1
";
    assert_eq!(
        call_method(task_src, "check"),
        Variant::Int(1),
        "a submitted task must report completed and be waitable",
    );

    // -- WorkerThreadPool: group task over 8 elements -----------------------
    let group_src = "\
extends Node

func check():
    var id = WorkerThreadPool.add_group_task(func(i): return i, 8)
    WorkerThreadPool.wait_for_group_task_completion(id)
    if not WorkerThreadPool.is_group_task_completed(id):
        return -1
    return WorkerThreadPool.get_group_processed_element_count(id)
";
    assert_eq!(
        call_method(group_src, "check"),
        Variant::Int(8),
        "add_group_task(f, 8) must process all 8 elements",
    );
}

// ===========================================================================
// Details
// ===========================================================================

/// All four must be registered, or a `.tscn`-attached script and the inspector
/// cannot see them even though the interpreter can construct them.
#[test]
fn threading_classes_are_registered() {
    register_3d_classes();
    for name in ["Thread", "Mutex", "Semaphore", "WorkerThreadPool"] {
        assert!(class_exists(name), "{name} must be registered in ClassDB");
    }

    let thread = gdobject::class_db::get_class_info("Thread").expect("registered");
    let methods: Vec<&str> = thread.methods.iter().map(|m| m.name.as_str()).collect();
    for expected in [
        "start",
        "is_started",
        "is_alive",
        "wait_to_finish",
        "get_id",
    ] {
        assert!(
            methods.contains(&expected),
            "Thread must expose `{expected}`; has {methods:?}",
        );
    }
    assert_eq!(
        thread.parent_class, "RefCounted",
        "Thread is RefCounted, so `.new()` yields a handle not a node",
    );
    assert_eq!(
        gdobject::class_db::get_class_info("WorkerThreadPool")
            .expect("registered")
            .parent_class,
        "Object",
        "WorkerThreadPool is a plain Object singleton",
    );
}

/// Each handle keeps its own state — the tables must not be shared.
#[test]
fn handles_are_independent() {
    let src = "\
extends Node

func check():
    var a = Semaphore.new()
    var b = Semaphore.new()
    a.post()
    a.post()
    # b never received a post, so its try_wait must fail.
    if b.try_wait():
        return \"semaphores share state\"
    if not a.try_wait():
        return \"a lost its posts\"
    return \"ok\"
";
    assert_eq!(
        call_method(src, "check"),
        Variant::String("ok".to_string()),
        "two Semaphores must not share a counter",
    );
}

/// A Thread that was never started yields Nil rather than panicking, and
/// reports `is_started() == false`.
#[test]
fn unstarted_thread_is_inert() {
    let src = "\
extends Node

func check():
    var t = Thread.new()
    if t.is_started():
        return \"reported started before start()\"
    if t.is_alive():
        return \"reported alive before start()\"
    return \"ok\"
";
    assert_eq!(
        call_method(src, "check"),
        Variant::String("ok".to_string()),
        "an unstarted Thread must be inert, not panic",
    );
}

/// A group task of zero elements completes immediately rather than reporting
/// an unfinished group forever.
#[test]
fn empty_group_task_completes() {
    let src = "\
extends Node

func check():
    var id = WorkerThreadPool.add_group_task(func(i): return i, 0)
    if not WorkerThreadPool.is_group_task_completed(id):
        return -1
    return WorkerThreadPool.get_group_processed_element_count(id)
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Int(0),
        "a zero-element group is complete with zero processed",
    );
}

/// A group task whose body errors surfaces the error rather than silently
/// reporting a completed group — the failure mode that would make a broken
/// worker look like a working one.
#[test]
fn group_task_body_error_is_not_swallowed() {
    let mut interp = Interpreter::new();
    let src = "\
extends Node

func check():
    return WorkerThreadPool.add_group_task(func(i): return missing_name, 3)
";
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");

    assert!(
        interp
            .call_instance_method(&mut inst, "check", &[])
            .is_err(),
        "an undefined name inside the group body must propagate, not be hidden \
         behind a completed-looking group",
    );
}
