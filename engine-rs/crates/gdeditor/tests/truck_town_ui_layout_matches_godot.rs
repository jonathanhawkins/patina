//! pat-9ww3e: truck_town's in-game HUD, as the editor's Play view draws it.
//!
//! `games/truck_town` (the official `3d/truck_town` demo, unmodified) adds its
//! HUD — `GameUI` (a « Back button over an "F1 - Toggle controls" panel) and
//! `CarUI` (turbometer + speedometer, anchored bottom-right) — as Control
//! children of the 3D `TownScene`. Real Godot 4.6.1 draws them at 24,24 and
//! 724,556 (captured: see `gdscene/tests/truck_town_ui_layout_matches_godot.rs`).
//!
//! The 3D Play frame (`play_3d::prepare_play_3d_frame`, what both the Game tab
//! and the agent `play` screenshot draw) built its HUD from the generic
//! per-node canvas walk only, with no Control pass: no container layout, so
//! TURBO and the help text piled up at the window's top-left and the Back
//! button never appeared; every text drew in opaque white (the text colour was
//! dropped); and the HUD was rasterized WITHOUT a texture cache, so the
//! turbometer was the rasterizer's 32px white placeholder square.

use gdcore::math::{Color, Vector2};
use gdplatform::input::{InputEvent, Key};
use gdscene::canvas_render::Render2DKind;
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

fn boot_town() -> MainLoop {
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &dir);
    let tscn = std::fs::read_to_string(dir.join("car_select/car_select.tscn")).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::begin_subscene_session();
    let id = gdscene::packed_scene::instance_packed_scene_detached(
        &mut tree,
        &packed,
        &gdscene::packed_scene::load_scene_from_res,
    )
    .unwrap();
    gdscene::scripting::attach_subtree_scripts_from_res(&mut tree, id);
    let node = tree.take_node(id).unwrap();
    tree.add_child(root, node).unwrap();
    gdscene::packed_scene::finish_attached_scene(&mut tree, id);
    let mut ml = MainLoop::new(tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.register_physics_bodies();
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }
    let key = |ml: &mut MainLoop, pressed| {
        ml.push_event(InputEvent::Key {
            key: Key::Enter,
            pressed,
            shift: false,
            ctrl: false,
            alt: false,
        })
    };
    key(&mut ml, true);
    ml.step(1.0 / 60.0);
    key(&mut ml, false);
    for _ in 0..12 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        ml.tree().get_node_by_path("/root/TownScene/GameUI/Back").is_some(),
        "Enter on the focused first car loads the town"
    );
    ml
}

fn text_items(items: &[gdscene::canvas_render::Render2DItem]) -> Vec<(String, Vector2, Color)> {
    items
        .iter()
        .filter_map(|i| match &i.kind {
            Render2DKind::Text { text, .. } => {
                Some((text.clone(), i.global_transform.origin, i.modulate))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn town_hud_draws_at_godots_rects_with_its_colours_and_textures() {
    let mut ml = boot_town();
    let vp = gdeditor::viewport_3d::Viewport3D::new(1152, 648);
    let frame = gdeditor::play_3d::prepare_play_3d_frame(
        ml.tree_mut(),
        &vp,
        false,
        Color::new(0.133, 0.133, 0.133, 1.0),
        1152,
        648,
    );
    let texts = text_items(&frame.canvas_items);
    // The LAST item for a string is its fill: an outlined label (TURBO's 6px
    // pink outline) stamps the outline first, under the text.
    let find = |s: &str| {
        texts
            .iter()
            .rev()
            .find(|(t, _, _)| t == s)
            .cloned()
            .unwrap_or_else(|| panic!("the HUD draws {s:?}; texts drawn: {texts:?}"))
    };

    // « Back: a 58x31 Button at (24, 24); its 16px text centred in it.
    let (_, back, _) = find("« Back");
    assert!(
        back.x > 24.0 && back.x < 34.0 && (back.y - 28.0).abs() < 1.0,
        "the Back label sits inside Godot's (24, 24) 58x31 button, at {back:?}"
    );
    // The help label: inside the PanelContainer at (24, 75), inset 4 by the
    // MarginContainer, in its authored half-alpha white.
    let (_, help, help_colour) = find("F1 - Toggle controls");
    assert!(
        (help.x - 28.0).abs() < 0.01 && (help.y - 79.0).abs() < 0.01,
        "help text at {help:?}, Godot's label rect starts at (28, 79)"
    );
    assert!(
        (help_colour.a - 0.502).abs() < 0.01,
        "font_color = Color(1, 1, 1, 0.5): drawn with alpha {}",
        help_colour.a
    );
    // TURBO: inside the bottom-right turbometer, in its authored grey (the
    // label is centred on the meter: 724 + 89, 556 + 9 in Godot).
    let (_, turbo, turbo_colour) = find("TURBO");
    assert!(
        (turbo.x - (724.0 + 89.0)).abs() < 0.01 && (turbo.y - (556.0 + 9.0)).abs() < 0.01,
        "TURBO at {turbo:?}, Godot's label rect is (813, 565) 102x42"
    );
    assert!(
        (turbo_colour.r - 0.487).abs() < 0.01,
        "font_color 0.487 grey, drawn with {turbo_colour:?}"
    );

    // The turbometer's textures load (the HUD raster has a texture cache):
    // its under-texture's orange right end shows in the meter's rect, and
    // no 32px placeholder square is drawn anywhere.
    let canvas = frame.render_canvas().expect("the town has a HUD");
    let px = |x: u32, y: u32| canvas.pixels[(y * canvas.width + x) as usize];
    let mut orange = 0;
    for y in 556..624 {
        for x in 724..984 {
            let c = px(x, y);
            if c.a > 0.5 && c.r > 0.8 && c.g > 0.35 && c.g < 0.75 && c.b < 0.3 {
                orange += 1;
            }
        }
    }
    assert!(orange > 200, "the turbometer texture draws ({orange} orange pixels)");
    let white_block = (556..588).all(|y| (724..756).all(|x| {
        let c = px(x, y);
        c.a > 0.99 && c.r > 0.99 && c.g > 0.99 && c.b > 0.99
    }));
    assert!(!white_block, "a missing-texture placeholder square sits on the turbometer");
}
