//! Software 3D renderer with wireframe and solid shaded modes.
//!
//! Provides a CPU-based 3D renderer for testing and validation.
//! Supports wireframe rendering (mesh edges) and solid rendering
//! with a programmable vertex/fragment shader pipeline.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;

use gdserver3d::environment::{BackgroundMode, Environment3D};
use gdserver3d::instance::{Instance3D, Instance3DId};
use gdserver3d::light::{Light3D, Light3DId, LightType};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::projection::perspective_projection_matrix;
use gdserver3d::reflection_probe::ReflectionProbeId;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::ShaderMaterial3D;
use gdserver3d::sky::{PhysicalSkyMaterial, ProceduralSkyMaterial, SkyMaterial};
use gdserver3d::viewport::Viewport3D;

use crate::depth_buffer::DepthBuffer;
use crate::rasterizer::{clip_to_screen, rasterize_triangle};
use crate::shader::{
    fragment_shader_for_mode, CustomFragmentShader, FragmentShader, LightKind, LightUniform,
    ShaderUniforms, StandardVertexShader, VertexInput, VertexShader,
};

/// A pixel framebuffer with depth for the 3D software renderer.
#[derive(Debug, Clone)]
pub struct FrameBuffer3D {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel data in row-major order.
    pub pixels: Vec<Color>,
    /// Depth buffer values (one per pixel).
    pub depth: Vec<f32>,
}

impl FrameBuffer3D {
    /// Creates a new framebuffer filled with `clear_color` and max depth.
    pub fn new(width: u32, height: u32, clear_color: Color) -> Self {
        let count = (width * height) as usize;
        Self {
            width,
            height,
            pixels: vec![clear_color; count],
            depth: vec![1.0; count],
        }
    }

    /// Sets a pixel at `(x, y)`. No-op if out of bounds.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize] = color;
        }
    }

    /// Returns the color at `(x, y)`.
    ///
    /// # Panics
    ///
    /// Panics if `(x, y)` is out of bounds.
    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        self.pixels[(y * self.width + x) as usize]
    }

    /// Returns the depth at `(x, y)`.
    ///
    /// # Panics
    ///
    /// Panics if `(x, y)` is out of bounds.
    pub fn get_depth(&self, x: u32, y: u32) -> f32 {
        self.depth[(y * self.width + x) as usize]
    }

    /// Sets the depth at `(x, y)`. No-op if out of bounds.
    pub fn set_depth(&mut self, x: u32, y: u32, d: f32) {
        if x < self.width && y < self.height {
            self.depth[(y * self.width + x) as usize] = d;
        }
    }
}

/// Controls how the software renderer draws geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Draw mesh edges as colored lines (original behavior).
    Wireframe,
    /// Fill triangles using the vertex/fragment shader pipeline.
    Solid,
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::Solid
    }
}

/// A software 3D renderer with wireframe and solid shaded modes.
///
/// In [`RenderMode::Wireframe`] mode, renders mesh edges as colored lines.
/// In [`RenderMode::Solid`] mode, rasterizes filled triangles using the
/// shader pipeline with vertex and fragment shaders selected by material
/// shading mode.
pub struct SoftwareRenderer3D {
    instances: Vec<Instance3D>,
    lights: Vec<Light3D>,
    next_id: u64,
    /// The rendering mode (wireframe or solid).
    pub render_mode: RenderMode,
}

impl SoftwareRenderer3D {
    /// Creates a new software renderer in solid mode (default).
    pub fn new() -> Self {
        Self {
            instances: Vec::new(),
            lights: Vec::new(),
            next_id: 1,
            render_mode: RenderMode::Solid,
        }
    }

    /// Creates a new software renderer in wireframe mode.
    pub fn wireframe() -> Self {
        Self {
            instances: Vec::new(),
            lights: Vec::new(),
            next_id: 1,
            render_mode: RenderMode::Wireframe,
        }
    }

    /// Projects a 3D world-space point to 2D screen coordinates.
    ///
    /// Returns `(x, y, depth)` where x/y are pixel coordinates and depth
    /// is the clip-space Z for depth testing. Returns `None` if behind camera.
    fn project_point(
        &self,
        point: Vector3,
        view: &Transform3D,
        proj: &[[f32; 4]; 4],
        width: u32,
        height: u32,
    ) -> Option<(f32, f32, f32)> {
        // Transform to view space (inverse of camera transform).
        let view_pos = view.inverse().xform(point);

        // Apply projection matrix (column-major).
        let x = proj[0][0] * view_pos.x
            + proj[1][0] * view_pos.y
            + proj[2][0] * view_pos.z
            + proj[3][0];
        let y = proj[0][1] * view_pos.x
            + proj[1][1] * view_pos.y
            + proj[2][1] * view_pos.z
            + proj[3][1];
        let z = proj[0][2] * view_pos.x
            + proj[1][2] * view_pos.y
            + proj[2][2] * view_pos.z
            + proj[3][2];
        let w = proj[0][3] * view_pos.x
            + proj[1][3] * view_pos.y
            + proj[2][3] * view_pos.z
            + proj[3][3];

        if w <= 0.0 {
            return None; // Behind camera.
        }

        let ndc_x = x / w;
        let ndc_y = y / w;
        let depth = z / w;

        let screen_x = (ndc_x + 1.0) * 0.5 * width as f32;
        let screen_y = (1.0 - ndc_y) * 0.5 * height as f32; // Y flipped

        Some((screen_x, screen_y, depth))
    }

