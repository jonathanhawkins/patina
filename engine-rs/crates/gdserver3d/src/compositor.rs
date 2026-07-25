//! `Compositor` + `CompositorEffect` — custom render passes injected into the
//! 3D pipeline.
//!
//! Mirrors Godot's `Compositor` (a `Resource` holding an ordered list of
//! effects) and `CompositorEffect` (a `Resource` describing one pass: its
//! pipeline callback stage, an enabled flag, and which resolved buffers it
//! needs). A `WorldEnvironment`/`Camera3D` references a `Compositor` to inject
//! passes; at each stage the pipeline runs the enabled effects whose
//! `effect_callback_type` matches — see [`Compositor::effects_for_stage`].

/// The 3D-pipeline stage at which a [`CompositorEffect`] runs
/// (Godot `CompositorEffect.EffectCallbackType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectCallbackType {
    /// Before opaque geometry is drawn.
    PreOpaque,
    /// After opaque geometry, before the sky.
    PostOpaque,
    /// After the sky is drawn.
    PostSky,
    /// Before transparent geometry.
    PreTransparent,
    /// After transparent geometry (the last stage).
    PostTransparent,
}

impl EffectCallbackType {
    /// Maps Godot's integer enum value (`0`..=`4`) to a stage; other values fall
    /// back to [`PreOpaque`](EffectCallbackType::PreOpaque).
    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => EffectCallbackType::PostOpaque,
            2 => EffectCallbackType::PostSky,
            3 => EffectCallbackType::PreTransparent,
            4 => EffectCallbackType::PostTransparent,
            _ => EffectCallbackType::PreOpaque,
        }
    }

    /// The Godot integer enum value for this stage.
    pub fn as_i64(self) -> i64 {
        match self {
            EffectCallbackType::PreOpaque => 0,
            EffectCallbackType::PostOpaque => 1,
            EffectCallbackType::PostSky => 2,
            EffectCallbackType::PreTransparent => 3,
            EffectCallbackType::PostTransparent => 4,
        }
    }
}

/// A custom render pass injected into the 3D pipeline (Godot `CompositorEffect`).
#[derive(Debug, Clone, PartialEq)]
pub struct CompositorEffect {
    /// Whether this effect runs. Disabled effects are skipped by
    /// [`Compositor::effects_for_stage`].
    pub enabled: bool,
    /// The pipeline stage at which this effect's callback runs.
    pub effect_callback_type: EffectCallbackType,
    /// Whether the effect reads the resolved color buffer.
    pub access_resolved_color: bool,
    /// Whether the effect reads the resolved depth buffer.
    pub access_resolved_depth: bool,
    /// Whether the pipeline must produce motion vectors for this effect.
    pub needs_motion_vectors: bool,
    /// Whether the pipeline must produce a normal/roughness buffer.
    pub needs_normal_roughness: bool,
    /// Whether the pipeline must keep specular in a separate buffer.
    pub needs_separate_specular: bool,
}

impl CompositorEffect {
    /// Creates an enabled effect that runs at `effect_callback_type` with no
    /// buffer-access requirements.
    pub fn new(effect_callback_type: EffectCallbackType) -> Self {
        Self {
            enabled: true,
            effect_callback_type,
            access_resolved_color: false,
            access_resolved_depth: false,
            needs_motion_vectors: false,
            needs_normal_roughness: false,
            needs_separate_specular: false,
        }
    }
}

/// An ordered stack of [`CompositorEffect`]s (Godot `Compositor`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compositor {
    /// The compositor effects, in the order they were added.
    pub effects: Vec<CompositorEffect>,
}

impl Compositor {
    /// Creates an empty compositor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an effect to the stack.
    pub fn add_effect(&mut self, effect: CompositorEffect) {
        self.effects.push(effect);
    }

    /// The enabled effects that run at `stage`, in insertion order — the passes
    /// the pipeline invokes at that callback point.
    pub fn effects_for_stage(&self, stage: EffectCallbackType) -> Vec<&CompositorEffect> {
        self.effects
            .iter()
            .filter(|e| e.enabled && e.effect_callback_type == stage)
            .collect()
    }

    /// Number of effects in the stack (enabled or not).
    pub fn effect_count(&self) -> usize {
        self.effects.len()
    }

    /// Whether the stack has no effects.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_type_int_roundtrip() {
        assert_eq!(
            EffectCallbackType::from_i64(0),
            EffectCallbackType::PreOpaque
        );
        assert_eq!(
            EffectCallbackType::from_i64(1),
            EffectCallbackType::PostOpaque
        );
        assert_eq!(EffectCallbackType::from_i64(2), EffectCallbackType::PostSky);
        assert_eq!(
            EffectCallbackType::from_i64(3),
            EffectCallbackType::PreTransparent
        );
        assert_eq!(
            EffectCallbackType::from_i64(4),
            EffectCallbackType::PostTransparent
        );
        assert_eq!(
            EffectCallbackType::from_i64(99),
            EffectCallbackType::PreOpaque
        );
        assert_eq!(EffectCallbackType::PostSky.as_i64(), 2);
    }

    #[test]
    fn compositor_effect_defaults() {
        let e = CompositorEffect::new(EffectCallbackType::PostOpaque);
        assert!(e.enabled);
        assert_eq!(e.effect_callback_type, EffectCallbackType::PostOpaque);
        assert!(!e.access_resolved_color);
        assert!(!e.needs_motion_vectors);
    }

    #[test]
    fn effects_for_stage_filters_enabled_and_matching_type() {
        let mut c = Compositor::new();
        assert!(c.is_empty());

        c.add_effect(CompositorEffect::new(EffectCallbackType::PostOpaque));
        c.add_effect(CompositorEffect::new(EffectCallbackType::PostTransparent));
        let mut disabled = CompositorEffect::new(EffectCallbackType::PostOpaque);
        disabled.enabled = false;
        c.add_effect(disabled);

        assert_eq!(c.effect_count(), 3);

        // Only the enabled PostOpaque effect runs at that stage.
        let post_opaque = c.effects_for_stage(EffectCallbackType::PostOpaque);
        assert_eq!(post_opaque.len(), 1);
        assert_eq!(
            post_opaque[0].effect_callback_type,
            EffectCallbackType::PostOpaque
        );

        assert_eq!(
            c.effects_for_stage(EffectCallbackType::PostTransparent)
                .len(),
            1
        );
        // No effect targets this stage.
        assert!(c
            .effects_for_stage(EffectCallbackType::PreOpaque)
            .is_empty());
    }
}
