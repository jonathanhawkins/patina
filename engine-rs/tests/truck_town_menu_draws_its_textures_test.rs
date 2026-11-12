//! pat-60d96: truck_town's car-select menu draws its logo and car pictures
//! where Godot does.
//!
//! The menu (`car_select/car_select.tscn`, unmodified) is a TextureRect logo,
//! three icon-only Buttons in a centre-line HBoxContainer strip and a
//! TextureButton. In editor Play the logo and the cars were missing and the
//! edit view drew white squares (compare-to-godot f90: 14.95% mismatch).
//! Four causes, each asserted on the Control render items:
//!
//! 1. the control arms read `texture = ExtResource("7_uu0k4")` LITERALLY and
//!    ignored the resolved `__extres__texture` companion;
//! 2. `Button.icon` was neither drawn nor counted in the button's minimum size,
//!    so each car button laid out 8x24;
//! 3. a control texture drew at its NATURAL size, not scaled into its rect, so
//!    the 1136x667 logo overflowed its 841x494 TextureRect;
//! 4. the strip container (anchor_top = anchor_bottom = 0.5, zero offsets)
//!    was laid out against the whole viewport, then pinned to the top of the
//!    menu as if its (0, 0) position were authored.
//!
//! Expected values are Godot 4.6.1's, captured with
//! `node scripts/godot-reference.mjs games/truck_town --frames 10 --watch
//! /root/CarSelect/CarContainer/MiniVan:global_position ...`:
//! MiniVan at (69, 202.5) size (328, 243); TextureRect at (156, -132) size
//! (841.355, 494); CarContainer at (64, 202.5).

use gdeditor::editor_server::{
    apply_project_path_roots, build_play_tree, load_launch_scene, EditorState,
};
use gdscene::canvas_render::Render2DKind;
use gdscene::MainLoop;
use std::path::Path;

#[test]
fn the_car_select_menu_draws_its_logo_and_car_icons_where_godot_does() {
    let game = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../games/truck_town")
        .canonicalize()
        .unwrap();
    let root = game.to_string_lossy().to_string();
    apply_project_path_roots(&root);
    let scene = game.join("car_select/car_select.tscn");
    let (tree, _r, _s) = load_launch_scene(&scene.to_string_lossy()).expect("opens");
    let mut state = EditorState::new(tree);
    state.scene_file = Some(scene.to_string_lossy().to_string());
    state.texture_cache = gdscene::texture_cache::TextureCache::new(&root);
    state.project_root = Some(root.clone());
    let play = build_play_tree(&mut state, None);
    let mut ml = MainLoop::new(play);
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }
    let tree = ml.tree_mut();
    let root_id = tree.root_id();
    let items = gdscene::canvas_render::build_control_render_items_sourced(
        tree,
        root_id,
        gdcore::math::Vector2::new(1152.0, 648.0),
    );
    let tree = ml.tree();
    let sprite = |node_path: &str| {
        let id = tree
            .get_node_by_path(node_path)
            .unwrap_or_else(|| panic!("{node_path}"));
        items
            .iter()
            .filter(|(_, src)| *src == Some(id))
            .find_map(|(item, _)| match &item.kind {
                Render2DKind::Sprite { texture, .. } => {
                    Some((texture.clone(), item.global_transform))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{node_path} draws no texture"))
    };
    let near = |a: f32, b: f32, tol: f32| (a - b).abs() <= tol;

    // 1 + 3. The logo: its own file, scaled into Godot's rect.
    let (tex, xf) = sprite("/root/CarSelect/TextureRect");
    assert_eq!(tex, "res://trucktown.png");
    assert!(
        near(xf.origin.x, 156.0, 0.5) && near(xf.origin.y, -132.0, 0.5),
        "{:?}",
        xf.origin
    );
    assert!(near(xf.x.x, 841.355 / 1136.0, 1e-3), "x scale {}", xf.x.x);
    assert!(near(xf.y.y, 494.0 / 667.0, 1e-3), "y scale {}", xf.y.y);

    // 2 + 4. The Mini Van icon at Godot's button, inside the 4px content margin.
    let (tex, xf) = sprite("/root/CarSelect/CarContainer/MiniVan");
    assert_eq!(tex, "res://car_select/choose_van.png");
    assert!(
        near(xf.origin.x, 69.0 + 4.0, 1.0) && near(xf.origin.y, 202.5 + 4.0, 1.0),
        "MiniVan icon at {:?}, Godot's button is at (69, 202.5)",
        xf.origin
    );
    assert!(
        near(xf.x.x, 1.0, 1e-3) && near(xf.y.y, 1.0, 1e-3),
        "a 320x235 icon fits unscaled"
    );
    let van = tree
        .get_node_by_path("/root/CarSelect/CarContainer/MiniVan")
        .unwrap();
    let size = gdscene::control::get_size(tree, van);
    assert_eq!((size.x, size.y), (328.0, 243.0), "Godot's MiniVan size");

    // 1. The TextureButton's normal texture.
    let (tex, _) = sprite("/root/CarSelect/OptionsPanel/MarginContainer/HBoxContainer/Audio/Mute");
    assert_eq!(tex, "res://car_select/audio_on.png");
}
