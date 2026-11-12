//! pat-0q2kx.156 — the skeleton modifier family reaches the modifier pass.
//!
//! `class_db.rs` already carried a comment explaining that FABRIK3D's siblings
//! differ only by algorithm, but only FABRIK3D was registered, and
//! `skeleton3d::process_modifiers` matched only `IKModifier3D` and `FABRIK3D`.
//! A `CCDIK3D` node in a scene was therefore inert twice over: ClassDB did not
//! know the class, and the modifier pass skipped it via `_ => continue`.
//! `LookAtModifier3D` did not exist in ClassDB at all.
//!
//! Covered here: `CCDIK3D` / `JacobianIK3D` registration + routing, and
//! `LookAtModifier3D`'s schema *and* aiming behaviour.
//!
//! These drive the real `process_modifiers` pipeline, so a registration without
//! routing (or routing without registration) fails here — which is the failure
//! mode this bead is about.
//!
//! The `SpringBone*` family is **registered and schema-tested, but not
//! simulated** — `process_modifiers` has no branch for it, and the test below
//! pins that a `SpringBoneSimulator3D` leaves the pose untouched rather than
//! half-moving it. Replace that test with a real sag assertion when the verlet
//! stage lands.
//!
//! Still absent entirely (registered nowhere, asserted nowhere): `TwoBoneIK3D`,
//! `SkeletonIK3D`, `SplineIK3D`, `RetargetModifier3D`, `BoneMap`,
//! `SkeletonProfile*` and `ModifierBoneTarget3D`.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::skeleton3d::{
    add_bone, get_bone_global_pose, process_modifiers, set_bone_parent, set_bone_rest,
};
use gdvariant::Variant;

/// A 3-bone chain along +Y: bone0 at (0,1,0), bone1 at (0,2,0), tip at (0,3,0).
/// Reach from the pivot at (0,1,0) is 2 units.
fn make_chain(tree: &mut SceneTree) -> (NodeId, usize, usize) {
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skeleton", "Skeleton3D"))
        .expect("skeleton added");

    let b0 = add_bone(tree, skel, "root");
    let b1 = add_bone(tree, skel, "mid");
    let b2 = add_bone(tree, skel, "tip");
    set_bone_parent(tree, skel, b1, b0 as i64);
    set_bone_parent(tree, skel, b2, b1 as i64);

    let up = Transform3D {
        origin: Vector3::new(0.0, 1.0, 0.0),
        ..Transform3D::IDENTITY
    };
    set_bone_rest(tree, skel, b0, up);
    set_bone_rest(tree, skel, b1, up);
    set_bone_rest(tree, skel, b2, up);

    (skel, b0, b2)
}

/// Attaches a modifier of `class` aiming `tip_bone` at `target`. Every knob is
/// inherited from `IterateIK3D`, so the property names are identical across the
/// three siblings — that shared surface is the point of the class family.
fn add_modifier(
    tree: &mut SceneTree,
    skel: NodeId,
    class: &str,
    root_bone: usize,
    tip_bone: usize,
    target: Vector3,
) -> NodeId {
    let mut m = Node::new("IK", class);
    m.set_property("target_position", Variant::Vector3(target));
    m.set_property("root_bone", Variant::Int(root_bone as i64));
    m.set_property("tip_bone", Variant::Int(tip_bone as i64));
    m.set_property("iterations", Variant::Int(16));
    m.set_property("influence", Variant::Float(1.0));
    tree.add_child(skel, m).expect("modifier added")
}

fn tip_pos(tree: &SceneTree, skel: NodeId, tip: usize) -> Vector3 {
    get_bone_global_pose(tree, skel, tip).origin
}

