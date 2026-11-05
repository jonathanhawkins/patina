//! Shadow map generation for directional and spot lights.
//!
//! Directional lights use an orthographic light-space projection; spot lights
//! use a perspective projection matching the cone FOV. Depth is stored in a
//! [`DepthBuffer`] and sampled during the main pass.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdserver3d::instance::Instance3D;
use gdserver3d::light::{CubeFace, Light3D, Light3DId, LightType, OmniShadowMode, ShadowCubemap};
use gdserver3d::projection::orthographic_projection_matrix;

use crate::depth_buffer::DepthBuffer;

/// Resolution of the shadow map (width and height in texels).
pub const SHADOW_MAP_SIZE: u32 = 256;

/// Half-extent of the orthographic frustum for directional light shadow maps.
/// Objects within [-SHADOW_EXTENT, SHADOW_EXTENT] along each axis from the
/// scene center will be captured.
pub const SHADOW_EXTENT: f32 = 20.0;

/// Near and far planes for the shadow camera.
pub const SHADOW_NEAR: f32 = 0.1;
pub const SHADOW_FAR: f32 = 100.0;

/// Shadow bias to prevent self-shadowing artifacts (acne).
pub const SHADOW_BIAS: f32 = 0.005;

/// A generated shadow map for a single light source.
#[derive(Debug, Clone)]
pub struct ShadowMap {
    /// The light this shadow map belongs to.
    pub light_id: Light3DId,
    /// Depth buffer rendered from the light's perspective.
    pub depth: DepthBuffer,
    /// Light-space view matrix (inverse of light transform).
    pub view_matrix: [[f32; 4]; 4],
    /// Light-space orthographic projection matrix.
    pub proj_matrix: [[f32; 4]; 4],
    /// Shadow map resolution.
    pub size: u32,
    /// Godot's filter for a camera-fitted directional map (pat-772py), or
    /// `None` for the legacy fixed-box maps, which keep their old single
    /// nearest-texel compare.
    pub filter: Option<ShadowFilter>,
}

/// How a camera-fitted directional map is compared (pat-772py), all derived
/// the way Godot derives them in `light_storage.cpp:696-724`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowFilter {
    /// Constant bias, already in NORMALIZED depth of this map:
    /// `shadow_bias / 100 * z_range * blur * quality_radius`, then times
    /// `2 / z_range` for the `[-1, 1]` orthographic depth this map stores.
    pub depth_bias: f32,
    /// World size of one texel of THIS map (what `shadow_normal_bias` scales).
    pub texel_world: f32,
    /// PCF radius in texels of this map. Godot's radius is
    /// `blur * quality_radius` texels of ITS atlas; converted through world
    /// units so the penumbra is as wide as Godot's at any resolution here.
    pub radius_texels: f32,
    /// Vogel-disk taps (0 = one bilinear tap, Godot's HARD level).
    pub taps: u32,
}

impl ShadowMap {
    /// Tests whether a world-space point is in shadow.
    ///
    /// Returns a shadow factor: 0.0 = fully lit, 1.0 = fully in shadow.
    /// [`sample`](Self::sample) with the light's authored `shadow_bias`, in
    /// WORLD UNITS.
    ///
    /// 🔑 The stored comparison is in NORMALIZED post-divide depth, so a world
    /// bias has to be scaled by the shadow frustum's depth range before it means
    /// anything here — Godot's 0.1 becomes ≈0.001 against the 0.1..100 range,
    /// where the constant this replaces was 0.005. Feeding the raw 0.1 straight
    /// in would be a hundred times too large and detach every shadow from its
    /// caster.
    ///
    /// ⚠️ EXACT for the DIRECTIONAL light's orthographic projection, where
    /// normalized depth is linear in world depth. A SpotLight3D uses a
    /// perspective projection (`spot_light_proj_matrix`), where it is not — the
    /// true conversion varies with the fragment's own depth. This is a uniform
    /// approximation for that case: right at the far plane, increasingly
    /// conservative closer in. Doing it exactly needs the view-space depth at
    /// the sample point, which is computed inside `sample` and not exposed.
    pub fn sample_with_bias(&self, world_pos: Vector3, world_bias: f32) -> f32 {
        let range = (SHADOW_FAR - SHADOW_NEAR).max(1e-6);
        self.sample_normalized_bias(world_pos, world_bias / range)
    }

    /// Percentage-closer filtering: averages `taps` samples spread over a disc
    /// of `blur` shadow-map texels, giving a soft penumbra instead of a hard
    /// edge (Godot's `Light3D.shadow_blur` + `shadow_filter_quality`).
    ///
    /// ⚠️ ADDITIVE BY CONSTRUCTION. `ShadowFilterQuality::Hard` is Godot's
    /// default and reports **1** tap, and a single tap with any blur reduces to
    /// exactly `sample_with_bias` — same call, same result. Nothing softens
    /// until a scene asks for it.
    ///
    /// ⚠️ The offsets are applied in WORLD space along the shadow map's own
    /// view axes, not in screen space and not to the world position blindly.
    /// The light's view matrix is stored column-major, so its first two ROWS —
    /// `m[0][0..2]` and `m[1][0..2]` read down the columns — are the light's
    /// right and up vectors. Offsetting along world X/Y instead would smear the
    /// penumbra along whichever way the light happens to face, and would
    /// collapse entirely for a light pointing down +X.
    ///
    /// Taps walk a Vogel/golden-angle spiral: it needs no lookup table, spreads
    /// evenly for ANY tap count (4, 8 and 16 are the quality levels, but the
    /// distribution does not care), and unlike a square kernel it has no
    /// preferred axis for edges to align with.
    pub fn sample_with_bias_filtered(
        &self,
        world_pos: Vector3,
        world_bias: f32,
        taps: u32,
        blur: f32,
    ) -> f32 {
        if taps <= 1 || blur <= 0.0 {
            return self.sample_with_bias(world_pos, world_bias);
        }
        // World size of one texel of this map, so `blur` stays in texels the way
        // Godot authors it.
        let texel = 2.0 * SHADOW_EXTENT / SHADOW_MAP_SIZE as f32;
        let radius = blur * texel;
        let right = Vector3::new(
            self.view_matrix[0][0],
            self.view_matrix[1][0],
            self.view_matrix[2][0],
        );
        let up = Vector3::new(
            self.view_matrix[0][1],
            self.view_matrix[1][1],
            self.view_matrix[2][1],
        );

        // Golden angle in radians: pi * (3 - sqrt(5)).
        const GOLDEN_ANGLE: f32 = 2.399_963_2;
        let mut sum = 0.0;
        for i in 0..taps {
            // sqrt spacing keeps the samples area-uniform rather than bunched
            // at the centre; the +0.5 keeps the first tap off the exact origin.
            let t = (i as f32 + 0.5) / taps as f32;
            let r = radius * t.sqrt();
            let a = i as f32 * GOLDEN_ANGLE;
            let offset = right * (r * a.cos()) + up * (r * a.sin());
            sum += self.sample_with_bias(world_pos + offset, world_bias);
        }
        sum / taps as f32
    }

    /// Tests a point with the module's default normalized bias.
    ///
    /// Kept so its seven existing callers — all tests — are unaffected by the
    /// authored-bias work.
    pub fn sample(&self, world_pos: Vector3) -> f32 {
        self.sample_normalized_bias(world_pos, SHADOW_BIAS)
    }

    fn sample_normalized_bias(&self, world_pos: Vector3, bias: f32) -> f32 {
        // Transform to light space.
        let view_pos = mat4_mul_point(&self.view_matrix, world_pos);
        let clip = mat4_mul_point4(&self.proj_matrix, view_pos);

        // Perspective divide (orthographic, so w=1 but be safe).
        let w = clip[3];
        if w <= 0.0 {
            return 0.0;
        }
        let ndc_x = clip[0] / w;
        let ndc_y = clip[1] / w;
        let depth = clip[2] / w;

        // Map NDC [-1,1] to texel coords [0, size]. `floor` (not `as i32`
        // truncation) so texel-space u ∈ (−1, 0) rejects as out-of-frustum
        // instead of aliasing onto column 0 — matching the GPU sampler.
        let u = (ndc_x + 1.0) * 0.5 * self.size as f32;
        let v = (1.0 - ndc_y) * 0.5 * self.size as f32;

        let tx = u.floor() as i32;
        let ty = v.floor() as i32;

        if tx < 0 || tx >= self.size as i32 || ty < 0 || ty >= self.size as i32 {
            return 0.0; // Outside shadow map = lit.
        }

        let stored_depth = self.depth.get(tx as u32, ty as u32);
        if stored_depth >= f32::MAX {
            return 0.0; // No occluder at this texel.
        }

        // Compare: if fragment is farther than stored depth + bias, it's in shadow.
        if depth > stored_depth + bias {
            1.0
        } else {
            0.0
        }
    }
}

