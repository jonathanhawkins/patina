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
    /// in world space.
    ///
    /// ⚠️ The box is centred on the probe's OWN origin; `origin_offset` moves
    /// only the capture point (pat-t42ca). Godot uploads `box_extents = size/2`
    /// and `local_matrix = probe transform⁻¹` and tests
    /// `abs(local_pos) > box_extents` (`scene_forward_lights_inc.glsl`
    /// `reflection_process`), while `origin_offset` goes only to
    /// `box_offset` (light_storage.cpp `update_reflection_probe_buffer`) and to
    /// the capture camera (renderer_scene_cull.cpp `set_look_at(origin_offset,
    /// …)`). Shifting the box by the offset moved the platformer's two big
    /// probes 6.5 m down, out from under the floor they light.
    pub fn world_aabb_min_max(&self) -> (Vector3, Vector3) {
        let center = self.transform.origin;
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
/// when its `ambient_mode` is `ConstantColor`. `Disabled` probes skip the
/// ambient term. Probes outside `world_pos` are skipped.
///
/// ⚠️ `Environment` (the default) contributes NOTHING here (pat-t42ca): Godot
/// reads `ambient_color` only for REFLECTION_AMBIENT_COLOR; an Environment
/// probe's ambient is its CAPTURE, applied per fragment through
/// [`ProbeReflection::ambient`] once the probe has been captured. This sum is
/// only the fallback for a probe with no capture yet.
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
            ReflectionProbeAmbientMode::Disabled | ReflectionProbeAmbientMode::Environment => {
                continue
            }
            ReflectionProbeAmbientMode::ConstantColor => probe.ambient_color_energy,
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
#[derive(Debug, Clone)]
pub struct ReflectionCubemap {
    /// Edge length of each square face, in texels.
    pub resolution: u32,
    /// Six faces in [`CubeFace`] order, each `resolution * resolution`,
    /// row-major.
    pub faces: [Vec<Color>; 6],
    /// The GGX-prefiltered roughness levels, built on first use (pat-9gjpr).
    /// Derived data: ignored by `PartialEq`, reset by [`Self::set_face`].
    /// ⚠️ Writing `faces` directly after a rough sample leaves this stale.
    roughness_chain: std::sync::OnceLock<RoughnessChain>,
}

impl PartialEq for ReflectionCubemap {
    fn eq(&self, other: &Self) -> bool {
        self.resolution == other.resolution && self.faces == other.faces
    }
}

/// Godot's default `rendering/reflections/sky_reflections/roughness_layers`.
pub const ROUGHNESS_LAYERS: usize = 8;
/// `MAX_ROUGHNESS_LOD` in Godot's shaders: `roughness_layers - 1`.
pub const MAX_ROUGHNESS_LOD: f32 = (ROUGHNESS_LAYERS - 1) as f32;
/// Smallest face edge of a prefiltered level.
pub const MIN_ROUGHNESS_LEVEL_RESOLUTION: u32 = 4;

/// Face edge, in texels, of roughness level `level` for a base of `base`.
pub fn roughness_level_resolution(base: u32, level: usize) -> u32 {
    if level == 0 {
        return base.max(1);
    }
    (base >> level)
        .max(MIN_ROUGHNESS_LEVEL_RESOLUTION)
        .min(base.max(1))
}

/// One cubemap level: six square faces of `resolution` texels.
#[derive(Debug, Clone, PartialEq)]
pub struct CubeLevel {
    /// Face edge in texels.
    pub resolution: u32,
    /// Six faces in [`CubeFace`] order, row-major.
    pub faces: [Vec<Color>; 6],
}

/// The radiance mip chain Godot samples reflections and probe ambient from
/// (pat-9gjpr).
///
/// Level `k` of [`ROUGHNESS_LAYERS`] is the capture convolved with GGX at
/// perceptual roughness `(k / MAX_ROUGHNESS_LOD)²`, ported from Godot's
/// `octmap_roughness.glsl`: `copy_effects.cpp` pushes `p²` with
/// `p = k / (layers - 1)` and the shader squares it twice more for
/// `roughness4`, which is GGX alpha². So sampling at
/// `lod = sqrt(roughness) * MAX_ROUGHNESS_LOD` (scene shader) lands on the
/// level filtered for exactly `roughness`. Level 0 is the raw capture.
#[derive(Debug, Clone, PartialEq)]
pub struct RoughnessChain {
    /// [`ROUGHNESS_LAYERS`] levels, level 0 first.
    pub levels: Vec<CubeLevel>,
}

/// The face and its `[0,1]` texture coordinates a direction lands on, with
/// the conventions of [`ReflectionCubemap::sample_direction`].
#[allow(clippy::neg_cmp_op_on_partial_ord)] // `!(x > 0)` also rejects NaN on purpose
fn cube_face_uv(dir: Vector3) -> Option<(usize, f32, f32)> {
    let (ax, ay, az) = (dir.x.abs(), dir.y.abs(), dir.z.abs());
    if !(ax + ay + az > 0.0) {
        return None;
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
    Some((face as usize, 0.5 * (sc / ma + 1.0), 0.5 * (tc / ma + 1.0)))
}

/// Inverse of [`cube_face_uv`]: the (unnormalized) direction through `(u, v)`
/// of `face`.
fn cube_face_dir(face: usize, u: f32, v: f32) -> Vector3 {
    let sc = 2.0 * u - 1.0;
    let tc = 2.0 * v - 1.0;
    match face {
        0 => Vector3::new(1.0, -tc, -sc),
        1 => Vector3::new(-1.0, -tc, sc),
        2 => Vector3::new(sc, 1.0, tc),
        3 => Vector3::new(sc, -1.0, -tc),
        4 => Vector3::new(sc, -tc, 1.0),
        _ => Vector3::new(-sc, -tc, -1.0),
    }
}

/// Bilinear lookup inside one face (clamped at the face edge — no
/// cross-face filtering), texel centres at `(i + 0.5) / res`.
#[cfg(test)]
fn bilinear_face(face: &[Color], res: u32, u: f32, v: f32) -> Color {
    let res_i = res.max(1) as i32;
    let x = u.clamp(0.0, 1.0) * res_i as f32 - 0.5;
    let y = v.clamp(0.0, 1.0) * res_i as f32 - 0.5;
    let x0f = x.floor();
    let y0f = y.floor();
    let fx = x - x0f;
    let fy = y - y0f;
    let cl = |i: i32| i.clamp(0, res_i - 1);
    let (x0, x1, y0, y1) = (
        cl(x0f as i32),
        cl(x0f as i32 + 1),
        cl(y0f as i32),
        cl(y0f as i32 + 1),
    );
    let at = |xx: i32, yy: i32| {
        face.get((yy * res_i + xx) as usize)
            .copied()
            .unwrap_or(Color::new(0.0, 0.0, 0.0, 1.0))
    };
    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    let mix = |p: f32, q: f32, r: f32, s: f32| {
        (p * (1.0 - fx) + q * fx) * (1.0 - fy) + (r * (1.0 - fx) + s * fx) * fy
    };
    Color::new(
        mix(a.r, b.r, c.r, d.r),
        mix(a.g, b.g, c.g, d.g),
        mix(a.b, b.b, c.b, d.b),
        1.0,
    )
}

/// The texel `(x, y)` of `face` at edge `res`, where `x`/`y` may lie ONE texel
/// outside the face: such a texel is fetched from the neighbouring face the
/// direction through its centre lands on (pat-h7sz7).
///
/// Godot 4.6 stores probes and the sky as octahedral maps with a border, so
/// its bilinear taps never stop at a seam; clamping at each cube face edge
/// (the old `bilinear_face`) left a visible line along every cube edge of a
/// blurred reflection. Mirrored by the WGSL `probe_texel`.
pub fn seamless_texel_index(face: usize, res: u32, x: i32, y: i32) -> (usize, usize) {
    let r = res.max(1) as i32;
    if (0..r).contains(&x) && (0..r).contains(&y) {
        return (face, (y * r + x) as usize);
    }
    let u = (x as f32 + 0.5) / r as f32;
    let v = (y as f32 + 0.5) / r as f32;
    let dir = cube_face_dir(face, u, v);
    match cube_face_uv(dir) {
        Some((f, fu, fv)) => {
            let tx = ((fu * r as f32).floor() as i32).clamp(0, r - 1);
            let ty = ((fv * r as f32).floor() as i32).clamp(0, r - 1);
            (f, (ty * r + tx) as usize)
        }
        None => (face, 0),
    }
}

impl CubeLevel {
    /// Bilinear sample along `dir`, filtering ACROSS face seams
    /// ([`seamless_texel_index`]).
    pub fn sample(&self, dir: Vector3) -> Color {
        let Some((face, u, v)) = cube_face_uv(dir) else {
            return Color::new(0.0, 0.0, 0.0, 1.0);
        };
        let res = self.resolution.max(1);
        let x = u.clamp(0.0, 1.0) * res as f32 - 0.5;
        let y = v.clamp(0.0, 1.0) * res as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let at = |xx: i32, yy: i32| {
            let (f, i) = seamless_texel_index(face, res, xx, yy);
            self.faces[f]
                .get(i)
                .copied()
                .unwrap_or(Color::new(0.0, 0.0, 0.0, 1.0))
        };
        let (a, b, c, d) = (
            at(x0, y0),
            at(x0 + 1, y0),
            at(x0, y0 + 1),
            at(x0 + 1, y0 + 1),
        );
        let mix = |p: f32, q: f32, r: f32, s: f32| {
            (p * (1.0 - fx) + q * fx) * (1.0 - fy) + (r * (1.0 - fx) + s * fx) * fy
        };
        Color::new(
            mix(a.r, b.r, c.r, d.r),
            mix(a.g, b.g, c.g, d.g),
            mix(a.b, b.b, c.b, d.b),
            1.0,
        )
    }

    /// The old face-clamped bilinear lookup, kept for the seam test.
    #[cfg(test)]
    fn sample_clamped(&self, dir: Vector3) -> Color {
        match cube_face_uv(dir) {
            Some((face, u, v)) => bilinear_face(&self.faces[face], self.resolution, u, v),
            None => Color::new(0.0, 0.0, 0.0, 1.0),
        }
    }

    /// 2x2 box downsample (a face of 1 texel stays 1 texel).
    fn downsample(&self) -> CubeLevel {
        let res = self.resolution;
        let half = (res / 2).max(1);
        let faces = std::array::from_fn(|f| {
            let src = &self.faces[f];
            let mut out = Vec::with_capacity((half * half) as usize);
            for y in 0..half {
                for x in 0..half {
                    let mut acc = [0.0f32; 3];
                    let mut n = 0.0;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(res - 1);
                        let sy = (y * 2 + dy).min(res - 1);
                        if let Some(c) = src.get((sy * res + sx) as usize) {
                            acc[0] += c.r;
                            acc[1] += c.g;
                            acc[2] += c.b;
                            n += 1.0;
                        }
                    }
                    let n = f32::max(n, 1.0);
                    out.push(Color::new(acc[0] / n, acc[1] / n, acc[2] / n, 1.0));
                }
            }
            out
        });
        CubeLevel {
            resolution: half,
            faces,
        }
    }
}

/// Van der Corput radical inverse, as Godot's `radicalInverse_VdC`.
fn radical_inverse_vdc(mut bits: u32) -> f32 {
    bits = bits.rotate_left(16);
    bits = ((bits & 0x5555_5555) << 1) | ((bits & 0xAAAA_AAAA) >> 1);
    bits = ((bits & 0x3333_3333) << 2) | ((bits & 0xCCCC_CCCC) >> 2);
    bits = ((bits & 0x0F0F_0F0F) << 4) | ((bits & 0xF0F0_F0F0) >> 4);
    bits = ((bits & 0x00FF_00FF) << 8) | ((bits & 0xFF00_FF00) >> 8);
    bits as f32 * 2.328_306_4e-10
}

impl RoughnessChain {
    /// Builds the chain from a capture (Godot `octmap_roughness.glsl`).
    pub fn build(base: &ReflectionCubemap) -> Self {
        let res = base.resolution.max(1);
        let level0 = CubeLevel {
            resolution: res,
            faces: base.faces.clone(),
        };
        // Box-downsampled source mips, which Godot's filter reads at the LOD
        // matching each sample's solid angle (avoids aliasing a wide lobe).
        let mut src = vec![level0.clone()];
        while src.last().map_or(1, |l| l.resolution) > 1 {
            let next = src.last().unwrap().downsample();
            src.push(next);
        }
        let solid_angle_texel = 4.0 * std::f32::consts::PI / (6.0 * (res * res) as f32);

        let mut levels = vec![level0];
        for k in 1..ROUGHNESS_LAYERS {
            let p = k as f32 / MAX_ROUGHNESS_LOD;
            let pushed = p * p; // copy_effects.cpp: `p_roughness * p_roughness`
            let roughness2 = pushed * pushed;
            let roughness4 = roughness2 * roughness2; // GGX alpha²
                                                      // Godot scales its 32 GGX samples by `4 * roughness`; a little more
                                                      // here keeps the tiny low-resolution levels free of banding.
            let total = ((256.0 * pushed) as u32).max(32);
            let mut samples: Vec<(Vector3, usize)> = Vec::with_capacity(total as usize);
            for i in 0..total {
                let xi = (i as f32 / total as f32, radical_inverse_vdc(i));
                let phi = 2.0 * std::f32::consts::PI * xi.0;
                let cos_t = ((1.0 - xi.1) / (1.0 + (roughness4 - 1.0) * xi.1))
                    .max(0.0)
                    .sqrt();
                let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
                let h = Vector3::new(sin_t * phi.cos(), sin_t * phi.sin(), cos_t);
                let n_dot_h = h.z;
                let l = Vector3::new(
                    2.0 * n_dot_h * h.x,
                    2.0 * n_dot_h * h.y,
                    2.0 * n_dot_h * h.z - 1.0,
                );
                if l.z <= 0.0 {
                    continue;
                }
                let nh2 = n_dot_h * n_dot_h;
                let denom = nh2 * (roughness4 - 1.0) + 1.0;
                let d = roughness4 / (std::f32::consts::PI * denom * denom);
                let pdf = d / 4.0 + 0.0001;
                let solid_angle_sample = 1.0 / (total as f32 * pdf + 0.0001);
                let mip = (0.5 * (solid_angle_sample / solid_angle_texel).log2())
                    .round()
                    .clamp(0.0, (src.len() - 1) as f32) as usize;
                samples.push((l, mip));
            }
            let lres = roughness_level_resolution(res, k);
            let faces = std::array::from_fn(|f| {
                let mut out = Vec::with_capacity((lres * lres) as usize);
                for y in 0..lres {
                    for x in 0..lres {
                        let n = cube_face_dir(
                            f,
                            (x as f32 + 0.5) / lres as f32,
                            (y as f32 + 0.5) / lres as f32,
                        )
                        .normalized();
                        let up = if n.y.abs() < 0.99999 {
                            Vector3::new(0.0, 1.0, 0.0)
                        } else {
                            Vector3::new(0.0, 0.0, 1.0)
                        };
                        let t0 = up.cross(n).normalized();
                        let t1 = n.cross(t0);
                        let mut acc = [0.0f32; 3];
                        let mut wsum = 0.0f32;
                        for (l, mip) in &samples {
                            let w = t0 * l.x + t1 * l.y + n * l.z;
                            let c = src[*mip].sample(w);
                            acc[0] += c.r * l.z;
                            acc[1] += c.g * l.z;
                            acc[2] += c.b * l.z;
                            wsum += l.z;
                        }
                        let wsum = wsum.max(1e-6);
                        out.push(Color::new(acc[0] / wsum, acc[1] / wsum, acc[2] / wsum, 1.0));
                    }
                }
                out
            });
            levels.push(CubeLevel {
                resolution: lres,
                faces,
            });
        }
        Self { levels }
    }

    /// Trilinear sample at `lod` in `0..=MAX_ROUGHNESS_LOD`.
    pub fn sample_lod(&self, dir: Vector3, lod: f32) -> Color {
        let max = (self.levels.len().max(1) - 1) as f32;
        let lod = if lod.is_finite() {
            lod.clamp(0.0, max)
        } else {
            0.0
        };
        let i = lod.floor() as usize;
        let f = lod - i as f32;
        let a = self.levels[i].sample(dir);
        if f <= 0.0 || i + 1 >= self.levels.len() {
            return a;
        }
        let b = self.levels[i + 1].sample(dir);
        Color::new(
            a.r + (b.r - a.r) * f,
            a.g + (b.g - a.g) * f,
            a.b + (b.b - a.b) * f,
            1.0,
        )
    }
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
            roughness_chain: std::sync::OnceLock::new(),
        }
    }

    /// A cubemap whose every texel is `f(direction through the texel centre)`
    /// — how a sky radiance map is rendered (pat-h7sz7). The direction is
    /// normalized and uses [`Self::sample_direction`]'s face conventions.
    pub fn from_directions(resolution: u32, mut f: impl FnMut(Vector3) -> Color) -> Self {
        let res = resolution.max(1);
        let faces = std::array::from_fn(|face| {
            let mut out = Vec::with_capacity((res * res) as usize);
            for y in 0..res {
                for x in 0..res {
                    let d = cube_face_dir(
                        face,
                        (x as f32 + 0.5) / res as f32,
                        (y as f32 + 0.5) / res as f32,
                    )
                    .normalized();
                    out.push(f(d));
                }
            }
            out
        });
        Self {
            resolution: res,
            faces,
            roughness_chain: std::sync::OnceLock::new(),
        }
    }

    /// Replaces one face. `pixels` must be `resolution * resolution` long.
    pub fn set_face(&mut self, face: CubeFace, pixels: Vec<Color>) {
        let n = (self.resolution as usize) * (self.resolution as usize);
        if pixels.len() == n {
            self.faces[face as usize] = pixels;
            self.roughness_chain = std::sync::OnceLock::new();
        }
    }

    /// The GGX-prefiltered levels (built once, then cached).
    pub fn roughness_chain(&self) -> &RoughnessChain {
        self.roughness_chain
            .get_or_init(|| RoughnessChain::build(self))
    }

    /// Roughness-filtered lookup: Godot's
    /// `textureLod(reflection_atlas, …, lod)` with
    /// `lod = sqrt(roughness) * MAX_ROUGHNESS_LOD` for a reflection and
    /// `lod = MAX_ROUGHNESS_LOD` for probe ambient.
    pub fn sample_lod(&self, dir: Vector3, lod: f32) -> Color {
        self.roughness_chain().sample_lod(dir, lod)
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
    /// How the probe replaces the ambient light inside its box (pat-t42ca).
    pub ambient_mode: ReflectionProbeAmbientMode,
    /// `ambient_color` (linear) times `ambient_color_energy`, used by
    /// [`ReflectionProbeAmbientMode::ConstantColor`].
    pub ambient_color: Color,
}

