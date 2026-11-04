//! The web editor drew every Play frame while holding the editor-state lock,
//! and the game loop needs that lock to step. On the platformer demo a
//! 995×843 frame took 12–17 ms to rasterize, so the game thread was blocked a
//! third of every tick: the browser showed 23–47 frames/s, `diagnose` said
//! "game-bound: 53 of 60 frames stepped", and the audio device ran dry.
//!
//! The fix splits `render_play_frame` into `prepare_play_frame` (everything
//! that reads the tree, under the lock) and `PreparedPlayFrame::rasterize`
//! (the raster, off it), with the texture cache lent out meanwhile. These pin:
//!
//! 1. the split draws exactly what the one-piece tree pipeline drew, on a scene
//!    that exercises every tree read the raster used to make (lights,
//!    occluders, per-item light masks, CanvasModulate, Controls);
//! 2. a prepared frame is a snapshot: changing the tree after preparing it does
//!    not change the frame — which is what makes releasing the lock sound;
//! 3. a texture cache lent out for the raster loses nothing the editor did to
//!    the stand-in meanwhile (invalidate, clear, re-root, loads).

use gdcore::math::{Color, Vector2};
use gdcore::project_settings::ProjectSettings;
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{
    apply_camera2d_view, apply_stretch_transform, build_control_render_items_sourced,
    build_runtime_render_list, camera2d_view_transform, canvas_size_of, merge_control_items,
    paint_black_bars, prepare_play_frame, render_canvas_from_list, render_play_frame, CanvasQuality,
};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdvariant::Variant;

const W: u32 = 96;
const H: u32 = 80;

/// A dimmed, lit, occluded scene with a Control: every tree read the old
/// one-piece pipeline made during the raster has something to find.
const LIT_SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Dim" type="CanvasModulate" parent="."]
color = Color(0.6, 0.7, 0.8, 1)

[node name="Rect" type="ColorRect" parent="."]
position = Vector2(0, 0)
size = Vector2(60, 60)
color = Color(1, 1, 1, 1)

[node name="Box" type="Polygon2D" parent="."]
position = Vector2(62, 10)
color = Color(0.2, 0.9, 0.3, 1)
polygon = PackedVector2Array(0, 0, 30, 0, 30, 30, 0, 30)
light_mask = 2

[node name="Lamp" type="PointLight2D" parent="."]
position = Vector2(30, 20)
color = Color(1, 0, 0, 1)
energy = 1.0
range = 200.0
texture = "res://light.png"
blend_mode = 2
shadow_enabled = true
shadow_color = Color(0.2, 0, 0, 0.5)
range_item_cull_mask = 1
shadow_item_cull_mask = 1

[node name="Wall" type="LightOccluder2D" parent="."]
polygon = PackedVector2Array(0, 40, 30, 40)
light_mask = 1
"#;