/// Both siblings must be registered under `IterateIK3D` and reach `Node` through
/// it — an unregistered intermediate would sever the chain and `class_exists`
/// alone would not notice.
#[test]
fn ik_siblings_are_registered_under_iterate_ik3d() {
    gdobject::class_db::register_3d_classes();

    for class in ["CCDIK3D", "JacobianIK3D"] {
        assert!(
            gdobject::class_exists(class),
            "ClassDB must know {class} — Godot lists it as instantiable"
        );
        assert!(
            gdobject::is_parent_class(class, "IterateIK3D"),
            "{class} is an IterateIK3D sibling of FABRIK3D"
        );
        assert!(
            gdobject::is_parent_class(class, "SkeletonModifier3D"),
            "{class} must reach SkeletonModifier3D through the chain"
        );
        // Algorithm-only siblings: they declare nothing of their own, so every
        // knob they expose is inherited. A property list of its own would be an
        // invented schema.
        let info = gdobject::class_db::get_class_info(class)
            .unwrap_or_else(|| panic!("{class} registered"));
        assert!(
            info.properties.is_empty(),
            "{class} declares no properties of its own, got {:?}",
            info.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
        // …but the inherited knobs must be visible through the chain, which is
        // what makes the node configurable at all.
        for knob in ["target_position", "root_bone", "tip_bone", "iterations"] {
            assert!(
                gdobject::class_db::class_has_property(class, knob),
                "{class} must inherit '{knob}' from IterateIK3D"
            );
        }
    }
}

/// **The routing test.** A `CCDIK3D` node must actually solve. Before this bead
/// `process_modifiers` hit `_ => continue` for it, so the chain never moved.
#[test]
fn ccdik3d_node_solves_the_chain() {
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);

    let start = tip_pos(&tree, skel, tip);
    assert!(
        (start - Vector3::new(0.0, 3.0, 0.0)).length() < 1e-5,
        "the chain starts straight up at (0,3,0), got {start:?}"
    );

    // Reachable: 1.5 from the pivot at (0,1,0), inside the reach of 2.
    let target = Vector3::new(1.5, 1.0, 0.0);
    add_modifier(&mut tree, skel, "CCDIK3D", b0, tip, target);

    for _ in 0..8 {
        process_modifiers(&mut tree, skel);
    }

    let reached = tip_pos(&tree, skel, tip);
    assert!(
        (reached - target).length() < 0.05,
        "a CCDIK3D node must drive the tip to {target:?}, got {reached:?} \
         (distance {}) — an unrouted class leaves the chain straight up",
        (reached - target).length()
    );
}

/// `JacobianIK3D` is routed too, so the node is not inert.
///
/// ⚠️ It currently runs **CCD**, not Godot's Jacobian transpose/DLS: Patina has
/// no Jacobian solver. The class therefore reaches its target — which is what
/// this asserts — but its convergence path is not Godot's. Deliberate
/// placeholder, recorded at the routing site in `skeleton3d.rs`.
#[test]
fn jacobianik3d_node_is_routed_not_inert() {
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);

    let target = Vector3::new(0.0, 2.0, 1.2);
    add_modifier(&mut tree, skel, "JacobianIK3D", b0, tip, target);

    for _ in 0..8 {
        process_modifiers(&mut tree, skel);
    }

    let reached = tip_pos(&tree, skel, tip);
    assert!(
        (reached - target).length() < 0.05,
        "a JacobianIK3D node must not be skipped by the modifier pass; \
         tip {reached:?} vs target {target:?}"
    );
}

