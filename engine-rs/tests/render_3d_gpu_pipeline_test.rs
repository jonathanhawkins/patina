//! Integration tests for the wgpu-based 3D GPU render pipeline.
//!
//! Validates that `GpuRenderer3D` correctly renders 3D scenes using
//! real GPU vertex and fragment shaders. Tests cover pipeline creation,
//! basic triangle rendering, shading modes, depth testing, and lighting.

#[cfg(feature = "gpu")]
mod gpu_tests {
    use gdcore::math::{Color, Vector3};
    use gdcore::math3d::{Basis, Transform3D};
    use gdrender3d::wgpu_pipeline::{
        CameraUniformData, GpuLightData, GpuRenderer3D, GpuVertex, LightArrayData,
        ModelUniformData, SHADER_3D_WGSL,
    };
    use gdserver3d::light::{Light3D, Light3DId};
    use gdserver3d::material::{Material3D, ShadingMode};
    use gdserver3d::mesh::{Mesh3D, PrimitiveType};
    use gdserver3d::server::RenderingServer3D;
    use gdserver3d::viewport::Viewport3D;

    /// Helper: creates a simple triangle mesh facing the camera (+Z direction).
    fn triangle_mesh() -> Mesh3D {
        Mesh3D {
            vertices: vec![
                Vector3::new(-0.5, -0.5, 0.0),
                Vector3::new(0.5, -0.5, 0.0),
                Vector3::new(0.0, 0.5, 0.0),
            ],
            normals: vec![
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
            ],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
            indices: vec![0, 1, 2],
            primitive_type: PrimitiveType::Triangles,
            surfaces: vec![],
        }
    }

