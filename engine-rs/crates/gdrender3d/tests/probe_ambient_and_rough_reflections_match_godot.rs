//! pat-t42ca + pat-9gjpr: ReflectionProbe ambient, roughness-filtered
//! reflections, and Godot's light-energy scale, on the software PBR path
//! (`pbr_shade`, which every `.tscn` StandardMaterial3D takes).
//!
//! What Godot does (4.6.1, `scene_forward_clustered.glsl` +
//! `scene_forward_lights_inc.glsl`):
//!
//! - Inside a probe whose `ambient_mode` is the default `Environment`, the
//!   ambient light is REPLACED by the probe capture's most-blurred level
//!   (`textureLod(…, MAX_ROUGHNESS_LOD)`) along the normal, weighted by the
//!   probe's fade (`reflection_process` 981-992, then 2082-2096). Patina used
//!   to ADD `ambient_color * energy` (black by default) and keep the flat
//!   environment ambient: on platformer_3d's floor that made shadows read 63
//!   where Godot reads 39.
//! - Reflections are sampled from a GGX-prefiltered mip chain at
//!   `sqrt(roughness) * MAX_ROUGHNESS_LOD`. Patina read one nearest texel, so
//!   the 0.4-rough metal robot showed hard mirror streaks.
//! - Every light's energy is multiplied by PI in non-physical units
//!   (light_storage.cpp:653/:881), cancelling the 1/PI inside Burley and
//!   D_GGX. Without it a PBR floor took a third of Godot's direct light.
//!
//! Measured on platformer_3d f90 (compare-to-godot, grass pixels): Godot
//! lit/shadow 115.7/39.3; Patina before 91.7/63.0, after 114.6/38.0.

use gdcore::math::{Color, Vector3};
use gdrender3d::shader::{
    pbr_shade, FragmentInput, LightUniform, MaterialTextures, ShaderUniforms,
};
use gdserver3d::reflection_probe::{
    CubeFace, ProbeReflection, ReflectionCubemap, ReflectionProbeAmbientMode,
};
use std::sync::Arc;

const FLAT_AMBIENT: f32 = 0.6;

fn probe(cube: ReflectionCubemap, mode: ReflectionProbeAmbientMode, blend: f32) -> ProbeReflection {
    ProbeReflection {
        cubemap: Arc::new(cube),
        box_min: Vector3::new(-10.0, -10.0, -10.0),
        box_max: Vector3::new(10.0, 10.0, 10.0),
        origin: Vector3::ZERO,
        box_projection: false,
        intensity: 1.0,
        blend_distance: blend,
        ambient_mode: mode,
        ambient_color: Color::new(0.0, 0.0, 0.5, 1.0),
    }
}

/// One fragment of a white, upward-facing surface at `pos`, seen from
/// straight above.
fn shade(
    pos: Vector3,
    metallic: f32,
    roughness: f32,
    camera: Vector3,
    probe: Option<ProbeReflection>,
    lights: Vec<LightUniform>,
) -> Color {
    let input = FragmentInput {
        world_position: pos,
        world_normal: Vector3::new(0.0, 1.0, 0.0),
        uv: [0.5, 0.5],
        depth: 0.0,
    };
    let uniforms = ShaderUniforms {
        model_matrix: [[0.0; 4]; 4],
        view_matrix: [[0.0; 4]; 4],
        projection_matrix: [[0.0; 4]; 4],
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        emission: Color::new(0.0, 0.0, 0.0, 1.0),
        ambient: Color::new(FLAT_AMBIENT, FLAT_AMBIENT, FLAT_AMBIENT, 1.0),
        roughness,
        metallic,
        camera_position: camera,
        probe: probe.map(Into::into),
        layers: 1,
        lights,
        directional_shadow_maps: Vec::new(),
        omni_shadow_cubemaps: Vec::new(),
        textures: MaterialTextures::default(),
    };
    pbr_shade(&input, &uniforms)
}

fn above(pos: Vector3) -> Vector3 {
    pos + Vector3::new(0.0, 3.0, 0.0)
}