/// `LookAtModifier3D`'s schema, with the defaults that the doc XML does **not**
/// carry (pat-0q2kx.156).
///
/// Every limit/damp member is absent from `doc/classes/LookAtModifier3D.xml`, so
/// the values come from `scene/3d/look_at_modifier_3d.h`. The distinction is
/// load-bearing: a guessed `0.0` for `primary_limit_angle` would mean "this bone
/// may not rotate at all", where Godot's real default is `TAU` — unlimited.
///
/// This asserts the **schema**; the aiming behaviour is covered by the tests
/// below it.
///
/// ⚠️ Registered but *not* modelled by the implementation:
/// `duration`/`transition_type`/`ease_type` (time-based interpolation),
/// `use_secondary_rotation`, the positive/negative limit split, the damp
/// thresholds, and `origin_from`/`origin_offset`/`origin_safe_margin`. Those
/// properties round-trip through the inspector but do not change the result yet.
#[test]
fn look_at_modifier_registers_godot_schema() {
    gdobject::class_db::register_3d_classes();

    assert!(gdobject::class_exists("LookAtModifier3D"));
    assert!(gdobject::is_parent_class(
        "LookAtModifier3D",
        "SkeletonModifier3D"
    ));

    let info = gdobject::class_db::get_class_info("LookAtModifier3D").expect("registered");
    assert_eq!(
        info.properties.len(),
        30,
        "Godot's LookAtModifier3D declares exactly 30 members; got {:?}",
        info.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
    );

    let default_of = |name: &str| {
        info.properties
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("LookAtModifier3D has no '{name}'"))
            .default_value
            .clone()
    };

    // The header defaults the doc XML omits — the whole point of reading source.
    match default_of("primary_limit_angle") {
        Variant::Float(f) => assert!(
            (f - std::f32::consts::TAU as f64).abs() < 1e-6,
            "primary_limit_angle defaults to TAU (unlimited), not {f}"
        ),
        v => panic!("primary_limit_angle should be a Float, got {v:?}"),
    }
    match default_of("primary_positive_limit_angle") {
        Variant::Float(f) => assert!(
            (f - std::f32::consts::PI as f64).abs() < 1e-6,
            "primary_positive_limit_angle defaults to PI, not {f}"
        ),
        v => panic!("expected Float, got {v:?}"),
    }
    assert_eq!(
        default_of("symmetry_limitation"),
        Variant::Bool(true),
        "symmetry_limitation is true in the header even though the doc omits it"
    );
    // BoneAxis::PLUS_Z = 4 — Godot aims +Z, and enum-zero would be +X.
    assert_eq!(default_of("forward_axis"), Variant::Int(4));
    // Vector3::AXIS_Y = 1.
    assert_eq!(default_of("primary_rotation_axis"), Variant::Int(1));
    assert_eq!(default_of("bone"), Variant::Int(-1));
    assert_eq!(default_of("origin_bone"), Variant::Int(-1));
    assert_eq!(default_of("use_angle_limitation"), Variant::Bool(false));
    assert_eq!(default_of("use_secondary_rotation"), Variant::Bool(true));

    assert!(
        gdobject::class_db::class_has_enum("LookAtModifier3D", "OriginFrom"),
        "OriginFrom must be a real enum, not bare ints"
    );
}

/// A single bone at the origin whose rest pose is identity, so its `+Z` forward
/// axis starts pointing at `(0,0,1)`.
fn single_bone(tree: &mut SceneTree) -> (NodeId, usize) {
    let root = tree.root_id();
    let skel = tree
        .add_child(root, Node::new("Skeleton", "Skeleton3D"))
        .expect("skeleton added");
    let b = add_bone(tree, skel, "head");
    set_bone_rest(tree, skel, b, Transform3D::IDENTITY);
    (skel, b)
}

/// The bone's forward direction in skeleton space — `forward_axis` defaults to
/// `BONE_AXIS_PLUS_Z`, so this is the axis the modifier aims.
fn bone_forward(tree: &SceneTree, skel: NodeId, bone: usize) -> Vector3 {
    get_bone_global_pose(tree, skel, bone)
        .basis
        .xform(Vector3::new(0.0, 0.0, 1.0))
        .normalized()
}

/// **LookAtModifier3D aims the bone.** Before this stage the class was
/// registered but `process_modifiers` had no branch for it, so the node was
/// inert — the schema existed and nothing used it.
#[test]
fn look_at_modifier_aims_the_bone_at_its_target() {
    let mut tree = SceneTree::new();
    let (skel, bone) = single_bone(&mut tree);

    // Starts aimed at +Z.
    let start = bone_forward(&tree, skel, bone);
    assert!(
        (start - Vector3::new(0.0, 0.0, 1.0)).length() < 1e-5,
        "the bone's +Z forward starts at (0,0,1), got {start:?}"
    );

    // Target straight out +X, so a correct aim is a 90° turn.
    let mut m = Node::new("Look", "LookAtModifier3D");
    m.set_property("bone", Variant::Int(bone as i64));
    m.set_property(
        "target_position",
        Variant::Vector3(Vector3::new(5.0, 0.0, 0.0)),
    );
    tree.add_child(skel, m).expect("modifier added");

    process_modifiers(&mut tree, skel);

    let aimed = bone_forward(&tree, skel, bone);
    assert!(
        (aimed - Vector3::new(1.0, 0.0, 0.0)).length() < 0.01,
        "the bone's forward axis must point at the target (1,0,0), got {aimed:?}"
    );
}

