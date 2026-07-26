//! pat-0q2kx.107 acceptance: a `Sprite2D` draws the sub-rect it is configured
//! for, not always its whole texture.
//!
//! Godot samples a sub-rectangle per `sprite_2d.cpp` `_get_rects` — an explicit
//! `region_rect`, or cell `frame` of an `hframes × vframes` sheet — placed by
//! `centered`/`offset`. Patina computed exactly that in
//! `gdscene::sprite_frames::sprite_source_rect` and **no renderer called it**:
//! every sprite drew its full texture, always centred, so a 4-cell sheet showed
//! all four cells squashed into the sprite's place.
//!
//! These build a 4×1 sheet with one distinct colour per cell and assert on the
//! pixels the runtime canvas produces, so "cell 2 only" is checked by what is on
//! screen rather than by the rect arithmetic (which already had unit tests).

use gdcore::math::{Color, Rect2, Vector2};
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::render_runtime_canvas;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// Cell colours of the 4×1 sheet, left to right.
const CELL: [Color; 4] = [
    Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    },
    Color {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    },
    Color {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    },
    Color {
        r: 1.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    },
];

/// A 40×10 sheet of four 10×10 cells, registered in the cache under `path` so
/// the renderer resolves it without touching the filesystem.
fn sheet_texture() -> gdrender2d::texture::Texture2D {
    let (w, h) = (40u32, 10u32);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for _y in 0..h {
        for x in 0..w {
            pixels.push(CELL[(x / 10) as usize]);
        }
    }
    gdrender2d::texture::Texture2D {
        width: w,
        height: h,
        pixels,
    }
}

fn new_tree() -> (SceneTree, NodeId) {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    (tree, root)
}

/// A `Sprite2D` at `pos` using the sheet texture.
fn sprite(tree: &mut SceneTree, root: NodeId, pos: Vector2) -> NodeId {
    let mut node = Node::new("Sprite", "Sprite2D");
    node.set_property("position", Variant::Vector2(pos));
    node.set_property("texture", Variant::String("res://sheet.png".to_string()));
    tree.add_child(root, node).unwrap()
}

fn set(tree: &mut SceneTree, id: NodeId, key: &str, value: Variant) {
    tree.get_node_mut(id).unwrap().set_property(key, value);
}

fn render(tree: &SceneTree) -> gdrender2d::renderer::FrameBuffer {
    let mut cache = TextureCache::new(".");
    cache.insert("res://sheet.png", sheet_texture());
    render_runtime_canvas(tree, W, H, CLEAR, Some(&mut cache))
}

