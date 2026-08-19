//! 3D instance types for the rendering server.

use gdcore::math::Vector3;
use gdcore::math3d::{Aabb, Transform3D};

use crate::material::Material3D;
use crate::mesh::Mesh3D;
use crate::multimesh::MultiMesh3D;
use crate::shader::ShaderMaterial3D;

/// Unique identifier for a 3D render instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instance3DId(pub u64);

/// Godot `GeometryInstance3D.ShadowCastingSetting` (pat-nzgdu.76).
///
/// Discriminants match Godot's enum so a `.tscn`'s `cast_shadow = N` maps
/// straight through [`ShadowCastingSetting::from_godot_int`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowCastingSetting {
    /// `SHADOW_CASTING_SETTING_OFF` — drawn, but casts no shadow.
    Off,
    /// `SHADOW_CASTING_SETTING_ON` — drawn and casts a shadow (Godot default).
    #[default]
    On,
    /// `SHADOW_CASTING_SETTING_DOUBLE_SIDED` — shadow cast from both faces.
    DoubleSided,
    /// `SHADOW_CASTING_SETTING_SHADOW_ONLY` — casts a shadow but is not drawn.
    ShadowsOnly,
}

impl ShadowCastingSetting {
    /// Maps Godot's integer enum value; anything unknown falls back to `On`
    /// (Godot's default), never to a silently-invisible mode.
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            0 => Self::Off,
            2 => Self::DoubleSided,
            3 => Self::ShadowsOnly,
            _ => Self::On,
        }
    }

    /// Whether this instance contributes to shadow maps.
    ///
    /// Only `Off` opts out — `ShadowsOnly` casts (that is its entire point).
    pub fn casts_shadow(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether this instance is drawn into the colour frame.
    ///
    /// `ShadowsOnly` is the only mode that casts without being drawn.
    pub fn draws_geometry(self) -> bool {
        !matches!(self, Self::ShadowsOnly)
    }
}

/// Godot `GeometryInstance3D.VisibilityRangeFadeMode` (pat-nzgdu.76).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisibilityRangeFadeMode {
    /// `VISIBILITY_RANGE_FADE_DISABLED` — hard pop in/out at the range edges.
    #[default]
    Disabled,
    /// `VISIBILITY_RANGE_FADE_SELF` — this instance fades out over its margins.
    SelfFade,
    /// `VISIBILITY_RANGE_FADE_DEPENDENCIES` — *children* fade instead of this
    /// instance.
    Dependencies,
}

impl VisibilityRangeFadeMode {
    /// Maps Godot's integer enum value; unknown values fall back to `Disabled`.
    pub fn from_godot_int(value: i64) -> Self {
        match value {
            1 => Self::SelfFade,
            2 => Self::Dependencies,
            _ => Self::Disabled,
        }
    }
}

/// Where a distance falls relative to an instance's visibility range.
///
/// Mirrors the three outcomes of Godot's
/// `RendererSceneCull::_visibility_range_check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityRangeState {
    /// Within the range — draw it.
    Visible,
    /// Past `visibility_range_end` — too far away to draw.
    HiddenFar,
    /// Nearer than `visibility_range_begin` — too close to draw (this is how
    /// Godot hands off to a higher-detail LOD sibling).
    HiddenClose,
}

impl VisibilityRangeState {
    /// Whether the instance should be drawn in this state.
    pub fn is_visible(self) -> bool {
        matches!(self, Self::Visible)
    }
}

