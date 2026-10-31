//! pat-mgixk.35 acceptance — Camera3D's projection and unprojection API, and
//! Godot's one-call mode constructors, reach a GDScript through the real
//! dispatch path.
//!
//! Godot's `Camera3D` exposes `unproject_position`, `project_position`,
//! `project_ray_origin`, `project_ray_normal`, `is_position_behind`,
//! `is_position_in_frustum`, `get_camera_projection`, `set_perspective`,
//! `set_orthogonal` and `set_frustum` to scripts. They are how a game turns a
//! click into a world ray and a world object into a HUD position.
//!
//! ⚠️ THE BUG WAS SILENT, which is why this test leads with `ran`. All of these
//! names were already registered in ClassDB (`class_db.rs`, the 19 methods of
//! `doc/classes/Camera3D.xml`), and `node_has_method` answers from the
//! registration table alone. So the interpreter took its "the method exists"
//! branch and returned `Variant::Nil` — a falsy value, with no error anywhere —
//! for every one of them. `$Camera3D.unproject_position(p)` gave back nothing,
//! and a script could not even detect it, because the thing it would ask
//! (`has_method`) was the thing that was wrong.
//!
//! 🔑 WHY THEY WERE UNWIRED, and what changed. Every projection function in
//! `camera3d.rs` takes explicit `viewport_width`/`viewport_height`, which
//! Godot's script API does not pass. That size lived only on `MainLoop`, and
//! `SceneTree` — the only thing the dispatch layer sees — had none. Wiring the
//! arms before that would have meant inventing a viewport and returning
//! confidently wrong screen coordinates. It was in fact already reachable —
//! `tree.root_window().visible_size()`, the same source `visibility_3d.rs`
//! builds a camera frustum from — so the arms read that and no new state was
//! added. `MainLoop::set_viewport_size` already resizes the root window, which
//! is why the `set_viewport_size(640, 480)` below is what the camera sees.
//!
//! Entry point, per the bead's Wiring requirement: a `.tscn` instanced into a
//! real tree, a GDScript attached to its root, and a `MainLoop` driving
//! `_ready` — the shape `affine_builtin_methods_and_euler_order_from_scene.rs`
//! uses. Every call below is made with the `$Camera3D.method()` dot syntax a
//! game writes, NOT by calling `camera3d::unproject_position` directly.
//! `camera3d_project_and_unproject_from_scene.rs` already covers the Rust
//! functions; what was missing was any way to reach them from a script.

use gdcore::math::{Vector2, Vector3};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// The acceptance's viewport. ⚠️ NOT the 1152x648 a `MainLoop` starts at — the
/// expected numbers below are halves and quarters of THESE, so a test that
/// forgot `set_viewport_size` would read a different aspect and fail on the
/// orthogonal assertion even against a correct engine.
const VIEW_W: u32 = 640;
const VIEW_H: u32 = 480;

/// A Camera3D at the world origin with an identity basis, which is Godot's
/// "looking down -Z". Written out rather than left to the node default so the
/// fixture states the orientation the expected values depend on.
///
/// ⚠️ `projection = 0` is declared rather than left implicit. An unset property
/// does NOT read back its ClassDB default from a script: `get_node_property`
/// falls through to `builtin_property_default`, which is a short hand-written
/// list (CharacterBody2D/3D and RigidBody3D velocities) and does not consult
/// the registration table. So `$Camera3D.projection` on a camera that never had
/// one assigned reads `Nil`, not `0`. Declaring it is also the honest fixture:
/// the camera starts perspective because the scene says so.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera3D" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
projection = 0
current = true
"#;

/// Reads a script `var`. ⚠️ Script vars and node properties are separate
/// stores; `get_node(..).get_property(..)` would not see any of these.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

#[track_caller]
fn vec2_of(got: &Variant, what: &str) -> Vector2 {
    match got {
        Variant::Vector2(v) => *v,
        Variant::Nil => panic!(
            "{what}: got Nil. That is the silent no-op this bead exists for — \
             the method is registered in ClassDB, so `has_method` says yes, but \
             no dispatch arm answered it."
        ),
        other => panic!("{what}: expected a Vector2, got {other:?}"),
    }
}

#[track_caller]
fn vec3_of(got: &Variant, what: &str) -> Vector3 {
    match got {
        Variant::Vector3(v) => *v,
        Variant::Nil => panic!(
            "{what}: got Nil — registered in ClassDB but never dispatched."
        ),
        other => panic!("{what}: expected a Vector3, got {other:?}"),
    }
}

#[track_caller]
fn near_vec2(got: Vector2, want: Vector2, tol: f32, what: &str) {
    assert!(
        (got - want).length() < tol,
        "{what}: expected about {want:?}, got {got:?}",
    );
}

/// **The acceptance clause.** A scene Camera3D at the origin on a 640x480
/// viewport, driven from GDScript.
#[test]
fn camera3d_script_projection_api() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    // ⚠️ Nothing here touches a value it then asserts on: every number comes
    // back from the engine. The script only stores what the camera returned.
    let script_src = "\
extends Node3D
var screen = null
var ray = null
var behind = null
var in_front = null
var inside = null
var ray_origin = null
var projection_before = -1
var projection_after = -1
var ortho_screen = null
var ortho_size = -1.0
var ortho_near = -1.0
var ortho_far = -1.0
var ran = false
func _ready():
    self.projection_before = $Camera3D.projection
    self.screen = $Camera3D.unproject_position(Vector3(0, 0, -5))
    self.ray = $Camera3D.project_ray_normal(Vector2(320, 240))
    self.ray_origin = $Camera3D.project_ray_origin(Vector2(320, 240))
    self.behind = $Camera3D.is_position_behind(Vector3(0, 0, 5))
    self.in_front = $Camera3D.is_position_behind(Vector3(0, 0, -5))
    self.inside = $Camera3D.is_position_in_frustum(Vector3(0, 0, -5))
    $Camera3D.set_orthogonal(10, 0.1, 100)
    self.projection_after = $Camera3D.projection
    self.ortho_size = $Camera3D.size
    self.ortho_near = $Camera3D.near
    self.ortho_far = $Camera3D.far
    self.ortho_screen = $Camera3D.unproject_position(Vector3(0, 2.5, -5))
    self.ran = true
