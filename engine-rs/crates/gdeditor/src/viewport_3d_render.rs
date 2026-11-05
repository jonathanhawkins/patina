//! Building the 3D viewport's render list from the scene's Node3D content.
//!
//! [`Viewport3D::collect_scene_content`](crate::viewport_3d::Viewport3D::collect_scene_content)
//! gathers the spatial nodes; this module turns them into concrete draw items
//! the renderer can mount: a **mesh** for mesh-bearing nodes (`MeshInstance3D`),
//! or a **primitive marker** (camera frustum, light icon, or a generic box) for
//! spatial nodes with no mesh so cameras, lights, and empty `Node3D`s are still
//! visible and pickable in the editor — matching Godot's 3D viewport.
//!
//! It lives in its own module (over the public `viewport_3d` API) to compose
//! with the viewport without growing that already-large file.

use gdcore::math::{Color, Vector2, Vector3};
use gdcore::math3d::Transform3D;
use gdrender2d::renderer::{Bgra8Image, FrameBuffer, Raster};
use gdrender3d::SoftwareRenderer3D;
use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdserver3d::compositor::{Compositor, CompositorRenderData, EffectCallbackType};
use gdserver3d::decal::Decal3D;
use gdserver3d::fog_volume::FogVolume;
use gdserver3d::gi::{VoxelGI, VoxelGIId, VoxelGISubdiv};
use gdserver3d::instance::GeometryInstanceSettings;
use gdserver3d::light::{Light3D, Light3DId, LightType};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::occluder::{Occluder3D, OccluderInstance3D};
use gdserver3d::reflection_probe::{
    ReflectionProbe, ReflectionProbeAmbientMode, ReflectionProbeId, ReflectionProbeUpdateMode,
};
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D as RenderViewport3D;

use crate::viewport_3d::{Node3DRenderable, Viewport3D};

/// The kind of content drawn for a spatial node.
#[derive(Debug, Clone, PartialEq)]
pub enum Render3DKind {
    /// A mesh resource to render, identified by its resource path.
    Mesh { path: String },
    /// A built-in primitive marker drawn for a spatial node that has no mesh
    /// (cameras, lights, empty `Node3D`s) so it stays visible and pickable.
    Primitive { shape: PrimitiveShape },
    /// A `CollisionShape3D`: drawn as a coloured wireframe of its shape (like
    /// Godot's collision gizmos) rather than solid geometry. `half_extents`
    /// carries the shape's real size (box half-size; x = radius / y = half-height
    /// for sphere/capsule/cylinder).
    Collision {
        shape: CollisionShapeKind,
        half_extents: Vector3,
    },
    /// Geometry generated from the scene rather than loaded from a resource
    /// path — a CSG boolean tree folded into one mesh (pat-s3rrv.89).
    ///
    /// ⚠️ Carries the mesh by value because there IS no path to load it from:
    /// it is computed from the node's own subtree, and the render pass runs
    /// after the `SceneTree` is out of scope.
    Geometry { mesh: Mesh3D },
}

/// The collision shape a `CollisionShape3D` carries, chosen from its `shape:`
/// marker. Unsupported shapes fall back to a box wireframe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionShapeKind {
    /// `BoxShape3D` (and the default fallback).
    Box,
    /// `SphereShape3D`.
    Sphere,
    /// `CapsuleShape3D`.
    Capsule,
    /// `CylinderShape3D`.
    Cylinder,
}

/// The primitive marker drawn for a non-mesh spatial node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveShape {
    /// A camera frustum/icon (for `Camera3D`).
    Camera,
    /// A light icon (for the `Light3D` family).
    Light,
    /// A generic box marker for any other spatial node.
    Box,
}

/// One drawable item in the 3D viewport's render list.
#[derive(Debug, Clone, PartialEq)]
pub struct Render3DItem {
    /// The source node's raw id.
    pub node_id: u64,
    /// The node's global (world-space) transform.
    pub global_transform: Transform3D,
    /// What to draw for this node.
    pub kind: Render3DKind,
    /// The mesh's material albedo colour, if any (else a default shade is used).
    pub albedo: Option<Color>,
    /// The authored `StandardMaterial3D` resolved from the node's
    /// `__material__*` companions, if it has one.
    ///
    /// Carried on the item because `populate_editor_scene_3d` has no
    /// `SceneTree` — resolution has to happen while the tree is still in scope,
    /// at construction. `None` keeps the previous flat-albedo behaviour, so a
    /// mesh without an authored material is unaffected (pat-0q2kx.42).
    pub material: Option<Material3D>,
    /// The node's `GeometryInstance3D` settings — `cast_shadow`,
    /// `visibility_range_*`, `transparency`, `material_overlay`, cull margins
    /// (pat-nzgdu.76).
    ///
    /// Resolved at construction for the same reason as `material`: the render
    /// pass downstream only receives items, with no `SceneTree` left to read.
    pub geometry: GeometryInstanceSettings,
    /// A `ShaderMaterial` on `material_override`, when the node has one.
    ///
    /// 🔑 Separate from [`material`](Self::material) because the two are
    /// different types: `Material3D` is the StandardMaterial3D equivalent
    /// (albedo/roughness/metallic) with no shader slot, and `ShaderMaterial3D`
    /// carries code plus parameters. `packed_scene` flattens a ShaderMaterial to
    /// `shader_code` / `shader_parameter/*` instead of the `__material__*` bag,
    /// so `material3d_for_node` correctly returns `None` for one — which is why
    /// this path drew every shader-material mesh flat (pat-mgixk.50).
    ///
    /// ⚠️ Carried, NOT collapsed to a constant albedo. `gdrender3d`'s software
    /// rasterizer evaluates these per fragment
    /// (`FragmentInstruction3D`/`ColorExpr3D` exist "so the software fallback
    /// can evaluate them per-fragment"), so flattening it here would discard
    /// UV math, uniforms, `TIME` and texture sampling the renderer can already
    /// do.
    pub shader_material: Option<gdserver3d::shader::ShaderMaterial3D>,
    /// Per-surface `surface_material_override/N` materials, indexed by surface.
    ///
    /// 🔑 A `Vec`, not an `Option<Material3D>`, because Godot overrides surfaces
    /// INDIVIDUALLY: a two-surface mesh with only `surface_material_override/1`
    /// set must keep surface 0's own material. [`material`](Self::material) is a
    /// single whole-mesh material and cannot express that, which is why these go
    /// to the renderer's separate `set_surface_materials` channel.
    ///
    /// `Vec::new()` for the overwhelming majority of nodes, and an empty vec is
    /// never sent, so a mesh without per-surface overrides is unaffected
    /// (pat-mgixk.50).
    pub surface_materials: Vec<Option<Material3D>>,
}

impl Viewport3D {
    /// Builds the 3D viewport's render list from `tree`: each **visible**
    /// `Node3D`-derived node becomes a [`Render3DItem`] — a mesh for mesh-bearing
    /// nodes, or a primitive marker otherwise. Hidden nodes are skipped (nothing
    /// to draw), and items appear in scene-tree order.
    pub fn build_render_list(&self, tree: &SceneTree) -> Vec<Render3DItem> {
        // One cache for the whole list: several meshes commonly share a texture,
        // and without this each would decode the same PNG again.
        //
        // 🔑 Kept across FRAMES too (pat-p3p1m): a fresh cache per call meant
        // every material texture was decoded from disk on every editor and
        // Play frame, under the editor-state lock. `drop_stale` re-decodes a
        // texture whose file changed. Stable `Arc`s are also what lets the GPU
        // renderer keep its uploads (it keys them by `Arc` identity).
        thread_local! {
            static MATERIAL_TEXTURES: std::cell::RefCell<gdscene::render_server_3d::MaterialTextureCache> =
                std::cell::RefCell::new(gdscene::render_server_3d::MaterialTextureCache::new());
        }
        MATERIAL_TEXTURES.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache.drop_stale();
            self.build_render_list_with(tree, &mut cache)
        })
    }

    fn build_render_list_with(
        &self,
        tree: &SceneTree,
        material_textures: &mut gdscene::render_server_3d::MaterialTextureCache,
    ) -> Vec<Render3DItem> {
        self.collect_scene_content(tree)
            .into_iter()
            .filter(|r| r.visible)
            // flat_map, not map: a GridMap expands into one item per used cell,
            // and a CSG descendant collapses to none (pat-s3rrv.89).
            .flat_map(|mut r| {
                // Fold the mesh sub-resource's authored properties into the marker
                // before the item leaves the tree behind (pat-0q2kx.43).
                r.mesh_path = r
                    .mesh_path
                    .take()
                    .map(|path| mesh_marker_with_props(tree, r.node_id, path));
                // Resolve the authored material HERE, while `tree` is still
                // available — the render pass downstream only receives items.
                let nid = gdscene::node::NodeId::from_object_id(gdcore::id::ObjectId::from_raw(
                    r.node_id,
                ));
                // A particle system draws its PARTICLES, never its draw-pass
                // mesh at the emitter; one that is not emitting has none alive
                // (pat-cezo4: the platformer's coin and enemy one-shot bursts
                // drew as static white/orange quads). There is no 3D particle
                // simulation on this path yet, so an emitting one keeps its old
                // stand-in.
                if matches!(r.class_name.as_str(), "CPUParticles3D" | "GPUParticles3D")
                    && tree.get_node(nid).is_some_and(|n| {
                        matches!(n.get_property("emitting"), gdvariant::Variant::Bool(false))
                    })
                {
                    return Vec::new();
                }
                let material = gdscene::render_server_3d::material3d_for_node(
                    tree,
                    nid,
                    material_textures,
                );
                // Same reason as `material`: read the GeometryInstance3D block
                // while the tree is still in scope (pat-nzgdu.76).
                let geometry =
                    gdscene::render_server_3d::geometry_instance_settings_for_node(tree, nid);
                // Read while `tree` is in scope, for the same reason `material`
                // is. `None` for every node without a ShaderMaterial, which is
                // the overwhelming majority (pat-mgixk.50).
                let mut shader_material =
                    gdscene::render_server_3d::RenderServer3DAdapter::resolve_shader_material(
                        tree, nid,
                    );
                // 🔑 Load each `sampler2D` uniform's image before the material
                // leaves the tree behind (pat-mgixk.50).
                // `resolve_shader_material` collects `shader_parameter/<name>`
                // into `parameters`, but a Variant holds a resource PATH, not
                // pixels, and the fragment stage samples the material's TEXTURE
                // MAP. So the first pass of this bead carried the ShaderMaterial
                // here with an empty map, and every `texture(tex, UV)` on the
                // editor/Play path sampled the missing-binding white placeholder
                // — the shader ran, and read white.
                //
                // ⚠️ `bind_shader_textures` used to be a PRIVATE method on
                // `RenderServer3DAdapter`, so this path could not reach it
                // without owning an adapter it does not have. It is now a free
                // function taking a cache, and this passes the SAME
                // `material_textures` the authored `material` and the
                // per-surface overrides already load through — so a texture
                // shared between a StandardMaterial3D and a shader uniform
                // decodes once.
                if let Some(mat) = shader_material.as_mut() {
                    gdscene::render_server_3d::bind_shader_textures(mat, material_textures);
                }
                // `surface_material_override/N`, resolved here for the same
                // reason — and through the SAME `surface_materials_for_node`
                // the render-server adapter uses, so the two paths cannot
                // disagree about a surface's material. It had no gdeditor
                // caller at all (pat-mgixk.50).
                let surface_materials = gdscene::render_server_3d::surface_materials_for_node(
                    tree,
                    nid,
                    material_textures,
                );
                // 🔑 MultiMeshInstance3D expands into one draw per VISIBLE
                // instance (pat-mgixk.50).
                //
                // This arm did not exist at all: `grep -i multimesh` in this
                // file and `viewport_3d.rs` returned ZERO, so every
                // MultiMeshInstance3D fell through to the 0.4 marker cube
                // however many instances it authored — while
                // `RenderServer3DAdapter` expanded it correctly and had no
                // production caller. Both now go through
                // `multimesh_instance_draws`, so the editor list and the render
                // server cannot disagree about how many blobs a scene has.
                //
                // ⚠️ Returns early, BEFORE the CSG and mesh-path arms below: a
                // multimesh has no single `mesh_path` to resolve, and falling
                // through is exactly how it became a marker.
                if let Some((mesh, instances)) =
                    gdscene::render_server_3d::multimesh_instance_draws(tree, nid)
                {
                    return instances
                        .into_iter()
                        .map(|(xform, color)| Render3DItem {
                            node_id: r.node_id,
                            global_transform: xform,
                            kind: Render3DKind::Geometry { mesh: mesh.clone() },
                            // The per-instance colour is the MultiMesh buffer's,
                            // which Godot multiplies into the instance's albedo.
                            albedo: Some(color),
                            material: material.clone(),
                            // MultiMeshInstance3D is a GeometryInstance3D, so its
                            // cast_shadow / visibility_range / transparency apply
                            // to every expanded entry (pat-nzgdu.76).
                            geometry: geometry.clone(),
                            // A MultiMeshInstance3D can carry a
                            // `material_override` too, and it applies to every
                            // expanded instance.
                            shader_material: shader_material.clone(),
                            // A MultiMesh draws ONE mesh resource, so a
                            // per-surface override on the node applies to every
                            // expanded instance alike.
                            surface_materials: surface_materials.clone(),
                        })
                        .collect();
                }
                // CSG: a root folds its whole boolean subtree into one mesh, and
                // its descendants draw NOTHING — they are already part of that
                // mesh. Without the skip a subtracted sphere would still appear
                // as its own marker cube next to the hole it cut (pat-s3rrv.89).
                let csg_mesh = if gdscene::render_server_3d::is_csg_root(tree, nid) {
                    match gdscene::render_server_3d::evaluate_csg_tree(tree, nid) {
                        Some(mesh) => Some(mesh),
                        // An empty combiner has no geometry; drawing its default
                        // marker cube would be worse than drawing nothing.
                        None => return Vec::new(),
                    }
                } else if gdscene::render_server_3d::is_csg_class_name(&r.class_name) {
                    return Vec::new();
                } else {
                    None
                };
                let is_gridmap = r.class_name == "GridMap";
                // Skinning and blend shapes (pat-s3rrv.23). A MeshInstance3D
                // bound to a Skeleton3D, or carrying blend-shape weights, draws
                // a DEFORMED mesh rather than the file's rest pose. Resolved
                // here while `tree` is still in scope, for the same reason
                // `material` is.
                //
                // ⚠️ Only actually-deformed meshes take this path — the helper
                // returns None otherwise, so every ordinary mesh keeps its
                // existing path-based kind and its downstream caching.
                //
                // ⚠️ The marker can carry a `?prop=value` query appended by
                // `mesh_marker_with_props`; the loader wants the bare path, and
                // a primitive marker simply fails to load and yields None.
                let deformed = r
                    .mesh_path
                    .as_deref()
                    .and_then(|marker| marker.split('?').next())
                    .and_then(mesh_from_file)
                    .and_then(|base| {
                        gdscene::render_server_3d::deformed_mesh_for_node(tree, nid, &base)
                    });
                // Sprite3D, AnimatedSprite3D and Label3D draw as a TEXTURED
                // QUAD, not as a mesh resource
                // (`SpriteBase3D::_draw`). It descends from Node3D, so it was
                // already in this list — with no mesh path and no shape, which
                // is exactly the fall-through that made it a grey marker cube
                // (pat-s3rrv.92). Built here, while `tree` is still in scope,
                // for the same reason `material` is.
                // 🔑 A mesh BUILT IN GDSCRIPT (`ImmediateMesh`, `SurfaceTool`)
                // arrives as a tagged `ObjectId` handle, never a path — so
                // `collect_scene_content`, which reads only `Variant::String`
                // mesh paths, left `mesh_path` empty and the node fell through
                // to the marker cube. `RenderServer3DAdapter::resolve_mesh`
                // already decodes those handles and had no caller outside the
                // adapter (pat-mgixk.50).
                //
                // ⚠️ Gated on the handle predicate, NOT called unconditionally:
                // `resolve_mesh` falls back to `Mesh3D::cube(1.0)` for anything
                // it cannot resolve, so calling it for every node would bake
                // every ordinary mesh into inline geometry — losing the
                // path-based kind and its downstream texture caching, and
                // turning "could not load" into a silent cube.
                let script_mesh = tree
                    .get_node(nid)
                    .and_then(|n| match n.get_property("mesh") {
                        gdvariant::Variant::ObjectId(oid)
                            if gdscript_interop::bindings::is_mesh_builder_handle(oid.raw()) =>
                        {
                            Some(
                                gdscene::render_server_3d::RenderServer3DAdapter::resolve_mesh(
                                    tree, nid,
                                ),
                            )
                        }
                        _ => None,
                    });
                let (generated_mesh, material) = match crate::sprite3d_render::billboard_geometry(
                    tree,
                    nid,
                    material_textures,
                ) {
                    Some((mesh, sprite_material)) => (Some(mesh), Some(sprite_material)),
                    // Deformed geometry outranks CSG here only because a node
                    // is never both: CSG roots carry no skin or blend shapes.
                    // Order matters only in that a node is never two of these
                    // at once: a CSG root carries no skin, and a script-built
                    // mesh is not a file path to deform.
                    None => (deformed.or(csg_mesh).or(script_mesh), material),
                };
                let mut base = render_item_for(
                    r,
                    material,
                    geometry,
                    generated_mesh,
                    shader_material,
                    surface_materials,
                );
                // `SpriteBase3D.billboard` / `Label3D.billboard` (pat-mgixk.34
                // section 2). Applied HERE rather than inside
                // `billboard_geometry` because the basis is what has to change:
                // the quad's vertices are in the node's local space and
                // `global_transform` is applied to them downstream, so rotating
                // the vertices would be rotated again by the node's own
                // transform. Godot likewise replaces the instance basis and
                // keeps the origin.
                //
                // ⚠️ `build_render_list` is where the camera lives, and this is
                // the one call site of `billboard_geometry` — the Play branch
                // and the editor viewport both come through here, so one
                // override serves both.
                //
                // ⚠️ The camera axes come from `camera.transform().basis`, NOT
                // from `camera.right()` / `camera.up()`. Those two derive from
                // the orbit `yaw`/`pitch`, and the Play path never sets them:
                // `viewport_for_scene_camera` installs the scene Camera3D as
                // `override_transform`, which ONLY `transform()` honours. Using
                // the orbit accessors would billboard every sprite toward the
                // default editor orientation and look right in the editor while
                // being wrong in the game — the half this bead is about.
                //
                // A camera basis is (right, up, -forward) in columns, per
                // `ViewportCamera3D::transform`.
                let cam_basis = self.camera.transform().basis;
                if let Some(basis) = crate::sprite3d_render::billboard_basis_for(
                    tree,
                    nid,
                    base.global_transform.basis,
                    cam_basis.x,
                    cam_basis.y,
                ) {
                    base.global_transform.basis = basis;
                } else if let Some(basis) = material_billboard_basis(
                    tree,
                    nid,
                    base.global_transform.basis,
                    cam_basis,
                ) {
                    base.global_transform.basis = basis;
                }
                if is_gridmap {
                    return gridmap_items(tree, nid, &base, material_textures);
                }
                fill_mesh_file_materials(tree, nid, &mut base, material_textures);
                vec![base]
            })
            .collect()
    }
}

/// Expands a `GridMap` into one render item per used cell (pat-s3rrv.89).
///
/// ⚠️ `GridMap::from_node` captures the mesh library's PATH; the items themselves
/// live in a separate `.tres` and nothing had ever loaded them, so every cell
/// resolved to no mesh and the node drew a single marker cube for the whole map.
/// Loading it here is what makes the cells appear at all.
///
/// ⚠️ Uses `mesh_instances_oriented`, not `mesh_instances`: the latter drops each
/// cell's rotation, and a tiled map drawn unrotated reads as badly authored art
/// rather than as missing data.
fn gridmap_items(
    tree: &SceneTree,
    nid: gdscene::NodeId,
    base: &Render3DItem,
    material_textures: &mut gdscene::render_server_3d::MaterialTextureCache,
) -> Vec<Render3DItem> {
    let Some(mut grid) = gdscene::grid_map::GridMap::from_node(tree, nid) else {
        return Vec::new();
    };
    if let Some(path) = grid.mesh_library_resource.clone() {
        // Either format (pat-33wmr); the items' meshes are `.res` ArrayMeshes
        // that `mesh_for_primitive` now decodes.
        if let Some(library) = gdscene::grid_map::MeshLibrary::load(&path) {
            grid.mesh_library = library;
        }
    }
    // One resolution per distinct tile, not per cell: a level is thousands of
    // cells over a dozen meshes.
    let mut tile_materials: std::collections::HashMap<String, Render3DItem> =
        std::collections::HashMap::new();
    grid.mesh_instances_oriented()
        .into_iter()
        .map(|(origin, basis, path)| {
            let template = tile_materials.entry(path.clone()).or_insert_with(|| {
                let mut t = base.clone();
                t.kind = Render3DKind::Mesh { path: path.clone() };
                fill_mesh_file_materials(tree, nid, &mut t, material_textures);
                t
            });
            Render3DItem {
                // The cell's placement is relative to the GridMap node, so it
                // composes with the node's own global transform rather than
                // replacing it.
                global_transform: base.global_transform * Transform3D { basis, origin },
                ..template.clone()
            }
        })
        .collect()
}

