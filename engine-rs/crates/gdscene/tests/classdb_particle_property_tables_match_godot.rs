//! pat-s3rrv.111: the particle nodes and ParticleProcessMaterial carry their
//! real property tables, and a one-shot 3D emitter actually says `finished`.
//!
//! Two defects, one bead:
//!
//! 1. THE TABLES WERE PARTIAL. CPUParticles3D registered 47 of its 77 members,
//!    GPUParticles3D 22 of 30, ParticleProcessMaterial 59 of 96 — every curve,
//!    ramp, texture and emission-shape parameter was missing, so a `.tscn` that
//!    authored `emission_sphere_radius` had nowhere to put it.
//!
//! 2. `finished` WAS DECLARED BUT NEVER EMITTED IN 3D. `ParticleSimulator3D`
//!    has carried `is_complete()` since pat-0q2kx.154 and nothing ever called
//!    it: `MainLoop`'s 3D pass stepped each simulator and moved on, while the
//!    2D pass right above it drained a completion edge and emitted the signal.
//!    So `has_signal("finished")` was true on CPUParticles3D and the signal
//!    could never fire — the worst combination, because the class advertises a
//!    contract the engine does not keep.
//!
//! 🔑 The signal assertion counts EXACTLY ONE emission, not "at least one".
//! `is_complete()` is a LEVEL predicate — true on every frame after the last
//! particle dies — so the natural-looking fix (emit whenever it returns true)
//! fires `finished` every frame for the rest of the scene's life. Only an
//! exact count separates the edge from the level.
//!
//! The expected names are machine-extracted from
//! `upstream/godot/doc/classes/<Class>.xml`; 299 names across
//! 5 classes is far past the point where hand-transcription stays
//! honest.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use gdcore::id::ObjectId;
use gdobject::class_db::{class_has_signal, get_property_list, register_all_classes};
use gdobject::signal::Connection;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// `(class, its own documented members)` — straight from the Godot doc XML.
const PARTICLE_CLASS_MEMBERS: &[(&str, &[&str])] = &[
    (
        "CPUParticles2D",
        &[
            "amount",
            "angle_curve",
            "angle_max",
            "angle_min",
            "angular_velocity_curve",
            "angular_velocity_max",
            "angular_velocity_min",
            "anim_offset_curve",
            "anim_offset_max",
            "anim_offset_min",
            "anim_speed_curve",
            "anim_speed_max",
            "anim_speed_min",
            "color",
            "color_initial_ramp",
            "color_ramp",
            "damping_curve",
            "damping_max",
            "damping_min",
            "direction",
            "draw_order",
            "emission_colors",
            "emission_normals",
            "emission_points",
            "emission_rect_extents",
            "emission_ring_inner_radius",
            "emission_ring_radius",
            "emission_shape",
            "emission_sphere_radius",
            "emitting",
            "explosiveness",
            "fixed_fps",
            "fract_delta",
            "gravity",
            "hue_variation_curve",
            "hue_variation_max",
            "hue_variation_min",
            "initial_velocity_max",
            "initial_velocity_min",
            "lifetime",
            "lifetime_randomness",
            "linear_accel_curve",
            "linear_accel_max",
            "linear_accel_min",
            "local_coords",
            "one_shot",
            "orbit_velocity_curve",
            "orbit_velocity_max",
            "orbit_velocity_min",
            "particle_flag_align_y",
            "physics_interpolation_mode",
            "preprocess",
            "radial_accel_curve",
            "radial_accel_max",
            "radial_accel_min",
            "randomness",
            "scale_amount_curve",
            "scale_amount_max",
            "scale_amount_min",
            "scale_curve_x",
            "scale_curve_y",
            "seed",
            "speed_scale",
            "split_scale",
            "spread",
            "tangential_accel_curve",
            "tangential_accel_max",
            "tangential_accel_min",
            "texture",
            "use_fixed_seed",
        ],
    ),
    (
        "CPUParticles3D",
        &[
            "amount",
            "angle_curve",
            "angle_max",
            "angle_min",
            "angular_velocity_curve",
            "angular_velocity_max",
            "angular_velocity_min",
            "anim_offset_curve",
            "anim_offset_max",
            "anim_offset_min",
            "anim_speed_curve",
            "anim_speed_max",
            "anim_speed_min",
            "color",
            "color_initial_ramp",
            "color_ramp",
            "damping_curve",
            "damping_max",
            "damping_min",
            "direction",
            "draw_order",
            "emission_box_extents",
            "emission_colors",
            "emission_normals",
            "emission_points",
            "emission_ring_axis",
            "emission_ring_cone_angle",
            "emission_ring_height",
            "emission_ring_inner_radius",
            "emission_ring_radius",
            "emission_shape",
            "emission_sphere_radius",
            "emitting",
            "explosiveness",
            "fixed_fps",
            "flatness",
            "fract_delta",
            "gravity",
            "hue_variation_curve",
            "hue_variation_max",
            "hue_variation_min",
            "initial_velocity_max",
            "initial_velocity_min",
            "lifetime",
            "lifetime_randomness",
            "linear_accel_curve",
            "linear_accel_max",
            "linear_accel_min",
            "local_coords",
            "mesh",
            "one_shot",
            "orbit_velocity_curve",
            "orbit_velocity_max",
            "orbit_velocity_min",
            "particle_flag_align_y",
            "particle_flag_disable_z",
            "particle_flag_rotate_y",
            "preprocess",
            "radial_accel_curve",
            "radial_accel_max",
            "radial_accel_min",
            "randomness",
            "scale_amount_curve",
            "scale_amount_max",
            "scale_amount_min",
            "scale_curve_x",
            "scale_curve_y",
            "scale_curve_z",
            "seed",
            "speed_scale",
            "split_scale",
            "spread",
            "tangential_accel_curve",
            "tangential_accel_max",
            "tangential_accel_min",
            "use_fixed_seed",
            "visibility_aabb",
        ],
    ),
    (
        "GPUParticles2D",
        &[
            "amount",
            "amount_ratio",
            "collision_base_size",
            "draw_order",
            "emitting",
            "explosiveness",
            "fixed_fps",
            "fract_delta",
            "interp_to_end",
            "interpolate",
            "lifetime",
            "local_coords",
            "one_shot",
            "preprocess",
            "process_material",
            "randomness",
            "seed",
            "speed_scale",
            "sub_emitter",
            "texture",
            "trail_enabled",
            "trail_lifetime",
            "trail_section_subdivisions",
            "trail_sections",
            "use_fixed_seed",
            "visibility_rect",
        ],
    ),
    (
        "GPUParticles3D",
        &[
            "amount",
            "amount_ratio",
            "collision_base_size",
            "draw_order",
            "draw_pass_1",
            "draw_pass_2",
            "draw_pass_3",
            "draw_pass_4",
            "draw_passes",
            "draw_skin",
            "emitting",
            "explosiveness",
            "fixed_fps",
            "fract_delta",
            "interp_to_end",
            "interpolate",
            "lifetime",
            "local_coords",
            "one_shot",
            "preprocess",
            "process_material",
            "randomness",
            "seed",
            "speed_scale",
            "sub_emitter",
            "trail_enabled",
            "trail_lifetime",
            "transform_align",
            "use_fixed_seed",
            "visibility_aabb",
        ],
    ),
    (
        "ParticleProcessMaterial",
        &[
            "alpha_curve",
            "angle_curve",
            "angle_max",
            "angle_min",
            "angular_velocity_curve",
            "angular_velocity_max",
            "angular_velocity_min",
            "anim_offset_curve",
            "anim_offset_max",
            "anim_offset_min",
            "anim_speed_curve",
            "anim_speed_max",
            "anim_speed_min",
            "attractor_interaction_enabled",
            "collision_bounce",
            "collision_friction",
            "collision_mode",
            "collision_use_scale",
            "color",
            "color_initial_ramp",
            "color_ramp",
            "damping_curve",
            "damping_max",
            "damping_min",
            "direction",
            "directional_velocity_curve",
            "directional_velocity_max",
            "directional_velocity_min",
            "emission_box_extents",
            "emission_color_texture",
            "emission_curve",
            "emission_normal_texture",
            "emission_point_count",
            "emission_point_texture",
            "emission_ring_axis",
            "emission_ring_cone_angle",
            "emission_ring_height",
            "emission_ring_inner_radius",
            "emission_ring_radius",
            "emission_shape",
            "emission_shape_offset",
            "emission_shape_scale",
            "emission_sphere_radius",
            "flatness",
            "gravity",
            "hue_variation_curve",
            "hue_variation_max",
            "hue_variation_min",
            "inherit_velocity_ratio",
            "initial_velocity_max",
            "initial_velocity_min",
            "lifetime_randomness",
            "linear_accel_curve",
            "linear_accel_max",
            "linear_accel_min",
            "orbit_velocity_curve",
            "orbit_velocity_max",
            "orbit_velocity_min",
            "particle_flag_align_y",
            "particle_flag_damping_as_friction",
            "particle_flag_disable_z",
            "particle_flag_rotate_y",
            "radial_accel_curve",
            "radial_accel_max",
            "radial_accel_min",
            "radial_velocity_curve",
            "radial_velocity_max",
            "radial_velocity_min",
            "scale_curve",
            "scale_max",
            "scale_min",
            "scale_over_velocity_curve",
            "scale_over_velocity_max",
            "scale_over_velocity_min",
            "spread",
            "sub_emitter_amount_at_collision",
            "sub_emitter_amount_at_end",
            "sub_emitter_amount_at_start",
            "sub_emitter_frequency",
            "sub_emitter_keep_velocity",
            "sub_emitter_mode",
            "tangential_accel_curve",
            "tangential_accel_max",
            "tangential_accel_min",
            "turbulence_enabled",
            "turbulence_influence_max",
            "turbulence_influence_min",
            "turbulence_influence_over_life",
            "turbulence_initial_displacement_max",
            "turbulence_initial_displacement_min",
            "turbulence_noise_scale",
            "turbulence_noise_speed",
            "turbulence_noise_speed_random",
            "turbulence_noise_strength",
            "velocity_limit_curve",
            "velocity_pivot",
        ],
    ),
];

