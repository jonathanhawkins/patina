//! XR skeleton modifiers — the joint-to-bone mapping `XRHandModifier3D` drives
//! a `Skeleton3D` through (pat-s3rrv.263).
//!
//! Godot ground truth: `upstream/godot/scene/3d/xr/xr_hand_modifier_3d.cpp`
//! (the `bone_names` table and `bone_name_format`) and
//! `doc/classes/XRHandTracker.xml` (the `HandJoint` and `HandJointFlags`
//! enums).
//!
//! # 🔑 The bone names are NOT derivable from the joint enum names
//!
//! **24 of the 26** differ, and two of the differences are not cosmetic:
//!
//! | joint constant | bone name |
//! |---|---|
//! | `HAND_JOINT_WRIST` | **`Hand`** — not `Wrist` |
//! | `HAND_JOINT_PINKY_FINGER_*` | **`Little*`** — not `Pinky*` |
//! | `HAND_JOINT_THUMB_PHALANX_PROXIMAL` | `ThumbProximal` — `PHALANX` is dropped |
//! | `HAND_JOINT_INDEX_FINGER_METACARPAL` | `IndexMetacarpal` — `FINGER` is dropped |
//!
//! ⚠️ A derived name fails **silently**: Godot's `find_bone` returns -1, the
//! joint is skipped with a warning, and the skeleton simply does not move. That
//! is indistinguishable from "the tracker had no data", which is why this table
//! is transcribed verbatim and pinned by tests rather than generated from the
//! constant names.
//!
//! ⚠️ `HAND_JOINT_WRIST` and `HAND_JOINT_INDEX_FINGER_TIP` are precisely the two
//! joints the bead's acceptance drives, and both are in the mismatched set.

/// `XRHandTracker.HAND_JOINT_MAX` — the joint count, from
/// `doc/classes/XRHandTracker.xml`.
pub const HAND_JOINT_COUNT: usize = 26;

/// Bone name suffixes per `HandJoint`, indexed by the enum's own value.
///
/// Transcribed verbatim from `bone_names[]` in `xr_hand_modifier_3d.cpp`. The
/// order IS the `HandJoint` enum order: index 0 is `HAND_JOINT_PALM`, 1 is
/// `HAND_JOINT_WRIST`, and 25 is `HAND_JOINT_PINKY_FINGER_TIP`.
pub const HAND_JOINT_BONE_NAMES: [&str; HAND_JOINT_COUNT] = [
    "Palm",               // 0  HAND_JOINT_PALM
    "Hand",               // 1  HAND_JOINT_WRIST                 ⚠️ not "Wrist"
    "ThumbMetacarpal",    // 2  HAND_JOINT_THUMB_METACARPAL
    "ThumbProximal",      // 3  HAND_JOINT_THUMB_PHALANX_PROXIMAL
    "ThumbDistal",        // 4  HAND_JOINT_THUMB_PHALANX_DISTAL
    "ThumbTip",           // 5  HAND_JOINT_THUMB_TIP
    "IndexMetacarpal",    // 6  HAND_JOINT_INDEX_FINGER_METACARPAL
    "IndexProximal",      // 7  HAND_JOINT_INDEX_FINGER_PHALANX_PROXIMAL
    "IndexIntermediate",  // 8  HAND_JOINT_INDEX_FINGER_PHALANX_INTERMEDIATE
    "IndexDistal",        // 9  HAND_JOINT_INDEX_FINGER_PHALANX_DISTAL
    "IndexTip",           // 10 HAND_JOINT_INDEX_FINGER_TIP
    "MiddleMetacarpal",   // 11 HAND_JOINT_MIDDLE_FINGER_METACARPAL
    "MiddleProximal",     // 12 HAND_JOINT_MIDDLE_FINGER_PHALANX_PROXIMAL
    "MiddleIntermediate", // 13 HAND_JOINT_MIDDLE_FINGER_PHALANX_INTERMEDIATE
    "MiddleDistal",       // 14 HAND_JOINT_MIDDLE_FINGER_PHALANX_DISTAL
    "MiddleTip",          // 15 HAND_JOINT_MIDDLE_FINGER_TIP
    "RingMetacarpal",     // 16 HAND_JOINT_RING_FINGER_METACARPAL
    "RingProximal",       // 17 HAND_JOINT_RING_FINGER_PHALANX_PROXIMAL
    "RingIntermediate",   // 18 HAND_JOINT_RING_FINGER_PHALANX_INTERMEDIATE
    "RingDistal",         // 19 HAND_JOINT_RING_FINGER_PHALANX_DISTAL
    "RingTip",            // 20 HAND_JOINT_RING_FINGER_TIP
    "LittleMetacarpal",   // 21 HAND_JOINT_PINKY_FINGER_METACARPAL  ⚠️ "Little"
    "LittleProximal",     // 22 HAND_JOINT_PINKY_FINGER_PHALANX_PROXIMAL
    "LittleIntermediate", // 23 HAND_JOINT_PINKY_FINGER_PHALANX_INTERMEDIATE
    "LittleDistal",       // 24 HAND_JOINT_PINKY_FINGER_PHALANX_DISTAL
    "LittleTip",          // 25 HAND_JOINT_PINKY_FINGER_TIP
];