/// The basis a mesh whose StandardMaterial3D sets `billboard_mode` draws with
/// (pat-cezo4), or `None` for `BILLBOARD_DISABLED`.
///
/// Godot applies this in the material's vertex stage (`material.cpp`,
/// `BILLBOARD_ENABLED`: `MODELVIEW = VIEW * mat4(INV_VIEW columns, origin)`),
/// so a `MeshInstance3D` + `QuadMesh` with a billboard material faces the
/// camera however the node is turned — the official 3D platformer's coin
/// glow. Without `billboard_keep_scale` the node's scale is dropped, as in
/// Godot. `BILLBOARD_FIXED_Y` keeps the node's up axis.
fn material_billboard_basis(
    tree: &SceneTree,
    nid: gdscene::NodeId,
    node_basis: gdcore::math3d::Basis,
    cam_basis: gdcore::math3d::Basis,
) -> Option<gdcore::math3d::Basis> {
    let node = tree.get_node(nid)?;
    let mode = match node.get_property("__material__billboard_mode") {
        gdvariant::Variant::Int(m) => m,
        _ => return None,
    };
    let keep_scale = matches!(
        node.get_property("__material__billboard_keep_scale"),
        gdvariant::Variant::Bool(true)
    );
    let (sx, sy, sz) = if keep_scale {
        (node_basis.x.length(), node_basis.y.length(), node_basis.z.length())
    } else {
        (1.0, 1.0, 1.0)
    };
    match mode {
        // BILLBOARD_ENABLED: the camera's own axes.
        1 => Some(gdcore::math3d::Basis {
            x: cam_basis.x * sx,
            y: cam_basis.y * sy,
            z: cam_basis.z * sz,
        }),
        // BILLBOARD_FIXED_Y: up stays world/node up, facing turns about it.
        2 => {
            let up = node_basis.y.normalized();
            let fwd = cam_basis.z - up * cam_basis.z.dot(up);
            if fwd.length() < 1e-6 {
                return None;
            }
            let z = fwd.normalized();
            let x = up.cross(z).normalized();
            Some(gdcore::math3d::Basis {
                x: x * sx,
                y: up * sy,
                z: z * sz,
            })
        }
        // BILLBOARD_PARTICLES (3) is per-particle; not a mesh billboard here.
        _ => None,
    }
}

/// Gives a mesh-file item the materials its `ArrayMesh` surfaces carry, in the
/// slots nothing on the node overrode (pat-zwvgw).
///
/// Godot's order is `material_override`, then `surface_material_override/N`,
/// then the mesh surface's own material. The first two are already resolved
/// onto `item` (`material` / `surface_materials`); this supplies the third,
/// which no render path read — so every `.res` tile of the 3D platformer, and
/// every GridMap cell, drew untextured.
///
/// ⚠️ A `material_override` covers EVERY surface, and the renderers fall back
/// to `item.material` for a surface with no entry; filling the extra surfaces
/// under an override would replace the override there, so nothing is filled.
fn fill_mesh_file_materials(
    tree: &SceneTree,
    nid: gdscene::NodeId,
    item: &mut Render3DItem,
    material_textures: &mut gdscene::render_server_3d::MaterialTextureCache,
) {
    let Some(node) = tree.get_node(nid) else {
        return;
    };
    if !matches!(node.get_property("material_override"), gdvariant::Variant::Nil) {
        return;
    }
    // A glTF mesh — skinned ones arrive as deformed `Geometry` — draws with the
    // document's own materials, one per primitive (pat-zwvgw). Before this
    // every glb character drew in the default white material.
    let gltf = |path: &str| {
        let lower = path.to_ascii_lowercase();
        if !(lower.ends_with(".glb") || lower.ends_with(".gltf")) {
            return None;
        }
        let mesh_index = match node.get_property("gltf_mesh_index") {
            gdvariant::Variant::Int(i) => Some(i),
            _ => None,
        };
        let mut mats = gdscene::render_server_3d::gltf_surface_materials(path, mesh_index)
            .into_iter()
            .map(Some);
        Some((mats.next().flatten(), mats.collect::<Vec<_>>()))
    };
    let (first, rest) = match &item.kind {
        Render3DKind::Mesh { path } => {
            let file = path.split('?').next().unwrap_or(path);
            match gltf(file) {
                Some(m) => m,
                None => {
                    gdscene::render_server_3d::array_mesh_surface_materials(file, material_textures)
                }
            }
        }
        Render3DKind::Geometry { .. } => {
            let Some(m) = gdscene::node3d::get_mesh_path(tree, nid).and_then(|p| gltf(&p)) else {
                return;
            };
            m
        }
        _ => return,
    };
    if item.material.is_none() {
        item.material = first;
    }
    if rest.is_empty() {
        return;
    }
    if item.surface_materials.len() < rest.len() {
        item.surface_materials.resize(rest.len(), None);
    }
    for (slot, mesh_mat) in item.surface_materials.iter_mut().zip(rest) {
        if slot.is_none() {
            *slot = mesh_mat;
        }
    }
}

/// Maps a collected spatial node to its draw item.
fn render_item_for(
    r: Node3DRenderable,
    material: Option<Material3D>,
    geometry: GeometryInstanceSettings,
    generated_mesh: Option<Mesh3D>,
    shader_material: Option<gdserver3d::shader::ShaderMaterial3D>,
    surface_materials: Vec<Option<Material3D>>,
) -> Render3DItem {
    // Generated geometry wins over every path-based kind: a CSG root has no mesh
    // resource to load, and a Sprite3D's quad is built from its texture, so
    // falling through would draw either as a marker cube.
    let kind = if let Some(mesh) = generated_mesh {
        Render3DKind::Geometry { mesh }
    } else if let Some(shape) = r.shape_path.as_deref() {
        let (shape, half_extents) = collision_shape_for(shape);
        Render3DKind::Collision {
            shape,
            half_extents,
        }
    } else if let Some(path) = r.mesh_path {
        Render3DKind::Mesh { path }
    } else {
        Render3DKind::Primitive {
            shape: primitive_for_class(&r.class_name),
        }
    };
    Render3DItem {
        node_id: r.node_id,
        global_transform: r.global_transform,
        kind,
        albedo: r.albedo,
        material,
        geometry,
        shader_material,
        surface_materials,
    }
}

/// Chooses the collision wireframe kind and its half-extents from a
/// `shape:<Type>[|dims]` marker. `dims` (when present) carries the real shape
/// size: box `size` (full), sphere `radius`, capsule/cylinder `radius,height`.
fn collision_shape_for(shape: &str) -> (CollisionShapeKind, Vector3) {
    let body = shape.strip_prefix("shape:").unwrap_or("");
    let (ty, dims) = match body.split_once('|') {
        Some((t, d)) => (t, Some(d)),
        None => (body, None),
    };
    let nums: Vec<f32> = dims
        .map(|d| d.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default();
    let n = |i: usize, d: f32| nums.get(i).copied().unwrap_or(d);
    match ty {
        "SphereShape3D" => {
            let r = n(0, 0.5);
            (CollisionShapeKind::Sphere, Vector3::new(r, r, r))
        }
        "CapsuleShape3D" => {
            let r = n(0, 0.5);
            (
                CollisionShapeKind::Capsule,
                Vector3::new(r, n(1, 2.0) * 0.5, r),
            )
        }
        "CylinderShape3D" => {
            let r = n(0, 0.5);
            (
                CollisionShapeKind::Cylinder,
                Vector3::new(r, n(1, 2.0) * 0.5, r),
            )
        }
        // BoxShape3D and anything else: dims are the full size, so halve them.
        _ => (
            CollisionShapeKind::Box,
            Vector3::new(n(0, 1.0) * 0.5, n(1, 1.0) * 0.5, n(2, 1.0) * 0.5),
        ),
    }
}

/// Resolves a `MeshInstance3D`'s mesh geometry from its `primitive:<Type>` path,
/// **memoized per thread** (pat-ism62 #2): the editor rebuilds its render list
/// every frame, so without a cache the procedural tessellation (`Mesh3D::sphere`,
/// `torus`, `capsule`, …) would re-run each frame for every mesh. Geometry is
/// deterministic per path, so we generate it once and clone from the cache — a
/// clone is a cheap memcpy vs. the trig-heavy tessellation. The cache is naturally
/// bounded by the fixed set of primitive descriptors.
/// `pub(crate)` so the Mesh toolbar's collision generator can read the same
/// geometry the viewport draws (pat-0q2kx.169) — a collision shape built from
/// different geometry than the rendered mesh would be a silent mismatch.
/// ⚠️ `pub` rather than `pub(crate)` so the editor's geometry is observable
/// from outside the crate (pat-s3rrv.199). The bead's wiring requirement is
/// that one resolver serve both views and that the same sub-resource "produce
/// the same geometry in both" — which cannot be asserted through
/// `editor_3d_render_inputs` (its `Render3DKind::Mesh` carries a path, not
/// vertices) or `render_editor_3d_frame` (pixels, not counts). This is the
/// editor's real resolution entry point, used at both of its own call sites,
/// so a cross-view test through it exercises the editor's route rather than a
/// lookalike.
pub fn mesh_for_primitive(path: &str) -> Mesh3D {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static MESH_CACHE: RefCell<HashMap<String, Mesh3D>> = RefCell::new(HashMap::new());
    }
    MESH_CACHE.with(|cache| {
        if let Some(mesh) = cache.borrow().get(path) {
            return mesh.clone();
        }
        let mesh = build_mesh_for_primitive(path);
        cache.borrow_mut().insert(path.to_string(), mesh.clone());
        mesh
    })
}

/// The uncached geometry builder behind [`mesh_for_primitive`].
///
/// Accepts `primitive:<Type>` optionally followed by `?k=v&k=v` carrying the mesh
/// sub-resource's authored properties (pat-0q2kx.43). Those properties travel in
/// the path string because the path is the only channel from the scene tree to the
/// render body — [`render_editor_3d_frame_from_inputs`] deliberately reads no tree
/// — and because [`mesh_for_primitive`]'s cache is path-keyed: without them, two
/// differently-sized boxes would collide on one cache entry and the second would
/// silently render at the first one's size.
///
/// Geometry comes from the real `gdserver3d` generators, so a `BoxMesh` with
/// `size = (4, 1, 4)` renders 4 units wide instead of a fixed 0.8 cube. Unspecified
/// properties fall back to each generator's `Default`, which already matches
/// Godot's documented defaults.
fn build_mesh_for_primitive(path: &str) -> Mesh3D {
    use gdserver3d::primitive_mesh::BoxMesh;

    let (marker, (props, text_props)) = split_mesh_marker(path);
    // A file-backed mesh (.obj, or a .glb/.gltf model since pat-s3rrv.31)
    // renders its own geometry rather than the placeholder marker below.
    if let Some(mesh) = mesh_from_file(marker) {
        return mesh;
    }

    // ⚠️ One resolver, shared with the runtime renderer (pat-s3rrv.199).
    // This used to be a private match with its own fallback numbers, which is
    // exactly how the editor came to draw a torus of 0.22/0.45/24/12 where
    // Godot documents 0.5/1.0/64/32 — and where the runner drew a default cube.
    // Godot's defaults now live in `build_primitive` and nowhere else, so the
    // two views cannot drift apart again.
    let class = marker.strip_prefix("primitive:").unwrap_or("");
    let authored = |key: &str| props.get(key).copied();

    // Unknown or unloadable mesh paths fall back to the default box — the same
    // geometry `primitive:BoxMesh` yields, so "we could not load this" and "a
    // plain box" are one placeholder rather than two subtly different ones.
    let authored_str = |key: &str| text_props.get(key).cloned();

    gdserver3d::primitive_mesh::build_primitive_with_text(class, &authored, &authored_str)
        .unwrap_or_else(|| BoxMesh::default().generate())
}

/// The mesh properties a `.tscn` sub-resource can carry, copied onto the node as
/// `__mesh__*` companions the way shapes use `__shape__*` (pat-0q2kx.43).
const MESH_PROP_KEYS: [&str; 17] = [
    "radius",
    "height",
    "radial_segments",
    "rings",
    "top_radius",
    "bottom_radius",
    "left_to_right",
    "inner_radius",
    "outer_radius",
    "ring_segments",
    // PlaneMesh/QuadMesh, pat-s3rrv.198. Without `orientation` here an authored
    // FACE_X/FACE_Z plane silently fell back to its class default, so the two
    // classes were distinguishable only by their defaults and never by what the
    // scene actually said.
    "orientation",
    "subdivide_width",
    "subdivide_depth",
    // RibbonTrailMesh, pat-s3rrv.198. `curve` is a resource reference, not a
    // number, so it cannot travel through this numeric bag.
    "sections",
    "section_length",
    "section_segments",
    "shape",
];

/// Appends a mesh node's authored sub-resource properties to its marker path as a
/// `?k=v` query, so the geometry builder and the path-keyed cache both see them.
///
/// Values are read from `__mesh__<prop>` companions — the same convention the
/// `.tscn` instancing path already uses for collision shapes (`__shape__*`). A node
/// with no companions yields its marker unchanged, so nothing regresses for scenes
/// that never authored mesh properties.
/// The exact geometry the viewport draws for `node_id`, or `None` when the node
/// names no mesh (pat-0q2kx.169).
///
/// The Mesh toolbar's collision generator goes through here rather than
/// re-deriving geometry: it resolves the same `__viewport_mesh__` marker and
/// folds in the same `__mesh__*` sub-resource properties, so a generated
/// collision shape cannot silently disagree with what is on screen.
pub(crate) fn mesh_geometry_for_node(
    tree: &SceneTree,
    node_id: gdscene::node::NodeId,
) -> Option<Mesh3D> {
    let base = gdscene::node3d::get_mesh_path(tree, node_id)?;
    let marker = mesh_marker_with_props(tree, node_id.raw(), base);
    Some(mesh_for_primitive(&marker))
}

fn mesh_marker_with_props(tree: &SceneTree, node_id: u64, path: String) -> String {
    if !path.starts_with("primitive:") {
        return path;
    }
    let nid = gdscene::node::NodeId::from_object_id(gdcore::id::ObjectId::from_raw(node_id));
    let Some(node) = tree.get_node(nid) else {
        return path;
    };

    let mut query: Vec<String> = Vec::new();
    // `size` is a Vector3 (or Vector2 for PlaneMesh) and flattens per component.
    match node.get_property("__mesh__size") {
        gdvariant::Variant::Vector3(v) => {
            query.push(format!("size.x={}", v.x));
            query.push(format!("size.y={}", v.y));
            query.push(format!("size.z={}", v.z));
        }
        gdvariant::Variant::Vector2(v) => {
            query.push(format!("size.x={}", v.x));
            query.push(format!("size.y={}", v.y));
        }
        _ => {}
    }
    // `center_offset` (PlaneMesh) is a Vector3 and flattens the same way
    // (pat-s3rrv.198). ⚠️ It is NOT in MESH_PROP_KEYS: that loop only reads
    // scalars, and a Vector3 there would be skipped silently rather than
    // failing — the exact shape of bug that leaves a property looking wired.
    if let gdvariant::Variant::Vector3(v) = node.get_property("__mesh__center_offset") {
        query.push(format!("center_offset.x={}", v.x));
        query.push(format!("center_offset.y={}", v.y));
        query.push(format!("center_offset.z={}", v.z));
    }
    for key in MESH_PROP_KEYS {
        match node.get_property(&format!("__mesh__{key}")) {
            gdvariant::Variant::Float(f) => query.push(format!("{key}={f}")),
            gdvariant::Variant::Int(i) => query.push(format!("{key}={i}")),
            _ => {}
        }
    }
    if query.is_empty() {
        return path;
    }
    format!("{path}?{}", query.join("&"))
}

/// Splits a mesh marker into its type and a numeric property bag, accepting both
/// encodings the codebase uses (pat-0q2kx.43).
///
/// * `primitive:BoxMesh|4,1,4` — the **positional** form, matching
///   `packed_scene::encode_shape_marker` / [`collision_shape_for`]. This is what a
///   `.tscn` load emits, so the order below is a contract with the encoder.
/// * `primitive:BoxMesh?size.x=4&size.y=1` — the **named** form, produced by
///   [`mesh_marker_with_props`] from `__mesh__*` companions on the node.
///
/// Malformed or non-numeric entries are skipped rather than poisoning the mesh.
/// Splits a `primitive:<Class>` marker into its class and authored properties.
///
/// Returns numeric properties and string properties separately: a `TextMesh`'s
/// `text` and `font` cannot be parsed as `f32`, and the editor resolves a mesh
/// from this string alone (pat-s3rrv.199). String values are percent-encoded by
/// the scene loader, since `?`, `&`, `=` and spaces would otherwise split the
/// marker in the wrong place.
type MarkerProps = (
    std::collections::HashMap<String, f32>,
    std::collections::HashMap<String, String>,
);

fn split_mesh_marker(path: &str) -> (&str, MarkerProps) {
    let mut props = std::collections::HashMap::new();
    let mut text_props: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // Named form first: it cannot be confused with the positional one.
    if let Some((marker, query)) = path.split_once('?') {
        for pair in query.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                if let Ok(parsed) = value.parse::<f32>() {
                    props.insert(key.to_string(), parsed);
                } else {
                    // Not a number: a string property, percent-decoded.
                    text_props.insert(
                        key.to_string(),
                        gdscene::packed_scene::percent_decode_marker_value(value),
                    );
                }
            }
        }
        return (marker, (props, text_props));
    }

    let Some((marker, dims)) = path.split_once('|') else {
        return (path, (props, text_props));
    };
    let nums: Vec<f32> = dims
        .split(',')
        .filter_map(|x| x.trim().parse::<f32>().ok())
        .collect();
    // Positional order per mesh type — the encoder writes these in this order.
    let keys: &[&str] = match marker.strip_prefix("primitive:").unwrap_or("") {
        "BoxMesh" | "CubeMesh" | "PrismMesh" => &["size.x", "size.y", "size.z", "left_to_right"],
        "SphereMesh" | "CapsuleMesh" => &["radius", "height", "radial_segments", "rings"],
        "CylinderMesh" => &[
            "top_radius",
            "bottom_radius",
            "height",
            "radial_segments",
            "rings",
        ],
        "PlaneMesh" | "QuadMesh" => &["size.x", "size.y"],
        "TorusMesh" => &["inner_radius", "outer_radius", "rings", "ring_segments"],
        _ => &[],
    };
    for (key, value) in keys.iter().zip(nums) {
        props.insert((*key).to_string(), value);
    }
    (marker, (props, text_props))
}

/// Loads geometry for a mesh path that names a file rather than a primitive.
///
/// `.obj` is parsed with [`Mesh3D::from_obj`]. `res://` resolves against the
/// current working directory, which is where the editor runs a project from.
/// Anything unreadable or unparseable returns `None` so the caller keeps its
/// placeholder — a missing asset should not blank the viewport.
fn mesh_from_file(path: &str) -> Option<Mesh3D> {
    let lowered = path.to_ascii_lowercase();
    let on_disk = path.strip_prefix("res://").unwrap_or(path);

    // pat-s3rrv.31: glTF/glb. Both halves of this already existed and never met
    // — `gdresource::gltf_import::import_gltf` produces the `Resource`, and
    // `mesh3d_from_gltf_resource` converts it to a `Mesh3D`, but the converter's
    // only caller was inside `RenderServer3DAdapter`, which has no production
    // callers at all. So the editor's sole file-backed loader accepted `.obj`
    // and nothing else, and every glTF model fell through to the placeholder
    // marker cube.
    if lowered.ends_with(".glb") || lowered.ends_with(".gltf") {
        let resource = gdresource::gltf_import::import_gltf(std::path::Path::new(on_disk)).ok()?;
        return gdscene::render_server_3d::mesh3d_from_gltf_resource(&resource);
    }

    // pat-33wmr: a saved ArrayMesh — every GridMap tile of a real Godot
    // MeshLibrary is one — used to fall through to the placeholder box.
    if gdscene::array_mesh::is_mesh_resource_path(path) {
        return gdscene::array_mesh::load_array_mesh_file(path);
    }

    if !lowered.ends_with(".obj") {
        return None;
    }
    let source = std::fs::read_to_string(on_disk).ok()?;
    Mesh3D::from_obj(&source).ok()
}

/// Chooses the primitive marker for a non-mesh spatial node from its class.
fn primitive_for_class(class: &str) -> PrimitiveShape {
    if class.contains("Camera") {
        PrimitiveShape::Camera
    } else if class.contains("Light") {
        PrimitiveShape::Light
    } else {
        PrimitiveShape::Box
    }
}