/// It must **track** a moving target rather than converge once and stick.
///
/// ⚠️ The second target is deliberately at **exactly** `-X`, i.e. 180° from where
/// the bone is then aimed. That is the anti-parallel degenerate case:
/// `from.cross(to)` vanishes, so there is no unique rotation axis. The first
/// version of `apply_look_at_modifier` returned early there and left the bone
/// stuck at `+X` — this test is what caught it. Do not "simplify" the target to
/// an off-axis value; that would silently drop the coverage.
#[test]
fn look_at_modifier_tracks_a_moving_target() {
    let mut tree = SceneTree::new();
    let (skel, bone) = single_bone(&mut tree);

    let mut m = Node::new("Look", "LookAtModifier3D");
    m.set_property("bone", Variant::Int(bone as i64));
    m.set_property(
        "target_position",
        Variant::Vector3(Vector3::new(5.0, 0.0, 0.0)),
    );
    let look = tree.add_child(skel, m).expect("modifier added");

    process_modifiers(&mut tree, skel);
    assert!((bone_forward(&tree, skel, bone) - Vector3::new(1.0, 0.0, 0.0)).length() < 0.01);

    // Move it to -X: a 180° swing from where the bone now points.
    tree.get_node_mut(look)
        .expect("modifier still in tree")
        .set_property(
            "target_position",
            Variant::Vector3(Vector3::new(-5.0, 0.0, 0.0)),
        );
    for _ in 0..4 {
        process_modifiers(&mut tree, skel);
    }
    let tracked = bone_forward(&tree, skel, bone);
    assert!(
        (tracked - Vector3::new(-1.0, 0.0, 0.0)).length() < 0.01,
        "the modifier must follow the target to (-1,0,0), got {tracked:?}"
    );
}

/// `use_angle_limitation` must cap the turn at `primary_limit_angle`.
///
/// This is the assertion that would fail if the limit default were read as `0.0`
/// instead of Godot's `TAU`: with a 0 limit every aim would be a no-op, and the
/// unlimited case below would look identical to the limited one.
#[test]
fn look_at_modifier_respects_its_angle_limit() {
    let limited_forward = |use_limit: bool| {
        let mut tree = SceneTree::new();
        let (skel, bone) = single_bone(&mut tree);
        let mut m = Node::new("Look", "LookAtModifier3D");
        m.set_property("bone", Variant::Int(bone as i64));
        // 90° away, but only 0.2 rad of turn allowed.
        m.set_property(
            "target_position",
            Variant::Vector3(Vector3::new(5.0, 0.0, 0.0)),
        );
        m.set_property("use_angle_limitation", Variant::Bool(use_limit));
        m.set_property("primary_limit_angle", Variant::Float(0.2));
        tree.add_child(skel, m).expect("modifier added");
        process_modifiers(&mut tree, skel);
        bone_forward(&tree, skel, bone)
    };

    // Unlimited: reaches the full 90°.
    let free = limited_forward(false);
    assert!(
        (free - Vector3::new(1.0, 0.0, 0.0)).length() < 0.01,
        "without the limit the bone reaches the target, got {free:?}"
    );

    // Limited: turns 0.2 rad from +Z and stops well short of +X.
    let capped = limited_forward(true);
    let turned = capped
        .dot(Vector3::new(0.0, 0.0, 1.0))
        .clamp(-1.0, 1.0)
        .acos();
    assert!(
        (turned - 0.2).abs() < 0.01,
        "the turn must be capped at primary_limit_angle=0.2 rad, got {turned}"
    );
    assert!(
        (capped - Vector3::new(1.0, 0.0, 0.0)).length() > 0.5,
        "a capped aim must NOT reach the target, got {capped:?}"
    );
}

