//! pat-0q2kx.112 acceptance: `Area2D` honours rotation, answers script overlap
//! queries, damps angular motion, and moves its point-gravity centre.
//!
//! Four independent gaps, each of which made an authored scene behave
//! differently from Godot:
//!
//! * overlap tests built a translation-only transform, so a rotated area was
//!   tested as though it were axis-aligned;
//! * the 2D server never published `_overlapping_bodies`, so
//!   `get_overlapping_bodies()` on an `Area2D` always returned empty — the
//!   interpreter arm and the 3D publisher both already existed;
//! * `angular_damp_space_override` did not exist, so an area could slow a
//!   body's travel but never its spin;
//! * `gravity_point_center` was resolved once at registration, so moving an
//!   area left its attraction behind.
//!
//! These drive real `MainLoop` physics ticks and assert on tree state.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node2d;
use gdscene::scene_tree::SceneTree;
// `get_script(..).get_property(..)` is a ScriptInstance trait method; without
// the trait in scope it does not resolve.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Attaches a `CollisionShape2D` child carrying its geometry inline — the
/// hand-built form `shape_from_node` accepts alongside `.tscn` sub-resources.
fn add_shape(tree: &mut SceneTree, parent: NodeId, class: &str, dim: (&str, Variant)) {
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("shape", Variant::String(class.to_string()));
    shape.set_property(dim.0, dim.1);
    tree.add_child(parent, shape).unwrap();
}

fn area(tree: &mut SceneTree, root: NodeId, name: &str, pos: Vector2, size: Vector2) -> NodeId {
    let id = tree.add_child(root, Node::new(name, "Area2D")).unwrap();
    node2d::set_position(tree, id, pos);
    add_shape(
        tree,
        id,
        "RectangleShape2D",
        ("size", Variant::Vector2(size)),
    );
    id
}

fn body(tree: &mut SceneTree, root: NodeId, name: &str, pos: Vector2, radius: f32) -> NodeId {
    let id = tree
        .add_child(root, Node::new(name, "RigidBody2D"))
        .unwrap();
    node2d::set_position(tree, id, pos);
    add_shape(
        tree,
        id,
        "CircleShape2D",
        ("radius", Variant::Float(radius as f64)),
    );
    id
}

fn set(tree: &mut SceneTree, id: NodeId, key: &str, value: Variant) {
    tree.get_node_mut(id).unwrap().set_property(key, value);
}

fn new_scene() -> (SceneTree, NodeId) {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    (tree, root)
}