impl ProbeReflection {
    /// The box-projected (when enabled) lookup direction for `reflection`.
    fn lookup_dir(&self, world_pos: Vector3, reflection: Vector3) -> Vector3 {
        if self.box_projection {
            crate::box_projection::box_project_reflection(
                world_pos,
                reflection,
                self.box_min,
                self.box_max,
                self.origin,
            )
        } else {
            reflection
        }
    }

    /// The probe's fade weight at `world_pos` (Godot `blend`), `0..=1`.
    pub fn blend_at(&self, world_pos: Vector3) -> f32 {
        self.blend_weight(world_pos)
    }

    /// Roughness-filtered reflection (pat-9gjpr), Godot `reflection_process`:
    /// `textureLod(atlas, box_projected(ref_vec), sqrt(roughness) *
    /// MAX_ROUGHNESS_LOD) * intensity * blend`.
    pub fn sample_rough(&self, world_pos: Vector3, reflection: Vector3, roughness: f32) -> Color {
        let dir = self.lookup_dir(world_pos, reflection);
        let lod = roughness.clamp(0.0, 1.0).sqrt() * MAX_ROUGHNESS_LOD;
        let c = self.cubemap.sample_lod(dir, lod);
        let strength = self.intensity * self.blend_weight(world_pos);
        Color::new(c.r * strength, c.g * strength, c.b * strength, 1.0)
    }