    /// Draws a line between two screen-space points with depth testing.
    fn draw_line(
        pixels: &mut [Color],
        depth_buf: &mut DepthBuffer,
        width: u32,
        height: u32,
        x0: f32,
        y0: f32,
        z0: f32,
        x1: f32,
        y1: f32,
        z1: f32,
        color: Color,
    ) {
        let steps = ((x1 - x0).abs().max((y1 - y0).abs()) as i32).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let x = (x0 + (x1 - x0) * t) as i32;
            let y = (y0 + (y1 - y0) * t) as i32;
            let z = z0 + (z1 - z0) * t;

            if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                let ux = x as u32;
                let uy = y as u32;
                if depth_buf.test_and_set(ux, uy, z) {
                    pixels[(uy * width + ux) as usize] = color;
                }
            }
        }
    }

    /// Renders a frame using wireframe edges (original behavior).
    fn render_frame_wireframe(&self, viewport: &Viewport3D) -> FrameData3D {
        let w = viewport.width;
        let h = viewport.height;
        let pixel_count = (w * h) as usize;

        let proj = perspective_projection_matrix(
            viewport.fov,
            viewport.aspect(),
            viewport.near,
            viewport.far,
        );

        // Fill background from environment.
        let mut pixels = if let Some(ref env) = viewport.environment {
            let inv_proj = invert_projection(&proj);
            Self::fill_background(w, h, env, &inv_proj, &viewport.camera_transform)
        } else {
            vec![Color::BLACK; pixel_count]
        };

        let mut depth_buf = DepthBuffer::new(w, h);

        for inst in &self.instances {
            if !inst.visible {
                continue;
            }
            let mesh = match &inst.mesh {
                Some(m) => m,
                None => continue,
            };
            let color = inst
                .material
                .as_ref()
                .map(|m| m.albedo)
                .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0));

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

                let projected: Vec<_> = tri
                    .iter()
                    .filter_map(|&vi| {
                        if vi >= verts.len() {
                            return None;
                        }
                        let world_pos = inst.transform.xform(verts[vi]);
                        self.project_point(world_pos, &viewport.camera_transform, &proj, w, h)
                    })
                    .collect();

                if projected.len() < 3 {
                    continue;
                }

                for edge in [(0, 1), (1, 2), (2, 0)] {
                    let (a, b) = (projected[edge.0], projected[edge.1]);
                    Self::draw_line(
                        &mut pixels,
                        &mut depth_buf,
                        w,
                        h,
                        a.0,
                        a.1,
                        a.2,
                        b.0,
                        b.1,
                        b.2,
                        color,
                    );
                }
            }
        }

        let depth_normalized = depth_buf.into_normalized();

        // Apply fog post-processing.
        if let Some(ref env) = viewport.environment {
            Self::apply_fog_pass(&mut pixels, &depth_normalized, w, h, env);
        }

        FrameData3D {
            width: w,
            height: h,
            pixels,
            depth: depth_normalized,
        }
    }

    /// Renders a frame using the vertex/fragment shader pipeline with solid
    /// triangle rasterization.
    fn render_frame_solid(&self, viewport: &Viewport3D) -> FrameData3D {
        let w = viewport.width;
        let h = viewport.height;
        let pixel_count = (w * h) as usize;

        let proj = perspective_projection_matrix(
            viewport.fov,
            viewport.aspect(),
            viewport.near,
            viewport.far,
        );

        // Fill background from environment.
        let mut pixels = if let Some(ref env) = viewport.environment {
            let inv_proj = invert_projection(&proj);
            Self::fill_background(w, h, env, &inv_proj, &viewport.camera_transform)
        } else {
            vec![Color::BLACK; pixel_count]
        };

        let mut depth_buf = DepthBuffer::new(w, h);
        let view_matrix = Self::transform_to_view_matrix(&viewport.camera_transform);

        // Build light uniforms from stored lights.
        let light_uniforms: Vec<LightUniform> = self
            .lights
            .iter()
            .map(|l| {
                let kind = match l.light_type {
                    LightType::Directional => LightKind::Directional,
                    LightType::Point => LightKind::Point,
                    LightType::Spot => LightKind::Spot,
                };
                // For directional lights, negate direction so it points
                // toward the light source (shader convention). For spot
                // lights, keep the forward direction for cone math.
                let direction = match l.light_type {
                    LightType::Directional => {
                        Vector3::new(-l.direction.x, -l.direction.y, -l.direction.z).normalized()
                    }
                    LightType::Spot => l.direction.normalized(),
                    LightType::Point => Vector3::ZERO,
                };
                LightUniform {
                    kind,
                    direction,
                    color: Color::new(
                        l.color.r * l.energy,
                        l.color.g * l.energy,
                        l.color.b * l.energy,
                        1.0,
                    ),
                    position: l.position,
                    range: l.range,
                    attenuation: 1.0,
                    spot_angle: l.spot_angle,
                    spot_angle_attenuation: 1.0,
                    shadow_enabled: l.shadow_enabled,
                }
            })
            .collect();

        // Generate shadow maps for directional lights with shadow_enabled.
        let shadow_maps = crate::shadow_map::generate_shadow_maps(&self.lights, &self.instances);
        let directional_shadow_maps: Vec<Option<std::sync::Arc<crate::shadow_map::ShadowMap>>> =
            light_uniforms
                .iter()
                .enumerate()
                .map(|(i, lu)| {
                    if lu.shadow_enabled && lu.kind == LightKind::Directional {
                        // Find the shadow map for this light.
                        let light_id = self.lights.get(i).map(|l| l.id);
                        light_id.and_then(|lid| {
                            shadow_maps
                                .iter()
                                .find(|sm| sm.light_id == lid)
                                .map(|sm| std::sync::Arc::new(sm.clone()))
                        })
                    } else {
                        None
                    }
                })
                .collect();

        // Generate omni shadow cubemaps for point lights with cubemap mode.
        let omni_cubemaps_raw =
            crate::shadow_map::generate_omni_shadow_cubemaps(&self.lights, &self.instances);
        let omni_shadow_cubemaps: Vec<Option<std::sync::Arc<gdserver3d::light::ShadowCubemap>>> =
            omni_cubemaps_raw
                .into_iter()
                .map(|opt| opt.map(std::sync::Arc::new))
                .collect();

        let vertex_shader = StandardVertexShader;

        for inst in &self.instances {
            if !inst.visible {
                continue;
            }

            // Collect draw entries: either a single (mesh, transform, material)
            // or multiple from a multimesh resource.
            struct DrawEntry<'a> {
                mesh: &'a Mesh3D,
                transform: Transform3D,
                material: Material3D,
            }

            let base_material = inst.material.as_ref().cloned().unwrap_or_default();
            let mut entries: Vec<DrawEntry<'_>> = Vec::new();

            if let Some(ref mm) = inst.multimesh {
                // Multimesh: draw the shared mesh once per instance entry.
                if let Some(ref shared_mesh) = mm.mesh {
                    let has_per_instance_colors = !mm.instance_colors.is_empty();
                    for i in 0..mm.instance_count() {
                        let per_inst = mm.get_instance_transform(i);
                        let composed = inst.transform * per_inst;
                        let mat = if has_per_instance_colors {
                            Material3D {
                                albedo: mm.get_instance_color(i),
                                ..base_material.clone()
                            }
                        } else {
                            base_material.clone()
                        };
                        entries.push(DrawEntry {
                            mesh: shared_mesh,
                            transform: composed,
                            material: mat,
                        });
                    }
                }
            } else if let Some(ref mesh) = inst.mesh {
                // Single mesh instance.
                entries.push(DrawEntry {
                    mesh,
                    transform: inst.transform,
                    material: base_material,
                });
            }

            for entry in &entries {
                let model_matrix = Self::transform_to_matrix(&entry.transform);

                // Use custom shader material if attached; otherwise fall back
                // to the standard fragment shader selected by shading mode.
                let fragment_shader: Box<dyn FragmentShader> =
                    if let Some(ref shader_mat) = inst.shader_material {
                        Box::new(CustomFragmentShader::from_material(shader_mat))
                    } else {
                        fragment_shader_for_mode(entry.material.shading_mode)
                    };

                let uniforms = ShaderUniforms {
                    model_matrix,
                    view_matrix,
                    projection_matrix: proj,
                    albedo: entry.material.albedo,
                    emission: entry.material.emission,
                    roughness: entry.material.roughness,
                    metallic: entry.material.metallic,
                    camera_position: viewport.camera_transform.origin,
                    lights: light_uniforms.clone(),
                    directional_shadow_maps: directional_shadow_maps.clone(),
                    omni_shadow_cubemaps: omni_shadow_cubemaps.clone(),
                };

                let mesh = entry.mesh;

                // Run vertex shader on all vertices.
                let vertex_outputs: Vec<_> = (0..mesh.vertices.len())
                    .map(|vi| {
                        let normal = if vi < mesh.normals.len() {
                            mesh.normals[vi]
                        } else {
                            Vector3::UP
                        };
                        let uv = if vi < mesh.uvs.len() {
                            mesh.uvs[vi]
                        } else {
                            [0.0, 0.0]
                        };
                        let input = VertexInput {
                            position: mesh.vertices[vi],
                            normal,
                            uv,
                        };
                        vertex_shader.process(&input, &uniforms)
                    })
                    .collect();

                // Rasterize triangles.
                let mut idx = 0;
                while idx + 2 < mesh.indices.len() {
                    let i0 = mesh.indices[idx] as usize;
                    let i1 = mesh.indices[idx + 1] as usize;
                    let i2 = mesh.indices[idx + 2] as usize;
                    idx += 3;

                    if i0 >= vertex_outputs.len()
                        || i1 >= vertex_outputs.len()
                        || i2 >= vertex_outputs.len()
                    {
                        continue;
                    }

                    let sv0 = match clip_to_screen(&vertex_outputs[i0], w, h) {
                        Some(v) => v,
                        None => continue,
                    };
                    let sv1 = match clip_to_screen(&vertex_outputs[i1], w, h) {
                        Some(v) => v,
                        None => continue,
                    };
                    let sv2 = match clip_to_screen(&vertex_outputs[i2], w, h) {
                        Some(v) => v,
                        None => continue,
                    };

                    rasterize_triangle(
                        &sv0,
                        &sv1,
                        &sv2,
                        &mut pixels,
                        &mut depth_buf,
                        w,
                        h,
                        fragment_shader.as_ref(),
                        &uniforms,
                    );
                }
            }
        }

        let depth_normalized = depth_buf.into_normalized();

        // Apply fog post-processing.
        if let Some(ref env) = viewport.environment {
            Self::apply_fog_pass(&mut pixels, &depth_normalized, w, h, env);
        }

        FrameData3D {
            width: w,
            height: h,
            pixels,
            depth: depth_normalized,
        }
    }

    /// Fills a pixel buffer with the environment background (sky or solid color).
    fn fill_background(
        w: u32,
        h: u32,
        env: &Environment3D,
        inv_proj: &[[f32; 4]; 4],
        camera_transform: &Transform3D,
    ) -> Vec<Color> {
        let pixel_count = (w * h) as usize;
        match env.background_mode {
            BackgroundMode::CustomColor => {
                vec![env.background_color; pixel_count]
            }
            BackgroundMode::Sky if env.sky.is_some() => {
                let mut pixels = vec![Color::BLACK; pixel_count];
                for y in 0..h {
                    let ndc_y = 1.0 - 2.0 * (y as f32 + 0.5) / h as f32;
                    for x in 0..w {
                        let ndc_x = 2.0 * (x as f32 + 0.5) / w as f32 - 1.0;
                        pixels[(y * w + x) as usize] = environment_background_color(
                            env,
                            ndc_x,
                            ndc_y,
                            inv_proj,
                            camera_transform,
                        );
                    }
                }
                pixels
            }
            _ => vec![Color::BLACK; pixel_count],
        }
    }

    /// Post-processes pixels with distance-based fog from the environment.
    fn apply_fog_pass(pixels: &mut [Color], depth: &[f32], w: u32, h: u32, env: &Environment3D) {
        if !env.fog_enabled || env.fog_density <= 0.0 {
            return;
        }
        let total = (w * h) as usize;
        for i in 0..total {
            let d = depth[i];
            // Only apply fog to pixels that had geometry written (depth < 1.0).
            if d < 1.0 {
                pixels[i] = apply_fog(pixels[i], d, env);
            }
        }
    }

    /// Converts a [`Transform3D`] to a column-major 4x4 model matrix.
    fn transform_to_matrix(t: &Transform3D) -> [[f32; 4]; 4] {
        let b = &t.basis;
        [
            [b.x.x, b.x.y, b.x.z, 0.0],
            [b.y.x, b.y.y, b.y.z, 0.0],
            [b.z.x, b.z.y, b.z.z, 0.0],
            [t.origin.x, t.origin.y, t.origin.z, 1.0],
        ]
    }

    /// Converts a camera [`Transform3D`] to a column-major 4x4 view matrix
    /// (inverse of camera transform).
    fn transform_to_view_matrix(t: &Transform3D) -> [[f32; 4]; 4] {
        let inv = t.inverse();
        Self::transform_to_matrix(&inv)
    }
}