/// How many bodies the area currently reports overlapping, read from the
/// property the script API answers from.
fn overlap_count(ml: &MainLoop, area_id: NodeId) -> usize {
    match ml
        .tree()
        .get_node(area_id)
        .unwrap()
        .get_property("_overlapping_bodies")
    {
        Variant::Array(items) => items.len(),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core test.** Rotation is honoured, script queries answer, angular damp
/// bites, and point gravity follows the area.
#[test]
fn area2d_rotation_queries_and_angular_damp() {
    // -- (a) a rotated area detects by its ROTATED extent --------------------
    //
    // A 100x20 bar rotated 90° becomes tall and narrow. A body directly above
    // the centre is inside the rotated bar and outside the unrotated one, so
    // the two orientations disagree about it — which is the whole point.
    {
        let (mut tree, root) = new_scene();
        let bar = area(
            &mut tree,
            root,
            "Bar",
            Vector2::ZERO,
            Vector2::new(100.0, 20.0),
        );
        set(
            &mut tree,
            bar,
            "rotation",
            Variant::Float(std::f64::consts::FRAC_PI_2),
        );
        // (0, 40): inside the rotated bar's long axis, well outside the flat
        // one's 10px half-height.
        body(&mut tree, root, "Ball", Vector2::new(0.0, 40.0), 4.0);

        let mut ml = MainLoop::new(tree);
        // `register_physics_bodies` is what populates `node_to_area` --
        // `PhysicsServer::register_bodies` (physics_server.rs:906) registers both
        // bodies AND Area2D nodes (:998), and `MainLoop::step` never calls it. Without
        // this the area map is empty, no overlap is ever detected, and every
        // `overlap_count` below reads 0 (pat-0q2kx.112 / same trap as pat-0q2kx.16).
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);
        assert_eq!(
            overlap_count(&ml, bar),
            1,
            "a 90°-rotated 100x20 area must contain a body 40px along its long \
             axis — with rotation dropped this point is outside the shape"
        );
    }
    {
        // The control: the SAME geometry unrotated must NOT contain it. Without
        // this, case (a) could pass on an area that overlaps everything.
        let (mut tree, root) = new_scene();
        let bar = area(
            &mut tree,
            root,
            "Bar",
            Vector2::ZERO,
            Vector2::new(100.0, 20.0),
        );
        body(&mut tree, root, "Ball", Vector2::new(0.0, 40.0), 4.0);

        let mut ml = MainLoop::new(tree);
        // Registers bodies AND areas -- see the note at the first call site.
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);
        assert_eq!(
            overlap_count(&ml, bar),
            0,
            "unrotated, the same body is outside the bar — so case (a) is \
             genuinely about rotation"
        );
    }

    // -- (b) the overlap set is published, and empties on exit ---------------
    {
        let (mut tree, root) = new_scene();
        let zone = area(
            &mut tree,
            root,
            "Zone",
            Vector2::ZERO,
            Vector2::new(40.0, 40.0),
        );
        let ball = body(&mut tree, root, "Ball", Vector2::ZERO, 4.0);

        let mut ml = MainLoop::new(tree);
        // Registers bodies AND areas -- see the note at the first call site.
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);
        assert_eq!(
            overlap_count(&ml, zone),
            1,
            "a body inside the area must appear in _overlapping_bodies — this \
             is what Area2D.get_overlapping_bodies() reads, and the 2D server \
             never filled it in"
        );

        // Move the body far away; the set must empty.
        node2d::set_position(ml.tree_mut(), ball, Vector2::new(500.0, 500.0));
        ml.step(1.0 / 60.0);
        assert_eq!(
            overlap_count(&ml, zone),
            0,
            "leaving the area must clear the published overlap set"
        );
    }

    // -- (c) an angular-damp Replace override spins a body down --------------
    {
        let spin_with = |override_mode: i64| -> f32 {
            let (mut tree, root) = new_scene();
            let zone = area(
                &mut tree,
                root,
                "Zone",
                Vector2::ZERO,
                Vector2::new(200.0, 200.0),
            );
            set(
                &mut tree,
                zone,
                "angular_damp_space_override",
                Variant::Int(override_mode),
            );
            set(&mut tree, zone, "angular_damp", Variant::Float(8.0));
            let ball = body(&mut tree, root, "Ball", Vector2::ZERO, 4.0);
            set(&mut tree, ball, "angular_velocity", Variant::Float(10.0));

            let mut ml = MainLoop::new(tree);
            // Registers bodies AND areas -- see the note at the first call site.
            ml.register_physics_bodies();
            for _ in 0..30 {
                ml.step(1.0 / 60.0);
            }
            match ml
                .tree()
                .get_node(ball)
                .unwrap()
                .get_property("angular_velocity")
            {
                Variant::Float(f) => f as f32,
                Variant::Int(i) => i as f32,
                other => panic!("angular_velocity should be a number, got {other:?}"),
            }
        };

        // 3 == SPACE_OVERRIDE_REPLACE.
        let damped = spin_with(3);
        // 0 == SPACE_OVERRIDE_DISABLED: the control, so the difference is
        // attributable to the override rather than to default damping.
        let undamped = spin_with(0);
        assert!(
            damped.abs() < undamped.abs(),
            "an angular_damp Replace override must spin the body down faster \
             than no override; damped {damped}, undamped {undamped}"
        );
    }

    // -- (d) point gravity follows the area ----------------------------------
    {
        let (mut tree, root) = new_scene();
        // 800x800, i.e. half-extent 400 — sized so the ball is still inside
        // after the area is moved to y = -400 below.
        //
        // An area only affects bodies *inside* it: `AreaStore` gates every
        // override on `shape.contains_point` (gdphysics2d/src/area2d.rs:281),
        // exactly as Godot does. At the original 400x400 (half-extent 200) the
        // moved well spanned y in [-600, -200] while the ball sat at y ~ -50,
        // so the REPLACE override stopped applying and the ball simply resumed
        // falling under default gravity — the assertion below read 360, which
        // is 980 * 21/60, not a frozen attractor. At 800x800 the moved well
        // spans y in [-800, 0], the ball stays inside, and the assertion tests
        // the attractor's position rather than the containment check.
        let well = area(
            &mut tree,
            root,
            "Well",
            Vector2::ZERO,
            Vector2::new(800.0, 800.0),
        );
        // 3 == SPACE_OVERRIDE_REPLACE.
        set(&mut tree, well, "gravity_space_override", Variant::Int(3));
        set(&mut tree, well, "gravity_point", Variant::Bool(true));
        set(&mut tree, well, "gravity", Variant::Float(2000.0));
        let ball = body(&mut tree, root, "Ball", Vector2::new(0.0, -50.0), 4.0);

        let mut ml = MainLoop::new(tree);
        // Registers bodies AND areas -- see the note at the first call site.
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);
        // The attractor sits at the area's origin, so a body above it is pulled
        // DOWN (+y).
        assert!(
            velocity_y(&ml, ball) > 0.0,
            "a point-gravity area at the origin must pull a body above it \
             downward; got {}",
            velocity_y(&ml, ball)
        );

        // Move the area far ABOVE the body. A centre frozen at registration
        // would keep pulling down; a live one now pulls up.
        node2d::set_position(ml.tree_mut(), well, Vector2::new(0.0, -400.0));
        for _ in 0..20 {
            ml.step(1.0 / 60.0);
        }
        assert!(
            velocity_y(&ml, ball) < 0.0,
            "moving the area must move its attraction centre — the body should \
             now be pulled upward, got {}",
            velocity_y(&ml, ball)
        );
    }
}

