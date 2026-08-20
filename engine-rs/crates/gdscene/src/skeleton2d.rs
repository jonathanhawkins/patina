//! The 2D bone phase: pose `Bone2D` chains and publish skinning transforms
//! (pat-s3rrv.185).
//!
//! Every piece of this existed and none of it ran. `gdrender2d::polygon_raster`
//! has held `Bone2D`, `Skeleton2D::skinning_transforms` and
//! `deform_polygon_vertices` with **no caller outside its own unit tests**;
//! `gdscene::ik_solver_2d::solve_two_bone_ik_2d` had exactly one reference in
//! the workspace — its own `pub mod` line; and pat-s3rrv.135 registered
//! `SkeletonModificationStack2D` / `SkeletonModification2DTwoBoneIK` while
//! recording in its own test header that "the stack is still never *executed*".
//! This module is the phase that runs them.
//!
//! Ported from `upstream/godot/scene/2d/skeleton_2d.cpp`.

use gdcore::math::Transform2D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// Where a `Skeleton2D` publishes one `Transform2D` per bone, in bone order.
///
/// 🔑 Published on the node rather than returned, because the consumer is the
/// render pass in a different crate and runs later in the frame. This is the
/// same shape `process_navigation` uses for baked navmesh data — an internal
/// `__`-prefixed key, which `scene_saver::is_internal_prop` keeps out of saved
/// scenes.
pub const SKIN_XFORMS_PROP: &str = "__skin_xforms__";

/// A bone gathered from the tree, with the data the skinning maths needs.
#[derive(Debug, Clone, Copy)]
pub struct GatheredBone {
    /// The node itself, so a solved pose can be written back.
    pub id: NodeId,
    /// The bone's rest transform in global space.
    pub rest: Transform2D,
    /// The bone's current global transform.
    pub pose: Transform2D,
}

/// Reads a `Bone2D`'s `rest`, or `None` when it has never been authored.
///
/// ⚠️ Godot's default for `Bone2D.rest` is `Transform2D(0, 0, 0, 0, 0, 0)` —
/// an all-zero, **singular** matrix, not the identity (`doc/classes/Bone2D.xml`).
/// The skinning transform is `pose * rest.affine_inverse()`, so inverting an
/// unauthored rest divides by a zero determinant and pushes NaN into every
/// vertex the bone touches. Upstream avoids it because `apply_rest` and
/// `set_autocalculate_length_and_angle` populate the field; a `.tscn` rig that
/// simply omits it does not, so "no rest recorded" has to be a real case.
pub fn bone_rest(tree: &SceneTree, bone: NodeId) -> Option<Transform2D> {
    let raw = match tree.get_node(bone).map(|n| n.get_property("rest")) {
        Some(Variant::Transform2D(t)) => t,
        _ => return None,
    };
    // A zero basis is the "unset" sentinel; a legitimate rest is invertible.
    if raw.x.length_squared() <= f32::EPSILON || raw.y.length_squared() <= f32::EPSILON {
        return None;
    }
    Some(raw)
}

/// Every `Bone2D` at or below `root`, depth-first in tree order.
///
/// 🔑 Tree order IS weight order: a `Polygon2D`'s per-bone weight arrays are
/// indexed by the skeleton's bone list, so the order this walk produces is what
/// makes `weights[b]` refer to the bone the scene author meant. Godot builds the
/// same list in `Skeleton2D::_update_bone_setup` by walking children in order.
pub fn gather_bones(tree: &SceneTree, root: NodeId) -> Vec<NodeId> {
    fn walk(tree: &SceneTree, id: NodeId, out: &mut Vec<NodeId>) {
        let children = tree
            .get_node(id)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();
        for child in children {
            if tree
                .get_node(child)
                .map(|n| n.class_name() == "Bone2D")
                .unwrap_or(false)
            {
                out.push(child);
            }
            // Descend regardless: a Bone2D chain nests, and Godot also allows a
            // non-bone node partway down without ending the skeleton.
            walk(tree, child, out);
        }
    }
    let mut out = Vec::new();
    walk(tree, root, &mut out);
    out
}

