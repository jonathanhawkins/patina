//! Scene → canvas render bridge (pat-nzgdu.16).
//!
//! Godot draws every `CanvasItem`-derived node each frame through the
//! `RenderingServer`, honoring the node's **global** [`Transform2D`] (composed
//! up the parent chain), accumulated `z_index` (with `z_as_relative`),
//! per-parent `y_sort`, and `modulate`/`self_modulate` propagation. Patina had
//! all of the *ordering/inheritance math* ([`crate::canvas_order`]) and a
//! [`SoftwareRenderer`] canvas pipeline, but nothing connected a live
//! [`SceneTree`] to a [`Viewport`] of [`CanvasItem`]s — the only 2D producers of
//! [`DrawCommand`]s were unit tests.
//!
//! This module is that missing bridge. It walks the tree in draw order (using
//! [`crate::canvas_order::draw_ordered_children`] so per-parent Y-sort is
//! honored), and for each visible drawable `CanvasItem` node it builds a
//! [`CanvasItem`] whose:
//!
//! - `transform` is the node's global [`Transform2D`]
//!   ([`crate::node2d::get_global_transform`]) — so a sprite under a rotated
//!   parent rasterizes at the globally rotated/translated location, not its
//!   local position;
//! - `z_index` is the accumulated effective z
//!   ([`crate::canvas_order::effective_z_index`], respecting `z_as_relative`);
//! - draw commands carry the node's effective modulate
//!   ([`crate::canvas_order::effective_modulate`] = inherited `modulate` ×
//!   `self_modulate`).
//!
//! The [`Viewport`] then stable-sorts unlayered items by `z_index`, so items are
//! painted back-to-front: lower z first, and within equal z the draw-order walk
//! (Y-sort then tree order) is preserved. Feeding the viewport to
//! [`SoftwareRenderer::render_frame`] yields the final [`FrameData`] pixels.
//!
//! Supported drawable classes: `Sprite2D`/`AnimatedSprite2D`, `ColorRect`,
//! `TextureRect`. Full Control-layout (anchors/offsets, styleboxes, text) is
//! handled by [`crate::control_render`]; polygon/line fills that have no single
//! [`DrawCommand`] representation are left to the editor's direct rasterizer.

use gdcore::math::{Color, Rect2, Vector2};
use gdvariant::Variant;

use gdrender2d::SoftwareRenderer;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::server::{FrameData, RenderingServer2D};
use gdserver2d::viewport::Viewport;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;
use crate::{canvas_order, control, node2d};

/// Resolves a texture's pixel size (width, height) given the texture path stored
/// on a `Sprite2D`/`TextureRect` node. Sizing lives outside the bridge because
/// the actual pixels are owned by the renderer / resource layer: the caller
/// passes a lookup backed by whatever textures it has registered. Return `None`
/// to skip drawing a sprite whose size is unknown.
pub type TextureSizeFn<'a> = dyn Fn(&str) -> Option<Vector2> + 'a;

/// Builds a [`Viewport`] of [`CanvasItem`]s from every visible drawable node in
/// `tree`, honoring global transforms, effective z-order, Y-sort, and modulate.
///
/// The returned viewport is renderer-agnostic; feed it to any
/// [`RenderingServer2D`]. See [`render_scene_to_frame`] for the one-shot path.
pub fn build_canvas_viewport(
    tree: &SceneTree,
    width: u32,
    height: u32,
    clear_color: Color,
    texture_size: &TextureSizeFn<'_>,
) -> Viewport {
    let mut viewport = Viewport::new(width, height, clear_color);
    let mut next_id: u64 = 1;
    visit(
        tree,
        tree.root_id(),
        &mut viewport,
        &mut next_id,
        texture_size,
    );
    viewport
}

/// Convenience: build the scene's [`Viewport`] and rasterize it through
/// `renderer`, returning the frame pixels. Proves the wiring end-to-end from a
/// [`SceneTree`] (e.g. instanced from `PackedScene::from_tscn`) to [`FrameData`].
pub fn render_scene_to_frame(
    tree: &SceneTree,
    renderer: &mut SoftwareRenderer,
    width: u32,
    height: u32,
    clear_color: Color,
    texture_size: &TextureSizeFn<'_>,
) -> FrameData {
    let viewport = build_canvas_viewport(tree, width, height, clear_color, texture_size);
    renderer.render_frame(&viewport)
}