    /// The ambient light at `world_pos` with this probe applied (pat-t42ca).
    ///
    /// Godot `reflection_process` + `scene_forward_clustered.glsl` 2082-2096:
    /// inside the box the probe's ambient REPLACES `base` (the environment
    /// ambient) by the fade weight —
    /// `ambient = probe_ambient * blend + base * (1 - blend)`. In
    /// `Environment` mode (the default) the probe ambient is the capture's
    /// most-blurred level (`lod = MAX_ROUGHNESS_LOD`) along the NORMAL, with
    /// no box projection and no `intensity`; `ConstantColor` uses
    /// `ambient_color * energy`; `Disabled` leaves `base` alone.
    pub fn ambient(&self, world_pos: Vector3, normal: Vector3, base: Color) -> Color {
        let probe = match self.ambient_mode {
            ReflectionProbeAmbientMode::Disabled => return base,
            ReflectionProbeAmbientMode::Environment => {
                self.cubemap.sample_lod(normal, MAX_ROUGHNESS_LOD)
            }
            ReflectionProbeAmbientMode::ConstantColor => self.ambient_color,
        };
        let b = self.blend_weight(world_pos);
        Color::new(
            probe.r * b + base.r * (1.0 - b),
            probe.g * b + base.g * (1.0 - b),
            probe.b * b + base.b * (1.0 - b),
            1.0,
        )
    }

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

/// The index of the probe Godot applies first at `world_pos`: the SMALLEST
/// captured probe whose influence box contains it.
///
/// Godot sorts probes by `-extents.length()` (light_storage.cpp
/// `update_reflection_probe_buffer`) and the cluster loop walks bits with
/// `findMSB`, so the smallest box is processed first and its full-weight
/// interior saturates both accumulators. Patina binds ONE probe per draw, so
/// it binds that one. Ties keep list order.
pub fn probe_index_at(probes: &[ReflectionProbe], world_pos: Vector3) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, probe) in probes.iter().enumerate() {
        if probe.captured.is_none() || !probe.contains_point(world_pos) {
            continue;
        }
        let size = (probe.size * 0.5).length();
        if best.is_none_or(|(_, s)| size < s) {
            best = Some((i, size));
        }
    }
    best.map(|(i, _)| i)
}

