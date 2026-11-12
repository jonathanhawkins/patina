//! pat-s3rrv.216 (stage 1) — `GPUParticlesAttractor*3D` and
//! `GPUParticlesCollision*3D` nodes actually steer and stop particles.
//!
//! All eight classes were ClassDB-registered and instantiated with the right
//! properties, but nothing simulated them: `ParticleSimulator3D::step` took no
//! collider or attractor input at all, and `particle3d.rs`'s own module doc
//! admitted particles "pass through every body in the scene".
//!
//! The maths is ported from Godot rather than invented:
//! `servers/rendering/renderer_rd/shaders/particles.glsl:452-491` (attractor
//! force), `:546-592` (box/sphere contact), `:654-664` (merging overlapping
//! contacts), and `scene/resources/particle_process_material.cpp:1021-1035`
//! (the rigid bounce/friction response and hide-on-contact).
//!
//! ⚠️ `amount`/`lifetime` are load-bearing here, exactly as in
//! `particles3d_sim.rs`: the simulator emits one particle every
//! `lifetime / amount` seconds, so a long lifetime with a small amount emits
//! nothing over a short run and every assertion then passes **vacuously** over
//! an empty set. Each test below asserts a non-empty cloud before asserting
//! anything about it.
//!
//! Stage 1 covers attractors plus the analytic Box/Sphere colliders.
//! `VectorField3D`, `HeightField3D` and `SDF3D` need baked textures and are
//! follow-ups.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Godot `ParticleProcessMaterial.CollisionMode`.
const COLLISION_DISABLED: i64 = 0;
const COLLISION_RIGID: i64 = 1;
const COLLISION_HIDE_ON_CONTACT: i64 = 2;

/// A scene with one `GPUParticles3D` plus optional sibling nodes.
///
/// The emitter fires straight up (`spread = 0`) so the no-attractor control has
/// a mean x of exactly zero — any lateral displacement is the attractor's doing
/// and nothing else.
fn scene(
    emitter_props: &[(&str, Variant)],
    siblings: &[(&str, &[(&str, Variant)])],
) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let emitter = tree
        .add_child(root, Node::new("P", "GPUParticles3D"))
        .unwrap();
    {
        let n = tree.get_node_mut(emitter).unwrap();
        n.set_property("emitting", Variant::Bool(true));
        // 32 over 2.0s is one particle every 62.5ms — several alive within a
        // few frames, none expiring during a 90-frame run.
        n.set_property("amount", Variant::Int(32));
        n.set_property("lifetime", Variant::Float(2.0));
        n.set_property("direction", Variant::Vector3(Vector3::new(0.0, 1.0, 0.0)));
        n.set_property("spread", Variant::Float(0.0));
        n.set_property("initial_velocity_min", Variant::Float(60.0));
        n.set_property("initial_velocity_max", Variant::Float(60.0));
        n.set_property("gravity", Variant::Vector3(Vector3::new(0.0, 0.0, 0.0)));
        for (k, v) in emitter_props {
            n.set_property(k, v.clone());
        }
    }
    for (class, props) in siblings {
        let id = tree.add_child(root, Node::new(*class, *class)).unwrap();
        let n = tree.get_node_mut(id).unwrap();
        for (k, v) in *props {
            n.set_property(k, v.clone());
        }
    }
    (MainLoop::new(tree), emitter)
}

/// Steps `frames` at 60Hz and returns the live particle positions.
fn run(main: &mut MainLoop, id: NodeId, frames: usize) -> Vec<Vector3> {
    for _ in 0..frames {
        main.step(1.0 / 60.0);
    }
    main.particle_positions_3d(id)
}

fn mean_x(ps: &[Vector3]) -> f32 {
    ps.iter().map(|p| p.x).sum::<f32>() / ps.len() as f32
}

fn min_y(ps: &[Vector3]) -> f32 {
    ps.iter().map(|p| p.y).fold(f32::INFINITY, f32::min)
}

