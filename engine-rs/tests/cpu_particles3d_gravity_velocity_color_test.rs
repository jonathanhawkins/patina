//! pat-ejw4r: CPUParticles3D with gravity, velocity, and color curves.
//!
//! Integration tests covering:
//! 1. ClassDB registration of CPUParticles3D (properties, methods, inheritance)
//! 2. Scene tree integration (create, add, path lookup)
//! 3. ParticleEmitter3D configuration and emission
//! 4. ParticleSimulator3D physics: gravity, damping, radial/tangential accel
//! 5. ColorCurve and ScaleCurve interpolation
//! 6. EmissionShape3D variants (Point, Sphere, Box, SphereSurface, Cone)
//! 7. One-shot and continuous emission modes
//! 8. Deterministic randomness (xorshift seeding)
//! 9. Draw data extraction
//! 10. Property get/set via scene tree nodes

use gdcore::math::{Color, Vector3};
use gdscene::node::Node;
use gdscene::particle3d::*;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

// ===========================================================================
// 1. ClassDB registration
// ===========================================================================

#[test]
fn classdb_registers_cpuparticles3d() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("CPUParticles3D"));
}

#[test]
fn classdb_cpuparticles3d_inherits_node3d() {
    gdobject::class_db::register_3d_classes();
    let info = gdobject::class_db::get_class_info("CPUParticles3D").unwrap();
    assert_eq!(info.parent_class.as_str(), "Node3D");
}

#[test]
fn classdb_cpuparticles3d_has_emitting_property() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("CPUParticles3D", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"emitting"), "Missing 'emitting' property");
}

#[test]
fn classdb_cpuparticles3d_has_core_properties() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("CPUParticles3D", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();

    let expected = [
        "emitting", "amount", "lifetime", "one_shot", "explosiveness",
        "speed_scale", "local_coords", "direction", "spread", "gravity",
        "initial_velocity_min", "initial_velocity_max", "damping",
        "scale_amount_min", "scale_amount_max", "emission_shape",
        "tangential_accel", "radial_accel", "lifetime_randomness",
    ];
    for prop in &expected {
        assert!(names.contains(prop), "Missing property: {prop}");
    }
}

#[test]
fn classdb_cpuparticles3d_has_restart_method() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method("CPUParticles3D", "restart"));
}

#[test]
fn classdb_cpuparticles3d_default_amount_is_8() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("CPUParticles3D", false);
    let amount = props.iter().find(|p| p.name == "amount").unwrap();
    assert_eq!(amount.default_value, Variant::Int(8));
}

#[test]
fn classdb_cpuparticles3d_default_lifetime_is_1() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("CPUParticles3D", false);
    let lt = props.iter().find(|p| p.name == "lifetime").unwrap();
    assert_eq!(lt.default_value, Variant::Float(1.0));
}

// ===========================================================================
// 2. Scene tree integration
// ===========================================================================

#[test]
fn cpuparticles3d_node_creation() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Fire", "CPUParticles3D");
    let id = tree.add_child(root, node).unwrap();
    assert_eq!(tree.get_node(id).unwrap().class_name(), "CPUParticles3D");
    assert_eq!(tree.get_node(id).unwrap().name(), "Fire");
}

#[test]
fn cpuparticles3d_path_lookup() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = Node::new("World", "Node3D");
    let pid = tree.add_child(root, parent).unwrap();
    let particles = Node::new("Sparks", "CPUParticles3D");
    tree.add_child(pid, particles).unwrap();

    let found = tree.get_node_by_path("/root/World/Sparks");
    assert!(found.is_some());
    assert_eq!(tree.get_node(found.unwrap()).unwrap().class_name(), "CPUParticles3D");
}