/// The captured probe covering `world_pos` (see [`probe_index_at`]).
///
/// `None` when no such probe has a capture yet — the caller then keeps whatever
/// flat ambient term it already had, so a scene with no probe, or one whose
/// probe has never been captured, is byte-identical to before.
pub fn probe_reflection_at(
    probes: &[ReflectionProbe],
    world_pos: Vector3,
) -> Option<ProbeReflection> {
    ProbeReflection::from_probe(&probes[probe_index_at(probes, world_pos)?])
}

impl ProbeReflection {
    /// The flattened shading snapshot of a CAPTURED probe; `None` without a
    /// capture.
    pub fn from_probe(probe: &ReflectionProbe) -> Option<Self> {
        let cubemap = probe.captured.as_ref()?.clone();
        let (box_min, box_max) = probe.world_aabb_min_max();
        let ac = probe.ambient_color.srgb_to_linear();
        let e = probe.ambient_color_energy;
        Some(ProbeReflection {
            cubemap,
            box_min,
            box_max,
            origin: probe.capture_origin(),
            box_projection: probe.box_projection,
            intensity: probe.intensity,
            blend_distance: probe.blend_distance,
            ambient_mode: probe.ambient_mode,
            // light_storage.cpp: `ambient_color.srgb_to_linear() * energy`.
            ambient_color: Color::new(ac.r * e, ac.g * e, ac.b * e, 1.0),
        })
    }

