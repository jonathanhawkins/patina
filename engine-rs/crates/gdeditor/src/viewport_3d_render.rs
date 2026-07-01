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

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender2d::renderer::{Bgra8Image, FrameBuffer, Raster};
use gdrender3d::SoftwareRenderer3D;
use gdscene::SceneTree;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
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
}

impl Viewport3D {
    /// Builds the 3D viewport's render list from `tree`: each **visible**
    /// `Node3D`-derived node becomes a [`Render3DItem`] — a mesh for mesh-bearing
    /// nodes, or a primitive marker otherwise. Hidden nodes are skipped (nothing
    /// to draw), and items appear in scene-tree order.
    pub fn build_render_list(&self, tree: &SceneTree) -> Vec<Render3DItem> {
        self.collect_scene_content(tree)
            .into_iter()
            .filter(|r| r.visible)
            .map(render_item_for)
            .collect()
    }
}

/// Maps a collected spatial node to its draw item.
fn render_item_for(r: Node3DRenderable) -> Render3DItem {
    let kind = if let Some(shape) = r.shape_path.as_deref() {
        let (shape, half_extents) = collision_shape_for(shape);
        Render3DKind::Collision { shape, half_extents }
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
            (CollisionShapeKind::Capsule, Vector3::new(r, n(1, 2.0) * 0.5, r))
        }
        "CylinderShape3D" => {
            let r = n(0, 0.5);
            (CollisionShapeKind::Cylinder, Vector3::new(r, n(1, 2.0) * 0.5, r))
        }
        // BoxShape3D and anything else: dims are the full size, so halve them.
        _ => (
            CollisionShapeKind::Box,
            Vector3::new(n(0, 1.0) * 0.5, n(1, 1.0) * 0.5, n(2, 1.0) * 0.5),
        ),
    }
}

/// Chooses the mesh geometry for a `MeshInstance3D` from its resolved `mesh`
/// path. `primitive:<Type>` markers (produced when the `.tscn` parser resolves an
/// inline mesh sub-resource) map to the matching built-in primitive; anything
/// else (e.g. an `.obj` path we can't load yet) falls back to a box placeholder.
fn mesh_for_primitive(path: &str) -> Mesh3D {
    match path.strip_prefix("primitive:").unwrap_or("") {
        "SphereMesh" => Mesh3D::sphere(0.45, 18),
        "BoxMesh" | "CubeMesh" => Mesh3D::cube(0.8),
        "CylinderMesh" => Mesh3D::cylinder(0.4, 0.4, 0.9, 18, 1),
        "PrismMesh" => Mesh3D::cylinder(0.0, 0.45, 0.9, 18, 1),
        "PlaneMesh" | "QuadMesh" => Mesh3D::plane(0.9),
        "CapsuleMesh" => Mesh3D::capsule(0.32, 1.0, 14, 8),
        "TorusMesh" => Mesh3D::torus(0.22, 0.45, 24, 12),
        _ => Mesh3D::cube(0.7),
    }
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

/// The world-space direction the scene's first `DirectionalLight3D` shines (its
/// local −Z), normalized — so the viewport lights the scene like Godot does.
/// `None` when there is no directional light (the renderer uses its default).
fn scene_directional_light_dir(tree: &SceneTree) -> Option<Vector3> {
    tree.all_nodes_in_tree_order().into_iter().find_map(|nid| {
        let node = tree.get_node(nid)?;
        if !node.is_class("DirectionalLight3D") {
            return None;
        }
        let z = gdscene::node3d::get_global_transform(tree, nid).basis.z;
        let dir = Vector3::new(-z.x, -z.y, -z.z);
        let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
        (len > 1e-6).then(|| Vector3::new(dir.x / len, dir.y / len, dir.z / len))
    })
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
const GRID_MINOR_COLOR_3D: Color = Color { r: 0.55, g: 0.58, b: 0.65, a: 1.0 };
/// Major grid line color (every 10 units) — brighter so the scale reads.
const GRID_MAJOR_COLOR_3D: Color = Color { r: 0.66, g: 0.69, b: 0.77, a: 1.0 };
/// Origin/gizmo axis colors — Godot convention: X = red, Y = green, Z = blue.
const X_AXIS_COLOR_3D: Color = Color { r: 0.86, g: 0.32, b: 0.34, a: 1.0 };
const Y_AXIS_COLOR_3D: Color = Color { r: 0.55, g: 0.78, b: 0.30, a: 1.0 };
const Z_AXIS_COLOR_3D: Color = Color { r: 0.33, g: 0.55, b: 0.88, a: 1.0 };
/// The uniform viewport backdrop behind the 3D scene (Godot's dark editor gray).
const VIEWPORT_BG_3D: Color = Color { r: 0.13, g: 0.14, b: 0.17, a: 1.0 };
/// Selection outline color drawn around the picked node (Godot's selection orange).
const SELECTION_COLOR_3D: Color = Color { r: 1.0, g: 0.62, b: 0.20, a: 1.0 };
const MESH_COLOR_3D: Color = Color { r: 0.78, g: 0.80, b: 0.85, a: 1.0 };
const CAMERA_COLOR_3D: Color = Color { r: 0.45, g: 0.75, b: 1.0, a: 1.0 };
const LIGHT_COLOR_3D: Color = Color { r: 1.0, g: 0.85, b: 0.35, a: 1.0 };
const BOX_COLOR_3D: Color = Color { r: 0.90, g: 0.62, b: 0.20, a: 1.0 };
/// Collision-shape wireframe colour (Godot's teal collision gizmos).
const COLLISION_COLOR_3D: Color = Color { r: 0.10, g: 0.75, b: 0.78, a: 1.0 };

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
            ((v.x * inv_z) / (half_fov * aspect), (v.y * inv_z) / half_fov)
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
    draw_aa_line_fade(fb, x0, y0, 1.0 / va.z, x1, y1, 1.0 / vb.z, color, base_alpha, fade_dist);
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
    draw_aa_line_fade(fb, x0, y0, 1.0 / va.z, x1, y1, 1.0 / vb.z, color, alpha, 1.0e9);
}

/// The 12 edges of a cube, as index pairs into its 8 corners (corner `i` has
/// component `j` set to the high value when bit `j` of `i` is set).
const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1), (2, 3), (4, 5), (6, 7), // along X
    (0, 2), (1, 3), (4, 6), (5, 7), // along Y
    (0, 4), (1, 5), (2, 6), (3, 7), // along Z
];

