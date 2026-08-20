//! ReflectionProbe type for local cubemap reflections.
//!
//! A ReflectionProbe captures a cubemap of its surroundings, providing
//! local reflections for nearby meshes. Mirrors Godot's ReflectionProbe node.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;

/// Unique identifier for a reflection probe in the rendering server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReflectionProbeId(pub u64);

/// How the reflection probe updates its cubemap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReflectionProbeUpdateMode {
    /// Update the cubemap once on creation.
    #[default]
    Once = 0,
    /// Update the cubemap every frame.
    Always = 1,
}

/// How the reflection probe contributes ambient light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReflectionProbeAmbientMode {
    /// Disabled — no ambient contribution.
    Disabled = 0,
    /// Use the environment's ambient light.
    #[default]
    Environment = 1,
    /// Use a constant color for ambient light.
    ConstantColor = 2,
}

/// A reflection probe that captures local cubemap reflections.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionProbe {
    /// Unique identifier.
    pub id: ReflectionProbeId,
    /// World-space transform.
    pub transform: Transform3D,
    /// Half-extents of the probe's influence box (Godot `size`).
    pub size: Vector3,
    /// Offset of the probe's capture origin relative to its position.
    pub origin_offset: Vector3,
    /// Whether box projection is enabled for parallax-corrected reflections.
    pub box_projection: bool,
    /// Whether the probe is interior-only (no sky contribution).
    pub interior: bool,
    /// Whether the probe should capture shadows in the cubemap.
    pub enable_shadows: bool,
    /// Maximum distance for objects included in the cubemap capture.
    pub max_distance: f32,
    /// Reflection intensity multiplier.
    pub intensity: f32,
    /// Metres over which the probe fades out at its box boundary
    /// (Godot `blend_distance`, default 1.0).
    ///
    /// ⚠️ Registered in ClassDB and shown by the inspector since before this
    /// bead, but absent from this struct — so an authored value was parsed,
    /// displayed, and silently dropped. `0.0` disables the fade, which is the
    /// hard-edged behaviour every probe had until now (pat-s3rrv.206).
    pub blend_distance: f32,
    /// How the cubemap updates.
    pub update_mode: ReflectionProbeUpdateMode,
    /// How the probe contributes ambient light.
    pub ambient_mode: ReflectionProbeAmbientMode,
    /// Ambient color (used when `ambient_mode` is `ConstantColor`).
    pub ambient_color: Color,
    /// Ambient color energy multiplier.
    pub ambient_color_energy: f32,
    /// Cull mask controlling which visual layers are captured.
    pub cull_mask: u32,
    /// LOD threshold for mesh detail in the cubemap.
    pub mesh_lod_threshold: f32,
    /// The environment captured by this probe, once a capture pass has run
    /// (pat-s3rrv.206).
    ///
    /// `None` means "never captured": the probe then falls back to its flat
    /// `ambient_color` term, which is all it could ever do before this change.
    pub captured: Option<std::sync::Arc<ReflectionCubemap>>,
}

impl ReflectionProbe {
    /// Creates a new reflection probe with Godot-compatible defaults.
    pub fn new(id: ReflectionProbeId) -> Self {
        Self {
            id,
            transform: Transform3D::IDENTITY,
            size: Vector3::new(20.0, 20.0, 20.0),
            origin_offset: Vector3::ZERO,
            box_projection: false,
            interior: false,
            enable_shadows: false,
            max_distance: 0.0,
            intensity: 1.0,
            // Godot's documented default (`ReflectionProbe.xml`).
            blend_distance: 1.0,
            update_mode: ReflectionProbeUpdateMode::default(),
            ambient_mode: ReflectionProbeAmbientMode::default(),
            ambient_color: Color::BLACK,
            ambient_color_energy: 1.0,
            cull_mask: 0xFFFFF, // 20 bits, Godot default
            mesh_lod_threshold: 1.0,
            captured: None,
        }
    }

    /// Returns the axis-aligned bounding box of the probe's influence volume
    /// in world space, accounting for `size` and `origin_offset`.
    pub fn world_aabb_min_max(&self) -> (Vector3, Vector3) {
        let center = self.transform.origin + self.origin_offset;
        let half = self.size * 0.5;
        (center - half, center + half)
    }

    /// Returns `true` if the given world-space point is inside the probe's
    /// influence box.
    pub fn contains_point(&self, point: Vector3) -> bool {
        let (min, max) = self.world_aabb_min_max();
        point.x >= min.x
            && point.x <= max.x
            && point.y >= min.y
            && point.y <= max.y
            && point.z >= min.z
            && point.z <= max.z
    }