/// 🔑 pat-t42ca acceptance: a dark capture DARKENS the ambient.
///
/// Uniform 0.1 capture, rough white dielectric, no lights. Godot's ambient is
/// the capture (0.1), not the flat 0.6; the diffuse is `0.1 * albedo` plus a
/// tiny `f0 * envBRDF` specular. Before the fix this read 0.6.
#[test]
fn environment_probe_replaces_the_flat_ambient_with_its_capture() {
    let cube = ReflectionCubemap::new(8, Color::new(0.1, 0.1, 0.1, 1.0));
    let p = Vector3::ZERO;
    let got = shade(
        p,
        0.0,
        1.0,
        above(p),
        Some(probe(cube, ReflectionProbeAmbientMode::Environment, 0.0)),
        vec![],
    );
    assert!(
        got.g > 0.09 && got.g < 0.13,
        "inside an Environment-mode probe the ambient must be the capture's \
         blurred radiance (0.1), not the flat environment ambient \
         ({FLAT_AMBIENT}); got {got:?}"
    );
}

/// Control: `ambient_mode = Disabled` keeps the environment ambient — this is
/// Godot's "all probes ambient_mode=0" measurement, which equals Patina's old
/// shadow value exactly.
#[test]
fn disabled_probe_keeps_the_environment_ambient() {
    let cube = ReflectionCubemap::new(8, Color::new(0.1, 0.1, 0.1, 1.0));
    let p = Vector3::ZERO;
    let got = shade(
        p,
        0.0,
        1.0,
        above(p),
        Some(probe(cube, ReflectionProbeAmbientMode::Disabled, 0.0)),
        vec![],
    );
    assert!(
        got.g > FLAT_AMBIENT - 0.01 && got.g < FLAT_AMBIENT + 0.03,
        "a Disabled probe must leave the flat ambient alone; got {got:?}"
    );
}

/// ConstantColor replaces the ambient with `ambient_color * energy`.
#[test]
fn constant_color_probe_replaces_the_ambient_with_its_colour() {
    let cube = ReflectionCubemap::new(8, Color::new(0.1, 0.1, 0.1, 1.0));
    let p = Vector3::ZERO;
    let got = shade(
        p,
        0.0,
        1.0,
        above(p),
        Some(probe(cube, ReflectionProbeAmbientMode::ConstantColor, 0.0)),
        vec![],
    );
    assert!(
        got.r < 0.05 && got.b > 0.45 && got.b < 0.56,
        "ConstantColor must REPLACE the ambient with (0, 0, 0.5); got {got:?}"
    );
}

/// The replacement is weighted by the probe's boundary fade: at the box face
/// (blend 0) the flat ambient is back, in the middle of the band it is mixed.
#[test]
fn probe_ambient_fades_back_to_the_environment_at_the_box_edge() {
    let mk = || {
        probe(
            ReflectionCubemap::new(8, Color::new(0.1, 0.1, 0.1, 1.0)),
            ReflectionProbeAmbientMode::Environment,
            2.0,
        )
    };
    let at = |x: f32| {
        let p = Vector3::new(x, 0.0, 0.0);
        shade(p, 0.0, 1.0, above(p), Some(mk()), vec![]).g
    };
    let (inside, mid, edge) = (at(0.0), at(9.0), at(10.0));
    assert!(
        inside < 0.13,
        "full-weight interior takes the capture: {inside}"
    );
    assert!(
        edge > FLAT_AMBIENT - 0.01,
        "at the face the env ambient is back: {edge}"
    );
    assert!(mid > inside && mid < edge, "the band mixes the two: {mid}");
}

/// A capture that is bright only on +X and black elsewhere.
fn half_lit_cube() -> ReflectionCubemap {
    let res = 16;
    let mut cube = ReflectionCubemap::new(res, Color::new(0.0, 0.0, 0.0, 1.0));
    cube.set_face(
        CubeFace::PosX,
        vec![Color::new(1.0, 1.0, 1.0, 1.0); (res * res) as usize],
    );
    cube
}

