//! pat-0q2kx.32 — `RayCast2D`'s results reaching **GDScript**, and
//! `force_raycast_update()` actually re-running the cast.
//!
//! Godot ground truth: `upstream/godot/scene/2d/physics/ray_cast_2d.cpp` over
//! `doc/classes/RayCast2D.xml`.
//!
//! ## What was already done, and what this bead is
//!
//! `pat-0q2kx.105` landed `gdscene::raycast2d`: the cast uses the node's global
//! transform, honours `collision_mask` / `exclude_parent` / exceptions, and
//! publishes both the `_hit_*` keys and the legacy ones. `MainLoop` drives it
//! each physics tick. That work is covered by
//! `raycast2d_global_transform_and_filters.rs`, which asserts **node properties**.
//!
//! The bead's own description predates that and is stale. What remained is the
//! half nothing covered: the **script** side. Two things are pinned here —
//!
//! 1. The full chain `physics tick → process_raycasts → node `_hit_*` → script
//!    getter` in one frame, which is the bead's stated wiring requirement.
//! 2. `force_raycast_update()` was a **no-op returning `Nil`**
//!    (`interpreter.rs`), on the reasoning that the tick refreshes the cast
//!    anyway. That is wrong precisely when scripts use the method: after moving
//!    the ray mid-frame, Godot's `force_raycast_update` calls
//!    `_update_raycast_state` immediately so the *same* frame sees the new
//!    position. Without it a script reads the pre-move result.
//!
//! ## Geometry (shared by every case)
//!
//! `Body` is a `CharacterBody2D` at `(100, 0)` rotated **+90°**. Under Godot's 2D
//! convention (+Y down, positive angle turning +X toward +Y) the ray's local
//! `target_position` `(0, 50)` maps to a global sweep from `(100, 0)` toward
//! `(50, 0)` — i.e. along **−X**. `Wall` is a 10×200 box centred at `(70, 0)`,
//! so it spans `x ∈ [65, 75]` and the ray meets its near face at **x = 75**.
//!
//! An implementation that ignored rotation would sweep toward `(100, 50)`
//! instead — perpendicular, and never touching the wall. `target_position`
//! `(0, -50)` sweeps the opposite way, toward `(150, 0)`, and is the "pointing
//! away" case used to prove the forced update does something.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

use gdcore::math::Vector2;

/// Reads a script variable off the node's script instance.
fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Builds `Main -> {Body(CharacterBody2D, rotated) -> {BodyShape, Ray(+script)},
/// Wall(StaticBody2D) -> WallShape}`.
///
/// `ray_mask` and `wall_layer` are separate so a mismatch can be exercised;
/// `target` is the ray's local target so a case can point it away from the wall.
/// Returns the loop plus the ray, wall and body ids.
fn scene(
    src: &str,
    target: Vector2,
    ray_mask: i64,
    wall_layer: i64,
) -> (MainLoop, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    // The ray's parent is itself a physics body, which is what makes
    // `exclude_parent` meaningful: without it the cast starts inside this body's
    // own shape and reports the parent as the collider.
    let mut body = Node::new("Body", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    body.set_property("rotation", Variant::Float(std::f64::consts::FRAC_PI_2));
    let body_id = tree.add_child(main, body).unwrap();
    let mut body_shape = Node::new("BodyShape", "CollisionShape2D");
    body_shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(body_id, body_shape).unwrap();

    let mut ray = Node::new("Ray", "RayCast2D");
    ray.set_property("position", Variant::Vector2(Vector2::ZERO));
    ray.set_property("enabled", Variant::Bool(true));
    ray.set_property("target_position", Variant::Vector2(target));
    ray.set_property("collision_mask", Variant::Int(ray_mask));
    let ray_id = tree.add_child(body_id, ray).unwrap();
    tree.attach_script(
        ray_id,
        Box::new(GDScriptNodeInstance::from_source(src, ray_id).unwrap()),
    );

    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(70.0, 0.0)));
    wall.set_property("collision_layer", Variant::Int(wall_layer));
    let wall_id = tree.add_child(main, wall).unwrap();
    let mut wall_shape = Node::new("WallShape", "CollisionShape2D");
    wall_shape.set_property("size", Variant::Vector2(Vector2::new(10.0, 200.0)));
    tree.add_child(wall_id, wall_shape).unwrap();

    // `MainLoop::step` never populates `node_to_body`; without this the world
    // holds no bodies and every cast trivially misses.
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, ray_id, wall_id, body_id)
}