/// A scene `DirectionalLight3D`'s render inputs: the direction it shines plus
/// the properties that drive both surface shading and the sky's sun disc.
#[derive(Debug, Clone, Copy)]
pub struct SceneLight3D {
    /// Which Light3D family this came from — the renderer needs a different light
    /// type and a different set of parameters for each (pat-0q2kx.41).
    pub kind: SceneLightKind,
    /// World-space position of the light. Meaningless for `Directional`, which is
    /// infinitely far away; load-bearing for omni and spot.
    pub position: Vector3,
    /// `omni_range` / `spot_range`: the distance at which the light falls to zero.
    pub range: f32,
    /// `omni_attenuation` / `spot_attenuation`: falloff curve exponent.
    pub attenuation: f32,
    /// `spot_angle` in degrees (half-angle of the cone).
    pub spot_angle: f32,
    /// `spot_angle_attenuation`: falloff across the cone.
    pub spot_angle_attenuation: f32,
    /// `shadow_enabled` — the renderer only generates a shadow map for a light
    /// that asks for one, so this is what makes shadows appear at all.
    pub shadow_enabled: bool,
    /// World-space direction the light shines (its global −Z), normalized.
    pub direction: Vector3,
    /// `light_color` (sRGB authoring).
    pub color: Color,
    /// `light_energy`.
    pub energy: f32,
    /// `light_angular_distance` in degrees (sun disc size).
    pub angular_distance: f32,
    /// Whether this light contributes to sky rendering (its sun disc). Godot's
    /// `sky_mode`: `LIGHT_ONLY` sets this false so the light shades the scene
    /// but draws no disc in the procedural/physical sky.
    pub sky: bool,
    /// Whether this light shades scene geometry. Godot's `sky_mode = SKY_ONLY`
    /// sets this false: the light draws its sky sun disc but adds no surface
    /// lighting (the complement of [`sky`](Self::sky)).
    pub affects_scene: bool,
    /// The rest of Godot's Light3D property set, read by the SHARED reader
    /// `gdscene::render_server_3d::sync_light_properties` (pat-s3rrv.90).
    ///
    /// ⚠️ Stored as scalars rather than by embedding `gdserver3d::light::Light3D`
    /// because this struct is `Copy` and `Light3D` is not — embedding it would
    /// quietly drop the `Copy` derive and change every use site.
    ///
    /// ⚠️ **This list used to say only the temperature reached pixels. That is
    /// no longer true** — the rasterizer now consumes `cull_mask`,
    /// `shadow_opacity`, `shadow_bias`, `shadow_normal_bias`, `specular`,
    /// `negative`, the `distance_fade_*` set and the directional shadow fade,
    /// on both the software and GPU paths.
    ///
    /// Still carried but NOT consumed by any shading code: `indirect_energy`
    /// and the `shadow_split_1/2/3`/`blend_splits` cascade controls. Checked
    /// 2026-08-20 for read sites across `gdrender3d/src` and `gdserver3d/src`;
    /// those four appear only as declarations and default initialisers.
    ///
    /// ⚠️ **Two entries were removed from this list because they were wrong**,
    /// and a third claim with them:
    ///
    /// * `shadow_caster_mask` IS consumed — `gdrender3d/src/shadow_map.rs:291`.
    /// * `volumetric_fog_energy` IS consumed — `gdrender3d/src/renderer.rs:2251`
    ///   scales the sky scatter term by it.
    /// * "no GI ... path exists" was false: `gdserver3d/src/gi.rs` bakes
    ///   lightmaps, and it is also where `light_projector` is sampled
    ///   (`gi.rs:512`, `projector_modulate`). What `light_projector` lacks is a
    ///   *rasterizer* path — `grep projector gdrender3d/src/*.rs` finds nothing.
    ///
    /// A stale "not consumed" list is worse than no list: it sends the next
    /// worker to re-implement something that already works. This one nearly
    /// did (pat-s3rrv.90).
    ///
    /// The temperature remains the odd one out: it reaches pixels only because
    /// the shared reader folds it into [`color`](Self::color), which the
    /// rasterizer reads knowing nothing about temperature.
    pub temperature: f32,
    pub cull_mask: u32,
    pub indirect_energy: f32,
    pub volumetric_fog_energy: f32,
    pub distance_fade_enabled: bool,
    pub distance_fade_begin: f32,
    pub distance_fade_shadow: f32,
    pub distance_fade_length: f32,
    pub shadow_opacity: f32,
    pub shadow_caster_mask: u32,
    pub shadow_split_1: f32,
    pub shadow_split_2: f32,
    pub shadow_split_3: f32,
    pub blend_splits: bool,
    /// Godot `Light3D.editor_only`: the light shades the **editor viewport**
    /// but contributes nothing to a running game (pat-0q2kx.110).
    ///
    /// Kept as a flag rather than filtered at collection time because the same
    /// snapshot feeds both paths — the editor keeps these lights and only
    /// [`strip_editor_only_items`] (the Play path) drops them.
    /// `shadow_bias` / `shadow_normal_bias` — the offsets that keep a surface
    /// from shadowing itself. Consumed by `compute_shadow_factor` on the
    /// software path and by the WGSL on the GPU one (pat-s3rrv.90).
    pub shadow_bias: f32,
    pub shadow_normal_bias: f32,
    /// `light_specular` — how much this light contributes to highlights.
    /// ⚠️ The positional x2 is applied in the shader, not here, so this stays
    /// the authored value.
    pub specular: f32,
    /// `light_negative` — the light SUBTRACTS instead of adding. Godot does it
    /// as a sign flip on the energy, so it must not become a colour change.
    pub negative: bool,
    /// `directional_shadow_max_distance` / `directional_shadow_fade_start` —
    /// where a directional light's shadow dissolves out. Inert for positional
    /// lights, which are bounded by their own `range`.
    pub directional_shadow_max_distance: f32,
    pub directional_shadow_fade_start: f32,
    pub editor_only: bool,
}

/// Collects the scene's `DirectionalLight3D` nodes in tree order (capped at 4,
/// matching Godot's sky directional-light limit), reading each one's direction,
/// color, energy, and angular size — so the viewport lights the scene *and*
/// draws sun discs exactly like the light configured in the scene, not just a
/// generic sun. Empty when the scene has no directional light (the caller then
/// falls back to the preview sun).
/// Which `Light3D` subclass a [`SceneLight3D`] came from (pat-0q2kx.41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneLightKind {
    /// `DirectionalLight3D` — infinite, direction only.
    Directional,
    /// `OmniLight3D` — a point light with a range.
    Omni,
    /// `SpotLight3D` — a cone from a position.
    Spot,
}

/// Every light the scene contributes: the directional lights above plus each
/// `OmniLight3D` and `SpotLight3D` with its own range/attenuation/cone
/// (pat-0q2kx.41).
///
/// Before this, the wired render path collected **only** directional lights, so an
/// `OmniLight3D` or `SpotLight3D` in the scene emitted nothing and appeared solely
/// as an editor marker cube — the renderer had supported both all along.
///
/// Directional lights stay capped at 4 (Godot's sky sun-disc limit); omni and spot
/// share a further budget so a pathological scene cannot unbound the light loop.
fn scene_lights(tree: &SceneTree) -> Vec<SceneLight3D> {
    let mut lights = scene_directional_lights(tree);

    // 🔑 The positional budget is whatever the frame's light array has LEFT, not
    // a second independent number.
    //
    // It used to be a hardcoded 8 beside the directional cap of 4, so a scene
    // could hand over at most 12 lights while the GPU path uploads
    // `array<LightData, 16>` — four slots permanently unused. Worse, the two
    // numbers were free to drift: the renderer truncates past its array SILENTLY,
    // so raising either side alone would have started dropping lights with no
    // diagnostic. Deriving it from the shared `MAX_SCENE_LIGHTS` makes the
    // collector and both renderers agree by construction.
    let max_positional = gdserver3d::light::MAX_SCENE_LIGHTS.saturating_sub(lights.len());

    let mut positional = 0usize;
    for nid in tree.all_nodes_in_tree_order() {
        if positional >= max_positional {
            break;
        }
        let Some(node) = tree.get_node(nid) else {
            continue;
        };
        let kind = if node.is_class("SpotLight3D") {
            SceneLightKind::Spot
        } else if node.is_class("OmniLight3D") {
            SceneLightKind::Omni
        } else {
            continue;
        };
        if !gdscene::node3d::is_visible_in_tree(tree, nid) {
            continue;
        }

        let transform = gdscene::node3d::get_global_transform(tree, nid);
        let z = transform.basis.z;
        let dir = Vector3::new(-z.x, -z.y, -z.z);
        let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
        let direction = if len > 1e-6 {
            Vector3::new(dir.x / len, dir.y / len, dir.z / len)
        } else {
            Vector3::new(0.0, -1.0, 0.0)
        };

        // doc/classes/{OmniLight3D,SpotLight3D}.xml defaults: range 5.0,
        // attenuation 1.0, spot_angle 45°, spot_angle_attenuation 1.0.
        let (range, attenuation) = match kind {
            SceneLightKind::Spot => (
                gdscene::node3d::get_spot_range(tree, nid) as f32,
                gdscene::node3d::get_spot_attenuation(tree, nid) as f32,
            ),
            _ => (
                float_prop_3d_render(tree, nid, "omni_range", 5.0),
                float_prop_3d_render(tree, nid, "omni_attenuation", 1.0),
            ),
        };

        let synced = synced_light(
            tree,
            nid,
            kind,
            gdscene::node3d::get_light_color(tree, nid),
            gdscene::node3d::get_light_energy(tree, nid) as f32,
        );
        lights.push(SceneLight3D {
            kind,
            position: transform.origin,
            range,
            attenuation,
            spot_angle: gdscene::node3d::get_spot_angle(tree, nid) as f32,
            spot_angle_attenuation: gdscene::node3d::get_spot_angle_attenuation(tree, nid) as f32,
            shadow_enabled: bool_prop_3d_render(tree, nid, "shadow_enabled"),
            direction,
            // From the shared reader, so the temperature tint it folded in
            // survives (pat-s3rrv.90).
            color: synced.color,
            energy: synced.energy,
            angular_distance: 0.0,
            // Only directional lights draw a sky sun disc.
            sky: false,
            affects_scene: true,
            temperature: synced.temperature,
            cull_mask: synced.cull_mask,
            shadow_bias: synced.shadow_bias,
            shadow_normal_bias: synced.shadow_normal_bias,
            specular: synced.specular,
            negative: synced.negative,
            directional_shadow_max_distance: synced.directional_shadow_max_distance,
            directional_shadow_fade_start: synced.directional_shadow_fade_start,
            indirect_energy: synced.indirect_energy,
            volumetric_fog_energy: synced.volumetric_fog_energy,
            distance_fade_enabled: synced.distance_fade_enabled,
            distance_fade_begin: synced.distance_fade_begin,
            distance_fade_shadow: synced.distance_fade_shadow,
            distance_fade_length: synced.distance_fade_length,
            shadow_opacity: synced.shadow_opacity,
            shadow_caster_mask: synced.shadow_caster_mask,
            shadow_split_1: synced.shadow_split_1,
            shadow_split_2: synced.shadow_split_2,
            shadow_split_3: synced.shadow_split_3,
            blend_splits: synced.blend_splits,
            editor_only: bool_prop_3d_render(tree, nid, "editor_only"),
        });
        positional += 1;
    }
    lights
}

/// Reads an `f32` property, defaulting to `default`.
fn float_prop_3d_render(tree: &SceneTree, nid: gdscene::NodeId, key: &str, default: f32) -> f32 {
    match tree.get_node(nid).map(|n| n.get_property(key)) {
        Some(gdvariant::Variant::Float(f)) => f as f32,
        Some(gdvariant::Variant::Int(i)) => i as f32,
        _ => default,
    }
}

/// Reads a bool property, defaulting to `false`.
fn bool_prop_3d_render(tree: &SceneTree, nid: gdscene::NodeId, key: &str) -> bool {
    matches!(
        tree.get_node(nid).map(|n| n.get_property(key)),
        Some(gdvariant::Variant::Bool(true))
    )
}

/// Runs the shared property reader over `nid`, seeded with the authored colour
/// and energy (pat-s3rrv.90).
///
/// ⚠️ The colour goes in FIRST and comes back out again, because
/// `sync_light_properties` folds `light_temperature` into whatever colour the
/// light already carries. Reading `light_color` afterwards would overwrite the
/// tint with the untinted authored value and make temperature a no-op — which is
/// the bug this bead is about, reintroduced one layer down.
fn synced_light(
    tree: &SceneTree,
    nid: gdscene::NodeId,
    kind: SceneLightKind,
    base_color: Color,
    energy: f32,
) -> Light3D {
    // ⚠️ SEED WITH THE LIGHT'S OWN CLASS. `sync_light_properties` only
    // overwrites what the node actually authored, so every unauthored property
    // keeps the SEED's default — and three of them differ per class:
    //
    //     shadow_bias         directional 0.1   omni 0.1   spot 0.03
    //     shadow_normal_bias  directional 2.0   omni 1.0   spot 1.0
    //     light_specular      directional 1.0   omni 0.5   spot 0.5
    //
    // Seeding everything as directional (as this did) gave an unauthored omni
    // light a normal bias of 2.0 and a specular of 1.0 instead of 1.0 and 0.5.
    // That was inert while `setup_editor_lighting_3d` dropped those fields;
    // the moment they are carried through it becomes a wrong picture for every
    // omni and spot light in every scene.
    let mut light = match kind {
        SceneLightKind::Directional => Light3D::directional(Light3DId(0)),
        SceneLightKind::Omni => Light3D::point(Light3DId(0), Vector3::ZERO),
        // Position and direction are placeholders — the caller supplies the
        // real ones; only this seed's DEFAULTS are load-bearing here.
        SceneLightKind::Spot => {
            Light3D::spot(Light3DId(0), Vector3::ZERO, Vector3::new(0.0, -1.0, 0.0))
        }
    };
    light.color = base_color;
    light.energy = energy;
    if let Some(node) = tree.get_node(nid) {
        gdscene::render_server_3d::sync_light_properties(node, &mut light);
    }
    light
}

fn scene_directional_lights(tree: &SceneTree) -> Vec<SceneLight3D> {
    let mut lights = Vec::new();
    for nid in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(nid) else {
            continue;
        };
        if !node.is_class("DirectionalLight3D") {
            continue;
        }
        // pat-mgixk.32: a hidden light contributes nothing, exactly as the
        // omni/spot path above already required. This loop had NO visibility
        // check at all, so hiding the sun left the scene lit — and because the
        // omni/spot collector did check, the two light families disagreed about
        // what `visible` means.
        if !gdscene::node3d::is_visible_in_tree(tree, nid) {
            continue;
        }
        let z = gdscene::node3d::get_global_transform(tree, nid).basis.z;
        let dir = Vector3::new(-z.x, -z.y, -z.z);
        let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
        if len <= 1e-6 {
            continue;
        }
        let sky_mode = gdscene::node3d::get_light_sky_mode(tree, nid);
        let synced = synced_light(
            tree,
            nid,
            SceneLightKind::Directional,
            gdscene::node3d::get_light_color(tree, nid),
            gdscene::node3d::get_light_energy(tree, nid) as f32,
        );
        lights.push(SceneLight3D {
            kind: SceneLightKind::Directional,
            // A directional light is infinitely far away, so only its direction
            // matters; the positional fields stay at their neutral values.
            position: Vector3::ZERO,
            range: 0.0,
            attenuation: 1.0,
            spot_angle: 0.0,
            spot_angle_attenuation: 1.0,
            shadow_enabled: bool_prop_3d_render(tree, nid, "shadow_enabled"),
            direction: Vector3::new(dir.x / len, dir.y / len, dir.z / len),
            color: synced.color,
            energy: synced.energy,
            angular_distance: gdscene::node3d::get_light_angular_distance(tree, nid) as f32,
            // Godot sky_mode: LIGHT_ONLY (1) draws no sky sun disc; SKY_ONLY (2)
            // draws the disc but does not shade the scene; LIGHT_AND_SKY (0) both.
            sky: sky_mode != 1,
            affects_scene: sky_mode != 2,
            temperature: synced.temperature,
            cull_mask: synced.cull_mask,
            shadow_bias: synced.shadow_bias,
            shadow_normal_bias: synced.shadow_normal_bias,
            specular: synced.specular,
            negative: synced.negative,
            directional_shadow_max_distance: synced.directional_shadow_max_distance,
            directional_shadow_fade_start: synced.directional_shadow_fade_start,
            indirect_energy: synced.indirect_energy,
            volumetric_fog_energy: synced.volumetric_fog_energy,
            distance_fade_enabled: synced.distance_fade_enabled,
            distance_fade_begin: synced.distance_fade_begin,
            distance_fade_shadow: synced.distance_fade_shadow,
            distance_fade_length: synced.distance_fade_length,
            shadow_opacity: synced.shadow_opacity,
            shadow_caster_mask: synced.shadow_caster_mask,
            shadow_split_1: synced.shadow_split_1,
            shadow_split_2: synced.shadow_split_2,
            shadow_split_3: synced.shadow_split_3,
            blend_splits: synced.blend_splits,
            editor_only: bool_prop_3d_render(tree, nid, "editor_only"),
        });
        if lights.len() >= 4 {
            break;
        }
    }
    lights
}

// ---------------------------------------------------------------------------
// Pixel rendering (pat-7i8u4)
// ---------------------------------------------------------------------------
//
// The render list above is data; nothing turned it into pixels, so the editor's
// 3D mode kept showing the 2D rasterizer. These functions render the editor
// viewport (camera + grid + the scene's spatial nodes) to a real perspective
// frame via the software 3D renderer, so 3D mode is no longer cosmetic.

/// Minor grid line color (one line per world unit). Drawn faint; far lines fade.
const GRID_MINOR_COLOR_3D: Color = Color {
    r: 0.55,
    g: 0.58,
    b: 0.65,
    a: 1.0,
};
/// Major grid line color (every 10 units) — brighter so the scale reads.
const GRID_MAJOR_COLOR_3D: Color = Color {
    r: 0.66,
    g: 0.69,
    b: 0.77,
    a: 1.0,
};
/// Origin/gizmo axis colors — Godot convention: X = red, Y = green, Z = blue.
const X_AXIS_COLOR_3D: Color = Color {
    r: 0.86,
    g: 0.32,
    b: 0.34,
    a: 1.0,
};
const Y_AXIS_COLOR_3D: Color = Color {
    r: 0.55,
    g: 0.78,
    b: 0.30,
    a: 1.0,
};
const Z_AXIS_COLOR_3D: Color = Color {
    r: 0.33,
    g: 0.55,
    b: 0.88,
    a: 1.0,
};
/// The uniform viewport backdrop behind the 3D scene (Godot's dark editor gray).
const VIEWPORT_BG_3D: Color = Color {
    r: 0.13,
    g: 0.14,
    b: 0.17,
    a: 1.0,
};
/// Selection outline color drawn around the picked node (Godot's selection orange).
const SELECTION_COLOR_3D: Color = Color {
    r: 1.0,
    g: 0.62,
    b: 0.20,
    a: 1.0,
};
const MESH_COLOR_3D: Color = Color {
    r: 0.78,
    g: 0.80,
    b: 0.85,
    a: 1.0,
};
const CAMERA_COLOR_3D: Color = Color {
    r: 0.45,
    g: 0.75,
    b: 1.0,
    a: 1.0,
};
const LIGHT_COLOR_3D: Color = Color {
    r: 1.0,
    g: 0.85,
    b: 0.35,
    a: 1.0,
};
const BOX_COLOR_3D: Color = Color {
    r: 0.90,
    g: 0.62,
    b: 0.20,
    a: 1.0,
};
/// Collision-shape wireframe colour (Godot's teal collision gizmos).
const COLLISION_COLOR_3D: Color = Color {
    r: 0.10,
    g: 0.75,
    b: 0.78,
    a: 1.0,
};

fn solid_material_3d(albedo: Color) -> Material3D {
    let mut m = Material3D::default();
    m.albedo = albedo;
    m
}

// ---------------------------------------------------------------------------
// Projected-line grid (pat-3dgrid)
//
// The grid used to be drawn as thin 3D boxes pushed through the triangle
// rasterizer, which produced broken, dashed, sub-pixel speckle instead of the
// crisp continuous lines Godot shows. Godot draws the grid as *line primitives*
// projected through the camera, so we do the same: project each grid line to
// screen, clip it against the near plane (lines run off to the horizon, so an
// endpoint is often behind the camera), and rasterize it as an anti-aliased
// line that fades with distance — giving the infinite-looking floor grid.
// ---------------------------------------------------------------------------

fn lerp3(a: Vector3, b: Vector3, t: f32) -> Vector3 {
    Vector3::new(
        a.x + (b.x - a.x) * t,
        a.y + (b.y - a.y) * t,
        a.z + (b.z - a.z) * t,
    )
}

/// World → camera/view space (camera looks down −Z; `z < 0` is in front).
/// Mirrors `Viewport3D::world_to_screen`'s view transform exactly.
fn world_to_view(view: &Transform3D, w: Vector3) -> Vector3 {
    Vector3::new(
        view.basis.x.x * w.x + view.basis.y.x * w.y + view.basis.z.x * w.z + view.origin.x,
        view.basis.x.y * w.x + view.basis.y.y * w.y + view.basis.z.y * w.z + view.origin.y,
        view.basis.x.z * w.x + view.basis.y.z * w.y + view.basis.z.z * w.z + view.origin.z,
    )
}

/// View space → screen pixels. Matches `world_to_screen`: perspective divides by
/// view-space depth; orthographic projects in parallel, scaled by `ortho_size`.
fn project_view(viewport: &Viewport3D, v: Vector3) -> (f32, f32) {
    let aspect = viewport.aspect_ratio();
    let (ndc_x, ndc_y) = match viewport.camera.projection {
        crate::viewport_3d::Projection::Orthographic => {
            let half_h = (viewport.camera.ortho_size * 0.5).max(1e-3);
            (v.x / (half_h * aspect), v.y / half_h)
        }
        crate::viewport_3d::Projection::Perspective => {
            let half_fov = (viewport.camera.fov_degrees * 0.5 * std::f32::consts::PI / 180.0).tan();
            let inv_z = if v.z.abs() < 1e-6 { -1e6 } else { -1.0 / v.z };
            (
                (v.x * inv_z) / (half_fov * aspect),
                (v.y * inv_z) / half_fov,
            )
        }
    };
    let px = (ndc_x + 1.0) * 0.5 * viewport.width as f32;
    let py = (1.0 - ndc_y) * 0.5 * viewport.height as f32;
    (px, py)
}

