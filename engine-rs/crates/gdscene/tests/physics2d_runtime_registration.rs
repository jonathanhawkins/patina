//! pat-s3rrv.25 — 2D physics bodies join and leave the world as their nodes
//! enter and leave the scene tree.
//!
//! Godot does this per node: `CollisionObject2D::_notification` calls
//! `PhysicsServer2D::body_set_space` / `area_set_space` on
//! `NOTIFICATION_ENTER_TREE` and clears the space on `NOTIFICATION_EXIT_TREE`
//! (`upstream/godot/scene/2d/physics/collision_object_2d.cpp`). So a body
//! instanced mid-game simulates on the very next physics tick, and a freed body
//! stops colliding immediately.
//!
//! Patina built 2D bodies **exactly once**. `PhysicsServer::register_bodies` was
//! reachable only through `MainLoop::register_physics_bodies`, which
//! `patina-runner` calls a single time before the frame loop starts;
//! `MainLoop::step`'s physics section synced and stepped but never re-scanned.
//! And nothing ever removed a scene-node body — `world.remove_body` was called
//! only from the `PhysicsServer2D` RID-free path and tilemap regeneration.
//!
//! The two consequences are what these tests pin:
//!   * `add_child(preload("res://Bullet.tscn").instantiate())` produced a
//!     RigidBody2D that never fell, never collided, and emitted no signals.
//!   * `queue_free()` on a body left a permanent **ghost collider** — the node
//!     was gone from the tree but its body kept blocking everything.
//!
//! The 3D server already re-scanned the tree every tick inside its own
//! `sync_to_physics` (`physics_server_3d.rs`), so these assertions describe
//! behaviour 3D already had and 2D did not.
//!
//! Every test asserts on the **world**, not on a return value: body counts,
//! `body_for_node`, and simulated positions. A registration call that returned
//! success while adding nothing would pass a weaker test.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

/// Reads a node's `position`, failing loudly rather than defaulting — a body
/// that never registered keeps whatever it was authored with, and a silent
/// `Vector2::ZERO` fallback would make that look like a real measurement.
fn position(ml: &MainLoop, node: NodeId) -> Vector2 {
    match ml.tree().get_node(node).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        other => panic!("`position` should be a Vector2, got {other:?}"),
    }
}

/// Adds a physics body node with a rectangular collider under `parent`.
///
/// Collision geometry comes from a child `CollisionShape2D` with a `size`,
/// which is what the server's `find_collision_shape` reads.
fn add_body(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    class: &str,
    at: Vector2,
    size: Vector2,
) -> NodeId {
    let mut node = Node::new(name, class);
    node.set_property("position", Variant::Vector2(at));
    let id = tree.add_child(parent, node).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(size));
    tree.add_child(id, shape).unwrap();
    id
}

/// `Main -> Platform` (a wide `StaticBody2D` at y=200), booted the way
/// `patina-runner` boots: one `register_physics_bodies()` before the first step.
///
/// Deliberately contains **no** RigidBody2D — each test adds its own after boot,
/// which is the whole point.
fn booted_scene() -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let platform = add_body(
        &mut tree,
        main,
        "Platform",
        "StaticBody2D",
        Vector2::new(0.0, 200.0),
        Vector2::new(1000.0, 20.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, main, platform)
}

/// **The acceptance test.** A body added to the tree after boot simulates.
///
/// Asserted in links so a failure names which half broke: registration, or the
/// simulation that registration was supposed to enable.
#[test]
fn physics2d_runtime_spawned_body_simulates() {
    let (mut ml, main, _platform) = booted_scene();

    let booted_bodies = ml.physics_server().body_count();
    assert_eq!(
        booted_bodies, 1,
        "fixture check: only the Platform is a body at boot",
    );

    // The spawn. `add_child` on a live tree is what
    // `preload(...).instantiate()` + `add_child` reduces to once the scene is
    // built, and it is the moment Godot fires NOTIFICATION_ENTER_TREE.
    let spawned = add_body(
        ml.tree_mut(),
        main,
        "Bullet",
        "RigidBody2D",
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 10.0),
    );

    // Link 0 — the fixture is honest: adding the node does not itself register
    // anything. If some future `add_child` hook starts registering bodies
    // directly, this test would otherwise keep passing while proving nothing
    // about the per-tick scan.
    assert!(
        ml.physics_server().body_for_node(spawned).is_none(),
        "link 0: `add_child` alone must not register the body — the tick does",
    );

    ml.step(DT);

    // Link 1 — the per-tick scan picked the new node up.
    assert!(
        ml.physics_server().body_for_node(spawned).is_some(),
        "link 1: the first physics tick after the spawn must register the body",
    );
    assert_eq!(
        ml.physics_server().body_count(),
        booted_bodies + 1,
        "link 1: body count must track the tree",
    );

    // Link 2 — it is really in the simulation, not merely in a map. In Godot's
    // 2D screen space +y is down, so falling means y increases.
    let start = position(&ml, spawned);
    for _ in 0..30 {
        ml.step(DT);
    }
    let fallen = position(&ml, spawned);
    assert!(
        fallen.y > start.y,
        "link 2: the spawned body must fall under gravity, went {start:?} -> {fallen:?}",
    );

    // Link 3 — it collides with what was already there. The platform's top face
    // is at y = 200 - 10 = 190; a body that fell through would keep going.
    for _ in 0..180 {
        ml.step(DT);
    }
    let landed = position(&ml, spawned);
    assert!(
        landed.y < 200.0,
        "link 3: the spawned body must land on the Platform, not fall through it — y={}",
        landed.y,
    );
}

