//! pat-s3rrv.48 — VRAM-compressed `.ctex` artifacts decode to pixels.
//!
//! `compress/mode = VRAM Compressed` is the **default** of Godot's "3D" texture
//! preset (`editor/import/resource_importer_texture.cpp`), so most 3D textures
//! in a real project are stored as block-compressed `DATA_FORMAT_IMAGE`. Patina
//! accepted only `L8`/`LA8`/`RGB8`/`RGBA8` and returned `Unsupported` for every
//! block format — while `gdresource::block_compression` had shipped working
//! `decode_bc1`/`decode_bc3` with **zero callers**, its own module header saying
//! the `.ctex` loader ought to call them.
//!
//! # Why this loads through `UnifiedLoader` and not `load_ctex_bytes`
//!
//! A direct `decode_bc1` unit call proves the decoder, which was never the gap.
//! The gap was the `.ctex` loader refusing the format, so the wiring under test
//! is: `.import` sidecar → remap to the `.godot/imported/*.ctex` artifact →
//! `load_ctex_bytes` → block decode → a `pixels` array a renderer can upload.
//! Every assertion below therefore goes through `UnifiedLoader::load`.
//!
//! # The fixtures are hand-built blocks with known colours
//!
//! Both fixtures use index 0 for every texel, so each block decodes to exactly
//! its `color0` and the expected pixels are arithmetic rather than a golden
//! blob: an RGB565 channel expands by replicating its high bits into the low
//! ones, so 5-bit `31` becomes `31 << 3 | 31 >> 2 == 255`.

use gdcore::math::Color;
use gdresource::importers::UnifiedLoader;
use gdresource::Resource;
use gdvariant::Variant;

/// `FORMAT_BIT_HAS_MIPMAPS` (`scene/resources/compressed_texture.h:56`).
const FORMAT_BIT_HAS_MIPMAPS: u32 = 1 << 23;

/// `Image::Format::FORMAT_DXT1` — ordinal 17 counting from `FORMAT_L8 = 0`.
const IMG_FORMAT_DXT1: u32 = 17;
/// `Image::Format::FORMAT_DXT5` — ordinal 19.
const IMG_FORMAT_DXT5: u32 = 19;
/// `Image::Format::FORMAT_RGTC_R` (BC4) — ordinal 20.
const IMG_FORMAT_RGTC_R: u32 = 20;
/// `Image::Format::FORMAT_DXT3` (BC2) — ordinal 18.
const IMG_FORMAT_DXT3: u32 = 18;

/// `DATA_FORMAT_IMAGE`.
const DATA_FORMAT_IMAGE: u32 = 0;

/// Builds a `GST2` `.ctex` around an already-encoded block payload.
///
/// `mipmaps` counts levels **beyond** level 0, matching the field Godot writes;
/// `payload` must already contain every level back to back, because
/// `DATA_FORMAT_IMAGE` levels carry no per-level size prefix (unlike the
/// PNG/WebP arms, which length-frame each one).
fn build_ctex_blocks(
    width: u32,
    height: u32,
    img_format: u32,
    mipmaps: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(b"GST2");
    d.extend_from_slice(&1u32.to_le_bytes()); // version
    d.extend_from_slice(&width.to_le_bytes());
    d.extend_from_slice(&height.to_le_bytes());
    let df = if mipmaps > 0 {
        FORMAT_BIT_HAS_MIPMAPS
    } else {
        0
    };
    d.extend_from_slice(&df.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // mipmap_limit
    d.extend_from_slice(&[0u8; 12]); // 3 reserved u32
                                     // Inner image header.
    d.extend_from_slice(&DATA_FORMAT_IMAGE.to_le_bytes());
    d.extend_from_slice(&(width as u16).to_le_bytes());
    d.extend_from_slice(&(height as u16).to_le_bytes());
    d.extend_from_slice(&mipmaps.to_le_bytes());
    d.extend_from_slice(&img_format.to_le_bytes());
    d.extend_from_slice(payload);
    d
}

/// One BC1 block (8 bytes) whose every texel is `color0`.
///
/// `color0 > color1` selects the opaque 4-colour mode; all-zero indices then
/// select `color0` for all 16 texels. Passing `color1 = 0` keeps the ordering
/// true for any non-zero `color0`.
fn bc1_block_solid(color0: u16) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[0..2].copy_from_slice(&color0.to_le_bytes());
    b[2..4].copy_from_slice(&0u16.to_le_bytes()); // color1 = black
                                                  // b[4..8] stay zero: index 0 for every texel.
    b
}

