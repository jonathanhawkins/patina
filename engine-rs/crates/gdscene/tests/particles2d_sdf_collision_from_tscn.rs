//! pat-mgixk.69 acceptance: an authored GPUParticles2D collides with an authored
//! LightOccluder2D floor.
//!
//! # What was wrong
//!
//! Two independent bugs, both of which had to be fixed for a single particle to
//! land on a floor:
//!
//! 1. **`collision_mode` was never read for 2D.** `main_loop` built a
//!    `CanvasSdf` every frame from the `sdf_collision` occluders and called
//!    `step_with_sdf` with it — but the 2D emitter block never set
//!    `material.collision_mode`, so the `Disabled` default short-circuited
//!    `resolve_sdf_collisions` on its first line. The field was rasterized and
//!    thrown away on every frame of every scene. (`grep -n collision_mode
//!    main_loop.rs` hit only the 3D reader.)
//! 2. **The field was sampled in the wrong space.** `resolve_sdf_collisions`
//!    passed `p.position` to `CanvasSdf::sample`, which takes a *canvas* point.
//!    `p.position` is relative to the transform stamped on the particle at spawn,
//!    so an emitter at (200, 100) sampled the occluder 200px left and 100px above
//!    where its particles actually were. Only an emitter at the canvas origin
//!    could ever have collided correctly — and the only existing test built its
//!    particles by hand with `spawn_transform: IDENTITY`, which is exactly the
//!    case that hides this.
//!
//! # The path driven
//!
//! ```text
//! .tscn (GPUParticles2D + LightOccluder2D) -> add_packed_scene_to_tree
//!   -> MainLoop::step -> process_particles -> CanvasSdf::generate
//!     -> ParticleSimulator::step_with_sdf -> resolve_sdf_collisions
//! ```

use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{MainLoop, SceneTree};
use gdvariant::Variant;

/// Canvas size, which is also the SDF's frame. It has to contain both the
/// emitter and the floor or the field would not cover them.
const VW: u32 = 400;
const VH: u32 = 400;

/// Top edge of the floor, and the line no particle may fall past.
const FLOOR_Y: f32 = 300.0;

/// 🔑 The floor polygon is **CLOSED**. `CanvasSdf::generate` only assigns
/// negative (inside) distances within a closed polygon — `if *closed &&
/// point_in_polygon(..)` (`gdrender2d/src/sdf2d.rs:274`), and its own doc says
/// that is "what `closed` means physically". An open polyline at y=300 would
/// produce a field that never goes negative, so nothing would ever collide and
/// this test would fail for a reason that has nothing to do with the bead.
///
/// `collision_mode` is spliced so the two variants differ in exactly that one
/// property.
fn scene(collision_mode: i64) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="OccluderPolygon2D" id="Occ_1"]
closed = true
polygon = PackedVector2Array(0, 300, 400, 300, 400, 360, 0, 360)

[node name="World" type="Node2D"]

[node name="Floor" type="LightOccluder2D" parent="."]
occluder = SubResource("Occ_1")
sdf_collision = true

[node name="Puff" type="GPUParticles2D" parent="."]
position = Vector2(200, 100)
emitting = true
amount = 24
lifetime = 10.0
one_shot = false
explosiveness = 0.0
gravity = Vector3(0, 600, 0)
initial_velocity_min = 0.0
initial_velocity_max = 0.0
spread = 0.0
collision_mode = {collision_mode}
collision_base_size = 1.0
"#
    )
}

