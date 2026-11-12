//! pat-0q2kx.261 — a `shader_type particles` program drives the CPU simulator.
//!
//! Godot ground truth: the particles-stage built-ins in
//! `servers/rendering/shader_language.cpp`. A particles shader replaces
//! `ParticleProcessMaterial`; `process()` integrates VELOCITY / COLOR /
//! TRANSFORM once per simulation step.
//!
//! Patina's front-end recognised `shader_type particles` and produced nothing
//! (`gdserver2d/src/shader.rs` maps every non-`canvas_item` type to
//! `wgsl_program: None`) and the simulator had no hook, so such a shader was
//! inert. These drive the real `ParticleSimulator`, so "the parser accepts it
//! but nothing moves" fails here.

use gdcore::math::Vector2;
use gdscene::particle::{ParticleEmitter, ParticleSimulator};

/// The bead's own shader.
const ACCEL_AND_RED: &str = "\
shader_type particles;

void process() {
    VELOCITY.y += 100.0 * DELTA;
    COLOR = vec4(1.0, 0.0, 0.0, 1.0);
}
";

/// A simulator pre-loaded with live particles and emission switched **off**.
///
/// ⚠️ Particles are seeded directly rather than waiting for the emitter,
/// because `step_exact` integrates existing particles **before** it emits new
/// ones. Left to emit on its own, a particle spawned during the final step
/// would have had zero shader runs, and `for p in &active_particles` would
/// assert on it — an intermittent failure that looks like the shader is flaky
/// rather than like the test is racing emission. (A burst also needs a whole
/// `lifetime` of accumulated time to fire, so it is not a shortcut either.)
///
/// `gravity` is zeroed so that any acceleration observed is provably the
/// shader's doing; otherwise an entirely inert shader would still produce
/// falling particles and the acceptance would pass for free.
fn simulator_with(gravity: Vector2) -> ParticleSimulator {
    let mut emitter = ParticleEmitter::default();
    emitter.amount = 4;
    emitter.lifetime = 1000.0;
    emitter.material.gravity = gravity;
    // Nothing new may appear mid-test.
    emitter.emitting = false;

    let mut sim = ParticleSimulator::new(emitter);
    for seed in 1..=4u32 {
        let particle = sim.emitter.emit_particle(seed);
        sim.active_particles.push(particle);
    }
    sim
}

fn simulator() -> ParticleSimulator {
    simulator_with(Vector2::new(0.0, 0.0))
}

fn step_n(sim: &mut ParticleSimulator, n: usize, dt: f32) {
    for _ in 0..n {
        sim.step(dt);
    }
}

/// **The acceptance.** With the shader: downward acceleration and red
/// particles. Without it: neither.
#[test]
fn particles_shader_drives_simulation() {
    // -- with the shader ----------------------------------------------------
    let mut shaded = simulator();
    let unsupported = shaded.set_particles_shader(ACCEL_AND_RED);
    assert!(
        unsupported.is_empty(),
        "the acceptance shader must parse cleanly; unsupported: {unsupported:?}"
    );
    assert!(shaded.has_particles_shader());
    step_n(&mut shaded, 10, 1.0 / 60.0);

    assert!(
        !shaded.active_particles.is_empty(),
        "the emitter must have produced particles for this test to mean anything"
    );
    for p in &shaded.active_particles {
        assert!(
            p.velocity.y > 0.0,
            "VELOCITY.y += 100.0 * DELTA must accelerate downward; got {}",
            p.velocity.y
        );
        assert_eq!(
            (p.color.r, p.color.g, p.color.b),
            (1.0, 0.0, 0.0),
            "COLOR = vec4(1,0,0,1) must reach the published particle colour"
        );
    }

    // -- the same scene without it -----------------------------------------
    let mut plain = simulator();
    assert!(!plain.has_particles_shader());
    step_n(&mut plain, 10, 1.0 / 60.0);
    assert!(
        !plain.active_particles.is_empty(),
        "control emitter must also emit"
    );
    for p in &plain.active_particles {
        assert_eq!(
            p.velocity.y, 0.0,
            "with no gravity and no shader nothing may accelerate — otherwise \
             the shaded assertion above proves nothing"
        );
        assert_ne!(
            (p.color.r, p.color.g, p.color.b),
            (1.0, 0.0, 0.0),
            "the default ramp must not happen to be red"
        );
    }
}

