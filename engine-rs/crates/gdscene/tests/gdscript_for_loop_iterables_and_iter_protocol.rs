//! pat-mgixk.72 acceptance: a GDScript `for` loop iterates everything Godot's
//! VM iterates, including an object implementing the `_iter_*` virtuals.
//!
//! Godot ground truth: `OPCODE_ITERATE_BEGIN_*` in
//! `modules/gdscript/gdscript_vm.cpp` — Dictionary (keys, insertion order),
//! String (single-char strings), every `Packed*Array`, float and Vector ranges,
//! and any Object implementing `_iter_init`/`_iter_next`/`_iter_get`
//! (`doc/classes/Object.xml`).
//!
//! # What was broken
//!
//! `Stmt::For` matched **only** `Variant::Array` and `Variant::Int` and raised
//! `TypeError: cannot iterate over <type>` for everything else, and
//! `grep -rn '_iter_init|_iter_next|_iter_get' crates/` was **0 hits**. So
//! `for k in dict` and `for c in "abc"` — core GDScript, not an edge case —
//! were runtime errors.
//!
//! 🔑 The sequence is asserted as ONE ordered list across all four loops. Four
//! independent "did it run" checks would pass for an implementation that
//! iterated the wrong number of elements or in the wrong order; the Dictionary
//! clause in particular is only meaningful because `Dictionary` stores
//! `entries: Vec<(Variant, Variant)>` and therefore has a defined key order.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A custom iterator class, as a `res://` script.
///
/// ⚠️ It keeps its cursor on ITSELF rather than in the state array. Godot allows
/// either — the state array is a convenience for stateless iterators — and this
/// shape avoids depending on `state[0] = x` subscript-assignment through a
/// parameter, which is a different feature from the one under test.
const MY_RANGE_GD: &str = "extends RefCounted\nvar cur = 0\nvar limit = 3\n\nfunc _iter_init(state):\n\tself.cur = 0\n\treturn self.cur < self.limit\n\nfunc _iter_get(state):\n\treturn self.cur\n\nfunc _iter_next(state):\n\tself.cur = self.cur + 1\n\treturn self.cur < self.limit\n";

fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("patina_mgixk72_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp res:// root is creatable");
        std::fs::write(dir.join("my_range.gd"), MY_RANGE_GD).expect("my_range.gd is writable");
        gdcore::file_access::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

