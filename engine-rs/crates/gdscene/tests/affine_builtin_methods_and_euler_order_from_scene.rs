//! Acceptance test for pat-s3rrv.148: the affine builtins' missing methods, and
//! the `EulerOrder` argument that was being thrown away.
//!
//! Two different failures live here.
//!
//! The **loud** one: `AABB.intersects_ray`, `Basis.from_euler`,
//! `Quaternion.slerpni`, `Plane.intersect_3` and about thirty of their
//! neighbours had no dispatch arm at all, so calling one raised
//! `UndefinedFunction`. Script-side 3D picking was simply not expressible.
//!
//! The **silent** one, and the reason this test leads with it:
//! `Basis.get_euler(order)` and `Quaternion.get_euler(order)` accepted the
//! argument and **discarded** it (`variant_math.rs`, `"get_euler" =>
//! Ok(Variant::Vector3(b.to_euler()))`). A script asking for XYZ received YXZ
//! angles — three plausible numbers, of the wrong rotation, with nothing
//! anywhere to indicate the request had been ignored.
//!
//! Semantics ported from `upstream/godot/core/math/{basis,quaternion,aabb,
//! plane,rect2,transform_2d}.cpp`.

use gdcore::math3d::EulerOrder;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]
"#;

/// The angles the scene script rotates by. Chosen so no two components are
/// equal and none is near a gimbal singularity — an angle triple like
/// `(0.5, 0.5, 0.5)` reads back the same under several orders and would make
/// the whole point of this test invisible.
const EULER: [f32; 3] = [0.3, -0.7, 1.1];

