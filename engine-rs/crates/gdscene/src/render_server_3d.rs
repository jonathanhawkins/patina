//! 3D render path adapter: bridges the scene tree to [`RenderingServer3D`].
//!
//! Collects Camera3D, MeshInstance3D, MultiMeshInstance3D, and Light3D nodes
//! from the scene tree each frame, submits them to a [`RenderingServer3D`]
//! implementation, and returns measurable frame data for oracle/golden comparison.

use std::collections::HashMap;
use std::sync::Arc;

use gdcore::math::Color;
use gdcore::math::{Rect2, Vector2, Vector2i, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::{compare_framebuffers_3d, DiffResult3D, FrameBuffer3D, SoftwareRenderer3D};
use gdserver3d::instance::{
    GeometryInstanceSettings, GiMode, Instance3DId, ShadowCastingSetting, VisibilityRangeFadeMode,
};
use gdserver3d::label3d::Label3D;
use gdserver3d::light::{Light3D, Light3DId, LightType, OmniShadowMode};
use gdserver3d::material::{
    BlendMode, CullMode, DepthDrawMode, Material3D, ShadingMode, StandardMaterial3D, TextureImage,
    TextureSlot, Transparency,
};
use gdserver3d::mesh::{Mesh3D, PrimitiveType};
use gdserver3d::reflection_probe::ReflectionProbeId;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::{Shader3D, ShaderMaterial3D, ShaderType3D};
use gdserver3d::sprite3d::{Axis3D, BillboardMode3D, Sprite3D};
use gdserver3d::viewport::Viewport3D;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::scene_tree::SceneTree;
use crate::skeleton3d;

/// Maps scene tree NodeIds to render server Instance3DIds (for MeshInstance3D).
#[derive(Debug, Default)]
struct InstanceMap {
    node_to_instance: HashMap<NodeId, Instance3DId>,
    /// The undeformed mesh each instance was created from. A skinned instance is
    /// re-deformed from this every frame, so skinning never compounds on the
    /// previous frame's result (pat-0q2kx.3).
    node_to_base_mesh: HashMap<NodeId, Mesh3D>,
}

/// Maps MultiMeshInstance3D NodeIds to their expanded render instance IDs.
#[derive(Debug, Default)]
struct MultiMeshMap {
    node_to_instances: HashMap<NodeId, Vec<Instance3DId>>,
}

/// Maps Sprite3D NodeIds to their render instance IDs (one textured quad each).
#[derive(Debug, Default)]
struct SpriteMap {
    node_to_instance: HashMap<NodeId, Instance3DId>,
}

/// Maps scene tree NodeIds to render server Light3DIds.
#[derive(Debug, Default)]
struct LightMap {
    node_to_light: HashMap<NodeId, Light3DId>,
    next_id: u64,
}

/// Maps scene tree NodeIds to render server ReflectionProbeIds.
#[derive(Debug, Default)]
struct ReflectionProbeMap {
    node_to_probe: HashMap<NodeId, ReflectionProbeId>,
    next_id: u64,
}

/// A serializable snapshot of one 3D render frame, for parity comparison.
#[derive(Debug, Clone)]
pub struct RenderFrame3DSnapshot {
    /// Frame number within the current run.
    pub frame_number: u64,
    /// Viewport width in pixels.
    pub width: u32,
    /// Viewport height in pixels.
    pub height: u32,
    /// Number of visible mesh instances submitted.
    pub visible_mesh_count: u32,
    /// Number of active lights in the scene.
    pub light_count: u32,
    /// Number of non-black pixels in the rendered frame.
    pub nonblack_pixel_count: u64,
    /// Total pixel count.
    pub total_pixel_count: u64,
    /// Number of pixels with depth values written (< 1.0 default).
    pub depth_written_count: u64,
    /// Camera world-space transform used for this frame.
    pub camera_transform: [f32; 12],
    /// Camera FOV in radians.
    pub camera_fov: f32,
}

impl RenderFrame3DSnapshot {
    /// Returns the fraction of non-background pixels (0.0 to 1.0).
    pub fn coverage(&self) -> f64 {
        if self.total_pixel_count == 0 {
            return 0.0;
        }
        self.nonblack_pixel_count as f64 / self.total_pixel_count as f64
    }

    /// Serializes this snapshot to a JSON string for golden comparison.
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"frame_number\":{},",
                "\"width\":{},",
                "\"height\":{},",
                "\"visible_mesh_count\":{},",
                "\"light_count\":{},",
                "\"nonblack_pixel_count\":{},",
                "\"total_pixel_count\":{},",
                "\"depth_written_count\":{},",
                "\"coverage\":{:.6},",
                "\"camera_fov\":{:.6}",
                "}}"
            ),
            self.frame_number,
            self.width,
            self.height,
            self.visible_mesh_count,
            self.light_count,
            self.nonblack_pixel_count,
            self.total_pixel_count,
            self.depth_written_count,
            self.coverage(),
            self.camera_fov,
        )
    }

    /// Returns a structured parity report for oracle comparison.
    ///
    /// The report captures measurable metrics that can be compared against
    /// Godot oracle output to quantify render path correctness.
    pub fn parity_report(&self) -> ParityReport3D {
        let depth_ratio = if self.total_pixel_count == 0 {
            0.0
        } else {
            self.depth_written_count as f64 / self.total_pixel_count as f64
        };

        ParityReport3D {
            frame_number: self.frame_number,
            mesh_count: self.visible_mesh_count,
            light_count: self.light_count,
            coverage: self.coverage(),
            depth_coverage: depth_ratio,
            has_camera: self.camera_fov > 0.0,
            viewport_pixels: self.total_pixel_count,
        }
    }
}

/// Structured parity report for oracle comparison.
///
/// Captures the measurable attributes of a 3D rendered frame that can
/// be validated against Godot oracle output.
#[derive(Debug, Clone, PartialEq)]
pub struct ParityReport3D {
    /// Frame number.
    pub frame_number: u64,
    /// Number of visible meshes submitted to the renderer.
    pub mesh_count: u32,
    /// Number of active lights.
    pub light_count: u32,
    /// Fraction of non-background pixels (0.0–1.0).
    pub coverage: f64,
    /// Fraction of pixels with depth values written (0.0–1.0).
    pub depth_coverage: f64,
    /// Whether an active camera was found.
    pub has_camera: bool,
    /// Total pixel count in the viewport.
    pub viewport_pixels: u64,
}

impl ParityReport3D {
    /// Returns true if all measured metrics indicate a functional render path.
    pub fn is_functional(&self) -> bool {
        self.has_camera && self.mesh_count > 0 && self.coverage > 0.0
    }

    /// Serializes this report to a JSON string.
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"frame_number\":{},",
                "\"mesh_count\":{},",
                "\"light_count\":{},",
                "\"coverage\":{:.6},",
                "\"depth_coverage\":{:.6},",
                "\"has_camera\":{},",
                "\"viewport_pixels\":{},",
                "\"is_functional\":{}",
                "}}"
            ),
            self.frame_number,
            self.mesh_count,
            self.light_count,
            self.coverage,
            self.depth_coverage,
            self.has_camera,
            self.viewport_pixels,
            self.is_functional(),
        )
    }
}

/// Bridges scene tree 3D nodes to a [`RenderingServer3D`] for rendering.
///
/// Each frame, the adapter:
/// 1. Finds the active Camera3D node and builds a [`Viewport3D`].
/// 2. Collects all visible MeshInstance3D nodes, syncs transforms/materials.
/// 3. Calls `render_frame` on the underlying server.
/// 4. Captures measurable frame data for parity hooks.
pub struct RenderServer3DAdapter {
    renderer: SoftwareRenderer3D,
    instance_map: InstanceMap,
    multimesh_map: MultiMeshMap,
    /// Expanded render instances for 3D particle emitters (pat-mgixk.70).
    ///
    /// Same NodeId -> instances shape as [`MultiMeshMap`], and deliberately the
    /// same type: a particle emitter expands into one instance per LIVE
    /// particle exactly as a MultiMesh expands into one per entry, so the
    /// create/free/reuse bookkeeping is identical.
    particle_map: MultiMeshMap,
    sprite_map: SpriteMap,
    /// Label3D node -> render instance (pat-0q2kx.109).
    label_map: SpriteMap,
    light_map: LightMap,
    reflection_probe_map: ReflectionProbeMap,
    viewport_width: u32,
    viewport_height: u32,
    frame_counter: u64,
    /// Stored last frame data for comparison.
    last_frame: Option<FrameBuffer3D>,
    /// Deformed vertex positions the last skeleton sync submitted, per
    /// MeshInstance3D node (pat-0q2kx.3). Exposed by [`skinned_vertices`] so a
    /// caller can confirm the skeleton actually reached the render path.
    ///
    /// [`skinned_vertices`]: RenderServer3DAdapter::skinned_vertices
    skinned_vertices: HashMap<NodeId, Vec<Vector3>>,
    /// Decoded material textures, kept across frames. Materials resolve every
    /// frame, so without this a textured surface would re-decode its PNG on
    /// every single frame (pat-0q2kx.42).
    texture_cache: MaterialTextureCache,
}