/// Builds a view matrix (world → light space) for a directional light.
///
/// The light "looks" along its direction from a position high above the scene
/// center.
pub fn directional_light_view_matrix(light: &Light3D) -> [[f32; 4]; 4] {
    let dir = light.direction.normalized();

    // Build a look-at transform: light looking along `dir`.
    // Pick an up vector that isn't parallel to dir.
    let up_candidate = if dir.y.abs() > 0.99 {
        Vector3::new(0.0, 0.0, 1.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };

    let right = up_candidate.cross(dir).normalized();
    let up = dir.cross(right).normalized();

    // Light position: offset opposite to direction so scene is in front.
    let light_pos = dir * -50.0;

    let light_transform = Transform3D {
        basis: Basis {
            x: right,
            y: up,
            z: dir * -1.0, // -Z is forward in our convention.
        },
        origin: light_pos,
    };

    transform_to_matrix(&light_transform.inverse())
}

/// Builds an orthographic projection for the shadow map.
pub fn directional_light_proj_matrix() -> [[f32; 4]; 4] {
    orthographic_projection_matrix(
        -SHADOW_EXTENT,
        SHADOW_EXTENT,
        -SHADOW_EXTENT,
        SHADOW_EXTENT,
        SHADOW_NEAR,
        SHADOW_FAR,
    )
}

/// Generates shadow maps for all directional **and spot** lights with
/// `shadow_enabled = true` (point lights use cubemaps separately).
pub fn generate_shadow_maps(lights: &[Light3D], instances: &[Instance3D]) -> Vec<ShadowMap> {
    lights
        .iter()
        .filter(|l| {
            l.shadow_enabled && matches!(l.light_type, LightType::Directional | LightType::Spot)
        })
        .map(|l| match l.light_type {
            LightType::Spot => generate_spot_shadow_map(l, instances),
            _ => generate_directional_shadow_map(l, instances),
        })
        .collect()
}

/// The shadow casters already rasterized into a base depth buffer per shadow
/// map, reused across frames by [`generate_shadow_maps_incremental`]
/// (pat-414k3).
#[derive(Debug, Default)]
pub struct ShadowCasterCache {
    /// Signature of the lights the bases were rasterized for.
    light_sig: u64,
    /// How many instances of each caster key the bases hold.
    base_counts: std::collections::HashMap<u64, u32>,
    /// One base depth buffer per shadow map, in `generate_shadow_maps` order.
    base: Vec<DepthBuffer>,
    /// How many calls in a row each caster key has been present. A caster
    /// unchanged for [`STATIC_CASTER_CALLS`] calls is treated as static and
    /// goes into the next rebuilt base; a body that merely paused for a frame
    /// or two would otherwise join the base and force a rebuild when it moves.
    ages: std::collections::HashMap<u64, u32>,
    /// How many times a base was rebuilt. Instrumentation for the tests.
    pub base_rebuilds: u64,
    /// How many casters the last call rasterized on top of the base.
    pub last_redrawn: usize,
}

/// See [`ShadowCasterCache::ages`].
const STATIC_CASTER_CALLS: u32 = 30;

/// [`generate_shadow_maps`] for a scene where most casters do not move between
/// calls (pat-414k3).
///
/// A CPU shadow map is the per-texel MINIMUM depth over every caster
/// triangle, and a minimum does not depend on the order it is taken in. So
/// the casters that have not changed since the previous call are rasterized
/// once into a cached base, and each call copies the base and rasterizes only
/// the casters the base does not hold — the player, the enemies and the
/// spinning coins, rather than thousands of GridMap tiles. The maps are
/// bit-identical to [`generate_shadow_maps`] over the same instances.
///
/// `keys[i]` must change whenever anything `instances[i]` contributes to a
/// shadow map changes (its mesh, transform, visibility, `cast_shadow` or
/// layers); `light_sig` likewise for the lights' shadow parameters.
pub fn generate_shadow_maps_incremental(
    lights: &[Light3D],
    instances: &[&Instance3D],
    keys: &[u64],
    light_sig: u64,
    cache: &mut ShadowCasterCache,
) -> Vec<ShadowMap> {
    use std::collections::HashMap;
    debug_assert_eq!(instances.len(), keys.len());
    let casting: Vec<&Light3D> = lights
        .iter()
        .filter(|l| {
            l.shadow_enabled && matches!(l.light_type, LightType::Directional | LightType::Spot)
        })
        .collect();
    // One frame per casting light: a camera-fitted directional map is larger
    // than a spot map (pat-772py).
    let frames: Vec<DirectionalShadowFrame> = casting
        .iter()
        .map(|l| match l.light_type {
            LightType::Spot => DirectionalShadowFrame {
                view: spot_light_view_matrix(l),
                proj: spot_light_proj_matrix(l),
                size: SHADOW_MAP_SIZE,
                filter: None,
            },
            _ => directional_shadow_frame(l),
        })
        .collect();

    let mut available: HashMap<u64, u32> = HashMap::new();
    for k in keys {
        *available.entry(*k).or_default() += 1;
    }
    let ages: HashMap<u64, u32> = available
        .keys()
        .map(|k| (*k, cache.ages.get(k).copied().unwrap_or(0) + 1))
        .collect();
    cache.ages = ages;
    // Everything the base does not already hold, counted per key so two
    // identical casters at one spot are both drawn.
    let outside_base = |counts: &HashMap<u64, u32>| -> Vec<usize> {
        let mut held = counts.clone();
        (0..instances.len())
            .filter(|i| match held.get_mut(&keys[*i]) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    false
                }
                _ => true,
            })
            .collect()
    };
    let base_valid = cache.light_sig == light_sig
        && cache.base.len() == casting.len()
        && cache
            .base_counts
            .iter()
            .all(|(k, n)| available.get(k).copied().unwrap_or(0) >= *n);
    let mut rest = outside_base(&cache.base_counts);
    // Fold casters that have settled into the base once they are at least half
    // of what is redrawn every call — on the first frames that is the whole
    // level, later only what has come to rest.
    let settled = rest
        .iter()
        .filter(|i| cache.ages[&keys[**i]] >= STATIC_CASTER_CALLS)
        .count();
    if !base_valid || (settled > 0 && settled * 2 >= rest.len()) {
        // Rebuild from the casters that have sat still for a while.
        let stable: Vec<usize> = (0..instances.len())
            .filter(|i| cache.ages[&keys[*i]] >= STATIC_CASTER_CALLS)
            .collect();
        cache.base = casting
            .iter()
            .zip(&frames)
            .map(|(l, f)| {
                let mut depth = DepthBuffer::new(f.size, f.size);
                rasterize_instance_refs_into_shadow(
                    stable.iter().map(|i| instances[*i]),
                    l.shadow_caster_mask,
                    &f.view,
                    &f.proj,
                    &mut depth,
                    f.size,
                );
                depth
            })
            .collect();
        cache.base_counts.clear();
        for i in &stable {
            *cache.base_counts.entry(keys[*i]).or_default() += 1;
        }
        cache.light_sig = light_sig;
        cache.base_rebuilds += 1;
        rest = outside_base(&cache.base_counts);
    }
    let rest: Vec<&Instance3D> = rest.into_iter().map(|i| instances[i]).collect();
    cache.last_redrawn = rest.len();

    casting
        .iter()
        .zip(&cache.base)
        .zip(&frames)
        .map(|((l, base), f)| {
            let mut depth = base.clone();
            rasterize_instance_refs_into_shadow(
                rest.iter().copied(),
                l.shadow_caster_mask,
                &f.view,
                &f.proj,
                &mut depth,
                f.size,
            );
            ShadowMap {
                light_id: l.id,
                depth,
                view_matrix: f.view,
                proj_matrix: f.proj,
                size: f.size,
                filter: f.filter,
            }
        })
        .collect()
}

