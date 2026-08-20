//! pat-s3rrv.263 — the `XRHandModifier3D` joint-to-bone name table.
//!
//! Godot ground truth: the `bone_names[]` table and `bone_name_format[2]` in
//! `upstream/godot/scene/3d/xr/xr_hand_modifier_3d.cpp`, and the `HandJoint` /
//! `HandJointFlags` enums in `doc/classes/XRHandTracker.xml`.
//!
//! # Why this table gets its own tests
//!
//! 🔑 **24 of the 26 bone names differ from their joint constant's name**, and a
//! wrong one fails *silently*: Godot's `find_bone` returns -1, the joint is
//! skipped, and the skeleton simply does not move — indistinguishable from "the
//! tracker sent nothing". There is no error to follow back to the typo.
//!
//! The two the bead's acceptance drives are both in the mismatched set:
//! `HAND_JOINT_WRIST` maps to **`Hand`** and `HAND_JOINT_INDEX_FINGER_TIP` to
//! `IndexTip`. So an implementation that derived names from the constants would
//! fail that acceptance while looking entirely reasonable.
//!
//! ⚠️ An integration target rather than in-crate unit tests: gating a bead on
//! `--lib` means an unrelated red test cancels the run before yours executes
//! (pat-s3rrv.235).

use gdscene::xr_modifiers::{
    hand_joint_bone_name, hand_joint_pose_is_valid, HAND_JOINT_BONE_NAMES, HAND_JOINT_COUNT,
    HAND_JOINT_FLAG_ORIENTATION_TRACKED, HAND_JOINT_FLAG_ORIENTATION_VALID,
    HAND_JOINT_FLAG_POSITION_TRACKED, HAND_JOINT_FLAG_POSITION_VALID, TRACKER_HAND_LEFT,
    TRACKER_HAND_RIGHT, TRACKER_HAND_UNKNOWN,
};

/// `HandJoint` values the tests name, from `doc/classes/XRHandTracker.xml`.
const HAND_JOINT_PALM: usize = 0;
const HAND_JOINT_WRIST: usize = 1;
const HAND_JOINT_THUMB_METACARPAL: usize = 2;
const HAND_JOINT_INDEX_FINGER_TIP: usize = 10;
const HAND_JOINT_PINKY_FINGER_METACARPAL: usize = 21;
const HAND_JOINT_PINKY_FINGER_TIP: usize = 25;

/// 🔑 **The acceptance's two joints**, which are exactly the ones a derived
/// name gets wrong.
#[test]
fn a_the_acceptance_joints_map_to_godots_bone_names() {
    assert_eq!(
        hand_joint_bone_name(HAND_JOINT_WRIST, TRACKER_HAND_LEFT).as_deref(),
        Some("LeftHand"),
        "HAND_JOINT_WRIST drives the bone named \"Hand\", NOT \"Wrist\" — \
         deriving the name from the constant finds no bone and the skeleton \
         silently does not move"
    );
    assert_eq!(
        hand_joint_bone_name(HAND_JOINT_INDEX_FINGER_TIP, TRACKER_HAND_LEFT).as_deref(),
        Some("LeftIndexTip"),
        "the bone name drops FINGER"
    );
}

/// The whole table, spot-checked at every kind of divergence.
#[test]
fn b_the_table_matches_godots_bone_names() {
    assert_eq!(HAND_JOINT_COUNT, 26, "HAND_JOINT_MAX");
    assert_eq!(HAND_JOINT_BONE_NAMES.len(), 26);

    // The three that DO match their constant, so the test is not just asserting
    // "everything is renamed".
    assert_eq!(HAND_JOINT_BONE_NAMES[HAND_JOINT_PALM], "Palm");
    assert_eq!(
        HAND_JOINT_BONE_NAMES[HAND_JOINT_THUMB_METACARPAL],
        "ThumbMetacarpal"
    );

    // 🔑 PINKY -> Little. The entire little-finger run is renamed, so an
    // implementation that got only the first one right still breaks four bones.
    assert_eq!(
        HAND_JOINT_BONE_NAMES[HAND_JOINT_PINKY_FINGER_METACARPAL],
        "LittleMetacarpal"
    );
    assert_eq!(
        HAND_JOINT_BONE_NAMES[HAND_JOINT_PINKY_FINGER_TIP],
        "LittleTip"
    );
    assert!(
        !HAND_JOINT_BONE_NAMES.iter().any(|n| n.contains("Pinky")),
        "no bone name uses \"Pinky\" — Godot's rig calls that finger \"Little\""
    );
    assert!(
        !HAND_JOINT_BONE_NAMES.iter().any(|n| n.contains("Wrist")),
        "no bone name uses \"Wrist\" — the wrist joint drives \"Hand\""
    );
    assert!(
        !HAND_JOINT_BONE_NAMES.iter().any(|n| n.contains("Phalanx")),
        "PHALANX is dropped from every bone name"
    );

    // Every entry is distinct: a copy-paste slip in a 26-line table would
    // otherwise leave two joints fighting over one bone.
    let mut sorted: Vec<&str> = HAND_JOINT_BONE_NAMES.to_vec();
    sorted.sort_unstable();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(before, sorted.len(), "duplicate bone name in the table");
}

