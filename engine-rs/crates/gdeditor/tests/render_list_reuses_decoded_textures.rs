//! pat-p3p1m: the 3D render list (editor viewport AND Play) must not re-decode
//! or re-rasterize textures on every frame.
//!
//! * `build_render_list` created a fresh `MaterialTextureCache` per call, so
//!   every material texture was decoded from disk on every frame, under the
//!   editor-state lock.
//! * `label3d_geometry` re-rasterized (and outline-dilated) the Label3D text
//!   on every frame — a quarter of the 3D platformer demo's Play frame CPU.
//!
//! Reuse is observable as `Arc` identity: the same texture comes back. A cache
//! that never notices a CHANGE would be worse than none, so both halves are
//! checked.

use std::sync::Arc;

use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::Render3DItem;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

fn load(tscn: &str) -> SceneTree {
    gdobject::class_db::register_3d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn albedo_texture(items: &[Render3DItem]) -> Arc<gdserver3d::material::TextureImage> {
    items
        .iter()
        .find_map(|i| i.material.as_ref().and_then(|m| m.albedo_texture.clone()))
        .expect("an item with an albedo texture")
}

/// A 2x2 PNG, solid `rgb`.
fn write_png(path: &std::path::Path, rgb: [u8; 3]) {
    let c = gdcore::math::Color::new(
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
        1.0,
    );
    let fb = gdrender2d::FrameBuffer::new(2, 2, c);
    std::fs::write(path, gdrender2d::export::encode_png(&fb)).unwrap();
}

#[test]
fn a_material_texture_is_decoded_once_and_again_only_when_its_file_changes() {
    let dir = std::env::temp_dir().join(format!("patina-p3p1m-tex-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("albedo.png");
    write_png(&png, [255, 0, 0]);
    std::fs::write(
        dir.join("mat.tres"),
        "[gd_resource type=\"StandardMaterial3D\" format=3]\n\n[resource]\nalbedo_texture = \"res://albedo.png\"\n",
    )
    .unwrap();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let tscn = "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Material\" path=\"res://mat.tres\" id=\"1_mat\"]\n\n[node name=\"Root\" type=\"Node3D\"]\n\n[node name=\"Box\" type=\"MeshInstance3D\" parent=\".\"]\nmesh = \"primitive:BoxMesh\"\nmaterial_override = ExtResource(\"1_mat\")\n".to_string();
    let tree = load(&tscn);
    let vp = Viewport3D::new(64, 64);

    let first = albedo_texture(&vp.build_render_list(&tree));
    let second = albedo_texture(&vp.build_render_list(&tree));
    assert!(
        Arc::ptr_eq(&first, &second),
        "the second frame must reuse the decoded texture, not decode the file again"
    );
    assert!(
        first.pixels[0].r > 0.9 && first.pixels[0].g < 0.1,
        "fixture: red texture"
    );

    // Rewrite the file (with a different mtime): the next frame decodes it.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_png(&png, [0, 0, 255]);
    let third = albedo_texture(&vp.build_render_list(&tree));
    assert!(
        !Arc::ptr_eq(&first, &third),
        "a changed file must be decoded again"
    );
    assert!(
        third.pixels[0].b > 0.9 && third.pixels[0].r < 0.1,
        "the re-decoded texture must have the new pixels: {:?}",
        third.pixels[0]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn label3d_text_is_rasterized_once_per_distinct_label() {
    let scene = |text: &str| {
        format!(
            "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node3D\"]\n\n[node name=\"Label\" type=\"Label3D\" parent=\".\"]\ntext = \"{text}\"\nfont_size = 48\noutline_size = 12\n"
        )
    };
    let tree = load(&scene("You have found\\na secret area!"));
    let id = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|&n| {
            tree.get_node(n)
                .map(|x| x.name() == "Label")
                .unwrap_or(false)
        })
        .unwrap();
    let a = gdeditor::sprite3d_render::label3d_geometry(&tree, id)
        .unwrap()
        .1;
    let b = gdeditor::sprite3d_render::label3d_geometry(&tree, id)
        .unwrap()
        .1;
    let (ta, tb) = (a.albedo_texture.unwrap(), b.albedo_texture.unwrap());
    assert!(
        Arc::ptr_eq(&ta, &tb),
        "an unchanged Label3D must reuse its text texture"
    );

    let other = load(&scene("Different"));
    let oid = other
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|&n| {
            other
                .get_node(n)
                .map(|x| x.name() == "Label")
                .unwrap_or(false)
        })
        .unwrap();
    let c = gdeditor::sprite3d_render::label3d_geometry(&other, oid)
        .unwrap()
        .1;
    let tc = c.albedo_texture.unwrap();
    assert!(
        !Arc::ptr_eq(&ta, &tc),
        "different text must rasterize its own texture"
    );
    assert_ne!((ta.width, ta.height), (tc.width, tc.height));
}
