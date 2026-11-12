//! pat-s3rrv.59 — the six missing Variant builtins, end to end.
//!
//! # What was wrong
//!
//! `Variant` carried 34 of Godot's 38 builtin kinds. `Vector4`, `Vector4i`,
//! `Rect2i`, `Projection`, `RID` and `PackedVector4Array` had none, and the
//! `.tscn` loader knew it: `UNPARSED_BUILTINS` kept five of those literals as a
//! **tagged dictionary** carrying the original text, so a file round-tripped
//! byte-for-byte while nothing could actually *read* the value. A script asking
//! a `Rect2i` property for `.size` got a Dictionary, `Vector4(1, 2, 3, 4)` could
//! not be constructed at all, and the binary codec refused the whole resource.
//!
//! Text fidelity was never the gap. **Readability** was — which is why this test
//! asserts the loaded values are real kinds *and* that the text survives, since
//! the old placeholder satisfied the second half on its own.

use gdcore::math::{Rect2i, Vector2i, Vector4, Vector4i};
use gdcore::math3d::Projection;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdvariant::Variant;

/// A scene whose root carries one property of each newly-added kind.
///
/// ⚠️ The `Projection` values are deliberately **asymmetric** (1..16 rather
/// than an identity). A transposed projection re-saves to the same sixteen
/// numbers in the same order, so a round-trip test built on a symmetric matrix
/// passes just as happily whether the convention is column-major or row-major.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]
vec4 = Vector4(1, 2, 3, 4)
vec4i = Vector4i(5, 6, 7, 8)
region = Rect2i(0, 0, 16, 16)
proj = Projection(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16)
weights = PackedVector4Array(1, 0, 0, 0, 0, 1, 0, 0)
"#;

fn load() -> (SceneTree, NodeId) {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    (tree, main)
}

fn prop(tree: &SceneTree, id: NodeId, name: &str) -> Variant {
    tree.get_node(id).expect("node exists").get_property(name)
}

/// Walks a value looking for the loader's preservation dictionary.
///
/// The placeholder is a `Variant::Dictionary` with a `__gd_unparsed_type__`
/// key, and it can hide inside an array or another dictionary, so the search
/// has to recurse rather than glance at the top level.
fn contains_unparsed(v: &Variant) -> bool {
    match v {
        Variant::Dictionary(d) => {
            d.get("__gd_unparsed_type__").is_some()
                || d.to_dict().iter().any(|(_, val)| contains_unparsed(val))
        }
        Variant::Array(items) => items.to_vec().iter().any(contains_unparsed),
        _ => false,
    }
}

/// **The acceptance test.**
#[test]
fn missing_variant_builtins_tscn_roundtrip_and_script_access() {
    let (tree, main) = load();

    // -- 1. every literal loads as a REAL Variant kind -----------------------
    //
    // Asserted as whole values rather than "is not a Dictionary": the
    // placeholder recorded the numbers too, so a weaker check would pass on it.
    assert_eq!(
        prop(&tree, main, "vec4"),
        Variant::Vector4(Vector4::new(1.0, 2.0, 3.0, 4.0)),
    );
    assert_eq!(
        prop(&tree, main, "vec4i"),
        Variant::Vector4i(Vector4i::new(5, 6, 7, 8)),
        "the INTEGER kind must come back — a Vector4 here would print the same \
         while having silently retyped the value",
    );
    assert_eq!(
        prop(&tree, main, "region"),
        Variant::Rect2i(Rect2i::new(Vector2i::new(0, 0), Vector2i::new(16, 16))),
    );
    assert_eq!(
        prop(&tree, main, "weights"),
        Variant::PackedVector4Array(vec![
            Vector4::new(1.0, 0.0, 0.0, 0.0),
            Vector4::new(0.0, 1.0, 0.0, 0.0),
        ]),
    );

    // The projection's columns, pinned individually: `m[0]` must be the FIRST
    // four numbers of the literal. `[1, 5, 9, 13]` here would mean the sixteen
    // values were read row-major and every camera matrix is transposed.
    let Variant::Projection(p) = prop(&tree, main, "proj") else {
        panic!(
            "proj must load as a Projection, got {:?}",
            prop(&tree, main, "proj")
        );
    };
    assert_eq!(p.m[0], [1.0, 2.0, 3.0, 4.0], "m[0] is the x COLUMN");
    assert_eq!(p.m[3], [13.0, 14.0, 15.0, 16.0], "m[3] is the w column");

    // -- 2. no preservation dictionary survives anywhere ---------------------
    for name in ["vec4", "vec4i", "region", "proj", "weights"] {
        assert!(
            !contains_unparsed(&prop(&tree, main, name)),
            "`{name}` still holds a `__gd_unparsed_type__` dictionary — the \
             loader's placeholder path is shadowing the real parse arm, so the \
             value only LOOKS right when printed",
        );
    }

    // -- 3. re-saving reproduces the original text ---------------------------
    //
    // This is what the placeholder bought, and it must not be lost in exchange
    // for readability: the point of the bead is having both.
    let saved = TscnSaver::save_tree(&tree, main);
    for line in SCENE.lines().filter(|l| l.contains(" = ")) {
        assert!(
            saved.contains(line),
            "re-saving must reproduce `{line}` verbatim, got:\n{saved}",
        );
    }
}