/// The other half: a freed body must stop colliding, rather than leaving a
/// ghost collider behind at its old position.
#[test]
fn physics2d_freed_body_stops_colliding() {
    let (mut ml, main, platform) = booted_scene();

    let faller = add_body(
        ml.tree_mut(),
        main,
        "Faller",
        "RigidBody2D",
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 10.0),
    );

    // Settle onto the platform first, so the "falls through" assertion below
    // cannot pass simply because the body never reached it.
    for _ in 0..240 {
        ml.step(DT);
    }
    let resting = position(&ml, faller);
    assert!(
        resting.y < 200.0 && resting.y > 0.0,
        "precondition: the body must be resting on the Platform, got y={}",
        resting.y,
    );

    // Precondition, and the whole reason this test is sharper than it looks:
    // after 4s at rest the body is ASLEEP (`time_before_sleep` is 0.5s) and no
    // longer integrates. So the fall asserted below cannot happen unless
    // removing the platform actively woke it — which is what Godot's space does
    // when it destroys a departing body's contacts. Without that wake the body
    // hangs in mid-air on a support that no longer exists, and the first version
    // of this test caught exactly that: y=185.3 before the free, y=185.3 after.
    let faller_body = ml
        .physics_server()
        .body_for_node(faller)
        .expect("the faller is registered");
    assert!(
        ml.physics_server()
            .world()
            .get_body(faller_body)
            .expect("the faller has a world body")
            .sleeping,
        "precondition: the body must have fallen asleep while resting, or this \
         test cannot tell a wake-on-removal from ordinary gravity",
    );

    let before = ml.physics_server().body_count();
    ml.tree_mut().remove_node(platform).unwrap();

    ml.step(DT);

    // The registration side: the entry is gone.
    assert!(
        ml.physics_server().body_for_node(platform).is_none(),
        "a freed node's body registration must be dropped",
    );
    assert_eq!(
        ml.physics_server().body_count(),
        before - 1,
        "body count must track the tree downwards too, not just upwards",
    );

    // The behavioural side, which is what a player would notice: the collider
    // is really gone from the world, not just from the map.
    for _ in 0..120 {
        ml.step(DT);
    }
    let after = position(&ml, faller);
    assert!(
        after.y > resting.y + 1.0,
        "the freed Platform must stop blocking: body sat at y={} and is now at y={} \
         — it is resting on a ghost collider",
        resting.y,
        after.y,
    );
}

/// Areas follow the same rule as bodies. Tracked separately in the server
/// (`node_to_area`), so a fix that only pruned `node_to_body` would leave a
/// freed `Area2D` detecting overlaps forever.
#[test]
fn physics2d_area_registration_tracks_the_tree() {
    let (mut ml, main, _platform) = booted_scene();

    assert_eq!(
        ml.physics_server().area_count(),
        0,
        "fixture check: no areas at boot",
    );

    let area = add_body(
        ml.tree_mut(),
        main,
        "Trigger",
        "Area2D",
        Vector2::new(0.0, 100.0),
        Vector2::new(50.0, 50.0),
    );

    ml.step(DT);
    assert!(
        ml.physics_server().area_for_node(area).is_some(),
        "an Area2D added after boot must register on the next tick",
    );
    assert_eq!(ml.physics_server().area_count(), 1);

    ml.tree_mut().remove_node(area).unwrap();
    ml.step(DT);

    assert!(
        ml.physics_server().area_for_node(area).is_none(),
        "a freed Area2D must stop being tracked",
    );
    assert_eq!(
        ml.physics_server().area_count(),
        0,
        "a freed Area2D must leave the area store",
    );
}

/// Same as [`add_body`], but the `CollisionShape2D` child carries its own
/// `position` — Godot's shape-owner transform.
fn add_body_with_collider_offset(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    class: &str,
    at: Vector2,
    size: Vector2,
    collider_offset: Vector2,
) -> NodeId {
    let mut node = Node::new(name, class);
    node.set_property("position", Variant::Vector2(at));
    let id = tree.add_child(parent, node).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(size));
    shape.set_property("position", Variant::Vector2(collider_offset));
    tree.add_child(id, shape).unwrap();
    id
}