/// Per-instance `GeometryInstance3D` render settings (pat-nzgdu.76).
///
/// Godot's `GeometryInstance3D` sits between `VisualInstance3D` and
/// `MeshInstance3D` and carries the knobs that change *how* an instance is
/// drawn rather than *what* geometry it holds. Patina stored these as node
/// properties and registered them in ClassDB, but nothing in the render path
/// read them; this struct is what carries them across into the renderer.
///
/// [`Default`] reproduces Godot's property defaults exactly, so an instance
/// that never sets any of them renders as it did before this type existed.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryInstanceSettings {
    /// Shadow casting mode.
    pub cast_shadow: ShadowCastingSetting,
    /// Per-instance transparency, `0.0` opaque … `1.0` fully invisible.
    pub transparency: f32,
    /// Hide when the camera is nearer than this (`0.0` = no near limit).
    pub visibility_range_begin: f32,
    /// Fade/hysteresis margin around [`Self::visibility_range_begin`].
    pub visibility_range_begin_margin: f32,
    /// Hide when the camera is further than this (`0.0` = no far limit).
    pub visibility_range_end: f32,
    /// Fade/hysteresis margin around [`Self::visibility_range_end`].
    pub visibility_range_end_margin: f32,
    /// How the instance transitions at the range edges.
    pub visibility_range_fade_mode: VisibilityRangeFadeMode,
    /// Extra material drawn as a second pass over the surface.
    pub material_overlay: Option<Material3D>,
    /// Mesh LOD selection bias (Godot default `1.0`).
    ///
    /// ⚠️ Carried but not yet consumed: Patina's meshes have no LOD chain to
    /// select from, so there is nothing for the bias to bias. Stored so the
    /// value survives a sync round-trip and the LOD work can read it later.
    pub lod_bias: f32,
    /// Grows the instance's culling bounds by this many world units.
    pub extra_cull_margin: f32,
    /// Render layers this instance is drawn on — Godot's
    /// `VisualInstance3D.layers`, a 1-based bitmask whose default is layer 1.
    ///
    /// ⚠️ Carried HERE rather than on [`Instance3D`] because
    /// `set_geometry_instance_settings` is an existing end-to-end channel from
    /// the scene to the renderer; a new field on `Instance3D` would need a new
    /// trait method and a new sync site to go with it.
    ///
    /// Read by the shading loop to honour each light's `light_cull_mask`.
    pub layers: u32,
    /// Overrides the mesh-derived bounds used for culling.
    pub custom_aabb: Option<Aabb>,
}

impl Default for GeometryInstanceSettings {
    fn default() -> Self {
        Self {
            cast_shadow: ShadowCastingSetting::On,
            transparency: 0.0,
            visibility_range_begin: 0.0,
            visibility_range_begin_margin: 0.0,
            visibility_range_end: 0.0,
            visibility_range_end_margin: 0.0,
            visibility_range_fade_mode: VisibilityRangeFadeMode::Disabled,
            material_overlay: None,
            lod_bias: 1.0,
            extra_cull_margin: 0.0,
            // Godot's default is layer 1 only, NOT all layers: a mesh left
            // alone sits on one layer and every light's default all-ones mask
            // covers it.
            layers: 1,
            custom_aabb: None,
        }
    }
}

impl GeometryInstanceSettings {
    /// Classifies `distance` (camera → instance) against the visibility range,
    /// following `RendererSceneCull::_visibility_range_check`.
    ///
    /// `was_visible` is Godot's per-viewport `viewport_state` bit: with fading
    /// disabled the margins act as *hysteresis*, so the threshold depends on
    /// whether the instance was already on screen. Patina's renderer is
    /// stateless per frame and passes `false` (the cold-start answer, matching
    /// an instance that has not been drawn yet). With the default margins of
    /// `0.0` the distinction disappears entirely.
    pub fn visibility_at_distance(&self, distance: f32, was_visible: bool) -> VisibilityRangeState {
        let mut begin_offset = -self.visibility_range_begin_margin;
        let mut end_offset = self.visibility_range_end_margin;

        if self.visibility_range_fade_mode == VisibilityRangeFadeMode::Disabled && !was_visible {
            begin_offset = -begin_offset;
            end_offset = -end_offset;
        }

        if self.visibility_range_end > 0.0 && distance > self.visibility_range_end + end_offset {
            VisibilityRangeState::HiddenFar
        } else if self.visibility_range_begin > 0.0
            && distance < self.visibility_range_begin + begin_offset
        {
            VisibilityRangeState::HiddenClose
        } else {
            VisibilityRangeState::Visible
        }
    }

    /// The instance's own opacity at `distance`, folding
    /// [`Self::transparency`] together with `VISIBILITY_RANGE_FADE_SELF`.
    ///
    /// Returns `1.0` for a fully opaque instance and `0.0` for an invisible
    /// one. Fade-out ramps across the margin window on each side, mirroring the
    /// window Godot uses for its dependency fade. `Dependencies` mode fades the
    /// *children* of a visibility parent, which Patina has no graph for, so it
    /// leaves this instance at full opacity — exactly as Godot does.
    pub fn opacity_at_distance(&self, distance: f32) -> f32 {
        let base = (1.0 - self.transparency).clamp(0.0, 1.0);
        if self.visibility_range_fade_mode != VisibilityRangeFadeMode::SelfFade {
            return base;
        }

        let mut fade = 1.0_f32;
        let end_margin = self.visibility_range_end_margin;
        let begin_margin = self.visibility_range_begin_margin;

        if self.visibility_range_end > 0.0 && end_margin > 0.0 {
            let fade_start = self.visibility_range_end - end_margin;
            if distance > fade_start {
                fade = fade.min(1.0 - ((distance - fade_start) / (2.0 * end_margin)).min(1.0));
            }
        }
        if self.visibility_range_begin > 0.0 && begin_margin > 0.0 {
            let fade_start = self.visibility_range_begin + begin_margin;
            if distance < fade_start {
                fade = fade.min(1.0 - ((fade_start - distance) / (2.0 * begin_margin)).min(1.0));
            }
        }

        (base * fade).clamp(0.0, 1.0)
    }
}

