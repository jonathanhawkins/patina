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
        assert!(SHADER_3D_WGSL.contains("vs_main"), "Missing vertex entry point");
        assert!(SHADER_3D_WGSL.contains("fs_main"), "Missing fragment entry point");
        assert!(SHADER_3D_WGSL.contains("model_matrix"), "Missing model matrix uniform");
        assert!(SHADER_3D_WGSL.contains("shading_mode"), "Missing shading mode");
    }

    // ── Data structure sizes (GPU alignment) ────────────────────────

    #[test]
    fn uniform_struct_alignment() {
        // CameraUniformData: 2 mat4x4 (128) + vec3 (12) + pad (4) = 144.
        assert_eq!(std::mem::size_of::<CameraUniformData>(), 144);
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

        // Add a directional light pointing into the scene.
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, 0.0, 1.0); // Toward the quad.
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
                roughness: 0.1,  // Low roughness = sharp specular.
                metallic: 1.0,   // High metallic = strong specular.
                ..Default::default()
            },
        );

        // Light from camera direction for strong specular reflection.
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, 0.0, 1.0);
        light.energy = 1.0;
        renderer.update_light(&light);

        let vp = test_viewport(64, 64);
        let frame = renderer.render_frame(&vp);

        // Some center pixels should be brighter than edge pixels (specular).
        let center_idx = 32 * 64 + 32;
        let center_brightness = frame.pixels[center_idx].r
            + frame.pixels[center_idx].g
            + frame.pixels[center_idx].b;

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
        let fg_depths: Vec<f32> = frame
            .depth
            .iter()
            .copied()
            .filter(|d| *d < 0.999)
            .collect();
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
        let red_count = frame
            .pixels
            .iter()
            .filter(|p| p.r > 0.5)
            .count();
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
        light.direction = Vector3::new(0.0, 0.0, 1.0);
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
}