/// Alpha-blends `color` onto the raster pixel at (`x`,`y`) with coverage `a`.
/// Delegates to [`Raster::blend`] so the same overlay code composites onto either
/// a float [`FrameBuffer`] (software path) or a byte [`Bgra8Image`] (GPU path).
fn blend_px(fb: &mut impl Raster, x: i32, y: i32, color: Color, a: f32) {
    fb.blend(x, y, color, a);
}

/// Xiaolin-Wu-style anti-aliased line from (`x0`,`y0`) to (`x1`,`y1`), faded by
/// camera depth. `iz0`/`iz1` are the *inverse* view-space z at the endpoints;
/// since 1/z is affine in screen space, interpolating it per pixel gives the
/// perspective-correct depth, from which the fade is computed (the grid dims
/// with distance, like Godot). Rasterization is clamped to the framebuffer so
/// near-grazing lines (one endpoint projected far off-screen) stay cheap.
#[allow(clippy::too_many_arguments)]
fn draw_aa_line_fade(
    fb: &mut impl Raster,
    mut x0: f32,
    mut y0: f32,
    mut iz0: f32,
    mut x1: f32,
    mut y1: f32,
    mut iz1: f32,
    color: Color,
    base_alpha: f32,
    fade_dist: f32,
) {
    let steep = (y1 - y0).abs() > (x1 - x0).abs();
    if steep {
        std::mem::swap(&mut x0, &mut y0);
        std::mem::swap(&mut x1, &mut y1);
    }
    if x0 > x1 {
        std::mem::swap(&mut x0, &mut x1);
        std::mem::swap(&mut y0, &mut y1);
        std::mem::swap(&mut iz0, &mut iz1);
    }
    let dx = x1 - x0;
    let gradient = if dx.abs() < 1e-6 { 0.0 } else { (y1 - y0) / dx };
    let span = dx.max(1e-6);
    let major_max = if steep { fb.height() } else { fb.width() } as i32;
    let xi_lo = (x0.floor() as i32).max(0);
    let xi_hi = (x1.ceil() as i32).min(major_max - 1);
    let mut xi = xi_lo;
    while xi <= xi_hi {
        let u = (((xi as f32) - x0) / span).clamp(0.0, 1.0);
        let iz = iz0 + (iz1 - iz0) * u;
        let dist = if iz.abs() < 1e-9 { 1e9 } else { -1.0 / iz }; // front is z<0
        let fade = (1.0 - dist / fade_dist).clamp(0.0, 1.0);
        let a = base_alpha * fade;
        if a > 0.003 {
            let yf = y0 + gradient * ((xi as f32) - x0);
            let fy = yf.floor();
            let frac = yf - fy;
            let y_lo = fy as i32;
            if steep {
                blend_px(fb, y_lo, xi, color, a * (1.0 - frac));
                blend_px(fb, y_lo + 1, xi, color, a * frac);
            } else {
                blend_px(fb, xi, y_lo, color, a * (1.0 - frac));
                blend_px(fb, xi, y_lo + 1, color, a * frac);
            }
        }
        xi += 1;
    }
}

/// Clips a world-space segment to the near plane, projects it, and draws it as a
/// depth-faded anti-aliased line. Skips segments fully behind the camera.
#[allow(clippy::too_many_arguments)]
fn draw_grid_segment(
    fb: &mut impl Raster,
    viewport: &Viewport3D,
    view: &Transform3D,
    wa: Vector3,
    wb: Vector3,
    color: Color,
    base_alpha: f32,
    fade_dist: f32,
) {
    const NEAR: f32 = 0.08;
    let (mut va, mut vb) = (world_to_view(view, wa), world_to_view(view, wb));
    // Signed distance in front of the near plane (camera faces −Z).
    let fa = -va.z - NEAR;
    let fbv = -vb.z - NEAR;
    if fa <= 0.0 && fbv <= 0.0 {
        return; // whole segment is behind the camera
    }
    if fa <= 0.0 {
        va = lerp3(va, vb, fa / (fa - fbv));
    } else if fbv <= 0.0 {
        vb = lerp3(va, vb, fa / (fa - fbv));
    }
    let (x0, y0) = project_view(viewport, va);
    let (x1, y1) = project_view(viewport, vb);
    draw_aa_line_fade(
        fb,
        x0,
        y0,
        1.0 / va.z,
        x1,
        y1,
        1.0 / vb.z,
        color,
        base_alpha,
        fade_dist,
    );
}

/// Transforms a local point by a [`Transform3D`] (basis * p + origin).
fn xform_point(t: &Transform3D, p: Vector3) -> Vector3 {
    Vector3::new(
        t.basis.x.x * p.x + t.basis.y.x * p.y + t.basis.z.x * p.z + t.origin.x,
        t.basis.x.y * p.x + t.basis.y.y * p.y + t.basis.z.y * p.z + t.origin.y,
        t.basis.x.z * p.x + t.basis.y.z * p.y + t.basis.z.z * p.z + t.origin.z,
    )
}

/// Clips a world-space segment to the near plane, projects it, and draws it as a
/// solid (un-faded) anti-aliased line — used for the selection outline.
fn draw_edge(
    fb: &mut impl Raster,
    viewport: &Viewport3D,
    view: &Transform3D,
    wa: Vector3,
    wb: Vector3,
    color: Color,
    alpha: f32,
) {
    const NEAR: f32 = 0.08;
    let (mut va, mut vb) = (world_to_view(view, wa), world_to_view(view, wb));
    let fa = -va.z - NEAR;
    let fbv = -vb.z - NEAR;
    if fa <= 0.0 && fbv <= 0.0 {
        return;
    }
    if fa <= 0.0 {
        va = lerp3(va, vb, fa / (fa - fbv));
    } else if fbv <= 0.0 {
        vb = lerp3(va, vb, fa / (fa - fbv));
    }
    let (x0, y0) = project_view(viewport, va);
    let (x1, y1) = project_view(viewport, vb);
    // A huge fade distance makes draw_aa_line_fade effectively solid.
    draw_aa_line_fade(
        fb,
        x0,
        y0,
        1.0 / va.z,
        x1,
        y1,
        1.0 / vb.z,
        color,
        alpha,
        1.0e9,
    );
}

/// The 12 edges of a cube, as index pairs into its 8 corners (corner `i` has
/// component `j` set to the high value when bit `j` of `i` is set).
const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7), // along X
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7), // along Y
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7), // along Z
];

/// Draws a wireframe box (12 edges) of half-size `h` at `transform`, used as the
/// selection outline around the picked node — like Godot's selection box.
fn draw_selection_box(
    fb: &mut impl Raster,
    viewport: &Viewport3D,
    transform: &Transform3D,
    h: f32,
) {
    let view = viewport.camera.view_transform();
    let s = [-h, h];
    let mut c = [Vector3::ZERO; 8];
    for (i, slot) in c.iter_mut().enumerate() {
        *slot = xform_point(
            transform,
            Vector3::new(s[i & 1], s[(i >> 1) & 1], s[(i >> 2) & 1]),
        );
    }
    for (a, b) in BOX_EDGES {
        draw_edge(fb, viewport, &view, c[a], c[b], SELECTION_COLOR_3D, 0.95);
    }
}

/// Draws a small axis-aligned wireframe cube (gizmo handle) centered at `center`.
fn draw_handle_box(
    fb: &mut impl Raster,
    viewport: &Viewport3D,
    view: &Transform3D,
    center: Vector3,
    h: f32,
    color: Color,
) {
    let s = [-h, h];
    let mut c = [Vector3::ZERO; 8];
    for (i, slot) in c.iter_mut().enumerate() {
        *slot = Vector3::new(
            center.x + s[i & 1],
            center.y + s[(i >> 1) & 1],
            center.z + s[(i >> 2) & 1],
        );
    }
    for (a, b) in BOX_EDGES {
        draw_edge(fb, viewport, view, c[a], c[b], color, 1.0);
    }
}

/// Draws a Camera3D gizmo: a wireframe view frustum (apex at the node, opening
/// down its local −Z) like Godot's camera gizmo.
fn draw_camera_gizmo(fb: &mut impl Raster, viewport: &Viewport3D, tf: &Transform3D) {
    let view = viewport.camera.view_transform();
    let (d, w, h) = (1.5_f32, 0.7_f32, 0.5_f32);
    let local = [
        Vector3::new(-w, -h, -d),
        Vector3::new(w, -h, -d),
        Vector3::new(w, h, -d),
        Vector3::new(-w, h, -d),
    ];
    let c: Vec<Vector3> = local.iter().map(|p| xform_point(tf, *p)).collect();
    let apex = tf.origin;
    for cc in &c {
        draw_edge(fb, viewport, &view, apex, *cc, CAMERA_COLOR_3D, 0.95);
    }
    for i in 0..4 {
        draw_edge(
            fb,
            viewport,
            &view,
            c[i],
            c[(i + 1) % 4],
            CAMERA_COLOR_3D,
            0.95,
        );
    }
}

/// Draws a light gizmo: a bundle of parallel rays pointing down the node's local
/// −Z (its emission direction), evoking Godot's directional-light gizmo.
fn draw_light_gizmo(fb: &mut impl Raster, viewport: &Viewport3D, tf: &Transform3D) {
    let view = viewport.camera.view_transform();
    let len = 1.8_f32;
    let r = 0.32_f32;
    let offsets = [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(r, r, 0.0),
        Vector3::new(-r, r, 0.0),
        Vector3::new(r, -r, 0.0),
        Vector3::new(-r, -r, 0.0),
    ];
    for o in offsets {
        let s = xform_point(tf, o);
        let e = xform_point(tf, Vector3::new(o.x, o.y, -len));
        draw_edge(fb, viewport, &view, s, e, LIGHT_COLOR_3D, 0.9);
    }
}

/// Draws a closed wireframe loop whose local-space points come from `pt(angle)`
/// for `angle` swept over a full turn, transformed by `tf`.
fn draw_loop<F: Fn(f32) -> Vector3>(
    fb: &mut impl Raster,
    viewport: &Viewport3D,
    view: &Transform3D,
    tf: &Transform3D,
    color: Color,
    pt: F,
) {
    const N: usize = 24;
    let mut prev = xform_point(tf, pt(0.0));
    for i in 1..=N {
        let a = std::f32::consts::TAU * i as f32 / N as f32;
        let cur = xform_point(tf, pt(a));
        draw_edge(fb, viewport, view, prev, cur, color, 0.9);
        prev = cur;
    }
}

/// Draws a CollisionShape3D wireframe gizmo (teal), matching the shape kind, at
/// Godot's default extents (box 1×1×1, sphere/capsule/cylinder radius 0.5,
/// height 2). The exact resource extents are a follow-up; the marker only
/// carries the shape type today.
fn draw_collision_gizmo(
    fb: &mut impl Raster,
    viewport: &Viewport3D,
    tf: &Transform3D,
    kind: CollisionShapeKind,
    ext: Vector3,
) {
    let view = viewport.camera.view_transform();
    let col = COLLISION_COLOR_3D;
    let r = ext.x; // radius (sphere/capsule/cylinder)
    let hh = ext.y; // half-height (capsule/cylinder)
    match kind {
        CollisionShapeKind::Box => {
            let (hx, hy, hz) = (ext.x, ext.y, ext.z);
            let sx = [-hx, hx];
            let sy = [-hy, hy];
            let sz = [-hz, hz];
            let mut c = [Vector3::ZERO; 8];
            for (i, slot) in c.iter_mut().enumerate() {
                *slot = xform_point(
                    tf,
                    Vector3::new(sx[i & 1], sy[(i >> 1) & 1], sz[(i >> 2) & 1]),
                );
            }
            for (a, b) in BOX_EDGES {
                draw_edge(fb, viewport, &view, c[a], c[b], col, 0.95);
            }
        }
        CollisionShapeKind::Sphere => {
            // Three great circles (one per plane).
            draw_loop(fb, viewport, &view, tf, col, |a| {
                Vector3::new(0.0, a.cos() * r, a.sin() * r)
            });
            draw_loop(fb, viewport, &view, tf, col, |a| {
                Vector3::new(a.cos() * r, 0.0, a.sin() * r)
            });
            draw_loop(fb, viewport, &view, tf, col, |a| {
                Vector3::new(a.cos() * r, a.sin() * r, 0.0)
            });
        }
        CollisionShapeKind::Cylinder | CollisionShapeKind::Capsule => {
            // Top and bottom rim circles + four vertical connectors.
            draw_loop(fb, viewport, &view, tf, col, |a| {
                Vector3::new(a.cos() * r, hh, a.sin() * r)
            });
            draw_loop(fb, viewport, &view, tf, col, |a| {
                Vector3::new(a.cos() * r, -hh, a.sin() * r)
            });
            for k in 0..4 {
                let a = std::f32::consts::FRAC_PI_2 * k as f32;
                let (sa, ca) = a.sin_cos();
                let top = xform_point(tf, Vector3::new(ca * r, hh, sa * r));
                let bot = xform_point(tf, Vector3::new(ca * r, -hh, sa * r));
                draw_edge(fb, viewport, &view, top, bot, col, 0.9);
            }
        }
    }
}

/// World-space length of the editor's transform-gizmo arrows, scaled by camera
/// distance so the gizmo keeps a usable on-screen size at any zoom. Shared by the
/// renderer (drawing) and the server (hit-testing) so the two stay aligned.
pub fn gizmo_arrow_length(viewport: &Viewport3D) -> f32 {
    (viewport.camera.distance * 0.18).clamp(0.6, 4.0)
}

/// Draws the move gizmo — three colored axis lines (X red, Y green, Z blue) from
/// `center`, each capped with a small handle cube — like Godot's translate gizmo.
fn draw_move_gizmo(fb: &mut impl Raster, viewport: &Viewport3D, center: Vector3, len: f32) {
    let view = viewport.camera.view_transform();
    let axes = [
        (Vector3::new(len, 0.0, 0.0), X_AXIS_COLOR_3D),
        (Vector3::new(0.0, len, 0.0), Y_AXIS_COLOR_3D),
        (Vector3::new(0.0, 0.0, len), Z_AXIS_COLOR_3D),
    ];
    for (d, color) in axes {
        let tip = Vector3::new(center.x + d.x, center.y + d.y, center.z + d.z);
        draw_edge(fb, viewport, &view, center, tip, color, 1.0);
        draw_handle_box(fb, viewport, &view, tip, len * 0.08, color);
    }

    // Plane handles — small squares between each axis pair that drag in that
    // plane. Positioned at `arrow_length * plane_handle_fraction` (0.3, matching
    // the hit-test). Colored by the plane normal axis, like Godot.
    let pf = len * 0.3;
    let s = pf * 0.45;
    // (offset from center, in-plane edge dir u, in-plane edge dir v, color)
    let planes = [
        (
            Vector3::new(pf, pf, 0.0),
            Vector3::new(s, 0.0, 0.0),
            Vector3::new(0.0, s, 0.0),
            Z_AXIS_COLOR_3D,
        ),
        (
            Vector3::new(pf, 0.0, pf),
            Vector3::new(s, 0.0, 0.0),
            Vector3::new(0.0, 0.0, s),
            Y_AXIS_COLOR_3D,
        ),
        (
            Vector3::new(0.0, pf, pf),
            Vector3::new(0.0, s, 0.0),
            Vector3::new(0.0, 0.0, s),
            X_AXIS_COLOR_3D,
        ),
    ];
    for (off, u, v, color) in planes {
        let c = Vector3::new(center.x + off.x, center.y + off.y, center.z + off.z);
        let corners = [
            Vector3::new(c.x + u.x + v.x, c.y + u.y + v.y, c.z + u.z + v.z),
            Vector3::new(c.x - u.x + v.x, c.y - u.y + v.y, c.z - u.z + v.z),
            Vector3::new(c.x - u.x - v.x, c.y - u.y - v.y, c.z - u.z - v.z),
            Vector3::new(c.x + u.x - v.x, c.y + u.y - v.y, c.z + u.z - v.z),
        ];
        for i in 0..4 {
            draw_edge(
                fb,
                viewport,
                &view,
                corners[i],
                corners[(i + 1) % 4],
                color,
                0.85,
            );
        }
    }
}

/// Draws the rotate gizmo — three colored rings (X red in YZ, Y green in XZ,
/// Z blue in XY) of radius `r` around `center`, like Godot's rotation gizmo.
fn draw_rotate_gizmo(fb: &mut impl Raster, viewport: &Viewport3D, center: Vector3, r: f32) {
    let view = viewport.camera.view_transform();
    const SEGMENTS: usize = 48;
    // (color, function mapping an angle to a ring point offset from center)
    let rings: [(Color, fn(f32, f32) -> Vector3); 3] = [
        (X_AXIS_COLOR_3D, |a, r| {
            Vector3::new(0.0, r * a.cos(), r * a.sin())
        }),
        (Y_AXIS_COLOR_3D, |a, r| {
            Vector3::new(r * a.cos(), 0.0, r * a.sin())
        }),
        (Z_AXIS_COLOR_3D, |a, r| {
            Vector3::new(r * a.cos(), r * a.sin(), 0.0)
        }),
    ];
    for (color, point) in rings {
        let mut prev: Option<Vector3> = None;
        for i in 0..=SEGMENTS {
            let a = (i as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
            let off = point(a, r);
            let p = Vector3::new(center.x + off.x, center.y + off.y, center.z + off.z);
            if let Some(pp) = prev {
                draw_edge(fb, viewport, &view, pp, p, color, 1.0);
            }
            prev = Some(p);
        }
    }
}

/// Picks the color and base opacity for a grid line at world coordinate `coord`.
/// `x_running` is true for lines that run along X (held at a fixed Z): those at
/// Z=0 are the red X axis; lines at X=0 (running along Z) are the blue Z axis.
fn grid_line_style(coord: f32, x_running: bool, show_origin: bool) -> (Color, f32) {
    if show_origin && coord.abs() < 1e-4 {
        if x_running {
            (X_AXIS_COLOR_3D, 0.95)
        } else {
            (Z_AXIS_COLOR_3D, 0.95)
        }
    } else if ((coord / 10.0).round() * 10.0 - coord).abs() < 1e-3 {
        (GRID_MAJOR_COLOR_3D, 0.5)
    } else {
        (GRID_MINOR_COLOR_3D, 0.26)
    }
}

/// Draws the editor's ground grid + colored origin axes as projected anti-aliased
/// lines over `fb`. The grid is centered on (and follows) the camera focus and
/// fades at its radius, so it always fills the view like Godot's infinite grid.
fn draw_grid_overlay(fb: &mut impl Raster, viewport: &Viewport3D) {
    if !viewport.grid.visible {
        return;
    }
    let cam = &viewport.camera;
    let view = cam.view_transform();
    let focus = cam.focus_point;
    let spacing = 1.0_f32;
    // The grid fades out by camera distance; its extent scales with zoom so it
    // always fills the view. Lines past the fade distance contribute nothing.
    let fade_dist = (cam.distance * 5.0).clamp(40.0, 100.0);
    let n = (fade_dist / spacing).ceil() as i32;
    let cxr = (focus.x / spacing).round();
    let czr = (focus.z / spacing).round();
    let z_lo = (czr - n as f32) * spacing;
    let z_hi = (czr + n as f32) * spacing;
    let x_lo = (cxr - n as f32) * spacing;
    let x_hi = (cxr + n as f32) * spacing;
    for i in -n..=n {
        // Line running along Z at world X = x (the Z axis when x == 0).
        let x = (cxr + i as f32) * spacing;
        let (c, a) = grid_line_style(x, false, viewport.show_origin);
        draw_grid_segment(
            fb,
            viewport,
            &view,
            Vector3::new(x, 0.0, z_lo),
            Vector3::new(x, 0.0, z_hi),
            c,
            a,
            fade_dist,
        );
        // Line running along X at world Z = z (the X axis when z == 0).
        let z = (czr + i as f32) * spacing;
        let (c2, a2) = grid_line_style(z, true, viewport.show_origin);
        draw_grid_segment(
            fb,
            viewport,
            &view,
            Vector3::new(x_lo, 0.0, z),
            Vector3::new(x_hi, 0.0, z),
            c2,
            a2,
            fade_dist,
        );
    }
}

/// Draws a `RootMotionView`'s counter-scrolled locomotion grid (pat-0q2kx.249).
///
/// Godot ground truth: `scene/animation/root_motion_view.cpp`. The grid is a flat
/// plane of `cell_size` squares extending `radius` from the view, shifted by the
/// wrapped root motion so it appears to slide under a character that is animating
/// in place. Drawn through the same projected-line path as the editor's ground
/// grid, so it fades with distance identically.
///
/// `offset` is already wrapped into one cell by `RootMotionView::grid_offset`, so
/// subtracting it counter-scrolls: the character walks +Z, the grid slides −Z.
fn draw_root_motion_grid(
    fb: &mut impl Raster,
    viewport: &Viewport3D,
    grid: &RootMotionGridOverlay,
) {
    // A non-positive cell would divide the plane into nothing (and `radius` 0
    // leaves no extent to draw), so there is no grid to show.
    if grid.cell_size <= 0.0 || grid.radius <= 0.0 || !grid.cell_size.is_finite() {
        return;
    }
    // Bound the line count: a tiny cell against a large radius would otherwise
    // emit unboundedly many segments for a gizmo.
    const MAX_LINES_PER_AXIS: i32 = 64;
    let half = (grid.radius / grid.cell_size).floor() as i32;
    let half = half.clamp(1, MAX_LINES_PER_AXIS);
    let extent = half as f32 * grid.cell_size;
    let fade_dist = extent * 2.0;
    // Counter-scroll, and keep the plane at the view's height.
    let ox = -grid.offset.x;
    let oz = -grid.offset.z;
    let y = grid.offset.y;
    let view = viewport.camera.view_transform();

    for i in -half..=half {
        let d = i as f32 * grid.cell_size;
        // Lines running along Z (constant X), and along X (constant Z).
        draw_grid_segment(
            fb,
            viewport,
            &view,
            Vector3::new(ox + d, y, oz - extent),
            Vector3::new(ox + d, y, oz + extent),
            grid.color,
            grid.color.a,
            fade_dist,
        );
        draw_grid_segment(
            fb,
            viewport,
            &view,
            Vector3::new(ox - extent, y, oz + d),
            Vector3::new(ox + extent, y, oz + d),
            grid.color,
            grid.color.a,
            fade_dist,
        );
    }
}

/// Overlay geometry collected while building the render list. These are drawn as
/// projected anti-aliased lines *over* the rasterized mesh frame, so both the
/// software and GPU editor render paths produce the same gizmos/selection chrome
/// regardless of which rasterizer filled the mesh pixels.
struct EditorOverlays3D {
    /// Transforms of every selected node (each gets a selection-box outline).
    selected_tfs: Vec<Transform3D>,
    /// The primary selection's transform (carries the transform gizmo).
    primary_tf: Option<Transform3D>,
    /// Camera/light nodes and their gizmo kind (frustum / direction rays).
    node_gizmos: Vec<(Transform3D, PrimitiveShape)>,
    /// CollisionShape3D wireframe gizmos.
    collision_gizmos: Vec<(Transform3D, CollisionShapeKind, Vector3)>,
    /// The `RootMotionView` locomotion grid, if the scene has one
    /// (pat-0q2kx.249).
    root_motion_grid: Option<RootMotionGridOverlay>,
}

/// A `RootMotionView`'s counter-scrolled floor grid, snapshotted for the draw
/// step (pat-0q2kx.249).
///
/// Godot ground truth: `scene/animation/root_motion_view.cpp` — an **editor-only**
/// visualisation. The grid scrolls *opposite* the AnimationMixer's root motion so
/// an animator can inspect locomotion while the character stays put.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RootMotionGridOverlay {
    /// Where the grid sits this frame: the root motion wrapped into one cell, so
    /// the pattern slides continuously instead of running away from the origin.
    pub offset: Vector3,
    /// The node's `color`.
    pub color: Color,
    /// Spacing between grid lines.
    pub cell_size: f32,
    /// How far the grid extends from its centre, in cells.
    pub radius: f32,
}

