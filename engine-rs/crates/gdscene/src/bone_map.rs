//! Godot's `SkeletonProfile` and `BoneMap` retargeting resources
//! (pat-s3rrv.134).
//!
//! A `SkeletonProfile` names an ordered set of *canonical* bones (Godot ships
//! `SkeletonProfileHumanoid` with 56 of them). A `BoneMap` then records, for
//! each profile bone, which bone of a particular skeleton plays that role —
//! which is what lets `RetargetModifier3D` replay one rig's animation on
//! another.
//!
//! Sources: `upstream/godot/scene/resources/bone_map.cpp`,
//! `skeleton_profile.cpp`.

use crate::node::NodeId;
use crate::scene_tree::SceneTree;
use gdcore::math3d::Transform3D;
use std::collections::BTreeMap;

/// An ordered set of canonical bone names.
///
/// ⓘ Godot's `SkeletonProfile` also carries per-bone groups, reference poses,
/// tails and handle offsets — those are editor-facing and are not modelled here
/// yet. The bone *names* are what `BoneMap` keys on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkeletonProfile {
    bone_names: Vec<String>,
    /// Parent bone name per entry, `""` for a root (pat-s3rrv.222).
    bone_parents: Vec<String>,
    /// Godot `SkeletonProfile.is_required(idx)`.
    required: Vec<bool>,
    /// Godot `SkeletonProfile.root_bone`.
    root_bone: String,
    /// Godot `SkeletonProfile.scale_base_bone`.
    scale_base_bone: String,
}

/// Godot's humanoid bone table: `(name, parent, required)`, in profile order.
///
/// Transcribed from `upstream/godot/scene/resources/skeleton_profile.cpp`'s
/// `SkeletonProfileHumanoid` constructor — all 56 entries, 17 of them required.
///
/// ⚠️ The order is load-bearing: `find_bone` returns an INDEX, and Godot's
/// `BoneMap` and the glTF/FBX importers key on it. Sorting this table
/// alphabetically would silently renumber every bone.
const HUMANOID_BONES: &[(&str, &str, bool)] = &[
    ("Root", "", false),
    ("Hips", "Root", true),
    ("Spine", "Hips", true),
    ("Chest", "Spine", false),
    ("UpperChest", "Chest", false),
    ("Neck", "UpperChest", false),
    ("Head", "Neck", true),
    ("LeftEye", "Head", false),
    ("RightEye", "Head", false),
    ("Jaw", "Head", false),
    ("LeftShoulder", "UpperChest", true),
    ("LeftUpperArm", "LeftShoulder", true),
    ("LeftLowerArm", "LeftUpperArm", true),
    ("LeftHand", "LeftLowerArm", true),
    ("LeftThumbMetacarpal", "LeftHand", false),
    ("LeftThumbProximal", "LeftThumbMetacarpal", false),
    ("LeftThumbDistal", "LeftThumbProximal", false),
    ("LeftIndexProximal", "LeftHand", false),
    ("LeftIndexIntermediate", "LeftIndexProximal", false),
    ("LeftIndexDistal", "LeftIndexIntermediate", false),
    ("LeftMiddleProximal", "LeftHand", false),
    ("LeftMiddleIntermediate", "LeftMiddleProximal", false),
    ("LeftMiddleDistal", "LeftMiddleIntermediate", false),
    ("LeftRingProximal", "LeftHand", false),
    ("LeftRingIntermediate", "LeftRingProximal", false),
    ("LeftRingDistal", "LeftRingIntermediate", false),
    ("LeftLittleProximal", "LeftHand", false),
    ("LeftLittleIntermediate", "LeftLittleProximal", false),
    ("LeftLittleDistal", "LeftLittleIntermediate", false),
    ("RightShoulder", "UpperChest", true),
    ("RightUpperArm", "RightShoulder", true),
    ("RightLowerArm", "RightUpperArm", true),
    ("RightHand", "RightLowerArm", true),
    ("RightThumbMetacarpal", "RightHand", false),
    ("RightThumbProximal", "RightThumbMetacarpal", false),
    ("RightThumbDistal", "RightThumbProximal", false),
    ("RightIndexProximal", "RightHand", false),
    ("RightIndexIntermediate", "RightIndexProximal", false),
    ("RightIndexDistal", "RightIndexIntermediate", false),
    ("RightMiddleProximal", "RightHand", false),
    ("RightMiddleIntermediate", "RightMiddleProximal", false),
    ("RightMiddleDistal", "RightMiddleIntermediate", false),
    ("RightRingProximal", "RightHand", false),
    ("RightRingIntermediate", "RightRingProximal", false),
    ("RightRingDistal", "RightRingIntermediate", false),
    ("RightLittleProximal", "RightHand", false),
    ("RightLittleIntermediate", "RightLittleProximal", false),
    ("RightLittleDistal", "RightLittleIntermediate", false),
    ("LeftUpperLeg", "Hips", true),
    ("LeftLowerLeg", "LeftUpperLeg", true),
    ("LeftFoot", "LeftLowerLeg", true),
    ("LeftToes", "LeftFoot", false),
    ("RightUpperLeg", "Hips", true),
    ("RightLowerLeg", "RightUpperLeg", true),
    ("RightFoot", "RightLowerLeg", true),
    ("RightToes", "RightFoot", false),
];

