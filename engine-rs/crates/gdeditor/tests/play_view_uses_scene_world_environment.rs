//! pat-mgixk.51 part 2 acceptance: a running game renders the environment the
//! **scene** authored, not the editor's preview.
//!
//! # The false-done this undoes
//!
//! Every input existed and nothing on the game path read any of them.
//! `world_environment_from_scene` lived in `editor_server.rs` and ran only from
//! `api_load_scene`, for the *editor* viewport, and resolved only inline
//! `SubResource` environments — not the `ExtResource` `.tres` a real project
//! uses. `RenderServer3DAdapter::build_viewport` set `environment: None`.
//! `ProjectSettings::default_clear_color` had only test callers and
//! `default_environment` had no reader at all. So:
//!
//! * the Play view drew the editor's **preview** procedural sky over every
//!   scene, because `editor_render_environment` forces `background_mode = Sky`
//!   whenever the preview is enabled — which is the default;
//! * an unlit scene was lit anyway, by the editor's **preview sun**
//!   (`setup_editor_lighting_3d`, fired whenever `scene_lights.is_empty()`);
//! * the runner adapter rendered with no environment whatsoever.
//!
//! Godot's order (`scene/3d/camera_3d.cpp`, `scene/3d/world_environment.cpp`) is
//! the camera's own `environment`, then the scene's `WorldEnvironment`, then
//! `rendering/environment/defaults/default_environment`, then
//! `default_clear_color` — and the runtime has no preview sky and no preview sun.
//!
//! # Why the fixture is a `.tscn` + a `.tres` on disk
//!
//! 🔑 Not only the bead's wiring requirement — it is the mechanism.
//! `packed_scene` stores `environment = ExtResource("1_e")` as that **literal
//! string**, putting the resolved `res://` path in a companion
//! `__extres__environment` (`packed_scene.rs:1551-1583`). The fix reads that
//! companion, so a tree assembled in Rust with `environment` set to a path
//! directly would exercise a different branch and leave the real one untested.
//! The `.tres` is read through `FileAccess`, so it resolves against the
//! installed `res://` root exactly as a shipped game reads it.
//!
//! # Why these colours cannot pass by accident
//!
//! The editor preview sky is blue-grey at the top (0.385, 0.454, 0.55) and brown
//! at the bottom (0.2, 0.169, 0.133) — never red-dominant and never
//! green-dominant. So "the background is red" cannot be the preview leaking
//! through, and neither can "the background is green". The two cases also
//! discriminate *each other*: one frame's background must be red and the other's
//! green, which no single unchanging backdrop can satisfy.

use gdcore::math::Color;
use gdcore::set_path_roots;
use gdeditor::viewport_3d_render::render_game_3d_frame;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;

const W: u32 = 96;
const H: u32 = 96;

/// `res://` is process-global, so these tests must not interleave.
static ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A Godot-authored `Environment`: flat **red** background, ambient light
/// **disabled**.
///
/// `background_mode = 1` is `BG_COLOR` and `ambient_light_source = 1` is
/// `AMBIENT_SOURCE_DISABLED` (`Environment.xml`).
///
/// 🔑 Disabling ambient is what makes the second assertion mean something. With
/// Godot's default `AMBIENT_SOURCE_BG` the ambient term *is* the background
/// colour, so an unlit mesh would come out red — indistinguishable from the
/// backdrop behind it, and "no white light touched this mesh" would hold for a
/// pixel where nothing was drawn at all. With ambient off, an unlit mesh is
/// black against a red frame.
const RED_ENV_TRES: &str = r#"[gd_resource type="Environment" format=3]

[resource]
background_mode = 1
background_color = Color(1, 0, 0, 1)
ambient_light_source = 1
"#;

/// A project whose clear colour is **green** — a colour neither the environment
/// above nor the editor preview can produce, so the fallback is observable.
const PROJECT_GODOT: &str = r#"config_version=5

[rendering]

environment/defaults/default_clear_color=Color(0, 1, 0, 1)
"#;

