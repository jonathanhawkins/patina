//! pat-h7sz7: the image-based-light gaps between Patina and Godot that made
//! the 3D platformer's metal robot reflect grey-blue where Godot's reflects
//! green, and its f318 wall read lit where Godot's is dark.
//!
//! - A probe capture is rendered WITHOUT shadows unless the probe sets
//!   `enable_shadows` (default false): Godot's `_render_reflection_probe_step`
//!   hands the probe pass no shadow atlas.
//! - Outside every probe (and in a probe's fade band) a surface reflects the
//!   sky's RADIANCE map, roughness-filtered; Patina reflected black.
//! - The split-sum environment BRDF is Godot's baked DFG table
//!   (`integrate_dfg.glsl`), not Karis' analytic fit.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::renderer::{sky_radiance_for, SKY_RADIANCE_RESOLUTION};
use gdrender3d::shader::{env_brdf, env_brdf_karis};
use gdrender3d::SoftwareRenderer3D;
use gdserver3d::environment::{BackgroundMode, Environment3D, ReflectionSource};
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::{CullMode, Material3D, ShadingMode};
use gdserver3d::reflection_probe::{ProbeSet, ReflectionProbe, ReflectionProbeId};
use gdserver3d::server::RenderingServer3D;
use gdserver3d::sky::{ProceduralSkyMaterial, Sky, SkyMaterial};
use gdserver3d::viewport::Viewport3D;

fn slab(r: &mut SoftwareRenderer3D, origin: Vector3, scale: Vector3) {
    let id = r.create_instance();
    r.set_mesh(id, gdserver3d::mesh::Mesh3D::cube(1.0));
    r.set_material(
        id,
        Material3D {
            albedo: Color::new(1.0, 1.0, 1.0, 1.0),
            shading_mode: ShadingMode::Lambert,
            cull_mode: CullMode::Disabled,
            ..Default::default()
        },
    );
    r.set_transform(
        id,
        Transform3D {
            basis: Basis {
                x: Vector3::new(scale.x, 0.0, 0.0),
                y: Vector3::new(0.0, scale.y, 0.0),
                z: Vector3::new(0.0, 0.0, scale.z),
            },
            origin,
        },
    );
    r.set_visible(id, true);
}

/// A probe at the origin between a white floor and an occluding slab above
/// it, under a straight-down shadowed sun; returns the capture looking DOWN at
/// the floor patch the slab shadows.
fn floor_seen_by_probe(enable_shadows: bool) -> Color {
    let mut r = SoftwareRenderer3D::new();
    slab(
        &mut r,
        Vector3::new(0.0, -1.0, 0.0),
        Vector3::new(12.0, 0.1, 12.0),
    );
    slab(
        &mut r,
        Vector3::new(0.0, 2.0, 0.0),
        Vector3::new(6.0, 0.1, 6.0),
    );
    let lid = Light3DId(1);
    r.add_light(lid);
    let mut sun = Light3D::directional(lid);
    sun.direction = Vector3::new(0.0, -1.0, 0.0);
    sun.shadow_enabled = true;
    r.update_light(&sun);
    let pid = ReflectionProbeId(1);
    r.add_reflection_probe(pid);
    let mut probe = ReflectionProbe::new(pid);
    probe.enable_shadows = enable_shadows;
    r.update_reflection_probe(&probe);
    r.capture_reflection_probes(&Viewport3D::new(32, 32), 16);
    r.reflection_probes()[0]
        .captured
        .as_ref()
        .expect("a Once probe captures on its first pass")
        .sample_direction(Vector3::new(0.0, -1.0, 0.0))
}

/// 🔑 The capture must not carry the camera's shadows. Godot's probe pass
/// renders shadowless by default, so the 3D platformer's probes see their
/// floors lit and its probe ambient / reflections are brighter and greener
/// than a shadowed capture.
#[test]
fn a_probe_captures_without_shadows_unless_enable_shadows_is_set() {
    let godot_default = floor_seen_by_probe(false);
    let with_shadows = floor_seen_by_probe(true);
    // Control: the fixture really does shadow the patch when shadows are on.
    assert!(
        with_shadows.r < 0.3,
        "control: with enable_shadows the occluder must shadow the floor patch \
         (ambient only); got {with_shadows:?}"
    );
    assert!(
        godot_default.r > 0.9,
        "a default probe (enable_shadows = false) must capture the floor LIT, \
         as Godot's shadowless probe pass does; got {godot_default:?} \
         (shadowed: {with_shadows:?})"
    );
}

fn sky_env(source: ReflectionSource, background: BackgroundMode) -> Environment3D {
    Environment3D {
        background_mode: background,
        sky: Some(Sky {
            material: SkyMaterial::Procedural(ProceduralSkyMaterial::default()),
            ..Default::default()
        }),
        reflection_source: source,
        ..Default::default()
    }
}