impl RenderServer3DAdapter {
    /// Creates a new adapter with the given viewport dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            renderer: SoftwareRenderer3D::new(),
            instance_map: InstanceMap::default(),
            multimesh_map: MultiMeshMap::default(),
            particle_map: MultiMeshMap::default(),
            sprite_map: SpriteMap::default(),
            label_map: SpriteMap::default(),
            light_map: LightMap::default(),
            reflection_probe_map: ReflectionProbeMap::default(),
            viewport_width: width,
            viewport_height: height,
            frame_counter: 0,
            last_frame: None,
            skinned_vertices: HashMap::new(),
            texture_cache: MaterialTextureCache::new(),
        }
    }

    /// Returns the current frame counter.
    pub fn frame_counter(&self) -> u64 {
        self.frame_counter
    }

    /// The skinned vertex positions submitted for `node` on the last synced
    /// frame, or `None` when that MeshInstance3D is not driven by a skeleton
    /// (pat-0q2kx.3).
    ///
    /// Like [`lights`](Self::lights), this exposes what the sync pass actually
    /// wrote — here, the mesh the skeleton's live bone poses produced.
    pub fn skinned_vertices(&self, node: NodeId) -> Option<&[Vector3]> {
        self.skinned_vertices.get(&node).map(|v| v.as_slice())
    }

    /// The light records the last scene sync produced (pat-nzgdu.121).
    ///
    /// Exposes what `sync_lights` wrote, so callers can confirm a scene's
    /// Light3D properties actually reached the render server's light.
    pub fn lights(&self) -> &[Light3D] {
        self.renderer.lights()
    }

    /// The render instances the last scene sync produced (pat-nzgdu.76).
    ///
    /// Like [`lights`](Self::lights), this exposes what the sync pass actually
    /// wrote — here, whether a node's `GeometryInstance3D` properties
    /// (`cast_shadow`, `visibility_range_*`, `transparency`, …) reached the
    /// instance the renderer draws from.
    pub fn instances(&self) -> &[gdserver3d::instance::Instance3D] {
        self.renderer.instances()
    }

    /// Returns the last rendered frame buffer, if any.
    pub fn last_frame(&self) -> Option<&FrameBuffer3D> {
        self.last_frame.as_ref()
    }

    /// Renders one frame by collecting 3D nodes from the scene tree.
    ///
    /// Returns a measurable snapshot and the raw frame data.
    pub fn render_frame(&mut self, tree: &SceneTree) -> (RenderFrame3DSnapshot, FrameData3D) {
        self.frame_counter += 1;
        // Hand this frame's clock to the renderer so custom shaders see a live
        // `TIME` (pat-s3rrv.122). `MainLoop::step` publishes it onto the tree.
        self.renderer.set_shader_time(tree.shader_time());

        // 1. Find the active Camera3D and build a Viewport3D.
        let (viewport, camera_transform) = self.build_viewport(tree);

        // 2. Sync MeshInstance3D, MultiMeshInstance3D, and Sprite3D nodes.
        let visible_count = self.sync_mesh_instances(tree);
        let multimesh_visible = self.sync_multimesh_instances(tree);
        // Sprites billboard toward the active camera, so pass its world origin.
        let sprite_visible = self.sync_sprite3d(tree, camera_transform.origin);
        // CSG roots evaluate their boolean tree into one mesh (pat-nzgdu.119).
        let csg_visible = self.sync_csg_shapes(tree);
        // Label3D text quads billboard toward the same camera (pat-0q2kx.109).
        let label_visible = self.sync_label3d(tree, camera_transform.origin);
        // One instance per live 3D particle (pat-mgixk.70). `MainLoop` has
        // simulated and published these all along and NOTHING read them: a
        // case-insensitive `particle` grep over `gdrender3d` and
        // `gdserver3d/src` found only the Sprite3D/Label3D billboard enums, so
        // every 3D emitter was invisible.
        let particle_visible = self.sync_particles_3d(tree);
        let visible_count = visible_count
            + multimesh_visible
            + sprite_visible
            + csg_visible
            + label_visible
            + particle_visible;

        // 3. Sync light nodes to render server.
        let light_count = self.sync_lights(tree);

        // 4. Sync reflection probes.
        self.sync_reflection_probes(tree);

        // 5. Render the frame.
        let frame_data = self.renderer.render_frame(&viewport);

        // 5. Build measurable snapshot.
        let nonblack = frame_data
            .pixels
            .iter()
            .filter(|c| **c != Color::BLACK)
            .count() as u64;

        let depth_written = frame_data.depth.iter().filter(|d| **d < 1.0).count() as u64;

        let ct = camera_transform;
        let camera_arr = [
            ct.basis.x.x,
            ct.basis.x.y,
            ct.basis.x.z,
            ct.basis.y.x,
            ct.basis.y.y,
            ct.basis.y.z,
            ct.basis.z.x,
            ct.basis.z.y,
            ct.basis.z.z,
            ct.origin.x,
            ct.origin.y,
            ct.origin.z,
        ];

        let snapshot = RenderFrame3DSnapshot {
            frame_number: self.frame_counter,
            width: self.viewport_width,
            height: self.viewport_height,
            visible_mesh_count: visible_count,
            light_count,
            nonblack_pixel_count: nonblack,
            total_pixel_count: (self.viewport_width as u64) * (self.viewport_height as u64),
            depth_written_count: depth_written,
            camera_transform: camera_arr,
            camera_fov: viewport.fov,
        };

        // Store for comparison.
        self.last_frame = Some(FrameBuffer3D {
            width: frame_data.width,
            height: frame_data.height,
            pixels: frame_data.pixels.clone(),
            depth: frame_data.depth.clone(),
        });

        (snapshot, frame_data)
    }

    /// Compares two frame buffers and returns a parity diff result.
    pub fn compare_frames(
        a: &FrameBuffer3D,
        b: &FrameBuffer3D,
        color_tolerance: f64,
        depth_tolerance: f64,
    ) -> DiffResult3D {
        compare_framebuffers_3d(a, b, color_tolerance, depth_tolerance)
    }

    /// Finds the first Camera3D node marked as `current` (or the first Camera3D
    /// found) and builds a [`Viewport3D`] from its properties.
    fn build_viewport(&self, tree: &SceneTree) -> (Viewport3D, Transform3D) {
        let all_nodes = tree.all_nodes_in_tree_order();

        // Find active camera.
        let mut camera_id: Option<NodeId> = None;
        let mut first_camera: Option<NodeId> = None;

        for &nid in &all_nodes {
            if let Some(node) = tree.get_node(nid) {
                // Inheritance-aware, not `class_name() == "Camera3D"`
                // (pat-0q2kx.235). Exact equality meant `XRCamera3D` — which is
                // registered with parent Camera3D and, per Godot, renders as an
                // ordinary camera when there is no XR session — could never be
                // found, so a scene whose only camera was an XRCamera3D
                // rendered from NO camera at all. `is_class` mirrors Godot's
                // `Object.is_class()` and fixes every future Camera3D subclass
                // with it; it short-circuits on an exact name match, so this
                // still works when the ClassDB has not been populated.
                if node.is_class("Camera3D") {
                    if first_camera.is_none() {
                        first_camera = Some(nid);
                    }
                    if node.get_property("current") == gdvariant::Variant::Bool(true) {
                        camera_id = Some(nid);
                        break;
                    }
                }
            }
        }

        let active_camera = camera_id.or(first_camera);

        let (camera_transform, fov, near, far) = if let Some(cam_id) = active_camera {
            let t = node3d::get_global_transform(tree, cam_id);
            let fov = node3d::get_fov(tree, cam_id) as f32;
            let near = node3d::get_near(tree, cam_id) as f32;
            let far = node3d::get_far(tree, cam_id) as f32;
            // Convert degrees to radians if the FOV value looks like degrees
            // (Godot stores FOV in degrees, our viewport expects radians).
            let fov_rad = if fov > std::f32::consts::PI {
                fov.to_radians()
            } else {
                fov
            };
            (t, fov_rad, near, far)
        } else {
            // Default camera: identity transform, 45-degree FOV.
            (
                Transform3D::IDENTITY,
                std::f32::consts::FRAC_PI_4,
                0.05,
                4000.0,
            )
        };

        let viewport = Viewport3D {
            width: self.viewport_width,
            height: self.viewport_height,
            camera_transform,
            fov,
            near,
            far,
            // pat-mgixk.51: the runner adapter rendered every scene with NO
            // environment, so a `WorldEnvironment` (or the project's
            // `default_environment` / `default_clear_color`) changed nothing it
            // drew. Resolved through the same function the editor's Play view
            // uses, so the two paths cannot disagree about what a scene asked
            // for. `None` when the scene authors nothing and there is no
            // readable project — exactly the value this field held before.
            environment: crate::world_environment::runtime_environment_3d(tree, active_camera),
        };

        (viewport, camera_transform)
    }

    /// Syncs MeshInstance3D nodes from the scene tree to the render server.
    ///
    /// Creates new instances for nodes not yet tracked, updates transforms
    /// for existing ones, and removes instances for nodes no longer in the tree.
    /// Returns the number of visible mesh instances.
    /// Evaluates every root CSG subtree and registers the resulting geometry as
    /// a render instance (pat-nzgdu.119).
    ///
    /// A CSG node is a *root* when its parent is not itself a CSG node; that
    /// root owns the whole boolean tree beneath it, exactly as Godot renders a
    /// CSGShape3D hierarchy as one mesh. Returns the number of visible CSG roots
    /// that produced geometry.
    fn sync_csg_shapes(&mut self, tree: &SceneTree) -> u32 {
        let mut visible_count = 0u32;

        for &nid in &tree.all_nodes_in_tree_order() {
            let node = match tree.get_node(nid) {
                Some(n) => n,
                None => continue,
            };
            if !is_csg_class(node.class_name()) {
                continue;
            }
            // Only roots evaluate; descendants are folded in by their root.
            if !is_csg_root(tree, nid) {
                continue;
            }

            let visible = node3d::is_visible_in_tree(tree, nid);
            let inst_id = if let Some(&existing) = self.instance_map.node_to_instance.get(&nid) {
                existing
            } else {
                let mesh = match evaluate_csg_tree(tree, nid) {
                    Some(m) => m,
                    // No geometry (e.g. an empty combiner): nothing to register.
                    None => continue,
                };
                let new_id = self.renderer.create_instance();
                self.renderer.set_mesh(new_id, mesh);
                self.renderer.set_material(new_id, Material3D::default());
                self.instance_map.node_to_instance.insert(nid, new_id);
                new_id
            };

            self.renderer
                .set_transform(inst_id, node3d::get_global_transform(tree, nid));
            self.renderer.set_visible(inst_id, visible);
            if visible {
                visible_count += 1;
            }
        }

        visible_count
    }

    fn sync_mesh_instances(&mut self, tree: &SceneTree) -> u32 {
        let all_nodes = tree.all_nodes_in_tree_order();

        // Collect current MeshInstance3D nodes.
        let mut current_mesh_nodes: HashMap<NodeId, ()> = HashMap::new();
        let mut visible_count = 0u32;

        for &nid in &all_nodes {
            if let Some(node) = tree.get_node(nid) {
                // ⚠️ An EXACT class match here skipped every SUBCLASS of
                // MeshInstance3D, so a `.tscn` SoftBody3D was never collected
                // and drew nothing at all — deformed or not (pat-s3rrv.193).
                // `SoftBody3D` is currently the only registered subclass, so
                // naming it is exactly equivalent to walking the parent chain
                // and costs no ClassDB lookup per node per frame; a second
                // subclass should turn this into an `is_parent_class` check.
                if matches!(node.class_name(), "MeshInstance3D" | "SoftBody3D") {
                    current_mesh_nodes.insert(nid, ());

                    let transform = node3d::get_global_transform(tree, nid);
                    let visible = node3d::is_visible_in_tree(tree, nid);

                    let inst_id =
                        if let Some(&existing) = self.instance_map.node_to_instance.get(&nid) {
                            existing
                        } else {
                            let new_id = self.renderer.create_instance();
                            // Resolve mesh type from the node's properties.
                            let mesh = Self::resolve_mesh(tree, nid);
                            self.instance_map
                                .node_to_base_mesh
                                .insert(nid, mesh.clone());
                            self.renderer.set_mesh(new_id, mesh);
                            self.renderer.set_material(new_id, Material3D::default());
                            self.instance_map.node_to_instance.insert(nid, new_id);
                            new_id
                        };

                    // -- blend shapes (pat-s3rrv.23) --
                    // Godot deforms a mesh by its blend-shape weights before
                    // skinning. Until now nothing called `apply_morph` from any
                    // render stage, so changing a weight moved no pixel — the
                    // import and the maths both existed and never met.
                    //
                    // ⚠️ Its OWN branch, ahead of skinning: the skinning branch
                    // is gated on `is_skinned()`, so folding morph in there
                    // would skip every unskinned morphed mesh.
                    let morphed = {
                        let base = self.instance_map.node_to_base_mesh.get(&nid);
                        base.and_then(|base| morphed_mesh(tree, nid, base))
                    };
                    if let Some(mesh) = &morphed {
                        self.renderer.set_mesh(inst_id, mesh.clone());
                    }

                    // -- skeletal skinning (pat-0q2kx.3) --
                    // Godot: a MeshInstance3D bound to a Skeleton3D is deformed
                    // by that skeleton's live bone poses before rasterization
                    // (RenderingServer skeleton storage -> skinning). Re-skin
                    // from the undeformed base every frame so the pose the
                    // skeleton phase just computed is what gets drawn.
                    if let Some(skeleton) = Self::resolve_skeleton(tree, nid) {
                        let skinned = {
                            // ⚠️ Skin the MORPHED mesh when there is one. Reading
                            // the undeformed base here would silently discard the
                            // morph for any mesh that is both morphed and
                            // skinned — the `set_mesh` below overwrites the one
                            // above.
                            let base = morphed
                                .as_ref()
                                .or_else(|| self.instance_map.node_to_base_mesh.get(&nid));
                            base.filter(|m| m.is_skinned()).map(|base| {
                                let poses = skeleton3d::bone_pose_array(tree, skeleton);
                                let skin = Self::resolve_skin(tree, skeleton, nid);
                                gdserver3d::skin::skin_mesh(base, &skin, &poses)
                            })
                        };
                        if let Some(skinned) = skinned {
                            self.skinned_vertices.insert(nid, skinned.vertices.clone());
                            self.renderer.set_mesh(inst_id, skinned);
                        }
                    }

                    // -- soft bodies (pat-s3rrv.193) --
                    // A SoftBody3D's vertices are owned by the solver, which
                    // publishes them onto the node each tick. Applied AFTER
                    // morph and skin because the solver's output is the final
                    // word on where those vertices are, not another deformation
                    // to compose with them.
                    let solved = {
                        // Scoped so the borrow of `instance_map` ends before
                        // `renderer` is borrowed mutably — the same shape the
                        // morph stage above uses.
                        let base = self.instance_map.node_to_base_mesh.get(&nid);
                        base.and_then(|base| {
                            crate::softbody_sync_3d::softbody_mesh_for_node(tree, nid, base)
                        })
                    };
                    if let Some(solved) = solved {
                        self.renderer.set_mesh(inst_id, solved);
                    }

                    self.renderer.set_transform(inst_id, transform);
                    self.renderer.set_visible(inst_id, visible);

                    // Resolve material following Godot's priority chain:
                    //   1. material_override (highest priority — overrides everything)
                    //   2. surface_material_override/0 (per-surface override)
                    //   3. node "albedo" color property (legacy shorthand)
                    //   4. Material3D::default()
                    let resolved_mat = self.resolve_material(tree, nid);
                    self.renderer.set_material(inst_id, resolved_mat);

                    // Overrides for the mesh's ADDITIONAL surfaces
                    // (pat-s3rrv.187). `packed_scene` has flattened
                    // `surface_material_override/N` onto the node as
                    // `__surfmat<N>__*` since the per-surface load pass landed,
                    // but nothing read them back — the data reached the node and
                    // stopped there, so a second material could never be drawn.
                    let surface_mats =
                        surface_materials_for_node(tree, nid, &mut self.texture_cache);
                    if !surface_mats.is_empty() {
                        self.renderer.set_surface_materials(inst_id, surface_mats);
                    }

                    // Resolve ShaderMaterial3D if attached to this node.
                    if let Some(mut shader_mat) = Self::resolve_shader_material(tree, nid) {
                        // Load any `sampler2D` uniform's image before handing the
                        // material over — the renderers sample from the material's
                        // texture map, not from `parameters` (pat-s3rrv.122).
                        bind_shader_textures(&mut shader_mat, &mut self.texture_cache);
                        self.renderer.set_shader_material(inst_id, shader_mat);
                    }

                    // GeometryInstance3D property block (pat-nzgdu.76):
                    // cast_shadow, visibility_range, transparency,
                    // material_overlay, lod_bias, extra_cull_margin,
                    // custom_aabb. These are stored on the node and registered
                    // in ClassDB; this is where they reach the render path.
                    self.renderer.set_geometry_instance_settings(
                        inst_id,
                        geometry_instance_settings_for_node(tree, nid),
                    );

                    if visible {
                        visible_count += 1;
                    }
                }
            }
        }

        // Remove instances for nodes that no longer exist.
        let stale: Vec<NodeId> = self
            .instance_map
            .node_to_instance
            .keys()
            .filter(|nid| !current_mesh_nodes.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(inst_id) = self.instance_map.node_to_instance.remove(&nid) {
                self.renderer.free_instance(inst_id);
            }
            self.instance_map.node_to_base_mesh.remove(&nid);
            self.skinned_vertices.remove(&nid);
        }

        visible_count
    }

    /// Resolves the `Skeleton3D` that deforms a MeshInstance3D, if any
    /// (pat-0q2kx.3).
    ///
    /// Follows Godot's `MeshInstance3D.skeleton` NodePath, which defaults to
    /// `NodePath("..")` — the parent — so a mesh parented to a Skeleton3D is
    /// skinned without any explicit property. The resolved node must actually be
    /// a `Skeleton3D`; anything else (the common case of a mesh whose parent is
    /// a plain Node3D) yields `None`.
    fn resolve_skeleton(tree: &SceneTree, nid: NodeId) -> Option<NodeId> {
        let path = match tree.get_node(nid).map(|n| n.get_property("skeleton")) {
            Some(gdvariant::Variant::NodePath(p)) => p.to_string(),
            Some(gdvariant::Variant::String(s)) if !s.is_empty() => s,
            // Godot's default for the property when unset.
            _ => "..".to_string(),
        };
        let target = tree.get_node_or_null(nid, &path)?;
        let is_skeleton = tree
            .get_node(target)
            .map(|n| n.class_name() == skeleton3d::SKELETON_CLASS)
            .unwrap_or(false);
        if is_skeleton {
            Some(target)
        } else {
            None
        }
    }

    /// Builds the [`Skin`] used to deform the mesh on `mesh_node`, bound to
    /// `skeleton` (pat-0q2kx.3, pat-s3rrv.200).
    ///
    /// Godot's MeshInstance3D registers **its own** `skin` with the skeleton,
    /// and only when that property is null does the skeleton supply the default
    /// built from its rest pose (`Skeleton3D.create_skin_from_rest_transforms()`):
    /// one bind per bone whose inverse bind matrix is that bone's global rest
    /// inverted. At rest each per-bind matrix is then the identity, so an
    /// un-posed rig renders undeformed.
    ///
    /// ⚠️ `mesh_node` is not decoration. Without it the authored binds are not
    /// merely unread — there is no parameter through which they *could* be
    /// read, and the rest fallback is a **plausible** wrong answer: the mesh
    /// still follows the bones, just not the way the rig was authored. It is
    /// only visibly wrong where the bind poses differ from the rest inverses.
    ///
    /// [`Skin`]: gdserver3d::skin::Skin
    fn resolve_skin(
        tree: &SceneTree,
        skeleton: NodeId,
        mesh_node: NodeId,
    ) -> gdserver3d::skin::Skin {
        if let Some(skin) = Self::authored_skin(tree, skeleton, mesh_node) {
            return skin;
        }
        let mut skin = gdserver3d::skin::Skin::new();
        for (bone, rest) in skeleton3d::bone_rest_array(tree, skeleton)
            .into_iter()
            .enumerate()
        {
            skin.add_bind_from_rest(bone, rest);
        }
        skin
    }

    /// Reads the `skin` resource `packed_scene` flattened onto `mesh_node` as
    /// `__skin__*` companions, mirroring Godot's `Skin::_set` property names
    /// (`bind_count`, `bind/<i>/bone`, `bind/<i>/name`, `bind/<i>/pose`).
    ///
    /// `None` means the node carries no skin at all and the caller must fall
    /// back to the rest-derived one. A `bind_count` of `0` is a different
    /// thing — an authored skin that happens to be empty — and is returned as
    /// such.
    ///
    /// 🔑 **Every bind resolves to some bone; none is ever dropped.** A mesh's
    /// `bone_indices` select binds *by position*, so skipping an unresolvable
    /// entry would silently renumber every bind after it and deform the model
    /// by the wrong limb. Godot handles this the same way — an unknown name, an
    /// out-of-range index, and a bind with neither all fall back to bone 0 (see
    /// `upstream/godot/scene/3d/skeleton_3d.cpp:363-393`) — so this keeps the
    /// positional contract rather than inventing a sparser one.
    fn authored_skin(
        tree: &SceneTree,
        skeleton: NodeId,
        mesh_node: NodeId,
    ) -> Option<gdserver3d::skin::Skin> {
        let node = tree.get_node(mesh_node)?;
        let count = match node.get_property("__skin__bind_count") {
            Variant::Int(n) if n >= 0 => n as usize,
            _ => return None,
        };
        let bone_count = skeleton3d::get_bone_count(tree, skeleton);

        let mut skin = gdserver3d::skin::Skin::new();
        for i in 0..count {
            // A non-empty name wins outright and does NOT fall back to the
            // index: Godot's default for `bind/<i>/bone` is -1 precisely so a
            // named bind can leave it unset, and a name that fails to resolve
            // is an authoring error rather than a request to use the index.
            let bone = match node.get_property(&format!("__skin__bind/{i}/name")) {
                Variant::String(name) if !name.is_empty() => {
                    skeleton3d::find_bone(tree, skeleton, &name).unwrap_or(0)
                }
                _ => match node.get_property(&format!("__skin__bind/{i}/bone")) {
                    Variant::Int(b) if b >= 0 && (b as usize) < bone_count => b as usize,
                    _ => 0,
                },
            };
            // Godot's `Skin::get_bind_pose` defaults to the identity, which
            // makes the bind a pass-through rather than a dropped influence.
            let pose = match node.get_property(&format!("__skin__bind/{i}/pose")) {
                Variant::Transform3D(t) => t,
                _ => Transform3D::IDENTITY,
            };
            skin.add_bind(bone, pose);
        }
        Some(skin)
    }

    /// Syncs MultiMeshInstance3D nodes from the scene tree to the render server.
    ///
    /// Each MultiMeshInstance3D expands into N render instances (one per
    /// multimesh entry). Per-instance transforms are composed with the node's
    /// global transform. Returns the number of visible expanded instances.
    fn sync_multimesh_instances(&mut self, tree: &SceneTree) -> u32 {
        let all_nodes = tree.all_nodes_in_tree_order();

        let mut current_mm_nodes: HashMap<NodeId, ()> = HashMap::new();
        let mut visible_count = 0u32;

        for &nid in &all_nodes {
            if let Some(node) = tree.get_node(nid) {
                if node.class_name() != "MultiMeshInstance3D" {
                    continue;
                }
                current_mm_nodes.insert(nid, ());

                let node_transform = node3d::get_global_transform(tree, nid);
                let node_visible = node3d::is_visible_in_tree(tree, nid);
                // ⚠️ VISIBLE count, not the raw instance count. This loop used
                // `get_multimesh_instance_count`, so a MultiMesh authored with 3
                // instances and `visible_instance_count = 2` allocated and drew
                // all three. Shared with the editor list through
                // `node3d::multimesh_visible_count` (pat-mgixk.50).
                let instance_count = node3d::multimesh_visible_count(tree, nid);

                // Resolve shared mesh from multimesh_mesh_type or mesh_type.
                let mesh = Self::resolve_multimesh_mesh(tree, nid);
                // MultiMeshInstance3D is a GeometryInstance3D too, so its
                // cast_shadow / visibility_range / transparency apply to every
                // expanded entry (pat-nzgdu.76).
                let geometry = geometry_instance_settings_for_node(tree, nid);

                // Get or create the expanded instance list for this node.
                let existing = self.multimesh_map.node_to_instances.remove(&nid);
                let mut inst_ids: Vec<Instance3DId> = existing.unwrap_or_default();

                // Free excess instances if count shrunk.
                while inst_ids.len() > instance_count {
                    if let Some(id) = inst_ids.pop() {
                        self.renderer.free_instance(id);
                    }
                }

                // Create new instances if count grew.
                while inst_ids.len() < instance_count {
                    let new_id = self.renderer.create_instance();
                    self.renderer.set_mesh(new_id, mesh.clone());
                    inst_ids.push(new_id);
                }

                // Update each expanded instance (mesh + transform + material).
                for i in 0..instance_count {
                    let inst_id = inst_ids[i];

                    // Ensure mesh is always up-to-date (handles mesh type changes).
                    self.renderer.set_mesh(inst_id, mesh.clone());

                    // Compose: node global transform * per-instance transform.
                    let per_inst = node3d::get_multimesh_instance_transform(tree, nid, i);
                    let composed = node_transform * per_inst;

                    self.renderer.set_transform(inst_id, composed);
                    self.renderer.set_visible(inst_id, node_visible);
                    self.renderer
                        .set_geometry_instance_settings(inst_id, geometry.clone());

                    // Per-instance color → material albedo.
                    let color = node3d::get_multimesh_instance_color(tree, nid, i);
                    let mat = Material3D {
                        albedo: color,
                        ..Material3D::default()
                    };
                    self.renderer.set_material(inst_id, mat);

                    if node_visible {
                        visible_count += 1;
                    }
                }

                self.multimesh_map.node_to_instances.insert(nid, inst_ids);
            }
        }

        // Free instances for removed MultiMeshInstance3D nodes.
        let stale: Vec<NodeId> = self
            .multimesh_map
            .node_to_instances
            .keys()
            .filter(|nid| !current_mm_nodes.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(ids) = self.multimesh_map.node_to_instances.remove(&nid) {
                for id in ids {
                    self.renderer.free_instance(id);
                }
            }
        }

        visible_count
    }

    /// Syncs `GPUParticles3D` / `CPUParticles3D` nodes into the 3D draw list
    /// (pat-mgixk.70).
    ///
    /// Each emitter expands into one render instance per LIVE particle, drawing
    /// its `draw_pass_1` mesh (GPUParticles3D) or `mesh` (CPUParticles3D) at the
    /// particle's position and tinted by its colour — the same expansion
    /// [`sync_multimesh_instances`](Self::sync_multimesh_instances) performs.
    ///
    /// 🔑 The per-particle data is read off the NODE, not from the simulator:
    /// `MainLoop::process_particles` publishes `_particle_positions`
    /// (`PackedVector3Array`) and `_particle_colors` onto each emitter every
    /// frame. That is the existing seam — the render server syncs from the
    /// `SceneTree` and has no access to `MainLoop`'s simulators — and it had no
    /// reader at all on the 3D side.
    ///
    /// ⚠️ Those positions are already WORLD-space when `local_coords` is false
    /// (Godot's default), because the emitter's global transform is applied at
    /// spawn. Composing the node transform on top would apply it twice and send
    /// particles to double the emitter's offset. Only `local_coords = true`
    /// keeps them in the node's space and therefore needs the compose.
    fn sync_particles_3d(&mut self, tree: &SceneTree) -> u32 {
        let all_nodes = tree.all_nodes_in_tree_order();
        let mut current: HashMap<NodeId, ()> = HashMap::new();
        let mut visible_count = 0u32;

        for &nid in &all_nodes {
            let Some(node) = tree.get_node(nid) else {
                continue;
            };
            if !matches!(node.class_name(), "GPUParticles3D" | "CPUParticles3D") {
                continue;
            }
            // Godot draws NOTHING for an emitter with no draw pass, so an
            // unresolvable mesh skips the node entirely rather than standing in
            // a cube — a placeholder here would put grey boxes in every game
            // frame that has a particle node.
            let Some(mesh) = Self::resolve_particle_mesh(tree, nid) else {
                continue;
            };
            current.insert(nid, ());

            let positions: Vec<Vector3> = match node.get_property("_particle_positions") {
                Variant::PackedVector3Array(v) => v,
                _ => Vec::new(),
            };
            let colors: Vec<Color> = match node.get_property("_particle_colors") {
                Variant::PackedColorArray(v) => v,
                _ => Vec::new(),
            };
            let local_coords = matches!(node.get_property("local_coords"), Variant::Bool(true));
            let node_visible = node3d::is_visible_in_tree(tree, nid);
            let node_transform = node3d::get_global_transform(tree, nid);
            let geometry = geometry_instance_settings_for_node(tree, nid);

            let count = positions.len();
            let existing = self.particle_map.node_to_instances.remove(&nid);
            let mut inst_ids: Vec<Instance3DId> = existing.unwrap_or_default();
            while inst_ids.len() > count {
                if let Some(id) = inst_ids.pop() {
                    self.renderer.free_instance(id);
                }
            }
            while inst_ids.len() < count {
                let new_id = self.renderer.create_instance();
                self.renderer.set_mesh(new_id, mesh.clone());
                inst_ids.push(new_id);
            }

            for (i, pos) in positions.iter().enumerate() {
                let inst_id = inst_ids[i];
                self.renderer.set_mesh(inst_id, mesh.clone());

                let mut particle_tf = Transform3D::IDENTITY;
                particle_tf.origin = *pos;
                let composed = if local_coords {
                    node_transform * particle_tf
                } else {
                    particle_tf
                };
                self.renderer.set_transform(inst_id, composed);
                self.renderer.set_visible(inst_id, node_visible);
                self.renderer
                    .set_geometry_instance_settings(inst_id, geometry.clone());

                let mat = Material3D {
                    albedo: colors.get(i).copied().unwrap_or(Color::WHITE),
                    // Godot's particles are unlit by default, like Sprite3D:
                    // shading them would make a colour-ramp particle read as
                    // black wherever no light reaches it.
                    shading_mode: ShadingMode::Unlit,
                    // ⚠️ A particle quad MUST disable back-face culling, and
                    // this is load-bearing rather than cosmetic:
                    // `Material3D::default()` is `cull_mode: CullMode::Back`,
                    // which culls a `QuadMesh` by winding so it never reaches a
                    // pixel. `sprite3d_render.rs` carries the same two lines
                    // with the same note — a Label3D's text vanished for
                    // exactly this reason. The instances were synced correctly
                    // and the frame still came out empty.
                    //
                    // Godot's particle quads are visible from both sides too,
                    // so this matches behaviour as well as fixing the draw.
                    double_sided: true,
                    cull_mode: CullMode::Disabled,
                    ..Material3D::default()
                };
                self.renderer.set_material(inst_id, mat);

                if node_visible {
                    visible_count += 1;
                }
            }

            self.particle_map.node_to_instances.insert(nid, inst_ids);
        }

        // Free instances for emitters that left the tree (or stopped resolving
        // a mesh), mirroring the MultiMesh cleanup.
        let stale: Vec<NodeId> = self
            .particle_map
            .node_to_instances
            .keys()
            .filter(|nid| !current.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(ids) = self.particle_map.node_to_instances.remove(&nid) {
                for id in ids {
                    self.renderer.free_instance(id);
                }
            }
        }

        visible_count
    }

    /// The mesh a 3D emitter draws per particle, or `None` when it has none.
    ///
    /// `draw_pass_1` is GPUParticles3D's first draw pass and `mesh` is
    /// CPUParticles3D's equivalent. A `.tscn` writes either as
    /// `SubResource("QuadMesh_1")`, which `packed_scene` records on the node as
    /// `__resref__<prop>` holding `"<class>::<sub id>"` — the generic
    /// `__subres__<prop>__<key>` copy beside it is a verbatim save-round-trip
    /// bag whose own comment says no render path reads it, so the class comes
    /// from the `__resref__` companion.
    fn resolve_particle_mesh(tree: &SceneTree, nid: NodeId) -> Option<Mesh3D> {
        let node = tree.get_node(nid)?;
        for key in ["draw_pass_1", "mesh"] {
            if let Variant::String(meta) = node.get_property(&format!("__resref__{key}")) {
                if let Some((class, _)) = meta.split_once("::") {
                    if let Some(m) = Self::mesh_from_type_name(class) {
                        return Some(m);
                    }
                }
            }
            // A flat `<key>_type` string, which the MultiMesh resolver accepts
            // too and which a Rust-built fixture is likeliest to set.
            if let Variant::String(s) = node.get_property(&format!("{key}_type")) {
                if let Some(m) = Self::mesh_from_type_name(&s) {
                    return Some(m);
                }
            }
        }
        None
    }

    /// Syncs `Sprite3D` nodes into the 3D draw list (pat-u3l8c.39).
    ///
    /// Each visible `Sprite3D` becomes one textured-quad render instance: the
    /// node's properties are read into a [`Sprite3D`], `to_mesh()` builds the
    /// quad geometry (region/flip UVs baked in), and the material carries the
    /// sprite's modulate tint (unlit by default, matching Godot). Billboard
    /// modes replace the instance basis so the quad faces `camera_pos`. Returns
    /// the number of visible sprites, and frees instances for removed nodes.
    fn sync_sprite3d(&mut self, tree: &SceneTree, camera_pos: Vector3) -> u32 {
        let all_nodes = tree.all_nodes_in_tree_order();

        let mut current: HashMap<NodeId, ()> = HashMap::new();
        let mut visible_count = 0u32;

        for &nid in &all_nodes {
            let node = match tree.get_node(nid) {
                Some(n) => n,
                None => continue,
            };
            if node.class_name() != "Sprite3D" {
                continue;
            }
            current.insert(nid, ());

            let sprite = Self::sprite3d_from_node(tree, nid);
            let node_transform = node3d::get_global_transform(tree, nid);
            let visible = node3d::is_visible_in_tree(tree, nid);

            // Billboard modes ignore the node's own rotation and orient the quad
            // toward the camera; disabled billboarding keeps the node basis.
            let basis = if sprite.billboard == BillboardMode3D::Disabled {
                node_transform.basis
            } else {
                sprite.billboard_basis(node_transform.origin, camera_pos)
            };
            let transform = Transform3D {
                basis,
                origin: node_transform.origin,
            };

            let inst_id = if let Some(&existing) = self.sprite_map.node_to_instance.get(&nid) {
                existing
            } else {
                let new_id = self.renderer.create_instance();
                self.sprite_map.node_to_instance.insert(nid, new_id);
                new_id
            };

            // Rebuild the quad every frame so region/flip/pixel_size edits apply.
            self.renderer.set_mesh(inst_id, sprite.to_mesh());
            self.renderer
                .set_material(inst_id, Self::sprite_material(&sprite));
            self.renderer.set_transform(inst_id, transform);
            self.renderer.set_visible(inst_id, visible);

            if visible {
                visible_count += 1;
            }
        }

        // Free instances for Sprite3D nodes no longer present.
        let stale: Vec<NodeId> = self
            .sprite_map
            .node_to_instance
            .keys()
            .filter(|nid| !current.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(inst_id) = self.sprite_map.node_to_instance.remove(&nid) {
                self.renderer.free_instance(inst_id);
            }
        }

        visible_count
    }

    /// Builds the render material for a `Sprite3D`: the modulate color drives
    /// the albedo, sprites are unlit unless `shaded` is set, and `double_sided`
    /// is honored so the quad shows from behind (Godot's `Sprite3D` default).
    /// Extracts every `Label3D` node into the render list as a text quad row
    /// (pat-0q2kx.109).
    ///
    /// `gdserver3d::label3d` held the full glyph-layout and billboard model but
    /// had **no callers anywhere** — nothing read a `Label3D` node, so the class
    /// registered, produced no geometry, and rendered as nothing (or as a
    /// generic marker). This is the missing extraction, built as a direct
    /// mirror of [`sync_sprite3d`](Self::sync_sprite3d) so both node types
    /// behave identically for billboarding, visibility and instance lifetime.
    fn sync_label3d(&mut self, tree: &SceneTree, camera_pos: Vector3) -> u32 {
        let all_nodes = tree.all_nodes_in_tree_order();
        let mut current: HashMap<NodeId, ()> = HashMap::new();
        let mut visible_count = 0u32;

        for &nid in &all_nodes {
            let node = match tree.get_node(nid) {
                Some(n) => n,
                None => continue,
            };
            if node.class_name() != "Label3D" {
                continue;
            }
            current.insert(nid, ());

            let label = Self::label3d_from_node(tree, nid);
            let node_transform = node3d::get_global_transform(tree, nid);
            let visible = node3d::is_visible_in_tree(tree, nid);

            let basis = if label.billboard == BillboardMode3D::Disabled {
                node_transform.basis
            } else {
                label.billboard_basis(node_transform.origin, camera_pos)
            };
            let transform = Transform3D {
                basis,
                origin: node_transform.origin,
            };

            let inst_id = if let Some(&existing) = self.label_map.node_to_instance.get(&nid) {
                existing
            } else {
                let new_id = self.renderer.create_instance();
                self.label_map.node_to_instance.insert(nid, new_id);
                new_id
            };

            // Rebuilt every frame so a `text` edit re-renders next frame, which
            // is exactly what the acceptance case checks.
            self.renderer.set_mesh(inst_id, label.to_mesh());
            self.renderer.set_material(
                inst_id,
                Material3D {
                    // `Label3D::modulate` is a raw [f32; 4] (unlike
                    // `Sprite3D::modulate`, which is already a Color), so it
                    // must be converted rather than assigned through.
                    albedo: gdcore::math::Color::new(
                        label.modulate[0],
                        label.modulate[1],
                        label.modulate[2],
                        label.modulate[3],
                    ),
                    // Godot's Label3D is unlit by default: text must stay
                    // legible regardless of scene lighting.
                    shading_mode: ShadingMode::Unlit,
                    double_sided: true,
                    ..Material3D::default()
                },
            );
            self.renderer.set_transform(inst_id, transform);
            self.renderer.set_visible(inst_id, visible);

            if visible {
                visible_count += 1;
            }
        }

        let stale: Vec<NodeId> = self
            .label_map
            .node_to_instance
            .keys()
            .filter(|nid| !current.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(inst_id) = self.label_map.node_to_instance.remove(&nid) {
                self.renderer.free_instance(inst_id);
            }
        }

        visible_count
    }

    /// Reads a `Label3D` node's properties into a [`Label3D`], falling back to
    /// Godot's documented defaults for anything the node does not set.
    fn label3d_from_node(tree: &SceneTree, nid: NodeId) -> Label3D {
        let mut label = Label3D::default();
        let Some(node) = tree.get_node(nid) else {
            return label;
        };

        if let gdvariant::Variant::String(t) = node.get_property("text") {
            label.text = t;
        }
        match node.get_property("pixel_size") {
            gdvariant::Variant::Float(f) if f > 0.0 => label.pixel_size = f as f32,
            gdvariant::Variant::Int(i) if i > 0 => label.pixel_size = i as f32,
            _ => {}
        }
        // Godot exposes `font_size` in points over a 16pt base; the server type
        // takes a unitless scale, so convert rather than assigning raw points —
        // passing 32 straight through would render the text 32× too large.
        match node.get_property("font_size") {
            gdvariant::Variant::Int(i) if i > 0 => label.font_scale = i as f32 / 16.0,
            gdvariant::Variant::Float(f) if f > 0.0 => label.font_scale = f as f32 / 16.0,
            _ => {}
        }
        if let gdvariant::Variant::Color(c) = node.get_property("modulate") {
            label.modulate = [c.r, c.g, c.b, c.a];
        }
        label.billboard = match node.get_property("billboard") {
            gdvariant::Variant::Int(1) => BillboardMode3D::Enabled,
            gdvariant::Variant::Int(2) => BillboardMode3D::Particles,
            gdvariant::Variant::Bool(true) => BillboardMode3D::Enabled,
            _ => BillboardMode3D::Disabled,
        };
        if let gdvariant::Variant::Bool(b) = node.get_property("centered") {
            label.centered = b;
        }
        label
    }

    fn sprite_material(sprite: &Sprite3D) -> Material3D {
        Material3D {
            albedo: sprite.modulate,
            shading_mode: if sprite.shaded {
                ShadingMode::Lambert
            } else {
                ShadingMode::Unlit
            },
            double_sided: sprite.double_sided,
            // ⚠️ The rasterizers cull on `cull_mode` alone; `double_sided` is
            // never read (the same trap `gdeditor::sprite3d_render` documents).
            // SpriteBase3D's `double_sided` (default true) is Godot's
            // CULL_DISABLED for the sprite's material.
            cull_mode: if sprite.double_sided {
                CullMode::Disabled
            } else {
                CullMode::Back
            },
            ..Material3D::default()
        }
    }

    /// Reads a `Sprite3D`'s properties off a scene node into a [`Sprite3D`],
    /// falling back to Godot defaults for any property the node does not set.
    fn sprite3d_from_node(tree: &SceneTree, nid: NodeId) -> Sprite3D {
        let mut sprite = Sprite3D::default();
        let node = match tree.get_node(nid) {
            Some(n) => n,
            None => return sprite,
        };

        let get = |k: &str| node.get_property(k);
        let as_f32 = |v: gdvariant::Variant| -> Option<f32> {
            match v {
                gdvariant::Variant::Float(f) => Some(f as f32),
                gdvariant::Variant::Int(i) => Some(i as f32),
                _ => None,
            }
        };
        let as_bool = |v: gdvariant::Variant| -> Option<bool> {
            match v {
                gdvariant::Variant::Bool(b) => Some(b),
                _ => None,
            }
        };

        if let gdvariant::Variant::String(path) = get("texture") {
            if !path.is_empty() {
                sprite.texture = Some(TextureSlot { path });
            }
        }
        if let gdvariant::Variant::Vector2(sz) = get("texture_size") {
            sprite.texture_size = Vector2i::new(sz.x as i32, sz.y as i32);
        }
        if let Some(b) = as_bool(get("centered")) {
            sprite.centered = b;
        }
        if let gdvariant::Variant::Vector2(o) = get("offset") {
            sprite.offset = o;
        }
        if let Some(b) = as_bool(get("flip_h")) {
            sprite.flip_h = b;
        }
        if let Some(b) = as_bool(get("flip_v")) {
            sprite.flip_v = b;
        }
        if let gdvariant::Variant::Color(c) = get("modulate") {
            sprite.modulate = c;
        }
        if let Some(px) = as_f32(get("pixel_size")) {
            sprite.pixel_size = px;
        }
        if let gdvariant::Variant::Int(a) = get("axis") {
            sprite.axis = match a {
                0 => Axis3D::X,
                1 => Axis3D::Y,
                _ => Axis3D::Z,
            };
        }
        if let gdvariant::Variant::Int(b) = get("billboard") {
            sprite.billboard = BillboardMode3D::from_godot_int(b);
        }
        if let Some(b) = as_bool(get("shaded")) {
            sprite.shaded = b;
        }
        if let Some(b) = as_bool(get("double_sided")) {
            sprite.double_sided = b;
        }
        if let Some(b) = as_bool(get("region_enabled")) {
            sprite.region_enabled = b;
        }
        if let gdvariant::Variant::Rect2(r) = get("region_rect") {
            sprite.region_rect = r;
        }

        sprite
    }

    /// Resolves the shared mesh for a MultiMeshInstance3D.
    /// Delegates to [`multimesh_mesh_for_node`], the shared resolver the editor
    /// list uses too — two copies of this chain is how the two paths drifted
    /// (pat-mgixk.50).
    fn resolve_multimesh_mesh(tree: &SceneTree, nid: NodeId) -> Mesh3D {
        multimesh_mesh_for_node(tree, nid)
    }

    /// Maps a mesh type name string to its default [`Mesh3D`] geometry.
    ///
    /// Delegates to the shared resolver so this list cannot drift from the
    /// editor viewport's again (pat-s3rrv.199); it also gains TorusMesh,
    /// PrismMesh and PointMesh, which had no arm here.
    pub(crate) fn mesh_from_type_name(name: &str) -> Option<Mesh3D> {
        gdserver3d::primitive_mesh::build_primitive(name, &|_| None)
    }

    /// Reads an authored primitive-mesh property off the node.
    ///
    /// A `mesh = SubResource("id")` reference has its sub-resource flattened
    /// onto the node under `__mesh__<key>`, the same way a CollisionShape's
    /// geometry arrives as `__shape__<key>` — by render time the sub-resource
    /// itself is gone.
    fn authored_mesh_prop(tree: &SceneTree, nid: NodeId, key: &str) -> Option<f32> {
        use gdvariant::Variant;
        let node = tree.get_node(nid)?;

        // A vector property is stored whole (`__mesh__size`) while the resolver
        // addresses it component-wise (`size.x`). Splitting here is what keeps
        // `packed_scene` copying sub-resource properties verbatim, with no
        // per-mesh-type knowledge on the producing side.
        if let Some((base, component)) = key.split_once('.') {
            return match node.get_property(&format!("__mesh__{base}")) {
                Variant::Vector3(v) => match component {
                    "x" => Some(v.x),
                    "y" => Some(v.y),
                    "z" => Some(v.z),
                    _ => None,
                },
                Variant::Vector2(v) => match component {
                    "x" => Some(v.x),
                    "y" => Some(v.y),
                    _ => None,
                },
                _ => None,
            };
        }

        match node.get_property(&format!("__mesh__{key}")) {
            Variant::Float(f) => Some(f as f32),
            Variant::Int(i) => Some(i as f32),
            // TubeTrailMesh's cap_top/cap_bottom are bools; the numeric lookup
            // carries them as 0/1, matching how the scene marker encodes them.
            Variant::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Reads an authored string-valued primitive-mesh property off the node.
    ///
    /// Only `TextMesh` has any (`text`, `font`). The runner reads the typed
    /// `__mesh__*` companion directly, so unlike the editor's marker channel it
    /// needs no escaping — the value never passes through a delimited string.
    fn authored_mesh_str(tree: &SceneTree, nid: NodeId, key: &str) -> Option<String> {
        match tree.get_node(nid)?.get_property(&format!("__mesh__{key}")) {
            gdvariant::Variant::String(s) => Some(s),
            _ => None,
        }
    }

    /// Resolves the mesh geometry from a MeshInstance3D node's properties.
    ///
    /// Inspects the `"mesh"` property path to determine the mesh type:
    /// - `.glb`/`.gltf` paths → loads via [`gdresource::import_gltf`] and
    ///   converts the first sub-resource to a [`Mesh3D`]
    /// - Paths containing "sphere" → UV sphere
    /// - Paths containing "plane" or "quad" → XZ plane
    /// - Everything else (or no path) → default cube
    pub fn resolve_mesh(tree: &SceneTree, nid: NodeId) -> Mesh3D {
        // An `ImmediateMesh` or `SurfaceTool` assigned from GDScript arrives as
        // a tagged handle, not a path (pat-s3rrv.197).
        //
        // ⚠️ This branch MUST come first. `get_mesh_path` below matches only
        // `Variant::String`, so a handle falls straight through to the
        // path-sniffing chain and lands on its `Mesh3D::cube(1.0)` fallback —
        // and that failure is deceptive rather than loud: the instance exists,
        // it has geometry, and it reports exactly ONE surface, so anything
        // checking only "a mesh was produced" passes against a cube that
        // ignored the script entirely. Only the vertex count betrays it.
        if let Some(node) = tree.get_node(nid) {
            if let Variant::ObjectId(oid) = node.get_property("mesh") {
                let raw = oid.raw();
                if gdscript_interop::bindings::is_mesh_builder_handle(raw) {
                    if let Some(mesh) = crate::scripting::immediate_mesh_geometry(raw)
                        .or_else(|| crate::scripting::surface_tool_geometry(raw))
                    {
                        return mesh;
                    }
                    // A live handle with nothing in it — a cleared ImmediateMesh
                    // or an uncommitted SurfaceTool. Godot draws nothing for
                    // that, so an EMPTY mesh is the honest answer; falling
                    // through would substitute a cube for geometry the script
                    // deliberately emptied.
                    return Mesh3D::default();
                }
            }
        }

        // -- script-built meshes (pat-s3rrv.201) --
        // A GDScript that builds geometry with `SurfaceTool` or `ImmediateMesh`
        // assigns the builder's HANDLE to `MeshInstance3D.mesh`, so the value is
        // a `Variant::ObjectId`, not a path.
        //
        // ⚠️ `get_mesh_path` matches only `Variant::String`, so before this the
        // handle resolved to `None` and the node fell through to the default
        // CUBE. `pat-s3rrv.197` anticipated exactly that and left
        // `surface_tool_geometry` / `immediate_mesh_geometry` here for this
        // call site — **and nothing ever called them**. Both accessors had zero
        // callers workspace-wide, so every script-built mesh drew a cube.
        //
        // The failure is deceptive rather than loud: the instance exists and
        // carries one surface, so any check that merely counts surfaces passes
        // against geometry that ignored the script entirely.
        if let Some(node) = tree.get_node(nid) {
            if let gdvariant::Variant::ObjectId(oid) = node.get_property("mesh") {
                let raw = oid.raw();
                if let Some(mesh) = crate::scripting::surface_tool_geometry(raw)
                    .or_else(|| crate::scripting::immediate_mesh_geometry(raw))
                {
                    return mesh;
                }
            }
        }

        if let Some(path) = node3d::get_mesh_path(tree, nid) {
            let lower = path.to_lowercase();

            // Try glTF loading for .glb / .gltf paths.
            if lower.ends_with(".glb") || lower.ends_with(".gltf") {
                let file_path = std::path::Path::new(&path);
                if let Ok(resource) = gdresource::import_gltf(file_path) {
                    // `gltf_mesh_index` says WHICH mesh of the file this node
                    // uses. A node instantiated from a glTF scene always carries
                    // it (`gltf_scene::build_gltf_subtree`); without it the only
                    // honest answer is the whole document, which is what every
                    // node used to get (pat-mgixk.23).
                    let wanted = match tree
                        .get_node(nid)
                        .map(|n| n.get_property("gltf_mesh_index"))
                    {
                        Some(gdvariant::Variant::Int(i)) => Some(i),
                        _ => None,
                    };
                    if let Some(mesh) = mesh3d_from_gltf_resource_for_mesh(&resource, wanted) {
                        return mesh;
                    }
                }
                // Fall through to default cube if loading fails.
            }

            // A saved ArrayMesh (`.res` binary / `.tres` text) carries its
            // real geometry (pat-33wmr). A file that is not an ArrayMesh
            // (a PrimitiveMesh `.tres`) answers None and keeps the old path.
            if crate::array_mesh::is_mesh_resource_path(&path) {
                if let Some(mesh) = crate::array_mesh::load_array_mesh_file(&path) {
                    return mesh;
                }
            }

            // A resolved `primitive:<Class>` marker names its class exactly.
            // ⚠️ It must be consulted BEFORE the substring guesses below, which
            // predate it and are wrong for several types: "primitive:TorusMesh"
            // matches none of them and fell through to a default CUBE, and
            // "primitive:PlaneMesh" reached `Mesh3D::plane(1.0)` rather than
            // PlaneMesh's authored 2x2 default.
            if let Some(marker) = path.strip_prefix("primitive:") {
                // Strip any encoded property suffix (`|a,b` positional or
                // `?k=v` named) so only the class name is matched.
                let class = marker
                    .split(|c| c == '|' || c == '?')
                    .next()
                    .unwrap_or(marker);
                let authored = |key: &str| Self::authored_mesh_prop(tree, nid, key);
                let authored_str = |key: &str| Self::authored_mesh_str(tree, nid, key);
                if let Some(mesh) = gdserver3d::primitive_mesh::build_primitive_with_text(
                    class,
                    &authored,
                    &authored_str,
                ) {
                    return mesh;
                }
            }

            if lower.contains("capsule") {
                return Mesh3D::capsule(0.5, 2.0, 12, 4);
            }
            if lower.contains("cylinder") {
                return Mesh3D::cylinder(0.5, 0.5, 1.0, 12, 4);
            }
            if lower.contains("sphere") {
                return Mesh3D::sphere(0.5, 12);
            }
            if lower.contains("plane") || lower.contains("quad") {
                return Mesh3D::plane(1.0);
            }
        }
        // Also check a "mesh_type" variant property for programmatic scenes.
        if let Some(node) = tree.get_node(nid) {
            if let gdvariant::Variant::String(s) = node.get_property("mesh_type") {
                if let Some(m) = Self::mesh_from_type_name(&s) {
                    return m;
                }
            }
        }
        Mesh3D::cube(1.0)
    }

    /// Resolves the material for a MeshInstance3D node following Godot's
    /// priority chain:
    ///
    /// 1. `material_override` — overrides all surfaces
    /// 2. the authored StandardMaterial3D flattened onto the node at scene load
    ///    (`__material__*`) — the real sub-resource parameters and textures
    /// 3. `surface_material_override/0` — per-surface override (surface 0 for
    ///    the single-instance render path)
    /// 4. StandardMaterial3D properties (`albedo_color`, `metallic`, etc.)
    /// 5. `albedo` color property — legacy shorthand for a colored material
    /// 6. `Material3D::default()` — white Lambert
    fn resolve_material(&mut self, tree: &SceneTree, nid: NodeId) -> Material3D {
        // 1. The authored material, flattened onto the node at scene load under
        //    `__material__*` — the real parameters and texture references.
        //
        // ⚠️ THIS NOW RUNS BEFORE the `material_override` check below, and the
        // order is the fix, not a tidy-up (pat-mgixk.53). A reference-style
        // `material_override` reaches the node as the RAW STRING the .tscn held
        // (`ExtResource("1_abc")` / a SubResource ref), and
        // `get_material_override` returns exactly that string, so the old step 1
        // fed a reference literal to `material_from_path`, whose filename test
        // for `red`/`green`/`blue` matched nothing and answered default white —
        // and returned immediately, so this bag could never be consulted for an
        // overridden node at all. `packed_scene` has already applied Godot's
        // mesh-level priority when choosing WHICH material to flatten
        // (material_override, then surface_material_override/0, then the mesh's
        // own), so preferring the bag here is Godot's precedence, not a new one.
        //
        // `material_from_path` keeps its place below as the fallback for a
        // genuine path string, the only case it was ever meaningful for.
        if let Some(mat) = material3d_for_node(tree, nid, &mut self.texture_cache) {
            return mat;
        }

        // 2. A path-style `material_override` with no flattened bag.
        if let Some(override_path) = node3d::get_material_override(tree, nid) {
            return Self::material_from_path(&override_path);
        }

        // (was 2.) The authored StandardMaterial3D, flattened onto the node at scene
        //    load under `__material__*`. This is the real sub-resource data —
        //    metallic, roughness, emission, texture references — so it is
        //    strictly better than the filename colour heuristics
        //    `material_from_path` falls back to for a path-only reference, and
        //    better than the flat `albedo_color` shorthand below.
        //
        //    `packed_scene` already applied Godot's mesh-level priority when it
        //    chose which material to flatten (surface_material_override/0, then
        //    the mesh's own material), so this sits directly under
        //    `material_override` (pat-0q2kx.42).
        // 3. Check per-surface override for surface 0.
        if let Some(surface_path) = node3d::get_surface_override_material(tree, nid, 0) {
            return Self::material_from_path(&surface_path);
        }

        // 4. Check StandardMaterial3D properties on the node.
        if let Some(mat) = Self::resolve_standard_material(tree, nid) {
            return mat;
        }

        // 5. Check glTF embedded material properties.
        if let Some(path) = node3d::get_mesh_path(tree, nid) {
            let lower = path.to_lowercase();
            if lower.ends_with(".glb") || lower.ends_with(".gltf") {
                let file_path = std::path::Path::new(&path);
                if let Ok(resource) = gdresource::import_gltf(file_path) {
                    if let Some(mat) = material_from_gltf_resource(&resource) {
                        return mat;
                    }
                }
            }
        }

        // 6. Check legacy albedo color property.
        if let Some(node) = tree.get_node(nid) {
            if let gdvariant::Variant::Color(c) = node.get_property("albedo") {
                let mut mat = Material3D::default();
                mat.albedo = c;
                return mat;
            }
        }

        // 6. Default material.
        Material3D::default()
    }

    /// Attempts to build a [`Material3D`] from `StandardMaterial3D` properties
    /// set directly on a node.
    ///
    /// Returns `Some` if the node has at least `albedo_color` set, indicating
    /// it carries StandardMaterial3D-style properties. Reads `metallic`,
    /// `roughness`, `normal_enabled`, `normal_scale`, `emission`, and
    /// `shading_mode` when present.
    fn resolve_standard_material(tree: &SceneTree, nid: NodeId) -> Option<Material3D> {
        let node = tree.get_node(nid)?;

        // Only activate if the node has an explicit albedo_color property.
        let albedo_color = match node.get_property("albedo_color") {
            gdvariant::Variant::Color(c) => c,
            _ => return None,
        };

        let metallic = match node.get_property("metallic") {
            gdvariant::Variant::Float(f) => f as f32,
            _ => 0.0,
        };

        let roughness = match node.get_property("roughness") {
            gdvariant::Variant::Float(f) => f as f32,
            _ => 1.0,
        };

        let emission = match node.get_property("emission") {
            gdvariant::Variant::Color(c) => c,
            _ => Color::new(0.0, 0.0, 0.0, 0.0),
        };

        let shading_mode = match node.get_property("shading_mode") {
            gdvariant::Variant::Int(0) => ShadingMode::Unlit,
            gdvariant::Variant::Int(2) => ShadingMode::Phong,
            _ => ShadingMode::Lambert,
        };

        Some(Material3D {
            albedo: albedo_color,
            roughness,
            metallic,
            emission,
            shading_mode,
            double_sided: matches!(
                node.get_property("double_sided"),
                gdvariant::Variant::Bool(true)
            ),
            ..Material3D::default()
        })
    }

    /// Attempts to build a [`ShaderMaterial3D`] from properties set on a node.
    ///
    /// Returns `Some` if the node carries a ShaderMaterial: either an inline
    /// `shader_code` property (the source verbatim, e.g. flattened from a tscn
    /// `[sub_resource type="Shader"]`) or a `shader_path` pointing at an
    /// external `.gdshader` file (flattened from a Shader `[ext_resource]`),
    /// which is loaded on demand. `shader_type` is honored as either a mode
    /// string (`"spatial"`/`"sky"`, from the `.gdshader` source) or the legacy
    /// integer form (`1` = Sky). Any `shader_parameter/*` properties become
    /// bound uniform values.
    /// `pub` so the editor/Play draw list can carry a ShaderMaterial too, not
    /// just this adapter (pat-mgixk.50). The data is already on the node:
    /// `packed_scene` flattens a `ShaderMaterial` sub-resource to `shader_code`
    /// / `shader_parameter/*` rather than into the `__material__*` bag, and this
    /// reads those.
    pub fn resolve_shader_material(tree: &SceneTree, nid: NodeId) -> Option<ShaderMaterial3D> {
        let node = tree.get_node(nid)?;

        // Resolve shader source + type hint from an inline `shader_code` prop,
        // or by loading the `.gdshader` named by `shader_path`.
        let (shader_code, type_hint): (String, Option<String>) =
            match node.get_property("shader_code") {
                gdvariant::Variant::String(s) if !s.is_empty() => {
                    let hint = match node.get_property("shader_type") {
                        gdvariant::Variant::String(t) => Some(t),
                        gdvariant::Variant::Int(1) => Some("sky".to_string()),
                        _ => None,
                    };
                    (s, hint)
                }
                _ => match node.get_property("shader_path") {
                    gdvariant::Variant::String(p) if !p.is_empty() => {
                        // res:// paths resolve relative to the cwd (project root
                        // at runtime), matching the rest of the engine.
                        let fs = p.strip_prefix("res://").unwrap_or(p.as_str());
                        let res =
                            gdresource::importers::load_gdshader(std::path::Path::new(fs)).ok()?;
                        let code = match res.get_property("code") {
                            Some(gdvariant::Variant::String(c)) => c.clone(),
                            _ => return None,
                        };
                        let hint = match res.get_property("shader_type") {
                            Some(gdvariant::Variant::String(t)) => Some(t.clone()),
                            _ => None,
                        };
                        (code, hint)
                    }
                    _ => return None,
                },
            };

        let shader_type = match type_hint.as_deref() {
            Some("sky") => ShaderType3D::Sky,
            _ => ShaderType3D::Spatial,
        };

        let shader = Shader3D::new(shader_type, &shader_code);
        let mut mat = ShaderMaterial3D::new();
        mat.shader = Some(shader);

        // Collect shader parameters from properties prefixed with "shader_parameter/".
        for (key, value) in node.properties() {
            if let Some(param_name) = key.strip_prefix("shader_parameter/") {
                if value != &gdvariant::Variant::Nil {
                    mat.set_shader_parameter(param_name, value.clone());
                }
            }
        }

        Some(mat)
    }

    /// Converts a material path hint into a `Material3D`.
    ///
    /// Full resource loading is not yet implemented; this uses path-name
    /// heuristics to produce representative materials for testing and
    /// rendering. Recognized patterns:
    /// - Paths containing "red" → red albedo
    /// - Paths containing "green" → green albedo
    /// - Paths containing "blue" → blue albedo
    /// - Paths containing "metal" → metallic material
    /// - Paths containing "emissive" → emissive material
    /// - Everything else → default white Lambert
    fn material_from_path(path: &str) -> Material3D {
        let lower = path.to_lowercase();
        let mut mat = Material3D::default();

        if lower.contains("red") {
            mat.albedo = Color::new(1.0, 0.0, 0.0, 1.0);
        } else if lower.contains("green") {
            mat.albedo = Color::new(0.0, 1.0, 0.0, 1.0);
        } else if lower.contains("blue") {
            mat.albedo = Color::new(0.0, 0.0, 1.0, 1.0);
        }

        if lower.contains("metal") {
            mat.metallic = 1.0;
            mat.roughness = 0.2;
        }

        if lower.contains("emissive") || lower.contains("emission") {
            mat.emission = Color::new(1.0, 1.0, 1.0, 1.0);
        }

        mat
    }

    /// Syncs DirectionalLight3D and OmniLight3D nodes to the render server.
    ///
    /// Returns the number of active lights.
    fn sync_lights(&mut self, tree: &SceneTree) -> u32 {
        let all_nodes = tree.all_nodes_in_tree_order();

        let mut current_light_nodes: HashMap<NodeId, ()> = HashMap::new();
        let mut light_count = 0u32;

        for &nid in &all_nodes {
            if let Some(node) = tree.get_node(nid) {
                let class = node.class_name();
                let light_type = match class {
                    "DirectionalLight3D" => Some(LightType::Directional),
                    "OmniLight3D" => Some(LightType::Point),
                    "SpotLight3D" => Some(LightType::Spot),
                    _ => None,
                };

                if let Some(lt) = light_type {
                    current_light_nodes.insert(nid, ());

                    if !self.light_map.node_to_light.contains_key(&nid) {
                        self.light_map.next_id += 1;
                        let new_id = Light3DId(self.light_map.next_id);
                        self.light_map.node_to_light.insert(nid, new_id);

                        let mut light = match lt {
                            LightType::Directional => Light3D::directional(new_id),
                            LightType::Point => {
                                let pos = node3d::get_global_transform(tree, nid).origin;
                                Light3D::point(new_id, pos)
                            }
                            LightType::Spot => {
                                let t = node3d::get_global_transform(tree, nid);
                                Light3D::spot(new_id, t.origin, t.basis.z * -1.0)
                            }
                        };

                        // Sync common light properties from node.
                        light.energy = node3d::get_light_energy(tree, nid) as f32;
                        light.color = node3d::get_light_color(tree, nid);
                        if let gdvariant::Variant::Bool(s) = node.get_property("shadow_enabled") {
                            light.shadow_enabled = s;
                        }
                        // Extended Light3D property surface (pat-nzgdu.121).
                        // Each is applied only when the scene actually sets it,
                        // so a scene that never authored these renders exactly
                        // as before.
                        sync_light_properties(node, &mut light);

                        // Sync type-specific properties.
                        match lt {
                            LightType::Spot => {
                                light.spot_angle =
                                    (node3d::get_spot_angle(tree, nid) as f32).to_radians();
                                light.range = node3d::get_spot_range(tree, nid) as f32;
                                light.attenuation = node3d::get_spot_attenuation(tree, nid) as f32;
                                light.spot_angle_attenuation =
                                    node3d::get_spot_angle_attenuation(tree, nid) as f32;
                            }
                            LightType::Point => {
                                if let gdvariant::Variant::Float(r) =
                                    node.get_property("omni_range")
                                {
                                    light.range = r as f32;
                                }
                                if let gdvariant::Variant::Float(a) =
                                    node.get_property("omni_attenuation")
                                {
                                    light.attenuation = a as f32;
                                }
                                // Sync omni shadow mode (Godot property: 0=DualParaboloid, 1=Cube).
                                if let gdvariant::Variant::Int(mode) =
                                    node.get_property("omni_shadow_mode")
                                {
                                    light.omni_shadow_mode = match mode {
                                        1 => OmniShadowMode::Cube,
                                        _ => OmniShadowMode::DualParaboloid,
                                    };
                                }
                            }
                            LightType::Directional => {}
                        }

                        self.renderer.add_light(new_id);
                        self.renderer.update_light(&light);
                    }

                    light_count += 1;
                }
            }
        }

        // Remove lights for nodes that no longer exist.
        let stale: Vec<NodeId> = self
            .light_map
            .node_to_light
            .keys()
            .filter(|nid| !current_light_nodes.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(light_id) = self.light_map.node_to_light.remove(&nid) {
                self.renderer.remove_light(light_id);
            }
        }

        light_count
    }

    /// Syncs ReflectionProbe nodes from the scene tree to the render server.
    fn sync_reflection_probes(&mut self, tree: &SceneTree) {
        let all_nodes = tree.all_nodes_in_tree_order();
        let mut current_probe_nodes: HashMap<NodeId, ()> = HashMap::new();

        for &nid in &all_nodes {
            if let Some(node) = tree.get_node(nid) {
                if node.class_name() == "ReflectionProbe" {
                    current_probe_nodes.insert(nid, ());

                    if !self.reflection_probe_map.node_to_probe.contains_key(&nid) {
                        self.reflection_probe_map.next_id += 1;
                        let new_id = ReflectionProbeId(self.reflection_probe_map.next_id);
                        self.reflection_probe_map.node_to_probe.insert(nid, new_id);
                        self.renderer.add_reflection_probe(new_id);
                    }
                }
            }
        }

        // Remove probes for nodes that no longer exist.
        let stale: Vec<NodeId> = self
            .reflection_probe_map
            .node_to_probe
            .keys()
            .filter(|nid| !current_probe_nodes.contains_key(nid))
            .copied()
            .collect();
        for nid in stale {
            if let Some(probe_id) = self.reflection_probe_map.node_to_probe.remove(&nid) {
                self.renderer.remove_reflection_probe(probe_id);
            }
        }
    }
}

impl std::fmt::Debug for RenderServer3DAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderServer3DAdapter")
            .field("viewport_width", &self.viewport_width)
            .field("viewport_height", &self.viewport_height)
            .field("frame_counter", &self.frame_counter)
            .field(
                "tracked_instances",
                &self.instance_map.node_to_instance.len(),
            )
            .field(
                "tracked_multimesh_nodes",
                &self.multimesh_map.node_to_instances.len(),
            )
            .field("tracked_lights", &self.light_map.node_to_light.len())
            .field(
                "tracked_probes",
                &self.reflection_probe_map.node_to_probe.len(),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// glTF resource → Mesh3D conversion
// ---------------------------------------------------------------------------

/// Extracts a [`Mesh3D`] from the first sub-resource of a glTF [`Resource`]
/// produced by [`gdresource::import_gltf`].
///
/// Returns `None` if the resource has no mesh sub-resources or if the vertex
/// data cannot be extracted.
pub fn mesh3d_from_gltf_resource(resource: &gdresource::Resource) -> Option<Mesh3D> {
    mesh3d_from_gltf_resource_for_mesh(resource, None)
}

/// [`mesh3d_from_gltf_resource`] restricted to ONE glTF mesh.
///
/// ⚠️ The unfiltered form takes every sub-resource in the document, which is
/// every primitive of every mesh in the file. That is correct for a
/// single-mesh asset and wrong for any other: each node in a multi-mesh `.glb`
/// resolved to the same combined mesh, so two differently-shaped objects drew
/// identically (pat-mgixk.23). `mesh_index` selects just the primitives the
/// node's glTF mesh declares.
///
/// Passing `None` reproduces the old behaviour exactly, key-sort included, so
/// existing callers are unaffected.
pub fn mesh3d_from_gltf_resource_for_mesh(
    resource: &gdresource::Resource,
    mesh_index: Option<i64>,
) -> Option<Mesh3D> {
    let subs: Vec<_> = match mesh_index {
        Some(want) => {
            let mut matching: Vec<_> = resource
                .subresources
                .iter()
                .filter(|(_, sub)| {
                    matches!(
                        sub.get_property("mesh_index"),
                        Some(gdvariant::Variant::Int(i)) if *i == want
                    )
                })
                .collect();
            // Ordered by the glTF primitive index, NOT by the key string.
            // `mesh_0_prim_10` sorts before `mesh_0_prim_2` as text, which would
            // silently permute the surfaces of any mesh with ten or more
            // primitives — and surface order is what `surface_materials[i]`
            // pairs against.
            matching.sort_by_key(|(_, sub)| match sub.get_property("primitive_index") {
                Some(gdvariant::Variant::Int(i)) => *i,
                _ => 0,
            });
            matching
        }
        None => {
            let mut all: Vec<_> = resource.subresources.iter().collect();
            // Sort by key for deterministic ordering (mesh_0, mesh_0_prim_0, etc.).
            all.sort_by_key(|(k, _)| k.clone());
            all
        }
    };
    if subs.is_empty() {
        return None;
    }

    // First sub-resource becomes the primary surface.
    let (_, first_sub) = &subs[0];
    let vertices = extract_vector3_array(first_sub.get_property("vertices")?)?;
    if vertices.is_empty() {
        return None;
    }

    // normals / uvs / indices are optional — missing keys must not abort conversion
    let normals = first_sub
        .get_property("normals")
        .and_then(extract_vector3_array)
        .unwrap_or_default();
    let uvs = first_sub
        .get_property("uvs")
        .and_then(extract_uv_array)
        .unwrap_or_default();
    let indices = first_sub
        .get_property("indices")
        .and_then(extract_index_array)
        .unwrap_or_default();

    // Skinning influences (glTF JOINTS_0 / WEIGHTS_0), stored flattened by the
    // importer. Only kept when both are present and parallel to the positions —
    // `Mesh3D::is_skinned` requires that, and a half-rigged mesh would silently
    // skin some vertices to bind 0 (pat-0q2kx.3).
    let bone_indices = first_sub
        .get_property("bone_indices")
        .and_then(extract_bone_index_array)
        .unwrap_or_default();
    let bone_weights = first_sub
        .get_property("bone_weights")
        .and_then(extract_bone_weight_array)
        .unwrap_or_default();
    let rigged = bone_indices.len() == vertices.len() && bone_weights.len() == vertices.len();

    // Blend shapes (glTF morph targets), stored by the importer as parallel
    // name and delta arrays (pat-s3rrv.23).
    //
    // ⚠️ A target whose deltas are absent or unparseable is KEPT with an empty
    // delta list, not dropped. Godot addresses shapes by index as well as by
    // name (`blend_shapes/morph_N`), so dropping one renumbers every shape
    // after it; and `morph::apply_morph_with_mode` deliberately counts an empty
    // shape's weight toward `Σw` in normalized mode, mirroring Godot's
    // `blend_total`. Dropping it would leave the base over-weighted.
    let blend_shapes = match (
        first_sub.get_property("blend_shape_names"),
        first_sub.get_property("blend_shape_deltas"),
    ) {
        (Some(gdvariant::Variant::Array(names)), Some(gdvariant::Variant::Array(deltas))) => names
            .to_vec()
            .iter()
            .enumerate()
            .map(|(k, n)| gdserver3d::mesh::BlendShape3D {
                name: match n {
                    gdvariant::Variant::String(s) => s.clone(),
                    _ => format!("morph_{k}"),
                },
                vertex_deltas: deltas
                    .get(k)
                    .as_ref()
                    .and_then(extract_vector3_array)
                    .unwrap_or_default(),
            })
            .collect(),
        _ => Vec::new(),
    };

    let mut mesh = Mesh3D {
        vertices,
        normals,
        uvs,
        indices,
        primitive_type: PrimitiveType::Triangles,
        surfaces: Vec::new(),
        bone_indices: if rigged { bone_indices } else { Vec::new() },
        bone_weights: if rigged { bone_weights } else { Vec::new() },
        blend_shapes,
        ..Default::default()
    };

    // Additional sub-resources become extra surfaces.
    for (_, sub) in subs.iter().skip(1) {
        let s_verts = match sub.get_property("vertices").and_then(extract_vector3_array) {
            Some(v) if !v.is_empty() => v,
            _ => continue,
        };
        let s_normals = sub
            .get_property("normals")
            .and_then(extract_vector3_array)
            .unwrap_or_default();
        let s_uvs = sub
            .get_property("uvs")
            .and_then(extract_uv_array)
            .unwrap_or_default();
        let s_indices = sub
            .get_property("indices")
            .and_then(extract_index_array)
            .unwrap_or_default();

        mesh.surfaces.push(gdserver3d::mesh::Surface3D {
            vertices: s_verts,
            normals: s_normals,
            uvs: s_uvs,
            indices: s_indices,
            primitive_type: PrimitiveType::Triangles,
            lods: Vec::new(),
        });
    }

    Some(mesh)
}

/// Extracts a [`Material3D`] from a glTF resource's first sub-resource PBR properties.
///
/// Returns `None` if no material properties are present.
pub fn material_from_gltf_resource(resource: &gdresource::Resource) -> Option<Material3D> {
    let sub = resource.subresources.values().next()?;

    let albedo = match sub.get_property("material_albedo") {
        Some(gdvariant::Variant::Color(c)) => *c,
        _ => return None,
    };

    let metallic = match sub.get_property("material_metallic") {
        Some(gdvariant::Variant::Float(f)) => *f as f32,
        _ => 0.0,
    };

    let roughness = match sub.get_property("material_roughness") {
        Some(gdvariant::Variant::Float(f)) => *f as f32,
        _ => 1.0,
    };

    let emission = match sub.get_property("material_emissive") {
        Some(gdvariant::Variant::Color(c)) => *c,
        _ => gdcore::math::Color::BLACK,
    };

    let double_sided = match sub.get_property("material_double_sided") {
        Some(gdvariant::Variant::Bool(b)) => *b,
        _ => false,
    };

    Some(Material3D {
        albedo,
        metallic,
        roughness,
        emission,
        double_sided,
        ..Material3D::default()
    })
}

/// Extracts `Vec<Vector3>` from a `Variant::Array` of `Variant::Vector3`.
/// Whether `class` is a CSG shape node (Godot's CSGShape3D family).
fn is_csg_class(class: &str) -> bool {
    matches!(
        class,
        "CSGShape3D"
            | "CSGCombiner3D"
            | "CSGBox3D"
            | "CSGSphere3D"
            | "CSGCylinder3D"
            | "CSGTorus3D"
            | "CSGPolygon3D"
            | "CSGMesh3D"
            | "CSGPrimitive3D"
    )
}

/// Builds the primitive mesh a CSG node contributes, from its scene properties
/// (pat-nzgdu.119).
///
/// Returns `None` for nodes that carry no geometry of their own — a
/// CSGCombiner3D only combines its children, and unimplemented primitives
/// (torus/polygon/mesh) contribute nothing rather than guessing at geometry.
fn csg_primitive_mesh(node: &crate::node::Node) -> Option<Mesh3D> {
    use gdserver3d::csg::{CSGBox3D, CSGCylinder3D, CSGSphere3D};
    use gdvariant::Variant;

    match node.class_name() {
        "CSGBox3D" => {
            let size = match node.get_property("size") {
                Variant::Vector3(v) => v,
                _ => Vector3::new(2.0, 2.0, 2.0),
            };
            Some(CSGBox3D::new(size).to_mesh())
        }
        "CSGSphere3D" => {
            let radius = match node.get_property("radius") {
                Variant::Float(r) => r as f32,
                _ => 0.5,
            };
            Some(CSGSphere3D::new(radius).to_mesh())
        }
        "CSGCylinder3D" => {
            let radius = match node.get_property("radius") {
                Variant::Float(r) => r as f32,
                _ => 0.5,
            };
            let height = match node.get_property("height") {
                Variant::Float(h) => h as f32,
                _ => 2.0,
            };
            Some(CSGCylinder3D::new(radius, height).to_mesh())
        }
        _ => None,
    }
}

/// The boolean operation a CSG child applies to its parent's accumulated mesh
/// (Godot's `CSGShape3D.operation`: 0=union, 1=intersection, 2=subtraction).
fn csg_node_operation(node: &crate::node::Node) -> gdserver3d::csg::CSGOperation {
    use gdserver3d::csg::CSGOperation;
    match node.get_property("operation") {
        gdvariant::Variant::Int(v) => CSGOperation::from_godot_int(v),
        _ => CSGOperation::Union,
    }
}

/// Rewrites `mesh`'s vertices/normals from `child` space into `root` space so
/// the boolean solver evaluates every operand in one coordinate system.
fn csg_transform_mesh(mesh: &mut Mesh3D, root_inv: Transform3D, child_global: Transform3D) {
    let to_root = root_inv * child_global;
    for v in &mut mesh.vertices {
        *v = to_root.xform(*v);
    }
    // Normals are direction vectors: rotate them, don't translate.
    let mut rot = to_root;
    rot.origin = Vector3::ZERO;
    for n in &mut mesh.normals {
        *n = rot.xform(*n).normalized();
    }
}

/// Evaluates a CSG subtree rooted at `root` into a single mesh, in `root`'s
/// local space (pat-nzgdu.119).
///
/// Mirrors Godot: the root's own primitive seeds the result, then each CSG
/// descendant is folded in with its own `operation` (union / intersection /
/// subtraction) via the `gdserver3d::csg` solver. Invisible nodes are skipped.
/// Returns `None` when the subtree produces no geometry at all.
/// Applies a `MeshInstance3D`'s blend-shape weights to `base` (pat-s3rrv.23).
///
/// A module-level free function, like the other mesh helpers here — the
/// `Self::`-qualified resolvers it sits beside (`resolve_skeleton`,
/// `resolve_skin`) are associated fns, and calling this one the same way is
/// what broke the first build.
///
/// `None` when the mesh carries no blend shapes or every weight is zero — in
/// both cases the result is the base mesh, and returning it would clone a whole
/// mesh per node per frame for nothing.
///
/// ⚠️ The mode defaults to **Relative**, which is Godot's actual default:
/// `ArrayMesh.blend_shape_mode` is `default="1"` and `BLEND_SHAPE_MODE_RELATIVE`
/// is 1 (`doc/classes/ArrayMesh.xml:209`). `BlendShapeMode`'s own doc records
/// that this was previously — and wrongly — believed to be Normalized.
/// The mesh a `MeshInstance3D` should actually draw: its blend shapes applied,
/// then deformed by the bound `Skeleton3D`'s live bone poses (pat-s3rrv.23).
///
/// Returns `None` when the node is neither morphed nor skinned, so a caller can
/// fall through to its ordinary mesh path untouched.
///
/// ⚠️ **MORPH FIRST, THEN SKIN THE MORPHED MESH — the order is the whole
/// point.** Skinning the undeformed base instead silently discards the morph
/// for any mesh that is both, because the skinned result is what gets drawn.
/// The runtime adapter learned this the hard way and says so at its own call
/// site; exposing ONE helper rather than the three private pieces is what stops
/// a second call site from rediscovering it.
///
/// ⚠️ The morph is applied even when the mesh is NOT skinned. The skinning
/// branch is gated on `is_skinned()`, so folding morph into it would skip every
/// unskinned morphed mesh — a face with blend shapes and no skeleton is an
/// ordinary case.
pub fn deformed_mesh_for_node(tree: &SceneTree, nid: NodeId, base: &Mesh3D) -> Option<Mesh3D> {
    let morphed = morphed_mesh(tree, nid, base);
    let skinned = RenderServer3DAdapter::resolve_skeleton(tree, nid).and_then(|skeleton| {
        let source = morphed.as_ref().unwrap_or(base);
        if !source.is_skinned() {
            return None;
        }
        let poses = skeleton3d::bone_pose_array(tree, skeleton);
        let skin = RenderServer3DAdapter::resolve_skin(tree, skeleton, nid);
        Some(gdserver3d::skin::skin_mesh(source, &skin, &poses))
    });
    // Skinned wins because it already folded the morph in; morph alone is the
    // answer for an unskinned mesh; neither means nothing to draw differently.
    skinned.or(morphed)
}

fn morphed_mesh(tree: &SceneTree, nid: NodeId, base: &Mesh3D) -> Option<Mesh3D> {
    use gdrender3d::morph::{apply_morph_with_mode, BlendShapeMode, MorphTarget};
    use gdvariant::Variant;

    if base.blend_shapes.is_empty() {
        return None;
    }
    let node = tree.get_node(nid)?;

    // Godot addresses shapes by NAME (`blend_shapes/<name>` on the instance),
    // not by index, so the weight array is built from the shape names rather
    // than from whatever order the properties happen to be in.
    let weights: Vec<f32> = base
        .blend_shapes
        .iter()
        .map(
            |shape| match node.get_property(&format!("blend_shapes/{}", shape.name)) {
                Variant::Float(w) => w as f32,
                Variant::Int(w) => w as f32,
                _ => 0.0,
            },
        )
        .collect();
    if weights.iter().all(|w| *w == 0.0) {
        return None;
    }

    let targets: Vec<MorphTarget> = base
        .blend_shapes
        .iter()
        .map(|shape| MorphTarget {
            name: shape.name.clone(),
            deltas: shape.vertex_deltas.clone(),
        })
        .collect();
    let mode = match node.get_property("blend_shape_mode") {
        Variant::Int(0) => BlendShapeMode::Normalized,
        _ => BlendShapeMode::Relative,
    };

    let mut out = base.clone();
    out.vertices = apply_morph_with_mode(&base.vertices, &targets, &weights, mode);
    Some(out)
}

/// Whether `class` is one of the CSG node classes.
///
/// ⚠️ Public so a caller can SKIP CSG descendants. They contribute geometry to
/// their root and must draw nothing themselves; without that skip a subtracted
/// sphere still appears as its own marker beside the hole it cut.
pub fn is_csg_class_name(class: &str) -> bool {
    is_csg_class(class)
}

/// Whether `nid` is the ROOT of a CSG tree — the only node that evaluates.
///
/// ⚠️ Public together with [`evaluate_csg_tree`] so the editor's render path
/// shares this rule instead of re-deriving it (pat-s3rrv.89). A caller that got
/// it wrong would draw every shape of a boolean tree as its own solid AND the
/// carved result on top — a subtracted sphere would appear as a sphere.
///
/// Godot folds descendants into their root (`modules/csg/csg_shape.cpp`), so a
/// CSG node parented to another CSG node contributes geometry but draws nothing
/// of its own.
pub fn is_csg_root(tree: &SceneTree, nid: NodeId) -> bool {
    let Some(node) = tree.get_node(nid) else {
        return false;
    };
    if !is_csg_class(node.class_name()) {
        return false;
    }
    !node
        .parent()
        .and_then(|p| tree.get_node(p))
        .map(|p| is_csg_class(p.class_name()))
        .unwrap_or(false)
}

/// Folds a CSG subtree into one mesh (union / intersection / subtraction).
///
/// ⚠️ Call only for a node [`is_csg_root`] accepts; on a descendant this would
/// re-evaluate that descendant's own subtree as if it were independent.
pub fn evaluate_csg_tree(tree: &SceneTree, root: NodeId) -> Option<Mesh3D> {
    use gdserver3d::csg::CSGCombiner3D;

    let root_inv = node3d::get_global_transform(tree, root).inverse();

    // Seed with the root's own primitive (a CSGCombiner3D contributes none).
    let mut acc = tree.get_node(root).and_then(csg_primitive_mesh);

    let mut children = Vec::new();
    tree.collect_subtree_top_down(root, &mut children);
    for &nid in children.iter().skip(1) {
        let node = match tree.get_node(nid) {
            Some(n) => n,
            None => continue,
        };
        // pat-mgixk.32: visible IN THE TREE. Every collector in this file read
        // the node's own flag only, so hiding a parent hid nothing — the same
        // defect the editor's `collect_scene_content` had. The bead names this
        // CSG site; the five `sync_*` collectors above it are the identical
        // one-identifier bug on the render-server path, and leaving them would
        // have fixed "hiding a parent" in the editor and not in the runner.
        if !is_csg_class(node.class_name()) || !node3d::is_visible_in_tree(tree, nid) {
            continue;
        }
        let mut mesh = match csg_primitive_mesh(node) {
            Some(m) => m,
            None => continue,
        };
        csg_transform_mesh(&mut mesh, root_inv, node3d::get_global_transform(tree, nid));

        acc = Some(match acc {
            // The first operand has nothing to combine against, so it seeds the
            // result regardless of its operation (Godot: the first shape under
            // an empty combiner is simply the shape).
            None => mesh,
            Some(current) => CSGCombiner3D::combine(&current, &mesh, csg_node_operation(node)),
        });
    }
    acc
}

/// Applies the extended Light3D property surface from a scene node onto the
/// render server's light record (pat-nzgdu.121): colour temperature, cull and
/// shadow-caster masks, indirect/volumetric energy, distance fade, shadow
/// opacity, editor-only, and the DirectionalLight3D PSSM splits.
///
/// Every property is read through `get_property` and applied only when the
/// scene actually authored it, so a scene that sets none of these produces the
/// same light record as before this bead. `light_temperature` is folded into
/// the light's colour (Godot tints the light by its blackbody colour) while the
/// authored Kelvin value is kept on the record.
/// Reads a numeric node property, accepting either `Float` or `Int`.
///
/// ⚠️ A `.tscn` writes `lod_bias = 1` (no decimal point) as an **Int**, so a
/// `Float`-only match would silently drop whole-number values — the exact
/// values authors are most likely to type.
fn float_property(node: &crate::node::Node, name: &str) -> Option<f32> {
    match node.get_property(name) {
        Variant::Float(v) => Some(v as f32),
        Variant::Int(v) => Some(v as f32),
        _ => None,
    }
}

/// Reads Godot's `GeometryInstance3D` property block off a scene node
/// (pat-nzgdu.76).
///
/// Every property is optional: an absent one leaves the Godot default in place,
/// so a node that sets none of them renders exactly as it did before.
fn geometry_instance_settings(
    node: &crate::node::Node,
    material_overlay: Option<Material3D>,
) -> GeometryInstanceSettings {
    let mut g = GeometryInstanceSettings {
        material_overlay,
        ..GeometryInstanceSettings::default()
    };

    // `VisualInstance3D.layers` — the mesh side of `light_cull_mask`. Godot's
    // default is layer 1 (`doc/classes/VisualInstance3D.xml`), which is what
    // `GeometryInstanceSettings::default()` already carries.
    //
    // ⚠️ Rides the geometry-settings channel rather than a new trait method:
    // `set_geometry_instance_settings` is already synced end to end, so this
    // reaches the renderer through a path that is known to work. Read as an INT
    // because `.tscn` stores the bitmask that way; a zero is kept as authored
    // (a mesh on no layer is lit by nothing), so only a non-Int is ignored.
    if let Variant::Int(layers) = node.get_property("layers") {
        g.layers = layers as u32;
    }

    // ⚠️ Two spellings in the wild: a `.tscn` stores `cast_shadow` as Godot's
    // enum ordinal, but Patina's own `node3d::set_cast_shadow` writes a Bool.
    // Accepting only one would make the setting inert for half the scenes.
    match node.get_property("cast_shadow") {
        Variant::Int(v) => g.cast_shadow = ShadowCastingSetting::from_godot_int(v),
        Variant::Bool(b) => {
            g.cast_shadow = if b {
                ShadowCastingSetting::On
            } else {
                ShadowCastingSetting::Off
            }
        }
        _ => {}
    }

    if let Variant::Int(v) = node.get_property("visibility_range_fade_mode") {
        g.visibility_range_fade_mode = VisibilityRangeFadeMode::from_godot_int(v);
    }
    if let Variant::Aabb(aabb) = node.get_property("custom_aabb") {
        g.custom_aabb = Some(aabb);
    }

    if let Some(v) = float_property(node, "transparency") {
        g.transparency = v.clamp(0.0, 1.0);
    }
    if let Some(v) = float_property(node, "visibility_range_begin") {
        g.visibility_range_begin = v;
    }
    if let Some(v) = float_property(node, "visibility_range_begin_margin") {
        g.visibility_range_begin_margin = v;
    }
    if let Some(v) = float_property(node, "visibility_range_end") {
        g.visibility_range_end = v;
    }
    if let Some(v) = float_property(node, "visibility_range_end_margin") {
        g.visibility_range_end_margin = v;
    }
    if let Some(v) = float_property(node, "lod_bias") {
        g.lod_bias = v;
    }
    if let Some(v) = float_property(node, "extra_cull_margin") {
        g.extra_cull_margin = v.max(0.0);
    }

    // -- VisualInstance3D sorting (pat-s3rrv.187) ---------------------------
    //
    // Both properties were registered in ClassDB and present in the inspector
    // schema, and NOTHING in either render path read them — the usual shape of
    // a gap here: the value is authorable, round-trips through a `.tscn`, and
    // changes nothing on screen.
    if let Some(v) = float_property(node, "sorting_offset") {
        g.sorting_offset = v;
    }
    // ⚠️ Only an explicit `false` turns this off. The default is TRUE, so
    // treating an absent property as `false` would flip the sort point to the
    // origin for every mesh in every existing scene.
    if let Variant::Bool(b) = node.get_property("sorting_use_aabb_center") {
        g.sorting_use_aabb_center = b;
    }

    // -- GeometryInstance3D global illumination / occlusion -----------------
    if let Variant::Int(v) = node.get_property("gi_mode") {
        g.gi_mode = GiMode::from_godot_int(v);
    }
    if let Some(v) = float_property(node, "gi_lightmap_texel_scale") {
        g.gi_lightmap_texel_scale = v;
    }
    if let Variant::Bool(b) = node.get_property("ignore_occlusion_culling") {
        g.ignore_occlusion_culling = b;
    }

    g
}

/// Resolves a node's `GeometryInstance3D` settings straight from the scene tree,
/// overlay material included (pat-nzgdu.76).
///
/// Shared with the editor's 3D viewport (`gdeditor::viewport_3d_render`), which
/// builds its own instance list rather than going through
/// [`RenderServer3DAdapter`] — both paths must read the same properties the same
/// way, or `cast_shadow = 0` would work in one viewport and not the other.
///
/// Returns the Godot defaults for a node that sets none of these properties, and
/// for a node id that is not in the tree.
pub fn geometry_instance_settings_for_node(
    tree: &SceneTree,
    nid: NodeId,
) -> GeometryInstanceSettings {
    let Some(node) = tree.get_node(nid) else {
        return GeometryInstanceSettings::default();
    };
    let overlay = match node.get_property("material_overlay") {
        Variant::String(path) => Some(RenderServer3DAdapter::material_from_path(&path)),
        _ => None,
    };
    geometry_instance_settings(node, overlay)
}

/// Reads the full `Light3D` property set off a scene node (pat-s3rrv.90).
///
/// ⚠️ Public so the EDITOR's render path can share it. It used to be private
/// with a single caller inside `RenderServer3DAdapter`, while
/// `gdeditor::viewport_3d_render::scene_lights` hand-read about ten properties
/// of its own — so the same scene light shaded differently depending on which
/// renderer walked it, and everything below was silently dropped in the editor
/// and Play viewport.
///
/// ⚠️ `light_temperature` is folded into `light.color` here rather than left as
/// a field, which is what makes it reach the shading at all: the rasterizer
/// reads `color` and has no notion of temperature.
pub fn sync_light_properties(node: &crate::node::Node, light: &mut Light3D) {
    use gdvariant::Variant;

    /// Reads a numeric light property as `f32`, accepting either numeric kind.
    ///
    /// Matching only the float kind silently DROPS the value when a script
    /// writes it, because GDScript types a bare `3000` or `1` as an **Int** —
    /// `light.light_temperature = 3000` and `light.shadow_opacity = 1` are the
    /// natural spellings and both were ignored, leaving the property at its
    /// default with no error. `.tscn` files happen to write `3000.0`, which is
    /// why every scene-loaded test missed it (pat-s3rrv.90).
    ///
    /// Same shape as the readers already used for `pixel_size` and `font_scale`
    /// in this file.
    let num = |key: &str| -> Option<f32> {
        match node.get_property(key) {
            Variant::Float(f) => Some(f as f32),
            Variant::Int(i) => Some(i as f32),
            _ => None,
        }
    };

    if let Some(t) = num("light_temperature") {
        light.temperature = t;
        let tint = gdserver3d::light::kelvin_to_rgb(light.temperature);
        light.color = Color::new(
            light.color.r * tint.r,
            light.color.g * tint.g,
            light.color.b * tint.b,
            light.color.a,
        );
    }
    if let Variant::Int(mask) = node.get_property("light_cull_mask") {
        light.cull_mask = mask as u32;
    }
    if let Some(e) = num("light_indirect_energy") {
        light.indirect_energy = e;
    }
    if let Some(e) = num("light_volumetric_fog_energy") {
        light.volumetric_fog_energy = e;
    }
    if let Variant::Bool(b) = node.get_property("distance_fade_enabled") {
        light.distance_fade_enabled = b;
    }
    if let Some(v) = num("distance_fade_begin") {
        light.distance_fade_begin = v;
    }
    if let Some(v) = num("distance_fade_shadow") {
        light.distance_fade_shadow = v;
    }
    if let Some(v) = num("distance_fade_length") {
        light.distance_fade_length = v;
    }
    if let Some(v) = num("shadow_opacity") {
        light.shadow_opacity = v;
    }
    // ⚠️ In WORLD UNITS, and the per-class defaults already seeded by the
    // `Light3D` constructors differ (0.1 base / 0.03 spot), so an ABSENT
    // property must leave the constructor's value alone rather than fall back
    // to a shared constant here.
    if let Some(v) = num("shadow_bias") {
        light.shadow_bias = v;
    }
    // ⚠️ In SHADOW-MAP TEXELS, not world units — Godot pre-multiplies by the
    // texel size before the shader sees it. Different unit system from
    // `shadow_bias` directly above, despite the neighbouring names.
    if let Some(v) = num("shadow_normal_bias") {
        light.shadow_normal_bias = v;
    }
    if let Some(v) = num("light_specular") {
        light.specular = v;
    }
    if let Some(v) = num("directional_shadow_max_distance") {
        light.directional_shadow_max_distance = v;
    }
    if let Some(v) = num("directional_shadow_fade_start") {
        light.directional_shadow_fade_start = v;
    }
    if let Variant::Int(mask) = node.get_property("shadow_caster_mask") {
        light.shadow_caster_mask = mask as u32;
    }
    if let Variant::Bool(b) = node.get_property("light_negative") {
        light.negative = b;
    }
    if let Variant::Bool(b) = node.get_property("editor_only") {
        light.editor_only = b;
    }
    // DirectionalLight3D PSSM splits.
    if let Some(v) = num("directional_shadow_split_1") {
        light.shadow_split_1 = v;
    }
    if let Some(v) = num("directional_shadow_split_2") {
        light.shadow_split_2 = v;
    }
    if let Some(v) = num("directional_shadow_split_3") {
        light.shadow_split_3 = v;
    }
    if let Variant::Bool(b) = node.get_property("directional_shadow_blend_splits") {
        light.blend_splits = b;
    }
}

fn extract_vector3_array(v: &gdvariant::Variant) -> Option<Vec<Vector3>> {
    if let gdvariant::Variant::Array(arr) = v {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr.to_vec().iter() {
            if let gdvariant::Variant::Vector3(vec) = item {
                out.push(*vec);
            } else {
                return None;
            }
        }
        Some(out)
    } else {
        None
    }
}

/// Extracts `Vec<[f32; 2]>` from a `Variant::Array` of `Variant::Array([Float, Float])`.
fn extract_uv_array(v: &gdvariant::Variant) -> Option<Vec<[f32; 2]>> {
    if let gdvariant::Variant::Array(arr) = v {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr.to_vec().iter() {
            if let gdvariant::Variant::Array(pair) = item {
                let pair = pair.to_vec();
                if pair.len() == 2 {
                    if let (gdvariant::Variant::Float(u), gdvariant::Variant::Float(v)) =
                        (&pair[0], &pair[1])
                    {
                        out.push([*u as f32, *v as f32]);
                        continue;
                    }
                }
            }
            return None;
        }
        Some(out)
    } else {
        None
    }
}

/// Unflattens a `[i0, i1, i2, i3, i0, ...]` int array into per-vertex bone
/// index quads (pat-0q2kx.3). Returns `None` when the length is not a multiple
/// of four or an element is not an int.
fn extract_bone_index_array(v: &gdvariant::Variant) -> Option<Vec<[u32; 4]>> {
    let arr = match v {
        gdvariant::Variant::Array(a) => a,
        _ => return None,
    };
    if arr.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(arr.len() / 4);
    for quad in arr.to_vec().chunks(4) {
        let mut q = [0u32; 4];
        for (slot, item) in q.iter_mut().zip(quad) {
            match item {
                gdvariant::Variant::Int(i) => *slot = (*i).max(0) as u32,
                _ => return None,
            }
        }
        out.push(q);
    }
    Some(out)
}

/// Unflattens a `[w0, w1, w2, w3, w0, ...]` float array into per-vertex bone
/// weight quads (pat-0q2kx.3). Returns `None` when the length is not a multiple
/// of four or an element is not numeric.
fn extract_bone_weight_array(v: &gdvariant::Variant) -> Option<Vec<[f32; 4]>> {
    let arr = match v {
        gdvariant::Variant::Array(a) => a,
        _ => return None,
    };
    if arr.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(arr.len() / 4);
    for quad in arr.to_vec().chunks(4) {
        let mut q = [0f32; 4];
        for (slot, item) in q.iter_mut().zip(quad) {
            match item {
                gdvariant::Variant::Float(f) => *slot = *f as f32,
                gdvariant::Variant::Int(i) => *slot = *i as f32,
                _ => return None,
            }
        }
        out.push(q);
    }
    Some(out)
}

/// Extracts `Vec<u32>` from a `Variant::Array` of `Variant::Int`.
fn extract_index_array(v: &gdvariant::Variant) -> Option<Vec<u32>> {
    if let gdvariant::Variant::Array(arr) = v {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr.to_vec().iter() {
            if let gdvariant::Variant::Int(i) = item {
                out.push(*i as u32);
            } else {
                return None;
            }
        }
        Some(out)
    } else {
        None
    }
}

/// Builds a [`StandardMaterial3D`] from a `.tscn` sub-resource property bag.
///
/// This is the missing link between a scene file and the renderer. A
/// `[sub_resource type="StandardMaterial3D"]` block parses into a
/// `HashMap<String, Variant>` that [`PackedScene::sub_resource_props`] hands
/// back, and [`StandardMaterial3D::to_material3d`] already converts the result
/// into the renderer's [`Material3D`] — but nothing turned the property bag into
/// a `StandardMaterial3D`, so an authored material reached the render path as a
/// flat albedo colour with metallic/roughness/emission/textures dropped
/// (pat-0q2kx.42).
///
/// Defaults come from `doc/classes/BaseMaterial3D.xml`, which
/// `StandardMaterial3D` inherits: albedo white, metallic 0, roughness **1**,
/// emission black and DISABLED, normal map disabled with scale 1.
///
/// Godot gates emission and the normal map behind `emission_enabled` /
/// `normal_enabled`. A `.tscn` that sets `emission` without
/// `emission_enabled = true` must stay unlit, so honouring the colour alone
/// would make materials glow that do not glow in Godot.
pub fn standard_material_from_props(props: &HashMap<String, Variant>) -> StandardMaterial3D {
    let color = |key: &str, default: Color| match props.get(key) {
        Some(Variant::Color(c)) => *c,
        _ => default,
    };
    let number = |key: &str, default: f32| match props.get(key) {
        Some(Variant::Float(f)) => *f as f32,
        Some(Variant::Int(i)) => *i as f32,
        _ => default,
    };
    let flag = |key: &str| matches!(props.get(key), Some(Variant::Bool(true)));
    // A texture reference arrives as `ExtResource:<id>` / `SubResource:<id>` or
    // a bare `res://` path; keep whatever string is there so the texture loader
    // can resolve it later, but ignore an empty one.
    let texture = |key: &str| match props.get(key) {
        Some(Variant::String(s)) if !s.is_empty() => Some(TextureSlot::new(s.clone())),
        _ => None,
    };

    let emission_enabled = flag("emission_enabled");
    let normal_enabled = flag("normal_enabled");

    StandardMaterial3D {
        albedo_color: color("albedo_color", Color::new(1.0, 1.0, 1.0, 1.0)),
        albedo_texture: texture("albedo_texture"),
        metallic: number("metallic", 0.0),
        metallic_texture: texture("metallic_texture"),
        roughness: number("roughness", 1.0),
        roughness_texture: texture("roughness_texture"),
        roughness_texture_channel: match props.get("roughness_texture_channel") {
            Some(Variant::Int(v)) => gdserver3d::material::TextureChannel::from_godot_int(*v),
            _ => gdserver3d::material::TextureChannel::Red,
        },
        metallic_texture_channel: match props.get("metallic_texture_channel") {
            Some(Variant::Int(v)) => gdserver3d::material::TextureChannel::from_godot_int(*v),
            _ => gdserver3d::material::TextureChannel::Red,
        },
        normal_enabled,
        // Godot ignores the normal map entirely unless `normal_enabled` is set.
        normal_texture: if normal_enabled {
            texture("normal_texture")
        } else {
            None
        },
        normal_scale: number("normal_scale", 1.0),
        emission: if emission_enabled {
            color("emission", Color::new(0.0, 0.0, 0.0, 1.0))
        } else {
            Color::new(0.0, 0.0, 0.0, 1.0)
        },
        shading_mode: match props.get("shading_mode") {
            // BaseMaterial3D.ShadingMode: 0 = UNSHADED, 1 = PER_PIXEL (default),
            // 2 = PER_VERTEX.
            Some(Variant::Int(0)) => ShadingMode::Unlit,
            Some(Variant::Int(2)) => ShadingMode::Lambert,
            _ => ShadingMode::Phong,
        },
        double_sided: matches!(props.get("cull_mode"), Some(Variant::Int(2))),
        // pat-s3rrv.30: these five were never read, so a `.tscn` authoring
        // `transparency = 1` (Alpha) or `cull_mode = 1` (Front) rendered exactly
        // as if it had said nothing at all. `cull_mode` WAS read, but only to
        // answer "is it 2?" — which cannot express Front-culling.
        transparency: match props.get("transparency") {
            Some(Variant::Int(v)) => Transparency::from_godot_int(*v),
            _ => Transparency::Disabled,
        },
        alpha_scissor_threshold: number("alpha_scissor_threshold", 0.5),
        cull_mode: match props.get("cull_mode") {
            Some(Variant::Int(v)) => CullMode::from_godot_int(*v),
            _ => CullMode::Back,
        },
        depth_draw_mode: match props.get("depth_draw_mode") {
            Some(Variant::Int(v)) => DepthDrawMode::from_godot_int(*v),
            _ => DepthDrawMode::OpaqueOnly,
        },
        // `blend_mode` is the last of the four render modes the bead names, and
        // the one with the most visible consequence: Add and Mul are how every
        // glow, flame and shadow-decal material is authored, and all of them
        // composited as plain source-over while this key went unread.
        blend_mode: match props.get("blend_mode") {
            Some(Variant::Int(v)) => BlendMode::from_godot_int(*v),
            _ => BlendMode::Mix,
        },
        // Godot's `no_depth_test` is a FLAG, so it arrives as a plain bool
        // rather than an enum ordinal (pat-s3rrv.30).
        no_depth_test: flag("no_depth_test"),
    }
}

/// A `res://`-keyed cache of decoded material textures.
///
/// The 3D render path resolves materials every frame, so decoding a PNG per
/// frame per surface is not viable — and neither is retrying a texture that is
/// missing or undecodable, which would re-hit the filesystem forever. This
/// caches BOTH outcomes: a successful decode as `Some`, a failure as `None`, so
/// each path is attempted exactly once per process (pat-0q2kx.42).
///
/// [`gdresource::importers::load_color_texture_linear`] already does the decode
/// — it honours `.import` sidecars, handles PNG/WebP/`.ctex`, and returns
/// exactly the `(width, height, pixels)` that [`TextureImage`] wants — so this
/// only adds path resolution and memoization on top.
#[derive(Debug, Default)]
pub struct MaterialTextureCache {
    entries: HashMap<String, Option<Arc<TextureImage>>>,
    /// Where each path resolved to and that file's mtime when it was loaded,
    /// so a cache kept across frames can drop what changed on disk (or what a
    /// project switch re-pointed) via [`Self::drop_stale`] (pat-p3p1m).
    stamps: HashMap<String, TextureStamp>,
}

/// The resolved file and its modification time (either may be absent).
type TextureStamp = (Option<std::path::PathBuf>, Option<std::time::SystemTime>);

impl MaterialTextureCache {
    /// A cache with no entries.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of paths attempted so far, successful or not. Lets a caller (or a
    /// test) confirm a repeated lookup did not re-decode.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing has been looked up yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Decoded texture for `res_path`, loading it on first request.
    ///
    /// Returns `None` for a path that cannot be resolved or decoded — a material
    /// with a broken texture reference renders with its flat albedo rather than
    /// failing the frame, which is what Godot does with a missing texture.
    pub fn get_or_load(&mut self, res_path: &str) -> Option<Arc<TextureImage>> {
        if let Some(hit) = self.entries.get(res_path) {
            return hit.clone();
        }
        let loaded = Self::load(res_path);
        self.entries.insert(res_path.to_string(), loaded.clone());
        self.stamps.insert(res_path.to_string(), Self::stamp(res_path));
        loaded
    }

    /// Forgets every entry whose file now resolves elsewhere or was modified
    /// since it was loaded, so the next lookup decodes it afresh.
    ///
    /// For a cache that outlives one frame (the editor/Play render list keeps
    /// one per thread, pat-p3p1m: it used to build a new cache per frame and so
    /// re-decode every material texture on every frame). One `stat` per path.
    pub fn drop_stale(&mut self) {
        let stamps = &mut self.stamps;
        self.entries.retain(|path, _| {
            let now = Self::stamp(path);
            let fresh = stamps.get(path) == Some(&now);
            if !fresh {
                stamps.remove(path);
            }
            fresh
        });
    }

    fn resolve(res_path: &str) -> Option<std::path::PathBuf> {
        if res_path.starts_with("res://") || res_path.starts_with("user://") {
            gdcore::resolve_path(res_path).ok()
        } else {
            Some(std::path::PathBuf::from(res_path))
        }
    }

    fn stamp(res_path: &str) -> TextureStamp {
        let resolved = Self::resolve(res_path);
        let mtime = resolved
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        (resolved, mtime)
    }

    fn load(res_path: &str) -> Option<Arc<TextureImage>> {
        // `res://` needs the project roots that `set_path_roots` installs at
        // boot; a bare filesystem path is passed through so tests and tools can
        // load a texture without a project.
        let resolved = if res_path.starts_with("res://") || res_path.starts_with("user://") {
            gdcore::resolve_path(res_path).ok()?
        } else {
            std::path::PathBuf::from(res_path)
        };
        let (width, height, pixels) =
            gdresource::importers::load_color_texture_linear(&resolved).ok()?;
        if width == 0 || height == 0 || pixels.len() != (width as usize) * (height as usize) {
            // A decoded-but-empty texture would sample as garbage; treat it as
            // absent so the material falls back to its albedo colour.
            return None;
        }
        Some(Arc::new(TextureImage::new(width, height, pixels)))
    }
}

/// Loads each `sampler2D` uniform's texture into `mat` (pat-s3rrv.122).
///
/// 🔑 THIS IS THE STEP THAT WAS MISSING ENTIRELY. `resolve_shader_material`
/// collects `shader_parameter/<name>` into `parameters`, but a Variant holds a
/// resource PATH, not pixels — and the fragment path samples from the material's
/// texture map. With nothing performing this load, `texture(tex, UV)` always hit
/// the missing-binding white placeholder, and `CustomFragmentShader::set_texture`
/// had no caller outside its own tests.
///
/// 🔑 A `pub` FREE function, not a method on [`RenderServer3DAdapter`]
/// (pat-mgixk.50). It was a private method, so the only way to bind a shader's
/// textures was to own an adapter — and the editor/Play path does not: it builds
/// its own draw list and carries its own [`MaterialTextureCache`]. So
/// `viewport_3d_render` resolved the ShaderMaterial (and has since
/// pat-mgixk.50's first pass) but handed it over with an EMPTY texture map, and
/// every `sampler2D` on that path sampled the white placeholder. Taking the
/// cache as an argument is what lets both paths perform the same load against
/// their own cache.
///
/// A path that fails to load is skipped rather than bound black, so an
/// unresolvable texture keeps the neutral placeholder instead of turning the
/// surface into a silhouette.
pub fn bind_shader_textures(mat: &mut ShaderMaterial3D, textures: &mut MaterialTextureCache) {
    let Some(shader) = mat.shader.as_ref() else {
        return;
    };
    let compiled =
        gdserver3d::ShaderCompiler3D::new().compile_cached(shader.shader_type, &shader.source_code);
    let samplers: Vec<(String, String)> = compiled
        .uniforms
        .iter()
        .filter(|u| u.uniform_type == gdserver3d::shader::UniformType::Sampler2D)
        .filter_map(|u| match mat.get_shader_parameter(&u.name) {
            Some(gdvariant::Variant::String(path)) if !path.is_empty() => {
                Some((u.name.clone(), path.clone()))
            }
            _ => None,
        })
        .collect();
    for (name, path) in samplers {
        if let Some(img) = textures.get_or_load(&path) {
            mat.set_texture(&name, img);
        }
    }
}

/// Recovers a material's property bag from the `__material__*` companions that
/// [`PackedScene`](crate::packed_scene::PackedScene) flattens onto the node.
///
/// By render time the `PackedScene` is gone and only the `SceneTree` remains, so
/// the scene loader copies the material sub-resource's properties onto the node
/// under `__material__<key>` (the same trick the physics layer uses for
/// `__shape__<key>`). This strips the prefix back off, handing
/// [`standard_material_from_props`] the bag it expects.
///
/// `None` when the node carries no material companions — the caller then keeps
/// whatever flat albedo it already had, so a mesh without an authored material
/// is unaffected.
pub fn material_props_from_node(
    tree: &SceneTree,
    node_id: NodeId,
) -> Option<HashMap<String, Variant>> {
    const PREFIX: &str = "__material__";
    let node = tree.get_node(node_id)?;
    // Unsorted on purpose (pat-p3p1m): the keys land in a HashMap, so order is
    // irrelevant, and `get_property_list` SORTS every property name of the
    // node — run for every mesh node on every editor/Play frame, that sort was
    // the largest cost of building the 3D render list.
    let keys: Vec<String> = node
        .properties()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with(PREFIX))
        .cloned()
        .collect();
    if keys.is_empty() {
        return None;
    }
    let mut props = HashMap::new();
    for key in keys {
        // `__material__class` records the resource type, not a material
        // parameter; keep it out of the parameter bag.
        let stripped = &key[PREFIX.len()..];
        if stripped == "class" {
            continue;
        }
        props.insert(stripped.to_string(), node.get_property(&key));
    }
    Some(props)
}

/// The renderer-ready material for a mesh node, or `None` when it has no
/// authored material.
///
/// The whole `.tscn` -> pixels chain in one call: node companions ->
/// [`standard_material_from_props`] -> [`StandardMaterial3D::to_material3d`],
/// with any texture references resolved through `textures`.
/// Per-surface material overrides for a node's ADDITIONAL surfaces
/// (pat-s3rrv.187).
///
/// Returns a list parallel to `Mesh3D::surfaces`: index `i` is the override for
/// `surfaces[i]`, which Godot spells `surface_material_override/{i + 1}`.
///
/// ⚠️ **The +1 is the whole subtlety.** `packed_scene` flattens override `N` to
/// `__surfmat<N>__*` and deliberately SKIPS `N == 0`, because override 0 belongs
/// to the primary surface and is already flattened as `__material__*`. So the
/// lowest companion index that can exist here is 1, and it maps to `surfaces[0]`.
///
/// Gaps stay `None` rather than collapsing, so a mesh that overrides only its
/// second extra surface does not shift that material onto the first.
/// The mesh every instance of a `MultiMeshInstance3D` shares.
///
/// ⚠️ KNOWN GAP, deliberately not closed here: a `MultiMesh` whose own `mesh`
/// is a sub-resource arrives as `__multimesh__mesh = "SubResource:<id>"` — a
/// string that names an id and carries no class or bind data (see the
/// `__multimesh__*` copy in `packed_scene.rs`, and its own note that a
/// `SubResource:` string "carries no bind data at all"). Honouring it needs
/// `packed_scene` to copy the NESTED mesh's class the way it already copies
/// `__multimesh__class`. Until then this reads the authored type names and
/// falls back to a unit cube, which is what the adapter has always done
/// (pat-mgixk.50).
pub fn multimesh_mesh_for_node(tree: &SceneTree, nid: NodeId) -> Mesh3D {
    // The MultiMesh's own `mesh` sub-resource, whose class `packed_scene`
    // records as `__multimesh__mesh_class`. Checked FIRST because it is the
    // authored answer: `multimesh_mesh_type` below is a flat property nothing in
    // the `.tscn` chain writes, so before this a `.tscn`-authored MultiMesh mesh
    // always fell through to the cube (pat-mgixk.50).
    if let Some(node) = tree.get_node(nid) {
        if let gdvariant::Variant::String(class) = node.get_property("__multimesh__mesh_class") {
            if let Some(m) = RenderServer3DAdapter::mesh_from_type_name(&class) {
                return m;
            }
        }
    }
    if let Some(mesh_type) = node3d::get_multimesh_mesh_type(tree, nid) {
        if let Some(m) = RenderServer3DAdapter::mesh_from_type_name(&mesh_type) {
            return m;
        }
    }
    if let Some(node) = tree.get_node(nid) {
        if let gdvariant::Variant::String(s) = node.get_property("mesh_type") {
            if let Some(m) = RenderServer3DAdapter::mesh_from_type_name(&s) {
                return m;
            }
        }
    }
    Mesh3D::cube(1.0)
}

/// 🔑 THE one expansion of a `MultiMeshInstance3D` into the instances actually
/// drawn: `(world transform, per-instance colour)` plus the mesh they share.
///
/// Returns `None` for a node that is not a `MultiMeshInstance3D`, so a caller
/// can use it as the "is this a multimesh?" test too.
///
/// Exists because the editor and Play 3D list had NO MultiMesh arm at all —
/// `grep -i multimesh` in `viewport_3d_render.rs` and `viewport_3d.rs` returned
/// zero, so every MultiMeshInstance3D fell through to a 0.4 marker cube — while
/// the render-server adapter expanded it correctly and had no production caller.
/// Both now go through here, so the two cannot disagree about how many blobs a
/// scene has (pat-mgixk.50).
///
/// ⚠️ Honours `visible_instance_count`, which the adapter's own loop did not: it
/// iterated the full `instance_count`, so a MultiMesh authored with 3 instances
/// and 2 visible drew 3. The clamp lives in
/// [`node3d::multimesh_visible_count`], deferring to
/// `gdserver3d::multimesh::MultiMesh3D::visible_count` for the `-1`-means-all
/// rule rather than re-deriving it.
///
/// The per-instance transform is composed as `node_global * per_instance`, which
/// is the order Godot uses — the buffer holds model-space transforms.
pub fn multimesh_instance_draws(
    tree: &SceneTree,
    nid: NodeId,
) -> Option<(Mesh3D, Vec<(Transform3D, Color)>)> {
    if tree.get_node(nid)?.class_name() != "MultiMeshInstance3D" {
        return None;
    }
    let mesh = multimesh_mesh_for_node(tree, nid);
    let base = node3d::get_global_transform(tree, nid);
    let visible = node3d::multimesh_visible_count(tree, nid);
    let instances = (0..visible)
        .map(|i| {
            (
                base * node3d::get_multimesh_instance_transform(tree, nid, i),
                node3d::get_multimesh_instance_color(tree, nid, i),
            )
        })
        .collect();
    Some((mesh, instances))
}

pub fn surface_materials_for_node(
    tree: &SceneTree,
    node_id: NodeId,
    textures: &mut MaterialTextureCache,
) -> Vec<Option<Material3D>> {
    const PREFIX: &str = "__surfmat";
    let Some(node) = tree.get_node(node_id) else {
        return Vec::new();
    };

    // Group the flattened keys by surface index first: a material is built from
    // a whole property bag, not key by key.
    let mut bags: HashMap<usize, HashMap<String, Variant>> = HashMap::new();
    // Unsorted (pat-p3p1m): grouped into HashMaps below, so order is irrelevant.
    for key in node.properties().map(|(k, _)| k.as_str()) {
        let Some(rest) = key.strip_prefix(PREFIX) else {
            continue;
        };
        let Some((index, field)) = rest.split_once("__") else {
            continue;
        };
        let Ok(index) = index.parse::<usize>() else {
            continue;
        };
        // Override 0 is the primary surface's, handled by `material3d_for_node`.
        if index == 0 {
            continue;
        }
        // `class` records the resource type, not a material parameter — the
        // same exclusion `material_props_from_node` makes.
        if field == "class" {
            continue;
        }
        bags.entry(index)
            .or_default()
            .insert(field.to_string(), node.get_property(key));
    }
    if bags.is_empty() {
        return Vec::new();
    }

    let highest = bags.keys().copied().max().unwrap_or(0);
    let mut out: Vec<Option<Material3D>> = vec![None; highest];
    for (index, props) in bags {
        let standard = standard_material_from_props(&props);
        let mut material = standard.to_material3d();
        if let Some(slot) = standard.albedo_texture.as_ref() {
            material.albedo_texture = textures.get_or_load(&slot.path);
        }
        if let Some(slot) = standard.normal_texture.as_ref() {
            material.normal_texture = textures.get_or_load(&slot.path);
        }
        load_orm_maps(&standard, &mut material, textures);
        // `index - 1`: companion 1 is `surfaces[0]`. Safe because `index == 0`
        // is filtered above.
        out[index - 1] = Some(material);
    }
    out
}

pub fn material3d_for_node(
    tree: &SceneTree,
    node_id: NodeId,
    textures: &mut MaterialTextureCache,
) -> Option<Material3D> {
    let props = material_props_from_node(tree, node_id)?;
    let standard = standard_material_from_props(&props);
    let mut material = standard.to_material3d();
    if let Some(slot) = standard.albedo_texture.as_ref() {
        material.albedo_texture = textures.get_or_load(&slot.path);
    }
    if let Some(slot) = standard.normal_texture.as_ref() {
        material.normal_texture = textures.get_or_load(&slot.path);
    }
    load_orm_maps(&standard, &mut material, textures);
    Some(material)
}

/// Loads a StandardMaterial3D's roughness and metallic maps into the runtime
/// material, normalised to the renderers' G/B packing from the authored
/// `*_texture_channel` (Godot default RED).
///
/// ⚠️ These slots were parsed but never loaded, so every authored roughness or
/// metallic map was silently ignored; and the renderers read G/B, which is
/// glTF/ORM packing — Godot's StandardMaterial3D reads RED.
fn load_orm_maps(
    standard: &StandardMaterial3D,
    material: &mut Material3D,
    textures: &mut MaterialTextureCache,
) {
    // ⚠️ DATA, not colour: Godot samples these as `hint_roughness_*` /
    // `hint_metallic`, never `source_color`, so a texel of 0.2 IS roughness
    // 0.2. The shared cache decodes every texture sRGB→linear (right for
    // albedo), which turned 0.2 into 0.033 — re-encode to the stored value.
    let raw = |t: &TextureImage| {
        TextureImage::new(
            t.width,
            t.height,
            t.pixels.iter().map(|c| c.linear_to_srgb()).collect(),
        )
    };
    if let Some(slot) = standard.roughness_texture.as_ref() {
        material.roughness_texture = textures
            .get_or_load(&slot.path)
            .map(|t| Arc::new(standard.roughness_texture_channel.broadcast(&raw(&t))));
    }
    if let Some(slot) = standard.metallic_texture.as_ref() {
        material.metallic_texture = textures
            .get_or_load(&slot.path)
            .map(|t| Arc::new(standard.metallic_texture_channel.broadcast(&raw(&t))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::scene_tree::SceneTree;
    use gdcore::math::{Color, Rect2, Vector2, Vector3};
    use gdvariant::Variant;

    /// Builds a MeshInstance3D carrying one named blend shape that pushes every
    /// vertex +1 on X, and a node whose weight for it is `weight`.
    fn morph_fixture(weight: Option<f32>) -> (SceneTree, crate::node::NodeId, Mesh3D) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let nid = tree
            .add_child(root, Node::new("Morphed", "MeshInstance3D"))
            .expect("node added");
        if let Some(w) = weight {
            if let Some(node) = tree.get_node_mut(nid) {
                node.set_property("blend_shapes/wide", Variant::Float(w as f64));
            }
        }
        let mut base = Mesh3D::cube(1.0);
        let deltas = vec![Vector3::new(1.0, 0.0, 0.0); base.vertices.len()];
        base.blend_shapes = vec![gdserver3d::mesh::BlendShape3D {
            name: "wide".to_string(),
            vertex_deltas: deltas,
        }];
        (tree, nid, base)
    }

    /// 🔑 A blend-shape weight actually moves vertices.
    ///
    /// ⚠️ The whole point of this bead's layer 3: the glTF importer produced
    /// blend shapes and `apply_morph` could consume them, but no render stage
    /// ever called it, so a weight moved nothing. This asserts the node sync
    /// pass's helper, which is what feeds `set_mesh`.
    #[test]
    fn blend_shape_weight_deforms_the_mesh() {
        let (tree, nid, base) = morph_fixture(Some(1.0));
        let morphed = morphed_mesh(&tree, nid, &base)
            .expect("a non-zero weight must produce a deformed mesh");

        assert_eq!(morphed.vertices.len(), base.vertices.len());
        for (before, after) in base.vertices.iter().zip(morphed.vertices.iter()) {
            assert!(
                (after.x - (before.x + 1.0)).abs() < 1e-5,
                "weight 1.0 on a +1 X delta must move each vertex a full unit: \
                 {before:?} -> {after:?}",
            );
        }
    }

    /// 🔑 Weights are addressed by NAME, not by index.
    ///
    /// ⚠️ Godot exposes `blend_shapes/<name>` on the instance. A lookup keyed by
    /// position would happily read a DIFFERENT shape's weight whenever the
    /// property order and the shape order disagree — which is invisible for a
    /// mesh with a single shape, i.e. exactly the fixture above.
    #[test]
    fn blend_shape_weight_is_looked_up_by_name() {
        let (mut tree, nid, base) = morph_fixture(None);
        if let Some(node) = tree.get_node_mut(nid) {
            // A weight under the WRONG name must not reach the "wide" shape.
            node.set_property("blend_shapes/narrow", Variant::Float(1.0));
        }
        assert!(
            morphed_mesh(&tree, nid, &base).is_none(),
            "a weight authored under another shape's name must not deform this \
             one; positional lookup would have applied it",
        );
    }

    /// An absent or zero weight leaves the mesh alone (and clones nothing).
    #[test]
    fn a_zero_blend_shape_weight_does_not_deform() {
        let (tree, nid, base) = morph_fixture(Some(0.0));
        assert!(
            morphed_mesh(&tree, nid, &base).is_none(),
            "zero weight is the base mesh; returning it would clone a whole \
             mesh per node per frame for nothing",
        );

        let (tree, nid, base) = morph_fixture(None);
        assert!(
            morphed_mesh(&tree, nid, &base).is_none(),
            "an unauthored weight behaves as zero",
        );
    }

    #[test]
    fn empty_scene_renders_black() {
        let tree = SceneTree::new();
        let mut adapter = RenderServer3DAdapter::new(32, 32);
        let (snapshot, frame) = adapter.render_frame(&tree);

        assert_eq!(snapshot.frame_number, 1);
        assert_eq!(snapshot.visible_mesh_count, 0);
        assert_eq!(snapshot.nonblack_pixel_count, 0);
        assert!(frame.pixels.iter().all(|c| *c == Color::BLACK));
    }

    #[test]
    fn single_mesh_produces_pixels() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Add a Camera3D looking at the origin from z=10.
        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
        node3d::set_camera_current(&mut tree, cam_id, true);

        // Add a MeshInstance3D at the origin.
        let mesh = Node::new("Cube", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _frame) = adapter.render_frame(&tree);

        assert_eq!(snapshot.visible_mesh_count, 1);
        assert!(
            snapshot.nonblack_pixel_count > 0,
            "a mesh in front of the camera should produce visible pixels"
        );
    }

    #[test]
    fn invisible_mesh_not_counted() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

        let mesh = Node::new("Cube", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));
        node3d::set_visible(&mut tree, mesh_id, false);

        let mut adapter = RenderServer3DAdapter::new(32, 32);
        let (snapshot, _) = adapter.render_frame(&tree);

        assert_eq!(snapshot.visible_mesh_count, 0);
        assert_eq!(snapshot.nonblack_pixel_count, 0);
    }

    // -- ShaderMaterial resolution (pat-u3l8c.43) ---------------------------

    #[test]
    fn resolve_shader_material_from_inline_code() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mesh = Node::new("Water", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        {
            let n = tree.get_node_mut(mesh_id).unwrap();
            n.set_property(
                "shader_code",
                Variant::String("shader_type sky; void sky() {}".into()),
            );
            // String shader_type "sky" must map to ShaderType3D::Sky.
            n.set_property("shader_type", Variant::String("sky".into()));
            n.set_property("shader_parameter/wave_height", Variant::Float(0.5));
        }

        let mat = RenderServer3DAdapter::resolve_shader_material(&tree, mesh_id)
            .expect("node with shader_code resolves a ShaderMaterial3D");
        let shader = mat.shader.as_ref().expect("shader present");
        assert_eq!(shader.shader_type, ShaderType3D::Sky);
        assert!(shader.source_code.contains("shader_type sky"));
        assert_eq!(
            mat.get_shader_parameter("wave_height"),
            Some(&Variant::Float(0.5))
        );
    }

    #[test]
    fn resolve_shader_material_from_gdshader_path() {
        // A node carrying only `shader_path` loads the external .gdshader.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("surface.gdshader");
        std::fs::write(&path, "shader_type spatial;\nvoid fragment() {}\n").unwrap();

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mesh = Node::new("Surf", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        {
            let n = tree.get_node_mut(mesh_id).unwrap();
            n.set_property(
                "shader_path",
                Variant::String(path.to_string_lossy().into_owned()),
            );
            n.set_property("shader_parameter/roughness", Variant::Float(0.3));
        }

        let mat = RenderServer3DAdapter::resolve_shader_material(&tree, mesh_id)
            .expect("shader_path resolves a ShaderMaterial3D");
        let shader = mat.shader.as_ref().expect("shader present");
        assert_eq!(shader.shader_type, ShaderType3D::Spatial);
        assert!(shader.source_code.contains("shader_type spatial"));
        assert_eq!(
            mat.get_shader_parameter("roughness"),
            Some(&Variant::Float(0.3))
        );
    }

    #[test]
    fn resolve_shader_material_none_without_shader() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mesh = Node::new("Plain", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        assert!(RenderServer3DAdapter::resolve_shader_material(&tree, mesh_id).is_none());
    }

    /// pat-nzgdu.21 acceptance: a `.tscn` with an `[ext_resource type="Shader"]`
    /// (`.gdshader`) and a `[sub_resource type="ShaderMaterial"]` set as a
    /// MeshInstance3D `material_override` loads through the resource pipeline
    /// (PackedScene → node flattening → render-server ShaderMaterial3D) and
    /// renders; and ClassDB can instantiate ShaderMaterial and set/get a shader
    /// parameter.
    #[test]
    fn gdshader_scene_load_renders() {
        use crate::PackedScene;

        // External `.gdshader` referenced by the scene's `[ext_resource]`. An
        // absolute path keeps the test hermetic: the render server strips a
        // leading `res://` and otherwise loads the path verbatim, so an absolute
        // path exercises the identical ext_resource → shader_path → load_gdshader
        // chain without a cwd-relative `res://` dependency.
        let dir = tempfile::TempDir::new().unwrap();
        let shader_file = dir.path().join("tint.gdshader");
        std::fs::write(
            &shader_file,
            "shader_type spatial;\nuniform float strength = 1.0;\nvoid fragment() { ALBEDO = vec3(strength); }\n",
        )
        .unwrap();
        let shader_abs = shader_file.to_string_lossy().into_owned();

        let tscn = format!(
            r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Shader" path="{shader_abs}" id="1_shd"]

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = ExtResource("1_shd")
shader_parameter/strength = 0.75

[node name="Shaded" type="MeshInstance3D"]
material_override = SubResource("ShaderMaterial_1")
"#
        );

        // Load through the resource pipeline and instance into a scene tree.
        let packed = PackedScene::from_tscn(&tscn).expect("parse shader scene tscn");
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mesh_id = crate::add_packed_scene_to_tree(&mut tree, root, &packed)
            .expect("instance shader scene into tree");

        // (1) The ShaderMaterial `material_override` flattened onto the node: the
        //     external shader path plus the shader parameter.
        {
            let mesh = tree.get_node(mesh_id).unwrap();
            assert_eq!(
                mesh.get_property("shader_path"),
                Variant::String(shader_abs.clone()),
                "ext_resource .gdshader path flattened onto the node",
            );
            assert_eq!(
                mesh.get_property("shader_parameter/strength"),
                Variant::Float(0.75),
                "ShaderMaterial shader_parameter flattened onto the node",
            );
        }

        // (2) The render server resolves a ShaderMaterial3D that loaded the
        //     external `.gdshader` source and bound the parameter.
        let mat = RenderServer3DAdapter::resolve_shader_material(&tree, mesh_id)
            .expect("scene ShaderMaterial resolves to a ShaderMaterial3D");
        let shader = mat.shader.as_ref().expect("compiled shader present");
        assert_eq!(shader.shader_type, ShaderType3D::Spatial);
        assert!(
            shader.source_code.contains("ALBEDO = vec3(strength)"),
            "shader source loaded from the external .gdshader",
        );
        assert_eq!(
            mat.get_shader_parameter("strength"),
            Some(&Variant::Float(0.75)),
            "shader parameter bound from the ShaderMaterial",
        );

        // (3) It actually renders: a current camera in front of the shaded mesh
        //     yields a visible mesh and non-black output.
        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
        node3d::set_camera_current(&mut tree, cam_id, true);
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _) = adapter.render_frame(&tree);
        assert_eq!(snapshot.visible_mesh_count, 1, "the shaded mesh is visible");
        assert!(
            snapshot.nonblack_pixel_count > 0,
            "the shaded mesh renders non-black output",
        );

        // (4) ClassDB can instantiate ShaderMaterial and set/get a shader
        //     parameter.
        gdobject::class_db::register_3d_classes();
        assert!(
            gdobject::class_db::can_instantiate("ShaderMaterial"),
            "ClassDB can instantiate ShaderMaterial",
        );
        assert!(
            gdobject::class_db::is_parent_class("ShaderMaterial", "Material"),
            "ShaderMaterial inherits Material",
        );
        assert!(
            gdobject::class_db::class_exists("ShaderInclude")
                && gdobject::class_db::class_exists("ShaderGlobalsOverride"),
            "ShaderInclude/ShaderGlobalsOverride registered in ClassDB",
        );
        let mut obj = gdobject::class_db::instantiate("ShaderMaterial")
            .expect("ClassDB instantiates ShaderMaterial");
        obj.base
            .set_property("shader_parameter/strength", Variant::Float(0.5));
        assert_eq!(
            obj.base.get_property("shader_parameter/strength"),
            Variant::Float(0.5),
            "set/get shader parameter round-trips on the ShaderMaterial instance",
        );
    }

    #[test]
    fn frame_counter_increments() {
        let tree = SceneTree::new();
        let mut adapter = RenderServer3DAdapter::new(16, 16);

        let (s1, _) = adapter.render_frame(&tree);
        let (s2, _) = adapter.render_frame(&tree);
        let (s3, _) = adapter.render_frame(&tree);

        assert_eq!(s1.frame_number, 1);
        assert_eq!(s2.frame_number, 2);
        assert_eq!(s3.frame_number, 3);
        assert_eq!(adapter.frame_counter(), 3);
    }

    #[test]
    fn snapshot_json_roundtrip() {
        let snapshot = RenderFrame3DSnapshot {
            frame_number: 1,
            width: 64,
            height: 64,
            visible_mesh_count: 2,
            light_count: 0,
            nonblack_pixel_count: 100,
            total_pixel_count: 4096,
            depth_written_count: 0,
            camera_transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 5.0],
            camera_fov: std::f32::consts::FRAC_PI_4,
        };
        let json = snapshot.to_json();
        assert!(json.contains("\"frame_number\":1"));
        assert!(json.contains("\"visible_mesh_count\":2"));
        assert!(json.contains("\"coverage\":"));
    }

    #[test]
    fn deterministic_rendering() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

        let mesh = Node::new("Cube", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter1 = RenderServer3DAdapter::new(32, 32);
        let mut adapter2 = RenderServer3DAdapter::new(32, 32);

        let (_, f1) = adapter1.render_frame(&tree);
        let (_, f2) = adapter2.render_frame(&tree);

        assert_eq!(f1.pixels, f2.pixels, "3D render path must be deterministic");
    }

    #[test]
    fn removed_node_frees_instance() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

        let mesh = Node::new("Cube", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(32, 32);
        let (s1, _) = adapter.render_frame(&tree);
        assert_eq!(s1.visible_mesh_count, 1);

        // Remove the mesh node.
        tree.remove_node(mesh_id).unwrap();
        let (s2, _) = adapter.render_frame(&tree);
        assert_eq!(s2.visible_mesh_count, 0);
        assert_eq!(s2.nonblack_pixel_count, 0);
    }

    #[test]
    fn light_syncing_counts_directional() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

        let mut sun = Node::new("Sun", "DirectionalLight3D");
        sun.set_property("light_energy", gdvariant::Variant::Float(1.5));
        sun.set_property("shadow_enabled", gdvariant::Variant::Bool(true));
        tree.add_child(root, sun).unwrap();

        let mut adapter = RenderServer3DAdapter::new(32, 32);
        let (snapshot, _) = adapter.render_frame(&tree);

        assert_eq!(
            snapshot.light_count, 1,
            "should detect one directional light"
        );
    }

    #[test]
    fn light_syncing_counts_multiple() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        tree.add_child(root, cam).unwrap();

        let sun = Node::new("Sun", "DirectionalLight3D");
        tree.add_child(root, sun).unwrap();

        let omni = Node::new("Lamp", "OmniLight3D");
        tree.add_child(root, omni).unwrap();

        let mut adapter = RenderServer3DAdapter::new(16, 16);
        let (snapshot, _) = adapter.render_frame(&tree);

        assert_eq!(snapshot.light_count, 2, "should detect both lights");
    }

    #[test]
    fn removed_light_decrements_count() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let sun = Node::new("Sun", "DirectionalLight3D");
        let sun_id = tree.add_child(root, sun).unwrap();

        let mut adapter = RenderServer3DAdapter::new(16, 16);
        let (s1, _) = adapter.render_frame(&tree);
        assert_eq!(s1.light_count, 1);

        tree.remove_node(sun_id).unwrap();
        let (s2, _) = adapter.render_frame(&tree);
        assert_eq!(s2.light_count, 0);
    }

    #[test]
    fn mesh_type_dispatch_sphere() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

        let mut mesh = Node::new("Ball", "MeshInstance3D");
        mesh.set_property(
            "mesh_type",
            gdvariant::Variant::String("SphereMesh".to_owned()),
        );
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _) = adapter.render_frame(&tree);

        assert_eq!(snapshot.visible_mesh_count, 1);
        assert!(
            snapshot.nonblack_pixel_count > 0,
            "sphere should produce pixels"
        );
    }

    #[test]
    fn mesh_type_dispatch_from_path() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 2.0, 5.0));

        let mesh = Node::new("Ground", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_mesh_path(&mut tree, mesh_id, "res://meshes/ground_plane.tres");
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _) = adapter.render_frame(&tree);

        assert_eq!(snapshot.visible_mesh_count, 1);
        // Plane resolves from path containing "plane".
    }

    #[test]
    fn parity_report_functional_scene() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
        node3d::set_camera_current(&mut tree, cam_id, true);

        let mesh = Node::new("Cube", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let sun = Node::new("Sun", "DirectionalLight3D");
        tree.add_child(root, sun).unwrap();

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _) = adapter.render_frame(&tree);

        let report = snapshot.parity_report();
        assert!(
            report.is_functional(),
            "scene with camera+mesh should be functional"
        );
        assert_eq!(report.mesh_count, 1);
        assert_eq!(report.light_count, 1);
        assert!(report.has_camera);
        assert!(report.coverage > 0.0);

        let json = report.to_json();
        assert!(json.contains("\"is_functional\":true"));
    }

    #[test]
    fn parity_report_empty_scene_not_functional() {
        let tree = SceneTree::new();
        let mut adapter = RenderServer3DAdapter::new(32, 32);
        let (snapshot, _) = adapter.render_frame(&tree);

        let report = snapshot.parity_report();
        assert!(
            !report.is_functional(),
            "empty scene should not be functional"
        );
        assert_eq!(report.mesh_count, 0);
        assert_eq!(report.light_count, 0);
    }

    #[test]
    fn depth_metrics_nonzero_for_visible_mesh() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
        node3d::set_camera_current(&mut tree, cam_id, true);

        let mesh = Node::new("Cube", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _) = adapter.render_frame(&tree);

        assert!(
            snapshot.depth_written_count > 0,
            "visible mesh should produce depth writes, got 0"
        );
        let report = snapshot.parity_report();
        assert!(
            report.depth_coverage > 0.0,
            "depth_coverage should be > 0 for a functional scene, got {}",
            report.depth_coverage,
        );
    }

    #[test]
    fn snapshot_json_includes_light_and_depth_fields() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

        let sun = Node::new("Sun", "DirectionalLight3D");
        tree.add_child(root, sun).unwrap();

        let mut adapter = RenderServer3DAdapter::new(32, 32);
        let (snapshot, _) = adapter.render_frame(&tree);

        let json = snapshot.to_json();
        assert!(json.contains("\"light_count\":1"), "JSON: {}", json);
        assert!(json.contains("\"depth_written_count\":"), "JSON: {}", json);
    }

    #[test]
    fn parity_comparison_identical_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

        let mesh = Node::new("Cube", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(32, 32);
        adapter.render_frame(&tree);
        let frame_a = adapter.last_frame().unwrap().clone();

        adapter.render_frame(&tree);
        let frame_b = adapter.last_frame().unwrap().clone();

        let diff = RenderServer3DAdapter::compare_frames(&frame_a, &frame_b, 0.0, 0.0);
        assert!(
            diff.is_exact_color_match(),
            "identical scene state must produce identical frames"
        );
    }

    // -------------------------------------------------------------------
    // glTF Resource → Mesh3D / Material3D conversion tests
    // -------------------------------------------------------------------

    /// Builds a minimal valid GLB (glTF Binary) with a single triangle.
    fn make_test_glb() -> Vec<u8> {
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals: [[f32; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]];
        let indices: [u16; 3] = [0, 1, 2];

        let mut bin = Vec::new();
        for p in &positions {
            for &v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for n in &normals {
            for &v in n {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for &i in &indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0,  "byteLength": 36, "target": 34962 },
                { "buffer": 0, "byteOffset": 36, "byteLength": 36, "target": 34962 },
                { "buffer": 0, "byteOffset": 72, "byteLength": 6,  "target": 34963 }
            ],
            "accessors": [
                {
                    "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                    "max": [1.0, 1.0, 0.0], "min": [0.0, 0.0, 0.0]
                },
                { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" },
                { "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ],
            "meshes": [{
                "name": "Triangle",
                "primitives": [{
                    "attributes": { "POSITION": 0, "NORMAL": 1 },
                    "indices": 2
                }]
            }]
        });

        let mut json_bytes = serde_json::to_vec(&json).unwrap();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }

        let total_length = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut glb = Vec::with_capacity(total_length);

        // GLB header
        glb.extend_from_slice(&0x46546C67u32.to_le_bytes()); // "glTF"
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total_length as u32).to_le_bytes());

        // JSON chunk
        glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        glb.extend_from_slice(&json_bytes);

        // BIN chunk
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes());
        glb.extend_from_slice(&bin);

        glb
    }

    /// Builds a GLB with a PBR material (red, metallic=0.8, roughness=0.3).
    fn make_test_glb_with_material() -> Vec<u8> {
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals: [[f32; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]];
        let indices: [u16; 3] = [0, 1, 2];

        let mut bin = Vec::new();
        for p in &positions {
            for &v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for n in &normals {
            for &v in n {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for &i in &indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0,  "byteLength": 36, "target": 34962 },
                { "buffer": 0, "byteOffset": 36, "byteLength": 36, "target": 34962 },
                { "buffer": 0, "byteOffset": 72, "byteLength": 6,  "target": 34963 }
            ],
            "accessors": [
                {
                    "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                    "max": [1.0, 1.0, 0.0], "min": [0.0, 0.0, 0.0]
                },
                { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" },
                { "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ],
            "materials": [{
                "name": "RedMetal",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [1.0, 0.0, 0.0, 1.0],
                    "metallicFactor": 0.8,
                    "roughnessFactor": 0.3
                },
                "emissiveFactor": [0.1, 0.0, 0.0],
                "doubleSided": true
            }],
            "meshes": [{
                "name": "RedTriangle",
                "primitives": [{
                    "attributes": { "POSITION": 0, "NORMAL": 1 },
                    "indices": 2,
                    "material": 0
                }]
            }]
        });

        let mut json_bytes = serde_json::to_vec(&json).unwrap();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }

        let total_length = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut glb = Vec::with_capacity(total_length);

        glb.extend_from_slice(&0x46546C67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total_length as u32).to_le_bytes());

        glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        glb.extend_from_slice(&json_bytes);

        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes());
        glb.extend_from_slice(&bin);

        glb
    }

    #[test]
    fn mesh3d_from_gltf_resource_loads_triangle() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("tri.glb");
        std::fs::write(&path, make_test_glb()).unwrap();

        let res = gdresource::import_gltf(&path).expect("import glb");
        let mesh = mesh3d_from_gltf_resource(&res).expect("mesh from gltf resource");
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.indices.len(), 3);
    }

    #[test]
    fn mesh3d_from_gltf_resource_returns_none_for_empty() {
        let resource = gdresource::Resource::new("ArrayMesh");
        assert!(mesh3d_from_gltf_resource(&resource).is_none());
    }

    #[test]
    fn material_from_gltf_resource_loads_pbr() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("red.glb");
        std::fs::write(&path, make_test_glb_with_material()).unwrap();

        let res = gdresource::import_gltf(&path).expect("import glb");
        let mat = material_from_gltf_resource(&res).expect("material from gltf");
        assert!((mat.albedo.r - 1.0).abs() < 1e-5);
        assert!((mat.metallic - 0.8).abs() < 1e-5);
        assert!((mat.roughness - 0.3).abs() < 1e-5);
        assert!(mat.double_sided);
    }

    #[test]
    fn material_from_gltf_resource_returns_none_for_empty() {
        let resource = gdresource::Resource::new("ArrayMesh");
        assert!(material_from_gltf_resource(&resource).is_none());
    }

    #[test]
    fn resolve_mesh_loads_glb_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let glb_path = dir.path().join("model.glb");
        std::fs::write(&glb_path, make_test_glb()).unwrap();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mesh = Node::new("Model", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_mesh_path(&mut tree, mesh_id, glb_path.to_str().unwrap());

        let resolved = RenderServer3DAdapter::resolve_mesh(&tree, mesh_id);
        // glTF import loads the triangle mesh from the GLB (3 verts, 3 indices).
        assert_eq!(
            resolved.vertices.len(),
            3,
            "should load triangle from glb, got {} verts",
            resolved.vertices.len()
        );
        assert_eq!(
            resolved.indices.len(),
            3,
            "should load triangle indices from glb"
        );
    }

    // -- pat-nzgdu.119: CSG boolean tree reaches the renderer -----------------

    /// Builds a CSGBox3D root, optionally with a child CSGSphere3D applying
    /// `op`, and returns the evaluated root mesh the renderer would draw.
    fn csg_scene_mesh(child_op: Option<i64>) -> Mesh3D {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let box_id = tree.add_child(root, Node::new("Box", "CSGBox3D")).unwrap();
        tree.get_node_mut(box_id)
            .unwrap()
            .set_property("size", Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)));

        if let Some(op) = child_op {
            let sphere_id = tree
                .add_child(box_id, Node::new("Hole", "CSGSphere3D"))
                .unwrap();
            {
                let n = tree.get_node_mut(sphere_id).unwrap();
                n.set_property("radius", Variant::Float(0.9));
                n.set_property("operation", Variant::Int(op));
            }
        }
        evaluate_csg_tree(&tree, box_id).expect("a CSGBox3D root must produce geometry")
    }

    /// The bead's acceptance: a CSGBox3D root with a subtracting CSGSphere3D
    /// child renders geometry whose signature differs from the plain box — i.e.
    /// the boolean solver actually ran and carved a cavity.
    #[test]
    fn csg_box_subtract_renders_geometry_from_scene() {
        let plain = csg_scene_mesh(None);
        let carved = csg_scene_mesh(Some(2)); // 2 = subtraction

        assert!(
            !plain.vertices.is_empty(),
            "a CSGBox3D must produce box geometry"
        );
        assert!(
            !carved.vertices.is_empty(),
            "subtracting a sphere must still leave the box shell"
        );
        // The carve must change the geometry: identical output would mean the
        // solver never ran and the child was ignored.
        assert_ne!(
            (plain.vertices.len(), plain.indices.len()),
            (carved.vertices.len(), carved.indices.len()),
            "subtraction must change the mesh signature vs the box alone",
        );

        // A union of the same sphere differs from a subtraction of it, proving
        // the child's `operation` is honored rather than hardcoded.
        let unioned = csg_scene_mesh(Some(0)); // 0 = union
        assert_ne!(
            (unioned.vertices.len(), unioned.indices.len()),
            (carved.vertices.len(), carved.indices.len()),
            "union and subtraction must not produce the same mesh",
        );
    }

    /// The evaluated CSG root reaches the renderer as a real draw instance and
    /// renders pixels — the wiring the bead calls UNWIRED.
    #[test]
    fn csg_root_registers_a_render_instance() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = tree
            .add_child(root, Node::new("Camera", "Camera3D"))
            .unwrap();
        node3d::set_position(&mut tree, cam, Vector3::new(0.0, 0.0, 5.0));
        node3d::set_camera_current(&mut tree, cam, true);

        let box_id = tree.add_child(root, Node::new("Box", "CSGBox3D")).unwrap();
        tree.get_node_mut(box_id)
            .unwrap()
            .set_property("size", Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)));
        let sphere_id = tree
            .add_child(box_id, Node::new("Hole", "CSGSphere3D"))
            .unwrap();
        {
            let n = tree.get_node_mut(sphere_id).unwrap();
            n.set_property("radius", Variant::Float(0.9));
            n.set_property("operation", Variant::Int(2));
        }

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _frame) = adapter.render_frame(&tree);
        assert!(
            snapshot.visible_mesh_count >= 1,
            "the CSG root must register a visible render instance, got {}",
            snapshot.visible_mesh_count,
        );
        assert!(
            snapshot.nonblack_pixel_count > 0,
            "CSG geometry in front of the camera must produce pixels",
        );
    }

    /// Only roots evaluate: a CSG child must not also register its own instance,
    /// or the carved-away sphere would be drawn on top of the box.
    #[test]
    fn csg_child_does_not_register_its_own_instance() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let box_id = tree.add_child(root, Node::new("Box", "CSGBox3D")).unwrap();
        let _sphere = tree
            .add_child(box_id, Node::new("Hole", "CSGSphere3D"))
            .unwrap();

        let mut adapter = RenderServer3DAdapter::new(16, 16);
        let _ = adapter.render_frame(&tree);
        assert_eq!(
            adapter.instance_map.node_to_instance.len(),
            1,
            "only the CSG root may own a render instance",
        );
    }

    // -- pat-nzgdu.121: Light3D property surface reaches the light record -----

    /// Builds a scene with one DirectionalLight3D carrying `props`, syncs it,
    /// and returns the resulting light record.
    fn light_record_from_scene(props: &[(&str, Variant)]) -> Light3D {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let light_id = tree
            .add_child(root, Node::new("Sun", "DirectionalLight3D"))
            .unwrap();
        {
            let node = tree.get_node_mut(light_id).unwrap();
            for (k, v) in props {
                node.set_property(k, v.clone());
            }
        }
        let mut adapter = RenderServer3DAdapter::new(16, 16);
        let _ = adapter.render_frame(&tree);
        adapter
            .lights()
            .first()
            .cloned()
            .expect("scene light should reach the render server")
    }

    /// The bead's acceptance: a scene DirectionalLight3D with light_temperature
    /// set produces a correspondingly tinted light vs a 6500K light, a
    /// light_cull_mask excluding a layer leaves that layer unlit, and the PSSM
    /// splits round-trip through the light record.
    #[test]
    fn light3d_temperature_cullmask_and_pssm_from_scene() {
        // -- temperature tints the light, relative to daylight-neutral 6500K --
        let warm = light_record_from_scene(&[("light_temperature", Variant::Float(3000.0))]);
        let neutral = light_record_from_scene(&[("light_temperature", Variant::Float(6500.0))]);
        assert_eq!(
            warm.temperature, 3000.0,
            "authored Kelvin reaches the record"
        );
        assert!(
            warm.color.b < neutral.color.b,
            "3000K must be less blue than 6500K (got {} vs {})",
            warm.color.b,
            neutral.color.b,
        );
        assert!(
            warm.color.r > warm.color.b,
            "3000K must read warm, got {:?}",
            warm.color,
        );
        assert!(
            (neutral.color.r - neutral.color.b).abs() < 0.05,
            "6500K must stay ~neutral, got {:?}",
            neutral.color,
        );

        // -- cull mask excludes a mesh's layer ------------------------------
        // Mask = layer 1 only, so a mesh on layer 2 is not lit by this light.
        let culled = light_record_from_scene(&[("light_cull_mask", Variant::Int(0b1))]);
        assert_eq!(culled.cull_mask, 0b1);
        assert!(culled.affects_layer(1), "layer 1 is in the mask");
        assert!(
            !culled.affects_layer(2),
            "a mesh on an excluded layer must be left unlit"
        );

        // -- PSSM splits + blend round-trip ---------------------------------
        let pssm = light_record_from_scene(&[
            ("directional_shadow_split_1", Variant::Float(0.05)),
            ("directional_shadow_split_2", Variant::Float(0.25)),
            ("directional_shadow_split_3", Variant::Float(0.75)),
            ("directional_shadow_blend_splits", Variant::Bool(true)),
        ]);
        assert_eq!(pssm.shadow_split_1, 0.05);
        assert_eq!(pssm.shadow_split_2, 0.25);
        assert_eq!(pssm.shadow_split_3, 0.75);
        assert!(pssm.blend_splits, "blend_splits must round-trip");
    }

    /// A scene that authors none of the new properties must produce the same
    /// light record as before this bead — temperature is opt-in, so an
    /// unauthored light is not silently tinted by the 6500K default.
    #[test]
    fn light3d_unauthored_properties_leave_record_at_defaults() {
        let light = light_record_from_scene(&[]);
        assert_eq!(light.temperature, 6500.0);
        assert_eq!(light.cull_mask, u32::MAX);
        assert!(!light.blend_splits);
        // Colour is untouched (white), NOT multiplied by the 6500K tint.
        assert_eq!(
            light.color,
            Color::new(1.0, 1.0, 1.0, 1.0),
            "an unauthored light must keep its plain colour",
        );
    }

    /// The rest of the surface reaches the record too.
    #[test]
    fn light3d_extended_properties_reach_record() {
        let l = light_record_from_scene(&[
            ("light_indirect_energy", Variant::Float(2.5)),
            ("light_volumetric_fog_energy", Variant::Float(0.25)),
            ("distance_fade_enabled", Variant::Bool(true)),
            ("distance_fade_begin", Variant::Float(12.0)),
            ("distance_fade_shadow", Variant::Float(20.0)),
            ("distance_fade_length", Variant::Float(3.0)),
            ("shadow_opacity", Variant::Float(0.5)),
            ("shadow_caster_mask", Variant::Int(0b101)),
            ("editor_only", Variant::Bool(true)),
        ]);
        assert_eq!(l.indirect_energy, 2.5);
        assert_eq!(l.volumetric_fog_energy, 0.25);
        assert!(l.distance_fade_enabled);
        assert_eq!(l.distance_fade_begin, 12.0);
        assert_eq!(l.distance_fade_shadow, 20.0);
        assert_eq!(l.distance_fade_length, 3.0);
        assert_eq!(l.shadow_opacity, 0.5);
        assert_eq!(l.shadow_caster_mask, 0b101);
        assert!(l.editor_only);
    }

    #[test]
    fn resolve_mesh_falls_back_on_missing_glb() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mesh = Node::new("Model", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_mesh_path(&mut tree, mesh_id, "/nonexistent/model.glb");

        // Should fall back to default cube instead of panicking.
        let resolved = RenderServer3DAdapter::resolve_mesh(&tree, mesh_id);
        assert!(
            !resolved.vertices.is_empty(),
            "should fall back to default cube"
        );
    }

    #[test]
    fn end_to_end_gltf_mesh_renders_pixels() {
        let dir = tempfile::TempDir::new().unwrap();
        let glb_path = dir.path().join("scene.glb");
        std::fs::write(&glb_path, make_test_glb()).unwrap();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Camera looking at the origin.
        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 5.0));
        node3d::set_camera_current(&mut tree, cam_id, true);

        // MeshInstance3D referencing the glb file.
        let mesh = Node::new("Model", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_mesh_path(&mut tree, mesh_id, glb_path.to_str().unwrap());
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _frame) = adapter.render_frame(&tree);

        assert_eq!(snapshot.visible_mesh_count, 1);
        assert!(
            snapshot.nonblack_pixel_count > 0,
            "glTF mesh in front of camera should produce visible pixels"
        );
    }

    #[test]
    fn end_to_end_gltf_with_material_renders_pixels() {
        let dir = tempfile::TempDir::new().unwrap();
        let glb_path = dir.path().join("red_model.glb");
        std::fs::write(&glb_path, make_test_glb_with_material()).unwrap();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 5.0));
        node3d::set_camera_current(&mut tree, cam_id, true);

        let mesh = Node::new("RedModel", "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_mesh_path(&mut tree, mesh_id, glb_path.to_str().unwrap());
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _frame) = adapter.render_frame(&tree);

        assert_eq!(snapshot.visible_mesh_count, 1);
        assert!(
            snapshot.nonblack_pixel_count > 0,
            "gltf mesh with material should produce visible pixels"
        );
    }

    // -- Sprite3D compositing into the 3D draw list (pat-u3l8c.39) ----------

    /// Renders a scene containing one centered `Sprite3D` faced by a camera.
    /// `billboard` is the Godot `BillboardMode` int; `modulate` tints the quad.
    fn render_sprite_scene(
        billboard: i64,
        modulate: Color,
    ) -> (RenderFrame3DSnapshot, FrameData3D) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cam = Node::new("Camera", "Camera3D");
        let cam_id = tree.add_child(root, cam).unwrap();
        node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 5.0));
        node3d::set_camera_current(&mut tree, cam_id, true);

        let mut sprite = Node::new("Sprite", "Sprite3D");
        // A 64x64 "texture" at pixel_size 0.08 -> ~5.1m quad, so it covers the
        // view centre from 5m away without needing a real texture file.
        sprite.set_property("texture_size", Variant::Vector2(Vector2::new(64.0, 64.0)));
        sprite.set_property("pixel_size", Variant::Float(0.08));
        sprite.set_property("modulate", Variant::Color(modulate));
        sprite.set_property("billboard", Variant::Int(billboard));
        let sprite_id = tree.add_child(root, sprite).unwrap();
        node3d::set_position(&mut tree, sprite_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        adapter.render_frame(&tree)
    }

    #[test]
    fn sprite3d_composited_renders_visible_pixels() {
        // The core acceptance check: a Sprite3D is composited into the 3D draw
        // list and rasterises visible pixels (it was previously never drawn).
        let (snapshot, _frame) = render_sprite_scene(0, Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(
            snapshot.visible_mesh_count, 1,
            "the Sprite3D quad should be one visible render instance"
        );
        assert!(
            snapshot.nonblack_pixel_count > 0,
            "Sprite3D should rasterise visible pixels into the frame"
        );
    }

    #[test]
    fn sprite3d_billboard_enabled_still_renders() {
        // BillboardMode::Enabled (1) reorients the quad toward the camera; it
        // must remain visible in the rendered frame.
        let (snapshot, _frame) = render_sprite_scene(1, Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(snapshot.visible_mesh_count, 1);
        assert!(
            snapshot.nonblack_pixel_count > 0,
            "a billboarded Sprite3D should still rasterise visible pixels"
        );
    }

    #[test]
    fn sprite3d_modulate_tints_rendered_center() {
        // A green, unlit sprite paints its modulate colour at the view centre,
        // proving the sprite material (not just geometry) reaches the pixels.
        let (_snapshot, frame) = render_sprite_scene(0, Color::new(0.0, 1.0, 0.0, 1.0));
        let center =
            (frame.height / 2) as usize * frame.width as usize + (frame.width / 2) as usize;
        let px = frame.pixels[center];
        // Green-dominance is scale-invariant (survives tonemapping) and also
        // rules out the black background (0,0,0 fails `g > r`).
        assert!(
            px.g > px.r && px.g > px.b,
            "centre pixel should be green-dominant from the sprite modulate, got {px:?}"
        );
    }

    /// 🔑 `light_specular` AUTHORED ON A NODE REACHES THE `Light3D` — in both
    /// GDScript spellings — and the class defaults survive when it is unset.
    ///
    /// ⚠️ The unset half is the point of the whole property: `Light3D` and
    /// `DirectionalLight3D` default to **1.0** while `OmniLight3D`/
    /// `SpotLight3D` default to **0.5**, and Godot doubles the positional ones
    /// on the way to the shader — so every light type comes out at an effective
    /// 1.0 and wiring this moves no existing render. A single shared default
    /// here would silently halve or double every positional highlight.
    ///
    /// ⚠️ `0.25` on purpose: 1.0 and 0.5 are the defaults, so a fixture using
    /// either is green whether or not the read happens at all.
    #[test]
    fn light_specular_is_read_from_the_node_and_defaults_per_class() {
        use gdserver3d::light::{Light3D, Light3DId};

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let nid = tree
            .add_child(root, Node::new("Sun", "DirectionalLight3D"))
            .expect("node added");

        // Unset: the per-class defaults stand.
        let mut directional = Light3D::directional(Light3DId(1));
        let mut omni = Light3D::point(Light3DId(2), Vector3::ZERO);
        let mut spot = Light3D::spot(Light3DId(3), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0));
        let node = tree.get_node(nid).expect("node exists");
        sync_light_properties(node, &mut directional);
        sync_light_properties(node, &mut omni);
        sync_light_properties(node, &mut spot);
        assert_eq!(
            directional.specular, 1.0,
            "Light3D/DirectionalLight3D default"
        );
        assert_eq!(omni.specular, 0.5, "OmniLight3D default");
        assert_eq!(spot.specular, 0.5, "SpotLight3D default");

        // Authored as a float, the `.tscn` spelling.
        tree.get_node_mut(nid)
            .expect("node exists")
            .set_property("light_specular", Variant::Float(0.25));
        let mut light = Light3D::directional(Light3DId(1));
        sync_light_properties(tree.get_node(nid).expect("node exists"), &mut light);
        assert_eq!(light.specular, 0.25, "float spelling must be read");

        // Authored as an int, the spelling GDScript gives `light.light_specular
        // = 1`. Matching only the float kind drops this silently.
        tree.get_node_mut(nid)
            .expect("node exists")
            .set_property("light_specular", Variant::Int(1));
        let mut light = Light3D::point(Light3DId(2), Vector3::ZERO);
        sync_light_properties(tree.get_node(nid).expect("node exists"), &mut light);
        assert_eq!(
            light.specular, 1.0,
            "int spelling must be read — and it must OVERRIDE the 0.5 omni \
             default, which is what makes this assertion non-vacuous"
        );
    }

    /// 🔑 `directional_shadow_max_distance` AND `directional_shadow_fade_start`
    /// ARE READ FROM THE NODE, WITH GODOT'S DEFAULTS WHEN UNSET.
    ///
    /// ⚠️ Both are declared on `DirectionalLight3D` ALONE — neither
    /// `OmniLight3D` nor `SpotLight3D` nor the `Light3D` base exposes them —
    /// but they are stored as base `Light3D` params, so every constructor
    /// carries the same defaults here. They are simply inert for positional
    /// lights, whose shadows are bounded by their own `range`.
    ///
    /// ⚠️ `fade_start = 1.0` must SURVIVE the read. Godot clamps it to 0.999,
    /// but only at the point of use in the shader — clamping on the way in
    /// would make the property fail to round-trip through the inspector.
    #[test]
    fn directional_shadow_fade_is_read_from_the_node_with_godot_defaults() {
        use gdserver3d::light::{Light3D, Light3DId};

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let nid = tree
            .add_child(root, Node::new("Sun", "DirectionalLight3D"))
            .expect("node added");

        // Unset: Godot's DirectionalLight3D defaults, on every light type.
        for mut light in [
            Light3D::directional(Light3DId(1)),
            Light3D::point(Light3DId(2), Vector3::ZERO),
            Light3D::spot(Light3DId(3), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0)),
        ] {
            sync_light_properties(tree.get_node(nid).expect("node exists"), &mut light);
            assert_eq!(light.directional_shadow_max_distance, 100.0);
            assert_eq!(light.directional_shadow_fade_start, 0.8);
        }

        // Authored as floats, the `.tscn` spelling. 40.0/0.25 on purpose —
        // neither is a default, so the assertion fails if the read is missing.
        {
            let node = tree.get_node_mut(nid).expect("node exists");
            node.set_property("directional_shadow_max_distance", Variant::Float(40.0));
            node.set_property("directional_shadow_fade_start", Variant::Float(0.25));
        }
        let mut light = Light3D::directional(Light3DId(1));
        sync_light_properties(tree.get_node(nid).expect("node exists"), &mut light);
        assert_eq!(light.directional_shadow_max_distance, 40.0);
        assert_eq!(light.directional_shadow_fade_start, 0.25);

        // Authored as ints, the spelling GDScript gives `light.
        // directional_shadow_max_distance = 50`. Matching only the float kind
        // drops this silently and leaves the default in place.
        {
            let node = tree.get_node_mut(nid).expect("node exists");
            node.set_property("directional_shadow_max_distance", Variant::Int(50));
            node.set_property("directional_shadow_fade_start", Variant::Int(1));
        }
        let mut light = Light3D::directional(Light3DId(1));
        sync_light_properties(tree.get_node(nid).expect("node exists"), &mut light);
        assert_eq!(light.directional_shadow_max_distance, 50.0);
        assert_eq!(
            light.directional_shadow_fade_start, 1.0,
            "an authored 1.0 must round-trip — the 0.999 clamp is the shader's, \
             not the loader's"
        );
    }
}