/// A renderable 3D instance in the scene.
#[derive(Debug, Clone)]
pub struct Instance3D {
    /// Unique identifier.
    pub id: Instance3DId,
    /// Mesh geometry (if assigned).
    pub mesh: Option<Mesh3D>,
    /// Surface material (if assigned).
    pub material: Option<Material3D>,
    /// Shader material override (takes precedence over `material` for color).
    pub shader_material: Option<ShaderMaterial3D>,
    /// MultiMesh resource for instanced rendering (if assigned).
    pub multimesh: Option<MultiMesh3D>,
    /// World-space transform.
    pub transform: Transform3D,
    /// Whether the instance is visible.
    pub visible: bool,
    /// `GeometryInstance3D` render settings — shadow mode, visibility range,
    /// transparency, overlay material, cull margins (pat-nzgdu.76).
    pub geometry: GeometryInstanceSettings,
}

impl Instance3D {
    /// Creates a new instance with default settings.
    pub fn new(id: Instance3DId) -> Self {
        Self {
            id,
            mesh: None,
            material: None,
            shader_material: None,
            multimesh: None,
            transform: Transform3D::IDENTITY,
            visible: true,
            geometry: GeometryInstanceSettings::default(),
        }
    }

    /// Whether this instance contributes to shadow maps (pat-nzgdu.76).
    ///
    /// An invisible instance casts nothing either — Godot hides the whole
    /// instance, shadow included.
    pub fn casts_shadow(&self) -> bool {
        self.visible && self.geometry.cast_shadow.casts_shadow()
    }

    /// Whether this instance is drawn into the colour frame (pat-nzgdu.76).
    pub fn draws_geometry(&self) -> bool {
        self.visible && self.geometry.cast_shadow.draws_geometry()
    }

    /// Whether the instance's visibility range admits it at this camera
    /// position (pat-nzgdu.76).
    ///
    /// Distance is measured camera → instance *origin*, matching Godot, which
    /// checks against the instance's transformed AABB centre for LOD purposes
    /// rather than the nearest surface point.
    pub fn is_within_visibility_range(&self, camera_position: Vector3) -> bool {
        self.geometry
            .visibility_at_distance(camera_position.distance_to(self.transform.origin), false)
            .is_visible()
    }

    /// This instance's world-space AABB — its mesh bounds under `transform`
    /// (pat-nzgdu.211).
    ///
    /// `None` when there is no mesh (and no `custom_aabb`), or the mesh has no
    /// vertices: an instance
    /// with nothing to draw has no bounds, and callers must treat that as
    /// "unknown", never as an empty box at the origin.
    ///
    /// ⚠️ Built with the same `transform_aabb` the occluder uses for its own
    /// world AABB, so the two are directly comparable. It is conservative
    /// (axis-aligned re-fit), so a rotated instance's box is larger than its
    /// geometry — which is the safe direction for culling: it can only make an
    /// occlusion test fail, never wrongly cull something visible.
    ///
    /// ⚠️ Ignores `multimesh`: a MultiMesh's per-instance transforms scatter
    /// geometry far outside this box, so a multimesh instance must not be culled
    /// on this bound. `Renderer::is_occluded` skips them for that reason.
    ///
    /// ⚠️ `custom_aabb` replaces the mesh bounds when set, and
    /// `extra_cull_margin` grows the result — both are `GeometryInstance3D`
    /// properties whose only job is to widen what culling considers visible
    /// (pat-nzgdu.76). A custom AABB is authored in *local* space, exactly like
    /// the mesh bounds it stands in for, so it goes through the same transform.
    pub fn world_aabb(&self) -> Option<Aabb> {
        let local = match self.geometry.custom_aabb {
            Some(custom) => custom,
            None => self.mesh.as_ref()?.aabb()?,
        };
        let world = crate::occluder::transform_aabb(&local, &self.transform);
        Some(grow_aabb(&world, self.geometry.extra_cull_margin))
    }
}