    /// Whether `world_pos` is inside the influence box (Godot
    /// `reflection_process`: `any(abs(local_pos) > box_extents)` returns).
    pub fn contains(&self, world_pos: Vector3) -> bool {
        world_pos.x >= self.box_min.x
            && world_pos.x <= self.box_max.x
            && world_pos.y >= self.box_min.y
            && world_pos.y <= self.box_max.y
            && world_pos.z >= self.box_min.z
            && world_pos.z <= self.box_max.z
    }
}

/// The environment's sky radiance map (pat-h7sz7): the sky rendered into a
/// cubemap and GGX-prefiltered into the same [`RoughnessChain`] a probe
/// capture gets (Godot `sky.cpp` `update_radiance_buffers` +
/// `octmap_roughness.glsl`).
///
/// It is what a surface reflects wherever no probe covers it, and what fills
/// the `1 - blend` share of a probe's fade band
/// (`scene_forward_clustered.glsl:2086`).
#[derive(Debug, Clone, PartialEq)]
pub struct SkyRadiance {
    /// The sky, linear HDR, at energy 1 (the background energy is applied at
    /// lookup, as Godot does).
    pub cubemap: std::sync::Arc<ReflectionCubemap>,
    /// `ambient_light_color_energy.a` in Godot's scene data: the
    /// environment's `background_energy_multiplier`
    /// (`render_scene_data_rd.cpp:169`).
    pub energy: f32,
}

