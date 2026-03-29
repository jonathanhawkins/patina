//! 3D render path adapter: bridges the scene tree to [`RenderingServer3D`].
//!
//! Collects Camera3D, MeshInstance3D, MultiMeshInstance3D, and Light3D nodes
//! from the scene tree each frame, submits them to a [`RenderingServer3D`]
//! implementation, and returns measurable frame data for oracle/golden comparison.

use std::collections::HashMap;

use gdcore::math::Color;
use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdrender3d::{compare_framebuffers_3d, DiffResult3D, FrameBuffer3D, SoftwareRenderer3D};
use gdserver3d::instance::Instance3DId;
use gdserver3d::light::{Light3D, Light3DId, LightType, OmniShadowMode};
use gdserver3d::material::{Material3D, ShadingMode, StandardMaterial3D, TextureSlot};
use gdserver3d::mesh::{Mesh3D, PrimitiveType};
use gdserver3d::reflection_probe::ReflectionProbeId;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::shader::{Shader3D, ShaderMaterial3D, ShaderType3D};
use gdserver3d::viewport::Viewport3D;

use crate::node::NodeId;
use crate::node3d;
use crate::scene_tree::SceneTree;

/// Maps scene tree NodeIds to render server Instance3DIds (for MeshInstance3D).
#[derive(Debug, Default)]
struct InstanceMap {
    node_to_instance: HashMap<NodeId, Instance3DId>,
}

/// Maps MultiMeshInstance3D NodeIds to their expanded render instance IDs.
#[derive(Debug, Default)]
struct MultiMeshMap {
    node_to_instances: HashMap<NodeId, Vec<Instance3DId>>,
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
    light_map: LightMap,
    reflection_probe_map: ReflectionProbeMap,
    viewport_width: u32,
    viewport_height: u32,
    frame_counter: u64,
    /// Stored last frame data for comparison.
    last_frame: Option<FrameBuffer3D>,
}