/// A sphere attractor straddling the emitter, offset in +x.
///
/// ⚠️ A radial attractor has two regimes, and only one of them can satisfy the
/// acceptance's "displaced by more than one attractor radius":
///
/// * **Capture** (slow traversal) — the particle is pulled to the attractor's
///   centre and oscillates about it, so its displacement converges on the
///   centre offset. Since a particle must start *inside* the volume to feel
///   anything at all, that offset is by definition **less than one radius**,
///   and the clause is unreachable. Measured at `initial_velocity = 10`: mean
///   x settled at 1.87 against a centre at x = 2.0 — correct physics, failing
///   assertion.
/// * **Deflection** (fast traversal) — the particle crosses the volume before
///   it can be captured, exits with the lateral velocity it gained, and then
///   coasts with zero force. Displacement grows with the remaining run time and
///   is unbounded.
///
/// So the emitter fires fast (60 u/s) and the attractor is strong: the cloud is
/// deflected rather than trapped. This is why the repelling case passed while
/// the attracting one failed at the same strength — a repelled particle always
/// leaves the volume, so it is always in the deflection regime.
fn attractor(strength: f32) -> (&'static str, Vec<(&'static str, Variant)>) {
    (
        "GPUParticlesAttractorSphere3D",
        vec![
            ("radius", Variant::Float(3.0)),
            ("strength", Variant::Float(strength as f64)),
            ("attenuation", Variant::Float(1.0)),
            ("position", Variant::Vector3(Vector3::new(2.0, 1.0, 0.0))),
        ],
    )
}

/// A wide box collider whose **top face** sits at y = -50.
///
/// It is 20 units thick on purpose. A thin slab can be tunnelled: at y = -50 a
/// particle under this gravity moves ~1.5 units per frame, so a contact band
/// narrower than that would be stepped straight over and the collider would
/// silently do nothing.
fn floor() -> (&'static str, Vec<(&'static str, Variant)>) {
    (
        "GPUParticlesCollisionBox3D",
        vec![
            ("size", Variant::Vector3(Vector3::new(200.0, 20.0, 200.0))),
            ("position", Variant::Vector3(Vector3::new(0.0, -60.0, 0.0))),
        ],
    )
}

// ---------------------------------------------------------------------------
// (a) and (b) — attractors bend the cloud, and the sign decides which way.
// ---------------------------------------------------------------------------

#[test]
fn without_an_attractor_the_cloud_stays_straight() {
    let (mut main, id) = scene(&[], &[]);
    let ps = run(&mut main, id, 90);
    assert!(
        !ps.is_empty(),
        "no particles emitted — the run proves nothing"
    );
    assert!(
        mean_x(&ps).abs() < 0.01,
        "a spread-0 emitter with no attractor must fire straight up, got mean x {}",
        mean_x(&ps)
    );
}

#[test]
fn a_positive_attractor_bends_the_cloud_toward_it() {
    let a = attractor(4000.0);
    let (mut main, id) = scene(&[], &[(a.0, a.1.as_slice())]);
    let ps = run(&mut main, id, 90);
    assert!(
        !ps.is_empty(),
        "no particles emitted — the run proves nothing"
    );
    // The acceptance's bar: displaced by more than one attractor radius (3.0)
    // relative to the straight-up control, whose mean x is 0.
    assert!(
        mean_x(&ps) > 3.0,
        "a positive strength must pull the cloud toward the +x attractor by \
         more than its radius, got mean x {}",
        mean_x(&ps)
    );
}

#[test]
fn a_negated_attractor_bends_the_cloud_away() {
    let a = attractor(-4000.0);
    let (mut main, id) = scene(&[], &[(a.0, a.1.as_slice())]);
    let ps = run(&mut main, id, 90);
    assert!(
        !ps.is_empty(),
        "no particles emitted — the run proves nothing"
    );
    assert!(
        mean_x(&ps) < -3.0,
        "negating `strength` must repel — Godot subtracts the force term \
         (particles.glsl:491), so the sign flip reverses the bend. Got mean x {}",
        mean_x(&ps)
    );
}

