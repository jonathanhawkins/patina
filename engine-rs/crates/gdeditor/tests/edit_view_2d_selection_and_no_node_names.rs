//! The 2D edit view on real games, against what Godot's 2D editor draws.
//!
//! - pat-5puo3: a selected Control is outlined as its whole rect. Godot's
//!   `CanvasItemEditor::_draw_selection` strokes the item's
//!   `get_anchorable_rect()` through its global transform; Patina drew an 8px
//!   marker square at the Control's origin, so spark_keeper's full-rect Title
//!   showed as a dot in the corner.
//! - pat-uipye: no node NAMES are drawn in the canvas. Godot's 2D editor never
//!   labels nodes; Patina drew each name cut at 12 characters ("CollisionSha")
//!   and centred under the node origin, off its control.

use gdeditor::scene_renderer::render_scene_with_instance_previews;
use gdeditor::texture_cache::TextureCache;
use gdrender2d::renderer::FrameBuffer;
use gdscene::packed_scene::{add_packed_scene_to_tree_for_editing, PackedScene};
use gdscene::SceneTree;
use std::path::PathBuf;

const W: u32 = 1400;
const H: u32 = 900;

fn game(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games")
        .join(name)
}

/// `res://` is process-global and `cargo test` runs these tests on parallel
/// threads of one process, each rooted at a different game: every test holds
/// this for its whole body.
static ROOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn roots() -> std::sync::MutexGuard<'static, ()> {
    ROOTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn open(game_dir: &PathBuf, scene: &str) -> (SceneTree, gdscene::node::NodeId) {
    gdobject::class_db::register_all_classes();
    gdcore::set_path_roots(Some(game_dir.clone()), None);
    let text = std::fs::read_to_string(game_dir.join(scene)).unwrap();
    let packed = PackedScene::from_tscn(&text).unwrap();
    let mut tree = SceneTree::new();
    let r = tree.root_id();
    let root = add_packed_scene_to_tree_for_editing(&mut tree, r, &packed).unwrap();
    (tree, root)
}

fn render(
    tree: &mut SceneTree,
    game_dir: &PathBuf,
    selected: Option<gdscene::node::NodeId>,
) -> FrameBuffer {
    let root = game_dir.to_string_lossy().to_string();
    let mut cache = TextureCache::new(&root);
    render_scene_with_instance_previews(tree, &root, &mut cache, selected, W, H, 1.0, (0.0, 0.0))
}

/// The selection colour (`COLOR_SELECTED`, bright amber), exactly.
fn is_selection(c: &gdcore::math::Color) -> bool {
    (c.r - 1.0).abs() < 1e-3 && (c.g - 0.85).abs() < 1e-3 && c.b.abs() < 1e-3
}

/// Bounding box (min x, min y, max x, max y) and count of selection pixels.
fn selection_box(fb: &FrameBuffer) -> (u32, u32, u32, u32, usize) {
    let (mut x0, mut y0, mut x1, mut y1, mut n) = (u32::MAX, u32::MAX, 0, 0, 0);
    for y in 0..fb.height {
        for x in 0..fb.width {
            if is_selection(&fb.pixels[(y * fb.width + x) as usize]) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
                n += 1;
            }
        }
    }
    (x0, y0, x1, y1, n)
}

#[test]
fn a_selected_full_rect_control_is_outlined_as_its_whole_rect() {
    let _roots = roots();
    let dir = game("spark_keeper");
    let (mut tree, title) = open(&dir, "scenes/title.tscn");
    let fb = render(&mut tree, &dir, Some(title));
    let (x0, y0, x1, y1, n) = selection_box(&fb);
    // The Title is anchored Full Rect inside the 640x360 game rect, so its
    // outline is a 640x360 box (corner handles add 2px each side).
    let (w, h) = (x1 - x0, y1 - y0);
    assert!(
        (638..=646).contains(&w) && (358..=366).contains(&h),
        "outline is {w}x{h} ({n} px) at ({x0},{y0})-({x1},{y1}), want ~640x360"
    );
    assert!(n >= 1900, "outline must be a stroked rect, only {n} px");
}

#[test]
fn a_selected_container_child_is_outlined_at_its_laid_out_rect() {
    let _roots = roots();
    let dir = game("spark_keeper");
    let (mut tree, title) = open(&dir, "scenes/title.tscn");
    let menu = tree.get_node_relative(title, "Menu").unwrap();
    let fb = render(&mut tree, &dir, Some(menu));
    let (x0, y0, x1, y1, _) = selection_box(&fb);
    // Menu: centre-anchored, offsets -150..150 x -110..110, but its combined
    // minimum width is 314, so Godot grows it: real Godot 4.6.1 reports
    // size (314, 220) at (163, 70). (The 300x220 the offsets alone give was
    // this test's first expectation, before pat-9ww3e's container minimums.)
    let (w, h) = (x1 - x0, y1 - y0);
    assert!(
        (312..=320).contains(&w) && (218..=226).contains(&h),
        "Menu outline is {w}x{h}"
    );
    // And it sits inside the Title's outline, centred: Title-relative centre.
    let (tx0, ty0, tx1, ty1, _) = {
        let fb = render(&mut tree, &dir, Some(title));
        selection_box(&fb)
    };
    let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
    let (tcx, tcy) = ((tx0 + tx1) / 2, (ty0 + ty1) / 2);
    assert!(
        cx.abs_diff(tcx) <= 2 && cy.abs_diff(tcy) <= 2,
        "Menu centre ({cx},{cy}) vs Title ({tcx},{tcy})"
    );
}

/// Renaming nodes changes nothing on the canvas: no names are drawn.
#[test]
fn the_canvas_draws_no_node_names() {
    let _roots = roots();
    for (g, scene) in [
        ("dodge_the_creeps", "player.tscn"),
        ("spark_keeper", "scenes/title.tscn"),
    ] {
        let dir = game(g);
        let (mut tree, root) = open(&dir, scene);
        let before = render(&mut tree, &dir, None);
        let mut ids = Vec::new();
        tree.collect_subtree_top_down(root, &mut ids);
        for (i, id) in ids.iter().enumerate() {
            if *id != root {
                tree.get_node_mut(*id)
                    .unwrap()
                    .set_name(format!("WWWWWWWWWWWWWWWW{i}"));
            }
        }
        let after = render(&mut tree, &dir, None);
        let differ = before
            .pixels
            .iter()
            .zip(&after.pixels)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differ, 0,
            "{g}/{scene}: renaming nodes changed {differ} canvas pixels"
        );
    }
}
