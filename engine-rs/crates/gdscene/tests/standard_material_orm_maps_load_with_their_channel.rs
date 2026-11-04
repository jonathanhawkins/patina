//! A StandardMaterial3D's `roughness_texture` / `metallic_texture` reach the
//! renderer, read from the channel Godot reads (`*_texture_channel`, default
//! RED — material.cpp:3977-3978, `ROUGHNESS = roughness_tex * roughness` with
//! `roughness_tex = dot(texel, roughness_texture_channel)`).
//!
//! # What was wrong
//! The two slots were parsed and then never loaded, so every authored map was
//! silently ignored; and the renderers read G/B (glTF/ORM packing), so even a
//! loaded map would have read the wrong channel for a StandardMaterial3D.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::{material3d_for_node, MaterialTextureCache};
use gdscene::scene_tree::SceneTree;

fn write_png(path: &std::path::Path, rgb: [u8; 3]) {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
    img.save(path).expect("png written");
}

fn material_for(dir: &std::path::Path, tres_body: &str) -> gdserver3d::material::Material3D {
    std::fs::write(
        dir.join("mat.tres"),
        format!("[gd_resource type=\"StandardMaterial3D\" format=3]\n\n[resource]\n{tres_body}"),
    )
    .unwrap();
    gdcore::file_access::set_path_roots(Some(dir.to_path_buf()), None);
    let tscn = "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Material\" path=\"res://mat.tres\" id=\"1_mat\"]\n\n[node name=\"Root\" type=\"Node3D\"]\n\n[node name=\"Mesh\" type=\"MeshInstance3D\" parent=\".\"]\nmaterial_override = ExtResource(\"1_mat\")\n";
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let mesh = tree.get_node_or_null(scene_root, "Mesh").unwrap();
    material3d_for_node(&tree, mesh, &mut MaterialTextureCache::new()).expect("material resolves")
}

#[test]
fn roughness_and_metallic_maps_load_from_godots_channels() {
    let dir = std::env::temp_dir().join(format!("patina_orm_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // R=51 (0.2), G=204 (0.8), B=128 (~0.502).
    write_png(&dir.join("orm.png"), [51, 204, 128]);
    let to_lin = |v: f32| v; // maps are linear data; the decoder must not sRGB them

    // Default channels: RED for both.
    let m = material_for(&dir, "roughness_texture = \"res://orm.png\"\nmetallic_texture = \"res://orm.png\"\n");
    let r = m.roughness_texture.as_ref().expect("roughness map must be LOADED, not dropped");
    let mt = m.metallic_texture.as_ref().expect("metallic map must be LOADED");
    // Renderers read roughness from G and metallic from B.
    assert!((r.pixels[0].g - to_lin(0.2)).abs() < 0.01, "default roughness channel is RED, got {:?}", r.pixels[0]);
    assert!((mt.pixels[0].b - to_lin(0.2)).abs() < 0.01, "default metallic channel is RED, got {:?}", mt.pixels[0]);

    // Explicit channels: roughness GREEN (1), metallic BLUE (2) — glTF/ORM packing.
    let m = material_for(
        &dir,
        "roughness_texture = \"res://orm.png\"\nroughness_texture_channel = 1\nmetallic_texture = \"res://orm.png\"\nmetallic_texture_channel = 2\n",
    );
    assert!((m.roughness_texture.unwrap().pixels[0].g - 0.8).abs() < 0.01);
    assert!((m.metallic_texture.unwrap().pixels[0].b - 0.502).abs() < 0.01);
}
