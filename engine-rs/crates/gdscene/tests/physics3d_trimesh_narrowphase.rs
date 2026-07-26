//! pat-0q2kx.119 acceptance: the 3D concave narrow phase covers *every* convex
//! shape, not just spheres and capsules.
//!
//! Before this bead `test_collision_rot` dispatched `ConcavePolygonShape` only
//! against `Sphere` and `CapsuleShape`; box, cylinder and convex-hull bodies
//! fell through the `CollisionResult3D::NONE` arm, so a crate dropped onto
//! authored level geometry tunnelled straight through it. `HeightMapShape3D`
//! and `SeparationRayShape3D` did not exist as shapes at all, and
//! `ConcavePolygonShape3D.backface_collision` was ignored.
//!
//! These tests drive real `MainLoop::step` frames on scene-authored nodes, so
//! they exercise the `.tscn` property path (`shape = "..."` plus the shape's
//! own properties) through `PhysicsServer3D` and into the narrow phase.

use gdcore::math::Vector3;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Two triangles spanning x,z ∈ [-6, 6] at y = 0, wound so the front face
/// (Godot's `Plane(a, b, c)` normal) points up.
fn ground_plane_faces() -> Vec<Vector3> {
    let v0 = Vector3::new(-6.0, 0.0, -6.0);
    let v1 = Vector3::new(6.0, 0.0, -6.0);
    let v2 = Vector3::new(6.0, 0.0, 6.0);
    let v3 = Vector3::new(-6.0, 0.0, 6.0);
    vec![v0, v1, v2, v0, v2, v3]
}

struct Scene {
    ml: MainLoop,
    body: NodeId,
}

impl Scene {
    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.ml.step(1.0 / 60.0);
        }
    }

    fn body_y(&self) -> f32 {
        node3d::get_position(self.ml.tree(), self.body).y
    }
}

/// `Root -> Ground (StaticBody3D + CollisionShape3D) + Falling (RigidBody3D +
/// CollisionShape3D)`, with the ground shape's properties supplied by the
/// caller. `start_y` is the falling body's initial height.
fn drop_scene(
    ground_class: &str,
    ground_props: &[(&str, Variant)],
    body_class: &str,
    body_props: &[(&str, Variant)],
    start_y: f32,
    initial_velocity: Vector3,
) -> Scene {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let ground = tree
        .add_child(root, Node::new("Ground", "StaticBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, ground, Vector3::ZERO);
    let mut ground_shape = Node::new("GroundShape", "CollisionShape3D");
    ground_shape.set_property("shape", Variant::String(ground_class.to_string()));
    for (key, value) in ground_props {
        ground_shape.set_property(key, value.clone());
    }
    tree.add_child(ground, ground_shape).unwrap();

    let body = tree
        .add_child(root, Node::new("Falling", "RigidBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, body, Vector3::new(0.0, start_y, 0.0));
    if let Some(node) = tree.get_node_mut(body) {
        node.set_property("linear_velocity", Variant::Vector3(initial_velocity));
        // Sleeping mid-fall would mask a tunnelling bug as a "rest".
        node.set_property("can_sleep", Variant::Bool(false));
    }
    let mut body_shape = Node::new("BodyShape", "CollisionShape3D");
    body_shape.set_property("shape", Variant::String(body_class.to_string()));
    for (key, value) in body_props {
        body_shape.set_property(key, value.clone());
    }
    tree.add_child(body, body_shape).unwrap();

    Scene {
        ml: MainLoop::new(tree),
        body,
    }
}

fn box_props(half: f32) -> Vec<(&'static str, Variant)> {
    vec![(
        "size",
        Variant::Vector3(Vector3::new(half * 2.0, half * 2.0, half * 2.0)),
    )]
}

fn trimesh_props(faces: Vec<Vector3>, backface: bool) -> Vec<(&'static str, Variant)> {
    vec![
        ("data", Variant::PackedVector3Array(faces)),
        ("backface_collision", Variant::Bool(backface)),
    ]
}

/// **The acceptance test.** A box crate dropped onto a two-triangle
/// `ConcavePolygonShape3D` ground plane comes to rest on it instead of
/// tunnelling through, and so do a cylinder and a convex hull.
#[test]
fn physics3d_trimesh_narrowphase() {
    // Half-extent 0.5 ⇒ the crate rests with its centre at y ≈ 0.5.
    let mut crate_scene = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), false),
        "BoxShape3D",
        &box_props(0.5),
        3.0,
        Vector3::ZERO,
    );
    crate_scene.run(180);
    let y = crate_scene.body_y();
    assert!(
        y > 0.2,
        "a BoxShape3D crate must come to rest on the trimesh ground plane \
         within 180 ticks, not tunnel through it: got y={y}"
    );
    assert!(
        y < 1.2,
        "the crate must settle *on* the plane, not hover above it: got y={y}"
    );

    // Same plane, cylinder body (height 1.0 ⇒ rests near y = 0.5).
    let mut cylinder = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), false),
        "CylinderShape3D",
        &[
            ("radius", Variant::Float(0.5)),
            ("height", Variant::Float(1.0)),
        ],
        3.0,
        Vector3::ZERO,
    );
    cylinder.run(180);
    let y = cylinder.body_y();
    assert!(
        y > 0.2 && y < 1.2,
        "a CylinderShape3D body must rest on the trimesh ground plane: got y={y}"
    );

    // Same plane, convex hull body: a unit cube expressed as 8 hull points.
    let hull: Vec<Vector3> = [-0.5f32, 0.5]
        .iter()
        .flat_map(|x| {
            [-0.5f32, 0.5]
                .iter()
                .flat_map(move |y| [-0.5f32, 0.5].iter().map(move |z| Vector3::new(*x, *y, *z)))
        })
        .collect();
    let mut convex = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), false),
        "ConvexPolygonShape3D",
        &[("points", Variant::PackedVector3Array(hull))],
        3.0,
        Vector3::ZERO,
    );
    convex.run(180);
    let y = convex.body_y();
    assert!(
        y > 0.2 && y < 1.2,
        "a ConvexPolygonShape3D body must rest on the trimesh ground plane: got y={y}"
    );
}