impl SkeletonProfile {
    /// Builds a profile from an ordered list of canonical bone names.
    pub fn new<I, S>(bone_names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            bone_names: bone_names.into_iter().map(Into::into).collect(),
            // ⚠️ A name-only profile has no parents, no required flags and no
            // root/scale bones — `..Default::default()` keeps this constructor
            // working unchanged and makes the humanoid preset the only source
            // of that richer data (pat-s3rrv.222).
            ..Default::default()
        }
    }

    /// The canonical bone names, in profile order.
    pub fn bone_names(&self) -> &[String] {
        &self.bone_names
    }

    /// `SkeletonProfile.get_bone_size()`.
    pub fn bone_size(&self) -> usize {
        self.bone_names.len()
    }

    /// Godot's `SkeletonProfileHumanoid` — the 56-bone canonical rig
    /// (pat-s3rrv.222).
    ///
    /// ⚠️ `root_bone` is **`Root`**, not `Hips`. `Hips` is the
    /// `scale_base_bone`. The two are different fields in Godot and conflating
    /// them puts the retarget origin one bone too low.
    pub fn humanoid() -> Self {
        Self {
            bone_names: HUMANOID_BONES.iter().map(|b| b.0.to_string()).collect(),
            bone_parents: HUMANOID_BONES.iter().map(|b| b.1.to_string()).collect(),
            required: HUMANOID_BONES.iter().map(|b| b.2).collect(),
            root_bone: "Root".to_string(),
            scale_base_bone: "Hips".to_string(),
        }
    }

    /// `SkeletonProfile.find_bone(name)` — the profile INDEX, or `None`.
    pub fn find_bone(&self, name: &str) -> Option<usize> {
        self.bone_names.iter().position(|n| n == name)
    }

    /// `SkeletonProfile.get_bone_name(idx)`.
    pub fn bone_name(&self, idx: usize) -> Option<&str> {
        self.bone_names.get(idx).map(|s| s.as_str())
    }

    /// `SkeletonProfile.get_bone_parent(idx)` — `""` for a root bone.
    pub fn bone_parent(&self, idx: usize) -> Option<&str> {
        self.bone_parents.get(idx).map(|s| s.as_str())
    }

    /// `SkeletonProfile.is_required(idx)`.
    ///
    /// A profile built from bare names has no required flags, so this answers
    /// `false` rather than pretending every bone is mandatory.
    pub fn is_required(&self, idx: usize) -> bool {
        self.required.get(idx).copied().unwrap_or(false)
    }

    /// `SkeletonProfile.root_bone`.
    pub fn root_bone(&self) -> &str {
        &self.root_bone
    }

    /// `SkeletonProfile.scale_base_bone`.
    pub fn scale_base_bone(&self) -> &str {
        &self.scale_base_bone
    }
}

/// Maps each of a profile's canonical bones to a bone of some concrete skeleton.
///
/// 🔑 **The key set is fixed by the profile.** Godot seeds one entry per profile
/// bone and both accessors `ERR_FAIL` on a name that is not already a key
/// (`bone_map.cpp:83`, `:88`) — so a `BoneMap` can never name a bone the profile
/// does not define.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoneMap {
    profile: SkeletonProfile,
    /// Profile bone name → skeleton bone name. Empty value = unmapped.
    ///
    /// ⚠️ Ordered (`BTreeMap`) on purpose — see [`BoneMap::find_profile_bone_name`].
    map: BTreeMap<String, String>,
}

impl BoneMap {
    /// Builds a map for `profile`, with every profile bone present and unmapped.
    pub fn with_profile(profile: SkeletonProfile) -> Self {
        let map = profile
            .bone_names()
            .iter()
            .map(|n| (n.clone(), String::new()))
            .collect();
        Self { profile, map }
    }

    /// The profile whose bones this map is keyed by.
    pub fn profile(&self) -> &SkeletonProfile {
        &self.profile
    }

    /// `BoneMap.get_skeleton_bone_name(profile_bone_name)`.
    ///
    /// `None` when the name is not one of the profile's bones; `Some("")` when
    /// it is a profile bone that has not been mapped yet. ⚠️ Those two are
    /// different states and collapsing them would hide a typo'd profile bone as
    /// an ordinary unmapped one.
    pub fn get_skeleton_bone_name(&self, profile_bone_name: &str) -> Option<&str> {
        self.map.get(profile_bone_name).map(|s| s.as_str())
    }