";
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    // ⚠️ BEFORE `_ready`. The script reads the projection during `_ready`, and
    // the viewport size is an input to every number it reads.
    ml.set_viewport_size(VIEW_W, VIEW_H);
    ml.tree_mut().process_script_ready(scene_root);

    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_ready` must run to completion — a call raising part-way through is \
         what an undispatched method looks like from here",
    );

    // -- (a) a world point unprojects to the centre of the screen -----------
    //
    // The camera is at the origin looking -Z, so (0, 0, -5) is dead ahead and
    // lands at the viewport centre. 🔑 Independent of FOV and of the near/far
    // planes: the centre of the frustum is the centre whatever its shape, so
    // this assertion isolates the WIRING from the projection maths.
    near_vec2(
        vec2_of(&script_var(ml.tree(), scene_root, "screen"), "unproject_position"),
        Vector2::new(VIEW_W as f32 / 2.0, VIEW_H as f32 / 2.0),
        0.5,
        "unproject_position(Vector3(0, 0, -5))",
    );

    // -- (b) the ray through the screen centre points straight ahead --------
    let ray = vec3_of(&script_var(ml.tree(), scene_root, "ray"), "project_ray_normal");
    assert!(
        (ray - Vector3::new(0.0, 0.0, -1.0)).length() < 1e-3,
        "project_ray_normal(Vector2(320, 240)) must be about (0, 0, -1), got {ray:?}",
    );
    // A perspective camera's rays all start at the camera.
    let origin = vec3_of(
        &script_var(ml.tree(), scene_root, "ray_origin"),
        "project_ray_origin",
    );
    assert!(
        origin.length() < 1e-3,
        "project_ray_origin must be the camera position (the origin), got {origin:?}",
    );

    // -- (c) the behind test, BOTH directions -------------------------------
    assert_eq!(
        script_var(ml.tree(), scene_root, "behind"),
        Variant::Bool(true),
        "is_position_behind(Vector3(0, 0, 5)) must be true — the camera looks \
         down -Z, so +Z is behind it",
    );
    // 🔑 The discriminating half. `is_position_behind` is the one method here
    // whose correct answer is a bare `true`, so a stub that returned `true`
    // unconditionally — or a `Variant::Nil` read as truthy — would satisfy the
    // assertion above on its own. A point AHEAD of the camera must read false.
    assert_eq!(
        script_var(ml.tree(), scene_root, "in_front"),
        Variant::Bool(false),
        "is_position_behind(Vector3(0, 0, -5)) must be FALSE — that point is in \
         front of the camera",
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "inside"),
        Variant::Bool(true),
        "is_position_in_frustum(Vector3(0, 0, -5)) must be true — 5 units ahead \
         is between the default 0.05 near and 4000 far planes",
    );

    // -- (d) set_orthogonal switches the mode -------------------------------
    // ⚠️ This one reads back the fixture's OWN `projection = 0`, so on its own
    // it proves nothing about the engine — it is here only to pin the starting
    // value that makes the next assertion a change rather than a coincidence.
    assert_eq!(
        script_var(ml.tree(), scene_root, "projection_before"),
        Variant::Int(0),
        "the scene declares the camera perspective, so it must read 0 before          set_orthogonal — if this is Nil the property read path is broken,          which would make the `after` assertion below untrustworthy too",
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "projection_after"),
        Variant::Int(1),
        "after set_orthogonal the `projection` property must read 1 \
         (ProjectionType::Orthogonal)",
    );
    // ⚠️ All three scalars, not just the mode. `set_orthogonal(size, near, far)`
    // is one call that changes four things, and a version that flipped only the
    // enum would pass the assertion above.
    assert_eq!(
        script_var(ml.tree(), scene_root, "ortho_size"),
        Variant::Float(10.0),
        "set_orthogonal's first argument is the vertical SIZE",
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "ortho_near"),
        Variant::Float(0.1),
        "set_orthogonal's second argument is the NEAR plane",
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "ortho_far"),
        Variant::Float(100.0),
        "set_orthogonal's third argument is the FAR plane",
    );

    // -- (e) 🔑 the new projection is actually in use ------------------------
    //
    // The strongest assertion in the file, and the reason the viewport size had
    // to reach this layer at all. Under the orthogonal projection `size` is the
    // vertical world extent that fills the viewport: size 10 gives a half-height
    // of 5, so a point 2.5 units up sits half-way to the top edge — screen y
    // 120 on a 480-pixel viewport, with x still centred.
    //
    // ⚠️ Under the PERSPECTIVE projection the same point reads y ≈ 84, so this
    // number can only be produced by a camera that really switched modes AND
    // really used size 10 against a 640x480 aspect. Reading the `projection`
    // property proves the enum changed; this proves the matrix did.
    near_vec2(
        vec2_of(
            &script_var(ml.tree(), scene_root, "ortho_screen"),
            "unproject_position (orthogonal)",
        ),
        Vector2::new(320.0, 120.0),
        0.5,
        "unproject_position(Vector3(0, 2.5, -5)) under set_orthogonal(10, 0.1, 100)",
    );
}
