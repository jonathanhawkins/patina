//! pat-mgixk.65 acceptance — a `.tscn`'s `theme` resource reaches Play pixels.
//!
//! # The false-done this undoes
//!
//! The theme *resolution* machinery was all there and had no caller that fed
//! it. `control_draw_items_themed` (control_render.rs:249) and
//! `effective_theme` (control_render.rs:1201) implement Godot's chain — node,
//! then each ancestor, then the default — but the only production entry point,
//! `build_control_render_items`, called the plain `control_draw_items`, which
//! forwards an **empty** `node_themes` map (control_render.rs:231-236). So a
//! scene's `theme = ExtResource("ui.tres")` changed nothing on screen while the
//! code read as though it worked.
//!
//! # Why this must load a `.tscn`, not build the tree in Rust
//!
//! 🔑 Not merely the bead's wiring requirement — it is the mechanism. A
//! `theme = ExtResource("1_t")` property is stored by `packed_scene` as the
//! **literal reference string**, with the resolved path put in a companion key
//! `__extres__theme` (packed_scene.rs:1524-1556); only `script` is rewritten in
//! place. The fix reads that companion key, so a tree assembled in Rust —
//! setting `theme` to a path directly — would exercise a different branch and
//! leave the real one untested.
//!
//! # Why the colours cannot pass by accident
//!
//! The default theme's `Button/normal` is a flat box of `(0.2, 0.2, 0.2)`
//! (theme.rs:500-504) and the fixture's is pure red, so "red" cannot be the
//! default leaking through. The unthemed case additionally asserts that it drew
//! *something*, because "no red pixels" is trivially true of a frame where
//! nothing rendered at all.

use gdcore::math::{Color, Vector2};
use gdcore::set_path_roots;
use gdeditor::viewport_2d_render::{build_control_render_items, rasterize_render_list};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const W: u32 = 120;
const H: u32 = 80;

/// `res://` is process-global, so these tests must not interleave.
static ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A Godot-authored Theme whose `Button/normal` is a pure red `StyleBoxFlat`.
///
/// The `[sub_resource]` + `SubResource("…")` shape is what `parse_theme_tres`
/// resolves (theme.rs:946-952, via `parse_styleboxes_from_tscn`): a *typed*
/// StyleBox rather than a bare `Variant::Color`, which is the form a real
/// `.tres` takes and the branch `stylebox_bg` reaches second.
const THEME_TRES: &str = r#"[gd_resource type="Theme" load_steps=2 format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_red"]
bg_color = Color(1, 0, 0, 1)

[resource]
Button/styles/normal = SubResource("StyleBoxFlat_red")
"#;

/// A Panel filling the viewport with one Button inside it.
///
/// The Button is the child, and the theme is on the **Panel** — so a pass that
/// only honoured a node's own theme would still fail. That is the ancestor hop
/// `effective_theme` exists for.
///
/// Explicit `offset_*` on both: a Control with no offsets has a zero rect and
/// draws nothing, which would make this fail for a reason unrelated to theming.
fn scene_tscn(with_theme: bool) -> String {
    let ext = if with_theme {
        "\n[ext_resource type=\"Theme\" path=\"res://ui_theme.tres\" id=\"1_t\"]\n"
    } else {
        ""
    };
    let theme_line = if with_theme {
        "theme = ExtResource(\"1_t\")\n"
    } else {
        ""
    };
    format!(
        r#"[gd_scene load_steps=2 format=3]
{ext}
[node name="Panel" type="Panel"]
offset_right = 120
offset_bottom = 80
{theme_line}
[node name="Press" type="Button" parent="."]
offset_left = 10
offset_top = 10
offset_right = 90
offset_bottom = 50
text = "Go"
"#
    )
}

struct Frame {
    red: usize,
    non_clear: usize,
}