/// Pre-order draw-walk. A node whose own `visible` flag is false hides itself
/// and its whole subtree (Godot CanvasItem semantics), so we short-circuit.
/// Children are visited in draw order (Y-sort aware) so items with equal
/// effective z paint in the correct order under the viewport's stable z-sort.
fn visit(
    tree: &SceneTree,
    node_id: NodeId,
    viewport: &mut Viewport,
    next_id: &mut u64,
    texture_size: &TextureSizeFn<'_>,
) {
    if !node2d::is_visible(tree, node_id) {
        return;
    }

    let commands = draw_commands_for(tree, node_id, texture_size);
    if !commands.is_empty() {
        let mut item = CanvasItem::new(CanvasItemId(*next_id));
        *next_id += 1;
        item.transform = node2d::get_global_transform(tree, node_id);
        item.z_index = canvas_order::effective_z_index(tree, node_id);
        item.commands = commands;
        viewport.add_canvas_item(item);
    }

    for child in canvas_order::draw_ordered_children(tree, node_id) {
        visit(tree, child, viewport, next_id, texture_size);
    }
}

/// Emits the draw commands for a single node based on its class. Returns an
/// empty vec for non-drawable (or unsupported) classes; those still recurse so
/// their drawable descendants are rendered.
fn draw_commands_for(
    tree: &SceneTree,
    node_id: NodeId,
    texture_size: &TextureSizeFn<'_>,
) -> Vec<DrawCommand> {
    let Some(class) = tree.get_node(node_id).map(|n| n.class_name().to_owned()) else {
        return Vec::new();
    };
    let modulate = canvas_order::effective_modulate(tree, node_id);
    match class.as_str() {
        "Sprite2D" | "AnimatedSprite2D" => sprite_commands(tree, node_id, modulate, texture_size),
        "ColorRect" => color_rect_commands(tree, node_id, modulate),
        "TextureRect" => texture_rect_commands(tree, node_id, modulate, texture_size),
        "GPUParticles2D" | "CPUParticles2D" => particle_commands(tree, node_id, modulate),
        _ => Vec::new(),
    }
}

/// `Sprite2D`: a textured rect placed at the sprite origin. Godot centers the
/// texture on the origin by default (`centered = true`) and applies `offset`.
/// The rect is in the node's local space; the item's global transform (parent
/// rotation/scale included) positions it.
fn sprite_commands(
    tree: &SceneTree,
    node_id: NodeId,
    modulate: Color,
    texture_size: &TextureSizeFn<'_>,
) -> Vec<DrawCommand> {
    let Some(path) = node2d::get_texture_path(tree, node_id) else {
        return Vec::new();
    };
    let Some(size) = texture_size(&path) else {
        return Vec::new();
    };
    let centered = read_bool(tree, node_id, "centered", true);
    let offset = read_vec2(tree, node_id, "offset");
    let top_left = if centered {
        Vector2::new(offset.x - size.x / 2.0, offset.y - size.y / 2.0)
    } else {
        offset
    };
    vec![DrawCommand::DrawTextureRect {
        texture_path: path,
        rect: Rect2::new(top_left, size),
        modulate,
    }]
}

/// `ColorRect`: a filled rect from the control origin spanning its size, tinted
/// by the effective modulate.
fn color_rect_commands(tree: &SceneTree, node_id: NodeId, modulate: Color) -> Vec<DrawCommand> {
    let size = control::get_size(tree, node_id);
    if size.x <= 0.0 || size.y <= 0.0 {
        return Vec::new();
    }
    let color = read_color(tree, node_id, "color", Color::WHITE);
    vec![DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::ZERO, size),
        color: mul_color(color, modulate),
        filled: true,
    }]
}

/// `TextureRect`: a textured rect spanning the control's size (falling back to
/// the texture's native size when the control size is unset).
fn texture_rect_commands(
    tree: &SceneTree,
    node_id: NodeId,
    modulate: Color,
    texture_size: &TextureSizeFn<'_>,
) -> Vec<DrawCommand> {
    let Some(path) = read_string(tree, node_id, "texture") else {
        return Vec::new();
    };
    let mut size = control::get_size(tree, node_id);
    if size.x <= 0.0 || size.y <= 0.0 {
        match texture_size(&path) {
            Some(ts) => size = ts,
            None => return Vec::new(),
        }
    }
    vec![DrawCommand::DrawTextureRect {
        texture_path: path,
        rect: Rect2::new(Vector2::ZERO, size),
        modulate,
    }]
}

/// Base edge length (px) of the solid billboard quad drawn for each live
/// particle. A particle's `scale` multiplies this.
const PARTICLE_QUAD_SIZE: f32 = 4.0;

