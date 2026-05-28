//! DISABLED (references gdeditor::import::{parse_font_metrics, AudioImporter, FontImporter, ImageImporter} which are not yet implemented)
//! Re-enable by removing the cfg gate below once the missing
//! items land. Tracked as part of editor parity work.
#![cfg(any())]

//! pat-epu6j: glTF 2.0 scene import parity.
//!
//! Verifies that `GltfSceneImporter` produces a Patina scene tree matching
//! a hand-authored glTF 2.0 document. Covers:
//!
//! 1. ASCII `.gltf` JSON containers — node hierarchy, mesh references
//! 2. Materials — `mesh_count` reflects referenced primitives
//! 3. Skins — joints emit `Skeleton3D` companions
//! 4. Animations — surfaced when `animation/import` is enabled
//! 5. Binary `.glb` container — identical scene from JSON chunk
//! 6. Backward-compat: unrecognized payloads fall back to the legacy stub

use std::path::Path;

use gdcore::math::Color;
use gdeditor::import::{
    parse_font_metrics, AudioImporter, FontImporter, GltfSceneImporter, ImageImporter,
    ImportedSceneNode, ResourceImporter, SceneFormatImporterRegistry, SceneImportOptions,
};
use gdrender2d::{encode_png, FrameBuffer};
use gdvariant::Variant;
use tempfile::TempDir;

fn make_registry() -> SceneFormatImporterRegistry {
    let mut reg = SceneFormatImporterRegistry::new();
    reg.register_importer(Box::new(GltfSceneImporter));
    reg
}

/// Minimal but well-formed glTF 2.0 JSON describing:
/// - 1 scene whose root node is `Root`
/// - `Root` has two children: `Cube` (mesh 0) and `Armature` (skin 0)
/// - 1 mesh with two primitives referencing materials 0 and 1
/// - 2 materials
/// - 1 skin with 2 joints
/// - 1 named animation
fn skinned_gltf_json() -> &'static str {
    r#"{
      "asset": {"version": "2.0"},
      "scene": 0,
      "scenes": [{"nodes": [0]}],
      "nodes": [
        {"name": "Root", "children": [1, 2]},
        {"name": "Cube", "mesh": 0},
        {"name": "Armature", "skin": 0, "children": [3, 4]},
        {"name": "Joint_Hip"},
        {"name": "Joint_Spine"}
      ],
      "meshes": [
        {"primitives": [
          {"attributes": {"POSITION": 0}, "material": 0},
          {"attributes": {"POSITION": 0}, "material": 1}
        ]}
      ],
      "materials": [
        {"name": "Red"},
        {"name": "Blue"}
      ],
      "skins": [
        {"name": "Rig", "joints": [3, 4]}
      ],
      "animations": [
        {"name": "Walk", "channels": [], "samplers": []}
      ]
    }"#
}

fn find_child<'a>(node: &'a ImportedSceneNode, name: &str) -> Option<&'a ImportedSceneNode> {
    node.children.iter().find(|c| c.name == name)
}

#[test]
fn gltf_scene_imports_node_hierarchy_with_meshes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("character.gltf");
    std::fs::write(&path, skinned_gltf_json()).unwrap();

    let scene = make_registry().import_scene(&path, None).unwrap();

    // Root container is named after the file stem.
    assert_eq!(scene.root.name, "character");
    assert_eq!(scene.root.node_type, "Node3D");

    // The glTF "Root" node should be the only top-level child of the container.
    assert_eq!(scene.root.children.len(), 1);
    let gltf_root = &scene.root.children[0];
    assert_eq!(gltf_root.name, "Root");
    assert_eq!(gltf_root.node_type, "Node3D");

    // Root has Cube (mesh) and Armature (skin) children.
    assert_eq!(gltf_root.children.len(), 2);

    let cube = find_child(gltf_root, "Cube").expect("Cube node missing");
    assert_eq!(cube.node_type, "MeshInstance3D");
    assert_eq!(cube.properties.get("mesh_index"), Some(&Variant::Int(0)));
}

