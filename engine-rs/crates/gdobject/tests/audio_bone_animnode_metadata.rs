//! pat-s3rrv.57 (merged finding 3): AudioEffectReverb (8), BoneAttachment3D (5)
//! and AnimationNodeAnimation (4 of 8).
//!
//! AudioEffectReverb and AnimationNodeAnimation are Resources, so registering
//! them cannot affect the runtime. BoneAttachment3D is a Node, and its one
//! runtime-read member (`bone_idx`, `skeleton3d.rs:522`) already falls back to
//! -1 — exactly Godot's default — so seeding it changes nothing.

use gdobject::class_db::{get_class_info, register_all_classes};
use gdvariant::Variant;

fn own(class: &str, name: &str) -> Variant {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`"))
        .default_value
}

fn own_count(class: &str) -> usize {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .len()
}

/// ⚠️ Seven of the reverb's eight defaults are non-zero, and every one is
/// destructive at zero: `room_size` 0 with `wet` 0 and `dry` 0 is an effect that
/// does nothing audible, which reads as "reverb is broken" rather than as a
/// wrong constant.
#[test]
fn audio_effect_reverb_carries_its_eight_members() {
    assert_eq!(own_count("AudioEffectReverb"), 8);

    assert_eq!(own("AudioEffectReverb", "room_size"), Variant::Float(0.8));
    assert_eq!(own("AudioEffectReverb", "dry"), Variant::Float(1.0));
    assert_eq!(own("AudioEffectReverb", "wet"), Variant::Float(0.5));
    assert_eq!(own("AudioEffectReverb", "spread"), Variant::Float(1.0));
    // Three orders of magnitude from the "obvious" zero.
    assert_eq!(
        own("AudioEffectReverb", "predelay_msec"),
        Variant::Float(150.0)
    );
    // The one that genuinely is zero — asserted so the others cannot be
    // dismissed as "everything is non-zero here".
    assert_eq!(own("AudioEffectReverb", "hipass"), Variant::Float(0.0));
}

/// ⚠️ `bone_idx` is -1 ("no bone"), not 0. A zero would silently attach every
/// unconfigured BoneAttachment3D to the skeleton's FIRST bone — a wrong result
/// that looks like a working feature.
#[test]
fn bone_attachment_defaults_to_no_bone() {
    assert_eq!(own_count("BoneAttachment3D"), 5);
    assert_eq!(own("BoneAttachment3D", "bone_idx"), Variant::Int(-1));
    assert_eq!(
        own("BoneAttachment3D", "bone_name"),
        Variant::String(String::new())
    );
    assert_eq!(
        own("BoneAttachment3D", "use_external_skeleton"),
        Variant::Bool(false)
    );
}

/// ⚠️ 4 of Godot's 8: `loop_mode`, `start_offset`, `stretch_time_scale` and
/// `timeline_length` state no default in the XML, so they are omitted rather
/// than invented — the same rule that left ten BaseMaterial3D members out.
#[test]
fn animation_node_animation_carries_its_stated_members() {
    assert_eq!(own_count("AnimationNodeAnimation"), 4);

    // The clip name is a StringName, not a String — `typeof()` distinguishes
    // them and a script comparing against a StringName would fail on a String.
    assert!(matches!(
        own("AnimationNodeAnimation", "animation"),
        Variant::StringName(_)
    ));
    assert_eq!(own("AnimationNodeAnimation", "play_mode"), Variant::Int(0));
    assert_eq!(
        own("AnimationNodeAnimation", "advance_on_start"),
        Variant::Bool(false)
    );
}