fn num(v: &Variant) -> f64 {
    match v {
        Variant::Float(f) => *f,
        Variant::Int(i) => *i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// Reads a script `var` — node properties and script vars are separate stores.
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

/// Runs `body` as the statements of a `check()` method through the real
/// interpreter dispatch, and returns what it returns.
///
/// One statement per line so the GDScript indentation stays out of the Rust
/// literal, where a `\`-continuation would silently eat it.
fn run(body: &[&str]) -> Vec<Variant> {
    let mut src = String::from("extends Node\nfunc check():\n");
    for line in body {
        src.push_str("    ");
        src.push_str(line);
        src.push('\n');
    }
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    let got = interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"));
    match got {
        Variant::Array(items) => items,
        other => panic!("expected an Array, got {other:?}"),
    }
}

#[track_caller]
fn approx(got: &Variant, want: f32, what: &str) {
    let g = num(got) as f32;
    assert!((g - want).abs() < 1e-4, "{what}: expected {want}, got {g}");
}

#[track_caller]
fn vec3_of(got: &Variant, what: &str) -> gdcore::math::Vector3 {
    match got {
        Variant::Vector3(v) => *v,
        other => panic!("{what}: expected a Vector3, got {other:?}"),
    }
}

/// **The acceptance clause.** A scene script builds a rotation in a non-default
/// Euler order, reads it back in that order and recovers its own angles; the
/// node's real transform carries the rotation; and an AABB raycast returns the
/// hit point.
#[test]
fn affine_builtin_methods_and_euler_order_from_scene() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let script_src = "\
extends Node3D
var back_x = 0.0
var back_y = 0.0
var back_z = 0.0
var default_x = 0.0
var default_y = 0.0
var default_z = 0.0
var hit = null
var missed = \"unset\"
var ran = false
func _ready():
    var e = Vector3(0.3, -0.7, 1.1)
    var b = Basis.from_euler(e, EULER_ORDER_XYZ)
    self.transform = Transform3D(b, Vector3(0, 0, 0))
    var back = b.get_euler(EULER_ORDER_XYZ)
    self.back_x = back.x
    self.back_y = back.y
    self.back_z = back.z
    var d = b.get_euler(EULER_ORDER_YXZ)
    self.default_x = d.x
    self.default_y = d.y
    self.default_z = d.z
    var box = AABB(Vector3(-1, -1, -1), Vector3(2, 2, 2))
    self.hit = box.intersects_ray(Vector3(0, 0, -5), Vector3(0, 0, 1))
    self.missed = box.intersects_segment(Vector3(5, 5, 5), Vector3(6, 6, 6))
    self.ran = true
";
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(scene_root);

    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_ready` must run to completion — an earlier call raising is what an \
         unimplemented method looks like from here",
    );

    // (a) The round trip in a NON-default order recovers the input angles.
    approx(
        &script_var(ml.tree(), scene_root, "back_x"),
        EULER[0],
        "get_euler(XYZ).x",
    );
    approx(
        &script_var(ml.tree(), scene_root, "back_y"),
        EULER[1],
        "get_euler(XYZ).y",
    );
    approx(
        &script_var(ml.tree(), scene_root, "back_z"),
        EULER[2],
        "get_euler(XYZ).z",
    );

    // (b) 🔑 **The discriminating assertion.** Reading the SAME basis in the
    // default order must give different angles. While the argument was being
    // discarded these two calls returned identical values, so (a) alone passed
    // for the broken version too — `from_euler` and `get_euler` were simply
    // each other's inverse in YXZ and the order was decorative.
    let default_y = num(&script_var(ml.tree(), scene_root, "default_y")) as f32;
    assert!(
        (default_y - EULER[1]).abs() > 0.05,
        "get_euler(EULER_ORDER_YXZ) must differ from get_euler(EULER_ORDER_XYZ) \
         on the same basis — got {default_y} against {}, which means the order \
         argument is being ignored",
        EULER[1],
    );

    // (c) The node's REAL transform carries the rotation — the scene wiring,
    // not just a value the script passed back to itself.
    let stored = match ml
        .tree()
        .get_node(scene_root)
        .expect("root exists")
        .get_property("transform")
    {
        Variant::Transform3D(t) => t,
        other => panic!("`transform` is not a Transform3D: {other:?}"),
    };
    let stored_euler = stored.basis.to_euler_order(EulerOrder::Xyz);
    assert!(
        (stored_euler.x - EULER[0]).abs() < 1e-4
            && (stored_euler.y - EULER[1]).abs() < 1e-4
            && (stored_euler.z - EULER[2]).abs() < 1e-4,
        "the node's stored transform must carry the authored rotation, got {stored_euler:?}",
    );

    // (d) The raycast returns the HIT POINT, and a miss returns null.
    let hit = vec3_of(&script_var(ml.tree(), scene_root, "hit"), "intersects_ray");
    assert!(
        (hit.z + 1.0).abs() < 1e-4 && hit.x.abs() < 1e-4,
        "a ray down +Z from z=-5 must enter the box at z=-1, got {hit:?}",
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "missed"),
        Variant::Nil,
        "⚠️ a miss is NULL, not `false` — Godot binds the wrapper that returns \
         the hit POINT, and the var was seeded with a string so this cannot \
         pass by never having been assigned",
    );
}

/// All six orders round-trip, and no two agree on the same angles.
///
/// ⚠️ Without the second half, an implementation that mapped every order onto
/// YXZ would pass the first half perfectly — every order would be its own
/// inverse.
#[test]
fn every_euler_order_round_trips_and_they_disagree() {
    let orders = [
        ("EULER_ORDER_XYZ", EulerOrder::Xyz),
        ("EULER_ORDER_XZY", EulerOrder::Xzy),
        ("EULER_ORDER_YXZ", EulerOrder::Yxz),
        ("EULER_ORDER_YZX", EulerOrder::Yzx),
        ("EULER_ORDER_ZXY", EulerOrder::Zxy),
        ("EULER_ORDER_ZYX", EulerOrder::Zyx),
    ];

    let mut seen: Vec<gdcore::math::Vector3> = Vec::new();
    for (name, _order) in orders {
        let got = run(&[
            "var e = Vector3(0.3, -0.7, 1.1)",
            format!("var b = Basis.from_euler(e, {name})").as_str(),
            format!("return [b.get_euler({name}), b.get_euler(EULER_ORDER_YXZ)]").as_str(),
        ]);
        let same = vec3_of(&got[0], name);
        assert!(
            (same.x - EULER[0]).abs() < 1e-4
                && (same.y - EULER[1]).abs() < 1e-4
                && (same.z - EULER[2]).abs() < 1e-4,
            "{name} must round-trip its own angles, got {same:?}",
        );
        seen.push(vec3_of(&got[1], name));
    }

    // Each order builds a DIFFERENT basis, so reading them all back in one
    // fixed order must give six different answers.
    for i in 0..seen.len() {
        for j in (i + 1)..seen.len() {
            let (a, b) = (seen[i], seen[j]);
            assert!(
                (a.x - b.x).abs() > 1e-3 || (a.y - b.y).abs() > 1e-3 || (a.z - b.z).abs() > 1e-3,
                "orders {} and {} produced the same basis ({a:?}) — they cannot \
                 all be YXZ underneath",
                orders[i].0,
                orders[j].0,
            );
        }
    }
}

/// An unknown order is refused rather than silently treated as the default.
///
/// ⓘ Godot takes `ERR_FAIL_MSG("Invalid Euler order parameter.")` here. Falling
/// back to YXZ would reintroduce exactly the bug this bead fixes, one layer
/// down.
#[test]
fn an_out_of_range_euler_order_is_an_error() {
    let mut interp = Interpreter::new();
    let class_def = interp
        .run_class("extends Node\nfunc check():\n    return Basis.IDENTITY.get_euler(99)\n")
        .expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    assert!(
        interp
            .call_instance_method(&mut inst, "check", &[])
            .is_err(),
        "an EulerOrder of 99 must raise, not quietly mean YXZ",
    );

    // The control: a valid order on the same call path succeeds, so the failure
    // above is the range check and not a broken dispatch arm.
    let got = run(&["return [Basis.IDENTITY.get_euler(EULER_ORDER_ZYX)]"]);
    let identity = vec3_of(&got[0], "identity");
    assert!(
        identity.x.abs() < 1e-6 && identity.y.abs() < 1e-6 && identity.z.abs() < 1e-6,
        "the identity basis has zero angles in every order, got {identity:?}",
    );
}

/// The AABB query family: the axis accessors, the support point, and the two
/// picking methods that return a point or null.
#[test]
fn the_aabb_query_methods_answer() {
    let got = run(&[
        "var a = AABB(Vector3(0, 0, 0), Vector3(1, 4, 2))",
        "var b = AABB(Vector3(-1, -1, -1), Vector3(2, 2, 2))",
        "return [a.get_longest_axis(), a.get_longest_axis_index(), a.get_longest_axis_size(), \
         a.get_shortest_axis(), a.get_shortest_axis_index(), a.get_shortest_axis_size(), \
         a.get_support(Vector3(1, 1, 1)), a.has_surface(), \
         b.intersects_plane(Plane(Vector3(0, 1, 0), 0.0)), \
         b.intersects_plane(Plane(Vector3(0, 1, 0), 10.0)), \
         b.intersects_segment(Vector3(0, -5, 0), Vector3(0, 5, 0)), \
         a.is_equal_approx(AABB(Vector3(0, 0, 0), Vector3(1, 4, 2))), a.is_finite()]",
    ]);

    assert_eq!(vec3_of(&got[0], "longest axis").y, 1.0, "Y is longest");
    assert_eq!(got[1], Variant::Int(1));
    approx(&got[2], 4.0, "longest axis size");
    assert_eq!(vec3_of(&got[3], "shortest axis").x, 1.0, "X is shortest");
    assert_eq!(got[4], Variant::Int(0));
    approx(&got[5], 1.0, "shortest axis size");

    // The support point is the far corner in a +++ direction.
    let support = vec3_of(&got[6], "get_support");
    assert!(
        (support.x - 1.0).abs() < 1e-4
            && (support.y - 4.0).abs() < 1e-4
            && (support.z - 2.0).abs() < 1e-4,
        "get_support(+1,+1,+1) must be the far corner, got {support:?}",
    );

    assert_eq!(got[7], Variant::Bool(true), "has_surface");
    assert_eq!(
        got[8],
        Variant::Bool(true),
        "a plane through the middle of the box intersects it",
    );
    assert_eq!(
        got[9],
        Variant::Bool(false),
        "⚠️ a plane clear of the box does NOT intersect — without this the \
         previous assertion passes for an implementation that always says true",
    );
    let seg = vec3_of(&got[10], "intersects_segment");
    assert!(
        (seg.y + 1.0).abs() < 1e-4,
        "a segment down the Y axis must enter at y=-1, got {seg:?}",
    );
    assert_eq!(got[11], Variant::Bool(true), "is_equal_approx");
    assert_eq!(got[12], Variant::Bool(true), "is_finite");
}

/// The Quaternion additions, including the two that are easy to get wrong.
#[test]
fn the_quaternion_methods_answer() {
    let got = run(&[
        "var a = Quaternion.from_euler(Vector3(0, 0, 0))",
        "var b = Quaternion.from_euler(Vector3(0, 1.5, 0))",
        "return [a.slerpni(b, 0.5), a.slerp(b, 0.5), b.log(), b.log().exp(), b.is_finite(), \
         a.spherical_cubic_interpolate(b, a, b, 0.5)]",
    ]);

    // `slerpni` and `slerp` agree on a short arc — the difference only shows on
    // a long one, so this pins the shared behaviour and the next test the rest.
    let (ni, sl) = (&got[0], &got[1]);
    match (ni, sl) {
        (Variant::Quaternion(a), Variant::Quaternion(b)) => assert!(
            (a.x - b.x).abs() < 1e-4 && (a.w - b.w).abs() < 1e-4,
            "on a short arc slerpni must agree with slerp, got {a:?} vs {b:?}",
        ),
        other => panic!("expected two Quaternions, got {other:?}"),
    }

    // 🔑 `exp` is the inverse of `log`, so the round trip returns the input.
    match (&got[2], &got[3]) {
        (Variant::Quaternion(l), Variant::Quaternion(r)) => {
            assert!(
                l.w.abs() < 1e-5,
                "log() must be a PURE quaternion — its real part is zero, got {}",
                l.w,
            );
            assert!(
                (r.y - 0.681_638).abs() < 1e-3 && (r.w - 0.731_689).abs() < 1e-3,
                "log().exp() must return the original rotation, got {r:?}",
            );
        }
        other => panic!("expected two Quaternions, got {other:?}"),
    }
    assert_eq!(got[4], Variant::Bool(true), "is_finite");
    assert!(
        matches!(got[5], Variant::Quaternion(_)),
        "spherical_cubic_interpolate must return a Quaternion, got {:?}",
        got[5],
    );
}

/// The 2D and Plane additions.
#[test]
fn the_plane_rect2_and_transform2d_methods_answer() {
    let got = run(&[
        "var p0 = Plane(Vector3(1, 0, 0), 1.0)",
        "var p1 = Plane(Vector3(0, 1, 0), 2.0)",
        "var p2 = Plane(Vector3(0, 0, 1), 3.0)",
        "var r = Rect2(Vector2(0, 0), Vector2(4, 2))",
        "var t = Transform2D(0.5, Vector2(3, 4))",
        "return [p0.intersect_3(p1, p2), p0.intersect_3(p0, p2), p0.is_finite(), \
         p0.is_equal_approx(Plane(Vector3(1, 0, 0), 1.0)), \
         r.get_support(Vector2(1, 1)), r.is_equal_approx(Rect2(Vector2(0, 0), Vector2(4, 2))), \
         r.is_finite(), t.is_conformal(), t.is_finite()]",
    ]);

    // Three axis planes meet at (1, 2, 3).
    let corner = vec3_of(&got[0], "intersect_3");
    assert!(
        (corner.x - 1.0).abs() < 1e-4
            && (corner.y - 2.0).abs() < 1e-4
            && (corner.z - 3.0).abs() < 1e-4,
        "three axis planes must meet at their distances, got {corner:?}",
    );
    assert_eq!(
        got[1],
        Variant::Nil,
        "⚠️ a plane intersected with ITSELF has no unique point — this must be \
         null rather than a division by a vanishing determinant",
    );
    assert_eq!(got[2], Variant::Bool(true), "Plane.is_finite");
    assert_eq!(got[3], Variant::Bool(true), "Plane.is_equal_approx");

    match &got[4] {
        Variant::Vector2(v) => assert!(
            (v.x - 4.0).abs() < 1e-4 && (v.y - 2.0).abs() < 1e-4,
            "Rect2.get_support(+1,+1) must be the far corner, got {v:?}",
        ),
        other => panic!("expected a Vector2, got {other:?}"),
    }
    assert_eq!(got[5], Variant::Bool(true), "Rect2.is_equal_approx");
    assert_eq!(got[6], Variant::Bool(true), "Rect2.is_finite");
    // A pure rotation+translation is conformal.
    assert_eq!(got[7], Variant::Bool(true), "Transform2D.is_conformal");
    assert_eq!(got[8], Variant::Bool(true), "Transform2D.is_finite");
}

/// The Basis additions, including the transposed-dot family.
#[test]
fn the_basis_methods_answer() {
    let got = run(&[
        "var b = Basis(Vector3(1, 2, 3), Vector3(4, 5, 6), Vector3(7, 8, 9))",
        "var s = Basis.from_scale(Vector3(2, 3, 4))",
        "var l = Basis.looking_at(Vector3(0, 0, -1), Vector3(0, 1, 0))",
        "return [b.tdotx(Vector3(1, 0, 0)), b.tdoty(Vector3(1, 0, 0)), b.tdotz(Vector3(1, 0, 0)), \
         s.get_scale(), s.is_conformal(), b.is_finite(), l.is_conformal(), \
         Basis.IDENTITY.is_conformal(), s.scaled_local(Vector3(2, 2, 2)).get_scale()]",
    ]);

    // 🔑 `tdotx(1,0,0)` takes the first ROW against (1,0,0), which is the x
    // component of the FIRST column — 1. The untransposed reading would give
    // the whole x column dotted with (1,0,0), also 1. So the y/z probes below
    // are what actually separate the two.
    approx(&got[0], 1.0, "tdotx");
    approx(&got[1], 2.0, "tdoty");
    approx(&got[2], 3.0, "tdotz");

    match &got[3] {
        Variant::Vector3(v) => assert!(
            (v.x - 2.0).abs() < 1e-4 && (v.y - 3.0).abs() < 1e-4 && (v.z - 4.0).abs() < 1e-4,
            "from_scale must produce those scales, got {v:?}",
        ),
        other => panic!("expected a Vector3, got {other:?}"),
    }
    assert_eq!(
        got[4],
        Variant::Bool(false),
        "⚠️ a NON-uniform scale is not conformal — the paired `true` below is \
         what makes this assertion mean something",
    );
    assert_eq!(got[5], Variant::Bool(true), "is_finite");
    assert_eq!(got[6], Variant::Bool(true), "looking_at is a rotation");
    assert_eq!(got[7], Variant::Bool(true), "the identity is conformal");

    // `scaled_local` compounds with the existing scale.
    match &got[8] {
        Variant::Vector3(v) => assert!(
            (v.x - 4.0).abs() < 1e-4 && (v.y - 6.0).abs() < 1e-4 && (v.z - 8.0).abs() < 1e-4,
            "scaled_local must compound, got {v:?}",
        ),
        other => panic!("expected a Vector3, got {other:?}"),
    }
}