/// Control: with the narrow phase working, a body dropped *next to* the soup
/// still falls. Without this a pass above could come from bodies simply never
/// moving.
#[test]
fn body_beside_the_trimesh_still_falls() {
    let mut scene = drop_scene(
        "ConcavePolygonShape3D",
        // A soup that only covers x,z ∈ [10, 12] — nowhere near the body.
        &trimesh_props(
            vec![
                Vector3::new(10.0, 0.0, 10.0),
                Vector3::new(12.0, 0.0, 10.0),
                Vector3::new(12.0, 0.0, 12.0),
            ],
            false,
        ),
        "BoxShape3D",
        &box_props(0.5),
        3.0,
        Vector3::ZERO,
    );
    scene.run(180);
    let y = scene.body_y();
    assert!(
        y < -1.0,
        "a box with no triangle beneath it must keep falling — otherwise the \
         resting assertions above prove nothing: got y={y}"
    );
}

/// A body dropped onto a `HeightMapShape3D` rests at the sampled height, not at
/// y = 0 and not through the terrain.
#[test]
fn box_rests_on_heightmap_terrain() {
    // 4x4 grid, uniformly 2.0 units high. Cells are one unit across and the grid
    // is centred on the origin, so it spans x,z ∈ [-1.5, 1.5].
    let heights: Vec<f32> = vec![2.0; 16];
    let mut scene = drop_scene(
        "HeightMapShape3D",
        &[
            ("map_width", Variant::Int(4)),
            ("map_depth", Variant::Int(4)),
            ("map_data", Variant::PackedFloat32Array(heights)),
        ],
        "BoxShape3D",
        &box_props(0.5),
        5.0,
        Vector3::ZERO,
    );
    scene.run(240);
    let y = scene.body_y();
    assert!(
        y > 2.2 && y < 3.0,
        "a box must settle on the 2.0-high heightmap surface (centre ≈ 2.5), \
         got y={y}"
    );
}

/// `backface_collision` is honoured: an upward-moving body passes through the
/// back of a soup when the flag is off, and is stopped by it when on.
#[test]
fn backface_collision_flag_gates_rear_contacts() {
    // The ground plane's front face (Godot's `Plane(a, b, c)` normal) points up,
    // so a body underneath it sees only its *back*. That is exactly the contact
    // `backface_collision` gates. Start under the plane and shoot upward.
    let launch = Vector3::new(0.0, 12.0, 0.0);

    let mut passes = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), false),
        "BoxShape3D",
        &box_props(0.5),
        -3.0,
        launch,
    );
    passes.run(40);
    let free = passes.body_y();
    assert!(
        free > 0.6,
        "with backface_collision=false an upward-moving box must pass through \
         the back of the soup: got y={free}"
    );

    let mut blocked = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), true),
        "BoxShape3D",
        &box_props(0.5),
        -3.0,
        launch,
    );
    blocked.run(40);
    let stopped = blocked.body_y();
    assert!(
        stopped < free,
        "with backface_collision=true the same box must be stopped by the soup \
         it otherwise passes: got y={stopped} vs free y={free}"
    );
    assert!(
        stopped < 0.0,
        "the blocked box must stay on its own side of the plane: got y={stopped}"
    );
}