/// 🔑 pat-9gjpr acceptance: a 0.4-rough metal's reflection is BLURRED.
///
/// The view is set so the mirror direction lands just across the +X face
/// edge, on the black +Y face. A nearest-texel mirror (the old code) reads
/// exactly 0 there; Godot's lod `sqrt(0.4) * 7 = 4.4` GGX level spreads the
/// bright face over it.
#[test]
fn a_rough_metal_reflection_is_roughness_filtered() {
    let p = Vector3::ZERO;
    // Mirror direction r = 2(n.v)n - v with n = +Y; pick r past the 45 degree
    // edge between +X and +Y, on the +Y face: r = (0.85, 1, 0), u = 0.925.
    // ⚠️ Not closer: since pat-h7sz7 a lookup filters ACROSS the face seam
    // (Godot's octmaps have a border), so a mirror within half a texel of the
    // edge (the old 0.95, u = 0.975 at 16 texels) rightly picks up the bright
    // neighbour face, and the roughness-0 control below would not be black.
    let r = Vector3::new(0.85, 1.0, 0.0).normalized();
    let v = Vector3::new(-r.x, r.y, -r.z);
    let camera = p + v * 3.0;
    let dark_side = shade(
        p,
        1.0,
        0.4,
        camera,
        Some(probe(
            half_lit_cube(),
            ReflectionProbeAmbientMode::Disabled,
            0.0,
        )),
        vec![],
    );
    let mirror = shade(
        p,
        1.0,
        0.0,
        camera,
        Some(probe(
            half_lit_cube(),
            ReflectionProbeAmbientMode::Disabled,
            0.0,
        )),
        vec![],
    );
    assert!(
        mirror.r < 0.05,
        "control: a perfect mirror (roughness 0) aimed onto the black face \
         must read black; got {mirror:?}"
    );
    assert!(
        dark_side.r > 0.1,
        "a 0.4-rough metal must reflect a blurred capture (GGX level at lod \
         sqrt(0.4)*7 = 4.4), which spreads the bright +X face across the edge; \
         got {dark_side:?} — a nearest-texel lookup reads 0 here"
    );
}

/// The blurred levels conserve the capture's average: a uniform capture reads
/// back the same at every lod.
#[test]
fn roughness_levels_preserve_a_uniform_capture() {
    let cube = ReflectionCubemap::new(16, Color::new(0.3, 0.3, 0.3, 1.0));
    for lod in [0.0, 1.5, 3.0, 4.4, 7.0] {
        for d in [
            Vector3::new(1.0, 0.2, 0.1),
            Vector3::new(-0.3, -1.0, 0.4),
            Vector3::new(0.1, 0.1, 1.0),
        ] {
            let c = cube.sample_lod(d, lod);
            assert!((c.r - 0.3).abs() < 1e-3, "lod {lod} dir {d:?}: {c:?}");
        }
    }
}

/// 🔑 Godot's `energy *= PI`: a white rough dielectric lit head-on by a
/// directional light of energy 1 reflects ~1.0 (Burley: `1/PI * PI`), plus a
/// ~0.01 GGX highlight. Without the factor it read 0.32.
#[test]
fn a_directional_light_of_energy_one_delivers_albedo_times_n_dot_l() {
    let p = Vector3::ZERO;
    let light =
        LightUniform::directional(Vector3::new(0.0, 1.0, 0.0), Color::new(1.0, 1.0, 1.0, 1.0));
    // No probe, and zero ambient isolated by subtracting the unlit render.
    let lit = shade(p, 0.0, 1.0, above(p), None, vec![light]);
    let unlit = shade(p, 0.0, 1.0, above(p), None, vec![]);
    let direct = lit.g - unlit.g;
    assert!(
        (direct - 1.01).abs() < 0.03,
        "Godot: light_color * PI * NdotL * burley(1/PI) = 1.0, plus D*V*F = \
         PI * (1/PI) * 0.25 * 0.04 = 0.01; got {direct}"
    );
}
