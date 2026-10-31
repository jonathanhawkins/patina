//! pat-mgixk.70 acceptance: 3D particles spawn at their emitter and are DRAWN.
//!
//! Godot ground truth: `scene/3d/gpu_particles_3d.cpp` (`draw_pass_1`,
//! `draw_passes`), `scene/3d/cpu_particles_3d.cpp` (`mesh` via MultiMesh), and
//! `doc/classes/GPUParticles3D.xml` `local_coords` — with the default
//! `local_coords = false`, particles spawn at the emitter's GLOBAL transform
//! and stay in world space.
//!
//! # Two independent bugs, both of which this fixture sees
//!
//! * **Every 3D emitter spawned at the world origin.** `ParticleEmitter3D` had
//!   no transform field at all and `EmissionShape3D::sample` starts at
//!   `Vector3::ZERO`, so the node's placement was never applied — while
//!   colliders and attractors in the same simulation used global transforms.
//!   The 2D emitter has carried `emitter_transform` since pat-0q2kx.51.
//! * **Nothing drew them.** `MainLoop` publishes `_particle_positions` as a
//!   `PackedVector3Array` on every emitter each frame, and a case-insensitive
//!   `particle` grep over `gdrender3d` and `gdserver3d/src` found only the
//!   Sprite3D/Label3D billboard enums. The data was produced and discarded.
//!
//! 🔑 The "not at the origin" half is asserted on the **render instances**, not
//! on sampled pixels. A pixel probe at a hand-computed screen position smuggles
//! in a projection claim this bead does not touch — and if the projection is
//! wrong the probe fails for an unrelated reason. Instance origins are the same
//! claim, checked exactly, at the layer the change actually alters. Pixels are
//! then asserted as a whole-frame difference against a particle-free control.

use gdcore::math::Vector3;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;

const W: u32 = 96;
const H: u32 = 96;
const STEPS: usize = 30;
const EMITTER_AT: Vector3 = Vector3::new(3.0, 0.0, 0.0);

/// `emitter` is spliced in so the control scene can omit it entirely.
///
/// ⚠️ `gravity` and `initial_velocity_*` are authored as FLAT node properties
/// because that is what `read_emitter_3d` reads. Both are zero so a live
/// particle sits exactly where it spawned: the position assertion then measures
/// the emitter transform and nothing else — with the default gravity the cloud
/// would fall out of the 0.5 tolerance for a reason unrelated to this bead.
fn scene_src(emitter: &str) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="QuadMesh" id="QuadMesh_1"]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 10)
current = true
{emitter}"#
    )
}

const EMITTER: &str = r#"
[node name="Emitter" type="GPUParticles3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 3, 0, 0)
draw_pass_1 = SubResource("QuadMesh_1")
amount = 16
lifetime = 5.0
emitting = true
gravity = Vector3(0, 0, 0)
initial_velocity_min = 0.0
initial_velocity_max = 0.0
"#;

fn boot(src: &str) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mut ml = MainLoop::new(tree);
    for _ in 0..STEPS {
        ml.step(1.0 / 60.0);
    }
    ml
}

/// The rendered frame's pixels.
fn frame_pixels(ml: &MainLoop) -> Vec<gdcore::math::Color> {
    let mut adapter = RenderServer3DAdapter::new(W, H);
    let _ = adapter.render_frame(ml.tree());
    adapter
        .last_frame()
        .expect("the adapter must produce a frame")
        .pixels
        .clone()
}

/// **The acceptance test.**
#[test]
fn particles3d_render_from_tscn() {
    let ml = boot(&scene_src(EMITTER));
    let emitter = ml
        .tree()
        .get_node_by_path("/root/Root/Emitter")
        .expect("the emitter is in the tree");

    // -- (1) the particles live at the EMITTER, not the origin ---------------
    let positions = ml.particle_positions_3d(emitter);
    // CONTROL: the simulation ran at all. Without this, an empty list would
    // satisfy "every position is near (3,0,0)" vacuously.
    assert!(
        !positions.is_empty(),
        "precondition: {STEPS} steps must leave live particles; none means the \
         emitter never ran and every assertion below is vacuous"
    );
    for p in &positions {
        let d = (*p - EMITTER_AT).length();
        assert!(
            d < 0.5,
            "a particle is {d} away from the emitter at {EMITTER_AT:?} (at \
             {p:?}). Before this bead the emitter transform was never applied \
             and every particle sat at the world ORIGIN — a distance of 3.0"
        );
    }

    // -- (2) each live particle produces a render instance -------------------
    let mut adapter = RenderServer3DAdapter::new(W, H);
    let _ = adapter.render_frame(ml.tree());
    let instances = adapter.instances().to_vec();

    let near_emitter = instances
        .iter()
        .filter(|i| (i.transform.origin - EMITTER_AT).length() < 0.5)
        .count();
    assert_eq!(
        near_emitter,
        positions.len(),
        "every live particle must become a render instance at its own \
         position; {} of {} landed near the emitter. Zero means nothing reads \
         `_particle_positions` on the 3D side, which is the second half of \
         this bead",
        near_emitter,
        positions.len()
    );
    // ...and each carries real geometry, so "an instance exists" cannot be
    // satisfied by an empty one.
    assert!(
        instances
            .iter()
            .filter(|i| (i.transform.origin - EMITTER_AT).length() < 0.5)
            .all(|i| i.mesh.is_some()),
        "a particle instance must carry its draw_pass_1 mesh"
    );
    // 🔑 The "nothing at the origin" half, checked where it is exact rather
    // than through a projected pixel.
    let at_origin = instances
        .iter()
        .filter(|i| i.transform.origin.length() < 0.5)
        .count();
    assert_eq!(
        at_origin, 0,
        "no particle instance may sit at the world origin; {at_origin} did, \
         which is where an unapplied emitter transform puts them"
    );

    // -- (3) and they reach the frame ---------------------------------------
    //
    // 🔑 A whole-frame DIFF against a particle-free control, not a brightness
    // threshold. The 3D path draws its own background, so an absolute count is
    // not the emitter's; and a threshold cannot distinguish "drew nothing" from
    // "drew something the threshold does not count" — the first version of this
    // test reported `0 with particles vs 0 without` when the quads were being
    // culled by winding, which a diff states directly as "the frames are
    // identical".
    let control = boot(&scene_src(""));
    let with_particles = frame_pixels(&ml);
    let without = frame_pixels(&control);
    assert_eq!(
        with_particles.len(),
        without.len(),
        "precondition: both frames are the same size"
    );
    let differing = with_particles
        .iter()
        .zip(without.iter())
        .filter(|(a, b)| a.r != b.r || a.g != b.g || a.b != b.b)
        .count();
    assert!(
        differing > 0,
        "the emitter must change the frame: it is pixel-identical to the same \
         scene with no emitter at all, so the synced instances never reached \
         the rasterizer"
    );
}