impl SkyRadiance {
    /// The sky's specular radiance for a fragment, Godot
    /// `scene_forward_clustered.glsl:1683-1714`:
    ///
    /// ```text
    /// ref_vec = mix(reflect(-view, normal), normal, roughness * roughness)
    /// horizon = min(1 + dot(ref_vec, normal), 1)
    /// radiance(ref_vec, sqrt(roughness) * MAX_ROUGHNESS_LOD) * horizon² * energy
    /// ```
    ///
    /// ⚠️ The sky mixes toward the normal by roughness², a probe by
    /// roughness⁴ (line 2054). Same file, different constants.
    pub fn specular(&self, n: Vector3, v: Vector3, roughness: f32) -> Color {
        let r = roughness.clamp(0.0, 1.0);
        let mirror = n * (2.0 * n.dot(v)) - v;
        let t = r * r;
        let ref_vec = mirror + (n - mirror) * t;
        let horizon = (1.0 + ref_vec.dot(n)).min(1.0);
        let k = horizon * horizon * self.energy;
        let c = self
            .cubemap
            .sample_lod(ref_vec, r.sqrt() * MAX_ROUGHNESS_LOD);
        Color::new(c.r * k, c.g * k, c.b * k, 1.0)
    }
}

/// Every captured probe of a frame plus the sky radiance, for PER-FRAGMENT
/// blending (pat-h7sz7).
///
/// Godot's clustered forward walks every probe whose box holds the fragment,
/// smallest box first, and accumulates reflection and ambient by each probe's
/// fade weight until both reach 1; whatever weight is left goes to the sky
/// radiance and the environment ambient (`scene_forward_clustered.glsl`
/// 2027-2099, `reflection_process` in `scene_forward_lights_inc.glsl`).
/// Patina used to pick ONE probe per instance at its origin, so a mesh whose
/// origin sat outside every box took the bright environment ambient over its
/// whole surface, and the next mesh in a GridMap wall the dark probe one: a
/// hard seam between two tiles of one wall (the platformer's f318 wall).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProbeSet {
    /// Captured probes in Godot's processing order: smallest box first.
    pub probes: std::sync::Arc<Vec<ProbeReflection>>,
    /// The sky radiance, when the environment reflects its sky.
    pub sky: Option<SkyRadiance>,
}