/// Builds a view matrix looking from the spot light position along its direction.
pub fn spot_light_view_matrix(light: &Light3D) -> [[f32; 4]; 4] {
    let dir = light.direction.normalized();
    let up_candidate = if dir.y.abs() > 0.99 {
        Vector3::new(0.0, 0.0, 1.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let right = up_candidate.cross(dir).normalized();
    let up = dir.cross(right).normalized();
    let light_transform = Transform3D {
        basis: Basis {
            x: right,
            y: up,
            z: dir * -1.0,
        },
        origin: light.position,
    };
    transform_to_matrix(&light_transform.inverse())
}

/// Perspective projection for a spot light (FOV = 2 * spot_angle).
pub fn spot_light_proj_matrix(light: &Light3D) -> [[f32; 4]; 4] {
    // Clamp below π: at Godot's max spot_angle (90°) the full fov reaches π,
    // tan(fov/2) → ∞ and f → 0, collapsing every fragment to the center texel.
    let fov = (light.spot_angle * 2.0).clamp(0.05, std::f32::consts::PI - 0.01);
    let near = SHADOW_NEAR;
    let far = if light.range > 0.0 {
        light.range.max(near + 0.1)
    } else {
        SHADOW_FAR
    };
    // Column-major perspective (same convention as perspective_90_projection).
    let f = 1.0 / (fov * 0.5).tan();
    [
        [f, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, -(far + near) / (far - near), -1.0],
        [0.0, 0.0, -2.0 * far * near / (far - near), 0.0],
    ]
}

/// Generates a perspective shadow map for a spot light.
fn generate_spot_shadow_map(light: &Light3D, instances: &[Instance3D]) -> ShadowMap {
    let size = SHADOW_MAP_SIZE;
    let mut depth_buf = DepthBuffer::new(size, size);
    let view_matrix = spot_light_view_matrix(light);
    let proj_matrix = spot_light_proj_matrix(light);
    rasterize_instances_into_shadow(
        instances,
        light.shadow_caster_mask,
        &view_matrix,
        &proj_matrix,
        &mut depth_buf,
        size,
    );
    ShadowMap {
        light_id: light.id,
        depth: depth_buf,
        view_matrix,
        proj_matrix,
        size,
        filter: None,
    }
}

/// Rasterizes all shadow-casting mesh instances into a shadow depth buffer.
///
/// ⚠️ Filters on [`Instance3D::casts_shadow`], not `visible` (pat-nzgdu.76):
/// `cast_shadow = Off` must drop out of the shadow map while still being drawn
/// in the colour pass, and `ShadowsOnly` is the reverse. This is the single
/// chokepoint for the directional, spot and cascade shadow maps, so filtering
/// here covers all three.
/// Whether `inst` contributes to a shadow map for a light whose
/// `shadow_caster_mask` is `caster_mask`.
///
/// Godot tests `instance->layer_mask & light_get_shadow_caster_mask(light)`
/// alongside the per-instance `can_cast_shadows`
/// (`renderer_scene_cull.cpp:2415`). Both are BITMASKS and the test is an
/// intersection — reading either as an index agrees for every single-layer mask
/// (including the default 1) and diverges from layer 3 up.
///
/// ⚠️ One predicate on purpose. The 2D and omni-cubemap rasterizers filter
/// separately, and a caster mask honoured in one but not the other means a mesh
/// that is excluded from a spot light's shadow and included in an omni's — the
/// same divergence class the `cast_shadow` filter already warns about below.
///
/// ⚠️ An instance on NO layer (`layers == 0`) casts nothing, whatever the mask
/// says. That is Godot's behaviour and falls out of the intersection.
pub fn casts_into_shadow(inst: &Instance3D, caster_mask: u32) -> bool {
    inst.casts_shadow() && (inst.geometry.layers & caster_mask) != 0
}

fn rasterize_instances_into_shadow(
    instances: &[Instance3D],
    caster_mask: u32,
    view_matrix: &[[f32; 4]; 4],
    proj_matrix: &[[f32; 4]; 4],
    depth_buf: &mut DepthBuffer,
    size: u32,
) {
    rasterize_instance_refs_into_shadow(
        instances.iter(),
        caster_mask,
        view_matrix,
        proj_matrix,
        depth_buf,
        size,
    );
}

/// [`rasterize_instances_into_shadow`] over borrowed instances.
fn rasterize_instance_refs_into_shadow<'a>(
    instances: impl IntoIterator<Item = &'a Instance3D>,
    caster_mask: u32,
    view_matrix: &[[f32; 4]; 4],
    proj_matrix: &[[f32; 4]; 4],
    depth_buf: &mut DepthBuffer,
    size: u32,
) {
    for inst in instances {
        if !casts_into_shadow(inst, caster_mask) {
            continue;
        }
        let mesh = match &inst.mesh {
            Some(m) => m,
            None => continue,
        };
        let model_matrix = transform_to_matrix(&inst.transform);
        let indices = &mesh.indices;
        let verts = &mesh.vertices;
        let mut i = 0;
        while i + 2 < indices.len() {
            let tri = [
                indices[i] as usize,
                indices[i + 1] as usize,
                indices[i + 2] as usize,
            ];
            i += 3;
            let mut screen_pts: [(f32, f32, f32); 3] = [(0.0, 0.0, 0.0); 3];
            let mut valid = true;
            for (j, &vi) in tri.iter().enumerate() {
                if vi >= verts.len() {
                    valid = false;
                    break;
                }
                let world = mat4_mul_point(&model_matrix, verts[vi]);
                let view = mat4_mul_point(view_matrix, world);
                let clip = mat4_mul_point4(proj_matrix, view);
                let w = clip[3];
                if w <= 0.0 {
                    valid = false;
                    break;
                }
                let ndc_x = clip[0] / w;
                let ndc_y = clip[1] / w;
                let depth = clip[2] / w;
                let sx = (ndc_x + 1.0) * 0.5 * size as f32;
                let sy = (1.0 - ndc_y) * 0.5 * size as f32;
                screen_pts[j] = (sx, sy, depth);
            }
            if !valid {
                continue;
            }
            rasterize_shadow_triangle(&screen_pts, depth_buf, size, size);
        }
    }
}

/// Resolution for omni shadow cubemap faces.
pub const OMNI_SHADOW_SIZE: u32 = 128;

/// Generates cubemap shadow maps for all shadow-casting point lights, in BOTH
/// omni shadow modes — the Godot default `DualParaboloid` and `Cube`.
///
/// Godot's omni lights default to `DualParaboloid`; previously only `Cube`
/// lights got a map here, so every default omni light was silently shadowless
/// in both the software and wgpu paths (which share this generator via
/// `pack_omni_cubemaps`). A cubemap is a valid occlusion representation for
/// either mode, so we render one for both (pat-u3l8c.35).
///
/// Returns a vec parallel to `lights` — entries are `Some` for qualifying
/// point lights, `None` otherwise.
pub fn generate_omni_shadow_cubemaps(
    lights: &[Light3D],
    instances: &[Instance3D],
) -> Vec<Option<ShadowCubemap>> {
    lights
        .iter()
        .map(|l| {
            if l.shadow_enabled
                && l.light_type == LightType::Point
                && matches!(
                    l.omni_shadow_mode,
                    OmniShadowMode::DualParaboloid | OmniShadowMode::Cube
                )
            {
                Some(generate_omni_shadow_cubemap(l, instances))
            } else {
                None
            }
        })
        .collect()
}

/// Generates a single cubemap shadow map for a point light.
///
/// Renders the scene from the light's position into 6 cubemap faces using a
/// 90-degree perspective projection. Depth values are linear distance from light.
fn generate_omni_shadow_cubemap(light: &Light3D, instances: &[Instance3D]) -> ShadowCubemap {
    let size = OMNI_SHADOW_SIZE;
    let mut cubemap = ShadowCubemap::new(size);
    let near = 0.05_f32;
    let far = if light.range > 0.0 {
        light.range
    } else {
        100.0
    };

    for face in CubeFace::ALL {
        for inst in instances {
            // Same filter as the 2D shadow path (pat-nzgdu.76), through the
            // SHARED predicate — an omni light must not shadow from an instance
            // the directional path already excluded, and that now includes
            // `shadow_caster_mask` (pat-s3rrv.90).
            if !casts_into_shadow(inst, light.shadow_caster_mask) {
                continue;
            }
            let mesh = match &inst.mesh {
                Some(m) => m,
                None => continue,
            };

            let model_matrix = transform_to_matrix(&inst.transform);
            let mut i = 0;
            while i + 2 < mesh.indices.len() {
                let tri = [
                    mesh.indices[i] as usize,
                    mesh.indices[i + 1] as usize,
                    mesh.indices[i + 2] as usize,
                ];
                i += 3;

                let mut screen_pts: [(f32, f32, f32); 3] = [(0.0, 0.0, 0.0); 3];
                let mut valid = true;

                for (j, &vi) in tri.iter().enumerate() {
                    if vi >= mesh.vertices.len() {
                        valid = false;
                        break;
                    }
                    let world = mat4_mul_point(&model_matrix, mesh.vertices[vi]);
                    // Project with the SAME face table `ShadowCubemap`'s
                    // sampler uses (`direction_to_face_uv`), so generator and
                    // sampler can never disagree on a texel — the previous
                    // view/projection matrices used ad-hoc face bases that
                    // mirrored some faces and silently voided off-center
                    // shadows. `ma` is the direction's dominant-axis extent
                    // toward this face; behind or beyond range rejects.
                    let rel = world - light.position;
                    let (sc, tc) = face_sc_tc(face, rel);
                    let ma = rel.dot(face.forward());
                    let depth = rel.length();
                    if ma <= near || depth > far {
                        valid = false;
                        break;
                    }
                    let u = (sc / ma) * 0.5 + 0.5;
                    let v = (tc / ma) * 0.5 + 0.5;
                    let sx = u * size as f32;
                    let sy = v * size as f32;

                    screen_pts[j] = (sx, sy, depth);
                }

                if !valid {
                    continue;
                }

                rasterize_cubemap_face_triangle(&screen_pts, &mut cubemap, face, size);
            }
        }
    }

    cubemap
}

/// The (sc, tc) face-plane coordinates for a direction on a given cube face —
/// the **same table** `ShadowCubemap::direction_to_face_uv` uses to sample, so
/// generation and sampling always agree (standard GL cubemap orientation).
fn face_sc_tc(face: CubeFace, dir: Vector3) -> (f32, f32) {
    match face {
        CubeFace::PositiveX => (-dir.z, -dir.y),
        CubeFace::NegativeX => (dir.z, -dir.y),
        CubeFace::PositiveY => (dir.x, dir.z),
        CubeFace::NegativeY => (dir.x, -dir.z),
        CubeFace::PositiveZ => (dir.x, -dir.y),
        CubeFace::NegativeZ => (-dir.x, -dir.y),
    }
}

/// Rasterizes a triangle into a cubemap face's depth storage.
fn rasterize_cubemap_face_triangle(
    pts: &[(f32, f32, f32); 3],
    cubemap: &mut ShadowCubemap,
    face: CubeFace,
    size: u32,
) {
    let (x0, y0, z0) = pts[0];
    let (x1, y1, z1) = pts[1];
    let (x2, y2, z2) = pts[2];

    let min_x = x0.min(x1).min(x2).max(0.0) as i32;
    let max_x = x0.max(x1).max(x2).min(size as f32 - 1.0) as i32;
    let min_y = y0.min(y1).min(y2).max(0.0) as i32;
    let max_y = y0.max(y1).max(y2).min(size as f32 - 1.0) as i32;

    if min_x > max_x || min_y > max_y {
        return;
    }

    let area = edge_function(x0, y0, x1, y1, x2, y2);
    if area.abs() < 1e-6 {
        return;
    }
    let area_recip = 1.0 / area;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let px_f = px as f32 + 0.5;
            let py_f = py as f32 + 0.5;

            let w0 = edge_function(x1, y1, x2, y2, px_f, py_f);
            let w1 = edge_function(x2, y2, x0, y0, px_f, py_f);
            let w2 = edge_function(x0, y0, x1, y1, px_f, py_f);

            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };

            if !inside {
                continue;
            }

            let b0 = w0 * area_recip;
            let b1 = w1 * area_recip;
            let b2 = w2 * area_recip;

            let depth = b0 * z0 + b1 * z1 + b2 * z2;
            cubemap.test_and_set(face, px as u32, py as u32, depth);
        }
    }
}

/// Generates a single shadow map for a directional light.
fn generate_directional_shadow_map(light: &Light3D, instances: &[Instance3D]) -> ShadowMap {
    let frame = directional_shadow_frame(light);
    let size = frame.size;
    let mut depth_buf = DepthBuffer::new(size, size);
    rasterize_instances_into_shadow(
        instances,
        light.shadow_caster_mask,
        &frame.view,
        &frame.proj,
        &mut depth_buf,
        size,
    );
    ShadowMap {
        light_id: light.id,
        depth: depth_buf,
        view_matrix: frame.view,
        proj_matrix: frame.proj,
        size,
        filter: frame.filter,
    }
}