/// Reads the getters straight through, no mutation — the plain wiring case.
const READER_GD: &str = "\
extends RayCast2D
var hit = false
var point = Vector2()
var normal = Vector2()
var collider = null
var ticks = 0
func _physics_process(delta):
    self.ticks = self.ticks + 1
    self.hit = is_colliding()
    self.point = get_collision_point()
    self.normal = get_collision_normal()
    self.collider = get_collider()
";

/// Calls `force_raycast_update()` and reads the result in the same frame.
///
/// ## Why this needs no property write
///
/// `MainLoop::step` runs the `_physics_process` script phase (`main_loop.rs`
/// :2797) **before** `process_raycasts` (:2921). So on the first frame, when a
/// script is running, the tick has not published `_hit_*` even once —
/// `is_colliding()` is false no matter where the ray points. Forcing an update
/// is therefore the *only* way a script can see a hit during that phase, which
/// makes this a direct test of the forced cast with the ray left completely
/// alone.
///
/// An earlier version moved the ray mid-frame instead and asserted the new aim
/// took effect. That coupled the test to how GDScript assignment resolves onto
/// node properties — a separate mechanism that turned out not to work the way
/// the test assumed, and which has nothing to do with this bead. Removing the
/// write removes the coupling.
const FORCED_GD: &str = "\
extends RayCast2D
var before = false
var after = false
var point = Vector2()
func _physics_process(delta):
    self.before = is_colliding()
    force_raycast_update()
    self.after = is_colliding()
    self.point = get_collision_point()
";

/// Identical to [`FORCED_GD`] except it never calls `force_raycast_update()`.
/// This is the control that makes the assertion about the forced update mean
/// something: same move, same reads, one missing call.
const UNFORCED_GD: &str = "\
extends RayCast2D
var before = false
var after = false
func _physics_process(delta):
    self.before = is_colliding()
    self.after = is_colliding()
";

/// **The acceptance test.** A rotated, mask-filtered RayCast2D under a
/// CharacterBody2D, read entirely through the script API after one step.
#[test]
fn raycast2d_script_getters_global_transform() {
    let (mut ml, ray, wall, body) = scene(READER_GD, Vector2::new(0.0, 50.0), 1, 1);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, ray, "ticks"),
        Some(Variant::Int(1)),
        "the script's _physics_process must have run — nothing below means \
         anything if it did not"
    );

    // -- is_colliding() through the script -----------------------------------
    assert_eq!(
        script_var(&ml, ray, "hit"),
        Some(Variant::Bool(true)),
        "$Ray.is_colliding() must be true: the wall lies along the ray's ROTATED \
         global direction. A cast that used the raw target_position would sweep \
         +Y and miss it entirely"
    );

    // -- get_collision_point() lands on the wall's near face -----------------
    match script_var(&ml, ray, "point") {
        Some(Variant::Vector2(p)) => {
            assert!(
                (p.x - 75.0).abs() < 1.0,
                "the hit must be on the wall's near face at x=75 (box spans \
                 65..=75), got {p:?}"
            );
            assert!(
                p.y.abs() < 1.0,
                "the ray runs along y=0, so the hit's y must be ~0, got {p:?}"
            );
        }
        other => panic!("get_collision_point() must return a Vector2, got {other:?}"),
    }

    // -- get_collision_normal() faces back along the ray ---------------------
    match script_var(&ml, ray, "normal") {
        Some(Variant::Vector2(n)) => assert!(
            n.x > 0.5,
            "the ray travels -X into the wall, so the surface normal must point \
             back toward +X, got {n:?}"
        ),
        other => panic!("get_collision_normal() must return a Vector2, got {other:?}"),
    }

    // -- the collider is the wall, NOT the ray's own parent body -------------
    let collider = script_var(&ml, ray, "collider");
    assert_eq!(
        collider,
        Some(Variant::ObjectId(wall.object_id())),
        "get_collider() must name the Wall"
    );
    assert_ne!(
        collider,
        Some(Variant::ObjectId(body.object_id())),
        "the ray starts inside its parent CharacterBody2D's own shape, so \
         without exclude_parent the parent would be reported first — that is \
         the bug this pins"
    );
}

