//! pat-9ww3e: truck_town's menu and in-game HUD lay out as real Godot 4.6.1
//! lays them out (`games/truck_town`, the official `3d/truck_town` demo,
//! unmodified).
//!
//! Every expected rect and font size below was CAPTURED from Godot, not
//! derived: a headless Godot 4.6.1 run of each scene dumping every Control's
//! `position`, `size`, `get_combined_minimum_size()` and
//! `get_theme_font_size("font_size")` after five frames (the probe script is
//! `godot --headless -s dump.gd -- res://car_select/car_select.tscn`, run on a
//! temp copy of the game). Headless Godot's window is 64x64 with a 1152x1152
//! visible rect, so only the PARENT-RELATIVE rects are compared — the absolute
//! ones depend on the window.
//!
//! What was wrong (Patina before the fix, same nodes):
//! - every default-theme Label was 16px tall where Godot's is 23 (the font
//!   size was used as the line height, and glyphs were drawn at
//!   `ascent - descent` = 16px instead of a 16px EM — 0.73x Godot's size);
//! - a container's minimum size was 0, so the anchored car strip was 0px tall
//!   with its buttons 122px above it, and the MarginContainer inside each
//!   PanelContainer handed its HBox a 0x0 rect: every CheckBox was 0px tall;
//! - BoxContainer `separation` defaulted to 0 (Godot: 4) and a PanelContainer
//!   inset by 4 (Godot: 0);
//! - layout only ran when a frame was RENDERED, so a frame stepped without a
//!   render (all of the agent `play`) read every `size` as zero.
//!
//! These step the real game through `MainLoop` and never render, which is the
//! last point above: the rects must come from the frame itself.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, Key};
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

/// `res://` roots are process-global; plain `cargo test` runs this file's
/// tests on parallel threads.
static ROOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn roots_lock() -> std::sync::MutexGuard<'static, ()> {
    ROOTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Boots `car_select/car_select.tscn` the way the runner does.
fn boot_menu() -> MainLoop {
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
    ml
}

fn key(ml: &mut MainLoop, key: Key, pressed: bool) {
    ml.push_event(InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    });
}

/// Boots the menu and presses Enter on the focused first car, as a player
/// does, then steps until the town is in the tree.
fn boot_town() -> MainLoop {
    let mut ml = boot_menu();
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }
    key(&mut ml, Key::Enter, true);
    ml.step(1.0 / 60.0);
    key(&mut ml, Key::Enter, false);
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
        if ml.tree().get_node_by_path("/root/TownScene/GameUI").is_some() {
            break;
        }
    }
    ml.step(1.0 / 60.0);
    ml
}

fn vec2(ml: &MainLoop, path: &str, prop: &str) -> Vector2 {
    let id = ml
        .tree()
        .get_node_by_path(path)
        .unwrap_or_else(|| panic!("no node at {path}"));
    match ml.tree().get_node(id).unwrap().get_property(prop) {
        Variant::Vector2(v) => v,
        other => panic!("{path}:{prop} is {other:?}"),
    }
}