/// Draws a wireframe box (12 edges) of half-size `h` at `transform`, used as the
/// selection outline around the picked node — like Godot's selection box.
fn draw_selection_box(fb: &mut impl Raster, viewport: &Viewport3D, transform: &Transform3D, h: f32) {
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
        draw_edge(fb, viewport, &view, c[i], c[(i + 1) % 4], CAMERA_COLOR_3D, 0.95);
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
                *slot =
                    xform_point(tf, Vector3::new(sx[i & 1], sy[(i >> 1) & 1], sz[(i >> 2) & 1]));
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
        (Vector3::new(pf, pf, 0.0), Vector3::new(s, 0.0, 0.0), Vector3::new(0.0, s, 0.0), Z_AXIS_COLOR_3D),
        (Vector3::new(pf, 0.0, pf), Vector3::new(s, 0.0, 0.0), Vector3::new(0.0, 0.0, s), Y_AXIS_COLOR_3D),
        (Vector3::new(0.0, pf, pf), Vector3::new(0.0, s, 0.0), Vector3::new(0.0, 0.0, s), X_AXIS_COLOR_3D),
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
            draw_edge(fb, viewport, &view, corners[i], corners[(i + 1) % 4], color, 0.85);
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
        (X_AXIS_COLOR_3D, |a, r| Vector3::new(0.0, r * a.cos(), r * a.sin())),
        (Y_AXIS_COLOR_3D, |a, r| Vector3::new(r * a.cos(), 0.0, r * a.sin())),
        (Z_AXIS_COLOR_3D, |a, r| Vector3::new(r * a.cos(), r * a.sin(), 0.0)),
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
}