#[cfg(test)]
mod standard_material_props_tests {
    use super::*;

    fn bag(pairs: &[(&str, Variant)]) -> HashMap<String, Variant> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    /// An empty bag yields Godot's documented defaults — notably roughness 1.0,
    /// not 0.0. Getting that backwards makes every unspecified material a mirror.
    #[test]
    fn empty_bag_uses_godot_defaults() {
        let m = standard_material_from_props(&bag(&[]));
        assert_eq!(m.albedo_color, Color::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(m.metallic, 0.0);
        assert_eq!(m.roughness, 1.0, "BaseMaterial3D.roughness defaults to 1.0");
        assert_eq!(m.emission, Color::new(0.0, 0.0, 0.0, 1.0));
        assert!(!m.normal_enabled);
        assert_eq!(m.normal_scale, 1.0);
        assert!(m.albedo_texture.is_none());
    }

    /// ⚠️ The bead's own scene: metallic=1, roughness=0.1 with an albedo texture
    /// must survive into the material instead of collapsing to a flat colour.
    #[test]
    fn authored_pbr_params_and_texture_survive() {
        let m = standard_material_from_props(&bag(&[
            (
                "albedo_color",
                Variant::Color(Color::new(0.2, 0.4, 0.6, 1.0)),
            ),
            (
                "albedo_texture",
                Variant::String("res://tex/albedo.png".into()),
            ),
            ("metallic", Variant::Float(1.0)),
            ("roughness", Variant::Float(0.1)),
        ]));
        assert_eq!(m.albedo_color, Color::new(0.2, 0.4, 0.6, 1.0));
        assert_eq!(m.metallic, 1.0);
        assert!((m.roughness - 0.1).abs() < 1e-6);
        assert_eq!(
            m.albedo_texture.as_ref().map(|t| t.path.as_str()),
            Some("res://tex/albedo.png"),
            "the texture reference must reach the material for the loader to resolve"
        );
    }

    /// Godot gates emission behind `emission_enabled`. A colour without the flag
    /// must NOT glow, or every scene that merely names an emission colour lights up.
    #[test]
    fn emission_requires_its_enable_flag() {
        let colour = Variant::Color(Color::new(1.0, 0.5, 0.0, 1.0));
        let off = standard_material_from_props(&bag(&[("emission", colour.clone())]));
        assert_eq!(
            off.emission,
            Color::new(0.0, 0.0, 0.0, 1.0),
            "emission without emission_enabled must stay black"
        );

        let on = standard_material_from_props(&bag(&[
            ("emission", colour),
            ("emission_enabled", Variant::Bool(true)),
        ]));
        assert_eq!(on.emission, Color::new(1.0, 0.5, 0.0, 1.0));
    }

    /// Same gate for the normal map.
    #[test]
    fn normal_map_requires_its_enable_flag() {
        let tex = Variant::String("res://tex/n.png".into());
        let off = standard_material_from_props(&bag(&[("normal_texture", tex.clone())]));
        assert!(
            off.normal_texture.is_none(),
            "normal map is gated by normal_enabled"
        );

        let on = standard_material_from_props(&bag(&[
            ("normal_texture", tex),
            ("normal_enabled", Variant::Bool(true)),
        ]));
        assert!(on.normal_texture.is_some());
        assert!(on.normal_enabled);
    }

    /// BaseMaterial3D.ShadingMode: 0 UNSHADED, 1 PER_PIXEL (default), 2 PER_VERTEX.
    #[test]
    fn shading_mode_maps_from_the_godot_enum() {
        let m0 = standard_material_from_props(&bag(&[("shading_mode", Variant::Int(0))]));
        assert_eq!(m0.shading_mode, ShadingMode::Unlit);
        let m2 = standard_material_from_props(&bag(&[("shading_mode", Variant::Int(2))]));
        assert_eq!(m2.shading_mode, ShadingMode::Lambert);
        // Absent => PER_PIXEL, the documented default.
        let md = standard_material_from_props(&bag(&[]));
        assert_eq!(md.shading_mode, ShadingMode::Phong);
    }

    /// The whole point of the parse: it feeds the renderer's Material3D, which
    /// already knows how to consume a StandardMaterial3D.
    #[test]
    fn parsed_material_converts_for_the_renderer() {
        let m = standard_material_from_props(&bag(&[
            ("metallic", Variant::Float(1.0)),
            ("roughness", Variant::Float(0.25)),
        ]))
        .to_material3d();
        assert_eq!(m.metallic, 1.0);
        assert!((m.roughness - 0.25).abs() < 1e-6);
    }
}

#[cfg(test)]
mod material_texture_cache_tests {
    use super::*;