/// A `CollisionShape2D` authored with its own `position` collides where it is
/// drawn, not at the body's origin (pat-s3rrv.25).
///
/// Godot feeds that child transform to the shape owner
/// (`collision_shape_2d.cpp::_update_in_shape_owner` →
/// `shape_owner_set_transform`). Patina read the body's transform and nothing
/// else, so every offset collider silently collided 'somewhere else' — a
/// character whose collider sits at its feet, a hitbox on a muzzle.
///
/// **The resting height decodes straight from the scene constants**, which is
/// what makes this test able to tell the two behaviours apart rather than just
/// asserting "it landed":
///
/// ```text
/// ground body y            = 300, half-height 10  -> top surface y = 290
/// faller collider          = 20x20, half-height 10
/// faller collider offset   = (0, 100) below the body origin
///
/// offset honoured : collider bottom rests on 290 -> collider centre 280
///                                                -> BODY rests at y = 180
/// offset ignored  : collider sits at the body    -> BODY rests at y = 280
/// ```
///
/// The 100px gap between those two answers is the bug, and it is exactly the
/// authored offset.
#[test]
fn collision_shape2d_offset_places_the_collider_not_the_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    add_body(
        &mut tree,
        main,
        "Ground",
        "StaticBody2D",
        Vector2::new(0.0, 300.0),
        Vector2::new(1000.0, 20.0),
    );
    let faller = add_body_with_collider_offset(
        &mut tree,
        main,
        "Faller",
        "RigidBody2D",
        Vector2::new(0.0, 0.0),
        Vector2::new(20.0, 20.0),
        Vector2::new(0.0, 100.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    for _ in 0..240 {
        ml.step(DT);
    }

    let resting = position(&ml, faller).y;

    // The precise assertion: the body settles 100px higher than it would if the
    // collider were pinned to its origin, because the collider hangs 100px below.
    assert!(
        (resting - 180.0).abs() < 5.0,
        "an authored CollisionShape2D offset of (0, 100) must place the collider \
         100px below the body, so the body rests at y=180 — got y={resting}",
    );

    // Stated separately so a failure names the actual bug rather than only
    // reporting a number that is off. This is the value the old code produced.
    assert!(
        (resting - 280.0).abs() > 5.0,
        "the body rested at y={resting}, which is where it lands when the \
         collider's own transform is ignored and the shape collides at the body \
         origin — the shape-owner transform is not reaching the physics world",
    );
}

/// How many bodies the area reports overlapping, read from the property
/// `Area2D.get_overlapping_bodies()` answers from.
fn overlap_count(ml: &MainLoop, area: NodeId) -> usize {
    match ml
        .tree()
        .get_node(area)
        .unwrap()
        .get_property("_overlapping_bodies")
    {
        Variant::Array(items) => items.len(),
        _ => 0,
    }
}

/// An `Area2D` detects where its collider is drawn, not at the area's origin
/// (pat-s3rrv.25).
///
/// The body half of this fix landed first, which briefly left the engine
/// inconsistent in the most confusing way available: a `StaticBody2D` and an
/// `Area2D` authored with the *same* offset collider would disagree about where
/// they were. This closes that.
///
/// **Two scenes rather than two bodies in one**, because a single scene cannot
/// tell the behaviours apart: with one body at the offset and one at the origin,
/// the count is 1 either way and only *which* body was caught differs. Split in
/// two, the honoured and ignored cases give opposite answers to both halves —
/// if the offset were dropped, `hits_at_offset` would be 0 and `hits_at_origin`
/// would be 1, so each assertion fails on its own.
#[test]
fn area2d_collider_offset_moves_where_it_detects() {
    // The collider hangs 200px to the right of the area's own origin.
    let collider_offset = Vector2::new(200.0, 0.0);

    let scene = |body_at: Vector2| {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
        let trigger = add_body_with_collider_offset(
            &mut tree,
            main,
            "Trigger",
            "Area2D",
            Vector2::ZERO,
            Vector2::new(40.0, 40.0),
            collider_offset,
        );
        add_body(
            &mut tree,
            main,
            "Ball",
            "RigidBody2D",
            body_at,
            Vector2::new(8.0, 8.0),
        );
        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();
        // A single tick: one frame of gravity moves the body ~0.27px, far
        // inside the 40px collider, so the result is about geometry and not
        // about how far the ball fell.
        ml.step(DT);
        overlap_count(&ml, trigger)
    };

    let hits_at_offset = scene(collider_offset);
    let hits_at_origin = scene(Vector2::ZERO);

    assert_eq!(
        hits_at_offset, 1,
        "a body sitting under the area's authored collider offset (200, 0) must \
         be detected — if this is 0 the area is still monitoring at its own \
         origin and ignoring the shape-owner transform",
    );
    assert_eq!(
        hits_at_origin, 0,
        "a body at the area's ORIGIN must NOT be detected: the collider was \
         authored 200px away. If this is 1, the collider is being placed at the \
         area rather than where the scene draws it",
    );
}

/// An `Area2D` under a scaled parent monitors at its **scaled** extent
/// (pat-s3rrv.25).
///
/// Bodies have scaled their collision geometry since pat-0q2kx.31
/// (`scale_shape` on the accumulated `get_global_scale`); the area registration
/// path used its shape raw. So an `Area2D` drawn twice as wide in the editor
/// still fired at its authored size, and a body and an area drawn the same size
/// under the same scaled parent disagreed about their own extent.
///
/// **The body sits in the gap between the two answers**, which is what makes
/// this decisive rather than merely plausible:
///
/// ```text
/// collider          = 40x40  -> half-extent 20 unscaled, 40 under scale 2
/// probe body at x   = 30     -> INSIDE the scaled extent, OUTSIDE the raw one
/// control body at x = 50     -> outside both
/// ```
///
/// The control matters: without it, an area that had accidentally become
/// enormous would satisfy the first assertion just as well as a correctly
/// scaled one.
#[test]
fn area2d_under_a_scaled_parent_monitors_at_scaled_size() {
    let overlaps_at = |ball_x: f32| {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // The scale lives on the PARENT, so it has to reach the area through
        // `get_global_scale`'s accumulation rather than off the area's own node.
        let scaled = tree.add_child(root, Node::new("Scaled", "Node2D")).unwrap();
        tree.get_node_mut(scaled)
            .unwrap()
            .set_property("scale", Variant::Vector2(Vector2::new(2.0, 2.0)));

        let trigger = add_body(
            &mut tree,
            scaled,
            "Trigger",
            "Area2D",
            Vector2::ZERO,
            Vector2::new(40.0, 40.0),
        );

        // Deliberately NOT under `Scaled`: the probe must keep its authored
        // world position, or the parent's scale would move it too and the test
        // would compare a moving body against a growing area.
        add_body(
            &mut tree,
            root,
            "Ball",
            "RigidBody2D",
            Vector2::new(ball_x, 0.0),
            Vector2::new(4.0, 4.0),
        );

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();
        ml.step(DT);
        overlap_count(&ml, trigger)
    };

    assert_eq!(
        overlaps_at(30.0),
        1,
        "a body at x=30 is inside the area's SCALED half-extent of 40 — if this \
         is 0 the area is still monitoring its raw 20px half-extent and ignoring \
         the parent's scale",
    );
    assert_eq!(
        overlaps_at(50.0),
        0,
        "a body at x=50 is outside even the scaled half-extent of 40 — if this \
         is 1 the area grew by more than the authored scale",
    );
}

/// Moves the `CollisionShape` child of `body` to a new local offset.
fn move_collider(tree: &mut SceneTree, body: NodeId, to: Vector2) {
    let child = tree
        .get_node(body)
        .unwrap()
        .children()
        .first()
        .copied()
        .expect("the body has a collider child");
    tree.get_node_mut(child)
        .unwrap()
        .set_property("position", Variant::Vector2(to));
}

/// A `CollisionShape2D` that moves at runtime takes its collision with it
/// (pat-s3rrv.25).
///
/// Godot re-pushes the shape-owner transform on every `CollisionShape2D`
/// transform change (`collision_shape_2d.cpp::_update_in_shape_owner`). Patina
/// read the collider's offset **once, at registration**, so a crouch animation
/// that drops the collider, or a door whose hitbox swings, left the collision
/// behind at the authored spot.
///
/// The area is checked alongside the body on purpose: these two have drifted
/// apart once already in this bead, and a live body paired with a frozen area
/// would be the same class of inconsistency again.
#[test]
fn a_collider_that_moves_at_runtime_takes_its_collision_with_it() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    // Both start with their collider ON the node origin, so the probe at
    // (200, 0) is outside each of them.
    let trigger = add_body(
        &mut tree,
        main,
        "Trigger",
        "Area2D",
        Vector2::ZERO,
        Vector2::new(40.0, 40.0),
    );
    add_body(
        &mut tree,
        main,
        "Ball",
        "RigidBody2D",
        Vector2::new(200.0, 0.0),
        Vector2::new(8.0, 8.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    // Baseline. If this were already 1 the test could not attribute the change
    // below to the collider having moved.
    assert_eq!(
        overlap_count(&ml, trigger),
        0,
        "baseline: with its collider at the origin, the area must not reach a \
         body 200px away",
    );

    // The runtime move — the thing an AnimationPlayer does to a hitbox.
    move_collider(ml.tree_mut(), trigger, Vector2::new(200.0, 0.0));
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        1,
        "after its CollisionShape2D moved to (200, 0) the area must monitor \
         there — if this is 0 the shape-owner transform is still frozen at the \
         value read when the area was first registered",
    );

    // And back again: the offset must track the collider both ways, not just
    // latch the first time it changes.
    move_collider(ml.tree_mut(), trigger, Vector2::ZERO);
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        0,
        "moving the collider back must stop the overlap — a one-way update \
         would leave the collision stuck wherever it last landed",
    );
}

