//! Parity helpers tests for panorama equirect, physical sky, and debanding.

#[cfg(test)]
mod tests {
    use crate::postprocess::{apply_camera_post, screen_space_dither};
    use crate::renderer::{equirect_uv, sample_equirect, sky_suns_from_lights};
    use gdcore::math::{Color, Vector3};
    use gdserver3d::environment::{Environment3D, ToneMapper};
    use gdserver3d::light::{Light3D, Light3DId};
    use gdserver3d::sky::{PanoramaTexture, PhysicalSkyMaterial, SkyMaterial};

    /// Locks Godot's `SKY_COORDS` convention (sky.glsl `panorama_coords`):
    /// `u = atan2(x, −z)/2π` wrapped into [0,1) with NO half-turn shift, so
    /// −Z → 0, +X → 0.25, +Z → 0.5, −X → 0.75; `v = acos(y)/π`. A +0.5 shift
    /// (the bug this replaces) yaw-rotates every panorama by 180°.
    #[test]
    fn equirect_uv_matches_godot_sky_coords() {
        let (u, v) = equirect_uv(&Vector3::new(0.0, 0.0, -1.0));
        assert!(u.abs() < 1e-5 || (u - 1.0).abs() < 1e-5, "-Z u=0 got {u}");
        assert!((v - 0.5).abs() < 1e-5, "-Z v=0.5 got {v}");
        let (u, _) = equirect_uv(&Vector3::new(1.0, 0.0, 0.0));
        assert!((u - 0.25).abs() < 1e-5, "+X u=0.25 got {u}");
        let (u, _) = equirect_uv(&Vector3::new(0.0, 0.0, 1.0));
        assert!((u - 0.5).abs() < 1e-5, "+Z u=0.5 got {u}");
        let (u, _) = equirect_uv(&Vector3::new(-1.0, 0.0, 0.0));
        assert!((u - 0.75).abs() < 1e-5, "-X u=0.75 got {u}");
        let (_, v) = equirect_uv(&Vector3::new(0.0, 1.0, 0.0));
        assert!(v.abs() < 1e-5, "zenith v=0 got {v}");
        let (_, v) = equirect_uv(&Vector3::new(0.0, -1.0, 0.0));
        assert!((v - 1.0).abs() < 1e-5, "nadir v=1 got {v}");
    }

    #[test]
    fn equirect_samples_known_texture() {
        // 2x1 texture: left red, right green.
        let tex = PanoramaTexture {
            width: 2,
            height: 1,
            pixels: vec![
                Color::new(1.0, 0.0, 0.0, 1.0),
                Color::new(0.0, 1.0, 0.0, 1.0),
            ],
        };
        // Godot convention: +X (u=0.25) lands in the left texel (red),
        // −X (u=0.75) in the right texel (green).
        let c_px = sample_equirect(&tex, &Vector3::new(1.0, 0.0, 0.0), false);
        assert!(c_px.r > 0.9 && c_px.g < 0.1, "+X must be red, got {c_px:?}");
        let c_nx = sample_equirect(&tex, &Vector3::new(-1.0, 0.0, 0.0), false);
        assert!(
            c_nx.g > 0.9 && c_nx.r < 0.1,
            "-X must be green, got {c_nx:?}"
        );
    }

    /// Structural anchors of Godot's PhysicalSkyMaterial shader port: no sun →
    /// night sky (black); sun below the horizon → `sun_energy = 0` → black;
    /// with a high sun the defaults give a blue-dominant zenith and a bright
    /// solar disk exactly where LIGHT0 points.
    #[test]
    fn physical_sky_matches_godot_model_anchors() {
        let material = SkyMaterial::Physical(PhysicalSkyMaterial::default());
        let sample = |suns: &[_], dir: Vector3| {
            crate::renderer::sample_sky_color(&material, &dir, 1.0, suns)
        };

        // No directional light → Godot renders only the (default black)
        // night-sky texture.
        let c = sample(&[], Vector3::new(0.0, 1.0, 0.0));
        assert!(
            c.r < 1e-5 && c.g < 1e-5 && c.b < 1e-5,
            "no sun must be black, got {c:?}"
        );

        // Sun far below the horizon → sun_energy clamps to 0 → black sky.
        let mut below = Light3D::directional(Light3DId(1));
        below.direction = Vector3::new(0.0, 1.0, 0.0); // travels up = sun below
        let suns_below = sky_suns_from_lights(&[below]);
        let c = sample(&suns_below, Vector3::new(0.0, 0.3, -1.0).normalized());
        assert!(
            c.r < 1e-4 && c.b < 1e-4,
            "below-horizon sun must be black, got {c:?}"
        );

        // High sun with defaults: zenith is blue-dominant (Rayleigh), and the
        // sun disc (0.5° angular size) is far brighter than 10° away.
        let mut sun = Light3D::directional(Light3DId(2));
        sun.direction = Vector3::new(0.0, -1.0, 0.2).normalized(); // travels down
        sun.angular_distance = 0.5;
        let suns = sky_suns_from_lights(&[sun]);
        let toward_sun = suns[0].direction;
        let zenith = sample(&suns, Vector3::new(0.0, 1.0, 0.0));
        assert!(
            zenith.b > zenith.r,
            "default Rayleigh zenith must be blue-dominant, got {zenith:?}"
        );
        let at_sun = sample(&suns, toward_sun);
        let off_axis = sample(
            &suns,
            (toward_sun + Vector3::new(0.17, 0.0, 0.0)).normalized(),
        );
        assert!(
            at_sun.r > off_axis.r * 1.5,
            "solar disk must be much brighter than nearby sky: {at_sun:?} vs {off_axis:?}"
        );
    }

