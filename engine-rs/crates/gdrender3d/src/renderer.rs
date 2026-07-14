//! Software 3D renderer with wireframe and solid shaded modes.
//!
//! Provides a CPU-based 3D renderer for testing and validation.
//! Supports wireframe rendering (mesh edges) and solid rendering
//! with a programmable vertex/fragment shader pipeline.

use gdcore::math::{srgb_to_linear_channel, Color, Vector3};
use gdcore::math3d::Transform3D;

use gdserver3d::environment::{BackgroundMode, Environment3D, FogMode};
use gdserver3d::instance::{Instance3D, Instance3DId};
use gdserver3d::light::{Light3D, Light3DId, LightType};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::projection::perspective_projection_matrix;
use gdserver3d::reflection_probe::{sample_probes_at, ReflectionProbe, ReflectionProbeId};
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::ShaderMaterial3D;
use gdserver3d::sky::{PhysicalSkyMaterial, ProceduralSkyMaterial, SkyMaterial};
use gdserver3d::viewport::Viewport3D;

use crate::depth_buffer::DepthBuffer;
use crate::rasterizer::{clip_to_screen, rasterize_triangle};
use crate::shader::{
    fragment_shader_for_mode, CustomFragmentShader, FragmentShader, LightKind, LightUniform,
    MaterialTextures, ShaderUniforms, StandardVertexShader, VertexInput, VertexShader,
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
    reflection_probes: Vec<ReflectionProbe>,
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
            reflection_probes: Vec::new(),
            next_id: 1,
            render_mode: RenderMode::Solid,
        }
    }

    /// Creates a new software renderer in wireframe mode.
    pub fn wireframe() -> Self {
        Self {
            instances: Vec::new(),
            lights: Vec::new(),
            reflection_probes: Vec::new(),
            next_id: 1,
            render_mode: RenderMode::Wireframe,
        }
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
            Self::fill_background(w, h, env, &inv_proj, &viewport.camera_transform, &sky_suns_from_lights(&self.lights))
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
                .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0))
                .srgb_to_linear();

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

        // Fog then the camera post-process (linear HDR → display sRGB), matching
        // the solid path; the default env applies when the viewport has none.
        let default_env = Environment3D::default();
        let env = viewport.environment.as_ref().unwrap_or(&default_env);
        let fog_inv_proj = invert_projection(&proj);
        Self::apply_fog_pass(
            &mut pixels,
            &depth_normalized,
            w,
            h,
            env,
            &fog_inv_proj,
            &viewport.camera_transform,
            &sky_suns_from_lights(&self.lights),
        );
        // SSAO runs before tonemapping, gated on the environment.
        crate::postprocess::apply_ssao_env(&mut pixels, &depth_normalized, w, h, env);
        crate::postprocess::apply_camera_post(&mut pixels, w, h, env);

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
            Self::fill_background(w, h, env, &inv_proj, &viewport.camera_transform, &sky_suns_from_lights(&self.lights))
        } else {
            vec![Color::BLACK; pixel_count]
        };

        let mut depth_buf = DepthBuffer::new(w, h);
        let view_matrix = Self::transform_to_view_matrix(&viewport.camera_transform);

        // Ambient light color (linear) from the environment, applied per surface
        // as `ambient * albedo`. Defaults to a small grey when there is no
        // environment (matching the old flat ambient for the bare renderer).
        let ambient = match viewport.environment.as_ref() {
            Some(env) => environment_ambient_color(env),
            None => Color::new(0.1, 0.1, 0.1, 1.0),
        };

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
                // SKY_ONLY lights (affects_scene == false) draw their sky sun
                // disc but must not shade geometry, so zero their scene energy.
                // The light keeps its slot — shadow-map indices stay aligned with
                // `self.lights`, and the sky-sun path reads `self.lights` directly
                // (not these uniforms), so its disc is unaffected.
                let shade_energy = if l.affects_scene { l.energy } else { 0.0 };
                LightUniform {
                    kind,
                    direction,
                    // Light color is sRGB authoring → linear, times energy
                    // (Godot shades in linear space).
                    color: Color::new(
                        srgb_to_linear_channel(l.color.r) * shade_energy,
                        srgb_to_linear_channel(l.color.g) * shade_energy,
                        srgb_to_linear_channel(l.color.b) * shade_energy,
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
                    // Directional + spot share 2D depth shadow maps (pat-bt77r).
                    if lu.shadow_enabled
                        && matches!(lu.kind, LightKind::Directional | LightKind::Spot)
                    {
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
                    // Material colors are sRGB authoring values; shade in linear
                    // (the camera post-process encodes back to sRGB).
                    albedo: entry.material.albedo.srgb_to_linear(),
                    emission: entry.material.emission.srgb_to_linear(),
                    ambient,
                    roughness: entry.material.roughness,
                    metallic: entry.material.metallic,
                    camera_position: viewport.camera_transform.origin,
                    lights: light_uniforms.clone(),
                    directional_shadow_maps: directional_shadow_maps.clone(),
                    omni_shadow_cubemaps: omni_shadow_cubemaps.clone(),
                    // Per-fragment material texture maps (albedo/normal/ORM/
                    // emission), sampled at the interpolated UV. Arc clones are
                    // cheap; `None` fields fall back to the flat scalars.
                    textures: MaterialTextures {
                        albedo: entry.material.albedo_texture.clone(),
                        normal: entry.material.normal_texture.clone(),
                        normal_scale: entry.material.normal_scale,
                        roughness: entry.material.roughness_texture.clone(),
                        metallic: entry.material.metallic_texture.clone(),
                        emission: entry.material.emission_texture.clone(),
                    },
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

        // Fog (linear scene space) then the camera post-process. The scene is
        // rendered in linear HDR; the post-process (glow → tonemap →
        // linear→sRGB) always runs — even with no environment it applies the
        // default (Linear tonemap, glow off), i.e. just the sRGB encode — so an
        // sRGB scene converted to linear on input round-trips to display sRGB.
        let default_env = Environment3D::default();
        let env = viewport.environment.as_ref().unwrap_or(&default_env);
        let fog_inv_proj = invert_projection(&proj);
        Self::apply_fog_pass(
            &mut pixels,
            &depth_normalized,
            w,
            h,
            env,
            &fog_inv_proj,
            &viewport.camera_transform,
            &sky_suns_from_lights(&self.lights),
        );
        // SSAO runs before tonemapping, gated on the environment.
        crate::postprocess::apply_ssao_env(&mut pixels, &depth_normalized, w, h, env);
        crate::postprocess::apply_camera_post(&mut pixels, w, h, env);

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
        suns: &[SkySun],
    ) -> Vec<Color> {
        let pixel_count = (w * h) as usize;
        match env.background_mode {
            BackgroundMode::CustomColor => {
                // sRGB authoring color → linear (the post-process encodes back).
                vec![env.background_color.srgb_to_linear(); pixel_count]
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
                            suns,
                        );
                    }
                }
                pixels
            }
            _ => vec![Color::BLACK; pixel_count],
        }
    }

    /// Applies Godot's fog to a linear HDR frame in place: distance/height fog
    /// (with sun scatter + aerial perspective) over geometry, and `fog_sky_affect`
    /// over the sky background. Reconstructs each fragment's view-space position
    /// from its depth via the inverse projection, so distances and heights are
    /// world-accurate rather than a raw depth fraction. Runs before the camera
    /// post-process, in linear space.
    /// `pub(crate)` so the GPU trait path applies the identical fog to its
    /// read-back HDR buffer (pat-kr04d) — same reconstruction, same math.
    pub(crate) fn apply_fog_pass(
        pixels: &mut [Color],
        depth: &[f32],
        w: u32,
        h: u32,
        env: &Environment3D,
        inv_proj: &[[f32; 4]; 4],
        camera_transform: &Transform3D,
        suns: &[SkySun],
    ) {
        if !env.fog_enabled {
            return;
        }
        for y in 0..h {
            let ndc_y = 1.0 - 2.0 * (y as f32 + 0.5) / h as f32;
            for x in 0..w {
                let ndc_x = 2.0 * (x as f32 + 0.5) / w as f32 - 1.0;
                let i = (y * w + x) as usize;
                let d = depth[i];

                // World-space view direction through this pixel (for scatter,
                // aerial perspective, and sky-color sampling).
                let far_view = pixel_view_position(ndc_x, ndc_y, 1.0, inv_proj);
                let view_dir_world = camera_transform.basis.xform(far_view).normalized();

                if d < 1.0 {
                    // Geometry: reconstruct the fragment's view/world position.
                    let view_pos = pixel_view_position(ndc_x, ndc_y, d, inv_proj);
                    let distance = view_pos.length();
                    let world_pos = camera_transform.xform(view_pos);
                    // Aerial perspective samples the sky along the view ray.
                    let sky_in_view = env.sky.as_ref().map(|s| {
                        sample_sky_color(
                            &s.material,
                            &view_dir_world,
                            env.background_energy_multiplier,
                            suns,
                        )
                    });
                    let (fog_color, amount) = fog_process(
                        env,
                        distance,
                        world_pos.y,
                        &view_dir_world,
                        suns,
                        sky_in_view,
                    );
                    pixels[i] = Color::new(
                        lerp(pixels[i].r, fog_color.r, amount),
                        lerp(pixels[i].g, fog_color.g, amount),
                        lerp(pixels[i].b, fog_color.b, amount),
                        pixels[i].a,
                    );
                } else if env.fog_sky_affect > 0.0 {
                    // Sky background: blend toward the fog color (base + sun
                    // scatter, no aerial — it *is* the sky) by fog_sky_affect.
                    let (fog_color, _) =
                        fog_process(env, 1.0, 0.0, &view_dir_world, suns, None);
                    let a = env.fog_sky_affect;
                    pixels[i] = Color::new(
                        lerp(pixels[i].r, fog_color.r, a),
                        lerp(pixels[i].g, fog_color.g, a),
                        lerp(pixels[i].b, fog_color.b, a),
                        pixels[i].a,
                    );
                }
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
    suns: &[SkySun],
) -> Color {
    match env.background_mode {
        BackgroundMode::CustomColor => env.background_color.srgb_to_linear(),
        BackgroundMode::Sky => {
            if let Some(ref sky) = env.sky {
                let dir = pixel_view_direction(ndc_x, ndc_y, inv_proj, camera_transform);
                sample_sky_color(&sky.material, &dir, env.background_energy_multiplier, suns)
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

/// A directional light's contribution to sky rendering — Godot's
/// `LIGHT0..3_*` sky-shader inputs (`SkyDirectionalLightData` in
/// `servers/rendering/renderer_rd/environment/sky.cpp`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkySun {
    /// World-space direction **toward** the light
    /// (`light_transform.basis.xform(Vector3(0, 0, 1))` in Godot — the
    /// opposite of the direction the light shines).
    pub direction: Vector3,
    /// Light color (`LIGHTn_COLOR`).
    pub color: Color,
    /// Light energy multiplier (`LIGHTn_ENERGY`).
    pub energy: f32,
    /// Angular diameter in radians (`LIGHTn_SIZE`, from the light's
    /// `angular_distance` degrees). 0 draws no hard disc, only the halo.
    pub size: f32,
}

/// Computes the environment's ambient light color in **linear** space, applied
/// to surfaces as `ambient * albedo`. Mirrors Godot's `AmbientSource`:
///
/// * `Sky` — a cheap estimate of the sky's diffuse irradiance: the average of
///   the procedural sky sampled over the 6 axis directions (no sun disc, so the
///   ambient is the soft dome color, not a hotspot), times `ambient_energy`.
/// * `Color` — the flat `ambient_color` (sRGB→linear) times energy.
/// * `Background` — the sky average if there is a sky, else the background color.
/// * `Disabled` — black.
///
/// This replaces the old flat grey ambient so sky-lit surfaces take on the
/// sky's tint (and brightness) the way Godot's editor viewport shows them.
pub fn environment_ambient_color(env: &Environment3D) -> Color {
    use gdserver3d::environment::AmbientSource;
    let energy = env.ambient_energy;

    let sky_average = |env: &Environment3D| -> Option<Color> {
        let sky = env.sky.as_ref()?;
        let dirs = [
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, -1.0),
        ];
        let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
        for d in &dirs {
            let c = sample_sky_color(&sky.material, d, env.background_energy_multiplier, &[]);
            r += c.r;
            g += c.g;
            b += c.b;
        }
        let n = dirs.len() as f32;
        Some(Color::new(r / n, g / n, b / n, 1.0))
    };

    let scaled = |c: Color| Color::new(c.r * energy, c.g * energy, c.b * energy, 1.0);

    match env.ambient_source {
        AmbientSource::Disabled => Color::BLACK,
        AmbientSource::Color => scaled(env.ambient_color.srgb_to_linear()),
        AmbientSource::Sky => scaled(sky_average(env).unwrap_or(Color::BLACK)),
        AmbientSource::Background => match sky_average(env) {
            Some(avg) => scaled(avg),
            None => scaled(env.background_color.srgb_to_linear()),
        },
    }
}

/// Collects the sky-visible directional lights the way Godot's sky renderer
/// does: scene order, capped at 4 (`MAX_DIRECTIONAL_LIGHT_DATA_STRUCTS`).
pub fn sky_suns_from_lights(lights: &[Light3D]) -> Vec<SkySun> {
    lights
        .iter()
        .filter(|l| l.light_type == LightType::Directional && l.sky)
        .take(4)
        .map(|l| SkySun {
            direction: (Vector3::ZERO - l.direction).normalized(),
            color: l.color,
            energy: l.energy,
            size: l.angular_distance.to_radians(),
        })
        .collect()
}

/// Samples a sky material color for the given world-space view direction.
/// `pub(crate)` so the sky parity tests can probe materials directly.
pub(crate) fn sample_sky_color(
    material: &SkyMaterial,
    dir: &Vector3,
    energy: f32,
    suns: &[SkySun],
) -> Color {
    match material {
        SkyMaterial::Procedural(mat) => sample_procedural_sky(mat, dir, energy, suns),
        SkyMaterial::Physical(mat) => sample_physical_sky(mat, dir, energy, suns),
        SkyMaterial::Panoramic(mat) => sample_panoramic_sky(mat, dir, energy),
    }
}

/// Equirectangular panorama sampling (Godot `SKY_COORDS`).
///
/// Godot's sky.glsl computes `atan(x, -z)` wrapped into [0, 2π) (negative →
/// +2π) then divides by 2π — **no half-turn shift** — so −Z maps to u = 0,
/// +X to 0.25, +Z to 0.5, −X to 0.75. `rem_euclid(1.0)` performs the same
/// wrap. `v = acos(y) / π` (zenith at the top row).
pub fn equirect_uv(dir: &Vector3) -> (f32, f32) {
    let d = dir.normalized();
    let u = d.x.atan2(-d.z) / (2.0 * std::f32::consts::PI);
    let v = d.y.clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
    (u.rem_euclid(1.0), v.clamp(0.0, 1.0))
}

/// Samples a panorama texture at a world direction.
pub fn sample_equirect(
    tex: &gdserver3d::sky::PanoramaTexture,
    dir: &Vector3,
    filter: bool,
) -> Color {
    let (u, v) = equirect_uv(dir);
    let w = tex.width.max(1) as f32;
    let h = tex.height.max(1) as f32;
    if !filter || tex.width < 2 || tex.height < 2 {
        let x = ((u * w).floor() as u32).min(tex.width.saturating_sub(1));
        let y = ((v * h).floor() as u32).min(tex.height.saturating_sub(1));
        let idx = (y * tex.width + x) as usize;
        return tex.pixels.get(idx).copied().unwrap_or(Color::BLACK);
    }
    // GL/Godot repeat-sampler semantics: texel centers sit at (i + 0.5)/w, so
    // sample at `u·w − 0.5` and wrap the x neighbors across the u = 0/1 seam
    // (the panorama is horizontally periodic; a clamped `u·(w−1)` mapping
    // leaves a visible seam column at the −Z direction). Vertically clamp —
    // the poles don't wrap.
    let x = u * w - 0.5;
    let y = (v * h - 0.5).clamp(0.0, h - 1.0);
    let x0f = x.floor();
    let y0 = y.floor() as u32;
    let x0 = x0f.rem_euclid(w) as u32 % tex.width;
    let x1 = (x0f + 1.0).rem_euclid(w) as u32 % tex.width;
    let y1 = (y0 + 1).min(tex.height - 1);
    let fx = x - x0f;
    let fy = y - y0 as f32;
    let sample = |xx: u32, yy: u32| {
        let idx = (yy * tex.width + xx) as usize;
        tex.pixels.get(idx).copied().unwrap_or(Color::BLACK)
    };
    let c00 = sample(x0, y0);
    let c10 = sample(x1, y0);
    let c01 = sample(x0, y1);
    let c11 = sample(x1, y1);
    let lerp_c = |a: Color, b: Color, t: f32| {
        Color::new(
            a.r + (b.r - a.r) * t,
            a.g + (b.g - a.g) * t,
            a.b + (b.b - a.b) * t,
            a.a + (b.a - a.a) * t,
        )
    };
    let top = lerp_c(c00, c10, fx);
    let bot = lerp_c(c01, c11, fx);
    lerp_c(top, bot, fy)
}

fn sample_panoramic_sky(mat: &gdserver3d::sky::PanoramicSkyMaterial, dir: &Vector3, energy: f32) -> Color {
    if let Some(tex) = mat.texture.as_ref() {
        if !tex.pixels.is_empty() && tex.width > 0 && tex.height > 0 {
            let c = sample_equirect(tex, dir, mat.filter);
            let e = mat.energy_multiplier * energy;
            return Color::new(c.r * e, c.g * e, c.b * e, 1.0);
        }
    }
    // No texture loaded — neutral linear placeholder (matches prior behavior).
    Color::new(
        srgb_to_linear_channel(0.3) * mat.energy_multiplier * energy,
        srgb_to_linear_channel(0.3) * mat.energy_multiplier * energy,
        srgb_to_linear_channel(0.35) * mat.energy_multiplier * energy,
        1.0,
    )
}

/// Evaluates the procedural sky for a view direction — a port of Godot 4.6's
/// generated `ProceduralSkyMaterial` sky shader
/// (`scene/resources/3d/sky_material.cpp`):
///
/// ```glsl
/// vec3 sky = mix(sky_top_color, sky_horizon_color,
///                clamp(pow(1.0 - v_angle, inv_sky_curve), 0.0, 1.0));
/// // per enabled directional light:
/// //   full LIGHT_COLOR * LIGHT_ENERGY inside the disc (dot > cos(LIGHT_SIZE)),
/// //   halo falloff pow(1.0 - c2, inv_sun_curve) out to sun_angle_max
/// vec3 ground = mix(ground_bottom_color, ground_horizon_color,
///                   clamp(pow(1.0 + v_angle, inv_ground_curve), 0.0, 1.0));
/// COLOR = mix(ground, sky, step(0.0, EYEDIR.y));
/// ```
///
/// with Godot's uniform mappings `inv_sky_curve = 0.6 / sky_curve`,
/// `inv_ground_curve = 0.6 / ground_curve`, `sun_angle_max` as
/// `cos(deg_to_rad(degrees))`, and `inv_sun_curve = 1.6 / pow(sun_curve, 1.4)`.
///
/// **Output is linear scene-referred HDR, not display sRGB.** The material's
/// `source_color` uniforms are sRGB authoring values, so each is premultiplied
/// by its hemisphere energy and converted sRGB→linear exactly as Godot's
/// setter + `source_color` binding does (`srgb_to_linear(color * energy)`), the
/// gradient/sun mix happens in linear space, and the result is *not* clamped —
/// a bright sun (`LIGHT_COLOR * LIGHT_ENERGY > 1`) stays HDR so the camera
/// post-process can bloom and tonemap it. The final `linear_to_srgb` encode is
/// the post-process's job, not the sky's.
fn sample_procedural_sky(
    mat: &ProceduralSkyMaterial,
    dir: &Vector3,
    energy: f32,
    suns: &[SkySun],
) -> Color {
    let v_angle = dir.y.clamp(-1.0, 1.0);
    // Godot applies hemisphere energy in sRGB space, then the `source_color`
    // bind converts to linear: `srgb_to_linear(channel * energy)`.
    let lin = |c: f32, e: f32| srgb_to_linear_channel(c * e);

    let (r, g, b) = if v_angle >= 0.0 {
        // Sky hemisphere: horizon→top gradient, then sun discs/halos on top.
        let inv_sky_curve = 0.6 / mat.sky_curve.max(1e-4);
        let t = (1.0 - v_angle).powf(inv_sky_curve).clamp(0.0, 1.0);
        let e = mat.sky_energy_multiplier;
        let mut rgb = (
            lerp(lin(mat.sky_top_color.r, e), lin(mat.sky_horizon_color.r, e), t),
            lerp(lin(mat.sky_top_color.g, e), lin(mat.sky_horizon_color.g, e), t),
            lerp(lin(mat.sky_top_color.b, e), lin(mat.sky_horizon_color.b, e), t),
        );

        let cos_angle_max = mat.sun_angle_max.to_radians().cos();
        let inv_sun_curve = 1.6 / mat.sun_curve.max(1e-4).powf(1.4);
        for sun in suns.iter().take(4) {
            let sun_angle = sun.direction.dot(*dir);
            let sun_size = sun.size.cos();
            // LIGHTn_COLOR is the light color converted to linear (sky.cpp:
            // `light_get_color(base).srgb_to_linear()`), times LIGHTn_ENERGY.
            let sun_rgb = (
                srgb_to_linear_channel(sun.color.r) * sun.energy,
                srgb_to_linear_channel(sun.color.g) * sun.energy,
                srgb_to_linear_channel(sun.color.b) * sun.energy,
            );
            if sun_angle > sun_size {
                rgb = sun_rgb;
            } else if sun_angle > cos_angle_max && sun_size > cos_angle_max {
                let c2 = (sun_size - sun_angle) / (sun_size - cos_angle_max);
                let f = (1.0 - c2).powf(inv_sun_curve).clamp(0.0, 1.0);
                rgb = (
                    lerp(rgb.0, sun_rgb.0, f),
                    lerp(rgb.1, sun_rgb.1, f),
                    lerp(rgb.2, sun_rgb.2, f),
                );
            }
        }

        // Godot's `sky_cover`: sample the cover texture equirectangularly
        // (`SKY_COORDS`) and add it over the sky hemisphere, modulated by
        // `sky_cover_modulate`. Per the material's setters the modulate rgb is
        // an sRGB source_color (→ linear) and its alpha carries the
        // `sky_energy_multiplier`:
        // `sky += cover.rgb·modulate.rgb · cover.a·(modulate.a·sky_energy)`.
        if let Some(cover) = &mat.sky_cover {
            let c = sample_equirect(cover, dir, true);
            let m = mat.sky_cover_modulate;
            let ma = m.a * mat.sky_energy_multiplier;
            rgb.0 += c.r * srgb_to_linear_channel(m.r) * c.a * ma;
            rgb.1 += c.g * srgb_to_linear_channel(m.g) * c.a * ma;
            rgb.2 += c.b * srgb_to_linear_channel(m.b) * c.a * ma;
        }
        rgb
    } else {
        // Ground hemisphere: bottom→horizon gradient (no sun below horizon).
        let inv_ground_curve = 0.6 / mat.ground_curve.max(1e-4);
        let t = (1.0 + v_angle).powf(inv_ground_curve).clamp(0.0, 1.0);
        let e = mat.ground_energy_multiplier;
        (
            lerp(lin(mat.ground_bottom_color.r, e), lin(mat.ground_horizon_color.r, e), t),
            lerp(lin(mat.ground_bottom_color.g, e), lin(mat.ground_horizon_color.g, e), t),
            lerp(lin(mat.ground_bottom_color.b, e), lin(mat.ground_horizon_color.b, e), t),
        )
    };

    // Environment background energy (the shader's `* exposure`), in linear.
    Color::new(r * energy, g * energy, b * energy, 1.0)
}

/// Physical sky — a **verbatim port** of Godot 4.6's generated
/// `PhysicalSkyMaterial` sky shader (`PhysicalSkyMaterial::_update_shader` in
/// `scene/resources/3d/sky_material.cpp`): Preetham-style Rayleigh/Mie betas,
/// the zenith optical-mass formula, per-channel extinction, the three.js
/// low-sun mix, the ground-color horizon blend, the smoothstep solar disk
/// (sized by `LIGHT0_SIZE * sun_disk_scale`), and the final
/// `pow(color, 1/(1.2 + 1.2·sun_fade))`. `LIGHT0` is the first sky-visible
/// directional light; with no sun Godot shows only the night-sky texture
/// (default black).
fn sample_physical_sky(
    mat: &PhysicalSkyMaterial,
    dir: &Vector3,
    energy: f32,
    suns: &[SkySun],
) -> Color {
    let d = dir.normalized();
    let exposure = mat.energy_multiplier * energy;
    // Godot's night_sky texture (source_color → linear, sampled at SKY_COORDS).
    let night = mat.night_sky.as_ref().map(|tex| {
        let c = sample_equirect(tex, &d, true);
        [c.r, c.g, c.b]
    });
    let Some(sun) = suns.first() else {
        // No sun: Godot displays only the night_sky (default black) × exposure.
        return match night {
            Some(n) => Color::new(n[0] * exposure, n[1] * exposure, n[2] * exposure, 1.0),
            None => Color::new(0.0, 0.0, 0.0, 1.0),
        };
    };
    // LIGHTn/material colors arrive linear (`source_color` / `srgb_to_linear`).
    let sun_dir = sun.direction.normalized();
    let sun_col = [
        srgb_to_linear_channel(sun.color.r),
        srgb_to_linear_channel(sun.color.g),
        srgb_to_linear_channel(sun.color.b),
    ];
    let rayleigh_col = [
        srgb_to_linear_channel(mat.rayleigh_color.r),
        srgb_to_linear_channel(mat.rayleigh_color.g),
        srgb_to_linear_channel(mat.rayleigh_color.b),
    ];
    let mie_col = [
        srgb_to_linear_channel(mat.mie_color.r),
        srgb_to_linear_channel(mat.mie_color.g),
        srgb_to_linear_channel(mat.mie_color.b),
    ];
    let ground_col = [
        srgb_to_linear_channel(mat.ground_color.r),
        srgb_to_linear_channel(mat.ground_color.g),
        srgb_to_linear_channel(mat.ground_color.b),
    ];

    // Optical length at zenith for molecules.
    const RAYLEIGH_ZENITH_SIZE: f32 = 8.4e3;
    const MIE_ZENITH_SIZE: f32 = 1.25e3;

    let zenith_angle = sun_dir.y.clamp(-1.0, 1.0); // dot(UP, LIGHT0_DIRECTION)
    let sun_energy = (0.757 * zenith_angle).max(0.0) * sun.energy;
    let sun_fade = 1.0 - (1.0 - sun_dir.y.exp()).clamp(0.0, 1.0);

    // Rayleigh coefficients (fade shrinks them as the sun sets).
    let rayleigh_coefficient = mat.rayleigh_coefficient - 1.0 * (1.0 - sun_fade);
    let rayleigh_beta = [
        rayleigh_coefficient * rayleigh_col[0] * 0.0001,
        rayleigh_coefficient * rayleigh_col[1] * 0.0001,
        rayleigh_coefficient * rayleigh_col[2] * 0.0001,
    ];
    // Mie coefficients from Preetham.
    let mie_beta = [
        mat.turbidity * mat.mie_coefficient * mie_col[0] * 0.000434,
        mat.turbidity * mat.mie_coefficient * mie_col[1] * 0.000434,
        mat.turbidity * mat.mie_coefficient * mie_col[2] * 0.000434,
    ];

    // Optical length.
    let zenith = d.y.max(0.0); // dot(UP, EYEDIR)
    let optical_mass = 1.0 / (zenith + 0.15 * (3.885 + 54.5 * zenith).powf(-1.253));
    let rayleigh_scatter = RAYLEIGH_ZENITH_SIZE * optical_mass;
    let mie_scatter = MIE_ZENITH_SIZE * optical_mass;

    // Light extinction based on thickness of atmosphere.
    let extinction = [
        (-(rayleigh_beta[0] * rayleigh_scatter + mie_beta[0] * mie_scatter)).exp(),
        (-(rayleigh_beta[1] * rayleigh_scatter + mie_beta[1] * mie_scatter)).exp(),
        (-(rayleigh_beta[2] * rayleigh_scatter + mie_beta[2] * mie_scatter)).exp(),
    ];

    // In scattering.
    let cos_theta = d.dot(sun_dir);
    let rayleigh_phase =
        (3.0 / (16.0 * std::f32::consts::PI)) * (1.0 + (cos_theta * 0.5 + 0.5).powi(2));
    let mie_phase = henyey_greenstein(cos_theta, mat.mie_eccentricity);

    // Lin = pow(sun_energy·(βθ/β)·(1−ext), 1.5), the three.js low-sun hack
    // mix toward pow(sun_energy·(βθ/β)·ext, 0.5), then the ground blend.
    let low_sun_mix = (1.0 - zenith_angle).powi(5).clamp(0.0, 1.0);
    let ground_blend = smoothstep(-0.1, 0.1, d.y);
    let mut lin = [0.0f32; 3];
    for i in 0..3 {
        let beta_theta = rayleigh_beta[i] * rayleigh_phase + mie_beta[i] * mie_phase;
        let beta_sum = (rayleigh_beta[i] + mie_beta[i]).max(1e-12);
        let base = sun_energy * (beta_theta / beta_sum);
        let mut l = (base * (1.0 - extinction[i])).max(0.0).powf(1.5);
        let l_low = (base * extinction[i]).max(0.0).powf(0.5);
        l *= 1.0 + (l_low - 1.0) * low_sun_mix;
        l *= ground_col[i] + (1.0 - ground_col[i]) * ground_blend;
        lin[i] = l;
    }

    // Solar disk and out-scattering.
    let sun_ang_cos = (sun.size * mat.sun_disk_scale).cos();
    let sun_ang_cos2 = (sun.size * mat.sun_disk_scale * 0.5).cos();
    let sundisk = smoothstep(sun_ang_cos, sun_ang_cos2, cos_theta);

    // COLOR = pow(Lin + L0, 1/(1.2 + 1.2·sun_fade)) * exposure.
    let final_pow = 1.0 / (1.2 + 1.2 * sun_fade);
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let mut l0 = sun_energy * extinction[i] * sundisk * sun_col[i];
        // Godot: `L0 += texture(night_sky, SKY_COORDS).xyz * extinction` — the
        // night sky shows through where the atmosphere is thin (high extinction,
        // i.e. toward the horizon / as the sun sets).
        if let Some(n) = night {
            l0 += n[i] * extinction[i];
        }
        out[i] = (lin[i] + l0).max(0.0).powf(final_pow) * exposure;
    }
    Color::new(out[0], out[1], out[2], 1.0)
}

/// Henyey-Greenstein phase function with Godot's baked `k = 1/(4π)` constant
/// (`0.0795774715459` in the generated PhysicalSkyMaterial shader).
fn henyey_greenstein(cos_theta: f32, g: f32) -> f32 {
    const K: f32 = 0.079_577_47;
    K * (1.0 - g * g) / (1.0 + g * g - 2.0 * g * cos_theta).max(1e-9).powf(1.5)
}

/// Reconstructs a view-space position from a pixel's NDC coordinates and depth,
/// using the inverse projection. `ndc_z` is the stored (GL-convention) depth.
fn pixel_view_position(
    ndc_x: f32,
    ndc_y: f32,
    ndc_z: f32,
    inv_proj: &[[f32; 4]; 4],
) -> Vector3 {
    let x = inv_proj[0][0] * ndc_x + inv_proj[1][0] * ndc_y + inv_proj[2][0] * ndc_z + inv_proj[3][0];
    let y = inv_proj[0][1] * ndc_x + inv_proj[1][1] * ndc_y + inv_proj[2][1] * ndc_z + inv_proj[3][1];
    let z = inv_proj[0][2] * ndc_x + inv_proj[1][2] * ndc_y + inv_proj[2][2] * ndc_z + inv_proj[3][2];
    let w = inv_proj[0][3] * ndc_x + inv_proj[1][3] * ndc_y + inv_proj[2][3] * ndc_z + inv_proj[3][3];
    if w.abs() > 1e-9 {
        Vector3::new(x / w, y / w, z / w)
    } else {
        Vector3::new(x, y, z)
    }
}

/// The fog color and amount for a fragment, a port of Godot 4.6's `fog_process`
/// (`scene_forward_clustered.glsl`). All colors are **linear** (fog runs in
/// linear scene space, before tonemapping).
///
/// * `view_distance` — distance from the camera to the fragment.
/// * `world_y` — the fragment's world-space height (for height fog).
/// * `view_dir_world` — normalized direction from the camera to the fragment
///   (for sun scatter and aerial perspective).
/// * `suns` — sky-visible directional lights (for sun scatter).
/// * `sky_in_view` — the linear sky color along `view_dir_world`, or `None`
///   (for aerial perspective).
fn fog_process(
    env: &Environment3D,
    view_distance: f32,
    world_y: f32,
    view_dir_world: &Vector3,
    suns: &[SkySun],
    sky_in_view: Option<Color>,
) -> (Color, f32) {
    // Base fog color: sRGB authoring → linear, times energy.
    let base = env.fog_light_color.srgb_to_linear();
    let mut fog = [
        base.r * env.fog_light_energy,
        base.g * env.fog_light_energy,
        base.b * env.fog_light_energy,
    ];

    // Aerial perspective: blend the fog color toward the sky in the view dir.
    if env.fog_aerial_perspective > 0.0 {
        if let Some(sky) = sky_in_view {
            let a = env.fog_aerial_perspective;
            fog = [
                lerp(fog[0], sky.r, a),
                lerp(fog[1], sky.g, a),
                lerp(fog[2], sky.b, a),
            ];
        }
    }

    // Sun scatter: directional lights bleed into the fog toward the viewer.
    if env.fog_sun_scatter > 0.001 {
        for sun in suns {
            let amt = view_dir_world.dot(sun.direction).max(0.0).powf(8.0) * env.fog_sun_scatter;
            fog[0] += srgb_to_linear_channel(sun.color.r) * sun.energy * amt;
            fog[1] += srgb_to_linear_channel(sun.color.g) * sun.energy * amt;
            fog[2] += srgb_to_linear_channel(sun.color.b) * sun.energy * amt;
        }
    }

    // Fog amount from distance.
    let mut amount = match env.fog_mode {
        FogMode::Exponential => {
            // Godot: 1 - exp(min(0, -distance * density)).
            1.0 - (-view_distance * env.fog_density).min(0.0).exp()
        }
        FogMode::Depth => {
            let z = smoothstep(env.fog_depth_begin, env.fog_depth_end, view_distance);
            z.powf(env.fog_depth_curve.max(1e-4)) * env.fog_density
        }
    };

    // Height fog: strongest below `fog_height`, combined by max.
    if env.fog_height_density.abs() >= 0.0001 {
        let y_dist = world_y - env.fog_height;
        let vfog = 1.0 - (y_dist * env.fog_height_density).min(0.0).exp();
        amount = vfog.max(amount);
    }

    (
        Color::new(fog[0], fog[1], fog[2], 1.0),
        amount.clamp(0.0, 1.0),
    )
}

/// GLSL `smoothstep`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() < 1e-9 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Inverts a column-major 4x4 projection matrix.
///
/// Uses a simplified inversion that works for typical perspective matrices.
/// `pub(crate)` so the GPU sky pass unprojects pixels with the exact same
/// matrix as the software background fill.
pub(crate) fn invert_projection(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
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

    fn add_reflection_probe(&mut self, id: ReflectionProbeId) {
        if !self.reflection_probes.iter().any(|p| p.id == id) {
            self.reflection_probes.push(ReflectionProbe::new(id));
        }
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
    fn sky_only_light_does_not_shade_scene() {
        // A SKY_ONLY directional light (affects_scene == false) draws its sky sun
        // disc but must not light scene geometry. Render the same lit cube three
        // ways and confirm SKY_ONLY shades exactly like a zero-energy light.
        fn render_cube(energy: f32, affects_scene: bool) -> Vec<Color> {
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
            let mut light = Light3D::directional(Light3DId(1));
            light.direction = Vector3::new(0.0, 0.0, -1.0); // toward the camera-facing face
            light.energy = energy;
            light.affects_scene = affects_scene;
            renderer.add_light(light.id);
            renderer.update_light(&light);
            renderer.render_frame(&Viewport3D::new(48, 48)).pixels
        }

        let lit = render_cube(5.0, true);
        let sky_only = render_cube(5.0, false);
        let zero_energy = render_cube(0.0, true);

        // SKY_ONLY contributes nothing to shading — pixel-identical to energy 0.
        assert_eq!(
            sky_only, zero_energy,
            "SKY_ONLY must shade the scene exactly like a zero-energy light"
        );
        // Guard against a no-op test: the light must matter when it does shade.
        let sum = |px: &[Color]| px.iter().map(|c| c.r + c.g + c.b).sum::<f32>();
        assert!(
            sum(&lit) > sum(&sky_only) + 1.0,
            "an affects_scene light must visibly shade the cube"
        );
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

    /// Regression: the procedural sky must use Godot's gradient formula
    /// `mix(top, horizon, pow(1 ∓ y, 0.6 / curve))`, not `pow(|y|, curve)`.
    /// The old formula raced to the top/bottom colors right at the horizon,
    /// erasing the bright horizon haze band ("fog") Godot shows.
    ///
    /// The sampler outputs **linear** HDR, so expected values are computed with
    /// the same sRGB→linear conversion the implementation uses.
    #[test]
    fn procedural_sky_horizon_band_matches_godot() {
        use gdcore::math::srgb_to_linear_channel as s2l;
        let mat = ProceduralSkyMaterial::default();

        // Slightly above the horizon: pow(0.9, 4) ≈ 0.656 → still mostly the
        // whitish horizon color (0.6463), not the darker top color (0.385).
        // Endpoints are linearized (Godot's source_color), then mixed in linear.
        let just_above = sample_procedural_sky(&mat, &Vector3::new(0.0, 0.1, -1.0).normalized(), 1.0, &[]);
        let t = (1.0f32 - 0.099_503_72).powf(0.6 / 0.15);
        let expected_r = s2l(0.385) + (s2l(0.6463) - s2l(0.385)) * t;
        assert!(
            (just_above.r - expected_r).abs() < 1e-3,
            "sky gradient must follow Godot's pow(1-y, 0.6/curve) in linear: got r={:.4}, want {:.4}",
            just_above.r,
            expected_r
        );
        // The horizon band must be visibly brighter than the zenith.
        let zenith = sample_procedural_sky(&mat, &Vector3::new(0.0, 1.0, 0.0), 1.0, &[]);
        assert!(
            just_above.r > zenith.r + 0.05,
            "horizon haze must be brighter than zenith: horizon r={:.3}, zenith r={:.3}",
            just_above.r,
            zenith.r
        );

        // Just below the horizon: pow(0.98, 30) ≈ 0.545 → a whitish band, not
        // the dark ground_bottom color the old formula produced immediately.
        let just_below = sample_procedural_sky(&mat, &Vector3::new(0.0, -0.02, -1.0).normalized(), 1.0, &[]);
        assert!(
            just_below.r > s2l(0.4),
            "ground horizon band must stay bright just below the horizon, got r={:.3}",
            just_below.r
        );
        // Far below: pure ground_bottom_color (dark brown), in linear.
        let far_below = sample_procedural_sky(&mat, &Vector3::new(0.0, -1.0, 0.0), 1.0, &[]);
        assert!(
            (far_below.r - s2l(0.2)).abs() < 1e-3 && (far_below.b - s2l(0.133)).abs() < 1e-3,
            "nadir must be ground_bottom_color (linear), got {:?}",
            far_below
        );
    }

    /// Godot's `sky_cover`: the cover texture is added over the sky hemisphere
    /// (modulated), and never touches the ground hemisphere.
    #[test]
    fn procedural_sky_cover_adds_over_sky_hemisphere_only() {
        use gdserver3d::sky::PanoramaTexture;
        // 1×1 opaque cover, linear red 0.7.
        let cover = PanoramaTexture { width: 1, height: 1, pixels: vec![Color::new(0.7, 0.0, 0.0, 1.0)] };
        let mut mat = ProceduralSkyMaterial::default();
        mat.sky_cover = Some(cover);
        let bare = ProceduralSkyMaterial::default();

        // Sky hemisphere: cover adds 0.7·modulate(white,1)·alpha(1)·sky_energy(1) = 0.7 to red.
        let up = Vector3::new(0.0, 1.0, 0.0);
        let with = sample_procedural_sky(&mat, &up, 1.0, &[]);
        let without = sample_procedural_sky(&bare, &up, 1.0, &[]);
        assert!(
            (with.r - without.r - 0.7).abs() < 1e-4,
            "sky_cover must add its texture over the sky: with={:.3} without={:.3}",
            with.r, without.r
        );

        // Ground hemisphere: untouched.
        let down = Vector3::new(0.0, -1.0, 0.0);
        let with_g = sample_procedural_sky(&mat, &down, 1.0, &[]);
        let without_g = sample_procedural_sky(&bare, &down, 1.0, &[]);
        assert!(
            (with_g.r - without_g.r).abs() < 1e-6,
            "sky_cover must not affect the ground hemisphere"
        );

        // Modulate alpha 0 disables the cover entirely.
        mat.sky_cover_modulate = Color::new(1.0, 1.0, 1.0, 0.0);
        let muted = sample_procedural_sky(&mat, &up, 1.0, &[]);
        assert!((muted.r - without.r).abs() < 1e-6, "modulate.a=0 must hide sky_cover");
    }

    /// Godot's physical-sky `night_sky`: with no sun the whole sky is the
    /// night_sky texture (×exposure); with a sun it adds into `L0` scaled by
    /// extinction (so it shows where the atmosphere is thin — overhead).
    #[test]
    fn physical_sky_night_sky_no_sun_and_extinction_scaled() {
        use gdserver3d::sky::PanoramaTexture;
        let stars = PanoramaTexture { width: 1, height: 1, pixels: vec![Color::new(0.1, 0.1, 0.4, 1.0)] };
        let mut mat = PhysicalSkyMaterial::default();
        mat.night_sky = Some(stars);
        let bare = PhysicalSkyMaterial::default();
        let up = Vector3::new(0.0, 1.0, 0.0);

        // No sun: sky == night_sky × exposure (energy 1). Bare == black.
        let no_sun = sample_physical_sky(&mat, &up, 1.0, &[]);
        assert!(
            (no_sun.r - 0.1).abs() < 1e-4 && (no_sun.b - 0.4).abs() < 1e-4,
            "no-sun physical sky must display night_sky, got {:?}",
            no_sun
        );
        let no_sun_bare = sample_physical_sky(&bare, &up, 1.0, &[]);
        assert!(no_sun_bare.r < 1e-6 && no_sun_bare.b < 1e-6, "no night_sky → black");

        // With a sun overhead-ish, night_sky adds into L0 (× extinction ≈ 1 when
        // looking up through thin atmosphere), brightening the sky.
        let sun = SkySun {
            direction: Vector3::new(0.0, 0.3, -1.0).normalized(),
            color: Color::WHITE,
            energy: 1.0,
            size: 0.0,
        };
        let with_night = sample_physical_sky(&mat, &up, 1.0, &[sun]);
        let without_night = sample_physical_sky(&bare, &up, 1.0, &[sun]);
        assert!(
            with_night.b > without_night.b + 0.01,
            "night_sky must add through thin atmosphere: {:.3} vs {:.3}",
            with_night.b, without_night.b
        );
    }

    /// The sky draws a sun disc + halo per directional light exactly like
    /// Godot's ProceduralSkyMaterial shader (LIGHT0..3 blocks).
    #[test]
    fn procedural_sky_renders_sun_disc_and_halo() {
        let mat = ProceduralSkyMaterial::default();
        let sun = SkySun {
            direction: Vector3::new(0.0, 1.0, 0.0),
            color: Color::WHITE,
            energy: 1.0,
            size: 0.0,
        };

        // Looking straight at the sun: c2 = 0 → full sun color.
        let center = sample_procedural_sky(&mat, &Vector3::new(0.0, 1.0, 0.0), 1.0, &[sun]);
        assert!(
            center.r > 0.99 && center.g > 0.99 && center.b > 0.99,
            "sun center must be the light color, got {:?}",
            center
        );

        // Same direction without the sun: the plain zenith gradient.
        let no_sun = sample_procedural_sky(&mat, &Vector3::new(0.0, 1.0, 0.0), 1.0, &[]);
        assert!(
            no_sun.r < 0.5,
            "zenith without a sun must stay the gradient color, got {:?}",
            no_sun
        );

        // 2° off-center (inside sun_angle_max=30°, where the tight default
        // falloff pow(1-c2, 22.78) still contributes ~90%): partial halo —
        // brighter than the gradient, dimmer than the disc.
        let off = Vector3::new(2f32.to_radians().sin(), 2f32.to_radians().cos(), 0.0);
        let halo = sample_procedural_sky(&mat, &off, 1.0, &[sun]);
        let base = sample_procedural_sky(&mat, &off, 1.0, &[]);
        assert!(
            halo.r > base.r + 0.01 && halo.r < 1.0,
            "halo must blend toward the sun color: halo r={:.3}, base r={:.3}",
            halo.r,
            base.r
        );

        // 45° off-center (outside sun_angle_max): no contribution.
        let far = Vector3::new(45f32.to_radians().sin(), 45f32.to_radians().cos(), 0.0);
        let outside = sample_procedural_sky(&mat, &far, 1.0, &[sun]);
        let outside_base = sample_procedural_sky(&mat, &far, 1.0, &[]);
        assert!(
            (outside.r - outside_base.r).abs() < 1e-6,
            "outside sun_angle_max the sky must be untouched"
        );

        // The ground hemisphere never shows the sun (Godot mixes the sun into
        // the sky term only).
        let below = sample_procedural_sky(
            &mat,
            &Vector3::new(0.0, -1.0, 0.0),
            1.0,
            &[SkySun { direction: Vector3::new(0.0, -1.0, 0.0), ..sun }],
        );
        assert!(
            below.r < 0.3,
            "sun must not render below the horizon, got {:?}",
            below
        );
    }

    /// `sky_suns_from_lights` mirrors Godot's sky light collection: scene
    /// order, directional + sky-visible only, capped at 4, direction negated
    /// to point toward the sun.
    #[test]
    fn sky_suns_collected_like_godot() {
        use gdserver3d::light::{Light3D, Light3DId};

        let mut dir_light = Light3D::directional(Light3DId(1));
        dir_light.direction = Vector3::new(0.0, -1.0, 0.0);
        dir_light.angular_distance = 0.5;

        let mut no_sky = Light3D::directional(Light3DId(2));
        no_sky.sky = false;

        let point = Light3D::point(Light3DId(3), Vector3::ZERO);

        let suns = sky_suns_from_lights(&[dir_light, no_sky, point]);
        assert_eq!(suns.len(), 1, "only sky-visible directional lights count");
        assert!((suns[0].direction.y - 1.0).abs() < 1e-6, "direction points toward the sun");
        assert!((suns[0].size - 0.5f32.to_radians()).abs() < 1e-6);

        let many: Vec<_> = (0..6)
            .map(|i| Light3D::directional(Light3DId(10 + i)))
            .collect();
        assert_eq!(sky_suns_from_lights(&many).len(), 4, "capped at 4 like Godot");
    }

    /// End-to-end: a directional light aimed at the camera puts a sun disc in
    /// the rendered sky background.
    #[test]
    fn rendered_frame_contains_sun_from_directional_light() {
        use gdserver3d::light::{Light3D, Light3DId};

        let mut renderer = SoftwareRenderer3D::new();
        let env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(gdserver3d::sky::Sky::default()),
            ..Default::default()
        };
        // Default camera looks down -Z; a light travelling +Z and slightly
        // down has its sun just above the horizon at -Z, in the upper half of
        // the viewport (the exact center ray sits a hair below the horizon,
        // where Godot never draws the sun).
        let lid = Light3DId(1);
        renderer.add_light(lid);
        let mut light = Light3D::directional(lid);
        light.direction = Vector3::new(0.0, -0.15, 1.0).normalized();
        renderer.update_light(&light);

        let brightest = |pixels: &[Color]| {
            pixels
                .iter()
                .map(|c| c.r.min(c.g).min(c.b))
                .fold(0.0f32, f32::max)
        };

        let vp = viewport_with_env(env.clone());
        let frame = renderer.render_frame(&vp);
        assert!(
            brightest(&frame.pixels) > 0.9,
            "the sky must contain a near-white sun disc, brightest={:.3}",
            brightest(&frame.pixels)
        );

        // Without the light the sky is the plain gradient — no white pixel.
        let mut bare = SoftwareRenderer3D::new();
        let frame_bare = bare.render_frame(&viewport_with_env(env));
        assert!(
            brightest(&frame_bare.pixels) < 0.8,
            "without a directional light there must be no sun, brightest={:.3}",
            brightest(&frame_bare.pixels)
        );
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

    /// `environment_ambient_color` derives a sky-tinted ambient (linear) from
    /// the environment, replacing the old flat grey — matching Godot's
    /// AmbientSource modes.
    #[test]
    fn ambient_color_from_environment() {
        use gdcore::math::srgb_to_linear_channel as s2l;
        use gdserver3d::environment::AmbientSource;

        // Sky source: soft dome average of the procedural sky, × energy. The
        // default sky is bluish, so ambient is non-zero and blue-biased.
        let mut env = Environment3D {
            background_mode: BackgroundMode::Sky,
            sky: Some(gdserver3d::sky::Sky::default()),
            ambient_source: AmbientSource::Sky,
            ambient_energy: 0.5,
            ..Default::default()
        };
        let amb = environment_ambient_color(&env);
        assert!(amb.r > 0.0 && amb.b > amb.r, "sky ambient must be soft and blue-biased: {amb:?}");
        // Halving energy halves the ambient.
        env.ambient_energy = 0.25;
        let amb_half = environment_ambient_color(&env);
        assert!((amb_half.b - amb.b * 0.5).abs() < 1e-4, "ambient scales with energy");

        // Color source: flat ambient_color (sRGB→linear) × energy.
        let env_c = Environment3D {
            ambient_source: AmbientSource::Color,
            ambient_color: Color::new(0.5, 0.25, 0.1, 1.0),
            ambient_energy: 1.0,
            ..Default::default()
        };
        let amb_c = environment_ambient_color(&env_c);
        assert!((amb_c.r - s2l(0.5)).abs() < 1e-4 && (amb_c.g - s2l(0.25)).abs() < 1e-4);

        // Disabled: black.
        let env_d = Environment3D { ambient_source: AmbientSource::Disabled, ..Default::default() };
        assert_eq!(environment_ambient_color(&env_d), Color::BLACK);
    }

    /// `fog_process` (the pure Godot fog math) — exponential falloff, height
    /// fog, sun scatter, aerial perspective, and linear fog color.
    #[test]
    fn fog_process_math_matches_godot() {
        use gdcore::math::srgb_to_linear_channel as s2l;
        let mut env = Environment3D {
            fog_enabled: true,
            fog_density: 0.1,
            ..Default::default()
        };
        let up = Vector3::new(0.0, 1.0, 0.0);

        // Exponential fog grows with distance: 1 - exp(-dist*density).
        let (color, near) = fog_process(&env, 1.0, 0.0, &up, &[], None);
        let (_, far) = fog_process(&env, 20.0, 0.0, &up, &[], None);
        assert!((near - (1.0 - (-0.1f32).exp())).abs() < 1e-5, "near amount {near}");
        assert!(far > near, "fog must deepen with distance: {near} -> {far}");
        // Fog color is the sRGB fog_light_color in linear (default 0.518,...).
        assert!((color.r - s2l(0.518)).abs() < 1e-4, "fog color must be linear");

        // Height fog: below fog_height with positive density it dominates.
        env.fog_height = 0.0;
        env.fog_height_density = 1.0;
        let (_, low) = fog_process(&env, 1.0, -5.0, &up, &[], None); // well below
        let (_, high) = fog_process(&env, 1.0, 5.0, &up, &[], None); // above
        assert!(low > high, "height fog must be denser below fog_height: {low} vs {high}");

        // Sun scatter adds the sun color when looking toward the sun.
        env.fog_height_density = 0.0;
        env.fog_sun_scatter = 1.0;
        let sun = SkySun {
            direction: Vector3::new(0.0, 0.0, -1.0),
            color: Color::WHITE,
            energy: 1.0,
            size: 0.0,
        };
        let toward = Vector3::new(0.0, 0.0, -1.0); // straight at the sun
        let away = Vector3::new(0.0, 0.0, 1.0);
        let (c_toward, _) = fog_process(&env, 1.0, 0.0, &toward, &[sun], None);
        let (c_away, _) = fog_process(&env, 1.0, 0.0, &away, &[sun], None);
        assert!(c_toward.r > c_away.r + 0.3, "sun scatter must brighten fog toward the sun");

        // Aerial perspective blends fog toward the sampled sky color.
        env.fog_sun_scatter = 0.0;
        env.fog_aerial_perspective = 1.0;
        let sky = Color::new(0.05, 0.1, 0.9, 1.0); // vivid blue sky
        let (c_aerial, _) = fog_process(&env, 1.0, 0.0, &up, &[], Some(sky));
        assert!((c_aerial.b - 0.9).abs() < 1e-4, "aerial=1 → fog color becomes the sky color");
    }

    #[test]
    fn fog_disabled_no_effect() {
        // Both frames carry an environment (attaching one changes the ambient
        // source from the bare-renderer 0.1 grey to the environment, so a
        // no-env frame is not a valid baseline); they must differ ONLY in the
        // fog settings, which — being disabled — must not change a pixel.
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
        // Light the cube so the frames aren't trivially black.
        let mut light = Light3D::directional(Light3DId(1));
        light.direction = Vector3::new(0.0, 0.0, -1.0);
        renderer.add_light(light.id);
        renderer.update_light(&light);

        let vp_base = viewport_with_env(Environment3D::default());
        let frame_base = renderer.render_frame(&vp_base);

        let env = Environment3D {
            fog_enabled: false,
            fog_density: 1.0,
            fog_light_color: Color::new(1.0, 0.0, 0.0, 1.0),
            ..Default::default()
        };
        let vp_env = viewport_with_env(env);
        let frame_env = renderer.render_frame(&vp_env);

        let avg_base = avg_color(&frame_base.pixels);
        let avg_env = avg_color(&frame_env.pixels);
        assert!(
            avg_base.r > 0.01,
            "baseline must render a lit cube, got avg r={:.4}",
            avg_base.r
        );
        assert!(
            (avg_base.r - avg_env.r).abs() < 1e-5 && (avg_base.g - avg_env.g).abs() < 1e-5,
            "Fog disabled should not change rendering: base=({:.4},{:.4}) env=({:.4},{:.4})",
            avg_base.r,
            avg_base.g,
            avg_env.r,
            avg_env.g
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