impl From<ProbeReflection> for ProbeSet {
    fn from(p: ProbeReflection) -> Self {
        Self {
            probes: std::sync::Arc::new(vec![p]),
            sky: None,
        }
    }
}

/// `probes` in Godot's processing order: Godot sorts by `-extents.length()`
/// (light_storage.cpp `update_reflection_probe_buffer`) and the cluster loop
/// walks bits with `findMSB`, so the SMALLEST box comes first. Ties keep list
/// order. Returns indices into `probes`.
pub fn probe_processing_order(probes: &[ReflectionProbe]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..probes.len()).collect();
    order.sort_by(|&a, &b| {
        let sa = (probes[a].size * 0.5).length();
        let sb = (probes[b].size * 0.5).length();
        sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
    });
    order
}

impl ProbeSet {
    /// The captured probes of `probes` (in processing order) and `sky`.
    /// `None` when there is neither, so a scene with no capture and no sky
    /// shades exactly as before.
    pub fn new(probes: &[ReflectionProbe], sky: Option<SkyRadiance>) -> Option<Self> {
        let list: Vec<ProbeReflection> = probe_processing_order(probes)
            .into_iter()
            .filter_map(|i| ProbeReflection::from_probe(&probes[i]))
            .collect();
        if list.is_empty() && sky.is_none() {
            return None;
        }
        Some(Self {
            probes: std::sync::Arc::new(list),
            sky,
        })
    }

    /// Godot's per-fragment reflection loop: `(ambient light, specular
    /// radiance)` at `world_pos` for unit normal `n` and unit view vector `v`
    /// (fragment to camera). The specular is the radiance BEFORE the
    /// environment BRDF; `base_ambient` is the environment ambient.
    pub fn indirect(
        &self,
        world_pos: Vector3,
        n: Vector3,
        v: Vector3,
        roughness: f32,
        base_ambient: Color,
    ) -> (Color, Color) {
        let r = roughness.clamp(0.0, 1.0);
        let mut refl = [0.0f32; 4];
        let mut amb = [0.0f32; 4];
        if !self.probes.is_empty() {
            let mirror = (n * (2.0 * n.dot(v)) - v).normalized();
            let r4 = r * r * r * r;
            let ref_vec = mirror + (n - mirror) * r4;
            let lod = r.sqrt() * MAX_ROUGHNESS_LOD;
            for p in self.probes.iter() {
                if !p.contains(world_pos) {
                    continue;
                }
                if refl[3] >= 1.0 && amb[3] >= 1.0 {
                    break;
                }
                let blend = p.blend_weight(world_pos);
                if p.intensity > 0.0 && refl[3] < 1.0 {
                    let rb = (blend - refl[3]).max(0.0);
                    let c = p.cubemap.sample_lod(p.lookup_dir(world_pos, ref_vec), lod);
                    let k = p.intensity * rb;
                    refl[0] += c.r * k;
                    refl[1] += c.g * k;
                    refl[2] += c.b * k;
                    refl[3] += rb;
                }
                if amb[3] < 1.0 {
                    let c = match p.ambient_mode {
                        ReflectionProbeAmbientMode::Disabled => None,
                        ReflectionProbeAmbientMode::Environment => {
                            Some(p.cubemap.sample_lod(n, MAX_ROUGHNESS_LOD))
                        }
                        ReflectionProbeAmbientMode::ConstantColor => Some(p.ambient_color),
                    };
                    if let Some(c) = c {
                        let ab = (blend - amb[3]).max(0.0);
                        amb[0] += c.r * ab;
                        amb[1] += c.g * ab;
                        amb[2] += c.b * ab;
                        amb[3] += ab;
                    }
                }
            }
        }
        if amb[3] < 1.0 {
            let k = 1.0 - amb[3];
            amb[0] += base_ambient.r * k;
            amb[1] += base_ambient.g * k;
            amb[2] += base_ambient.b * k;
        }
        if refl[3] < 1.0 {
            if let Some(sky) = &self.sky {
                let s = sky.specular(n, v, r);
                let k = 1.0 - refl[3];
                refl[0] += s.r * k;
                refl[1] += s.g * k;
                refl[2] += s.b * k;
            }
        }
        (
            Color::new(amb[0], amb[1], amb[2], 1.0),
            Color::new(refl[0], refl[1], refl[2], 1.0),
        )
    }
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

