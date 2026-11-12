//! pat-s3rrv.262 — a GDScript `MovieWriter` registers itself and is driven by
//! the frame loop.
//!
//! # The gap
//!
//! `gdobject::movie_writer` held a process-global registry with **no non-test
//! callers**: `"add_writer"` appeared once in the whole workspace, as a
//! `MethodInfo` in ClassDB, and never as a dispatch arm. So
//! `MovieWriter.add_writer(self)` raised from GDScript, and nothing anywhere
//! called `_write_begin`/`_write_frame`/`_write_end`. The module's own test
//! called the Rust functions directly, which is why it stayed green while the
//! feature was script-unreachable.
//!
//! # Two deliberate divergences from the bead
//!
//! * **`class_db.rs` is untouched.** The bead lists it, but `MovieWriter` is
//!   already registered with all six virtuals at the correct arity, abstract,
//!   under `Object`. There was nothing to add.
//! * **`MainLoop` does not render the frame.** The bead says pixels come from
//!   "the same publish the renderer already produces", but `MainLoop` has no
//!   renderer — `FrameOutput` carries no image and no render call exists in the
//!   loop. That mirrors Godot, where the writer is fed by the rendering server,
//!   so the host publishes pixels via `set_movie_frame_pixels` and the capture
//!   defaults to an opaque black frame at the capture size until it does.
//!
//! ⚠️ `_write_frame`'s second parameter is Godot's `audio_frame_block`, typed
//! `const void*` — a raw pointer, so that virtual is overridable only from
//! C++/GDExtension upstream. A GDScript writer cannot receive it; `Nil` is
//! passed and the arity still matches. The image half is real, and it is the
//! half the acceptance is about.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Serialises these tests against the process-global writer registry.
///
/// ⚠️ `gdobject::movie_writer` is process-global, matching Godot's own
/// `MovieWriter::add_writer`. The nextest runner gives each test its **own
/// process**, so they cannot interfere and the suite is green there — but the
/// plain in-process runner, which CLAUDE.md lists as the fallback, runs them as
/// threads in ONE process. There `clear_writers()` in a starting test wipes a
/// running test's registration and `instance_writers()` returns every test's
/// writers at once, which surfaces as `left: 4, right: 1`.
///
/// Green under one runner and flaky under the other is the worst shape a test
/// can have, so these tests take this lock rather than trusting the runner to
/// isolate them.
static REGISTRY: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Takes the registry lock for the duration of a test.
///
/// Poisoning is ignored deliberately: the guarded data is `()`, so a panicking
/// test leaves nothing corrupt behind, and refusing the lock afterwards would
/// turn one failure into a cascade of unrelated ones.
fn lock_registry() -> std::sync::MutexGuard<'static, ()> {
    REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The path the writer under test accepts.
const OUT: &str = "user://capture.avi";

/// A writer that accepts `OUT` and counts every virtual it receives.
const WRITER: &str = "\
extends MovieWriter

var begins = 0
var frames = 0
var ends = 0
var width = -1
var height = -1
var fps_seen = -1
var path_seen = \"\"

func _ready():
    MovieWriter.add_writer(self)

func _handles_file(path):
    return path == \"user://capture.avi\"

func _write_begin(size, fps, base_path):
    self.begins += 1
    self.fps_seen = fps
    self.path_seen = base_path
    return 0

func _write_frame(image, audio):
    self.frames += 1
    self.width = image.get_width()
    self.height = image.get_height()
    return 0

func _write_end():
    self.ends += 1
";

/// A writer that refuses every path — the control for writer selection.
const REFUSER: &str = "\
extends MovieWriter

var frames = 0
var asked = 0

func _ready():
    MovieWriter.add_writer(self)

func _handles_file(path):
    self.asked += 1
    return false

func _write_begin(size, fps, base_path):
    return 0

func _write_frame(image, audio):
    self.frames += 1
    return 0

func _write_end():
    pass
";

/// Builds a tree with one scripted node per source, runs `_ready` on each, and
/// returns the loop plus the node ids in the order given.
fn scene(sources: &[&str]) -> (MainLoop, Vec<NodeId>) {
    // Godot's registry is process-global and so is this one. nextest runs each
    // test in its own process, but clearing keeps the test honest if that ever
    // changes.
    gdobject::movie_writer::clear_writers();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = Vec::new();
    for (i, src) in sources.iter().enumerate() {
        let node = tree
            .add_child(root, Node::new(format!("Writer{i}"), "Node"))
            .unwrap();
        tree.attach_script(
            node,
            Box::new(GDScriptNodeInstance::from_source(src, node).expect("writer script parses")),
        );
        ids.push(node);
    }
    // `add_writer(self)` lives in `_ready`, per the acceptance.
    for id in &ids {
        tree.process_script_ready(*id);
    }
    (MainLoop::new(tree), ids)
}