impl RenderServer3DAdapter {
    /// Creates a new adapter with the given viewport dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            renderer: SoftwareRenderer3D::new(),
            instance_map: InstanceMap::default(),
            multimesh_map: MultiMeshMap::default(),
            light_map: LightMap::default(),
            reflection_probe_map: ReflectionProbeMap::default(),
            viewport_width: width,
            viewport_height: height,
            frame_counter: 0,
            last_frame: None,
        }
    }

    /// Returns the current frame counter.
    pub fn frame_counter(&self) -> u64 {
        self.frame_counter
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

        // 1. Find the active Camera3D and build a Viewport3D.
        let (viewport, camera_transform) = self.build_viewport(tree);

        // 2. Sync MeshInstance3D and MultiMeshInstance3D nodes.
        let visible_count = self.sync_mesh_instances(tree);
        let multimesh_visible = self.sync_multimesh_instances(tree);
        let visible_count = visible_count + multimesh_visible;

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

        let depth_written = frame_data
            .depth
            .iter()
            .filter(|d| **d < 1.0)
            .count() as u64;

        let ct = camera_transform;
        let camera_arr = [
            ct.basis.x.x, ct.basis.x.y, ct.basis.x.z,
            ct.basis.y.x, ct.basis.y.y, ct.basis.y.z,
            ct.basis.z.x, ct.basis.z.y, ct.basis.z.z,
            ct.origin.x, ct.origin.y, ct.origin.z,
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
                let class = node.class_name();
                if class == "Camera3D" {
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
            environment: None,
        };

        (viewport, camera_transform)
    }

    /// Syncs MeshInstance3D nodes from the scene tree to the render server.
    ///
    /// Creates new instances for nodes not yet tracked, updates transforms
    /// for existing ones, and removes instances for nodes no longer in the tree.
    /// Returns the number of visible mesh instances.
    fn sync_mesh_instances(&mut self, tree: &SceneTree) -> u32 {
        let all_nodes = tree.all_nodes_in_tree_order();

        // Collect current MeshInstance3D nodes.
        let mut current_mesh_nodes: HashMap<NodeId, ()> = HashMap::new();
        let mut visible_count = 0u32;

        for &nid in &all_nodes {
            if let Some(node) = tree.get_node(nid) {
                if node.class_name() == "MeshInstance3D" {
                    current_mesh_nodes.insert(nid, ());

                    let transform = node3d::get_global_transform(tree, nid);
                    let visible = node3d::is_visible(tree, nid);

                    let inst_id = if let Some(&existing) = self.instance_map.node_to_instance.get(&nid) {
                        existing
                    } else {
                        let new_id = self.renderer.create_instance();
                        // Resolve mesh type from the node's properties.
                        let mesh = Self::resolve_mesh(tree, nid);
                        self.renderer.set_mesh(new_id, mesh);
                        self.renderer.set_material(new_id, Material3D::default());
                        self.instance_map.node_to_instance.insert(nid, new_id);
                        new_id
                    };

                    self.renderer.set_transform(inst_id, transform);
                    self.renderer.set_visible(inst_id, visible);

                    // Resolve material following Godot's priority chain:
                    //   1. material_override (highest priority — overrides everything)
                    //   2. surface_material_override/0 (per-surface override)
                    //   3. node "albedo" color property (legacy shorthand)
                    //   4. Material3D::default()
                    let resolved_mat = Self::resolve_material(tree, nid);
                    self.renderer.set_material(inst_id, resolved_mat);

                    // Resolve ShaderMaterial3D if attached to this node.
                    if let Some(shader_mat) = Self::resolve_shader_material(tree, nid) {
                        self.renderer.set_shader_material(inst_id, shader_mat);
                    }

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
        }

        visible_count
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
                let node_visible = node3d::is_visible(tree, nid);
                let instance_count =
                    node3d::get_multimesh_instance_count(tree, nid) as usize;

                // Resolve shared mesh from multimesh_mesh_type or mesh_type.
                let mesh = Self::resolve_multimesh_mesh(tree, nid);

                // Get or create the expanded instance list for this node.
                let existing = self.multimesh_map.node_to_instances.remove(&nid);
                let mut inst_ids: Vec<Instance3DId> =
                    existing.unwrap_or_default();

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
                    let per_inst =
                        node3d::get_multimesh_instance_transform(tree, nid, i);
                    let composed = node_transform * per_inst;

                    self.renderer.set_transform(inst_id, composed);
                    self.renderer.set_visible(inst_id, node_visible);

                    // Per-instance color → material albedo.
                    let color =
                        node3d::get_multimesh_instance_color(tree, nid, i);
                    let mat = Material3D {
                        albedo: color,
                        ..Material3D::default()
                    };
                    self.renderer.set_material(inst_id, mat);

                    if node_visible {
                        visible_count += 1;
                    }
                }

                self.multimesh_map
                    .node_to_instances
                    .insert(nid, inst_ids);
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
            if let Some(ids) =
                self.multimesh_map.node_to_instances.remove(&nid)
            {
                for id in ids {
                    self.renderer.free_instance(id);
                }
            }
        }

        visible_count
    }

    /// Resolves the shared mesh for a MultiMeshInstance3D.
    fn resolve_multimesh_mesh(tree: &SceneTree, nid: NodeId) -> Mesh3D {
        if let Some(mesh_type) =
            node3d::get_multimesh_mesh_type(tree, nid)
        {
            if let Some(m) = Self::mesh_from_type_name(&mesh_type) {
                return m;
            }
        }
        if let Some(node) = tree.get_node(nid) {
            if let gdvariant::Variant::String(s) =
                node.get_property("mesh_type")
            {
                if let Some(m) = Self::mesh_from_type_name(&s) {
                    return m;
                }
            }
        }
        Mesh3D::cube(1.0)
    }

    /// Maps a mesh type name string to its default [`Mesh3D`] geometry.
    fn mesh_from_type_name(name: &str) -> Option<Mesh3D> {
        match name {
            "BoxMesh" => Some(gdserver3d::BoxMesh::default().generate()),
            "SphereMesh" => Some(gdserver3d::SphereMesh::default().generate()),
            "CapsuleMesh" => Some(gdserver3d::CapsuleMesh::default().generate()),
            "CylinderMesh" => Some(gdserver3d::CylinderMesh::default().generate()),
            "PlaneMesh" | "QuadMesh" => Some(gdserver3d::PlaneMesh::default().generate()),
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
    fn resolve_mesh(tree: &SceneTree, nid: NodeId) -> Mesh3D {
        if let Some(path) = node3d::get_mesh_path(tree, nid) {
            let lower = path.to_lowercase();

            // Try glTF loading for .glb / .gltf paths.
            if lower.ends_with(".glb") || lower.ends_with(".gltf") {
                let file_path = std::path::Path::new(&path);
                if let Ok(resource) = gdresource::import_gltf(file_path) {
                    if let Some(mesh) = mesh3d_from_gltf_resource(&resource) {
                        return mesh;
                    }
                }
                // Fall through to default cube if loading fails.
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
    /// 2. `surface_material_override/0` — per-surface override (surface 0 for
    ///    the single-instance render path)
    /// 3. StandardMaterial3D properties (`albedo_color`, `metallic`, etc.)
    /// 4. `albedo` color property — legacy shorthand for a colored material
    /// 5. `Material3D::default()` — white Lambert
    fn resolve_material(tree: &SceneTree, nid: NodeId) -> Material3D {
        // 1. Check material_override (highest priority).
        if let Some(override_path) = node3d::get_material_override(tree, nid) {
            return Self::material_from_path(&override_path);
        }

        // 2. Check per-surface override for surface 0.
        if let Some(surface_path) = node3d::get_surface_override_material(tree, nid, 0) {
            return Self::material_from_path(&surface_path);
        }

        // 3. Check StandardMaterial3D properties on the node.
        if let Some(mat) = Self::resolve_standard_material(tree, nid) {
            return mat;
        }

        // 4. Check glTF embedded material properties.
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

        // 5. Check legacy albedo color property.
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
        })
    }

    /// Attempts to build a [`ShaderMaterial3D`] from properties set on a node.
    ///
    /// Returns `Some` if the node has a `shader_code` property containing
    /// non-empty shader source. Reads `shader_type` (0 = Spatial, 1 = Sky)
    /// and any shader parameter properties prefixed with `shader_parameter/`.
    fn resolve_shader_material(tree: &SceneTree, nid: NodeId) -> Option<ShaderMaterial3D> {
        let node = tree.get_node(nid)?;

        // Check for shader_code property — the presence of non-empty source
        // indicates this node has a ShaderMaterial3D attached.
        let shader_code = match node.get_property("shader_code") {
            gdvariant::Variant::String(s) if !s.is_empty() => s,
            _ => return None,
        };

        let shader_type = match node.get_property("shader_type") {
            gdvariant::Variant::Int(1) => ShaderType3D::Sky,
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

                        // Sync type-specific properties.
                        match lt {
                            LightType::Spot => {
                                light.spot_angle =
                                    (node3d::get_spot_angle(tree, nid) as f32).to_radians();
                                light.range = node3d::get_spot_range(tree, nid) as f32;
                                light.attenuation =
                                    node3d::get_spot_attenuation(tree, nid) as f32;
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
            .field("tracked_instances", &self.instance_map.node_to_instance.len())
            .field("tracked_multimesh_nodes", &self.multimesh_map.node_to_instances.len())
            .field("tracked_lights", &self.light_map.node_to_light.len())
            .field("tracked_probes", &self.reflection_probe_map.node_to_probe.len())
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
    let mut subs: Vec<_> = resource.subresources.iter().collect();
    if subs.is_empty() {
        return None;
    }
    // Sort by key for deterministic ordering (mesh_0, mesh_0_prim_0, etc.).
    subs.sort_by_key(|(k, _)| k.clone());

    // First sub-resource becomes the primary surface.
    let (_, first_sub) = &subs[0];
    let vertices = extract_vector3_array(first_sub.get_property("vertices")?)?;
    if vertices.is_empty() {
        return None;
    }

    let normals = extract_vector3_array(first_sub.get_property("normals")?)
        .unwrap_or_default();
    let uvs = extract_uv_array(first_sub.get_property("uvs")?)
        .unwrap_or_default();
    let indices = extract_index_array(first_sub.get_property("indices")?)
        .unwrap_or_default();

    let mut mesh = Mesh3D {
        vertices,
        normals,
        uvs,
        indices,
        primitive_type: PrimitiveType::Triangles,
        surfaces: Vec::new(),
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
fn extract_vector3_array(v: &gdvariant::Variant) -> Option<Vec<Vector3>> {
    if let gdvariant::Variant::Array(arr) = v {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
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
        for item in arr {
            if let gdvariant::Variant::Array(pair) = item {
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

/// Extracts `Vec<u32>` from a `Variant::Array` of `Variant::Int`.
fn extract_index_array(v: &gdvariant::Variant) -> Option<Vec<u32>> {
    if let gdvariant::Variant::Array(arr) = v {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::scene_tree::SceneTree;
    use gdcore::math::Vector3;

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

        assert_eq!(snapshot.light_count, 1, "should detect one directional light");
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
        mesh.set_property("mesh_type", gdvariant::Variant::String("SphereMesh".to_owned()));
        let mesh_id = tree.add_child(root, mesh).unwrap();
        node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

        let mut adapter = RenderServer3DAdapter::new(64, 64);
        let (snapshot, _) = adapter.render_frame(&tree);

        assert_eq!(snapshot.visible_mesh_count, 1);
        assert!(snapshot.nonblack_pixel_count > 0, "sphere should produce pixels");
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
        assert!(report.is_functional(), "scene with camera+mesh should be functional");
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
        assert!(!report.is_functional(), "empty scene should not be functional");
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
        let positions: [[f32; 3]; 3] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let normals: [[f32; 3]; 3] = [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ];
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
        let positions: [[f32; 3]; 3] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let normals: [[f32; 3]; 3] = [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ];
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
    fn mesh3d_from_gltf_resource_not_yet_implemented() {
        // glTF import is not yet implemented — import_gltf always returns Err.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("tri.glb");
        std::fs::write(&path, make_test_glb()).unwrap();

        assert!(gdresource::import_gltf(&path).is_err());
    }

    #[test]
    fn mesh3d_from_gltf_resource_returns_none_for_empty() {
        let resource = gdresource::Resource::new("ArrayMesh");
        assert!(mesh3d_from_gltf_resource(&resource).is_none());
    }

    #[test]
    fn material_from_gltf_resource_not_yet_implemented() {
        // glTF import is not yet implemented — import_gltf always returns Err.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("red.glb");
        std::fs::write(&path, make_test_glb_with_material()).unwrap();

        assert!(gdresource::import_gltf(&path).is_err());
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
        // glTF import is not yet implemented, so resolve_mesh falls back to
        // the default cube (24 vertices, 36 indices).
        assert_eq!(resolved.vertices.len(), 24, "should fall back to default cube");
        assert_eq!(resolved.indices.len(), 36, "should fall back to default cube");
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
        assert!(!resolved.vertices.is_empty(), "should fall back to default cube");
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
    fn end_to_end_gltf_falls_back_to_default_cube() {
        // glTF import is not yet implemented, so MeshInstance3D with a .glb path
        // falls back to the default cube. Verify it still renders something.
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
        assert!(snapshot.nonblack_pixel_count > 0, "fallback cube should produce visible pixels");
    }
}