/// `influence = 0` leaves the pose untouched, matching every other modifier here.
#[test]
fn look_at_modifier_influence_zero_is_a_no_op() {
    let mut tree = SceneTree::new();
    let (skel, bone) = single_bone(&mut tree);

    let mut m = Node::new("Look", "LookAtModifier3D");
    m.set_property("bone", Variant::Int(bone as i64));
    m.set_property(
        "target_position",
        Variant::Vector3(Vector3::new(5.0, 0.0, 0.0)),
    );
    m.set_property("influence", Variant::Float(0.0));
    tree.add_child(skel, m).expect("modifier added");

    process_modifiers(&mut tree, skel);

    let after = bone_forward(&tree, skel, bone);
    assert!(
        (after - Vector3::new(0.0, 0.0, 1.0)).length() < 1e-5,
        "influence 0 must leave the bone aimed at +Z, got {after:?}"
    );
}

/// An unset `bone` (-1, the registered default) must aim nothing rather than
/// panicking or defaulting to bone 0.
#[test]
fn look_at_modifier_with_unset_bone_does_nothing() {
    let mut tree = SceneTree::new();
    let (skel, bone) = single_bone(&mut tree);

    let mut m = Node::new("Look", "LookAtModifier3D");
    m.set_property(
        "target_position",
        Variant::Vector3(Vector3::new(5.0, 0.0, 0.0)),
    );
    tree.add_child(skel, m).expect("modifier added");

    process_modifiers(&mut tree, skel);

    let after = bone_forward(&tree, skel, bone);
    assert!(
        (after - Vector3::new(0.0, 0.0, 1.0)).length() < 1e-5,
        "with bone unset (-1) no bone may move, got {after:?}"
    );
}