/// Boots the scene and runs 120 frames at 1/60s (two seconds).
///
/// Returns the published particle positions in CANVAS space.
///
/// 🔑 Read from the node's own `_particle_positions`, which `process_particles`
/// publishes every frame — a surface that exists at HEAD, so this test compiles
/// against the unmodified engine and its red check is a behavioural failure
/// rather than a compile error. (An accessor added by this bead would have made
/// the revert fail to build, which passes the red check while proving nothing.)
///
/// Those positions are emitter-LOCAL, which is exactly what a renderer draws
/// through the node's global transform, so converting with that transform is the
/// same round trip the renderer performs.
fn run(collision_mode: i64) -> Vec<Vector2> {
    gdobject::class_db::register_all_classes();
    let src = scene(collision_mode);
    let packed = PackedScene::from_tscn(&src).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("the scene instances");

    // ⚠️ `sdf_scale = 0` is the 100% enum index (`sdf_scale_factor`), not a
    // multiplier. The shipped default is index 1 = 50%, which samples the field
    // at one texel per two canvas units and would resolve the floor's surface
    // only to within ~2px. Asking for the full-resolution field keeps the
    // tolerance below honest rather than papering over quantisation.
    if let Some(n) = tree.get_node_mut(root) {
        n.set_property("sdf_scale", Variant::Int(0));
    }

    let emitter = tree
        .get_node_by_path("/root/World/Puff")
        .expect("the fixture has /root/World/Puff");

    let mut ml = MainLoop::new(tree);
    ml.set_viewport_size(VW, VH);
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }

    let local = match ml
        .tree()
        .get_node(emitter)
        .map(|n| n.get_property("_particle_positions"))
    {
        Some(Variant::PackedVector2Array(v)) => v,
        _ => Vec::new(),
    };
    let to_global = gdscene::node2d::get_global_transform(ml.tree(), emitter);
    local.into_iter().map(|p| to_global.xform(p)).collect()
}

/// **The acceptance.** With `collision_mode = 1` (RIGID) the particles rest on
/// the floor; with `0` (DISABLED) they fall straight through it.
#[test]
fn particles2d_sdf_collision_from_tscn() {
    let collided = run(1);
    let passed_through = run(0);

    // -- the control first: the particles really are falling -----------------
    //
    // Without this the assertions below would pass just as well on an emitter
    // that never emitted, or on particles frozen at their spawn point.
    assert!(
        !passed_through.is_empty(),
        "the emitter must produce particles at all; an empty published list \
         means the fixture never emitted and nothing below proves anything",
    );
    let deepest = passed_through
        .iter()
        .map(|p| p.y)
        .fold(f32::MIN, |a, b| if b > a { b } else { a });
    assert!(
        deepest > FLOOR_Y + 60.0,
        "with collision_mode = 0 the particles must fall THROUGH the floor \
         (deepest y = {deepest}, floor at {FLOOR_Y}). Gravity 600 over two \
         seconds is ~1200px of fall, so a deepest y near the floor would mean \
         they were stopped by something other than collision",
    );

    // -- and with collision on, none of them gets past it --------------------
    assert!(
        !collided.is_empty(),
        "the colliding run must still have live particles — COLLISION_RIGID \
         rests them, it does not remove them (that is HIDE_ON_CONTACT)",
    );
    let worst = collided
        .iter()
        .map(|p| p.y)
        .fold(f32::MIN, |a, b| if b > a { b } else { a });
    // The field is sampled, not analytic: at 100% scale a texel is one canvas
    // unit and `sample` interpolates between texel centres, so the resolved
    // surface sits within ~1px of y=300. The collision radius
    // (`collision_base_size * scale * 0.5`) holds the particle a further 0.5px
    // off. 2px covers both; the control above is 60px away, so the two outcomes
    // are never in doubt.
    assert!(
        worst <= FLOOR_Y + 2.0,
        "no particle may fall past the floor once collision_mode = 1: deepest \
         y = {worst}, floor at {FLOOR_Y}. This is the whole bead — before it, \
         `collision_mode` was never read for 2D so the material's Disabled \
         default short-circuited, and the SDF was sampled in the emitter's local \
         space so an emitter at (200, 100) tested the wrong point entirely",
    );

    // -- and some actually rest ON it, rather than stopping early ------------
    //
    // 🔑 Guards the direction of the fix. A collision resolver that pushed
    // particles far away, or a spawn that never fell, would satisfy "nothing
    // below the floor" while colliding with nothing.
    let resting = collided
        .iter()
        .filter(|p| (p.y - FLOOR_Y).abs() <= 6.0)
        .count();
    assert!(
        resting > 0,
        "some particle must come to rest AT the floor: none of {} was within \
         6px of y={FLOOR_Y} (deepest {worst}). Particles that merely stay high \
         up never touched the field",
        collided.len(),
    );
}
