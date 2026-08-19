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

    // BPTC_RGBA / BC7 (ordinal 22) — still undecoded: eight block modes with
    // partition tables, and no reference decoder vendored to port from.
    //
    // ⚠️ This case used to use ETC2_RGBA8, which pat-s3rrv.48 has since
    // implemented. A test that pins "format X is unsupported" goes stale the
    // moment X is delivered, so it must name a format that is *actually* still
    // missing — otherwise it fails for the best possible reason and reads like
    // a regression.
    let payload = vec![0u8; 16];
    write_remapped(root, "bc7", &build_ctex_blocks(4, 4, 22, 0, &payload));

    let loader = UnifiedLoader::new(root);
    let err = match loader.load("res://bc7.png") {
        Ok(_) => panic!("BC7 has no decoder, so this must not claim to have loaded"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("BPTC_RGBA") || err.contains("BC7"),
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
/// ETC2 RGB8 blocks, one per encoding mode, with the pixels
/// `upstream/godot/thirdparty/etcpak/DecodeRGB.cpp` produces for them.
///
/// The expected values were generated by transcribing that same C++ into Python
/// and executing it — a second, independent transcription of the decoder this
/// module ports, so a slip in either one shows up here as a mismatch instead of
/// as silently wrong pixels. (`etcpak` is the encoder/decoder Godot itself
/// ships, so these are the bytes Godot would produce.)
#[allow(clippy::type_complexity)]
const ETC2_RGB_VECTORS: [(&str, [u8; 8], [[u8; 4]; 16]); 5] = [
    (
        "individual",
        [0xFA, 0xDF, 0x4B, 0xA1, 0xD1, 0x59, 0xC0, 0x27],
        [
            [175, 141, 0, 255],
            [231, 197, 44, 255],
            [231, 197, 44, 255],
            [231, 197, 44, 255],
            [255, 255, 148, 255],
            [255, 255, 148, 255],
            [255, 245, 92, 255],
            [255, 245, 92, 255],
            [178, 255, 195, 255],
            [168, 253, 185, 255],
            [172, 255, 189, 255],
            [162, 247, 179, 255],
            [168, 253, 185, 255],
            [172, 255, 189, 255],
            [172, 255, 189, 255],
            [162, 247, 179, 255],
        ],
    ),
    (
        "differential",
        [0x9D, 0x97, 0xD8, 0x86, 0x40, 0x8C, 0xCC, 0x2D],
        [
            [216, 208, 255, 255],
            [174, 166, 240, 255],
            [137, 145, 227, 255],
            [137, 145, 227, 255],
            [174, 166, 240, 255],
            [216, 208, 255, 255],
            [137, 145, 227, 255],
            [137, 145, 227, 255],
            [96, 88, 162, 255],
            [174, 166, 240, 255],
            [149, 157, 239, 255],
            [115, 123, 205, 255],
            [96, 88, 162, 255],
            [138, 130, 204, 255],
            [149, 157, 239, 255],
            [149, 157, 239, 255],
        ],
    ),
    (
        "t_mode",
        [0x14, 0x41, 0x58, 0x46, 0xC5, 0xF8, 0xE7, 0xE6],
        [
            [136, 68, 17, 255],
            [85, 136, 68, 255],
            [74, 125, 57, 255],
            [136, 68, 17, 255],
            [96, 147, 79, 255],
            [74, 125, 57, 255],
            [96, 147, 79, 255],
            [96, 147, 79, 255],
            [96, 147, 79, 255],
            [74, 125, 57, 255],
            [74, 125, 57, 255],
            [74, 125, 57, 255],
            [85, 136, 68, 255],
            [74, 125, 57, 255],
            [136, 68, 17, 255],
            [74, 125, 57, 255],
        ],
    ),
    (
        "h_mode",
        [0xA2, 0x14, 0x4B, 0xBF, 0xCF, 0x11, 0x0F, 0x01],
        [
            [112, 78, 78, 255],
            [194, 160, 160, 255],
            [112, 78, 78, 255],
            [109, 126, 41, 255],
            [109, 126, 41, 255],
            [109, 126, 41, 255],
            [112, 78, 78, 255],
            [109, 126, 41, 255],
            [109, 126, 41, 255],
            [109, 126, 41, 255],
            [112, 78, 78, 255],
            [194, 160, 160, 255],
            [109, 126, 41, 255],
            [109, 126, 41, 255],
            [112, 78, 78, 255],
            [194, 160, 160, 255],
        ],
    ),
    (
        "planar",
        [0xB9, 0x74, 0x06, 0xBA, 0xB0, 0xE6, 0xAE, 0x90],
        [
            [113, 245, 20, 255],
            [113, 228, 43, 255],
            [113, 211, 67, 255],
            [113, 194, 90, 255],
            [139, 213, 31, 255],
            [139, 196, 55, 255],
            [139, 179, 78, 255],
            [139, 162, 101, 255],
            [164, 181, 43, 255],
            [164, 164, 66, 255],
            [164, 147, 89, 255],
            [164, 130, 112, 255],
            [190, 148, 54, 255],
            [190, 131, 77, 255],
            [190, 114, 100, 255],
            [190, 97, 124, 255],
        ],
    ),
];

/// An ETC2 RGBA8 block: the EAC alpha word followed by the colour word.
const ETC2_RGBA_BLOCK: [u8; 16] = [
    0x7F, 0x4B, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0x9D, 0x97, 0xD8, 0x86, 0x40, 0x8C, 0xCC, 0x2D,
];
const ETC2_RGBA_PIXELS: [[u8; 4]; 16] = [
    [216, 208, 255, 119],
    [174, 166, 240, 107],
    [137, 145, 227, 87],
    [137, 145, 227, 131],
    [174, 166, 240, 119],
    [216, 208, 255, 143],
    [137, 145, 227, 107],
    [137, 145, 227, 151],
    [96, 88, 162, 99],
    [174, 166, 240, 119],
    [149, 157, 239, 163],
    [115, 123, 205, 143],
    [96, 88, 162, 99],
    [138, 130, 204, 143],
    [149, 157, 239, 119],
    [149, 157, 239, 87],
];

/// An EAC R11 block, decompressed to R8 the way Godot stores it.
const EAC_R11_BLOCK: [u8; 8] = [0xA3, 0x5F, 0x0F, 0x1E, 0x2D, 0x3C, 0x4B, 0x5A];
const EAC_R11_PIXELS: [[u8; 4]; 16] = [
    [148, 0, 0, 255],
    [203, 0, 0, 255],
    [138, 0, 0, 255],
    [183, 0, 0, 255],
    [118, 0, 0, 255],
    [148, 0, 0, 255],
    [203, 0, 0, 255],
    [183, 0, 0, 255],
    [193, 0, 0, 255],
    [183, 0, 0, 255],
    [148, 0, 0, 255],
    [118, 0, 0, 255],
    [138, 0, 0, 255],
    [183, 0, 0, 255],
    [173, 0, 0, 255],
    [128, 0, 0, 255],
];

/// An EAC RG11 block: the red word followed by the green word.
const EAC_RG11_BLOCK: [u8; 16] = [
    0xA3, 0x5F, 0x0F, 0x1E, 0x2D, 0x3C, 0x4B, 0x5A, 0x40, 0xE1, 0x12, 0x23, 0x34, 0x45, 0x56, 0x67,
];
const EAC_RG11_PIXELS: [[u8; 4]; 16] = [
    [148, 22, 0, 255],
    [203, 0, 0, 255],
    [138, 0, 0, 255],
    [183, 0, 0, 255],
    [118, 92, 0, 255],
    [148, 92, 0, 255],
    [203, 0, 0, 255],
    [183, 0, 0, 255],
    [193, 92, 0, 255],
    [183, 190, 0, 255],
    [148, 0, 0, 255],
    [118, 92, 0, 255],
    [138, 0, 0, 255],
    [183, 92, 0, 255],
    [173, 148, 0, 255],
    [128, 232, 0, 255],
];

/// `Image::Format::FORMAT_ETC2_RGB8` — ordinal 30.
const IMG_FORMAT_ETC2_RGB8: u32 = 30;
/// `Image::Format::FORMAT_ETC2_RGBA8` — ordinal 31.
const IMG_FORMAT_ETC2_RGBA8: u32 = 31;
/// `Image::Format::FORMAT_ETC2_R11` — ordinal 26.
const IMG_FORMAT_ETC2_R11: u32 = 26;
/// `Image::Format::FORMAT_ETC2_RG11` — ordinal 28.
const IMG_FORMAT_ETC2_RG11: u32 = 28;

/// Loads a single-block `.ctex` of `img_format` through the unified loader.
fn load_single_block(stem: &str, img_format: u32, payload: &[u8]) -> Vec<Color> {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");
    write_remapped(root, stem, &build_ctex_blocks(4, 4, img_format, 0, payload));
    let tex = UnifiedLoader::new(root)
        .load(&format!("res://{stem}.png"))
        .unwrap_or_else(|e| panic!("{stem} .ctex must load through the unified loader: {e:?}"));
    assert_eq!((int_prop(&tex, "width"), int_prop(&tex, "height")), (4, 4));
    let px = pixels_of(&tex);
    assert_eq!(px.len(), 16, "one block expands to 16 texels");
    px
}

/// **ETC2 RGB8 through the loader, one case per block mode** (pat-s3rrv.48).
///
/// ETC2 is Godot's Web-export VRAM format, and it is the mode dispatch — not
/// the arithmetic — that makes it easy to get wrong: an out-of-range
/// differential on R, G or B escapes to the T, H or planar encoding instead,
/// and each of those reads the block's bits completely differently. All five
/// paths are exercised here, so a decoder that handles only the common
/// differential block fails four of them rather than passing by luck.
#[test]
fn ctex_etc2_rgb8_decodes_every_block_mode() {
    for (name, block, expected) in ETC2_RGB_VECTORS {
        let px = load_single_block(&format!("etc2_{name}"), IMG_FORMAT_ETC2_RGB8, &block);
        for (i, want) in expected.iter().enumerate() {
            assert_rgba(
                px[i],
                want[0],
                want[1],
                want[2],
                want[3],
                &format!("ETC2 {name} texel {i}"),
            );
        }
    }
}

/// ETC2 RGBA8: the same colour block plus a per-texel EAC alpha word.
///
/// The colours are identical to the `differential` case above — the alpha word
/// is the only difference — so a decoder that ignored the EAC half would still
/// produce the right RGB and fail only on alpha, which is exactly the assertion
/// that catches it.
#[test]
fn ctex_etc2_rgba8_decodes_with_eac_alpha() {
    let px = load_single_block("etc2_rgba", IMG_FORMAT_ETC2_RGBA8, &ETC2_RGBA_BLOCK);
    let mut varied = false;
    for (i, want) in ETC2_RGBA_PIXELS.iter().enumerate() {
        assert_rgba(
            px[i],
            want[0],
            want[1],
            want[2],
            want[3],
            &format!("ETC2 RGBA8 texel {i}"),
        );
        if want[3] != ETC2_RGBA_PIXELS[0][3] {
            varied = true;
        }
    }
    assert!(
        varied,
        "the fixture must carry a VARYING alpha, or an all-opaque decoder would pass"
    );
}

/// EAC R11 and RG11, the single- and dual-channel formats Godot decompresses to
/// R8 and RG8.
#[test]
fn ctex_eac_r11_and_rg11_decode_through_the_loader() {
    let r = load_single_block("eac_r11", IMG_FORMAT_ETC2_R11, &EAC_R11_BLOCK);
    for (i, want) in EAC_R11_PIXELS.iter().enumerate() {
        assert_rgba(
            r[i],
            want[0],
            want[1],
            want[2],
            want[3],
            &format!("EAC R11 texel {i}"),
        );
    }

    let rg = load_single_block("eac_rg11", IMG_FORMAT_ETC2_RG11, &EAC_RG11_BLOCK);
    for (i, want) in EAC_RG11_PIXELS.iter().enumerate() {
        assert_rgba(
            rg[i],
            want[0],
            want[1],
            want[2],
            want[3],
            &format!("EAC RG11 texel {i}"),
        );
    }
    // The red word is shared with the R11 case, so red must read identically
    // and green must not be a copy of it.
    assert!(
        EAC_RG11_PIXELS.iter().any(|p| p[1] != p[0]),
        "the RG11 fixture must have a green channel distinct from red"
    );
}

// ---------------------------------------------------------------------------
// Layered and volume artifacts (pat-s3rrv.48)
// ---------------------------------------------------------------------------

/// Builds one `.ctex` image blob — the self-delimiting unit a layered artifact
/// repeats per layer, identical to what follows a plain `.ctex`'s own header.
fn image_blob(width: u16, height: u16, img_format: u32, payload: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&DATA_FORMAT_IMAGE.to_le_bytes());
    d.extend_from_slice(&width.to_le_bytes());
    d.extend_from_slice(&height.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // mip levels beyond level 0
    d.extend_from_slice(&img_format.to_le_bytes());
    d.extend_from_slice(payload);
    d
}

/// Builds a `GSTL` layered/volume artifact around already-built image blobs.
///
/// Mirrors `ResourceImporterLayeredTexture::_save_tex`
/// (`editor/import/resource_importer_layered_texture.cpp:170`): a 36-byte
/// header whose second word is the layer count (or depth) and whose sixth is
/// the number of extra volume-mipmap slices, then the blobs back to back.
fn build_gstl(count: u32, mode: u32, extra_mipmaps: u32, blobs: &[Vec<u8>]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(b"GSTL");
    d.extend_from_slice(&0u32.to_le_bytes()); // format version
    d.extend_from_slice(&count.to_le_bytes());
    d.extend_from_slice(&mode.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&extra_mipmaps.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    for b in blobs {
        d.extend_from_slice(b);
    }
    d
}

/// A 4x4 BC1 blob whose every texel is `color0`.
fn bc1_layer(color0: u16) -> Vec<u8> {
    image_blob(4, 4, IMG_FORMAT_DXT1, &bc1_block_solid(color0))
}

fn layer_of(res: &Resource, i: usize) -> &Resource {
    res.subresources
        .get(&format!("layer_{i}"))
        .unwrap_or_else(|| panic!("layer_{i} must be stored as a subresource"))
}

/// **A `.ctexarray` decodes EVERY layer, not just the first** (pat-s3rrv.48).
///
/// A layered artifact is one short header followed by N whole image blobs, so
/// the parser has to leave the cursor exactly at the next layer's first byte.
/// Giving each layer a different colour is what makes an off-by-one in that
/// walk visible: a parser that re-read layer 0 three times, or drifted into the
/// middle of a blob, cannot produce red/green/blue in order.
#[test]
fn ctexarray_decodes_every_layer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    let blobs = vec![
        bc1_layer(RGB565_RED),
        bc1_layer(RGB565_GREEN),
        bc1_layer(RGB565_BLUE),
    ];
    std::fs::write(root.join("slices.ctexarray"), build_gstl(3, 0, 0, &blobs)).expect("artifact");

    let tex = UnifiedLoader::new(root)
        .load("res://slices.ctexarray")
        .expect("a .ctexarray must load through the unified loader");

    assert_eq!(tex.class_name, "CompressedTexture2DArray");
    assert_eq!(int_prop(&tex, "layers"), 3, "the header's layer count");
    assert_eq!((int_prop(&tex, "width"), int_prop(&tex, "height")), (4, 4));

    // Layer 0's pixels are also on the resource itself, so a consumer that only
    // understands Texture2D still sees an image.
    assert_rgba(pixels_of(&tex)[0], 255, 0, 0, 255, "array level pixel");

    for (i, (r, g, b)) in [(255, 0, 0), (0, 255, 0), (0, 0, 255)].iter().enumerate() {
        let layer = layer_of(&tex, i);
        let px = pixels_of(layer);
        assert_eq!(px.len(), 16, "layer {i} expands to 16 texels");
        for (n, c) in px.iter().enumerate() {
            assert_rgba(*c, *r, *g, *b, 255, &format!("layer {i} texel {n}"));
        }
    }
}

/// A `.ccube` is the same container with six layers and a different mode word.
#[test]
fn ccube_decodes_six_faces() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    let blobs: Vec<Vec<u8>> = (0..6).map(|_| bc1_layer(RGB565_GREEN)).collect();
    std::fs::write(root.join("sky.ccube"), build_gstl(6, 1, 0, &blobs)).expect("artifact");

    let tex = UnifiedLoader::new(root)
        .load("res://sky.ccube")
        .expect("a .ccube must load through the unified loader");
    assert_eq!(tex.class_name, "CompressedCubemap");
    assert_eq!(int_prop(&tex, "layers"), 6, "a cubemap is six faces");
    assert_rgba(
        pixels_of(layer_of(&tex, 5))[0],
        0,
        255,
        0,
        255,
        "the sixth face must be decoded, not skipped",
    );
}

/// A `.ctex3d` reports `depth` and carries its extra mipmap slices after the
/// volume's own slices.
#[test]
fn ctex3d_decodes_slices_and_volume_mipmaps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    // Depth 2 plus one half-size mipmap slice: 3 blobs in total, and the
    // trailing one must be consumed or a truncated read would go unnoticed.
    let blobs = vec![
        bc1_layer(RGB565_RED),
        bc1_layer(RGB565_BLUE),
        image_blob(2, 2, IMG_FORMAT_DXT1, &bc1_block_solid(RGB565_GREEN)),
    ];
    std::fs::write(root.join("fog.ctex3d"), build_gstl(2, 3, 1, &blobs)).expect("artifact");

    let tex = UnifiedLoader::new(root)
        .load("res://fog.ctex3d")
        .expect("a .ctex3d must load through the unified loader");
    assert_eq!(tex.class_name, "CompressedTexture3D");
    assert_eq!(int_prop(&tex, "depth"), 2, "a volume reports depth");
    assert_eq!(int_prop(&tex, "mipmap_slices"), 1);
    assert_rgba(
        pixels_of(layer_of(&tex, 1))[0],
        0,
        0,
        255,
        255,
        "the second depth slice",
    );
}