/// Snapshot of the tree-dependent inputs the editor 3D render needs: the render
/// list and the scene's key-light direction. Built **under the editor state lock**
/// (both read the scene tree) so the actual render — GPU submit, blocking
/// readback, overlay rasterization, BMP encode — can run *after* releasing the
/// lock, keeping `/api` handlers (orbit/select/gizmo) responsive during
/// interaction (pat-ism62 lock-hold reduction).
pub struct Editor3DRenderInputs {
    /// The drawable spatial nodes (meshes + primitive markers) for this frame.
    pub items: Vec<Render3DItem>,
    /// The scene's `DirectionalLight3D` lights (direction + color/energy/angular
    /// size), in tree order, capped at 4. Empty → fall back to the preview sun.
    pub scene_lights: Vec<SceneLight3D>,
    /// The scene's environment-effect nodes (pat-0q2kx.132).
    pub env_effects: SceneEnvEffects3D,
    /// The `RootMotionView` grid, or `None` when the scene has no such node
    /// (pat-0q2kx.249). Cleared by [`strip_editor_only_items`] so a game frame
    /// never shows it.
    pub root_motion_grid: Option<RootMotionGridOverlay>,
    /// The compositor whose effects run at the pipeline stage boundaries
    /// (pat-s3rrv.251), or `None` for an uncomposited frame.
    ///
    /// ⚠️ Carried here rather than on `Viewport3D`, which is where the bead
    /// suggests it. `Viewport3D` has **24 exhaustive struct literals across
    /// four crates**, so a new field there is a workspace-wide compile break;
    /// this struct has two, both in this file. The render path is the same
    /// either way — this is where the environment is resolved too.
    pub compositor: Option<Compositor>,
}

/// The `FogVolume` / `Decal` / `ReflectionProbe` / `OccluderInstance3D` nodes of
/// a scene, snapshotted from the tree so the render step needs no tree access
/// (pat-0q2kx.132).
///
/// The renderer had a full pass for each of these — `apply_fog_volume_pass`,
/// `apply_decal_pass`, `is_occluded`, `sample_reflection_probes` — and a rich
/// type behind each, but **no production code ever called `add_*`**. Fog volumes
/// reached a preview list nobody read, probes only a test-only adapter, and
/// decals and occluders had no producer at all. Authoring any of the four
/// changed not one pixel; this is what carries them from `.tscn` to the frame.
#[derive(Debug, Clone, Default)]
pub struct SceneEnvEffects3D {
    /// Fog volumes with the node's global transform — the transform is the half
    /// that was missing before: the preview list built the volume without one,
    /// so every fog volume sat at the origin regardless of where it was placed.
    pub fog_volumes: Vec<(FogVolume, Transform3D)>,
    /// Decals, each already carrying its own `global_transform`.
    pub decals: Vec<Decal3D>,
    /// Reflection probes, positioned and sized from their nodes.
    pub reflection_probes: Vec<ReflectionProbe>,
    /// Occluders, each with a shape and world transform.
    pub occluders: Vec<OccluderInstance3D>,
    /// `VoxelGI` volumes contributing indirect ambient (pat-s3rrv.114).
    ///
    /// ⚠️ Both renderers already held a `voxel_gis` list, a full `VoxelGI` type
    /// and `sample_voxel_gi_at` — and **nothing in the scene layer ever filled
    /// it**. `scene_env_effects` collected the other four effect families and
    /// skipped this one, so authoring a VoxelGI changed no pixel. This is the
    /// producer that was missing, not the shading.
    pub voxel_gis: Vec<VoxelGI>,
}

/// Builds [`Editor3DRenderInputs`] from the viewport + scene tree. This is the
/// only step that touches the tree, so callers run *just this* under the state
/// lock and render from the returned owned snapshot afterward.
pub fn editor_3d_render_inputs(viewport: &Viewport3D, tree: &SceneTree) -> Editor3DRenderInputs {
    let items = viewport.build_render_list(tree);
    let scene_lights = scene_lights(tree);
    let mut env_effects = scene_env_effects(tree);

    // pat-s3rrv.114 — bake the collected `VoxelGI` volumes.
    //
    // ⚠️ This is the step that was missing, and its absence was invisible: the
    // collector, both renderers' `voxel_gis` lists and the per-pixel
    // `sample_voxel_gi_at` modulation all existed, but an UNBAKED volume samples
    // pure black and the shading pass skips black. So authoring a VoxelGI
    // changed no pixel while every piece looked wired.
    //
    // Here rather than in `scene_env_effects` because the bake needs the scene's
    // lights and surfaces, and this is the only place all three are in hand.
    crate::gi_bake::bake_voxel_gis(&mut env_effects.voxel_gis, &scene_lights, &items);

    Editor3DRenderInputs {
        items,
        scene_lights,
        env_effects,
        root_motion_grid: root_motion_grid(tree),
        // Effects carry Rust callbacks, which a `.tscn` cannot express; a
        // caller installs the compositor for the frame. See
        // `render_editor_3d_frame_with_compositor`.
        compositor: None,
    }
}

/// The scene's `RootMotionView` grid, counter-scrolled by its AnimationMixer's
/// root motion (pat-0q2kx.249).
///
/// **Why this needs no stored scroll state.** The mixer keeps the running total
/// itself — `AnimationPlayer::root_motion().position_accumulator` is the travel
/// since the last reset (pat-mgixk.78). Reading it per frame and wrapping gives
/// the same scroll Godot shows, with nothing to keep between frames and no
/// `&mut SceneTree` — which is what lets this run inside the read-only input
/// snapshot.
///
/// ⚠️ This used to read the DRIVEN NODE'S POSITION, which worked only because
/// `process_animations` added each delta onto it. That auto-apply is gone —
/// Godot's mixer never moves the node — so reading the node would now leave the
/// grid motionless, and **nothing would have failed**, because the reason for
/// reading the node was the behaviour that was removed.
///
/// `animation_path` names the mixer and the accumulator is read from it
/// directly, so the mixer's `root_node` no longer matters here. (It never
/// defaulted to `"."` either, whatever the old note said: ClassDB seeds
/// `AnimationMixer.root_node` as `NodePath("..")`.)
fn root_motion_grid(tree: &SceneTree) -> Option<RootMotionGridOverlay> {
    use gdscene::root_motion_view::RootMotionView;

    let view_id = tree.all_nodes_in_tree_order().into_iter().find(|id| {
        tree.get_node(*id)
            .is_some_and(|n| n.class_name() == "RootMotionView")
    })?;
    let view_node = tree.get_node(view_id)?;

    let mut view = RootMotionView::new();
    if let gdvariant::Variant::Float(f) = view_node.get_property("cell_size") {
        view.cell_size = f as f32;
    }
    if let gdvariant::Variant::Float(f) = view_node.get_property("radius") {
        view.radius = f as f32;
    }
    let color = match view_node.get_property("color") {
        gdvariant::Variant::Color(c) => c,
        // `root_motion_view.h:44` — Color(0.5, 0.5, 1.0), i.e. fully opaque.
        _ => Color::new(0.5, 0.5, 1.0, 1.0),
    };

    // Resolve `animation_path` -> mixer -> the node root motion drives. A missing
    // or unresolvable path leaves the grid parked at the origin rather than
    // hiding it: Godot still draws the grid, it simply does not scroll.
    let travel = animation_path_root_travel(tree, view_id, view_node).unwrap_or(Vector3::ZERO);
    view.accumulate(&gdscene::animation::RootMotionDelta {
        position: travel,
        rotation_y: 0.0,
    });

    Some(RootMotionGridOverlay {
        offset: view.grid_offset(),
        color,
        cell_size: view.cell_size,
        radius: view.radius,
    })
}

/// The position of the node the `RootMotionView`'s mixer drives, if it resolves.
fn animation_path_root_travel(
    tree: &SceneTree,
    view_id: NodeId,
    view_node: &gdscene::node::Node,
) -> Option<Vector3> {
    let path = match view_node.get_property("animation_path") {
        gdvariant::Variant::NodePath(p) => p.to_string(),
        gdvariant::Variant::String(s) => s,
        _ => return None,
    };
    if path.is_empty() {
        return None;
    }
    let mixer = tree.get_node_or_null(view_id, &path)?;
    // (The comment this replaces claimed `root_node` defaults to "." — the
    // mixer itself. It does not: ClassDB seeds `AnimationMixer.root_node` as
    // `NodePath("..")` (`class_db.rs:5428`), so every `.tscn` mixer resolved to
    // its PARENT. The resolution is gone either way, but the claim was wrong.)
    //
    // 🔑 The mixer's own root-motion ACCUMULATOR, not the driven node's
    // position (pat-mgixk.78).
    //
    // This used to resolve `root_node` and read that node's `position`, which
    // worked only because `apply_root_motion_delta` added each delta straight
    // onto it — so the node's current position *was* the accumulated travel.
    // That auto-apply is what this bead removes (Godot's `AnimationMixer` never
    // moves the node), and the grid would then have sat still with no test
    // failing, because its own reason for reading the node was the behaviour
    // being deleted.
    //
    // Reading the accumulator is also strictly more correct today: it is TRUE
    // travel, whereas the node's position includes wherever the rig was placed.
    // The two agree only for a rig that starts at the origin.
    //
    // ⚠️ `None` for an `AnimationTree` mixer, which has no root-motion track
    // field at all yet (see the note on `AnimationTree::non_property_track_paths`),
    // so there is genuinely nothing to report — and the caller's
    // `unwrap_or(Vector3::ZERO)` parks the grid rather than hiding it, which is
    // what it already did for an unresolvable path.
    Some(
        tree.get_animation_player(mixer)?
            .root_motion()
            .position_accumulator,
    )
}

/// Reads a float node property, accepting an `Int` spelling too (a `.tscn`
/// writes `intensity = 1` without a decimal point).
fn env_f32(node: &gdscene::node::Node, key: &str, default: f32) -> f32 {
    match node.get_property(key) {
        gdvariant::Variant::Float(f) => f as f32,
        gdvariant::Variant::Int(i) => i as f32,
        _ => default,
    }
}

fn env_bool(node: &gdscene::node::Node, key: &str, default: bool) -> bool {
    match node.get_property(key) {
        gdvariant::Variant::Bool(b) => b,
        gdvariant::Variant::Int(i) => i != 0,
        _ => default,
    }
}

/// Decodes a decal texture path to pixels, memoised for the process
/// (pat-s3rrv.204).
///
/// ⚠️ A thread-local cache rather than a local one: `scene_env_effects` runs
/// per frame and takes only a `&SceneTree`, so a cache created inside it would
/// re-decode every decal texture on every frame. Threading a cache through the
/// signature would touch each caller for no behavioural gain.
fn decal_texture_image(
    path: Option<&str>,
) -> Option<std::sync::Arc<gdserver3d::material::TextureImage>> {
    let path = path?;
    if path.is_empty() {
        return None;
    }
    // The material cache keeps texels as authored (sRGB); the decal pass
    // blends its texels as linear colour, so decode here (pat-zwvgw kept the
    // decal path's input unchanged).
    let raw = DECAL_TEXTURES.with(|c| c.borrow_mut().get_or_load(path))?;
    // Memoised by the raw image's identity, so the decoded `Arc` is stable
    // across frames like the cache's own.
    thread_local! {
        static DECAL_LINEAR: std::cell::RefCell<
            std::collections::HashMap<usize, std::sync::Arc<gdserver3d::material::TextureImage>>,
        > = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let key = std::sync::Arc::as_ptr(&raw) as usize;
    Some(DECAL_LINEAR.with(|m| {
        m.borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                std::sync::Arc::new(gdserver3d::material::TextureImage::new(
                    raw.width,
                    raw.height,
                    raw.pixels.iter().map(|c| c.srgb_to_linear()).collect(),
                ))
            })
            .clone()
    }))
}

thread_local! {
    static DECAL_TEXTURES: std::cell::RefCell<gdscene::render_server_3d::MaterialTextureCache> =
        std::cell::RefCell::new(gdscene::render_server_3d::MaterialTextureCache::new());
}

/// Reads a texture-resource path off a node.
///
/// A `.tscn` writes `texture_albedo = ExtResource("1_abc")`, which the loader
/// flattens into an `__extres__texture_albedo` companion holding the path (see
/// `packed_scene.rs:809`) while the property itself keeps the resource. Both
/// spellings are accepted, companion first, so a scene-loaded node and a
/// hand-built one behave the same. An empty path is `None`, not `Some("")`.
fn env_texture_path(node: &gdscene::node::Node, key: &str) -> Option<String> {
    let raw = match node.get_property(&format!("__extres__{key}")) {
        gdvariant::Variant::String(s) => s,
        _ => match node.get_property(key) {
            gdvariant::Variant::String(s) => s,
            _ => return None,
        },
    };
    (!raw.is_empty()).then_some(raw)
}

fn env_vec3(node: &gdscene::node::Node, key: &str, default: Vector3) -> Vector3 {
    match node.get_property(key) {
        gdvariant::Variant::Vector3(v) => v,
        _ => default,
    }
}

/// Walks the tree for the four environment-effect node classes and snapshots
/// them, ready to hand to the renderer (pat-0q2kx.132).
///
/// Hidden nodes are skipped, matching Godot: all four are `VisualInstance3D`s,
/// and hiding one drops it from the scenario rather than merely dimming it.
fn scene_env_effects(tree: &SceneTree) -> SceneEnvEffects3D {
    let mut out = SceneEnvEffects3D::default();
    let mut next_probe_id = 1u64;
    // Minted per collection pass, exactly as probe ids are: the renderer keys
    // its GI list on them and the snapshot is rebuilt each frame, so they only
    // need to be stable within one frame.
    let mut next_voxel_gi_id = 1u64;

    for nid in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(nid) else {
            continue;
        };
        let is_fog = node.is_class("FogVolume");
        let is_decal = node.is_class("Decal");
        let is_probe = node.is_class("ReflectionProbe");
        let is_occluder = node.is_class("OccluderInstance3D");
        let is_voxel_gi = node.is_class("VoxelGI");
        if !(is_fog || is_decal || is_probe || is_occluder || is_voxel_gi) {
            continue;
        }
        if !gdscene::node3d::is_visible_in_tree(tree, nid) {
            continue;
        }
        let transform = gdscene::node3d::get_global_transform(tree, nid);

        if is_fog {
            // The parser already existed (`viewport_3d.rs`), but the one caller
            // threw the transform away, so a fog volume authored anywhere in the
            // scene fogged the origin.
            let volume = crate::viewport_3d::fog_volume_from_node_properties(node.properties());
            out.fog_volumes.push((volume, transform));
        } else if is_decal {
            let mut decal = Decal3D::new();
            decal.global_transform = transform;
            decal.size = env_vec3(node, "size", decal.size);
            decal.modulate = match node.get_property("modulate") {
                gdvariant::Variant::Color(c) => c,
                _ => decal.modulate,
            };
            decal.albedo_mix = env_f32(node, "albedo_mix", decal.albedo_mix as f32) as f64;
            decal.emission_energy =
                env_f32(node, "emission_energy", decal.emission_energy as f32) as f64;
            decal.normal_fade = env_f32(node, "normal_fade", decal.normal_fade as f32) as f64;
            decal.upper_fade = env_f32(node, "upper_fade", decal.upper_fade as f32) as f64;
            decal.lower_fade = env_f32(node, "lower_fade", decal.lower_fade as f32) as f64;
            decal.distance_fade_enabled =
                env_bool(node, "distance_fade_enabled", decal.distance_fade_enabled);
            decal.distance_fade_begin = env_f32(
                node,
                "distance_fade_begin",
                decal.distance_fade_begin as f32,
            ) as f64;
            decal.distance_fade_length = env_f32(
                node,
                "distance_fade_length",
                decal.distance_fade_length as f32,
            ) as f64;
            if let gdvariant::Variant::Int(mask) = node.get_property("cull_mask") {
                decal.cull_mask = mask as u32;
            }
            // The four projected texture channels. `Decal3D` has carried these
            // path fields all along but nothing ever read them off a node, so a
            // `.tscn` naming `texture_albedo` reached the renderer with `None`
            // in every channel (pat-0q2kx.195).
            decal.texture_albedo = env_texture_path(node, "texture_albedo");
            decal.texture_normal = env_texture_path(node, "texture_normal");
            decal.texture_orm = env_texture_path(node, "texture_orm");
            decal.texture_emission = env_texture_path(node, "texture_emission");
            // ⚠️ Resolve the paths to PIXELS here (pat-s3rrv.204). Carrying the
            // path alone is what the previous bead did, and the projection had
            // nothing to sample — every decal rendered as one flat `modulate`
            // colour regardless of the art assigned.
            decal.albedo_image = decal_texture_image(decal.texture_albedo.as_deref());
            decal.normal_image = decal_texture_image(decal.texture_normal.as_deref());
            decal.orm_image = decal_texture_image(decal.texture_orm.as_deref());
            decal.emission_image = decal_texture_image(decal.texture_emission.as_deref());
            out.decals.push(decal);
        } else if is_probe {
            let mut probe = ReflectionProbe::new(ReflectionProbeId(next_probe_id));
            next_probe_id += 1;
            probe.transform = transform;
            probe.size = env_vec3(node, "size", probe.size);
            probe.origin_offset = env_vec3(node, "origin_offset", probe.origin_offset);
            probe.intensity = env_f32(node, "intensity", probe.intensity);
            // ⚠️ Registered in ClassDB and shown by the inspector long before it
            // reached the renderer, so an authored `blend_distance` round-tripped
            // through the .tscn and changed nothing (pat-s3rrv.206).
            probe.blend_distance = env_f32(node, "blend_distance", probe.blend_distance);
            probe.max_distance = env_f32(node, "max_distance", probe.max_distance);
            probe.box_projection = env_bool(node, "box_projection", probe.box_projection);
            probe.interior = env_bool(node, "interior", probe.interior);
            probe.enable_shadows = env_bool(node, "enable_shadows", probe.enable_shadows);
            probe.ambient_color_energy =
                env_f32(node, "ambient_color_energy", probe.ambient_color_energy);
            if let gdvariant::Variant::Color(c) = node.get_property("ambient_color") {
                probe.ambient_color = c;
            }
            if let gdvariant::Variant::Int(mask) = node.get_property("cull_mask") {
                probe.cull_mask = mask as u32;
            }
            probe.mesh_lod_threshold =
                env_f32(node, "mesh_lod_threshold", probe.mesh_lod_threshold);
            // ⚠️ `ambient_mode` gates the probe's whole ambient contribution in
            // `sample_probes_at` — leaving it unread meant `ambient_mode = 0`
            // (Disabled) in a `.tscn` was silently ignored and the probe lit
            // anyway. Unknown ints keep the default rather than falling through
            // to Disabled, so a newer Godot enum value does not blank a probe.
            if let gdvariant::Variant::Int(mode) = node.get_property("ambient_mode") {
                probe.ambient_mode = match mode {
                    0 => ReflectionProbeAmbientMode::Disabled,
                    1 => ReflectionProbeAmbientMode::Environment,
                    2 => ReflectionProbeAmbientMode::ConstantColor,
                    _ => probe.ambient_mode,
                };
            }
            if let gdvariant::Variant::Int(mode) = node.get_property("update_mode") {
                probe.update_mode = match mode {
                    0 => ReflectionProbeUpdateMode::Once,
                    1 => ReflectionProbeUpdateMode::Always,
                    _ => probe.update_mode,
                };
            }
            out.reflection_probes.push(probe);
        } else if is_voxel_gi {
            // ⚠️ The node carries `size` and `subdiv`; the rest of the surface
            // (energy/bias/normal_bias/propagation/interior) belongs to the
            // `VoxelGIData` sub-resource, which the `.tscn` loader flattens onto
            // the node — the same treatment `OccluderInstance3D`'s shape gets
            // below. Reading them off the node is therefore correct, not a
            // shortcut.
            let mut gi = VoxelGI::new(VoxelGIId(next_voxel_gi_id));
            next_voxel_gi_id += 1;
            gi.transform = transform;
            gi.size = env_vec3(node, "size", gi.size);
            // Godot's default is subdiv 1 (128³), NOT the zeroth constant — so
            // an absent property must keep the type's own default rather than
            // fall to `Subdiv64`.
            if let gdvariant::Variant::Int(sd) = node.get_property("subdiv") {
                gi.subdiv = match sd {
                    0 => VoxelGISubdiv::Subdiv64,
                    2 => VoxelGISubdiv::Subdiv256,
                    3 => VoxelGISubdiv::Subdiv512,
                    _ => VoxelGISubdiv::Subdiv128,
                };
            }
            gi.energy = env_f32(node, "energy", gi.energy);
            gi.bias = env_f32(node, "bias", gi.bias);
            gi.normal_bias = env_f32(node, "normal_bias", gi.normal_bias);
            gi.propagation = env_f32(node, "propagation", gi.propagation);
            gi.interior = env_bool(node, "interior", gi.interior);
            out.voxel_gis.push(gi);
        } else {
            let mut inst = OccluderInstance3D::new();
            inst.transform = transform;
            inst.occluder = occluder_shape_from_node(node);
            if let gdvariant::Variant::Int(mask) = node.get_property("bake_mask") {
                inst.bake_mask = mask as u32;
            }
            inst.bake_simplification_distance = env_f32(
                node,
                "bake_simplification_distance",
                inst.bake_simplification_distance,
            );
            out.occluders.push(inst);
        }
    }
    out
}

