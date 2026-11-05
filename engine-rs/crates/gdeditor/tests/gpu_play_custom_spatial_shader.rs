//! pat-9gjpr: the GPU editor/Play path (`render_editor_color_bgra`) draws a
//! custom `shader_type spatial` ShaderMaterial, honours `no_depth_test`, and
//! culls a `double_sided = false` Label3D seen from behind.
//!
//! Driven the way the editor binary runs Play (open the project, play the
//! main scene, `prepare_play_3d_frame`, `render_world`) on
//! `fixtures/conformance/custom_spatial_shader`, a runnable Godot project. Real
//! Godot 4.6.1 renders that scene as (`node scripts/godot-reference.mjs
//! engine-rs/fixtures/conformance/custom_spatial_shader --frames 10 --shots 5`):
//!
//! - the left quad as 8 vertical bands (4 stripes x blue/orange), the lower
//!   half at half brightness;
//! - the red "XRAY" label drawn OVER the grey wall that stands in front of it;
//! - no green "HIDDEN" label: it faces away from the camera.
//!
//! Before: the quad was one flat orange (`resolve_shader_albedo` picked the
//! stripe colour), XRAY was hidden by the wall, HIDDEN drew mirrored.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::play_3d::{Play3DScene, PreparedPlay3DFrame};
use gdscene::SceneTree;
use gdserver3d::material::CullMode;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const W: u32 = 320;
const H: u32 = 240;

fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(120))).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

fn project() -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/conformance/custom_spatial_shader")
        .canonicalize()
        .expect("custom_spatial_shader fixture")
        .to_string_lossy()
        .into_owned()
}

/// The Play frame, plus each named node's id.
fn play() -> (PreparedPlay3DFrame, HashMap<String, u64>) {
    gdobject::class_db::register_all_classes();
    let project = project();
    // No `set_current_dir` (pat-0lcow): `resolve_shader_material` resolves a
    // `shader_path` (`res://stripes.gdshader`) against the project root that
    // opening the project installs, not the process cwd. This test used to
    // chdir into the fixture to make the shader load; without the chdir every
    // pixel assertion below fails if `res://` reads from the cwd again.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    std::thread::sleep(Duration::from_millis(100));
    request(port, "POST", "/api/project/open", &format!(r#"{{"path":"{project}"}}"#));
    let load = request(
        port,
        "POST",
        "/api/scene/load",
        &format!(r#"{{"path":"{project}/main.tscn"}}"#),
    );
    assert!(load.contains("\"ok\""), "load: {load}");
    let r = request(port, "POST", "/api/runtime/play", "{}");
    assert!(r.contains("\"ok\":true"), "play: {r}");
    std::thread::sleep(Duration::from_millis(400));
    let out = {
        let state = handle.state();
        let st = state.lock().unwrap();
        let tree = st
            .run_main_loop
            .as_ref()
            .map(|ml| ml.tree())
            .unwrap_or(&st.scene_tree);
        let mut ids = HashMap::new();
        for n in tree.all_nodes_in_tree_order() {
            if let Some(node) = tree.get_node(n) {
                ids.insert(node.name().to_string(), n.raw());
            }
        }
        let clear = st
            .run_project_settings
            .as_ref()
            .unwrap_or(&st.project_settings)
            .default_clear_color();
        let prepared = gdeditor::play_3d::prepare_play_3d_frame(
            tree,
            &st.editor_viewport_3d,
            st.runtime_camera_override,
            clear,
            W,
            H,
        );
        (prepared, ids)
    };
    request(port, "POST", "/api/runtime/stop", "");
    handle.stop();
    out
}

fn inputs_mut(p: &mut PreparedPlay3DFrame) -> &mut gdeditor::viewport_3d_render::Editor3DRenderInputs {
    match &mut p.scene {
        Play3DScene::Camera { inputs, .. } => inputs,
        _ => panic!("the fixture plays through its Camera3D"),
    }
}

/// Applies `f` to the draw item of node `id`.
fn edit(p: &mut PreparedPlay3DFrame, id: u64, f: impl FnOnce(&mut gdeditor::viewport_3d_render::Render3DItem)) {
    f(inputs_mut(p).items.iter_mut().find(|i| i.node_id == id).expect("item"));
}

/// RGB at (x, y).
fn px(img: &gdrender2d::renderer::Bgra8Image, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * img.width + x) * 4) as usize;
    [img.data[i + 2], img.data[i + 1], img.data[i]]
}

/// Pixels that differ between two frames inside `[x0, x1) x [y0, y1)`.
fn diff(a: &gdrender2d::renderer::Bgra8Image, b: &gdrender2d::renderer::Bgra8Image, x0: u32, x1: u32, y0: u32, y1: u32) -> usize {
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let (p, q) = (px(a, x, y), px(b, x, y));
            if p.iter().zip(q).any(|(c, d)| (*c as i32 - d as i32).abs() > 3) {
                n += 1;
            }
        }
    }
    n
}