fn tree_of(source: &str) -> SceneTree {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(source).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// `render_play_frame` as it was before the split: one pass over the tree,
/// `render_canvas_from_list` reading lights, occluders, masks and modulate
/// from it after rasterizing. The reference the split must reproduce.
fn one_piece_pipeline(
    tree: &mut SceneTree,
    cache: &mut TextureCache,
    settings: &ProjectSettings,
) -> FrameBuffer {
    let mut window = *tree.root_window();
    window.size = Vector2::new(W as f32, H as f32);
    let canvas_size = canvas_size_of(&window);
    let root = tree.root_id();
    let controls = build_control_render_items_sourced(tree, root, canvas_size);
    let runtime = build_runtime_render_list(tree);
    let mut items = merge_control_items(tree, runtime, controls);
    if let Some(view) = camera2d_view_transform(tree, canvas_size, None, 1.0 / 60.0) {
        apply_camera2d_view(&mut items, view);
    }
    apply_stretch_transform(&mut items, window.stretch_transform());
    let quality = CanvasQuality::from_project_settings(settings);
    let mut fb = render_canvas_from_list(
        tree,
        &items,
        W,
        H,
        settings.default_clear_color(),
        Some(cache),
        &quality,
    );
    paint_black_bars(&mut fb, &window);
    fb
}

fn px(fb: &FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

#[test]
fn the_split_play_frame_draws_exactly_what_the_one_piece_tree_pipeline_drew() {
    let settings = ProjectSettings::default();
    let mut tree = tree_of(LIT_SCENE);
    let mut cache = TextureCache::new(".");
    let reference = one_piece_pipeline(&mut tree, &mut cache, &settings);

    let mut tree = tree_of(LIT_SCENE);
    let mut cache = TextureCache::new(".");
    let split = prepare_play_frame(&mut tree, W, H, &mut cache, &settings).rasterize(&mut cache);
    assert_eq!((split.width, split.height), (W, H));
    assert!(
        split.pixels == reference.pixels,
        "the prepared frame must match the one-piece pipeline pixel for pixel"
    );

    // Not vacuous: the light reaches the frame. The lit rect is red-dominant;
    // the green box is off the light's cull mask (layer 2 vs 1), so the red
    // lamp does not tint it and it stays green-dominant.
    let lit = px(&split, 30, 25);
    assert!(lit.r > lit.g + 0.2, "the red lamp must tint the rect: {lit:?}");
    let boxed = px(&split, 75, 25);
    assert!(
        boxed.g > boxed.r && boxed.g > boxed.b,
        "the culled box must keep its own green: {boxed:?}"
    );
    // And the convenience wrapper is the same two halves.
    let mut tree = tree_of(LIT_SCENE);
    let mut cache = TextureCache::new(".");
    let wrapped = render_play_frame(&mut tree, W, H, &mut cache, &settings);
    assert!(wrapped.pixels == split.pixels, "render_play_frame = prepare + rasterize");
}

#[test]
fn a_prepared_frame_is_a_snapshot_that_later_tree_changes_cannot_reach() {
    let settings = ProjectSettings::default();
    let mut tree = tree_of(LIT_SCENE);
    let mut cache = TextureCache::new(".");
    let before = render_play_frame(&mut tree, W, H, &mut cache, &settings);

    let prepared = prepare_play_frame(&mut tree, W, H, &mut cache, &settings);
    // What the game thread may do while the frame rasterizes off the lock:
    // move a node, recolour the modulate, turn the light off.
    let root = tree.root_id();
    let find = |tree: &SceneTree, name: &str| {
        let world = tree.get_node(root).unwrap().children()[0];
        *tree
            .get_node(world)
            .unwrap()
            .children()
            .iter()
            .find(|&&c| tree.get_node(c).unwrap().name() == name)
            .unwrap()
    };
    let rect = find(&tree, "Rect");
    let dim = find(&tree, "Dim");
    let lamp = find(&tree, "Lamp");
    tree.get_node_mut(rect).unwrap().set_property("position", Variant::Vector2(Vector2::new(30.0, 30.0)));
    tree.get_node_mut(dim).unwrap().set_property("color", Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)));
    tree.get_node_mut(lamp).unwrap().set_property("enabled", Variant::Bool(false));

    let snapshot = prepared.rasterize(&mut cache);
    assert!(
        snapshot.pixels == before.pixels,
        "a frame prepared before the change must draw the pre-change scene"
    );
    // Not vacuous: the changes do show on the next frame.
    let after = render_play_frame(&mut tree, W, H, &mut cache, &settings);
    assert!(after.pixels != before.pixels, "the tree changes must alter a fresh frame");
}

fn tex(c: Color) -> Texture2D {
    Texture2D::solid(2, 2, c)
}

#[test]
fn a_lent_texture_cache_keeps_every_change_made_to_its_stand_in() {
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);

    // invalidate + a fresh load on the stand-in.
    let mut cache = TextureCache::new("/proj");
    cache.insert("res://a.png", tex(red));
    cache.insert("res://b.png", tex(red));
    let mut stand_in = cache.lend_placeholder();
    assert_eq!(stand_in.project_root(), "/proj", "handlers read the root from the cache");
    assert!(stand_in.is_empty());
    stand_in.invalidate("res://a.png");
    stand_in.insert("res://c.png", tex(blue));
    cache.adopt_newer(stand_in);
    let names: std::collections::BTreeSet<String> =
        cache.loaded().map(|(k, _)| k.to_string()).collect();
    assert_eq!(
        names,
        ["res://b.png", "res://c.png"].iter().map(|s| s.to_string()).collect(),
        "an invalidation made while lent must not be undone by the cache coming back"
    );

    // An entry the stand-in reloaded wins over the lent copy.
    let mut stand_in = cache.lend_placeholder();
    stand_in.insert("res://b.png", tex(blue));
    cache.adopt_newer(stand_in);
    let b = cache.loaded().find(|(k, _)| *k == "res://b.png").unwrap().1;
    assert_eq!(b.get_pixel(0, 0), blue, "the newer load wins");

    // clear, and a re-root (opening another project), empty the returning cache.
    let mut stand_in = cache.lend_placeholder();
    stand_in.clear();
    cache.adopt_newer(stand_in);
    assert!(cache.is_empty(), "a clear while lent clears the cache");
    cache.insert("res://a.png", tex(red));
    let mut stand_in = cache.lend_placeholder();
    stand_in.set_project_root("/other");
    cache.adopt_newer(stand_in);
    assert_eq!(cache.project_root(), "/other");
    assert!(cache.is_empty(), "textures of the old project must not survive a re-root");

    // A normal cache records nothing (no unbounded invalidation log).
    let mut plain = TextureCache::new("/p");
    for i in 0..1000 {
        plain.invalidate(&format!("res://{i}.png"));
    }
    let mut target = TextureCache::new("/p");
    target.insert("res://7.png", tex(red));
    target.adopt_newer(plain);
    assert_eq!(target.len(), 1, "a plain cache carries no lent-change log");
}