#[test]
fn cpuparticles3d_set_get_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("P", "CPUParticles3D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id).unwrap().set_property("emitting", Variant::Bool(false));
    assert_eq!(tree.get_node(id).unwrap().get_property("emitting"), Variant::Bool(false));

    tree.get_node_mut(id).unwrap().set_property("amount", Variant::Int(32));
    assert_eq!(tree.get_node(id).unwrap().get_property("amount"), Variant::Int(32));

    tree.get_node_mut(id).unwrap().set_property("lifetime", Variant::Float(2.5));
    assert_eq!(tree.get_node(id).unwrap().get_property("lifetime"), Variant::Float(2.5));
}

#[test]
fn cpuparticles3d_multiple_instances_independent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "CPUParticles3D");
    let aid = tree.add_child(root, a).unwrap();
    let b = Node::new("B", "CPUParticles3D");
    let bid = tree.add_child(root, b).unwrap();

    tree.get_node_mut(aid).unwrap().set_property("amount", Variant::Int(10));
    tree.get_node_mut(bid).unwrap().set_property("amount", Variant::Int(20));

    assert_eq!(tree.get_node(aid).unwrap().get_property("amount"), Variant::Int(10));
    assert_eq!(tree.get_node(bid).unwrap().get_property("amount"), Variant::Int(20));
}

// ===========================================================================
// 3. EmissionShape3D variants (tested via emitter, since sample() is private)
// ===========================================================================