/// Both hands, and the refusal to guess a side.
#[test]
fn c_hand_prefix_and_the_unknown_hand() {
    assert_eq!(
        hand_joint_bone_name(HAND_JOINT_WRIST, TRACKER_HAND_RIGHT).as_deref(),
        Some("RightHand")
    );
    assert_eq!(
        hand_joint_bone_name(HAND_JOINT_PALM, TRACKER_HAND_RIGHT).as_deref(),
        Some("RightPalm")
    );

    // ⚠️ Godot returns early unless the tracker reports LEFT or RIGHT, rather
    // than defaulting to one. Guessing "Left" would drive the wrong hand's
    // bones from an unconfigured tracker — visibly wrong and hard to trace.
    assert_eq!(
        hand_joint_bone_name(HAND_JOINT_WRIST, TRACKER_HAND_UNKNOWN),
        None,
        "an UNKNOWN hand must map to no bone at all"
    );
    assert_eq!(hand_joint_bone_name(HAND_JOINT_WRIST, 99), None);

    // Out of range is None, not a panic — the joint index arrives from script.
    assert_eq!(
        hand_joint_bone_name(HAND_JOINT_COUNT, TRACKER_HAND_LEFT),
        None
    );
    assert_eq!(hand_joint_bone_name(999, TRACKER_HAND_LEFT), None);
}

/// 🔑 The flags are a bitmask, and VALID is a different bit from TRACKED.
///
/// ⚠️ `ORIENTATION_VALID` is 1 while `ORIENTATION_TRACKED` is 2. Godot gates on
/// the VALID bits; gating on TRACKED instead would drop joints that carry
/// perfectly usable data, and the two are adjacent enough to swap by accident.
#[test]
fn d_pose_validity_reads_the_valid_bits_not_the_tracked_bits() {
    assert_eq!(HAND_JOINT_FLAG_ORIENTATION_VALID, 1);
    assert_eq!(HAND_JOINT_FLAG_ORIENTATION_TRACKED, 2);
    assert_eq!(HAND_JOINT_FLAG_POSITION_VALID, 4);
    assert_eq!(HAND_JOINT_FLAG_POSITION_TRACKED, 8);

    // The acceptance's combination: POSITION_VALID | ORIENTATION_VALID == 5.
    assert!(hand_joint_pose_is_valid(
        HAND_JOINT_FLAG_POSITION_VALID | HAND_JOINT_FLAG_ORIENTATION_VALID
    ));
    assert!(hand_joint_pose_is_valid(5));

    // Either bit missing means the joint is skipped.
    assert!(!hand_joint_pose_is_valid(HAND_JOINT_FLAG_POSITION_VALID));
    assert!(!hand_joint_pose_is_valid(HAND_JOINT_FLAG_ORIENTATION_VALID));
    assert!(!hand_joint_pose_is_valid(0));

    // 🔑 TRACKED alone is NOT valid — this is the swap the doc comment warns of.
    assert!(
        !hand_joint_pose_is_valid(
            HAND_JOINT_FLAG_POSITION_TRACKED | HAND_JOINT_FLAG_ORIENTATION_TRACKED
        ),
        "the TRACKED bits must not stand in for the VALID bits"
    );

    // Unrelated high bits set alongside the valid ones must not disturb it.
    assert!(hand_joint_pose_is_valid(5 | 16 | 32));
}