fn num(ml: &MainLoop, node: NodeId, prop: &str) -> i64 {
    match ml
        .tree()
        .get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
    {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => panic!("expected a number for `{prop}`, got {other:?}"),
    }
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** Registration from `_ready`, one `_write_begin`,
/// exactly one `_write_frame` per frame with a real image, one `_write_end`.
#[test]
fn movie_writer_script_registry() {
    let _registry = lock_registry();
    let (mut ml, ids) = scene(&[WRITER]);
    let writer = ids[0];

    // The registry saw the registration, and it carries the *instance* — a
    // class name alone could not identify which object to call back into.
    let registered = gdobject::movie_writer::instance_writers();
    assert_eq!(
        registered.len(),
        1,
        "the script must have registered itself from _ready"
    );
    assert_eq!(
        registered[0].object_id,
        Some(writer.raw()),
        "the registered writer must be the node that called add_writer(self)"
    );

    ml.start_movie_capture(OUT, (320, 180), 60);
    ml.run_frames(3, 1.0 / 60.0);

    assert_eq!(
        num(&ml, writer, "begins"),
        1,
        "_write_begin runs exactly once per session, not once per frame"
    );
    assert_eq!(
        num(&ml, writer, "frames"),
        3,
        "_write_frame runs exactly once per frame over 3 frames"
    );
    assert_eq!(
        num(&ml, writer, "ends"),
        0,
        "_write_end must not fire before the session is finished"
    );

    // -- the image is real ---------------------------------------------------
    // Exact dimensions rather than "non-empty": a writer handed a 0x0 image, or
    // one sized to the viewport instead of the capture, would still be
    // "non-empty" by a laxer check.
    assert_eq!(
        (num(&ml, writer, "width"), num(&ml, writer, "height")),
        (320, 180),
        "_write_frame receives an Image at the capture size"
    );

    // -- what _write_begin was told ------------------------------------------
    assert_eq!(
        num(&ml, writer, "fps_seen"),
        60,
        "_write_begin receives fps"
    );
    assert_eq!(
        ml.tree()
            .get_script(writer)
            .expect("script")
            .get_property("path_seen"),
        Some(Variant::String(OUT.to_string())),
        "_write_begin receives the output path"
    );

    // -- shutdown ------------------------------------------------------------
    assert!(
        ml.finish_movie_capture(),
        "finishing an active capture closes its writer"
    );
    assert_eq!(
        num(&ml, writer, "ends"),
        1,
        "_write_end fires exactly once on shutdown"
    );
    assert!(
        !ml.movie_capture_active(),
        "the session is over once it is finished"
    );
}

/// With capture off, none of the three virtuals is called.
///
/// The other half of the acceptance, and the one that catches a frame loop that
/// drives writers unconditionally.
#[test]
fn movie_writer_script_registry_inert_without_capture() {
    let _registry = lock_registry();
    let (mut ml, ids) = scene(&[WRITER]);
    let writer = ids[0];

    ml.run_frames(3, 1.0 / 60.0);

    assert_eq!(
        (
            num(&ml, writer, "begins"),
            num(&ml, writer, "frames"),
            num(&ml, writer, "ends")
        ),
        (0, 0, 0),
        "a registered writer must stay untouched while capture is off"
    );
    assert!(
        !ml.finish_movie_capture(),
        "finishing when nothing was started reports that it closed nothing"
    );
}

/// The first writer whose `_handles_file` accepts the path wins; one that
/// refuses is never driven.
#[test]
fn movie_writer_script_registry_selects_by_handles_file() {
    let _registry = lock_registry();
    // The refuser registers FIRST, so a loop that simply took the first
    // registered writer would pick it and this would fail.
    let (mut ml, ids) = scene(&[REFUSER, WRITER]);
    let (refuser, writer) = (ids[0], ids[1]);

    ml.start_movie_capture(OUT, (16, 16), 30);
    ml.run_frames(2, 1.0 / 30.0);

    assert_eq!(
        num(&ml, refuser, "frames"),
        0,
        "a writer whose _handles_file returned false must never be driven"
    );
    assert!(
        num(&ml, refuser, "asked") >= 1,
        "…but it must have been asked, or selection never consulted it"
    );
    assert_eq!(
        num(&ml, writer, "frames"),
        2,
        "the accepting writer is the one driven"
    );

    // Asked once per session, not once per frame: selection is resolved at
    // begin and remembered.
    assert_eq!(
        num(&ml, refuser, "asked"),
        1,
        "_handles_file is consulted once per session, not every frame"
    );
}

/// Registering the same instance twice does not double-drive it.
///
/// Godot ignores a duplicate `add_writer` of the same object; without that, a
/// writer whose node is removed and re-added would receive two `_write_frame`
/// calls per frame while the virtual is specified as one.
#[test]
fn movie_writer_script_registry_ignores_duplicate_registration() {
    let _registry = lock_registry();
    let (mut ml, ids) = scene(&[WRITER]);
    let writer = ids[0];

    // A second `_ready` is the realistic way this happens.
    ml.tree_mut().process_script_ready(writer);
    assert_eq!(
        gdobject::movie_writer::instance_writers().len(),
        1,
        "the same instance must be registered once"
    );

    ml.start_movie_capture(OUT, (8, 8), 30);
    ml.run_frames(2, 1.0 / 30.0);
    assert_eq!(
        num(&ml, writer, "frames"),
        2,
        "one _write_frame per frame, not one per registration"
    );
}
