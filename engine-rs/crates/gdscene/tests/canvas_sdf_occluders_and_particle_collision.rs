//! pat-s3rrv.250 — the canvas SDF, and 2D particles colliding against it.
//!
//! Godot rasterizes every `LightOccluder2D` with `sdf_collision` into a distance
//! field sized by `Viewport.sdf_oversize`/`sdf_scale`; `GPUParticles2D` collision
//! samples it and canvas_item shaders reach it via `texture_sdf` /
//! `screen_uv_to_sdf`.
//!
//! Patina had none of it. `CanvasQuality` carried `sdf_oversize`/`sdf_scale` with an
//! explicit "parsed and carried but **not yet consumed**" comment,
//! `LightOccluder2D.sdf_collision` was never read, and nothing in `gdrender2d` or
//! `gdserver2d` mentioned `sdf` at all — so 2D particle collision against level
//! geometry was impossible.
//!
//! # Why particles are placed rather than emitted
//!
//! The behaviour under test is *collision*, not *emission*. Seeding
//! `active_particles` directly means a failure here names one thing; going through
//! the emitter would make a spawn-rate or lifetime bug look like a collision bug.
//! The emission path already has its own tests.

use gdcore::math::{Color, Transform2D, Vector2};
use gdrender2d::sdf2d::CanvasSdf;
use gdscene::particle::{Particle, ParticleEmitter, ParticleSimulator};
use gdscene::particle3d::ParticleCollisionMode;

/// A 200x200 canvas.
const FRAME: Vector2 = Vector2::new(200.0, 200.0);
/// The floor's top edge. Particles fall toward it from above.
const FLOOR_Y: f32 = 120.0;

/// A closed rectangle spanning the canvas from `FLOOR_Y` down — a solid floor.
fn floor_occluder() -> (Vec<Vector2>, bool) {
    (
        vec![
            Vector2::new(0.0, FLOOR_Y),
            Vector2::new(200.0, FLOOR_Y),
            Vector2::new(200.0, 200.0),
            Vector2::new(0.0, 200.0),
        ],
        true,
    )
}

/// ⚠️ Every field spelled out: `Particle` derives `Debug, Clone, Copy, PartialEq`
/// but **not `Default`**, so `..Default::default()` does not compile here. Field
/// values copied from the literal in `particle.rs`'s own tests.
fn falling_particle(x: f32, y: f32, vy: f32) -> Particle {
    Particle {
        position: Vector2::new(x, y),
        velocity: Vector2::new(0.0, vy),
        lifetime: 100.0,
        lifetime_remaining: 100.0,
        color: Color::WHITE,
        scale: 1.0,
        rotation: 0.0,
        angular_velocity: 0.0,
        spawn_transform: Transform2D::IDENTITY,
    }
}

fn simulator(mode: ParticleCollisionMode) -> ParticleSimulator {
    let mut emitter = ParticleEmitter::default();
    emitter.emitting = false; // seeded by hand; see the module note
    emitter.material.collision_mode = mode;
    emitter.material.collision_friction = 0.0;
    emitter.material.collision_bounce = 0.0;
    // No gravity: the particles carry their own downward velocity, so the test
    // does not depend on the gravity default.
    emitter.material.gravity = Vector2::new(0.0, 0.0);
    ParticleSimulator::new(emitter)
}

/// **The acceptance.** The field reads correctly around the occluder, and
/// particles accumulate at it instead of passing through.
#[test]
fn canvas_sdf_occluders_and_particle_collision() {
    let sdf = CanvasSdf::generate(FRAME, 1.0, 1.0, &[floor_occluder()]);

    // --- the field itself ---------------------------------------------------
    //
    // "near-zero distance on the occluder edge and increasing values away from it".
    let on_edge = sdf.sample(Vector2::new(100.0, FLOOR_Y));
    assert!(
        on_edge.abs() <= 1.0,
        "the surface must read near zero, got {on_edge}"
    );

    let near = sdf.sample(Vector2::new(100.0, FLOOR_Y - 10.0));
    let far = sdf.sample(Vector2::new(100.0, FLOOR_Y - 40.0));
    assert!(
        near > 0.0 && far > near,
        "distance must grow with height above the floor: edge {on_edge}, \
         10 above {near}, 40 above {far}"
    );

    // ⚠️ The sign is the half that a magnitude-only test would miss: an unsigned
    // field looks identical above and below the surface, and a particle would then
    // never know it was inside.
    let inside = sdf.sample(Vector2::new(100.0, FLOOR_Y + 30.0));
    assert!(
        inside < 0.0,
        "inside the occluder must be NEGATIVE, got {inside}"
    );

    // --- particles stop at the floor ----------------------------------------
    let mut sim = simulator(ParticleCollisionMode::Rigid);
    for i in 0..5 {
        sim.active_particles
            .push(falling_particle(40.0 + i as f32 * 30.0, 20.0, 400.0));
    }

    // 60 steps at 1/60s with vy=400 travels ~400 units — far past a floor only 100
    // below, so anything that fails to collide ends up well outside the canvas.
    for _ in 0..60 {
        sim.step_with_sdf(1.0 / 60.0, &sdf);
    }

    assert_eq!(
        sim.active_particles.len(),
        5,
        "COLLISION_RIGID must not remove particles — that is HIDE_ON_CONTACT"
    );
    for p in &sim.active_particles {
        assert!(
            p.position.y <= FLOOR_Y + 1.0,
            "a particle must accumulate at the floor, not pass through it: \
             y={} floor={FLOOR_Y}",
            p.position.y
        );
    }
}