/// The `CollisionShape` child of `body` — the one `find_collision_shape` picks.
fn collider_of(tree: &SceneTree, body: NodeId) -> NodeId {
    tree.get_node(body)
        .unwrap()
        .children()
        .first()
        .copied()
        .expect("the body has a collider child")
}

/// A `CollisionShape2D` that is RESIZED at runtime collides at its new size
/// (pat-s3rrv.25).
///
/// The transform was made live first; the geometry was still read once at
/// registration, so a hitbox that grows for a single attack frame — or any
/// `size` driven by an AnimationPlayer — did nothing.
///
/// Sizes are chosen so the probe sits between the two answers: half-extent 20
/// excludes a body at x=60, half-extent 100 includes it.
#[test]
fn a_collider_that_resizes_at_runtime_collides_at_its_new_size() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let trigger = add_body(
        &mut tree,
        main,
        "Trigger",
        "Area2D",
        Vector2::ZERO,
        Vector2::new(40.0, 40.0),
    );
    add_body(
        &mut tree,
        main,
        "Ball",
        "RigidBody2D",
        Vector2::new(60.0, 0.0),
        Vector2::new(8.0, 8.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        0,
        "baseline: a 40x40 collider (half-extent 20) must not reach a body at x=60",
    );

    let collider = collider_of(ml.tree(), trigger);
    ml.tree_mut()
        .get_node_mut(collider)
        .unwrap()
        .set_property("size", Variant::Vector2(Vector2::new(200.0, 200.0)));
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        1,
        "after its CollisionShape2D grew to 200x200 (half-extent 100) the area \
         must reach the body at x=60 — if this is 0 the shape is still the one \
         resolved when the area was first registered",
    );

    // Shrink back: the geometry must track the scene, not latch at its largest.
    ml.tree_mut()
        .get_node_mut(collider)
        .unwrap()
        .set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        0,
        "shrinking the collider back must drop the overlap — a grow-only update \
         would leave every hitbox stuck at its widest frame",
    );
}

