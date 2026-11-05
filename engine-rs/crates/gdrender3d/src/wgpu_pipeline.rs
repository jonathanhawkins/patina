//! GPU-accelerated 3D render pipeline using wgpu with WGSL shaders.
//!
//! Provides [`GpuRenderer3D`] — a wgpu-based renderer that implements the
//! [`RenderingServer3D`] trait with real vertex and fragment shader stages
//! running on the GPU. Feature-gated behind `"gpu"`.
//!
//! The pipeline supports three shading modes matching the software renderer:
//! - **Unlit**: albedo + emission, no lighting
//! - **Lambert**: diffuse lighting with ambient term
//! - **Phong**: diffuse + Blinn-Phong specular highlights

use std::collections::HashMap;

use gdcore::math::{srgb_to_linear_channel, Color, Vector3};
use gdcore::math3d::Transform3D;

use gdserver3d::decal::Decal3D;
use gdserver3d::environment::{BackgroundMode, Environment3D};
use gdserver3d::fog_volume::FogVolume;
use gdserver3d::gi::{VoxelGI, VoxelGIId};
use gdserver3d::instance::{GeometryInstanceSettings, Instance3D, Instance3DId};
use gdserver3d::occluder::OccluderInstance3D;
use gdserver3d::light::{Light3D, Light3DId, LightType};
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::projection::perspective_projection_matrix;
use gdserver3d::reflection_probe::{sample_probes_at, ReflectionProbe, ReflectionProbeId};
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::ShaderMaterial3D;
use gdserver3d::sky::SkyMaterial;
use gdserver3d::viewport::Viewport3D;

use crate::renderer::invert_projection;

// ── WGSL shader source ──────────────────────────────────────────────

/// Standard 3D vertex + fragment shader in WGSL.
///
/// Vertex stage: applies model-view-projection transforms, passes world
/// position and normal to fragment stage.
///
/// Fragment stage: selects shading via `shading_mode` uniform —
/// 0 = unlit, 1 = Lambert, 2 = Phong.
pub const SHADER_3D_WGSL: &str = r#"
// ── Bind group 0: per-frame camera uniforms ─────────────────────────

struct CameraUniforms {
    view_matrix: mat4x4<f32>,
    projection_matrix: mat4x4<f32>,
    camera_position: vec3<f32>,
    _pad0: f32,
    ambient: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

// ── Bind group 1: per-object model + material uniforms ──────────────

struct ModelUniforms {
    model_matrix: mat4x4<f32>,
    albedo: vec4<f32>,
    emission: vec4<f32>,
    roughness: f32,
    metallic: f32,
    shading_mode: u32,   // 0=unlit, 1=lambert, 2=phong
    num_lights: u32,
    // `VisualInstance3D.layers` — the render-layer BITMASK of the instance being
    // drawn. Intersected with each light's `cull_mask` below.
    //
    // ⚠️ This is why `light_cull_mask` needed a WGSL change at all, unlike
    // `shadow_opacity` and the distance fade: it is the only property in the set
    // that is PER DRAW rather than per light, so it cannot be folded into the
    // light uniform.
    layers: u32,
    // Explicit padding back to a multiple of 16: the struct's alignment comes
    // from its mat4x4, and Rust's [[f32; 4]; 4] has an align of only 4, so the
    // two languages agree only if the tail is spelled out. See the note on
    // LightData.
    // `BaseMaterial3D.normal_texture` presence + `normal_scale` (pat-mgixk.52).
    //
    // ⚠️ A FLAG rather than an identity texel. A flat normal map is nominally
    // (0.5, 0.5, 1.0) -> no tilt, but 0.5 is NOT representable in 8 bits:
    // 128/255 = 0.50196 decodes to 0.00392, so a "flat" dummy would tilt every
    // normal slightly and no untextured scene would stay bit-identical. Only
    // k/255 values are exact.
    //
    // These take the FIRST TWO of the three slots reserved when the struct was
    // padded out, so the struct size (128) and every existing field offset —
    // including `layers` at 112, which a test pins — are unchanged.
    has_normal_map: u32,
    normal_scale: f32,
    _model_pad2: u32,
    // ── ReflectionProbe covering this draw (pat-p3p1m) ──────────────────
    // Appended AFTER the original 128 bytes so no existing offset moves. The
    // probe is chosen per INSTANCE at its world origin, exactly as the software
    // renderer does (`renderer.rs`: `sample_probes_at` / `probe_reflection_at`
    // at `entry.transform.origin`).
    // The summed UNCAPTURED-probe ambient (`sample_probes_at`) at the
    // instance origin, ADDED to the camera ambient; zero when a captured
    // probe covers the origin, as in the software `entry_ambient`.
    probe_ambient: vec4<f32>,
    // pat-h7sz7: the per-instance box/origin fields are gone — probes are
    // blended per FRAGMENT from `probe_array`. Kept as padding so the struct
    // size and every later offset are unchanged.
    _probe_unused0: vec4<f32>,
    _probe_unused1: vec4<f32>,
    _probe_unused2: vec4<f32>,
    // Bit i = `probe_array.probes[i]` may cover this draw.
    probe_mask: u32,
    _probe_unused3: u32,
    // 1 = run `env_indirect` (a captured probe or the sky radiance exists);
    // 0 = no image-based light, exactly the old probe-less path.
    probe_env: u32,
    _probe_pad1: u32,
};
@group(1) @binding(0)
var<uniform> model: ModelUniforms;

// `BaseMaterial3D.albedo_texture`, sampled per fragment (pat-mgixk.52).
//
// ⚠️ Deliberately in group 1 rather than a new group: group 3 is already taken
// by the ShaderMaterial custom samplers, and the model bind group is the only
// one of 0-2 that is rebuilt PER DRAW — which is exactly the granularity a
// per-material texture needs. Groups 0 and 2 are built once per frame.
//
// The base, cull-variant and custom pipelines all share `model_layout`, so
// adding entries here reaches every draw at once. A shader that does not
// reference a binding its layout declares is valid in wgpu, so this cannot
// break a pipeline that ignores it.
//
// Uploaded as `Rgba8UnormSrgb`, which makes the sampler perform the same
// sRGB->linear decode the software path applies per texel
// (`shader.rs:218`: `map.sample_bilinear(u, v).srgb_to_linear()`) — same
// piecewise curve, and both leave alpha untouched. `FilterMode::Linear`
// matches `sample_bilinear`; hardware decodes before filtering where software
// filters before decoding, which differs only between texel centres and
// agrees exactly at them.
// ⚠️ NOT named `t_albedo`/`s_albedo`: the custom-shader codegen emits its
// group-3 samplers as `t_<uniform name>` / `s_<uniform name>`, so a GDSL
// shader declaring `uniform sampler2D albedo;` would emit `t_albedo` and
// duplicate the declaration below. naga would then reject the spliced
// module and `custom_spatial_wgsl` would fall back to flat albedo —
// silently. A `base_`-prefixed name cannot be generated.
@group(1) @binding(1) var base_albedo_map: texture_2d<f32>;
@group(1) @binding(2) var base_material_sampler: sampler;
// `BaseMaterial3D.normal_texture` (pat-mgixk.52), sharing the sampler above —
// same filter and wrap, so there is no reason to burn a second sampler slot.
//
// ⚠️ Uploaded `Rgba8Unorm`, NOT `Rgba8UnormSrgb` like the albedo map: the
// software path does NOT sRGB-decode the normal texel (`shader.rs:252` samples
// it raw and hands it straight to `apply_normal_map`, unlike the albedo and
// emission maps two lines up). Decoding it would bend every perturbed normal
// toward the surface.
@group(1) @binding(3) var base_normal_map: texture_2d<f32>;
// `BaseMaterial3D` roughness / metallic / emission maps (pat-mgixk.52), all
// sharing the sampler at binding 2.
//
// ⚠️ Godot reads roughness from the **green** channel and metallic from the
// **blue** channel by default, not from red or from luminance — the software
// path spells this out (`shader.rs:241`, `:247`). Both are raw `Rgba8Unorm`
// because those channels are plain scalars, not colour; emission IS a colour
// map and so is `Rgba8UnormSrgb` like albedo (`shader.rs:229`).
@group(1) @binding(4) var base_roughness_map: texture_2d<f32>;
@group(1) @binding(5) var base_metallic_map: texture_2d<f32>;
@group(1) @binding(6) var base_emission_map: texture_2d<f32>;

// ── Bind group 2: light array ───────────────────────────────────────

struct LightData {
    // kind: 0=directional, 1=point, 2=spot
    kind: u32,
    shadow_enabled: u32,
    range: f32,
    attenuation: f32,
    direction: vec3<f32>,
    spot_angle: f32,
    color: vec3<f32>,
    spot_angle_attenuation: f32,
    position: vec3<f32>,
    // -1 = no shadow map bound; 0 = sample `shadow_depth` (primary directional).
    shadow_map_index: i32,
    // Godot `Light3D.shadow_opacity`: how dark this light's shadow is
    // (pat-s3rrv.90). Mirrors the software `LightUniform` field of the same name.
    shadow_opacity: f32,
    // `light_cull_mask`: which render layers this light touches. Takes the
    // FIRST of the three slots reserved when shadow_opacity was added, so the
    // struct size and every existing offset are unchanged.
    cull_mask: u32,
    // ⚠️ EXPLICIT PADDING, not decoration. WGSL gives `mat4x4<f32>` an align of
    // 16, so the matrix below has to start on a 16-byte boundary; Rust's
    // `[[f32; 4]; 4]` has an align of only 4 and would place it at offset 68.
    // Three bare `f32`s (align 4 each) put both languages at offset 80 and keep
    // the struct a multiple of 16, which is also what the uniform ARRAY stride
    // requires. A `vec3<f32>` would NOT work here: its align is 16, so WGSL
    // would push it to offset 80 and disagree with Rust by twelve bytes.
    //
    // These three slots are where `distance_fade_begin` / `_length` /
    // `_shadow` go next, so the layout change is paid once for the whole bead.
    // `Light3D.shadow_bias`, in WORLD UNITS. Takes the second of the three slots
    // reserved when shadow_opacity landed, so the struct size and every existing
    // offset are unchanged.
    shadow_bias: f32,
    // `Light3D.shadow_normal_bias`, in SHADOW-MAP TEXELS. Takes the LAST slot
    // reserved when shadow_opacity landed — the struct is now full, and the next
    // property needs a new 16-byte row.
    shadow_normal_bias: f32,
    // Light-space clip matrix: proj * view (column-major), matching software
    // `ShadowMap::{view_matrix, proj_matrix}` composition.
    shadow_view_proj: mat4x4<f32>,
    // `Light3D.light_specular`, as authored. ⚠️ A NEW 16-byte row: the three
    // slots reserved when shadow_opacity landed were all spent by
    // cull_mask/shadow_bias/shadow_normal_bias, so this is the first property
    // to grow the struct since the shadow matrix (144 -> 160 bytes).
    //
    // ⚠️ Placed AFTER the mat4x4 on purpose — inserting it before would move the
    // matrix off its 16-byte boundary and silently reinterpret every existing
    // field on the GPU side. Three explicit f32 pads (align 4 each) keep Rust
    // and WGSL agreeing at 160 and keep the uniform ARRAY STRIDE a multiple of
    // 16. A `vec3<f32>` would NOT work: its align is 16, so WGSL would round
    // the struct to 176 while Rust stayed at 160.
    specular: f32,
    // `DirectionalLight3D.directional_shadow_max_distance` / `_fade_start`.
    // These take TWO of the three pads reserved with `specular`, so the struct
    // is still 160 bytes and every offset below the matrix is untouched.
    directional_shadow_max_distance: f32,
    directional_shadow_fade_start: f32,
    // This light's map size over the shadow atlas layer size: maps of different
    // sizes share one array texture, each in its layer's top-left corner
    // (pat-772py). The last of `specular`'s pads.
    shadow_uv_scale: f32,
    // pat-772py: Godot's directional filter, a NEW 16-byte row (160 -> 176).
    // `shadow_pcf_radius < 0` keeps the legacy single nearest compare (spot
    // maps and an unfitted directional one).
    shadow_depth_bias: f32,
    shadow_texel_world: f32,
    shadow_pcf_radius: f32,
    shadow_pcf_taps: u32,
};

struct LightArray {
    lights: array<LightData, 16>,
};
@group(2) @binding(0)
var<uniform> light_array: LightArray;

// pat-32edz / pat-mst61: CPU shadow maps as R32Float 2D-array layers
// (layer index = LightData.shadow_map_index).
@group(2) @binding(1)
var shadow_depth: texture_2d_array<f32>;

// pat-g4i60: omni (point-light) shadow cubemap faces as R32Float layers —
// 6 consecutive layers per shadow-casting point light, +X,−X,+Y,−Y,+Z,−Z
// (LightData.shadow_map_index = the light's base layer, i.e. its +X face).
@group(2) @binding(2)
var omni_shadow_depth: texture_2d_array<f32>;

// pat-p3p1m: captured ReflectionProbe cubemaps — 6 consecutive layers per
// probe in `CubeFace` order (+X,-X,+Y,-Y,+Z,-Z), linear HDR colour. Read with
// `textureLoad` (nearest texel), matching `ReflectionCubemap::sample_direction`.
@group(2) @binding(3)
var probe_cubes: texture_2d_array<f32>;

// pat-h7sz7: every captured probe of the frame, smallest box first (Godot's
// processing order), plus the sky radiance map, for PER-FRAGMENT blending.
// Mirrors `ProbeArrayData` / `gdserver3d::reflection_probe::ProbeSet`.
struct ProbeEntry {
    // xyz = influence box min, w = intensity.
    box_min: vec4<f32>,
    // xyz = influence box max, w = blend_distance.
    box_max: vec4<f32>,
    // xyz = capture origin, w = box_projection (1.0 = on).
    origin: vec4<f32>,
    // rgb = ConstantColor ambient (linear * energy).
    ambient: vec4<f32>,
    // x = base layer of the six faces in `probe_cubes`, PLUS ONE;
    // y = ambient mode (0 Disabled, 1 Environment, 2 ConstantColor).
    info: vec4<u32>,
};
struct ProbeArray {
    // x = probe count, y = sky base layer PLUS ONE (0 = no sky), z = face
    // resolution of every layer.
    header: vec4<u32>,
    // x = sky energy (`background_energy_multiplier`).
    sky: vec4<f32>,
    probes: array<ProbeEntry, 16>,
};
@group(2) @binding(4)
var<uniform> probe_array: ProbeArray;

// pat-h7sz7: Godot's baked split-sum DFG table (`shader::dfg_lut`): texel
// (i, j) = (A, B) at NdotV (i + 0.5) / N, roughness (j + 0.5) / N.
@group(2) @binding(5)
var dfg_lut: texture_2d<f32>;

// Port of `gdserver3d::box_projection::axis_exit`; 3e38 stands in for +inf.
fn probe_axis_exit(p: f32, r: f32, lo: f32, hi: f32) -> f32 {
    if (abs(r) < 1e-9) {
        return 3.0e38;
    }
    return max((hi - p) / r, (lo - p) / r);
}

// pat-9gjpr: each probe face layer is an ATLAS of the GGX roughness chain
// (`RoughnessChain`): level 0 (the raw capture, `res` square) at (0, 0), and
// level k >= 1 (`roughness_level_resolution`) at (res, sum of the sizes of
// levels 1..k-1). Mirrors `probe_atlas_layout` on the Rust side.
fn probe_level_size(k: i32) -> i32 {
    let res = i32(probe_array.header.z);
    if (k == 0) {
        return res;
    }
    return min(max(res >> u32(k), 4), res);
}

fn probe_level_origin(k: i32) -> vec2<i32> {
    if (k == 0) {
        return vec2<i32>(0, 0);
    }
    var y = 0;
    for (var j = 1; j < k; j = j + 1) {
        y = y + probe_level_size(j);
    }
    return vec2<i32>(i32(probe_array.header.z), y);
}

// Face index + [0,1] uv, the conventions of `ReflectionCubemap::sample_direction`.
fn probe_face_uv(dir: vec3<f32>) -> vec3<f32> {
    let a = abs(dir);
    var face = 0.0;
    var sc = 0.0;
    var tc = 0.0;
    var ma = 1.0;
    if (a.x >= a.y && a.x >= a.z) {
        ma = a.x;
        if (dir.x > 0.0) { face = 0.0; sc = -dir.z; tc = -dir.y; }
        else { face = 1.0; sc = dir.z; tc = -dir.y; }
    } else if (a.y >= a.z) {
        ma = a.y;
        if (dir.y > 0.0) { face = 2.0; sc = dir.x; tc = dir.z; }
        else { face = 3.0; sc = dir.x; tc = -dir.z; }
    } else {
        ma = a.z;
        if (dir.z > 0.0) { face = 4.0; sc = dir.x; tc = -dir.y; }
        else { face = 5.0; sc = -dir.x; tc = -dir.y; }
    }
    return vec3<f32>(face, 0.5 * (sc / ma + 1.0), 0.5 * (tc / ma + 1.0));
}

// Inverse of `probe_face_uv` (`cube_face_dir`): the direction through (u, v)
// of `face`.
fn probe_face_dir(face: i32, u: f32, v: f32) -> vec3<f32> {
    let sc = 2.0 * u - 1.0;
    let tc = 2.0 * v - 1.0;
    switch face {
        case 0: { return vec3<f32>(1.0, -tc, -sc); }
        case 1: { return vec3<f32>(-1.0, -tc, sc); }
        case 2: { return vec3<f32>(sc, 1.0, tc); }
        case 3: { return vec3<f32>(sc, -1.0, -tc); }
        case 4: { return vec3<f32>(sc, -tc, 1.0); }
        default: { return vec3<f32>(-sc, -tc, -1.0); }
    }
}

// One texel of roughness level `k` of `face`; a texel ONE past the face edge
// is fetched from the neighbouring face (`seamless_texel_index`, pat-h7sz7).
fn probe_tap(base: i32, face: i32, size: i32, origin: vec2<i32>, x: i32, y: i32) -> vec3<f32> {
    var f = face;
    var tx = x;
    var ty = y;
    if (x < 0 || y < 0 || x >= size || y >= size) {
        let d = probe_face_dir(face, (f32(x) + 0.5) / f32(size), (f32(y) + 0.5) / f32(size));
        let fuv = probe_face_uv(d);
        f = i32(fuv.x);
        tx = clamp(i32(floor(fuv.y * f32(size))), 0, size - 1);
        ty = clamp(i32(floor(fuv.z * f32(size))), 0, size - 1);
    }
    return textureLoad(probe_cubes, origin + vec2<i32>(tx, ty), base + f, 0).rgb;
}

// `CubeLevel::sample`: bilinear across face seams.
fn probe_bilinear(base: i32, face: i32, k: i32, uv: vec2<f32>) -> vec3<f32> {
    let size = probe_level_size(k);
    let o = probe_level_origin(k);
    let p = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)) * f32(size) - vec2<f32>(0.5);
    let p0 = floor(p);
    let f = p - p0;
    let x0 = i32(p0.x);
    let y0 = i32(p0.y);
    let a = probe_tap(base, face, size, o, x0, y0);
    let b = probe_tap(base, face, size, o, x0 + 1, y0);
    let c = probe_tap(base, face, size, o, x0, y0 + 1);
    let d = probe_tap(base, face, size, o, x0 + 1, y0 + 1);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// `RoughnessChain::sample_lod`: trilinear between two roughness levels of the
// cubemap whose +X face is layer `base`.
fn probe_sample_lod(base: i32, dir: vec3<f32>, lod_in: f32) -> vec3<f32> {
    let a = abs(dir);
    if (a.x + a.y + a.z <= 0.0) {
        return vec3<f32>(0.0);
    }
    let fuv = probe_face_uv(dir);
    let face = i32(fuv.x);
    let lod = clamp(lod_in, 0.0, 7.0);
    let i = i32(floor(lod));
    let t = lod - f32(i);
    let ca = probe_bilinear(base, face, i, fuv.yz);
    if (t <= 0.0 || i >= 7) {
        return ca;
    }
    let cb = probe_bilinear(base, face, i + 1, fuv.yz);
    return mix(ca, cb, t);
}

fn probe_contains(p: ProbeEntry, pos: vec3<f32>) -> bool {
    return all(pos >= p.box_min.xyz) && all(pos <= p.box_max.xyz);
}

// `ProbeReflection::blend_weight`.
fn probe_blend(p: ProbeEntry, world_pos: vec3<f32>) -> f32 {
    let box_min = p.box_min.xyz;
    let box_max = p.box_max.xyz;
    var blend = 1.0;
    let bd = p.box_max.w;
    if (bd > 0.0) {
        let center = (box_min + box_max) * 0.5;
        let extents = (box_max - box_min) * 0.5;
        let local = world_pos - center;
        for (var i = 0; i < 3; i = i + 1) {
            let e = extents[i];
            let band = min(bd, e);
            if (band > 0.0) {
                let axis = (abs(local[i]) - e + band) / band;
                blend = blend * clamp(1.0 - axis, 0.0, 1.0);
            }
        }
        blend = blend * blend;
    }
    return blend;
}

// `ProbeReflection::lookup_dir`: box projection when enabled.
fn probe_lookup_dir(p: ProbeEntry, world_pos: vec3<f32>, reflection: vec3<f32>) -> vec3<f32> {
    if (p.origin.w > 0.5 && length(reflection) >= 1e-9) {
        let ex = probe_axis_exit(world_pos.x, reflection.x, p.box_min.x, p.box_max.x);
        let ey = probe_axis_exit(world_pos.y, reflection.y, p.box_min.y, p.box_max.y);
        let ez = probe_axis_exit(world_pos.z, reflection.z, p.box_min.z, p.box_max.z);
        let dist = min(ex, min(ey, ez));
        if (dist >= 1.0e38 || dist <= 0.0) {
            return normalize(reflection);
        }
        return normalize(world_pos + reflection * dist - p.origin.xyz);
    }
    return reflection;
}

struct EnvIndirect {
    ambient: vec3<f32>,
    specular: vec3<f32>,
};

// Port of `ProbeSet::indirect` (pat-h7sz7), Godot's cluster loop
// (`scene_forward_clustered.glsl` 2027-2099 + `reflection_process`): every
// probe holding the fragment, smallest first, accumulates reflection and
// ambient by its fade weight until both reach 1; the sky radiance and the
// environment ambient take what is left.
fn env_indirect(world_pos: vec3<f32>, n: vec3<f32>, v: vec3<f32>, roughness: f32, base_ambient: vec3<f32>) -> EnvIndirect {
    let r = clamp(roughness, 0.0, 1.0);
    var refl = vec4<f32>(0.0);
    var amb = vec4<f32>(0.0);
    let count = min(probe_array.header.x, 16u);
    if (count > 0u) {
        let mirror = normalize(n * (2.0 * dot(n, v)) - v);
        let r2 = r * r;
        let ref_vec = mix(mirror, n, r2 * r2);
        let lod = sqrt(r) * 7.0;
        for (var i = 0u; i < count; i = i + 1u) {
            if ((model.probe_mask & (1u << i)) == 0u) {
                continue;
            }
            // Box test on two fields before touching the rest of the entry.
            if (any(world_pos < probe_array.probes[i].box_min.xyz)
                || any(world_pos > probe_array.probes[i].box_max.xyz)) {
                continue;
            }
            let p = probe_array.probes[i];
            if (refl.a >= 1.0 && amb.a >= 1.0) {
                break;
            }
            let blend = probe_blend(p, world_pos);
            let base = i32(p.info.x) - 1;
            if (p.box_min.w > 0.0 && refl.a < 1.0) {
                let rb = max(blend - refl.a, 0.0);
                let c = probe_sample_lod(base, probe_lookup_dir(p, world_pos, ref_vec), lod);
                refl = refl + vec4<f32>(c * (p.box_min.w * rb), rb);
            }
            if (amb.a < 1.0 && p.info.y != 0u) {
                let ab = max(blend - amb.a, 0.0);
                var c = p.ambient.rgb;
                if (p.info.y == 1u) {
                    c = probe_sample_lod(base, n, 7.0);
                }
                amb = amb + vec4<f32>(c * ab, ab);
            }
        }
    }
    var out: EnvIndirect;
    out.ambient = amb.rgb + base_ambient * max(1.0 - amb.a, 0.0);
    out.specular = refl.rgb;
    if (refl.a < 1.0 && probe_array.header.y != 0u) {
        // `SkyRadiance::specular`: mix by roughness² (not ⁴) and horizon².
        let rv = mix(n * (2.0 * dot(n, v)) - v, n, r * r);
        let horizon = min(1.0 + dot(rv, n), 1.0);
        let s = probe_sample_lod(i32(probe_array.header.y) - 1, rv, sqrt(r) * 7.0)
            * (horizon * horizon * probe_array.sky.x);
        out.specular = refl.rgb + s * (1.0 - refl.a);
    }
    return out;
}

// `shader::env_brdf`: bilinear, edge-clamped lookup of Godot's baked DFG
// table: (f90 term, total).
fn env_brdf(roughness: f32, n_dot_v: f32) -> vec2<f32> {
    let n = textureDimensions(dfg_lut).x;
    let nf = f32(n);
    let fx = clamp(clamp(n_dot_v, 0.0, 1.0) * nf - 0.5, 0.0, nf - 1.0);
    let fy = clamp(clamp(roughness, 0.0, 1.0) * nf - 0.5, 0.0, nf - 1.0);
    let x0 = i32(floor(fx));
    let y0 = i32(floor(fy));
    let x1 = min(x0 + 1, i32(n) - 1);
    let y1 = min(y0 + 1, i32(n) - 1);
    let tx = fx - f32(x0);
    let ty = fy - f32(y0);
    let a = textureLoad(dfg_lut, vec2<i32>(x0, y0), 0).xy;
    let b = textureLoad(dfg_lut, vec2<i32>(x1, y0), 0).xy;
    let c = textureLoad(dfg_lut, vec2<i32>(x0, y1), 0).xy;
    let d = textureLoad(dfg_lut, vec2<i32>(x1, y1), 0).xy;
    let v = mix(mix(a, b, tx), mix(c, d, tx), ty);
    return vec2<f32>(v.x, max(v.y, 1e-4));
}

// ── Vertex stage ────────────────────────────────────────────────────

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    let world_pos = model.model_matrix * vec4<f32>(in.position, 1.0);
    out.world_position = world_pos.xyz;

    let view_pos = camera.view_matrix * world_pos;
    out.clip_position = camera.projection_matrix * view_pos;

    // Transform normal by upper-left 3x3 of model matrix.
    let normal_mat = mat3x3<f32>(
        model.model_matrix[0].xyz,
        model.model_matrix[1].xyz,
        model.model_matrix[2].xyz,
    );
    out.world_normal = normalize(normal_mat * in.normal);

    out.uv = in.uv;

    return out;
}

// ── Fragment stage ──────────────────────────────────────────────────

fn distance_attenuation(dist: f32, range: f32, atten: f32) -> f32 {
    if (range <= 0.0) {
        return 1.0;
    }
    let ratio = min(dist / range, 1.0);
    return max(1.0 - pow(ratio, atten), 0.0);
}

fn evaluate_light(light: LightData, frag_pos: vec3<f32>) -> vec2<f32> {
    // Returns (dot_product_factor, intensity) packed as vec2.
    // Caller uses the light direction separately.
    if (light.kind == 0u) {
        // Directional.
        return vec2<f32>(1.0, 1.0);
    }
    let to_light = light.position - frag_pos;
    let dist = length(to_light);
    if (dist < 1e-6) {
        return vec2<f32>(1.0, 1.0);
    }
    let atten = distance_attenuation(dist, light.range, light.attenuation);
    if (light.kind == 2u) {
        // Spot cone attenuation.
        let dir = normalize(to_light);
        let cos_angle = dot(-dir, light.direction);
        let cos_outer = cos(light.spot_angle);
        if (cos_angle <= cos_outer) {
            return vec2<f32>(0.0, 0.0);
        }
        let t = clamp((cos_angle - cos_outer) / (1.0 - cos_outer), 0.0, 1.0);
        return vec2<f32>(1.0, atten * pow(t, light.spot_angle_attenuation));
    }
    return vec2<f32>(1.0, atten);
}

fn get_light_dir(light: LightData, frag_pos: vec3<f32>) -> vec3<f32> {
    if (light.kind == 0u) {
        return light.direction;
    }
    let to_light = light.position - frag_pos;
    let dist = length(to_light);
    if (dist < 1e-6) {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    return to_light / dist;
}

// pat-32edz: directional shadow factor. Mirrors software `ShadowMap::sample` +
// `compute_shadow_factor` (returns 1.0 fully lit, 0.0 fully shadowed).
// 🔑 The depth range the shadow frustum projects with — `shadow_map::SHADOW_FAR
// - SHADOW_NEAR`. A `shadow_bias` is authored in WORLD units while this
// comparison is in NORMALIZED post-divide depth, so the bias must be divided by
// this before it means anything here: Godot's directional default of 0.1 world
// units is ~0.001 normalized. Feeding the raw value in would be a hundred times
// too large and detach every shadow from its caster.
//
// ⚠️ A WGSL literal cannot be derived from the Rust constants; a test asserts
// they agree.
const SHADOW_DEPTH_RANGE: f32 = 99.9;
// World size of one shadow-map texel: `2 * shadow_map::SHADOW_EXTENT /
// SHADOW_MAP_SIZE`. `shadow_normal_bias` is authored relative to this, so the
// offset below scales by it.
//
// ⚠️ Another literal WGSL cannot derive; a test asserts it matches the Rust
// constants, exactly like SHADOW_DEPTH_RANGE above.
const SHADOW_TEXEL_SIZE: f32 = 0.15625;

// Godot's NORMAL-OFFSET shadows (`scene_forward_lights_inc.glsl:489`): push the
// sample point along the surface normal before the lookup, by
// `shadow_normal_bias * texel_size * (1 - |dot(n, l)|)`.
//
// 🔑 The `1 - |dot|` term is the point — the offset grows as the surface turns
// EDGE-ON to the light, where acne happens, and vanishes where it faces the
// light squarely. Without it this is just a second constant bias.
//
// ⚠️ POSITIONAL lights carry an extra x10 (`light_storage.cpp:968`, which sits
// above Godot's spot/omni split so it covers BOTH); directional uses the
// unscaled form at `:724`.
fn shadow_normal_offset(light: LightData, world_pos: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let light_dir = get_light_dir(light, world_pos);
    var type_scale = 10.0;
    if (light.kind == 0u) {
        type_scale = 1.0;
    }
    let scale = light.shadow_normal_bias * SHADOW_TEXEL_SIZE * type_scale
        * (1.0 - abs(dot(normal, light_dir)));
    return normal * scale;
}
// Sentinel for unwritten shadow texels (CPU uses f32::MAX).
const SHADOW_EMPTY: f32 = 1.0e30;

// Mirrors `ShadowCubemap::sample` + `direction_to_face_uv` (gdserver3d
// light.rs): dominant-axis face select, (sc/ma, tc/ma) → [0,1] UV, truncating
// texel conversion, then the linear-distance compare with the light's own
// `shadow_bias` (world units, so no conversion — see `LightData.shadow_bias`).
// Returns 1.0 fully lit, 0.0 fully shadowed.
fn omni_shadow_factor(light: LightData, world_pos: vec3<f32>, normal: vec3<f32>) -> f32 {
    if (light.shadow_enabled == 0u || light.kind != 1u || light.shadow_map_index < 0) {
        return 1.0;
    }
    let biased = world_pos + shadow_normal_offset(light, world_pos, normal);
    let to_frag = biased - light.position;
    let dist = length(to_frag);

    let ax = abs(to_frag.x);
    let ay = abs(to_frag.y);
    let az = abs(to_frag.z);
    var face: i32;
    var sc: f32;
    var tc: f32;
    var ma: f32;
    if (ax >= ay && ax >= az) {
        if (to_frag.x > 0.0) {
            face = 0; sc = -to_frag.z; tc = -to_frag.y; ma = ax;
        } else {
            face = 1; sc = to_frag.z; tc = -to_frag.y; ma = ax;
        }
    } else if (ay >= ax && ay >= az) {
        if (to_frag.y > 0.0) {
            face = 2; sc = to_frag.x; tc = to_frag.z; ma = ay;
        } else {
            face = 3; sc = to_frag.x; tc = -to_frag.z; ma = ay;
        }
    } else {
        if (to_frag.z > 0.0) {
            face = 4; sc = to_frag.x; tc = -to_frag.y; ma = az;
        } else {
            face = 5; sc = -to_frag.x; tc = -to_frag.y; ma = az;
        }
    }
    var u = 0.5;
    var v = 0.5;
    if (ma >= 1.1920929e-7) { // f32::EPSILON, like the software guard
        u = clamp(sc / ma * 0.5 + 0.5, 0.0, 1.0);
        v = clamp(tc / ma * 0.5 + 0.5, 0.0, 1.0);
    }

    let dims = textureDimensions(omni_shadow_depth);
    let tx = min(u32(u * f32(dims.x)), dims.x - 1u);
    let ty = min(u32(v * f32(dims.y)), dims.y - 1u);
    let layer = light.shadow_map_index + face;
    let stored = textureLoad(omni_shadow_depth, vec2<i32>(i32(tx), i32(ty)), layer, 0).r;
    if (stored >= SHADOW_EMPTY) {
        return 1.0; // No occluder recorded in this texel.
    }
    // ⚠️ Directly, with NO division: cubemap depths are a linear distance from
    // the light, so this comparison is already in the world units the property
    // is authored in — unlike the normalized 2D compare above.
    if (dist > stored + light.shadow_bias) {
        // `shadow_opacity` scales the OCCLUSION, not the visibility, exactly as
        // the software `compute_shadow_factor` does: `1 - occlusion * opacity`.
        // Occlusion is binary here, so a full occluder returns `1 - opacity` —
        // opacity 0 leaves the fragment fully lit and 1 is the old `0.0`.
        return 1.0 - clamp(light.shadow_opacity, 0.0, 1.0);
    }
    return 1.0;
}

// Godot's `quick_hash` (`scene_forward_lights_inc.glsl`): the per-pixel
// rotation of the PCF disk, so the tap pattern becomes fine noise instead of
// banding.
fn shadow_quick_hash(pos: vec2<f32>) -> f32 {
    let magic = vec3<f32>(0.06711056, 0.00583715, 52.9829189);
    return fract(magic.z * fract(dot(pos, magic.xy)));
}

// One bilinear depth-compare tap at `p`, in texels of this light's map: the
// `sampler2DShadow` + linear filter Godot samples with, done by hand on the
// R32Float atlas. Returns the LIT fraction. Texels outside the map are lit.
fn shadow_tap_bilinear(layer: i32, p: vec2<f32>, size: f32, depth: f32) -> f32 {
    let q = p - vec2<f32>(0.5);
    let base = floor(q);
    let f = q - base;
    let b = vec2<i32>(base);
    var lit: array<f32, 4>;
    for (var k = 0; k < 4; k = k + 1) {
        let t = b + vec2<i32>(k & 1, k >> 1u);
        var v = 1.0;
        if (t.x >= 0 && t.y >= 0 && f32(t.x) < size && f32(t.y) < size) {
            let stored = textureLoad(shadow_depth, t, layer, 0).r;
            if (stored < SHADOW_EMPTY && depth > stored) {
                v = 0.0;
            }
        }
        lit[k] = v;
    }
    return mix(mix(lit[0], lit[1], f.x), mix(lit[2], lit[3], f.x), f.y);
}

// pat-772py: a camera-fitted directional map is sampled the way Godot's
// forward renderer samples it (`scene_forward_clustered.glsl:2285-2430`):
//
// - `BIAS_FUNC`'s normal offset: the GEOMETRY normal times
//   `1 - max(0, dot(L, -n))`, times `shadow_normal_bias * texel`, with its
//   component along the light ray removed;
// - the constant bias, already folded to this map's normalized depth on the CPU;
// - `sample_directional_pcf_shadow`: `taps` bilinear compares on a Vogel disk
//   rotated per pixel by `quick_hash(gl_FragCoord)`, or ONE bilinear compare at
//   the HARD level.
fn fitted_directional_shadow(light: LightData, world_pos: vec3<f32>, geo_normal: vec3<f32>, frag_xy: vec2<f32>) -> f32 {
    let l = light.direction;
    var nb = geo_normal * (1.0 - max(0.0, dot(l, -geo_normal)))
        * light.shadow_normal_bias * light.shadow_texel_world;
    nb = nb - l * dot(l, nb);
    let clip = light.shadow_view_proj * vec4<f32>(world_pos + nb, 1.0);
    if (clip.w <= 0.0) {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    let u = (ndc.x + 1.0) * 0.5;
    let v = (1.0 - ndc.y) * 0.5;
    if (u < 0.0 || u > 1.0 || v < 0.0 || v > 1.0) {
        return 1.0; // Outside shadow map = lit.
    }
    let size = f32(textureDimensions(shadow_depth).x) * light.shadow_uv_scale;
    let p = vec2<f32>(u, v) * size;
    let depth = ndc.z - light.shadow_depth_bias;
    let layer = light.shadow_map_index;
    var lit = 0.0;
    if (light.shadow_pcf_taps == 0u) {
        lit = shadow_tap_bilinear(layer, p, size, depth);
    } else {
        let r = shadow_quick_hash(frag_xy) * 6.28318530718;
        let sr = sin(r);
        let cr = cos(r);
        let n = f32(light.shadow_pcf_taps);
        for (var i = 0u; i < light.shadow_pcf_taps; i = i + 1u) {
            // Godot's `get_vogel_disk`: r = sqrt(i + 0.5) / sqrt(n), theta = 2.4 i.
            let kr = sqrt(f32(i) + 0.5) / sqrt(n);
            let kt = f32(i) * 2.4;
            let k = vec2<f32>(cos(kt) * kr, sin(kt) * kr);
            // GLSL `mat2(vec2(cr, -sr), vec2(sr, cr)) * k`.
            let rk = vec2<f32>(cr * k.x + sr * k.y, -sr * k.x + cr * k.y);
            lit = lit + shadow_tap_bilinear(layer, p + rk * light.shadow_pcf_radius, size, depth);
        }
        lit = lit / n;
    }
    return 1.0 - (1.0 - lit) * clamp(light.shadow_opacity, 0.0, 1.0);
}

fn directional_shadow_factor(light: LightData, world_pos: vec3<f32>, normal: vec3<f32>, geo_normal: vec3<f32>, frag_xy: vec2<f32>) -> f32 {
    // kind 0 = directional, 2 = spot (both use the 2D depth map).
    if (light.shadow_enabled == 0u || (light.kind != 0u && light.kind != 2u) || light.shadow_map_index < 0) {
        return 1.0;
    }
    if (light.kind == 0u && light.shadow_pcf_radius >= 0.0) {
        return fitted_directional_shadow(light, world_pos, geo_normal, frag_xy);
    }
    let biased = world_pos + shadow_normal_offset(light, world_pos, normal);
    let clip = light.shadow_view_proj * vec4<f32>(biased, 1.0);
    let w = clip.w;
    if (w <= 0.0) {
        return 1.0;
    }
    let ndc_x = clip.x / w;
    let ndc_y = clip.y / w;
    let depth = clip.z / w;

    // NDC [-1,1] → UV [0,1], flip Y like software ShadowMap::sample.
    let u = (ndc_x + 1.0) * 0.5;
    let v = (1.0 - ndc_y) * 0.5;
    if (u < 0.0 || u > 1.0 || v < 0.0 || v > 1.0) {
        return 1.0; // Outside shadow map = lit.
    }

    // `shadow_uv_scale`: this map fills only the top-left of its atlas layer
    // when a larger map shares the array (pat-772py).
    let dims = vec2<f32>(textureDimensions(shadow_depth)) * light.shadow_uv_scale;
    let tx = i32(u * dims.x);
    let ty = i32(v * dims.y);
    if (tx < 0 || ty < 0 || f32(tx) >= dims.x || f32(ty) >= dims.y) {
        return 1.0;
    }
    let layer = light.shadow_map_index;
    let stored_depth = textureLoad(shadow_depth, vec2<i32>(tx, ty), layer, 0).r;
    if (stored_depth >= SHADOW_EMPTY) {
        return 1.0; // No occluder at this texel.
    }
    // Farther than stored depth + bias → in shadow.
    if (depth > stored_depth + light.shadow_bias / SHADOW_DEPTH_RANGE) {
        // Same rule as `omni_shadow_factor`: scale the occlusion, not the
        // visibility, so this matches the software renderer term for term.
        return 1.0 - clamp(light.shadow_opacity, 0.0, 1.0);
    }
    return 1.0;
}

// Fades a DIRECTIONAL light's shadow out as it approaches the light's shadow
// distance, so the last cascade dissolves instead of ending on a visible edge.
//
// Godot: fade_to = -max_distance, fade_from = -max_distance * min(fade_start,
// 0.999), shadow = mix(shadow, 1.0, smoothstep(fade_from, fade_to, view_z))
// (`light_storage.cpp:753-754`, `scene_forward_clustered.glsl:2471`).
//
// ⚠️ `view_z` is NEGATIVE in front of the camera, so fade_from (-80 at the
// defaults) is GREATER than fade_to (-100) and the interval runs downwards.
// Reading the edges as positive distances inverts the fade — the near geometry
// would lose its shadows instead of the far.
//
// ⚠️ DIRECTIONAL ONLY. `directional_shadow_factor` also serves SPOT lights
// (kind 2), so the kind test here is not redundant: without it every spot
// light would lose its shadow past 100 units too.
//
// ⚠️ The `max_distance <= 0` guard is ours, not Godot's — a `.tscn` may author
// zero, which collapses both edges onto 0.0 and makes the ratio 0/0. A NaN
// here poisons every fragment, not just the distant ones.
fn directional_shadow_fade(light: LightData, world_position: vec3<f32>, shadow: f32) -> f32 {
    if (light.kind != 0u || light.directional_shadow_max_distance <= 0.0) {
        return shadow;
    }
    let view_z = (camera.view_matrix * vec4<f32>(world_position, 1.0)).z;
    let fade_to = -light.directional_shadow_max_distance;
    let fade_from = -light.directional_shadow_max_distance
        * min(light.directional_shadow_fade_start, 0.999);
    let t = clamp((view_z - fade_from) / (fade_to - fade_from), 0.0, 1.0);
    let fade = t * t * (3.0 - 2.0 * t);
    return shadow + (1.0 - shadow) * fade;
}

struct TangentFrame {
    tangent: vec3<f32>,
    bitangent: vec3<f32>,
};

// Duff et al.'s branchless orthonormal basis, built from the NORMAL ALONE.
//
// ⚠️ The bead asked for a "screen-derivative TBN like the software path", but
// the software path does not use screen derivatives at all: `tangent_frame`
// (`shader.rs:273`) derives the frame from the normal and never touches
// `dpdx`/`dpdy` or the UVs. Mirroring what the code does rather than what the
// bead says is the whole point — the acceptance compares the two backends, so a
// different (even defensible) frame is a visible mismatch.
//
// No singularity to guard: `s + n.z` is zero only when `n.z == -s`, and `s` is
// chosen as the sign of `n.z`, so the two can never cancel. That is what
// "branchless" buys.
fn tangent_frame(normal: vec3<f32>) -> TangentFrame {
    let n = normalize(normal);
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    var out: TangentFrame;
    out.tangent = vec3<f32>(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    out.bitangent = vec3<f32>(b, s + n.y * n.y * a, -n.y);
    return out;
}

// Decodes a tangent-space texel and rotates it into world space, term for term
// as `apply_normal_map` (`shader.rs:286`) does — including the `max(.., 1e-4)`
// floor on the Z term, which keeps the perturbed normal on the outward side of
// the surface even for a texel whose blue channel is below 0.5.
fn apply_normal_map(world_normal: vec3<f32>, texel: vec4<f32>, scale: f32) -> vec3<f32> {
    let n = normalize(world_normal);
    let tx = (texel.r * 2.0 - 1.0) * scale;
    let ty = (texel.g * 2.0 - 1.0) * scale;
    let tz = max(texel.b * 2.0 - 1.0, 1e-4);
    let f = tangent_frame(n);
    let perturbed = vec3<f32>(
        f.tangent.x * tx + f.bitangent.x * ty + n.x * tz,
        f.tangent.y * tx + f.bitangent.y * ty + n.y * tz,
        f.tangent.z * tx + f.bitangent.z * ty + n.z * tz,
    );
    return normalize(perturbed);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let albedo = model.albedo;
    // ⚠️ The line ABOVE is the splice marker `custom_spatial_wgsl` matches on
    // (see the `marker` literal below in this file). It must stay
    // byte-identical or that function bails and every custom spatial shader
    // silently falls back to flat albedo (pat-s3rrv.122). So the texture
    // multiply lands here under a NEW name and the uses below are renamed,
    // rather than changing the binding itself.
    //
    // An untextured material binds a 1x1 WHITE texel, so this multiply is an
    // exact no-op and such draws stay bit-identical to before. A custom shader
    // also binds white (its entry carries no albedo texture), so `albedo_tex`
    // is `custom_albedo(in)` unchanged.
    let albedo_tex = albedo * textureSample(base_albedo_map, base_material_sampler, in.uv);
    let emission = model.emission * textureSample(base_emission_map, base_material_sampler, in.uv);
    // ⚠️ Sampled HERE, not where they are used. The roughness and metallic
    // scalars are consumed inside the light loop, which sits behind a `for` and
    // an `if` — `textureSample` requires UNIFORM control flow, so fetching them
    // there would be invalid WGSL. Hoisting also matches the software path,
    // which samples the material once per fragment (`sample_material`).
    //
    // Green for roughness and blue for metallic are Godot's defaults, not an
    // arbitrary choice of channel. A 1x1 white texel reads 1.0 in every
    // channel, so an untextured material multiplies by one.
    let roughness_map = textureSample(base_roughness_map, base_material_sampler, in.uv).g;
    let metallic_map = textureSample(base_metallic_map, base_material_sampler, in.uv).b;

    // Unlit mode. Scene-referred linear HDR: no clamp, so emissive values > 1
    // survive to feed glow (the target is Rgba16Float; the camera post pass
    // tonemaps and clamps at the end) — same as the software shaders.
    if (model.shading_mode == 0u) {
        return vec4<f32>(
            albedo_tex.r + emission.r,
            albedo_tex.g + emission.g,
            albedo_tex.b + emission.b,
            albedo_tex.a,
        );
    }

    let geo_normal = normalize(in.world_normal);
    // Sampled UNCONDITIONALLY: `textureSample` requires uniform control flow,
    // and an unconditional fetch is trivially uniform. The flag below only
    // selects between two already-computed values, so it never gates a fetch.
    let normal_texel = textureSample(base_normal_map, base_material_sampler, in.uv);
    let mapped_normal = apply_normal_map(geo_normal, normal_texel, model.normal_scale);
    let normal = select(geo_normal, mapped_normal, model.has_normal_map != 0u);
    // An UNCAPTURED probe's ambient is per INSTANCE and adds to the
    // environment ambient, as in the software `entry_ambient` (pat-p3p1m). A
    // CAPTURED probe replaces it per fragment (`probe_ambient_at`, pat-t42ca).
    let base_ambient = camera.ambient.rgb + model.probe_ambient.rgb;
    let probe_roughness = clamp(model.roughness * roughness_map, 0.0, 1.0);
    let probe_metallic = clamp(model.metallic * metallic_map, 0.0, 1.0);
    // pat-h7sz7: probes blended per fragment, the sky radiance behind them.
    let has_env = model.probe_env != 0u;
    var ambient = base_ambient;
    var env_specular = vec3<f32>(0.0);
    if (has_env) {
        let ev = normalize(camera.camera_position - in.world_position);
        let e = env_indirect(in.world_position, normal, ev, probe_roughness, base_ambient);
        ambient = e.ambient;
        env_specular = e.specular;
    }
    // Ambient is DIFFUSE light, so in PBR (mode 2) a metal takes none of it:
    // Godot multiplies it by albedo and then by `1.0 - metallic`, which is what
    // the software `pbr_shade` does (`albedo_lin`). The GPU used the raw
    // albedo, leaving a full ambient under every metal (pat-p3p1m: a probe
    // mirror read ~half as red as the software reference). Lambert (mode 1)
    // keeps the raw albedo here, as the software Lambert shader does.
    let ambient_albedo = select(
        albedo_tex.rgb,
        albedo_tex.rgb * (1.0 - probe_metallic),
        model.shading_mode == 2u,
    );
    var color = ambient * ambient_albedo;

    // Image-based specular from a captured ReflectionProbe — the software
    // shaders' block, term for term (`shader.rs`, pat-s3rrv.206). Only a
    // smooth surface (`sharpness > 0`) takes it, so a default rough material
    // is unchanged. Lambert (mode 1) trades its diffuse for the reflection by
    // `keep` (metals have no diffuse); PBR (mode 2) adds it to the ambient.
    let probe_sharpness = 1.0 - probe_roughness;
    var probe_env = vec3<f32>(0.0);
    // Lambert (mode 1) keeps its sharpness-weighted mirror term, now
    // roughness-filtered (pat-9gjpr).
    let has_probe = has_env && probe_sharpness > 0.0;
    if (has_probe && model.shading_mode == 1u) {
        let f0 = mix(vec3<f32>(0.04), albedo_tex.rgb, probe_metallic);
        probe_env = env_specular * f0 * probe_sharpness;
    }
    // PBR (mode 2): `shader::probe_indirect`, Godot's IBL specular — any
    // roughness, specular occlusion from the ambient, split-sum env BRDF.
    if (has_env && model.shading_mode == 2u) {
        let v = normalize(camera.camera_position - in.world_position);
        let env = env_specular;
        let lum = dot(ambient, vec3<f32>(0.3, 0.59, 0.11)) * 2.0;
        var so = min(lum * 4.0, 1.0);
        let reflective_f = (1.0 - probe_roughness) * probe_metallic;
        so = max(min(reflective_f * so * 10.0, 1.0), so);
        let n_dot_v = clamp(dot(normal, v), 1e-4, 1.0);
        let dfg = env_brdf(probe_roughness, n_dot_v);
        let f0 = mix(vec3<f32>(0.04), albedo_tex.rgb, probe_metallic);
        let f90 = clamp(50.0 * f0.g, probe_metallic, 1.0);
        let ec = vec3<f32>(1.0) + f0 * (1.0 / dfg.y - 1.0);
        color += env * so * ec * ((vec3<f32>(f90) - f0) * dfg.x + f0 * dfg.y);
    }

    let num_lights = min(model.num_lights, 16u);
    for (var i = 0u; i < num_lights; i = i + 1u) {
        let light = light_array.lights[i];
        // Godot's `light_cull_mask`: both sides are BITMASKS, so the test is an
        // intersection. ⚠️ NOT an index shift — `layers` of 0b101 means "on
        // layers 1 and 3", and reading it as the number 5 agrees for every
        // single-layer mask (including the default 1) while mislighting
        // everything from layer 3 up.
        if ((light.cull_mask & model.layers) == 0u) {
            continue;
        }
        let light_dir = get_light_dir(light, in.world_position);
        let eval = evaluate_light(light, in.world_position);
        let intensity = eval.y;

        let n_dot_l = max(dot(normal, light_dir), 0.0);
        // Each factor is 1.0 for light kinds it doesn't handle, so the
        // product applies whichever map (2D or cubemap) this light carries.
        let raw_shadow = directional_shadow_factor(light, in.world_position, normal, geo_normal, in.clip_position.xy)
            * omni_shadow_factor(light, in.world_position, normal);
        let shadow = directional_shadow_fade(light, in.world_position, raw_shadow);
        let contrib = n_dot_l * intensity * shadow;

        // Lambert diffuse (mode == 1).
        if (model.shading_mode == 1u) {
            color += vec3<f32>(
                albedo_tex.r * light.color.r * contrib,
                albedo_tex.g * light.color.g * contrib,
                albedo_tex.b * light.color.b * contrib,
            );
        }

        // Burley + Schlick-GGX (mode == 2, Godot default PBR / pat-sf07y).
        if (model.shading_mode == 2u && n_dot_l > 0.0 && intensity > 0.0) {
            let view_dir = normalize(camera.camera_position - in.world_position);
            let n_dot_v = max(dot(normal, view_dir), 1e-4);
            let h = normalize(light_dir + view_dir);
            let n_dot_h = max(dot(normal, h), 1e-4);
            let l_dot_h = max(dot(light_dir, h), 1e-4);
            // Multiply THEN clamp, which is the software order
            // (`shader.rs:239-248`: `roughness *= texel.g`, clamped only once
            // the sampled material is assembled). Clamping first would discard
            // the texture's contribution for any authored value at the rail.
            let roughness = clamp(model.roughness * roughness_map, 0.0, 1.0);
            let metallic = clamp(model.metallic * metallic_map, 0.0, 1.0);
            let alpha = max(roughness * roughness, 1e-4);
            let f0_d = 0.04;
            let f0 = mix(vec3<f32>(f0_d), albedo_tex.rgb, metallic);
            let albedo_lin = albedo_tex.rgb * (1.0 - metallic);

            // Burley diffuse.
            let fd90 = 0.5 + 2.0 * l_dot_h * l_dot_h * roughness;
            let fl = pow(1.0 - n_dot_l, 5.0);
            let fv = pow(1.0 - n_dot_v, 5.0);
            let burley = (1.0 / 3.14159265) * (1.0 + (fd90 - 1.0) * fl) * (1.0 + (fd90 - 1.0) * fv);
            // Godot's `energy *= PI` (light_storage.cpp) cancels the 1/PI in
            // Burley and D_GGX: `shader::GODOT_LIGHT_ENERGY_SCALE` (pat-9gjpr).
            let pi_energy = 3.14159265;
            color += albedo_lin * light.color * (n_dot_l * intensity * shadow * burley * pi_energy);

            // GGX D * V * F.
            let a = n_dot_h * alpha;
            let k = alpha / max(1.0 - n_dot_h * n_dot_h + a * a, 1e-6);
            let d = k * k * (1.0 / 3.14159265);
            let vis_a = 2.0 * n_dot_l * n_dot_v;
            let vis_b = n_dot_l + n_dot_v;
            // Cap like the software v_ggx (min 1e4): uncapped grazing-angle
            // visibility fireflies only on the GPU.
            let vis = min(0.5 / max(mix(vis_a, vis_b, alpha), 1e-6), 1.0e4);
            let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - l_dot_h, 5.0);
            // `light_specular` scales the light's specular contribution.
            // ⚠️ Godot applies it RAW for directional lights
            // (light_storage.cpp:665) and DOUBLED for positional ones (:891),
            // paired with defaults of 1.0 on Light3D/DirectionalLight3D against
            // 0.5 on OmniLight3D/SpotLight3D — so both come out at an effective
            // 1.0 and this is invisible at default values. Mirrors the software
            // `pbr_shade`; dropping the x2 here alone would make the GPU path
            // half as glossy as the CPU one for every omni and spot light.
            let specular_type_scale = select(2.0, 1.0, light.kind == 0u);
            color += light.color
                * f
                * (d * vis * n_dot_l * intensity * shadow * light.specular * specular_type_scale * pi_energy);
        }
    }

    if (has_probe && model.shading_mode == 1u) {
        let keep = 1.0 - probe_metallic * probe_sharpness;
        color = color * keep + probe_env;
    }

    // Scene-referred linear HDR: no clamp (tonemapping clamps at the end).
    return vec4<f32>(
        color.r + emission.r,
        color.g + emission.g,
        color.b + emission.b,
        albedo_tex.a,
    );
}
"#;

/// Fullscreen procedural-sky background shader in WGSL.
///
/// Draws one fullscreen triangle and evaluates the exact math of the software
/// rasterizer's background fill per pixel: `pixel_view_direction` (unproject
/// the pixel's NDC through the inverse projection, rotate by the camera
/// basis) followed by `sample_procedural_sky` — the Godot 4.6
/// `ProceduralSkyMaterial` shader port: `mix(top, horizon, pow(1 ∓ y,
/// inv_curve))` gradients per hemisphere plus up to 4 directional-light sun
/// discs/halos on the sky half. Keeping the math identical is what makes the
/// GPU editor viewport background match the software reference 1:1.
pub const SKY_WGSL: &str = r#"
struct SkyUniforms {
    inv_projection: mat4x4<f32>,
    // Camera world basis (rotation only) in the upper-left 3x3.
    camera_basis: mat4x4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    ground_bottom: vec4<f32>,
    ground_horizon: vec4<f32>,
    // x = inv_sky_curve (0.6 / sky_curve), y = inv_ground_curve,
    // z = sky material energy, w = ground material energy
    params: vec4<f32>,
    // x = viewport width, y = viewport height
    size: vec4<f32>,
    // Per directional light: xyz = world direction toward the sun, w = energy.
    sun_dir_energy: array<vec4<f32>, 4>,
    // Per directional light: rgb = color, w = angular size in radians.
    sun_color_size: array<vec4<f32>, 4>,
    // x = cos(sun_angle_max), y = inv_sun_curve (1.6 / pow(sun_curve, 1.4)),
    // z = enabled sun count, w = environment background energy.
    sun_params: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> sky: SkyUniforms;
// pat-cezo4: the equirect panorama (a PanoramaSkyMaterial's texture, or a
// cubemap shader sky baked on the CPU). A 1x1 black texel when unbound;
// `sky.params.w > 1.5` says one is bound.
@group(0) @binding(1)
var sky_panorama: texture_2d<f32>;
@group(0) @binding(2)
var sky_panorama_sampler: sampler;

struct SkyVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyVertexOutput {
    // Fullscreen triangle, no vertex buffer.
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: SkyVertexOutput;
    out.clip_position = vec4<f32>(pos[vi], 1.0, 1.0);
    return out;
}

@fragment
fn fs_sky(in: SkyVertexOutput) -> @location(0) vec4<f32> {
    // Framebuffer coords are pixel centers (x+0.5, y+0.5) — the same sample
    // points the software fill uses when it computes NDC per pixel.
    let ndc_x = 2.0 * in.clip_position.x / sky.size.x - 1.0;
    let ndc_y = 1.0 - 2.0 * in.clip_position.y / sky.size.y;

    // Unproject NDC to a view-space direction (software pixel_view_direction).
    let v = sky.inv_projection * vec4<f32>(ndc_x, ndc_y, -1.0, 1.0);
    var view_dir = v.xyz;
    if (abs(v.w) > 1e-6) {
        view_dir = v.xyz / v.w;
    }
    let world = normalize((sky.camera_basis * vec4<f32>(view_dir, 0.0)).xyz);

    // pat-fw141: physical (Preetham) sky mode (sun_params.z >= 2000) — a
    // verbatim port of Godot's generated PhysicalSkyMaterial shader and of the
    // software `sample_physical_sky`. LIGHT0 = sun_dir_energy[0] /
    // sun_color_size[0]; with no sun Godot shows only the (default black)
    // night-sky texture.
    if (sky.sun_params.z >= 2000.0) {
        if (sky.sun_params.z < 2000.5) {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0); // no sun → night sky
        }
        let d = normalize(world);
        let sun_dir = normalize(sky.sun_dir_energy[0].xyz);
        let zenith_angle = clamp(sun_dir.y, -1.0, 1.0);
        let sun_energy_p = max(0.0, 0.757 * zenith_angle) * sky.sun_dir_energy[0].w;
        let sun_fade = 1.0 - clamp(1.0 - exp(sun_dir.y), 0.0, 1.0);

        let rayleigh_coeff = sky.sky_top.a - 1.0 * (1.0 - sun_fade);
        let rayleigh_beta = rayleigh_coeff * sky.sky_top.rgb * 0.0001;
        // mie_beta = turbidity * mie * mie_color * 0.000434 (Preetham).
        let mie_beta = sky.ground_bottom.a * sky.sky_horizon.a * sky.sky_horizon.rgb * 0.000434;

        let zenith = max(0.0, d.y);
        let optical_mass = 1.0 / (zenith + 0.15 * pow(3.885 + 54.5 * zenith, -1.253));
        let extinction = exp(-(rayleigh_beta * (8.4e3 * optical_mass) + mie_beta * (1.25e3 * optical_mass)));

        let cos_theta = dot(d, sun_dir);
        let rayleigh_phase = (3.0 / (16.0 * 3.14159265)) * (1.0 + pow(cos_theta * 0.5 + 0.5, 2.0));
        let g = sky.params.x;
        let mie_phase = 0.0795774715459 * (1.0 - g * g)
            / pow(max(1.0 + g * g - 2.0 * g * cos_theta, 1e-9), 1.5);

        let beta_theta = rayleigh_beta * rayleigh_phase + mie_beta * mie_phase;
        let beta_sum = max(rayleigh_beta + mie_beta, vec3<f32>(1e-12));
        let base = sun_energy_p * (beta_theta / beta_sum);
        var lin = pow(max(base * (vec3<f32>(1.0) - extinction), vec3<f32>(0.0)), vec3<f32>(1.5));
        let l_low = pow(max(base * extinction, vec3<f32>(0.0)), vec3<f32>(0.5));
        let low_mix = clamp(pow(1.0 - zenith_angle, 5.0), 0.0, 1.0);
        lin = lin * (vec3<f32>(1.0) + (l_low - vec3<f32>(1.0)) * low_mix);
        let ground_blend = smoothstep(-0.1, 0.1, d.y);
        lin = lin * (sky.ground_bottom.rgb + (vec3<f32>(1.0) - sky.ground_bottom.rgb) * ground_blend);

        // Solar disk (guarded like the software smoothstep for size 0).
        let cos_max = cos(sky.sun_color_size[0].w * sky.params.y);
        let cos_max2 = cos(sky.sun_color_size[0].w * sky.params.y * 0.5);
        var sundisk = 0.0;
        if (cos_max2 - cos_max > 1e-9) {
            sundisk = smoothstep(cos_max, cos_max2, cos_theta);
        } else if (cos_theta >= cos_max) {
            sundisk = 1.0;
        }
        let l0 = (sun_energy_p * sundisk) * extinction * sky.sun_color_size[0].rgb;

        let final_pow = 1.0 / (1.2 + 1.2 * sun_fade);
        let col = pow(max(lin + l0, vec3<f32>(0.0)), vec3<f32>(final_pow)) * sky.sun_params.w;
        return vec4<f32>(col, 1.0);
    }

    // pat-coz7r: panoramic / equirect mode (sun_params.z >= 1000).
    // UV matches software equirect_uv AND Godot sky.glsl panorama_coords:
    // u = atan2(x,-z)/(2π) wrapped into [0,1) with NO half-turn shift
    // (a +0.5 here yaw-rotates every panorama by 180°), v = acos(y)/π.
    // Full HDR texture bind is a follow-up; until then, sample a vertical
    // gradient as a stand-in that uses the same UV so paths stay aligned.
    if (sky.sun_params.z >= 1000.0) {
        let d = normalize(world);
        let u = atan2(d.x, -d.z) / (2.0 * 3.14159265);
        let v = acos(clamp(d.y, -1.0, 1.0)) / 3.14159265;
        let uw = fract(u + 1.0); // wrap like rem_euclid
        let gradient = mix(sky.sky_top.rgb, sky.sky_horizon.rgb, clamp(v, 0.0, 1.0));
        // `textureSampleLevel`, not `textureSample`: this sits behind a
        // non-uniform branch, where implicit-derivative sampling is invalid.
        let texel = textureSampleLevel(sky_panorama, sky_panorama_sampler, vec2<f32>(uw, clamp(v, 0.0, 1.0)), 0.0).rgb;
        let rgb = select(gradient, texel, sky.params.w > 1.5);
        return vec4<f32>(rgb * sky.sun_params.w, 1.0);
    }

    // Godot ProceduralSkyMaterial (software sample_procedural_sky).
    let y = clamp(world.y, -1.0, 1.0);
    var rgb: vec3<f32>;
    if (y >= 0.0) {
        // Sky hemisphere: horizon→top gradient, then sun discs/halos.
        let t = clamp(pow(1.0 - y, sky.params.x), 0.0, 1.0);
        rgb = mix(sky.sky_top.rgb, sky.sky_horizon.rgb, t) * sky.params.z;
        let count = u32(sky.sun_params.z);
        for (var i = 0u; i < count; i = i + 1u) {
            let de = sky.sun_dir_energy[i];
            let cs = sky.sun_color_size[i];
            let sun_angle = dot(de.xyz, world);
            let sun_size = cos(cs.w);
            let sun_rgb = cs.rgb * de.w;
            if (sun_angle > sun_size) {
                rgb = sun_rgb;
            } else if (sun_angle > sky.sun_params.x && sun_size > sky.sun_params.x) {
                let c2 = (sun_size - sun_angle) / (sun_size - sky.sun_params.x);
                let f = clamp(pow(1.0 - c2, sky.sun_params.y), 0.0, 1.0);
                rgb = mix(rgb, sun_rgb, f);
            }
        }
    } else {
        // Ground hemisphere: bottom→horizon gradient (no sun below horizon).
        let t = clamp(pow(1.0 + y, sky.params.y), 0.0, 1.0);
        rgb = mix(sky.ground_bottom.rgb, sky.ground_horizon.rgb, t) * sky.params.w;
    }
    // Output linear scene-referred HDR (background energy applied, no clamp):
    // the sun can exceed 1.0 so the camera post-process can bloom/tonemap it.
    return vec4<f32>(rgb * sky.sun_params.w, 1.0);
}
"#;

// ── GPU-side data types ─────────────────────────────────────────────

/// GPU vertex with position, normal, and UV.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuVertex {
    /// Position (x, y, z).
    pub position: [f32; 3],
    /// Normal (x, y, z).
    pub normal: [f32; 3],
    /// Texture coordinates (u, v).
    pub uv: [f32; 2],
}

impl GpuVertex {
    /// Returns the wgpu vertex buffer layout for this type.
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // normal
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // uv
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

/// Camera uniform data (bind group 0).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniformData {
    /// View matrix (column-major 4x4).
    pub view_matrix: [[f32; 4]; 4],
    /// Projection matrix (column-major 4x4).
    pub projection_matrix: [[f32; 4]; 4],
    /// Camera world position.
    pub camera_position: [f32; 3],
    /// Padding.
    pub _pad0: f32,
    /// Environment ambient light color (linear); `rgb` used, `a` unused. Applied
    /// as `ambient * albedo` in the fragment shader, matching the software path.
    pub ambient: [f32; 4],
}

/// Byte stride of one draw's [`ModelUniformData`] in the editor path's shared
/// per-frame uniform buffer (pat-414k3): the struct rounded up to wgpu's
/// default `min_uniform_buffer_offset_alignment`, which every offset a bind
/// group names must be a multiple of.
const MODEL_UNIFORM_STRIDE: usize = 256;
const _: () = assert!(std::mem::size_of::<ModelUniformData>() <= MODEL_UNIFORM_STRIDE);

/// Per-object model + material uniform data (bind group 1).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelUniformData {
    /// Model matrix (column-major 4x4).
    pub model_matrix: [[f32; 4]; 4],
    /// Albedo RGBA.
    pub albedo: [f32; 4],
    /// Emission RGBA.
    pub emission: [f32; 4],
    /// Roughness.
    pub roughness: f32,
    /// Metallic.
    pub metallic: f32,
    /// Shading mode: 0=unlit, 1=lambert, 2=phong.
    pub shading_mode: u32,
    /// Number of active lights.
    pub num_lights: u32,
    /// `VisualInstance3D.layers` — the instance's render-layer BITMASK, tested
    /// against each light's `cull_mask` in the shader.
    pub layers: u32,
    /// Whether this draw has a `normal_texture` bound (pat-mgixk.52).
    ///
    /// ⚠️ A flag rather than a "flat" (0.5, 0.5, 1.0) dummy texel, because 0.5
    /// is not representable in 8 bits (128/255 = 0.50196 decodes to 0.00392).
    /// A dummy would tilt every normal slightly and no untextured scene would
    /// stay bit-identical.
    pub has_normal_map: u32,
    /// `BaseMaterial3D.normal_scale`, the XY perturbation multiplier.
    pub normal_scale: f32,
    /// ⚠️ Explicit padding: the struct's align comes from its `mat4x4`, and
    /// `[[f32; 4]; 4]` has a Rust align of 4, so the size must be spelled back
    /// out to a multiple of 16 or the two declarations disagree.
    ///
    /// The two fields above took the first two of the three slots this used to
    /// cover; `u32` and `f32` are both align 4, so the struct is still 128
    /// bytes and no existing offset moved (`layers` stays at 112, which
    /// `cull_mask_gpu_*` pins).
    pub _model_pad: u32,
    /// Summed UNCAPTURED ReflectionProbe ambient at the instance origin
    /// (linear RGB), added to the camera ambient (pat-p3p1m); zero when a
    /// captured probe covers the origin. Appended after the original 128
    /// bytes so no earlier offset moves.
    pub probe_ambient: [f32; 4],
    /// Unused since pat-h7sz7 (probes are read per fragment from the probe
    /// array); kept so the struct layout is unchanged.
    pub _probe_unused0: [f32; 4],
    /// Unused (see `_probe_unused0`).
    pub _probe_unused1: [f32; 4],
    /// Unused (see `_probe_unused0`).
    pub _probe_unused2: [f32; 4],
    /// Bit `i` set = entry `i` of the frame's probe array may cover this draw
    /// (pat-h7sz7).
    pub probe_mask: u32,
    /// Unused (see `_probe_unused0`).
    pub _probe_unused3: u32,
    /// `1` = image-based light (captured probes and/or sky radiance) is
    /// evaluated for this draw; `0` = the old probe-less path, bit for bit.
    pub probe_env: u32,
    /// Padding to a multiple of 16 bytes.
    pub _probe_pad1: u32,
}

/// Most probes the GPU path blends per fragment (pat-h7sz7). Godot's default
/// `max_cluster_elements` is far higher, but a uniform array has a fixed
/// size; the smallest 16 boxes win, which is Godot's own priority order.
pub const MAX_GPU_PROBES: usize = 16;

/// One probe of [`ProbeArrayData`] (WGSL `ProbeEntry`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuProbeEntry {
    /// Influence box min (xyz), `intensity` (w).
    pub box_min: [f32; 4],
    /// Influence box max (xyz), `blend_distance` (w).
    pub box_max: [f32; 4],
    /// Capture origin (xyz), `box_projection` (w, 1/0).
    pub origin: [f32; 4],
    /// ConstantColor ambient (linear * energy).
    pub ambient: [f32; 4],
    /// x = base layer + 1, y = ambient mode (0 Disabled, 1 Environment,
    /// 2 ConstantColor).
    pub info: [u32; 4],
}

/// The frame's probes + sky radiance, uniform at group 2 binding 4
/// (WGSL `ProbeArray`, pat-h7sz7).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ProbeArrayData {
    /// x = probe count, y = sky base layer + 1 (0 = no sky), z = face
    /// resolution.
    pub header: [u32; 4],
    /// x = sky energy.
    pub sky: [f32; 4],
    /// Probes, smallest box first.
    pub probes: [GpuProbeEntry; MAX_GPU_PROBES],
}

impl Default for ProbeArrayData {
    fn default() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

/// The captured reflection probes and the sky radiance map uploaded for one
/// frame (pat-p3p1m, pat-h7sz7): a `Rgba32Float` 2D array, six consecutive
/// layers per cubemap in `CubeFace` order (each layer an atlas of the GGX
/// roughness chain), the per-probe uniform array, and Godot's DFG table.
struct ProbeCubeArray {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    _dfg_texture: wgpu::Texture,
    dfg_view: wgpu::TextureView,
    /// The probe array uniform (WGSL `probe_array`).
    data: ProbeArrayData,
    /// The cache key (see [`probe_cube_key`]).
    key: Vec<usize>,
}

/// The `Arc` identities of the uploaded captures and sky radiance plus the
/// sky energy: what decides a re-upload.
fn probe_cube_key(
    probes: &[ReflectionProbe],
    sky: Option<&gdserver3d::reflection_probe::SkyRadiance>,
) -> Vec<usize> {
    let mut key: Vec<usize> = probes
        .iter()
        .map(|p| {
            p.captured
                .as_ref()
                .map_or(0, |c| std::sync::Arc::as_ptr(c) as usize)
        })
        .collect();
    key.push(sky.map_or(0, |s| std::sync::Arc::as_ptr(&s.cubemap) as usize));
    key.push(sky.map_or(0, |s| s.energy.to_bits() as usize));
    key
}

/// The per-face roughness atlas for a capture of `res` texels (pat-9gjpr):
/// `(width, height, origin of each level)`. Level 0 sits at `(0, 0)`, level
/// `k >= 1` at `(res, sum of the edges of levels 1..k)`.
pub fn probe_atlas_layout(res: u32) -> (u32, u32, Vec<(u32, u32)>) {
    use gdserver3d::reflection_probe::{roughness_level_resolution, ROUGHNESS_LAYERS};
    let res = res.max(1);
    let mut origins = vec![(0, 0)];
    let mut y = 0;
    for k in 1..ROUGHNESS_LAYERS {
        origins.push((res, y));
        y += roughness_level_resolution(res, k);
    }
    (res + roughness_level_resolution(res, 1), res.max(y), origins)
}

/// The uniform the probe loop reads (pat-h7sz7): up to [`MAX_GPU_PROBES`]
/// captured probes in Godot's processing order (smallest box first), each
/// with the base layer its six faces will have in the cube array, then the
/// sky. Also returns the cubemaps to upload, in layer order. A capture whose
/// resolution differs from the first one's is left out (one atlas layout per
/// array), as is a sky radiance of another resolution.
pub fn build_probe_array_data<'a>(
    probes: &'a [ReflectionProbe],
    sky: Option<&'a gdserver3d::reflection_probe::SkyRadiance>,
) -> (ProbeArrayData, Vec<&'a gdserver3d::reflection_probe::ReflectionCubemap>) {
    use gdserver3d::reflection_probe::probe_processing_order;
    let resolution = probes
        .iter()
        .find_map(|p| p.captured.as_ref().map(|c| c.resolution))
        .or(sky.map(|s| s.cubemap.resolution))
        .unwrap_or(1)
        .max(1);
    let mut data = ProbeArrayData::default();
    let mut cubes = Vec::new();
    let mut count = 0usize;
    for i in probe_processing_order(probes) {
        if count >= MAX_GPU_PROBES {
            break;
        }
        let p = &probes[i];
        let Some(c) = p.captured.as_deref() else {
            continue;
        };
        if c.resolution != resolution {
            continue;
        }
        let (bmin, bmax) = p.world_aabb_min_max();
        let o = p.capture_origin();
        let ac = p.ambient_color.srgb_to_linear();
        let e = p.ambient_color_energy;
        data.probes[count] = GpuProbeEntry {
            box_min: [bmin.x, bmin.y, bmin.z, p.intensity],
            box_max: [bmax.x, bmax.y, bmax.z, p.blend_distance],
            origin: [o.x, o.y, o.z, if p.box_projection { 1.0 } else { 0.0 }],
            ambient: [ac.r * e, ac.g * e, ac.b * e, 0.0],
            info: [cubes.len() as u32 * 6 + 1, p.ambient_mode as u32, 0, 0],
        };
        cubes.push(c);
        count += 1;
    }
    data.header = [count as u32, 0, resolution, 0];
    if let Some(s) = sky.filter(|s| s.cubemap.resolution == resolution) {
        data.header[1] = cubes.len() as u32 * 6 + 1;
        data.sky = [s.energy, 0.0, 0.0, 0.0];
        cubes.push(&s.cubemap);
    }
    (data, cubes)
}

/// Uploads every captured probe cubemap and the sky radiance map
/// ([`build_probe_array_data`]).
fn upload_probe_cube_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    probes: &[ReflectionProbe],
    sky: Option<&gdserver3d::reflection_probe::SkyRadiance>,
) -> ProbeCubeArray {
    let (data, cubes) = build_probe_array_data(probes, sky);
    let resolution = data.header[2];
    let mut bytes: Vec<u8> = Vec::new();
    let mut layers = 0u32;
    // pat-9gjpr: one layer per face holds that face's whole GGX roughness
    // chain as an atlas (`probe_atlas_layout`, mirrored by the WGSL
    // `probe_level_origin`).
    let (atlas_w, atlas_h, origins) = probe_atlas_layout(resolution);
    for c in &cubes {
        let chain = c.roughness_chain();
        for face in 0..6 {
            let mut texels = vec![[0.0f32; 4]; (atlas_w * atlas_h) as usize];
            for (level, &(ox, oy)) in chain.levels.iter().zip(origins.iter()) {
                let lr = level.resolution;
                for y in 0..lr {
                    for x in 0..lr {
                        let c = level.faces[face][(y * lr + x) as usize];
                        texels[((oy + y) * atlas_w + ox + x) as usize] = [c.r, c.g, c.b, c.a];
                    }
                }
            }
            for t in texels {
                for v in t {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        layers += 6;
    }
    let (side_w, side_h) = if layers == 0 {
        bytes = vec![0u8; 16];
        layers = 1;
        (1, 1)
    } else {
        (atlas_w, atlas_h)
    };
    let size = wgpu::Extent3d {
        width: side_w,
        height: side_h,
        depth_or_array_layers: layers,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("probe-cube-array"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side_w * 16),
            rows_per_image: Some(side_h),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("probe-cube-array-view"),
        format: Some(wgpu::TextureFormat::Rgba32Float),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        aspect: wgpu::TextureAspect::All,
        base_mip_level: 0,
        mip_level_count: Some(1),
        base_array_layer: 0,
        array_layer_count: Some(layers),
        usage: None,
    });

    // pat-h7sz7: Godot's baked DFG table, the same numbers `shader::env_brdf`
    // reads on the software path.
    let n = crate::shader::DFG_LUT_SIZE as u32;
    let mut dfg_bytes = Vec::with_capacity((n * n * 16) as usize);
    for ab in crate::shader::dfg_lut() {
        for v in [ab[0], ab[1], 0.0, 1.0] {
            dfg_bytes.extend_from_slice(&f32::to_le_bytes(v));
        }
    }
    let dfg_size = wgpu::Extent3d { width: n, height: n, depth_or_array_layers: 1 };
    let dfg_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dfg-lut"),
        size: dfg_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &dfg_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &dfg_bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(n * 16),
            rows_per_image: Some(n),
        },
        dfg_size,
    );
    let dfg_view = dfg_texture.create_view(&wgpu::TextureViewDescriptor::default());
    ProbeCubeArray {
        _texture: texture,
        view,
        _dfg_texture: dfg_texture,
        dfg_view,
        data,
        key: probe_cube_key(probes, sky),
    }
}

/// Fills a draw's probe fields (pat-h7sz7): the uncaptured-probe ambient at
/// `origin` (the software `entry_ambient` rule: only when no CAPTURED probe
/// covers the origin), the mask of probe-array entries whose box may touch
/// the draw, and whether image-based light runs at all.
///
/// With `bounds` (the draw's world-space bounding sphere), the mask keeps
/// only the probes whose box the sphere touches, so the fragment loop skips
/// the rest; without it every probe stays in (the loop tests each box
/// itself, so the mask only ever saves work, never changes a pixel).
fn apply_probe_fields(
    model: &mut ModelUniformData,
    probes: &[ReflectionProbe],
    cubes: &ProbeCubeArray,
    origin: Vector3,
    bounds: Option<(Vector3, f32)>,
) {
    let count = cubes.data.header[0] as usize;
    if count > 0 || cubes.data.header[1] != 0 {
        model.probe_env = 1;
        let mut mask = 0u32;
        for (i, p) in cubes.data.probes.iter().take(count.min(32)).enumerate() {
            let touches = match bounds {
                None => true,
                Some((c, r)) => {
                    let d2: f32 = (0..3)
                        .map(|a| {
                            let v = [c.x, c.y, c.z][a];
                            let q = v.clamp(p.box_min[a], p.box_max[a]);
                            (v - q) * (v - q)
                        })
                        .sum();
                    !(d2 > r * r)
                }
            };
            if touches {
                mask |= 1 << i;
            }
        }
        model.probe_mask = mask;
    }
    if probes.is_empty() {
        return;
    }
    if gdserver3d::reflection_probe::probe_index_at(probes, origin).is_none() {
        let amb = sample_probes_at(probes, origin);
        model.probe_ambient = [amb.r, amb.g, amb.b, 0.0];
    }
}

/// Single light data for the GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuLightData {
    /// 0=directional, 1=point, 2=spot.
    pub kind: u32,
    /// Whether shadows are enabled.
    pub shadow_enabled: u32,
    /// Light range.
    pub range: f32,
    /// Distance attenuation exponent.
    pub attenuation: f32,
    /// Light direction (normalized).
    pub direction: [f32; 3],
    /// Spot angle in radians.
    pub spot_angle: f32,
    /// Light color (RGB).
    pub color: [f32; 3],
    /// Spot angle attenuation.
    pub spot_angle_attenuation: f32,
    /// Light position.
    pub position: [f32; 3],
    /// Shadow map slot: `-1` = none, `0` = primary directional map texture.
    pub shadow_map_index: i32,
    /// Godot `Light3D.shadow_opacity`: how dark this light's shadow is.
    ///
    /// `1.0` is fully opaque (Godot's default); `0.0` means the light casts no
    /// visible shadow. Scales the OCCLUSION, matching the software
    /// `LightUniform::shadow_opacity`.
    pub shadow_opacity: f32,
    /// ⚠️ Explicit padding so `shadow_view_proj` starts at offset 80 in BOTH
    /// languages — see the comment on the WGSL `LightData` struct. Without it
    /// Rust would place the matrix at 68 (its align is 4) while WGSL placed it
    /// at 80 (mat4x4 align 16), and every light after the first would be read
    /// from the wrong bytes — a silent corruption, not a compile error.
    /// `light_cull_mask`: which render layers this light touches.
    ///
    /// Occupies the FIRST of the three slots reserved when `shadow_opacity` was
    /// added, so the layout change was paid once — size and every existing
    /// offset are unchanged.
    pub cull_mask: u32,
    /// `Light3D.shadow_bias`, in WORLD UNITS — the shader divides by the shadow
    /// frustum's depth range for the 2D compare and uses it raw for the omni one.
    pub shadow_bias: f32,
    /// `Light3D.shadow_normal_bias`, in SHADOW-MAP TEXELS — the last reserved
    /// slot. The struct is now full; the next field needs a new 16-byte row.
    pub shadow_normal_bias: f32,
    /// Light-space clip matrix (`proj * view`), column-major.
    pub shadow_view_proj: [[f32; 4]; 4],
    /// `Light3D.light_specular`, as authored — the x2 for positional lights is
    /// applied in the shader, matching the software `LightUniform`.
    ///
    /// ⚠️ First field in a NEW 16-byte row (144 -> 160 bytes). It sits AFTER
    /// the matrix so every existing offset is unchanged; putting it before
    /// would push `shadow_view_proj` off its 16-byte boundary and reinterpret
    /// every field the GPU already reads.
    pub specular: f32,
    /// ⚠️ EXPLICIT PADDING to 160 bytes, and it must be three separate `f32`s.
    /// A `[f32; 3]` in Rust sits at offset 148, but its WGSL counterpart
    /// `vec3<f32>` has an align of 16 and would land at 160, making the WGSL
    /// struct 176 while Rust stayed at 160 — the two languages would disagree
    /// about every light after the first. Same trap as the pad before
    /// `shadow_view_proj`.
    /// `DirectionalLight3D.directional_shadow_max_distance` — takes the first
    /// pad reserved beside `specular`, so the struct stays 160 bytes.
    pub directional_shadow_max_distance: f32,
    /// `DirectionalLight3D.directional_shadow_fade_start`, as authored. The
    /// 0.999 clamp is applied in the shader, matching the software path and
    /// Godot itself — the authored value must round-trip.
    pub directional_shadow_fade_start: f32,
    /// This light's shadow map size over the atlas layer size (pat-772py):
    /// maps of different sizes share one array texture, each in the top-left
    /// of its layer. Was the last of `specular`'s three pads.
    pub shadow_uv_scale: f32,
    /// pat-772py: a NEW 16-byte row (160 -> 176 bytes). The constant bias in
    /// this map's normalized depth (Godot's formula, see
    /// `shadow_map::ShadowFilter`).
    pub shadow_depth_bias: f32,
    /// World size of one texel of this light's map.
    pub shadow_texel_world: f32,
    /// PCF radius in texels of this light's map; NEGATIVE selects the legacy
    /// single nearest compare.
    pub shadow_pcf_radius: f32,
    /// Vogel-disk taps (0 = one bilinear tap).
    pub shadow_pcf_taps: u32,
}

/// Light array uniform (bind group 2). Fixed-size array of 16 lights.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightArrayData {
    /// Up to 16 lights.
    pub lights: [GpuLightData; gdserver3d::light::MAX_SCENE_LIGHTS],
}

/// Sky-pass uniform (group 0 of the sky pipeline). Mirrors `SkyUniforms` in
/// [`SKY_WGSL`]: the pixel-ray reconstruction matrices plus the procedural
/// gradient parameters, all taken verbatim from the environment so the GPU
/// background matches the software fill exactly.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkyUniformData {
    /// Inverse of the viewport's perspective projection (column-major).
    pub inv_projection: [[f32; 4]; 4],
    /// Camera world basis in the upper-left 3x3 (column-major).
    pub camera_basis: [[f32; 4]; 4],
    /// `ProceduralSkyMaterial::sky_top_color`.
    pub sky_top: [f32; 4],
    /// `ProceduralSkyMaterial::sky_horizon_color`.
    pub sky_horizon: [f32; 4],
    /// `ProceduralSkyMaterial::ground_bottom_color`.
    pub ground_bottom: [f32; 4],
    /// `ProceduralSkyMaterial::ground_horizon_color`.
    pub ground_horizon: [f32; 4],
    /// x = `0.6 / sky_curve`, y = `0.6 / ground_curve` (Godot's
    /// `inv_sky_curve`/`inv_ground_curve` uniform mapping), z/w = sky/ground
    /// material energy multipliers.
    pub params: [f32; 4],
    /// x = viewport width, y = viewport height (z, w unused).
    pub size: [f32; 4],
    /// Up to 4 sky suns: xyz = world direction toward the light, w = energy.
    pub sun_dir_energy: [[f32; 4]; 4],
    /// Up to 4 sky suns: rgb = color, w = angular size in radians.
    pub sun_color_size: [[f32; 4]; 4],
    /// x = `cos(sun_angle_max)`, y = `1.6 / pow(sun_curve, 1.4)` (Godot's
    /// `inv_sun_curve`), z = enabled sun count, w = environment background
    /// energy multiplier.
    pub sun_params: [f32; 4],
}

// ── GPU context ─────────────────────────────────────────────────────

/// Core wgpu handles for 3D rendering.
pub struct GpuContext3D {
    /// The wgpu instance.
    pub instance: wgpu::Instance,
    /// The selected adapter.
    pub adapter: wgpu::Adapter,
    /// The logical device.
    pub device: wgpu::Device,
    /// The command queue.
    pub queue: wgpu::Queue,
}

impl std::fmt::Debug for GpuContext3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let info = self.adapter.get_info();
        f.debug_struct("GpuContext3D")
            .field("adapter", &info.name)
            .field("backend", &info.backend)
            .finish()
    }
}

impl GpuContext3D {
    /// Creates a new headless GPU context.
    ///
    /// Returns `None` if no adapter is available (headless CI without GPU).
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("patina-3d-gpu"),
            required_features: wgpu::Features::empty(),
            required_limits:
                wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .ok()?;

        Some(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Returns the adapter name string.
    pub fn adapter_name(&self) -> String {
        self.adapter.get_info().name
    }
}

// ── Render pipeline ─────────────────────────────────────────────────

/// A compiled 3D render pipeline with bind group layouts.
/// Pipeline state of one editor/Play draw (pat-9gjpr) — the cache key of
/// `GpuRenderer3D::editor_pipe_cache`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPipelineKey {
    /// Face culling.
    pub cull: crate::render_modes::CullMode,
    /// Whether the draw writes depth.
    pub depth_write: bool,
    /// Whether the draw depth-TESTS (`false` = Godot `no_depth_test`).
    pub depth_test: bool,
    /// `BLEND_MODE_ADD` instead of mix.
    pub additive: bool,
    /// The custom module's identity (its `Arc` address), `0` for the base
    /// module.
    pub module: usize,
}

// By hand: `CullMode` lives in gdserver3d and derives no `Hash`.
impl std::hash::Hash for EditorPipelineKey {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (self.cull as u8, self.depth_write, self.depth_test, self.additive, self.module).hash(h);
    }
}

pub struct RenderPipeline3D {
    /// The wgpu render pipeline.
    pub pipeline: wgpu::RenderPipeline,
    /// Bind group layout for camera uniforms (group 0).
    pub camera_layout: wgpu::BindGroupLayout,
    /// Bind group layout for model uniforms (group 1).
    pub model_layout: wgpu::BindGroupLayout,
    /// Bind group layout for a custom shader's uniform block (group 3).
    ///
    /// Only the per-material pipelines from [`Self::build_variant`] declare it;
    /// the shared pipeline stops at group 2. A draw using a custom pipeline MUST
    /// bind it — the block always carries at least `time`, so the buffer is
    /// never zero-sized (pat-s3rrv.122).
    pub custom_layout: wgpu::BindGroupLayout,
    /// Bind group layout for lights (group 2).
    pub light_layout: wgpu::BindGroupLayout,
}

impl RenderPipeline3D {
    /// Creates the standard 3D render pipeline from the built-in WGSL shader.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("patina-3d-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_3D_WGSL.into()),
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Binding 0 is the uniform block; then CUSTOM_SAMPLER_SLOTS fixed
        // texture+sampler pairs (pat-s3rrv.122). The count is fixed so ONE
        // layout serves every custom pipeline — wgpu permits layout entries a
        // shader never references, but a bind GROUP must fill them all, which is
        // why unused slots get a dummy 1x1 texture rather than being omitted.
        let mut custom_entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            // Fragment only: the custom body transpiles into `custom_albedo`,
            // which `fs_main` calls. The vertex stage never reads it.
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        for slot in 0..CUSTOM_SAMPLER_SLOTS {
            custom_entries.push(wgpu::BindGroupLayoutEntry {
                binding: custom_texture_binding(slot),
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
            custom_entries.push(wgpu::BindGroupLayoutEntry {
                binding: custom_sampler_binding(slot),
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let custom_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("custom-shader-bind-group-layout"),
            entries: &custom_entries,
        });

        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // `BaseMaterial3D.albedo_texture` + its sampler (pat-mgixk.52).
                //
                // ⚠️ `filterable: true` and `SamplerBindingType::Filtering`
                // are a PAIR — a filtering sampler against a non-filterable
                // texture is a layout validation error. Filtering is what
                // matches the software path's `sample_bilinear`; the shadow
                // arrays above use `filterable: false` because they are
                // compared, not blended.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // `BaseMaterial3D` texture slots (pat-mgixk.52): 3 normal,
                // 4 roughness, 5 metallic, 6 emission. All share the sampler at
                // binding 2 — same filter and wrap, so extra sampler slots
                // would buy nothing.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // pat-32edz / pat-mst61: shadow depth array (R32Float layers).
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                // pat-g4i60: omni (point-light) shadow cubemap faces — 6
                // layers per shadow-casting point light, in its own array
                // (128² faces vs the 256² directional/spot maps).
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                // pat-p3p1m: captured ReflectionProbe cubemaps (Rgba32Float,
                // 6 layers per probe).
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                // pat-h7sz7: the frame's probe array + sky radiance header.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // pat-h7sz7: Godot's DFG table (Rgba32Float, textureLoad).
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("patina-3d-pipeline-layout"),
            bind_group_layouts: &[
                Some(&camera_layout),
                Some(&model_layout),
                Some(&light_layout),
            ],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("patina-3d-render-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[GpuVertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                // Godot's front face is CLOCKWISE (`doc/classes/ArrayMesh.xml:49`);
                // the software cull agrees (`render_modes::should_cull`).
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            camera_layout,
            custom_layout,
            model_layout,
            light_layout,
        }
    }

    /// Builds a variant of the standard pipeline with a different face-cull
    /// mode (pat-s3rrv.30).
    ///
    /// On the GPU, cull mode is *pipeline state*, not a per-draw uniform, so a
    /// material asking for `CULL_FRONT` or `CULL_DISABLED` cannot be honoured by
    /// setting a flag — it needs its own pipeline. `RenderPipeline3D::new`
    /// hardcodes `Face::Back`, which is why an authored `cull_mode` changed
    /// nothing in the wgpu backend no matter what the scene said.
    ///
    /// Mirrors `new`'s descriptor exactly apart from `cull_mode`, and reuses
    /// this pipeline's bind group layouts so the existing camera / model / light
    /// bind groups stay valid across a pipeline switch.
    pub fn build_cull_variant(
        &self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        cull: Option<wgpu::Face>,
        depth_write: bool,
        blend: wgpu::BlendState,
    ) -> wgpu::RenderPipeline {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("patina-3d-shader-cull-variant"),
            source: wgpu::ShaderSource::Wgsl(SHADER_3D_WGSL.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("patina-3d-pipeline-layout-cull-variant"),
            bind_group_layouts: &[
                Some(&self.camera_layout),
                Some(&self.model_layout),
                Some(&self.light_layout),
            ],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("patina-3d-render-pipeline-cull-variant"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[GpuVertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: cull,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                // pat-s3rrv.30: alpha-blended draws must NOT write depth
                // (Godot's DEPTH_DRAW_OPAQUE_ONLY). Writing it lets whichever
                // transparent surface happens to draw first depth-reject the
                // one behind it, so "see-through" would depend on draw order.
                depth_write_enabled: Some(depth_write),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Builds the editor/Play pipeline for `key` (pat-9gjpr): every pipeline-
    /// state axis a draw can ask for — cull, depth write, depth TEST, blend —
    /// over either the base module or a transpiled custom spatial module.
    ///
    /// `custom_wgsl` adds group 3 (`custom_layout`); the camera/model/light
    /// layouts are shared, so the frame's bind groups stay valid across a
    /// pipeline switch.
    pub fn build_editor_pipeline(
        &self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        key: EditorPipelineKey,
        custom_wgsl: Option<&str>,
    ) -> wgpu::RenderPipeline {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("patina-3d-editor-keyed"),
            source: wgpu::ShaderSource::Wgsl(custom_wgsl.unwrap_or(SHADER_3D_WGSL).into()),
        });
        let mut layouts = vec![
            Some(&self.camera_layout),
            Some(&self.model_layout),
            Some(&self.light_layout),
        ];
        if custom_wgsl.is_some() {
            layouts.push(Some(&self.custom_layout));
        }
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("patina-3d-editor-keyed-layout"),
            bind_group_layouts: &layouts,
            immediate_size: 0,
        });
        let blend = if key.additive {
            ADDITIVE_BLEND
        } else {
            wgpu::BlendState::ALPHA_BLENDING
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("patina-3d-editor-keyed-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[GpuVertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Self::wgpu_cull_face(key.cull),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(key.depth_write),
                // Godot `no_depth_test` / `depth_test_disabled`: the
                // comparison always passes (the write is decided separately,
                // as in the software rasterizer).
                depth_compare: Some(if key.depth_test {
                    wgpu::CompareFunction::Less
                } else {
                    wgpu::CompareFunction::Always
                }),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Maps a material's cull mode to wgpu's, so both backends read the same
    /// enum (pat-s3rrv.30).
    pub fn wgpu_cull_face(mode: crate::render_modes::CullMode) -> Option<wgpu::Face> {
        match mode {
            crate::render_modes::CullMode::Back => Some(wgpu::Face::Back),
            crate::render_modes::CullMode::Front => Some(wgpu::Face::Front),
            crate::render_modes::CullMode::Disabled => None,
        }
    }

    /// Builds a per-material pipeline variant that reuses this pipeline's bind
    /// group layouts (camera / model / light — groups 0–2) but swaps in a custom
    /// fragment module transpiled from a spatial shader's `fragment()` body (see
    /// [`custom_spatial_module_wgsl`]). Only used for shaders that bind no extra
    /// uniforms/textures, so the layout is identical and the existing camera /
    /// model / light bind groups stay valid across a pipeline switch.
    ///
    /// `module_wgsl` must already be validated WGSL (the caller validates via
    /// naga); `target_format` must match the render pass's colour attachment.
    /// This mirrors [`RenderPipeline3D::new`]'s pipeline descriptor exactly,
    /// changing only the shader module.
    pub fn build_variant(
        &self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        module_wgsl: &str,
    ) -> wgpu::RenderPipeline {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("patina-3d-custom-shader"),
            source: wgpu::ShaderSource::Wgsl(module_wgsl.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("patina-3d-custom-pipeline-layout"),
            bind_group_layouts: &[
                Some(&self.camera_layout),
                Some(&self.model_layout),
                Some(&self.light_layout),
                // Group 3 exists ONLY here. Every draw that selects a custom
                // pipeline must bind it or the draw fails validation
                // (pat-s3rrv.122).
                Some(&self.custom_layout),
            ],
            immediate_size: 0,
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("patina-3d-custom-render-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[GpuVertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }
}

/// Builds a complete, naga-validated WGSL module for a spatial shader material
/// whose `fragment()` body is transpilable to the base pipeline (UV math,
/// arithmetic, `mix`, vector constructors — no custom uniforms, textures, or
/// `TIME`). The module is [`SHADER_3D_WGSL`] with its fragment `albedo` source
/// swapped from the flat `model.albedo` uniform to a per-fragment `custom_albedo`
/// function transpiled from the shader body, so an `ALBEDO = vec3(UV.x, UV.y,
/// 1.0)` shader renders a real UV gradient on the GPU instead of a flat colour.
///
/// Returns `None` when the body is empty, isn't UV/varying-driven, isn't
/// transpilable, the base shader's shape changed, or (defensively) the produced
/// module fails WGSL validation — in every such case the caller keeps the flat
/// albedo path.
/// Uploads a [`TextureImage`] as an RGBA8 colour texture (pat-s3rrv.122).
///
/// `TextureImage` stores linear `Color` floats; the GPU wants 8-bit texels, so
/// each channel is clamped and scaled here. `Rgba8Unorm` (not `…Srgb`) because
/// the whole 3D path shades in linear and the camera post-process does the sRGB
/// encode at the end — letting the sampler decode as well would double-correct.
fn upload_custom_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    img: &gdserver3d::material::TextureImage,
    source_color: bool,
) -> wgpu::Texture {
    let (w, h) = (img.width.max(1), img.height.max(1));
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let c = img
                .pixels
                .get(i)
                .copied()
                .unwrap_or(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0));
            for ch in [c.r, c.g, c.b, c.a] {
                bytes.push((ch.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("custom-shader-texture"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // pat-3u1wu: a `source_color` sampler is sRGB-DECODED on sampling in
        // Godot (`texture_srgb`), so its texels must reach `texture()` linear;
        // every other sampler returns the texel as stored.
        format: custom_texture_format(source_color),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    texture
}

/// The texture format a custom shader's `sampler2D` is uploaded in
/// (pat-3u1wu): sRGB for a `source_color` sampler, so the hardware decodes it
/// the way Godot binds such a texture, raw otherwise.
pub fn custom_texture_format(source_color: bool) -> wgpu::TextureFormat {
    if source_color {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    }
}

/// Uploads a [`TextureImage`] as an RGBA8 texture for `BaseMaterial3D`
/// sampling, in the caller's choice of `format` (pat-mgixk.52).
///
/// [`TextureImage`]: gdserver3d::material::TextureImage
///
/// ⚠️ **The format is the caller's decision because the right answer differs
/// per map**, and getting it wrong is silent — the image still appears, just
/// transfer-shifted:
///
/// * **Colour maps** (albedo, emission) use `Rgba8UnormSrgb` so the sampler
///   applies the same sRGB->linear transfer the software path applies per texel
///   (`shader.rs:218`). `Color::srgb_to_linear` is the standard piecewise sRGB
///   EOTF (`gdcore::math::srgb_to_linear_channel`), which is precisely what the
///   hardware format implements, and both leave alpha untouched.
/// * **Normal maps** use `Rgba8Unorm`: the software path samples the normal
///   texel RAW and hands it straight to `apply_normal_map` (`shader.rs:252`)
///   with no decode. Decoding would bend every perturbed normal toward the
///   surface.
///
/// Still deliberately separate from [`upload_custom_texture`] rather than
/// folding that in too: custom samplers must stay `Rgba8Unorm` so an authored
/// `texture()` returns the texel undecoded, and refactoring a function owned by
/// another bead for no behavioural gain is not this bead's business.
fn upload_material_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    img: &gdserver3d::material::TextureImage,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    let (w, h) = (img.width.max(1), img.height.max(1));
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let c = img
                .pixels
                .get(i)
                .copied()
                .unwrap_or(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0));
            for ch in [c.r, c.g, c.b, c.a] {
                bytes.push((ch.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    // pat-3u1wu: a full mip chain, as Godot's importer generates for a texture
    // used in 3D (`mipmaps/generate=true`). The `*_WITH_MIPMAPS` filters read
    // it; the plain NEAREST/LINEAR samplers clamp to level 0, so a material
    // that asks for no mipmaps still sees exactly the base image.
    let levels = material_mip_chain(w, h, bytes);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("material-texture"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (lw, lh, data)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(lw * 4),
                rows_per_image: Some(*lh),
            },
            wgpu::Extent3d {
                width: *lw,
                height: *lh,
                depth_or_array_layers: 1,
            },
        );
    }
    texture
}

/// The mip chain of a tightly packed RGBA8 image, level 0 first, down to 1x1
/// (pat-3u1wu). Each level is a 2x2 box filter of the one above, the filter
/// Godot's `Image::generate_mipmaps` uses for 8-bit formats; an odd edge
/// repeats its last texel.
pub fn material_mip_chain(w: u32, h: u32, base: Vec<u8>) -> Vec<(u32, u32, Vec<u8>)> {
    let mut levels = vec![(w, h, base)];
    loop {
        let (pw, ph, prev) = levels.last().expect("level 0 pushed");
        let (pw, ph) = (*pw, *ph);
        if pw == 1 && ph == 1 {
            break;
        }
        let (nw, nh) = ((pw / 2).max(1), (ph / 2).max(1));
        let mut next = Vec::with_capacity((nw * nh * 4) as usize);
        for y in 0..nh {
            for x in 0..nw {
                let xs = [(2 * x).min(pw - 1), (2 * x + 1).min(pw - 1)];
                let ys = [(2 * y).min(ph - 1), (2 * y + 1).min(ph - 1)];
                for ch in 0..4 {
                    let mut sum = 0u32;
                    for &sy in &ys {
                        for &sx in &xs {
                            sum += prev[((sy * pw + sx) * 4 + ch) as usize] as u32;
                        }
                    }
                    next.push(((sum + 2) / 4) as u8);
                }
            }
        }
        levels.push((nw, nh, next));
    }
    levels
}

/// Texture format for `BaseMaterial3D` **colour** maps (albedo, and emission
/// when it lands): sRGB, so the sampler performs the transfer the software path
/// does per texel.
///
/// 🔑 Named rather than spelled at each call site for the same reason
/// [`custom_texture_binding`] is a function: one place decides, so a colour map
/// and a normal map cannot silently drift onto the wrong transfer.
const MATERIAL_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Texture format for `BaseMaterial3D` maps the software path samples **raw**,
/// with no transfer applied: the normal map, and the roughness/metallic maps
/// whose green/blue channels are read as plain scalars. See
/// [`upload_material_texture`].
const MATERIAL_RAW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The 1x1 white texel bound when a material has no map for a slot.
///
/// 🔑 For **albedo** this is what keeps the change a no-op for untextured
/// scenes: the WGSL multiplies unconditionally, and white is the multiplicative
/// identity in both sRGB and linear, so an untextured draw comes out
/// bit-identical to before. A bind group must fill every entry its layout
/// declares, so there is no option to leave a binding empty.
///
/// For the **normal** slot the value is irrelevant — `has_normal_map` gates
/// whether the sampled texel is used at all, because no 8-bit texel decodes to
/// exactly "no tilt" (see [`ModelUniformData::has_normal_map`]). White is bound
/// simply because it is the texture already to hand.
fn white_material_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    upload_material_texture(
        device,
        queue,
        &gdserver3d::material::TextureImage::new(
            1,
            1,
            vec![gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)],
        ),
        format,
    )
}

/// Every `BaseMaterial3D` texture slot's view for one draw, with the uploaded
/// textures kept alongside them.
struct MaterialSlotViews {
    albedo: wgpu::TextureView,
    normal: wgpu::TextureView,
    roughness: wgpu::TextureView,
    metallic: wgpu::TextureView,
    emission: wgpu::TextureView,
    /// The uploads themselves. Held rather than dropped so the textures outlive
    /// the bind group that samples them, the way the custom-sampler path holds
    /// its own uploads.
    _uploaded: [wgpu::Texture; 5],
}

/// The five material slots of one editor draw, borrowed from the renderer's
/// texture caches (pat-p3p1m).
struct CachedSlotViews<'a> {
    albedo: &'a wgpu::TextureView,
    normal: &'a wgpu::TextureView,
    roughness: &'a wgpu::TextureView,
    metallic: &'a wgpu::TextureView,
    emission: &'a wgpu::TextureView,
}

/// Builds all five material texture slots for one draw (pat-mgixk.52).
///
/// 🔑 ONE place assembles the slots for BOTH render paths. A bind group must
/// fill **every** entry its layout declares, and a slot missed at a call site
/// is a *runtime validation failure that aborts the draw*, not a compile error.
/// With the slots built here, adding the next map is one edit instead of two
/// that have to agree.
///
/// The per-slot format is not uniform and the difference is silent — see
/// [`upload_material_texture`]. Colour maps (albedo, emission) carry the sRGB
/// transfer; the normal map and the roughness/metallic scalars are raw.
fn material_slot_views(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    albedo: Option<&gdserver3d::material::TextureImage>,
    normal: Option<&gdserver3d::material::TextureImage>,
    roughness: Option<&gdserver3d::material::TextureImage>,
    metallic: Option<&gdserver3d::material::TextureImage>,
    emission: Option<&gdserver3d::material::TextureImage>,
) -> MaterialSlotViews {
    let slot =
        |img: Option<&gdserver3d::material::TextureImage>, format: wgpu::TextureFormat| match img {
            Some(i) => upload_material_texture(device, queue, i, format),
            None => white_material_texture(device, queue, format),
        };
    let uploaded = [
        slot(albedo, MATERIAL_COLOR_FORMAT),
        slot(normal, MATERIAL_RAW_FORMAT),
        slot(roughness, MATERIAL_RAW_FORMAT),
        slot(metallic, MATERIAL_RAW_FORMAT),
        slot(emission, MATERIAL_COLOR_FORMAT),
    ];
    let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
    MaterialSlotViews {
        albedo: view(&uploaded[0]),
        normal: view(&uploaded[1]),
        roughness: view(&uploaded[2]),
        metallic: view(&uploaded[3]),
        emission: view(&uploaded[4]),
        _uploaded: uploaded,
    }
}

/// The sampler for `BaseMaterial3D` maps (pat-mgixk.52).
///
/// ⚠️ `AddressMode::Repeat`, NOT the `..Default::default()` the custom-shader
/// sampler uses — that default is `ClampToEdge`. Godot's `BaseMaterial3D`
/// default is repeat, and `TextureImage::sample_bilinear` wraps
/// (`wrap_index`), so clamping here would disagree with the software path for
/// any UV outside 0..1.
fn material_sampler(device: &wgpu::Device, key: MaterialSamplerKey) -> wgpu::Sampler {
    let desc = material_sampler_descriptor(key);
    device.create_sampler(&desc)
}

/// Which `BaseMaterial3D` sampler a draw needs (pat-3u1wu): the filter and
/// repeat flags of its material, the only two inputs Godot's sampler choice
/// for a material map depends on (`material_storage.cpp`,
/// `sampler_rd_configure_custom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MaterialSamplerKey {
    /// The material's `texture_filter`.
    pub filter: gdserver3d::material::TextureFilter,
    /// The material's `texture_repeat`; `false` clamps to the edge.
    pub clamp: bool,
}

impl MaterialSamplerKey {
    /// The key for `material`'s maps.
    pub fn of(material: &Material3D) -> Self {
        Self {
            filter: material.texture_filter,
            clamp: !material.texture_repeat,
        }
    }

    /// A dense index for the renderer's sampler cache.
    fn index(self) -> usize {
        let f = match self.filter {
            gdserver3d::material::TextureFilter::Nearest => 0,
            gdserver3d::material::TextureFilter::Linear => 1,
            gdserver3d::material::TextureFilter::NearestWithMipmaps => 2,
            gdserver3d::material::TextureFilter::LinearWithMipmaps => 3,
            gdserver3d::material::TextureFilter::NearestWithMipmapsAnisotropic => 4,
            gdserver3d::material::TextureFilter::LinearWithMipmapsAnisotropic => 5,
        };
        f * 2 + usize::from(self.clamp)
    }
}

/// Number of distinct [`MaterialSamplerKey`]s.
const MATERIAL_SAMPLER_KINDS: usize = 12;

/// Every [`MaterialSamplerKey`], in [`MaterialSamplerKey::index`] order.
fn material_sampler_keys() -> impl Iterator<Item = MaterialSamplerKey> {
    use gdserver3d::material::TextureFilter as F;
    let keys: Vec<MaterialSamplerKey> = [
        F::Nearest,
        F::Linear,
        F::NearestWithMipmaps,
        F::LinearWithMipmaps,
        F::NearestWithMipmapsAnisotropic,
        F::LinearWithMipmapsAnisotropic,
    ]
    .into_iter()
    .flat_map(|filter| {
        [false, true]
            .into_iter()
            .map(move |clamp| MaterialSamplerKey { filter, clamp })
    })
    .collect();
    debug_assert_eq!(keys.len(), MATERIAL_SAMPLER_KINDS);
    debug_assert!(keys.iter().enumerate().all(|(i, k)| k.index() == i));
    keys.into_iter()
}

/// The sampler descriptor Godot's renderer configures for `key`
/// (pat-3u1wu), transcribed from `material_storage.cpp:2460-2510`:
///
/// * the NEAREST family is nearest for BOTH mag and min — not just
///   magnification — with a LINEAR mip filter (`use_nearest_mipmap_filter`
///   defaults to false);
/// * the non-mipmap modes read the base level only (`lod_max_clamp = 0`);
/// * `texture_repeat = false` is CLAMP, not mirror.
///
/// Anisotropy is not configured: wgpu only allows it when all three filters
/// are linear, and it changes pixels only at grazing angles.
pub fn material_sampler_descriptor(key: MaterialSamplerKey) -> wgpu::SamplerDescriptor<'static> {
    let filter = if key.filter.is_nearest() {
        wgpu::FilterMode::Nearest
    } else {
        wgpu::FilterMode::Linear
    };
    let address = if key.clamp {
        wgpu::AddressMode::ClampToEdge
    } else {
        wgpu::AddressMode::Repeat
    };
    let mips = key.filter.uses_mipmaps();
    wgpu::SamplerDescriptor {
        label: Some("material-texture-sampler"),
        address_mode_u: address,
        address_mode_v: address,
        address_mode_w: address,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: if mips {
            wgpu::MipmapFilterMode::Linear
        } else {
            wgpu::MipmapFilterMode::Nearest
        },
        lod_min_clamp: 0.0,
        lod_max_clamp: if mips { 32.0 } else { 0.0 },
        ..Default::default()
    }
}

/// How many `sampler2D` uniforms a custom spatial shader can bind
/// (pat-s3rrv.122).
///
/// Fixed rather than per-shader so a single `custom_layout` is valid for every
/// custom pipeline. A shader declaring fewer simply leaves the tail unreferenced,
/// which wgpu allows; the bind group still fills them with a dummy texture.
pub const CUSTOM_SAMPLER_SLOTS: u32 = 4;

/// Group-3 binding index of sampler slot `slot`'s texture.
///
/// 🔑 ONE function decides these on both sides — the emitted WGSL and the bind
/// group entries — so a texture and its sampler cannot drift onto mismatched
/// bindings, which would sample the wrong image rather than fail.
pub fn custom_texture_binding(slot: u32) -> u32 {
    1 + slot * 2
}

/// Group-3 binding index of sampler slot `slot`'s sampler.
pub fn custom_sampler_binding(slot: u32) -> u32 {
    2 + slot * 2
}

/// One shader-parameter value as the `vec4<f32>` slot the block reserves for it
/// (pat-s3rrv.122).
///
/// Every field is a `vec4` regardless of the declared type, which keeps the
/// struct free of WGSL alignment padding — a `vec3` field would silently pad to
/// 16 bytes anyway and a mixed-size struct would make the packed buffer and the
/// declaration disagree. Narrower types occupy the leading components and the
/// transpiler reads `.x` for a scalar.
fn uniform_variant_to_vec4(v: &gdvariant::Variant) -> [f32; 4] {
    use gdvariant::Variant;
    match v {
        Variant::Color(c) => [c.r, c.g, c.b, c.a],
        Variant::Vector3(p) => [p.x, p.y, p.z, 0.0],
        Variant::Vector2(p) => [p.x, p.y, 0.0, 0.0],
        Variant::Float(f) => [*f as f32, 0.0, 0.0, 0.0],
        Variant::Int(i) => [*i as f32, 0.0, 0.0, 0.0],
        Variant::Bool(b) => [if *b { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        // An unset or unsupported parameter contributes zero rather than
        // failing the draw — matching how the software path treats a missing
        // value.
        _ => [0.0; 4],
    }
}

/// The `sampler2D` uniform names of `compiled`, in declaration order, capped at
/// [`CUSTOM_SAMPLER_SLOTS`] (pat-s3rrv.122).
///
/// 🔑 THE SAMPLER ORDERING AUTHORITY, the exact counterpart of
/// [`custom_uniform_fields`]. The emitted WGSL declarations and the per-draw
/// bind group entries both walk this, so slot N means the same sampler on both
/// sides. A mismatch would sample the WRONG texture rather than fail.
pub fn custom_sampler_fields(compiled: &gdserver3d::CompiledShader3D) -> Vec<String> {
    compiled
        .uniforms
        .iter()
        .filter(|u| u.uniform_type == gdserver3d::shader::UniformType::Sampler2D)
        .take(CUSTOM_SAMPLER_SLOTS as usize)
        .map(|u| gdserver3d::shader::wgsl_uniform_field(&u.name))
        .collect()
}

/// The buffer-backed uniform fields of `compiled`, in declaration order.
///
/// 🔑 THE ONE ORDERING AUTHORITY. Both the emitted `CustomUniforms` struct and
/// the per-draw buffer are built by walking this, so a field cannot be declared
/// in one order and packed in another — a mismatch would silently read the
/// wrong value rather than fail (pat-s3rrv.122).
///
/// `Sampler2D` is excluded: a texture binds separately, not as a buffer field.
pub fn custom_uniform_fields(compiled: &gdserver3d::CompiledShader3D) -> Vec<String> {
    compiled
        .uniforms
        .iter()
        .filter(|u| u.uniform_type != gdserver3d::shader::UniformType::Sampler2D)
        .map(|u| gdserver3d::shader::wgsl_uniform_field(&u.name))
        .collect()
}

pub fn custom_spatial_module_wgsl(compiled: &gdserver3d::CompiledShader3D) -> Option<String> {
    if compiled.fragment_body.is_empty() {
        return None;
    }
    let stmts = gdserver3d::shader::fragment_body_to_wgsl_statements(&compiled.fragment_body)?;
    // Only divert to a per-material pipeline when the body is genuinely
    // per-fragment (reads UV); constant / uniform-flat shaders keep the cheaper
    // shared pipeline plus the resolved flat albedo.
    if !stmts.iter().any(|s| s.contains("in.uv")) {
        return None;
    }
    // ⚠️ `TIME` STILL FALLS BACK (pat-s3rrv.122). The block reserves a `time`
    // field, but nothing feeds a per-frame value to this renderer yet — there is
    // no elapsed-time source on `GpuRenderer3D` or the viewport. Rendering a
    // TIME-driven shader with a frozen 0.0 would produce a WRONG picture that
    // looks like a working feature; falling back to flat albedo stays visibly
    // unimplemented. Wiring a real clock is a one-field change once a source
    // exists.
    if stmts.iter().any(|s| s.contains("custom.time")) {
        return None;
    }
    // A shader may declare more samplers than the layout has slots. Bail rather
    // than emit a module referencing a binding that does not exist — pipeline
    // creation would fail, which is worse than the flat-albedo fallback.
    let sampler_count = compiled
        .uniforms
        .iter()
        .filter(|u| u.uniform_type == gdserver3d::shader::UniformType::Sampler2D)
        .count();
    if sampler_count > CUSTOM_SAMPLER_SLOTS as usize {
        return None;
    }

    // The group-3 uniform block the transpiler's `custom.<field>` reads refer
    // to. Field ORDER is the contract with the buffer packed at draw time:
    // `time` first, then every buffer-backed uniform in declaration order, both
    // sides walking `compiled.uniforms` the same way. Sampler uniforms are
    // skipped — a texture is not a buffer field, and `ColorExpr3D::Texture`
    // still refuses to transpile, so nothing can reference one.
    let mut block = String::from("struct CustomUniforms {\n    time: vec4<f32>,\n");
    for u in custom_uniform_fields(compiled) {
        block.push_str(&format!("    {u}: vec4<f32>,\n"));
    }
    block.push_str("};\n@group(3) @binding(0) var<uniform> custom: CustomUniforms;\n");
    // Sampler pairs, named exactly as the transpiler emits them and bound at the
    // slot indices `custom_sampler_fields` assigns (pat-s3rrv.122).
    for (slot, field) in custom_sampler_fields(compiled).iter().enumerate() {
        block.push_str(&format!(
            "@group(3) @binding({}) var t_{}: texture_2d<f32>;\n@group(3) @binding({}) var s_{}: sampler;\n",
            custom_texture_binding(slot as u32),
            field,
            custom_sampler_binding(slot as u32),
            field
        ));
    }
    block.push('\n');

    let mut func = block;
    func.push_str("fn custom_albedo(in: VertexOutput) -> vec4<f32> {\n");
    func.push_str("    var color: vec4<f32> = model.albedo;\n");
    for s in &stmts {
        func.push_str("    ");
        func.push_str(s);
        func.push('\n');
    }
    func.push_str("    return color;\n}\n\n");

    // Insert `custom_albedo` before `fs_main` (WGSL requires a callee to be
    // declared before its caller at module scope) and swap the flat albedo
    // source for the per-fragment call.
    let marker = "@fragment\nfn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {\n    let albedo = model.albedo;";
    if !SHADER_3D_WGSL.contains(marker) {
        // The base shader changed shape; bail rather than emit a broken module.
        return None;
    }
    let mut replacement = func;
    replacement.push_str(
        "@fragment\nfn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {\n    let albedo = custom_albedo(in);",
    );
    let module = SHADER_3D_WGSL.replacen(marker, &replacement, 1);

    // Never hand invalid WGSL to `create_shader_module` (a device validation
    // error); validate up front and fall back to the flat path on failure.
    if wgpu::naga::front::wgsl::parse_str(&module).is_err() {
        return None;
    }
    Some(module)
}

/// Fullscreen procedural-sky background pipeline (see [`SKY_WGSL`]).
///
/// Drawn as the first 3 vertices of a render pass, before the meshes: the
/// depth test is disabled and the depth write off, so scene geometry always
/// rasterizes over the sky exactly like meshes drawn over the software
/// background fill.
pub struct SkyPipeline3D {
    /// The wgpu render pipeline.
    pub pipeline: wgpu::RenderPipeline,
    /// Bind group layout for [`SkyUniformData`] (group 0).
    pub layout: wgpu::BindGroupLayout,
}

impl SkyPipeline3D {
    /// Creates the sky background pipeline targeting `target_format`.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("patina-sky-shader"),
            source: wgpu::ShaderSource::Wgsl(SKY_WGSL.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // pat-cezo4: the equirect panorama + its sampler.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("patina-sky-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("patina-sky-render-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_sky"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_sky"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self { pipeline, layout }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Converts a [`Transform3D`] into the inverse view matrix as column-major 4x4.
fn transform_to_view_matrix(t: &Transform3D) -> [[f32; 4]; 4] {
    let inv = t.inverse();
    [
        [inv.basis.x.x, inv.basis.x.y, inv.basis.x.z, 0.0],
        [inv.basis.y.x, inv.basis.y.y, inv.basis.y.z, 0.0],
        [inv.basis.z.x, inv.basis.z.y, inv.basis.z.z, 0.0],
        [inv.origin.x, inv.origin.y, inv.origin.z, 1.0],
    ]
}

/// Converts a [`Transform3D`] into a column-major model matrix.
fn transform_to_model_matrix(t: &Transform3D) -> [[f32; 4]; 4] {
    [
        [t.basis.x.x, t.basis.x.y, t.basis.x.z, 0.0],
        [t.basis.y.x, t.basis.y.y, t.basis.y.z, 0.0],
        [t.basis.z.x, t.basis.z.y, t.basis.z.z, 0.0],
        [t.origin.x, t.origin.y, t.origin.z, 1.0],
    ]
}

/// Identity 4×4 (column-major).
fn identity_mat4() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Column-major `a * b` (apply `b` then `a`).
fn mul_mat4(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut r = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            r[col][row] = a[0][row] * b[col][0]
                + a[1][row] * b[col][1]
                + a[2][row] * b[col][2]
                + a[3][row] * b[col][3];
        }
    }
    r
}

/// Default empty [`GpuLightData`] slot.
fn empty_gpu_light() -> GpuLightData {
    GpuLightData {
        kind: 0,
        shadow_enabled: 0,
        range: 0.0,
        attenuation: 1.0,
        direction: [0.0, -1.0, 0.0],
        spot_angle: 0.0,
        color: [0.0, 0.0, 0.0],
        spot_angle_attenuation: 1.0,
        position: [0.0, 0.0, 0.0],
        shadow_map_index: -1,
        // An empty slot casts nothing, but the default matches Godot's so a
        // slot that is later filled in place does not start out shadowless.
        shadow_opacity: 1.0,
        // An empty slot lights nothing, but an all-ones mask is Godot's
        // default, so a slot later filled in place does not start out culled.
        cull_mask: u32::MAX,
        shadow_bias: 0.1,
        shadow_normal_bias: 2.0,
        shadow_view_proj: identity_mat4(),
        // An empty slot contributes nothing either way, but 1.0 is the base
        // `Light3D` default, so a slot later filled in place is never dimmer
        // than the light that lands in it.
        specular: 1.0,
        // An empty slot casts no shadow, so these are inert; Godot's defaults
        // keep it consistent with the light that later fills the slot.
        directional_shadow_max_distance: 100.0,
        directional_shadow_fade_start: 0.8,
        shadow_uv_scale: 1.0,
        shadow_depth_bias: 0.0,
        shadow_texel_world: 0.0,
        shadow_pcf_radius: -1.0,
        shadow_pcf_taps: 0,
    }
}

/// Converts a scene [`Light3D`] into a [`GpuLightData`] (no shadow matrix yet).
///
/// `camera` is the view position, needed for Godot's light-LOD distance fade.
/// ⚠️ That fade is measured camera-to-**LIGHT**, not camera-to-fragment
/// (`light_storage.cpp:857-864`), so it is a per-light constant and belongs
/// here rather than in the shader — which is also why it needs no WGSL change
/// and no new uniform field at all.
fn light_to_gpu(light: &Light3D, camera: Vector3) -> GpuLightData {
    let kind = match light.light_type {
        LightType::Directional => 0u32,
        LightType::Point => 1u32,
        LightType::Spot => 2u32,
    };

    // A directional light's stored direction is the direction it *travels*,
    // but the shader's `dot(N, L)` wants the direction *toward* the source —
    // negate it, exactly like the software rasterizer's light uniforms. Spot
    // lights keep the forward direction for cone math; point lights derive
    // their direction per-fragment from the position.
    let direction = match light.light_type {
        LightType::Directional => {
            Vector3::new(-light.direction.x, -light.direction.y, -light.direction.z).normalized()
        }
        LightType::Spot => light.direction.normalized(),
        LightType::Point => Vector3::ZERO,
    };

    GpuLightData {
        kind,
        shadow_enabled: if light.shadow_enabled { 1 } else { 0 },
        range: light.range,
        attenuation: light.attenuation,
        direction: [direction.x, direction.y, direction.z],
        spot_angle: light.spot_angle,
        // Light color is sRGB authoring → linear, times energy — the scene is
        // shaded in linear space (matches the software `LightUniform`). A
        // SKY_ONLY light (`affects_scene == false`) contributes no surface
        // lighting; its sky sun disc is drawn separately from `self.lights`.
        color: {
            let shade_energy = if light.affects_scene {
                light.energy
            } else {
                0.0
            };
            // Godot's light LOD and `light_negative`, both folded into the
            // energy exactly as the software renderer does — the SAME
            // `distance_fade_factors`, not a second transcription of the curve.
            // `sign = negative ? -1 : 1` (`light_storage.cpp:840`) makes the
            // light SUBTRACT; it must not become a colour change, or the
            // authored colour stops surviving round-trips.
            let (energy_fade, _) = crate::renderer::distance_fade_factors(light, camera);
            let sign = if light.negative { -1.0 } else { 1.0 };
            let shade_energy = shade_energy * energy_fade * sign;
            [
                srgb_to_linear_channel(light.color.r) * shade_energy,
                srgb_to_linear_channel(light.color.g) * shade_energy,
                srgb_to_linear_channel(light.color.b) * shade_energy,
            ]
        },
        spot_angle_attenuation: light.spot_angle_attenuation,
        position: [light.position.x, light.position.y, light.position.z],
        shadow_map_index: -1,
        // Clamped here rather than only in the shader so a `.tscn` carrying an
        // out-of-range value cannot make the uniform disagree with the software
        // renderer, which clamps in `compute_shadow_factor`.
        //
        // ⚠️ The shadow gets its OWN fade, from `distance_fade_shadow`, which is
        // usually NEARER than `distance_fade_begin` — Godot fades a distant
        // light's shadow out before the light itself, so the cheap thing goes
        // first and the transition is not visible as a pop.
        shadow_opacity: light.shadow_opacity.clamp(0.0, 1.0)
            * crate::renderer::distance_fade_factors(light, camera).1,
        cull_mask: light.cull_mask,
        shadow_bias: light.shadow_bias,
        shadow_normal_bias: light.shadow_normal_bias,
        shadow_view_proj: identity_mat4(),
        specular: light.specular,
        directional_shadow_max_distance: light.directional_shadow_max_distance,
        directional_shadow_fade_start: light.directional_shadow_fade_start,
        shadow_uv_scale: 1.0,
        shadow_depth_bias: 0.0,
        shadow_texel_world: 0.0,
        shadow_pcf_radius: -1.0,
        shadow_pcf_taps: 0,
    }
}

/// Builds light-array uniforms from lights + already-generated shadow maps:
/// each directional/spot light with a map gets that map's atlas layer as its
/// `shadow_map_index` (layer order = `maps` order, pat-mst61 multi-light atlas).
fn pack_lights_from_maps(
    lights: &[Light3D],
    maps: &[crate::shadow_map::ShadowMap],
    omni: &[(u64, gdserver3d::light::ShadowCubemap)],
    camera: Vector3,
) -> LightArrayData {
    let mut light_data = LightArrayData {
        lights: [empty_gpu_light(); gdserver3d::light::MAX_SCENE_LIGHTS],
    };
    // Every map sits in the top-left of an atlas layer this big (see
    // `upload_shadow_depth_array`).
    let atlas_size = maps.iter().map(|m| m.size).max().unwrap_or(1).max(1);
    for (i, light) in lights
        .iter()
        .enumerate()
        .take(gdserver3d::light::MAX_SCENE_LIGHTS)
    {
        let mut gpu = light_to_gpu(light, camera);
        if light.shadow_enabled
            && matches!(
                light.light_type,
                LightType::Directional | LightType::Spot
            )
        {
            if let Some((layer, sm)) =
                maps.iter().enumerate().find(|(_, m)| m.light_id == light.id)
            {
                gpu.shadow_view_proj = mul_mat4(&sm.proj_matrix, &sm.view_matrix);
                gpu.shadow_map_index = layer as i32;
                gpu.shadow_uv_scale = sm.size as f32 / atlas_size as f32;
                if let Some(f) = sm.filter {
                    gpu.shadow_depth_bias = f.depth_bias;
                    gpu.shadow_texel_world = f.texel_world;
                    gpu.shadow_pcf_radius = f.radius_texels.max(0.0);
                    gpu.shadow_pcf_taps = f.taps;
                }
            }
        }
        // Point lights index into the omni face array instead: base layer =
        // 6 × the cubemap's packed position (pat-g4i60).
        if light.shadow_enabled && light.light_type == LightType::Point {
            if let Some(k) = omni.iter().position(|(id, _)| *id == light.id.0) {
                gpu.shadow_map_index = (k * 6) as i32;
            }
        }
        light_data.lights[i] = gpu;
    }
    light_data
}

/// Builds light-array uniforms and ordered shadow maps for GPU upload.
///
/// Uncached variant kept for tests; the render paths go through
/// [`GpuRenderer3D::ensure_shadow_atlas`], which skips generation entirely
/// when the shadow-relevant scene state hasn't changed (pat-2be0q).
fn pack_lights_with_shadows<'a>(
    lights: &[Light3D],
    instances: impl Iterator<Item = &'a Instance3D>,
    camera: Vector3,
) -> (LightArrayData, Vec<crate::shadow_map::ShadowMap>) {
    let instance_list: Vec<Instance3D> = instances.cloned().collect();
    let maps = crate::shadow_map::generate_shadow_maps(lights, &instance_list);
    let omni = pack_omni_cubemaps(lights, &instance_list);
    let light_data = pack_lights_from_maps(lights, &maps, &omni, camera);
    (light_data, maps)
}

/// Generates omni cubemaps for the shadow-casting point lights and packs the
/// `Some` results as `(light id, cubemap)` in light order — the layout
/// [`pack_lights_from_maps`] and the omni face array share (pat-g4i60).
/// [`pack_omni_cubemaps`] with each light's cubemap reused while neither the
/// light nor any caster within its range changed. `casters`, `keys` and
/// `mesh_hashes` are parallel (one caster key and mesh fingerprint per
/// caster). Returns the packed cubemaps, and whether any differs from the
/// previous call's (so the GPU array needs uploading again).
fn omni_cubemaps_cached(
    lights: &[Light3D],
    casters: &[&Instance3D],
    keys: &[u64],
    mesh_hashes: &[Option<u64>],
    cache: &mut HashMap<u64, (u64, gdserver3d::light::ShadowCubemap)>,
    aabbs: &mut HashMap<u64, Option<gdcore::math3d::Aabb>>,
) -> (Vec<(u64, gdserver3d::light::ShadowCubemap)>, bool) {
    use std::hash::Hasher;
    let mut out = Vec::new();
    let mut changed = false;
    let mut seen = Vec::new();
    if aabbs.len() > MESH_CACHE_CAP {
        aabbs.clear();
    }
    for l in lights {
        let casts = l.shadow_enabled
            && l.light_type == LightType::Point
            && matches!(
                l.omni_shadow_mode,
                gdserver3d::light::OmniShadowMode::DualParaboloid
                    | gdserver3d::light::OmniShadowMode::Cube
            );
        if !casts {
            continue;
        }
        let mut in_range: Vec<&Instance3D> = Vec::new();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        h.write_u64(l.id.0);
        for v in [l.position.x, l.position.y, l.position.z, l.range] {
            h.write_u32(v.to_bits());
        }
        h.write_u32(l.shadow_caster_mask);
        for (i, inst) in casters.iter().enumerate() {
            let aabb = match (mesh_hashes.get(i).copied().flatten(), inst.mesh.as_ref()) {
                (Some(mh), Some(m)) => *aabbs.entry(mh).or_insert_with(|| m.aabb()),
                (None, Some(m)) => m.aabb(),
                (_, None) => None,
            };
            if crate::shadow_map::omni_caster_in_range(l, inst, aabb) {
                in_range.push(inst);
                h.write_u64(keys.get(i).copied().unwrap_or(0));
            }
        }
        let key = h.finish();
        seen.push(l.id.0);
        let hit = cache.get(&l.id.0).filter(|(k, _)| *k == key).map(|(_, c)| c.clone());
        let cube = match hit {
            Some(c) => c,
            None => {
                changed = true;
                let c = crate::shadow_map::generate_omni_shadow_cubemap_refs(l, &in_range);
                cache.insert(l.id.0, (key, c.clone()));
                c
            }
        };
        out.push((l.id.0, cube));
    }
    let before = cache.len();
    cache.retain(|id, _| seen.contains(id));
    changed |= cache.len() != before;
    (out, changed)
}

fn pack_omni_cubemaps(
    lights: &[Light3D],
    instances: &[Instance3D],
) -> Vec<(u64, gdserver3d::light::ShadowCubemap)> {
    crate::shadow_map::generate_omni_shadow_cubemaps(lights, instances)
        .into_iter()
        .zip(lights)
        .filter_map(|(cube, light)| cube.map(|c| (light.id.0, c)))
        .collect()
}

/// Content hash of everything that can change a shadow map: each light's
/// shadow-relevant parameters, and — only when some light actually casts —
/// every occluder's visibility, transform, and mesh geometry. Multimesh and
/// material state are deliberately excluded: the CPU shadow rasterizer draws
/// `inst.mesh` only (extend this key if that ever changes).
fn shadow_state_key<'a>(
    lights: &[Light3D],
    instances: impl Iterator<Item = &'a Instance3D>,
) -> u64 {
    let (sig, any_shadow) = shadow_light_signature(lights);
    let keys: Vec<u64> = if any_shadow {
        instances.map(shadow_caster_key).collect()
    } else {
        Vec::new()
    };
    combine_shadow_key(sig, any_shadow, &keys)
}

/// The light half of [`shadow_state_key`], and whether any light casts.
fn shadow_light_signature(lights: &[Light3D]) -> (u64, bool) {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut any_shadow = false;
    for l in lights {
        let casts = l.shadow_enabled
            && (matches!(l.light_type, LightType::Directional | LightType::Spot)
                // Both omni shadow modes cast (default DualParaboloid + Cube),
                // matching generate_omni_shadow_cubemaps (pat-u3l8c.35).
                || (l.light_type == LightType::Point
                    && matches!(
                        l.omni_shadow_mode,
                        gdserver3d::light::OmniShadowMode::DualParaboloid
                            | gdserver3d::light::OmniShadowMode::Cube
                    )));
        any_shadow |= casts;
        h.write_u64(l.id.0);
        h.write_u8(casts as u8);
        h.write_u8(match l.light_type {
            LightType::Directional => 0,
            LightType::Point => 1,
            LightType::Spot => 2,
        });
        for f in [
            l.direction.x,
            l.direction.y,
            l.direction.z,
            l.position.x,
            l.position.y,
            l.position.z,
            l.spot_angle,
            l.range,
        ] {
            h.write_u32(f.to_bits());
        }
        // A camera-fitted directional map moves with the camera (pat-772py):
        // its frame is part of what the map depends on. Snapped, so this only
        // changes every few world units of camera travel.
        if casts && l.light_type == LightType::Directional && l.shadow_camera.is_some() {
            let frame = crate::shadow_map::directional_shadow_frame(l);
            h.write_u32(frame.size);
            for col in frame.view.iter().chain(frame.proj.iter()) {
                for v in col {
                    h.write_u32(v.to_bits());
                }
            }
        }
    }
    (h.finish(), any_shadow)
}

/// Everything one instance contributes to a shadow map: visibility,
/// transform, geometry, and the `cast_shadow` / `layers` settings
/// `casts_into_shadow` reads. The per-caster half of [`shadow_state_key`], and
/// the identity [`crate::shadow_map::generate_shadow_maps_incremental`] uses
/// to tell a static caster from a moving one (pat-414k3).
fn shadow_caster_key(inst: &Instance3D) -> u64 {
    shadow_caster_key_with(inst, inst.mesh.as_ref().map(mesh_content_hash))
}

/// [`shadow_caster_key`] with the mesh's `mesh_content_hash` already known.
fn shadow_caster_key_with(inst: &Instance3D, mesh_hash: Option<u64>) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_u8(inst.visible as u8);
    h.write_u8(inst.casts_shadow() as u8);
    h.write_u32(inst.geometry.layers);
    let t = &inst.transform;
    for f in [
        t.basis.x.x, t.basis.x.y, t.basis.x.z,
        t.basis.y.x, t.basis.y.y, t.basis.y.z,
        t.basis.z.x, t.basis.z.y, t.basis.z.z,
        t.origin.x, t.origin.y, t.origin.z,
    ] {
        h.write_u32(f.to_bits());
    }
    h.write_u64(mesh_hash.unwrap_or(0));
    h.finish()
}

/// Folds the light signature and the per-caster keys into one atlas key.
/// No caster → occluders can't matter; every no-shadow scene shares one
/// cached (empty) atlas.
fn combine_shadow_key(light_sig: u64, any_shadow: bool, caster_keys: &[u64]) -> u64 {
    use std::hash::Hasher;
    if !any_shadow {
        return light_sig;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_u64(light_sig);
    for k in caster_keys {
        h.write_u64(*k);
    }
    h.finish()
}

/// Uploads one or more CPU shadow maps as an `R32Float` 2D array texture.
/// Empty atlas is a 1×1×1 sentinel layer of `f32::MAX` (fully lit).
/// Rewrites the `dirty` texel rectangles of each map (layer) in a shadow
/// array [`upload_shadow_depth_array`] built for maps of the same sizes.
/// Returns how many texels it wrote. Overlapping rectangles are merged into
/// one when they overlap or touch, so a texel is rarely written twice.
fn write_shadow_rects(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    maps: &[crate::shadow_map::ShadowMap],
    dirty: &[Vec<crate::shadow_map::TexelRect>],
) -> u64 {
    let mut written = 0u64;
    let mut rows: Vec<f32> = Vec::new();
    for (layer, (sm, rects)) in maps.iter().zip(dirty).enumerate() {
        let (mw, mh) = sm.depth.dimensions();
        let src = sm.depth.as_slice();
        for &(x0, y0, x1, y1) in &merge_rects(rects) {
            let (x1, y1) = (x1.min(mw.saturating_sub(1)), y1.min(mh.saturating_sub(1)));
            if x0 > x1 || y0 > y1 {
                continue;
            }
            let (rw, rh) = (x1 - x0 + 1, y1 - y0 + 1);
            rows.clear();
            for y in y0..=y1 {
                let start = (y * mw + x0) as usize;
                rows.extend_from_slice(&src[start..start + rw as usize]);
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: x0, y: y0, z: layer as u32 },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&rows),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(rw * 4),
                    rows_per_image: Some(rh),
                },
                wgpu::Extent3d { width: rw, height: rh, depth_or_array_layers: 1 },
            );
            written += rw as u64 * rh as u64;
        }
    }
    written
}

/// `rects` with every pair that overlaps or touches replaced by its bounding
/// rectangle, until no two do.
fn merge_rects(rects: &[crate::shadow_map::TexelRect]) -> Vec<crate::shadow_map::TexelRect> {
    let mut out: Vec<crate::shadow_map::TexelRect> = rects.to_vec();
    loop {
        let mut merged = false;
        'outer: for i in 0..out.len() {
            for j in i + 1..out.len() {
                let (a, b) = (out[i], out[j]);
                if a.0 <= b.2 + 1 && b.0 <= a.2 + 1 && a.1 <= b.3 + 1 && b.1 <= a.3 + 1 {
                    out[i] = (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
                    out.swap_remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            return out;
        }
    }
}

fn upload_shadow_depth_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    maps: &[crate::shadow_map::ShadowMap],
) -> (wgpu::Texture, wgpu::TextureView) {
    let (w, h, layers, bytes): (u32, u32, u32, Vec<u8>) = if maps.is_empty() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&f32::MAX.to_le_bytes());
        (1, 1, 1, bytes)
    } else {
        // Maps of different sizes share the array (pat-772py: a camera-fitted
        // directional map is larger than a spot map). Each goes in the
        // top-left of a layer as big as the largest, the rest of the layer is
        // the empty sentinel, and `shadow_uv_scale` tells the shader how much
        // of the layer is its map.
        let dw = maps.iter().map(|m| m.depth.dimensions().0).max().unwrap_or(1);
        let dh = maps.iter().map(|m| m.depth.dimensions().1).max().unwrap_or(1);
        // Row copies, not per-texel pushes: a fitted map is 2048² texels and
        // this runs whenever a caster moves.
        let (dwu, dhu) = (dw as usize, dh as usize);
        let mut texels = vec![f32::MAX; maps.len() * dwu * dhu];
        for (layer, sm) in maps.iter().enumerate() {
            let (mw, mh) = sm.depth.dimensions();
            let (mw, mh) = (mw as usize, mh as usize);
            let src = sm.depth.as_slice();
            let base = layer * dwu * dhu;
            for y in 0..mh {
                texels[base + y * dwu..base + y * dwu + mw]
                    .copy_from_slice(&src[y * mw..(y + 1) * mw]);
            }
        }
        let bytes: Vec<u8> = bytemuck::cast_slice(&texels).to_vec();
        (dw, dh, maps.len() as u32, bytes)
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow-depth-array"),
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: layers.max(1),
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w.max(1) * 4),
            rows_per_image: Some(h.max(1)),
        },
        wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: layers.max(1),
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("shadow-depth-array-view"),
        format: Some(wgpu::TextureFormat::R32Float),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        aspect: wgpu::TextureAspect::All,
        base_mip_level: 0,
        mip_level_count: Some(1),
        base_array_layer: 0,
        array_layer_count: Some(layers.max(1)),
        usage: None,
    });
    (texture, view)
}

/// Uploads omni shadow cubemaps as an `R32Float` 2D array: 6 face layers per
/// cubemap in [`CubeFace`] order (+X, −X, +Y, −Y, +Z, −Z), so cubemap `k`'s
/// faces occupy layers `6k..6k+6` (pat-g4i60). Empty input uploads a 1×1×1
/// `f32::MAX` sentinel layer (fully lit).
fn upload_omni_shadow_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cubes: &[(u64, gdserver3d::light::ShadowCubemap)],
) -> (wgpu::Texture, wgpu::TextureView) {
    let (res, layers, bytes): (u32, u32, Vec<u8>) = if cubes.is_empty() {
        (1, 1, f32::MAX.to_le_bytes().to_vec())
    } else {
        let res = cubes[0].1.resolution.max(1);
        let mut bytes =
            Vec::with_capacity(cubes.len() * 6 * (res * res) as usize * 4);
        for (_, cube) in cubes {
            debug_assert_eq!(
                cube.resolution, res,
                "omni cubemaps in one array must share a resolution"
            );
            for face in &cube.faces {
                for &d in face {
                    bytes.extend_from_slice(&d.to_le_bytes());
                }
            }
        }
        (res, cubes.len() as u32 * 6, bytes)
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("omni-shadow-array"),
        size: wgpu::Extent3d {
            width: res,
            height: res,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(res * 4),
            rows_per_image: Some(res),
        },
        wgpu::Extent3d {
            width: res,
            height: res,
            depth_or_array_layers: layers,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("omni-shadow-array-view"),
        format: Some(wgpu::TextureFormat::R32Float),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        aspect: wgpu::TextureAspect::All,
        base_mip_level: 0,
        mip_level_count: Some(1),
        base_array_layer: 0,
        array_layer_count: Some(layers),
        usage: None,
    });
    (texture, view)
}

/// Builds the light bind group (uniform + shadow depth array).
fn create_light_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    light_data: &LightArrayData,
    shadow_view: &wgpu::TextureView,
    omni_view: &wgpu::TextureView,
    probes: &ProbeCubeArray,
    label: &str,
) -> (wgpu::BindGroup, wgpu::Buffer) {
    let light_buf = create_buffer_init(
        device,
        queue,
        &format!("{label}-light-uniform"),
        bytemuck::bytes_of(light_data),
        wgpu::BufferUsages::UNIFORM,
    );
    let probe_buf = create_buffer_init(
        device,
        queue,
        &format!("{label}-probe-uniform"),
        bytemuck::bytes_of(&probes.data),
        wgpu::BufferUsages::UNIFORM,
    );
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(omni_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&probes.view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: probe_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&probes.dfg_view),
            },
        ],
    });
    (bg, light_buf)
}

/// Bytes per pixel for the editor color/readback formats used here.
fn bytes_per_pixel(format: wgpu::TextureFormat) -> u32 {
    match format {
        wgpu::TextureFormat::Rgba16Float => 8,
        wgpu::TextureFormat::Rgba32Float => 16,
        // Bgra8Unorm / Rgba8Unorm and friends.
        _ => 4,
    }
}

/// Decodes an IEEE-754 binary16 (half) bit pattern to `f32`. Used to unpack the
/// `Rgba16Float` HDR readback buffer.
/// Calls `f(row, row_slice)` for every `row_len`-long row of `out`, in bands on
/// up to 8 worker threads (pat-414k3). For per-pixel readback conversions,
/// whose result does not depend on which thread computes it.
fn par_rows<T: Send>(out: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = if row_len == 0 { 0 } else { out.len() / row_len };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, 8)
        .min(rows.max(1));
    if threads <= 1 || rows < 64 {
        for (r, row) in out.chunks_mut(row_len.max(1)).enumerate() {
            f(r, row);
        }
        return;
    }
    let band_rows = rows.div_ceil(threads);
    let f = &f;
    std::thread::scope(|scope| {
        for (b, band) in out.chunks_mut(band_rows * row_len).enumerate() {
            scope.spawn(move || {
                for (r, row) in band.chunks_mut(row_len).enumerate() {
                    f(b * band_rows + r, row);
                }
            });
        }
    });
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) & 0x1;
    let exp = (bits >> 10) & 0x1f;
    let frac = bits & 0x3ff;
    let sign_f = if sign == 1 { -1.0f32 } else { 1.0f32 };
    match exp {
        0 => {
            // Subnormal or zero: value = frac / 2^10 * 2^(-14).
            // 2^-24, exactly.
            sign_f * (frac as f32) * (1.0 / 16_777_216.0)
        }
        0x1f => {
            // Inf/NaN — clamp to a finite large value so downstream math is safe.
            if frac == 0 {
                sign_f * f32::MAX
            } else {
                0.0
            }
        }
        _ => {
            // Normalized: value = (1 + frac/2^10) * 2^(exp-15), built directly
            // as f32 bits (rebias the exponent, widen the mantissa). Exactly the
            // same value as the float formula — every half is representable in
            // f32 — without a `powi` per channel, which was ~8% of a 3D Play
            // frame (pat-p3p1m).
            let bits32 = ((sign as u32) << 31) | ((exp as u32 + 112) << 23) | ((frac as u32) << 13);
            f32::from_bits(bits32)
        }
    }
}

/// Builds the sky-pass uniform for `env` if (and only if) its background is a
/// procedural sky: `BackgroundMode::Sky` with a `SkyMaterial::Procedural`
/// material. Everything the WGSL sky needs is taken from the same inputs
/// the software background fill reads — the viewport's projection (inverted),
/// the camera basis, the material's colors/curves/energies with Godot's
/// uniform mappings applied, and the sky-visible directional lights — so the
/// two paths compute identical pixels. Returns `None` for any other
/// background (the caller clears to a flat color instead, like the software
/// fill's non-sky arms).
fn sky_uniform_for_env(
    env: &Environment3D,
    viewport: &Viewport3D,
    w: u32,
    h: u32,
    suns: &[crate::renderer::SkySun],
) -> Option<SkyUniformData> {
    if env.background_mode != BackgroundMode::Sky {
        return None;
    }
    let sky = env.sky.as_ref()?;

    let proj = perspective_projection_matrix(
        viewport.fov,
        viewport.aspect(),
        viewport.near,
        viewport.far,
    );
    let inv_projection = invert_projection(&proj);
    let b = &viewport.camera_transform.basis;
    let energy = env.background_energy_multiplier;

    let mut sun_dir_energy = [[0.0f32; 4]; 4];
    let mut sun_color_size = [[0.0f32; 4]; 4];
    let sun_count = suns.len().min(4);
    for (i, sun) in suns.iter().take(4).enumerate() {
        sun_dir_energy[i] = [sun.direction.x, sun.direction.y, sun.direction.z, sun.energy];
        // LIGHTn_COLOR is the light color in linear space (sky.cpp), * energy
        // applied in the shader via `de.w`.
        sun_color_size[i] = [
            srgb_to_linear_channel(sun.color.r),
            srgb_to_linear_channel(sun.color.g),
            srgb_to_linear_channel(sun.color.b),
            sun.size,
        ];
    }

    // The gradient endpoints are `source_color` uniforms: Godot premultiplies
    // each by its hemisphere energy in sRGB space, then binds it as linear —
    // `srgb_to_linear(channel * energy)`. Folding the energy into the color
    // here (and leaving the shader's per-hemisphere energy at 1.0) makes the
    // GPU sky mix identical to the software `sample_procedural_sky`.
    let lin4 = |c: &Color, e: f32| {
        [
            srgb_to_linear_channel(c.r * e),
            srgb_to_linear_channel(c.g * e),
            srgb_to_linear_channel(c.b * e),
            1.0,
        ]
    };

    // pat-coz7r: panoramic skies also run through the sky pass (equirect UV in
    // WGSL). Without a bound HDR texture yet, use a neutral gradient tinted by
    // energy so the path is active; software still owns pixel-accurate sampling.
    let (sky_top, sky_horizon, ground_bottom, ground_horizon, params, sky_mode) =
        match &sky.material {
            SkyMaterial::Procedural(mat) => (
                lin4(&mat.sky_top_color, mat.sky_energy_multiplier),
                lin4(&mat.sky_horizon_color, mat.sky_energy_multiplier),
                lin4(&mat.ground_bottom_color, mat.ground_energy_multiplier),
                lin4(&mat.ground_horizon_color, mat.ground_energy_multiplier),
                [
                    0.6 / mat.sky_curve.max(1e-4),
                    0.6 / mat.ground_curve.max(1e-4),
                    1.0,
                    1.0,
                ],
                0.0_f32,
            ),
            SkyMaterial::Panoramic(mat) => {
                let e = mat.energy_multiplier;
                let top = Color::new(0.35, 0.4, 0.55, 1.0);
                let hor = Color::new(0.45, 0.48, 0.52, 1.0);
                let bot = Color::new(0.2, 0.18, 0.15, 1.0);
                (
                    lin4(&top, e),
                    lin4(&hor, e),
                    lin4(&bot, e),
                    lin4(&hor, e),
                    // w = 2 when the panorama is decoded and bound (pat-cezo4).
                    [4.0, 30.0, 1.0, if mat.texture.is_some() { 2.0 } else { 1.0 }],
                    1.0_f32, // equirect mode flag (sun_params.z)
                )
            }
            // pat-fw141: physical (Preetham) sky — pack the material through
            // the existing uniform slots (colors linear via `source_color`,
            // coefficients in the alpha channels; see the WGSL physical
            // branch): sky_top = rayleigh color + coefficient, sky_horizon =
            // mie color + coefficient, ground_bottom = ground color +
            // turbidity, params = (mie eccentricity, sun disk scale, -, -).
            SkyMaterial::Physical(mat) => (
                [
                    srgb_to_linear_channel(mat.rayleigh_color.r),
                    srgb_to_linear_channel(mat.rayleigh_color.g),
                    srgb_to_linear_channel(mat.rayleigh_color.b),
                    mat.rayleigh_coefficient,
                ],
                [
                    srgb_to_linear_channel(mat.mie_color.r),
                    srgb_to_linear_channel(mat.mie_color.g),
                    srgb_to_linear_channel(mat.mie_color.b),
                    mat.mie_coefficient,
                ],
                [
                    srgb_to_linear_channel(mat.ground_color.r),
                    srgb_to_linear_channel(mat.ground_color.g),
                    srgb_to_linear_channel(mat.ground_color.b),
                    mat.turbidity,
                ],
                [0.0, 0.0, 0.0, 0.0],
                [mat.mie_eccentricity, mat.sun_disk_scale, 0.0, 0.0],
                2.0_f32, // physical mode flag (sun_params.z)
            ),
            // A custom `shader_type sky` material has no WGSL module yet — the
            // GPU sky pass is a follow-up, and the software renderer owns
            // pixel-accurate custom-sky output today (pat-0q2kx.224). Feed the
            // procedural slots from Godot's own ProceduralSkyMaterial defaults so
            // the GPU path shows a plausible sky rather than uninitialized
            // values; it will differ from software until the module lands.
            // pat-cezo4: a cubemap shader sky the host baked into a panorama
            // draws through the equirect branch, sampling the bake.
            SkyMaterial::Shader(m) if m.baked.is_some() => {
                let hor = Color::new(0.45, 0.48, 0.52, 1.0);
                (
                    lin4(&hor, 1.0),
                    lin4(&hor, 1.0),
                    lin4(&hor, 1.0),
                    lin4(&hor, 1.0),
                    [4.0, 30.0, 1.0, 2.0],
                    1.0_f32,
                )
            }
            SkyMaterial::Shader(_) => {
                let d = gdserver3d::sky::ProceduralSkyMaterial::default();
                (
                    lin4(&d.sky_top_color, d.sky_energy_multiplier),
                    lin4(&d.sky_horizon_color, d.sky_energy_multiplier),
                    lin4(&d.ground_bottom_color, d.ground_energy_multiplier),
                    lin4(&d.ground_horizon_color, d.ground_energy_multiplier),
                    [
                        0.6 / d.sky_curve.max(1e-4),
                        0.6 / d.ground_curve.max(1e-4),
                        1.0,
                        1.0,
                    ],
                    0.0_f32,
                )
            }
        };

    Some(SkyUniformData {
        inv_projection,
        camera_basis: [
            [b.x.x, b.x.y, b.x.z, 0.0],
            [b.y.x, b.y.y, b.y.z, 0.0],
            [b.z.x, b.z.y, b.z.z, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        sky_top,
        sky_horizon,
        ground_bottom,
        ground_horizon,
        params,
        size: [w as f32, h as f32, 0.0, 0.0],
        sun_dir_energy,
        sun_color_size,
        sun_params: {
            let (cos_sun, inv_curve) = match &sky.material {
                SkyMaterial::Procedural(mat) => (
                    mat.sun_angle_max.to_radians().cos(),
                    1.6 / mat.sun_curve.max(1e-4).powf(1.4),
                ),
                _ => (0.0_f32, 1.0_f32),
            };
            // The physical sky's `exposure` uniform is the material energy
            // multiplied with the environment's background energy.
            let w_energy = match &sky.material {
                SkyMaterial::Physical(mat) => mat.energy_multiplier * energy,
                _ => energy,
            };
            [
                cos_sun,
                inv_curve,
                // z encodes mode + sun count: procedural = count (0..4),
                // panoramic = 1000 + count, physical = 2000 + count.
                if sky_mode > 1.5 {
                    2000.0 + sun_count as f32
                } else if sky_mode > 0.5 {
                    1000.0 + sun_count as f32
                } else {
                    sun_count as f32
                },
                w_energy,
            ]
        },
    })
}

/// Creates a buffer and writes initial data to it.
fn create_buffer_init(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    data: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: data.len() as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buf, 0, data);
    buf
}

/// Packs mesh vertex data into a GPU vertex buffer.
fn mesh_to_gpu_vertices(mesh: &Mesh3D) -> Vec<GpuVertex> {
    let vert_count = mesh.vertices.len();
    let mut out = Vec::with_capacity(vert_count);
    for i in 0..vert_count {
        let pos = mesh.vertices[i];
        let normal = if i < mesh.normals.len() {
            mesh.normals[i]
        } else {
            Vector3::new(0.0, 1.0, 0.0)
        };
        let uv = if i < mesh.uvs.len() {
            mesh.uvs[i]
        } else {
            [0.0, 0.0]
        };
        out.push(GpuVertex {
            position: [pos.x, pos.y, pos.z],
            normal: [normal.x, normal.y, normal.z],
            uv,
        });
    }
    out
}

// ── Draw-entry expansion (multimesh + shader material) ──────────────

/// `BaseMaterial3D.BLEND_MODE_ADD` (pat-cezo4): colour `src * src_alpha + dst`,
/// destination alpha kept.
const ADDITIVE_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// A single resolved draw: one mesh + world transform + material parameters,
/// ready to upload to the GPU. Produced by [`instance_draw_entries`].
struct GpuDrawEntry<'a> {
    mesh: &'a Mesh3D,
    transform: Transform3D,
    albedo: Color,
    emission: Color,
    roughness: f32,
    metallic: f32,
    shading_mode: u32,
    /// Face-cull mode for this draw (pat-s3rrv.30). Flattened out of the
    /// material like the fields above, because the draw loop needs it to pick a
    /// pipeline — cull state is baked into the pipeline, not a uniform.
    cull_mode: crate::render_modes::CullMode,
    /// Transparency mode and its scissor threshold (pat-s3rrv.30).
    ///
    /// Used for the AlphaScissor decision below. With a flat albedo every texel
    /// of the surface carries the SAME alpha, so "discard sub-threshold texels"
    /// and "skip the draw" are the same thing — the per-draw test is exact here,
    /// not an approximation. Per-texel scissor for a *textured* albedo would
    /// need a threshold uniform and a `discard` in the WGSL, and is reported as
    /// remaining scope.
    transparency: crate::render_modes::Transparency,
    /// See [`Self::transparency`].
    alpha_scissor_threshold: f32,
    /// Godot `depth_draw_mode` (pat-s3rrv.30); decides whether this draw writes
    /// depth once its transparency is known.
    depth_draw_mode: crate::render_modes::DepthDrawMode,
    /// Godot `blend_mode` (pat-cezo4); `Add` picks the additive pipeline.
    blend_mode: gdserver3d::material::BlendMode,
    /// Godot `no_depth_test` (pat-9gjpr): the editor path depth-tested every
    /// draw, so a Label3D authored to draw over everything was hidden behind
    /// whatever stood in front of it.
    no_depth_test: bool,
    /// `BaseMaterial3D.albedo_texture` for this draw (pat-mgixk.52).
    ///
    /// An owned `Arc` clone rather than a borrow: `instance_draw_entries` reads
    /// it off a LOCAL clone of the material, so a reference would not outlive
    /// the function. The refcount bump is the whole cost — the pixels are
    /// shared.
    ///
    /// `None` when the material has no map **and also** when a shader material
    /// resolved the albedo, which keeps pat-s3rrv.122's custom path on the
    /// white dummy and therefore unchanged.
    albedo_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// `BaseMaterial3D.normal_texture` and its strength (pat-mgixk.52), carried
    /// the same owned-`Arc` way as `albedo_texture`.
    normal_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// See [`Self::normal_texture`]. Carried even when there is no map so the
    /// uniform always has a defined value; the shader's `has_normal_map` flag
    /// is what decides whether it is read.
    normal_scale: f32,
    /// `BaseMaterial3D` roughness / metallic / emission maps (pat-mgixk.52).
    ///
    /// No presence flag for these three: unlike the normal map they are plain
    /// MULTIPLIERS, so a 1x1 white texel is an exact identity and `None` is
    /// indistinguishable from "no map" without one.
    roughness_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// See [`Self::roughness_texture`].
    metallic_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// See [`Self::roughness_texture`].
    emission_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// The material's `texture_filter` / `texture_repeat` (pat-3u1wu): which
    /// sampler this draw's maps are read with.
    sampler: MaterialSamplerKey,
}

/// Resolves an albedo colour for a shader material, matching the software path's
/// common case: a `source_color` albedo uniform (e.g. `albedo_color`) on an
/// otherwise `unshaded` shader. Full custom-shader execution is out of scope for
/// the GPU path (it would need a shader→WGSL transpiler); this covers the constant
/// albedo override the parity tests + typical editor shader materials use.
fn resolve_shader_albedo(mat: &ShaderMaterial3D) -> Option<Color> {
    for name in ["albedo_color", "albedo", "color", "modulate", "source_color"] {
        if let Some(gdvariant::Variant::Color(c)) = mat.get_shader_parameter(name) {
            return Some(*c);
        }
    }
    // Fall back to any Color-typed shader parameter.
    mat.parameters().find_map(|(_, v)| match v {
        gdvariant::Variant::Color(c) => Some(*c),
        _ => None,
    })
}

/// Expands one instance into the concrete draws the GPU must record, mirroring
/// [`SoftwareRenderer3D`](crate::renderer::SoftwareRenderer3D)'s logic so the two
/// backends agree:
///
/// - **Multimesh**: one entry per multimesh instance, composing the instance's
///   world transform with each per-instance transform and applying per-instance
///   colours (when present).
/// - **Single mesh**: one entry at the instance transform.
/// - **Shader material**: overrides the albedo with the resolved shader colour and
///   shades unlit (matching the common `render_mode unshaded` override).
///
/// Returns no entries for an instance with neither a mesh nor a multimesh-with-
/// mesh — the previous GPU loop silently skipped those; this makes multimesh and
/// shader-material instances render on the GPU like they already do in software.
fn instance_draw_entries<'a>(inst: &'a Instance3D, surfaces: &'a [Mesh3D]) -> Vec<GpuDrawEntry<'a>> {
    let base = inst.material.clone().unwrap_or_default();
    let base_mode = match base.shading_mode {
        ShadingMode::Unlit => 0u32,
        ShadingMode::Lambert => 1u32,
        ShadingMode::Phong => 2u32,
    };
    // A shader material overrides the albedo and shades unlit (the common
    // `unshaded; uniform vec4 albedo_color : source_color;` case).
    let shader_albedo = inst.shader_material.as_ref().and_then(resolve_shader_albedo);
    let mode = if shader_albedo.is_some() { 0 } else { base_mode };
    // A resolved shader albedo REPLACES the material, so its draw must not also
    // be modulated by the BaseMaterial3D map — that would tint pat-s3rrv.122's
    // custom shaders by a texture their author never referenced. Binding no
    // texture here means the white dummy, i.e. no change at all.
    let albedo_texture = if shader_albedo.is_some() {
        None
    } else {
        base.albedo_texture.clone()
    };
    // Same reasoning as the albedo map: a shader material replaces the
    // material, so its draw must not be normal-mapped by a texture its author
    // never referenced.
    let normal_texture = if shader_albedo.is_some() {
        None
    } else {
        base.normal_texture.clone()
    };
    // Same reasoning for the remaining three maps: a shader material replaces
    // the material, so none of its maps should modulate the custom result.
    let (roughness_texture, metallic_texture, emission_texture) = if shader_albedo.is_some() {
        (None, None, None)
    } else {
        (
            base.roughness_texture.clone(),
            base.metallic_texture.clone(),
            base.emission_texture.clone(),
        )
    };

    let mut entries = Vec::new();
    if let Some(mm) = &inst.multimesh {
        if let Some(shared) = &mm.mesh {
            let has_colors = !mm.instance_colors.is_empty();
            for i in 0..mm.instance_count() {
                let transform = inst.transform * mm.get_instance_transform(i);
                let albedo = shader_albedo
                    .or_else(|| has_colors.then(|| mm.get_instance_color(i)))
                    .unwrap_or(base.albedo);
                entries.push(GpuDrawEntry {
                    mesh: shared,
                    transform,
                    albedo,
                    emission: base.emission,
                    roughness: base.roughness,
                    metallic: base.metallic,
                    shading_mode: mode,
                    cull_mode: base.cull_mode,
                    transparency: base.transparency,
                    alpha_scissor_threshold: base.alpha_scissor_threshold,
                    depth_draw_mode: base.depth_draw_mode,
                    blend_mode: base.blend_mode,
                    no_depth_test: base.no_depth_test,
                    albedo_texture: albedo_texture.clone(),
                    normal_texture: normal_texture.clone(),
                    normal_scale: base.normal_scale,
                    roughness_texture: roughness_texture.clone(),
                    metallic_texture: metallic_texture.clone(),
                    emission_texture: emission_texture.clone(),
                    sampler: MaterialSamplerKey::of(&base),
                });
            }
        }
    } else if let Some(mesh) = &inst.mesh {
        entries.push(GpuDrawEntry {
            mesh,
            transform: inst.transform,
            albedo: shader_albedo.unwrap_or(base.albedo),
            emission: base.emission,
            roughness: base.roughness,
            metallic: base.metallic,
            shading_mode: mode,
            cull_mode: base.cull_mode,
            transparency: base.transparency,
            alpha_scissor_threshold: base.alpha_scissor_threshold,
            depth_draw_mode: base.depth_draw_mode,
            blend_mode: base.blend_mode,
            no_depth_test: base.no_depth_test,
            albedo_texture,
            normal_texture,
            normal_scale: base.normal_scale,
            roughness_texture,
            metallic_texture,
            emission_texture,
            sampler: MaterialSamplerKey::of(&base),
        });
        // pat-zwvgw: the mesh's ADDITIONAL surfaces, each with its own
        // material — `surface_materials[i]` pairs with `surfaces[i]`, falling
        // back to the instance material — as the software renderer draws them
        // (pat-s3rrv.187). The GPU used to draw only the primary surface, so a
        // multi-material glTF character lost every part after the first.
        for (i, surface) in surfaces.iter().enumerate() {
            if surface.indices.is_empty() {
                continue;
            }
            let m = inst
                .surface_materials
                .get(i)
                .cloned()
                .flatten()
                .unwrap_or_else(|| base.clone());
            let shaded = shader_albedo.is_none();
            let surface_mode = match (shaded, m.shading_mode) {
                (false, _) | (true, ShadingMode::Unlit) => 0u32,
                (true, ShadingMode::Lambert) => 1u32,
                (true, ShadingMode::Phong) => 2u32,
            };
            let tex = |t: &Option<std::sync::Arc<gdserver3d::material::TextureImage>>| {
                if shaded {
                    t.clone()
                } else {
                    None
                }
            };
            entries.push(GpuDrawEntry {
                mesh: surface,
                transform: inst.transform,
                albedo: shader_albedo.unwrap_or(m.albedo),
                emission: m.emission,
                roughness: m.roughness,
                metallic: m.metallic,
                shading_mode: surface_mode,
                cull_mode: m.cull_mode,
                transparency: m.transparency,
                alpha_scissor_threshold: m.alpha_scissor_threshold,
                depth_draw_mode: m.depth_draw_mode,
                blend_mode: m.blend_mode,
                no_depth_test: m.no_depth_test,
                albedo_texture: tex(&m.albedo_texture),
                normal_texture: tex(&m.normal_texture),
                normal_scale: m.normal_scale,
                roughness_texture: tex(&m.roughness_texture),
                metallic_texture: tex(&m.metallic_texture),
                emission_texture: tex(&m.emission_texture),
                sampler: MaterialSamplerKey::of(&m),
            });
        }
    }
    entries
}

/// The group-3 bind group of one custom spatial shader draw (pat-9gjpr): the
/// `time` slot, one `vec4` per buffer uniform in [`CustomSpatialModule`]
/// order, and every sampler slot of `custom_layout` (unused ones on a 1x1
/// stand-in — a bind group must fill each entry its layout declares).
#[allow(clippy::too_many_arguments)]
fn custom_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    module: &crate::spatial_wgsl::CustomSpatialModule,
    material: &ShaderMaterial3D,
    time: f32,
    (w, h): (u32, u32),
    tex_cache: &mut HashMap<
        (usize, bool),
        (std::sync::Arc<gdserver3d::material::TextureImage>, wgpu::TextureView),
    >,
    unbound_cache: &mut HashMap<[u32; 4], wgpu::TextureView>,
    sampler_cache: &mut HashMap<(bool, bool), wgpu::Sampler>,
) -> wgpu::BindGroup {
    let mut values: Vec<[f32; 4]> = vec![[time, w as f32, h as f32, 0.0]];
    for slot in &module.uniforms {
        values.push(crate::spatial_wgsl::slot_value(
            slot,
            material.get_shader_parameter(&slot.name),
        ));
    }
    let buf = create_buffer_init(
        device,
        queue,
        "custom-spatial-uniforms",
        bytemuck::cast_slice(&values),
        wgpu::BufferUsages::UNIFORM,
    );
    // Images may be evicted only when their source is gone, so a cached view
    // never outlives the address it is keyed by.
    if tex_cache.len() > 256 {
        tex_cache.retain(|_, (img, _)| std::sync::Arc::strong_count(img) > 1);
    }
    let mut views: Vec<wgpu::TextureView> = Vec::new();
    let mut samplers: Vec<wgpu::Sampler> = Vec::new();
    for i in 0..CUSTOM_SAMPLER_SLOTS as usize {
        let slot = module.samplers.get(i);
        let bound = slot.and_then(|s| material.get_texture(&s.name).map(|img| (s, img)));
        let view = match bound {
            Some((s, img)) => {
                let key = (std::sync::Arc::as_ptr(img) as usize, s.source_color);
                tex_cache
                    .entry(key)
                    .or_insert_with(|| {
                        let t = upload_custom_texture(device, queue, img, s.source_color);
                        (img.clone(), t.create_view(&wgpu::TextureViewDescriptor::default()))
                    })
                    .1
                    .clone()
            }
            None => {
                let c = slot.map_or([1.0; 4], |s| s.unbound);
                unbound_cache
                    .entry(c.map(f32::to_bits))
                    .or_insert_with(|| {
                        let img = gdserver3d::material::TextureImage::new(
                            1,
                            1,
                            vec![Color::new(c[0], c[1], c[2], c[3])],
                        );
                        upload_custom_texture(device, queue, &img, false)
                            .create_view(&wgpu::TextureViewDescriptor::default())
                    })
                    .clone()
            }
        };
        views.push(view);
        let (nearest, repeat) = slot.map_or((false, false), |s| (s.nearest, s.repeat));
        let sampler = sampler_cache
            .entry((nearest, repeat))
            .or_insert_with(|| {
                let filter = if nearest {
                    wgpu::FilterMode::Nearest
                } else {
                    wgpu::FilterMode::Linear
                };
                let address = if repeat {
                    wgpu::AddressMode::Repeat
                } else {
                    wgpu::AddressMode::ClampToEdge
                };
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("custom-spatial-sampler"),
                    mag_filter: filter,
                    min_filter: filter,
                    address_mode_u: address,
                    address_mode_v: address,
                    address_mode_w: address,
                    ..Default::default()
                })
            })
            .clone();
        samplers.push(sampler);
    }
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: buf.as_entire_binding(),
    }];
    for i in 0..CUSTOM_SAMPLER_SLOTS {
        entries.push(wgpu::BindGroupEntry {
            binding: custom_texture_binding(i),
            resource: wgpu::BindingResource::TextureView(&views[i as usize]),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: custom_sampler_binding(i),
            resource: wgpu::BindingResource::Sampler(&samplers[i as usize]),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("custom-spatial-bind-group"),
        layout,
        entries: &entries,
    })
}

/// A mesh's additional surfaces as standalone meshes, for
/// [`instance_draw_entries`] (pat-zwvgw).
fn split_surface_meshes(mesh: &Mesh3D) -> Vec<Mesh3D> {
    mesh.surfaces
        .iter()
        .map(|s| Mesh3D {
            vertices: s.vertices.clone(),
            normals: s.normals.clone(),
            uvs: s.uvs.clone(),
            indices: s.indices.clone(),
            primitive_type: s.primitive_type,
            ..Default::default()
        })
        .collect()
}

// ── GpuRenderer3D ───────────────────────────────────────────────────

/// A wgpu-based 3D renderer implementing [`RenderingServer3D`].
///
/// Renders 3D scenes using a real GPU render pipeline with WGSL vertex
/// and fragment shaders. Supports unlit, Lambert, and Phong shading.
pub struct GpuRenderer3D {
    ctx: GpuContext3D,
    pipeline: RenderPipeline3D,
    instances: HashMap<u64, Instance3D>,
    lights: Vec<Light3D>,
    reflection_probes: Vec<ReflectionProbe>,
    /// `VoxelGI` volumes contributing indirect ambient (pat-s3rrv.114).
    voxel_gis: Vec<VoxelGI>,
    /// Decals to project onto shaded geometry this frame (pat-s3rrv.29).
    ///
    /// The GPU backend used to drop these on the floor: the four env-effect
    /// trait methods are DEFAULT NO-OPS on `RenderingServer3D`, the software
    /// renderer overrode them and this one did not, and `gdeditor` ships with
    /// `gpu-render` on by default — so a `.tscn` Decal changed nothing in the
    /// editor viewport or in Play while the software tests stayed green.
    decals: Vec<Decal3D>,
    /// Fog volumes injecting local density, with their world transforms.
    fog_volumes: Vec<crate::renderer::PlacedFogVolume>,
    /// Occluders registered for this scene.
    occluders: Vec<OccluderInstance3D>,
    next_id: u64,
    /// The texture format used for color attachments.
    pub target_format: wgpu::TextureFormat,
    /// Pipeline variants of [`Self::editor_pipeline`] keyed by
    /// `(cull mode, writes depth)` (pat-s3rrv.30).
    ///
    /// Both are baked into a wgpu pipeline rather than settable per draw, so a
    /// material that culls FRONT, or an alpha-blended one that must not write
    /// depth, needs its own pipeline. Built once on first use; the default
    /// (Back, writes depth) is `editor_pipeline` itself.
    editor_variants: Vec<(crate::render_modes::CullMode, bool, wgpu::RenderPipeline)>,
    /// Additive-blend (`BLEND_MODE_ADD`) variants by cull mode (pat-cezo4),
    /// built with [`Self::editor_variants`].
    editor_add_variants: Vec<(crate::render_modes::CullMode, wgpu::RenderPipeline)>,
    /// Every pipeline `render_editor_color_bgra` has needed, by state
    /// (pat-9gjpr), built on first use. Holds the custom modules' `Arc`s too
    /// so a key's module address cannot be reused by another module.
    editor_pipe_cache: HashMap<EditorPipelineKey, wgpu::RenderPipeline>,
    editor_pipe_modules: Vec<std::sync::Arc<crate::spatial_wgsl::CustomSpatialModule>>,
    /// Custom-shader sampler textures by (image address, sRGB), with the image
    /// kept alive so the address stays unique (pat-9gjpr).
    custom_tex_cache: HashMap<
        (usize, bool),
        (std::sync::Arc<gdserver3d::material::TextureImage>, wgpu::TextureView),
    >,
    /// 1x1 stand-ins for unbound custom samplers, by their linear RGBA bits.
    custom_unbound_cache: HashMap<[u32; 4], wgpu::TextureView>,
    /// Custom-shader samplers by (nearest, repeat).
    custom_sampler_cache: HashMap<(bool, bool), wgpu::Sampler>,
    /// Shader `TIME` in seconds for custom spatial shaders (pat-9gjpr).
    shader_time: f32,
    /// Lazily-built pipeline for the editor color-only path. Uses a **non-sRGB**
    /// (`Bgra8Unorm`) target so shader output is written to bytes linearly,
    /// matching the software rasterizer's `color * 255` — which keeps GPU meshes
    /// visually consistent with the (linear) software overlays and background
    /// drawn around them. Separate from `pipeline` so the sRGB `render_frame`
    /// trait path and its tests are unaffected.
    editor_pipeline: Option<RenderPipeline3D>,
    /// Lazily-built fullscreen sky pipeline for the editor path (same non-sRGB
    /// target as `editor_pipeline`). Draws the environment's procedural sky
    /// gradient behind the meshes, matching the software background fill.
    editor_sky_pipeline: Option<SkyPipeline3D>,
    /// Cached editor render targets, reused across frames and recreated only on
    /// resize — the per-frame texture/readback-buffer churn is a real cost at
    /// 60fps.
    editor_targets: Option<EditorTargets>,
    /// Persistent cache of uploaded mesh vertex/index buffers, keyed by a content
    /// hash, so static geometry isn't re-tessellated/re-uploaded every frame
    /// (pat-ism62 #2b — matters once a scene holds hundreds+ of meshes). Survives
    /// `reset_scene` (it's GPU resource state, not scene state); bounded by
    /// [`MESH_CACHE_CAP`] so per-frame-changing geometry can't leak unboundedly.
    mesh_gpu_cache: HashMap<u64, CachedMeshBuffers>,
    /// Each instance's additional mesh surfaces, split out at `set_mesh`
    /// (pat-zwvgw), keyed by instance id.
    surface_meshes: HashMap<u64, Vec<Mesh3D>>,
    /// Cached CPU-rasterized shadow maps + their uploaded depth-array texture,
    /// keyed by a content hash of the shadow-relevant scene state (pat-2be0q).
    /// A static scene re-renders without deep-cloning every instance,
    /// re-rasterizing 256² maps, or re-uploading the atlas each frame. Survives
    /// `reset_scene` like the other GPU resource caches — the key is
    /// content-based, so the editor's rebuild-everything-per-frame flow still
    /// hits.
    shadow_atlas_cache: Option<ShadowAtlasCache>,
    /// How many times the shadow atlas was (re)built. Instrumentation for the
    /// cache-reuse tests; also a useful perf probe.
    shadow_atlas_rebuilds: u64,
    /// Shadow texels written to the GPU so far (full and partial uploads).
    shadow_upload_texels: u64,
    /// Per omni light, its last cubemap and the key it was rendered for (see
    /// `omni_cubemaps_cached`).
    omni_cube_cache: HashMap<u64, (u64, gdserver3d::light::ShadowCubemap)>,
    /// Mesh fingerprint -> local bounds, for the omni range test.
    omni_aabb_cache: HashMap<u64, Option<gdcore::math3d::Aabb>>,
    /// Shadow rebuilds that had to render an omni cubemap again.
    omni_cube_builds: u64,
    /// The static shadow casters, rasterized once and reused while the moving
    /// ones are redrawn each frame (pat-414k3).
    shadow_casters: crate::shadow_map::ShadowCasterCache,
    /// The uploaded reflection-probe cubemaps (pat-p3p1m), rebuilt only when a
    /// probe's capture changes (keyed by the captures' `Arc` identities), so a
    /// `Once` probe uploads once.
    probe_cube_cache: Option<ProbeCubeArray>,
    /// The sky pass's equirect panorama (pat-cezo4): (identity key, view,
    /// sampler), the key being the panorama's address so a persistent sky
    /// uploads once. Key 0 is the 1x1 black stand-in.
    sky_panorama: Option<(usize, wgpu::TextureView, wgpu::Sampler)>,
    /// Uploaded `BaseMaterial3D` maps for the editor path, keyed by the
    /// `TextureImage`'s `Arc` identity and the slot's format (pat-p3p1m). The
    /// editor path used to upload EVERY map of EVERY draw on EVERY frame.
    /// Each entry holds an `Arc` clone so the identity cannot be reused while
    /// cached; entries nobody else references any more are evicted per frame.
    material_tex_cache: HashMap<(usize, wgpu::TextureFormat), CachedMaterialTexture>,
    /// The 1x1 white stand-ins for an empty slot, one per format.
    white_tex_cache: HashMap<wgpu::TextureFormat, (wgpu::Texture, wgpu::TextureView)>,
    /// One material sampler per [`MaterialSamplerKey`], created once
    /// (pat-3u1wu: was a single bilinear sampler for every material).
    material_sampler_cache: Vec<wgpu::Sampler>,
    /// This frame's mesh-address -> `mesh_gpu_cache` key, filled by
    /// `prime_editor_draw_caches` so the draw loop hashes nothing.
    editor_frame_mesh_keys: HashMap<usize, u64>,
    /// How many mesh / material-map uploads the editor path has made.
    /// Instrumentation for the reuse tests (pat-p3p1m).
    editor_mesh_uploads: u64,
    editor_texture_uploads: u64,
    /// Draw calls the editor color path issued on its last frame, after
    /// frustum culling (pat-414k3). Instrumentation for the culling tests.
    editor_last_draws: usize,
    /// The linear HDR frame the readback decodes into, kept between frames.
    editor_hdr_scratch: Vec<Color>,
}

/// One uploaded material map (see `GpuRenderer3D::material_tex_cache`).
struct CachedMaterialTexture {
    image: std::sync::Arc<gdserver3d::material::TextureImage>,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Cached shadow state for [`GpuRenderer3D::shadow_atlas_cache`].
struct ShadowAtlasCache {
    /// Hash of the lights' shadow parameters + occluder geometry/transforms.
    key: u64,
    /// CPU shadow maps in atlas-layer order (drives `shadow_map_index`).
    maps: Vec<crate::shadow_map::ShadowMap>,
    /// The uploaded `R32Float` depth array (kept alive for `view`).
    _texture: wgpu::Texture,
    /// View bound as the shadow depth array in the light bind group.
    view: wgpu::TextureView,
    /// Omni cubemaps in packed order `(light id, cubemap)` — the k-th entry's
    /// faces occupy layers `6k..6k+6` of the omni array (pat-g4i60).
    omni: Vec<(u64, gdserver3d::light::ShadowCubemap)>,
    /// The uploaded omni face array (kept alive for `omni_view`).
    _omni_texture: wgpu::Texture,
    /// View bound as the omni shadow array (binding 2).
    omni_view: wgpu::TextureView,
}

/// Cached GPU buffers for one mesh's geometry (see `GpuRenderer3D::mesh_gpu_cache`).
struct CachedMeshBuffers {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    /// A sphere enclosing every vertex, in mesh space (pat-414k3): what the
    /// editor colour pass frustum-culls against.
    bounds: MeshBounds,
}

/// A mesh-space sphere enclosing every vertex of a mesh (pat-414k3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshBounds {
    /// The sphere's centre (the vertex AABB's centre).
    pub center: Vector3,
    /// The largest vertex distance from `center`.
    pub radius: f32,
}

impl MeshBounds {
    /// The enclosing sphere of `mesh`'s vertices.
    pub fn of(mesh: &Mesh3D) -> Self {
        let Some(first) = mesh.vertices.first() else {
            return Self { center: Vector3::ZERO, radius: 0.0 };
        };
        let (mut lo, mut hi) = (*first, *first);
        for v in &mesh.vertices {
            lo = Vector3::new(lo.x.min(v.x), lo.y.min(v.y), lo.z.min(v.z));
            hi = Vector3::new(hi.x.max(v.x), hi.y.max(v.y), hi.z.max(v.z));
        }
        let center = (lo + hi) * 0.5;
        let radius = mesh
            .vertices
            .iter()
            .map(|v| (*v - center).length())
            .fold(0.0f32, f32::max);
        Self { center, radius }
    }
}

/// The six planes of a view-projection's clip volume, in world space, each
/// normalised so `dot(n, p) + d` is a signed distance (pat-414k3).
///
/// Built only from tests a point must pass to be drawn under EITHER depth
/// convention (`-w <= x, y <= w`, `z <= w`, and `z >= -w`, which every point
/// with `z >= 0` also passes), so a sphere outside any one plane produces no
/// fragment at all — culling it leaves the frame bit-identical.
pub struct ClipPlanes([[f32; 4]; 6]);

impl ClipPlanes {
    /// Planes of the column-major `view_proj` (`clip = view_proj * world`).
    pub fn from_view_proj(m: &[[f32; 4]; 4]) -> Self {
        let row = |r: usize| [m[0][r], m[1][r], m[2][r], m[3][r]];
        let (x, y, z, w) = (row(0), row(1), row(2), row(3));
        let add = |a: [f32; 4], b: [f32; 4], s: f32| {
            [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2], a[3] + s * b[3]]
        };
        let mut planes = [
            add(w, x, 1.0),
            add(w, x, -1.0),
            add(w, y, 1.0),
            add(w, y, -1.0),
            add(w, z, 1.0),
            add(w, z, -1.0),
        ];
        for p in &mut planes {
            let len = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            if len > 0.0 {
                for c in p.iter_mut() {
                    *c /= len;
                }
            }
        }
        Self(planes)
    }

    /// `true` when `bounds` placed by `transform` lies wholly outside one
    /// plane. Conservative: the radius grows by the basis' Frobenius norm,
    /// which bounds any stretch the transform applies, plus a margin for the
    /// rounding of the plane maths.
    pub fn culls(&self, bounds: &MeshBounds, transform: &Transform3D) -> bool {
        let c = transform.xform(bounds.center);
        let b = &transform.basis;
        let stretch = (b.x.length_squared() + b.y.length_squared() + b.z.length_squared()).sqrt();
        let r = bounds.radius * stretch;
        if !(r.is_finite() && c.x.is_finite() && c.y.is_finite() && c.z.is_finite()) {
            return false;
        }
        let margin = r * 1e-3 + 1e-3;
        self.0
            .iter()
            .any(|p| p[0] * c.x + p[1] * c.y + p[2] * c.z + p[3] < -(r + margin))
    }
}

/// A resolved draw for the buffer-cached render path: references cached geometry
/// by `mesh_key` (into [`GpuRenderer3D::mesh_gpu_cache`]) and carries the per-draw
/// transform + material. Built by `build_draw_calls` (which uploads any uncached
/// geometry first), then consumed inside the render pass.
struct GpuDrawCall {
    mesh_key: u64,
    transform: Transform3D,
    albedo: Color,
    emission: Color,
    roughness: f32,
    metallic: f32,
    shading_mode: u32,
    /// `VisualInstance3D.layers` — the instance's render-layer bitmask, carried
    /// per draw so the shader can test it against each light's `cull_mask`.
    layers: u32,
    /// Face-cull mode for this draw (pat-s3rrv.30), carried the same way as the
    /// other flattened material fields above.
    cull_mode: crate::render_modes::CullMode,
    /// Transparency mode and its scissor threshold (pat-s3rrv.30).
    ///
    /// Used for the AlphaScissor decision below. With a flat albedo every texel
    /// of the surface carries the SAME alpha, so "discard sub-threshold texels"
    /// and "skip the draw" are the same thing — the per-draw test is exact here,
    /// not an approximation. Per-texel scissor for a *textured* albedo would
    /// need a threshold uniform and a `discard` in the WGSL, and is reported as
    /// remaining scope.
    transparency: crate::render_modes::Transparency,
    /// See [`Self::transparency`].
    alpha_scissor_threshold: f32,
    /// Godot `depth_draw_mode` (pat-s3rrv.30); decides whether this draw writes
    /// depth once its transparency is known.
    depth_draw_mode: crate::render_modes::DepthDrawMode,
    /// Godot `blend_mode` (pat-cezo4); `Add` picks the additive pipeline.
    blend_mode: gdserver3d::material::BlendMode,
    /// The group-3 uniform block for a custom shader: `time` first, then one
    /// `vec4` per buffer-backed uniform in `custom_uniform_fields` order
    /// (pat-s3rrv.122). Empty when this draw uses no custom module.
    custom_uniforms: Vec<[f32; 4]>,
    /// Images for this draw's `sampler2D` uniforms, in `custom_sampler_fields`
    /// order — slot N here is slot N in the emitted WGSL. Shorter than the
    /// layout's slot count when the shader declares fewer; the rest bind a dummy.
    custom_textures: Vec<(std::sync::Arc<gdserver3d::material::TextureImage>, bool)>,
    /// When `Some`, a complete transpiled WGSL module for this draw's spatial
    /// shader material whose `fragment()` body is evaluated per-fragment (e.g. a
    /// UV-driven ALBEDO). The render pass builds a per-material pipeline from it
    /// and binds that instead of the flat-albedo default pipeline. `None` for
    /// plain materials and for shaders whose body isn't GPU-transpilable (those
    /// keep the resolved flat albedo). See [`custom_spatial_module_wgsl`].
    custom_module: Option<String>,
    /// `BaseMaterial3D.albedo_texture` for this draw (pat-mgixk.52), carried
    /// the same owned-`Arc` way as `custom_textures` above. `None` binds the
    /// 1x1 white dummy, which the WGSL multiply treats as no texture at all.
    albedo_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// `BaseMaterial3D.normal_texture` and its strength (pat-mgixk.52).
    normal_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// See [`Self::normal_texture`].
    normal_scale: f32,
    /// `BaseMaterial3D` roughness / metallic / emission maps (pat-mgixk.52).
    roughness_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// See [`Self::roughness_texture`].
    metallic_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// See [`Self::roughness_texture`].
    emission_texture: Option<std::sync::Arc<gdserver3d::material::TextureImage>>,
    /// The material's sampler (pat-3u1wu).
    sampler: MaterialSamplerKey,
}

/// Max distinct meshes cached before the whole cache is dropped, bounding memory
/// for scenes whose geometry changes every frame.
const MESH_CACHE_CAP: usize = 2048;

/// A cheap content fingerprint of a mesh's geometry, keying
/// [`GpuRenderer3D::mesh_gpu_cache`]. Hashes vertex positions/normals/uvs + index
/// bytes, so meshes that pack to identical GPU vertex buffers share a cache entry.
/// A 64-bit collision between *distinct* geometry would draw the wrong mesh —
/// astronomically unlikely with lengths + full attribute bytes hashed, but not
/// harmless, so keep the full-content hash if this is ever "optimized".
fn mesh_content_hash(mesh: &Mesh3D) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_usize(mesh.vertices.len());
    h.write_usize(mesh.normals.len());
    h.write_usize(mesh.uvs.len());
    h.write_usize(mesh.indices.len());
    // The same attribute bits as before, fed in blocks: one `write` per few
    // KB instead of one per float. The per-call overhead was the cost — the
    // editor hashes every instance's mesh every frame (pat-414k3).
    let mut block = [0u32; 1024];
    let mut n = 0;
    let mut push = |h: &mut std::collections::hash_map::DefaultHasher, bits: u32| {
        block[n] = bits;
        n += 1;
        if n == block.len() {
            h.write(bytemuck::cast_slice::<u32, u8>(&block));
            n = 0;
        }
    };
    for v in mesh.vertices.iter().chain(&mesh.normals) {
        push(&mut h, v.x.to_bits());
        push(&mut h, v.y.to_bits());
        push(&mut h, v.z.to_bits());
    }
    for uv in &mesh.uvs {
        push(&mut h, uv[0].to_bits());
        push(&mut h, uv[1].to_bits());
    }
    drop(push);
    h.write(bytemuck::cast_slice::<u32, u8>(&block[..n]));
    h.write(bytemuck::cast_slice::<u32, u8>(&mesh.indices));
    h.finish()
}

/// Cached GPU render targets for the editor color path. Held across frames and
/// rebuilt only when the viewport size changes.
struct EditorTargets {
    /// Cached width the targets were built for.
    width: u32,
    /// Cached height the targets were built for.
    height: u32,
    /// Color render target (also a copy source for readback).
    color_tex: wgpu::Texture,
    /// View of `color_tex` used as the render-pass color attachment.
    color_view: wgpu::TextureView,
    /// Depth attachment view (used for z-testing; never read back).
    /// Kept alongside the view so the editor path can copy depth back to the
    /// CPU for the decal/fog-volume passes (pat-s3rrv.29) — a view cannot be
    /// the source of `copy_texture_to_buffer`.
    depth_tex: wgpu::Texture,
    depth_view: wgpu::TextureView,
    /// Persistent mappable buffer the color texture is copied into each frame.
    readback_buf: wgpu::Buffer,
    /// 256-aligned bytes-per-row of `readback_buf`.
    padded_row: u32,
}

impl EditorTargets {
    /// Builds fresh color + depth targets and a readback buffer sized `w`×`h`.
    fn new(device: &wgpu::Device, w: u32, h: u32, color_format: wgpu::TextureFormat) -> Self {
        let color_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("editor-color-target"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("editor-depth-target"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            // COPY_SRC so the editor path can read depth back to the CPU
            // (pat-s3rrv.29). The decal and fog-volume passes are per-pixel
            // reprojections and need the depth buffer; without this flag the
            // copy is a validation error, so the effects could not run on the
            // editor viewport at all.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (w * bytes_per_pixel(color_format) + align - 1) / align * align;
        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("editor-color-readback"),
            size: padded_row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self {
            width: w,
            height: h,
            color_view,
            color_tex,
            depth_tex,
            depth_view,
            readback_buf,
            padded_row,
        }
    }
}

impl std::fmt::Debug for GpuRenderer3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuRenderer3D")
            .field("adapter", &self.ctx.adapter_name())
            .field("instances", &self.instances.len())
            .field("lights", &self.lights.len())
            .finish()
    }
}

impl GpuRenderer3D {
    /// Creates a new GPU renderer.
    ///
    /// Returns `None` if no GPU adapter is available.
    pub fn new() -> Option<Self> {
        let ctx = GpuContext3D::new()?;
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let pipeline = RenderPipeline3D::new(&ctx.device, format);
        Some(Self {
            ctx,
            pipeline,
            instances: HashMap::new(),
            lights: Vec::new(),
            reflection_probes: Vec::new(),
            voxel_gis: Vec::new(),
            decals: Vec::new(),
            fog_volumes: Vec::new(),
            occluders: Vec::new(),
            next_id: 1,
            target_format: format,
            editor_pipeline: None,
            editor_variants: Vec::new(),
            editor_add_variants: Vec::new(),
            editor_pipe_cache: HashMap::new(),
            editor_pipe_modules: Vec::new(),
            custom_tex_cache: HashMap::new(),
            custom_unbound_cache: HashMap::new(),
            custom_sampler_cache: HashMap::new(),
            shader_time: 0.0,
            editor_sky_pipeline: None,
            editor_targets: None,
            mesh_gpu_cache: HashMap::new(),
            surface_meshes: HashMap::new(),
            shadow_atlas_cache: None,
            shadow_atlas_rebuilds: 0,
            shadow_upload_texels: 0,
            omni_cube_cache: HashMap::new(),
            omni_aabb_cache: HashMap::new(),
            omni_cube_builds: 0,
            shadow_casters: Default::default(),
            probe_cube_cache: None,
            sky_panorama: None,
            material_tex_cache: HashMap::new(),
            white_tex_cache: HashMap::new(),
            material_sampler_cache: Vec::new(),
            editor_frame_mesh_keys: HashMap::new(),
            editor_mesh_uploads: 0,
            editor_texture_uploads: 0,
            editor_last_draws: 0,
            editor_hdr_scratch: Vec::new(),
        })
    }

    /// Clears all instances, lights, and probes so the persistent renderer can be
    /// repopulated for a fresh frame, while keeping the expensive-to-create GPU
    /// state (device, pipelines, cached targets) alive. The editor rebuilds its
    /// render list every frame, so its per-frame render calls this first.
    /// Sets the `TIME` custom spatial shaders read (pat-9gjpr) — the scene
    /// tree's shader clock, as the software renderer's `set_shader_time`.
    pub fn set_shader_time(&mut self, seconds: f32) {
        self.shader_time = seconds;
    }

    /// The pipeline keys `render_editor_color_bgra` built so far (pat-9gjpr),
    /// for tests that assert which state a draw asked for.
    pub fn editor_pipeline_keys(&self) -> Vec<EditorPipelineKey> {
        self.editor_pipe_cache.keys().copied().collect()
    }

    pub fn reset_scene(&mut self) {
        self.instances.clear();
        self.surface_meshes.clear();
        self.lights.clear();
        self.reflection_probes.clear();
        // Same reason: a stale GI volume would bleed the previous scene's
        // colour into the next Play run.
        self.voxel_gis.clear();
        // pat-s3rrv.29: env effects are per-scene like everything else here.
        // Missing these would leak a previous scene's decals and fog into the
        // next Play run — the editor reuses one renderer across scenes.
        self.decals.clear();
        self.fog_volumes.clear();
        self.occluders.clear();
        self.next_id = 1;
    }

    /// How many times the shadow atlas has been (re)rasterized + uploaded.
    /// A static scene should hold this constant across frames; it increments
    /// only when a light or occluder actually changes (pat-2be0q).
    pub fn shadow_atlas_rebuilds(&self) -> u64 {
        self.shadow_atlas_rebuilds
    }

    /// Shadow-map texels uploaded to the GPU so far. A rebuild where only a
    /// few casters moved over an unchanged base uploads just their texel
    /// rectangles, not the whole atlas.
    pub fn shadow_upload_texels(&self) -> u64 {
        self.shadow_upload_texels
    }

    /// Shadow rebuilds that rendered an omni light's cubemap again. A caster
    /// moving outside every omni light's range renders none.
    pub fn omni_cube_builds(&self) -> u64 {
        self.omni_cube_builds
    }

    /// `(meshes, material maps)` uploaded by the editor color path so far
    /// (pat-p3p1m). A static scene re-rendered frame after frame uploads each
    /// mesh and each map once.
    pub fn editor_upload_counts(&self) -> (u64, u64) {
        (self.editor_mesh_uploads, self.editor_texture_uploads)
    }

    /// Draw calls the editor color path issued on its last frame — what was
    /// left after frustum culling (pat-414k3).
    pub fn editor_last_draw_count(&self) -> usize {
        self.editor_last_draws
    }

    /// Ensures `shadow_atlas_cache` matches the current shadow-relevant scene
    /// state and returns this frame's light uniforms (pat-2be0q). On a cache
    /// hit — the common case for a static scene — no instance is cloned, no
    /// shadow map is rasterized, and no texture is uploaded; only the cheap
    /// uniform packing runs. Call with `&mut self` before the render pass
    /// takes its immutable device/target borrows; read the atlas view from
    /// `shadow_atlas_cache` afterwards.
    /// Uploads, once, every mesh and material map this frame's editor draws
    /// need, so the draw loop only looks them up (pat-p3p1m). Before this the
    /// editor path built a vertex buffer, an index buffer, five textures and a
    /// sampler for every draw of every frame.
    fn prime_editor_draw_caches(&mut self) {
        if self.mesh_gpu_cache.len() > MESH_CACHE_CAP {
            self.mesh_gpu_cache.clear();
        }
        // Evict maps only the cache still references: their material is gone.
        self.material_tex_cache
            .retain(|_, c| std::sync::Arc::strong_count(&c.image) > 1);
        if self.material_sampler_cache.is_empty() {
            self.material_sampler_cache = material_sampler_keys()
                .map(|k| material_sampler(&self.ctx.device, k))
                .collect();
        }
        for format in [MATERIAL_COLOR_FORMAT, MATERIAL_RAW_FORMAT] {
            if !self.white_tex_cache.contains_key(&format) {
                let t = white_material_texture(&self.ctx.device, &self.ctx.queue, format);
                let v = t.create_view(&wgpu::TextureViewDescriptor::default());
                self.white_tex_cache.insert(format, (t, v));
            }
        }
        self.editor_frame_mesh_keys.clear();
        let mut mesh_uploads: Vec<(u64, Mesh3D)> = Vec::new();
        let mut tex_uploads: Vec<(
            (usize, wgpu::TextureFormat),
            std::sync::Arc<gdserver3d::material::TextureImage>,
        )> = Vec::new();
        for inst in self.instances.values() {
            if !inst.visible {
                continue;
            }
            for entry in instance_draw_entries(inst, self.surface_meshes.get(&inst.id.0).map_or(&[][..], Vec::as_slice)) {
                let addr = entry.mesh as *const Mesh3D as usize;
                if !self.editor_frame_mesh_keys.contains_key(&addr) {
                    let key = mesh_content_hash(entry.mesh);
                    self.editor_frame_mesh_keys.insert(addr, key);
                    if !self.mesh_gpu_cache.contains_key(&key)
                        && !mesh_uploads.iter().any(|(k, _)| *k == key)
                    {
                        mesh_uploads.push((key, entry.mesh.clone()));
                    }
                }
                for (img, format) in [
                    (&entry.albedo_texture, MATERIAL_COLOR_FORMAT),
                    (&entry.normal_texture, MATERIAL_RAW_FORMAT),
                    (&entry.roughness_texture, MATERIAL_RAW_FORMAT),
                    (&entry.metallic_texture, MATERIAL_RAW_FORMAT),
                    (&entry.emission_texture, MATERIAL_COLOR_FORMAT),
                ] {
                    if let Some(img) = img {
                        let key = (std::sync::Arc::as_ptr(img) as usize, format);
                        if !self.material_tex_cache.contains_key(&key)
                            && !tex_uploads.iter().any(|(k, _)| *k == key)
                        {
                            tex_uploads.push((key, img.clone()));
                        }
                    }
                }
            }
        }
        for (key, mesh) in mesh_uploads {
            let vertices = mesh_to_gpu_vertices(&mesh);
            if vertices.is_empty() {
                continue;
            }
            let vertex_buf = create_buffer_init(
                &self.ctx.device,
                &self.ctx.queue,
                "editor-cached-vertex",
                bytemuck::cast_slice(&vertices),
                wgpu::BufferUsages::VERTEX,
            );
            let index_buf = create_buffer_init(
                &self.ctx.device,
                &self.ctx.queue,
                "editor-cached-index",
                bytemuck::cast_slice(&mesh.indices),
                wgpu::BufferUsages::INDEX,
            );
            self.mesh_gpu_cache.insert(
                key,
                CachedMeshBuffers {
                    vertex_buf,
                    index_buf,
                    index_count: mesh.indices.len() as u32,
                    bounds: MeshBounds::of(&mesh),
                },
            );
            self.editor_mesh_uploads += 1;
        }
        for (key, image) in tex_uploads {
            self.editor_texture_uploads += 1;
            let texture = upload_material_texture(&self.ctx.device, &self.ctx.queue, &image, key.1);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.material_tex_cache
                .insert(key, CachedMaterialTexture { image, _texture: texture, view });
        }
    }

    /// Uploads the probe cubemaps unless the cached upload already holds
    /// exactly these captures (pat-p3p1m).
    /// Uploads the environment's sky panorama for the sky pass, once per
    /// distinct panorama (pat-cezo4): a `PanoramaSkyMaterial`'s texture or a
    /// cubemap shader sky's CPU bake. A 1x1 black texel otherwise.
    fn ensure_sky_panorama(&mut self, env: Option<&Environment3D>) {
        let pano = env.and_then(|e| e.sky.as_ref()).and_then(|sky| match &sky.material {
            SkyMaterial::Panoramic(m) => m.texture.as_ref(),
            SkyMaterial::Shader(m) => m.baked.as_deref(),
            _ => None,
        });
        let key = pano.map_or(0, |p| p.pixels.as_ptr() as usize);
        if self.sky_panorama.as_ref().map(|c| c.0) == Some(key) {
            return;
        }
        let black = gdserver3d::sky::PanoramaTexture {
            width: 1,
            height: 1,
            pixels: vec![Color::new(0.0, 0.0, 0.0, 1.0)],
        };
        let pano = pano.unwrap_or(&black);
        let (w, h) = (pano.width.max(1), pano.height.max(1));
        // Linear texels, stored sRGB-encoded in an sRGB format so the sampler
        // filters in linear space and 8 bits cover the LDR range evenly.
        let mut bytes = Vec::with_capacity((w * h * 4) as usize);
        for c in &pano.pixels {
            let e = c.linear_to_srgb();
            for ch in [e.r, e.g, e.b] {
                bytes.push((ch.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            bytes.push(255);
        }
        bytes.resize((w * h * 4) as usize, 0);
        let device = &self.ctx.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky-panorama"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky-panorama-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.sky_panorama = Some((key, view, sampler));
    }

    /// Uploads the frame's probe captures and the environment's sky radiance
    /// map (pat-h7sz7) when either changed.
    fn ensure_probe_cubes(&mut self, env: Option<&Environment3D>) {
        let sky = env.and_then(|e| {
            crate::renderer::sky_radiance_for(
                e,
                &crate::renderer::sky_suns_from_lights(&self.lights),
                crate::renderer::SKY_RADIANCE_RESOLUTION,
            )
        });
        let key = probe_cube_key(&self.reflection_probes, sky.as_ref());
        if self.probe_cube_cache.as_ref().map(|c| &c.key) != Some(&key) {
            let cubes = upload_probe_cube_array(
                &self.ctx.device,
                &self.ctx.queue,
                &self.reflection_probes,
                sky.as_ref(),
            );
            self.probe_cube_cache = Some(cubes);
        }
    }

    fn ensure_shadow_atlas(&mut self, viewport: &Viewport3D) -> LightArrayData {
        self.ensure_shadow_atlas_with(viewport, None)
    }

    /// Hands every directional light the camera its shadow map is fitted to
    /// (pat-772py), as Godot fits the map to the frustum of the camera being
    /// drawn.
    fn fit_directional_shadows_to(&mut self, viewport: &Viewport3D) {
        let camera = gdserver3d::light::ShadowCamera {
            transform: viewport.camera_transform,
            fov_y: viewport.fov,
            aspect: viewport.aspect(),
            near: viewport.near,
            far: viewport.far,
        };
        for l in self.lights.iter_mut() {
            if l.light_type == LightType::Directional {
                l.shadow_camera = Some(camera);
            }
        }
    }

    /// [`Self::ensure_shadow_atlas`], looking each instance's mesh fingerprint
    /// up in `mesh_keys` (mesh address -> `mesh_content_hash`) first.
    ///
    /// ⚠️ `mesh_keys` must have been filled from THIS frame's instances (as
    /// `prime_editor_draw_caches` just did): an address from an earlier frame
    /// can belong to a different mesh by now.
    fn ensure_shadow_atlas_with(
        &mut self,
        viewport: &Viewport3D,
        mesh_keys: Option<&HashMap<usize, u64>>,
    ) -> LightArrayData {
        self.fit_directional_shadows_to(viewport);
        let camera = viewport.camera_transform.origin;
        // In id order, so the key and the caster list are the same every frame
        // for the same scene.
        let mut ordered: Vec<(&u64, &Instance3D)> = self.instances.iter().collect();
        ordered.sort_unstable_by_key(|(id, _)| **id);
        let (light_sig, any_shadow) = shadow_light_signature(&self.lights);
        let mut mesh_hashes: Vec<Option<u64>> = Vec::new();
        let caster_keys: Vec<u64> = if any_shadow {
            ordered
                .iter()
                .map(|(_, inst)| {
                    let mesh_hash = inst.mesh.as_ref().map(|m| {
                        mesh_keys
                            .and_then(|k| k.get(&(m as *const Mesh3D as usize)).copied())
                            .unwrap_or_else(|| mesh_content_hash(m))
                    });
                    mesh_hashes.push(mesh_hash);
                    shadow_caster_key_with(inst, mesh_hash)
                })
                .collect()
        } else {
            Vec::new()
        };
        let key = combine_shadow_key(light_sig, any_shadow, &caster_keys);
        if self.shadow_atlas_cache.as_ref().map(|c| c.key) != Some(key) {
            // Miss: only the casters that changed since the last frame are
            // rasterized again (pat-414k3) — a running game moves a handful of
            // bodies over thousands of static tiles.
            let casters: Vec<&Instance3D> = if any_shadow {
                ordered.iter().map(|(_, inst)| *inst).collect()
            } else {
                Vec::new()
            };
            // The last maps go back to the generator to be patched in place
            // (it checks they are its own); their sizes are kept for the
            // partial-upload test below.
            let prev_dims: Vec<(u32, u32)> = self
                .shadow_atlas_cache
                .as_ref()
                .map(|c| c.maps.iter().map(|m| m.depth.dimensions()).collect())
                .unwrap_or_default();
            let previous = self
                .shadow_atlas_cache
                .as_mut()
                .map(|c| std::mem::take(&mut c.maps))
                .unwrap_or_default();
            let maps = crate::shadow_map::generate_shadow_maps_incremental_reusing(
                &self.lights,
                &casters,
                &caster_keys,
                light_sig,
                &mut self.shadow_casters,
                previous,
            );
            // The omni cubemaps still take owned instances; clone them only
            // when a point light actually casts.
            // 🔑 Each omni cubemap is cached on what it can see: the light,
            // and the casters within its range (a caster wholly beyond the
            // range draws nothing into it). truck_town's street lamps were
            // re-rasterized — six faces over the whole town each — on every
            // frame the car moved anywhere.
            let (omni, omni_changed) = omni_cubemaps_cached(
                &self.lights,
                &casters,
                &caster_keys,
                &mesh_hashes,
                &mut self.omni_cube_cache,
                &mut self.omni_aabb_cache,
            );
            self.omni_cube_builds += omni_changed as u64;
            // 🔑 Over an unchanged base only the casters drawn on top of it
            // can change texels, so a frame where the coins spin rewrites
            // their rectangles in the texture already bound instead of
            // re-creating and re-uploading the whole 2048² f32 atlas (16 MB)
            // every frame. The texels are the same either way.
            let partial = match (&self.shadow_atlas_cache, &self.shadow_casters.last_dirty) {
                (Some(_), Some(dirty)) => {
                    dirty.len() == maps.len()
                        && prev_dims.len() == maps.len()
                        && prev_dims.iter().zip(&maps).all(|(a, b)| *a == b.depth.dimensions())
                }
                _ => false,
            };
            if partial {
                let dirty = self.shadow_casters.last_dirty.as_ref().expect("checked above");
                let cache = self.shadow_atlas_cache.as_mut().expect("checked above");
                self.shadow_upload_texels +=
                    write_shadow_rects(&self.ctx.queue, &cache._texture, &maps, dirty);
                cache.key = key;
                cache.maps = maps;
                if omni_changed {
                    let (omni_texture, omni_view) =
                        upload_omni_shadow_array(&self.ctx.device, &self.ctx.queue, &omni);
                    cache.omni = omni;
                    cache._omni_texture = omni_texture;
                    cache.omni_view = omni_view;
                }
            } else {
                let (texture, view) =
                    upload_shadow_depth_array(&self.ctx.device, &self.ctx.queue, &maps);
                let size = texture.size();
                self.shadow_upload_texels +=
                    size.width as u64 * size.height as u64 * size.depth_or_array_layers as u64;
                let (omni_texture, omni_view) =
                    upload_omni_shadow_array(&self.ctx.device, &self.ctx.queue, &omni);
                self.shadow_atlas_cache = Some(ShadowAtlasCache {
                    key,
                    maps,
                    _texture: texture,
                    view,
                    omni,
                    _omni_texture: omni_texture,
                    omni_view,
                });
            }
            self.shadow_atlas_rebuilds += 1;
        }
        let cache = self.shadow_atlas_cache.as_ref().expect("just ensured");
        pack_lights_from_maps(&self.lights, &cache.maps, &cache.omni, camera)
    }

    /// Phase 1 of a cached render (pat-ism62 #2b): expands every visible instance
    /// into concrete [`GpuDrawCall`]s (multimesh + shader material via
    /// [`instance_draw_entries`]) and uploads any geometry not already in
    /// `mesh_gpu_cache`, so the render pass reuses cached vertex/index buffers
    /// instead of re-uploading static geometry every frame. Runs with `&mut self`
    /// before the immutable device/target borrows the render pass needs.
    fn build_draw_calls(&mut self) -> Vec<GpuDrawCall> {
        // Bound memory for scenes whose geometry changes every frame.
        if self.mesh_gpu_cache.len() > MESH_CACHE_CAP {
            self.mesh_gpu_cache.clear();
        }
        let mut calls: Vec<GpuDrawCall> = Vec::new();
        // Collect misses (key → mesh clone) without holding the `instances` borrow
        // across the `&mut mesh_gpu_cache` insert below.
        let mut to_upload: Vec<(u64, Mesh3D)> = Vec::new();
        for inst in self.instances.values() {
            if !inst.visible {
                continue;
            }
            // Transpile a per-fragment spatial shader body to a complete WGSL
            // module once per instance (shared by all of its expanded draws), so
            // UV/varying-driven shaders render on the GPU instead of collapsing
            // to a flat albedo. `None` keeps the shared flat-albedo pipeline.
            let custom_shader = inst.shader_material.as_ref().and_then(|mat| {
                let compiler = gdserver3d::ShaderCompiler3D::new();
                let compiled = compiler.compile_material(mat)?;
                let module = custom_spatial_module_wgsl(&compiled)?;
                // Packed by walking the SAME filter `custom_uniform_fields`
                // uses, so declaration order and packing order cannot disagree.
                // `time` leads, matching the struct, and guarantees the buffer
                // is never zero-sized even for a uniform-free shader.
                let mut values: Vec<[f32; 4]> = vec![[0.0; 4]];
                for u in compiled
                    .uniforms
                    .iter()
                    .filter(|u| u.uniform_type != gdserver3d::shader::UniformType::Sampler2D)
                {
                    // The material's `shader_parameter/<name>` wins; the
                    // shader's own declared default is the fallback.
                    let v = mat
                        .get_shader_parameter(&u.name)
                        .cloned()
                        .unwrap_or_else(|| u.default_value.clone());
                    values.push(uniform_variant_to_vec4(&v));
                }
                // Same walk as `custom_sampler_fields`, so slot N is the same
                // sampler in the module, the bind group and here. A material
                // that never bound an image simply yields a shorter list and
                // the tail falls back to the dummy texture.
                let mut images = Vec::new();
                for u in compiled
                    .uniforms
                    .iter()
                    .filter(|u| u.uniform_type == gdserver3d::shader::UniformType::Sampler2D)
                    .take(CUSTOM_SAMPLER_SLOTS as usize)
                {
                    match mat.get_texture(&u.name) {
                        Some(img) => images.push((img.clone(), u.is_source_color())),
                        // Stop at the first unbound sampler: pushing nothing
                        // would shift every later image onto the wrong slot.
                        None => break,
                    }
                }
                Some((module, values, images))
            });
            let custom_module = custom_shader.as_ref().map(|(m, _, _)| m.clone());
            let custom_uniform_values: Vec<[f32; 4]> = custom_shader
                .as_ref()
                .map(|(_, v, _)| v.clone())
                .unwrap_or_default();
            let custom_texture_images: Vec<(
                std::sync::Arc<gdserver3d::material::TextureImage>,
                bool,
            )> =
                custom_shader
                    .as_ref()
                    .map(|(_, _, t)| t.clone())
                    .unwrap_or_default();
            for entry in instance_draw_entries(inst, self.surface_meshes.get(&inst.id.0).map_or(&[][..], Vec::as_slice)) {
                if entry.mesh.indices.is_empty() {
                    continue;
                }
                let key = mesh_content_hash(entry.mesh);
                if !self.mesh_gpu_cache.contains_key(&key)
                    && !to_upload.iter().any(|(k, _)| *k == key)
                {
                    to_upload.push((key, entry.mesh.clone()));
                }
                calls.push(GpuDrawCall {
                    mesh_key: key,
                    transform: entry.transform,
                    albedo: entry.albedo,
                    emission: entry.emission,
                    roughness: entry.roughness,
                    metallic: entry.metallic,
                    shading_mode: entry.shading_mode,
                    layers: inst.geometry.layers,
                    cull_mode: entry.cull_mode,
                    transparency: entry.transparency,
                    alpha_scissor_threshold: entry.alpha_scissor_threshold,
                    depth_draw_mode: entry.depth_draw_mode,
                    blend_mode: entry.blend_mode,
                    custom_module: custom_module.clone(),
                    custom_uniforms: custom_uniform_values.clone(),
                    custom_textures: custom_texture_images.clone(),
                    albedo_texture: entry.albedo_texture.clone(),
                    normal_texture: entry.normal_texture.clone(),
                    normal_scale: entry.normal_scale,
                    roughness_texture: entry.roughness_texture.clone(),
                    metallic_texture: entry.metallic_texture.clone(),
                    emission_texture: entry.emission_texture.clone(),
                    sampler: entry.sampler,
                });
            }
        }
        for (key, mesh) in to_upload {
            let vertices = mesh_to_gpu_vertices(&mesh);
            if vertices.is_empty() {
                continue;
            }
            let vertex_buf = create_buffer_init(
                &self.ctx.device,
                &self.ctx.queue,
                "cached-vertex",
                bytemuck::cast_slice(&vertices),
                wgpu::BufferUsages::VERTEX,
            );
            let index_buf = create_buffer_init(
                &self.ctx.device,
                &self.ctx.queue,
                "cached-index",
                bytemuck::cast_slice(&mesh.indices),
                wgpu::BufferUsages::INDEX,
            );
            self.mesh_gpu_cache.insert(
                key,
                CachedMeshBuffers {
                    vertex_buf,
                    index_buf,
                    index_count: mesh.indices.len() as u32,
                    bounds: MeshBounds::of(&mesh),
                },
            );
        }
        calls
    }

    /// Editor-optimized render: rasterizes the scene's meshes into a **cached**,
    /// non-sRGB color target and reads back only the color buffer (clearing to
    /// `clear`), returning tight **top-down BGRA bytes** `(width, height, data)`.
    /// Tuned for per-frame use at 60fps, it differs from the trait
    /// [`render_frame`](RenderingServer3D::render_frame) in four ways:
    ///
    /// - **Cached targets**: the color texture, depth texture, and readback buffer
    ///   are reused across frames and rebuilt only on resize, instead of being
    ///   allocated every frame.
    /// - **No depth readback**: depth is still used for z-testing but is never
    ///   copied back, eliminating a second texture copy and a second blocking GPU
    ///   sync per frame (the editor draws its own overlays and never reads GPU
    ///   depth).
    /// - **Linear target** (`Bgra8Unorm`): shader output is written to bytes
    ///   without sRGB encoding, so the bytes match the software rasterizer's
    ///   `color * 255` exactly and GPU meshes stay consistent with the software
    ///   overlays/background composited around them.
    /// - **Byte-native output**: returns raw BGRA (de-padded per row, no per-pixel
    ///   float conversion), so the caller can composite overlays and encode BMP
    ///   without the `bytes → Color → bytes` round-trip that dominates a full-res
    ///   frame.
    pub fn render_editor_color_bgra(
        &mut self,
        viewport: &Viewport3D,
        clear: Color,
    ) -> (u32, u32, Vec<u8>) {
        let w = viewport.width.max(1);
        let h = viewport.height.max(1);
        // The editor scene is rendered to a linear **HDR** target so the sky's
        // bright sun and any emissive materials keep values above 1.0 for the
        // camera post-process (glow + tonemap) to work on. The post-process
        // (shared with the software path) collapses it to display sRGB on
        // readback, so the two backends produce identical images.
        let format = wgpu::TextureFormat::Rgba16Float;

        // Lazily build the editor HDR pipelines and (re)build cached targets on
        // first use or resize. Done before borrowing device/queue immutably.
        if self.editor_pipeline.is_none() {
            self.editor_pipeline = Some(RenderPipeline3D::new(&self.ctx.device, format));
        }
        // pat-s3rrv.30: build every (cull, depth-write) pipeline variant once.
        // Both are pipeline state on the GPU, so a material that culls FRONT or
        // an alpha-blended one that must not write depth cannot be served by
        // flipping a uniform — it needs its own pipeline.
        if self.editor_variants.is_empty() {
            let base = self
                .editor_pipeline
                .as_ref()
                .expect("editor pipeline just ensured");
            let mut built = Vec::new();
            for cull in [
                crate::render_modes::CullMode::Back,
                crate::render_modes::CullMode::Front,
                crate::render_modes::CullMode::Disabled,
            ] {
                for depth_write in [true, false] {
                    let face = RenderPipeline3D::wgpu_cull_face(cull);
                    let p = base.build_cull_variant(
                        &self.ctx.device,
                        format,
                        face,
                        depth_write,
                        wgpu::BlendState::ALPHA_BLENDING,
                    );
                    built.push((cull, depth_write, p));
                }
                // pat-cezo4: BLEND_MODE_ADD (`src * alpha + dst`), never writing
                // depth — Godot's additive materials are transparent.
                let face = RenderPipeline3D::wgpu_cull_face(cull);
                let add = base.build_cull_variant(
                    &self.ctx.device,
                    format,
                    face,
                    false,
                    ADDITIVE_BLEND,
                );
                self.editor_add_variants.push((cull, add));
            }
            self.editor_variants = built;
        }
        if self.editor_sky_pipeline.is_none() {
            self.editor_sky_pipeline = Some(SkyPipeline3D::new(&self.ctx.device, format));
        }
        let need_new = match &self.editor_targets {
            Some(t) => t.width != w || t.height != h,
            None => true,
        };
        if need_new {
            self.editor_targets = Some(EditorTargets::new(&self.ctx.device, w, h, format));
        }

        // Shadow atlas + light uniforms (pat-32edz / pat-2be0q) — needs
        // `&mut self`, so it runs before the immutable borrows below; a
        // static scene reuses the cached maps + texture untouched.
        // Prime first: it fingerprints every mesh of this frame once, and the
        // shadow atlas key reuses those fingerprints instead of hashing every
        // instance's geometry a second time (pat-414k3).
        self.prime_editor_draw_caches();
        let mesh_keys = std::mem::take(&mut self.editor_frame_mesh_keys);
        let light_data =
            self.ensure_shadow_atlas_with(viewport, Some(&mesh_keys));
        self.editor_frame_mesh_keys = mesh_keys;
        self.ensure_probe_cubes(viewport.environment.as_ref());
        self.ensure_sky_panorama(viewport.environment.as_ref());

        // pat-9gjpr: the keyed pipeline / custom-shader caches are filled while
        // the loop below holds `&self`, so they are moved out for the frame and
        // put back after the pass.
        let mut pipe_cache = std::mem::take(&mut self.editor_pipe_cache);
        let mut pipe_modules = std::mem::take(&mut self.editor_pipe_modules);
        let mut custom_tex_cache = std::mem::take(&mut self.custom_tex_cache);
        let mut custom_unbound_cache = std::mem::take(&mut self.custom_unbound_cache);
        let mut custom_sampler_cache = std::mem::take(&mut self.custom_sampler_cache);
        let shader_time = self.shader_time;

        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        let pipeline = self.editor_pipeline.as_ref().unwrap();
        let sky_pipeline = self.editor_sky_pipeline.as_ref().unwrap();
        let targets = self.editor_targets.as_ref().unwrap();

        // Resolve the background from the viewport environment the way the
        // software rasterizer's `fill_background` does: a procedural sky draws
        // a fullscreen gradient pass behind the meshes, a custom color clears
        // to it, and anything else clears to the caller-provided backdrop.
        let sky_suns = crate::renderer::sky_suns_from_lights(&self.lights);
        let sky_panorama_bindings = (
            &self.sky_panorama.as_ref().expect("ensured above").1,
            &self.sky_panorama.as_ref().expect("ensured above").2,
        );
        let sky_bind = viewport
            .environment
            .as_ref()
            .and_then(|env| sky_uniform_for_env(env, viewport, w, h, &sky_suns))
            .map(|data| {
                let buf = create_buffer_init(
                    device,
                    queue,
                    "editor-sky-uniform",
                    bytemuck::bytes_of(&data),
                    wgpu::BufferUsages::UNIFORM,
                );
                let (pano_view, pano_sampler) = sky_panorama_bindings;
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("editor-sky-bind-group"),
                    layout: &sky_pipeline.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(pano_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(pano_sampler),
                        },
                    ],
                });
                (buf, bg)
            });
        // `clear` stays the display-sRGB backdrop (used for the device-lost
        // fallback); `clear_linear` is what actually clears the linear HDR
        // target, so after the post-process's linear→sRGB encode the flat
        // backdrop comes back out at its authored sRGB value.
        let clear = match viewport.environment.as_ref() {
            Some(env) if env.background_mode == BackgroundMode::CustomColor => {
                env.background_color
            }
            _ => clear,
        };
        let clear_linear = clear.srgb_to_linear();

        // Camera uniforms.
        let view_matrix = transform_to_view_matrix(&viewport.camera_transform);
        let projection_matrix = perspective_projection_matrix(
            viewport.fov,
            viewport.aspect(),
            viewport.near,
            viewport.far,
        );
        let cam_pos = viewport.camera_transform.origin;
        // Environment ambient (linear), so the GPU shades surfaces with the
        // sky's tint like the software path.
        let ambient = viewport
            .environment
            .as_ref()
            .map(crate::renderer::environment_ambient_color)
            .unwrap_or(Color::new(0.1, 0.1, 0.1, 1.0));
        let camera_data = CameraUniformData {
            view_matrix,
            projection_matrix,
            camera_position: [cam_pos.x, cam_pos.y, cam_pos.z],
            _pad0: 0.0,
            ambient: [ambient.r, ambient.g, ambient.b, 1.0],
        };
        let camera_buf = create_buffer_init(
            device,
            queue,
            "editor-camera-uniform",
            bytemuck::bytes_of(&camera_data),
            wgpu::BufferUsages::UNIFORM,
        );
        let camera_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("editor-camera-bind-group"),
            layout: &pipeline.camera_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: camera_buf.as_entire_binding() }],
        });

        // Light bind group over the cached shadow atlases (pat-2be0q).
        let atlas = self
            .shadow_atlas_cache
            .as_ref()
            .expect("ensure_shadow_atlas ran above");
        let probe_cubes = self
            .probe_cube_cache
            .as_ref()
            .expect("ensure_probe_cubes ran above");
        let (light_bg, _light_buf) = create_light_bind_group(
            device,
            queue,
            &pipeline.light_layout,
            &light_data,
            &atlas.view,
            &atlas.omni_view,
            probe_cubes,
            "editor-light-bind-group",
        );

        // pat-414k3: resolve every draw BEFORE recording, so the per-draw model
        // uniforms go up in ONE buffer instead of one buffer allocation and one
        // queue write per draw — with a few thousand GridMap cells, buffer
        // creation, staging and release were over half the frame.
        struct EditorDraw<'a> {
            /// Owned: a `wgpu::RenderPipeline` is a refcounted handle, and the
            /// keyed cache it comes from is mutated during this loop.
            pipeline: wgpu::RenderPipeline,
            /// Group 3 for a custom spatial shader draw (pat-9gjpr).
            custom_bg: Option<wgpu::BindGroup>,
            maps: CachedSlotViews<'a>,
            mesh: &'a CachedMeshBuffers,
            sampler: MaterialSamplerKey,
            /// This draw's block in `model_bytes`, fixed before sorting.
            uniform_index: usize,
            /// Camera distance when the draw blends, `None` when opaque.
            blend_depth: Option<f32>,
        }
        let clip_planes = ClipPlanes::from_view_proj(&mul_mat4(&projection_matrix, &view_matrix));
        let mut draws: Vec<EditorDraw<'_>> = Vec::new();
        let mut model_bytes: Vec<u8> = Vec::new();
        let mut draw_count = 0;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("editor-3d-encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("editor-3d-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_linear.r as f64,
                            g: clear_linear.g as f64,
                            b: clear_linear.b as f64,
                            a: clear_linear.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            // Sky background first (depth-test off, depth-write off), so the
            // meshes drawn next rasterize over it — the GPU equivalent of the
            // software path's background fill before rasterization.
            if let Some((_sky_buf, sky_bg)) = &sky_bind {
                pass.set_pipeline(&sky_pipeline.pipeline);
                pass.set_bind_group(0, sky_bg, &[]);
                pass.draw(0..3, 0..1);
            }

            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &camera_bg, &[]);
            pass.set_bind_group(2, &light_bg, &[]);

            // 🔑 In creation order (pat-414k3), not HashMap order. Draw order
            // decides which of two coplanar surfaces wins the depth test, and
            // a HashMap's order is seeded per process: the same scene drew a
            // different picture in every editor run.
            let mut ordered: Vec<(&u64, &Instance3D)> = self.instances.iter().collect();
            ordered.sort_unstable_by_key(|(id, _)| **id);
            for (_, inst) in ordered {
                if !inst.visible {
                    continue;
                }
                // pat-s3rrv.29: GeometryInstance3D settings, mirroring the
                // software renderer's colour pass (renderer.rs:669-692). None of
                // this was honoured here, so `cast_shadow = ShadowsOnly`,
                // `visibility_range_end` and `transparency` authored in a .tscn
                // were silently ignored by the SHIPPED (gpu-render) editor.
                //
                // `draws_geometry()` is false for ShadowsOnly — such an instance
                // still exists for the shadow pass but must not appear in colour.
                if !inst.draws_geometry() {
                    continue;
                }
                if !inst.is_within_visibility_range(viewport.camera_transform.origin) {
                    continue;
                }
                // Fully transparent means there is nothing to draw at all —
                // `opacity_at_distance` folds `transparency` together with the
                // VISIBILITY_RANGE_FADE_SELF ramp.
                if inst
                    .geometry
                    .opacity_at_distance(
                        viewport
                            .camera_transform
                            .origin
                            .distance_to(inst.transform.origin),
                    )
                    <= 0.0
                {
                    continue;
                }
                // pat-9gjpr: a custom `shader_type spatial` material runs as a
                // transpiled WGSL module. One that cannot be transpiled keeps
                // the old flat-albedo draw here; the editor entry point routes
                // such a frame to the software renderer before reaching this.
                let custom = inst
                    .shader_material
                    .as_ref()
                    .and_then(crate::spatial_wgsl::module_for)
                    .and_then(Result::ok);
                // Expand multimesh + shader material into concrete draws (matching
                // the software renderer), instead of drawing only a single mesh.
                for entry in instance_draw_entries(inst, self.surface_meshes.get(&inst.id.0).map_or(&[][..], Vec::as_slice)) {
                    // Geometry uploaded once by `prime_editor_draw_caches`; a
                    // mesh with no vertices was never cached and draws nothing,
                    // as before.
                    let cached_mesh = self
                        .editor_frame_mesh_keys
                        .get(&(entry.mesh as *const Mesh3D as usize))
                        .and_then(|k| self.mesh_gpu_cache.get(k));
                    let Some(cached_mesh) = cached_mesh else {
                        continue;
                    };
                    // pat-414k3: a draw wholly outside the camera's clip volume
                    // rasterizes no fragment, so skipping it changes no pixel —
                    // and most of a level is off screen at any moment.
                    if clip_planes.culls(&cached_mesh.bounds, &entry.transform) {
                        continue;
                    }
                    // pat-s3rrv.30: AlphaScissor. Same helper the software
                    // rasterizer uses, so both backends make the identical
                    // decision from the identical enum.
                    if custom.is_none()
                        && crate::render_modes::alpha_scissor_discard(
                            entry.transparency,
                            entry.albedo.a,
                            entry.alpha_scissor_threshold,
                        )
                    {
                        continue;
                    }
                    // pat-s3rrv.30: bind the pipeline whose cull state matches
                    // this material. The pass binds the Back-culling pipeline
                    // once above, which was the ONLY state the wgpu backend
                    // could express — an authored `cull_mode = Front` drew the
                    // near faces exactly as if it had said nothing.
                    // Alpha-blended draws must not write depth, or whichever
                    // transparent surface happened to draw first would
                    // depth-reject the one behind it and nothing would show
                    // through — an order-dependent result from a HashMap
                    // iteration, i.e. a coin flip.
                    //
                    // pat-9gjpr: and its depth TEST. Godot `no_depth_test` puts
                    // the draw in the alpha pass (`uses_alpha_pass` in
                    // scene_shader_forward_clustered.h), sorted far-first and
                    // drawn over whatever is already there.
                    let key = match &custom {
                        Some(m) => {
                            let r = m.render_modes;
                            let transparent = m.writes_alpha
                                || r.blend_add
                                || r.depth_test_disabled
                                || r.depth_draw == crate::render_modes::DepthDrawMode::Never;
                            EditorPipelineKey {
                                cull: r.cull,
                                depth_write: crate::render_modes::writes_depth(
                                    r.depth_draw,
                                    transparent,
                                ),
                                depth_test: !r.depth_test_disabled,
                                additive: r.blend_add,
                                module: std::sync::Arc::as_ptr(m) as usize,
                            }
                        }
                        None => {
                            let additive =
                                entry.blend_mode == gdserver3d::material::BlendMode::Add;
                            let transparent =
                                crate::render_modes::is_transparent(entry.transparency);
                            EditorPipelineKey {
                                cull: entry.cull_mode,
                                // Additive draws never write depth (pat-cezo4).
                                depth_write: !additive
                                    && crate::render_modes::writes_depth(
                                        entry.depth_draw_mode,
                                        transparent || entry.no_depth_test,
                                    ),
                                depth_test: !entry.no_depth_test,
                                additive,
                                module: 0,
                            }
                        }
                    };
                    let chosen = pipe_cache
                        .entry(key)
                        .or_insert_with(|| {
                            if let Some(m) = &custom {
                                if !pipe_modules.iter().any(|k| std::sync::Arc::ptr_eq(k, m)) {
                                    pipe_modules.push(m.clone());
                                }
                            }
                            pipeline.build_editor_pipeline(
                                device,
                                format,
                                key,
                                custom.as_deref().map(|m| m.wgsl.as_str()),
                            )
                        })
                        .clone();
                    let additive = key.additive.then_some(());
                    // Material colors are sRGB authoring values; upload them in
                    // linear so the WGSL shades in linear (the HDR target + camera
                    // post-process encode back to sRGB), matching the software path.
                    let alb = entry.albedo.srgb_to_linear();
                    let emis = entry.emission.srgb_to_linear();
                    let model_data = ModelUniformData {
                        model_matrix: transform_to_model_matrix(&entry.transform),
                        albedo: [alb.r, alb.g, alb.b, entry.albedo.a],
                        emission: [emis.r, emis.g, emis.b, entry.emission.a],
                        roughness: entry.roughness,
                        metallic: entry.metallic,
                        shading_mode: entry.shading_mode,
                        num_lights: self.lights.len().min(gdserver3d::light::MAX_SCENE_LIGHTS)
                            as u32,
                        // ⚠️ From the INSTANCE, not `entry`: this loop's `entry`
                        // is a `GpuDrawEntry` (the per-surface expansion), which
                        // carries material fields only. `GpuDrawCall` is the one
                        // that flattens `layers`.
                        layers: inst.geometry.layers,
                        has_normal_map: u32::from(entry.normal_texture.is_some()),
                        normal_scale: entry.normal_scale,
                        _model_pad: 0,
                        ..bytemuck::Zeroable::zeroed()
                    };
                    let mut model_data = model_data;
                    if let Some(m) = &custom {
                        // The shader replaces the material: its outputs reach
                        // `fs_main` through `cs.*`, and these are the identities
                        // the remaining `model.*` multiplies need. Godot's
                        // spatial defaults are ROUGHNESS 1, METALLIC 0, which
                        // the module seeds itself.
                        model_data.albedo = [1.0; 4];
                        model_data.emission = [0.0; 4];
                        model_data.roughness = 1.0;
                        model_data.metallic =
                            if m.wgsl.contains("let metallic_map = cs.metallic;") { 1.0 } else { 0.0 };
                        model_data.shading_mode = if m.render_modes.unshaded { 0 } else { 2 };
                        model_data.has_normal_map = 0;
                    }
                    // pat-h7sz7: probes blend per fragment; the draw's world
                    // bounding sphere only narrows which ones the loop visits.
                    let b = &entry.transform.basis;
                    let stretch =
                        (b.x.length_squared() + b.y.length_squared() + b.z.length_squared()).sqrt();
                    apply_probe_fields(
                        &mut model_data,
                        &self.reflection_probes,
                        probe_cubes,
                        entry.transform.origin,
                        Some((
                            entry.transform.xform(cached_mesh.bounds.center),
                            cached_mesh.bounds.radius * stretch + 1e-3,
                        )),
                    );
                    let mut uniform = [0u8; MODEL_UNIFORM_STRIDE];
                    uniform[..std::mem::size_of::<ModelUniformData>()]
                        .copy_from_slice(bytemuck::bytes_of(&model_data));
                    model_bytes.extend_from_slice(&uniform);
                    // pat-mgixk.52: this draw's material maps, or a 1x1 white texel per
                    // slot that has none. A bind group must fill EVERY entry its layout
                    // declares, so there is no "bind nothing" option — white is the
                    // multiplicative identity for albedo/roughness/metallic/emission,
                    // which is what keeps an untextured draw bit-identical to before, and
                    // for the normal slot `has_normal_map` decides whether it is read.
                    // Uploaded once by `prime_editor_draw_caches`.
                    let slot = |img: &Option<
                        std::sync::Arc<gdserver3d::material::TextureImage>,
                    >,
                                format: wgpu::TextureFormat|
                     -> &wgpu::TextureView {
                        img.as_ref()
                            .and_then(|i| {
                                self.material_tex_cache
                                    .get(&(std::sync::Arc::as_ptr(i) as usize, format))
                            })
                            .map(|c| &c.view)
                            .unwrap_or_else(|| &self.white_tex_cache[&format].1)
                    };
                    // A custom shader binds none of the base material's maps.
                    let slot = |img: &Option<
                        std::sync::Arc<gdserver3d::material::TextureImage>,
                    >,
                                format: wgpu::TextureFormat|
                     -> &wgpu::TextureView {
                        if custom.is_some() {
                            &self.white_tex_cache[&format].1
                        } else {
                            slot(img, format)
                        }
                    };
                    let custom_bg = custom.as_ref().map(|m| {
                        custom_bind_group(
                            device,
                            queue,
                            &pipeline.custom_layout,
                            m,
                            inst.shader_material.as_ref().expect("custom implies a material"),
                            shader_time,
                            (w, h),
                            &mut custom_tex_cache,
                            &mut custom_unbound_cache,
                            &mut custom_sampler_cache,
                        )
                    });
                    let maps = CachedSlotViews {
                        albedo: slot(&entry.albedo_texture, MATERIAL_COLOR_FORMAT),
                        normal: slot(&entry.normal_texture, MATERIAL_RAW_FORMAT),
                        roughness: slot(&entry.roughness_texture, MATERIAL_RAW_FORMAT),
                        metallic: slot(&entry.metallic_texture, MATERIAL_RAW_FORMAT),
                        emission: slot(&entry.emission_texture, MATERIAL_COLOR_FORMAT),
                    };
                    // Everything that does not write depth draws in the sorted
                    // transparent pass, as in Godot.
                    let blends = match &custom {
                        Some(_) => !key.depth_write || !key.depth_test || additive.is_some(),
                        None => {
                            crate::render_modes::is_transparent(entry.transparency)
                                || additive.is_some()
                                || entry.no_depth_test
                        }
                    };
                    draws.push(EditorDraw {
                        pipeline: chosen,
                        custom_bg,
                        maps,
                        mesh: cached_mesh,
                        sampler: entry.sampler,
                        uniform_index: draws.len(),
                        blend_depth: blends.then(|| {
                            viewport
                                .camera_transform
                                .origin
                                .distance_to(entry.transform.origin)
                        }),
                    });
                }
            }

            draw_count = draws.len();
            // pat-wyei2: opaque first in creation order, then blended draws
            // FARTHEST first, as Godot's transparent pass sorts them. Without
            // it the platformer's coin counter — a Label3D with four darker
            // "Parallax" Label3D children stacked just behind it — composited
            // its children OVER itself, since they are created after it.
            draws.sort_by(|a, b| match (a.blend_depth, b.blend_depth) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(da), Some(db)) => db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal),
            });
            // One buffer for every draw's model block, bound at its offset.
            let model_buf = (!draws.is_empty()).then(|| {
                create_buffer_init(
                    device,
                    queue,
                    "editor-model-uniforms",
                    &model_bytes,
                    wgpu::BufferUsages::UNIFORM,
                )
            });
            let samplers = &self.material_sampler_cache;
            for draw in draws.iter() {
                let i = draw.uniform_index;
                let model_buf = model_buf.as_ref().expect("non-empty draws");
                let maps = &draw.maps;
                // pat-3u1wu: the sampler THIS material asks for.
                let albedo_sampler = &samplers[draw.sampler.index()];
                pass.set_pipeline(&draw.pipeline);
                if let Some(bg) = &draw.custom_bg {
                    pass.set_bind_group(3, bg, &[]);
                }
                let model_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("editor-model-bind-group"),
                    layout: &pipeline.model_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: model_buf,
                                offset: (i * MODEL_UNIFORM_STRIDE) as u64,
                                size: std::num::NonZeroU64::new(
                                    std::mem::size_of::<ModelUniformData>() as u64,
                                ),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&maps.albedo),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&albedo_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&maps.normal),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&maps.roughness),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&maps.metallic),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::TextureView(&maps.emission),
                        },
                    ],
                });
                pass.set_bind_group(1, &model_bg, &[]);
                pass.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                pass.set_index_buffer(draw.mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
            }
        }

        // Copy color → cached readback buffer (no depth copy).
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.color_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        // pat-s3rrv.29: depth comes back too, because the decal and fog-volume
        // passes reproject each pixel and cannot run without it. Encoded into
        // THIS encoder, before the submit below — a copy recorded after the
        // submit would never execute.
        //
        // Allocated per frame rather than cached on `EditorTargets`: this path
        // already reallocates on resize, and depth readback only happens when
        // the scene actually has env effects (see the early-out below).
        // ⚠️ SSAO needs this depth too, and asks for it here or it silently
        // gets none: `depth` is only populated inside the `if let
        // Some(depth_readback_buf)` below (pat-mgixk.52). The condition mirrors
        // `apply_ssao_env`'s own early-out so a scene whose SSAO is disabled or
        // zero-intensity still pays nothing for the readback.
        // pat-p3p1m: environment FOG needs it too. The editor/Play entry point
        // used to route every fogged scene to the software renderer, because
        // this path never applied `apply_fog_pass` — a fogged game (the 3D
        // platformer demo) rendered at ~80 ms a frame on the CPU.
        let wants_depth = !self.decals.is_empty()
            || !self.fog_volumes.is_empty()
            || viewport.environment.as_ref().map_or(false, |e| {
                (e.ssao_enabled && e.ssao_intensity > 0.0) || e.fog_enabled
            });
        let depth_align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let depth_padded_row = (w * 4 + depth_align - 1) / depth_align * depth_align;
        let depth_readback_buf = if wants_depth {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("editor-depth-readback"),
                size: depth_padded_row as u64 * h as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &targets.depth_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::DepthOnly,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(depth_padded_row),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            Some(buf)
        } else {
            None
        };
        queue.submit(std::iter::once(encoder.finish()));
        // The decals' depth-independent work (their screen rectangles and the
        // raster of the instances a `cull_mask` excludes) runs HERE, while the
        // GPU renders the frame just submitted, instead of after the readback
        // with the GPU idle. Same inputs, same plan.
        let fx_inv_proj = invert_projection(&projection_matrix);
        let decal_plan = depth_readback_buf.as_ref().and_then(|_| {
            let decal_receivers: Vec<&Instance3D> = self.instances.values().collect();
            crate::renderer::DecalPlan::new(
                w,
                h,
                &self.decals,
                &fx_inv_proj,
                &viewport.camera_transform,
                &decal_receivers,
            )
        });
        // pat-9gjpr: the caches moved out before the pass come back.
        self.editor_pipe_cache = pipe_cache;
        self.editor_pipe_modules = pipe_modules;
        self.custom_tex_cache = custom_tex_cache;
        self.custom_unbound_cache = custom_unbound_cache;
        self.custom_sampler_cache = custom_sampler_cache;

        // Map, sync once, de-pad each row into a tight top-down BGRA buffer, then
        // unmap so the cached buffer can be mapped again next frame. This is a
        // per-row copy (no per-pixel float work): the caller composites overlays
        // and encodes BMP directly on these bytes.
        let slice = targets.readback_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r.is_ok());
        });
        device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
            .ok();
        // Device-lost hardening (pat-ism62 #4): if the map failed (e.g. the GPU
        // device was lost on sleep/wake), degrade gracefully — return a flat
        // cleared frame instead of panicking the (indefinitely-running) editor
        // render loop inside `get_mapped_range`. A failed map leaves the buffer
        // unmapped, so the next frame can retry.
        if !matches!(rx.try_recv(), Ok(true)) {
            let px = [
                (clear.b.clamp(0.0, 1.0) * 255.0) as u8,
                (clear.g.clamp(0.0, 1.0) * 255.0) as u8,
                (clear.r.clamp(0.0, 1.0) * 255.0) as u8,
                255,
            ];
            let data: Vec<u8> = px.iter().copied().cycle().take((w * h * 4) as usize).collect();
            return (w, h, data);
        }
        // Decode the Rgba16Float HDR readback (8 bytes/pixel, top-down) into a
        // linear `Color` buffer, run the shared camera post-process (glow →
        // tonemap → linear→sRGB — identical code to the software path, so the
        // two backends match), then pack the display image to BGRA bytes.
        let mapped = slice.get_mapped_range();
        // Decoded in row bands on worker threads (pat-414k3): per pixel the
        // same conversion, so the same bits.
        // Reused across frames: a fresh zeroed 13 MB buffer per frame was a
        // visible cost of its own. Every element is overwritten below.
        let mut pixels = std::mem::take(&mut self.editor_hdr_scratch);
        pixels.resize(w as usize * h as usize, Color::new(0.0, 0.0, 0.0, 0.0));
        {
            let src: &[u8] = &mapped;
            let padded_row = targets.padded_row as usize;
            par_rows(&mut pixels, w as usize, |row, out| {
                let start = row * padded_row;
                let row_bytes = &src[start..start + (w * 8) as usize];
                for (px, c) in row_bytes.chunks_exact(8).zip(out.iter_mut()) {
                    let r = f16_to_f32(u16::from_le_bytes([px[0], px[1]]));
                    let g = f16_to_f32(u16::from_le_bytes([px[2], px[3]]));
                    let b = f16_to_f32(u16::from_le_bytes([px[4], px[5]]));
                    let a = f16_to_f32(u16::from_le_bytes([px[6], px[7]]));
                    *c = Color::new(r, g, b, a);
                }
            });
        }
        drop(mapped);
        targets.readback_buf.unmap();

        let default_env = Environment3D::default();
        let env = viewport.environment.as_ref().unwrap_or(&default_env);

        // pat-s3rrv.29: env effects, in the software renderer's exact order —
        // decals modulate surface albedo first, then fog covers the result
        // (renderer.rs:499-526). The acceptance compares these pixels against
        // the software backend's, so the order is part of the contract.
        //
        // Calls the software implementations rather than reimplementing the
        // projection math, so the two backends cannot drift.
        if let Some(depth_buf) = depth_readback_buf {
            let depth_slice = depth_buf.slice(..);
            depth_slice.map_async(wgpu::MapMode::Read, |_| {});
            device
                .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
                .ok();
            let depth_mapped = depth_slice.get_mapped_range();
            let mut depth: Vec<f32> = vec![0.0; w as usize * h as usize];
            {
                let src: &[u8] = &depth_mapped;
                par_rows(&mut depth, w as usize, |row, out| {
                    let start = row * depth_padded_row as usize;
                    for (col, d) in out.iter_mut().enumerate() {
                        let o = start + col * 4;
                        *d = f32::from_le_bytes([src[o], src[o + 1], src[o + 2], src[o + 3]]);
                    }
                });
            }
            drop(depth_mapped);
            depth_buf.unmap();

            if let Some(plan) = &decal_plan {
                plan.apply(&mut pixels, &depth, &self.decals, &fx_inv_proj, &viewport.camera_transform);
            }
            // Environment fog covers the decal-modulated surface, in the
            // software renderer's order (decal -> fog -> fog volumes), and with
            // the same suns for its sun scatter (pat-p3p1m).
            crate::renderer::SoftwareRenderer3D::apply_fog_pass(
                &mut pixels,
                &depth,
                w,
                h,
                env,
                &fx_inv_proj,
                &viewport.camera_transform,
                &crate::renderer::sky_suns_from_lights(&self.lights),
            );
            crate::renderer::SoftwareRenderer3D::apply_voxel_gi_pass(
                &mut pixels,
                &depth,
                w,
                h,
                &self.voxel_gis,
                &fx_inv_proj,
                &viewport.camera_transform,
            );
            crate::renderer::SoftwareRenderer3D::apply_fog_volume_pass(
                &mut pixels,
                &depth,
                w,
                h,
                &self.fog_volumes,
                &fx_inv_proj,
                &viewport.camera_transform,
            );
            // SSAO runs BEFORE tonemapping, in the software path's exact order
            // (`renderer.rs:683`): decals and fog modulate scene-space colour
            // first, then ambient occlusion darkens creases, and only then does
            // `apply_camera_post` collapse HDR to display sRGB. Swapping the last
            // two would occlude already-tonemapped pixels and the two backends
            // would stop matching, which is the contract this path is held to
            // (pat-mgixk.52).
            //
            // Called unconditionally: `apply_ssao_env` self-gates on
            // `ssao_enabled && ssao_intensity > 0.0` and returns immediately
            // otherwise, so a scene without SSAO stays bit-identical to before.
            crate::postprocess::apply_ssao_env(&mut pixels, &depth, w, h, env);
        }

        // Only packed to bytes below, so the byte variant (same bytes, no
        // per-channel sRGB `powf` where it applies).
        crate::postprocess::apply_camera_post_for_bytes(&mut pixels, w, h, env);

        let mut data = vec![0u8; w as usize * h as usize * 4];
        par_rows(&mut data, w as usize * 4, |row, out| {
            let src = &pixels[row * w as usize..(row + 1) * w as usize];
            for (c, px) in src.iter().zip(out.chunks_exact_mut(4)) {
                // Stored order is B, G, R, A (the format the caller's BMP
                // encode expects). Post-process already clamped to [0, 1].
                px[0] = (c.b * 255.0) as u8;
                px[1] = (c.g * 255.0) as u8;
                px[2] = (c.r * 255.0) as u8;
                px[3] = (c.a * 255.0) as u8;
            }
        });

        self.editor_last_draws = draw_count;
        self.editor_hdr_scratch = pixels;
        (w, h, data)
    }

    /// Color-buffer twin of [`render_editor_color_bgra`], returning a
    /// [`FrameData3D`] (RGBA `Color` pixels, empty depth). Kept for tests and any
    /// consumer that wants `Color`; the editor hot path uses the byte-native
    /// `_bgra` method to avoid this per-pixel unpack.
    pub fn render_editor_color(&mut self, viewport: &Viewport3D, clear: Color) -> FrameData3D {
        let (w, h, data) = self.render_editor_color_bgra(viewport, clear);
        let mut pixels = Vec::with_capacity(w as usize * h as usize);
        for px in data.chunks_exact(4) {
            // Stored order is B, G, R, A.
            pixels.push(Color::new(
                px[2] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[0] as f32 / 255.0,
                px[3] as f32 / 255.0,
            ));
        }
        FrameData3D { width: w, height: h, pixels, depth: Vec::new() }
    }

    /// Returns the reflection probes currently registered with the renderer.
    pub fn reflection_probes(&self) -> &[ReflectionProbe] {
        &self.reflection_probes
    }

    /// Samples the combined ambient contribution of all reflection probes
    /// whose influence box contains `world_pos`. See
    /// [`gdserver3d::reflection_probe::sample_probes_at`] for the parity rule.
    pub fn sample_reflection_probes(&self, world_pos: Vector3) -> Color {
        sample_probes_at(&self.reflection_probes, world_pos)
    }

    /// Returns a reference to the underlying GPU context.
    pub fn context(&self) -> &GpuContext3D {
        &self.ctx
    }

    /// Renders a frame and reads back the pixel data from the GPU.
    fn render_and_readback(&mut self, viewport: &Viewport3D) -> FrameData3D {
        let w = viewport.width.max(1);
        let h = viewport.height.max(1);
        // Render to a linear HDR target + procedural sky + linear-shaded meshes,
        // then apply the shared camera post-process (glow → tonemap → sRGB) on
        // readback, so `render_frame` produces the SAME display-sRGB image as
        // `SoftwareRenderer3D::render_frame` and the editor GPU path (not the old
        // sRGB-target / no-sky / no-tonemap output). Depth is still read back.
        let color_format = wgpu::TextureFormat::Rgba16Float;

        // Lazily build the HDR mesh + sky pipelines (shared with the editor path),
        // and upload any uncached mesh geometry — all with `&mut self`, before the
        // immutable device/target borrows the render pass needs.
        if self.editor_pipeline.is_none() {
            self.editor_pipeline = Some(RenderPipeline3D::new(&self.ctx.device, color_format));
        }
        // pat-s3rrv.30: build every (cull, depth-write) pipeline variant once.
        // Both are pipeline state on the GPU, so a material that culls FRONT or
        // an alpha-blended one that must not write depth cannot be served by
        // flipping a uniform — it needs its own pipeline.
        if self.editor_variants.is_empty() {
            let base = self
                .editor_pipeline
                .as_ref()
                .expect("editor pipeline just ensured");
            let mut built = Vec::new();
            for cull in [
                crate::render_modes::CullMode::Back,
                crate::render_modes::CullMode::Front,
                crate::render_modes::CullMode::Disabled,
            ] {
                for depth_write in [true, false] {
                    let face = RenderPipeline3D::wgpu_cull_face(cull);
                    let p = base.build_cull_variant(
                        &self.ctx.device,
                        color_format,
                        face,
                        depth_write,
                        wgpu::BlendState::ALPHA_BLENDING,
                    );
                    built.push((cull, depth_write, p));
                }
                // pat-cezo4: BLEND_MODE_ADD (`src * alpha + dst`), never writing
                // depth — Godot's additive materials are transparent.
                let face = RenderPipeline3D::wgpu_cull_face(cull);
                let add = base.build_cull_variant(
                    &self.ctx.device,
                    color_format,
                    face,
                    false,
                    ADDITIVE_BLEND,
                );
                self.editor_add_variants.push((cull, add));
            }
            self.editor_variants = built;
        }
        if self.editor_sky_pipeline.is_none() {
            self.editor_sky_pipeline = Some(SkyPipeline3D::new(&self.ctx.device, color_format));
        }
        let draw_calls = self.build_draw_calls();
        // Shadow atlas + light uniforms (pat-32edz / pat-2be0q): cached across
        // frames, regenerated only when the shadow-relevant state changes.
        let light_data = self.ensure_shadow_atlas(viewport);
        self.ensure_probe_cubes(viewport.environment.as_ref());
        self.ensure_sky_panorama(viewport.environment.as_ref());

        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        let pipeline = self.editor_pipeline.as_ref().unwrap();
        // pat-s3rrv.30: the (cull, depth-write) variants, ensured above.
        let variants = &self.editor_variants;
        let add_variants = &self.editor_add_variants;
        let sky_pipeline = self.editor_sky_pipeline.as_ref().unwrap();

        // Build a per-material pipeline for any draw carrying a transpiled custom
        // fragment module (UV/varying-driven spatial shaders). Keyed by module
        // source so identical shaders share one pipeline this frame; the module
        // was already WGSL-validated in `custom_spatial_module_wgsl`.
        let mut custom_pipelines: HashMap<&str, wgpu::RenderPipeline> = HashMap::new();
        for call in &draw_calls {
            if let Some(module) = &call.custom_module {
                if !custom_pipelines.contains_key(module.as_str()) {
                    let variant = pipeline.build_variant(device, color_format, module);
                    custom_pipelines.insert(module.as_str(), variant);
                }
            }
        }

        // Create color (linear HDR) and depth textures.
        let color_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3d-color-target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3d-depth-target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Procedural sky background (same as the editor path): a fullscreen pass
        // drawn before the meshes when the environment uses a Sky background.
        let sky_suns = crate::renderer::sky_suns_from_lights(&self.lights);
        let sky_panorama_bindings = (
            &self.sky_panorama.as_ref().expect("ensured above").1,
            &self.sky_panorama.as_ref().expect("ensured above").2,
        );
        let sky_bind = viewport
            .environment
            .as_ref()
            .and_then(|env| sky_uniform_for_env(env, viewport, w, h, &sky_suns))
            .map(|data| {
                let buf = create_buffer_init(
                    device,
                    queue,
                    "sky-uniform",
                    bytemuck::bytes_of(&data),
                    wgpu::BufferUsages::UNIFORM,
                );
                let (pano_view, pano_sampler) = sky_panorama_bindings;
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sky-bind-group"),
                    layout: &sky_pipeline.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(pano_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(pano_sampler),
                        },
                    ],
                });
                (buf, bg)
            });
        // Clear the linear HDR target to the environment's custom backdrop (linear)
        // or black; after the post-process's linear→sRGB encode the flat backdrop
        // returns at its authored value.
        let clear_linear = match viewport.environment.as_ref() {
            Some(env) if env.background_mode == BackgroundMode::CustomColor => env.background_color,
            _ => Color::new(0.0, 0.0, 0.0, 1.0),
        }
        .srgb_to_linear();

        // Prepare camera uniforms.
        let view_matrix = transform_to_view_matrix(&viewport.camera_transform);
        let projection_matrix = perspective_projection_matrix(
            viewport.fov,
            viewport.aspect(),
            viewport.near,
            viewport.far,
        );
        let cam_pos = viewport.camera_transform.origin;
        let ambient = viewport
            .environment
            .as_ref()
            .map(crate::renderer::environment_ambient_color)
            .unwrap_or(Color::new(0.1, 0.1, 0.1, 1.0));
        let camera_data = CameraUniformData {
            view_matrix,
            projection_matrix,
            camera_position: [cam_pos.x, cam_pos.y, cam_pos.z],
            _pad0: 0.0,
            ambient: [ambient.r, ambient.g, ambient.b, 1.0],
        };
        let camera_buf = create_buffer_init(
            device,
            queue,
            "camera-uniform-buf",
            bytemuck::bytes_of(&camera_data),
            wgpu::BufferUsages::UNIFORM,
        );
        let camera_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera-bind-group"),
            layout: &pipeline.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            }],
        });

        // Light bind group over the cached shadow atlases (pat-2be0q).
        let atlas = self
            .shadow_atlas_cache
            .as_ref()
            .expect("ensure_shadow_atlas ran above");
        let probe_cubes = self
            .probe_cube_cache
            .as_ref()
            .expect("ensure_probe_cubes ran above");
        let (light_bg, _light_buf) = create_light_bind_group(
            device,
            queue,
            &pipeline.light_layout,
            &light_data,
            &atlas.view,
            &atlas.omni_view,
            probe_cubes,
            "light-bind-group",
        );
        let num_lights = self.lights.len().min(gdserver3d::light::MAX_SCENE_LIGHTS) as u32;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("3d-render-encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        // The environment's backdrop (linear), not plain black —
                        // a `CustomColor` background must survive the GPU trait
                        // path exactly like the software `fill_background`.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_linear.r as f64,
                            g: clear_linear.g as f64,
                            b: clear_linear.b as f64,
                            a: clear_linear.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            // Sky background first (depth-test off), so meshes rasterize over it.
            if let Some((_sky_buf, sky_bg)) = &sky_bind {
                pass.set_pipeline(&sky_pipeline.pipeline);
                pass.set_bind_group(0, sky_bg, &[]);
                pass.draw(0..3, 0..1);
            }

            // Draw each expanded instance from the cached geometry (pat-ism62 #2b).
            // Groups 0 (camera) and 2 (lights) are (re)bound per draw so they stay
            // valid across per-material pipeline switches (identical layouts).
            for call in &draw_calls {
                let cached = match self.mesh_gpu_cache.get(&call.mesh_key) {
                    Some(c) => c,
                    None => continue,
                };
                // Per-material custom pipeline (real per-fragment shader body)
                // when this draw carries a transpiled module; otherwise the
                // shared flat-albedo pipeline.
                // pat-s3rrv.30: AlphaScissor, as in the editor path above.
                if crate::render_modes::alpha_scissor_discard(
                    call.transparency,
                    call.albedo.a,
                    call.alpha_scissor_threshold,
                ) {
                    continue;
                }
                // pat-s3rrv.30: a non-default cull mode also needs its own
                // pipeline. A custom shader module still wins — it carries the
                // material's actual fragment body, and re-culling it here would
                // silently drop the shader; that combination is reported as
                // remaining scope rather than resolved wrongly.
                let want_depth = crate::render_modes::writes_depth(
                    call.depth_draw_mode,
                    crate::render_modes::is_transparent(call.transparency),
                );
                let additive = (call.blend_mode == gdserver3d::material::BlendMode::Add)
                    .then(|| {
                        add_variants
                            .iter()
                            .find(|(c, _)| *c == call.cull_mode)
                            .map(|(_, p)| p)
                    })
                    .flatten();
                let mode_pipeline = additive.or_else(|| {
                    variants
                        .iter()
                        .find(|(c, d, _)| *c == call.cull_mode && *d == want_depth)
                        .map(|(_, _, p)| p)
                });
                let custom_pipeline = call
                    .custom_module
                    .as_deref()
                    .and_then(|m| custom_pipelines.get(m));
                let draw_pipeline = custom_pipeline
                    .or(mode_pipeline)
                    .unwrap_or(&pipeline.pipeline);
                pass.set_pipeline(draw_pipeline);
                // Group 3 exists only on a custom pipeline, and when it exists
                // it MUST be bound or the draw fails validation. `custom_uniforms`
                // always leads with `time`, so this is never a zero-sized buffer
                // (pat-s3rrv.122).
                let custom_bg = custom_pipeline.map(|_| {
                    let buf = create_buffer_init(
                        device,
                        queue,
                        "custom-uniform-buf",
                        bytemuck::cast_slice(&call.custom_uniforms),
                        wgpu::BufferUsages::UNIFORM,
                    );
                    // The layout declares a FIXED slot count, and a bind group
                    // must fill every entry — so slots the shader does not use
                    // get a 1x1 white dummy. Omitting them fails validation and
                    // would take down every custom draw, not just textured ones.
                    let dummy = upload_custom_texture(
                        device,
                        queue,
                        &gdserver3d::material::TextureImage::new(
                            1,
                            1,
                            vec![gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)],
                        ),
                        false,
                    );
                    let uploaded: Vec<wgpu::Texture> = call
                        .custom_textures
                        .iter()
                        .map(|(img, srgb)| upload_custom_texture(device, queue, img, *srgb))
                        .collect();
                    let views: Vec<wgpu::TextureView> = (0..CUSTOM_SAMPLER_SLOTS)
                        .map(|slot| match uploaded.get(slot as usize) {
                            Some(t) => t.create_view(&wgpu::TextureViewDescriptor::default()),
                            None => dummy.create_view(&wgpu::TextureViewDescriptor::default()),
                        })
                        .collect();
                    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                        label: Some("custom-shader-sampler"),
                        mag_filter: wgpu::FilterMode::Linear,
                        min_filter: wgpu::FilterMode::Linear,
                        ..Default::default()
                    });
                    let mut entries = vec![wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    }];
                    for slot in 0..CUSTOM_SAMPLER_SLOTS {
                        entries.push(wgpu::BindGroupEntry {
                            binding: custom_texture_binding(slot),
                            resource: wgpu::BindingResource::TextureView(&views[slot as usize]),
                        });
                        entries.push(wgpu::BindGroupEntry {
                            binding: custom_sampler_binding(slot),
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        });
                    }
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("custom-shader-bind-group"),
                        layout: &pipeline.custom_layout,
                        entries: &entries,
                    })
                });
                if let Some(bg) = custom_bg.as_ref() {
                    pass.set_bind_group(3, bg, &[]);
                }
                pass.set_bind_group(0, &camera_bg, &[]);
                pass.set_bind_group(2, &light_bg, &[]);
                // Shade in linear (HDR target + post-process encode back to sRGB).
                let alb = call.albedo.srgb_to_linear();
                let emis = call.emission.srgb_to_linear();
                let model_data = ModelUniformData {
                    model_matrix: transform_to_model_matrix(&call.transform),
                    albedo: [alb.r, alb.g, alb.b, call.albedo.a],
                    emission: [emis.r, emis.g, emis.b, call.emission.a],
                    roughness: call.roughness,
                    metallic: call.metallic,
                    shading_mode: call.shading_mode,
                    num_lights,
                    layers: call.layers,
                    has_normal_map: u32::from(call.normal_texture.is_some()),
                    normal_scale: call.normal_scale,
                    _model_pad: 0,
                    ..bytemuck::Zeroable::zeroed()
                };
                let mut model_data = model_data;
                apply_probe_fields(
                    &mut model_data,
                    &self.reflection_probes,
                    probe_cubes,
                    call.transform.origin,
                    None,
                );
                let model_buf = create_buffer_init(
                    device,
                    queue,
                    "model-uniform-buf",
                    bytemuck::bytes_of(&model_data),
                    wgpu::BufferUsages::UNIFORM,
                );
                // pat-mgixk.52: this draw's material maps, or a 1x1 white texel per
                // slot that has none. A bind group must fill EVERY entry its layout
                // declares, so there is no "bind nothing" option — white is the
                // multiplicative identity for albedo/roughness/metallic/emission,
                // which is what keeps an untextured draw bit-identical to before, and
                // for the normal slot `has_normal_map` decides whether it is read.
                let maps = material_slot_views(
                    device,
                    queue,
                    call.albedo_texture.as_deref(),
                    call.normal_texture.as_deref(),
                    call.roughness_texture.as_deref(),
                    call.metallic_texture.as_deref(),
                    call.emission_texture.as_deref(),
                );
                let albedo_sampler = material_sampler(device, call.sampler);
                let model_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("model-bind-group"),
                    layout: &pipeline.model_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: model_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&maps.albedo),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&albedo_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&maps.normal),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&maps.roughness),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&maps.metallic),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::TextureView(&maps.emission),
                        },
                    ],
                });

                pass.set_bind_group(1, &model_bg, &[]);
                pass.set_vertex_buffer(0, cached.vertex_buf.slice(..));
                pass.set_index_buffer(cached.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..cached.index_count, 0, 0..1);
            }
        }

        // Copy color texture to readback buffer (Rgba16Float = 8 bytes/pixel).
        let bytes_per_pixel = 8u32;
        let unpadded_row = w * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (unpadded_row + align - 1) / align * align;

        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("color-readback"),
            size: padded_row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &color_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        // Copy depth texture to readback buffer.
        let depth_bytes_per_pixel = 4u32; // Depth32Float = 4 bytes
        let depth_unpadded_row = w * depth_bytes_per_pixel;
        let depth_padded_row = (depth_unpadded_row + align - 1) / align * align;
        let depth_readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("depth-readback"),
            size: depth_padded_row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &depth_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::DepthOnly,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &depth_readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(depth_padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(std::iter::once(encoder.finish()));

        // Map and read color data.
        let color_slice = readback_buf.slice(..);
        color_slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok();
        let color_mapped = color_slice.get_mapped_range();

        let mut pixels = Vec::with_capacity(w as usize * h as usize);
        for row in 0..h {
            let start = (row * padded_row) as usize;
            for col in 0..w {
                let o = start + (col * bytes_per_pixel) as usize;
                // Rgba16Float, RGBA order: decode each half-float channel.
                let r = f16_to_f32(u16::from_le_bytes([color_mapped[o], color_mapped[o + 1]]));
                let g = f16_to_f32(u16::from_le_bytes([color_mapped[o + 2], color_mapped[o + 3]]));
                let b = f16_to_f32(u16::from_le_bytes([color_mapped[o + 4], color_mapped[o + 5]]));
                let a = f16_to_f32(u16::from_le_bytes([color_mapped[o + 6], color_mapped[o + 7]]));
                pixels.push(Color::new(r, g, b, a));
            }
        }
        drop(color_mapped);

        // Map and read depth data (needed by the fog pass below, before the
        // camera post-process — the software path fogs in linear scene space).
        let depth_slice = depth_readback_buf.slice(..);
        depth_slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok();
        let depth_mapped = depth_slice.get_mapped_range();

        let mut depth = Vec::with_capacity(w as usize * h as usize);
        for row in 0..h {
            let start = (row * depth_padded_row) as usize;
            for col in 0..w {
                let offset = start + (col * depth_bytes_per_pixel) as usize;
                let d = f32::from_le_bytes([
                    depth_mapped[offset],
                    depth_mapped[offset + 1],
                    depth_mapped[offset + 2],
                    depth_mapped[offset + 3],
                ]);
                depth.push(d);
            }
        }
        drop(depth_mapped);

        // Fog (linear scene space) then the shared camera post-process
        // (glow → tonemap → linear→sRGB) — the exact order and functions the
        // software `render_frame` uses, so `render_frame`'s pixels come out
        // 1:1 with `SoftwareRenderer3D::render_frame` including fog
        // (pat-kr04d: fog runs on the read-back HDR buffer with the read-back
        // depth; both backends store raw z/w with background = 1.0).
        let default_env = gdserver3d::environment::Environment3D::default();
        let env = viewport.environment.as_ref().unwrap_or(&default_env);
        let fog_inv_proj = invert_projection(&projection_matrix);
        // pat-s3rrv.29: decals project onto shaded surfaces BEFORE any fog
        // covers the result, and local fog volumes layer on TOP of the
        // environment fog. This is the software renderer's exact ordering
        // (renderer.rs:499-526) rather than a plausible-looking one: the
        // acceptance compares GPU pixels against software pixels, so a
        // different order is a visible mismatch, not a stylistic choice.
        //
        // Both passes are the software implementations, called directly rather
        // than reimplemented — one copy of the projection math means the two
        // backends cannot drift apart.
        let decal_receivers: Vec<&Instance3D> = self.instances.values().collect();
        crate::renderer::SoftwareRenderer3D::apply_decal_pass_masked(
            &mut pixels,
            &depth,
            w,
            h,
            &self.decals,
            &fog_inv_proj,
            &viewport.camera_transform,
            &decal_receivers,
        );
        crate::renderer::SoftwareRenderer3D::apply_fog_pass(
            &mut pixels,
            &depth,
            w,
            h,
            env,
            &fog_inv_proj,
            &viewport.camera_transform,
            &crate::renderer::sky_suns_from_lights(&self.lights),
        );
        crate::renderer::SoftwareRenderer3D::apply_voxel_gi_pass(
            &mut pixels,
            &depth,
            w,
            h,
            &self.voxel_gis,
            &fog_inv_proj,
            &viewport.camera_transform,
        );
        crate::renderer::SoftwareRenderer3D::apply_fog_volume_pass(
            &mut pixels,
            &depth,
            w,
            h,
            &self.fog_volumes,
            &fog_inv_proj,
            &viewport.camera_transform,
        );
        // SSAO runs BEFORE tonemapping, in the software path's exact order
        // (`renderer.rs:683`): decals and fog modulate scene-space colour
        // first, then ambient occlusion darkens creases, and only then does
        // `apply_camera_post` collapse HDR to display sRGB. Swapping the last
        // two would occlude already-tonemapped pixels and the two backends
        // would stop matching, which is the contract this path is held to
        // (pat-mgixk.52).
        //
        // Called unconditionally: `apply_ssao_env` self-gates on
        // `ssao_enabled && ssao_intensity > 0.0` and returns immediately
        // otherwise, so a scene without SSAO stays bit-identical to before.
        crate::postprocess::apply_ssao_env(&mut pixels, &depth, w, h, env);
        crate::postprocess::apply_camera_post(&mut pixels, w, h, env);

        FrameData3D {
            width: w,
            height: h,
            pixels,
            depth,
        }
    }
}

impl RenderingServer3D for GpuRenderer3D {
    fn create_instance(&mut self) -> Instance3DId {
        let id_val = self.next_id;
        self.next_id += 1;
        let id = Instance3DId(id_val);
        self.instances.insert(id_val, Instance3D::new(id));
        id
    }

    fn free_instance(&mut self, id: Instance3DId) {
        self.instances.remove(&id.0);
        self.surface_meshes.remove(&id.0);
    }

    fn set_mesh(&mut self, id: Instance3DId, mesh: Mesh3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            if mesh.surfaces.is_empty() {
                self.surface_meshes.remove(&id.0);
            } else {
                self.surface_meshes.insert(id.0, split_surface_meshes(&mesh));
            }
            inst.mesh = Some(mesh);
        }
    }

    fn set_surface_materials(&mut self, id: Instance3DId, materials: Vec<Option<Material3D>>) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.surface_materials = materials;
        }
    }

    fn set_material(&mut self, id: Instance3DId, material: Material3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.material = Some(material);
        }
    }

    fn set_shader_material(&mut self, id: Instance3DId, material: ShaderMaterial3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.shader_material = Some(material);
        }
    }

    fn set_multimesh(&mut self, id: Instance3DId, multimesh: gdserver3d::multimesh::MultiMesh3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.multimesh = Some(multimesh);
        }
    }

    fn clear_multimesh(&mut self, id: Instance3DId) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.multimesh = None;
        }
    }

    fn set_transform(&mut self, id: Instance3DId, transform: Transform3D) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.transform = transform;
        }
    }

    fn set_visible(&mut self, id: Instance3DId, visible: bool) {
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.visible = visible;
        }
    }

    fn add_light(&mut self, _id: Light3DId) {
        // Light is added via update_light.
    }

    fn remove_light(&mut self, id: Light3DId) {
        self.lights.retain(|l| l.id != id);
    }

    fn update_light(&mut self, light: &Light3D) {
        if let Some(existing) = self.lights.iter_mut().find(|l| l.id == light.id) {
            *existing = light.clone();
        } else {
            self.lights.push(light.clone());
        }
    }

    fn add_reflection_probe(&mut self, id: ReflectionProbeId) {
        if !self.reflection_probes.iter().any(|p| p.id == id) {
            self.reflection_probes.push(ReflectionProbe::new(id));
        }
    }

    fn add_voxel_gi(&mut self, id: VoxelGIId) {
        if !self.voxel_gis.iter().any(|g| g.id == id) {
            self.voxel_gis.push(VoxelGI::new(id));
        }
    }

    fn update_voxel_gi(&mut self, gi: &VoxelGI) {
        if let Some(existing) = self.voxel_gis.iter_mut().find(|g| g.id == gi.id) {
            *existing = gi.clone();
        } else {
            self.voxel_gis.push(gi.clone());
        }
    }

    fn remove_voxel_gi(&mut self, id: VoxelGIId) {
        self.voxel_gis.retain(|g| g.id != id);
    }

    fn update_reflection_probe(&mut self, probe: &ReflectionProbe) {
        if let Some(existing) = self
            .reflection_probes
            .iter_mut()
            .find(|p| p.id == probe.id)
        {
            *existing = probe.clone();
        } else {
            self.reflection_probes.push(probe.clone());
        }
    }

    fn remove_reflection_probe(&mut self, id: ReflectionProbeId) {
        self.reflection_probes.retain(|p| p.id != id);
    }

    // -- env effects (pat-s3rrv.29) ----------------------------------------
    //
    // These four were the trait's default no-ops. `SoftwareRenderer3D`
    // overrode all of them and this backend did not, so the SHIPPED editor —
    // `gdeditor`'s default feature is `gpu-render` — silently discarded every
    // decal, fog volume, occluder and GeometryInstance3D setting a scene
    // authored, while the software renderer's tests kept passing.

    fn set_geometry_instance_settings(
        &mut self,
        id: Instance3DId,
        settings: GeometryInstanceSettings,
    ) {
        // Stored on the instance itself, exactly as the software backend does
        // — `Instance3D` already carries `geometry`, so both renderers read the
        // settings from one place and cannot drift.
        if let Some(inst) = self.instances.get_mut(&id.0) {
            inst.geometry = settings;
        }
    }

    fn add_fog_volume(&mut self, volume: FogVolume, transform: Transform3D) {
        self.fog_volumes
            .push(crate::renderer::PlacedFogVolume::new(volume, transform));
    }

    fn add_decal(&mut self, decal: Decal3D) {
        self.decals.push(decal);
    }

    fn add_occluder(&mut self, occluder: OccluderInstance3D) {
        self.occluders.push(occluder);
    }

    fn render_frame(&mut self, viewport: &Viewport3D) -> FrameData3D {
        self.render_and_readback(viewport)
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// pat-3u1wu: the mip chain halves down to 1x1 and box-averages.
    #[test]
    fn material_mip_chain_halves_to_one_texel_and_averages() {
        // 4x2: left half black, right half white.
        let mut base = Vec::new();
        for _y in 0..2 {
            for x in 0..4 {
                let v = if x < 2 { 0u8 } else { 255u8 };
                base.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let levels = material_mip_chain(4, 2, base);
        let dims: Vec<(u32, u32)> = levels.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(dims, vec![(4, 2), (2, 1), (1, 1)]);
        assert_eq!(&levels[1].2[..], &[0, 0, 0, 255, 255, 255, 255, 255]);
        // The last level averages black and white.
        assert_eq!(levels[2].2[0], 128);
        assert_eq!(material_mip_chain(1, 1, vec![9, 9, 9, 9]).len(), 1);
    }

    /// pat-3u1wu: every sampler key has its own cache slot.
    #[test]
    fn material_sampler_keys_are_dense_and_distinct() {
        let keys: Vec<MaterialSamplerKey> = material_sampler_keys().collect();
        assert_eq!(keys.len(), MATERIAL_SAMPLER_KINDS);
        for (i, k) in keys.iter().enumerate() {
            assert_eq!(k.index(), i);
        }
        let plain = material_sampler_descriptor(MaterialSamplerKey {
            filter: gdserver3d::material::TextureFilter::Nearest,
            clamp: false,
        });
        assert_eq!(plain.mag_filter, wgpu::FilterMode::Nearest);
        // No-mipmap modes read the base level only.
        assert_eq!(plain.lod_max_clamp, 0.0);
    }

    #[test]
    fn shader_source_compiles() {
        // Validate that the WGSL source parses without errors.
        // This doesn't need a GPU — naga can validate WGSL statically.
        let result = wgpu::naga::front::wgsl::parse_str(SHADER_3D_WGSL);
        assert!(result.is_ok(), "WGSL parse error: {:?}", result.err());
    }

    #[test]
    fn shader_has_directional_shadow_hook_and_pbr() {
        // pat-32edz / pat-sf07y: WGSL must wire shadow factor + Burley/GGX.
        assert!(SHADER_3D_WGSL.contains("directional_shadow_factor"));
        assert!(SHADER_3D_WGSL.contains("burley") || SHADER_3D_WGSL.contains("Burley") || SHADER_3D_WGSL.contains("fd90"));
        assert!(SHADER_3D_WGSL.contains("metallic"));
        // Real sampling — not the old always-1.0 placeholder.
        assert!(
            SHADER_3D_WGSL.contains("textureLoad(shadow_depth"),
            "WGSL must sample the bound shadow depth texture"
        );
        assert!(
            SHADER_3D_WGSL.contains("texture_2d_array"),
            "WGSL must use a shadow depth array (multi-light atlas)"
        );
        assert!(
            SHADER_3D_WGSL.contains("light.shadow_bias"),
            "WGSL must apply shadow bias matching software path"
        );
        assert!(
            !SHADER_3D_WGSL.contains("Placeholder: full light"),
            "placeholder shadow factor must be removed"
        );
        assert!(
            SHADER_3D_WGSL.contains("shadow_view_proj"),
            "light data must carry light-space view-projection"
        );
    }

    #[test]
    fn gpu_light_data_layout_includes_shadow_matrix() {
        // 64 (base) + 16 (shadow_opacity, cull_mask, shadow_bias,
        // shadow_normal_bias) + 64 (mat4) + 16 (specular + 3 pads) = 160 bytes
        // per light.
        //
        // ⚠️ Was 128 until the shadow row landed (pat-s3rrv.90). This module is
        // behind `--features gpu`, so a default-feature test run never compiles
        // it and a stale size here stays invisible; anyone growing
        // `GpuLightData` must update THIS assertion, `light_data_size` and
        // `light_array_size` below, and the byte-level layout tests.
        assert_eq!(std::mem::size_of::<GpuLightData>(), 176); // pat-772py row
        assert_eq!(std::mem::size_of::<LightArrayData>(), 176 * 16);
    }

    #[test]
    fn pack_lights_assigns_primary_shadow_slot() {
        use gdcore::math3d::Transform3D;
        use gdserver3d::instance::Instance3DId;
        use gdserver3d::light::Light3DId;
        use gdserver3d::mesh::Mesh3D;

        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        light.direction = Vector3::new(0.0, -1.0, 0.0);

        // Occluder so the map has written depth.
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(4.0));
        inst.visible = true;
        inst.transform = Transform3D::IDENTITY;

        let (data, maps) =
            pack_lights_with_shadows(&[light], std::iter::once(&inst), Vector3::ZERO);
        assert_eq!(maps.len(), 1, "one directional shadow map");
        assert_eq!(
            data.lights[0].shadow_map_index, 0,
            "primary directional light must bind slot 0"
        );
        assert_eq!(data.lights[0].shadow_enabled, 1);
        // Matrix must not be identity when a map was generated.
        let m = data.lights[0].shadow_view_proj;
        let is_identity = m == identity_mat4();
        assert!(!is_identity, "shadow_view_proj should be proj*view, not identity");
    }

    /// pat-mst61: two shadow-enabled directionals each get a unique layer index.
    #[test]
    fn pack_lights_assigns_multi_shadow_layers() {
        use gdcore::math3d::Transform3D;
        use gdserver3d::instance::Instance3DId;
        use gdserver3d::light::Light3DId;
        use gdserver3d::mesh::Mesh3D;

        let mut l0 = Light3D::directional(Light3DId(1));
        l0.shadow_enabled = true;
        l0.direction = Vector3::new(0.0, -1.0, 0.0);
        let mut l1 = Light3D::directional(Light3DId(2));
        l1.shadow_enabled = true;
        l1.direction = Vector3::new(-1.0, -1.0, 0.0).normalized();

        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        inst.visible = true;
        inst.transform = Transform3D::IDENTITY;

        let (data, maps) =
            pack_lights_with_shadows(&[l0, l1], std::iter::once(&inst), Vector3::ZERO);
        assert_eq!(maps.len(), 2, "two shadow maps for two lights");
        assert_eq!(data.lights[0].shadow_map_index, 0);
        assert_eq!(data.lights[1].shadow_map_index, 1);
    }

    #[test]
    fn mul_mat4_identity_is_noop() {
        let a = identity_mat4();
        let b = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [1.0, 2.0, 3.0, 1.0],
        ];
        assert_eq!(mul_mat4(&a, &b), b);
        assert_eq!(mul_mat4(&b, &a), b);
    }

    #[test]
    fn sky_shader_source_compiles() {
        let result = wgpu::naga::front::wgsl::parse_str(SKY_WGSL);
        assert!(result.is_ok(), "sky WGSL parse error: {:?}", result.err());
    }

    #[test]
    fn sky_uniform_size_and_alignment() {
        // 2 mat4x4 (128) + 4 vec4 colors (64) + params (16) + size (16)
        // + sun_dir_energy 4×vec4 (64) + sun_color_size 4×vec4 (64)
        // + sun_params (16) = 368.
        assert_eq!(std::mem::size_of::<SkyUniformData>(), 368);
    }

    /// The sky pass runs for procedural and panoramic sky materials.
    #[test]
    fn sky_uniform_only_for_procedural_sky() {
        use gdserver3d::sky::{PanoramicSkyMaterial, Sky, SkyMaterial};

        let vp = Viewport3D::new(64, 64);

        // Default environment: ClearColor → no sky pass.
        let env = Environment3D::default();
        assert!(sky_uniform_for_env(&env, &vp, 64, 64, &[]).is_none());

        // Sky mode without a sky resource → no sky pass.
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        assert!(sky_uniform_for_env(&env, &vp, 64, 64, &[]).is_none());

        // Sky mode with the default (procedural) sky → sky pass with Godot's
        // uniform mappings and the background energy multiplier.
        env.sky = Some(Sky::default());
        env.background_energy_multiplier = 2.0;
        let data = sky_uniform_for_env(&env, &vp, 64, 64, &[]).expect("procedural sky pass");
        assert_eq!(data.size[0], 64.0);
        assert_eq!(data.size[1], 64.0);
        assert!(data.sun_params[2] < 1000.0, "procedural mode flag");
        // Defaults: sky_curve 0.15 → inv 4.0, ground_curve 0.02 → inv 30.0
        // (Godot's `0.6 / curve` mapping); material energies stay 1.0 and the
        // environment background multiplier rides in sun_params.w.
        assert!((data.params[0] - 4.0).abs() < 1e-4);
        assert!((data.params[1] - 30.0).abs() < 1e-4);
        // Hemisphere energy is folded into the colors, so params.z/w stay 1.0.
        assert!((data.params[2] - 1.0).abs() < 1e-6);
        assert!((data.params[3] - 1.0).abs() < 1e-6);
        // Default ProceduralSkyMaterial top color, linearized (source_color).
        assert!((data.sky_top[0] - srgb_to_linear_channel(0.385)).abs() < 1e-5);
        assert!((data.sky_top[1] - srgb_to_linear_channel(0.454)).abs() < 1e-5);
        assert!((data.sky_top[2] - srgb_to_linear_channel(0.55)).abs() < 1e-5);
        // Sun uniforms: no suns → count 0; Godot's sun_angle_max default 30°
        // → cos ≈ 0.866, sun_curve 0.15 → inv_sun_curve = 1.6/0.15^1.4 ≈ 22.78.
        assert_eq!(data.sun_params[2], 0.0);
        assert!((data.sun_params[0] - 30f32.to_radians().cos()).abs() < 1e-6);
        assert!((data.sun_params[1] - 22.78).abs() < 0.05);
        assert!((data.sun_params[3] - 2.0).abs() < 1e-6);

        // pat-coz7r: panoramic material also yields a sky pass with equirect mode.
        env.sky = Some(Sky {
            material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
                energy_multiplier: 1.5,
                ..Default::default()
            }),
            ..Default::default()
        });
        let pano = sky_uniform_for_env(&env, &vp, 32, 32, &[]).expect("panoramic sky pass");
        assert!(
            pano.sun_params[2] >= 1000.0,
            "panoramic mode flag encoded in sun_params.z"
        );
        assert!(SKY_WGSL.contains("atan2(d.x, -d.z)"), "WGSL equirect u");
        assert!(SKY_WGSL.contains("acos(clamp(d.y"), "WGSL equirect v");
        // Godot's convention has NO half-turn shift — a `+ 0.5` on the
        // equirect u yaw-rotates every panorama by 180° (this regressed once).
        assert!(
            !SKY_WGSL.contains("3.14159265) + 0.5"),
            "WGSL equirect u must not carry a +0.5 shift"
        );
    }

    /// Sky suns flow into the uniform the way Godot feeds `LIGHTn_*` to the
    /// sky shader: direction toward the light, energy in w, color + angular
    /// size packed per light, count in sun_params.z.
    #[test]
    fn sky_uniform_packs_directional_suns() {
        use crate::renderer::SkySun;
        use gdcore::math::Vector3;
        use gdserver3d::sky::Sky;

        let vp = Viewport3D::new(64, 64);
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        env.sky = Some(Sky::default());

        let suns = [SkySun {
            direction: Vector3::new(0.0, 1.0, 0.0),
            color: Color::new(1.0, 0.9, 0.8, 1.0),
            energy: 2.5,
            size: 0.5f32.to_radians(),
            volumetric_fog_energy: 1.0,
        }];
        let data = sky_uniform_for_env(&env, &vp, 64, 64, &suns).expect("sky pass");
        assert_eq!(data.sun_params[2], 1.0);
        assert_eq!(data.sun_dir_energy[0][1], 1.0);
        assert!((data.sun_dir_energy[0][3] - 2.5).abs() < 1e-6);
        // Sun color is linearized (LIGHTn_COLOR); energy rides in dir_energy.w.
        assert!((data.sun_color_size[0][1] - srgb_to_linear_channel(0.9)).abs() < 1e-5);
        assert!((data.sun_color_size[0][3] - 0.5f32.to_radians()).abs() < 1e-6);
        // Unused slots stay zeroed.
        assert_eq!(data.sun_dir_energy[1], [0.0; 4]);
    }

    /// A physical sky packs its material through the shared uniform slots
    /// (pat-fw141): mode flag 2000+count in sun_params.z, coefficients in the
    /// color alpha channels, exposure = material energy × background energy.
    #[test]
    fn sky_uniform_packs_physical_material() {
        use crate::renderer::SkySun;
        use gdcore::math::Vector3;
        use gdserver3d::sky::{PhysicalSkyMaterial, Sky, SkyMaterial};

        let vp = Viewport3D::new(64, 64);
        let mut mat = PhysicalSkyMaterial::default();
        mat.energy_multiplier = 1.5;
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        env.background_energy_multiplier = 2.0;
        env.sky = Some(Sky {
            material: SkyMaterial::Physical(mat.clone()),
            ..Sky::default()
        });

        let suns = [SkySun {
            direction: Vector3::new(0.0, 1.0, 0.0),
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            energy: 1.0,
            size: 0.5f32.to_radians(),
            volumetric_fog_energy: 1.0,
        }];
        let data = sky_uniform_for_env(&env, &vp, 64, 64, &suns).expect("physical sky pass");
        assert_eq!(data.sun_params[2], 2001.0, "mode flag 2000 + sun count");
        assert!((data.sky_top[3] - mat.rayleigh_coefficient).abs() < 1e-6);
        assert!((data.sky_horizon[3] - mat.mie_coefficient).abs() < 1e-6);
        assert!((data.ground_bottom[3] - mat.turbidity).abs() < 1e-6);
        assert!((data.params[0] - mat.mie_eccentricity).abs() < 1e-6);
        assert!((data.params[1] - mat.sun_disk_scale).abs() < 1e-6);
        assert!(
            (data.sun_params[3] - 3.0).abs() < 1e-6,
            "exposure = material 1.5 × background 2.0"
        );
        // No sun → the mode flag alone (shader shows the black night sky).
        let data = sky_uniform_for_env(&env, &vp, 64, 64, &[]).expect("physical, no sun");
        assert_eq!(data.sun_params[2], 2000.0);
    }

    #[test]
    fn gpu_vertex_layout_size() {
        // position (3*4) + normal (3*4) + uv (2*4) = 32 bytes.
        assert_eq!(std::mem::size_of::<GpuVertex>(), 32);
    }

    #[test]
    fn camera_uniform_size() {
        // 2 mat4x4 (2*64) + vec3 (12) + pad (4) + ambient vec4 (16) = 160 bytes.
        assert_eq!(std::mem::size_of::<CameraUniformData>(), 160);
    }

    /// pat-p3p1m: the bit-built half decode is the float formula, exactly,
    /// for every finite half.
    #[test]
    fn f16_decode_matches_the_float_formula_for_every_finite_half() {
        for bits in 0u16..=u16::MAX {
            let exp = (bits >> 10) & 0x1f;
            if exp == 0x1f {
                continue;
            }
            let sign_f = if bits >> 15 == 1 { -1.0f32 } else { 1.0f32 };
            let frac = (bits & 0x3ff) as f32;
            let want = if exp == 0 {
                sign_f * frac * 2.0f32.powi(-24)
            } else {
                sign_f * (1.0 + frac / 1024.0) * 2.0f32.powi(exp as i32 - 15)
            };
            assert_eq!(f16_to_f32(bits).to_bits(), want.to_bits(), "half {bits:#06x}");
        }
    }

    #[test]
    fn model_uniform_size() {
        // mat4x4 (64) + 2*vec4 (32) + 2*f32 (8) + 2*u32 (8) + layers u32 (4)
        // + 3*u32 pad (12) = 128 bytes. Was 112 before `layers` (pat-s3rrv.90);
        // the pad keeps the struct a multiple of 16 for the uniform stride.
        // + the pat-p3p1m probe block (4 vec4 + 4 u32 = 80) = 208.
        assert_eq!(std::mem::size_of::<ModelUniformData>(), 208);
    }

    #[test]
    fn light_data_size() {
        // Base 64 + shadow row 16 + shadow_view_proj mat4 (64) + specular
        // row 16 = 160 (pat-32edz, grown twice by pat-s3rrv.90).
        assert_eq!(std::mem::size_of::<GpuLightData>(), 176); // pat-772py row
    }

    #[test]
    fn light_array_size() {
        // 16 * 176 = 2816 bytes (pat-772py added a row).
        assert_eq!(std::mem::size_of::<LightArrayData>(), 2816);
    }

    #[test]
    fn mesh_to_gpu_vertices_basic() {
        let mesh = Mesh3D {
            vertices: vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
            ],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            primitive_type: gdserver3d::mesh::PrimitiveType::Triangles,
            surfaces: vec![],
            ..Default::default()
        };
        let verts = mesh_to_gpu_vertices(&mesh);
        assert_eq!(verts.len(), 3);
        assert_eq!(verts[0].position, [0.0, 0.0, 0.0]);
        assert_eq!(verts[1].position, [1.0, 0.0, 0.0]);
        assert_eq!(verts[2].normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn transform_to_model_identity() {
        let m = transform_to_model_matrix(&Transform3D::IDENTITY);
        // Column-major identity.
        assert_eq!(m[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(m[1], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(m[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(m[3], [0.0, 0.0, 0.0, 1.0]);
    }

    /// The shadow cache key is stable for identical state and sensitive to
    /// exactly the inputs that change a shadow map — and with no shadow
    /// caster it ignores occluder churn entirely (pat-2be0q).
    #[test]
    fn shadow_state_key_tracks_relevant_changes() {
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_enabled = true;
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(1.0));

        let k = shadow_state_key(&[light.clone()], std::iter::once(&inst));
        assert_eq!(
            k,
            shadow_state_key(&[light.clone()], std::iter::once(&inst)),
            "identical state must hash identically"
        );

        let mut moved = inst.clone();
        moved.transform.origin.x += 1.0;
        assert_ne!(
            k,
            shadow_state_key(&[light.clone()], std::iter::once(&moved)),
            "moving an occluder must change the key"
        );

        let mut turned = light.clone();
        turned.direction = Vector3::new(0.3, -1.0, 0.0);
        assert_ne!(
            k,
            shadow_state_key(&[turned], std::iter::once(&inst)),
            "turning the light must change the key"
        );

        let mut no_shadow = light.clone();
        no_shadow.shadow_enabled = false;
        assert_eq!(
            shadow_state_key(&[no_shadow.clone()], std::iter::once(&inst)),
            shadow_state_key(&[no_shadow], std::iter::once(&moved)),
            "with no shadow caster, occluder moves must not churn the key"
        );
    }

    #[test]
    fn light_to_gpu_directional() {
        // A sun shining straight down (travel −Y) must reach the shader as the
        // toward-the-source direction (+Y), matching the software rasterizer —
        // the unnegated direction made every sun-facing surface ambient-black.
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, -1.0, 0.0);
        let gpu = light_to_gpu(&light, Vector3::ZERO);
        assert_eq!(gpu.kind, 0);
        assert_eq!(gpu.direction, [0.0, 1.0, 0.0]);
        assert_eq!(gpu.color, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn light_to_gpu_directional_normalizes() {
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(-3.0, -4.0, 0.0);
        let gpu = light_to_gpu(&light, Vector3::ZERO);
        assert!((gpu.direction[0] - 0.6).abs() < 1e-6);
        assert!((gpu.direction[1] - 0.8).abs() < 1e-6);
        assert!((gpu.direction[2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn light_to_gpu_sky_only_contributes_no_scene_color() {
        // SKY_ONLY: draws the sky sun disc but must not shade geometry, so the
        // shading color uploaded to the GPU is zero regardless of energy.
        let mut light = Light3D::directional(Light3DId(1));
        light.energy = 5.0;
        light.affects_scene = false;
        let gpu = light_to_gpu(&light, Vector3::ZERO);
        assert_eq!(gpu.color, [0.0, 0.0, 0.0], "SKY_ONLY adds no surface light");

        // The default (LIGHT_AND_SKY) still shades.
        light.affects_scene = true;
        let gpu = light_to_gpu(&light, Vector3::ZERO);
        assert!(gpu.color[0] > 0.0, "LIGHT_AND_SKY must still shade the scene");
    }

    /// 🔑 THE RUST AND WGSL `LightData` STRUCTS MUST AGREE BYTE FOR BYTE.
    ///
    /// ⚠️ A mismatch here is NOT a compile error — it is a silent misread of
    /// every light after the first, which shows up as garbled shadows rather
    /// than as a failure. `shadow_opacity` sits at offset 64 and the matrix has
    /// to start at 80, because WGSL gives `mat4x4<f32>` an align of 16 while
    /// Rust's `[[f32; 4]; 4]` has an align of 4 and would have placed it at 68.
    /// The three explicit `f32` pads are what close that gap; a `vec3<f32>`
    /// would not, since its own align of 16 pushes it to 80 in WGSL only.
    ///
    /// Checked through the actual bytes rather than `offset_of!`, so the
    /// assertion holds on any toolchain.
    #[test]
    fn shadow_opacity_gpu_layout_matches_the_wgsl_struct() {
        let mut light = Light3D::directional(Light3DId(1));
        light.shadow_opacity = 0.25;
        let mut gpu = light_to_gpu(&light, Vector3::ZERO);
        gpu.shadow_view_proj[0][0] = 7.5;
        let bytes = bytemuck::bytes_of(&gpu);

        assert_eq!(
            std::mem::size_of::<GpuLightData>(),
            // 176 since pat-772py's shadow-filter row.
            176,
            "the uniform ARRAY stride must stay a multiple of 16"
        );
        assert_eq!(std::mem::size_of::<GpuLightData>() % 16, 0);
        assert_eq!(
            &bytes[64..68],
            &0.25f32.to_le_bytes(),
            "shadow_opacity must sit at offset 64, right after shadow_map_index"
        );
        assert_eq!(
            &bytes[80..84],
            &7.5f32.to_le_bytes(),
            "shadow_view_proj must start at offset 80, the 16-byte boundary WGSL puts it on"
        );
    }

    /// The value has to be in the shader, in the struct AND in both shadow
    /// functions — the software renderer applies it in the directional/spot and
    /// the omni branch alike.
    ///
    /// ⚠️ Asserting the struct field alone would pass against a uniform that is
    /// uploaded and never read, which is the failure mode this whole bead is
    /// about.
    /// pat-772py: the shadow-filter fields sit where WGSL reads them. WGSL
    /// `LightData` puts `shadow_uv_scale` in `specular`'s last pad (offset
    /// 156) and the new row at 160..176.
    #[test]
    fn shadow_filter_gpu_row_matches_the_wgsl_struct() {
        let mut gpu = light_to_gpu(&Light3D::directional(Light3DId(1)), Vector3::ZERO);
        gpu.shadow_uv_scale = 0.125;
        gpu.shadow_depth_bias = 0.25;
        gpu.shadow_texel_world = 0.5;
        gpu.shadow_pcf_radius = 0.75;
        gpu.shadow_pcf_taps = 8;
        let bytes = bytemuck::bytes_of(&gpu);
        let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        assert_eq!(f(156), 0.125);
        assert_eq!(f(160), 0.25);
        assert_eq!(f(164), 0.5);
        assert_eq!(f(168), 0.75);
        assert_eq!(u32::from_le_bytes(bytes[172..176].try_into().unwrap()), 8);
        for field in [
            "shadow_uv_scale: f32",
            "shadow_depth_bias: f32",
            "shadow_texel_world: f32",
            "shadow_pcf_radius: f32",
            "shadow_pcf_taps: u32",
        ] {
            assert!(SHADER_3D_WGSL.contains(field), "WGSL LightData lacks {field}");
        }
        // A light with no fitted map keeps the legacy nearest compare.
        assert!(empty_gpu_light().shadow_pcf_radius < 0.0);
    }

    #[test]
    fn shadow_opacity_gpu_reaches_both_shader_branches() {
        assert!(
            SHADER_3D_WGSL.contains("shadow_opacity: f32"),
            "the WGSL LightData struct must carry the field"
        );
        assert_eq!(
            SHADER_3D_WGSL.matches("light.shadow_opacity").count(),
            3,
            "omni_shadow_factor, directional_shadow_factor and the camera-fitted \
             directional branch (pat-772py) must all use it"
        );
        // The old unconditional `return 0.0;` inside a shadow test is what this
        // replaces; a stray one left behind would mean one branch still ignores
        // the property.
        assert!(
            SHADER_3D_WGSL.contains("1.0 - clamp(light.shadow_opacity, 0.0, 1.0)"),
            "the shader must scale the OCCLUSION, matching `1 - occlusion * opacity`"
        );
    }

    /// `light_to_gpu` carries the value and clamps it, so an out-of-range
    /// `.tscn` value cannot make the GPU disagree with the software renderer
    /// (which clamps inside `compute_shadow_factor`).
    #[test]
    fn shadow_opacity_gpu_is_carried_and_clamped() {
        let mut light = Light3D::directional(Light3DId(1));
        assert_eq!(
            light_to_gpu(&light, Vector3::ZERO).shadow_opacity,
            1.0,
            "Godot's default is a fully opaque shadow"
        );

        light.shadow_opacity = 0.5;
        assert_eq!(light_to_gpu(&light, Vector3::ZERO).shadow_opacity, 0.5);

        light.shadow_opacity = 4.0;
        assert_eq!(
            light_to_gpu(&light, Vector3::ZERO).shadow_opacity,
            1.0,
            "clamped high"
        );
        light.shadow_opacity = -2.0;
        assert_eq!(
            light_to_gpu(&light, Vector3::ZERO).shadow_opacity,
            0.0,
            "clamped low"
        );
    }
    /// A positional light fixture matching the software renderer's own fade
    /// test, so the two paths are compared on identical numbers.
    ///
    /// `distance_fade_shadow` (5) is deliberately NEARER than
    /// `distance_fade_begin` (10): Godot fades a distant light's SHADOW out
    /// before the light itself.
    fn faded_point_light() -> Light3D {
        let mut light = Light3D::point(Light3DId(1), Vector3::ZERO);
        light.energy = 1.0;
        light.distance_fade_enabled = true;
        light.distance_fade_begin = 10.0;
        light.distance_fade_shadow = 5.0;
        light.distance_fade_length = 10.0;
        light
    }

    /// 🔑 THE GPU PATH USES THE SOFTWARE PATH'S OWN FADE FUNCTION.
    ///
    /// This bead exists because the engine has two render paths and one was
    /// lossy. A second transcription of the fade curve would recreate that on a
    /// smaller scale — and the divergence would only appear at a distance, which
    /// is the hardest place to notice it. So the assertion is not "the GPU fades
    /// too" but "the GPU's energy is EXACTLY the shared factor times the
    /// unfaded energy".
    #[test]
    fn distance_fade_gpu_uses_the_shared_factors() {
        let light = faded_point_light();
        for distance in [4.0f32, 15.0, 25.0] {
            let camera = Vector3::new(distance, 0.0, 0.0);
            let (energy_fade, _) = crate::renderer::distance_fade_factors(&light, camera);
            let gpu = light_to_gpu(&light, camera);
            let expected = srgb_to_linear_channel(light.color.r) * light.energy * energy_fade;
            assert!(
                (gpu.color[0] - expected).abs() < 1e-6,
                "at {distance} away the GPU energy must be the shared fade factor \
                 {energy_fade} applied to the light: got {}, expected {expected}",
                gpu.color[0]
            );
        }
    }

    /// The fade curve itself, in absolute terms — inside the threshold nothing
    /// changes, at half the ramp the light is at half strength, past the end it
    /// is gone.
    ///
    /// ⚠️ Stated separately from the test above because that one would still
    /// pass if BOTH paths shared the same WRONG curve. These numbers come from
    /// Godot's `smoothstep(0, 1, 1 - (d - begin) / length)`.
    #[test]
    fn distance_fade_gpu_dims_a_far_light() {
        let light = faded_point_light();
        let at = |d: f32| light_to_gpu(&light, Vector3::new(d, 0.0, 0.0)).color[0];
        let full = srgb_to_linear_channel(light.color.r);

        assert!(
            (at(4.0) - full).abs() < 1e-6,
            "inside distance_fade_begin the light is at full strength"
        );
        assert!(
            (at(15.0) - full * 0.5).abs() < 1e-5,
            "halfway along the ramp smoothstep gives exactly 0.5, got {}",
            at(15.0)
        );
        assert!(
            at(25.0).abs() < 1e-6,
            "past begin + length the light contributes nothing, got {}",
            at(25.0)
        );
    }

    /// 🔑 THE SHADOW FADES OUT BEFORE THE LIGHT DOES.
    ///
    /// At 15 units the energy is still at half strength while the shadow is
    /// already fully gone — `distance_fade_shadow` (5) is nearer than
    /// `distance_fade_begin` (10). Godot does this so the expensive term
    /// disappears first and the transition is not visible as a pop.
    ///
    /// ⚠️ A single fade factor reused for both would put these two at the same
    /// value and pass every other test in this file.
    #[test]
    fn distance_fade_gpu_fades_the_shadow_before_the_light() {
        let mut light = faded_point_light();
        light.shadow_opacity = 1.0;
        let camera = Vector3::new(15.0, 0.0, 0.0);
        let gpu = light_to_gpu(&light, camera);

        assert!(
            gpu.shadow_opacity.abs() < 1e-6,
            "the shadow must be gone at 15 units, got {}",
            gpu.shadow_opacity
        );
        let full = srgb_to_linear_channel(light.color.r);
        assert!(
            (gpu.color[0] - full * 0.5).abs() < 1e-5,
            "...while the light itself is still at half strength, got {}",
            gpu.color[0]
        );
    }

    /// ⚠️ DIRECTIONAL LIGHTS ARE EXEMPT, and that is not an oversight.
    ///
    /// Godot hides all four properties on DirectionalLight3D (`light_3d.cpp:561`:
    /// "the light LOD system only pertains to point lights") and its fade block
    /// runs over the omni/spot sorts alone. Fading the sun by the distance to
    /// whatever arbitrary point its node happens to sit at would be nonsense —
    /// and a directional light's position is exactly that.
    #[test]
    fn distance_fade_gpu_leaves_directional_lights_alone() {
        let mut light = Light3D::directional(Light3DId(1));
        light.energy = 1.0;
        light.distance_fade_enabled = true;
        light.distance_fade_begin = 10.0;
        light.distance_fade_length = 10.0;

        let near = light_to_gpu(&light, Vector3::ZERO).color[0];
        let far = light_to_gpu(&light, Vector3::new(1000.0, 0.0, 0.0)).color[0];
        assert!(
            (near - far).abs() < 1e-6 && near > 0.0,
            "a directional light must not fade with distance: {near} vs {far}"
        );
    }

    /// `light_negative` makes the light SUBTRACT, as a sign flip on the energy.
    ///
    /// ⚠️ Godot does it as `sign = negative ? -1 : 1` on the energy
    /// (`light_storage.cpp:840`), NOT as a colour change — the authored colour
    /// has to survive so introspection and re-tinting still see it. The uploaded
    /// colour therefore goes negative while `light.color` itself does not.
    #[test]
    fn distance_fade_gpu_carries_light_negative_as_an_energy_sign() {
        let mut light = Light3D::point(Light3DId(1), Vector3::ZERO);
        light.energy = 1.0;
        let positive = light_to_gpu(&light, Vector3::ZERO).color[0];
        assert!(positive > 0.0);

        light.negative = true;
        let negative = light_to_gpu(&light, Vector3::ZERO).color[0];
        assert!(
            (negative + positive).abs() < 1e-6,
            "a negative light must upload the exact negation: {negative} vs {positive}"
        );
        assert!(
            light.color.r > 0.0,
            "the AUTHORED colour must be untouched, got {}",
            light.color.r
        );
    }

    /// 🔑 THE GPU CULL MASK IS AN INTERSECTION OF TWO BITMASKS, AND THE STRUCTS
    /// MUST STILL AGREE BYTE FOR BYTE.
    ///
    /// `cull_mask` takes the FIRST slot reserved when `shadow_opacity` landed,
    /// so it cost no size at the time and the matrix stayed at 80.
    /// `ModelUniformData` grows a new 16-byte row instead, because its four
    /// trailing scalars were already full.
    ///
    /// ⚠️ `GpuLightData` is now **160** bytes: `light_specular` exhausted the
    /// reserved slots and added a row AFTER the matrix. Every offset asserted
    /// here is unchanged — that is the point of appending rather than
    /// inserting.
    ///
    /// ⚠️ Checked through real bytes rather than `offset_of!`, and on BOTH
    /// structs: a mismatch is a silent misread, not a compile error.
    #[test]
    fn cull_mask_gpu_layout_matches_the_wgsl_structs() {
        let mut light = Light3D::point(Light3DId(1), Vector3::ZERO);
        light.cull_mask = 0xABCD_1234;
        let gpu = light_to_gpu(&light, Vector3::ZERO);
        let bytes = bytemuck::bytes_of(&gpu);
        assert_eq!(
            std::mem::size_of::<GpuLightData>(),
            // 176 since pat-772py's shadow-filter row.
            176,
            "reusing a reserved pad must NOT move any offset below; the size \
             grew only later, when light_specular added a row after the matrix"
        );
        assert_eq!(
            &bytes[68..72],
            &0xABCD_1234u32.to_le_bytes(),
            "cull_mask sits at offset 68, the first reserved slot"
        );

        assert_eq!(
            std::mem::size_of::<ModelUniformData>() % 16,
            0,
            "the model uniform must stay a multiple of 16"
        );
        assert_eq!(std::mem::size_of::<ModelUniformData>(), 208);
        let model = ModelUniformData {
            model_matrix: identity_mat4(),
            albedo: [0.0; 4],
            emission: [0.0; 4],
            roughness: 0.0,
            metallic: 0.0,
            shading_mode: 0,
            num_lights: 0,
            layers: 0x0000_BEEF,
            has_normal_map: 0,
            normal_scale: 1.0,
            _model_pad: 0,
            ..bytemuck::Zeroable::zeroed()
        };
        assert_eq!(
            &bytemuck::bytes_of(&model)[112..116],
            &0x0000_BEEFu32.to_le_bytes(),
            "layers sits at offset 112, right after num_lights"
        );
    }

    /// The shader must actually TEST the mask, not merely carry it.
    ///
    /// ⚠️ Asserting the struct fields alone would pass against a uniform that is
    /// uploaded and never read — the exact failure this whole bead is about. And
    /// the test has to be an INTERSECTION: `layers` is a bitmask on both sides,
    /// so an index shift agrees for every single-layer mask (including the
    /// default 1) and mislights everything from layer 3 up.
    #[test]
    fn cull_mask_gpu_is_tested_in_the_shader() {
        assert!(
            SHADER_3D_WGSL.contains("(light.cull_mask & model.layers) == 0u"),
            "the WGSL light loop must skip a light whose mask misses this mesh"
        );
        assert!(
            SHADER_3D_WGSL.contains("    cull_mask: u32,"),
            "LightData must carry the mask"
        );
        assert!(
            SHADER_3D_WGSL.contains("    layers: u32,"),
            "ModelUniforms must carry the instance's layers"
        );
    }

    /// `light_to_gpu` carries the mask verbatim, defaults included.
    #[test]
    fn cull_mask_gpu_is_carried_verbatim() {
        let mut light = Light3D::point(Light3DId(1), Vector3::ZERO);
        assert_eq!(
            light_to_gpu(&light, Vector3::ZERO).cull_mask,
            u32::MAX,
            "Godot's default lights every layer"
        );
        light.cull_mask = 0b101;
        assert_eq!(light_to_gpu(&light, Vector3::ZERO).cull_mask, 0b101);
    }

    /// 🔑 THE WGSL ARRAY LENGTH IS A LITERAL AND CANNOT BE DERIVED FROM THE
    /// CONSTANT — so it is asserted instead.
    ///
    /// `LightArrayData` is now sized by `gdserver3d::light::MAX_SCENE_LIGHTS`,
    /// but the shader source says `array<LightData, 16>` in text. If someone
    /// raises the constant without editing the WGSL, the uniform buffer grows
    /// while the shader keeps reading 16 entries — every light past the
    /// sixteenth uploaded and silently ignored, with no error anywhere.
    #[test]
    fn max_scene_lights_matches_the_wgsl_array_length() {
        let n = gdserver3d::light::MAX_SCENE_LIGHTS;
        assert!(
            SHADER_3D_WGSL.contains(&format!("array<LightData, {n}>")),
            "the WGSL light array must be declared with {n} entries to match \
             MAX_SCENE_LIGHTS; raising the constant needs the shader edited too"
        );
        assert_eq!(
            LightArrayData {
                lights: [empty_gpu_light(); gdserver3d::light::MAX_SCENE_LIGHTS],
            }
            .lights
            .len(),
            n
        );
    }

    /// 🔑 THE WGSL DEPTH RANGE IS A LITERAL AND MUST MATCH THE RUST CONSTANTS.
    ///
    /// The 2D shadow compare divides the authored world-space `shadow_bias` by
    /// `SHADOW_DEPTH_RANGE` to reach the normalized depth it compares in. That
    /// range is `shadow_map::SHADOW_FAR - SHADOW_NEAR`, but WGSL cannot derive
    /// it — so if either constant moves and the shader does not, every GPU
    /// shadow silently gets the wrong bias while the software path stays right.
    #[test]
    fn shadow_bias_gpu_depth_range_matches_the_rust_constants() {
        let range = crate::shadow_map::SHADOW_FAR - crate::shadow_map::SHADOW_NEAR;
        assert!(
            SHADER_3D_WGSL.contains(&format!("SHADOW_DEPTH_RANGE: f32 = {range};")),
            "the WGSL depth range must equal SHADOW_FAR - SHADOW_NEAR ({range})"
        );
    }

    /// Both shader branches use the authored bias, each in its own space.
    ///
    /// ⚠️ The 2D branch DIVIDES by the depth range and the omni branch does NOT —
    /// cubemap depths are a linear distance from the light, so they are already
    /// in the world units the property is authored in. Asserting only that the
    /// field is referenced would pass against a shader that divided in both.
    #[test]
    fn shadow_bias_gpu_is_applied_in_both_spaces() {
        assert!(
            SHADER_3D_WGSL.contains("stored_depth + light.shadow_bias / SHADOW_DEPTH_RANGE"),
            "the 2D compare must convert the world bias to normalized depth"
        );
        assert!(
            SHADER_3D_WGSL.contains("stored + light.shadow_bias"),
            "the omni compare must use the world bias directly"
        );
        // The constants it replaces are gone, so neither branch can silently
        // fall back to a hardcoded value.
        assert!(!SHADER_3D_WGSL.contains("OMNI_SHADOW_BIAS"));
    }

    /// The uniform carries the value, and reusing a reserved pad kept the layout
    /// byte-identical.
    #[test]
    fn shadow_bias_gpu_layout_is_unchanged() {
        let mut light = Light3D::spot(Light3DId(1), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0));
        assert!(
            (light.shadow_bias - 0.03).abs() < 1e-6,
            "a SpotLight3D's Godot default is 0.03, not the base 0.1"
        );
        light.shadow_bias = 0.25;
        let gpu = light_to_gpu(&light, Vector3::ZERO);
        let bytes = bytemuck::bytes_of(&gpu);

        assert_eq!(
            std::mem::size_of::<GpuLightData>(),
            // 176 since pat-772py's shadow-filter row.
            176,
            "offsets below the matrix unchanged; 160 since light_specular's row"
        );
        assert_eq!(
            &bytes[72..76],
            &0.25f32.to_le_bytes(),
            "shadow_bias sits at offset 72, the second reserved slot"
        );
        assert_eq!(
            &bytes[80..84],
            &gpu.shadow_view_proj[0][0].to_le_bytes(),
            "the matrix still starts at offset 80"
        );
    }

    /// 🔑 THE THIRD WGSL LITERAL THAT MUST TRACK A RUST CONSTANT.
    ///
    /// `SHADOW_TEXEL_SIZE` is `2 * SHADOW_EXTENT / SHADOW_MAP_SIZE`, and WGSL
    /// cannot derive it. Moving either constant without editing the shader
    /// leaves every GPU normal offset scaled wrong while the software path stays
    /// right — the same silent divergence class as `SHADOW_DEPTH_RANGE` and the
    /// light-array length, both already guarded here.
    #[test]
    fn shadow_texel_size_gpu_matches_the_rust_constants() {
        let texel =
            2.0 * crate::shadow_map::SHADOW_EXTENT / crate::shadow_map::SHADOW_MAP_SIZE as f32;
        assert!(
            SHADER_3D_WGSL.contains(&format!("SHADOW_TEXEL_SIZE: f32 = {texel};")),
            "the WGSL texel size must equal 2*SHADOW_EXTENT/SHADOW_MAP_SIZE ({texel})"
        );
    }

    /// The normal offset is applied in BOTH shadow branches, with the positional
    /// ×10 and the grazing-angle term.
    ///
    /// ⚠️ Asserting the field is merely referenced would pass against a shader
    /// that dropped the `1 - |dot|` factor — which is what makes it a NORMAL
    /// bias rather than a second constant one — or that applied the ×10 to
    /// directional lights too.
    #[test]
    fn shadow_normal_bias_gpu_is_applied_in_both_branches() {
        assert_eq!(
            SHADER_3D_WGSL
                .matches("shadow_normal_offset(light, world_pos, normal)")
                .count(),
            2,
            "both the omni and the 2D shadow branch must offset the sample point"
        );
        assert!(
            SHADER_3D_WGSL.contains("(1.0 - abs(dot(normal, light_dir)))"),
            "the grazing-angle term is what distinguishes a normal bias from a \
             constant one"
        );
        // The x10 is keyed on light kind: directional (0) is unscaled.
        assert!(
            SHADER_3D_WGSL.contains("var type_scale = 10.0;")
                && SHADER_3D_WGSL.contains("if (light.kind == 0u) {"),
            "positional lights take the x10; directional does not"
        );
    }

    /// The last reserved slot is used, and the layout is still unchanged.
    #[test]
    fn shadow_normal_bias_gpu_fills_the_last_reserved_slot() {
        let mut light = Light3D::point(Light3DId(1), Vector3::ZERO);
        assert!(
            (light.shadow_normal_bias - 1.0).abs() < 1e-6,
            "an OmniLight3D's Godot default is 1.0, not the base 2.0"
        );
        light.shadow_normal_bias = 0.75;
        let gpu = light_to_gpu(&light, Vector3::ZERO);
        let bytes = bytemuck::bytes_of(&gpu);

        assert_eq!(
            std::mem::size_of::<GpuLightData>(),
            // 176 since pat-772py's shadow-filter row.
            176,
            "offsets below the matrix unchanged; 160 since light_specular's row"
        );
        assert_eq!(
            &bytes[76..80],
            &0.75f32.to_le_bytes(),
            "shadow_normal_bias sits at offset 76, the last reserved slot"
        );
        assert_eq!(
            &bytes[80..84],
            &gpu.shadow_view_proj[0][0].to_le_bytes(),
            "the matrix still starts at offset 80"
        );
    }

    /// 🔑 `light_specular` REACHES THE GPU, IN A NEW ROW THAT MOVES NOTHING.
    ///
    /// The three slots reserved when `shadow_opacity` landed were spent by
    /// `cull_mask`/`shadow_bias`/`shadow_normal_bias`, so this property is the
    /// first to grow `GpuLightData` since the shadow matrix: 144 → **160**.
    ///
    /// ⚠️ The row goes AFTER `shadow_view_proj`. Inserting it before would push
    /// the matrix off the 16-byte boundary WGSL requires and silently
    /// reinterpret every field the shader already reads — a wrong picture, not
    /// a compile error. Both facts are asserted through real bytes.
    #[test]
    fn light_specular_gpu_layout_adds_a_row_without_moving_the_matrix() {
        let mut light = Light3D::spot(Light3DId(1), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0));
        assert!(
            (light.specular - 0.5).abs() < 1e-6,
            "a SpotLight3D's Godot default is 0.5, not the base 1.0"
        );
        light.specular = 0.25;
        let mut gpu = light_to_gpu(&light, Vector3::ZERO);
        gpu.shadow_view_proj[0][0] = 7.5;
        let bytes = bytemuck::bytes_of(&gpu);

        assert_eq!(
            std::mem::size_of::<GpuLightData>(),
            // 176 since pat-772py's shadow-filter row.
            176,
            "specular's row grows the struct from 144 to 160"
        );
        assert_eq!(
            std::mem::size_of::<GpuLightData>() % 16,
            0,
            "the uniform ARRAY stride must stay a multiple of 16"
        );
        assert_eq!(
            &bytes[80..84],
            &7.5f32.to_le_bytes(),
            "the matrix must STILL start at offset 80 — appending after it is \
             the whole reason this layout change is safe"
        );
        assert_eq!(
            &bytes[144..148],
            &0.25f32.to_le_bytes(),
            "specular sits at offset 144, immediately after the matrix"
        );
        // ⚠️ Was `bytes[148..160] == [0u8; 12]` when specular's row was all
        // pad. Two of those three slots are now
        // `directional_shadow_max_distance` / `_fade_start`, so only the last
        // four bytes are still pad. Left as a byte assertion rather than
        // deleted: a stale non-zero tail is exactly what a future field
        // forgetting its initializer would look like.
        //
        // pat-772py spent that last pad on `shadow_uv_scale`, whose neutral
        // value is 1.0 (the map fills its whole atlas layer), so the tail is
        // now an explicit 1.0 rather than zero.
        assert_eq!(
            &bytes[156..160],
            &1.0f32.to_le_bytes(),
            "the last of specular's pads is shadow_uv_scale, explicitly 1.0"
        );
    }

    /// 🔑 THE x2 FOR POSITIONAL LIGHTS IS A WGSL LITERAL THAT NO RUST CONSTANT
    /// CAN REACH — the fourth of its kind in this shader.
    ///
    /// Godot stores `light_specular` raw for directional lights
    /// (`light_storage.cpp:665`) and doubles it for positional ones (`:891`),
    /// against defaults of 1.0 (`Light3D`/`DirectionalLight3D`) and 0.5
    /// (`OmniLight3D`/`SpotLight3D`). The two cancel, so every light type sits
    /// at an effective 1.0 and the property is invisible until authored.
    ///
    /// ⚠️ Dropping the x2 on this side alone would leave the GPU path exactly
    /// HALF as glossy as the software one for every omni and spot light, at
    /// default values, with no test failing anywhere else. Same silent-
    /// divergence class as `SHADOW_DEPTH_RANGE` and `SHADOW_TEXEL_SIZE`.
    #[test]
    fn light_specular_wgsl_doubles_positional_lights_only() {
        assert!(
            SHADER_3D_WGSL.contains("specular: f32,"),
            "LightData must carry the authored specular"
        );
        assert!(
            SHADER_3D_WGSL.contains("select(2.0, 1.0, light.kind == 0u)"),
            "positional lights (kind 1/2) must be doubled and directional \
             (kind 0) left raw — select(false_val, true_val, cond)"
        );
        assert!(
            SHADER_3D_WGSL.contains("light.specular * specular_type_scale"),
            "the scale must actually multiply the specular accumulation, not \
             just be computed"
        );
    }

    /// 🔑 THE CPU AND GPU PATHS AGREE ON THE EFFECTIVE SPECULAR AT EVERY CLASS
    /// DEFAULT.
    ///
    /// The software renderer applies the same authored value and the same
    /// per-kind scale in `pbr_shade`. Uploading the value while the shader
    /// applied a different scale — or the reverse — is the failure this pins:
    /// both halves are individually plausible and only disagree in the picture.
    #[test]
    fn light_specular_uploads_the_authored_value_for_every_light_kind() {
        let directional = Light3D::directional(Light3DId(1));
        let omni = Light3D::point(Light3DId(2), Vector3::ZERO);
        let spot = Light3D::spot(Light3DId(3), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0));

        // Godot's per-class defaults, and the kind tags the shader switches on.
        for (light, want_specular, want_kind) in [
            (&directional, 1.0f32, 0u32),
            (&omni, 0.5, 1),
            (&spot, 0.5, 2),
        ] {
            let gpu = light_to_gpu(light, Vector3::ZERO);
            assert_eq!(gpu.kind, want_kind, "kind tag drives the x2 in WGSL");
            assert!(
                (gpu.specular - want_specular).abs() < 1e-6,
                "kind {want_kind} must upload {want_specular}, got {}",
                gpu.specular
            );
            // The scale the shader will apply, mirrored here.
            let scale = if want_kind == 0 { 1.0 } else { 2.0 };
            assert!(
                (gpu.specular * scale - 1.0).abs() < 1e-6,
                "every light type must come out at an effective 1.0 at its \
                 class default — that is what makes this change additive"
            );
        }

        // Positive control: an authored value is not flattened to the default.
        let mut loud = omni;
        loud.specular = 0.25;
        assert!(
            (light_to_gpu(&loud, Vector3::ZERO).specular - 0.25).abs() < 1e-6,
            "an authored specular must survive the upload"
        );
    }
    /// 🔑 THE DIRECTIONAL SHADOW FADE REACHES THE GPU IN THE SLOTS SPECULAR
    /// RESERVED — the struct does NOT grow again.
    ///
    /// `specular` added a 16-byte row with three spare `f32`s; two of them are
    /// now `directional_shadow_max_distance` and `_fade_start`, so
    /// `GpuLightData` is still 160 bytes and every offset is where it was.
    ///
    /// ⚠️ Asserted through real bytes, because this is exactly the change that
    /// looks free and is not: reordering the row, or padding it differently,
    /// would leave the Rust and WGSL structs disagreeing with no compile error
    /// on either side.
    #[test]
    fn directional_shadow_fade_gpu_fills_two_of_speculars_reserved_slots() {
        let mut light = Light3D::directional(Light3DId(1));
        assert_eq!(
            light.directional_shadow_max_distance, 100.0,
            "DirectionalLight3D's Godot default"
        );
        assert_eq!(light.directional_shadow_fade_start, 0.8, "Godot default");

        light.directional_shadow_max_distance = 40.0;
        light.directional_shadow_fade_start = 0.25;
        let gpu = light_to_gpu(&light, Vector3::ZERO);
        let bytes = bytemuck::bytes_of(&gpu);

        assert_eq!(
            std::mem::size_of::<GpuLightData>(),
            // 176 since pat-772py's shadow-filter row.
            176,
            "reusing specular's reserved pads must NOT grow the struct"
        );
        assert_eq!(
            &bytes[148..152],
            &40.0f32.to_le_bytes(),
            "directional_shadow_max_distance sits at offset 148"
        );
        assert_eq!(
            &bytes[152..156],
            &0.25f32.to_le_bytes(),
            "directional_shadow_fade_start sits at offset 152"
        );
        assert_eq!(
            &bytes[144..148],
            &gpu.specular.to_le_bytes(),
            "specular must not have moved off offset 144"
        );
    }

    /// 🔑 THE FADE IS DIRECTIONAL-ONLY, AND THE 0.999 CLAMP IS IN THE SHADER.
    ///
    /// ⚠️ `directional_shadow_factor` serves SPOT lights too (its guard admits
    /// `kind == 0 || kind == 2`), so the `light.kind != 0u` test in the fade is
    /// load-bearing, not defensive: without it every spot light would also lose
    /// its shadow past `directional_shadow_max_distance`, which is a
    /// DirectionalLight3D property that spot lights never author.
    ///
    /// ⚠️ `min(fade_start, 0.999)` is Godot's guard against both fade edges
    /// coinciding and dividing by zero. It has to live in the shader — the
    /// authored 1.0 must survive the upload, which the byte test above pins.
    #[test]
    fn directional_shadow_fade_wgsl_is_directional_only_and_clamps_fade_start() {
        assert!(
            SHADER_3D_WGSL.contains("directional_shadow_max_distance: f32,")
                && SHADER_3D_WGSL.contains("directional_shadow_fade_start: f32,"),
            "LightData must carry both fade parameters"
        );
        // ⚠️ The WHOLE guard line, not just `light.kind != 0u`. That fragment
        // already appears in `directional_shadow_factor`'s own admission test
        // (`kind != 0u && kind != 2u`), so asserting it alone passes with this
        // fade deleted — a green test proving nothing.
        assert!(
            SHADER_3D_WGSL.contains(
                "if (light.kind != 0u || light.directional_shadow_max_distance <= 0.0) {"
            ),
            "the fade must skip every non-directional light (spot included) and \
             short-circuit a zero shadow distance, which would be 0/0 -> NaN"
        );
        assert!(
            SHADER_3D_WGSL.contains("min(light.directional_shadow_fade_start, 0.999)"),
            "fade_start must be clamped in the shader, or fade_from == fade_to \
             and the ratio divides by zero"
        );
        assert!(
            SHADER_3D_WGSL.contains(
                "let shadow = directional_shadow_fade(light, in.world_position, raw_shadow);"
            ),
            "the fade must actually wrap the shadow term, not merely be defined"
        );
    }

    /// 🔑 THE GPU FADE IS THE SAME CURVE AS THE SOFTWARE ONE.
    ///
    /// The WGSL cannot be executed here, so this pins the two halves that CAN
    /// diverge silently: the uploaded parameters, and the arithmetic. The
    /// expected values come from `gdrender3d::shader`'s own port, so if either
    /// side is edited alone this fails.
    ///
    /// ⚠️ The smoothstep is checked at the QUARTER point. Smoothstep and a
    /// linear ramp agree exactly at the midpoint, so a midpoint-only assertion
    /// cannot tell a cubic from a lerp.
    #[test]
    fn directional_shadow_fade_gpu_parameters_match_the_software_curve() {
        let light = Light3D::directional(Light3DId(1));
        let gpu = light_to_gpu(&light, Vector3::ZERO);

        // The shader recomputes the edges from these two numbers, so agreeing
        // on them is what makes the WGSL arithmetic above meaningful.
        assert_eq!(gpu.directional_shadow_max_distance, 100.0);
        assert_eq!(gpu.directional_shadow_fade_start, 0.8);
        assert_eq!(gpu.kind, 0, "only kind 0 is faded");

        // The curve the WGSL implements, mirrored from the uploaded values.
        let fade = |view_z: f32| {
            let fade_to = -gpu.directional_shadow_max_distance;
            let fade_from =
                -gpu.directional_shadow_max_distance * gpu.directional_shadow_fade_start.min(0.999);
            let t = ((view_z - fade_from) / (fade_to - fade_from)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        assert_eq!(fade(-50.0), 0.0, "inside fade_start, nothing fades");
        assert_eq!(fade(-80.0), 0.0, "exactly at fade_start");
        assert!(
            (fade(-85.0) - 0.15625).abs() < 1e-6,
            "quarter point is the cubic, not 0.25"
        );
        assert!((fade(-90.0) - 0.5).abs() < 1e-6, "midpoint");
        assert_eq!(fade(-100.0), 1.0, "at max_distance the shadow is gone");
        assert_eq!(fade(-500.0), 1.0, "and it stays clamped");
    }

    /// pat-zwvgw: a mesh's additional surfaces become draws of their own,
    /// each with its `surface_materials` entry (or the instance material), and
    /// a `BLEND_MODE_ADD` material says so on its draw (pat-cezo4).
    ///
    /// Before this the GPU drew only the primary surface, so the platformer's
    /// three-material enemy lost two of its parts.
    #[test]
    fn additional_surfaces_draw_with_their_own_materials() {
        use gdserver3d::material::BlendMode;
        let mut mesh = Mesh3D::cube(1.0);
        let extra = Mesh3D::cube(0.5);
        mesh.surfaces.push(gdserver3d::mesh::Surface3D {
            vertices: extra.vertices.clone(),
            normals: extra.normals.clone(),
            uvs: extra.uvs.clone(),
            indices: extra.indices.clone(),
            primitive_type: extra.primitive_type,
            lods: Vec::new(),
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
        });
        let mut inst = Instance3D::new(gdserver3d::instance::Instance3DId(1));
        inst.material = Some(Material3D {
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
            ..Default::default()
        });
        inst.surface_materials = vec![Some(Material3D {
            albedo: Color::new(0.0, 0.0, 1.0, 1.0),
            blend_mode: BlendMode::Add,
            ..Default::default()
        })];
        let surfaces = split_surface_meshes(&mesh);
        inst.mesh = Some(mesh);
        let entries = instance_draw_entries(&inst, &surfaces);
        assert_eq!(entries.len(), 2, "primary + one extra surface");
        assert_eq!(entries[0].albedo, Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(entries[0].blend_mode, BlendMode::Mix);
        assert_eq!(entries[1].albedo, Color::new(0.0, 0.0, 1.0, 1.0));
        assert_eq!(entries[1].blend_mode, BlendMode::Add);
        assert_eq!(entries[1].mesh.vertices, extra.vertices);

        // No override for the extra surface: it falls back to the instance's.
        inst.surface_materials.clear();
        let entries = instance_draw_entries(&inst, &surfaces);
        assert_eq!(entries[1].albedo, Color::new(1.0, 0.0, 0.0, 1.0));
    }

    /// pat-cezo4: a cubemap shader sky that the host baked into a panorama
    /// draws through the sky pass's equirect branch with the bake bound
    /// (`params.w = 2`), instead of the default procedural stand-in.
    #[test]
    fn a_baked_shader_sky_uses_the_textured_equirect_pass() {
        use gdserver3d::sky::{PanoramaTexture, ShaderSkyMaterial, Sky, SkyMaterial};
        let mut mat = ShaderSkyMaterial::new(gdserver3d::shader::ShaderMaterial3D::new());
        mat.baked = Some(std::sync::Arc::new(PanoramaTexture {
            width: 2,
            height: 1,
            pixels: vec![Color::new(0.0, 0.0, 1.0, 1.0); 2],
        }));
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        env.sky = Some(Sky {
            material: SkyMaterial::Shader(mat),
            ..Default::default()
        });
        let vp = Viewport3D::new(32, 32);
        let data = sky_uniform_for_env(&env, &vp, 32, 32, &[]).expect("sky pass");
        assert!(data.sun_params[2] >= 1000.0, "equirect mode");
        assert_eq!(data.params[3], 2.0, "panorama bound");
        assert!(SKY_WGSL.contains("textureSampleLevel(sky_panorama"));
        assert!(wgpu::naga::front::wgsl::parse_str(SKY_WGSL).is_ok());
    }
}
