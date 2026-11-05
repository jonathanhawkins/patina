//! The global random draws Godot makes while CONSTRUCTING a node (pat-y2ylj).
//!
//! Some Godot constructors take a number from the global RNG (`Math::rand()`,
//! the generator behind `randf()`/`randi()`), so building one shifts every
//! random number a game draws afterwards:
//!
//! - `GPUParticles2D()` / `GPUParticles3D()`: `particles_create()` builds the
//!   rendering server's `Particles`, whose constructor does
//!   `random_seed = Math::rand()` (`drivers/gles3/storage/particles_storage.h`,
//!   `renderer_rd/storage_rd/particles_storage.h`), then the node itself does
//!   `set_seed(Math::rand())` — TWO draws;
//! - `CPUParticles2D()` / `CPUParticles3D()`: `set_seed(Math::rand())` — one.
//!
//! 🔑 Dodge the Creeps' Player carries a `Trail` (GPUParticles2D), so in Godot
//! the first mob's `randf()` is the THIRD number of the seeded stream (a pinned
//! Godot capture's first spawn reads 0.829341 = draw #3 for seed 42). Without
//! these two draws every Patina mob spawned from the wrong numbers.
//!
//! Patina's particle simulation does not read the node's `seed`, so only the
//! draws are reproduced; the property is left alone (Godot does not save it
//! either unless `use_fixed_seed` is set).
//!
//! ⚠️ Not reproduced: the draws in `set_emitting(true)` of a ONE-SHOT
//! particles node and in `restart()` (one each, unless `use_fixed_seed`).

use std::cell::Cell;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// How many global random numbers Godot's constructor of `class_name` draws.
pub fn constructor_draws(class_name: &str) -> u32 {
    match class_name {
        "GPUParticles2D" | "GPUParticles3D" => 2,
        "CPUParticles2D" | "CPUParticles3D" => 1,
        _ => 0,
    }
}

thread_local! {
    /// Depth of [`suppress`] guards alive on this thread.
    static SUPPRESSED: Cell<u32> = const { Cell::new(0) };
}

/// While the returned guard lives, constructing a node on THIS thread draws
/// nothing. A host that builds a tree in pieces (the editor's Play clones the
/// edited tree, then expands its instances) suppresses the per-node draws and
/// makes them once, in Godot's construction order, with [`draw_for_subtree`].
#[must_use = "the draws are suppressed only while the guard is alive"]
pub fn suppress() -> SuppressGuard {
    SUPPRESSED.with(|s| s.set(s.get() + 1));
    SuppressGuard(())
}

/// See [`suppress`].
pub struct SuppressGuard(());

impl Drop for SuppressGuard {
    fn drop(&mut self) {
        SUPPRESSED.with(|s| s.set(s.get().saturating_sub(1)));
    }
}

/// Makes the draws Godot's constructor of `class_name` makes — what building
/// the node at runtime does (`PackedScene.instantiate()`, `ClassName.new()`).
pub fn on_construct(class_name: &str) {
    if SUPPRESSED.with(|s| s.get()) > 0 {
        return;
    }
    for _ in 0..constructor_draws(class_name) {
        gdcore::rng::randi();
    }
}

/// Makes the constructor draws of every node under `root` (inclusive), in
/// pre-order — the order a `PackedScene` constructs them in. Returns how many
/// numbers were drawn. Draws even while suppressed: this IS the deferred pass.
pub fn draw_for_subtree(tree: &SceneTree, root: NodeId) -> u32 {
    let mut ids = Vec::new();
    tree.collect_subtree_top_down(root, &mut ids);
    let mut drawn = 0;
    for id in ids {
        if let Some(node) = tree.get_node(id) {
            for _ in 0..constructor_draws(node.class_name()) {
                gdcore::rng::randi();
                drawn += 1;
            }
        }
    }
    drawn
}