/// Disabling a `CollisionShape2D` at runtime removes its collision, and
/// re-enabling restores it (pat-s3rrv.25).
///
/// Godot drops a `disabled` shape from the body's shape list
/// (`collision_shape_2d.cpp`), which is how games turn hitboxes on and off.
/// This falls out of reading the geometry live, but it is asserted separately
/// because it goes through a different arm — the degenerate-rect fallback that
/// `register_bodies` uses for "shape children exist but none is usable".
#[test]
fn disabling_a_collider_at_runtime_removes_its_collision() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let trigger = add_body(
        &mut tree,
        main,
        "Trigger",
        "Area2D",
        Vector2::ZERO,
        Vector2::new(200.0, 200.0),
    );
    add_body(
        &mut tree,
        main,
        "Ball",
        "RigidBody2D",
        Vector2::new(60.0, 0.0),
        Vector2::new(8.0, 8.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        1,
        "baseline: a 200x200 collider must reach the body at x=60",
    );

    let collider = collider_of(ml.tree(), trigger);
    ml.tree_mut()
        .get_node_mut(collider)
        .unwrap()
        .set_property("disabled", Variant::Bool(true));
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        0,
        "a disabled CollisionShape2D must contribute no collision — Godot drops \
         it from the body's shape list entirely",
    );

    ml.tree_mut()
        .get_node_mut(collider)
        .unwrap()
        .set_property("disabled", Variant::Bool(false));
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        1,
        "re-enabling must restore the collision — a one-way disable would make \
         every temporarily-disabled hitbox permanently dead",
    );
}

// ---------------------------------------------------------------------------
// MULTI-SHAPE STATUS — pat-s3rrv.25 (no remaining gap)
// ---------------------------------------------------------------------------
//
// Multi-collider bodies are DONE. `PhysicsBody2D` and `Area2D` each carry
// `extra_shapes` plus a `shape_owners()` accessor, and every consumer iterates
// it: body-vs-body, both area-vs-body sites, area-vs-area, all five
// `character.rs` sweeps, `point_query`, `point_query_first`, the shape cast and
// `raycast_shape`. Picking reports the real `shape_idx`, and the read-only
// `shape_owner_*` script API answers from the collider children. Each of those
// is covered by a test in this file or in
// `collision_object_input_event_godot_signature.rs`.
//
// The LAST item — per-shape `one_way_collision` — is now done too.
// `shape_owners()` yields `ShapeOwner2D { shape, transform, one_way_collision,
// one_way_direction }`; `extra_collision_shapes` reads each child's own
// `one_way_collision` the same way it reads the transform; `world.rs`'s pair
// loop and `character.rs`'s `deepest_hit_against` both ask the OWNER rather
// than the body. A platformer character can now carry a solid torso collider
// and a one-way foot sensor at once. Pinned by
// `gdphysics2d/tests/per_shape_one_way_collision.rs` (behaviour) and
// `each_collider_child_carries_its_own_one_way_flag` below (the scene wiring).
//
//   The BODY-level `one_way_collision` / `one_way_direction` fields did not go
//   away: they are owner 0's flags, which is why every scene authored with a
//   single one-way collider behaves exactly as it did.
//
// STILL-TRUE TRAPS from the original inventory:
//
//   1. `raycast_shape` needs its composed transform RIGID — rotation and
//      translation, no scale — so a hit distance round-trips through
//      `affine_inverse`. Collider scale stays baked into the geometry by
//      `scale_shape`. No test in the physics chain catches this if you get it
//      wrong.
//   2. Composition order is `body_tf * shape_tf`: `impl Mul for Transform2D`
//      documents that `a * b` applies `b` first.
//   3. `find_collision_shape`, `find_collision_shape_transform`,
//      `extra_collision_shapes`, `collider_local_transforms` AND
//      `one_way_collision_for` all walk the collider children with the SAME
//      `disabled` skip and usable-shape test. They must stay in lockstep or
//      shape-owner ids stop matching the `shape_idx` picking reports. This is
//      not hypothetical: `one_way_collision_for` used to filter on
//      `class_name() == "CollisionShape2D"` instead of `shape_from_node`, so on
//      a body whose first collider is a `CollisionPolygon2D` it skipped past it
//      and lifted a LATER sibling's one-way flag onto the polygon.
//   4. 3D still sends a constant `shape_idx` of 0 from `main_loop.rs`, which is
//      correct only because `PhysicsBody3D` has no collider list. It becomes
//      wrong the moment 3D gains one.
//
// The `shape_owner_*` MUTATORS (`create_shape_owner`, `shape_owner_add_shape`,
// `shape_owner_set_transform`, `remove_shape_owner`) are deliberately NOT
// registered in ClassDB: they have no dispatch, and registering them would make
// `has_method` answer true for methods that do nothing.