/// Gathers a skeleton's bones with their rest and current global transforms.
pub fn gather(tree: &SceneTree, skeleton: NodeId) -> Vec<GatheredBone> {
    gather_bones(tree, skeleton)
        .into_iter()
        .map(|id| {
            let pose = crate::node2d::get_global_transform(tree, id);
            GatheredBone {
                id,
                // ⚠️ Falling back to the CURRENT pose, not to identity. A bone
                // with no authored rest is by definition already at rest, so
                // `pose * rest.affine_inverse()` collapses to the identity and
                // the vertex stays put — whereas an identity rest would treat
                // the bone's whole world transform as deformation and fling the
                // polygon across the scene on frame 0.
                rest: bone_rest(tree, id).unwrap_or(pose),
                pose,
            }
        })
        .collect()
}

/// The skinning transform per bone, in bone order.
pub fn skinning_transforms(bones: &[GatheredBone]) -> Vec<Transform2D> {
    bones
        .iter()
        .map(|b| b.pose * b.rest.affine_inverse())
        .collect()
}

/// Publishes `xforms` on the skeleton node for the render pass to read.
pub fn publish(tree: &mut SceneTree, skeleton: NodeId, xforms: &[Transform2D]) {
    let arr: Vec<Variant> = xforms.iter().map(|t| Variant::Transform2D(*t)).collect();
    if let Some(node) = tree.get_node_mut(skeleton) {
        node.set_property(SKIN_XFORMS_PROP, Variant::Array(arr));
    }
}

