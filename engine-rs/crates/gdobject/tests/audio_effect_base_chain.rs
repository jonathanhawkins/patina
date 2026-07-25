//! pat-0q2kx.61 acceptance: `AudioEffect` exists as a base and the concrete
//! effects inherit through it.
//!
//! Godot ground truth: `servers/audio/audio_effect.h` and `extension_api.json`
//! — `AudioEffect <- Resource`, and `AudioEffectAmplify`/`Reverb`/`EQ <-
//! AudioEffect`.
//!
//! Patina registered the three concrete effects directly under `Resource` and
//! never registered the base at all, so anything filtering by "inherits
//! AudioEffect" — the AudioServer's effect-slot type validation, the editor's
//! audio-bus effect picker — matched nothing.

use gdobject::class_db::{class_exists, is_parent_class, register_2d_classes};

/// `register_2d_classes` is the entry point that registers the audio effects.
fn registered() {
    register_2d_classes();
}

/// **The acceptance test.**
#[test]
fn audio_effect_base_chain() {
    registered();

    assert!(
        class_exists("AudioEffect"),
        "the AudioEffect base must be registered"
    );
    assert!(
        is_parent_class("AudioEffect", "Resource"),
        "AudioEffect derives from Resource"
    );

    for effect in ["AudioEffectAmplify", "AudioEffectReverb", "AudioEffectEQ"] {
        assert!(class_exists(effect), "{effect} must be registered");
        assert!(
            is_parent_class(effect, "AudioEffect"),
            "{effect} must inherit AudioEffect — this is what the effect-slot \
             type check and the editor's effect picker filter on"
        );
        // ⚠️ The re-parent must not sever the chain above it. `inheritance_chain`
        // resolves by name at query time and stops at the first unregistered
        // name, so inserting a link without registering it would break these.
        assert!(
            is_parent_class(effect, "Resource"),
            "{effect} must still reach Resource through the new base"
        );
        assert!(
            is_parent_class(effect, "Object"),
            "{effect} must still reach Object — a severed chain is worse than \
             the missing base this bead fixes"
        );
    }
}

/// ⚠️ `AudioEffectInstance` is the per-playback companion and derives from
/// `RefCounted`, NOT from `AudioEffect` (`audio_effect.h`). Registering it under
/// `AudioEffect` would make the editor's effect picker offer it as an insert.
#[test]
fn audio_effect_instance_is_not_an_audio_effect() {
    registered();

    assert!(class_exists("AudioEffectInstance"));
    assert!(
        is_parent_class("AudioEffectInstance", "RefCounted"),
        "AudioEffectInstance derives from RefCounted"
    );
    assert!(
        !is_parent_class("AudioEffectInstance", "AudioEffect"),
        "AudioEffectInstance must NOT be selectable as a bus effect"
    );
}

/// The base is abstract in Godot — a category, not something a scene
/// instantiates as an effect.
///
/// ⚠️ Asserted through `is_abstract`, NOT by expecting `instantiate` to return
/// `None`: Patina's `instantiate` does not honour the abstract flag (Godot
/// returns null there), so an `is_none()` assertion would fail here for a
/// reason that has nothing to do with this bead.
#[test]
fn audio_effect_base_is_abstract() {
    registered();
    assert!(
        gdobject::class_db::is_abstract("AudioEffect"),
        "AudioEffect is an abstract base in Godot"
    );
    assert!(
        !gdobject::class_db::is_abstract("AudioEffectAmplify"),
        "the concrete effects stay instantiable"
    );
}