/// Expands `aabb` outward by `margin` on every side (pat-nzgdu.76).
///
/// A negative margin is ignored rather than shrinking the box: Godot's
/// `extra_cull_margin` is clamped to `>= 0` on the property, and a box that
/// *shrank* would cull geometry that is genuinely on screen — the one failure
/// mode culling must never have.
fn grow_aabb(aabb: &Aabb, margin: f32) -> Aabb {
    if margin <= 0.0 {
        return *aabb;
    }
    Aabb::new(
        Vector3::new(
            aabb.position.x - margin,
            aabb.position.y - margin,
            aabb.position.z - margin,
        ),
        Vector3::new(
            aabb.size.x + margin * 2.0,
            aabb.size.y + margin * 2.0,
            aabb.size.z + margin * 2.0,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector3;
    use gdcore::math3d::Basis;

    #[test]
    fn instance_creation_defaults() {
        let inst = Instance3D::new(Instance3DId(1));
        assert_eq!(inst.id, Instance3DId(1));
        assert!(inst.visible);
        assert!(inst.mesh.is_none());
        assert!(inst.material.is_none());
        assert_eq!(inst.transform, Transform3D::IDENTITY);
    }

    #[test]
    fn instance_id_equality() {
        assert_eq!(Instance3DId(42), Instance3DId(42));
        assert_ne!(Instance3DId(42), Instance3DId(99));
    }

    #[test]
    fn instance_set_transform() {
        let mut inst = Instance3D::new(Instance3DId(10));
        let t = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(5.0, 10.0, 15.0),
        };
        inst.transform = t;
        assert_eq!(inst.transform.origin, Vector3::new(5.0, 10.0, 15.0));
    }

    #[test]
    fn instance_visibility_toggle() {
        let mut inst = Instance3D::new(Instance3DId(1));
        assert!(inst.visible);
        inst.visible = false;
        assert!(!inst.visible);
    }

    // -- world_aabb (pat-nzgdu.211) ----------------------------------------

    /// ⚠️ No mesh ⇒ `None`, never an empty box. A caller culling on a zero-sized
    /// AABB at the origin would be acting on a bound that does not exist.
    #[test]
    fn world_aabb_is_none_without_a_mesh() {
        assert!(Instance3D::new(Instance3DId(1)).world_aabb().is_none());
    }

    /// A mesh with no vertices has no bounds either.
    #[test]
    fn world_aabb_is_none_for_an_empty_mesh() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::new(crate::mesh::PrimitiveType::Triangles));
        assert!(inst.world_aabb().is_none());
    }

    #[test]
    fn world_aabb_is_the_mesh_bounds_moved_by_the_transform() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        inst.transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(10.0, 0.0, -5.0),
        };

        let aabb = inst.world_aabb().expect("a cube has bounds");
        // cube(2.0) spans -1..1 locally, so the world box is centred on the
        // transform's origin.
        assert!((aabb.position.x - 9.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.position.z - (-6.0)).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.x - 2.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.z - 2.0).abs() < 1e-4, "{aabb:?}");
    }

    /// The identity transform must not move or resize the bounds — otherwise
    /// every cull would be comparing a box against a subtly shifted copy.
    #[test]
    fn world_aabb_under_identity_equals_the_mesh_aabb() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(1.0));
        let local = inst.mesh.as_ref().unwrap().aabb().unwrap();
        let world = inst.world_aabb().unwrap();
        assert!((world.position.x - local.position.x).abs() < 1e-4);
        assert!((world.size.y - local.size.y).abs() < 1e-4);
    }

    // -- GeometryInstance3D settings (pat-nzgdu.76) -------------------------

    /// The defaults must reproduce Godot's, or every existing scene silently
    /// changes appearance the moment this struct is introduced.
    #[test]
    fn geometry_settings_default_to_godot_defaults() {
        let g = GeometryInstanceSettings::default();
        assert_eq!(g.cast_shadow, ShadowCastingSetting::On);
        assert_eq!(g.transparency, 0.0);
        assert_eq!(g.visibility_range_begin, 0.0);
        assert_eq!(g.visibility_range_end, 0.0);
        assert_eq!(
            g.visibility_range_fade_mode,
            VisibilityRangeFadeMode::Disabled
        );
        assert_eq!(g.lod_bias, 1.0);
        assert_eq!(g.extra_cull_margin, 0.0);
        assert!(g.material_overlay.is_none());
        assert!(g.custom_aabb.is_none());
    }

    /// `.tscn` stores `cast_shadow` as a raw ordinal, so the mapping is
    /// load-bearing. Unknown values must land on `On`, never `Off` — a scene
    /// with a future enum value has to keep casting.
    #[test]
    fn cast_shadow_ordinals_match_godot() {
        assert_eq!(
            ShadowCastingSetting::from_godot_int(0),
            ShadowCastingSetting::Off
        );
        assert_eq!(
            ShadowCastingSetting::from_godot_int(1),
            ShadowCastingSetting::On
        );
        assert_eq!(
            ShadowCastingSetting::from_godot_int(2),
            ShadowCastingSetting::DoubleSided
        );
        assert_eq!(
            ShadowCastingSetting::from_godot_int(3),
            ShadowCastingSetting::ShadowsOnly
        );
        assert_eq!(
            ShadowCastingSetting::from_godot_int(99),
            ShadowCastingSetting::On
        );
        assert_eq!(
            ShadowCastingSetting::from_godot_int(-1),
            ShadowCastingSetting::On
        );
    }

    /// `Off` is the only mode that stops casting; `ShadowsOnly` is the only one
    /// that stops drawing. Getting these two backwards is the whole bug.
    #[test]
    fn cast_shadow_modes_split_casting_from_drawing() {
        for (mode, casts, draws) in [
            (ShadowCastingSetting::Off, false, true),
            (ShadowCastingSetting::On, true, true),
            (ShadowCastingSetting::DoubleSided, true, true),
            (ShadowCastingSetting::ShadowsOnly, true, false),
        ] {
            assert_eq!(mode.casts_shadow(), casts, "{mode:?} casts_shadow");
            assert_eq!(mode.draws_geometry(), draws, "{mode:?} draws_geometry");
        }
    }

    /// An instance hidden by `visible = false` casts nothing either — hiding a
    /// node in Godot takes its shadow with it.
    #[test]
    fn invisible_instance_neither_casts_nor_draws() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.visible = false;
        assert!(!inst.casts_shadow());
        assert!(!inst.draws_geometry());
    }

    #[test]
    fn cast_shadow_off_still_draws_but_stops_casting() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.geometry.cast_shadow = ShadowCastingSetting::Off;
        assert!(!inst.casts_shadow());
        assert!(
            inst.draws_geometry(),
            "cast_shadow=Off must not hide the mesh"
        );
    }

    #[test]
    fn shadows_only_casts_without_drawing() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.geometry.cast_shadow = ShadowCastingSetting::ShadowsOnly;
        assert!(inst.casts_shadow());
        assert!(!inst.draws_geometry());
    }

    /// A range of `0.0` means "no limit" in Godot — an unset visibility range
    /// must never cull anything, at any distance.
    #[test]
    fn zero_visibility_range_never_culls() {
        let g = GeometryInstanceSettings::default();
        for dist in [0.0, 1.0, 1_000.0, 1e9] {
            assert_eq!(
                g.visibility_at_distance(dist, false),
                VisibilityRangeState::Visible,
                "dist {dist}"
            );
        }
    }

    #[test]
    fn beyond_visibility_range_end_is_hidden_far() {
        let mut g = GeometryInstanceSettings::default();
        g.visibility_range_end = 50.0;
        assert_eq!(
            g.visibility_at_distance(49.0, false),
            VisibilityRangeState::Visible
        );
        assert_eq!(
            g.visibility_at_distance(51.0, false),
            VisibilityRangeState::HiddenFar
        );
    }

    #[test]
    fn nearer_than_visibility_range_begin_is_hidden_close() {
        let mut g = GeometryInstanceSettings::default();
        g.visibility_range_begin = 10.0;
        assert_eq!(
            g.visibility_at_distance(9.0, false),
            VisibilityRangeState::HiddenClose
        );
        assert_eq!(
            g.visibility_at_distance(11.0, false),
            VisibilityRangeState::Visible
        );
    }

    /// With fading disabled the margin is *hysteresis*: an instance already on
    /// screen survives further out than one that has to pop in. Mirrors
    /// `_visibility_range_check`'s `viewport_state` branch.
    #[test]
    fn fade_disabled_margin_acts_as_hysteresis() {
        let mut g = GeometryInstanceSettings::default();
        g.visibility_range_end = 50.0;
        g.visibility_range_end_margin = 5.0;

        // Already visible: survives out to end + margin.
        assert_eq!(
            g.visibility_at_distance(53.0, true),
            VisibilityRangeState::Visible
        );
        // Not yet visible: must come within end - margin to appear.
        assert_eq!(
            g.visibility_at_distance(53.0, false),
            VisibilityRangeState::HiddenFar
        );
        assert_eq!(
            g.visibility_at_distance(44.0, false),
            VisibilityRangeState::Visible
        );
    }

    #[test]
    fn transparency_scales_opacity() {
        let mut g = GeometryInstanceSettings::default();
        assert_eq!(g.opacity_at_distance(0.0), 1.0);
        g.transparency = 0.25;
        assert!((g.opacity_at_distance(0.0) - 0.75).abs() < 1e-6);
        g.transparency = 1.0;
        assert_eq!(g.opacity_at_distance(0.0), 0.0);
    }

    /// Self-fade ramps the instance out across its own margin window; with the
    /// fade disabled the same distances stay fully opaque (hard pop).
    #[test]
    fn self_fade_ramps_opacity_near_the_far_edge() {
        let mut g = GeometryInstanceSettings::default();
        g.visibility_range_end = 50.0;
        g.visibility_range_end_margin = 10.0;

        assert_eq!(g.opacity_at_distance(30.0), 1.0, "well inside the range");

        g.visibility_range_fade_mode = VisibilityRangeFadeMode::SelfFade;
        assert_eq!(g.opacity_at_distance(30.0), 1.0, "before the fade window");
        let mid = g.opacity_at_distance(50.0);
        assert!(mid > 0.0 && mid < 1.0, "inside the fade window, got {mid}");
        assert!(
            g.opacity_at_distance(55.0) < mid,
            "opacity must keep falling as the camera retreats"
        );
    }

    /// `Dependencies` fades a visibility *parent*'s children. Patina has no
    /// visibility-parent graph, so the instance itself must stay opaque rather
    /// than silently fading.
    #[test]
    fn dependencies_fade_leaves_this_instance_opaque() {
        let mut g = GeometryInstanceSettings::default();
        g.visibility_range_end = 50.0;
        g.visibility_range_end_margin = 10.0;
        g.visibility_range_fade_mode = VisibilityRangeFadeMode::Dependencies;
        assert_eq!(g.opacity_at_distance(50.0), 1.0);
    }

    #[test]
    fn visibility_range_uses_camera_to_origin_distance() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(1.0));
        inst.transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -100.0),
        };
        inst.geometry.visibility_range_end = 50.0;

        assert!(!inst.is_within_visibility_range(Vector3::ZERO), "100 > 50");
        assert!(
            inst.is_within_visibility_range(Vector3::new(0.0, 0.0, -80.0)),
            "20 < 50"
        );
    }

    /// `extra_cull_margin` may only ever *grow* the culling bounds.
    #[test]
    fn extra_cull_margin_grows_the_world_aabb() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        let base = inst.world_aabb().unwrap();

        inst.geometry.extra_cull_margin = 3.0;
        let grown = inst.world_aabb().unwrap();
        assert!((grown.position.x - (base.position.x - 3.0)).abs() < 1e-4);
        assert!((grown.size.x - (base.size.x + 6.0)).abs() < 1e-4);
    }

    /// A negative margin must not shrink the box — shrinking would cull
    /// geometry that is genuinely on screen.
    #[test]
    fn negative_extra_cull_margin_is_ignored() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        let base = inst.world_aabb().unwrap();
        inst.geometry.extra_cull_margin = -5.0;
        assert_eq!(inst.world_aabb().unwrap(), base);
    }

    /// `custom_aabb` stands in for the mesh bounds, in the same local space —
    /// so it moves with the transform exactly as the mesh bounds would.
    #[test]
    fn custom_aabb_replaces_the_mesh_bounds() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        inst.transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(5.0, 0.0, 0.0),
        };
        inst.geometry.custom_aabb = Some(Aabb::new(
            Vector3::new(-10.0, -10.0, -10.0),
            Vector3::new(20.0, 20.0, 20.0),
        ));

        let aabb = inst.world_aabb().unwrap();
        assert!((aabb.size.x - 20.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.position.x - (-5.0)).abs() < 1e-4, "{aabb:?}");
    }
}
