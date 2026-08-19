//! pat-s3rrv.57 (merged finding 3, prerequisite): registering a class twice
//! must ACCUMULATE, not let whichever pass ran first win permanently.
//!
//! Every registration pass is guarded with `if !class_exists(name)` — 472 of
//! them in `class_db.rs`. That made registration order decide the outcome: a
//! bare `name + parent` entry in an early pass silently beat a later one
//! carrying the class's real properties, methods and signals, which is the
//! mechanism behind "86 classes carry zero properties". Worse, it meant the
//! bulk metadata work this bead's remaining scope calls for would **no-op**
//! rather than land.
//!
//! Merging fixes it at the root: order stops mattering. These tests use class
//! names no real registration uses, so they neither depend on nor disturb the
//! rest of the registry.

use gdobject::class_db::{
    get_class_info, register_class, ClassRegistration, EnumInfo, MethodInfo, PropertyInfo,
    SignalInfo,
};
use gdvariant::Variant;

/// **The acceptance case.** A bare registration first, a rich one second — the
/// order that used to lose everything in the second one.
#[test]
fn a_later_rich_registration_enriches_a_bare_one() {
    let name = "MergeProbeBareThenRich";

    let first_id = register_class(ClassRegistration::new(name).parent("Node"));
    let bare = get_class_info(name).expect("registered");
    assert!(
        bare.properties.is_empty(),
        "the bare pass carries no members"
    );

    let second_id = register_class(
        ClassRegistration::new(name)
            .parent("Node")
            .property(PropertyInfo::new("speed", Variant::Float(200.0)))
            .method(MethodInfo::new("accelerate", 1))
            .signal(SignalInfo::new("stopped"))
            .constant("MODE_FAST", 7)
            .enumeration(EnumInfo::new("Mode").constant("MODE_FAST", 7)),
    );

    // ⚠️ The id must survive. Ids are handed to callers and stored on objects,
    // so minting a new one on re-registration would orphan every holder.
    assert_eq!(
        first_id, second_id,
        "re-registering a class must keep its ClassId"
    );

    let merged = get_class_info(name).expect("still registered");
    assert_eq!(merged.class_id, first_id);
    assert_eq!(merged.parent_class, "Node");
    assert_eq!(
        merged.properties.len(),
        1,
        "the later pass's property must land, not be discarded"
    );
    assert_eq!(merged.properties[0].name, "speed");
    assert_eq!(merged.methods.len(), 1);
    assert_eq!(merged.methods[0].name, "accelerate");
    assert_eq!(merged.signals.len(), 1);
    assert_eq!(merged.signals[0].name, "stopped");
    assert_eq!(merged.integer_constants, vec![("MODE_FAST".to_string(), 7)]);
    assert_eq!(merged.enums.len(), 1);
    assert_eq!(merged.enums[0].name, "Mode");
}

/// The other order must lose nothing either — a bare pass arriving second is
/// exactly as common, and it must not blank the entry.
#[test]
fn a_later_bare_registration_loses_nothing() {
    let name = "MergeProbeRichThenBare";

    register_class(
        ClassRegistration::new(name)
            .parent("Node2D")
            .property(PropertyInfo::new("speed", Variant::Float(200.0)))
            .method(MethodInfo::new("accelerate", 1)),
    );
    register_class(ClassRegistration::new(name));

    let merged = get_class_info(name).expect("registered");
    assert_eq!(
        merged.parent_class, "Node2D",
        "a real parent must never be overwritten by an empty one — that would \
         sever every is_parent_class chain running through it"
    );
    assert_eq!(merged.properties.len(), 1, "the rich pass's data survives");
    assert_eq!(merged.methods.len(), 1);
}

/// A member registered twice must appear ONCE.
///
/// `get_property_list` walks the inheritance chain and concatenates without
/// deduping, so a duplicate stored here shows up as a doubled row in the
/// inspector and in script reflection.
#[test]
fn duplicate_members_are_not_appended_twice() {
    let name = "MergeProbeDuplicates";

    for _ in 0..3 {
        register_class(
            ClassRegistration::new(name)
                .parent("Node")
                .property(PropertyInfo::new("speed", Variant::Float(1.0)))
                .method(MethodInfo::new("go", 0))
                .signal(SignalInfo::new("went"))
                .constant("K", 1),
        );
    }

    let info = get_class_info(name).expect("registered");
    assert_eq!(info.properties.len(), 1, "one property, not three");
    assert_eq!(info.methods.len(), 1);
    assert_eq!(info.signals.len(), 1);
    assert_eq!(info.integer_constants.len(), 1);

    // First-write-wins per name: the original default is kept rather than being
    // replaced by a later pass that may know less about it.
    assert_eq!(info.properties[0].default_value, Variant::Float(1.0));
}

/// `is_abstract` is sticky-true, and an enum can be upgraded to a bitfield by a
/// later pass.
///
/// Whether a class is abstract is fixed in Godot; the realistic disagreement is
/// a bare registration omitting the flag, so the pass that knows must win —
/// whichever order it arrives in.
#[test]
fn abstractness_and_bitfield_flags_are_sticky() {
    let concrete_first = "MergeProbeAbstractSecond";
    register_class(ClassRegistration::new(concrete_first).parent("Node"));
    register_class(ClassRegistration::new(concrete_first).abstract_class());
    assert!(
        get_class_info(concrete_first)
            .expect("registered")
            .is_abstract,
        "a later pass that knows the class is abstract must win"
    );

    let abstract_first = "MergeProbeAbstractFirst";
    register_class(ClassRegistration::new(abstract_first).abstract_class());
    register_class(ClassRegistration::new(abstract_first).parent("Node"));
    assert!(
        get_class_info(abstract_first)
            .expect("registered")
            .is_abstract,
        "and a later bare pass must not un-abstract it"
    );

    let flags = "MergeProbeBitfield";
    register_class(
        ClassRegistration::new(flags).enumeration(EnumInfo::new("Flags").constant("FLAG_A", 1)),
    );
    register_class(
        ClassRegistration::new(flags)
            .enumeration(EnumInfo::new("Flags").constant("FLAG_B", 2).bitfield()),
    );

    let info = get_class_info(flags).expect("registered");
    assert_eq!(info.enums.len(), 1, "the two passes describe ONE enum");
    let e = &info.enums[0];
    assert!(
        e.is_bitfield,
        "a later pass may reveal the enum is a bitfield"
    );
    assert_eq!(
        e.constants,
        vec![("FLAG_A".to_string(), 1), ("FLAG_B".to_string(), 2)],
        "constants from both passes, in first-seen order"
    );
}
