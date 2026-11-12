//! pat-null9: two of truck_town's models drew untextured.
//!
//! - The tree trunk's extracted `tree_material.tres` names
//!   `tree_material_baseColor.jpeg`, and Patina had no JPEG decoder: the load
//!   failed the PNG magic check and the trunk drew in flat white.
//! - The lamp's `scene.gltf` (`materials/extract=0`) carries its own
//!   `images[0].uri = textures/Palette_baseColor.png` as `baseColorTexture`.
//!   The import recorded it as `material_albedo_texture` and nothing read it,
//!   so the lamp drew in its flat white `baseColorFactor`.
//!
//! The expected texels are real Godot 4.6.1's:
//! `Image.load_from_file(<jpeg>).get_pixelv(p)` (`r8 g8 b8`).

use gdscene::render_server_3d::{gltf_surface_materials, MaterialTextureCache};

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

fn roots() {
    gdcore::file_access::set_path_roots(Some(game_dir()), None);
}

fn rgb8(c: gdcore::math::Color) -> [i32; 3] {
    [c.r, c.g, c.b].map(|v| (v * 255.0).round() as i32)
}

/// Godot's decode of the trunk JPEG, at four texels.
const GODOT_JPEG_TEXELS: [((u32, u32), [i32; 3]); 4] = [
    ((0, 0), [19, 17, 22]),
    ((10, 20), [61, 56, 62]),
    ((100, 100), [137, 129, 140]),
    ((1023, 1023), [113, 117, 128]),
];

#[test]
fn the_tree_trunk_jpeg_decodes_to_godots_texels() {
    roots();
    let mut cache = MaterialTextureCache::new();
    let tex = cache
        .get_or_load("res://town/tree/textures/tree_material_baseColor.jpeg")
        .expect("the trunk JPEG decodes");
    assert_eq!((tex.width, tex.height), (1024, 1024), "Godot: 1024x1024");
    for ((x, y), want) in GODOT_JPEG_TEXELS {
        let got = rgb8(tex.pixels[(y * tex.width + x) as usize]);
        // libjpeg-turbo (Godot) and zune-jpeg round the IDCT/upsampling
        // differently; 3/255 is well inside one JPEG quantisation step.
        for ch in 0..3 {
            assert!(
                (got[ch] - want[ch]).abs() <= 3,
                "texel ({x},{y}): Patina {got:?}, Godot {want:?}"
            );
        }
    }
}

#[test]
fn the_tree_trunk_material_carries_its_jpeg_albedo_texture() {
    roots();
    // Mesh 0 is the trunk (glTF material 0, `tree_material`, extracted to
    // `tree_material.tres` with `albedo_texture = <the jpeg>`).
    let mats = gltf_surface_materials("res://town/tree/scene.gltf", Some(0));
    assert_eq!(mats.len(), 1, "the trunk mesh has one surface");
    let tex = mats[0]
        .albedo_texture
        .as_ref()
        .expect("Godot draws the trunk with tree_material_baseColor.jpeg");
    assert_eq!((tex.width, tex.height), (1024, 1024));
}

#[test]
fn the_lamp_gltf_material_uses_its_own_base_color_image() {
    roots();
    let mats = gltf_surface_materials("res://town/lamp/scene.gltf", Some(0));
    assert!(!mats.is_empty(), "the lamp mesh has surfaces");
    let tex = mats[0]
        .albedo_texture
        .as_ref()
        .expect("Godot's importer sets albedo_texture from baseColorTexture");
    // textures/Palette_baseColor.png is an 8x8 palette.
    assert_eq!((tex.width, tex.height), (8, 8));
    // The palette is not flat white: a texture that is read draws colours.
    let mut cache = MaterialTextureCache::new();
    let direct = cache
        .get_or_load("res://town/lamp/textures/Palette_baseColor.png")
        .expect("the palette PNG decodes");
    assert_eq!(
        tex.pixels, direct.pixels,
        "the image the glTF names, resolved relative to it"
    );
}