/// `GPUParticles2D`/`CPUParticles2D`: one small billboard quad per live particle.
///
/// The live particle set is produced by the `MainLoop` particle phase
/// ([`crate::main_loop::MainLoop::process_particles`]) and published back onto
/// the node each frame as parallel `_particle_positions` (`PackedVector2Array`,
/// emitter-local space), `_particle_colors` (`PackedColorArray`), and
/// `_particle_scales` (`PackedFloat32Array`). Each particle becomes a solid
/// [`DrawCommand::DrawRect`] centered on its position, sized `PARTICLE_QUAD_SIZE
/// × scale` and tinted by the particle color × the node's effective `modulate`.
/// The commands are in the node's local space; the caller applies the emitter's
/// global [`Transform2D`], so the whole cloud is positioned (and, for
/// `local_coords`, moves) with the emitter.
fn particle_commands(tree: &SceneTree, node_id: NodeId, modulate: Color) -> Vec<DrawCommand> {
    let Some(node) = tree.get_node(node_id) else {
        return Vec::new();
    };
    let positions = match node.get_property("_particle_positions") {
        Variant::PackedVector2Array(v) => v,
        _ => return Vec::new(),
    };
    if positions.is_empty() {
        return Vec::new();
    }
    let colors = match node.get_property("_particle_colors") {
        Variant::PackedColorArray(v) => v,
        _ => Vec::new(),
    };
    let scales = match node.get_property("_particle_scales") {
        Variant::PackedFloat32Array(v) => v,
        _ => Vec::new(),
    };
    let mut cmds = Vec::with_capacity(positions.len());
    for (i, pos) in positions.iter().enumerate() {
        let scale = scales.get(i).copied().unwrap_or(1.0);
        let size = PARTICLE_QUAD_SIZE * if scale > 0.0 { scale } else { 1.0 };
        let half = size / 2.0;
        let color = colors.get(i).copied().unwrap_or(Color::WHITE);
        cmds.push(DrawCommand::DrawRect {
            rect: Rect2::new(
                Vector2::new(pos.x - half, pos.y - half),
                Vector2::new(size, size),
            ),
            color: mul_color(color, modulate),
            filled: true,
        });
    }
    cmds
}

// ---------------------------------------------------------------------------
// Small typed property readers (no ClassDB dependency — safe in bare unit tests
// where the inheritance chain is not populated).
// ---------------------------------------------------------------------------

fn read_bool(tree: &SceneTree, node_id: NodeId, key: &str, default: bool) -> bool {
    match tree.get_node(node_id).map(|n| n.get_property(key)) {
        Some(Variant::Bool(b)) => b,
        _ => default,
    }
}

