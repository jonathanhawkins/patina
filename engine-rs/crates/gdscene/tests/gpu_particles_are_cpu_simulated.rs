//! pat-z35r2.19 — `GPUParticles2D`/`3D` are CPU-simulated aliases, and this pins it.
//!
//! The bead's finding was that the `GPU` prefix "may overclaim". It does:
//! `MainLoop::process_particles` drives `GPUParticles2D`, `GPUParticles3D`,
//! `CPUParticles2D` and `CPUParticles3D` through the same CPU `ParticleSimulator`,
//! so the prefix is a Godot-compatibility name, not a statement about where the
//! work happens.
//!
//! The bead accepts "document CPU-sim **or** add a collision-bounce test". A
//! collision-bounce test cannot pass: there is no collider input to the simulator
//! at all (no `ParticlesCollision*` equivalent, no `collision_mode`), so particles
//! pass through every body — a bounce test would assert a feature that does not
//! exist. So this documents the CPU simulation instead, and does it *executably*:
//! a prose note can rot silently, whereas this fails the moment the two classes
//! stop sharing a path — which is exactly what would happen when someone really
//! does move `GPUParticles*` onto the GPU. At that point this test should fail and
//! be rewritten, not deleted; its failure is the signal that the name stopped
//! overclaiming.
//!
//! See the parity note at the top of `gdscene::particle` for the full gap list
//! (collision + trails).

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Steps to simulate — long enough to cover ramp-up, steady state, and death.
const FRAMES: usize = 40;

/// A particle node of `class`, configured identically regardless of class.
///
/// The simulator seeds from its own `total_emitted` counter (not the node id), so
/// two nodes with identical config are expected to produce identical sequences —
/// that is what makes the comparison below meaningful rather than coincidental.
fn particle_node(name: &str, class: &str) -> Node {
    let mut n = Node::new(name, class);
    n.set_property("emitting", Variant::Bool(true));
    n.set_property("amount", Variant::Int(16));
    n.set_property("lifetime", Variant::Float(0.5));
    n.set_property("one_shot", Variant::Bool(false));
    n
}

/// Runs both classes side by side and returns their per-frame live-particle counts.
fn counts_for(gpu_class: &str, cpu_class: &str) -> (Vec<usize>, Vec<usize>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let gpu_id = tree.add_child(root, particle_node("Gpu", gpu_class)).unwrap();
    let cpu_id = tree.add_child(root, particle_node("Cpu", cpu_class)).unwrap();

    let mut ml = MainLoop::new(tree);
    let mut gpu = Vec::with_capacity(FRAMES);
    let mut cpu = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        ml.step(1.0 / 60.0);
        gpu.push(ml.particle_count(gpu_id));
        cpu.push(ml.particle_count(cpu_id));
    }
    (gpu, cpu)
}

/// `GPUParticles2D` produces the same simulation as `CPUParticles2D`, frame for
/// frame — the "GPU" node is running the CPU simulator.
#[test]
fn gpu_particles_are_cpu_simulated() {
    let (gpu, cpu) = counts_for("GPUParticles2D", "CPUParticles2D");

    // Guard the test's own premise: a comparison of two all-zero series would
    // pass while proving nothing.
    assert!(
        gpu.iter().any(|&c| c > 0),
        "GPUParticles2D emitted nothing in {FRAMES} frames — test proves nothing"
    );
    assert_eq!(
        gpu, cpu,
        "GPUParticles2D must match CPUParticles2D frame-for-frame: both run \
         MainLoop::process_particles' CPU ParticleSimulator. If this now differs, \
         GPU particles became real — update the parity note in gdscene::particle."
    );
}

/// Same for the 3D pair.
#[test]
fn gpu_particles_3d_are_cpu_simulated() {
    let (gpu, cpu) = counts_for("GPUParticles3D", "CPUParticles3D");

    assert!(
        gpu.iter().any(|&c| c > 0),
        "GPUParticles3D emitted nothing in {FRAMES} frames — test proves nothing"
    );
    assert_eq!(
        gpu, cpu,
        "GPUParticles3D must match CPUParticles3D frame-for-frame (shared CPU simulator)"
    );
}