/// The acceleration compounds, which is what separates "the shader ran once"
/// from "the shader runs every step". A single-step check cannot tell them
/// apart, and running the hook once at spawn is the likely wrong wiring.
#[test]
fn the_shader_runs_every_step_not_once() {
    let mut sim = simulator();
    sim.set_particles_shader(ACCEL_AND_RED);
    sim.step(1.0 / 60.0);
    let after_one: Vec<f32> = sim.active_particles.iter().map(|p| p.velocity.y).collect();
    assert!(!after_one.is_empty());

    step_n(&mut sim, 9, 1.0 / 60.0);
    for (p, before) in sim.active_particles.iter().zip(&after_one) {
        assert!(
            p.velocity.y > *before + 1.0,
            "velocity must keep growing across steps: {} vs {before}",
            p.velocity.y
        );
    }
}

/// ⚠️ The shader runs AFTER the built-in integration, so it wins.
///
/// Running it first would let gravity overwrite the shader's VELOCITY every
/// step — which presents as "the shader does nothing", the exact failure this
/// bead exists to fix.
#[test]
fn the_shader_overrides_built_in_gravity() {
    // Strong upward gravity, which the shader must be able to override.
    let mut sim = simulator_with(Vector2::new(0.0, -500.0));
    sim.set_particles_shader(
        "shader_type particles;\nvoid process() { VELOCITY = vec2(7.0, 3.0); }\n",
    );
    step_n(&mut sim, 5, 1.0 / 60.0);

    for p in &sim.active_particles {
        assert_eq!(
            (p.velocity.x, p.velocity.y),
            (7.0, 3.0),
            "an absolute VELOCITY write must survive the gravity applied earlier \
             in the same step"
        );
    }
}

/// Clearing the shader restores the built-in motion, so the hook is a real
/// toggle rather than a one-way switch.
#[test]
fn clearing_the_shader_restores_default_motion() {
    let mut sim = simulator();
    sim.set_particles_shader(ACCEL_AND_RED);
    step_n(&mut sim, 5, 1.0 / 60.0);
    let accelerated: Vec<f32> = sim.active_particles.iter().map(|p| p.velocity.y).collect();
    assert!(accelerated.iter().all(|v| *v > 0.0));

    sim.clear_particles_shader();
    assert!(!sim.has_particles_shader());
    step_n(&mut sim, 5, 1.0 / 60.0);
    for (p, before) in sim.active_particles.iter().zip(&accelerated) {
        assert_eq!(
            p.velocity.y, *before,
            "with the shader cleared and no gravity, velocity must stop changing"
        );
    }
}

/// Unsupported constructs are reported to the caller, and the recognised
/// statements in the same shader still run. Silently dropping them would leave
/// a shader that moves particles — so it looks wired — while specific lines do
/// nothing.
#[test]
fn unsupported_constructs_are_reported_but_do_not_disable_the_shader() {
    let mut sim = simulator();
    let unsupported = sim.set_particles_shader(
        "shader_type particles;\nvoid process() {\n    VELOCITY.y += 100.0 * DELTA;\n    emit_subparticle(TRANSFORM, VELOCITY, COLOR, CUSTOM, 0);\n}\n",
    );
    assert_eq!(unsupported.len(), 1, "{unsupported:?}");
    assert!(
        unsupported[0].contains("emit_subparticle"),
        "the report must name what was dropped: {unsupported:?}"
    );

    assert!(
        sim.has_particles_shader(),
        "the rest of the shader still runs"
    );
    step_n(&mut sim, 5, 1.0 / 60.0);
    assert!(sim.active_particles.iter().all(|p| p.velocity.y > 0.0));
}

/// A shader with nothing interpretable leaves the simulator on its defaults
/// rather than installing an inert program that reports as active.
#[test]
fn a_shader_with_no_recognised_statements_is_not_installed() {
    let mut sim = simulator();
    let unsupported = sim.set_particles_shader("shader_type particles;\nuniform float speed;\n");
    assert!(unsupported.is_empty(), "nothing to report: {unsupported:?}");
    assert!(
        !sim.has_particles_shader(),
        "an empty program must not count as a shader — reporting one would make \
         `has_particles_shader` lie about whether anything drives the emitter"
    );
}