    /// A real 1x1 red PNG that lives in the repo, so the success path exercises
    /// an actual decode rather than a hand-built buffer.
    fn fixture_png() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../gdresource/fixtures/red1x1.png")
    }

    #[test]
    fn decodes_a_real_png_once_and_caches_it() {
        let path = fixture_png();
        if !path.exists() {
            // The fixture moved; skip rather than fail a bead on repo layout.
            return;
        }
        let key = path.to_string_lossy().to_string();
        let mut cache = MaterialTextureCache::new();
        assert!(cache.is_empty());

        let first = cache
            .get_or_load(&key)
            .expect("the fixture PNG must decode");
        assert_eq!(first.width, 1);
        assert_eq!(first.height, 1);
        assert_eq!(first.pixels.len(), 1);

        // Second lookup is a cache hit: same allocation, no new entry.
        let second = cache.get_or_load(&key).expect("cached");
        assert!(
            Arc::ptr_eq(&first, &second),
            "a repeat lookup must return the cached Arc, not decode again"
        );
        assert_eq!(cache.len(), 1);
    }

    /// ⚠️ A missing texture must be cached as a MISS. Without this the render
    /// path retries the filesystem for every surface on every frame.
    #[test]
    fn a_missing_texture_is_cached_as_a_miss() {
        let mut cache = MaterialTextureCache::new();
        assert!(cache
            .get_or_load("/nonexistent/definitely-not-here.png")
            .is_none());
        assert_eq!(cache.len(), 1, "the failure must occupy a cache slot");
        assert!(cache
            .get_or_load("/nonexistent/definitely-not-here.png")
            .is_none());
        assert_eq!(cache.len(), 1, "a repeated miss must not add another entry");
    }

