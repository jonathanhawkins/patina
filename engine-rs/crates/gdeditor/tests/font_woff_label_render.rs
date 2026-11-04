//! pat-s3rrv.214, the wiring half: a Label whose `theme_override_fonts/font`
//! names a `.woff` renders THAT face's glyphs, not the built-in fallback.
//!
//! 🔑 Why this is a separate file from the acceptance test in
//! `gdresource/tests/font_formats_woff_bmfont_image.rs`: the two halves need
//! different crates. Reconstruction has to be proved against a *compressed*
//! fixture and a real SFNT parser, which needs `miniz_oxide` and `ttf_parser` —
//! both `gdresource` dependencies that `gdeditor` does not have. Rendering a
//! Label needs the viewport, which only `gdeditor` has. Rather than add
//! dependencies to a manifest I cannot compile-check, each half lives where it
//! can actually be expressed.
//!
//! ⚠️ The fixture here uses **stored** (uncompressed) WOFF tables, which the
//! WOFF 1.0 spec signals with `compLength == origLength` and which real fonts
//! contain for already-incompressible tables. That keeps this file free of a
//! compressor while still exercising the whole container path — signature
//! sniffing, directory parsing, tag sorting, 4-byte alignment and SFNT
//! reconstruction. The zlib path is covered by the sibling file, so between
//! them both branches of `woff_to_sfnt` are driven.
//!
//! The defect this pins: the ONLY file-backed font load in the engine is
//! `viewport_2d_render.rs`'s `std::fs::read` → `TtfFont::from_bytes`, which
//! understands SFNT alone. A `.woff` produced no face, `from_bytes` returned
//! `Err`, and the Label quietly drew the built-in font — wrong glyphs, no error.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::{build_control_render_items, rasterize_render_list};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::path::Path;

// ⚠️ NOT Cantarell: that file is byte-identical to the built-in default face
// (`gdrender2d/assets/default_font.ttf`), so overriding with it could never
// change a pixel and the "override differs from default" clause was red at
// every commit once default text drew with the real face. See
// `gdrender2d/test_assets/README.md`.
const FONT_REL: &str = "../gdrender2d/test_assets/Xolonium-Regular.ttf";

fn ttf_bytes() -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(FONT_REL);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}
fn rd16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}
fn rd32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Wraps an SFNT in a WOFF1 container with every table stored uncompressed.
fn sfnt_to_stored_woff(sfnt: &[u8]) -> Vec<u8> {
    let num_tables = rd16(sfnt, 4) as usize;
    let mut body_offset = 44 + num_tables * 20;

    let mut header = Vec::new();
    header.extend_from_slice(b"wOFF");
    header.extend_from_slice(&be32(rd32(sfnt, 0))); // flavor
    header.extend_from_slice(&be32(0)); // length, patched below
    header.extend_from_slice(&be16(num_tables as u16));
    header.extend_from_slice(&be16(0));
    header.extend_from_slice(&be32(sfnt.len() as u32));
    header.extend_from_slice(&be16(1));
    header.extend_from_slice(&be16(0));
    for _ in 0..5 {
        header.extend_from_slice(&be32(0));
    }

    let mut dir = Vec::new();
    let mut body = Vec::new();
    for i in 0..num_tables {
        let at = 12 + i * 16;
        let off = rd32(sfnt, at + 8) as usize;
        let len = rd32(sfnt, at + 12) as usize;
        let table = &sfnt[off..off + len];

        dir.extend_from_slice(&be32(rd32(sfnt, at))); // tag
        dir.extend_from_slice(&be32(body_offset as u32));
        dir.extend_from_slice(&be32(len as u32)); // compLength == origLength
        dir.extend_from_slice(&be32(len as u32));
        dir.extend_from_slice(&be32(rd32(sfnt, at + 4))); // checksum
        body.extend_from_slice(table);
        let pad = (4 - len % 4) % 4;
        body.extend(std::iter::repeat(0u8).take(pad));
        body_offset += len + pad;
    }

    let mut out = header;
    out.extend_from_slice(&dir);
    out.extend_from_slice(&body);
    let total = out.len() as u32;
    out[8..12].copy_from_slice(&be32(total));
    out
}