    /// pat-h7sz7: bilinear filtering crosses cube face seams (Godot's
    /// octmaps have a border). A lookup half a texel from the +X/+Y edge
    /// mixes in the neighbouring face; the old face-clamped lookup read only
    /// its own face, which drew a line along every cube edge of a blurred
    /// reflection.
    #[test]
    fn a_lookup_at_a_face_edge_filters_across_the_seam() {
        let res = 8;
        let mut faces: [Vec<Color>; 6] = std::array::from_fn(|_| vec![Color::BLACK; 64]);
        faces[CubeFace::PosX as usize] = vec![Color::WHITE; 64];
        let level = CubeLevel {
            resolution: res,
            faces,
        };
        // On +Y, u = 0.99 (x = 1, z = 0 is the +X edge): the last texel's
        // centre is at u = 0.9375, so the right-hand tap is past the edge.
        let dir = Vector3::new(0.98, 1.0, 0.0);
        let seamless = level.sample(dir);
        let clamped = level.sample_clamped(dir);
        assert_eq!(
            clamped.r, 0.0,
            "control: the clamped lookup sees only black +Y"
        );
        assert!(
            seamless.r > 0.2 && seamless.r < 0.8,
            "the seamless lookup must blend in the white +X face: {seamless:?}"
        );
        // Away from any edge nothing changes.
        let centre = Vector3::new(0.1, 1.0, 0.05);
        assert_eq!(level.sample(centre), level.sample_clamped(centre));
    }

    #[test]
    fn a_seam_texel_maps_to_the_adjacent_face() {
        // One texel right of +Y's right edge lies on +X.
        let (face, _) = seamless_texel_index(CubeFace::PosY as usize, 8, 8, 3);
        assert_eq!(face, CubeFace::PosX as usize);
        let (face, idx) = seamless_texel_index(CubeFace::PosY as usize, 8, 2, 3);
        assert_eq!((face, idx), (CubeFace::PosY as usize, 3 * 8 + 2));
    }

    fn captured(id: u64, centre: Vector3, size: f32, colour: Color) -> ReflectionProbe {
        let mut p = ReflectionProbe::new(ReflectionProbeId(id));
        p.transform.origin = centre;
        p.size = Vector3::new(size, size, size);
        p.blend_distance = 0.0;
        p.captured = Some(std::sync::Arc::new(ReflectionCubemap::new(4, colour)));
        p
    }

    /// pat-h7sz7: probes are blended per FRAGMENT, smallest box first, and a
    /// fragment outside a probe's box ignores it — whatever probe covers the
    /// instance's origin.
    #[test]
    fn probes_are_blended_per_fragment_smallest_box_first() {
        let red = Color::new(1.0, 0.0, 0.0, 1.0);
        let blue = Color::new(0.0, 0.0, 1.0, 1.0);
        // Big blue box listed FIRST, small red box inside it.
        let probes = [
            captured(1, Vector3::ZERO, 20.0, blue),
            captured(2, Vector3::ZERO, 4.0, red),
        ];
        let set = ProbeSet::new(&probes, None).unwrap();
        let n = Vector3::new(0.0, 1.0, 0.0);
        let base = Color::new(0.5, 0.5, 0.5, 1.0);
        let (amb_in, spec_in) = set.indirect(Vector3::ZERO, n, n, 0.0, base);
        assert!(
            spec_in.r > 0.99 && spec_in.b < 1e-4,
            "the smaller box wins: {spec_in:?}"
        );
        assert!(
            amb_in.r > 0.99 && amb_in.b < 1e-4,
            "its ambient too: {amb_in:?}"
        );
        let (amb_out, spec_out) = set.indirect(Vector3::new(5.0, 0.0, 0.0), n, n, 0.0, base);
        assert!(
            spec_out.b > 0.99 && spec_out.r < 1e-4,
            "outside the small box, the big one: {spec_out:?}"
        );
        assert!(amb_out.b > 0.99, "{amb_out:?}");
        let (amb_far, spec_far) = set.indirect(Vector3::new(50.0, 0.0, 0.0), n, n, 0.0, base);
        assert_eq!(
            spec_far,
            Color::new(0.0, 0.0, 0.0, 1.0),
            "no probe, no sky: black"
        );
        assert_eq!(amb_far, base, "no probe: the environment ambient");
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
        // Godot: `origin_offset` moves the capture point, NOT the influence
        // box, which stays centred on the probe origin (-10..10 on X). The old
        // expectation here (-5..15) encoded the shifted-box misconception.
        assert!(probe.contains_point(Vector3::new(-9.0, 0.0, 0.0)));
        assert!(!probe.contains_point(Vector3::new(14.0, 0.0, 0.0)));
        assert_eq!(probe.capture_origin(), Vector3::new(5.0, 0.0, 0.0));
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