/// One BC3 block (16 bytes): opaque alpha over a solid `color0` colour block.
///
/// Bytes 0..8 are the BC4-style alpha block — `alpha0 = 255`, `alpha1 = 0`, and
/// all-zero 3-bit indices select `alpha0`. Bytes 8..16 are a BC1 colour block,
/// which BC3 always reads in 4-colour mode regardless of endpoint order.
fn bc3_block_solid(color0: u16) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = 255; // alpha0
    b[1] = 0; // alpha1
              // b[2..8] stay zero: alpha index 0 -> alpha0 -> fully opaque.
    b[8..16].copy_from_slice(&bc1_block_solid(color0));
    b
}

// RGB565 endpoints. Each expands to a saturated 8-bit channel.
/// Pure red: R=31, G=0, B=0.
const RGB565_RED: u16 = 0b11111_000000_00000;
/// Pure green: R=0, G=63, B=0.
const RGB565_GREEN: u16 = 0b00000_111111_00000;
/// Pure blue: R=0, G=0, B=31.
const RGB565_BLUE: u16 = 0b00000_000000_11111;

fn int_prop(res: &Resource, key: &str) -> i64 {
    match res.get_property(key) {
        Some(Variant::Int(v)) => *v,
        other => panic!("expected Int for {key}, got {other:?}"),
    }
}

fn pixels_of(res: &Resource) -> Vec<Color> {
    match res.get_property("pixels") {
        Some(Variant::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Variant::Color(c) => Some(*c),
                _ => None,
            })
            .collect(),
        other => panic!("expected a pixels Array, got {other:?}"),
    }
}

/// Asserts a colour matches the given 0-255 channels, with a tolerance that
/// only admits rounding in the 5/6-bit expansion.
fn assert_rgba(actual: Color, r: u8, g: u8, b: u8, a: u8, what: &str) {
    let want = [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ];
    let got = [actual.r, actual.g, actual.b, actual.a];
    for (i, chan) in ["r", "g", "b", "a"].iter().enumerate() {
        assert!(
            (got[i] - want[i]).abs() < 0.01,
            "{what}: channel {chan} expected {} got {} (full colour {actual:?})",
            want[i],
            got[i],
        );
    }
}

/// Writes a source file plus the `.import` sidecar that remaps it to `artifact`,
/// and the artifact itself.
///
/// The source bytes are deliberately not a decodable image: if the loader ever
/// stopped following the sidecar and decoded the source instead, these tests
/// would fail loudly rather than silently testing the wrong path.
fn write_remapped(root: &std::path::Path, stem: &str, ctex: &[u8]) {
    std::fs::write(root.join(format!("{stem}.png")), b"NOT-A-REAL-PNG-SOURCE").expect("source");
    std::fs::write(
        root.join(format!("{stem}.png.import")),
        format!(
            "[remap]\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\npath=\"res://.godot/imported/{stem}.ctex\"\n"
        ),
    )
    .expect("sidecar");
    let imported = root.join(".godot/imported");
    std::fs::create_dir_all(&imported).expect("imported dir");
    std::fs::write(imported.join(format!("{stem}.ctex")), ctex).expect("ctex artifact");
}