/// A rigid body publishes `linear_velocity`, not `velocity` — the latter is the
/// CharacterBody2D script property and is never written for a rigid body, so
/// reading it here would silently return zero and prove nothing.
fn velocity_y(ml: &MainLoop, id: NodeId) -> f32 {
    match ml
        .tree()
        .get_node(id)
        .unwrap()
        .get_property("linear_velocity")
    {
        Variant::Vector2(v) => v.y,
        _ => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

/// `monitorable = false` hides an area from OTHER areas.
///
/// Asserted through `area_entered`, which is the observable Godot contract —
/// counting body overlaps here would say nothing about area-vs-area detection.
#[test]
fn non_monitorable_area_is_invisible_to_other_areas() {
    let seen = |monitorable: bool| -> i64 {
        let (mut tree, root) = new_scene();
        let watcher = area(
            &mut tree,
            root,
            "Watcher",
            Vector2::ZERO,
            Vector2::new(80.0, 80.0),
        );
        let hidden = area(
            &mut tree,
            root,
            "Hidden",
            Vector2::ZERO,
            Vector2::new(40.0, 40.0),
        );
        set(&mut tree, hidden, "monitorable", Variant::Bool(monitorable));

        let listener = tree.add_child(root, Node::new("Listener", "Node")).unwrap();
        let src = "\
extends Node
var entered = 0
func _on_area_entered(other):
    self.entered = self.entered + 1
";
        tree.attach_script(
            listener,
            Box::new(
                gdscene::GDScriptNodeInstance::from_source(src, listener).expect("script parses"),
            ),
        );
        tree.connect_signal(
            watcher,
            "area_entered",
            gdobject::signal::Connection::new(listener.object_id(), "_on_area_entered"),
        );

        let mut ml = MainLoop::new(tree);
        // Registers bodies AND areas -- see the note at the first call site.
        ml.register_physics_bodies();
        for _ in 0..3 {
            ml.step(1.0 / 60.0);
        }
        match ml
            .tree()
            .get_script(listener)
            .unwrap()
            .get_property("entered")
        {
            Some(Variant::Int(i)) => i,
            other => panic!("entered should be an int, got {other:?}"),
        }
    };

    assert!(
        seen(true) >= 1,
        "a monitorable area overlapping the watcher must fire area_entered — \
         otherwise the negative case below proves nothing"
    );
    assert_eq!(
        seen(false),
        0,
        "monitorable = false must hide the area from other areas' detection"
    );
}