/// Upper bound on the resolution of a camera-fitted directional map
/// (pat-772py).
///
/// Godot renders the map on the GPU at `directional_shadow/size` (8192 in the
/// 3D platformer). This one is rasterized on the CPU and uploaded, so it is
/// capped. The PCF radius is converted through WORLD units
/// ([`ShadowFilter::radius_texels`]), so the penumbra keeps Godot's width and
/// only detail finer than one texel here is lost.
pub const DIRECTIONAL_FIT_MAX_SIZE: u32 = 2048;

/// The light-space frame a directional or spot shadow map is rasterized and
/// sampled in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalShadowFrame {
    /// World -> light view matrix (column-major).
    pub view: [[f32; 4]; 4],
    /// Projection (column-major).
    pub proj: [[f32; 4]; 4],
    /// Map resolution.
    pub size: u32,
    /// `Some` for a camera-fitted map; `None` for the legacy fixed box.
    pub filter: Option<ShadowFilter>,
}

/// The frame for a directional light: fitted to [`Light3D::shadow_camera`]
/// when one is set, else the legacy `±SHADOW_EXTENT` box at the origin.
///
/// The fit is Godot's ORTHOGONAL-mode `_light_instance_setup_directional_shadow`
/// (`renderer_scene_cull.cpp:2134`). The camera frustum slice from `near` to
/// `min(far, directional_shadow_max_distance)` is bounded by a sphere around the
/// mean of its 8 corners, grown by one texel per side. The map spans that
/// sphere across the light's X/Y axes, and along Z from `radius` behind the
/// centre to `radius + pancake_size` toward the light.
///
/// 🔑 Two deliberate departures, both affecting resolution only:
///
/// - The 2- and 4-split modes get ONE map over the whole range (there are no
///   cascades here), so their near shadows are coarser than Godot's.
/// - The sphere centre is snapped to a grid of `radius / 8`, not Godot's
///   `4 * radius / size`, and the half-extent grows by half a step to keep the
///   frustum covered. Godot snaps to stop jaggies crawling. This snaps so the
///   map's static casters stay valid for many frames, because the incremental
///   generator reuses them only while the frame is unchanged (pat-414k3).
pub fn directional_shadow_frame(light: &Light3D) -> DirectionalShadowFrame {
    let Some(cam) = light.shadow_camera else {
        return DirectionalShadowFrame {
            view: directional_light_view_matrix(light),
            proj: directional_light_proj_matrix(),
            size: SHADOW_MAP_SIZE,
            filter: None,
        };
    };
    let params = light.directional_shadow;

    let mut max_distance = cam.far;
    if light.directional_shadow_max_distance > 0.0 {
        max_distance = max_distance.min(light.directional_shadow_max_distance);
    }
    let max_distance = max_distance.max(cam.near + 0.001);
    let min_distance = cam.near.min(max_distance);

    // The frustum slice's corners. Camera3D looks down its local -Z and the
    // fov is vertical (the caller has already resolved KEEP_WIDTH).
    let tan_half = (cam.fov_y * 0.5).tan();
    let mut corners = [Vector3::ZERO; 8];
    let mut k = 0;
    for d in [min_distance, max_distance] {
        for sy in [-1.0f32, 1.0] {
            for sx in [-1.0f32, 1.0] {
                let local = Vector3::new(sx * d * tan_half * cam.aspect, sy * d * tan_half, -d);
                corners[k] = cam.transform.xform(local);
                k += 1;
            }
        }
    }
    let mut center = Vector3::ZERO;
    for c in &corners {
        center = center + *c;
    }
    center = center * (1.0 / 8.0);
    let mut radius = 0.0f32;
    for c in &corners {
        radius = radius.max((*c - center).length());
    }
    // Godot: "add a texel by each side", at ITS split size.
    let godot_size = params.split_size() as f32;
    if godot_size > 2.0 {
        radius *= godot_size / (godot_size - 2.0);
    }
    let radius = radius.max(1e-3);

    // The light basis, as `directional_light_view_matrix` builds it. Only the
    // rotation about the light ray differs from Godot's (which uses the node's
    // own X/Y), and a sphere fit does not depend on it.
    let dir = light.direction.normalized();
    let up_candidate = if dir.y.abs() > 0.99 {
        Vector3::new(0.0, 0.0, 1.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let right = up_candidate.cross(dir).normalized();
    let up = dir.cross(right).normalized();
    let z_vec = dir * -1.0; // points TOWARD the light, Godot's z_vec

    let step = radius / 8.0;
    let snap = |v: f32| (v / step).round() * step;
    let cx = snap(right.dot(center));
    let cy = snap(up.dot(center));
    let cz = snap(z_vec.dot(center));
    let half = radius + step * 0.5;
    let pancake = params.pancake_size.max(0.0);
    let z_max = cz + half + pancake;
    let z_min = cz - half;
    let z_range = (z_max - z_min).max(1e-3);

    let light_transform = Transform3D {
        basis: Basis {
            x: right,
            y: up,
            z: z_vec,
        },
        origin: right * cx + up * cy + z_vec * z_max,
    };
    let view = transform_to_matrix(&light_transform.inverse());
    let proj = orthographic_projection_matrix(-half, half, -half, half, 0.0, z_range);

    let size = params.split_size().min(DIRECTIONAL_FIT_MAX_SIZE).max(16);
    let texel_world = 2.0 * half / size as f32;

    // Godot's own numbers for the filter and bias (`light_storage.cpp:696`,
    // `:721-723`; `renderer_scene_cull.cpp:2365-2367`). `soft_shadow_scale` is
    // `shadow_blur`, times the quality radius for PCF (no angular size).
    let (samples, quality_radius) = params.soft_samples_and_radius();
    let soft_scale = if light.angular_distance > 0.0 {
        params.blur
    } else {
        params.blur * quality_radius
    };
    let godot_texel = radius * 2.0 / godot_size;
    let godot_z_range = 2.0 * radius + pancake;
    let bias_world = light.shadow_bias / 100.0 * godot_z_range * soft_scale;
    let filter = ShadowFilter {
        depth_bias: bias_world * 2.0 / z_range,
        texel_world,
        radius_texels: soft_scale * godot_texel / texel_world,
        taps: samples,
    };

    DirectionalShadowFrame {
        view,
        proj,
        size,
        filter: Some(filter),
    }
}

/// Rasterizes a triangle into a depth buffer only (no color output).
fn rasterize_shadow_triangle(
    pts: &[(f32, f32, f32); 3],
    depth_buf: &mut DepthBuffer,
    width: u32,
    height: u32,
) {
    let (x0, y0, z0) = pts[0];
    let (x1, y1, z1) = pts[1];
    let (x2, y2, z2) = pts[2];

    let min_x = x0.min(x1).min(x2).max(0.0) as i32;
    let max_x = x0.max(x1).max(x2).min(width as f32 - 1.0) as i32;
    let min_y = y0.min(y1).min(y2).max(0.0) as i32;
    let max_y = y0.max(y1).max(y2).min(height as f32 - 1.0) as i32;

    if min_x > max_x || min_y > max_y {
        return;
    }

    let area = edge_function(x0, y0, x1, y1, x2, y2);
    if area.abs() < 1e-6 {
        return;
    }
    let area_recip = 1.0 / area;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let px_f = px as f32 + 0.5;
            let py_f = py as f32 + 0.5;

            let w0 = edge_function(x1, y1, x2, y2, px_f, py_f);
            let w1 = edge_function(x2, y2, x0, y0, px_f, py_f);
            let w2 = edge_function(x0, y0, x1, y1, px_f, py_f);

            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };

            if !inside {
                continue;
            }

            let b0 = w0 * area_recip;
            let b1 = w1 * area_recip;
            let b2 = w2 * area_recip;

            let depth = b0 * z0 + b1 * z1 + b2 * z2;
            depth_buf.test_and_set(px as u32, py as u32, depth);
        }
    }
}