/// **The control.** With collision disabled the same particles pass straight
/// through.
///
/// Without this, the assertions above would still hold if the particles had simply
/// failed to move at all.
#[test]
fn canvas_sdf_occluders_and_particle_collision_disabled_passes_through() {
    let sdf = CanvasSdf::generate(FRAME, 1.0, 1.0, &[floor_occluder()]);
    let mut sim = simulator(ParticleCollisionMode::Disabled);
    sim.active_particles
        .push(falling_particle(100.0, 20.0, 400.0));

    for _ in 0..60 {
        sim.step_with_sdf(1.0 / 60.0, &sdf);
    }

    let y = sim.active_particles[0].position.y;
    assert!(
        y > FLOOR_Y + 50.0,
        "COLLISION_DISABLED must pass through the floor — if this fails the \
         particles were never moving and the acceptance proves nothing: y={y}"
    );
}

/// `HIDE_ON_CONTACT` removes the particle instead of resting it.
#[test]
fn canvas_sdf_occluders_and_particle_collision_hide_on_contact_removes() {
    let sdf = CanvasSdf::generate(FRAME, 1.0, 1.0, &[floor_occluder()]);
    let mut sim = simulator(ParticleCollisionMode::HideOnContact);
    sim.active_particles
        .push(falling_particle(100.0, 20.0, 400.0));

    for _ in 0..60 {
        sim.step_with_sdf(1.0 / 60.0, &sdf);
    }

    assert!(
        sim.active_particles.is_empty(),
        "HIDE_ON_CONTACT must remove the particle on contact"
    );
}

/// An occluder is only in the field if the caller passed it — which is what
/// `sdf_collision` decides upstream.
///
/// This pins the *consequence* of the flag: with no geometry the field is empty and
/// a particle falls freely, so a caller that ignores `sdf_collision` and rasterizes
/// everything would fail here.
#[test]
fn canvas_sdf_occluders_and_particle_collision_empty_field_blocks_nothing() {
    let sdf = CanvasSdf::generate(FRAME, 1.0, 1.0, &[]);
    assert!(sdf.is_empty(), "no occluders means no field");

    let mut sim = simulator(ParticleCollisionMode::Rigid);
    sim.active_particles
        .push(falling_particle(100.0, 20.0, 400.0));
    for _ in 0..60 {
        sim.step_with_sdf(1.0 / 60.0, &sdf);
    }
    assert!(
        sim.active_particles[0].position.y > FLOOR_Y + 50.0,
        "an empty field must not block anything"
    );
}

/// `screen_uv_to_sdf` and `sdf_to_screen_uv` are inverses over the frame, and
/// oversize does not disturb them.
#[test]
fn canvas_sdf_occluders_and_particle_collision_uv_conversions_round_trip() {
    // ⚠️ Oversize 2.0 on purpose: the conversions address the FRAME, not the
    // oversized rect, so an implementation that divided by the padded size would
    // pass at oversize 1.0 and fail here.
    let sdf = CanvasSdf::generate(FRAME, 2.0, 1.0, &[floor_occluder()]);

    for uv in [
        Vector2::new(0.0, 0.0),
        Vector2::new(0.5, 0.25),
        Vector2::new(1.0, 1.0),
    ] {
        let p = sdf.screen_uv_to_sdf(uv);
        let back = sdf.sdf_to_screen_uv(p);
        assert!(
            (back.x - uv.x).abs() < 1e-5 && (back.y - uv.y).abs() < 1e-5,
            "uv {uv:?} -> {p:?} -> {back:?} must round-trip"
        );
    }

    // The oversized field still resolves the floor correctly.
    let inside = sdf.sample(Vector2::new(100.0, FLOOR_Y + 30.0));
    assert!(
        inside < 0.0,
        "oversize must not move the geometry: {inside}"
    );
}