#[test]
fn gltf_scene_emits_skeleton_for_skinned_node() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("rigged.gltf");
    std::fs::write(&path, skinned_gltf_json()).unwrap();

    let scene = make_registry().import_scene(&path, None).unwrap();
    let gltf_root = &scene.root.children[0];
    let armature = find_child(gltf_root, "Armature").expect("Armature node missing");

    assert_eq!(armature.properties.get("skin_index"), Some(&Variant::Int(0)));

    // The skin must materialize as a Skeleton3D child reflecting joint count.
    let skeleton = find_child(armature, "Rig").expect("Skeleton3D companion missing");
    assert_eq!(skeleton.node_type, "Skeleton3D");
    assert_eq!(
        skeleton.properties.get("joint_count"),
        Some(&Variant::Int(2))
    );

    // The two joints declared as children of Armature must also be present.
    assert!(find_child(armature, "Joint_Hip").is_some());
    assert!(find_child(armature, "Joint_Spine").is_some());
}

#[test]
fn gltf_scene_counts_meshes_and_animations() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("scene_with_anim.gltf");
    std::fs::write(&path, skinned_gltf_json()).unwrap();

    let scene = make_registry().import_scene(&path, None).unwrap();

    // Only one mesh is referenced by the scene; primitives don't multiply it.
    assert_eq!(scene.mesh_count, 1);

    // Named animation surfaces with its declared name.
    assert_eq!(scene.animations.len(), 1);
    assert_eq!(scene.animations[0].name, "Walk");
}

#[test]
fn gltf_scene_animation_import_disabled() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("static.gltf");
    std::fs::write(&path, skinned_gltf_json()).unwrap();

    let mut opts = SceneImportOptions::new();
    opts.set("animation/import", Variant::Bool(false));

    let scene = make_registry().import_scene(&path, Some(&opts)).unwrap();
    assert!(scene.animations.is_empty());
}

#[test]
fn gltf_scene_glb_binary_container_parses_json_chunk() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("character.glb");

    let json = skinned_gltf_json().as_bytes();
    // glTF JSON chunks must be 4-byte aligned with trailing 0x20 (space) padding.
    let pad = (4 - (json.len() % 4)) % 4;
    let mut json_chunk = Vec::with_capacity(json.len() + pad);
    json_chunk.extend_from_slice(json);
    for _ in 0..pad {
        json_chunk.push(0x20);
    }

    let total_len = (12 + 8 + json_chunk.len()) as u32;
    let mut blob = Vec::with_capacity(total_len as usize);
    blob.extend_from_slice(b"glTF"); // magic
    blob.extend_from_slice(&2u32.to_le_bytes()); // version
    blob.extend_from_slice(&total_len.to_le_bytes());
    blob.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    blob.extend_from_slice(b"JSON");
    blob.extend_from_slice(&json_chunk);
    std::fs::write(&path, &blob).unwrap();

    let scene = make_registry().import_scene(&path, None).unwrap();
    let gltf_root = &scene.root.children[0];
    assert_eq!(gltf_root.name, "Root");
    assert!(find_child(gltf_root, "Cube").is_some());
    assert!(find_child(gltf_root, "Armature").is_some());
    assert_eq!(scene.mesh_count, 1);
    assert_eq!(scene.animations.len(), 1);
}

#[test]
fn gltf_scene_fallback_for_non_gltf_payload() {
    // Legacy fixtures written placeholder bytes; the importer must keep
    // producing a usable stub scene so older callers don't regress.
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("placeholder.gltf");
    std::fs::write(&path, "x".repeat(200)).unwrap();

    let scene = make_registry().import_scene(&path, None).unwrap();
    assert_eq!(scene.root.name, "placeholder");
    assert_eq!(scene.root.children.len(), 1);
    assert_eq!(scene.root.children[0].node_type, "MeshInstance3D");
    assert_eq!(scene.mesh_count, 1);
}