#[test]
fn an_emitter_with_attractor_interaction_disabled_ignores_the_attractor() {
    let a = attractor(4000.0);
    let (mut main, id) = scene(
        &[("attractor_interaction_enabled", Variant::Bool(false))],
        &[(a.0, a.1.as_slice())],
    );
    let ps = run(&mut main, id, 90);
    assert!(
        !ps.is_empty(),
        "no particles emitted — the run proves nothing"
    );
    assert!(
        mean_x(&ps).abs() < 0.01,
        "`attractor_interaction_enabled = false` must ignore every attractor, \
         got mean x {}",
        mean_x(&ps)
    );
}

#[test]
fn a_cull_masked_attractor_is_ignored() {
    let mut a = attractor(4000.0);
    a.1.push(("cull_mask", Variant::Int(0b10)));
    let (mut main, id) = scene(
        &[("visibility_layer", Variant::Int(0b01))],
        &[(a.0, a.1.as_slice())],
    );
    let ps = run(&mut main, id, 90);
    assert!(
        !ps.is_empty(),
        "no particles emitted — the run proves nothing"
    );
    assert!(
        mean_x(&ps).abs() < 0.01,
        "a cull_mask sharing no bit with the emitter's layer must not pull it, \
         got mean x {}",
        mean_x(&ps)
    );
}

// ---------------------------------------------------------------------------
// (c) — colliders stop particles instead of letting them through.
// ---------------------------------------------------------------------------

/// Gravity strong enough to reach the collider well inside the run.
fn falling(mode: i64) -> Vec<(&'static str, Variant)> {
    vec![
        ("gravity", Variant::Vector3(Vector3::new(0.0, -98.0, 0.0))),
        ("initial_velocity_min", Variant::Float(0.0)),
        ("initial_velocity_max", Variant::Float(0.0)),
        ("collision_mode", Variant::Int(mode)),
        ("collision_base_size", Variant::Float(0.5)),
        ("collision_friction", Variant::Float(1.0)),
        ("collision_bounce", Variant::Float(0.0)),
    ]
}

#[test]
fn without_a_collider_particles_fall_straight_past() {
    let props = falling(COLLISION_RIGID);
    let (mut main, id) = scene(&props, &[]);
    let ps = run(&mut main, id, 90);
    assert!(
        !ps.is_empty(),
        "no particles emitted — the run proves nothing"
    );
    assert!(
        min_y(&ps) < -60.0,
        "with no collider in the scene the cloud must fall well past y = -50, \
         got min y {} — otherwise the collider test below proves nothing",
        min_y(&ps)
    );
}

#[test]
fn a_collision_disabled_emitter_falls_through_the_collider() {
    let f = floor();
    let props = falling(COLLISION_DISABLED);
    let (mut main, id) = scene(&props, &[(f.0, f.1.as_slice())]);
    let ps = run(&mut main, id, 90);
    assert!(
        !ps.is_empty(),
        "no particles emitted — the run proves nothing"
    );
    assert!(
        min_y(&ps) < -60.0,
        "COLLISION_DISABLED must pass straight through the collider, got min y {}",
        min_y(&ps)
    );
}

#[test]
fn a_rigid_emitter_stops_at_the_collider_surface() {
    let f = floor();
    let props = falling(COLLISION_RIGID);
    let (mut main, id) = scene(&props, &[(f.0, f.1.as_slice())]);
    let ps = run(&mut main, id, 90);
    assert!(
        !ps.is_empty(),
        "no particles emitted — the run proves nothing"
    );
    assert!(
        min_y(&ps) > -51.0,
        "COLLISION_RIGID must stop particles at the collider's top face \
         (y = -50), not let them through — got min y {}",
        min_y(&ps)
    );
}

// ---------------------------------------------------------------------------
// (d) — hide-on-contact kills the particle.
// ---------------------------------------------------------------------------