// ---------------------------------------------------------------------------
// Environment preview helpers
// ---------------------------------------------------------------------------

/// Computes the background color for a pixel given its normalized screen
/// coordinates and the camera's inverse view matrix.
///
/// For procedural skies this evaluates the sky gradient based on the
/// view-space direction of the pixel. For custom-color backgrounds it
/// returns the flat color. For clear-color backgrounds it returns black.
fn environment_background_color(
    env: &Environment3D,
    ndc_x: f32,
    ndc_y: f32,
    inv_proj: &[[f32; 4]; 4],
    camera_transform: &Transform3D,
) -> Color {
    match env.background_mode {
        BackgroundMode::CustomColor => env.background_color,
        BackgroundMode::Sky => {
            if let Some(ref sky) = env.sky {
                let dir = pixel_view_direction(ndc_x, ndc_y, inv_proj, camera_transform);
                sample_sky_color(&sky.material, &dir, env.background_energy_multiplier)
            } else {
                Color::BLACK
            }
        }
        _ => Color::BLACK,
    }
}

/// Reconstructs a world-space view direction from NDC coordinates.
fn pixel_view_direction(
    ndc_x: f32,
    ndc_y: f32,
    inv_proj: &[[f32; 4]; 4],
    camera_transform: &Transform3D,
) -> Vector3 {
    // Unproject from NDC to view space.
    let vx =
        inv_proj[0][0] * ndc_x + inv_proj[1][0] * ndc_y + inv_proj[2][0] * (-1.0) + inv_proj[3][0];
    let vy =
        inv_proj[0][1] * ndc_x + inv_proj[1][1] * ndc_y + inv_proj[2][1] * (-1.0) + inv_proj[3][1];
    let vz =
        inv_proj[0][2] * ndc_x + inv_proj[1][2] * ndc_y + inv_proj[2][2] * (-1.0) + inv_proj[3][2];
    let vw =
        inv_proj[0][3] * ndc_x + inv_proj[1][3] * ndc_y + inv_proj[2][3] * (-1.0) + inv_proj[3][3];

    let view_dir = if vw.abs() > 1e-6 {
        Vector3::new(vx / vw, vy / vw, vz / vw)
    } else {
        Vector3::new(vx, vy, vz)
    };

    // Transform direction from view space to world space using the camera basis.
    camera_transform.basis.xform(view_dir).normalized()
}

