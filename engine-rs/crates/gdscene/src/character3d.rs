//! CharacterBody3D motion driver — the 3D mirror of
//! [`PhysicsServer::process_character_movement`](crate::physics_server).
//!
//! Godot ground truth: `scene/3d/physics/character_body_3d.cpp`. A script calls
//! `move_and_slide()` each `_physics_process`; the engine consumes `velocity`,
//! slides along collisions, and updates `is_on_floor` / `is_on_wall` /
//! `is_on_ceiling`, the floor normal and the platform velocity.
//!
//! Patina had both halves and no seam (pat-0q2kx.36). `gdphysics3d::character`
//! implements the full `move_and_slide` — sub-stepping, slope snapping, platform
//! carry — and nothing ever called it: `MainLoop`'s 3D physics phase was
//! sync → step → sync, and only the 2D server had a character pass. A
//! `CharacterBody3D` in a scene therefore never moved, and `is_on_floor()`
//! answered `false` forever, with nothing to indicate why.
//!
//! This lives in its own module rather than on `PhysicsServer3D` for the same
//! reason [`raycast3d`](crate::raycast3d) and [`shapecast3d`](crate::shapecast3d)
//! do: it only needs read access to the server, so a free function taking
//! `&PhysicsServer3D` keeps it out of that type's `impl` block.
//!
//! The scripting bridge needs no 3D-specific work — `scripting.rs` dispatches
//! `move_and_slide()` / `is_on_floor()` through NODE PROPERTIES, which are
//! dimension-agnostic, so writing the same property names the 2D pass writes is
//! what makes `$Player.is_on_floor()` answer correctly in 3D.

use gdcore::math::Vector3;
use gdphysics3d::body::PhysicsBody3D;
use gdphysics3d::character::{CharacterBody3D, MotionMode};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::physics_server_3d::PhysicsServer3D;
use crate::scene_tree::SceneTree;

/// Godot's `CharacterBody3D` defaults, from `doc/classes/CharacterBody3D.xml`.
const DEFAULT_UP_DIRECTION: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const DEFAULT_FLOOR_MAX_ANGLE: f32 = 0.7853982;
const DEFAULT_FLOOR_SNAP_LENGTH: f32 = 0.1;
const DEFAULT_WALL_MIN_SLIDE_ANGLE: f32 = 0.2617994;
const DEFAULT_MAX_SLIDES: u32 = 6;

fn vec3_prop(tree: &SceneTree, id: NodeId, key: &str, default: Vector3) -> Vector3 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Vector3(v)) => v,
        _ => default,
    }
}

fn f32_prop(tree: &SceneTree, id: NodeId, key: &str, default: f32) -> f32 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        _ => default,
    }
}

fn bool_prop(tree: &SceneTree, id: NodeId, key: &str, default: bool) -> bool {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Bool(b)) => b,
        _ => default,
    }
}

fn u32_prop(tree: &SceneTree, id: NodeId, key: &str, default: u32) -> u32 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Int(i)) if i >= 0 => i as u32,
        _ => default,
    }
}