#[test]
fn emission_point_emits_at_origin() {
    let e = ParticleEmitter3D {
        emission_shape: EmissionShape3D::Point,
        material: ParticleMaterial3D {
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    for seed in 1..20u32 {
        let p = e.emit_particle(seed);
        assert_eq!(p.position, Vector3::ZERO, "Point shape should emit at origin");
    }
}

#[test]
fn emission_sphere_particles_within_radius() {
    let e = ParticleEmitter3D {
        emission_shape: EmissionShape3D::Sphere { radius: 5.0 },
        material: ParticleMaterial3D {
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    for seed in 1..100u32 {
        let p = e.emit_particle(seed);
        assert!(p.position.length() <= 5.0 + EPSILON, "Outside sphere: len={}", p.position.length());
    }
}

#[test]
fn emission_box_particles_within_extents() {
    let e = ParticleEmitter3D {
        emission_shape: EmissionShape3D::Box { extents: Vector3::new(2.0, 3.0, 4.0) },
        material: ParticleMaterial3D {
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    for seed in 1..100u32 {
        let p = e.emit_particle(seed);
        assert!(p.position.x.abs() <= 2.0 + EPSILON);
        assert!(p.position.y.abs() <= 3.0 + EPSILON);
        assert!(p.position.z.abs() <= 4.0 + EPSILON);
    }
}

#[test]
fn emission_sphere_surface_on_radius() {
    let e = ParticleEmitter3D {
        emission_shape: EmissionShape3D::SphereSurface { radius: 3.0 },
        material: ParticleMaterial3D {
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    for seed in 1..100u32 {
        let p = e.emit_particle(seed);
        assert!((p.position.length() - 3.0).abs() < 0.05, "Not on surface: len={}", p.position.length());
    }
}

#[test]
fn emission_shape_default_is_point() {
    assert_eq!(EmissionShape3D::default(), EmissionShape3D::Point);
}

// ===========================================================================
// 4. ColorCurve
// ===========================================================================

#[test]
fn color_curve_default_white_throughout() {
    let c = ColorCurve::default();
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        assert_eq!(c.sample(t), Color::WHITE);
    }
}

#[test]
fn color_curve_linear_red_to_blue() {
    let c = ColorCurve::linear(
        Color::new(1.0, 0.0, 0.0, 1.0),
        Color::new(0.0, 0.0, 1.0, 1.0),
    );
    let start = c.sample(0.0);
    assert!(approx(start.r, 1.0) && approx(start.b, 0.0));
    let mid = c.sample(0.5);
    assert!(approx(mid.r, 0.5) && approx(mid.b, 0.5));
    let end = c.sample(1.0);
    assert!(approx(end.r, 0.0) && approx(end.b, 1.0));
}

#[test]
fn color_curve_three_stop_midpoint() {
    let c = ColorCurve::three_stop(
        Color::new(1.0, 0.0, 0.0, 1.0), // red
        Color::new(0.0, 1.0, 0.0, 1.0), // green
        Color::new(0.0, 0.0, 1.0, 1.0), // blue
    );
    let at_half = c.sample(0.5);
    assert!(approx(at_half.g, 1.0), "Midpoint should be green: {at_half:?}");
}

#[test]
fn color_curve_clamps_below_zero() {
    let c = ColorCurve::linear(Color::new(1.0, 0.0, 0.0, 1.0), Color::new(0.0, 0.0, 1.0, 1.0));
    let v = c.sample(-1.0);
    assert!(approx(v.r, 1.0));
}

#[test]
fn color_curve_clamps_above_one() {
    let c = ColorCurve::linear(Color::new(1.0, 0.0, 0.0, 1.0), Color::new(0.0, 0.0, 1.0, 1.0));
    let v = c.sample(2.0);
    assert!(approx(v.b, 1.0));
}

// ===========================================================================
// 5. ScaleCurve
// ===========================================================================

#[test]
fn scale_curve_default_one() {
    let s = ScaleCurve::default();
    assert!(approx(s.sample(0.0), 1.0));
    assert!(approx(s.sample(1.0), 1.0));
}

#[test]
fn scale_curve_linear_ramp_down() {
    let s = ScaleCurve::linear(2.0, 0.0);
    assert!(approx(s.sample(0.0), 2.0));
    assert!(approx(s.sample(0.5), 1.0));
    assert!(approx(s.sample(1.0), 0.0));
}

// ===========================================================================
// 6. ParticleMaterial3D defaults
// ===========================================================================

#[test]
fn material_default_gravity() {
    let m = ParticleMaterial3D::default();
    assert!(approx(m.gravity.y, -9.8));
    assert!(approx(m.gravity.x, 0.0));
    assert!(approx(m.gravity.z, 0.0));
}

#[test]
fn material_default_spread() {
    let m = ParticleMaterial3D::default();
    assert!(approx(m.spread, 45.0));
}

#[test]
fn material_default_no_velocity() {
    let m = ParticleMaterial3D::default();
    assert!(approx(m.initial_velocity_min, 0.0));
    assert!(approx(m.initial_velocity_max, 0.0));
}

// ===========================================================================
// 7. ParticleEmitter3D and emission
// ===========================================================================

#[test]
fn emitter_defaults_match_godot() {
    let e = ParticleEmitter3D::default();
    assert_eq!(e.amount, 8);
    assert!(approx(e.lifetime, 1.0));
    assert!(!e.one_shot);
    assert!(approx(e.explosiveness, 0.0));
    assert!(approx(e.speed_scale, 1.0));
    assert!(e.emitting);
    assert!(!e.local_coords);
}

#[test]
fn emit_particle_deterministic_same_seed() {
    let e = ParticleEmitter3D {
        material: ParticleMaterial3D {
            initial_velocity_min: 5.0,
            initial_velocity_max: 10.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let p1 = e.emit_particle(42);
    let p2 = e.emit_particle(42);
    assert_eq!(p1.position, p2.position);
    assert_eq!(p1.velocity, p2.velocity);
    assert_eq!(p1.lifetime, p2.lifetime);
    assert_eq!(p1.color, p2.color);
}

#[test]
fn emit_particle_different_seeds_diverge() {
    let e = ParticleEmitter3D {
        material: ParticleMaterial3D {
            initial_velocity_min: 5.0,
            initial_velocity_max: 10.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let p1 = e.emit_particle(42);
    let p2 = e.emit_particle(99);
    assert!(p1.velocity != p2.velocity || p1.position != p2.position);
}

#[test]
fn emitter_sphere_shape_particles_within_bounds() {
    let e = ParticleEmitter3D {
        emission_shape: EmissionShape3D::Sphere { radius: 3.0 },
        material: ParticleMaterial3D {
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    for seed in 1..50u32 {
        let p = e.emit_particle(seed);
        assert!(p.position.length() <= 3.0 + EPSILON);
    }
}

// ===========================================================================
// 8. ParticleSimulator3D — gravity
// ===========================================================================

#[test]
fn simulator_gravity_moves_particles_down() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 10.0,
        explosiveness: 1.0,
        material: ParticleMaterial3D {
            gravity: Vector3::new(0.0, -10.0, 0.0),
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(10.0); // emit
    let start_y = sim.active_particles[0].position.y;
    sim.step(1.0); // 1s of gravity
    let end_y = sim.active_particles[0].position.y;
    assert!(end_y < start_y, "Gravity should move particle down: {start_y} -> {end_y}");
}

#[test]
fn simulator_zero_gravity_no_acceleration() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 10.0,
        explosiveness: 1.0,
        material: ParticleMaterial3D {
            gravity: Vector3::ZERO,
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(10.0); // emit
    let v0 = sim.active_particles[0].velocity;
    sim.step(1.0);
    let v1 = sim.active_particles[0].velocity;
    assert!(approx(v0.length(), v1.length()), "No gravity → no velocity change");
}

#[test]
fn simulator_custom_gravity_direction() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 10.0,
        explosiveness: 1.0,
        material: ParticleMaterial3D {
            gravity: Vector3::new(5.0, 0.0, 0.0), // rightward gravity
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(10.0); // emit
    sim.step(1.0);
    assert!(sim.active_particles[0].velocity.x > 3.0, "Custom gravity not applied");
}

// ===========================================================================
// 9. ParticleSimulator3D — velocity and damping
// ===========================================================================

#[test]
fn simulator_initial_velocity_nonzero() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 10.0,
        explosiveness: 1.0,
        material: ParticleMaterial3D {
            gravity: Vector3::ZERO,
            initial_velocity_min: 10.0,
            initial_velocity_max: 10.0,
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(10.0); // emit
    assert!(sim.active_particles[0].velocity.length() > 5.0, "Should have initial velocity");
}

#[test]
fn simulator_damping_decays_speed() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 10.0,
        explosiveness: 1.0,
        material: ParticleMaterial3D {
            gravity: Vector3::ZERO,
            damping: 0.8,
            initial_velocity_min: 10.0,
            initial_velocity_max: 10.0,
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(10.0);
    let speed0 = sim.active_particles[0].velocity.length();
    sim.step(1.0);
    let speed1 = sim.active_particles[0].velocity.length();
    assert!(speed1 < speed0 * 0.5, "Damping should significantly reduce speed: {speed0} -> {speed1}");
}

#[test]
fn simulator_no_damping_preserves_speed() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 10.0,
        explosiveness: 1.0,
        material: ParticleMaterial3D {
            gravity: Vector3::ZERO,
            damping: 0.0,
            initial_velocity_min: 10.0,
            initial_velocity_max: 10.0,
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(10.0);
    let speed0 = sim.active_particles[0].velocity.length();
    sim.step(0.5);
    let speed1 = sim.active_particles[0].velocity.length();
    assert!(approx(speed0, speed1), "No damping should preserve speed: {speed0} vs {speed1}");
}

// ===========================================================================
// 10. ParticleSimulator3D — color and scale curves
// ===========================================================================

#[test]
fn simulator_color_curve_transitions() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 1.0,
        explosiveness: 1.0,
        material: ParticleMaterial3D {
            gravity: Vector3::ZERO,
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            color_curve: ColorCurve::linear(
                Color::new(1.0, 0.0, 0.0, 1.0),
                Color::new(0.0, 1.0, 0.0, 1.0),
            ),
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(1.0); // emit
    assert!(sim.active_particles[0].color.r > 0.8, "Start should be red");
    sim.step(0.5);
    let c = sim.active_particles[0].color;
    assert!(c.r > 0.2 && c.g > 0.2, "Mid should be mixed: {c:?}");
}

#[test]
fn simulator_scale_curve_shrinks() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 1.0,
        explosiveness: 1.0,
        material: ParticleMaterial3D {
            gravity: Vector3::ZERO,
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            scale_curve: ScaleCurve::linear(1.0, 0.0),
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(1.0); // emit
    assert!(approx(sim.active_particles[0].scale, 1.0));
    sim.step(0.5);
    assert!(sim.active_particles[0].scale < 0.7, "Scale should decrease over lifetime");
}

// ===========================================================================
// 11. One-shot vs continuous
// ===========================================================================

#[test]
fn one_shot_stops_after_burst() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 5,
        lifetime: 0.1,
        one_shot: true,
        explosiveness: 1.0,
        ..Default::default()
    });
    sim.step(0.1);
    assert!(!sim.emitter.emitting, "One-shot should stop emitting after burst");
    let count_after_burst = sim.total_emitted;
    sim.step(1.0);
    assert_eq!(sim.total_emitted, count_after_burst, "No new particles after one-shot");
}

#[test]
fn continuous_keeps_emitting() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 4,
        lifetime: 2.0,
        one_shot: false,
        ..Default::default()
    });
    sim.step(1.0);
    let first = sim.total_emitted;
    sim.step(1.0);
    assert!(sim.total_emitted > first, "Continuous emitter should keep emitting");
}

#[test]
fn one_shot_is_complete_after_all_dead() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 3,
        lifetime: 0.1,
        one_shot: true,
        explosiveness: 1.0,
        ..Default::default()
    });
    sim.step(0.1);
    assert!(!sim.is_complete(), "Still has live particles");
    sim.step(0.2);
    assert!(sim.is_complete(), "All particles dead → complete");
}

// ===========================================================================
// 12. Restart
// ===========================================================================

#[test]
fn restart_clears_and_reactivates() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 3,
        lifetime: 0.1,
        one_shot: true,
        explosiveness: 1.0,
        ..Default::default()
    });
    sim.step(0.1);
    sim.step(0.2);
    assert!(sim.is_complete());

    sim.restart();
    assert_eq!(sim.particle_count(), 0);
    assert!(sim.emitter.emitting);
    assert_eq!(sim.total_emitted, 0);
    assert!(approx(sim.time_accumulator, 0.0));
}

// ===========================================================================
// 13. Draw data extraction
// ===========================================================================

#[test]
fn draw_data_matches_particle_count() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 7,
        lifetime: 1.0,
        explosiveness: 1.0,
        ..Default::default()
    });
    sim.step(1.0);
    let data = sim.get_draw_data();
    assert_eq!(data.len(), sim.particle_count());
    assert_eq!(data.len(), 7);
}