fn reddish(c: [u8; 3]) -> bool {
    c[0] > 150 && c[1] < 90 && c[2] < 90
}

fn greenish(c: [u8; 3]) -> bool {
    c[1] > 150 && c[0] < 120 && c[2] < 120
}

#[test]
fn the_draw_items_carry_the_shader_and_the_label_flags() {
    let (mut p, ids) = play();
    let items = &inputs_mut(&mut p).items;
    let item = |name: &str| {
        items
            .iter()
            .find(|i| i.node_id == ids[name])
            .unwrap_or_else(|| panic!("{name} is drawn"))
    };
    // The custom shader reaches the draw item AND transpiles for the GPU.
    let mat = item("Stripes").shader_material.as_ref().expect("Stripes has its ShaderMaterial");
    let module = gdrender3d::spatial_wgsl::module_for(mat)
        .expect("a spatial shader")
        .unwrap_or_else(|e| panic!("stripes.gdshader must transpile: {e}"));
    assert!(module.render_modes.unshaded);
    assert!(!module.writes_alpha);
    assert_eq!(module.uniforms.len(), 2, "stripe_color, stripes");
    assert!(module.uniforms[0].source_color);
    assert!(!gdeditor::viewport_3d_render::has_gpu_unsupported_spatial_shader(
        inputs_mut(&mut p)
    ));

    let items = &inputs_mut(&mut p).items;
    let item = |name: &str| items.iter().find(|i| i.node_id == ids[name]).unwrap();
    let xray = item("XRay").material.as_ref().expect("label material");
    assert!(xray.no_depth_test, "player.tscn-style `no_depth_test = true`");
    assert_eq!(xray.cull_mode, CullMode::Disabled, "double_sided defaults to true");
    let back = item("Backface").material.as_ref().expect("label material");
    assert!(!back.double_sided);
    assert_eq!(back.cull_mode, CullMode::Back, "authored `double_sided = false`");
}