/// The four particle NODE classes (ParticleProcessMaterial is a Resource and
/// has no `finished`).
const PARTICLE_NODES: &[&str] = &[
    "CPUParticles2D",
    "CPUParticles3D",
    "GPUParticles2D",
    "GPUParticles3D",
];

fn prop_default(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("`{class}` registers no `{name}` property"))
        .default_value
}

/// **The acceptance.** Every documented member of all five classes is
/// registered.
#[test]
fn classdb_particle_property_tables_match_godot() {
    register_all_classes();

    let mut missing: Vec<String> = Vec::new();
    for (class, expected) in PARTICLE_CLASS_MEMBERS {
        let got: Vec<String> = get_property_list(class)
            .into_iter()
            .map(|p| p.name)
            .collect();
        // CONTROL: an unregistered class returns an empty list, which would
        // otherwise read as "every property missing" rather than "no class".
        assert!(
            !got.is_empty(),
            "`{class}` has no properties at all — the class is not registered"
        );
        for name in *expected {
            if !got.iter().any(|g| g == name) {
                missing.push(format!("{class}.{name}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "{} documented member(s) are not registered: {missing:?}",
        missing.len()
    );
}

/// The defaults that the doc XML does NOT carry.
///
/// 🔑 These are the ones worth pinning. Godot documents no `default=` for the
/// emission-shape parameters, so they were transcribed from the C++ headers
/// (`scene/3d/cpu_particles_3d.h`, `scene/resources/particle_process_material.h`).
/// A property registered with the type's zero value instead would leave a
/// sphere emitter with radius 0 — every particle spawning at a point, which
/// looks like a broken emitter rather than a missing default.
#[test]
fn particle_defaults_come_from_the_engine_not_the_type_zero() {
    register_all_classes();

    // `real_t emission_sphere_radius = 1.0;` — NOT 0.0.
    assert_eq!(
        prop_default("CPUParticles3D", "emission_sphere_radius"),
        Variant::Float(1.0)
    );

    // `Vector3 emission_box_extents = Vector3(1, 1, 1);` — NOT ZERO.
    match prop_default("CPUParticles3D", "emission_box_extents") {
        Variant::Vector3(v) => assert!(
            v.x == 1.0 && v.y == 1.0 && v.z == 1.0,
            "emission_box_extents must default to (1,1,1), got {v:?}"
        ),
        other => panic!("emission_box_extents must be a Vector3, got {other:?}"),
    }

    // `int emission_point_count = 1;` — NOT 0.
    assert_eq!(
        prop_default("ParticleProcessMaterial", "emission_point_count"),
        Variant::Int(1)
    );

    // `set_param_min/max(PARAM_DIRECTIONAL_VELOCITY, 1.0)` in the constructor.
    assert_eq!(
        prop_default("ParticleProcessMaterial", "directional_velocity_min"),
        Variant::Float(1.0)
    );
    assert_eq!(
        prop_default("ParticleProcessMaterial", "directional_velocity_max"),
        Variant::Float(1.0)
    );

    // Documented: `default="AABB(-4, -4, -4, 8, 8, 8)"`. A zero AABB would cull
    // the emitter's own particles.
    match prop_default("GPUParticles3D", "visibility_aabb") {
        Variant::Aabb(a) => assert!(
            a.position.x == -4.0 && a.size.x == 8.0,
            "GPUParticles3D.visibility_aabb must default to AABB(-4,-4,-4, 8,8,8), got {a:?}"
        ),
        other => panic!("visibility_aabb must be an AABB, got {other:?}"),
    }
}

/// `has_signal("finished")` is true on all four particle node types.
#[test]
fn every_particle_node_declares_finished() {
    register_all_classes();

    for class in PARTICLE_NODES {
        assert!(
            class_has_signal(class, "finished"),
            "`{class}` must declare Godot's `finished` signal"
        );
    }
}

/// **The behavioural acceptance.** A one_shot CPUParticles3D emits `finished`
/// to a connected handler once its burst has drained.
#[test]
fn a_one_shot_cpu_particles_3d_emits_finished_exactly_once() {
    register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Burst", "CPUParticles3D");
    node.set_property("emitting", Variant::Bool(true));
    node.set_property("one_shot", Variant::Bool(true));
    node.set_property("amount", Variant::Int(8));
    node.set_property("lifetime", Variant::Float(0.2));
    let pid = tree.add_child(root, node).unwrap();

    let hits = Arc::new(AtomicU32::new(0));
    let hits_cb = Arc::clone(&hits);
    tree.connect_signal(
        pid,
        "finished",
        Connection::with_callback(ObjectId::from_raw(1), "_on_finished", move |_args| {
            hits_cb.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    // 3 s — well past the burst (8 particles over 0.2 s) plus their lifetime,
    // and long enough that a LEVEL-triggered emission would have fired well
    // over a hundred times by now.
    for _ in 0..180 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "a one_shot CPUParticles3D must emit `finished` exactly once. 0 means \
         the 3D pass still never drains the completion edge; a large number \
         means it is emitting off the level predicate every frame"
    );
}

/// An emitter that never emits must not announce completion.
///
/// CONTROL for the test above: "one_shot, not emitting, no live particles" is
/// also true of a freshly built idle emitter, so without the `total_emitted > 0`
/// guard every such node would fire `finished` on its very first frame.
#[test]
fn an_idle_one_shot_emitter_never_says_finished() {
    register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Idle", "CPUParticles3D");
    node.set_property("emitting", Variant::Bool(false));
    node.set_property("one_shot", Variant::Bool(true));
    node.set_property("amount", Variant::Int(8));
    node.set_property("lifetime", Variant::Float(0.2));
    let pid = tree.add_child(root, node).unwrap();

    let hits = Arc::new(AtomicU32::new(0));
    let hits_cb = Arc::clone(&hits);
    tree.connect_signal(
        pid,
        "finished",
        Connection::with_callback(ObjectId::from_raw(1), "_on_finished", move |_args| {
            hits_cb.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    let mut ml = MainLoop::new(tree);
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "an emitter that never emitted a particle has not `finished`"
    );
}