fn edge_function(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// Converts a Transform3D to a column-major 4x4 matrix.
fn transform_to_matrix(t: &Transform3D) -> [[f32; 4]; 4] {
    let b = &t.basis;
    [
        [b.x.x, b.x.y, b.x.z, 0.0],
        [b.y.x, b.y.y, b.y.z, 0.0],
        [b.z.x, b.z.y, b.z.z, 0.0],
        [t.origin.x, t.origin.y, t.origin.z, 1.0],
    ]
}

/// Multiplies a 4x4 column-major matrix by a 3D point (w=1).
fn mat4_mul_point(m: &[[f32; 4]; 4], p: Vector3) -> Vector3 {
    Vector3::new(
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
    )
}

/// Multiplies a 4x4 column-major matrix by a 3D point, returning [x, y, z, w].
fn mat4_mul_point4(m: &[[f32; 4]; 4], p: Vector3) -> [f32; 4] {
    [
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
        m[0][3] * p.x + m[1][3] * p.y + m[2][3] * p.z + m[3][3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;
    use gdserver3d::mesh::Mesh3D;

    #[test]
    fn directional_light_view_matrix_is_valid() {
        let light = Light3D::directional(Light3DId(1));
        let view = directional_light_view_matrix(&light);
        // Should produce a non-degenerate matrix.
        let det = view[0][0] * (view[1][1] * view[2][2] - view[1][2] * view[2][1])
            - view[1][0] * (view[0][1] * view[2][2] - view[0][2] * view[2][1])
            + view[2][0] * (view[0][1] * view[1][2] - view[0][2] * view[1][1]);
        assert!(
            det.abs() > 0.01,
            "view matrix should be non-degenerate, det={det}"
        );
    }

    /// The cubemap generator's per-face view/projection and the sampler's
    /// `direction_to_face_uv` must agree texel-for-texel, or stored depths
    /// land in the wrong texels and omni shadows silently vanish. Probe an
    /// occluder in several face directions and expect its distance back.
    #[test]
    fn omni_cubemap_generator_and_sampler_agree() {
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::light::OmniShadowMode;

        // An occluder in a given direction from the light; sampling that
        // direction must return roughly the occluder's distance.
        let cases = [
            Vector3::new(0.0, 0.0, -1.5),  // −Z face
            Vector3::new(0.0, 0.0, 1.5),   // +Z face
            Vector3::new(1.5, 0.0, 0.0),   // +X face
            Vector3::new(-1.5, 0.0, 0.0),  // −X face
            Vector3::new(0.0, 1.5, 0.0),   // +Y face
            Vector3::new(0.0, -1.5, 0.0),  // −Y face
            Vector3::new(-0.6, 0.2, -1.1), // off-axis (−Z dominant)
        ];
        for rel in cases {
            let light_pos = Vector3::new(1.2, 0.0, 2.2);
            let mut light = Light3D::point(Light3DId(1), light_pos);
            light.range = 20.0;
            light.shadow_enabled = true;
            light.omni_shadow_mode = OmniShadowMode::Cube;

            let mut inst = Instance3D::new(Instance3DId(1));
            inst.mesh = Some(Mesh3D::cube(0.6));
            inst.transform.origin = light_pos + rel;

            let maps = generate_omni_shadow_cubemaps(&[light], &[inst]);
            let cm = maps[0].as_ref().expect("cube-mode light must get a map");
            let stored = cm.sample(rel);
            let dist = rel.length();
            assert!(
                stored < dist + 0.35 && stored > dist - 0.65,
                "sampling toward the occluder at {rel:?} (dist {dist:.2}) must \
                 return its recorded depth, got {stored}"
            );
        }
    }

    /// Godot omni lights default to `DualParaboloid`, and that default MUST cast
    /// a shadow. The audit found `DualParaboloid` returned `None` here, so every
    /// default omni light was silently shadowless in BOTH render paths (the
    /// software renderer and the wgpu path share this generator via
    /// `pack_omni_cubemaps`). A default-mode point light must now get a cubemap
    /// whose sample toward an occluder returns the occluder's recorded depth
    /// (pat-u3l8c.35).
    #[test]
    fn omni_default_dual_paraboloid_light_casts_shadow() {
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::light::OmniShadowMode;

        let light_pos = Vector3::new(0.5, 0.0, 1.0);
        let mut light = Light3D::point(Light3DId(1), light_pos);
        light.range = 20.0;
        light.shadow_enabled = true;
        // Deliberately do NOT set omni_shadow_mode: it must default to
        // DualParaboloid, the mode that previously produced no shadow map.
        assert!(
            matches!(light.omni_shadow_mode, OmniShadowMode::DualParaboloid),
            "Godot omni lights default to DualParaboloid",
        );

        // Occluder on the light's −Z face.
        let rel = Vector3::new(0.0, 0.0, -1.5);
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(0.6));
        inst.transform.origin = light_pos + rel;

        let maps = generate_omni_shadow_cubemaps(&[light], &[inst]);
        let cm = maps[0]
            .as_ref()
            .expect("a default (DualParaboloid) omni light must get a shadow map");
        let stored = cm.sample(rel);
        let dist = rel.length();
        assert!(
            stored < dist + 0.35 && stored > dist - 0.65,
            "default omni light must record the occluder depth toward {rel:?} \
             (dist {dist:.2}), got {stored}",
        );
    }

    /// At Godot's maximum spot half-angle (90°) the full fov reaches π, where
    /// an unclamped `tan(fov/2)` explodes and the focal factor collapses to 0
    /// (every fragment mapping to the center texel). The clamp keeps the
    /// projection non-degenerate.
    #[test]
    fn spot_proj_matrix_stays_finite_at_wide_cone_angles() {
        let mut light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 2.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.spot_angle = std::f32::consts::FRAC_PI_2; // 90° half-angle
        let proj = spot_light_proj_matrix(&light);
        let f = proj[0][0];
        // The fov clamp (π − 0.01) puts f at ~tan(0.005) ≈ 0.005 — assert it's
        // finite and strictly positive (unclamped it would be ~0 / NaN).
        assert!(
            f.is_finite() && f > 1e-4,
            "wide-cone spot projection must stay non-degenerate, f={f}"
        );
    }

    /// Texel-space coordinates just outside the frustum's left/top edge
    /// (u ∈ (−1, 0)) must reject as out-of-map (lit), not truncate onto
    /// column 0 — the GPU sampler rejects them, and the two must agree.
    #[test]
    fn sample_rejects_negative_texel_band_as_lit() {
        // An empty map (no occluders): every in-bounds texel returns 0.0
        // ("no occluder"), so any 0.0 here proves we didn't panic and the
        // boundary behavior is exercised via a map with a full-depth column.
        let mut map = ShadowMap {
            light_id: Light3DId(1),
            size: 4,
            depth: crate::depth_buffer::DepthBuffer::new(4, 4),
            view_matrix: identity(),
            proj_matrix: identity(),
            filter: None,
        };
        // Occluder depth 0 across the whole map: any sampled texel shadows a
        // fragment at depth > bias.
        map.depth.clear(0.0);
        // NDC x = −1.001 → texel u ∈ (−0.01, 0): floor → −1 → out of map → lit.
        let just_outside = Vector3::new(-1.001, 0.0, 0.5);
        assert_eq!(
            map.sample(just_outside),
            0.0,
            "fragment just outside the frustum edge must be lit"
        );
        // NDC x = −0.999 → texel 0 → shadowed (occluder at depth 0).
        let just_inside = Vector3::new(-0.999, 0.0, 0.5);
        assert_eq!(
            map.sample(just_inside),
            1.0,
            "fragment just inside the frustum edge must sample the map"
        );
    }

    fn identity() -> [[f32; 4]; 4] {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    #[test]
    fn shadow_map_generation_empty_scene() {
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        let maps = generate_shadow_maps(&[light], &[]);
        assert_eq!(maps.len(), 1);
        // All depths should be unwritten (MAX).
        let (w, h) = maps[0].depth.dimensions();
        assert_eq!(w, SHADOW_MAP_SIZE);
        assert_eq!(h, SHADOW_MAP_SIZE);
    }

    #[test]
    fn shadow_map_disabled_light_skipped() {
        let light = Light3D::directional(Light3DId(1));
        assert!(!light.shadow_enabled);
        let maps = generate_shadow_maps(&[light], &[]);
        assert_eq!(maps.len(), 0);
    }

    #[test]
    fn shadow_map_point_light_skipped() {
        let mut light = Light3D::point(Light3DId(1), Vector3::new(0.0, 5.0, 0.0));
        light.shadow_enabled = true;
        let maps = generate_shadow_maps(&[light], &[]);
        assert_eq!(
            maps.len(),
            0,
            "point lights use cubemaps, not 2D shadow maps"
        );
    }

    #[test]
    fn spot_shadow_map_generated_for_enabled_spot() {
        let mut light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.shadow_enabled = true;
        let maps = generate_shadow_maps(&[light], &[]);
        assert_eq!(maps.len(), 1, "spot lights with shadows get a depth map");
    }

    #[test]
    fn spot_shadow_sample_behind_occluder() {
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut light = Light3D::spot(
            Light3DId(1),
            Vector3::new(0.0, 8.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        );
        light.shadow_enabled = true;
        light.spot_angle = std::f32::consts::FRAC_PI_3; // wide cone
        light.range = 20.0;

        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(3.0));
        inst.visible = true;
        inst.transform = Transform3D {
            basis: gdcore::math3d::Basis::IDENTITY,
            origin: Vector3::new(0.0, 4.0, 0.0),
        };

        let maps = generate_shadow_maps(&[light], &[inst]);
        assert_eq!(maps.len(), 1);
        // Point under the occluder along the spot axis.
        let shadow = maps[0].sample(Vector3::new(0.0, 0.0, 0.0));
        assert!(
            shadow > 0.5,
            "spot: point below occluder should be shadowed, got {shadow}"
        );
        // Far off-axis / outside frustum → lit (outside map).
        let lit = maps[0].sample(Vector3::new(50.0, 0.0, 50.0));
        assert!(
            lit < 0.5,
            "spot: point outside frustum should be lit, got {lit}"
        );
    }

    #[test]
    fn shadow_map_with_occluder_writes_depth() {
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        // Light pointing straight down.
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        // Place a cube at origin.
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        inst.visible = true;
        inst.transform = Transform3D::IDENTITY;

        let maps = generate_shadow_maps(&[light], &[inst]);
        assert_eq!(maps.len(), 1);

        // Some depth values should be written.
        let (w, h) = maps[0].depth.dimensions();
        let mut written = 0u32;
        for y in 0..h {
            for x in 0..w {
                if maps[0].depth.get(x, y) < f32::MAX {
                    written += 1;
                }
            }
        }
        assert!(
            written > 0,
            "occluder cube should write depth values, got 0"
        );
    }

    #[test]
    fn shadow_sample_behind_occluder_is_shadowed() {
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        // Occluder cube at y=5 (above the test point).
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(4.0));
        inst.visible = true;
        inst.transform = Transform3D {
            basis: gdcore::math3d::Basis::IDENTITY,
            origin: Vector3::new(0.0, 5.0, 0.0),
        };

        let maps = generate_shadow_maps(&[light], &[inst]);
        assert_eq!(maps.len(), 1);

        // A point below the cube should be in shadow.
        let shadow = maps[0].sample(Vector3::new(0.0, 0.0, 0.0));
        assert!(
            shadow > 0.5,
            "point below occluder should be shadowed, got factor={shadow}"
        );
    }

    #[test]
    fn shadow_sample_beside_occluder_is_lit() {
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        // Small occluder cube at origin.
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(1.0));
        inst.visible = true;
        inst.transform = Transform3D::IDENTITY;

        let maps = generate_shadow_maps(&[light], &[inst]);

        // A point far to the side should be lit.
        let shadow = maps[0].sample(Vector3::new(10.0, 0.0, 10.0));
        assert!(
            shadow < 0.5,
            "point beside occluder should be lit, got factor={shadow}"
        );
    }

    /// 🔑 A WORLD-SPACE BIAS MUST BE SCALED BY THE SHADOW FRUSTUM'S DEPTH RANGE.
    ///
    /// The stored comparison is in NORMALIZED post-divide depth. Godot's
    /// directional default of 0.1 WORLD units is ≈0.001 against the 0.1..100
    /// range this module projects with — feeding the raw 0.1 in would be a
    /// hundred times too large and detach every shadow from its caster.
    ///
    /// ⚠️ Asserted as a RATIO between two calls rather than an absolute, so the
    /// test still means the same thing if `SHADOW_NEAR`/`SHADOW_FAR` change.
    #[test]
    fn a_world_space_bias_is_converted_to_normalized_depth() {
        let mut map = ShadowMap {
            light_id: Light3DId(1),
            size: 4,
            depth: crate::depth_buffer::DepthBuffer::new(4, 4),
            view_matrix: identity(),
            proj_matrix: identity(),
            filter: None,
        };
        // An occluder at depth 0 everywhere; the fragment sits just behind it by
        // a margin that straddles the two biases below.
        map.depth.clear(0.0);
        let range = SHADOW_FAR - SHADOW_NEAR;

        // A world bias whose normalized value is comfortably BELOW the margin:
        // the fragment stays in shadow.
        let small = 0.001 * range; // → 0.001 normalized
        let frag = Vector3::new(0.0, 0.0, 0.01);
        assert_eq!(
            map.sample_with_bias(frag, small),
            1.0,
            "a bias smaller than the depth margin leaves the fragment shadowed"
        );

        // A world bias whose normalized value EXCEEDS the margin: now lit.
        let large = 0.1 * range; // → 0.1 normalized, ten times the margin
        assert_eq!(
            map.sample_with_bias(frag, large),
            0.0,
            "a bias larger than the depth margin lifts the fragment out of shadow"
        );
    }

    /// The unbiased entry point is unchanged, so its existing callers are not
    /// affected by the authored-bias work.
    ///
    /// ⚠️ This is the regression guard for the refactor: `sample` now delegates
    /// to a shared helper, and a wrong default there would move every shadow in
    /// the engine while all the new assertions above still passed.
    #[test]
    fn the_unbiased_sampler_still_uses_the_module_default() {
        let mut map = ShadowMap {
            light_id: Light3DId(1),
            size: 4,
            depth: crate::depth_buffer::DepthBuffer::new(4, 4),
            view_matrix: identity(),
            proj_matrix: identity(),
            filter: None,
        };
        map.depth.clear(0.0);
        // Straddle SHADOW_BIAS (0.005) from both sides.
        let inside = Vector3::new(0.0, 0.0, SHADOW_BIAS * 0.5);
        let beyond = Vector3::new(0.0, 0.0, SHADOW_BIAS * 2.0);
        assert_eq!(map.sample(inside), 0.0, "within the default bias: lit");
        assert_eq!(map.sample(beyond), 1.0, "past the default bias: shadowed");
    }

    // -- Light3D.shadow_caster_mask -----------------------------------------

    /// A directional shadow map for a light with `caster_mask`, cast by a single
    /// cube on render layer(s) `layers`, floating above the origin.
    fn shadow_of_cube_on_layers(layers: u32, caster_mask: u32) -> ShadowMap {
        use gdcore::math3d::{Basis, Transform3D};
        use gdserver3d::instance::{Instance3D, Instance3DId};

        let mut caster = Light3D::directional(Light3DId(1));
        caster.shadow_enabled = true;
        caster.direction = Vector3::new(0.0, -1.0, 0.0);
        caster.shadow_caster_mask = caster_mask;

        let mut occluder = Instance3D::new(Instance3DId(1));
        occluder.mesh = Some(Mesh3D::cube(4.0));
        occluder.visible = true;
        occluder.geometry.layers = layers;
        occluder.transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 5.0, 0.0),
        };

        generate_shadow_maps(&[caster], &[occluder])
            .into_iter()
            .next()
            .expect("a shadow-enabled directional light produces one map")
    }

    /// 🔑 `shadow_caster_mask` DECIDES WHICH MESHES CAST INTO THIS LIGHT'S
    /// SHADOW MAP — and it is a BITMASK intersected with the instance's layers.
    ///
    /// Godot tests `instance->layer_mask & light_get_shadow_caster_mask(light)`
    /// (`renderer_scene_cull.cpp:2415`).
    ///
    /// ⚠️ The high-layer case is not padding. Reading either side as an INDEX
    /// rather than a mask agrees for every single-layer value including the
    /// default 1, so a fixture using only layer 1 cannot tell the two apart —
    /// the same trap `light_cull_mask` hit on this bead.
    #[test]
    fn shadow_caster_mask_selects_which_layers_cast() {
        let under = Vector3::ZERO;

        // Default mask admits the default layer: the positive control, without
        // which every "not shadowed" assertion below is satisfied by a fixture
        // that never cast a shadow at all.
        let included = shadow_of_cube_on_layers(1, u32::MAX);
        assert!(
            included.sample(under) > 0.5,
            "the cube must actually shadow the origin under the default mask, \
             or the rest of this test proves nothing; got {}",
            included.sample(under)
        );

        // A mask that excludes the cube's only layer drops it from the map.
        let excluded = shadow_of_cube_on_layers(1, 0b10);
        assert!(
            excluded.sample(under) < 0.5,
            "a caster mask without the cube's layer must leave the origin \
             unshadowed; got {}",
            excluded.sample(under)
        );

        // Layer 5 against a mask naming layer 5 — both far above any value an
        // index-vs-mask confusion would agree on.
        let high = 1u32 << 4;
        assert!(
            shadow_of_cube_on_layers(high, high).sample(under) > 0.5,
            "a layer-5 cube must cast into a mask that names layer 5"
        );
        assert!(
            shadow_of_cube_on_layers(high, 1).sample(under) < 0.5,
            "and must NOT cast into a mask naming only layer 1"
        );
    }

    /// 🔑 AN INSTANCE ON NO LAYER CASTS NOTHING, WHATEVER THE MASK SAYS.
    ///
    /// Godot's behaviour, and it falls out of the intersection rather than
    /// needing a special case — `0 & anything == 0`. Worth pinning because the
    /// obvious "empty means unrestricted" reading is the opposite.
    #[test]
    fn an_instance_on_no_layer_casts_no_shadow() {
        assert!(
            shadow_of_cube_on_layers(0, u32::MAX).sample(Vector3::ZERO) < 0.5,
            "layers == 0 must cast nothing even under an all-ones caster mask"
        );
    }

    /// 🔑 THE OMNI CUBEMAP PATH APPLIES THE SAME MASK.
    ///
    /// ⚠️ The two rasterizers filter in separate loops. A caster mask honoured
    /// in one and ignored in the other means a mesh excluded from a spot
    /// light's shadow and included in an omni's — which is why both go through
    /// the shared `casts_into_shadow` predicate.
    #[test]
    fn the_omni_cubemap_path_honours_the_same_caster_mask() {
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::light::OmniShadowMode;

        let light_pos = Vector3::new(1.2, 0.0, 2.2);
        let rel = Vector3::new(0.0, 0.0, -1.5);

        let cube_with_mask = |caster_mask: u32| {
            let mut light = Light3D::point(Light3DId(1), light_pos);
            light.range = 20.0;
            light.shadow_enabled = true;
            light.omni_shadow_mode = OmniShadowMode::Cube;
            light.shadow_caster_mask = caster_mask;

            let mut inst = Instance3D::new(Instance3DId(1));
            inst.mesh = Some(Mesh3D::cube(0.6));
            inst.transform.origin = light_pos + rel;

            let maps = generate_omni_shadow_cubemaps(&[light], &[inst]);
            maps[0]
                .as_ref()
                .expect("cube-mode light must get a map")
                .sample(rel)
        };

        // Positive control first: the occluder is recorded at roughly its own
        // distance when the mask admits it.
        let dist = rel.length();
        let recorded = cube_with_mask(u32::MAX);
        assert!(
            recorded < dist + 0.35,
            "the occluder must be recorded under the default mask, got \
             {recorded} for a distance of {dist}"
        );

        // Excluded, the texel keeps the `f32::MAX` that `ShadowCubemap::new`
        // clears every face to. Asserted EXACTLY rather than as "further than
        // the occluder": the strong form is "nothing was ever written here",
        // and a merely-larger depth would also be satisfied by the cube being
        // rasterized to the wrong face.
        let masked_out = cube_with_mask(0b10);
        assert_eq!(
            masked_out,
            f32::MAX,
            "a caster mask without the instance's layer must leave the cubemap \
             texel untouched; recorded={recorded} when the mask admitted it"
        );
    }

    // -- Light3D.shadow_blur / shadow_filter_quality (PCF) -------------------

    /// 🔑 GODOT'S DEFAULT FILTER QUALITY CHANGES NOTHING AT ALL.
    ///
    /// `ShadowFilterQuality::Hard` reports **1** tap and is the default, so the
    /// filtered path must reduce to exactly the unfiltered one — not
    /// approximately, bit-for-bit, because it is the same call. Same for any
    /// tap count with a zero blur radius.
    ///
    /// ⚠️ This is the assertion that makes the whole feature additive. Without
    /// it, wiring PCF would soften every shadow in every existing scene.
    #[test]
    fn hard_quality_and_zero_blur_are_exactly_the_unfiltered_sample() {
        use gdserver3d::light::ShadowFilterQuality;

        let map = shadow_of_cube_on_layers(1, u32::MAX);
        for probe in [
            Vector3::ZERO,
            Vector3::new(1.5, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(6.0, 0.0, 0.0),
        ] {
            let plain = map.sample_with_bias(probe, 0.1);
            assert_eq!(
                map.sample_with_bias_filtered(probe, 0.1, ShadowFilterQuality::Hard.taps(), 1.0),
                plain,
                "Hard is 1 tap and must be the unfiltered result at {probe:?}"
            );
            assert_eq!(
                map.sample_with_bias_filtered(probe, 0.1, 16, 0.0),
                plain,
                "a zero blur radius must also collapse to one tap at {probe:?}"
            );
        }
    }

    /// 🔑 A SOFT QUALITY PRODUCES A PENUMBRA AT THE SHADOW EDGE.
    ///
    /// The occluder is a 4-unit cube centred over the origin, so its hard shadow
    /// edge sits at x = ±2. A hard sample there is a step — exactly 0 or 1;
    /// filtering across the edge must land strictly between the two.
    ///
    /// ⚠️ Both ends are asserted as well. A filter that returned "somewhere
    /// between 0 and 1" everywhere would pass the penumbra check alone while
    /// having destroyed the shadow — deep inside must stay fully occluded and
    /// well outside must stay fully lit.
    #[test]
    fn a_soft_filter_quality_produces_a_penumbra_only_at_the_edge() {
        use gdserver3d::light::ShadowFilterQuality;

        let map = shadow_of_cube_on_layers(1, u32::MAX);
        let taps = ShadowFilterQuality::High.taps();
        let blur = 4.0;
        let filtered =
            |x: f32| map.sample_with_bias_filtered(Vector3::new(x, 0.0, 0.0), 0.1, taps, blur);

        let inside = filtered(0.0);
        let edge = filtered(2.0);
        let outside = filtered(6.0);

        assert!(
            inside > 0.99,
            "deep inside the shadow must stay fully occluded, got {inside}"
        );
        assert!(
            outside < 0.01,
            "well outside must stay fully lit, got {outside}"
        );
        assert!(
            edge > 0.01 && edge < 0.99,
            "the edge must be a partial average — a penumbra — got {edge}; a \
             hard sample there is exactly 0.0 or 1.0"
        );
    }

    /// 🔑 THE TAP OFFSETS FOLLOW THE LIGHT'S OWN AXES, NOT WORLD X/Y.
    ///
    /// ⚠️ The obvious implementation offsets the world position along world X
    /// and Y. For this fixture — a light pointing straight down −Y — the world-Y
    /// component runs ALONG the light direction, so half the kernel collapses
    /// onto the same texel and the penumbra narrows or vanishes. Reading the
    /// light's right/up out of its view matrix is what keeps the disc
    /// perpendicular to the light for any orientation.
    ///
    /// Asserted as a property rather than by rotating the fixture: every tap
    /// must land in the shadow map's own plane, so sweeping the edge produces a
    /// MONOTONIC ramp from occluded to lit. A collapsed kernel gives a step.
    #[test]
    fn the_penumbra_ramps_monotonically_across_the_edge() {
        use gdserver3d::light::ShadowFilterQuality;

        let map = shadow_of_cube_on_layers(1, u32::MAX);
        let taps = ShadowFilterQuality::High.taps();
        let samples: Vec<f32> = (0..=12)
            .map(|i| {
                let x = 1.4 + i as f32 * 0.1;
                map.sample_with_bias_filtered(Vector3::new(x, 0.0, 0.0), 0.1, taps, 4.0)
            })
            .collect();

        for pair in samples.windows(2) {
            assert!(
                pair[1] <= pair[0] + 1e-6,
                "occlusion must not increase as the probe leaves the shadow: \
                 {samples:?}"
            );
        }
        assert!(
            samples.first().copied().unwrap_or(0.0) > samples.last().copied().unwrap_or(1.0),
            "and the sweep must actually fall from occluded to lit: {samples:?}"
        );
        // At least one intermediate value, or this is a step rather than a ramp.
        assert!(
            samples.iter().any(|v| *v > 0.05 && *v < 0.95),
            "the sweep must pass through a partial value: {samples:?}"
        );
    }
}