/// A box at the origin, framed by a `current` camera, with no light of any kind.
///
/// `with_env` adds the `WorldEnvironment` naming the `.tres`; everything else is
/// held constant, so the two frames differ by exactly the thing under test.
fn scene_tscn(with_env: bool) -> String {
    let ext = if with_env {
        "[ext_resource type=\"Environment\" path=\"res://env.tres\" id=\"1_e\"]\n"
    } else {
        ""
    };
    let node = if with_env {
        "\n[node name=\"World\" type=\"WorldEnvironment\" parent=\".\"]\nenvironment = ExtResource(\"1_e\")\n"
    } else {
        ""
    };
    format!(
        r#"[gd_scene load_steps=2 format=3]
{ext}
[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Cube" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"
{node}"#
    )
}

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// A temp `res://` root holding the environment resource and the project file.
fn project_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("patina_worldenv_{tag}_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("env.tres"), RED_ENV_TRES).expect("write env.tres");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).expect("write project.godot");
    dir
}

fn is_red(c: Color) -> bool {
    c.r > 0.5 && c.g < 0.25 && c.b < 0.25
}

fn is_green(c: Color) -> bool {
    c.g > 0.5 && c.r < 0.25 && c.b < 0.25
}

fn corner(pixels: &[Color]) -> Color {
    pixels[0]
}

fn centre(pixels: &[Color]) -> Color {
    pixels[(H as usize / 2) * W as usize + W as usize / 2]
}

/// **The acceptance test.**
#[test]
fn play_view_uses_scene_world_environment() {
    let _lock = ROOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // The render list type-tests against the ClassDB (`collect_scene_content`
    // filters on `is_class("Node3D")`, and `MeshInstance3D` only answers that
    // through the registry), so without this the list comes back empty and the
    // frame is a flat backdrop — which would make the mesh assertions vacuous.
    gdobject::class_db::register_3d_classes();

    let dir = project_dir("red");
    set_path_roots(Some(dir), None);

    let tree = load(&scene_tscn(true));
    let fb = render_game_3d_frame(&tree, W, H).expect("the scene has a current Camera3D");

    // -- (1) the background is the environment the SCENE authored --------------
    let bg = corner(&fb.pixels);
    assert!(
        is_red(bg),
        "the scene's WorldEnvironment names an Environment .tres with \
         background_mode = Color and background_color = red, so a game frame's \
         backdrop must be red; got {bg:?}. A blue-grey value is the editor's \
         PREVIEW sky — `editor_render_environment` forcing background_mode = Sky \
         over whatever the scene asked for. GREEN would mean the project's \
         `default_clear_color` won, which is one step too far down Godot's \
         resolution order: a WorldEnvironment outranks it"
    );

    // -- (2) …and nothing lit the mesh but the (disabled) ambient -------------
    //
    // The centre pixel is the cube. Godot's runtime has no preview sun, so with
    // no light in the scene and ambient off the cube is black.
    //
    // 🔑 This cannot pass on an empty frame: assertion (1) has already shown the
    // frame is red where the cube is NOT, so a black centre is a drawn, unlit
    // cube rather than a render that produced nothing.
    let mesh = centre(&fb.pixels);
    assert!(
        mesh.r < 0.1 && mesh.g < 0.1 && mesh.b < 0.1,
        "with no light in the scene and ambient_light_source = DISABLED the cube \
         must be black; got {mesh:?}. Non-zero green and blue mean the editor's \
         white PREVIEW SUN lit it — `setup_editor_lighting_3d` adds that light \
         whenever the scene has none, which a running game never does"
    );

    // -- (3) the runner adapter renders the same scene the same way -----------
    //
    // A second renderer, reached through `RenderServer3DAdapter` rather than the
    // editor, so this is the bead's "the same scene in the runner adapter
    // matches" clause and not a restatement of (1). It used to pass
    // `environment: None`, which clears to black.
    let mut adapter = RenderServer3DAdapter::new(W, H);
    let (_snapshot, frame) = adapter.render_frame(&tree);
    let adapter_bg = corner(&frame.pixels);
    assert!(
        is_red(adapter_bg),
        "the runner adapter must resolve the same WorldEnvironment as the Play \
         view; got {adapter_bg:?}. Black means `build_viewport` is still handing \
         the renderer `environment: None`"
    );

    set_path_roots(None, None);

    // -- (4) with the WorldEnvironment gone, the project's clear colour --------
    let dir = project_dir("clear");
    set_path_roots(Some(dir), None);

    let bare = load(&scene_tscn(false));
    let fb = render_game_3d_frame(&bare, W, H).expect("the scene has a current Camera3D");
    let bg = corner(&fb.pixels);
    assert!(
        is_green(bg),
        "with no WorldEnvironment the frame must clear to the project's \
         `rendering/environment/defaults/default_clear_color`, which this \
         project authors as green; got {bg:?}. Red would mean the previous \
         scene's environment leaked; a blue-grey value means the preview sky is \
         still winning and `default_clear_color` still has no reader"
    );

    let mut adapter = RenderServer3DAdapter::new(W, H);
    let (_snapshot, frame) = adapter.render_frame(&bare);
    let adapter_bg = corner(&frame.pixels);
    assert!(
        is_green(adapter_bg),
        "the adapter must take the project clear colour too; got {adapter_bg:?}"
    );

    set_path_roots(None, None);
}

/// A project that authors **no** clear colour still gets Godot's documented
/// grey, not black and not the preview sky.
///
/// Separate from the acceptance test because it pins the *default of the
/// default*: `default_clear_color` falls back to `Color(0.3, 0.3, 0.3)`
/// (`project_settings.rs:399-403`), which is the value a project gets for free.
#[test]
fn an_unauthored_clear_colour_is_godots_grey() {
    let _lock = ROOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdobject::class_db::register_3d_classes();

    let dir = std::env::temp_dir().join(format!("patina_worldenv_grey_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("project.godot"), "config_version=5\n").expect("write project.godot");
    set_path_roots(Some(dir), None);

    let tree = load(&scene_tscn(false));
    let fb = render_game_3d_frame(&tree, W, H).expect("the scene has a current Camera3D");
    let bg = corner(&fb.pixels);

    set_path_roots(None, None);

    // The authored value round-trips: the renderer converts the custom
    // background to linear on the way in (`renderer.rs:1700`) and
    // `apply_camera_post` encodes back to sRGB on the way out
    // (`postprocess.rs:37`), and with the default `ToneMapper::Linear` and
    // exposure 1.0 those two cancel. So the frame carries 0.3, not its linear
    // 0.0732 — asserting the linear value here would be red forever.
    let expected = Color::new(0.3, 0.3, 0.3, 1.0);
    let d = (bg.r - expected.r)
        .abs()
        .max((bg.g - expected.g).abs())
        .max((bg.b - expected.b).abs());
    assert!(
        d < 0.02,
        "a project that authors no clear colour must still get Godot's grey \
         {expected:?}; got {bg:?} (max channel delta {d})"
    );
}

/// Nothing changes for a scene that authors no environment **and** has no
/// project — the case every Rust-built fixture in this crate is in.
///
/// 🔑 This is the guard that keeps the change additive. `runtime_environment_3d`
/// answers `None` there, so the Play view keeps the editor preview sky and
/// preview sun, and the nine existing `gdeditor` fixtures that render the game
/// path with a tree built in Rust are untouched. If this ever goes red, those
/// nine need their expectations revisited in the same change.
#[test]
fn no_project_and_no_world_environment_keeps_the_preview() {
    let _lock = ROOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdobject::class_db::register_3d_classes();

    // No `res://` root at all: `FileAccess` cannot resolve `res://project.godot`,
    // so there is no project to take a clear colour from.
    set_path_roots(None, None);

    let tree = load(&scene_tscn(false));
    let fb = render_game_3d_frame(&tree, W, H).expect("the scene has a current Camera3D");
    let bg = corner(&fb.pixels);
    assert!(
        !is_red(bg) && !is_green(bg),
        "with no project and no WorldEnvironment the backdrop should still be \
         the editor preview sky; got {bg:?}"
    );
    // …and the cube is still lit by the preview environment, so it is not black.
    //
    // The dominant term here is the preview's `AmbientSource::Sky` at energy
    // 0.5, not the preview sun: the sun arrives from above-left and the face
    // toward the camera has a +Z normal, which that direction does not reach. So
    // this is "the preview environment is still in effect", which is the
    // property that keeps the Rust-built fixtures unchanged.
    let mesh = centre(&fb.pixels);
    assert!(
        mesh.r + mesh.g + mesh.b > 0.1,
        "the editor preview environment must still light a scene that authors \
         none of its own; the cube came out {mesh:?}. If this is black, the \
         preview was suppressed unconditionally and the nine Rust-built Play \
         fixtures in this crate change with it"
    );
}