/// **The acceptance test.**
#[test]
fn ctex_vram_compressed_decodes_to_pixels() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    // -- DXT1 (BC1), 8x4 = two blocks side by side ----------------------------
    //
    // Two *different* block colours, because a single-colour image would still
    // pass if the decoder ignored block coordinates entirely.
    let mut dxt1 = Vec::new();
    dxt1.extend_from_slice(&bc1_block_solid(RGB565_RED)); // block (0,0)
    dxt1.extend_from_slice(&bc1_block_solid(RGB565_GREEN)); // block (1,0)
    assert_eq!(dxt1.len(), 16, "8x4 DXT1 is 2 blocks of 8 bytes");
    write_remapped(
        root,
        "tex",
        &build_ctex_blocks(8, 4, IMG_FORMAT_DXT1, 0, &dxt1),
    );

    let loader = UnifiedLoader::new(root);
    let tex = loader
        .load("res://tex.png")
        .expect("a VRAM-compressed DXT1 .ctex must load through the unified loader");

    assert_eq!(tex.class_name, "Texture2D");
    assert_eq!(
        (int_prop(&tex, "width"), int_prop(&tex, "height")),
        (8, 4),
        "dimensions must come from the .ctex header",
    );
    let px = pixels_of(&tex);
    assert_eq!(
        px.len(),
        8 * 4,
        "a block-compressed texture must expand to one colour per texel, not one per block",
    );
    // Left block red, right block green, on every row.
    for row in 0..4usize {
        assert_rgba(
            px[row * 8],
            255,
            0,
            0,
            255,
            &format!("row {row} left block (DXT1 color0 = red)"),
        );
        assert_rgba(
            px[row * 8 + 4],
            0,
            255,
            0,
            255,
            &format!("row {row} right block (DXT1 color0 = green)"),
        );
    }
    assert_eq!(
        int_prop(&tex, "mipmap_count"),
        1,
        "a chain-less texture still has its own level",
    );

    // -- DXT5 (BC3), 8x8 with one extra mip level -----------------------------
    //
    // Level 0 is 2x2 blocks (64 bytes); level 1 is 4x4 texels — ONE block (16
    // bytes), because a block format's mip chain floors at 4x4 rather than 1x1
    // (`Image::get_format_min_pixel_size`). A loader that halved to 2x2 and
    // expected 4 bytes would desync here and mis-read the level.
    let mut dxt5 = Vec::new();
    for _ in 0..4 {
        dxt5.extend_from_slice(&bc3_block_solid(RGB565_BLUE)); // level 0: 2x2 blocks
    }
    dxt5.extend_from_slice(&bc3_block_solid(RGB565_BLUE)); // level 1: 4x4 = 1 block
    assert_eq!(dxt5.len(), 80, "64 bytes of level 0 plus 16 of level 1");
    write_remapped(
        root,
        "tex2",
        &build_ctex_blocks(8, 8, IMG_FORMAT_DXT5, 1, &dxt5),
    );

    let tex2 = loader
        .load("res://tex2.png")
        .expect("a VRAM-compressed DXT5 .ctex with a mip chain must load");
    assert_eq!(
        (int_prop(&tex2, "width"), int_prop(&tex2, "height")),
        (8, 8),
    );
    let px2 = pixels_of(&tex2);
    assert_eq!(px2.len(), 8 * 8, "level 0 supplies the pixel array");
    // BC3's alpha ramp is independent of the colour block: alpha0 = 255 with
    // index 0 must come through opaque. A decoder that ignored the alpha block
    // would leave these transparent.
    for (i, c) in px2.iter().enumerate() {
        assert_rgba(*c, 0, 0, 255, 255, &format!("DXT5 texel {i}"));
    }
    assert_eq!(
        int_prop(&tex2, "mipmap_count"),
        2,
        "one extra level plus level 0 is two levels in total",
    );
}

