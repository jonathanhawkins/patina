//! pat-mgixk.77 — the two pure queries the tree's mixer output needs:
//! `AnimationTree::current_track_types` and
//! `AnimationTree::non_property_track_paths`.
//!
//! `get_current_values_targeted` reports `(node_path, property, value)` and
//! erases the track type, so the scene-tree loop could not tell a method key
//! from a bone transform and wrote every sampled value as an ordinary property.
//! These two restore what it erased.
//!
//! ⚠️ Deliberately split out of
//! `animation_tree_bone_tracks_reach_the_skeleton.rs`. That file is the wiring
//! acceptance and must still COMPILE with the implementation reverted, so its
//! red check fails behaviourally. A test calling these new methods by name
//! cannot compile at HEAD, which would make the whole file's red check read
//! `Detected("does not compile")` — a pass earned by the signature rather than
//! by the behaviour.

use gdscene::animation::{Animation, AnimationTrack, KeyFrame, TrackType};
use gdscene::animation_tree::AnimationTree;
use gdvariant::Variant;

/// The skip-set and the type table, which the shared mixer-output path will
/// consume to stop writing non-property tracks as properties.
///
/// ⚠️ This is a unit test of two queries, not an acceptance test. The wiring
/// that consumes them — `process_animation_trees_in_mode` skipping method and
/// bone tracks in its property loop — is asserted through `MainLoop::step` in
/// `animation_tree_bone_tracks_reach_the_skeleton.rs`. This file exists because
/// the *discriminator* below cannot be expressed through the wiring.
///
/// 🔑 The discriminator is that a **Property** track's path must NOT appear in
/// the skip-set. A `non_property_track_paths` that returned every path would
/// satisfy any "the method track is skipped" assertion while silently stopping
/// the tree from animating anything at all.
#[test]
fn tree_reports_track_types_and_the_non_property_skip_set() {
    let mut anim = Animation::new("mixed", 1.0);

    let mut prop = AnimationTrack::new("modulate");
    prop.node_path = "Sprite".to_string();
    prop.track_type = TrackType::Property;
    prop.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0)));
    anim.tracks.push(prop);

    let mut method = AnimationTrack::new("footstep");
    method.node_path = ".".to_string();
    method.track_type = TrackType::Method;
    method.add_keyframe(KeyFrame::linear(
        0.5,
        Variant::String("footstep".to_string()),
    ));
    anim.tracks.push(method);

    let mut tree = AnimationTree::new();
    tree.add_animation(anim);

    // -- the type table carries the type, which sampling alone erases ---------
    let types = tree.current_track_types();
    let modulate = types
        .iter()
        .find(|(n, p, _)| n == "Sprite" && p == "modulate")
        .map(|(_, _, t)| *t);
    let footstep = types
        .iter()
        .find(|(_, p, _)| p == "footstep")
        .map(|(_, _, t)| *t);
    assert_eq!(
        modulate,
        Some(TrackType::Property),
        "the property track must be reported with its node path and type; \
         `get_current_values_targeted` erases both, which is why method and \
         audio keys were written as properties"
    );
    assert_eq!(
        footstep,
        Some(TrackType::Method),
        "the method track must be reported as Method"
    );

    // -- the skip-set names the method track, and ONLY it ---------------------
    let skip = tree.non_property_track_paths();
    assert!(
        skip.contains(&"footstep".to_string()),
        "a method track's path must be in the skip-set, or its sampled value \
         (the method NAME) gets written onto the node as a property; got {skip:?}"
    );
    assert!(
        !skip.contains(&"modulate".to_string()),
        "a PROPERTY track must NOT be skipped — skipping everything would stop \
         the tree animating anything while still passing the assertion above; \
         got {skip:?}"
    );
}