    /// The world-space point the cubemap is captured from.
    pub fn capture_origin(&self) -> Vector3 {
        self.transform.origin + self.origin_offset
    }

    /// Whether a capture pass should (re)render this probe's cubemap now.
    ///
    /// This is the whole of Godot's `update_mode` semantics: `Always`
    /// re-captures every frame, `Once` captures on the first pass and then
    /// keeps that cubemap even as the scene changes around it.
    pub fn needs_capture(&self) -> bool {
        match self.update_mode {
            ReflectionProbeUpdateMode::Always => true,
            ReflectionProbeUpdateMode::Once => self.captured.is_none(),
        }
    }
}

/// Sums the ambient reflection contribution of every probe whose influence box
/// contains `world_pos`.
///
/// Each probe contributes `intensity * (ambient_color * ambient_color_energy)`
/// when its `ambient_mode` is `Environment` or `ConstantColor`. `Disabled`
/// probes skip the ambient term. Probes outside `world_pos` are skipped.
///
/// This is the parity oracle used by both the software and wgpu render paths
/// to fold local probe ambience into the lit color of fragments inside the
/// probe's AABB. Specular cubemap lookup and box projection are handled by
/// the renderers themselves at shading time and are not part of this sum.
pub fn sample_probes_at(probes: &[ReflectionProbe], world_pos: Vector3) -> Color {
    let mut r = 0.0_f32;
    let mut g = 0.0_f32;
    let mut b = 0.0_f32;
    for probe in probes {
        if !probe.contains_point(world_pos) {
            continue;
        }
        let energy = match probe.ambient_mode {
            ReflectionProbeAmbientMode::Disabled => continue,
            ReflectionProbeAmbientMode::Environment | ReflectionProbeAmbientMode::ConstantColor => {
                probe.ambient_color_energy
            }
        };
        r += probe.ambient_color.r * energy * probe.intensity;
        g += probe.ambient_color.g * energy * probe.intensity;
        b += probe.ambient_color.b * energy * probe.intensity;
    }
    Color::new(r, g, b, 1.0)
}

/// The six cubemap faces, in the GL / Godot order used by `faces` below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CubeFace {
    PosX = 0,
    NegX = 1,
    PosY = 2,
    NegY = 3,
    PosZ = 4,
    NegZ = 5,
}

/// An environment cubemap captured by a [`ReflectionProbe`] (pat-s3rrv.206).
///
/// Six square faces of linear colour. Deliberately low resolution: a probe
/// re-renders the whole scene once per face, so this is the term that decides
/// whether `update_mode = Always` is affordable at all.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionCubemap {
    /// Edge length of each square face, in texels.
    pub resolution: u32,
    /// Six faces in [`CubeFace`] order, each `resolution * resolution`,
    /// row-major.
    pub faces: [Vec<Color>; 6],
}

impl ReflectionCubemap {
    /// A cubemap with every texel of every face set to `fill`.
    pub fn new(resolution: u32, fill: Color) -> Self {
        let n = (resolution as usize) * (resolution as usize);
        Self {
            resolution,
            faces: [
                vec![fill; n],
                vec![fill; n],
                vec![fill; n],
                vec![fill; n],
                vec![fill; n],
                vec![fill; n],
            ],
        }
    }

    /// Replaces one face. `pixels` must be `resolution * resolution` long.
    pub fn set_face(&mut self, face: CubeFace, pixels: Vec<Color>) {
        let n = (self.resolution as usize) * (self.resolution as usize);
        if pixels.len() == n {
            self.faces[face as usize] = pixels;
        }
    }