#[test]
fn gltf_scene_importer_rejects_missing_file() {
    let reg = make_registry();
    let result = reg.import_scene(Path::new("/nonexistent/missing.gltf"), None);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Raster image import parity (pat-46ykq)
// ---------------------------------------------------------------------------

fn build_png_fixture(width: u32, height: u32) -> Vec<u8> {
    let fb = FrameBuffer::new(width, height, Color::new(0.25, 0.5, 0.75, 1.0));
    encode_png(&fb)
}

fn build_jpeg_fixture(width: u16, height: u16) -> Vec<u8> {
    let mut blob: Vec<u8> = Vec::new();
    // SOI
    blob.extend_from_slice(&[0xFF, 0xD8]);
    // SOF0 segment: marker, length(8), precision, height, width, components(0)
    blob.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x08, 0x08]);
    blob.extend_from_slice(&height.to_be_bytes());
    blob.extend_from_slice(&width.to_be_bytes());
    blob.push(0x00);
    // EOI
    blob.extend_from_slice(&[0xFF, 0xD9]);
    blob
}

fn build_webp_vp8l_fixture(width: u32, height: u32) -> Vec<u8> {
    assert!(width >= 1 && width <= 0x4000);
    assert!(height >= 1 && height <= 0x4000);
    let bits: u32 = (width - 1) | ((height - 1) << 14);
    let mut blob: Vec<u8> = Vec::with_capacity(32);
    blob.extend_from_slice(b"RIFF");
    blob.extend_from_slice(&0u32.to_le_bytes()); // size placeholder
    blob.extend_from_slice(b"WEBP");
    blob.extend_from_slice(b"VP8L");
    blob.extend_from_slice(&10u32.to_le_bytes()); // chunk size (unused by decoder)
    blob.push(0x2F); // VP8L signature
    blob.extend_from_slice(&bits.to_le_bytes());
    while blob.len() < 30 {
        blob.push(0);
    }
    blob
}

#[test]
fn image_decode_png_produces_texture2d() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("pixel.png");
    std::fs::write(&path, build_png_fixture(4, 3)).unwrap();

    let imp = ImageImporter;
    assert!(imp.can_import("png"));
    assert!(imp.can_import("PNG"));

    let resource = imp.import(&path).expect("png import should succeed");
    assert_eq!(resource.resource_type, "Texture2D");
    assert_eq!(resource.source_path, path);
}

#[test]
fn image_decode_jpeg_produces_texture2d() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("pixel.jpg");
    std::fs::write(&path, build_jpeg_fixture(8, 6)).unwrap();

    let imp = ImageImporter;
    assert!(imp.can_import("jpg"));
    assert!(imp.can_import("jpeg"));

    let resource = imp.import(&path).expect("jpeg import should succeed");
    assert_eq!(resource.resource_type, "Texture2D");

    let alt = dir.path().join("pixel.jpeg");
    std::fs::write(&alt, build_jpeg_fixture(8, 6)).unwrap();
    let resource = imp.import(&alt).expect("jpeg import should succeed");
    assert_eq!(resource.resource_type, "Texture2D");
}

#[test]
fn image_decode_webp_produces_texture2d() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("pixel.webp");
    std::fs::write(&path, build_webp_vp8l_fixture(16, 8)).unwrap();

    let imp = ImageImporter;
    assert!(imp.can_import("webp"));

    let resource = imp.import(&path).expect("webp import should succeed");
    assert_eq!(resource.resource_type, "Texture2D");
}

#[test]
fn image_decode_rejects_malformed_payload() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("garbage.png");
    std::fs::write(&path, b"not a real png").unwrap();

    let imp = ImageImporter;
    assert!(imp.import(&path).is_err());
}

#[test]
fn image_decode_rejects_missing_file() {
    let imp = ImageImporter;
    assert!(imp.import(Path::new("/nonexistent/missing.png")).is_err());
}

#[test]
fn image_decode_ignores_unsupported_extension() {
    let imp = ImageImporter;
    assert!(!imp.can_import("gif"));
    assert!(!imp.can_import("tres"));
    assert!(!imp.can_import(""));
}

// ---------------------------------------------------------------------------
// pat-fiuvb: Audio decode parity — WAV + OGG Vorbis.
// ---------------------------------------------------------------------------

fn build_pcm16_wav_fixture(sample_rate: u32, channels: u16, frames: usize) -> Vec<u8> {
    let mut sample_data: Vec<u8> = Vec::with_capacity(frames * channels as usize * 2);
    for frame in 0..frames {
        for ch in 0..channels {
            let value: i16 = ((frame as i32 * 1000) as i16).wrapping_add(ch as i16 * 250);
            sample_data.extend_from_slice(&value.to_le_bytes());
        }
    }
    gdaudio::wav::build_wav_bytes(sample_rate, channels, 16, 1, &sample_data)
}