/// The occluder shape an `OccluderInstance3D` node carries.
///
/// Godot points `occluder` at a separate `Occluder3D` *resource*; a `.tscn`
/// spells that as a sub-resource, which the loader flattens onto the node (see
/// the `__x__<key>` companion convention). Both spellings are read here, and a
/// node that names no shape yields `None` — an occluder with no shape occludes
/// nothing, which is what Godot does too.
fn occluder_shape_from_node(node: &gdscene::node::Node) -> Option<Occluder3D> {
    let kind = match node.get_property("occluder") {
        gdvariant::Variant::String(s) => s,
        _ => match node.get_property("__occluder__type") {
            gdvariant::Variant::String(s) => s,
            _ => return None,
        },
    };
    let size = env_vec3(node, "__occluder__size", Vector3::new(1.0, 1.0, 1.0));
    match kind.as_str() {
        "BoxOccluder3D" => Some(Occluder3D::Box { size }),
        "SphereOccluder3D" => Some(Occluder3D::Sphere {
            radius: env_f32(node, "__occluder__radius", 1.0),
        }),
        "QuadOccluder3D" => Some(Occluder3D::Quad {
            size: Vector2::new(size.x, size.y),
        }),
        _ => None,
    }
}

/// Populates `renderer` with one solid instance per drawable spatial node (mesh
/// or primitive marker) at its global transform, and returns the overlay geometry
/// (selection outlines + node/collision gizmos) to draw over the frame afterward.
///
/// Written against `&mut dyn RenderingServer3D` so the software and GPU editor
/// paths share the exact same scene-building logic — only the concrete rasterizer
/// differs. Collision shapes are wireframe-only (recorded, not instanced). Takes a
/// pre-built render list (see [`Editor3DRenderInputs`]) so no tree read happens
/// here — the GPU path calls this off the state lock.
fn populate_editor_scene_3d(
    renderer: &mut dyn RenderingServer3D,
    viewport: &Viewport3D,
    items: &[Render3DItem],
    env_effects: &SceneEnvEffects3D,
    // Carried through rather than recomputed: this function has no SceneTree, so
    // the grid is snapshotted upstream in `editor_3d_render_inputs`
    // (pat-0q2kx.249).
    root_motion_grid: Option<RootMotionGridOverlay>,
) -> EditorOverlays3D {
    // Environment effects first: `is_occluded` runs during the instance draw
    // loop, so the occluders have to be in place before the meshes are, or the
    // first frame after a change culls nothing (pat-0q2kx.132).
    for occluder in &env_effects.occluders {
        renderer.add_occluder(occluder.clone());
    }
    for (volume, transform) in &env_effects.fog_volumes {
        renderer.add_fog_volume(volume.clone(), *transform);
    }
    for decal in &env_effects.decals {
        renderer.add_decal(decal.clone());
    }
    for probe in &env_effects.reflection_probes {
        renderer.add_reflection_probe(probe.id);
        renderer.update_reflection_probe(probe);
    }
    // The same add-then-update pair the probes use: `add_*` reserves the slot,
    // `update_*` fills it. Both are `RenderingServer3D` methods, so this works
    // for the software and GPU editor renderers alike (pat-s3rrv.114).
    for gi in &env_effects.voxel_gis {
        renderer.add_voxel_gi(gi.id);
        renderer.update_voxel_gi(gi);
    }
    let mut selected_tfs: Vec<Transform3D> = Vec::new();
    let mut primary_tf: Option<Transform3D> = None;
    let mut node_gizmos: Vec<(Transform3D, PrimitiveShape)> = Vec::new();
    let mut collision_gizmos: Vec<(Transform3D, CollisionShapeKind, Vector3)> = Vec::new();
    for item in items {
        // Collision shapes are wireframe-only; record selection and skip the
        // solid mesh instance.
        if let Render3DKind::Collision {
            shape,
            half_extents,
        } = &item.kind
        {
            collision_gizmos.push((item.global_transform, *shape, *half_extents));
            if viewport.selection.is_selected(item.node_id) {
                selected_tfs.push(item.global_transform);
                if viewport.selection.primary() == Some(item.node_id) {
                    primary_tf = Some(item.global_transform);
                }
            }
            continue;
        }
        let (mesh, color) = match &item.kind {
            // Shade the mesh in its material albedo when it has one, else default.
            Render3DKind::Mesh { path } => (
                mesh_for_primitive(path),
                item.albedo.unwrap_or(MESH_COLOR_3D),
            ),
            Render3DKind::Collision { .. } => unreachable!("handled above"),
            Render3DKind::Geometry { mesh } => (mesh.clone(), item.albedo.unwrap_or(MESH_COLOR_3D)),
            Render3DKind::Primitive { shape } => {
                if matches!(shape, PrimitiveShape::Camera | PrimitiveShape::Light) {
                    node_gizmos.push((item.global_transform, *shape));
                }
                let color = match shape {
                    PrimitiveShape::Camera => CAMERA_COLOR_3D,
                    PrimitiveShape::Light => LIGHT_COLOR_3D,
                    PrimitiveShape::Box => BOX_COLOR_3D,
                };
                (Mesh3D::cube(0.4), color)
            }
        };
        if viewport.selection.is_selected(item.node_id) {
            selected_tfs.push(item.global_transform);
            if viewport.selection.primary() == Some(item.node_id) {
                primary_tf = Some(item.global_transform);
            }
        }
        let id = renderer.create_instance();
        renderer.set_mesh(id, mesh);
        // The authored material wins; a mesh without one keeps the flat shade.
        renderer.set_material(
            id,
            item.material
                .clone()
                .unwrap_or_else(|| solid_material_3d(color)),
        );
        // A ShaderMaterial is a SEPARATE renderer channel from `set_material`,
        // and both the software rasterizer (gdrender3d/renderer.rs:1102, which
        // reads `inst.shader_material` per fragment) and the wgpu pipeline
        // already implement it — the editor path simply never passed one
        // (pat-mgixk.50).
        if let Some(shader_mat) = item.shader_material.clone() {
            renderer.set_shader_material(id, shader_mat);
        }
        // `surface_material_override/N`: a THIRD renderer channel, because a
        // per-surface override cannot be expressed by `set_material`'s single
        // whole-mesh material. `set_surface_materials` is already implemented by
        // the software rasterizer; the editor path never called it
        // (pat-mgixk.50).
        //
        // ⚠️ Guarded on non-empty so a mesh with no per-surface overrides sends
        // nothing and renders byte-identically to before.
        if !item.surface_materials.is_empty() {
            renderer.set_surface_materials(id, item.surface_materials.clone());
        }
        renderer.set_transform(id, item.global_transform);
        renderer.set_visible(id, true);
        // GeometryInstance3D (pat-nzgdu.76): cast_shadow=Off must drop this
        // instance out of the shadow maps, and a mesh past its
        // visibility_range_end must not be drawn — in the editor viewport too,
        // not just the runtime adapter.
        renderer.set_geometry_instance_settings(id, item.geometry.clone());
    }
    EditorOverlays3D {
        selected_tfs,
        primary_tf,
        node_gizmos,
        collision_gizmos,
        root_motion_grid,
    }
}

/// Adds the editor's key lighting to `renderer`: every scene
/// `DirectionalLight3D` (up to 4) with its real color, energy, and angular size
/// like Godot, or the preview sun when the scene has none. Each contributes to
/// both surface shading and the sky's sun disc. Godot's "Preview Sun" toggle
/// dims the sun to a flat fill (and hides its sky disc) when off. Shared by both
/// editor render paths.
fn setup_editor_lighting_3d(
    renderer: &mut dyn RenderingServer3D,
    viewport: &Viewport3D,
    scene_lights: &[SceneLight3D],
) {
    if scene_lights.is_empty() {
        // pat-mgixk.51: Godot's runtime has no preview sun. A scene that
        // authored its own environment is being rendered as a GAME, so an
        // unlit scene must come out unlit (ambient only) rather than being
        // handed a light it never placed.
        if viewport.environment.authored {
            return;
        }
        // No scene light: shine the preview sun from its configured direction,
        // color, and energy (Godot's Sun and Environment settings).
        let mut light = Light3D::directional(Light3DId(9_000_001));
        light.direction = viewport.environment.sun_direction;
        light.color = viewport.environment.sun_color;
        light.energy = if viewport.environment.sun_enabled {
            viewport.environment.sun_energy.max(0.0)
        } else {
            // Godot removes the preview sun when toggled off; the dim fill light
            // is an editor-only shading aid and must not draw a sky sun disc.
            light.sky = false;
            0.18
        };
        renderer.add_light(light.id);
        renderer.update_light(&light);
        return;
    }

    // Scene lights: add each with its authored properties so the sky sun disc(s)
    // and surface shading match the lights in the scene. Omni and spot carry a
    // position plus range/attenuation (and a cone, for spot) instead of just a
    // direction (pat-0q2kx.41).
    for (i, sl) in scene_lights.iter().enumerate() {
        let light = scene_light_to_light3d(sl, Light3DId(9_000_001 + i as u64));
        renderer.add_light(light.id);
        renderer.update_light(&light);
    }
}

/// Converts an editor [`SceneLight3D`] into the renderer's [`Light3D`]
/// (pat-s3rrv.114).
///
/// ⚠️ Extracted from the render push, which was its only home. Anything
/// else needing real `Light3D` values — the VoxelGI bake, for one — had
/// nowhere to get them, and a second copy of this twenty-field mapping
/// would be two hand-maintained copies of one conversion: the frame and
/// the bake would then be lit by subtly different lights, with nothing to
/// say so.
///
/// Behaviour-preserving: the body is the render loop's, verbatim.
pub(crate) fn scene_light_to_light3d(sl: &SceneLight3D, id: Light3DId) -> Light3D {
    let mut light = match sl.kind {
        SceneLightKind::Directional => Light3D::directional(id),
        // Point is Godot's omni. Spot starts from the same positional light
        // and switches type, since the library has no spot constructor.
        SceneLightKind::Omni => Light3D::point(id, sl.position),
        SceneLightKind::Spot => {
            let mut l = Light3D::point(id, sl.position);
            l.light_type = LightType::Spot;
            l.spot_angle = sl.spot_angle;
            l.spot_angle_attenuation = sl.spot_angle_attenuation;
            l
        }
    };
    if sl.kind != SceneLightKind::Directional {
        light.position = sl.position;
        light.range = sl.range;
        light.attenuation = sl.attenuation;
    }
    // Without this no shadow map is ever generated, which is why shadows
    // never appeared in the product however they were authored.
    light.shadow_enabled = sl.shadow_enabled;
    light.direction = sl.direction;
    light.color = sl.color;
    light.energy = sl.energy.max(0.0);
    light.angular_distance = sl.angular_distance;
    // Everything the editor path used to drop on the floor (pat-s3rrv.90).
    // `light.color` above already carries the `light_temperature` tint,
    // folded in by the shared reader — that is the one property here the
    // rasterizer actually consumes, because it reads `color` and knows
    // nothing about temperature.
    light.temperature = sl.temperature;
    light.cull_mask = sl.cull_mask;
    light.indirect_energy = sl.indirect_energy;
    light.volumetric_fog_energy = sl.volumetric_fog_energy;
    light.distance_fade_enabled = sl.distance_fade_enabled;
    light.distance_fade_begin = sl.distance_fade_begin;
    light.distance_fade_shadow = sl.distance_fade_shadow;
    light.distance_fade_length = sl.distance_fade_length;
    light.shadow_opacity = sl.shadow_opacity;
    light.shadow_caster_mask = sl.shadow_caster_mask;
    // ⚠️ These six were read by the shared reader and then DROPPED here, so
    // they reached `RenderServer3DAdapter` (which has no non-test callers)
    // but never the shipped editor or Play view. Carrying a property into
    // `SceneLight3D` is only half the wiring; this copy is the other half,
    // and a field missing from it fails silently at its default.
    light.shadow_bias = sl.shadow_bias;
    light.shadow_normal_bias = sl.shadow_normal_bias;
    light.specular = sl.specular;
    light.negative = sl.negative;
    light.directional_shadow_max_distance = sl.directional_shadow_max_distance;
    light.directional_shadow_fade_start = sl.directional_shadow_fade_start;
    light.shadow_split_1 = sl.shadow_split_1;
    light.shadow_split_2 = sl.shadow_split_2;
    light.shadow_split_3 = sl.shadow_split_3;
    light.blend_splits = sl.blend_splits;
    // Godot's sky_mode: LIGHT_ONLY lights shade the scene but draw no sky
    // sun disc (sky_suns_from_lights filters on this); SKY_ONLY lights draw
    // the disc but do not shade the scene (affects_scene gates that).
    light.sky = sl.sky;
    light.affects_scene = sl.affects_scene;
    light
}

/// Builds the `RenderViewport3D` (editor camera + clip planes) at `width`×
/// `height`. The background/environment is applied by the caller via
/// [`editor_render_environment`].
fn editor_render_viewport(viewport: &Viewport3D, width: u32, height: u32) -> RenderViewport3D {
    let mut rv = RenderViewport3D::new(width, height);

    // pat-s3rrv.186: `h_offset`/`v_offset` shift the VIEW, not the node.
    // Godot's `Camera3D::_get_adjusted_camera_transform`
    // (camera_3d.cpp:253-258) orthonormalizes, then adds
    // `basis.column(1) * v_offset` and `basis.column(0) * h_offset` to the
    // origin — so the shift is along the camera's own right/up axes, not the
    // world's, and a rotated camera offsets diagonally in world space.
    // NOTE: `orthonormalized` belongs to `Basis`, not `Transform3D`.
    let mut tr = viewport.camera.transform();
    tr.basis = tr.basis.orthonormalized();
    tr.origin =
        tr.origin + tr.basis.y * viewport.camera.v_offset + tr.basis.x * viewport.camera.h_offset;
    rv.camera_transform = tr;

    // `fov` is VERTICAL here. Under KEEP_WIDTH the authored angle is
    // horizontal, so it is re-derived for the viewport's aspect exactly as
    // `projection.cpp:253-255` does via `get_fovy(fov, 1.0 / aspect)`.
    let aspect = (width.max(1) as f32) / (height.max(1) as f32);
    rv.fov = match viewport.camera.keep_aspect {
        gdscene::camera3d::KeepAspect::KeepHeight => viewport.camera.fov_radians(),
        gdscene::camera3d::KeepAspect::KeepWidth => {
            let half = (viewport.camera.fov_radians() * 0.5).tan();
            ((half / aspect.max(f32::EPSILON)).atan()) * 2.0
        }
    };

    // ⚠️ These were HARDCODED to 0.05 / 2000.0, so a scene camera's authored
    // near/far never clipped anything — and 2000 is not even Godot's default
    // far of 4000, so distant geometry vanished for a reason no property
    // explained.
    rv.near = viewport.camera.near;
    rv.far = viewport.camera.far;
    rv
}

/// The environment for the editor viewport render: Godot's "Preview
/// Environment" toolbar toggle shows a procedural sky gradient (bluish top →
/// light horizon → ground) when on, and the flat editor void color when off.
///
/// Shared by the software and GPU render paths so their backgrounds match
/// 1:1 — this environment is the single source of truth; neither path decides
/// its background on its own. The flat backdrop uses `CustomColor` (not
/// `ClearColor`) because the renderer's background fill paints `ClearColor`
/// black; `CustomColor` carries the actual void color through both backends.
pub(crate) fn editor_render_environment(
    viewport: &Viewport3D,
) -> gdserver3d::environment::Environment3D {
    // Start from the preview's own environment so its post-processing settings
    // (Godot's editor default: filmic tonemapping + glow, plus ambient/fog)
    // reach the renderer — not just the background. Then override the
    // background per the Preview Environment / Preview Sky toggles.
    let mut env = viewport.environment.environment.clone();
    // pat-mgixk.51: a game frame renders the environment the SCENE authored, so
    // the Preview Environment / Preview Sky toggles — which are editor chrome —
    // must not reach it. Without this early return the override below forced
    // `background_mode = Sky` on every frame whose preview was enabled, which is
    // the default, so an authored `background_mode = Color` could never be seen.
    if viewport.environment.authored {
        return env;
    }
    if viewport.environment.enabled && viewport.environment.sky_visible {
        env.background_mode = gdserver3d::environment::BackgroundMode::Sky;
        if env.sky.is_none() {
            env.sky = Some(gdserver3d::sky::Sky::default());
        }
    } else {
        // Preview sky hidden: flat editor backdrop, no sky resource (so nothing
        // — background or ambient — samples it). Tonemap/glow still apply.
        env.background_mode = gdserver3d::environment::BackgroundMode::CustomColor;
        env.background_color = VIEWPORT_BG_3D;
        env.sky = None;
    }
    env
}

/// Draws all editor overlays over a rasterized mesh frame `fb`: the ground grid +
/// colored origin axes, per-node gizmos (camera frustums / light rays), collision
/// wireframes, selection outlines, and the transform gizmo on the primary
/// selection. Shared by both editor render paths so the chrome is identical.
fn draw_editor_overlays_3d(
    fb: &mut impl Raster,
    viewport: &Viewport3D,
    overlays: &EditorOverlays3D,
) {
    // --- Ground grid + origin axes, projected as crisp anti-aliased lines. ---
    draw_grid_overlay(fb, viewport);

    // --- RootMotionView's counter-scrolled locomotion grid (pat-0q2kx.249). ---
    if let Some(grid) = &overlays.root_motion_grid {
        draw_root_motion_grid(fb, viewport, grid);
    }

    // --- Per-node gizmos: camera frustums and light direction rays. ---
    for (tf, shape) in &overlays.node_gizmos {
        match shape {
            PrimitiveShape::Camera => draw_camera_gizmo(fb, viewport, tf),
            PrimitiveShape::Light => draw_light_gizmo(fb, viewport, tf),
            PrimitiveShape::Box => {}
        }
    }

    // --- CollisionShape3D wireframe gizmos (Godot's collision shapes). ---
    for (tf, kind, ext) in &overlays.collision_gizmos {
        draw_collision_gizmo(fb, viewport, tf, *kind, *ext);
    }

    // --- Selection outlines (every selected node) + the transform gizmo on the
    //     primary node (drawn on top). ---
    for tf in &overlays.selected_tfs {
        draw_selection_box(fb, viewport, tf, 0.55);
    }
    if let Some(tf) = overlays
        .primary_tf
        .or_else(|| overlays.selected_tfs.first().copied())
    {
        use crate::viewport_3d::GizmoMode3D;
        let len = gizmo_arrow_length(viewport);
        match viewport.selection.gizmo_mode {
            // Scale uses the same axis-and-handle look as move (cube handles).
            GizmoMode3D::Move | GizmoMode3D::Scale => draw_move_gizmo(fb, viewport, tf.origin, len),
            GizmoMode3D::Rotate => draw_rotate_gizmo(fb, viewport, tf.origin, len),
            GizmoMode3D::Select => {}
        }
    }
}