/// 🔑 Outside every probe a smooth metal reflects the SKY's radiance map,
/// filtered at Godot's `sqrt(roughness) * 7` lod, scaled by the background
/// energy; before pat-h7sz7 it reflected black (the platformer's robot was
/// black wherever no probe covered it).
#[test]
fn outside_every_probe_a_surface_reflects_the_sky_radiance() {
    let env = sky_env(ReflectionSource::Background, BackgroundMode::Sky);
    let sky = sky_radiance_for(&env, &[], SKY_RADIANCE_RESOLUTION)
        .expect("a sky background with the default BG reflection source reflects the sky");
    let set = ProbeSet::new(&[], Some(sky.clone())).expect("sky-only set");
    // A floor (n = +Y) seen from above: the mirror direction is straight up.
    let n = Vector3::new(0.0, 1.0, 0.0);
    let (_, spec) = set.indirect(Vector3::ZERO, n, n, 0.0, Color::BLACK);
    let top = ProceduralSkyMaterial::default()
        .sky_top_color
        .srgb_to_linear();
    assert!(
        (spec.b - top.b).abs() < 0.05 && spec.b > spec.r,
        "a mirror floor must reflect the sky zenith ({top:?}); got {spec:?}"
    );
    // Roughness 1 still reflects (the most-blurred level), only dimmer by
    // the horizon term — never black.
    let (_, rough) = set.indirect(Vector3::ZERO, n, n, 1.0, Color::BLACK);
    assert!(
        rough.b > 0.05,
        "a rough surface reflects the blurred sky: {rough:?}"
    );
    // `background_energy_multiplier` scales the reflected sky.
    let mut bright = env.clone();
    bright.background_energy_multiplier = 2.0;
    let set2 = ProbeSet::new(&[], sky_radiance_for(&bright, &[], SKY_RADIANCE_RESOLUTION)).unwrap();
    let (_, spec2) = set2.indirect(Vector3::ZERO, n, n, 0.0, Color::BLACK);
    assert!(
        (spec2.b - 2.0 * spec.b).abs() < 1e-3,
        "energy 2 doubles it: {spec2:?} vs {spec:?}"
    );
}

/// `reflected_light_source`: Disabled reflects nothing, Background reflects
/// the sky only when the background IS the sky, Sky always.
#[test]
fn the_reflection_source_decides_whether_the_sky_is_reflected() {
    let r = |s, b| sky_radiance_for(&sky_env(s, b), &[], SKY_RADIANCE_RESOLUTION).is_some();
    assert!(!r(ReflectionSource::Disabled, BackgroundMode::Sky));
    assert!(r(ReflectionSource::Background, BackgroundMode::Sky));
    assert!(!r(ReflectionSource::Background, BackgroundMode::ClearColor));
    assert!(r(ReflectionSource::Sky, BackgroundMode::ClearColor));
}

/// In a probe's fade band the probe takes `blend`, the sky the rest
/// (`scene_forward_clustered.glsl:2086`); at full weight the sky is gone.
#[test]
fn a_probe_fade_band_hands_the_remainder_to_the_sky() {
    let env = sky_env(ReflectionSource::Sky, BackgroundMode::Sky);
    let sky = sky_radiance_for(&env, &[], SKY_RADIANCE_RESOLUTION).unwrap();
    let mut probe = ReflectionProbe::new(ReflectionProbeId(1));
    probe.size = Vector3::new(10.0, 10.0, 10.0);
    probe.blend_distance = 2.0;
    probe.captured = Some(std::sync::Arc::new(
        gdserver3d::reflection_probe::ReflectionCubemap::new(8, Color::new(1.0, 0.0, 0.0, 1.0)),
    ));
    let set = ProbeSet::new(&[probe], Some(sky)).unwrap();
    let n = Vector3::new(0.0, 1.0, 0.0);
    let at = |x: f32| {
        set.indirect(Vector3::new(x, 0.0, 0.0), n, n, 0.0, Color::BLACK)
            .1
    };
    let (centre, band, outside) = (at(0.0), at(4.0), at(6.0));
    assert!(
        centre.r > 0.99 && centre.b < 1e-4,
        "full weight = all probe: {centre:?}"
    );
    assert!(
        outside.r < 0.2 && outside.b > 0.2,
        "outside the box = all sky: {outside:?}"
    );
    assert!(
        band.r > 0.1 && band.r < 0.9 && band.b > 0.01,
        "the fade band mixes probe and sky: {band:?}"
    );
}

/// The DFG table is Godot's integral, not Karis' fit: at roughness → 0 the
/// integral is exactly `((1 - NdotV)^5, 1)` (a perfect mirror: Fresnel only),
/// which the fit misses by several percent at grazing angles.
#[test]
fn the_environment_brdf_is_godots_dfg_table() {
    for nov in [0.1f32, 0.3, 0.6, 0.95] {
        // The table's first row is roughness 0.5/64: GGX is nearly a delta.
        let (a, b) = env_brdf(0.0, nov);
        let fc = (1.0 - nov).powi(5);
        assert!(
            (b - 1.0).abs() < 0.02,
            "total at roughness 0, NdotV {nov}: {b}"
        );
        assert!(
            (a - fc).abs() < 0.02,
            "Fresnel term at roughness 0, NdotV {nov}: {a} vs {fc}"
        );
    }
    // Where the fit is known to be off: grazing and rough.
    let (lut_a, lut_b) = env_brdf(0.0, 0.1);
    let (fit_a, fit_b) = env_brdf_karis(0.0, 0.1);
    assert!(
        (lut_a - fit_a).abs() + (lut_b - fit_b).abs() > 0.03,
        "control: the table and Karis' fit must differ at grazing angles \
         (else this test cannot tell them apart): lut ({lut_a}, {lut_b}) fit ({fit_a}, {fit_b})"
    );
    // Energy: a rough surface at normal incidence keeps less than a mirror.
    assert!(env_brdf(1.0, 1.0).1 < env_brdf(0.0, 1.0).1);
}