/// A script reads the new kinds off its own node: components, a sub-vector, an
/// indexed projection column, and a push into the packed array.
///
/// ⚠️ `_ready` is dispatched by `LifecycleManager::enter_tree`, **not** by
/// `MainLoop::step`, and `enter_tree` runs `process_script_ready` over the whole
/// subtree unconditionally — there is no already-entered guard, so calling it on
/// a node `add_packed_scene_to_tree` already inserted is correct.
///
/// ⚠️ Every var below is initialised to a sentinel, so if `_ready` raises at its
/// FIRST statement each one reads back as its initialiser and every assertion
/// fails identically — indistinguishable from "the property read wrong". That is
/// exactly how this test failed its first verify round (`x` came back `-1.0`),
/// and reading the source could not separate the two: `_ready` demonstrably runs
/// (the constructor test below shares this fixture and passes), `get_node_property`
/// falls through to the node when the script has no such var, and the node really
/// does hold a `Vector4` (asserted in the acceptance test above).
///
/// So `stage` records how far `_ready` actually got. It turns one opaque
/// sentinel into a named statement, and it costs nothing when the path works.
const READ_GD: &str = "\
extends Node2D
var stage = 0
var raw_is_vec4 = false
var x = -1.0
var w = -1.0
var iw = -1
var size_x = -1
var col_y = -1.0
var appended = -1

func _ready():
    self.stage = 1
    var v = self.vec4
    self.stage = 2
    self.raw_is_vec4 = v is Vector4
    self.x = v.x
    self.w = v.w
    self.stage = 3
    self.iw = self.vec4i.w
    self.stage = 4
    self.size_x = self.region.size.x
    self.stage = 5
    self.col_y = self.proj[1].y
    self.stage = 6
    var ws = self.weights
    ws.push_back(Vector4(9, 9, 9, 9))
    self.appended = ws.size()
    self.stage = 7
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

#[test]
fn scripts_read_the_new_builtins_off_a_node() {
    let (mut tree, main) = load();
    let script = GDScriptNodeInstance::from_source(READ_GD, main).expect("script parses");
    tree.attach_script(main, Box::new(script));
    LifecycleManager::enter_tree(&mut tree, main);
    let ml = MainLoop::new(tree);

    // ⚠️ Read this FIRST — nothing below means anything until it passes.
    // Each value names the exact statement that raised.
    assert_eq!(
        script_var(&ml, main, "stage"),
        Some(Variant::Int(7)),
        "`_ready` did not run to completion, so every assertion below is \
         reading an initialiser rather than a value. The stage says where it \
         stopped:\n  \
         0 = `_ready` never ran at all (fixture problem, not a Variant one)\n  \
         1 = `self.vec4` raised — reading an ad-hoc .tscn node property off \
         `self` does not reach `SceneAccess::get_node_property`\n  \
         2 = `v.x`/`v.w` raised — `variant_math::member_get` has no Vector4 arm\n  \
         3 = `self.vec4i.w` raised\n  \
         4 = `self.region.size.x` raised — Rect2i has no `size` member\n  \
         5 = `self.proj[1]` raised — a Projection cannot be indexed\n  \
         6 = `push_back` onto a PackedVector4Array raised",
    );

    assert_eq!(
        script_var(&ml, main, "raw_is_vec4"),
        Some(Variant::Bool(true)),
        "the node property must arrive as a real Vector4. `false` means it \
         reached the script as something else — a Dictionary placeholder would \
         still print as four numbers",
    );
    assert_eq!(
        script_var(&ml, main, "x"),
        Some(Variant::Float(1.0)),
        "`vec4.x` must reach the script as a Float",
    );
    assert_eq!(script_var(&ml, main, "w"), Some(Variant::Float(4.0)));
    assert_eq!(
        script_var(&ml, main, "iw"),
        Some(Variant::Int(8)),
        "a Vector4i component must arrive as an Int — a Float here would make \
         `v.w == 8` fail for a value that IS 8",
    );
    assert_eq!(
        script_var(&ml, main, "size_x"),
        Some(Variant::Int(16)),
        "`region.size` is a Vector2i, so its `.x` is an Int",
    );
    // `proj[1]` is the y COLUMN (5, 6, 7, 8), whose `.y` is 6.
    assert_eq!(
        script_var(&ml, main, "col_y"),
        Some(Variant::Float(6.0)),
        "indexing a Projection yields a whole Vector4 COLUMN; 2.0 here would \
         mean it handed back a row",
    );
    assert_eq!(
        script_var(&ml, main, "appended"),
        Some(Variant::Int(3)),
        "push_back on a PackedVector4Array must grow it from 2 to 3",
    );
}