/// Directional light cascade split (pat-p0s82.8).
#[derive(Debug, Clone)]
pub struct DirectionalCascade {
    /// Near plane distance of this cascade.
    pub near: f32,
    /// Far plane distance of this cascade.
    pub far: f32,
    /// Shadow map for this cascade.
    pub map: ShadowMap,
}

/// Compute cascade split distances (PSSM-style, log/linear blended at 0.5).
pub fn cascade_split_distances(near: f32, far: f32, count: usize) -> Vec<f32> {
    cascade_split_distances_lambda(near, far, count, 0.5)
}

/// Compute cascade split distances with an explicit log/linear blend weight.
///
/// Mirrors Godot's practical split scheme: `lambda = 0.0` gives uniform
/// (linear) splits, `lambda = 1.0` gives fully logarithmic splits, and values
/// in between blend the two (Godot sits near 0.5). `count` maps to Godot's
/// directional shadow mode (2 or 4 splits; 1 and 3 are also accepted).
///
/// Returns `count + 1` strictly-increasing boundaries `[near, .., far]`.
pub fn cascade_split_distances_lambda(near: f32, far: f32, count: usize, lambda: f32) -> Vec<f32> {
    let count = count.max(1);
    let lambda = lambda.clamp(0.0, 1.0);
    let mut splits = Vec::with_capacity(count + 1);
    splits.push(near);
    for i in 1..count {
        let id = i as f32 / count as f32;
        let log_s = near * (far / near).powf(id);
        let uni_s = near + (far - near) * id;
        splits.push(lambda * log_s + (1.0 - lambda) * uni_s);
    }
    splits.push(far);
    splits
}