#[test]
fn draw_data_has_valid_values() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 5,
        lifetime: 1.0,
        explosiveness: 1.0,
        ..Default::default()
    });
    sim.step(1.0);
    for (pos, color, scale) in sim.get_draw_data() {
        assert!(pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite());
        assert!(color.a >= 0.0 && color.a <= 1.0);
        assert!(scale.is_finite() && scale >= 0.0);
    }
}

// ===========================================================================
// 14. Speed scale
// ===========================================================================

#[test]
fn speed_scale_affects_simulation_rate() {
    let make_sim = |speed: f32| {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 1,
            lifetime: 10.0,
            explosiveness: 1.0,
            speed_scale: speed,
            material: ParticleMaterial3D {
                gravity: Vector3::new(0.0, -10.0, 0.0),
                initial_velocity_min: 0.0,
                initial_velocity_max: 0.0,
                ..Default::default()
            },
            ..Default::default()
        });
        sim.step(10.0); // emit
        sim.step(1.0);
        sim.active_particles[0].velocity.y
    };

    let vy_1x = make_sim(1.0);
    let vy_2x = make_sim(2.0);
    // 2x speed should result in roughly 2x velocity change
    assert!(vy_2x < vy_1x * 1.5, "2x speed should produce more gravity effect: 1x={vy_1x}, 2x={vy_2x}");
}