/// Runs a falling cloud into the floor, stopping emission part-way so the live
/// count can reach zero, and returns the count at the end.
///
/// `lifetime` is 10s and the whole run is under 2s, so **nothing here expires
/// of old age** — a drop to zero can only be the collider.
fn live_count_after_contact(mode: i64) -> usize {
    let f = floor();
    let mut props = falling(mode);
    props.push(("lifetime", Variant::Float(10.0)));
    props.push(("amount", Variant::Int(320)));
    let (mut main, id) = scene(&props, &[(f.0, f.1.as_slice())]);

    // Emit for 20 frames, then stop, so no new particles mask the result.
    let early = run(&mut main, id, 20);
    assert!(
        !early.is_empty(),
        "no particles emitted in the first 20 frames — the run proves nothing"
    );
    main.tree_mut()
        .get_node_mut(id)
        .unwrap()
        .set_property("emitting", Variant::Bool(false));

    run(&mut main, id, 100).len()
}

#[test]
fn hide_on_contact_kills_every_particle_that_reaches_the_collider() {
    assert_eq!(
        live_count_after_contact(COLLISION_HIDE_ON_CONTACT),
        0,
        "COLLISION_HIDE_ON_CONTACT must kill particles on contact"
    );
}

#[test]
fn the_same_run_without_collision_keeps_its_particles() {
    // The control that makes the assertion above non-vacuous: identical timing
    // and lifetime, only the collision mode differs.
    assert!(
        live_count_after_contact(COLLISION_DISABLED) > 0,
        "with collision disabled the same cloud must still be alive — \
         otherwise the hide-on-contact test could pass for the wrong reason"
    );
}

// ---------------------------------------------------------------------------
// Unit coverage of the ported maths, independent of the scene wiring.
// ---------------------------------------------------------------------------

#[test]
fn a_particle_outside_the_attractor_volume_feels_nothing() {
    use gdcore::math3d::Transform3D;
    use gdscene::particle3d::{ParticleAttractor3D, ParticleVolume3D};

    let a = ParticleAttractor3D {
        transform: Transform3D::IDENTITY,
        volume: ParticleVolume3D::Sphere { radius: 1.0 },
        strength: 100.0,
        attenuation: 1.0,
        directionality: 0.0,
        cull_mask: 0xFFFF_FFFF,
    };
    assert!(
        a.force_at(Vector3::new(5.0, 0.0, 0.0)).is_none(),
        "particles.glsl:462 skips the attractor once the normalized distance \
         exceeds 1"
    );
    assert!(
        a.force_at(Vector3::new(0.5, 0.0, 0.0)).is_some(),
        "a particle inside the volume must feel the attractor"
    );
}

#[test]
fn the_attractor_force_points_inward_for_a_positive_strength() {
    use gdcore::math3d::Transform3D;
    use gdscene::particle3d::{ParticleAttractor3D, ParticleVolume3D};

    let a = ParticleAttractor3D {
        transform: Transform3D::IDENTITY,
        volume: ParticleVolume3D::Sphere { radius: 2.0 },
        strength: 10.0,
        attenuation: 1.0,
        directionality: 0.0,
        cull_mask: 0xFFFF_FFFF,
    };
    let f = a
        .force_at(Vector3::new(1.0, 0.0, 0.0))
        .expect("inside the volume");
    assert!(
        f.x < 0.0,
        "a particle at +x of a positive attractor must be pulled back toward \
         the centre (-x), got {f:?}"
    );
    let repel = ParticleAttractor3D {
        strength: -10.0,
        ..a
    };
    let g = repel
        .force_at(Vector3::new(1.0, 0.0, 0.0))
        .expect("inside the volume");
    assert!(g.x > 0.0, "a negative strength must repel, got {g:?}");
}

#[test]
fn a_box_collider_reports_an_outward_normal() {
    use gdcore::math3d::Transform3D;
    use gdscene::particle3d::{ParticleCollider3D, ParticleVolume3D};

    let c = ParticleCollider3D {
        transform: Transform3D::IDENTITY,
        volume: ParticleVolume3D::Box {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        },
        cull_mask: 0xFFFF_FFFF,
    };
    assert!(
        c.contact(Vector3::new(0.0, 5.0, 0.0), 0.0).is_none(),
        "a particle clear of the box must not report a contact"
    );
    let hit = c
        .contact(Vector3::new(0.0, 1.0, 0.0), 0.1)
        .expect("touching the top face is a contact");
    assert!(
        hit.normal.y > 0.9,
        "the contact normal on the top face must point +y, got {:?}",
        hit.normal
    );
}