/// Runs `move_and_slide` for every `CharacterBody3D` node and writes the result
/// back onto the node.
///
/// Call this from the 3D physics phase AFTER `step`, so the character moves
/// against this tick's body positions.
pub fn process_character_movement(tree: &mut SceneTree, physics: &PhysicsServer3D, dt: f32) {
    if dt <= 0.0 {
        return;
    }

    let mut characters: Vec<NodeId> = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        if let Some(node) = tree.get_node(id) {
            if node.class_name() == "CharacterBody3D" {
                characters.push(id);
            }
        }
    }

    for node_id in characters {
        // Godot only moves a CharacterBody3D when the script calls
        // `move_and_slide()`; the engine never auto-moves it. The gate applies
        // only to SCRIPTED bodies — one with no script has nothing that could
        // call it, and scenes/tests that drive `velocity` directly rely on
        // auto-movement. Mirrors the 2D pass exactly (pat-0q2kx.16).
        if tree.has_script(node_id) {
            if !bool_prop(tree, node_id, "_move_and_slide_requested", false) {
                continue;
            }
            // One-shot: a script that stops calling move_and_slide stops moving.
            if let Some(node) = tree.get_node_mut(node_id) {
                node.set_property("_move_and_slide_requested", Variant::Bool(false));
            }
        }

        let velocity = vec3_prop(tree, node_id, "velocity", Vector3::ZERO);
        let stored_platform = vec3_prop(tree, node_id, "platform_velocity", Vector3::ZERO);
        if velocity.length_squared() < 1e-8 && stored_platform.length_squared() < 1e-8 {
            continue;
        }

        // A character with no resolvable collider cannot be swept; Godot
        // likewise does nothing rather than substituting a default shape.
        //
        // The shape lives on a CollisionShape3D CHILD, not on the body itself —
        // `shape_from_node_3d` returns `None` for any other class, so calling it
        // on the CharacterBody3D silently skipped every character and nothing
        // ever moved. Registration resolves shapes the same way (`find_map` over
        // the body's children in `sync_to_physics`).
        let children = match tree.get_node(node_id) {
            Some(n) => n.children().to_vec(),
            None => continue,
        };
        let Some(shape) = children
            .iter()
            .find_map(|&child| crate::physics_server_3d::shape_from_node_3d(tree, child))
        else {
            continue;
        };

        // World space, like every other physics read (pat-0q2kx.31).
        let start = node3d::get_global_transform(tree, node_id).origin;
        let mut character = CharacterBody3D::new(start, shape);
        character.up_direction = vec3_prop(tree, node_id, "up_direction", DEFAULT_UP_DIRECTION);
        character.floor_max_angle =
            f32_prop(tree, node_id, "floor_max_angle", DEFAULT_FLOOR_MAX_ANGLE);
        character.floor_snap_length = f32_prop(
            tree,
            node_id,
            "floor_snap_length",
            DEFAULT_FLOOR_SNAP_LENGTH,
        );
        character.floor_stop_on_slope = bool_prop(tree, node_id, "floor_stop_on_slope", true);
        character.floor_constant_speed = bool_prop(tree, node_id, "floor_constant_speed", false);
        character.floor_block_on_wall = bool_prop(tree, node_id, "floor_block_on_wall", true);
        character.wall_min_slide_angle = f32_prop(
            tree,
            node_id,
            "wall_min_slide_angle",
            DEFAULT_WALL_MIN_SLIDE_ANGLE,
        );
        character.max_slides = u32_prop(tree, node_id, "max_slides", DEFAULT_MAX_SLIDES).max(1);
        character.motion_mode = match u32_prop(tree, node_id, "motion_mode", 0) {
            1 => MotionMode::Floating,
            _ => MotionMode::Grounded,
        };
        character.collision_layer = u32_prop(tree, node_id, "collision_layer", 1);
        character.collision_mask = u32_prop(tree, node_id, "collision_mask", 1);

        // Everything except this character's own body. Without the exclusion a
        // character immediately collides with itself and never moves.
        let self_body = physics.body_for_node(node_id);
        let obstacles: Vec<&PhysicsBody3D> = physics
            .world()
            .bodies()
            .filter(|b| Some(b.id) != self_body)
            .collect();

        // `move_and_slide` takes THIS FRAME'S MOTION, not a per-second velocity
        // (gdphysics3d::character docs; its own tests pass raw distances), and
        // returns the motion left after sliding. Scale in and back out so the
        // node's `velocity` property stays a per-second value, as in Godot.
        let motion = velocity * dt;
        let result_motion = character.move_and_slide(motion, &obstacles);
        let new_velocity = result_motion / dt;

        let end = character.position;
        let on_floor = character.is_on_floor();
        let on_wall = character.is_on_wall();
        let on_ceiling = character.is_on_ceiling();
        let floor_normal = character.get_floor_normal();
        let platform_velocity = character.get_platform_velocity();

        // Godot's `get_real_velocity()` — the motion the last move ACTUALLY
        // achieved (pat-s3rrv.103). The 2D solver has published this since
        // pat-s3rrv.97; the 3D one never did, so `get_real_velocity()` on a
        // CharacterBody3D read a property that was not there.
        //
        // ⚠️ Derived from the position DELTA, not from `new_velocity` just
        // above. The two look interchangeable and are not: `new_velocity` is
        // the post-slide velocity the body carries into the NEXT tick, which
        // stays non-zero along a wall the body did not actually move along this
        // tick — exactly the case this method exists to detect.
        let real_velocity = if dt > 0.0 {
            (end - start) / dt
        } else {
            Vector3::ZERO
        };

        // De-parent through the parent chain before storing — `position` is a
        // LOCAL property, and writing a world coordinate into it re-applies the
        // parent offset every tick (pat-0q2kx.31).
        node3d::set_global_position(tree, node_id, end);
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property("velocity", Variant::Vector3(new_velocity));
            node.set_property("is_on_floor", Variant::Bool(on_floor));
            node.set_property("is_on_wall", Variant::Bool(on_wall));
            node.set_property("is_on_ceiling", Variant::Bool(on_ceiling));
            node.set_property("floor_normal", Variant::Vector3(floor_normal));
            node.set_property("platform_velocity", Variant::Vector3(platform_velocity));
            node.set_property("_real_velocity", Variant::Vector3(real_velocity));
        }
    }
}