/// Populates `renderer` with one solid instance per drawable spatial node (mesh
/// or primitive marker) at its global transform, and returns the overlay geometry
/// (selection outlines + node/collision gizmos) to draw over the frame afterward.
///
/// Written against `&mut dyn RenderingServer3D` so the software and GPU editor
/// paths share the exact same scene-building logic — only the concrete rasterizer
/// differs. Collision shapes are wireframe-only (recorded, not instanced).
fn populate_editor_scene_3d(
    renderer: &mut dyn RenderingServer3D,
    viewport: &Viewport3D,
    tree: &SceneTree,
) -> EditorOverlays3D {
    let mut selected_tfs: Vec<Transform3D> = Vec::new();
    let mut primary_tf: Option<Transform3D> = None;
    let mut node_gizmos: Vec<(Transform3D, PrimitiveShape)> = Vec::new();
    let mut collision_gizmos: Vec<(Transform3D, CollisionShapeKind, Vector3)> = Vec::new();
    for item in viewport.build_render_list(tree) {
        // Collision shapes are wireframe-only; record selection and skip the
        // solid mesh instance.
        if let Render3DKind::Collision { shape, half_extents } = &item.kind {
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
            Render3DKind::Mesh { path } => {
                (mesh_for_primitive(path), item.albedo.unwrap_or(MESH_COLOR_3D))
            }
            Render3DKind::Collision { .. } => unreachable!("handled above"),
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
        renderer.set_material(id, solid_material_3d(color));
        renderer.set_transform(id, item.global_transform);
        renderer.set_visible(id, true);
    }
    EditorOverlays3D { selected_tfs, primary_tf, node_gizmos, collision_gizmos }
}

/// Adds the editor's key light to `renderer`: the scene's first
/// `DirectionalLight3D` (its local −Z) like Godot, or the preview sun otherwise.
/// Godot's "Preview Sun" toggle dims the sun to a flat fill when off. Shared by
/// both editor render paths.
fn setup_editor_lighting_3d(
    renderer: &mut dyn RenderingServer3D,
    viewport: &Viewport3D,
    tree: &SceneTree,
) {
    let light_id = Light3DId(9_000_001);
    renderer.add_light(light_id);
    let mut light = Light3D::directional(light_id);
    if let Some(dir) = scene_directional_light_dir(tree) {
        light.direction = dir;
    } else {
        // No scene light: shine the preview sun from its configured direction
        // (Godot's Sun and Environment settings), so editing it is visible.
        light.direction = viewport.environment.sun_direction;
    }
    light.energy = if viewport.environment.sun_enabled {
        viewport.environment.sun_energy.max(0.0)
    } else {
        0.18
    };
    renderer.update_light(&light);
}

/// Builds the `RenderViewport3D` (editor camera + clip planes) at `width`×
/// `height`. The background/environment is applied by the caller: the software
/// path sets a Sky/ClearColor env on the returned viewport; the GPU path clears
/// the target to a color instead.
fn editor_render_viewport(viewport: &Viewport3D, width: u32, height: u32) -> RenderViewport3D {
    let mut rv = RenderViewport3D::new(width, height);
    rv.camera_transform = viewport.camera.transform();
    rv.fov = viewport.camera.fov_radians();
    rv.near = 0.05;
    rv.far = 2000.0;
    rv
}

/// Draws all editor overlays over a rasterized mesh frame `fb`: the ground grid +
/// colored origin axes, per-node gizmos (camera frustums / light rays), collision
/// wireframes, selection outlines, and the transform gizmo on the primary
/// selection. Shared by both editor render paths so the chrome is identical.
fn draw_editor_overlays_3d(fb: &mut impl Raster, viewport: &Viewport3D, overlays: &EditorOverlays3D) {
    // --- Ground grid + origin axes, projected as crisp anti-aliased lines. ---
    draw_grid_overlay(fb, viewport);

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
    if let Some(tf) = overlays.primary_tf.or_else(|| overlays.selected_tfs.first().copied()) {
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
    let width = viewport.width.max(1);
    let height = viewport.height.max(1);
    let mut renderer = SoftwareRenderer3D::new();

    let overlays = populate_editor_scene_3d(&mut renderer, viewport, tree);
    setup_editor_lighting_3d(&mut renderer, viewport, tree);

    // --- Render from the editor camera. Godot's "Preview Environment" toolbar
    //     toggle shows a procedural sky gradient (bluish top → light horizon →
    //     ground) when on, and a flat editor backdrop when off. ---
    let mut rv = editor_render_viewport(viewport, width, height);
    let mut env = gdserver3d::environment::Environment3D::default();
    if viewport.environment.enabled && viewport.environment.sky_visible {
        env.background_mode = gdserver3d::environment::BackgroundMode::Sky;
        env.sky = Some(gdserver3d::sky::Sky::default());
    } else {
        // Flat backdrop, matching the uniform viewport void color.
        env.background_mode = gdserver3d::environment::BackgroundMode::ClearColor;
        env.background_color = VIEWPORT_BG_3D;
    }
    rv.environment = Some(env);
    let frame = renderer.render_frame(&rv);

    // --- FrameData3D -> editor FrameBuffer (both are row-major RGBA Color). ---
    let mut fb = FrameBuffer::new(width, height, VIEWPORT_BG_3D);
    let count = frame.pixels.len().min(fb.pixels.len());
    fb.pixels[..count].copy_from_slice(&frame.pixels[..count]);

    draw_editor_overlays_3d(&mut fb, viewport, &overlays);
    fb
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
pub fn render_editor_3d_frame_gpu(viewport: &Viewport3D, tree: &SceneTree) -> Bgra8Image {
    use gdrender3d::wgpu_pipeline::GpuRenderer3D;
    use std::cell::RefCell;

    // Per-thread persistent GPU renderer. `Failed` means adapter init returned
    // None (no GPU) → use the software path forever.
    enum GpuSlot {
        Uninit,
        Failed,
        Ready(GpuRenderer3D),
    }
    thread_local! {
        static GPU: RefCell<GpuSlot> = RefCell::new(GpuSlot::Uninit);
    }

    let width = viewport.width.max(1);
    let height = viewport.height.max(1);

    GPU.with(|cell| {
        // Lazy init on the first call for this thread.
        {
            let mut slot = cell.borrow_mut();
            if matches!(&*slot, GpuSlot::Uninit) {
                *slot = match GpuRenderer3D::new() {
                    Some(r) => GpuSlot::Ready(r),
                    None => GpuSlot::Failed,
                };
            }
        }
        let mut slot = cell.borrow_mut();
        let renderer = match &mut *slot {
            GpuSlot::Ready(r) => r,
            // No GPU adapter → software reference path, converted to bytes (the
            // software path doesn't touch this thread-local, so the borrow is fine).
            _ => return framebuffer_to_bgra(&render_editor_3d_frame(viewport, tree)),
        };

        // The persistent renderer accumulates instances/lights across calls, so
        // reset it before rebuilding this frame's scene. Reborrow (`&mut *`) for
        // the shared `&mut dyn RenderingServer3D` helpers so `renderer` stays
        // usable for the render call below.
        renderer.reset_scene();
        let overlays = populate_editor_scene_3d(&mut *renderer, viewport, tree);
        setup_editor_lighting_3d(&mut *renderer, viewport, tree);
        let rv = editor_render_viewport(viewport, width, height);
        // Clear to the flat editor backdrop (matching the software ClearColor
        // path); the linear `Bgra8Unorm` target keeps mesh bytes consistent with
        // the software overlays drawn next.
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
pub fn render_editor_3d_frame_gpu(viewport: &Viewport3D, tree: &SceneTree) -> Bgra8Image {
    framebuffer_to_bgra(&render_editor_3d_frame(viewport, tree))
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

    /// Every Godot geometric primitive resolves to its own real geometry; only
    /// unsupported paths fall back to the box placeholder. Regression guard for
    /// the "renders as a box" class of bugs (e.g. the CylinderMesh / TorusMesh
    /// fallthroughs).
    #[test]
    fn primitives_resolve_to_real_geometry_not_box() {
        let cube = mesh_for_primitive("primitive:BoxMesh");
        for name in ["SphereMesh", "CylinderMesh", "CapsuleMesh", "PrismMesh", "TorusMesh"] {
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
        sized.set_property("shape", gdvariant::Variant::String("shape:BoxShape3D|2,4,6".into()));
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
            Render3DKind::Collision { shape: CollisionShapeKind::Box, half_extents } => {
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
        assert!(scene_directional_light_dir(&tree).is_none(), "no light → None");
        tree.add_child(root, Node::new("Sun", "DirectionalLight3D")).unwrap();
        let dir = scene_directional_light_dir(&tree).expect("light direction");
        assert!(
            dir.x.abs() < 1e-4 && dir.y.abs() < 1e-4 && (dir.z + 1.0).abs() < 1e-4,
            "an identity DirectionalLight3D shines along -Z: {dir:?}"
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
        n.set_property("mesh", gdvariant::Variant::String("primitive:BoxMesh".into()));
        n.set_property(
            "albedo_color",
            gdvariant::Variant::Color(gdcore::math::Color::new(1.0, 0.0, 0.0, 1.0)),
        );
        let id = tree.add_child(root, n).unwrap().raw();

        let vp = Viewport3D::new(640, 480);
        let items = vp.build_render_list(&tree);
        let it = find(&items, id).expect("mesh item");
        let a = it.albedo.expect("albedo present on the render item");
        assert!((a.r - 1.0).abs() < 1e-4 && a.g.abs() < 1e-4 && a.b.abs() < 1e-4, "red: {a:?}");
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

        node3d::set_position(&mut tree, mesh_id, gdcore::math::Vector3::new(1.0, 2.0, 3.0));
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
        assert_eq!(mesh.kind, Render3DKind::Mesh { path: "res://crate.obj".to_string() });
        assert_eq!(mesh.global_transform.origin, gdcore::math::Vector3::new(1.0, 2.0, 3.0));

        // Camera and light render primitive markers.
        assert_eq!(
            find(&items, cam_id.raw()).unwrap().kind,
            Render3DKind::Primitive { shape: PrimitiveShape::Camera }
        );
        assert_eq!(
            find(&items, light_id.raw()).unwrap().kind,
            Render3DKind::Primitive { shape: PrimitiveShape::Light }
        );

        // A plain, mesh-less Node3D renders a generic box marker.
        assert_eq!(
            find(&items, empty_id.raw()).unwrap().kind,
            Render3DKind::Primitive { shape: PrimitiveShape::Box }
        );
    }
}