// ===========================================================================
// 15. Radial and tangential acceleration
// ===========================================================================

#[test]
fn radial_accel_pushes_outward() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 1,
        lifetime: 10.0,
        explosiveness: 1.0,
        emission_shape: EmissionShape3D::Sphere { radius: 5.0 },
        material: ParticleMaterial3D {
            gravity: Vector3::ZERO,
            radial_accel: 10.0,
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            ..Default::default()
        },
        ..Default::default()
    });
    sim.step(10.0); // emit
    let start_dist = sim.active_particles[0].position.length();
    sim.step(1.0);
    let end_dist = sim.active_particles[0].position.length();
    // Positive radial accel should push outward (if particle not at origin)
    if start_dist > 0.1 {
        assert!(end_dist > start_dist, "Radial accel should push outward: {start_dist} -> {end_dist}");
    }
}

// ===========================================================================
// 16. Particle age and death
// ===========================================================================

#[test]
fn particle_age_ratio_progresses() {
    let p = Particle3D {
        position: Vector3::ZERO,
        velocity: Vector3::ZERO,
        lifetime: 4.0,
        lifetime_remaining: 3.0,
        color: Color::WHITE,
        base_scale: 1.0,
        scale: 1.0,
    };
    assert!(approx(p.age_ratio(), 0.25));
}