/// Boots a one-node scene with `src` attached and runs `_ready`.
fn run(src: &str) -> MainLoop {
    res_root();
    gdobject::class_db::register_all_classes();

    let scene =
        PackedScene::from_tscn("[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node\"]\n")
            .expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(src, main).expect("script parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(main);
    ml.step(1.0 / 60.0);
    ml
}

fn script_var(ml: &MainLoop, var: &str) -> Variant {
    let id = ml
        .tree()
        .get_node_by_path("/root/Main")
        .expect("Main is in the tree");
    ml.tree()
        .get_script(id)
        .expect("Main has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

fn seq(ml: &MainLoop, var: &str) -> Vec<Variant> {
    match script_var(ml, var) {
        Variant::Array(a) => a.to_vec(),
        other => panic!("`{var}` should be an Array, got {other:?}"),
    }
}

const ALL_ITERABLES_GD: &str = "\
extends Node
const MyRange = preload(\"res://my_range.gd\")
var seq = []

func _ready():
    for k in {\"a\": 1, \"b\": 2}:
        self.seq.append(k)
    for c in \"hi\":
        self.seq.append(c)
    for v in PackedInt32Array([3, 4]):
        self.seq.append(v)
    for x in MyRange.new():
        self.seq.append(x)
";

/// **The acceptance test.** All four iterable kinds, in one ordered sequence.
#[test]
fn gdscript_for_loop_iterables_and_iter_protocol() {
    let ml = run(ALL_ITERABLES_GD);
    let got = seq(&ml, "seq");

    let want = vec![
        Variant::String("a".into()),
        Variant::String("b".into()),
        Variant::String("h".into()),
        Variant::String("i".into()),
        Variant::Int(3),
        Variant::Int(4),
        Variant::Int(0),
        Variant::Int(1),
        Variant::Int(2),
    ];
    // An empty sequence is the pre-bead behaviour: the first `for k in {..}`
    // raised `cannot iterate over Dictionary` and killed the rest of `_ready`.
    assert!(
        !got.is_empty(),
        "nothing was recorded — `_ready` raised on the first loop, which is \
         exactly the TypeError this bead removes"
    );
    assert_eq!(
        got, want,
        "the four loops must record keys, chars, packed elements and the custom \
         iterator's values in that order"
    );
}

/// The other iterables Godot's VM covers, which the acceptance list does not
/// name. Separate so a failure here points at the kind rather than at ordering.
const MORE_ITERABLES_GD: &str = "\
extends Node
var f = []
var v2 = []
var v2i = []
var v3i = []
var strs = []

func _ready():
    for x in 2.5:
        self.f.append(x)
    for x in Vector2(1, 4):
        self.v2.append(x)
    for x in Vector2i(1, 4):
        self.v2i.append(x)
    for x in Vector3i(1, 7, 2):
        self.v3i.append(x)
    for s in PackedStringArray([\"p\", \"q\"]):
        self.strs.append(s)
";

#[test]
fn float_vector_and_packed_string_iteration() {
    let ml = run(MORE_ITERABLES_GD);

    // A float iterates like range() but KEEPS float values.
    assert_eq!(
        seq(&ml, "f"),
        vec![
            Variant::Float(0.0),
            Variant::Float(1.0),
            Variant::Float(2.0)
        ],
        "`for x in 2.5` must yield floats 0.0, 1.0, 2.0"
    );
    // Vector2 is range(from, to) with FLOAT elements...
    assert_eq!(
        seq(&ml, "v2"),
        vec![
            Variant::Float(1.0),
            Variant::Float(2.0),
            Variant::Float(3.0)
        ],
        "`for x in Vector2(1, 4)` must yield floats 1.0, 2.0, 3.0"
    );
    // ...and Vector2i the same range with INT elements. Asserted separately
    // because the element TYPE is part of the observable: a single shared arm
    // would make one of these two wrong while the values still looked right.
    assert_eq!(
        seq(&ml, "v2i"),
        vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)],
        "`for x in Vector2i(1, 4)` must yield ints 1, 2, 3"
    );
    // Vector3i adds the step.
    assert_eq!(
        seq(&ml, "v3i"),
        vec![Variant::Int(1), Variant::Int(3), Variant::Int(5)],
        "`for x in Vector3i(1, 7, 2)` must step by 2"
    );
    assert_eq!(
        seq(&ml, "strs"),
        vec![Variant::String("p".into()), Variant::String("q".into())],
        "a PackedStringArray must iterate its elements"
    );
}

const AWAIT_IN_OBJECT_LOOP_GD: &str = "\
extends Node
const MyRange = preload(\"res://my_range.gd\")
var seq = []

func _ready():
    for x in MyRange.new():
        await get_tree().create_timer(0.01).timeout
        self.seq.append(x)
";

/// An `await` inside an object-iterator loop resumes and finishes the iteration.
///
/// 🔑 This is why the object protocol is MATERIALISED into the same `Vec` the
/// Array arm builds: `ResumeFrame::For` already carries `pending_items`, so a
/// suspended loop resumes through the path that already existed. A lazily-driven
/// iterator would need its own resume frame, and the classic failure — recording
/// only the FIRST element and silently dropping the rest (pat-0q2kx.78) — is
/// what this assertion catches.
#[test]
fn an_await_inside_an_object_iterator_loop_resumes() {
    let mut ml = run(AWAIT_IN_OBJECT_LOOP_GD);
    // Enough frames for three 0.01s timers at 1/60s per step.
    for _ in 0..8 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        seq(&ml, "seq"),
        vec![Variant::Int(0), Variant::Int(1), Variant::Int(2)],
        "the loop must resume after each await and record all three values; a \
         single value means the continuation dropped the pending items"
    );
}