/// Samples a sky material color for the given world-space view direction.
fn sample_sky_color(material: &SkyMaterial, dir: &Vector3, energy: f32) -> Color {
    match material {
        SkyMaterial::Procedural(mat) => sample_procedural_sky(mat, dir, energy),
        SkyMaterial::Physical(mat) => sample_physical_sky(mat, dir, energy),
        SkyMaterial::Panoramic(_) => {
            // Panoramic sky requires texture lookup — show a neutral color.
            Color::new(0.3, 0.3, 0.35, 1.0)
        }
    }
}

/// Evaluates the procedural sky gradient for a view direction.
///
/// The Y component of the direction determines the blend between sky/ground
/// colors and horizon colors, matching Godot's `ProceduralSkyMaterial` behavior.
fn sample_procedural_sky(mat: &ProceduralSkyMaterial, dir: &Vector3, energy: f32) -> Color {
    let y = dir.y.clamp(-1.0, 1.0);

    let (top, horizon, curve, mat_energy) = if y >= 0.0 {
        // Sky hemisphere
        (
            &mat.sky_top_color,
            &mat.sky_horizon_color,
            mat.sky_curve,
            mat.sky_energy_multiplier,
        )
    } else {
        // Ground hemisphere
        (
            &mat.ground_bottom_color,
            &mat.ground_horizon_color,
            mat.ground_curve,
            mat.ground_energy_multiplier,
        )
    };

    // Godot uses pow(abs(y), curve) to blend from horizon to top/bottom.
    let t = y.abs().powf(curve.max(0.001));
    let r = lerp(horizon.r, top.r, t) * mat_energy * energy;
    let g = lerp(horizon.g, top.g, t) * mat_energy * energy;
    let b = lerp(horizon.b, top.b, t) * mat_energy * energy;

    Color::new(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), 1.0)
}