/// Loads the scene, renders it the way the Play view does, and counts pixels.
fn render(with_theme: bool, dir: &std::path::Path) -> Frame {
    let scene = PackedScene::from_tscn(&scene_tscn(with_theme)).expect("scene parses");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let root = add_packed_scene_to_tree(&mut tree, tree_root, &scene).expect("scene instances");

    // `res://` resolves against the directory holding `ui_theme.tres`, so the
    // theme is read through `FileAccess` exactly as a running game reads it.
    set_path_roots(Some(dir.to_path_buf()), None);

    let items = build_control_render_items(&mut tree, root, Vector2::new(W as f32, H as f32));
    let clear = Color::new(0.0, 0.0, 0.0, 1.0);
    let fb = rasterize_render_list(&items, W, H, clear, None);

    set_path_roots(None, None);

    Frame {
        red: fb
            .pixels
            .iter()
            .filter(|c| c.r > 0.5 && c.g < 0.3 && c.b < 0.3)
            .count(),
        non_clear: fb
            .pixels
            .iter()
            .filter(|c| c.r > 0.01 || c.g > 0.01 || c.b > 0.01)
            .count(),
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("patina_theme_{tag}_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("ui_theme.tres"), THEME_TRES).expect("write theme");
    dir
}

/// **The acceptance test.**
#[test]
fn theme_resource_chain_reaches_play_render() {
    let _lock = ROOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Godot fills ClassDB during engine init, and the control walk type-tests
    // against it. Without this the scene's classes are unknown and the render
    // list can come back empty, which would make every count below vacuous.
    gdobject::class_db::register_all_classes();

    let dir = temp_dir("themed");

    // -- the theme on the Panel reaches the Button's pixels -----------------
    let themed = render(true, &dir);
    assert!(
        themed.red >= 500,
        "the Panel's `theme = ExtResource(\"ui_theme.tres\")` sets \
         Button/normal to a pure red StyleBoxFlat, so the child Button must \
         rasterize red: found {} red pixels of {}x{} ({} non-clear). Zero means \
         the theme never reached the render — `build_control_render_items` used \
         to call the UNTHEMED `control_draw_items`, which forwards an empty \
         node_themes map",
        themed.red,
        W,
        H,
        themed.non_clear
    );

    // -- and without it, the default theme's grey button -------------------
    let plain = render(false, &dir);
    assert_eq!(
        plain.red, 0,
        "with no `theme` property the default theme applies, whose Button/normal \
         is (0.2, 0.2, 0.2); {} red pixels would mean the red came from \
         somewhere other than the authored theme and the assertion above proves \
         less than it claims",
        plain.red
    );

    // 🔑 The vacuity guard. "No red" is trivially true of an empty frame, so the
    // unthemed case has to be shown to have drawn a button at all.
    assert!(
        plain.non_clear > 500,
        "the unthemed scene must still draw its Panel and Button — only {} \
         non-clear pixels. If this is ~0 the comparison above is between two \
         empty frames and means nothing",
        plain.non_clear
    );
}

/// A `theme` naming a file that is not there must fall back, not panic or blank
/// the UI.
///
/// A missing theme is ordinary — a moved `.tres`, a half-exported project — and
/// Godot keeps drawing with the inherited theme.
#[test]
fn a_missing_theme_resource_falls_back_to_the_default() {
    let _lock = ROOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdobject::class_db::register_all_classes();

    // A directory with NO `ui_theme.tres` written into it.
    let dir = std::env::temp_dir().join(format!("patina_theme_missing_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let _ = std::fs::remove_file(dir.join("ui_theme.tres"));

    let frame = render(true, &dir);
    assert_eq!(
        frame.red, 0,
        "there is no theme file to supply red; {} red pixels means a stale one \
         was read from somewhere",
        frame.red
    );
    assert!(
        frame.non_clear > 500,
        "a missing theme must leave the UI drawing with the default theme, not \
         blank it: only {} non-clear pixels",
        frame.non_clear
    );
}

/// The bead's second acceptance clause: `gui/theme/custom` themes a UI whose
/// nodes name **no** theme of their own.
///
/// This is how a shipped game is usually themed — one project-wide theme rather
/// than a `theme =` on every root Control — and it was the half of the chain
/// still unwired after the node-theme pass: `grep gui/theme/custom` over the
/// whole workspace returned nothing, so the setting was inert.
///
/// 🔑 The scene here is the **unthemed** one, so the project setting is the only
/// possible route to a red pixel. The acceptance test above already proves this
/// exact scene draws **zero** red without it, which is what makes the assertion
/// below a real discriminator rather than a restatement.
#[test]
fn a_project_theme_setting_reaches_play_render() {
    let _lock = ROOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("project_setting");

    // `[gui]` + `theme/custom` composes to the `gui/theme/custom` path
    // `ProjectSettings::from_godot_format` stores (`project_settings.rs:516`),
    // which is how Godot writes it too.
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[gui]\n\ntheme/custom=\"res://ui_theme.tres\"\n",
    )
    .expect("write project.godot");

    let themed = render(false, &dir);
    assert!(
        themed.red > 50,
        "`gui/theme/custom` must theme a scene whose nodes set no `theme`: got \
         {} red pixels of {} drawn. Zero means the project setting is still \
         inert — nothing read it, so the Button fell back to the default theme.",
        themed.red,
        themed.non_clear
    );

    // Removing the setting restores default pixels — the bead's third clause,
    // checked here for the project route specifically. Without this, a bug that
    // painted the Button red unconditionally would satisfy the assertion above.
    std::fs::remove_file(dir.join("project.godot")).expect("remove project.godot");
    let plain = render(false, &dir);
    assert_eq!(
        plain.red, 0,
        "with no node theme and no `gui/theme/custom`, nothing should be red; \
         {} red pixels means the red is coming from somewhere other than the \
         project setting",
        plain.red
    );
    assert!(
        plain.non_clear > 100,
        "fixture check: the Button must still be DRAWN once the theme is gone, \
         or `red == 0` above would hold for an empty frame; got {} drawn pixels",
        plain.non_clear
    );
}