/// The GDScript constructors for all six kinds, including the two a `.tscn`
/// never writes (`RID`, and `Projection()`'s no-arg form).
#[test]
fn gdscript_constructs_every_new_builtin() {
    let src = "\
extends Node2D
var v4 = null
var v4i = null
var r2i = null
var proj_is_identity = false
var rid_valid = true
var packed_size = -1

func _ready():
    self.v4 = Vector4(1, 2, 3, 4)
    self.v4i = Vector4i(1, 2, 3, 4)
    self.r2i = Rect2i(0, 0, 16, 16)
    var idp = Projection()
    self.proj_is_identity = idp[2].z == 1.0
    self.rid_valid = RID().is_valid()
    self.packed_size = PackedVector4Array([Vector4(1, 2, 3, 4)]).size()
";
    let (mut tree, main) = load();
    let script = GDScriptNodeInstance::from_source(src, main).expect("script parses");
    tree.attach_script(main, Box::new(script));
    LifecycleManager::enter_tree(&mut tree, main);
    let ml = MainLoop::new(tree);

    assert_eq!(
        script_var(&ml, main, "v4"),
        Some(Variant::Vector4(Vector4::new(1.0, 2.0, 3.0, 4.0))),
    );
    assert_eq!(
        script_var(&ml, main, "v4i"),
        Some(Variant::Vector4i(Vector4i::new(1, 2, 3, 4))),
    );
    assert_eq!(
        script_var(&ml, main, "r2i"),
        Some(Variant::Rect2i(Rect2i::from_components(0, 0, 16, 16))),
    );
    // ⚠️ Godot's default-constructed Projection is the IDENTITY, not zero — a
    // zero matrix would collapse everything it transformed to the origin, and
    // the two are indistinguishable until something is actually projected.
    assert_eq!(
        script_var(&ml, main, "proj_is_identity"),
        Some(Variant::Bool(true)),
        "Projection() must be IDENTITY: its z column is (0, 0, 1, 0)",
    );
    assert_eq!(
        script_var(&ml, main, "rid_valid"),
        Some(Variant::Bool(false)),
        "RID() is the INVALID handle — script can never mint a valid one, since \
         a real handle only comes back from a *Server call",
    );
    assert_eq!(script_var(&ml, main, "packed_size"), Some(Variant::Int(1)));
}

/// The binary codec round-trips all six, at Godot's own type ordinals.
///
/// The tags are not a judgement call: they are the gaps in
/// `Variant::Type` (`core/variant/variant.h`), and a wrong one produces a
/// buffer real Godot misreads as a different type rather than rejecting.
#[test]
fn binary_codec_round_trips_the_new_builtins() {
    for value in [
        Variant::Rect2i(Rect2i::from_components(-1, 2, 16, 16)),
        Variant::Vector4(Vector4::new(1.5, -2.5, 3.0, 4.0)),
        Variant::Vector4i(Vector4i::new(-1, 2, -3, 4)),
        Variant::Projection(Projection {
            m: [
                [1.0, 2.0, 3.0, 4.0],
                [5.0, 6.0, 7.0, 8.0],
                [9.0, 10.0, 11.0, 12.0],
                [13.0, 14.0, 15.0, 16.0],
            ],
        }),
        Variant::Rid(gdcore::id::Rid::from_raw(0xDEAD_BEEF_1234)),
        Variant::PackedVector4Array(vec![Vector4::new(1.0, 2.0, 3.0, 4.0)]),
    ] {
        let bytes = gdvariant::binary::encode_variant(&value)
            .unwrap_or_else(|e| panic!("{:?} must encode: {e}", value.variant_type()));
        let (back, used) = gdvariant::binary::decode_variant(&bytes)
            .unwrap_or_else(|e| panic!("{:?} must decode: {e}", value.variant_type()));
        assert_eq!(
            used,
            bytes.len(),
            "{:?} must consume its whole buffer — a short read means the \
             payload width is wrong and every following value in a real \
             resource stream would be misaligned",
            value.variant_type()
        );
        assert_eq!(
            back,
            value,
            "{:?} must survive the wire",
            value.variant_type()
        );
    }
}