/// Simplified physical sky approximation using Rayleigh-like scattering.
fn sample_physical_sky(mat: &PhysicalSkyMaterial, dir: &Vector3, energy: f32) -> Color {
    let y = dir.y.clamp(-1.0, 1.0);

    if y < 0.0 {
        // Below horizon — return ground color.
        return Color::new(
            mat.ground_color.r * energy,
            mat.ground_color.g * energy,
            mat.ground_color.b * energy,
            1.0,
        );
    }

    // Simplified atmospheric scattering: blend from horizon to zenith.
    let t = y.powf(0.5);
    let zenith = &mat.rayleigh_color;
    let horizon_factor = 1.0 / (1.0 + mat.turbidity * 0.1);
    let hr = lerp(1.0, zenith.r, t) * horizon_factor;
    let hg = lerp(1.0, zenith.g, t) * horizon_factor;
    let hb = lerp(1.0, zenith.b, t) * horizon_factor;

    let e = mat.energy_multiplier * energy;
    Color::new(
        (hr * e).clamp(0.0, 1.0),
        (hg * e).clamp(0.0, 1.0),
        (hb * e).clamp(0.0, 1.0),
        1.0,
    )
}

/// Applies distance-based fog to a pixel color.
///
/// Blends the fragment color toward the fog color based on the linear
/// depth fraction and the environment's fog density.
fn apply_fog(fragment: Color, depth_fraction: f32, env: &Environment3D) -> Color {
    if !env.fog_enabled || env.fog_density <= 0.0 {
        return fragment;
    }
    // Exponential fog: factor = exp(-density * distance).
    // depth_fraction is 0..1 where 1 is the far plane.
    let fog_amount = 1.0 - (-env.fog_density * depth_fraction * 100.0).exp();
    let fog_amount = fog_amount.clamp(0.0, 1.0);
    let fc = &env.fog_light_color;
    Color::new(
        lerp(fragment.r, fc.r, fog_amount),
        lerp(fragment.g, fc.g, fog_amount),
        lerp(fragment.b, fc.b, fog_amount),
        fragment.a,
    )
}