    /// Filtered equirect sampling wraps across the u = 0/1 seam like a GPU
    /// repeat sampler: at exactly −Z (u = 0, sample point half a texel left of
    /// column 0's center) the result blends the LAST column with column 0 —
    /// a clamped mapping would return column 0 alone and show a seam.
    #[test]
    fn equirect_filtered_sampling_wraps_across_seam() {
        // 4×2 (filtered path needs both dims ≥ 2); identical rows so the
        // vertical blend is a no-op and only the horizontal seam matters.
        let row = [
            Color::new(1.0, 0.0, 0.0, 1.0), // col 0: red
            Color::new(0.0, 1.0, 0.0, 1.0),
            Color::new(0.0, 0.0, 1.0, 1.0),
            Color::new(1.0, 1.0, 1.0, 1.0), // col 3: white
        ];
        let tex = PanoramaTexture {
            width: 4,
            height: 2,
            pixels: row.iter().chain(row.iter()).copied().collect(),
        };
        // −Z at the horizon → u = 0 → x = −0.5 → 50/50 blend of col 3 (white)
        // wrapped with col 0 (red).
        let c = sample_equirect(&tex, &Vector3::new(0.0, 0.0, -1.0), true);
        assert!(
            (c.r - 1.0).abs() < 1e-4 && (c.g - 0.5).abs() < 1e-4 && (c.b - 0.5).abs() < 1e-4,
            "seam sample must blend white+red = (1.0, 0.5, 0.5), got {c:?}"
        );
    }