#[test]
fn the_gpu_play_frame_matches_godot_and_each_fix_is_load_bearing() {
    let (mut p, ids) = play();
    let before = gdeditor::viewport_3d_render::software_fallback_frames();
    let frame = p.render_world();
    assert_eq!(
        gdeditor::viewport_3d_render::software_fallback_frames(),
        before,
        "the fixture must render on the GPU path, not the software fallback"
    );
    assert_eq!((frame.width, frame.height), (W, H));

    // -- custom shader: 8 bands across the quad, lower half darker ---------
    // Godot (f00005.png): the quad spans x 69..152, y 78..161.
    let row = |y: u32| -> Vec<[u8; 3]> { (72..150).map(|x| px(&frame, x, y)).collect() };
    let top = row(98);
    let bottom = row(140);
    let transitions = top
        .windows(2)
        .filter(|w| w[0].iter().zip(w[1]).any(|(a, b)| (*a as i32 - b as i32).abs() > 40))
        .count();
    assert!(
        (6..=8).contains(&transitions),
        "4 stripes = 7 band edges across the quad, got {transitions} in {top:?}"
    );
    // Same column, lower half: the `if (UV.y > 0.5) c *= 0.5` branch (UV.y
    // grows DOWN in Godot).
    let (t, b) = (top[5], bottom[5]);
    assert!(
        b.iter().zip(t).all(|(lo, hi)| *lo < hi || hi < 10),
        "lower half is darker: top {t:?} bottom {b:?}"
    );
    // Leftmost band is the blue base (Godot: (72,127,239)-ish).
    assert!(t[2] > 180 && t[0] < 120, "leftmost band is the blue base, got {t:?}");

    // -- whole-frame differential: drop only the ShaderMaterial ------------
    // Each control mutates ONE field of the same snapshot, renders, and puts
    // it back (the inputs are not `Clone`: a compositor carries callbacks).
    let mut saved = None;
    edit(&mut p, ids["Stripes"], |i| saved = i.shader_material.take());
    let flat_frame = p.render_world();
    edit(&mut p, ids["Stripes"], |i| i.shader_material = saved.take());
    assert!(
        diff(&frame, &flat_frame, 60, 160, 70, 170) > 2000,
        "the shader must change the quad"
    );
    assert_eq!(
        diff(&frame, &flat_frame, 170, W, 0, H),
        0,
        "control: nothing right of the quad depends on its material"
    );

    // -- no_depth_test: XRAY red pixels over the grey wall -----------------
    let red_in_wall = |f: &gdrender2d::renderer::Bgra8Image| {
        (192..266).flat_map(|x| (86..156).map(move |y| (x, y))).filter(|&(x, y)| reddish(px(f, x, y))).count()
    };
    edit(&mut p, ids["XRay"], |i| i.material.as_mut().unwrap().no_depth_test = false);
    let tested_frame = p.render_world();
    edit(&mut p, ids["XRay"], |i| i.material.as_mut().unwrap().no_depth_test = true);
    assert!(red_in_wall(&frame) > 40, "XRAY draws over the wall, as in Godot");
    // pat-3ljz4: the text is the real TTF face at `font_size` = 64 px EM, one
    // texel per font pixel under `pixel_size`, so the glyphs land where
    // Godot's do. Godot (f00005.png, `r>150 g<90 b<90`): bbox x 198..229,
    // y 116..124. The 5x7 bitmap path drew wider, taller cells (bbox
    // 193..235 x 113..126).
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 60..180 {
        for x in 180..300 {
            if reddish(px(&frame, x, y)) {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    for (got, want, what) in [(x0, 198, "left"), (x1, 229, "right"), (y0, 116, "top"), (y1, 124, "bottom")] {
        assert!(
            (got as i32 - want).abs() <= 2,
            "XRAY {what} edge at {got}, Godot {want} (bbox {x0}..{x1} x {y0}..{y1})"
        );
    }
    assert_eq!(red_in_wall(&tested_frame), 0, "control: depth-tested, the wall hides it");

    // -- double_sided = false: HIDDEN is culled from behind ----------------
    let green = |f: &gdrender2d::renderer::Bgra8Image| {
        (0..W).flat_map(|x| (0..80).map(move |y| (x, y))).filter(|&(x, y)| greenish(px(f, x, y))).count()
    };
    assert_eq!(green(&frame), 0, "Godot shows no HIDDEN label");
    edit(&mut p, ids["Backface"], |i| {
        let m = i.material.as_mut().unwrap();
        m.double_sided = true;
        m.cull_mode = CullMode::Disabled;
    });
    assert!(green(&p.render_world()) > 40, "control: double-sided, it shows");
}

#[test]
fn the_platformer_coin_counter_is_single_sided_and_ignores_depth() {
    // The unmodified game file: CoinCount and its four Parallax children.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d/player/player.tscn");
    let src = std::fs::read_to_string(path).expect("player.tscn");
    // Only the Label3D nodes: the robot's glTF instance needs the project.
    let mut tscn = String::from("[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node3D\"]\n");
    for block in src.split("\n[").filter(|b| b.contains("type=\"Label3D\"")) {
        let body: String = block
            .lines()
            .skip(1)
            .filter(|l| !l.starts_with("transform") && !l.starts_with("unique_name"))
            .map(|l| format!("{l}\n"))
            .collect();
        let name = block.split('"').nth(1).unwrap();
        tscn.push_str(&format!("\n[node name=\"{name}\" type=\"Label3D\" parent=\".\"]\n{body}"));
    }
    gdobject::class_db::register_all_classes();
    let scene = gdscene::packed_scene::PackedScene::from_tscn(&tscn).expect("parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).expect("instances");
    let labels: Vec<_> = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .filter(|&n| tree.get_node(n).is_some_and(|x| x.class_name() == "Label3D"))
        .collect();
    assert_eq!(labels.len(), 5, "CoinCount + Parallax 1-4");
    for n in labels {
        let (_, m) = gdeditor::sprite3d_render::label3d_geometry(&tree, n).expect("has text");
        assert_eq!(m.cull_mode, CullMode::Back, "player.tscn authors double_sided = false");
        assert!(m.no_depth_test);
    }
}
