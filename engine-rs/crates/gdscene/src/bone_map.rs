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

use std::collections::BTreeMap;

/// An ordered set of canonical bone names.
///
/// ⓘ Godot's `SkeletonProfile` also carries per-bone groups, reference poses,
/// tails and handle offsets — those are editor-facing and are not modelled here
/// yet. The bone *names* are what `BoneMap` keys on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkeletonProfile {
    bone_names: Vec<String>,
}

impl SkeletonProfile {
    /// Builds a profile from an ordered list of canonical bone names.
    pub fn new<I, S>(bone_names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            bone_names: bone_names.into_iter().map(Into::into).collect(),
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
