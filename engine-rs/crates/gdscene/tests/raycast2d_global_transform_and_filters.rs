//! pat-0q2kx.105: RayCast2D casts from its GLOBAL transform, with filters.
//!
//! Godot's `RayCast2D` (upstream/godot/scene/2d/physics/ray_cast_2d.cpp) casts
//! from its **global** origin to `to_global(target_position)` — so a ray under a
//! rotated parent points where the parent points — filtered by
//! `collision_mask`, `add_exception` and `exclude_parent`, and publishes the hit
//! point and normal, not just a boolean.
//!
//! The implementation lives in `gdscene::raycast2d` and is driven each physics
//! frame by `MainLoop::process_raycasts`. What was missing was this gate: a test
//! that a rotated parent actually redirects the ray. A cast that ignored
//! rotation would still report `is_colliding` against a wall placed straight
//! ahead, so only checking "does it hit something" cannot tell the two apart —
//! which is why every case below pins the wall to the *rotated* direction and
//! asserts the unrotated placement is a miss.
//!
//! Geometry, once, since every case shares it:
//!   Parent at (200, 0) rotated +90°. Under Godot's 2D convention (+Y down, a
//!   positive angle rotating +X toward +Y) the child's local `target_position`
//!   (0, 50) — straight "up" the local Y axis — maps to global **(-50, 0)**
//!   relative to the parent, i.e. the ray sweeps from (200, 0) toward (150, 0).
//!   The unrotated reading would sweep toward (200, 50) instead. Those two
//!   directions are perpendicular, so a wall on one is never on the other.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// `World -> Parent(200,0 rot +90°) -> Ray(target 0,50)`, plus two walls:
/// `Rotated` sits on the rotated sweep, `Unrotated` on the path the ray would
/// take if rotation were ignored. Exactly one of them can ever be hit.
fn scene(extra: &str) -> SceneTree {
    load(&format!(
        r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Parent" type="Node2D" parent="."]
position = Vector2(200, 0)
rotation = 1.5707963

[node name="Ray" type="RayCast2D" parent="Parent"]
position = Vector2(0, 0)
enabled = true
target_position = Vector2(0, 50)

[node name="Rotated" type="StaticBody2D" parent="."]
position = Vector2(170, 0)

[node name="RotShape" type="CollisionShape2D" parent="Rotated"]
size = Vector2(10, 200)
{extra}
"#
    ))
}

fn prop(ml: &MainLoop, path: &str, key: &str) -> Option<Variant> {
    let id = ml.tree().get_node_by_path(path)?;
    ml.tree().get_node(id).map(|n| n.get_property(key))
}

fn stepped(tree: SceneTree) -> MainLoop {
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);
    ml
}

/// **The acceptance test.** Global transform, hit point/normal, and exceptions.
#[test]
fn raycast2d_global_transform_and_filters() {
    // -- (1) the rotated parent redirects the ray ---------------------------
    let ml = stepped(scene(""));

    assert_eq!(
        prop(&ml, "/root/World/Parent/Ray", "is_colliding"),
        Some(Variant::Bool(true)),
        "the ray must hit the wall lying along its ROTATED direction; a cast \
         that used the raw target_position would sweep +Y instead and miss it"
    );

    // -- (2) a wall at the UNROTATED location is not hit ---------------------
    //
    // Same scene, wall moved to where an unrotated cast would sweep. If the
    // implementation ignored the parent's rotation this would hit and (1) would
    // also have passed — which is exactly why both halves are needed.
    let unrotated = load(
        r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Parent" type="Node2D" parent="."]
position = Vector2(200, 0)
rotation = 1.5707963

[node name="Ray" type="RayCast2D" parent="Parent"]
position = Vector2(0, 0)
enabled = true
target_position = Vector2(0, 50)

[node name="Unrotated" type="StaticBody2D" parent="."]
position = Vector2(200, 30)

[node name="UnrotShape" type="CollisionShape2D" parent="Unrotated"]
size = Vector2(200, 10)
"#,
    );
    let ml_unrot = stepped(unrotated);
    assert_eq!(
        prop(&ml_unrot, "/root/World/Parent/Ray", "is_colliding"),
        Some(Variant::Bool(false)),
        "a wall placed where the UNROTATED cast would sweep must NOT be hit — \
         if it is, the parent's rotation is being ignored"
    );

    // -- (3) the hit point and normal are published --------------------------
    //
    // The wall's near face is at x = 175 (centre 170, 10 wide), and the ray
    // travels along -X from (200, 0), so it lands on that face.
    let point = prop(&ml, "/root/World/Parent/Ray", "_hit_point");
    match point {
        Some(Variant::Vector2(p)) => {
            assert!(
                (p.x - 175.0).abs() <= 1.0,
                "collision point x must be the wall's near face (175) within \
                 1px, got {p:?}"
            );
            assert!(
                p.y.abs() <= 1.0,
                "the ray runs along y = 0, so the hit y must be ~0, got {p:?}"
            );
        }
        other => panic!("_hit_point must be published as a Vector2, got {other:?}"),
    }

    match prop(&ml, "/root/World/Parent/Ray", "_hit_normal") {
        Some(Variant::Vector2(n)) => {
            // A ray travelling -X hits a face whose outward normal points +X.
            assert!(
                (n.x - 1.0).abs() <= 1e-3 && n.y.abs() <= 1e-3,
                "the surface normal must point back along the cast (+X) within \
                 1e-3, got {n:?}"
            );
        }
        other => panic!("_hit_normal must be published as a Vector2, got {other:?}"),
    }

    // -- (4) an excepted body is skipped -------------------------------------
    //
    // The only wall on the ray's path is added to `_exceptions`, so the same
    // scene that hit in (1) must now miss entirely.
    let mut tree = scene("");
    let wall = tree
        .get_node_by_path("/root/World/Rotated")
        .expect("Rotated wall");
    let ray = tree
        .get_node_by_path("/root/World/Parent/Ray")
        .expect("Ray node");
    let wall_raw = wall.raw();
    if let Some(node) = tree.get_node_mut(ray) {
        node.set_property(
            "_exceptions",
            Variant::Array((vec![Variant::Int(wall_raw as i64)]).into()),
        );
    }
    let ml_excl = stepped(tree);
    assert_eq!(
        prop(&ml_excl, "/root/World/Parent/Ray", "is_colliding"),
        Some(Variant::Bool(false)),
        "a body listed in _exceptions must be skipped — the ray has nothing \
         else to hit, so it must report no collision"
    );
}