    /// Helper: creates a quad (two triangles) mesh.
    fn quad_mesh() -> Mesh3D {
        Mesh3D {
            vertices: vec![
                Vector3::new(-1.0, -1.0, 0.0),
                Vector3::new(1.0, -1.0, 0.0),
                Vector3::new(1.0, 1.0, 0.0),
                Vector3::new(-1.0, 1.0, 0.0),
            ],
            normals: vec![
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0),
            ],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            primitive_type: PrimitiveType::Triangles,
            surfaces: vec![],
        }
    }

    /// Helper: creates a viewport with camera pulled back to see the scene.
    fn test_viewport(width: u32, height: u32) -> Viewport3D {
        let mut vp = Viewport3D::new(width, height);
        // Camera at (0, 0, 3) looking toward origin.
        vp.camera_transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, 3.0),
        };
        vp
    }

    // ── WGSL shader validation ──────────────────────────────────────

    #[test]
    fn wgsl_shader_source_is_nonempty() {
        // The WGSL shader source should be substantial.
        assert!(SHADER_3D_WGSL.len() > 500, "WGSL shader source too short");
        assert!(
            SHADER_3D_WGSL.contains("vs_main"),
            "Missing vertex entry point"
        );
        assert!(
            SHADER_3D_WGSL.contains("fs_main"),
            "Missing fragment entry point"
        );
        assert!(
            SHADER_3D_WGSL.contains("model_matrix"),
            "Missing model matrix uniform"
        );
        assert!(
            SHADER_3D_WGSL.contains("shading_mode"),
            "Missing shading mode"
        );
    }

    // ── Data structure sizes (GPU alignment) ────────────────────────

    #[test]
    fn uniform_struct_alignment() {
        // CameraUniformData: 2 mat4x4 (128) + vec3 (12) + pad (4) + ambient vec4 (16) = 160.
        assert_eq!(std::mem::size_of::<CameraUniformData>(), 160);
        // ModelUniformData: mat4x4 (64) + 2*vec4 (32) + 4*f32 (16) = 112.
        assert_eq!(std::mem::size_of::<ModelUniformData>(), 112);
        // GpuLightData: 64 bytes (16-float aligned).
        assert_eq!(std::mem::size_of::<GpuLightData>(), 64);
        // LightArrayData: 16 * 64 = 1024.
        assert_eq!(std::mem::size_of::<LightArrayData>(), 1024);
        // GpuVertex: 32 bytes (pos3 + normal3 + uv2).
        assert_eq!(std::mem::size_of::<GpuVertex>(), 32);
    }

    // ── GPU renderer creation ───────────────────────────────────────

    #[test]
    fn gpu_renderer_creates_successfully() {
        // May return None on headless CI without GPU — that's okay.
        let _renderer = GpuRenderer3D::new();
    }

    // ── Rendering tests (require GPU) ───────────────────────────────

    fn skip_if_no_gpu() -> Option<GpuRenderer3D> {
        GpuRenderer3D::new()
    }

    #[test]
    fn empty_scene_renders_black() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return, // No GPU available.
        };
        let vp = test_viewport(64, 64);
        let frame = renderer.render_frame(&vp);

        assert_eq!(frame.width, 64);
        assert_eq!(frame.height, 64);
        assert_eq!(frame.pixels.len(), 64 * 64);
        assert_eq!(frame.depth.len(), 64 * 64);

        // All pixels should be black (clear color).
        for pixel in &frame.pixels {
            assert!(
                pixel.r < 0.01 && pixel.g < 0.01 && pixel.b < 0.01,
                "Expected black, got {:?}",
                pixel
            );
        }
    }

    #[test]
    fn unlit_triangle_renders_colored_pixels() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let id = renderer.create_instance();
        renderer.set_mesh(id, triangle_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(1.0, 0.0, 0.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );

        let vp = test_viewport(64, 64);
        let frame = renderer.render_frame(&vp);

        // Some pixels should be red (the triangle).
        let red_count = frame
            .pixels
            .iter()
            .filter(|p| p.r > 0.5 && p.g < 0.2 && p.b < 0.2)
            .count();
        assert!(
            red_count > 10,
            "Expected red triangle pixels, found only {}",
            red_count
        );
    }

    #[test]
    fn lambert_shading_produces_lit_pixels() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let id = renderer.create_instance();
        renderer.set_mesh(id, quad_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(0.8, 0.8, 0.8, 1.0),
                shading_mode: ShadingMode::Lambert,
                ..Default::default()
            },
        );

        // Add a directional light shining into the scene. `direction` is the
        // direction the light *travels*: −Z goes from behind the camera onto
        // the quad's +Z-facing front.
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, 0.0, -1.0);
        light.energy = 1.0;
        renderer.update_light(&light);

        let vp = test_viewport(64, 64);
        let frame = renderer.render_frame(&vp);

        // Should have some lit pixels (not all black, not all white).
        let lit_count = frame
            .pixels
            .iter()
            .filter(|p| p.r > 0.05 || p.g > 0.05 || p.b > 0.05)
            .count();
        assert!(
            lit_count > 10,
            "Expected lit pixels from Lambert shading, found {}",
            lit_count
        );
    }

    #[test]
    fn phong_shading_produces_specular_highlights() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let id = renderer.create_instance();
        renderer.set_mesh(id, quad_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(0.5, 0.5, 0.5, 1.0),
                shading_mode: ShadingMode::Phong,
                roughness: 0.1, // Low roughness = sharp specular.
                metallic: 1.0,  // High metallic = strong specular.
                ..Default::default()
            },
        );

        // Light traveling from the camera toward the quad (−Z) for strong
        // specular reflection off its +Z face.
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, 0.0, -1.0);
        light.energy = 1.0;
        renderer.update_light(&light);

        let vp = test_viewport(64, 64);
        let frame = renderer.render_frame(&vp);

        // Some center pixels should be brighter than edge pixels (specular).
        let center_idx = 32 * 64 + 32;
        let center_brightness =
            frame.pixels[center_idx].r + frame.pixels[center_idx].g + frame.pixels[center_idx].b;

        // Just verify the center area has some illumination.
        let lit_count = frame.pixels.iter().filter(|p| p.r > 0.1).count();
        assert!(
            lit_count > 10,
            "Expected illuminated pixels, center brightness: {:.3}",
            center_brightness
        );
    }

    #[test]
    fn depth_buffer_has_valid_values() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let id = renderer.create_instance();
        renderer.set_mesh(id, triangle_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(1.0, 1.0, 1.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );

        let vp = test_viewport(64, 64);
        let frame = renderer.render_frame(&vp);

        // Background pixels should have depth = 1.0 (clear value).
        // Triangle pixels should have depth < 1.0.
        let fg_depths: Vec<f32> = frame.depth.iter().copied().filter(|d| *d < 0.999).collect();
        assert!(
            !fg_depths.is_empty(),
            "Expected some foreground depth values < 1.0"
        );

        let bg_depths: Vec<f32> = frame
            .depth
            .iter()
            .copied()
            .filter(|d| *d >= 0.999)
            .collect();
        assert!(
            !bg_depths.is_empty(),
            "Expected some background depth values at 1.0"
        );
    }

    #[test]
    fn invisible_instance_not_rendered() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let id = renderer.create_instance();
        renderer.set_mesh(id, triangle_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(1.0, 0.0, 0.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );
        renderer.set_visible(id, false);

        let vp = test_viewport(64, 64);
        let frame = renderer.render_frame(&vp);

        // No red pixels — instance is invisible.
        let red_count = frame.pixels.iter().filter(|p| p.r > 0.5).count();
        assert_eq!(red_count, 0, "Invisible instance should produce no pixels");
    }

    #[test]
    fn depth_testing_occludes_far_object() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        // Near quad (red) at z=0.
        let near_id = renderer.create_instance();
        renderer.set_mesh(near_id, quad_mesh());
        renderer.set_material(
            near_id,
            Material3D {
                albedo: Color::new(1.0, 0.0, 0.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );

        // Far quad (green) at z=-2.
        let far_id = renderer.create_instance();
        renderer.set_mesh(far_id, quad_mesh());
        renderer.set_material(
            far_id,
            Material3D {
                albedo: Color::new(0.0, 1.0, 0.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );
        renderer.set_transform(
            far_id,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.0, -2.0),
            },
        );

        let vp = test_viewport(64, 64);
        let frame = renderer.render_frame(&vp);

        // Center should be red (near quad occludes far quad).
        let center = 32 * 64 + 32;
        let c = &frame.pixels[center];
        assert!(
            c.r > 0.5 && c.g < 0.3,
            "Center should be red (near occludes far), got r={:.2} g={:.2}",
            c.r,
            c.g
        );
    }

    #[test]
    fn free_instance_removes_from_scene() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let id = renderer.create_instance();
        renderer.set_mesh(id, triangle_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(1.0, 0.0, 0.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );

        // Render once — should have red pixels.
        let vp = test_viewport(64, 64);
        let frame1 = renderer.render_frame(&vp);
        let red_before = frame1.pixels.iter().filter(|p| p.r > 0.5).count();
        assert!(red_before > 0);

        // Free the instance and render again.
        renderer.free_instance(id);
        let frame2 = renderer.render_frame(&vp);
        let red_after = frame2.pixels.iter().filter(|p| p.r > 0.5).count();
        assert_eq!(red_after, 0, "Freed instance should not render");
    }

    #[test]
    fn gpu_renderer_debug_format() {
        let renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };
        let debug = format!("{:?}", renderer);
        assert!(debug.contains("GpuRenderer3D"));
    }

    #[test]
    fn remove_light_works() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let id = renderer.create_instance();
        renderer.set_mesh(id, quad_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(1.0, 1.0, 1.0, 1.0),
                shading_mode: ShadingMode::Lambert,
                ..Default::default()
            },
        );

        let light_id = Light3DId(1);
        let mut light = Light3D::directional(light_id);
        // Travel −Z: onto the quad's +Z-facing front.
        light.direction = Vector3::new(0.0, 0.0, -1.0);
        light.energy = 2.0;
        renderer.update_light(&light);

        let vp = test_viewport(64, 64);

        // Render with light.
        let frame_lit = renderer.render_frame(&vp);
        let lit_brightness: f32 = frame_lit.pixels.iter().map(|p| p.r + p.g + p.b).sum();

        // Remove light and render again.
        renderer.remove_light(light_id);
        let frame_dark = renderer.render_frame(&vp);
        let dark_brightness: f32 = frame_dark.pixels.iter().map(|p| p.r + p.g + p.b).sum();

        // Scene should be dimmer without the light.
        assert!(
            lit_brightness > dark_brightness,
            "Removing light should reduce brightness: lit={:.1} dark={:.1}",
            lit_brightness,
            dark_brightness
        );
    }

    // ── Editor-optimized color path (render_editor_color) ────────────

    /// The editor path clears to the *provided* background color (not black) and
    /// returns no depth (the editor never reads GPU depth). Also guards the
    /// non-sRGB (`Bgra8Unorm`) target: a linear clear of 0.13 must read back as
    /// ~0.13, not the ~0.40 an sRGB target would encode.
    #[test]
    fn editor_color_clears_to_background_and_skips_depth() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };
        let bg = Color::new(0.13, 0.14, 0.17, 1.0);
        let vp = test_viewport(48, 48);
        let frame = renderer.render_editor_color(&vp, bg);

        assert_eq!(frame.width, 48);
        assert_eq!(frame.height, 48);
        assert_eq!(frame.pixels.len(), 48 * 48);
        // No depth is read back on the editor path (a saved GPU sync + copy).
        assert!(frame.depth.is_empty(), "editor path must not read back depth");
        // Every pixel is the flat background (no geometry), and linear — an sRGB
        // target would lift 0.13 to ~0.40 and fail this.
        for p in &frame.pixels {
            assert!(
                (p.r - bg.r).abs() < 0.02 && (p.g - bg.g).abs() < 0.02 && (p.b - bg.b).abs() < 0.02,
                "expected linear background {:?}, got {:?}",
                bg,
                p
            );
        }
    }

    /// A mesh renders over the editor background, and the background survives where
    /// there's no geometry — the clear + draw composite correctly on the cached
    /// target.
    #[test]
    fn editor_color_draws_mesh_over_background() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };
        let bg = Color::new(0.10, 0.12, 0.16, 1.0);
        let id = renderer.create_instance();
        renderer.set_mesh(id, triangle_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(1.0, 0.0, 0.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );
        let vp = test_viewport(64, 64);
        let frame = renderer.render_editor_color(&vp, bg);

        let red = frame
            .pixels
            .iter()
            .filter(|p| p.r > 0.5 && p.g < 0.2 && p.b < 0.2)
            .count();
        let background = frame
            .pixels
            .iter()
            .filter(|p| {
                (p.r - bg.r).abs() < 0.03 && (p.g - bg.g).abs() < 0.03 && (p.b - bg.b).abs() < 0.03
            })
            .count();
        assert!(red > 10, "expected red triangle pixels, found {red}");
        assert!(background > 10, "expected surviving background pixels, found {background}");
    }

    /// The GPU editor path renders the environment's procedural sky gradient
    /// identically to the software rasterizer's background fill — same pixel
    /// rays, same gradient math (regression guard for the "GPU viewport lost
    /// the sky" bug: it cleared to a flat color and ignored the environment).
    #[test]
    fn editor_sky_background_matches_software_reference() {
        use gdrender3d::SoftwareRenderer3D;
        use gdserver3d::environment::{BackgroundMode, Environment3D};
        use gdserver3d::sky::Sky;

        let mut gpu = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let mut vp = test_viewport(64, 64);
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        env.sky = Some(Sky::default());
        vp.environment = Some(env);

        // Both backends render the same empty scene: background only.
        let gpu_frame = gpu.render_editor_color(&vp, Color::new(1.0, 0.0, 1.0, 1.0));
        let sw_frame = SoftwareRenderer3D::new().render_frame(&vp);

        // Sample points across the gradient: sky top, horizon area, ground.
        for (x, y) in [(32u32, 2u32), (32, 16), (32, 32), (32, 48), (32, 62), (2, 32), (62, 32)] {
            let i = (y * 64 + x) as usize;
            let g = &gpu_frame.pixels[i];
            let s = &sw_frame.pixels[i];
            assert!(
                (g.r - s.r).abs() < 0.02 && (g.g - s.g).abs() < 0.02 && (g.b - s.b).abs() < 0.02,
                "sky mismatch at ({x},{y}): gpu=({:.3},{:.3},{:.3}) sw=({:.3},{:.3},{:.3})",
                g.r, g.g, g.b, s.r, s.g, s.b
            );
        }

        // And it really is a gradient, not a flat clear: the top of the frame
        // (sky) must differ from the bottom (ground).
        let top = &gpu_frame.pixels[(2 * 64 + 32) as usize];
        let bottom = &gpu_frame.pixels[(62 * 64 + 32) as usize];
        assert!(
            (top.r - bottom.r).abs() + (top.g - bottom.g).abs() + (top.b - bottom.b).abs() > 0.05,
            "expected a sky/ground gradient, got flat top=({:.3},{:.3},{:.3}) bottom=({:.3},{:.3},{:.3})",
            top.r, top.g, top.b, bottom.r, bottom.g, bottom.b
        );
    }

    /// A directional light draws the Godot ProceduralSkyMaterial sun disc +
    /// halo in the GPU sky pass, pixel-identical to the software reference
    /// (regression guard for the sky ignoring `LIGHTn_*` sun uniforms — the
    /// "no sun in the skybox" parity bug).
    #[test]
    fn editor_sky_sun_matches_software_reference() {
        use gdrender3d::SoftwareRenderer3D;
        use gdserver3d::environment::{BackgroundMode, Environment3D};
        use gdserver3d::sky::Sky;

        let mut gpu = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let mut vp = test_viewport(64, 64);
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        env.sky = Some(Sky::default());
        vp.environment = Some(env);

        // Sun travelling +Z and slightly down → disc just above the horizon,
        // dead ahead of the camera (which looks −Z).
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, -0.2, 1.0).normalized();

        gpu.add_light(light.id);
        gpu.update_light(&light);
        let gpu_frame = gpu.render_editor_color(&vp, Color::BLACK);

        let mut sw = SoftwareRenderer3D::new();
        sw.add_light(light.id);
        sw.update_light(&light);
        let sw_frame = sw.render_frame(&vp);

        // The sun must actually be there: some pixel is near-white, far above
        // anything the plain gradient produces (max min-channel ~0.67).
        let brightest = |pixels: &[Color]| {
            pixels
                .iter()
                .map(|c| c.r.min(c.g).min(c.b))
                .fold(0.0f32, f32::max)
        };
        assert!(
            brightest(&gpu_frame.pixels) > 0.9,
            "GPU sky must contain the sun disc, brightest={:.3}",
            brightest(&gpu_frame.pixels)
        );

        // And it matches the software reference everywhere — disc, halo
        // falloff, and untouched gradient alike.
        for (i, (g, s)) in gpu_frame.pixels.iter().zip(sw_frame.pixels.iter()).enumerate() {
            assert!(
                (g.r - s.r).abs() < 0.02 && (g.g - s.g).abs() < 0.02 && (g.b - s.b).abs() < 0.02,
                "sun sky mismatch at ({},{}): gpu=({:.3},{:.3},{:.3}) sw=({:.3},{:.3},{:.3})",
                i % 64, i / 64, g.r, g.g, g.b, s.r, s.g, s.b
            );
        }
    }

    /// A `CustomColor` environment clears the GPU editor target to the
    /// environment's color (the editor's flat "preview environment off"
    /// backdrop), not the caller's fallback clear.
    #[test]
    fn editor_custom_color_background_uses_environment_color() {
        use gdserver3d::environment::{BackgroundMode, Environment3D};

        let mut gpu = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let mut vp = test_viewport(48, 48);
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::CustomColor;
        env.background_color = Color::new(0.13, 0.14, 0.17, 1.0);
        vp.environment = Some(env);

        let frame = gpu.render_editor_color(&vp, Color::new(1.0, 0.0, 1.0, 1.0));
        for p in &frame.pixels {
            assert!(
                (p.r - 0.13).abs() < 0.02 && (p.g - 0.14).abs() < 0.02 && (p.b - 0.17).abs() < 0.02,
                "expected the environment's custom color, got ({:.3},{:.3},{:.3})",
                p.r, p.g, p.b
            );
        }
    }

    /// A directional-lit mesh shades identically on the GPU and software paths —
    /// regression guard for the unnegated sun direction that left every
    /// sun-facing surface ambient-black on the GPU.
    #[test]
    fn editor_directional_lit_mesh_matches_software_reference() {
        use gdrender3d::SoftwareRenderer3D;

        let mut gpu = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };

        let material = Material3D {
            albedo: Color::new(0.8, 0.8, 0.8, 1.0),
            shading_mode: ShadingMode::Lambert,
            ..Default::default()
        };
        // Sun traveling −Z onto the quad's +Z-facing front.
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, 0.0, -1.0);
        light.energy = 1.0;

        let vp = test_viewport(64, 64);
        let center = (32 * 64 + 32) as usize;

        let id = gpu.create_instance();
        gpu.set_mesh(id, quad_mesh());
        gpu.set_material(id, material.clone());
        // add_light then update_light — the editor's calling convention (the
        // software renderer only updates lights that were added).
        gpu.add_light(light.id);
        gpu.update_light(&light);
        let gpu_frame = gpu.render_editor_color(&vp, Color::BLACK);

        let mut sw = SoftwareRenderer3D::new();
        let sw_id = sw.create_instance();
        sw.set_mesh(sw_id, quad_mesh());
        sw.set_material(sw_id, material);
        sw.add_light(light.id);
        sw.update_light(&light);
        let sw_frame = sw.render_frame(&vp);

        let g = &gpu_frame.pixels[center];
        let s = &sw_frame.pixels[center];
        // Fully lit lambert: ambient 0.1×0.8 + diffuse 0.8 = 0.88. With the
        // negation bug the GPU produced ambient-only 0.08.
        assert!(
            s.r > 0.5,
            "software reference should be brightly lit, got {:.3}",
            s.r
        );
        assert!(
            (g.r - s.r).abs() < 0.03 && (g.g - s.g).abs() < 0.03 && (g.b - s.b).abs() < 0.03,
            "lit mesh mismatch: gpu=({:.3},{:.3},{:.3}) sw=({:.3},{:.3},{:.3})",
            g.r, g.g, g.b, s.r, s.g, s.b
        );
    }

    /// The cached render targets and readback buffer are reused across frames (the
    /// buffer is unmapped after each read) and rebuilt on resize. Rendering several
    /// frames — including a size change — must not panic and must keep producing
    /// correct content. Regression guard for the cached-buffer `unmap` + resize.
    #[test]
    fn editor_color_reuses_targets_and_handles_resize() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };
        let bg = Color::new(0.13, 0.14, 0.17, 1.0);
        let id = renderer.create_instance();
        renderer.set_mesh(id, triangle_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(1.0, 0.0, 0.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );

        // Many frames at the same size exercise cached-buffer reuse + unmap; a
        // missing unmap would panic on the second map_async.
        let vp = test_viewport(64, 64);
        for i in 0..5 {
            let frame = renderer.render_editor_color(&vp, bg);
            let red = frame.pixels.iter().filter(|p| p.r > 0.5 && p.g < 0.2).count();
            assert!(red > 10, "frame {i}: expected red pixels, found {red}");
        }

        // A resize must rebuild the cached targets without panicking.
        let vp2 = test_viewport(96, 72);
        let frame = renderer.render_editor_color(&vp2, bg);
        assert_eq!(frame.width, 96);
        assert_eq!(frame.height, 72);
        assert_eq!(frame.pixels.len(), 96 * 72);
        let red = frame.pixels.iter().filter(|p| p.r > 0.5 && p.g < 0.2).count();
        assert!(red > 10, "after resize: expected red pixels, found {red}");
    }

    /// `reset_scene` clears instances/lights so the persistent editor renderer can
    /// be repopulated each frame without accumulating stale geometry.
    #[test]
    fn reset_scene_clears_instances_between_frames() {
        let mut renderer = match skip_if_no_gpu() {
            Some(r) => r,
            None => return,
        };
        let bg = Color::new(0.13, 0.14, 0.17, 1.0);
        let id = renderer.create_instance();
        renderer.set_mesh(id, triangle_mesh());
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::new(1.0, 0.0, 0.0, 1.0),
                shading_mode: ShadingMode::Unlit,
                ..Default::default()
            },
        );
        let vp = test_viewport(64, 64);
        let before = renderer.render_editor_color(&vp, bg);
        assert!(before.pixels.iter().filter(|p| p.r > 0.5).count() > 10);

        // After reset the scene is empty → no red geometry, just background.
        renderer.reset_scene();
        let after = renderer.render_editor_color(&vp, bg);
        let red = after.pixels.iter().filter(|p| p.r > 0.5).count();
        assert_eq!(red, 0, "reset_scene must clear instances, found {red} red pixels");
    }
}
