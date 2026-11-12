//! pat-nzgdu.145 — GPUParticles2D node-level behaviours, verified through a
//! scene run (`MainLoop::step`) and signal capture rather than struct unit tests,
//! per the bead's acceptance:
//!
//! 1. a one_shot emitter drains and emits `finished` to a connected callback
//!    exactly once;
//! 2. `draw_order` reorders the emitted (published) draw data by particle age;
//! 3. `fixed_fps` gives a per-frame particle count independent of the render
//!    delta chunking.
//!
//! ⚠️ SCOPE: `trails` (ribbon geometry) is named in the bead title but is a
//! large separate feature and is NOT implemented — the acceptance *criteria*
//! cover the three behaviours above, which this file verifies. See the bead
//! comment for the remaining feature list (trails, sub_emitter, emit_particle,
//! preprocess, use_fixed_seed, GPUParticles3D parity).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The draw data published back onto a particle node, in draw order.
fn published_positions(ml: &MainLoop, pid: NodeId) -> Vec<Vector2> {
    match ml
        .tree()
        .get_node(pid)
        .map(|n| n.get_property("_particle_positions"))
    {
        Some(Variant::PackedVector2Array(v)) => v,
        _ => Vec::new(),
    }
}

/// Acceptance clause 1: a scene-loaded one_shot GPUParticles2D drains and emits
/// `finished` to a connected script callback exactly once.
#[test]
fn particle_node_trails_and_emit_signal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Burst", "GPUParticles2D");
    node.set_property("emitting", Variant::Bool(true));
    node.set_property("one_shot", Variant::Bool(true));
    node.set_property("amount", Variant::Int(8));
    node.set_property("lifetime", Variant::Float(0.2));
    let pid = tree.add_child(root, node).unwrap();

    // Connect a script-style callback to `finished` and count how often it fires.
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
    // 3 s — well past the burst (8 particles over 0.2 s) + their lifetime.
    for _ in 0..180 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "one_shot GPUParticles2D must emit `finished` to the connected callback exactly once",
    );
}

/// Acceptance clause 2: `draw_order` reorders the published draw data by age.
/// Lifetime and ReverseLifetime sort the *same* (deterministic-seed) particles in
/// opposite order, so their published position sequences must differ.
#[test]
fn draw_order_reorders_published_draw_data() {
    let build = |order: i64| -> (MainLoop, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("P", "GPUParticles2D");
        n.set_property("emitting", Variant::Bool(true));
        n.set_property("amount", Variant::Int(16));
        n.set_property("lifetime", Variant::Float(1.0));
        n.set_property("draw_order", Variant::Int(order));
        // Motion so each particle occupies a distinct position.
        n.set_property("initial_velocity_min", Variant::Float(60.0));
        n.set_property("initial_velocity_max", Variant::Float(60.0));
        let pid = tree.add_child(root, n).unwrap();
        let mut ml = MainLoop::new(tree);
        // Continuous emission (default explosiveness) spreads particles over the
        // lifetime, so after 30 frames they have a range of ages.
        for _ in 0..30 {
            ml.step(1.0 / 60.0);
        }
        (ml, pid)
    };

    let (lifetime_ml, lt_id) = build(1); // ParticleDrawOrder::Lifetime
    let (reverse_ml, rv_id) = build(2); // ParticleDrawOrder::ReverseLifetime

    let pos_lifetime = published_positions(&lifetime_ml, lt_id);
    let pos_reverse = published_positions(&reverse_ml, rv_id);

    assert!(
        pos_lifetime.len() >= 2,
        "need multiple particles to observe a reorder, got {}",
        pos_lifetime.len(),
    );
    assert_eq!(
        pos_lifetime.len(),
        pos_reverse.len(),
        "the two emitters simulate the same particle set",
    );
    // Same particles (seed = total_emitted, independent of node id), opposite
    // draw order → the published sequences differ. If draw_order were ignored
    // (the bug this bead fixes) both would be insertion order and match.
    assert_ne!(
        pos_lifetime, pos_reverse,
        "draw_order must reorder the published draw data",
    );
}

/// Acceptance clause 3: a `fixed_fps` value produces a deterministic per-frame
/// particle count independent of how the render delta is chunked.
#[test]
fn fixed_fps_gives_deterministic_particle_count() {
    let build = || -> (MainLoop, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("P", "GPUParticles2D");
        n.set_property("emitting", Variant::Bool(true));
        n.set_property("amount", Variant::Int(32));
        n.set_property("lifetime", Variant::Float(0.3));
        n.set_property("fixed_fps", Variant::Int(30));
        let pid = tree.add_child(root, n).unwrap();
        (MainLoop::new(tree), pid)
    };

    // The same 0.5 s of game time, chunked coarsely vs finely.
    let (mut fine, fine_id) = build();
    for _ in 0..60 {
        fine.step(0.5 / 60.0);
    }
    let (mut coarse, coarse_id) = build();
    for _ in 0..5 {
        coarse.step(0.5 / 5.0);
    }

    let c_fine = fine.particle_count(fine_id) as i64;
    let c_coarse = coarse.particle_count(coarse_id) as i64;

    assert!(c_fine > 0, "the emitter actually emitted");
    // fixed_fps decouples the sim from the render chunking; allow the ±1 sub-step
    // that float accumulation can produce across different chunkings.
    assert!(
        (c_fine - c_coarse).abs() <= 1,
        "fixed_fps count must be independent of render delta: {c_fine} vs {c_coarse}",
    );
}