/// The `SpringBone*` family's ClassDB schema (pat-0q2kx.156).
///
/// ⚠️ **Schema only — there is no simulation yet.** `process_modifiers` has no
/// `SpringBoneSimulator3D` branch, so such a node does not move any bone. This
/// test exists so the registration is pinned against Godot rather than drifting,
/// and it deliberately does *not* assert any physical behaviour, because there is
/// none to assert. The verlet chain + collision response is the remaining stage
/// of this bead.
#[test]
fn spring_bone_family_registers_godot_schema() {
    gdobject::class_db::register_3d_classes();

    // The simulator declares only THREE plain members. Every per-chain and
    // per-joint knob (stiffness / drag / gravity / radius) lives behind indexed
    // `settings/<i>/...` accessors in Godot, so a flat property for them here
    // would be an invented schema.
    let sim = gdobject::class_db::get_class_info("SpringBoneSimulator3D")
        .expect("SpringBoneSimulator3D registered");
    assert!(gdobject::is_parent_class(
        "SpringBoneSimulator3D",
        "SkeletonModifier3D"
    ));
    assert_eq!(
        sim.properties.len(),
        3,
        "Godot's SpringBoneSimulator3D declares exactly 3 plain members; got {:?}",
        sim.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    let sim_default = |name: &str| {
        sim.properties
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("SpringBoneSimulator3D has no '{name}'"))
            .default_value
            .clone()
    };
    assert_eq!(sim_default("setting_count"), Variant::Int(0));
    assert_eq!(sim_default("mutable_bone_axes"), Variant::Bool(true));
    assert_eq!(
        sim_default("external_force"),
        Variant::Vector3(Vector3::new(0.0, 0.0, 0.0))
    );

    // Collision shapes: the base carries the bone binding, each shape its own
    // geometry.
    for class in [
        "SpringBoneCollisionSphere3D",
        "SpringBoneCollisionCapsule3D",
        "SpringBoneCollisionPlane3D",
    ] {
        assert!(
            gdobject::is_parent_class(class, "SpringBoneCollision3D"),
            "{class} derives SpringBoneCollision3D"
        );
        assert!(
            gdobject::is_parent_class(class, "Node3D"),
            "{class} must reach Node3D through the chain"
        );
        // The bone binding is inherited, not redeclared per shape.
        assert!(
            gdobject::class_db::class_has_property(class, "bone"),
            "{class} inherits the base's 'bone'"
        );
    }

    let sphere = gdobject::class_db::get_class_info("SpringBoneCollisionSphere3D").unwrap();
    let sphere_radius = sphere
        .properties
        .iter()
        .find(|p| p.name == "radius")
        .expect("sphere has its own radius");
    assert_eq!(sphere_radius.default_value, Variant::Float(0.1));

    let capsule = gdobject::class_db::get_class_info("SpringBoneCollisionCapsule3D").unwrap();
    assert_eq!(
        capsule
            .properties
            .iter()
            .find(|p| p.name == "height")
            .expect("capsule has its own height")
            .default_value,
        Variant::Float(0.5)
    );

    // A plane has no geometry of its own — its normal comes from the node
    // transform, so declaring a size/extent here would invent one.
    let plane = gdobject::class_db::get_class_info("SpringBoneCollisionPlane3D").unwrap();
    assert!(
        plane.properties.is_empty(),
        "SpringBoneCollisionPlane3D declares nothing of its own, got {:?}",
        plane.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

/// A `SpringBoneSimulator3D` in the tree must be *skipped*, not crash the pass.
///
/// This pins current honest behaviour: the class is registered but unrouted, so
/// the modifier pass leaves the pose alone. When the verlet stage lands, this
/// test should be replaced by one asserting the chain actually sags — it is here
/// to prove "registered" has not been mistaken for "working".
#[test]
fn spring_bone_simulator_is_registered_but_not_yet_simulated() {
    let mut tree = SceneTree::new();
    let (skel, bone) = single_bone(&mut tree);

    let mut sim = Node::new("Spring", "SpringBoneSimulator3D");
    // Even with a downward external force, nothing should move yet.
    sim.set_property(
        "external_force",
        Variant::Vector3(Vector3::new(0.0, -9.8, 0.0)),
    );
    tree.add_child(skel, sim).expect("simulator added");

    for _ in 0..16 {
        process_modifiers(&mut tree, skel);
    }

    let after = bone_forward(&tree, skel, bone);
    assert!(
        (after - Vector3::new(0.0, 0.0, 1.0)).length() < 1e-5,
        "SpringBoneSimulator3D is not implemented yet, so the pass must leave \
         the bone untouched rather than half-simulate it; got {after:?}"
    );
}

/// The remaining registered leaves (pat-0q2kx.156).
///
/// ⚠️ **Registration only.** `process_modifiers` has no branch for any of these,
/// so a node of these classes does not move a bone. Pinned here so the schema
/// cannot drift, and asserted as *unrouted* below so "registered" is never read
/// as "working".
#[test]
fn remaining_modifier_leaves_register_their_schema() {
    gdobject::class_db::register_3d_classes();

    // Each declares exactly one plain member; the usable knobs are inherited.
    for (class, parent) in [("TwoBoneIK3D", "IKModifier3D"), ("SplineIK3D", "ChainIK3D")] {
        let info =
            gdobject::class_db::get_class_info(class).unwrap_or_else(|| panic!("{class} exists"));
        assert!(
            gdobject::is_parent_class(class, parent),
            "{class} derives {parent}"
        );
        assert_eq!(
            info.properties.len(),
            1,
            "{class} declares only setting_count; the rest are indexed settings. Got {:?}",
            info.properties.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
        assert_eq!(info.properties[0].name, "setting_count");
        assert_eq!(info.properties[0].default_value, Variant::Int(0));
        // The inherited knobs are what make the node configurable at all.
        assert!(
            gdobject::class_db::class_has_property(class, "target_position"),
            "{class} must inherit target_position through {parent}"
        );
    }

    let bone_target = gdobject::class_db::get_class_info("ModifierBoneTarget3D")
        .expect("ModifierBoneTarget3D exists");
    assert!(gdobject::is_parent_class(
        "ModifierBoneTarget3D",
        "SkeletonModifier3D"
    ));
    assert_eq!(bone_target.properties.len(), 2);
    assert_eq!(
        bone_target
            .properties
            .iter()
            .find(|p| p.name == "bone")
            .expect("has bone")
            .default_value,
        Variant::Int(-1)
    );

    // `SkeletonIK3D` is deliberately NOT registered: its `interpolation` member
    // has no default in the doc XML and no plain field in the header (the class
    // is deprecated in Godot 4), so registering 9 of its 10 members would
    // misreport the schema. Asserting its absence keeps that decision explicit
    // rather than looking like an oversight.
    assert!(
        !gdobject::class_exists("SkeletonIK3D"),
        "SkeletonIK3D is intentionally unregistered — see the note in class_db.rs. \
         If you register it, declare all 10 members and delete this assertion."
    );
}

/// None of the newly registered leaves are routed yet, so the modifier pass must
/// leave the pose untouched for each of them.
#[test]
fn registered_but_unrouted_leaves_do_not_move_bones() {
    for class in [
        "TwoBoneIK3D",
        "SplineIK3D",
        "ModifierBoneTarget3D",
        "SpringBoneSimulator3D",
    ] {
        let mut tree = SceneTree::new();
        let (skel, bone) = single_bone(&mut tree);

        let mut m = Node::new("M", class);
        m.set_property("bone", Variant::Int(bone as i64));
        m.set_property(
            "target_position",
            Variant::Vector3(Vector3::new(5.0, 0.0, 0.0)),
        );
        tree.add_child(skel, m).expect("modifier added");

        for _ in 0..8 {
            process_modifiers(&mut tree, skel);
        }

        let after = bone_forward(&tree, skel, bone);
        assert!(
            (after - Vector3::new(0.0, 0.0, 1.0)).length() < 1e-5,
            "{class} is registered but unrouted, so it must not move a bone \
             (a half-implementation is worse than none); got {after:?}"
        );
    }
}

/// **Regression guard for the shared dispatch.** This bead adds branches to
/// `process_modifiers`, which every skeleton modifier flows through, so the
/// pre-existing `FABRIK3D` path must still solve exactly as before.
///
/// Duplicated deliberately from `fabrik3d_chain_reaches_target`: that test's
/// command is 65 characters, past the verifier's 58-char truncation, so it cannot
/// be named as a `--test` target for this bead. Keeping the assertion inside this
/// binary means the regression is actually covered by a command that runs.
#[test]
fn fabrik3d_still_solves_after_dispatch_change() {
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);

    let target = Vector3::new(1.5, 1.0, 0.0);
    add_modifier(&mut tree, skel, "FABRIK3D", b0, tip, target);

    for _ in 0..8 {
        process_modifiers(&mut tree, skel);
    }

    let reached = tip_pos(&tree, skel, tip);
    assert!(
        (reached - target).length() < 0.05,
        "adding CCDIK3D/JacobianIK3D/LookAtModifier3D branches must not disturb \
         FABRIK3D: tip {reached:?} vs target {target:?}"
    );
}

/// The `IKModifier3D` path (CCD) must also survive the dispatch change.
#[test]
fn ik_modifier3d_still_solves_after_dispatch_change() {
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);

    let target = Vector3::new(0.0, 2.0, 1.2);
    add_modifier(&mut tree, skel, "IKModifier3D", b0, tip, target);

    for _ in 0..8 {
        process_modifiers(&mut tree, skel);
    }

    let reached = tip_pos(&tree, skel, tip);
    assert!(
        (reached - target).length() < 0.05,
        "IKModifier3D must still reach its target; got {reached:?}"
    );
}

/// A class the pass does not know must still be skipped rather than solved with
/// some default — the `_ => continue` arm has to keep working.
#[test]
fn unknown_modifier_class_is_still_skipped() {
    let mut tree = SceneTree::new();
    let (skel, b0, tip) = make_chain(&mut tree);

    add_modifier(
        &mut tree,
        skel,
        // Registered by this bead but deliberately NOT routed, so it still takes
        // the `_ => continue` arm. (It was unregistered when this test was
        // written; registering it must not change the outcome.)
        "SplineIK3D",
        b0,
        tip,
        Vector3::new(1.5, 1.0, 0.0),
    );

    for _ in 0..8 {
        process_modifiers(&mut tree, skel);
    }

    let after = tip_pos(&tree, skel, tip);
    assert!(
        (after - Vector3::new(0.0, 3.0, 0.0)).length() < 1e-5,
        "an unrouted modifier class must leave the pose untouched, got {after:?}"
    );
}