/// `XRPositionalTracker.TrackerHand`, from `xr_positional_tracker.cpp`'s
/// `_bind_methods` (`TRACKER_HAND_UNKNOWN`, `_LEFT`, `_RIGHT`, `_MAX`).
pub const TRACKER_HAND_UNKNOWN: i64 = 0;
/// See [`TRACKER_HAND_UNKNOWN`].
pub const TRACKER_HAND_LEFT: i64 = 1;
/// See [`TRACKER_HAND_UNKNOWN`].
pub const TRACKER_HAND_RIGHT: i64 = 2;

/// `XRHandTracker.HandJointFlags`, from `doc/classes/XRHandTracker.xml`.
///
/// ⚠️ These are a **bitmask**, and VALID is a different bit from TRACKED:
/// `ORIENTATION_VALID` is 1 while `ORIENTATION_TRACKED` is 2. Godot gates on the
/// VALID bits, so testing the TRACKED ones would skip joints that carry usable
/// data.
pub const HAND_JOINT_FLAG_ORIENTATION_VALID: i64 = 1;
/// See [`HAND_JOINT_FLAG_ORIENTATION_VALID`].
pub const HAND_JOINT_FLAG_ORIENTATION_TRACKED: i64 = 2;
/// See [`HAND_JOINT_FLAG_ORIENTATION_VALID`].
pub const HAND_JOINT_FLAG_POSITION_VALID: i64 = 4;
/// See [`HAND_JOINT_FLAG_ORIENTATION_VALID`].
pub const HAND_JOINT_FLAG_POSITION_TRACKED: i64 = 8;

/// The `Skeleton3D` bone a hand joint drives, e.g. `(1, LEFT) -> "LeftHand"`.
///
/// `None` when the joint index is out of range, or when the tracker's hand is
/// neither left nor right — Godot returns early for an `UNKNOWN` hand rather
/// than guessing a side, so an unconfigured tracker moves nothing.
pub fn hand_joint_bone_name(joint: usize, tracker_hand: i64) -> Option<String> {
    let suffix = HAND_JOINT_BONE_NAMES.get(joint)?;
    let prefix = match tracker_hand {
        TRACKER_HAND_LEFT => "Left",
        TRACKER_HAND_RIGHT => "Right",
        _ => return None,
    };
    Some(format!("{prefix}{suffix}"))
}

/// Whether a joint's flags say its transform is usable.
///
/// Godot skips joints whose `HAND_JOINT_FLAG_*_VALID` bits are clear. Both
/// position and orientation must be valid for a full pose to be written.
pub fn hand_joint_pose_is_valid(flags: i64) -> bool {
    flags & HAND_JOINT_FLAG_POSITION_VALID != 0 && flags & HAND_JOINT_FLAG_ORIENTATION_VALID != 0
}