/// pat-nzgdu.173: renders the scene from a scene `Camera3D`'s viewpoint — the
/// frame behind the camera-preview inset and cinematic mode. The preview
/// viewport's camera is pinned to the node's global transform (via the camera's
/// `override_transform`), with fov/projection read off the node, so the frame is
/// what that `Camera3D` sees — distinct from the editor's orbit camera.
///
/// This is the *editor's* preview: unlike [`render_game_3d_frame`] it keeps the
/// editor furniture (grid, gizmos), because the inset previews the shot inside the
/// editor rather than standing in for the game view.
pub fn render_camera3d_preview(
    tree: &SceneTree,
    camera_node: NodeId,
    width: u32,
    height: u32,
) -> FrameBuffer {
    let viewport = viewport_for_scene_camera(tree, camera_node, width, height);
    render_editor_3d_frame(&viewport, tree)
}

/// A [`Viewport3D`] that sees exactly what a scene `Camera3D` sees: its camera is
/// pinned to the node's global transform via `override_transform`, bypassing the
/// orbit derivation, with projection/fov/size read off the node.
///
/// Shared by the camera-preview inset (pat-nzgdu.173) and the Play/game view
/// (pat-0q2kx.30) so both agree on what "through that camera" means.
pub fn viewport_for_scene_camera(
    tree: &SceneTree,
    camera_node: NodeId,
    width: u32,
    height: u32,
) -> Viewport3D {
    let mut viewport = Viewport3D::new(width.max(1), height.max(1));
    viewport.camera.override_transform =
        Some(gdscene::node3d::get_global_transform(tree, camera_node));
    viewport.camera.projection = match gdscene::camera3d::get_projection_type(tree, camera_node) {
        gdscene::camera3d::ProjectionType::Orthogonal => {
            crate::viewport_3d::Projection::Orthographic
        }
        // Perspective (and Frustum, which the software renderer approximates as
        // perspective) both render through the perspective path.
        _ => crate::viewport_3d::Projection::Perspective,
    };
    if let Some(node) = tree.get_node(camera_node) {
        if let gdvariant::Variant::Float(fov) = node.get_property("fov") {
            viewport.camera.fov_degrees = fov as f32;
        }
        if let gdvariant::Variant::Float(size) = node.get_property("size") {
            viewport.camera.ortho_size = size as f32;
        }
    }
    // pat-s3rrv.186: everything below was authored, correct and unread — the
    // accessors are unit-tested in `gdscene::camera3d`, and nothing on the
    // wired Play path called them. Read through the accessors rather than
    // matching `Variant::Float` as the two above do: a `.tscn` writing
    // `near = 2` stores an Int, which a Float-only match silently skips,
    // leaving the Godot default in place.
    viewport.camera.near = gdscene::node3d::get_near(tree, camera_node) as f32;
    viewport.camera.far = gdscene::node3d::get_far(tree, camera_node) as f32;
    viewport.camera.keep_aspect = gdscene::camera3d::get_keep_aspect(tree, camera_node);
    viewport.camera.h_offset = gdscene::camera3d::get_h_offset(tree, camera_node) as f32;
    viewport.camera.v_offset = gdscene::camera3d::get_v_offset(tree, camera_node) as f32;
    viewport.camera.frustum_offset = gdscene::camera3d::get_frustum_offset(tree, camera_node);
    viewport
}

/// Installs the environment a **running game** renders this scene with onto
/// `viewport`, in place of the editor preview (pat-mgixk.51).
///
/// Resolution order is Godot's, and lives in
/// [`gdscene::world_environment::runtime_environment_3d`] so the editor's Play
/// view and `RenderServer3DAdapter` cannot disagree about what a scene asked
/// for: the camera's own `environment`, then the scene's `WorldEnvironment`,
/// then `rendering/environment/defaults/default_environment`, then the project's
/// `default_clear_color`.
///
/// Returns `true` when something was installed. `false` leaves the preview
/// untouched, which is what a scene that authors no environment and has no
/// readable project still gets — so this cannot change a frame that had nothing
/// to say.
///
/// ⚠️ Setting `environment.authored` is half the point: it is what suppresses
/// the Preview Sky background override in [`editor_render_environment`] and the
/// preview sun in `setup_editor_lighting_3d`. Assigning the environment without
/// it would leave an authored `background_mode = Color` overridden to `Sky` and
/// an unlit scene lit by a sun it never placed.
pub fn apply_runtime_environment_3d(
    tree: &SceneTree,
    camera: NodeId,
    viewport: &mut Viewport3D,
) -> bool {
    match gdscene::world_environment::runtime_environment_3d(tree, Some(camera)) {
        Some(env) => {
            viewport.environment.environment = env;
            viewport.environment.authored = true;
            true
        }
        None => false,
    }
}

/// `true` for a render item that exists only to make an invisible node visible in
/// the *editor*: the orange camera/light marker cubes and the collision-shape
/// wireframes.
///
/// A running game shows none of them (pat-0q2kx.30) — Godot's game viewport draws
/// the scene, not the editor's affordances, and collision shapes appear only behind
/// the "Visible Collision Shapes" debug toggle, which is off by default. Only MESH
/// and generated-geometry items survive into a game frame.
///
/// 🔑 **Every `Primitive` is stripped, `Box` included** (pat-mgixk.51). This used to
/// keep box markers on the reasoning that they "stand in for scene content". They do
/// not: `primitive_for_class` maps EVERYTHING that is not a Camera or a Light to
/// `Box`, so a running game drew a grey `Mesh3D::cube(0.4)` at every plain `Node3D`,
/// `Marker3D`, `Area3D`, `Skeleton3D`, `AudioStreamPlayer3D` and physics body. Godot
/// draws nothing for any of them — none is a `VisualInstance3D`
/// (`scene/3d/node_3d.cpp`). The editor viewport, which does not call this, still
/// draws them.
///
/// ⚠️ `Box` is ALSO where a `MeshInstance3D` whose mesh did not resolve lands, and
/// stripping it is right for that case too: Godot draws nothing for a
/// `VisualInstance3D` with no geometry either, so the cube was an editor affordance
/// exactly like the camera gizmo.
///
/// That second case is why this change touches two fixtures. Both
/// `play_uses_scene_camera3d` and `render3d_hidden_parent_top_level` (pat-mgixk.32)
/// used a mesh-less `MeshInstance3D` as their stand-in for real content, so each was
/// pinning the PLACEHOLDER. Both now carry `mesh = "primitive:BoxMesh"`, which makes
/// their assertions mean what they say. pat-mgixk.32's fixture comment justified
/// going mesh-less with "`is_pivot_only_class` is false so it is never stripped" —
/// that function no longer exists, so the justification was stale.
fn is_editor_only_marker(item: &Render3DItem) -> bool {
    matches!(
        item.kind,
        Render3DKind::Primitive { .. } | Render3DKind::Collision { .. }
    )
}

/// Drops the editor-only items from a render-input snapshot, so a game frame shows
/// the scene without the camera/light marker cubes (pat-0q2kx.30).
///
/// Exposed for the GPU Play path, which builds its own inputs and cannot go through
/// [`render_game_3d_frame`].
pub fn strip_editor_only_items(inputs: &mut Editor3DRenderInputs) {
    inputs.items.retain(|item| !is_editor_only_marker(item));
    // `Light3D.editor_only` lights light the editor viewport but must not reach
    // a game frame (pat-0q2kx.110). Dropping them here rather than at collection
    // is what lets one snapshot serve both paths.
    inputs.scene_lights.retain(|light| !light.editor_only);
    // RootMotionView is an editor-only visualisation and hides at runtime
    // (pat-0q2kx.249). The software Play path already never calls
    // `draw_editor_overlays_3d`, but the GPU Play path builds its own inputs
    // through here, so clearing it is what makes the guarantee hold for both.
    inputs.root_motion_grid = None;
}

/// The render layers a spatial node is drawn on — `VisualInstance3D.layers`,
/// default `1` (Godot doc `VisualInstance3D.xml`).
fn visual_layers(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("layers") {
            gdvariant::Variant::Int(i) => i,
            _ => 1,
        })
        .unwrap_or(1)
}

/// The camera's `cull_mask` applied to the render list: a node is drawn only when
/// its layers intersect the mask (`Camera3D.cull_mask`, default all 20 layers).
fn strip_cull_masked_items(
    tree: &SceneTree,
    camera_node: NodeId,
    inputs: &mut Editor3DRenderInputs,
) {
    let mask = gdscene::camera3d::get_cull_mask(tree, camera_node);
    // The render items carry raw ids, so collect the excluded ids from one tree
    // walk rather than looking each item's node back up.
    let culled: std::collections::HashSet<u64> = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .filter(|id| visual_layers(tree, *id) & mask == 0)
        .map(|id| id.raw())
        .collect();
    if culled.is_empty() {
        return;
    }
    inputs.items.retain(|item| !culled.contains(&item.node_id));
}

/// Builds the render inputs a *running game* draws from: the editor's list minus
/// the editor-only markers, minus everything the camera's `cull_mask` excludes
/// (pat-0q2kx.30).
///
/// `viewport` should come from [`viewport_for_scene_camera`] so the list is culled
/// against the camera the game actually renders from.
pub fn game_3d_render_inputs(
    tree: &SceneTree,
    camera_node: NodeId,
    viewport: &Viewport3D,
) -> Editor3DRenderInputs {
    let mut inputs = editor_3d_render_inputs(viewport, tree);
    strip_editor_only_items(&mut inputs);
    strip_cull_masked_items(tree, camera_node, &mut inputs);
    inputs
}

/// The `Camera3D` the Play view renders from, honouring the editor's runtime
/// camera override.
///
/// With the override on, the editor deliberately keeps its own orbit camera while
/// the game runs (Godot's "Camera Override" button), so this returns `None` and the
/// caller uses the editor viewport.
pub fn game_camera_for_play(tree: &SceneTree, camera_override: bool) -> Option<NodeId> {
    if camera_override {
        return None;
    }
    gdscene::camera3d::current_camera3d(tree)
}

/// Renders the running scene the way the *game* sees it: from the scene's current
/// `Camera3D`, with no editor furniture (pat-0q2kx.30).
///
/// Returns `None` when the scene has no `Camera3D`. ⚠️ Godot then draws NO 3D at
/// all — the view is the project's clear colour — so a caller must not fall back
/// to the editor viewport here (pat-p3p1m: that fallback put the editor grid and
/// gizmos into the Play view). See [`crate::play_3d`].
///
/// Four differences from [`render_editor_3d_frame`], all of them the point of the
/// bead: the view comes from the scene camera rather than the editor's orbit
/// camera; the grid and origin axes are off; the camera/light marker cubes are
/// filtered out of the render list; and the camera's `cull_mask` is applied.
pub fn render_game_3d_frame(tree: &SceneTree, width: u32, height: u32) -> Option<FrameBuffer> {
    let camera = gdscene::camera3d::current_camera3d(tree)?;
    let mut viewport = viewport_for_scene_camera(tree, camera, width, height);
    // Editor furniture off. `Viewport3D::new` starts with an empty selection, so
    // no selection outline can leak in either.
    viewport.grid.visible = false;
    viewport.show_origin = false;
    // The scene's own environment, not the editor preview (pat-mgixk.51).
    apply_runtime_environment_3d(tree, camera, &mut viewport);

    let inputs = game_3d_render_inputs(tree, camera, &viewport);
    Some(render_editor_3d_frame_from_inputs(&viewport, &inputs))
}

/// Renders the editor 3D viewport — its camera, the scene's spatial nodes, and a
/// projected ground grid — to a pixel [`FrameBuffer`] using the **software** 3D
/// rasterizer. Spatial nodes are drawn as solid geometry colored by kind; the
/// ground grid, colored origin axes, node gizmos, and selection chrome are then
/// drawn over the frame as crisp anti-aliased projected lines, the way Godot
/// draws them.
///
/// This is the reference path (and the fallback for the GPU path); the
/// GPU-accelerated twin is [`render_editor_3d_frame_gpu`].
pub fn render_editor_3d_frame(viewport: &Viewport3D, tree: &SceneTree) -> FrameBuffer {
    let inputs = editor_3d_render_inputs(viewport, tree);
    render_editor_3d_frame_from_inputs(viewport, &inputs)
}

/// Edge length of each reflection-probe cubemap face, in texels.
///
/// Small on purpose: a probe re-renders the whole scene once per face, six
/// faces per capture, so this is the term that decides whether
/// `update_mode = Always` is affordable at all.
const REFLECTION_PROBE_CAPTURE_RESOLUTION: u32 = 32;

/// Core software render from a pre-built [`Editor3DRenderInputs`] snapshot — the
/// no-tree-read body shared by [`render_editor_3d_frame`] and the GPU path's
/// software fallback, so neither touches the scene tree while rendering.
fn render_editor_3d_frame_from_inputs(
    viewport: &Viewport3D,
    inputs: &Editor3DRenderInputs,
) -> FrameBuffer {
    let width = viewport.width.max(1);
    let height = viewport.height.max(1);
    let mut renderer = SoftwareRenderer3D::new();

    let overlays = populate_editor_scene_3d(
        &mut renderer,
        viewport,
        &inputs.items,
        &inputs.env_effects,
        inputs.root_motion_grid,
    );
    setup_editor_lighting_3d(&mut renderer, viewport, &inputs.scene_lights);

    // --- Render from the editor camera. Godot's "Preview Environment" toolbar
    //     toggle shows a procedural sky gradient (bluish top → light horizon →
    //     ground) when on, and a flat editor backdrop when off. ---
    let mut rv = editor_render_viewport(viewport, width, height);
    rv.environment = Some(editor_render_environment(viewport));
    // Capture each probe's surroundings into a cubemap BEFORE shading, so a
    // reflective surface has something to sample (pat-s3rrv.206). Gated by
    // `needs_capture()`, which is where `update_mode` finally does something:
    // an `Once` probe pays for its six face renders exactly once, an `Always`
    // probe re-captures every frame.
    //
    // With no probes in the scene this returns immediately, so an ordinary
    // frame costs nothing extra.
    renderer.capture_reflection_probes(&rv, REFLECTION_PROBE_CAPTURE_RESOLUTION);

    let frame = renderer.render_frame(&rv);

    // --- FrameData3D -> editor FrameBuffer (both are row-major RGBA Color). ---
    let mut fb = FrameBuffer::new(width, height, VIEWPORT_BG_3D);
    let count = frame.pixels.len().min(fb.pixels.len());
    fb.pixels[..count].copy_from_slice(&frame.pixels[..count]);

    // --- CompositorEffect callbacks (pat-s3rrv.251) ------------------------
    //
    // Run BEFORE the editor overlays: the overlays are editor furniture — grid,
    // gizmos, root-motion view — and Godot's compositor operates on the game
    // frame. Tinting the gizmos would be a visible parity bug.
    //
    // Stages run in Godot's pipeline order, and effects within a stage in
    // insertion order, which is what `effects_for_stage` preserves. Disabled
    // effects are skipped there, and an effect with no callback is inert.
    if let Some(compositor) = inputs.compositor.as_ref() {
        let mut data = CompositorRenderData {
            width,
            height,
            color: &mut fb.pixels,
        };
        for stage in [
            EffectCallbackType::PreOpaque,
            EffectCallbackType::PostOpaque,
            EffectCallbackType::PostSky,
            EffectCallbackType::PreTransparent,
            EffectCallbackType::PostTransparent,
        ] {
            for effect in compositor.effects_for_stage(stage) {
                if let Some(callback) = effect.callback.as_ref() {
                    callback.render_callback(stage, &mut data);
                }
            }
        }
    }

    draw_editor_overlays_3d(&mut fb, viewport, &overlays);
    fb
}

/// [`render_editor_3d_frame`] with a compositor installed for this frame
/// (pat-s3rrv.251).
///
/// The effects hold Rust callbacks, which a `.tscn` cannot express, so the
/// compositor is supplied by the caller rather than parsed out of the scene.
pub fn render_editor_3d_frame_with_compositor(
    viewport: &Viewport3D,
    tree: &SceneTree,
    compositor: Option<Compositor>,
) -> FrameBuffer {
    let mut inputs = editor_3d_render_inputs(viewport, tree);
    inputs.compositor = compositor;
    render_editor_3d_frame_from_inputs(viewport, &inputs)
}

/// `inputs`' reflection probes with their cubemaps captured, for the GPU path
/// (pat-p3p1m).
///
/// The capture is the software renderer's own `capture_reflection_probes`, run
/// on a throwaway software scene built exactly like the software frame, so both
/// backends reflect the same cubemap. Godot's `update_mode` decides reuse: an
/// `Once` probe keeps its first capture for as long as the probe itself is
/// unchanged (a thread-local cache keyed by the uncaptured probe), an `Always`
/// probe is re-captured every frame.
#[cfg(feature = "gpu-render")]
fn captured_reflection_probes(
    viewport: &Viewport3D,
    inputs: &Editor3DRenderInputs,
) -> Vec<ReflectionProbe> {
    use std::cell::RefCell;
    use std::sync::Arc;
    thread_local! {
        static CAPTURES: RefCell<Vec<(ReflectionProbe, Arc<gdserver3d::reflection_probe::ReflectionCubemap>)>> =
            const { RefCell::new(Vec::new()) };
    }
    let mut probes: Vec<ReflectionProbe> = inputs.env_effects.reflection_probes.clone();
    CAPTURES.with(|cell| {
        let mut cache = cell.borrow_mut();
        let mut pending = false;
        for probe in probes.iter_mut() {
            let mut key = probe.clone();
            key.captured = None;
            let hit = (probe.update_mode == ReflectionProbeUpdateMode::Once)
                .then(|| cache.iter().find(|(k, _)| *k == key).map(|(_, c)| c.clone()))
                .flatten();
            match hit {
                Some(cube) => probe.captured = Some(cube),
                None => pending = true,
            }
        }
        if !pending {
            return;
        }
        let mut sw = SoftwareRenderer3D::new();
        let mut fx = inputs.env_effects.clone();
        fx.reflection_probes = probes.clone();
        populate_editor_scene_3d(&mut sw, viewport, &inputs.items, &fx, None);
        setup_editor_lighting_3d(&mut sw, viewport, &inputs.scene_lights);
        let mut rv = editor_render_viewport(viewport, viewport.width.max(1), viewport.height.max(1));
        rv.environment = Some(editor_render_environment(viewport));
        sw.capture_reflection_probes(&rv, REFLECTION_PROBE_CAPTURE_RESOLUTION);
        for probe in probes.iter_mut() {
            let Some(done) = sw.reflection_probes().iter().find(|p| p.id == probe.id) else {
                continue;
            };
            if let Some(cube) = done.captured.clone() {
                if probe.captured.is_none() && probe.update_mode == ReflectionProbeUpdateMode::Once {
                    let mut key = probe.clone();
                    key.captured = None;
                    cache.retain(|(k, _)| k.id != key.id);
                    // Bounded: a probe that keeps changing replaces its own
                    // entry rather than accumulating.
                    cache.push((key, cube.clone()));
                }
                probe.captured = Some(cube);
            }
        }
    });
    probes
}