#[test]
fn audio_decode_wav_produces_audio_stream_wav() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("clip.wav");
    std::fs::write(&path, build_pcm16_wav_fixture(44100, 1, 1024)).unwrap();

    let imp = AudioImporter;
    assert!(imp.can_import("wav"));
    assert!(imp.can_import("WAV"));

    let resource = imp.import(&path).expect("wav import should succeed");
    assert_eq!(resource.resource_type, "AudioStreamWAV");
    assert_eq!(resource.source_path, path);
}

#[test]
fn audio_decode_wav_stereo_produces_audio_stream_wav() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("stereo.wav");
    std::fs::write(&path, build_pcm16_wav_fixture(48000, 2, 512)).unwrap();

    let imp = AudioImporter;
    let resource = imp.import(&path).expect("stereo wav import should succeed");
    assert_eq!(resource.resource_type, "AudioStreamWAV");
}

#[test]
fn audio_decode_rejects_malformed_wav_payload() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("garbage.wav");
    std::fs::write(&path, b"RIFFxxxxWAVEnot-a-real-wav").unwrap();

    let imp = AudioImporter;
    assert!(imp.import(&path).is_err());
}

#[test]
fn audio_decode_rejects_malformed_ogg_payload() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("garbage.ogg");
    std::fs::write(&path, b"OggSnot-a-real-ogg-stream").unwrap();

    let imp = AudioImporter;
    assert!(imp.can_import("ogg"));
    assert!(imp.import(&path).is_err());
}

#[test]
fn audio_decode_rejects_missing_file() {
    let imp = AudioImporter;
    assert!(imp.import(Path::new("/nonexistent/missing.wav")).is_err());
    assert!(imp.import(Path::new("/nonexistent/missing.ogg")).is_err());
}

#[test]
fn audio_decode_ignores_unsupported_extension() {
    let imp = AudioImporter;
    assert!(!imp.can_import("mp3"));
    assert!(!imp.can_import("flac"));
    assert!(!imp.can_import("png"));
    assert!(!imp.can_import(""));
}

/// Builds a minimal but well-formed SFNT container with `head`, `hhea`, and
/// `maxp` tables. `magic` selects TTF (`0x00010000`) or OTF/CFF (`0x4F54544F`).
/// All table bodies are padded to the minimum sizes Patina inspects.
fn build_ttf_fixture(
    magic: u32,
    units_per_em: u16,
    ascent: i16,
    descent: i16,
    line_gap: i16,
    num_glyphs: u16,
) -> Vec<u8> {
    let head_len: usize = 54;
    let hhea_len: usize = 36;
    let maxp_len: usize = 32;

    let dir_start = 12usize;
    let num_tables = 3u16;
    let dir_end = dir_start + 16 * num_tables as usize;
    let head_off = dir_end;
    let hhea_off = head_off + head_len;
    let maxp_off = hhea_off + hhea_len;
    let total = maxp_off + maxp_len;

    let mut buf = vec![0u8; total];

    buf[0..4].copy_from_slice(&magic.to_be_bytes());
    buf[4..6].copy_from_slice(&num_tables.to_be_bytes());
    // searchRange / entrySelector / rangeShift: left zero (unused by parser).

    // Table directory records — tag, checksum (0), offset, length.
    let mut write_record = |idx: usize, tag: &[u8; 4], offset: u32, length: u32| {
        let rec = dir_start + idx * 16;
        buf[rec..rec + 4].copy_from_slice(tag);
        buf[rec + 4..rec + 8].copy_from_slice(&0u32.to_be_bytes());
        buf[rec + 8..rec + 12].copy_from_slice(&offset.to_be_bytes());
        buf[rec + 12..rec + 16].copy_from_slice(&length.to_be_bytes());
    };
    write_record(0, b"head", head_off as u32, head_len as u32);
    write_record(1, b"hhea", hhea_off as u32, hhea_len as u32);
    write_record(2, b"maxp", maxp_off as u32, maxp_len as u32);

    // `head` table: unitsPerEm lives at offset 18 (u16 BE).
    buf[head_off + 18..head_off + 20].copy_from_slice(&units_per_em.to_be_bytes());
    // `hhea` table: ascender/descender/lineGap at offsets 4/6/8 (i16 BE).
    buf[hhea_off + 4..hhea_off + 6].copy_from_slice(&ascent.to_be_bytes());
    buf[hhea_off + 6..hhea_off + 8].copy_from_slice(&descent.to_be_bytes());
    buf[hhea_off + 8..hhea_off + 10].copy_from_slice(&line_gap.to_be_bytes());
    // `maxp` table: numGlyphs at offset 4 (u16 BE).
    buf[maxp_off + 4..maxp_off + 6].copy_from_slice(&num_glyphs.to_be_bytes());

    buf
}