#[test]
fn particle_dead_at_zero_remaining() {
    let p = Particle3D {
        position: Vector3::ZERO,
        velocity: Vector3::ZERO,
        lifetime: 1.0,
        lifetime_remaining: 0.0,
        color: Color::WHITE,
        base_scale: 1.0,
        scale: 1.0,
    };
    assert!(p.is_dead());
}

#[test]
fn particle_alive_with_remaining() {
    let p = Particle3D {
        position: Vector3::ZERO,
        velocity: Vector3::ZERO,
        lifetime: 1.0,
        lifetime_remaining: 0.5,
        color: Color::WHITE,
        base_scale: 1.0,
        scale: 1.0,
    };
    assert!(!p.is_dead());
}

// ===========================================================================
// 17. Lifetime randomness
// ===========================================================================

#[test]
fn lifetime_randomness_varies_particle_lifetime() {
    let e = ParticleEmitter3D {
        lifetime: 2.0,
        material: ParticleMaterial3D {
            lifetime_randomness: 0.5,
            ..Default::default()
        },
        ..Default::default()
    };
    let lifetimes: Vec<f32> = (1..20u32).map(|s| e.emit_particle(s).lifetime).collect();
    let min = lifetimes.iter().copied().fold(f32::MAX, f32::min);
    let max = lifetimes.iter().copied().fold(f32::MIN, f32::max);
    assert!(max - min > 0.1, "Lifetime randomness should vary lifetimes: min={min}, max={max}");
}

#[test]
fn zero_lifetime_randomness_uniform() {
    let e = ParticleEmitter3D {
        lifetime: 2.0,
        material: ParticleMaterial3D {
            lifetime_randomness: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let lifetimes: Vec<f32> = (1..20u32).map(|s| e.emit_particle(s).lifetime).collect();
    for lt in &lifetimes {
        assert!(approx(*lt, 2.0), "Zero randomness should give uniform lifetime: {lt}");
    }
}

// ===========================================================================
// 18. Explosiveness
// ===========================================================================

#[test]
fn full_explosiveness_emits_all_at_once() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 10,
        lifetime: 1.0,
        explosiveness: 1.0,
        ..Default::default()
    });
    sim.step(1.0); // trigger burst
    assert_eq!(sim.particle_count(), 10, "Explosiveness 1.0 should emit all at once");
}

#[test]
fn low_explosiveness_emits_gradually() {
    let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
        amount: 100,
        lifetime: 10.0,
        explosiveness: 0.0,
        ..Default::default()
    });
    sim.step(0.1); // small step
    assert!(sim.particle_count() < 100, "Low explosiveness should emit gradually");
    assert!(sim.particle_count() > 0, "Should still emit some particles");
}