thread_local! {
    static SOFTWARE_FALLBACK_FRAMES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn note_software_fallback() {
    SOFTWARE_FALLBACK_FRAMES.with(|c| c.set(c.get() + 1));
}

/// How many frames [`render_editor_3d_frame_gpu`] rendered on the SOFTWARE
/// reference path on this thread (no GPU adapter, or a feature the WGSL path
/// cannot draw). Perf instrumentation (pat-p3p1m): a scene that silently falls
/// back runs ~10x slower in the Play view, and nothing else shows it.
pub fn software_fallback_frames() -> u64 {
    SOFTWARE_FALLBACK_FRAMES.with(|c| c.get())
}

/// GPU-accelerated twin of [`render_editor_3d_frame`] (pat-ism62 follow-through).
///
/// Rasterizes the scene meshes on the GPU via a **persistent** [`GpuRenderer3D`]
/// (its wgpu device + pipeline are created once and reused every frame), then
/// draws the identical editor overlays over the frame in software. The mesh
/// raster is the expensive part of the software path — moving it to the GPU is
/// what lets the live viewport render at full resolution and 60fps, instead of
/// the reduced-resolution software raster.
///
/// The renderer is thread-local: the editor's render loop is single-threaded, so
/// one persistent GPU renderer serves every frame. If no GPU adapter is available
/// (a headless environment), this transparently falls back to the software
/// [`render_editor_3d_frame`] for the life of the process.
///
/// Returns a byte-native [`Bgra8Image`] (top-down BGRA) — the format the live
/// viewport streams as BMP — so the whole GPU path stays in bytes with no
/// per-pixel `bytes → Color → bytes` round-trip.
///
/// Only compiled with the `gpu-render` feature (the default editor build); the
/// `software-render` build has only [`render_editor_3d_frame`].
#[cfg(feature = "gpu-render")]
pub fn render_editor_3d_frame_gpu(
    viewport: &Viewport3D,
    inputs: &Editor3DRenderInputs,
) -> Bgra8Image {
    use gdrender3d::wgpu_pipeline::GpuRenderer3D;
    use std::cell::RefCell;

    // Per-thread persistent GPU renderer. `Failed` means adapter init returned
    // None (no GPU) → use the software path forever.
    //
    // ⚠️ `ManuallyDrop`: the renderer is deliberately LEAKED when its thread
    // exits. Dropping wgpu buffers from a thread-local destructor touches
    // wgpu-core's own thread-locals (`snatch::LockTrace`), which may already be
    // destroyed by then, and that panics with "cannot access a Thread Local
    // Storage value during or after destruction" (pat-p3p1m: every test thread
    // that rendered a frame through here died at exit). The editor renders on
    // its main thread for the process's whole life, so nothing is lost.
    enum GpuSlot {
        Uninit,
        Failed,
        Ready(std::mem::ManuallyDrop<GpuRenderer3D>),
    }
    thread_local! {
        static GPU: RefCell<GpuSlot> = RefCell::new(GpuSlot::Uninit);
    }

    let width = viewport.width.max(1);
    let height = viewport.height.max(1);

    // Any sky feature that needs a texture the WGSL sky pass can't bind: a
    // procedural `sky_cover` or a physical `night_sky`. Render those on the
    // software reference path.
    //
    // 🔑 Environment FOG and ReflectionProbes used to route here too, and that
    // is what made 3D Play slow (pat-p3p1m): the official 3D platformer demo
    // authors both, so every Play frame ran the software rasterizer (~80 ms at
    // 995x807). The GPU path now applies the same `apply_fog_pass` on its
    // read-back depth, and shades probe ambient + captured probe reflections in
    // WGSL (`probe_sample`), with the cubemaps captured below by the software
    // renderer — once per `Once` probe, cached across frames.
    let render_env = editor_render_environment(viewport);
    let sky_needs_software = match render_env.sky.as_ref().map(|s| &s.material) {
        Some(gdserver3d::sky::SkyMaterial::Procedural(m)) => m.sky_cover.is_some(),
        Some(gdserver3d::sky::SkyMaterial::Physical(m)) => m.night_sky.is_some(),
        _ => false,
    };
    if sky_needs_software {
        note_software_fallback();
        return framebuffer_to_bgra(&render_editor_3d_frame_from_inputs(viewport, inputs));
    }

    GPU.with(|cell| {
        // Lazy init on the first call for this thread.
        {
            let mut slot = cell.borrow_mut();
            if matches!(&*slot, GpuSlot::Uninit) {
                *slot = match GpuRenderer3D::new() {
                    Some(r) => GpuSlot::Ready(std::mem::ManuallyDrop::new(r)),
                    None => GpuSlot::Failed,
                };
            }
        }
        let mut slot = cell.borrow_mut();
        let renderer: &mut GpuRenderer3D = match &mut *slot {
            GpuSlot::Ready(r) => r,
            // No GPU adapter → software reference path, converted to bytes (the
            // software path doesn't touch this thread-local, so the borrow is fine).
            _ => {
                note_software_fallback();
                return framebuffer_to_bgra(&render_editor_3d_frame_from_inputs(viewport, inputs));
            }
        };

        // The persistent renderer accumulates instances/lights across calls, so
        // reset it before rebuilding this frame's scene. Reborrow (`&mut *`) for
        // the shared `&mut dyn RenderingServer3D` helpers so `renderer` stays
        // usable for the render call below. All of this runs OFF the editor state
        // lock (the caller snapshotted `inputs` under it), so `/api` handlers stay
        // responsive during the render.
        renderer.reset_scene();
        // Probe cubemaps come from the software capture pass (the GPU
        // pipeline has no cubemap render of its own); `Once` probes reuse
        // their first capture (pat-p3p1m).
        let captured_effects;
        let env_effects = if inputs.env_effects.reflection_probes.is_empty() {
            &inputs.env_effects
        } else {
            let mut fx = inputs.env_effects.clone();
            fx.reflection_probes = captured_reflection_probes(viewport, inputs);
            captured_effects = fx;
            &captured_effects
        };
        let overlays = populate_editor_scene_3d(
            &mut *renderer,
            viewport,
            &inputs.items,
            env_effects,
            inputs.root_motion_grid,
        );
        setup_editor_lighting_3d(&mut *renderer, viewport, &inputs.scene_lights);
        let mut rv = editor_render_viewport(viewport, width, height);
        // Same environment as the software path: the GPU renderer draws the
        // procedural sky gradient (or flat void color) from it, so the two
        // backends produce identical backgrounds. The linear `Bgra8Unorm`
        // target keeps mesh bytes consistent with the software overlays drawn
        // next.
        rv.environment = Some(editor_render_environment(viewport));
        let (w, h, data) = renderer.render_editor_color_bgra(&rv, VIEWPORT_BG_3D);
        let mut img = Bgra8Image::from_bgra(w, h, data);
        // Composite the grid/gizmos/selection directly onto the BGRA bytes.
        draw_editor_overlays_3d(&mut img, viewport, &overlays);
        img
    })
}

/// Software-build stand-in for [`render_editor_3d_frame_gpu`]: with the GPU
/// renderer compiled out (`--no-default-features --features software-render`),
/// the "GPU" entry point renders in software and converts to bytes. Lets callers
/// (the editor example) call one name regardless of which backend is compiled in.
#[cfg(not(feature = "gpu-render"))]
pub fn render_editor_3d_frame_gpu(
    viewport: &Viewport3D,
    inputs: &Editor3DRenderInputs,
) -> Bgra8Image {
    note_software_fallback();
    framebuffer_to_bgra(&render_editor_3d_frame_from_inputs(viewport, inputs))
}

/// Converts a float [`FrameBuffer`] to a byte [`Bgra8Image`] (top-down BGRA).
/// Only used on the no-GPU software fallback of [`render_editor_3d_frame_gpu`], so
/// the per-pixel conversion cost is off the GPU hot path.
fn framebuffer_to_bgra(fb: &FrameBuffer) -> Bgra8Image {
    let mut data = Vec::with_capacity((fb.width * fb.height * 4) as usize);
    for c in &fb.pixels {
        data.push((c.b.clamp(0.0, 1.0) * 255.0) as u8);
        data.push((c.g.clamp(0.0, 1.0) * 255.0) as u8);
        data.push((c.r.clamp(0.0, 1.0) * 255.0) as u8);
        data.push((c.a.clamp(0.0, 1.0) * 255.0) as u8);
    }
    Bgra8Image::from_bgra(fb.width, fb.height, data)
}

/// Convenience wrapper: render the scene in 3D mode with a default editor
/// camera/grid sized to `width`x`height`. Used by the editor render loop and
/// tests where no persistent viewport camera state is kept yet.
pub fn render_editor_3d(tree: &SceneTree, width: u32, height: u32) -> FrameBuffer {
    let mut viewport = Viewport3D::new(width, height);
    // A 3/4 "floor" framing: camera above and to the side looking down at the
    // origin, so the ground grid reads as a receding floor rather than edge-on.
    // (Negative pitch tilts the view downward; see ViewportCamera3D.)
    viewport.camera.focus_point = Vector3::ZERO;
    viewport.camera.distance = 14.0;
    viewport.camera.yaw = 0.6;
    viewport.camera.pitch = -0.5;
    render_editor_3d_frame(&viewport, tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_3d::Viewport3D;
    use gdscene::node::Node;
    use gdscene::node3d;

    fn find<'a>(items: &'a [Render3DItem], node_id: u64) -> Option<&'a Render3DItem> {
        items.iter().find(|i| i.node_id == node_id)
    }

    /// Both render paths take their background from this one environment: the
    /// preview toggle on yields a procedural sky, off yields the flat void
    /// color carried as `CustomColor` (the renderer paints `ClearColor` black,
    /// which is not the editor backdrop). Regression guard for the GPU path
    /// ignoring the environment and clearing flat while the software path drew
    /// the sky.
    #[test]
    fn editor_environment_is_sky_when_preview_enabled_flat_otherwise() {
        use gdserver3d::environment::BackgroundMode;

        let mut viewport = Viewport3D::new(64, 64);
        viewport.environment.enabled = true;
        viewport.environment.sky_visible = true;
        let env = editor_render_environment(&viewport);
        assert_eq!(env.background_mode, BackgroundMode::Sky);
        assert!(env.sky.is_some(), "sky mode must carry a sky resource");

        viewport.environment.sky_visible = false;
        let env = editor_render_environment(&viewport);
        assert_eq!(env.background_mode, BackgroundMode::CustomColor);
        assert_eq!(env.background_color, VIEWPORT_BG_3D);
        assert!(env.sky.is_none());
    }

    /// Every Godot geometric primitive resolves to its own real geometry; only
    /// unsupported paths fall back to the box placeholder. Regression guard for
    /// the "renders as a box" class of bugs (e.g. the CylinderMesh / TorusMesh
    /// fallthroughs).
    #[test]
    fn primitives_resolve_to_real_geometry_not_box() {
        let cube = mesh_for_primitive("primitive:BoxMesh");
        for name in [
            "SphereMesh",
            "CylinderMesh",
            "CapsuleMesh",
            "PrismMesh",
            "TorusMesh",
        ] {
            let m = mesh_for_primitive(&format!("primitive:{name}"));
            assert_ne!(
                m.vertex_count(),
                cube.vertex_count(),
                "{name} must render as its own geometry, not a box"
            );
        }
        // The torus is a substantial ring, clearly not a box placeholder.
        assert!(mesh_for_primitive("primitive:TorusMesh").vertex_count() > 100);
        // An unknown mesh path (e.g. an .obj we can't load) still falls back.
        assert_eq!(
            mesh_for_primitive("res://model.obj").vertex_count(),
            cube.vertex_count(),
            "unknown mesh paths fall back to the box placeholder"
        );
    }

    /// A `CollisionShape3D` carrying a `shape:<Type>` marker becomes a
    /// `Collision` render item of the matching wireframe kind (drawn as a
    /// wireframe gizmo, not a solid box) — Godot's collision gizmos.
    #[test]
    fn collision_shapes_render_as_wireframe_kinds() {
        gdobject::class_db::register_3d_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let cases = [
            ("BoxShape3D", CollisionShapeKind::Box),
            ("SphereShape3D", CollisionShapeKind::Sphere),
            ("CapsuleShape3D", CollisionShapeKind::Capsule),
            ("CylinderShape3D", CollisionShapeKind::Cylinder),
            ("ConvexPolygonShape3D", CollisionShapeKind::Box), // unknown → box
        ];
        let mut ids = Vec::new();
        for (ty, _) in cases {
            let mut n = Node::new(ty, "CollisionShape3D");
            n.set_property("shape", gdvariant::Variant::String(format!("shape:{ty}")));
            ids.push(tree.add_child(root, n).unwrap().raw());
        }

        // A custom-sized box carries its real extents through the marker.
        let mut sized = Node::new("SizedBox", "CollisionShape3D");
        sized.set_property(
            "shape",
            gdvariant::Variant::String("shape:BoxShape3D|2,4,6".into()),
        );
        let sized_id = tree.add_child(root, sized).unwrap().raw();

        let vp = Viewport3D::new(640, 480);
        let items = vp.build_render_list(&tree);
        for (i, (_, expected)) in cases.iter().enumerate() {
            let it = find(&items, ids[i]).expect("collision render item");
            assert!(
                matches!(it.kind, Render3DKind::Collision { shape, .. } if shape == *expected),
                "case {i}: {:?}",
                it.kind
            );
        }
        // The custom box's half-extents are size/2 = (1, 2, 3).
        let sb = find(&items, sized_id).expect("sized box item");
        match &sb.kind {
            Render3DKind::Collision {
                shape: CollisionShapeKind::Box,
                half_extents,
            } => {
                assert!(
                    (half_extents.x - 1.0).abs() < 1e-4
                        && (half_extents.y - 2.0).abs() < 1e-4
                        && (half_extents.z - 3.0).abs() < 1e-4,
                    "box half-extents from size: {half_extents:?}"
                );
            }
            other => panic!("expected sized box collision, got {other:?}"),
        }
    }

    /// The viewport lights the scene from its `DirectionalLight3D` (local −Z);
    /// with no light it returns `None` (renderer falls back to a default).
    #[test]
    fn scene_light_direction_from_directional_light() {
        gdobject::class_db::register_3d_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        assert!(
            scene_directional_lights(&tree).is_empty(),
            "no light → empty"
        );
        tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
            .unwrap();
        let lights = scene_directional_lights(&tree);
        assert_eq!(lights.len(), 1);
        let dir = lights[0].direction;
        assert!(
            dir.x.abs() < 1e-4 && dir.y.abs() < 1e-4 && (dir.z + 1.0).abs() < 1e-4,
            "an identity DirectionalLight3D shines along -Z: {dir:?}"
        );
        // Defaults when the node carries no light_* properties.
        assert_eq!(lights[0].color, gdcore::math::Color::WHITE);
        assert!((lights[0].energy - 1.0).abs() < 1e-6);
        assert!(lights[0].angular_distance.abs() < 1e-6);
    }

    /// A scene DirectionalLight3D's authored color, energy, and angular size
    /// flow into the render inputs (so its sky sun disc and shading match).
    #[test]
    fn scene_light_reads_color_energy_angular() {
        gdobject::class_db::register_3d_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree
            .add_child(root, Node::new("Sun", "DirectionalLight3D"))
            .unwrap();
        gdscene::node3d::set_light_color(
            &mut tree,
            id,
            gdcore::math::Color::new(1.0, 0.8, 0.5, 1.0),
        );
        gdscene::node3d::set_light_energy(&mut tree, id, 3.0);
        gdscene::node3d::set_light_angular_distance(&mut tree, id, 2.5);
        let lights = scene_directional_lights(&tree);
        assert_eq!(lights.len(), 1);
        assert_eq!(
            lights[0].color,
            gdcore::math::Color::new(1.0, 0.8, 0.5, 1.0)
        );
        assert!((lights[0].energy - 3.0).abs() < 1e-6);
        assert!((lights[0].angular_distance - 2.5).abs() < 1e-6);
        // Default sky_mode (LIGHT_AND_SKY) → contributes to the sky.
        assert!(lights[0].sky, "default sky_mode must draw the sky sun");
    }

    /// Godot's `sky_mode = LIGHT_ONLY` (1) excludes the light from the sky, so
    /// the light shades the scene but draws no sun disc in the procedural /
    /// physical sky.
    #[test]
    fn scene_light_sky_mode_light_only_excludes_from_sky() {
        gdobject::class_db::register_3d_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree
            .add_child(root, Node::new("Sun", "DirectionalLight3D"))
            .unwrap();
        if let Some(n) = tree.get_node_mut(id) {
            n.set_property("sky_mode", gdvariant::Variant::Int(1)); // LIGHT_ONLY
        }
        let lights = scene_directional_lights(&tree);
        assert_eq!(lights.len(), 1);
        assert!(!lights[0].sky, "LIGHT_ONLY must be excluded from the sky");
        assert!(lights[0].affects_scene, "LIGHT_ONLY still shades the scene");
        // SKY_ONLY (2) draws the sky sun but does NOT shade the scene.
        if let Some(n) = tree.get_node_mut(id) {
            n.set_property("sky_mode", gdvariant::Variant::Int(2));
        }
        let sky_only = scene_directional_lights(&tree);
        assert!(sky_only[0].sky, "SKY_ONLY still draws the sky sun");
        assert!(
            !sky_only[0].affects_scene,
            "SKY_ONLY must not contribute to surface shading"
        );
    }

    /// Godot caps sky directional lights at 4; the collector does too.
    #[test]
    fn scene_lights_capped_at_four() {
        gdobject::class_db::register_3d_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        for i in 0..6 {
            tree.add_child(root, Node::new(&format!("Sun{i}"), "DirectionalLight3D"))
                .unwrap();
        }
        assert_eq!(scene_directional_lights(&tree).len(), 4);
    }

    /// 🔑 THE POSITIONAL BUDGET IS THE FRAME'S REMAINING LIGHT SLOTS, NOT A
    /// SECOND INDEPENDENT NUMBER.
    ///
    /// It used to be a hardcoded 8 beside the directional cap of 4, so a scene
    /// could hand over at most 12 lights while the GPU uploads
    /// `array<LightData, 16>` — four slots permanently unused, and the two
    /// numbers free to drift apart with no diagnostic (the renderer truncates
    /// past its array silently).
    ///
    /// ⚠️ Asserted as EXACTLY `MAX_SCENE_LIGHTS`, not "more than 12": the point
    /// is that the frame is now filled, and a plain inequality would also pass
    /// for a budget that overshot the array and started dropping lights.
    #[test]
    fn positional_lights_fill_the_remaining_light_budget() {
        gdobject::class_db::register_3d_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        // Far more of each than the budget allows, so the caps are what decide.
        for i in 0..6 {
            tree.add_child(root, Node::new(&format!("Sun{i}"), "DirectionalLight3D"))
                .unwrap();
        }
        for i in 0..20 {
            tree.add_child(root, Node::new(&format!("Lamp{i}"), "OmniLight3D"))
                .unwrap();
        }

        let lights = scene_lights(&tree);
        assert_eq!(
            lights.len(),
            gdserver3d::light::MAX_SCENE_LIGHTS,
            "the collector must fill the frame's light array, not stop at 12"
        );
        let directional = lights
            .iter()
            .filter(|l| matches!(l.kind, SceneLightKind::Directional))
            .count();
        assert_eq!(directional, 4, "the directional cap is unchanged");
    }

    /// With no directional lights at all, every slot goes to positional ones.
    ///
    /// This is what "derived budget" buys over a fixed 8, and it fails under the
    /// old hardcoded cap — which is why it is a separate test rather than a
    /// second assertion above.
    #[test]
    fn a_scene_without_suns_gives_every_slot_to_positional_lights() {
        gdobject::class_db::register_3d_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        for i in 0..20 {
            tree.add_child(root, Node::new(&format!("Lamp{i}"), "OmniLight3D"))
                .unwrap();
        }
        assert_eq!(
            scene_lights(&tree).len(),
            gdserver3d::light::MAX_SCENE_LIGHTS,
            "with no suns the whole budget is positional; the old fixed 8 gave 8"
        );
    }

    /// A mesh's resolved `albedo_color` flows into its render item, so the
    /// viewport shades it in the material's colour instead of the default grey.
    #[test]
    fn mesh_albedo_flows_into_render_item() {
        gdobject::class_db::register_3d_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("RedBox", "MeshInstance3D");
        n.set_property(
            "mesh",
            gdvariant::Variant::String("primitive:BoxMesh".into()),
        );
        n.set_property(
            "albedo_color",
            gdvariant::Variant::Color(gdcore::math::Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        let id = tree.add_child(root, n).unwrap().raw();

        let vp = Viewport3D::new(640, 480);
        let items = vp.build_render_list(&tree);
        let it = find(&items, id).expect("mesh item");
        let a = it.albedo.expect("albedo present on the render item");
        assert!(
            (a.r - 1.0).abs() < 1e-4 && a.g.abs() < 1e-4 && a.b.abs() < 1e-4,
            "red: {a:?}"
        );
    }

    /// Acceptance (pat-kka71.2): the 3D viewport builds a render list from the
    /// scene's Node3D content — meshes for mesh-bearing nodes, primitive markers
    /// for cameras/lights/empties — skipping hidden and non-spatial nodes.
    #[test]
    fn editor3d_renders_node3d_content() {
        gdobject::class_db::register_3d_classes();
        gdobject::class_db::register_2d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mesh_id = tree
            .add_child(root, Node::new("Crate", "MeshInstance3D"))
            .unwrap();
        let cam_id = tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
        let light_id = tree
            .add_child(root, Node::new("Sun", "DirectionalLight3D"))
            .unwrap();
        let empty_id = tree.add_child(root, Node::new("Pivot", "Node3D")).unwrap();
        let hidden_id = tree
            .add_child(root, Node::new("HiddenMesh", "MeshInstance3D"))
            .unwrap();
        // A non-spatial node must never appear in the 3D render list.
        let sprite2d_id = tree
            .add_child(root, Node::new("Sprite", "Sprite2D"))
            .unwrap();

        node3d::set_position(
            &mut tree,
            mesh_id,
            gdcore::math::Vector3::new(1.0, 2.0, 3.0),
        );
        node3d::set_mesh_path(&mut tree, mesh_id, "res://crate.obj");
        node3d::set_visible(&mut tree, hidden_id, false);

        let vp = Viewport3D::default();
        let items = vp.build_render_list(&tree);
        let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();

        // Hidden and non-spatial nodes are excluded.
        assert!(!ids.contains(&hidden_id.raw()), "hidden node is not drawn");
        assert!(!ids.contains(&sprite2d_id.raw()), "2D node is not drawn");

        // The mesh node renders its mesh, at its global transform.
        let mesh = find(&items, mesh_id.raw()).expect("mesh item present");
        assert_eq!(
            mesh.kind,
            Render3DKind::Mesh {
                path: "res://crate.obj".to_string()
            }
        );
        assert_eq!(
            mesh.global_transform.origin,
            gdcore::math::Vector3::new(1.0, 2.0, 3.0)
        );

        // Camera and light render primitive markers.
        assert_eq!(
            find(&items, cam_id.raw()).unwrap().kind,
            Render3DKind::Primitive {
                shape: PrimitiveShape::Camera
            }
        );
        assert_eq!(
            find(&items, light_id.raw()).unwrap().kind,
            Render3DKind::Primitive {
                shape: PrimitiveShape::Light
            }
        );

        // A plain, mesh-less Node3D renders a generic box marker.
        assert_eq!(
            find(&items, empty_id.raw()).unwrap().kind,
            Render3DKind::Primitive {
                shape: PrimitiveShape::Box
            }
        );
    }
}