/// Adds a SECOND `CollisionShape2D` child to `body`, offset from the first.
fn add_second_collider(tree: &mut SceneTree, body: NodeId, at: Vector2, size: Vector2) {
    let mut shape = Node::new("CollisionShape2", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(size));
    shape.set_property("position", Variant::Vector2(at));
    tree.add_child(body, shape).unwrap();
}

/// `add_second_collider` with Godot's per-shape `one_way_collision` set on the
/// child, which is where Godot puts it (collision_shape_2d.cpp).
fn add_one_way_collider(tree: &mut SceneTree, body: NodeId, at: Vector2, size: Vector2) {
    let mut shape = Node::new("OneWayCollider", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(size));
    shape.set_property("position", Variant::Vector2(at));
    shape.set_property("one_way_collision", Variant::Bool(true));
    tree.add_child(body, shape).unwrap();
}

// STATUS: the whole narrow phase is multi-shape — body-vs-body (world.rs's pair
// loop), both area-vs-body sites, area-vs-area, and all five character.rs
// sweeps, on BOTH sides of every test. Bodies and areas each carry
// `extra_shapes` and a `shape_owners()` accessor.
//
// The point/ray queries are converted too (world.rs `point_query`,
// `point_query_first`, the ray's hit-from-inside check, the shape cast, and
// `raycast_shape`), so nothing in gdphysics2d reads `body.shape` directly any
// more. The ray takes the NEAREST collider rather than the first.
//
// One-way collision is per-shape as well: `shape_owners()` yields
// `ShapeOwner2D`, which carries `one_way_collision` / `one_way_direction`
// alongside the geometry, and both the pair loop and `deepest_hit_against` ask
// the OWNER. See `gdphysics2d/tests/per_shape_one_way_collision.rs`.
//
// STILL OPEN: the `shape_owner_*` MUTATORS have no dispatch (only the read-only
// accessors do), and 3D still reports a constant `shape_idx` of 0 in
// main_loop.rs — correct only because `PhysicsBody3D` has no collider list.