/// Inverts a column-major 4x4 projection matrix.
///
/// Uses a simplified inversion that works for typical perspective matrices.
fn invert_projection(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // For a standard perspective matrix the structure is simple enough
    // that we can invert element-wise with the known layout.
    let mut inv = [[0.0f32; 4]; 4];

    // Copy and use Gauss-Jordan elimination for a generic 4x4.
    let mut aug = [[0.0f64; 8]; 4];
    for r in 0..4 {
        for c in 0..4 {
            aug[r][c] = m[c][r] as f64; // transpose to row-major
        }
        aug[r][r + 4] = 1.0;
    }

    for col in 0..4 {
        // Find pivot.
        let mut max_row = col;
        let mut max_val = aug[col][col].abs();
        for row in (col + 1)..4 {
            if aug[row][col].abs() > max_val {
                max_val = aug[row][col].abs();
                max_row = row;
            }
        }
        aug.swap(col, max_row);

        let pivot = aug[col][col];
        if pivot.abs() < 1e-12 {
            return inv; // Singular, return zeros.
        }
        for j in 0..8 {
            aug[col][j] /= pivot;
        }
        for row in 0..4 {
            if row == col {
                continue;
            }
            let factor = aug[row][col];
            for j in 0..8 {
                aug[row][j] -= factor * aug[col][j];
            }
        }
    }

    // Extract result back to column-major.
    for r in 0..4 {
        for c in 0..4 {
            inv[c][r] = aug[r][c + 4] as f32;
        }
    }
    inv
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Default for SoftwareRenderer3D {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderingServer3D for SoftwareRenderer3D {
    fn create_instance(&mut self) -> Instance3DId {
        let id = Instance3DId(self.next_id);
        self.next_id += 1;
        self.instances.push(Instance3D::new(id));
        id
    }

    fn free_instance(&mut self, id: Instance3DId) {
        self.instances.retain(|i| i.id != id);
    }

    fn set_mesh(&mut self, id: Instance3DId, mesh: Mesh3D) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.mesh = Some(mesh);
        }
    }

    fn set_material(&mut self, id: Instance3DId, material: Material3D) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.material = Some(material);
        }
    }

    fn set_transform(&mut self, id: Instance3DId, transform: Transform3D) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.transform = transform;
        }
    }

    fn set_visible(&mut self, id: Instance3DId, visible: bool) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.visible = visible;
        }
    }

    fn add_light(&mut self, id: Light3DId) {
        if !self.lights.iter().any(|l| l.id == id) {
            self.lights.push(Light3D::directional(id));
        }
    }

    fn remove_light(&mut self, id: Light3DId) {
        self.lights.retain(|l| l.id != id);
    }

    fn update_light(&mut self, light: &Light3D) {
        if let Some(existing) = self.lights.iter_mut().find(|l| l.id == light.id) {
            *existing = light.clone();
        }
    }

    fn set_shader_material(&mut self, id: Instance3DId, material: ShaderMaterial3D) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.shader_material = Some(material);
        }
    }

    fn set_multimesh(&mut self, id: Instance3DId, multimesh: gdserver3d::multimesh::MultiMesh3D) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.multimesh = Some(multimesh);
        }
    }

    fn clear_multimesh(&mut self, id: Instance3DId) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.multimesh = None;
        }
    }

    fn add_reflection_probe(&mut self, _id: ReflectionProbeId) {
        // Reflection probes are not yet supported.
    }

    fn remove_reflection_probe(&mut self, _id: ReflectionProbeId) {}

    fn render_frame(&mut self, viewport: &Viewport3D) -> FrameData3D {
        match self.render_mode {
            RenderMode::Wireframe => self.render_frame_wireframe(viewport),
            RenderMode::Solid => self.render_frame_solid(viewport),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math3d::Basis;
    use gdserver3d::material::ShadingMode;

    fn cube_at(renderer: &mut SoftwareRenderer3D, z: f32) -> Instance3DId {
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));
        renderer.set_material(id, Material3D::default());
        let transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, z),
        };
        renderer.set_transform(id, transform);
        id
    }

    #[test]
    fn create_and_free_instance() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        assert_eq!(renderer.instances.len(), 1);
        renderer.free_instance(id);
        assert_eq!(renderer.instances.len(), 0);
    }

    #[test]
    fn render_empty_scene() {
        let mut renderer = SoftwareRenderer3D::new();
        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);
        assert_eq!(frame.width, 64);
        assert_eq!(frame.height, 64);
        assert_eq!(frame.pixels.len(), 64 * 64);
        assert!(frame.pixels.iter().all(|c| *c == Color::BLACK));
    }

    // ── Wireframe mode tests ──

    #[test]
    fn wireframe_cube_produces_nonblack_pixels() {
        let mut renderer = SoftwareRenderer3D::wireframe();
        cube_at(&mut renderer, -5.0);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert!(nonblack > 0, "cube wireframe should produce visible pixels");
    }

    #[test]
    fn wireframe_invisible_instance_not_rendered() {
        let mut renderer = SoftwareRenderer3D::wireframe();
        let id = cube_at(&mut renderer, -5.0);
        renderer.set_visible(id, false);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);
        assert!(frame.pixels.iter().all(|c| *c == Color::BLACK));
    }

    #[test]
    fn wireframe_deterministic_rendering() {
        let mut renderer = SoftwareRenderer3D::wireframe();
        cube_at(&mut renderer, -5.0);

        let vp = Viewport3D::new(32, 32);
        let f1 = renderer.render_frame(&vp);
        let f2 = renderer.render_frame(&vp);
        assert_eq!(f1.pixels, f2.pixels, "rendering must be deterministic");
    }

    #[test]
    fn wireframe_set_material_changes_color() {
        let mut renderer = SoftwareRenderer3D::wireframe();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));

        let mut mat = Material3D::default();
        mat.albedo = Color::new(1.0, 0.0, 0.0, 1.0);
        renderer.set_material(id, mat);

        let transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        };
        renderer.set_transform(id, transform);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let red_pixels = frame
            .pixels
            .iter()
            .filter(|c| c.r > 0.9 && c.g < 0.1 && c.b < 0.1)
            .count();
        assert!(red_pixels > 0, "red material should produce red pixels");
    }

    #[test]
    fn wireframe_depth_data() {
        let mut renderer = SoftwareRenderer3D::wireframe();
        cube_at(&mut renderer, -5.0);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let depth_written = frame.depth.iter().filter(|d| **d < 1.0).count();
        let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert!(depth_written > 0);
        assert_eq!(depth_written, nonblack);
    }

    // ── Solid mode tests ──

    #[test]
    fn solid_cube_produces_filled_pixels() {
        let mut renderer = SoftwareRenderer3D::new();
        cube_at(&mut renderer, -5.0);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert!(
            nonblack > 100,
            "solid cube should fill many pixels, got {nonblack}"
        );
    }

    #[test]
    fn solid_more_pixels_than_wireframe() {
        let mut solid = SoftwareRenderer3D::new();
        cube_at(&mut solid, -5.0);

        let mut wire = SoftwareRenderer3D::wireframe();
        cube_at(&mut wire, -5.0);

        let vp = Viewport3D::new(64, 64);
        let solid_frame = solid.render_frame(&vp);
        let wire_frame = wire.render_frame(&vp);

        let solid_px = solid_frame
            .pixels
            .iter()
            .filter(|c| **c != Color::BLACK)
            .count();
        let wire_px = wire_frame
            .pixels
            .iter()
            .filter(|c| **c != Color::BLACK)
            .count();

        assert!(
            solid_px > wire_px,
            "solid ({solid_px}) should fill more pixels than wireframe ({wire_px})"
        );
    }

    #[test]
    fn solid_invisible_instance_not_rendered() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = cube_at(&mut renderer, -5.0);
        renderer.set_visible(id, false);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);
        assert!(frame.pixels.iter().all(|c| *c == Color::BLACK));
    }

    #[test]
    fn solid_deterministic_rendering() {
        let mut renderer = SoftwareRenderer3D::new();
        cube_at(&mut renderer, -5.0);

        let vp = Viewport3D::new(32, 32);
        let f1 = renderer.render_frame(&vp);
        let f2 = renderer.render_frame(&vp);
        assert_eq!(
            f1.pixels, f2.pixels,
            "solid rendering must be deterministic"
        );
        assert_eq!(f1.depth, f2.depth, "solid depth must be deterministic");
    }

    #[test]
    fn solid_depth_buffer_written() {
        let mut renderer = SoftwareRenderer3D::new();
        cube_at(&mut renderer, -5.0);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let depth_written = frame.depth.iter().filter(|d| **d < 1.0).count();
        let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert!(depth_written > 0);
        assert_eq!(depth_written, nonblack);
    }

    #[test]
    fn solid_unlit_material() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));

        let mat = Material3D {
            albedo: Color::new(0.5, 0.5, 0.5, 1.0),
            shading_mode: ShadingMode::Unlit,
            ..Material3D::default()
        };
        renderer.set_material(id, mat);

        let transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        };
        renderer.set_transform(id, transform);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let matching = frame
            .pixels
            .iter()
            .filter(|c| {
                (c.r - 0.5).abs() < 1e-3 && (c.g - 0.5).abs() < 1e-3 && (c.b - 0.5).abs() < 1e-3
            })
            .count();
        assert!(
            matching > 50,
            "unlit material should produce uniform-colored pixels, got {matching}"
        );
    }

    #[test]
    fn solid_red_material() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));

        let mat = Material3D {
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
            shading_mode: ShadingMode::Unlit,
            ..Material3D::default()
        };
        renderer.set_material(id, mat);

        let transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        };
        renderer.set_transform(id, transform);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let red = frame
            .pixels
            .iter()
            .filter(|c| c.r > 0.9 && c.g < 0.1 && c.b < 0.1)
            .count();
        assert!(
            red > 50,
            "red unlit material should produce red pixels, got {red}"
        );
    }

    #[test]
    fn solid_lambert_with_light() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));
        renderer.set_material(id, Material3D::default()); // Lambert by default

        let transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        };
        renderer.set_transform(id, transform);

        renderer.add_light(Light3DId(1));

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert!(nonblack > 100, "lambert lit cube should fill pixels");
    }

    #[test]
    fn solid_phong_shading() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));

        let mat = Material3D {
            shading_mode: ShadingMode::Phong,
            roughness: 0.2,
            metallic: 0.8,
            ..Material3D::default()
        };
        renderer.set_material(id, mat);

        let transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        };
        renderer.set_transform(id, transform);

        renderer.add_light(Light3DId(1));

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let nonblack = frame.pixels.iter().filter(|c| **c != Color::BLACK).count();
        assert!(nonblack > 100, "phong lit cube should fill pixels");
    }

    #[test]
    fn render_mode_default_is_solid() {
        let renderer = SoftwareRenderer3D::new();
        assert_eq!(renderer.render_mode, RenderMode::Solid);
    }

    #[test]
    fn render_mode_wireframe_constructor() {
        let renderer = SoftwareRenderer3D::wireframe();
        assert_eq!(renderer.render_mode, RenderMode::Wireframe);
    }

    #[test]
    fn add_and_remove_light() {
        let mut renderer = SoftwareRenderer3D::new();
        renderer.add_light(Light3DId(1));
        assert_eq!(renderer.lights.len(), 1);
        renderer.add_light(Light3DId(1)); // duplicate ignored
        assert_eq!(renderer.lights.len(), 1);
        renderer.add_light(Light3DId(2));
        assert_eq!(renderer.lights.len(), 2);
        renderer.remove_light(Light3DId(1));
        assert_eq!(renderer.lights.len(), 1);
        assert_eq!(renderer.lights[0].id, Light3DId(2));
    }

    #[test]
    fn solid_sphere_renders() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::sphere(1.0, 8));

        let mat = Material3D {
            shading_mode: ShadingMode::Unlit,
            albedo: Color::new(0.0, 1.0, 0.0, 1.0),
            ..Material3D::default()
        };
        renderer.set_material(id, mat);

        let transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        };
        renderer.set_transform(id, transform);

        let vp = Viewport3D::new(64, 64);
        let frame = renderer.render_frame(&vp);

        let green = frame.pixels.iter().filter(|c| c.g > 0.9).count();
        assert!(
            green > 50,
            "sphere should produce green pixels, got {green}"
        );
    }

    // ── FrameBuffer3D tests (pat-5p5q) ──

    #[test]
    fn framebuffer3d_new_initializes_correctly() {
        let fb = FrameBuffer3D::new(8, 4, Color::rgb(0.5, 0.5, 0.5));
        assert_eq!(fb.width, 8);
        assert_eq!(fb.height, 4);
        assert_eq!(fb.pixels.len(), 32);
        assert_eq!(fb.depth.len(), 32);
        assert_eq!(fb.get_pixel(0, 0), Color::rgb(0.5, 0.5, 0.5));
        assert_eq!(fb.get_depth(0, 0), 1.0);
    }

    #[test]
    fn framebuffer3d_set_get_pixel() {
        let mut fb = FrameBuffer3D::new(4, 4, Color::BLACK);
        fb.set_pixel(2, 3, Color::WHITE);
        assert_eq!(fb.get_pixel(2, 3), Color::WHITE);
        assert_eq!(fb.get_pixel(0, 0), Color::BLACK);
    }

    #[test]
    fn framebuffer3d_set_get_depth() {
        let mut fb = FrameBuffer3D::new(4, 4, Color::BLACK);
        fb.set_depth(1, 1, 0.42);
        assert_eq!(fb.get_depth(1, 1), 0.42);
        assert_eq!(fb.get_depth(0, 0), 1.0);
    }

    #[test]
    fn framebuffer3d_out_of_bounds_set_is_noop() {
        let mut fb = FrameBuffer3D::new(4, 4, Color::BLACK);
        fb.set_pixel(10, 10, Color::WHITE); // no panic
        fb.set_depth(10, 10, 0.5); // no panic
        assert_eq!(fb.pixels.len(), 16);
    }

    #[test]
    fn framebuffer3d_zero_size() {
        let fb = FrameBuffer3D::new(0, 0, Color::BLACK);
        assert_eq!(fb.pixels.len(), 0);
        assert_eq!(fb.depth.len(), 0);
    }

    // ── Environment preview tests ──────────────────────────────────

    fn avg_color(pixels: &[Color]) -> Color {
        let n = pixels.len() as f32;
        let (mut r, mut g, mut b) = (0.0f32, 0.0f32, 0.0f32);
        for c in pixels {
            r += c.r;
            g += c.g;
            b += c.b;
        }
        Color::new(r / n, g / n, b / n, 1.0)
    }

    fn viewport_with_env(env: Environment3D) -> Viewport3D {
        let mut vp = Viewport3D::new(32, 32);
        vp.environment = Some(env);
        vp.camera_transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, 5.0),
        };
        vp
    }

    #[test]
    fn no_environment_renders_black() {
        let mut renderer = SoftwareRenderer3D::new();
        let vp = Viewport3D::new(32, 32);
        let frame = renderer.render_frame(&vp);
        assert!(frame.pixels.iter().all(|c| *c == Color::BLACK));
    }

    #[test]
    fn custom_color_background() {
        let mut renderer = SoftwareRenderer3D::new();
        let env = Environment3D {
            background_mode: BackgroundMode::CustomColor,
            background_color: Color::new(0.5, 0.2, 0.1, 1.0),
            ..Default::default()
        };
        let vp = viewport_with_env(env);
        let frame = renderer.render_frame(&vp);

        for px in &frame.pixels {
            assert!(
                (px.r - 0.5).abs() < 0.01 && (px.g - 0.2).abs() < 0.01 && (px.b - 0.1).abs() < 0.01,
                "Expected custom bg color, got {:?}",
                px
            );
        }
    }

    #[test]
    fn procedural_sky_produces_visible_colors() {
        let mut renderer = SoftwareRenderer3D::new();
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(gdserver3d::sky::Sky::default()),
            ..Default::default()
        };
        let vp = viewport_with_env(env);
        let frame = renderer.render_frame(&vp);
        assert!(
            frame
                .pixels
                .iter()
                .any(|c| c.r > 0.01 || c.g > 0.01 || c.b > 0.01),
            "Procedural sky should produce visible colors"
        );
    }

    #[test]
    fn procedural_sky_custom_red() {
        use gdserver3d::sky::{ProceduralSkyMaterial as PSM, Sky as Sky3D, SkyMaterial as SM};
        let mut renderer = SoftwareRenderer3D::new();
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(Sky3D {
                material: SM::Procedural(PSM {
                    sky_top_color: Color::new(1.0, 0.0, 0.0, 1.0),
                    sky_horizon_color: Color::new(1.0, 0.0, 0.0, 1.0),
                    ground_bottom_color: Color::new(1.0, 0.0, 0.0, 1.0),
                    ground_horizon_color: Color::new(1.0, 0.0, 0.0, 1.0),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let vp = viewport_with_env(env);
        let frame = renderer.render_frame(&vp);
        let avg = avg_color(&frame.pixels);
        assert!(avg.r > 0.3, "Expected red sky, got r={:.3}", avg.r);
        assert!(avg.g < 0.1, "Expected no green, got g={:.3}", avg.g);
    }

    #[test]
    fn sky_mode_without_sky_resource_is_black() {
        let mut renderer = SoftwareRenderer3D::new();
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: None,
            ..Default::default()
        };
        let vp = viewport_with_env(env);
        let frame = renderer.render_frame(&vp);
        assert!(frame
            .pixels
            .iter()
            .all(|c| c.r < 0.01 && c.g < 0.01 && c.b < 0.01));
    }

    #[test]
    fn fog_blends_toward_fog_color() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));
        renderer.set_material(
            id,
            Material3D {
                albedo: Color::WHITE,
                ..Default::default()
            },
        );
        renderer.set_transform(
            id,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.0, -5.0),
            },
        );
        let lid = gdserver3d::light::Light3DId(1);
        renderer.add_light(lid);

        // Without fog.
        let vp_no = Viewport3D::new(32, 32);
        let frame_no = renderer.render_frame(&vp_no);

        // With heavy red fog.
        let env = Environment3D {
            fog_enabled: true,
            fog_light_color: Color::new(0.8, 0.1, 0.1, 1.0),
            fog_density: 0.5,
            ..Default::default()
        };
        let vp_fog = viewport_with_env(env);
        let frame_fog = renderer.render_frame(&vp_fog);

        let mut fog_shifted = false;
        for (i, px) in frame_no.pixels.iter().enumerate() {
            if px.r > 0.05 || px.g > 0.05 || px.b > 0.05 {
                if frame_fog.pixels[i].r > px.r + 0.01 {
                    fog_shifted = true;
                    break;
                }
            }
        }
        assert!(fog_shifted, "Fog should shift geometry toward fog color");
    }

    #[test]
    fn fog_disabled_no_effect() {
        let mut renderer = SoftwareRenderer3D::new();
        let id = renderer.create_instance();
        renderer.set_mesh(id, Mesh3D::cube(1.0));
        renderer.set_material(id, Material3D::default());
        renderer.set_transform(
            id,
            Transform3D {
                basis: Basis::IDENTITY,
                origin: Vector3::new(0.0, 0.0, -5.0),
            },
        );

        let vp_no = Viewport3D::new(32, 32);
        let frame_no = renderer.render_frame(&vp_no);

        let env = Environment3D {
            fog_enabled: false,
            fog_density: 1.0,
            fog_light_color: Color::new(1.0, 0.0, 0.0, 1.0),
            ..Default::default()
        };
        let vp_env = viewport_with_env(env);
        let frame_env = renderer.render_frame(&vp_env);

        let avg_no = avg_color(&frame_no.pixels);
        let avg_env = avg_color(&frame_env.pixels);
        assert!(
            (avg_no.r - avg_env.r).abs() < 0.02 && (avg_no.g - avg_env.g).abs() < 0.02,
            "Fog disabled should not change rendering"
        );
    }

    #[test]
    fn wireframe_renders_env_background() {
        let mut renderer = SoftwareRenderer3D::wireframe();
        let env = Environment3D {
            background_mode: BackgroundMode::CustomColor,
            background_color: Color::new(0.0, 0.5, 0.0, 1.0),
            ..Default::default()
        };
        let vp = viewport_with_env(env);
        let frame = renderer.render_frame(&vp);
        let avg = avg_color(&frame.pixels);
        assert!(
            avg.g > 0.4,
            "Wireframe should show env background, got g={:.3}",
            avg.g
        );
    }

    #[test]
    fn clear_color_mode_is_black() {
        let mut renderer = SoftwareRenderer3D::new();
        let env = Environment3D {
            background_mode: BackgroundMode::ClearColor,
            ..Default::default()
        };
        let vp = viewport_with_env(env);
        let frame = renderer.render_frame(&vp);
        assert!(frame
            .pixels
            .iter()
            .all(|c| c.r < 0.01 && c.g < 0.01 && c.b < 0.01));
    }

    #[test]
    fn viewport_environment_defaults_to_none() {
        let vp = Viewport3D::new(64, 64);
        assert!(vp.environment.is_none());
    }
}