/// Reads back what [`publish`] wrote.
pub fn published_skin_transforms(tree: &SceneTree, skeleton: NodeId) -> Vec<Transform2D> {
    match tree
        .get_node(skeleton)
        .map(|n| n.get_property(SKIN_XFORMS_PROP))
    {
        Some(Variant::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Variant::Transform2D(t) => Some(*t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Every `Skeleton2D` in the tree, in tree order.
pub fn skeletons_in_tree(tree: &SceneTree) -> Vec<NodeId> {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .filter(|&id| {
            tree.get_node(id)
                .map(|n| n.class_name() == "Skeleton2D")
                .unwrap_or(false)
        })
        .collect()
}

/// Resolves a modification's `NodePath`, which is relative to the SKELETON.
///
/// ⚠️ Not relative to the modification or to the scene root. Godot resolves a
/// `SkeletonModification2D`'s node paths through `stack->skeleton`
/// (`skeleton_modification_2d_twoboneik.cpp`), so a rig that says
/// `NodePath("Bone1/Bone2")` means "under the skeleton" — resolving from the
/// root would find nothing and the modifier would silently no-op.
fn resolve_from_skeleton(tree: &SceneTree, skeleton: NodeId, path: &str) -> Option<NodeId> {
    if path.is_empty() {
        return None;
    }
    let mut current = skeleton;
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        // `..` is a real name in a relative NodePath, not a separator artifact:
        // `NodePath::new("../Skel")` keeps it as `get_name(0)`. Without this arm
        // the walk looked for a CHILD literally named "..", found none, and
        // resolved to `None` — turning every rig that reaches outside its own
        // subtree into a silent no-op.
        if part == ".." {
            current = tree.get_node(current).and_then(|n| n.parent())?;
            continue;
        }
        let children = tree.get_node(current).map(|n| n.children().to_vec())?;
        let next = children
            .into_iter()
            .find(|&c| tree.get_node(c).map(|n| n.name() == part).unwrap_or(false))?;
        current = next;
    }
    Some(current)
}

fn mod_string(tree: &SceneTree, skeleton: NodeId, i: usize, key: &str) -> Option<String> {
    match tree
        .get_node(skeleton)
        .map(|n| n.get_property(&format!("__skelmod_{i}_{key}__")))
    {
        Some(Variant::String(s)) if !s.is_empty() => Some(s),
        // A `.tscn` writes every one of these keys as `NodePath("Bone1")`, not
        // as a String, and the flattener in `packed_scene.rs` copies the parsed
        // Variant through verbatim. Accepting only `String` meant each solver
        // returned at its FIRST guard, so a scene-loaded modification stack did
        // nothing at all — silently, with the stack flattened and `enabled`
        // true. The unit tests stayed green because they inject
        // `Variant::String` properties instead of loading a scene, which is
        // exactly the load boundary this bead exists to close (pat-s3rrv.185).
        Some(Variant::NodePath(np)) => {
            let joined = np.get_concatenated_names();
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

/// The bone's length: its authored `length`, else the distance to its first
/// child bone, else zero.
///
/// 🔑 The fallback matters for the tip. A two-bone chain's end-effector is the
/// far end of the SECOND bone, and nothing else in the scene records where that
/// is — Godot keeps it in `Bone2D.length`, which is unregistered here, so a rig
/// that omits it would otherwise solve for a zero-length final segment and put
/// the tip on the joint.
fn bone_length(tree: &SceneTree, bone: NodeId) -> f32 {
    match tree.get_node(bone).map(|n| n.get_property("length")) {
        Some(Variant::Float(f)) => return f as f32,
        Some(Variant::Int(i)) => return i as f32,
        _ => {}
    }
    let here = crate::node2d::get_global_position(tree, bone);
    gather_bones(tree, bone)
        .first()
        .map(|&child| (crate::node2d::get_global_position(tree, child) - here).length())
        .unwrap_or(0.0)
}

/// Points `bone` at `target` in world space by setting its LOCAL rotation.
///
/// ⚠️ Local, because a Bone2D's position is parent-relative: writing a world
/// angle straight onto the node double-counts every ancestor's rotation. The
/// parent's accumulated global rotation is subtracted, which is exactly what
/// `set_global_rotation` does — spelled out here because the value being
/// subtracted is the thing that makes the second bone of a chain land right.
fn aim_bone(
    tree: &mut SceneTree,
    bone: NodeId,
    from: gdcore::math::Vector2,
    target: gdcore::math::Vector2,
) {
    let delta = target - from;
    if delta.length_squared() <= 1e-12 {
        return;
    }
    let world_angle = delta.y.atan2(delta.x);
    let parent_angle = tree
        .get_node(bone)
        .and_then(|n| n.parent())
        .map(|p| crate::node2d::get_global_rotation(tree, p))
        .unwrap_or(0.0);
    crate::node2d::set_rotation(tree, bone, world_angle - parent_angle);
}

/// Runs one `SkeletonModification2DTwoBoneIK` entry.
fn two_bone_ik(tree: &mut SceneTree, skeleton: NodeId, i: usize) {
    let Some(target_path) = mod_string(tree, skeleton, i, "target_nodepath") else {
        return;
    };
    let Some(one_path) = mod_string(tree, skeleton, i, "joint_one_bone2d_node") else {
        return;
    };
    let Some(two_path) = mod_string(tree, skeleton, i, "joint_two_bone2d_node") else {
        return;
    };
    let (Some(target), Some(one), Some(two)) = (
        resolve_from_skeleton(tree, skeleton, &target_path),
        resolve_from_skeleton(tree, skeleton, &one_path),
        resolve_from_skeleton(tree, skeleton, &two_path),
    ) else {
        return;
    };

    let root = crate::node2d::get_global_position(tree, one);
    let joint_pos = crate::node2d::get_global_position(tree, two);
    let l1 = (joint_pos - root).length();
    let l2 = bone_length(tree, two);
    let goal = crate::node2d::get_global_position(tree, target);

    let flip = matches!(
        tree.get_node(skeleton)
            .map(|n| n.get_property(&format!("__skelmod_{i}_flip_bend_direction__"))),
        Some(Variant::Bool(true))
    );
    // `flip_bend_direction` inverts which of the two mirror solutions is taken,
    // so it is the negation of the solver's `bend_positive`.
    let solution = crate::ik_solver_2d::solve_two_bone_ik_2d(root, goal, l1, l2, !flip);

    // Root bone first: aiming it moves the joint, and the second bone's own
    // aim is measured from where the joint ENDS UP, not where it started.
    aim_bone(tree, one, root, solution.joint);
    aim_bone(tree, two, solution.joint, solution.tip);
}

/// Runs one `SkeletonModification2DLookAt` entry (pat-s3rrv.221).
///
/// Godot's LookAt has exactly three members — `bone2d_node`, `bone_index` and
/// `target_nodepath` (`doc/classes/SkeletonModification2DLookAt.xml`) — so the
/// whole modification is "point this bone at that node". `bone_index` is an
/// alternative way to name the same bone and is ignored while the path is
/// present, which is the case Godot's own editor writes.
fn look_at(tree: &mut SceneTree, skeleton: NodeId, i: usize) {
    let Some(bone_path) = mod_string(tree, skeleton, i, "bone2d_node") else {
        return;
    };
    let Some(target_path) = mod_string(tree, skeleton, i, "target_nodepath") else {
        return;
    };
    let (Some(bone), Some(target)) = (
        resolve_from_skeleton(tree, skeleton, &bone_path),
        resolve_from_skeleton(tree, skeleton, &target_path),
    ) else {
        return;
    };

    let from = crate::node2d::get_global_position(tree, bone);
    let goal = crate::node2d::get_global_position(tree, target);
    aim_bone(tree, bone, from, goal);
}

/// Puts every bone back on its authored `rest` rotation.
///
/// What a disabled stack means: Godot stops running the modifications, and the
/// bones fall back to the rest pose the rig was authored in. Without this a
/// disabled stack would simply freeze the last solved pose, which looks like
/// the stack is still on.
fn restore_rest_pose(tree: &mut SceneTree, skeleton: NodeId) {
    for bone in gather_bones(tree, skeleton) {
        let Some(rest) = bone_rest(tree, bone) else {
            continue;
        };
        // The rest basis's X column is the bone's rest direction.
        crate::node2d::set_rotation(tree, bone, rest.x.y.atan2(rest.x.x));
    }
}

/// Whether the skeleton's modification stack is enabled.
///
/// ⚠️ **Deliberate, documented divergence from Godot's default.**
/// `SkeletonModificationStack2D.enabled` defaults to **false** in Godot
/// (`doc/classes/SkeletonModificationStack2D.xml`, and our own ClassDB entry
/// records that faithfully) — the opposite of `SkeletonModification2D.enabled`,
/// which defaults to true.
///
/// Here only an explicit `false` disables. The reason is that the `.tscn`
/// flattener does not emit `__skelmod_enabled__` at all, so an absent key means
/// "nobody told us", not "the author set false". Taking Godot's default would
/// make **every** existing rig's stack inert *and* rest-restore it every frame,
/// silently killing the `TwoBoneIK` solver that already ships.
///
/// The real fix is for the flattener to carry the stack's `enabled` through
/// (`packed_scene.rs`); once it does, this should flip to Godot's default.
fn stack_enabled(tree: &SceneTree, skeleton: NodeId) -> bool {
    !matches!(
        tree.get_node(skeleton)
            .map(|n| n.get_property("__skelmod_enabled__")),
        Some(Variant::Bool(false))
    )
}

/// Runs a skeleton's modification stack, in stack order.
pub fn execute_modifications(tree: &mut SceneTree, skeleton: NodeId) {
    let count = match tree
        .get_node(skeleton)
        .map(|n| n.get_property("__skelmod_count__"))
    {
        Some(Variant::Int(c)) if c > 0 => c as usize,
        _ => return,
    };
    // A disabled stack does not merely stop solving — it returns the rig to its
    // rest pose (pat-s3rrv.221).
    if !stack_enabled(tree, skeleton) {
        restore_rest_pose(tree, skeleton);
        return;
    }
    for i in 0..count {
        // An entry explicitly disabled is skipped; an absent `enabled` key means
        // the rig never wrote one, which Godot treats as enabled.
        if matches!(
            tree.get_node(skeleton)
                .map(|n| n.get_property(&format!("__skelmod_{i}_enabled__"))),
            Some(Variant::Bool(false))
        ) {
            continue;
        }
        match mod_string(tree, skeleton, i, "class").as_deref() {
            Some("SkeletonModification2DTwoBoneIK") => two_bone_ik(tree, skeleton, i),
            Some("SkeletonModification2DLookAt") => look_at(tree, skeleton, i),
            // Other modification classes are registered but not executed yet;
            // declining is what keeps an unknown entry from silently corrupting
            // a pose it does not understand.
            _ => {}
        }
    }
}

/// The 2D bone phase — run once per frame, beside `skeleton3d::process_skeletons`.
///
/// Poses each skeleton's bones through its modification stack, then publishes
/// the skinning transforms the render pass deforms polygons with.
pub fn process_skeletons_2d(tree: &mut SceneTree) {
    for skeleton in skeletons_in_tree(tree) {
        execute_modifications(tree, skeleton);
        let bones = gather(tree, skeleton);
        let xforms = skinning_transforms(&bones);
        publish(tree, skeleton, &xforms);
    }
}
