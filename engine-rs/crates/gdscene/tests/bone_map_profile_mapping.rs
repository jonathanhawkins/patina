//! pat-s3rrv.134 — `SkeletonProfile` / `BoneMap`, the retargeting resources.
//!
//! Neither type existed anywhere in the workspace — not merely unregistered,
//! unimplemented. They are the prerequisite for `RetargetModifier3D`, which
//! remaps one rig's poses onto another through a profile's canonical bone names.
//!
//! Semantics ported from `upstream/godot/scene/resources/bone_map.cpp`.

use gdscene::bone_map::{BoneMap, SkeletonProfile};

fn humanoid_ish() -> BoneMap {
    BoneMap::with_profile(SkeletonProfile::new(["Hips", "Spine", "Head"]))
}

/// Every profile bone starts present and unmapped.
#[test]
fn a_new_map_has_one_entry_per_profile_bone() {
    let map = humanoid_ish();
    assert_eq!(map.profile().bone_size(), 3);
    for bone in ["Hips", "Spine", "Head"] {
        assert_eq!(
            map.get_skeleton_bone_name(bone),
            Some(""),
            "{bone} must be present and unmapped"
        );
    }
}

/// 🔑 **The key set is fixed by the profile.** Assigning to a name the profile
/// never declared is REFUSED, not inserted (`bone_map.cpp:88`).
///
/// ⚠️ The obvious `map.insert(k, v)` accepts it and silently grows the map with
/// a bone role that does not exist — after which the map claims to retarget
/// something the profile has no concept of. This is the assertion that fails for
/// that version; every other test here still passes.
#[test]
fn assigning_to_a_non_profile_bone_is_refused() {
    let mut map = humanoid_ish();

    assert!(
        !map.set_skeleton_bone_name("Tail", "DeformTail"),
        "a bone outside the profile must be rejected"
    );
    assert_eq!(
        map.get_skeleton_bone_name("Tail"),
        None,
        "and must NOT have been created as a key"
    );
    assert_eq!(map.profile().bone_size(), 3, "the profile is unchanged");
}

/// ⚠️ "Unknown bone" and "known but unmapped" are different states: `None` vs
/// `Some("")`. Collapsing them would hide a typo'd profile bone as an ordinary
/// unmapped one — the map would look merely incomplete rather than wrong.
#[test]
fn unknown_and_unmapped_are_distinguishable() {
    let map = humanoid_ish();
    assert_eq!(map.get_skeleton_bone_name("Spine"), Some(""));
    assert_eq!(map.get_skeleton_bone_name("NoSuchBone"), None);
}

/// The ordinary round trip, plus the reverse lookup `RetargetModifier3D` needs
/// to answer "which canonical role does this rig bone play?".
#[test]
fn a_mapping_round_trips_and_reverses() {
    let mut map = humanoid_ish();
    assert!(map.set_skeleton_bone_name("Hips", "mixamorig:Hips"));

    assert_eq!(map.get_skeleton_bone_name("Hips"), Some("mixamorig:Hips"));
    assert_eq!(map.find_profile_bone_name("mixamorig:Hips"), Some("Hips"));
    assert_eq!(
        map.find_profile_bone_name("mixamorig:LeftFoot"),
        None,
        "an unmapped skeleton bone has no profile role"
    );
}

/// ⚠️ An empty query must not match the unmapped entries, which all hold `""`.
/// Without that guard the reverse lookup reports the first unmapped profile bone
/// as a genuine hit for every bone the rig does not have.
#[test]
fn an_empty_query_does_not_match_unmapped_entries() {
    let map = humanoid_ish();
    assert_eq!(map.find_profile_bone_name(""), None);
    assert_eq!(map.skeleton_bone_name_count(""), 0);
}

/// Godot exposes the duplicate count so the editor can flag a rig the user still
/// has to disambiguate — two canonical roles pointing at one real bone.
#[test]
fn duplicate_mappings_are_countable() {
    let mut map = humanoid_ish();
    map.set_skeleton_bone_name("Spine", "Bone_01");
    map.set_skeleton_bone_name("Head", "Bone_01");

    assert_eq!(map.skeleton_bone_name_count("Bone_01"), 2);
    assert_eq!(map.skeleton_bone_name_count("Bone_02"), 0);

    // ⓘ Which of the two the reverse lookup returns is UNORDERED in Godot (it
    // scans a HashMap). This implementation walks profile order, so it is
    // deterministic — asserted as profile order, not as Godot parity.
    assert_eq!(map.find_profile_bone_name("Bone_01"), Some("Spine"));
}