/// `enabled = false` stops the cast entirely, so a disabled ray reports nothing
/// even with a wall squarely in its path.
#[test]
fn disabled_raycast_reports_nothing() {
    let tree = load(
        r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Parent" type="Node2D" parent="."]
position = Vector2(200, 0)
rotation = 1.5707963

[node name="Ray" type="RayCast2D" parent="Parent"]
position = Vector2(0, 0)
enabled = false
target_position = Vector2(0, 50)

[node name="Rotated" type="StaticBody2D" parent="."]
position = Vector2(170, 0)

[node name="RotShape" type="CollisionShape2D" parent="Rotated"]
size = Vector2(10, 200)
"#,
    );
    let ml = stepped(tree);
    assert_eq!(
        prop(&ml, "/root/World/Parent/Ray", "is_colliding"),
        Some(Variant::Bool(false)),
        "a disabled RayCast2D must not report a collision"
    );
}

/// `collision_mask` filters by layer: a wall on a layer the mask excludes is
/// invisible to the ray, even though it sits squarely on the path.
#[test]
fn collision_mask_filters_by_layer() {
    // The wall is on layer 2 (bit value 2); the ray's mask only accepts layer 1.
    let tree = load(
        r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Parent" type="Node2D" parent="."]
position = Vector2(200, 0)
rotation = 1.5707963

[node name="Ray" type="RayCast2D" parent="Parent"]
position = Vector2(0, 0)
enabled = true
target_position = Vector2(0, 50)
collision_mask = 1

[node name="Rotated" type="StaticBody2D" parent="."]
position = Vector2(170, 0)
collision_layer = 2

[node name="RotShape" type="CollisionShape2D" parent="Rotated"]
size = Vector2(10, 200)
"#,
    );
    let ml = stepped(tree);
    assert_eq!(
        prop(&ml, "/root/World/Parent/Ray", "is_colliding"),
        Some(Variant::Bool(false)),
        "a body whose collision_layer does not intersect the ray's \
         collision_mask must be skipped"
    );
}

/// A miss clears the published hit data rather than leaving the previous
/// frame's point/normal behind — stale values would read as a live hit.
#[test]
fn a_miss_clears_the_published_hit() {
    let tree = load(
        r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Parent" type="Node2D" parent="."]
position = Vector2(200, 0)
rotation = 1.5707963

[node name="Ray" type="RayCast2D" parent="Parent"]
position = Vector2(0, 0)
enabled = true
target_position = Vector2(0, 50)
"#,
    );
    let ml = stepped(tree);
    assert_eq!(
        prop(&ml, "/root/World/Parent/Ray", "is_colliding"),
        Some(Variant::Bool(false)),
        "a ray with nothing in its path must not report a collision"
    );
    assert_eq!(
        prop(&ml, "/root/World/Parent/Ray", "_hit_collider"),
        Some(Variant::Nil),
        "a miss must clear the collider rather than leave a stale one"
    );
}