/// BC4 reaches the same `.ctex` path (pat-s3rrv.48).
///
/// `block_compression` has its own unit tests for the BC4 ramp, but those prove
/// the *decoder*; they would still pass if `.ctex`'s format table never routed
/// `RGTC_R` to it. This asserts the table entry, which is where the gap was.
#[test]
fn ctex_bc4_single_channel_decodes_through_the_loader() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    // One 4x4 BC4 block: e0 = 255 > e1 = 0 (8-value mode), every index 0, so
    // every texel takes e0. Godot decompresses RGTC_R to R8, whose pixel reads
    // are (r, 0, 0, 1) — blue must be zero and alpha opaque.
    let mut block = [0u8; 8];
    block[0] = 255; // e0
    write_remapped(
        root,
        "bc4",
        &build_ctex_blocks(4, 4, IMG_FORMAT_RGTC_R, 0, &block),
    );

    let loader = UnifiedLoader::new(root);
    let tex = loader
        .load("res://bc4.png")
        .expect("a BC4 .ctex must load through the unified loader");
    assert_eq!((int_prop(&tex, "width"), int_prop(&tex, "height")), (4, 4),);
    let px = pixels_of(&tex);
    assert_eq!(px.len(), 16, "one block expands to 16 texels");
    for (i, c) in px.iter().enumerate() {
        assert_rgba(*c, 255, 0, 0, 255, &format!("BC4 texel {i}"));
    }
}

/// BC2 reaches the same `.ctex` path (pat-s3rrv.48).
///
/// Same reasoning as the BC4 case: `block_compression` unit-tests the decoder,
/// but the bug this bead exists to fix was a decoder with no caller, so the
/// format-table entry needs its own end-to-end assertion.
#[test]
fn ctex_bc2_explicit_alpha_decodes_through_the_loader() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    // One 4x4 BC2 block: every alpha nibble 15 (-> 255), colour c0 = red with
    // all indices 0. Alpha is explicit per texel here, not a ramp.
    let mut block = [0u8; 16];
    block[0..8].copy_from_slice(&[0xFF; 8]);
    block[8] = 0x00;
    block[9] = 0xF8; // c0 = RGB565 red
    write_remapped(
        root,
        "bc2",
        &build_ctex_blocks(4, 4, IMG_FORMAT_DXT3, 0, &block),
    );

    let loader = UnifiedLoader::new(root);
    let tex = loader
        .load("res://bc2.png")
        .expect("a BC2 .ctex must load through the unified loader");
    let px = pixels_of(&tex);
    assert_eq!(px.len(), 16, "one block expands to 16 texels");
    for (i, c) in px.iter().enumerate() {
        assert_rgba(*c, 255, 0, 0, 255, &format!("BC2 texel {i}"));
    }
}

/// A format with no decoder must say *which* format it was.
///
/// The old message lumped every non-8-bit format into "only L8/LA8/RGB8/RGBA8",
/// so a project that failed to load gave no clue which encoder produced it.
#[test]
fn undecodable_ctex_format_names_itself() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    // ETC2_RGBA8 (ordinal 31) — Godot's Web-export VRAM format, still undecoded.
    let payload = vec![0u8; 16];
    write_remapped(root, "etc", &build_ctex_blocks(4, 4, 31, 0, &payload));

    let loader = UnifiedLoader::new(root);
    let err = match loader.load("res://etc.png") {
        Ok(_) => panic!("ETC2_RGBA8 has no decoder, so this must not claim to have loaded"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("ETC2_RGBA8"),
        "the error must name the format so the failure is actionable, got: {err}"
    );
}

/// A file whose header promises a mip chain it does not carry is corrupt, and
/// must be reported rather than silently yielding level 0.
#[test]
fn truncated_mip_chain_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    // Declares one extra level but supplies only level 0's four blocks.
    let mut short = Vec::new();
    for _ in 0..4 {
        short.extend_from_slice(&bc3_block_solid(RGB565_BLUE));
    }
    write_remapped(
        root,
        "short",
        &build_ctex_blocks(8, 8, IMG_FORMAT_DXT5, 1, &short),
    );

    let loader = UnifiedLoader::new(root);
    assert!(
        loader.load("res://short.png").is_err(),
        "a mip level the header declares but the file lacks must be an error, \
         not a texture with a silent hole",
    );
}