/// A file whose header declares a different mode than its extension promises is
/// a mislabelled artifact, and must be reported rather than decoded as whatever
/// it happens to contain.
#[test]
fn layered_mode_must_match_the_extension() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    // Mode 1 (cubemap) written into a file named `.ctexarray`.
    let blobs = vec![bc1_layer(RGB565_RED)];
    std::fs::write(root.join("wrong.ctexarray"), build_gstl(1, 1, 0, &blobs)).expect("artifact");

    let err = match UnifiedLoader::new(root).load("res://wrong.ctexarray") {
        Ok(_) => panic!("a mode/extension mismatch must not load"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("mode"),
        "the error must say the mode disagreed, got: {err}"
    );
}

/// Following a `.import` remap must reach a layered artifact too — the sidecar
/// path is how a real project references one.
#[test]
fn import_remap_reaches_a_layered_artifact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");

    std::fs::write(root.join("atlas.png"), b"NOT-A-REAL-PNG-SOURCE").expect("source");
    std::fs::write(
        root.join("atlas.png.import"),
        "[remap]\nimporter=\"2d_array_texture\"\ntype=\"CompressedTexture2DArray\"\npath=\"res://.godot/imported/atlas.ctexarray\"\n",
    )
    .expect("sidecar");
    let imported = root.join(".godot/imported");
    std::fs::create_dir_all(&imported).expect("imported dir");
    let blobs = vec![bc1_layer(RGB565_RED), bc1_layer(RGB565_BLUE)];
    std::fs::write(
        imported.join("atlas.ctexarray"),
        build_gstl(2, 0, 0, &blobs),
    )
    .expect("artifact");

    let tex = UnifiedLoader::new(root)
        .load("res://atlas.png")
        .expect("the sidecar must remap the source to its layered artifact");
    assert_eq!(tex.class_name, "CompressedTexture2DArray");
    assert_eq!(int_prop(&tex, "layers"), 2);
}
