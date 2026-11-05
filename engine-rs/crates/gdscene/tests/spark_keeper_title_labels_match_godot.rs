//! pat-9ww3e: spark_keeper's title screen hint (`games/spark_keeper`,
//! `scenes/title.tscn`) as Godot 4.6.1 draws it: the Label
//! `Hint` has `modulate = Color(1, 1, 1, 0.6)` and the text
//! `"WASD / arrows to move  ·  Space to pulse"` with DOUBLE spaces around the
//! dot. Godot's reference frame (compare-to-godot, f48) shows it grey and
//! with the wide gaps; Patina drew it opaque white with the gaps collapsed to
//! single spaces (the Control pass dropped the node's modulate, and the Label
//! layout re-joined its words with one space).

use gdcore::math::Vector2;
use gdscene::canvas_render::Render2DKind;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

/// Rows of `fb` holding any pixel brighter than the dark clear.
fn ink_rows(fb: &gdrender2d::renderer::FrameBuffer) -> usize {
    (0..fb.height)
        .filter(|&y| (0..fb.width).any(|x| fb.get_pixel(x, y).r > 0.5))
        .count()
}

/// A `canvas_items` stretch scales the TEXT with the boxes around it, as
/// Godot's canvas transform does. In a Game view smaller than the project's
/// 1152x648 the boxes shrank and the glyphs did not, so every label overflowed
/// its button or panel.
#[test]
fn stretched_canvas_scales_the_glyphs_too() {
    gdobject::class_db::register_all_classes();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/spark_keeper")
        .canonicalize()
        .expect("games/spark_keeper fixture");
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let tscn = std::fs::read_to_string(dir.join("scenes/title.tscn")).unwrap();
    let scene = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let items = gdscene::canvas_render::build_control_render_items(
        &mut tree,
        root,
        Vector2::new(1152.0, 648.0),
    );
    let clear = gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0);
    let full = gdscene::canvas_render::rasterize_render_list(&items, 1152, 648, clear, None);
    let mut half_items = items.clone();
    gdscene::canvas_render::apply_stretch_transform(
        &mut half_items,
        gdcore::math::Transform2D::scaled(Vector2::new(0.5, 0.5)),
    );
    let half = gdscene::canvas_render::rasterize_render_list(&half_items, 576, 324, clear, None);
    let (a, b) = (ink_rows(&full) as f32, ink_rows(&half) as f32);
    assert!(a > 40.0, "the title screen draws text ({a} inked rows)");
    assert!(
        (b / a - 0.5).abs() < 0.12,
        "at a 0.5 canvas scale the text is half as tall: {a} rows -> {b}"
    );
}

#[test]
fn title_hint_keeps_its_spaces_and_its_modulate() {
    gdobject::class_db::register_all_classes();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/spark_keeper")
        .canonicalize()
        .expect("games/spark_keeper fixture");
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let tscn = std::fs::read_to_string(dir.join("scenes/title.tscn")).unwrap();
    let scene = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let items = gdscene::canvas_render::build_control_render_items(
        &mut tree,
        root,
        Vector2::new(1152.0, 648.0),
    );
    let hint: Vec<_> = items
        .iter()
        .filter_map(|i| match &i.kind {
            Render2DKind::Text { text, .. } if text.contains("WASD") => {
                Some((text.clone(), i.modulate))
            }
            _ => None,
        })
        .collect();
    assert_eq!(hint.len(), 1, "one hint line: {hint:?}");
    assert_eq!(
        hint[0].0, "WASD / arrows to move  ·  Space to pulse",
        "an unwrapped Label draws its text as authored, double spaces included"
    );
    assert!(
        (hint[0].1.a - 0.6).abs() < 1e-3,
        "the Label's modulate (alpha 0.6) applies to its text, got {:?}",
        hint[0].1
    );
}