/// The collider list itself IS collected, with each shape paired to its own
/// offset — even though the narrow phase does not consult it yet
/// (pat-s3rrv.25).
///
/// This is the half of the multi-shape change with a subtle ordering
/// requirement, done and tested up front so it is not one more thing to get
/// right later. `extra_collision_shapes` must walk children exactly as
/// `find_collision_shape` and `find_collision_shape_transform` do — same
/// `disabled` skip, same usable-shape test, same order — or "everything after
/// the first" means something different to each of the three and one collider's
/// geometry gets paired with another's offset.
///
/// The two extras are given DIFFERENT offsets and sizes precisely so a
/// mispairing shows up: swapping them would put (300, 0) with the 60px shape.
#[test]
fn every_collider_child_is_collected_with_its_own_offset() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let body = add_body(
        &mut tree,
        main,
        "Multi",
        "StaticBody2D",
        Vector2::ZERO,
        Vector2::new(20.0, 20.0),
    );
    add_second_collider(
        &mut tree,
        body,
        Vector2::new(100.0, 0.0),
        Vector2::new(40.0, 40.0),
    );
    add_second_collider(
        &mut tree,
        body,
        Vector2::new(300.0, 0.0),
        Vector2::new(60.0, 60.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    let body_id = ml
        .physics_server()
        .body_for_node(body)
        .expect("the body registered");
    let world = ml.physics_server().world();
    let b = world.get_body(body_id).expect("world body");

    assert_eq!(
        b.extra_shapes.len(),
        2,
        "both colliders after the first must be collected — got {:?}",
        b.extra_shapes,
    );

    // Pairing, not just count: each extra keeps ITS OWN offset. A swap would
    // still give len 2 and would still look fine to a count-only assertion.
    let offsets: Vec<f32> = b
        .extra_shapes
        .iter()
        .map(|owner| owner.transform.origin.x)
        .collect();
    assert_eq!(
        offsets,
        vec![100.0, 300.0],
        "extras must keep scene order and their own offsets",
    );
    let widths: Vec<f32> = b
        .extra_shapes
        .iter()
        .map(|owner| match owner.shape {
            gdphysics2d::shape::Shape2D::Rectangle { half_extents } => half_extents.x,
            other => panic!("expected rectangles, got {other:?}"),
        })
        .collect();
    assert_eq!(
        widths,
        vec![20.0, 30.0],
        "each extra must carry its OWN geometry — if these are swapped relative \
         to the offsets above, the child walks have drifted out of lockstep",
    );

    // `shape_owners()` is the seam the whole narrow phase runs on: primary
    // first, then the extras, each already in world space.
    let owners = b.shape_owners();
    assert_eq!(owners.len(), 3, "primary shape plus the two extras");
    assert_eq!(
        owners[0].transform.origin,
        Vector2::ZERO,
        "the primary shape must come first and sit at the body origin",
    );
    assert_eq!(
        owners[2].transform.origin.x, 300.0,
        "the last owner must be the furthest collider, in world space",
    );
}

/// **Body-vs-body now collides on EVERY collider a body owns** (pat-s3rrv.25).
///
/// `world.rs`'s pair loop iterates `shape_owners()` rather than reading the one
/// primary shape, so an L-shaped body — two `CollisionShape2D` children — is
/// solid across both.
///
/// The fixture makes the answer unambiguous: the platform's FIRST collider sits
/// at its origin, far from the faller, and its SECOND sits directly under the
/// faller at x=300. Only the second can catch it.
///
///   platform body y = 300, second collider 200x20 at local (300, 0)
///   -> its top face is world y = 300 - 10 = 290
///   -> a 20x20 faller rests at y = 290 - 10 = 280
///
/// With only the first collider consulted there is nothing at x=300 at all and
/// the body falls forever, so the two behaviours are hundreds of pixels apart.
#[test]
fn a_body_collides_on_its_second_collider_too() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let platform = add_body(
        &mut tree,
        main,
        "Platform",
        "StaticBody2D",
        Vector2::new(0.0, 300.0),
        Vector2::new(20.0, 20.0),
    );
    add_second_collider(
        &mut tree,
        platform,
        Vector2::new(300.0, 0.0),
        Vector2::new(200.0, 20.0),
    );

    let faller = add_body(
        &mut tree,
        main,
        "Faller",
        "RigidBody2D",
        Vector2::new(300.0, 0.0),
        Vector2::new(20.0, 20.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    for _ in 0..240 {
        ml.step(DT);
    }

    let resting = position(&ml, faller).y;
    assert!(
        (resting - 280.0).abs() < 5.0,
        "the faller must land on the platform's SECOND collider at y=280 — got \
         y={resting}. A value in the thousands means the pair loop is still \
         reading only the primary shape, which sits at x=0 with nothing under \
         the faller at all",
    );
}

/// An area detects a body through **any** of that body's colliders
/// (pat-s3rrv.25).
///
/// Body-vs-body learned this first; without the same change here an `Area2D`
/// and a `StaticBody2D` would disagree about whether a two-collider body is
/// touching them — the body would be blocked by geometry the trigger could not
/// see. That inconsistency is worse than the original single-shape behaviour,
/// which is why both area-vs-body sites moved in the same step.
///
/// The body's FIRST collider is at its origin, far outside the trigger; only
/// its second reaches in. A detection can therefore only mean the second was
/// consulted.
#[test]
fn an_area_detects_a_body_through_its_second_collider() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let trigger = add_body(
        &mut tree,
        main,
        "Trigger",
        "Area2D",
        Vector2::new(300.0, 0.0),
        Vector2::new(60.0, 60.0),
    );

    // Body at the origin: its own first collider is nowhere near the trigger.
    let walker = add_body(
        &mut tree,
        main,
        "Walker",
        "RigidBody2D",
        Vector2::ZERO,
        Vector2::new(20.0, 20.0),
    );
    add_second_collider(
        &mut tree,
        walker,
        Vector2::new(300.0, 0.0),
        Vector2::new(40.0, 40.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        1,
        "the area must detect the body through its SECOND collider — 0 means \
         area-vs-body is still reading only the body's primary shape, which \
         would leave a body solid against geometry no trigger can see",
    );
}

/// Point queries and raycasts see every collider a body owns, and the ray
/// reports the **nearest** one (pat-s3rrv.25).
///
/// These were the last single-shape readers. A picking click on an L-shaped
/// body's second arm used to miss it entirely, and a `RayCast2D` would aim at
/// whichever collider happened to come first in scene order.
///
/// The fixture separates the two colliders by a gap, so each assertion has only
/// one explanation:
///
/// ```text
/// collider A: 20x20 at the body origin      -> covers x in [-10, 10]
/// collider B: 40x40 at local (300, 0)       -> covers x in [280, 320]
/// gap at x = 150                            -> inside neither
/// ```
#[test]
fn point_queries_and_rays_see_every_collider() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let body = add_body(
        &mut tree,
        main,
        "Multi",
        "StaticBody2D",
        Vector2::ZERO,
        Vector2::new(20.0, 20.0),
    );
    add_second_collider(
        &mut tree,
        body,
        Vector2::new(300.0, 0.0),
        Vector2::new(40.0, 40.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    let body_id = ml
        .physics_server()
        .body_for_node(body)
        .expect("the body registered");
    let world = ml.physics_server().world();

    // -- point query ---------------------------------------------------------
    assert_eq!(
        world.point_query(Vector2::new(300.0, 0.0)),
        vec![body_id],
        "a point inside the body's SECOND collider must find it — an empty \
         result means point_query still reads only the primary shape",
    );
    assert_eq!(
        world.point_query(Vector2::new(0.0, 0.0)),
        vec![body_id],
        "and the first collider must still answer",
    );
    assert!(
        world.point_query(Vector2::new(150.0, 0.0)).is_empty(),
        "the gap between the two colliders must hit nothing — if this finds the \
         body, the query is testing a bounding region rather than the shapes",
    );

    // -- ray, fired from beyond the far collider back toward the origin -------
    //
    // It meets collider B's far face (x=320) long before collider A. Reporting
    // A would mean the ray picked a hit it only reaches by passing THROUGH B.
    let hit = world
        .raycast(Vector2::new(600.0, 0.0), Vector2::new(-1.0, 0.0), 1000.0)
        .expect("the ray must hit the body");
    assert_eq!(hit.body_id, body_id);
    assert!(
        (hit.point.x - 320.0).abs() < 2.0,
        "the ray must report the NEAREST collider's surface at x=320, got {:?}. \
         A value near 10 is collider A — the ray would be aiming at a face it \
         reaches only by passing through collider B",
        hit.point,
    );
}

/// An `Area2D` monitors through **every** collider it owns (pat-s3rrv.25).
///
/// This test began life as the pin for the known gap — it asserted `0` and
/// carried instructions to invert it when the area's shape list landed. It has
/// now been inverted, which is the whole point of having written it that way:
/// the boundary marker turned into the acceptance rather than being deleted and
/// re-derived.
///
/// The body sits under the area's SECOND collider only, so a detection can mean
/// nothing except that the second was consulted.
///
/// This began as the pin for the known gap, asserting `0` with instructions to
/// invert it once the area gained a shape list. It has been inverted — which is
/// why it was written that way rather than as a bead comment.
///
/// The two shapes are placed so the answer is unambiguous: the probe at
/// (200, 0) is reachable ONLY through the second collider, so a detection can
/// only mean the second shape was consulted.
#[test]
fn an_area_monitors_through_every_collider_it_owns() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    // First collider: small, at the area's origin. Second: at (200, 0).
    let trigger = add_body(
        &mut tree,
        main,
        "Trigger",
        "Area2D",
        Vector2::ZERO,
        Vector2::new(20.0, 20.0),
    );
    add_second_collider(
        &mut tree,
        trigger,
        Vector2::new(200.0, 0.0),
        Vector2::new(40.0, 40.0),
    );

    add_body(
        &mut tree,
        main,
        "Ball",
        "RigidBody2D",
        Vector2::new(200.0, 0.0),
        Vector2::new(8.0, 8.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    assert_eq!(
        overlap_count(&ml, trigger),
        1,
        "the area must detect the body through its SECOND collider — 0 means \
         Area2D is back to monitoring through its primary shape alone",
    );

    // The control: the FIRST collider still works too, so the assertion above is
    // "the second collider was consulted" rather than "the shape list replaced
    // the primary shape with the second one".
    //
    // ⚠️ This expects 2, not 1. When this test was the pin it expected 1,
    // because the area saw only the body at the origin. Now it sees both — the
    // one under its second collider AND the one under its first. Inverting the
    // assertion above without re-deriving this number is exactly the mistake
    // that made this test fail: a control is not a constant, it is a
    // consequence of the behaviour being asserted.
    add_body(
        ml.tree_mut(),
        main,
        "Near",
        "RigidBody2D",
        Vector2::ZERO,
        Vector2::new(8.0, 8.0),
    );
    ml.step(DT);
    assert_eq!(
        overlap_count(&ml, trigger),
        2,
        "control: the area must now detect BOTH bodies — the one under its \
         second collider and the one under its first. A 1 here means the shape \
         list is being used INSTEAD of the primary shape rather than in \
         addition to it",
    );
}

/// **Each collider child carries its OWN `one_way_collision`** (pat-s3rrv.25).
///
/// Godot puts the flag on the `CollisionShape2D` node, so one body may own a
/// solid collider and a one-way collider at the same time — the shape every
/// platformer character has. Patina lifted the flag to the body, which made
/// those two states mutually exclusive.
///
/// This is the SCENE-WIRING half of the fix: `extra_collision_shapes` must read
/// each child's own flag the way it already reads each child's own transform.
/// The behavioural half — that the solver and the character sweeps act on the
/// per-shape value — lives in
/// `gdphysics2d/tests/per_shape_one_way_collision.rs`.
///
/// The fixture is deliberately asymmetric: the FIRST collider is solid and the
/// SECOND is one-way. A body-wide flag cannot represent that, and neither can a
/// wiring that reads the flag off the wrong child — both halves would come back
/// the same.
#[test]
fn each_collider_child_carries_its_own_one_way_flag() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let body = add_body(
        &mut tree,
        main,
        "SplitPlatform",
        "StaticBody2D",
        Vector2::ZERO,
        Vector2::new(50.0, 10.0),
    );
    add_one_way_collider(
        &mut tree,
        body,
        Vector2::new(100.0, 0.0),
        Vector2::new(50.0, 10.0),
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(DT);

    let body_id = ml
        .physics_server()
        .body_for_node(body)
        .expect("the body registered");
    let b = ml
        .physics_server()
        .world()
        .get_body(body_id)
        .expect("world body");

    assert!(
        !b.one_way_collision,
        "the PRIMARY shape's flag is the body-level one, and its collider child \
         did not set `one_way_collision` — a true here means the flag is being \
         lifted off the wrong child",
    );
    assert_eq!(
        b.extra_shapes.len(),
        1,
        "the one-way collider must be collected as an extra shape owner",
    );
    assert!(
        b.extra_shapes[0].one_way_collision,
        "the SECOND collider set `one_way_collision` on itself, so its shape \
         owner must carry it — a false here means `extra_collision_shapes` is \
         still dropping the per-child flag",
    );

    // The accessor is what every solver and sweep actually reads, so assert
    // through it rather than only through the storage field.
    let owners = b.shape_owners();
    assert_eq!(owners.len(), 2, "primary shape plus the one-way extra");
    assert_eq!(
        (owners[0].one_way_collision, owners[1].one_way_collision),
        (false, true),
        "`shape_owners()` must hand the narrow phase one solid owner and one \
         one-way owner from the SAME body",
    );
    assert_eq!(
        owners[1].one_way_direction,
        Vector2::new(0.0, -1.0),
        "the blocking face defaults to the shape's local up, matching the \
         direction `PhysicsBody2D::new` seeds for the primary shape",
    );
}