// ---------------------------------------------------------------------------
// The `.tscn` shape-parse path for the two new shapes (pat-0q2kx.119)
//
// `shape_from_node_3d` maps `SeparationRayShape3D` and reads
// `backface_collision`/`slide_on_slope` under BOTH spellings: the plain
// property name, and the `__shape__<key>` form that `PackedScene` produces when
// the shape is an inline sub-resource (which is what a Godot-authored `.tscn`
// actually writes). The solver for separation rays is unit-tested in
// `gdphysics3d::collision`, and the sub-resource spelling is what a real scene
// takes — but nothing proved either reaches the world from a scene, so a typo
// in a key would have made every authored shape silently inert.
// ---------------------------------------------------------------------------

/// Reads the shape the physics server parsed for the falling body. One tick is
/// stepped first because `MainLoop::new` does not register bodies — that
/// happens in the physics phase of the first `step`.
fn parsed_body_shape(scene: &mut Scene) -> gdphysics3d::shape::Shape3D {
    scene.run(1);
    let server = scene.ml.physics_server_3d();
    let body_id = server
        .body_for_node(scene.body)
        .expect("the falling body was registered");
    server
        .world()
        .get_body(body_id)
        .expect("body is in the world")
        .shape
        .clone()
}

/// A `.tscn`-authored `SeparationRayShape3D` reaches the world with its authored
/// `length` and `slide_on_slope`.
#[test]
fn separation_ray_shape3d_parses_from_the_scene() {
    use gdphysics3d::shape::Shape3D;

    let mut authored = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), false),
        "SeparationRayShape3D",
        &[
            ("length", Variant::Float(2.5)),
            ("slide_on_slope", Variant::Bool(true)),
        ],
        3.0,
        Vector3::ZERO,
    );
    assert_eq!(
        parsed_body_shape(&mut authored),
        Shape3D::SeparationRayShape {
            length: 2.5,
            slide_on_slope: true,
        },
        "SeparationRayShape3D must reach the world with its authored properties"
    );

    // Godot's documented defaults when the scene omits both properties.
    let mut defaults = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), false),
        "SeparationRayShape3D",
        &[],
        3.0,
        Vector3::ZERO,
    );
    assert_eq!(
        parsed_body_shape(&mut defaults),
        Shape3D::SeparationRayShape {
            length: 1.0,
            slide_on_slope: false,
        },
        "an unconfigured SeparationRayShape3D must take Godot's defaults"
    );
}

/// `PackedScene` flattens an inline shape sub-resource's keys to
/// `__shape__<key>`, so the parse has to answer to that spelling too — it is the
/// one a Godot-authored `.tscn` produces.
#[test]
fn sub_resource_shape_keys_are_honored() {
    use gdphysics3d::shape::Shape3D;

    let mut ray = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), false),
        "SeparationRayShape3D",
        &[
            ("length", Variant::Float(3.0)),
            ("__shape__slide_on_slope", Variant::Bool(true)),
        ],
        3.0,
        Vector3::ZERO,
    );
    assert_eq!(
        parsed_body_shape(&mut ray),
        Shape3D::SeparationRayShape {
            length: 3.0,
            slide_on_slope: true,
        },
        "the __shape__ sub-resource spelling of slide_on_slope must be read"
    );

    // Same for the trimesh flag, on the ground body this time.
    let mut soup = drop_scene(
        "ConcavePolygonShape3D",
        &[
            ("data", Variant::PackedVector3Array(ground_plane_faces())),
            ("__shape__backface_collision", Variant::Bool(true)),
        ],
        "BoxShape3D",
        &box_props(0.5),
        -3.0,
        Vector3::new(0.0, 12.0, 0.0),
    );
    soup.run(40);
    let stopped = soup.body_y();

    // The same launch through a soup whose backface flag is off passes straight
    // through, so a difference here can only come from the flag being read.
    let mut through = drop_scene(
        "ConcavePolygonShape3D",
        &trimesh_props(ground_plane_faces(), false),
        "BoxShape3D",
        &box_props(0.5),
        -3.0,
        Vector3::new(0.0, 12.0, 0.0),
    );
    through.run(40);
    let free = through.body_y();
    assert!(
        stopped < free,
        "__shape__backface_collision=true must stop the box the unset soup lets \
         through: got y={stopped} vs free y={free}"
    );
}