fn pixel(fb: &gdrender2d::renderer::FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

fn approx(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.02 && (a.g - b.g).abs() < 0.02 && (a.b - b.b).abs() < 0.02
}

/// Whether `color` appears anywhere in the frame — used to prove a cell is
/// *absent*, which is the half of "only cell 2" that a positive check misses.
fn frame_contains(fb: &gdrender2d::renderer::FrameBuffer, color: Color) -> bool {
    fb.pixels.iter().any(|p| approx(*p, color))
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core wiring test.** `hframes=4, frame=2` draws only cell 2;
/// `centered=false` moves the draw; `region_rect` crops.
#[test]
fn sprite2d_region_frames_render_wiring() {
    // -- hframes=4, frame=2 shows ONLY cell 2 --------------------------------
    {
        let (mut tree, root) = new_tree();
        let s = sprite(&mut tree, root, Vector2::new(32.0, 32.0));
        set(&mut tree, s, "hframes", Variant::Int(4));
        set(&mut tree, s, "frame", Variant::Int(2));

        let fb = render(&tree);
        // Cell 2 is blue, and a 10x10 cell centred at (32,32) covers (32,32).
        assert!(
            approx(pixel(&fb, 32, 32), CELL[2]),
            "frame=2 of a 4-cell sheet must draw cell 2's colour at the node \
             position; got {:?}",
            pixel(&fb, 32, 32)
        );
        // The decisive half: no other cell may appear anywhere. Drawing the
        // whole texture would put all four on screen and still satisfy a
        // centre-pixel check on its own.
        assert!(
            !frame_contains(&fb, CELL[0]),
            "cell 0's colour must be absent — the sprite must sample only its \
             own cell, not blit the whole sheet"
        );
        assert!(
            !frame_contains(&fb, CELL[3]),
            "cell 3's colour must be absent too"
        );
    }

    // -- centered=false puts the sub-rect's top-left at the node origin ------
    {
        let (mut tree, root) = new_tree();
        let s = sprite(&mut tree, root, Vector2::new(20.0, 20.0));
        set(&mut tree, s, "hframes", Variant::Int(4));
        set(&mut tree, s, "frame", Variant::Int(1));
        set(&mut tree, s, "centered", Variant::Bool(false));

        let fb = render(&tree);
        // Uncentred: the 10x10 cell spans [20,30) from the origin, so (22,22)
        // is inside and (16,16) — which the centred placement would have
        // covered — is not.
        assert!(
            approx(pixel(&fb, 22, 22), CELL[1]),
            "centered=false must place the sub-rect's top-left at the origin; \
             got {:?}",
            pixel(&fb, 22, 22)
        );
        assert!(
            !approx(pixel(&fb, 16, 16), CELL[1]),
            "the pixel a CENTRED sprite would have covered must be empty when \
             centered=false — otherwise the flag did nothing"
        );
    }

    // -- region_enabled + region_rect crops ----------------------------------
    {
        let (mut tree, root) = new_tree();
        let s = sprite(&mut tree, root, Vector2::new(32.0, 32.0));
        set(&mut tree, s, "region_enabled", Variant::Bool(true));
        // The right half of cell 3 only.
        set(
            &mut tree,
            s,
            "region_rect",
            Variant::Rect2(Rect2::new(Vector2::new(35.0, 0.0), Vector2::new(5.0, 10.0))),
        );

        let fb = render(&tree);
        assert!(
            approx(pixel(&fb, 32, 32), CELL[3]),
            "an explicit region must sample the pixels it names; got {:?}",
            pixel(&fb, 32, 32)
        );
        assert!(
            !frame_contains(&fb, CELL[0]),
            "a region crop must exclude the rest of the texture"
        );
    }
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

/// **The regression this change could most easily cause.** A plain sprite — no
/// region, no sheet — must still draw its whole texture, centred, exactly as
/// before. Every default in `SpriteSheet` exists to make this case unchanged.
#[test]
fn a_plain_sprite_still_draws_its_whole_texture_centred() {
    let (mut tree, root) = new_tree();
    sprite(&mut tree, root, Vector2::new(32.0, 32.0));

    let fb = render(&tree);
    // The whole 40x10 sheet centred at (32,32) spans x in [12,52): cell 0's
    // colour lands left of centre and cell 3's to the right.
    assert!(
        frame_contains(&fb, CELL[0]) && frame_contains(&fb, CELL[3]),
        "a sprite with no sheet or region configured must draw the entire \
         texture, as it did before this change"
    );
}

/// Flipping mirrors the *selected cell*, not the whole sheet — so a flipped
/// sheet sprite still shows its own cell's colour rather than a neighbour's.
#[test]
fn flip_keeps_the_selected_cell() {
    let (mut tree, root) = new_tree();
    let s = sprite(&mut tree, root, Vector2::new(32.0, 32.0));
    set(&mut tree, s, "hframes", Variant::Int(4));
    set(&mut tree, s, "frame", Variant::Int(2));
    set(&mut tree, s, "flip_h", Variant::Bool(true));

    let fb = render(&tree);
    assert!(
        approx(pixel(&fb, 32, 32), CELL[2]),
        "flip_h must mirror the chosen cell, not select a different one; got {:?}",
        pixel(&fb, 32, 32)
    );
}