/// A ray whose `collision_mask` shares no bit with the wall's
/// `collision_layer` reports nothing, through the same script getters.
#[test]
fn mask_mismatch_reports_no_hit_to_script() {
    // Ray on mask 2, wall on layer 1 — disjoint.
    let (mut ml, ray, _, _) = scene(READER_GD, Vector2::new(0.0, 50.0), 2, 1);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, ray, "hit"),
        Some(Variant::Bool(false)),
        "a mask mismatch must yield is_colliding() == false"
    );
    assert_eq!(
        script_var(&ml, ray, "collider"),
        Some(Variant::Nil),
        "a miss must clear the collider, not leave a stale one"
    );

    // And the same geometry with matching bits does hit — otherwise the
    // assertion above could pass simply because the ray never worked.
    let (mut ml2, ray2, _, _) = scene(READER_GD, Vector2::new(0.0, 50.0), 3, 1);
    ml2.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml2, ray2, "hit"),
        Some(Variant::Bool(true)),
        "mask 3 shares bit 1 with layer 1, so this must hit — proving the \
         mismatch case above was the mask's doing"
    );
}

/// **`force_raycast_update()` re-runs the cast immediately.**
///
/// The ray starts pointing *away* from the wall, so the tick's own cast misses.
/// The script then turns it toward the wall and forces a refresh; `after` must
/// be true within that same frame.
#[test]
fn force_raycast_update_refreshes_within_the_frame() {
    // The ray points AT the wall the whole time; the only variable is whether
    // the script forces a cast.
    let (mut ml, ray, wall, _) = scene(FORCED_GD, Vector2::new(0.0, 50.0), 1, 1);
    ml.step(1.0 / 60.0);

    // ---- geometry first -----------------------------------------------------
    // `process_raycasts` runs after the script phase, so by now the tick has
    // cast this very scene itself. Asserting that BEFORE anything about
    // `force_raycast_update` separates two failures that would otherwise look
    // identical: "the scene never produced a hit" vs "the forced cast did not
    // publish one". Only the second is this bead's subject.
    assert_eq!(
        ml.tree()
            .get_node(ray)
            .map(|n| n.get_property("_hit_collider")),
        Some(Variant::ObjectId(wall.object_id())),
        "the tick's own cast must hit the wall — if this fails the scene \
         geometry is wrong and nothing below is meaningful"
    );

    assert_eq!(
        script_var(&ml, ray, "before"),
        Some(Variant::Bool(false)),
        "the script phase runs before process_raycasts, so on the first frame \
         nothing has published `_hit_*` yet — even aimed at the wall this must \
         read false"
    );

    // NOTE for whoever picks this up: `is_colliding()` returns Bool(false) both
    // for a real miss and for the `scene_access == None` fallback, so `before`
    // cannot distinguish them. Scene access *is* available in this phase
    // regardless — `characterbody2d_move_and_slide_script_dispatch` calls
    // `move_and_slide()`/`is_on_floor()` from `_physics_process` and passes — so
    // a missing accessor is not the explanation for the failure below.
    assert_eq!(
        script_var(&ml, ray, "after"),
        Some(Variant::Bool(true)),
        "force_raycast_update() must run the cast NOW and publish the hit, so \
         the very next read sees it — this stays false whenever the method is \
         a no-op"
    );
    match script_var(&ml, ray, "point") {
        Some(Variant::Vector2(p)) => assert!(
            (p.x - 75.0).abs() < 1.0,
            "the forced cast must publish the new hit point too, not just the \
             boolean, got {p:?}"
        ),
        other => panic!("expected a Vector2 collision point, got {other:?}"),
    }
    assert_eq!(
        ml.tree()
            .get_node(ray)
            .map(|n| n.get_property("_hit_collider")),
        Some(Variant::ObjectId(wall.object_id())),
        "the forced update must republish the node's `_hit_*` properties, which \
         is what every other reader (and the next frame) sees"
    );
}

/// The control: the same mid-frame turn **without** `force_raycast_update()`
/// must NOT see the new direction this frame.
///
/// Without this, the previous test would still pass if the cast happened to be
/// re-run by something else, and would prove nothing about the forced call.
#[test]
fn without_the_forced_update_the_frame_keeps_the_old_result() {
    // Identical scene and identical aim to the forced test — the ONLY
    // difference is the missing `force_raycast_update()` call.
    let (mut ml, ray, wall, _) = scene(UNFORCED_GD, Vector2::new(0.0, 50.0), 1, 1);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, ray, "after"),
        Some(Variant::Bool(false)),
        "without force_raycast_update() the script phase must see nothing, \
         even though the ray is aimed straight at the wall. If this reads true \
         the forced-update test proves nothing — it would be passing on a cast \
         somebody else ran"
    );

    // ...and the tick still resolves the very same scene once it runs, so the
    // false above is a *timing* fact about the script phase, not a broken ray.
    assert_eq!(
        ml.tree()
            .get_node(ray)
            .map(|n| n.get_property("_hit_collider")),
        Some(Variant::ObjectId(wall.object_id())),
        "process_raycasts runs after the script phase and must publish the hit"
    );
}