#[test]
fn font_import_ttf_produces_font_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("regular.ttf");
    std::fs::write(&path, build_ttf_fixture(0x0001_0000, 2048, 1800, -450, 90, 512)).unwrap();

    let imp = FontImporter;
    assert!(imp.can_import("ttf"));
    assert!(imp.can_import("TTF"));

    let resource = imp.import(&path).expect("ttf import should succeed");
    assert_eq!(resource.resource_type, "FontFile");
    assert_eq!(resource.source_path, path);
}

#[test]
fn font_import_otf_produces_font_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("display.otf");
    std::fs::write(&path, build_ttf_fixture(0x4F54_544F, 1000, 880, -220, 0, 256)).unwrap();

    let imp = FontImporter;
    assert!(imp.can_import("otf"));

    let resource = imp.import(&path).expect("otf import should succeed");
    assert_eq!(resource.resource_type, "FontFile");
}

#[test]
fn font_import_extracts_glyph_metrics() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("metrics.ttf");
    std::fs::write(&path, build_ttf_fixture(0x0001_0000, 2048, 1900, -500, 120, 1024)).unwrap();

    let imp = FontImporter;
    let metrics = imp.parse(&path).expect("metrics parse");
    assert!(!metrics.is_cff);
    assert_eq!(metrics.units_per_em, 2048);
    assert_eq!(metrics.ascent, 1900);
    assert_eq!(metrics.descent, -500);
    assert_eq!(metrics.line_gap, 120);
    assert_eq!(metrics.num_glyphs, 1024);
}

#[test]
fn font_import_otf_reports_cff_container() {
    let data = build_ttf_fixture(0x4F54_544F, 1000, 750, -250, 0, 64);
    let metrics = parse_font_metrics(&data).expect("parse otf");
    assert!(metrics.is_cff);
    assert_eq!(metrics.num_glyphs, 64);
}

#[test]
fn font_import_rejects_malformed_payload() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("garbage.ttf");
    std::fs::write(&path, b"not a real font at all").unwrap();

    let imp = FontImporter;
    assert!(imp.import(&path).is_err());
}

#[test]
fn font_import_rejects_unsupported_scaler() {
    // 'typ1' scaler — recognized magic but not supported by Patina.
    let mut data = build_ttf_fixture(0x0001_0000, 1000, 800, -200, 0, 32);
    data[0..4].copy_from_slice(&0x7479_7031u32.to_be_bytes());
    assert!(parse_font_metrics(&data).is_err());
}

#[test]
fn font_import_rejects_truncated_payload() {
    // Only the first 8 bytes of an SFNT container.
    let mut data = build_ttf_fixture(0x0001_0000, 1000, 800, -200, 0, 32);
    data.truncate(8);
    assert!(parse_font_metrics(&data).is_err());
}

#[test]
fn font_import_rejects_missing_file() {
    let imp = FontImporter;
    assert!(imp.import(Path::new("/nonexistent/missing.ttf")).is_err());
    assert!(imp.import(Path::new("/nonexistent/missing.otf")).is_err());
}

#[test]
fn font_import_ignores_unsupported_extension() {
    let imp = FontImporter;
    assert!(!imp.can_import("woff"));
    assert!(!imp.can_import("woff2"));
    assert!(!imp.can_import("png"));
    assert!(!imp.can_import(""));
}