    /// Nearest-texel lookup along `dir` (need not be normalized).
    ///
    /// ⚠️ The face-selection and s/t sign conventions are the standard cubemap
    /// ones. They are NOT symmetric between faces — `+X` flips s against z
    /// while `+Z` does not, and every face except `+Y`/`-Y` flips t against y.
    /// Deriving them "by symmetry" produces a mapping that looks plausible and
    /// mirrors half the faces.
    pub fn sample_direction(&self, dir: Vector3) -> Color {
        let fallback = Color::new(0.0, 0.0, 0.0, 1.0);
        if self.resolution == 0 {
            return fallback;
        }
        let (ax, ay, az) = (dir.x.abs(), dir.y.abs(), dir.z.abs());
        if ax + ay + az <= 0.0 {
            return fallback;
        }

        let (face, sc, tc, ma) = if ax >= ay && ax >= az {
            if dir.x > 0.0 {
                (CubeFace::PosX, -dir.z, -dir.y, ax)
            } else {
                (CubeFace::NegX, dir.z, -dir.y, ax)
            }
        } else if ay >= az {
            if dir.y > 0.0 {
                (CubeFace::PosY, dir.x, dir.z, ay)
            } else {
                (CubeFace::NegY, dir.x, -dir.z, ay)
            }
        } else if dir.z > 0.0 {
            (CubeFace::PosZ, dir.x, -dir.y, az)
        } else {
            (CubeFace::NegZ, -dir.x, -dir.y, az)
        };

        let u = 0.5 * (sc / ma + 1.0);
        let v = 0.5 * (tc / ma + 1.0);
        let res = self.resolution;
        let x = ((u.clamp(0.0, 1.0) * res as f32) as u32).min(res - 1);
        let y = ((v.clamp(0.0, 1.0) * res as f32) as u32).min(res - 1);
        let i = (y * res + x) as usize;
        self.faces[face as usize]
            .get(i)
            .copied()
            .unwrap_or(fallback)
    }
}

/// One probe, resolved and flattened for a shading pass (pat-s3rrv.206).
///
/// The probe *list* is queried once per instance (a point query, the
/// granularity the shading loop already used for ambient), but the cubemap has
/// to be sampled **per fragment** — a mirror only reflects different things at
/// different pixels if the reflection direction varies across it. This is the
/// owned snapshot that crosses that boundary, so the fragment stage needs no
/// borrow of the probe list.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeReflection {
    /// The captured environment.
    pub cubemap: std::sync::Arc<ReflectionCubemap>,
    /// World-space influence box, for box projection.
    pub box_min: Vector3,
    /// World-space influence box, for box projection.
    pub box_max: Vector3,
    /// Point the cubemap was captured from.
    pub origin: Vector3,
    /// Whether to parallax-correct the lookup against the box.
    pub box_projection: bool,
    /// Reflection strength multiplier.
    pub intensity: f32,
    /// Metres of fade at the box boundary; `0.0` means a hard edge.
    pub blend_distance: f32,
}

impl ProbeReflection {
    /// The reflected colour for a surface at `world_pos` reflecting along
    /// `reflection`.
    ///
    /// How much of the reflection survives at `world_pos`, in `0..=1`.
    ///
    /// Ported from `scene_forward_lights_inc.glsl:931-938`:
    ///
    /// ```text
    /// axis_blend_distance = min(blend_distance, box_extents)
    /// blend_axes = (abs(local_pos) - box_extents + axis_blend_distance) / axis_blend_distance
    /// blend_axes = clamp(1 - blend_axes, 0, 1)
    /// blend      = (blend_axes.x * blend_axes.y * blend_axes.z) ^ 2
    /// ```
    ///
    /// So the weight is 1 until a fragment comes within `blend_distance` of a
    /// box face, then falls to 0 exactly at the face — squared, and multiplied
    /// across the three axes so a corner fades faster than a face.
    ///
    /// ⚠️ `min(blend_distance, box_extents)` is not tidying: a blend distance
    /// wider than the box would otherwise divide the fade by a band the box
    /// cannot contain, leaving the probe dim at its own centre.
    ///
    /// ⚠️ `0.0` short-circuits to a hard edge. The reference gates the whole
    /// computation on `!= 0.0`, and dividing by a zero band would be NaN — which
    /// propagates into the pixel rather than failing.
    fn blend_weight(&self, world_pos: Vector3) -> f32 {
        if self.blend_distance <= 0.0 {
            return 1.0;
        }
        let center = (self.box_min + self.box_max) * 0.5;
        let extents = (self.box_max - self.box_min) * 0.5;
        let local = world_pos - center;
        let mut blend = 1.0_f32;
        for (l, e) in [
            (local.x, extents.x),
            (local.y, extents.y),
            (local.z, extents.z),
        ] {
            let band = self.blend_distance.min(e);
            if band <= 0.0 {
                continue;
            }
            let axis = (l.abs() - e + band) / band;
            blend *= (1.0 - axis).clamp(0.0, 1.0);
        }
        blend * blend
    }

    /// 🔑 The `box_projection` branch here is the first and only caller of
    /// [`crate::box_projection::box_project_reflection`]. That module has been
    /// present, complete and unit-tested with ZERO callers, so authoring
    /// `box_projection = true` in a `.tscn` changed nothing at all.
    pub fn sample(&self, world_pos: Vector3, reflection: Vector3) -> Color {
        let dir = if self.box_projection {
            crate::box_projection::box_project_reflection(
                world_pos,
                reflection,
                self.box_min,
                self.box_max,
                self.origin,
            )
        } else {
            // Without box projection the environment is treated as infinitely
            // far away, so the capture origin is irrelevant and the raw
            // reflection direction is the lookup. This is exactly the
            // difference the parallax correction exists to fix.
            reflection
        };
        let c = self.cubemap.sample_direction(dir);
        let strength = self.intensity * self.blend_weight(world_pos);
        Color::new(c.r * strength, c.g * strength, c.b * strength, 1.0)
    }
}