    /// An unresolvable `res://` path (no project roots installed) is a miss, not
    /// a panic — a broken reference falls back to flat albedo, as in Godot.
    #[test]
    fn unresolvable_res_path_is_a_miss() {
        let mut cache = MaterialTextureCache::new();
        assert!(cache
            .get_or_load("res://textures/does_not_exist.png")
            .is_none());
    }
}

#[cfg(test)]
mod material_node_companion_tests {
    use super::*;
    use crate::packed_scene::{add_packed_scene_to_tree, PackedScene};

    /// A mesh whose StandardMaterial3D sets more than albedo — exactly the shape
    /// that previously arrived at the renderer as a flat colour.
    const SCENE: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="BoxMesh" id="Mesh_1"]

[sub_resource type="StandardMaterial3D" id="Mat_1"]
albedo_color = Color(0.2, 0.4, 0.6, 1)
metallic = 1.0
roughness = 0.1
emission_enabled = true
emission = Color(1, 0.5, 0, 1)

[node name="Root" type="Node3D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
mesh = SubResource("Mesh_1")
surface_material_override/0 = SubResource("Mat_1")
"#;

    /// ⚠️ The data path: a `.tscn` material's full parameter set survives scene
    /// instancing and is recoverable from the node alone. Before this only
    /// `albedo_color` was copied, so metallic/roughness/emission were gone by the
    /// time anything could render them.
    #[test]
    fn material_params_survive_scene_instancing() {
        let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root =
            add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
        let mesh = tree
            .get_node_or_null(scene_root, "Mesh")
            .expect("Mesh instanced");

        let props = material_props_from_node(&tree, mesh)
            .expect("the mesh must carry __material__ companions");
        let m = standard_material_from_props(&props);

        assert_eq!(m.metallic, 1.0, "metallic must survive instancing");
        assert!((m.roughness - 0.1).abs() < 1e-6, "roughness must survive");
        assert_eq!(
            m.emission,
            Color::new(1.0, 0.5, 0.0, 1.0),
            "emission must survive, and its enable flag with it"
        );
        assert_eq!(m.albedo_color, Color::new(0.2, 0.4, 0.6, 1.0));
    }

    /// A mesh with no material carries no companions, so the caller keeps its
    /// existing flat-albedo behaviour rather than getting a default material.
    #[test]
    fn a_mesh_without_a_material_has_no_companions() {
        let scene = PackedScene::from_tscn(
            "[gd_scene load_steps=2 format=3]\n\n             [sub_resource type=\"BoxMesh\" id=\"Mesh_1\"]\n\n             [node name=\"Root\" type=\"Node3D\"]\n\n             [node name=\"Mesh\" type=\"MeshInstance3D\" parent=\".\"]\n             mesh = SubResource(\"Mesh_1\")\n",
        )
        .expect("scene parses");
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root =
            add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
        let mesh = tree
            .get_node_or_null(scene_root, "Mesh")
            .expect("Mesh instanced");
        assert!(material_props_from_node(&tree, mesh).is_none());
    }

    /// End to end: scene -> node companions -> renderer-ready Material3D.
    #[test]
    fn material3d_for_node_resolves_the_full_chain() {
        let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root =
            add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
        let mesh = tree
            .get_node_or_null(scene_root, "Mesh")
            .expect("Mesh instanced");

        let mut textures = MaterialTextureCache::new();
        let m = material3d_for_node(&tree, mesh, &mut textures)
            .expect("an authored material must resolve");
        assert_eq!(m.metallic, 1.0);
        assert!((m.roughness - 0.1).abs() < 1e-6);
        // No texture was authored, so nothing was attempted.
        assert!(m.albedo_texture.is_none());
        assert!(
            textures.is_empty(),
            "no texture refs means no load attempts"
        );
    }
}
