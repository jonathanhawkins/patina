//! pat-s3rrv.58 acceptance (scene half) — the `Color` builtin surface reaches a
//! node's property through the real `.tscn` → script → frame path.
//!
//! The interpreter-level half of this bead is pinned by
//! `gdscript-interop/tests/color_builtin_surface_from_scene.rs`, which drives
//! the methods, the 146 named constants and the member *reads* through a bare
//! `Interpreter`. That leaves the bead's actual wiring requirement untested:
//! *".tscn → node with a Color property (e.g. `modulate`) → attached script
//! mutates it → the property read-back reflects the new value after a frame."*
//!
//! Two things only this layer can catch:
//!
//! 1. **The write half of the members.** Every existing assertion *reads*
//!    `col.r8` / `col.h`. Godot's Color members are read-WRITE, and the write
//!    path is a different function (`member_set`) reached through a different
//!    interpreter branch. A read-only bridge passes every test above and still
//!    drops `modulate.a8 = 128` on the floor.
//!
//! 2. **The seed.** A bare `modulate = …` assignment writes the *node* property
//!    only when that property is already non-Nil — the interpreter uses
//!    "reads back non-Nil" as its test for "this name is a node property"
//!    (`interpreter.rs`, `Expr::Ident` arm of `exec_assignment`). An
//!    unpopulated ClassDB seeds nothing, so the whole script would run clean
//!    and change nothing at all. `register_all_classes()` below is load-bearing,
//!    not boilerplate, and the precondition assertion exists to say so if the
//!    seeding ever regresses.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager, MainLoop, NodeId};
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Tint" type="Sprite2D" parent="."]
"#;

/// Mutates `modulate` from `_ready` exactly as a game would: assign a whole
/// colour built from a named constant and an instance method, then reach into
/// one of its 8-bit members. The `shifted` var exercises the HSV write on a
/// script-owned colour, where the target is a nested member access rather than
/// a bare name.
const TINT_SCRIPT: &str = "\
extends Sprite2D
var shifted = Color.RED
func _ready():
    modulate = Color.YELLOW.darkened(0.2)
    modulate.a8 = 128
    self.shifted.h = 0.5
";

fn boot() -> (MainLoop, NodeId) {
    gdobject::class_db::register_all_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let tint = tree
        .get_node_or_null(scene_root, "Tint")
        .expect("Tint node");

    // ⚠️ Precondition, not decoration. If `Sprite2D` stops inheriting
    // CanvasItem's `modulate` default, the script below assigns to a fresh
    // local instead of the node and every assertion here reads WHITE — a
    // failure that would otherwise point at Color rather than at the seeding.
    assert!(
        !tree
            .get_node(tint)
            .expect("Tint is in the tree")
            .get_property("modulate")
            .is_nil(),
        "Sprite2D must carry CanvasItem's `modulate` default before the script \
         runs, or a bare `modulate = …` writes a local and silently changes \
         nothing",
    );

    let script = GDScriptNodeInstance::from_source(TINT_SCRIPT, tint).expect("script parses");
    tree.attach_script(tint, Box::new(script));
    LifecycleManager::enter_tree(&mut tree, scene_root);

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    (ml, tint)
}

fn node_color(ml: &MainLoop, node: NodeId, prop: &str) -> gdcore::math::Color {
    match ml
        .tree()
        .get_node(node)
        .expect("node is in the tree")
        .get_property(prop)
    {
        Variant::Color(c) => c,
        other => panic!("`{prop}` should be a Color, got {other:?}"),
    }
}

fn script_color(ml: &MainLoop, node: NodeId, prop: &str) -> gdcore::math::Color {
    use gdscript_interop::bindings::ScriptInstance;
    match ml
        .tree()
        .get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
    {
        Variant::Color(c) => c,
        other => panic!("`{prop}` should be a Color, got {other:?}"),
    }
}

/// **The acceptance test.** The bead's wiring clause: the node's `modulate`
/// reads back the darkened yellow after one frame.
#[test]
fn color_members_reach_a_scene_node() {
    let (ml, tint) = boot();
    let modulate = node_color(&ml, tint, "modulate");

    // `darkened(0.2)` scales each channel by (1 - amount), so YELLOW's 1.0
    // channels land on 0.8 and its zero blue stays zero.
    assert!(
        (modulate.r - 0.8).abs() < 1e-5 && (modulate.g - 0.8).abs() < 1e-5 && modulate.b == 0.0,
        "modulate must read back as the darkened yellow after a frame, got {modulate:?}",
    );
}

/// The write half of the 8-bit members, through a node property.
///
/// ⚠️ 128 is deliberately not 127: `a8` rounds rather than truncating, so the
/// float it stores is 128/255 = 0.50196…, and reading it back must give 128
/// again. A truncating bridge round-trips to 127 and pulls every converted
/// colour one step dark.
#[test]
fn an_eight_bit_member_write_lands_on_the_node_property() {
    let (ml, tint) = boot();
    let modulate = node_color(&ml, tint, "modulate");

    assert!(
        (modulate.a - 128.0 / 255.0).abs() < 1e-5,
        "`modulate.a8 = 128` must write 128/255 into the node's colour, got \
         alpha {}",
        modulate.a,
    );
    assert_eq!(
        modulate.a8(),
        128,
        "the stored alpha must round-trip back to 128, not 127",
    );

    // The write must not disturb the channels it does not name — a bridge that
    // rebuilds the colour from the wrong component order passes the alpha
    // assertion above and quietly rewrites the hue.
    assert!(
        (modulate.r - 0.8).abs() < 1e-5 && (modulate.g - 0.8).abs() < 1e-5 && modulate.b == 0.0,
        "the a8 write must leave r/g/b alone, got {modulate:?}",
    );
}

/// The HSV write, on a script-owned colour reached through `self.<var>.<member>`.
///
/// ⚠️ This is a *derived* member: writing `h` has to re-derive r/g/b from the
/// colour's existing s/v. RED is a good probe precisely because its saturation
/// and value are both 1 — rotating the hue to 0.5 must produce fully saturated
/// cyan, and a bridge that stored the hue in a field nobody reads leaves it red.
#[test]
fn an_hsv_member_write_rederives_the_channels() {
    let (ml, tint) = boot();
    let shifted = script_color(&ml, tint, "shifted");

    assert!(
        shifted.r.abs() < 1e-5 && (shifted.g - 1.0).abs() < 1e-5 && (shifted.b - 1.0).abs() < 1e-5,
        "`shifted.h = 0.5` must rotate RED to cyan, got {shifted:?}",
    );
    assert!(
        (shifted.h() - 0.5).abs() < 1e-4,
        "…and the hue must read back as 0.5, got {}",
        shifted.h(),
    );
}

/// The member *reads* through the node property, which is what a script
/// actually does with a colour it did not build itself.
///
/// ⚠️ Yellow's hue is 1/6, not 0 and not 60: Godot normalises hue to 0..1
/// everywhere in the Color API, so a bridge handing back degrees produces a
/// wildly wrong colour that still looks like a colour.
#[test]
fn the_members_read_back_off_the_node_property() {
    let (ml, tint) = boot();
    let modulate = node_color(&ml, tint, "modulate");

    assert_eq!(modulate.r8(), 204, "0.8 * 255 rounds to 204",);
    assert!(
        (modulate.h() - 1.0 / 6.0).abs() < 1e-4,
        "yellow's hue is 1/6 in Godot's 0..1 range, got {}",
        modulate.h(),
    );
}