/// The first captured probe whose influence box contains `world_pos`.
///
/// `None` when no such probe has a capture yet — the caller then keeps whatever
/// flat ambient term it already had, so a scene with no probe, or one whose
/// probe has never been captured, is byte-identical to before.
pub fn probe_reflection_at(
    probes: &[ReflectionProbe],
    world_pos: Vector3,
) -> Option<ProbeReflection> {
    for probe in probes {
        if !probe.contains_point(world_pos) {
            continue;
        }
        let cubemap = match probe.captured.as_ref() {
            Some(c) => c.clone(),
            None => continue,
        };
        let (box_min, box_max) = probe.world_aabb_min_max();
        return Some(ProbeReflection {
            cubemap,
            box_min,
            box_max,
            origin: probe.capture_origin(),
            box_projection: probe.box_projection,
            intensity: probe.intensity,
            blend_distance: probe.blend_distance,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_probe_values_match_godot() {
        let probe = ReflectionProbe::new(ReflectionProbeId(1));
        assert_eq!(probe.size, Vector3::new(20.0, 20.0, 20.0));
        assert_eq!(probe.origin_offset, Vector3::ZERO);
        assert!(!probe.box_projection);
        assert!(!probe.interior);
        assert!(!probe.enable_shadows);
        assert!((probe.max_distance - 0.0).abs() < f32::EPSILON);
        assert!((probe.intensity - 1.0).abs() < f32::EPSILON);
        assert_eq!(probe.update_mode, ReflectionProbeUpdateMode::Once);
        assert_eq!(probe.ambient_mode, ReflectionProbeAmbientMode::Environment);
        assert_eq!(probe.ambient_color, Color::BLACK);
        assert!((probe.ambient_color_energy - 1.0).abs() < f32::EPSILON);
        assert_eq!(probe.cull_mask, 0xFFFFF);
        assert!((probe.mesh_lod_threshold - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn probe_id_equality() {
        assert_eq!(ReflectionProbeId(10), ReflectionProbeId(10));
        assert_ne!(ReflectionProbeId(10), ReflectionProbeId(20));
    }

    #[test]
    fn contains_point_at_center() {
        let probe = ReflectionProbe::new(ReflectionProbeId(1));
        assert!(probe.contains_point(Vector3::ZERO));
    }

    #[test]
    fn contains_point_at_edge() {
        let probe = ReflectionProbe::new(ReflectionProbeId(1));
        // Half-extent is 10.0, so edge is at ±10.
        assert!(probe.contains_point(Vector3::new(10.0, 10.0, 10.0)));
        assert!(!probe.contains_point(Vector3::new(10.1, 0.0, 0.0)));
    }

    #[test]
    fn contains_point_with_offset() {
        let mut probe = ReflectionProbe::new(ReflectionProbeId(1));
        probe.origin_offset = Vector3::new(5.0, 0.0, 0.0);
        // Center shifted to (5, 0, 0), so range on X is -5..15.
        assert!(probe.contains_point(Vector3::new(14.0, 0.0, 0.0)));
        assert!(!probe.contains_point(Vector3::new(-6.0, 0.0, 0.0)));
    }

    #[test]
    fn world_aabb_with_transform() {
        let mut probe = ReflectionProbe::new(ReflectionProbeId(1));
        probe.transform.origin = Vector3::new(100.0, 0.0, 0.0);
        let (min, max) = probe.world_aabb_min_max();
        assert!((min.x - 90.0).abs() < f32::EPSILON);
        assert!((max.x - 110.0).abs() < f32::EPSILON);
    }

    #[test]
    fn update_mode_variants() {
        assert_eq!(ReflectionProbeUpdateMode::Once as u32, 0);
        assert_eq!(ReflectionProbeUpdateMode::Always as u32, 1);
    }

    #[test]
    fn ambient_mode_variants() {
        assert_eq!(ReflectionProbeAmbientMode::Disabled as u32, 0);
        assert_eq!(ReflectionProbeAmbientMode::Environment as u32, 1);
        assert_eq!(ReflectionProbeAmbientMode::ConstantColor as u32, 2);
    }
}