/// `(path, Godot position, Godot size)`, parent-relative.
type Rect = (&'static str, [f32; 2], [f32; 2]);

fn assert_rects(ml: &MainLoop, rects: &[Rect]) {
    let mut wrong = Vec::new();
    for (path, pos, size) in rects {
        let p = vec2(ml, path, "position");
        let s = vec2(ml, path, "size");
        let close = |a: Vector2, b: [f32; 2]| (a.x - b[0]).abs() < 0.01 && (a.y - b[1]).abs() < 0.01;
        if !close(p, *pos) || !close(s, *size) {
            wrong.push(format!(
                "{path}: Patina pos ({}, {}) size ({}, {}), Godot pos ({}, {}) size ({}, {})",
                p.x, p.y, s.x, s.y, pos[0], pos[1], size[0], size[1]
            ));
        }
    }
    assert!(wrong.is_empty(), "rects differ from Godot 4.6.1:\n{}", wrong.join("\n"));
}

const MENU: &str = "/root/CarSelect";
const MOOD: &str = "/root/CarSelect/MoodPanel/MarginContainer/HBoxContainer";
const OPTIONS: &str = "/root/CarSelect/OptionsPanel/MarginContainer/HBoxContainer";

#[test]
fn car_select_menu_rects_match_godot_without_rendering() {
    let _roots = roots_lock();
    let mut ml = boot_menu();
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    let mood = |n: &str| -> &'static str { Box::leak(format!("{MOOD}/{n}").into_boxed_str()) };
    let opt = |n: &str| -> &'static str { Box::leak(format!("{OPTIONS}/{n}").into_boxed_str()) };
    assert_rects(
        &ml,
        &[
            // The car strip, anchored to a 0px line across the middle, grown
            // to its buttons' 243px around it.
            ("/root/CarSelect/CarContainer", [0.0, 178.5], [1024.0, 243.0]),
            ("/root/CarSelect/CarContainer/MiniVan", [5.0, 0.0], [328.0, 243.0]),
            ("/root/CarSelect/CarContainer/TrailerTruck", [347.0, 0.0], [328.0, 243.0]),
            ("/root/CarSelect/CarContainer/TowTruck", [690.0, 0.0], [328.0, 243.0]),
            // PanelContainer insets by 0; MarginContainer by 20/5.
            ("/root/CarSelect/MoodPanel", [187.0, 464.0], [650.0, 41.0]),
            ("/root/CarSelect/MoodPanel/MarginContainer", [0.0, 0.0], [650.0, 41.0]),
            (MOOD, [20.0, 5.0], [610.0, 31.0]),
            // A default Label is one 23px font height, centred (SHRINK_CENTER).
            (mood("Label"), [0.0, 4.0], [70.0, 23.0]),
            // CheckBoxes: 31px tall (23 + 4 + 4), separated by 4, expanding.
            (mood("Sunrise"), [74.0, 0.0], [131.0, 31.0]),
            (mood("Day"), [209.0, 0.0], [131.0, 31.0]),
            (mood("Sunset"), [344.0, 0.0], [131.0, 31.0]),
            (mood("Night"), [479.0, 0.0], [131.0, 31.0]),
            (OPTIONS, [20.0, 5.0], [610.0, 31.0]),
            (opt("Label"), [0.0, 4.0], [70.0, 23.0]),
            // "SDFGI" + 16px icon + 4 + 8 of margins = 74.
            (opt("SDFGI"), [86.0, 0.0], [74.0, 31.0]),
            (opt("Audio"), [176.0, 0.0], [434.0, 31.0]),
            (opt("Audio/Mute"), [0.0, 0.0], [30.0, 31.0]),
            // HSlider: 16px (the grabber icon), centred, expanding to 400.
            (opt("Audio/Volume"), [34.0, 7.0], [400.0, 16.0]),
            (MENU, [64.0, 24.0], [1024.0, 600.0]),
        ],
    );
}

#[test]
fn car_select_minimum_sizes_and_font_sizes_match_godot() {
    let _roots = roots_lock();
    let mut ml = boot_menu();
    ml.step(1.0 / 60.0);
    let tree = ml.tree();
    let id = |p: &str| tree.get_node_by_path(p).unwrap_or_else(|| panic!("no node at {p}"));
    // Godot's `get_combined_minimum_size()` for each, from the same capture.
    for (path, min) in [
        (format!("{MOOD}/Label"), [70.0, 23.0]),
        (format!("{MOOD}/Sunrise"), [86.0, 31.0]),
        (format!("{MOOD}/Day"), [58.0, 31.0]),
        (format!("{MOOD}/Sunset"), [81.0, 31.0]),
        (format!("{MOOD}/Night"), [71.0, 31.0]),
        (format!("{OPTIONS}/SDFGI"), [74.0, 31.0]),
        (format!("{OPTIONS}/Audio/Mute"), [30.0, 30.0]),
        (format!("{OPTIONS}/Audio/Volume"), [100.0, 16.0]),
        (format!("{OPTIONS}/Audio"), [134.0, 30.0]),
        (MOOD.to_string(), [382.0, 31.0]),
        ("/root/CarSelect/MoodPanel".to_string(), [422.0, 41.0]),
        ("/root/CarSelect/CarContainer".to_string(), [992.0, 243.0]),
        ("/root/CarSelect/CarContainer/MiniVan".to_string(), [328.0, 243.0]),
        // Two lines of a 22px LabelSettings font: 2 * 31 + 3 of line spacing.
        (
            "/root/CarSelect/LoadingPanel/CenterContainer/Label".to_string(),
            [254.0, 65.0],
        ),
    ] {
        let m = gdscene::control::combined_minimum_size(tree, id(&path));
        assert!(
            (m.x - min[0]).abs() <= 1.0 && (m.y - min[1]).abs() < 0.01,
            "{path}: combined minimum ({}, {}), Godot ({}, {})",
            m.x,
            m.y,
            min[0],
            min[1]
        );
    }
    // `get_theme_font_size("font_size")`: the default theme's 16 everywhere
    // here, and the LabelSettings' 22 for the loading label.
    for path in [format!("{MOOD}/Label"), format!("{MOOD}/Sunrise"), format!("{OPTIONS}/SDFGI")] {
        assert_eq!(gdscene::control::resolved_font_size(tree, id(&path)), 16.0, "{path}");
    }
    assert_eq!(
        gdscene::control::resolved_font_size(
            tree,
            id("/root/CarSelect/LoadingPanel/CenterContainer/Label")
        ),
        22.0
    );
}

/// What the menu's controls DRAW, against Godot's draw code for the same nodes.
#[test]
fn car_select_menu_draws_godot_checkboxes_slider_and_mute_icon() {
    use gdscene::control_render::ControlDrawItem;
    let _roots = roots_lock();
    let mut ml = boot_menu();
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    let root = ml.tree().root_id();
    let theme = gdscene::theme::Theme::default_theme();
    let items = gdscene::control_render::control_draw_items(
        ml.tree_mut(),
        root,
        &theme,
        Vector2::new(1152.0, 648.0),
    );
    let gpos = |ml: &MainLoop, p: &str| {
        let id = ml.tree().get_node_by_path(p).unwrap();
        gdscene::control::get_global_position(ml.tree(), id)
    };

    // The Mute TextureButton: a 50x50 icon, `stretch_mode = 5`
    // (KEEP_ASPECT_CENTERED) in a 30x31 rect -> 30x30 at y + 0.5
    // (`texture_button.cpp:198-214`). It used to fill the rect (SCALE).
    let mute = gpos(&ml, &format!("{OPTIONS}/Audio/Mute"));
    let icon = items
        .iter()
        .find_map(|i| match i {
            ControlDrawItem::Texture { texture_path, rect, .. }
                if texture_path.ends_with("audio_on.png") =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the Mute button draws audio_on.png");
    assert!(
        (icon.position.x - mute.x).abs() < 0.01
            && (icon.position.y - (mute.y + 0.5)).abs() < 0.01
            && (icon.size.x - 30.0).abs() < 0.01
            && (icon.size.y - 30.0).abs() < 0.01,
        "mute icon at {icon:?}, Godot draws it 30x30 at ({}, {})",
        mute.x,
        mute.y + 0.5
    );

    // The HSlider: Godot's 16x16 grabber disc (r = 7) centred on the track,
    // which is the 8px-tall `slider` stylebox, not the whole control.
    let vol = gpos(&ml, &format!("{OPTIONS}/Audio/Volume"));
    let grabbers: Vec<Vector2> = items
        .iter()
        .filter_map(|i| match i {
            ControlDrawItem::Circle { center, radius, .. } if (*radius - 7.0).abs() < 0.01 => {
                Some(*center)
            }
            _ => None,
        })
        .filter(|c| c.x >= vol.x && c.x <= vol.x + 400.0 && (c.y - (vol.y + 8.0)).abs() < 0.01)
        .collect();
    assert_eq!(grabbers.len(), 1, "one r=7 grabber disc on the volume slider's centre line");

    // The mood CheckBoxes share a ButtonGroup, so Godot draws RADIO icons:
    // Day (pressed) is an r=7 white ring around an r=4 dark centre at
    // content-left 4px, vertically centred; the others an r=7 faint disc.
    let day = gpos(&ml, &format!("{MOOD}/Day"));
    let at_day = Vector2::new(day.x + 4.0 + 8.0, day.y + 7.0 + 8.0);
    let discs: Vec<(f32, f32)> = items
        .iter()
        .filter_map(|i| match i {
            ControlDrawItem::Circle { center, radius, color, .. }
                if (center.x - at_day.x).abs() < 0.01 && (center.y - at_day.y).abs() < 0.01 =>
            {
                Some((*radius, color.a))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        discs,
        vec![(7.0, 0.75), (4.0, 1.0)],
        "Day's radio_checked icon at {at_day:?}"
    );
    // Nothing of the old outline-box glyph is left on an unchecked radio.
    let sunrise = gpos(&ml, &format!("{MOOD}/Sunrise"));
    assert!(
        !items.iter().any(|i| matches!(i,
            ControlDrawItem::Rect { rect, filled: false, .. }
                if rect.position.x >= sunrise.x && rect.position.x < sunrise.x + 131.0
                    && rect.position.y >= sunrise.y && rect.position.y < sunrise.y + 31.0)),
        "an unchecked radio CheckBox draws no outline box"
    );

    // The label text starts after the 16px icon and 4px separation, and a
    // default 16px label is drawn at Godot's 16px EM (cap height ~11.5px).
    let day_text = items
        .iter()
        .find_map(|i| match i {
            ControlDrawItem::Text { text, position, font_size, .. } if text == "Day" => {
                Some((*position, *font_size))
            }
            _ => None,
        })
        .expect("Day's text");
    assert_eq!(day_text.1, 16);
    assert!((day_text.0.x - (day.x + 24.0)).abs() < 0.01, "Day text at {:?}", day_text.0);
    assert!((day_text.0.y - (day.y + 4.0)).abs() < 0.01, "Day text at {:?}", day_text.0);
}

/// Godot draws a default 16px string with a 16px EM: Open Sans' capital H is
/// 0.714 em tall, so 11-12 rows of ink, and "Mood" is about 42px wide. Patina
/// drew it at 0.73x that.
#[test]
fn default_font_draws_at_godots_em_size() {
    let face = gdrender2d::TtfFont::builtin();
    let h = face.rasterize('H', 16.0).expect("H has an outline");
    assert!(
        (11..=12).contains(&h.height),
        "a 16px H must be ~11.4px of ink (Open Sans cap height 1462/2048 em), got {}",
        h.height
    );
    assert_eq!(face.ascent(16.0), 18.0, "Godot's ascent for 16px Open Sans");
    assert_eq!(face.descent(16.0), 5.0, "Godot's descent for 16px Open Sans");
    assert_eq!(face.line_height(16.0), 23.0, "Godot's Label is 23px tall");
    assert_eq!(face.line_height(30.0), 42.0, "TURBO's 30px line is 42px in Godot");
    assert_eq!(face.line_height(22.0), 31.0, "the loading label's 22px line is 31px");
}

#[test]
fn town_hud_rects_match_godot_without_rendering() {
    let _roots = roots_lock();
    let ml = boot_town();
    let tree = ml.tree();
    assert!(
        tree.get_node_by_path("/root/TownScene/GameUI/Back").is_some(),
        "Enter on the focused first car must load the town"
    );
    // GameUI sits under a Node3D, so it anchors to the 1152x648 window;
    // CarUI is anchored to the bottom-right corner (Godot: 1152 - 428 = 724,
    // 648 - 92 = 556 in a 1152x648 window).
    assert_rects(
        &ml,
        &[
            // Grown from its 144x74 offsets to its 157x82 minimum.
            ("/root/TownScene/GameUI", [24.0, 24.0], [157.0, 82.0]),
            ("/root/TownScene/GameUI/Back", [0.0, 0.0], [58.0, 31.0]),
            // separation 20 below the 31px Back button.
            ("/root/TownScene/GameUI/PanelContainer", [0.0, 51.0], [157.0, 31.0]),
            (
                "/root/TownScene/GameUI/PanelContainer/MarginContainer/VBoxContainer",
                [4.0, 4.0],
                [149.0, 23.0],
            ),
            (
                "/root/TownScene/GameUI/PanelContainer/MarginContainer/VBoxContainer/Label",
                [0.0, 0.0],
                [149.0, 23.0],
            ),
            ("/root/TownScene/CarUI", [724.0, 556.0], [404.0, 68.0]),
            // The 260x68 turbometer texture decides its minimum.
            ("/root/TownScene/CarUI/Turbometer", [0.0, 0.0], [260.0, 68.0]),
            ("/root/TownScene/CarUI/Turbometer/Label", [89.0, 9.0], [102.0, 42.0]),
            ("/root/TownScene/CarUI/Speedometer", [264.0, 28.0], [140.0, 40.0]),
        ],
    );
    let id = |p: &str| tree.get_node_by_path(p).unwrap();
    // TURBO's own `theme_override_font_sizes/font_size = 30`; Godot's
    // minimum for it is 102x42 (custom 102 wide, 30px font height 33 + 9).
    assert_eq!(
        gdscene::control::resolved_font_size(tree, id("/root/TownScene/CarUI/Turbometer/Label")),
        30.0
    );
    let turbo = gdscene::control::combined_minimum_size(
        tree,
        id("/root/TownScene/CarUI/Turbometer/Label"),
    );
    assert_eq!(turbo.y, 42.0, "a 30px Open Sans line is 42px tall in Godot");
    assert_eq!(
        gdscene::control::resolved_font_size(
            tree,
            id("/root/TownScene/GameUI/PanelContainer/MarginContainer/VBoxContainer/Label")
        ),
        16.0
    );
    // The hidden "Controls" help text takes no space in its VBox, so the
    // panel is one line tall; its own minimum is still Godot's 160x188
    // (4-line label 101 + separator 4 + 3-line label 75 + 2 * 4 separation).
    let controls = gdscene::control::combined_minimum_size(
        tree,
        id("/root/TownScene/GameUI/PanelContainer/MarginContainer/VBoxContainer/Controls"),
    );
    assert!(
        (controls.x - 160.0).abs() <= 1.0 && controls.y == 188.0,
        "Controls VBox minimum ({}, {}), Godot (160, 188)",
        controls.x,
        controls.y
    );
}

/// The turbometer's progress texture is drawn at its OWN size, shifted by
/// `texture_progress_offset = Vector2(2, 2)` inside the under-texture's
/// outline (`texture_progress_bar.cpp` DRAW) — not stretched over the whole
/// control, which hid the outline.
#[test]
fn town_turbometer_progress_sits_at_its_offset() {
    use gdscene::control_render::ControlDrawItem;
    let _roots = roots_lock();
    let mut ml = boot_town();
    let root = ml.tree().root_id();
    let theme = gdscene::theme::Theme::default_theme();
    let items = gdscene::control_render::control_draw_items(
        ml.tree_mut(),
        root,
        &theme,
        Vector2::new(1152.0, 648.0),
    );
    let id = ml.tree().get_node_by_path("/root/TownScene/CarUI/Turbometer").unwrap();
    let origin = gdscene::control::get_global_position(ml.tree(), id);
    let tex = |name: &str| {
        items
            .iter()
            .find_map(|i| match i {
                ControlDrawItem::Texture { texture_path, rect, .. }
                    if texture_path.ends_with(name) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the turbometer draws {name}"))
    };
    let progress = tex("turbometer.webp");
    let natural = gdscene::control_render::texture_pixel_size("res://town/ui/turbometer.webp")
        .expect("turbometer.webp has a size");
    assert!(
        (progress.position.x - (origin.x + 2.0)).abs() < 0.01
            && (progress.position.y - (origin.y + 2.0)).abs() < 0.01
            && (progress.size.x - natural.x).abs() < 0.01
            && (progress.size.y - natural.y).abs() < 0.01,
        "progress drawn at {progress:?}, Godot draws it {natural:?} at {:?} + (2, 2)",
        origin
    );
}