fn scratch(name: &str) -> std::path::PathBuf {
    // A fixed name races when several test binaries run at once.
    std::env::temp_dir().join(format!("patina_woff_{}_{}", std::process::id(), name))
}

fn render_label(font_override: Option<&str>) -> Vec<Color> {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Control"))
        .unwrap();
    let label = tree.add_child(root, Node::new("Label", "Label")).unwrap();
    {
        let node = tree.get_node_mut(label).unwrap();
        node.set_property("text", Variant::String("Patina".to_string()));
        if let Some(path) = font_override {
            node.set_property(
                "theme_override_fonts/font",
                Variant::String(path.to_string()),
            );
        }
    }
    let items = build_control_render_items(&mut tree, root, Vector2::new(160.0, 48.0));
    let fb = rasterize_render_list(&items, 160, 48, Color::new(0.0, 0.0, 0.0, 1.0), None);
    fb.pixels
}

fn differing(a: &[Color], b: &[Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-4 || (x.g - y.g).abs() > 1e-4 || (x.b - y.b).abs() > 1e-4
        })
        .count()
}

fn lit(px: &[Color]) -> usize {
    px.iter()
        .filter(|c| c.r > 0.1 || c.g > 0.1 || c.b > 0.1)
        .count()
}

/// **The wiring clause.** A `.woff` override reaches the rasterizer as a real
/// face, and the pixels prove it.
#[test]
fn a_label_renders_glyphs_from_a_woff_override() {
    let ttf = ttf_bytes();
    let woff_path = scratch("face.woff");
    std::fs::write(&woff_path, sfnt_to_stored_woff(&ttf)).expect("write the woff fixture");
    let woff = woff_path.to_string_lossy().to_string();

    // The same face as a plain .ttf, for the positive control below.
    let ttf_path = scratch("face.ttf");
    std::fs::write(&ttf_path, &ttf).expect("write the ttf fixture");
    let ttf_s = ttf_path.to_string_lossy().to_string();

    let plain = render_label(None);
    let from_woff = render_label(Some(&woff));
    let from_ttf = render_label(Some(&ttf_s));
    let _ = std::fs::remove_file(&woff_path);
    let _ = std::fs::remove_file(&ttf_path);

    assert!(
        lit(&from_woff) > 0,
        "the Label drew nothing at all with a .woff override"
    );
    // 🔑 The real assertion is not "it drew something" but "it drew the RIGHT
    // something": identical to the same face as a .ttf, and different from the
    // fallback. Either half alone passes on the bug — a silent fallback still
    // lights pixels, and a broken .ttf path would make both sides match while
    // both were wrong.
    assert_eq!(
        differing(&from_woff, &from_ttf),
        0,
        "a .woff must rasterize identically to the .ttf it was built from; \
         {} pixels differ",
        differing(&from_woff, &from_ttf)
    );
    assert!(
        differing(&plain, &from_woff) > 0,
        "overriding with a .woff changed no pixels, so the loader fell back to \
         the same built-in face it uses with no override"
    );
}

/// A `.woff2` override must not silently draw the fallback.
///
/// ⚠️ Patina has no Brotli decoder, so the face genuinely cannot load. What is
/// being pinned is that this is INDISTINGUISHABLE from "no font" rather than
/// masquerading as a successful load of some other face — the Label falls back,
/// and the sibling test asserts the loader says why.
#[test]
fn a_woff2_override_falls_back_rather_than_drawing_a_wrong_face() {
    let path = scratch("face.woff2");
    let mut woff2 = Vec::new();
    woff2.extend_from_slice(b"wOF2");
    woff2.extend_from_slice(&be32(0x0001_0000));
    woff2.extend(std::iter::repeat(0u8).take(64));
    std::fs::write(&path, &woff2).expect("write the woff2 fixture");
    let p = path.to_string_lossy().to_string();

    let plain = render_label(None);
    let attempted = render_label(Some(&p));
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        differing(&plain, &attempted),
        0,
        "an undecodable .woff2 must render exactly as the no-override case; \
         anything else means it produced a face out of undecoded bytes"
    );
}