/// Renders a directional light's shadow as `count` cascades (PSSM).
///
/// This is the directional shadow **cascade loop**: it slices the light-view
/// depth range `[SHADOW_NEAR, SHADOW_FAR]` into `count` successive cascades via
/// [`cascade_split_distances`] and renders one [`ShadowMap`] per cascade. Each
/// cascade `i` records the *selection* slice `[near, far] = [split[i],
/// split[i+1]]` (consumed by [`select_cascade`]), while its map is rendered
/// over the depth range `[SHADOW_NEAR, split[i+1]]` — so occluders *in front of*
/// the slice still cast into it — and over a tightened orthographic extent
/// proportional to the cascade's far distance, so near cascades pack more
/// texels onto near geometry (the point of PSSM).
///
/// `count` is clamped to `1..=4` (Godot supports 1/2/4 directional splits).
pub fn generate_directional_cascades(
    light: &Light3D,
    instances: &[Instance3D],
    count: usize,
) -> Vec<DirectionalCascade> {
    let count = count.clamp(1, 4);
    let splits = cascade_split_distances(SHADOW_NEAR, SHADOW_FAR, count);
    let view_matrix = directional_light_view_matrix(light);
    let size = SHADOW_MAP_SIZE;

    (0..count)
        .map(|i| {
            let near = splits[i];
            let far = splits[i + 1];
            // Tighten the ortho extent for nearer cascades (higher effective
            // resolution up close); floor it so it never collapses.
            let extent = (SHADOW_EXTENT * far / SHADOW_FAR).max(2.0);
            // Render depth from the light (SHADOW_NEAR) to this cascade's far so
            // occluders ahead of the slice are captured.
            let proj_matrix =
                orthographic_projection_matrix(-extent, extent, -extent, extent, SHADOW_NEAR, far);
            let mut depth = DepthBuffer::new(size, size);
            rasterize_instances_into_shadow(
                instances,
                light.shadow_caster_mask,
                &view_matrix,
                &proj_matrix,
                &mut depth,
                size,
            );
            DirectionalCascade {
                near,
                far,
                map: ShadowMap {
                    light_id: light.id,
                    depth,
                    view_matrix,
                    proj_matrix,
                    size,
                    filter: None,
                },
            }
        })
        .collect()
}

/// Light-view depth of a world point along a directional light's forward axis
/// (its distance from the light), used to pick a cascade for the fragment.
fn light_view_depth(view_matrix: &[[f32; 4]; 4], world_pos: Vector3) -> f32 {
    // The light looks along its local -Z; a point in front has negative view z.
    -mat4_mul_point(view_matrix, world_pos).z
}

/// Selects the cascade whose depth slice contains `depth`: the first cascade
/// whose `far` is at or beyond `depth`. Clamps to the last cascade for depths
/// past the final split (and to the first for depths before the first).
pub fn select_cascade(cascades: &[DirectionalCascade], depth: f32) -> usize {
    for (i, c) in cascades.iter().enumerate() {
        if depth <= c.far {
            return i;
        }
    }
    cascades.len().saturating_sub(1)
}

/// Index of the cascade a world-space fragment falls into (its light-view depth
/// routed through [`select_cascade`]).
pub fn cascade_index_for(cascades: &[DirectionalCascade], world_pos: Vector3) -> usize {
    if cascades.is_empty() {
        return 0;
    }
    let depth = light_view_depth(&cascades[0].map.view_matrix, world_pos);
    select_cascade(cascades, depth)
}

/// Samples cascaded directional shadows for a world-space fragment: picks the
/// cascade covering the fragment's light-view depth (per-cascade split
/// selection), then samples that cascade's shadow map. Returns a shadow factor
/// (`0.0` lit .. `1.0` shadowed); `0.0` for an empty cascade set.
pub fn sample_cascaded_shadow(cascades: &[DirectionalCascade], world_pos: Vector3) -> f32 {
    if cascades.is_empty() {
        return 0.0;
    }
    cascades[cascade_index_for(cascades, world_pos)]
        .map
        .sample(world_pos)
}

#[cfg(test)]
mod cascade_tests {
    use super::*;

    #[test]
    fn cascade_splits_monotonic() {
        let s = cascade_split_distances(0.1, 100.0, 4);
        assert_eq!(s.len(), 5);
        for w in s.windows(2) {
            assert!(w[1] > w[0]);
        }
    }