    /// Software reference for the GPU omni-shadow parity test's scene: a
    /// cubemap-mode point light behind a small occluder must darken part of
    /// the floor quad (diagnoses whether a GPU/software mismatch is in the
    /// scene setup or in the GPU sampling).
    #[test]
    fn omni_cubemap_shadow_darkens_floor_in_software() {
        use crate::renderer::SoftwareRenderer3D;
        use gdcore::math3d::{Basis, Transform3D};
        use gdserver3d::light::{Light3D, OmniShadowMode};
        use gdserver3d::material::{Material3D, ShadingMode};
        use gdserver3d::mesh::Mesh3D;
        use gdserver3d::server::RenderingServer3D;
        use gdserver3d::viewport::Viewport3D;

        let mut sw = SoftwareRenderer3D::new();
        let floor = sw.create_instance();
        sw.set_mesh(floor, Mesh3D::plane(2.0));
        // plane() is horizontal; rotate to face +Z like the GPU test's quad?
        // Use a cube's front face instead: a flat box at z=0.
        sw.set_mesh(floor, Mesh3D::cube(2.0));
        sw.set_transform(
            floor,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.0, -1.0),
            },
        );
        sw.set_material(
            floor,
            Material3D {
                albedo: Color::new(0.9, 0.9, 0.9, 1.0),
                shading_mode: ShadingMode::Lambert,
                ..Default::default()
            },
        );
        let occ = sw.create_instance();
        sw.set_mesh(occ, Mesh3D::cube(0.6));
        sw.set_transform(
            occ,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.6, 0.0, 1.1),
            },
        );
        sw.set_material(
            occ,
            Material3D {
                albedo: Color::new(0.9, 0.2, 0.2, 1.0),
                shading_mode: ShadingMode::Lambert,
                ..Default::default()
            },
        );
        let mut light =
            Light3D::point(gdserver3d::light::Light3DId(1), Vector3::new(1.2, 0.0, 2.2));
        light.energy = 2.0;
        light.range = 20.0;
        light.shadow_enabled = true;
        light.omni_shadow_mode = OmniShadowMode::Cube;
        sw.add_light(light.id);
        sw.update_light(&light);

        let mut vp = Viewport3D::new(64, 64);
        vp.camera_transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, 3.0),
        };
        let frame = sw.render_frame(&vp);

        let lum = |c: &Color| (c.r + c.g + c.b) / 3.0;
        let is_floor = |c: &Color| c.g > c.r * 0.7;
        let (mut lit, mut dark) = (0.0f32, 1.0e9f32);
        for p in &frame.pixels {
            if is_floor(p) && lum(p) > 0.02 {
                lit = lit.max(lum(p));
                dark = dark.min(lum(p));
            }
        }
        assert!(
            dark < lit * 0.6,
            "cubemap point light must shadow part of the floor: lit={lit:.3} dark={dark:.3}"
        );
    }

    #[test]
    fn physical_sky_material_defaults_are_sane() {
        let mat = PhysicalSkyMaterial::default();
        assert!(mat.rayleigh_coefficient > 0.0);
        assert!(mat.mie_coefficient > 0.0);
        assert!(mat.turbidity > 0.0);
        assert!(mat.sun_disk_scale > 0.0);
    }

    /// Godot's debanding is Iestyn's per-channel RGB dither
    /// (tonemap.glsl `screen_space_dither`): bounded by ±0.5/255 and the
    /// channels are decorrelated (different divisors), unlike a single shared
    /// noise value.
    #[test]
    fn debanding_dither_is_bounded_and_per_channel() {
        let d = screen_space_dither(12.0, 34.0);
        for ch in d {
            assert!(
                ch >= -0.5 / 255.0 - 1e-6 && ch <= 0.5 / 255.0 + 1e-6,
                "out of range: {ch}"
            );
        }
        // Channels use divisors 103/71/97 — not all equal at a generic coord.
        assert!(
            (d[0] - d[1]).abs() > 1e-9 || (d[1] - d[2]).abs() > 1e-9,
            "dither must be per-channel, got {d:?}"
        );
    }

    #[test]
    fn debanding_changes_some_pixels_keeps_mean() {
        let w = 32u32;
        let h = 32u32;
        let mut px: Vec<Color> = (0..(w * h) as usize)
            .map(|i| {
                let t = (i as f32) / (w * h) as f32;
                Color::new(t * 0.5 + 0.25, t * 0.5 + 0.25, t * 0.5 + 0.25, 1.0)
            })
            .collect();
        let mut env = Environment3D::default();
        env.tone_mapper = ToneMapper::Linear;
        env.glow_enabled = false;
        env.use_debanding = false;
        let mut base = px.clone();
        apply_camera_post(&mut base, w, h, &env);
        env.use_debanding = true;
        apply_camera_post(&mut px, w, h, &env);
        let mean = |buf: &[Color]| {
            let n = buf.len() as f32;
            buf.iter().map(|c| c.r).sum::<f32>() / n
        };
        let mb = mean(&base);
        let md = mean(&px);
        assert!(
            (mb - md).abs() < 0.02,
            "mean should stay similar: base={mb} dithered={md}"
        );
        let diffs = base
            .iter()
            .zip(px.iter())
            .filter(|(a, b)| (a.r - b.r).abs() > 1e-6)
            .count();
        assert!(diffs > 0, "debanding should alter some pixels");
    }

    /// pat-0q2kx.224 — **the acceptance test.** A custom `shader_type sky`
    /// material renders a vertical gradient distinct from the default procedural
    /// sky, and its uniforms change the output.
    ///
    /// Godot ground truth: `shader_types.cpp`'s sky section — `sky()` writes
    /// `COLOR` per direction from `EYEDIR`. This drives the real
    /// `sample_sky_color`, so it fails if the `SkyMaterial::Shader` arm or the
    /// EYEDIR builtin is missing rather than only if the parser is.
    #[test]
    fn sky_custom_shader_render() {
        use gdserver3d::shader::{Shader3D, ShaderMaterial3D, ShaderType3D};
        use gdserver3d::sky::{ProceduralSkyMaterial, ShaderSkyMaterial};

        // The bead's shader verbatim: blue at the horizon, green-free, red rising
        // with EYEDIR.y.
        const SRC: &str =
            "shader_type sky;\nvoid sky() {\n    COLOR = vec3(EYEDIR.y, 0.0, 1.0 - EYEDIR.y);\n}\n";

        let mut mat = ShaderMaterial3D::new();
        mat.shader = Some(Shader3D::new(ShaderType3D::Sky, SRC));
        let sky = ShaderSkyMaterial::new(mat);
        assert!(
            !sky.is_inert(),
            "the sky() body must parse — an inert body silently falls back to \
             the procedural sky and would make every assertion below vacuous"
        );
        let material = SkyMaterial::Shader(sky);

        let up =
            crate::renderer::sample_sky_color(&material, &Vector3::new(0.0, 1.0, 0.0), 1.0, &[]);
        let down =
            crate::renderer::sample_sky_color(&material, &Vector3::new(0.0, -1.0, 0.0), 1.0, &[]);

        // Link 1 — EYEDIR reached the evaluator: looking up gives r=1,b=0 and
        // looking down gives r=0,b=1. A zeroed EYEDIR would make both identical.
        assert!(
            (up.r - 1.0).abs() < 1e-4 && up.b.abs() < 1e-4,
            "up should be COLOR=(1,0,0), got {up:?}"
        );
        // ⚠️ Looking DOWN, `EYEDIR.y` is **-1**, so this shader yields
        // `(-1, 0, 2)` — NOT `(0, 0, 1)`. This assertion used to expect the
        // saturated values and failed the bead for it (verifier.log:4498).
        //
        // The saturated reading is the wrong one: **the sky path is HDR by
        // design**. `sample_procedural_sky` multiplies sun colour by
        // `sun.energy` and the whole sky by `background_energy_multiplier`
        // (renderer.rs:1249), both of which routinely exceed 1.0 — that is how a
        // bright sun feeds glow/tonemapping. Clamping the *shader* path to
        // [0,1] to satisfy this line would have made it inconsistent with the
        // procedural path and crushed exactly those highlights, to fix an
        // assertion rather than a defect.
        //
        // So the evaluator was right all along: it computes the expression the
        // bead specifies, out-of-range components included. Do not "restore"
        // (0,0,1) here without first making the whole sky path clamp — and see
        // the note above before deciding that is what you want.
        assert!(
            (down.r + 1.0).abs() < 1e-4 && (down.b - 2.0).abs() < 1e-4,
            "down is EYEDIR.y = -1, so COLOR = vec3(-1, 0, 1-(-1)) = (-1,0,2); got {down:?}"
        );
        assert!(
            up.g.abs() < 1e-4 && down.g.abs() < 1e-4,
            "the shader never writes green"
        );

        // Link 2 — it is a real vertical gradient, not two flat halves.
        let mid =
            crate::renderer::sample_sky_color(&material, &Vector3::new(0.0, 0.25, 0.0), 1.0, &[]);
        assert!(
            mid.r > down.r && mid.r < up.r,
            "a mid direction must interpolate: {} not between {} and {}",
            mid.r,
            down.r,
            up.r
        );

        // Link 3 — distinct from the default procedural sky, which is the
        // "did the custom sky actually apply" check.
        let proc = SkyMaterial::Procedural(ProceduralSkyMaterial::default());
        let proc_up =
            crate::renderer::sample_sky_color(&proc, &Vector3::new(0.0, 1.0, 0.0), 1.0, &[]);
        assert!(
            (proc_up.r - up.r).abs() > 0.1 || (proc_up.b - up.b).abs() > 0.1,
            "the shader sky must differ from the procedural default (shader {up:?} vs procedural {proc_up:?})"
        );

        // Link 4 — uniforms alter the output. `tint` is read straight into COLOR,
        // so changing the parameter must change the pixel.
        const UNI_SRC: &str =
            "shader_type sky;\nuniform vec4 tint;\nvoid sky() {\n    COLOR = tint;\n}\n";
        let sample_tinted = |r: f32| {
            let mut m = ShaderMaterial3D::new();
            m.shader = Some(Shader3D::new(ShaderType3D::Sky, UNI_SRC));
            m.set_shader_parameter(
                "tint",
                gdvariant::Variant::Color(Color::new(r, 0.0, 0.0, 1.0)),
            );
            let mat = SkyMaterial::Shader(ShaderSkyMaterial::new(m));
            crate::renderer::sample_sky_color(&mat, &Vector3::new(0.0, 1.0, 0.0), 1.0, &[])
        };
        let dark = sample_tinted(0.2);
        let bright = sample_tinted(0.9);
        assert!(
            bright.r > dark.r + 0.5,
            "a sky uniform must reach the output: tint 0.2 -> {dark:?}, tint 0.9 -> {bright:?}"
        );
    }
}