    /// `BoneMap.set_skeleton_bone_name(profile_bone_name, skeleton_bone_name)`.
    ///
    /// Returns whether the assignment was accepted.
    ///
    /// 🔑 **A name that is not already a profile bone is REFUSED, not inserted**
    /// — Godot's `_set_skeleton_bone_name` opens with
    /// `ERR_FAIL_COND(!bone_map.has(p_profile_bone_name))` (`bone_map.cpp:88`).
    /// The obvious `map.insert(k, v)` would silently grow the map with bones the
    /// profile never declared, and the map would then claim to retarget a rig
    /// role that does not exist.
    pub fn set_skeleton_bone_name(
        &mut self,
        profile_bone_name: &str,
        skeleton_bone_name: impl Into<String>,
    ) -> bool {
        match self.map.get_mut(profile_bone_name) {
            Some(slot) => {
                *slot = skeleton_bone_name.into();
                true
            }
            None => false,
        }
    }

    /// `BoneMap.find_profile_bone_name(skeleton_bone_name)` — the reverse
    /// lookup.
    ///
    /// ⚠️ Godot scans a `HashMap` and returns the first match
    /// (`bone_map.cpp:97-108`), so when two profile bones map to the SAME
    /// skeleton bone its answer is **unordered** — an editor warning case rather
    /// than a defined one. This walks profile order instead, so the answer is
    /// deterministic; that is a deliberate strengthening, not a parity claim.
    pub fn find_profile_bone_name(&self, skeleton_bone_name: &str) -> Option<&str> {
        if skeleton_bone_name.is_empty() {
            // Every unmapped bone holds "", so an empty query would match the
            // first unmapped entry and read as a real hit.
            return None;
        }
        self.profile
            .bone_names()
            .iter()
            .find(|name| {
                self.map
                    .get(name.as_str())
                    .is_some_and(|v| v == skeleton_bone_name)
            })
            .map(|s| s.as_str())
    }

    /// `BoneMap.get_skeleton_bone_name_count(skeleton_bone_name)` — how many
    /// profile bones claim this skeleton bone.
    ///
    /// Godot exposes this so the editor can flag a duplicate mapping; anything
    /// above 1 is a rig the user still has to disambiguate.
    pub fn skeleton_bone_name_count(&self, skeleton_bone_name: &str) -> usize {
        if skeleton_bone_name.is_empty() {
            return 0;
        }
        self.map
            .values()
            .filter(|v| v.as_str() == skeleton_bone_name)
            .count()
    }
}

/// Applies `pose` to whichever bone of `skeleton` plays `profile_bone_name`
/// (pat-s3rrv.222).
///
/// This is the retargeting step: the caller speaks in the profile's canonical
/// vocabulary ("LeftUpperArm") and the map resolves it to whatever the source
/// rig happens to call that bone ("arm_L", "mixamorig:LeftArm", ...). That
/// indirection is the entire point — an animation authored against the profile
/// replays on a rig that shares none of its bone names.
///
/// Returns `false` when the profile bone is unmapped, or when the rig has no
/// bone under the mapped name. ⚠️ Both are *silent* in Godot; returning the
/// outcome here lets a caller tell "retargeted" from "quietly did nothing",
/// which is the failure a mis-typed map produces.
pub fn retarget_pose(
    tree: &mut SceneTree,
    skeleton: NodeId,
    map: &BoneMap,
    profile_bone_name: &str,
    pose: Transform3D,
) -> bool {
    let Some(skeleton_bone) = map.get_skeleton_bone_name(profile_bone_name) else {
        return false;
    };
    if skeleton_bone.is_empty() {
        return false;
    }
    let Some(idx) = crate::skeleton3d::find_bone(tree, skeleton, skeleton_bone) else {
        return false;
    };
    crate::skeleton3d::set_bone_pose(tree, skeleton, idx, pose);
    true
}

/// Retargets a whole set of `(profile bone, pose)` pairs, returning how many
/// landed.
///
/// Godot's `RetargetModifier3D` drives every mapped bone each frame; this is the
/// batch form of [`retarget_pose`]. A count rather than a bool because a partial
/// map is normal — a rig without fingers still retargets its limbs.
pub fn retarget_poses(
    tree: &mut SceneTree,
    skeleton: NodeId,
    map: &BoneMap,
    poses: &[(&str, Transform3D)],
) -> usize {
    let mut applied = 0;
    for (name, pose) in poses {
        if retarget_pose(tree, skeleton, map, name, *pose) {
            applied += 1;
        }
    }
    applied
}
