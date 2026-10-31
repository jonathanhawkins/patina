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

/// A float property, or `default` when absent (pat-s3rrv.103).
///
/// ⚠️ Accepts `Variant::Int` too: a `.tscn` writes a whole number without a
/// decimal point, so an authored `safe_margin = 0` arrives as an Int and would
/// otherwise silently fall back to the default it was written to override.
fn float_prop(tree: &SceneTree, id: NodeId, key: &str, default: f64) -> f64 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
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
        // Godot's `apply_floor_snap()` — snap to the floor WITHOUT moving
        // (pat-s3rrv.103). A script calls it after teleporting, or to stay glued
        // while standing still.
        //
        // ⚠️ Read BEFORE the move gate below, and it is an INDEPENDENT reason to
        // run this body's pass. A script that calls only `apply_floor_snap()`
        // never sets `_move_and_slide_requested`, so the gate would `continue`
        // past it and the snap would never be considered — the method would
        // look wired and do nothing.
        //
        // One-shot like `_move_and_slide_requested`, and cleared for the same
        // reason: a script that stops asking stops snapping.
        let snap_requested = bool_prop(tree, node_id, "_apply_floor_snap_requested", false);
        if snap_requested {
            if let Some(node) = tree.get_node_mut(node_id) {
                node.set_property("_apply_floor_snap_requested", Variant::Bool(false));
            }
        }

        // Godot only moves a CharacterBody3D when the script calls
        // `move_and_slide()`; the engine never auto-moves it. The gate applies
        // only to SCRIPTED bodies — one with no script has nothing that could
        // call it, and scenes/tests that drive `velocity` directly rely on
        // auto-movement. Mirrors the 2D pass exactly (pat-0q2kx.16).
        if tree.has_script(node_id) {
            let move_requested = bool_prop(tree, node_id, "_move_and_slide_requested", false);
            if !move_requested && !snap_requested {
                continue;
            }
            // One-shot: a script that stops calling move_and_slide stops moving.
            // Cleared only when it was actually set, so a snap-only frame does
            // not consume a move request the script has not made.
            if move_requested {
                if let Some(node) = tree.get_node_mut(node_id) {
                    node.set_property("_move_and_slide_requested", Variant::Bool(false));
                }
            }
        }

        let velocity = vec3_prop(tree, node_id, "velocity", Vector3::ZERO);
        let stored_platform = vec3_prop(tree, node_id, "platform_velocity", Vector3::ZERO);
        // ⚠️ `snap_requested` deliberately defeats the idle skip. Snapping while
        // STANDING STILL is the whole point of the standalone call — a body that
        // is already moving gets snapped by `move_and_slide` itself
        // (`character.rs`, the `grounded_mode` branch), so gating this on
        // velocity would make the method a no-op in exactly the case it exists
        // for.
        //
        // Nothing else is needed: the loop below runs its normal course with
        // zero motion, `move_and_slide(ZERO)` moves the body nowhere and applies
        // the snap internally, and the existing publish block reports the new
        // position and `is_on_floor` — so this path cannot drift from the
        // moving one.
        if velocity.length_squared() < 1e-8
            && stored_platform.length_squared() < 1e-8
            && !snap_requested
        {
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
        // Godot's default is TRUE — a body slides along a ceiling unless the
        // scene explicitly turns it off (`CharacterBody3D.xml:183`).
        character.slide_on_ceiling = bool_prop(tree, node_id, "slide_on_ceiling", true);
        // Godot's default is 0.001, and 0 is a legal authored value meaning
        // "resolve to exact contact" — so the default belongs here rather than
        // being synthesised from a missing property (pat-s3rrv.103).
        character.safe_margin = float_prop(tree, node_id, "safe_margin", 0.001) as f32;
        // Godot's default is EVERY layer (4294967295), so an unauthored scene
        // keeps the carry-everything behaviour it had before this existed
        // (`CharacterBody3D.xml:168`).
        character.platform_floor_layers =
            u32_prop(tree, node_id, "platform_floor_layers", u32::MAX);
        // ⚠️ Default 0, NOT u32::MAX like its floor sibling directly above.
        // Godot ignores every wall body unless a scene opts one in
        // (`CharacterBody3D.xml`, "all wall bodies are ignored"), so the two
        // masks deliberately disagree — copying the line above would make every
        // wall drag the character sideways.
        character.platform_wall_layers = u32_prop(tree, node_id, "platform_wall_layers", 0);
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
        // pat-s3rrv.103 — the standalone `apply_floor_snap()`, applied BEFORE
        // the move so the body is already grounded when it slides.
        //
        // ⚠️ Calls the MANUAL variant, not the one `move_and_slide` uses
        // internally. That one refuses unless the body was already grounded or
        // is descending, which is right mid-jump and wrong here: a body
        // standing still is neither, so routing the request through it left the
        // method silently doing nothing.
        if snap_requested {
            character.apply_floor_snap_manual(&obstacles);
        }

        let motion = velocity * dt;
        let result_motion = character.move_and_slide(motion, &obstacles);
        let new_velocity = result_motion / dt;

        let end = character.position;
        let on_floor = character.is_on_floor();
        let on_wall = character.is_on_wall();
        let on_ceiling = character.is_on_ceiling();
        let floor_normal = character.get_floor_normal();
        // Never published before pat-mgixk.37, so `get_wall_normal()` had
        // nothing to read in 3D even though the solver tracked it — the same
        // gap the 2D side had, and the dispatch arm is shared.
        let wall_normal = character.get_wall_normal();
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

        // Godot's `get_position_delta` — the travel over the last
        // `move_and_slide` (pat-s3rrv.103). Same numerator as `_real_velocity`
        // above WITHOUT the division, so the two cannot disagree about how far
        // the body went.
        let position_delta = end - start;
        // ⚠️ `get_last_motion` is NOT this. Godot splits the movement into
        // several motions when sliding occurs and reports the LAST one, so it
        // equals the delta only when nothing was hit. The solver records it per
        // sub-step; reusing `position_delta` here would look right in every
        // fixture that never collides.
        let last_motion = character.get_last_motion();
        let platform_angular_velocity = character.get_platform_angular_velocity();

        // Godot's `get_slide_collision(i)` / `KinematicCollision3D`
        // (pat-s3rrv.103). Published as the SAME `_slide_collisions` array the
        // 2D bridge writes, with the same keys, because the script surface that
        // reads it is dimension-agnostic: `kinematic_collision_call` hands each
        // field straight back with `rec.get(k).cloned()`, so a Vector3 stored
        // here is what `get_normal()` returns — no 3D-specific arm needed.
        //
        // ⚠️ The collider is resolved from `BodyId3D` to a NODE here. The solver
        // deliberately records only the id (it knows nothing about the tree),
        // and this is the only place holding both halves of that map.
        let slide_records: Vec<Variant> = (0..character.get_slide_collision_count())
            .filter_map(|i| character.get_slide_collision(i).copied())
            .map(|c| {
                let mut rec = gdvariant::Dictionary::new();
                rec.insert("normal", Variant::Vector3(c.normal));
                rec.insert("depth", Variant::Float(c.depth as f64));
                rec.insert("position", Variant::Vector3(c.position));
                rec.insert("remainder", Variant::Vector3(c.remainder));
                rec.insert(
                    "collider",
                    c.collider
                        .and_then(|bid| physics.node_for_body(bid))
                        .map(|nid| Variant::ObjectId(nid.object_id()))
                        .unwrap_or(Variant::Nil),
                );
                Variant::Dictionary(rec.into())
            })
            .collect();

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
            node.set_property("wall_normal", Variant::Vector3(wall_normal));
            node.set_property("platform_velocity", Variant::Vector3(platform_velocity));
            node.set_property("_real_velocity", Variant::Vector3(real_velocity));
            node.set_property("_position_delta", Variant::Vector3(position_delta));
            node.set_property("_last_motion", Variant::Vector3(last_motion));
            node.set_property(
                "_platform_angular_velocity",
                Variant::Vector3(platform_angular_velocity),
            );
            node.set_property("_slide_collisions", Variant::Array(slide_records.into()));
        }
    }
}