fn read_vec2(tree: &SceneTree, node_id: NodeId, key: &str) -> Vector2 {
    match tree.get_node(node_id).map(|n| n.get_property(key)) {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

fn read_color(tree: &SceneTree, node_id: NodeId, key: &str, default: Color) -> Color {
    match tree.get_node(node_id).map(|n| n.get_property(key)) {
        Some(Variant::Color(c)) => c,
        _ => default,
    }
}

fn read_string(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<String> {
    match tree.get_node(node_id).map(|n| n.get_property(key)) {
        Some(Variant::String(s)) => Some(s),
        _ => None,
    }
}

fn mul_color(a: Color, b: Color) -> Color {
    Color::new(a.r * b.r, a.g * b.g, a.b * b.b, a.a * b.a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packed_scene::{add_packed_scene_to_tree, PackedScene};
    use gdrender2d::Texture2D;

    fn find_by_name(tree: &SceneTree, name: &str) -> Option<NodeId> {
        tree.all_nodes_in_tree_order()
            .into_iter()
            .find(|&id| tree.get_node(id).map(|n| n.name() == name).unwrap_or(false))
    }

    fn is_greenish(c: Color) -> bool {
        c.g > 0.5 && c.r < 0.5 && c.b < 0.5
    }

    /// End-to-end acceptance (pat-nzgdu.16): a `.tscn` whose root `Node2D` is
    /// rotated 45° and translated, containing a `Sprite2D` child plus two
    /// overlapping `ColorRect`s with `z_index` 0 and 1, must render (a) the
    /// sprite pixels at the globally rotated+translated location (not the local
    /// position) and (b) the `z_index=1` rect on top of the `z_index=0` rect —
    /// proving the wiring from `PackedScene::from_tscn` to `FrameData` pixels.
    #[test]
    fn scene_canvas_render_bridge() {
        // Root at (128,128) rotated 45° (0.7853982 rad). Sprite child at local
        // (60,0) → global center ≈ (128+60·cos45, 128+60·sin45) ≈ (170,170).
        // Both rects are children at local (-20,-20) size 40 → both map to a
        // 40×40 square centered on the root origin (128,128); the z=1 (blue)
        // rect must paint over the z=0 (red) rect.
        let tscn = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1_tex"]

[node name="Root" type="Node2D"]
position = Vector2(128, 128)
rotation = 0.7853982

[node name="Sprite" type="Sprite2D" parent="."]
position = Vector2(60, 0)
texture = ExtResource("1_tex")

[node name="RectBack" type="ColorRect" parent="."]
position = Vector2(-20, -20)
size = Vector2(40, 40)
color = Color(1, 0, 0, 1)
z_index = 0

[node name="RectFront" type="ColorRect" parent="."]
position = Vector2(-20, -20)
size = Vector2(40, 40)
color = Color(0, 0, 1, 1)
z_index = 1
"#;

        let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
        let mut tree = SceneTree::new();
        let tree_root = tree.root_id();
        let root = add_packed_scene_to_tree(&mut tree, tree_root, &scene).expect("instance");

        // The parser stores the sprite's texture as the raw `ExtResource(..)`
        // reference; register a solid-green texture under that exact key so the
        // renderer resolves it — no hardcoding of the parser's ref format.
        let sprite_id = find_by_name(&tree, "Sprite").expect("sprite node");
        assert_eq!(
            tree.get_node(sprite_id).map(|n| n.parent()),
            Some(Some(root))
        );
        let tex_ref = node2d::get_texture_path(&tree, sprite_id).expect("sprite texture ref");

        let size_of = |_p: &str| -> Option<Vector2> { Some(Vector2::new(24.0, 24.0)) };
        let mut renderer = SoftwareRenderer::new();
        renderer.register_texture(
            &tex_ref,
            Texture2D::solid(24, 24, Color::new(0.0, 1.0, 0.0, 1.0)),
        );

        let frame = render_scene_to_frame(&tree, &mut renderer, 256, 256, Color::BLACK, &size_of);
        assert_eq!(frame.width, 256);
        assert_eq!(frame.height, 256);
        assert_eq!(frame.pixels.len(), 256 * 256);

        let w = frame.width as usize;
        let px = |x: usize, y: usize| frame.pixels[y * w + x];

        // (a) Sprite renders at the global (rotated+translated) location…
        let at_global = px(170, 170);
        assert!(
            is_greenish(at_global),
            "sprite must render at the rotated+translated global location (~170,170), got {at_global:?}"
        );
        // …and NOT at its untransformed local position — if the bridge ignored
        // the parent transform the sprite would sit near local (60,0).
        let at_local = px(60, 8);
        assert!(
            !is_greenish(at_local),
            "sprite must not render at its local position (global transform must be applied), got {at_local:?}"
        );

        // (b) z-order: the fully-overlapping z=1 (blue) rect paints over z=0 (red).
        let overlap = px(128, 128);
        assert!(
            overlap.b > 0.5 && overlap.r < 0.5,
            "z_index=1 (blue) rect must be on top of z_index=0 (red) rect at (128,128), got {overlap:?}"
        );
    }

    /// Toggling a node's `visible` flag off must remove its pixels from the
    /// frame (whole-subtree visibility short-circuit).
    #[test]
    fn hidden_node_is_not_drawn() {
        let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Rect" type="ColorRect" parent="."]
position = Vector2(50, 50)
size = Vector2(20, 20)
color = Color(1, 0, 0, 1)
"#;
        let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
        let mut tree = SceneTree::new();
        let tree_root = tree.root_id();
        add_packed_scene_to_tree(&mut tree, tree_root, &scene).expect("instance");

        let size_of = |_p: &str| -> Option<Vector2> { None };
        let mut renderer = SoftwareRenderer::new();

        let w = 128usize;
        let visible = render_scene_to_frame(&tree, &mut renderer, 128, 128, Color::BLACK, &size_of);
        assert!(
            visible.pixels[60 * w + 60].r > 0.5,
            "rect should be visible before hiding"
        );

        let rect_id = find_by_name(&tree, "Rect").expect("rect node");
        node2d::set_visible(&mut tree, rect_id, false);
        let hidden = render_scene_to_frame(&tree, &mut renderer, 128, 128, Color::BLACK, &size_of);
        assert!(
            hidden.pixels[60 * w + 60].r < 0.5,
            "hidden rect must not be drawn"
        );
    }
}