    #[test]
    fn cascade_split_lambda_extremes() {
        // lambda = 0 → uniform (linear) interior splits: 1, 25.75, 50.5, 75.25.
        let uni = cascade_split_distances_lambda(1.0, 100.0, 4, 0.0);
        assert!((uni[1] - 25.75).abs() < 0.01, "uniform split1 {}", uni[1]);
        assert!((uni[2] - 50.5).abs() < 0.01, "uniform split2 {}", uni[2]);
        // lambda = 1 → logarithmic interior splits: 1, 3.162, 10, 31.62.
        let log = cascade_split_distances_lambda(1.0, 100.0, 4, 1.0);
        assert!((log[1] - 3.1623).abs() < 0.01, "log split1 {}", log[1]);
        assert!((log[2] - 10.0).abs() < 0.02, "log split2 {}", log[2]);
        // Logarithmic packs more resolution near the camera than uniform.
        assert!(log[1] < uni[1], "log should be tighter near the camera");
    }

    #[test]
    fn cascade_count_maps_to_mode() {
        let light = Light3D::directional(Light3DId(1));
        for count in [2usize, 3, 4] {
            let cascades = generate_directional_cascades(&light, &[], count);
            assert_eq!(cascades.len(), count, "count {count}");
            // Slices span [SHADOW_NEAR, SHADOW_FAR], contiguous and increasing.
            assert!((cascades[0].near - SHADOW_NEAR).abs() < 1e-3);
            assert!((cascades[count - 1].far - SHADOW_FAR).abs() < 1e-3);
            for w in cascades.windows(2) {
                assert!(w[1].near > w[0].near, "cascade nears must increase");
                assert!(
                    (w[0].far - w[1].near).abs() < 1e-4,
                    "cascade slices must be contiguous"
                );
            }
        }
        // Godot caps directional splits at 4.
        assert_eq!(generate_directional_cascades(&light, &[], 9).len(), 4);
    }

    #[test]
    fn select_cascade_routes_by_depth() {
        let light = Light3D::directional(Light3DId(1));
        let cascades = generate_directional_cascades(&light, &[], 4);
        // Before the first near → 0; past the last far → the last cascade.
        assert_eq!(select_cascade(&cascades, -5.0), 0);
        assert_eq!(select_cascade(&cascades, 1000.0), 3);
        // A depth inside a cascade's own slice selects that cascade.
        let mid0 = 0.5 * (cascades[0].near + cascades[0].far);
        assert_eq!(select_cascade(&cascades, mid0), 0);
        let mid_last = 0.5 * (cascades[3].near + cascades[3].far);
        assert_eq!(select_cascade(&cascades, mid_last), 3);
    }

    /// The core parity goal: with a single shadow map both a near caster and a
    /// far caster cannot be resolved well; cascades give each its own slice.
    /// Here a high occluder (small light-view depth) and a low occluder (large
    /// depth) each shadow a receiver beneath them, routed through *different*
    /// cascades.
    #[test]
    fn near_and_far_casters_both_shadowed() {
        use gdserver3d::instance::{Instance3D, Instance3DId};
        use gdserver3d::mesh::Mesh3D;

        // Light straight down: it sits at y≈+50 looking along -Y, so a world
        // point's light-view depth ≈ 50 − y. A high occluder is "near" (small
        // depth); a low occluder is "far" (large depth).
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        let cube = |id: u64, y: f32| {
            let mut inst = Instance3D::new(Instance3DId(id));
            inst.mesh = Some(Mesh3D::cube(2.0));
            inst.visible = true;
            inst.transform = Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, y, 0.0),
            };
            inst
        };

        // Near occluder high up (depth ≈ 3), far occluder low down (depth ≈ 88).
        let occluders = [cube(1, 47.0), cube(2, -38.0)];
        let cascades = generate_directional_cascades(&light, &occluders, 4);
        assert_eq!(cascades.len(), 4);

        // Receivers just beneath each occluder along the same light ray.
        let near_recv = Vector3::new(0.0, 44.0, 0.0); // depth ≈ 6
        let far_recv = Vector3::new(0.0, -41.0, 0.0); // depth ≈ 91

        // They must route to *different* cascades — proving cascades are
        // actually exercised, not collapsed onto one map.
        let near_idx = cascade_index_for(&cascades, near_recv);
        let far_idx = cascade_index_for(&cascades, far_recv);
        assert!(
            near_idx < far_idx,
            "near receiver (cascade {near_idx}) should use an earlier cascade \
             than the far receiver (cascade {far_idx})"
        );

        // Both receivers are shadowed by their respective casters.
        let near_shadow = sample_cascaded_shadow(&cascades, near_recv);
        let far_shadow = sample_cascaded_shadow(&cascades, far_recv);
        assert!(
            near_shadow > 0.5,
            "near caster must shadow its receiver, got {near_shadow}"
        );
        assert!(
            far_shadow > 0.5,
            "far caster must shadow its receiver, got {far_shadow}"
        );
    }
}

/// pat-772py: the camera-fitted directional frame.
#[cfg(test)]
mod fitted_frame_tests {
    use super::*;
    use gdserver3d::light::{DirectionalShadowParams, ShadowCamera};

    /// The 3D platformer's sun and camera settings (`stage.tscn`,
    /// `player.tscn`, `project.godot`), with the camera placed at `at`.
    fn platformer_sun(at: Vector3) -> Light3D {
        let mut l = Light3D::directional(Light3DId(1));
        l.shadow_enabled = true;
        l.direction = Vector3::new(-0.198, -0.643, 0.740).normalized();
        l.shadow_bias = 0.02;
        l.directional_shadow_max_distance = 55.0;
        l.directional_shadow = DirectionalShadowParams {
            atlas_size: 8192,
            soft_quality: 3,
            blur: 1.5,
            pancake_size: 20.0,
            mode: 0,
        };
        l.shadow_camera = Some(ShadowCamera {
            transform: Transform3D {
                basis: Basis::IDENTITY,
                origin: at,
            },
            fov_y: 74f32.to_radians(),
            aspect: 1152.0 / 648.0,
            near: 0.1,
            far: 4000.0,
        });
        l
    }

    fn project(frame: &DirectionalShadowFrame, p: Vector3) -> (f32, f32, f32) {
        let v = mat4_mul_point(&frame.view, p);
        let c = mat4_mul_point4(&frame.proj, v);
        (c[0] / c[3], c[1] / c[3], c[2] / c[3])
    }

    #[test]
    fn the_whole_frustum_to_max_distance_lands_inside_the_map() {
        let at = Vector3::new(140.0, 3.0, -75.0);
        let light = platformer_sun(at);
        let frame = directional_shadow_frame(&light);
        let cam = light.shadow_camera.unwrap();
        let t = (cam.fov_y * 0.5).tan();
        for d in [0.1f32, 20.0, 55.0] {
            for (sx, sy) in [
                (-1.0, -1.0),
                (1.0, -1.0),
                (-1.0, 1.0),
                (1.0, 1.0),
                (0.0, 0.0),
            ] {
                let p = cam
                    .transform
                    .xform(Vector3::new(sx * d * t * cam.aspect, sy * d * t, -d));
                let (x, y, z) = project(&frame, p);
                assert!(
                    x.abs() <= 1.0 && y.abs() <= 1.0 && z.abs() <= 1.0,
                    "frustum point at distance {d} ({sx}, {sy}) falls outside the map: \
                     ndc ({x}, {y}, {z}). The legacy box only reached |x|, |z| <= 20."
                );
            }
        }
        assert_eq!(frame.size, DIRECTIONAL_FIT_MAX_SIZE);
    }

    #[test]
    fn the_filter_and_bias_are_godots() {
        let light = platformer_sun(Vector3::ZERO);
        let frame = directional_shadow_frame(&light);
        let f = frame.filter.expect("fitted");
        // soft_shadow_filter_quality 3 = SOFT_MEDIUM: 8 taps at quality radius
        // 2, so Godot's `soft_shadow_scale` = blur 1.5 * 2 = 3.
        assert_eq!(f.taps, 8);
        // Recover the sphere radius and depth range from the matrices:
        // proj[0][0] = 1 / half, half = radius * (1 + 1/16) (half a radius/8
        // snap step), proj[2][2] = -2 / z_range.
        let half = 1.0 / frame.proj[0][0];
        let radius = half / (1.0 + 1.0 / 16.0);
        let z_range = -2.0 / frame.proj[2][2];
        // Godot: bias = shadow_bias / 100 * (2 * radius + pancake) * scale, in
        // world units (`light_storage.cpp:721`, `renderer_scene_cull.cpp:2366`).
        let godot_bias_world = 0.02 / 100.0 * (2.0 * radius + 20.0) * 3.0;
        let ours_world = f.depth_bias * z_range / 2.0;
        assert!(
            (ours_world - godot_bias_world).abs() < 1e-4,
            "bias {ours_world} world units, Godot's formula gives {godot_bias_world}"
        );
        // Godot's PCF radius: 3 texels of ITS 8192 map, i.e. 3 * 2r / 8192
        // world units, converted to our texels.
        let godot_radius_world = 3.0 * 2.0 * radius / 8192.0;
        assert!(
            (f.radius_texels * f.texel_world - godot_radius_world).abs() < 1e-4,
            "PCF radius {} world, Godot {godot_radius_world}",
            f.radius_texels * f.texel_world
        );
    }

    #[test]
    fn a_small_camera_move_keeps_the_frame_so_static_casters_are_reused() {
        let a = directional_shadow_frame(&platformer_sun(Vector3::new(10.0, 0.0, 0.0)));
        let b = directional_shadow_frame(&platformer_sun(Vector3::new(10.3, 0.0, 0.1)));
        assert_eq!(
            a, b,
            "the centre snaps to radius/8, so a 0.3-unit step keeps the frame"
        );
    }

    #[test]
    fn without_a_camera_the_legacy_box_is_unchanged() {
        let mut light = platformer_sun(Vector3::ZERO);
        light.shadow_camera = None;
        let f = directional_shadow_frame(&light);
        assert_eq!(f.size, SHADOW_MAP_SIZE);
        assert_eq!(f.filter, None);
        assert_eq!(f.view, directional_light_view_matrix(&light));
        assert_eq!(f.proj, directional_light_proj_matrix());
    }
}
