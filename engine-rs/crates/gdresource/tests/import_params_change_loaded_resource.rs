//! pat-s3rrv.50 acceptance: a `.import` sidecar's `[params]` must change what
//! loading the asset produces.
//!
//! Godot persists every importer option per asset in the `.import` file's
//! `[params]` section, and the importer applies them when it builds the
//! artifact. Patina **parsed that section and threw it away**:
//! `ImportFile::other_sections` had zero readers outside its own module, so
//! `import_svg` rasterized at a hardcoded `1.0`, `import_wav` ignored every
//! `force/*` and `edit/*` option, and `import_image` ignored `process/*`. A
//! project's import settings were decoration.
//!
//! Every case here goes through `UnifiedLoader::load("res://…")` — the engine
//! -wide entry point — never into an option struct directly, so a regression
//! that leaves the params parsed-but-unread fails here.

use gdresource::importers::UnifiedLoader;
use gdresource::Resource;
use gdvariant::Variant;
use std::path::Path;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Fixture builders — no encoder crate is a dev-dependency of gdresource, so the
// PNG and WAV bytes are assembled here.
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for x in data {
        a = (a + *x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// A `w`×`h` 8-bit RGBA PNG whose IDAT uses **stored** (uncompressed) deflate
/// blocks — valid zlib that `miniz_oxide` inflates like any other, and simple
/// enough to emit without a compressor.
fn rgba_png(w: u32, h: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity((h * (1 + w * 4)) as usize);
    for y in 0..h {
        raw.push(0u8); // filter type 0 (None)
        for x in 0..w {
            // A gradient, so a resample has something to average and a
            // channel remap has something to move.
            raw.extend_from_slice(&[(x * 4) as u8, (y * 4) as u8, 128, 255]);
        }
    }

    let mut z = vec![0x78, 0x01]; // zlib header: deflate, 32K window, no dict
    let mut i = 0usize;
    while i < raw.len() {
        let n = (raw.len() - i).min(65_535);
        let last = if i + n >= raw.len() { 1u8 } else { 0u8 };
        z.push(last); // BFINAL + BTYPE=00 (stored)
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&raw[i..i + n]);
        i += n;
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    // bit depth 8, color type 6 (RGBA), deflate, adaptive filtering, no interlace
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &z);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

/// A silent 16-bit **stereo** PCM WAV.
fn stereo_wav(sample_rate: u32, frames: u32) -> Vec<u8> {
    let channels: u16 = 2;
    let bits: u16 = 16;
    let block_align = channels * bits / 8;
    let data_len = frames * block_align as u32;

    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    v.extend_from_slice(&vec![0u8; data_len as usize]);
    v
}

/// A 32×32 SVG: a red square. No text, so no font database is needed.
const RED_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><rect x="0" y="0" width="32" height="32" fill="#ff0000"/></svg>"##;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn int_prop(res: &Resource, key: &str) -> i64 {
    match res.get_property(key) {
        Some(Variant::Int(v)) => *v,
        other => panic!("expected Int for '{key}', got {other:?}"),
    }
}

fn pixel_count(res: &Resource) -> usize {
    match res.get_property("pixels") {
        Some(Variant::Array(a)) => a.len(),
        other => panic!("expected a pixels Array, got {other:?}"),
    }
}

fn first_pixel(res: &Resource) -> gdcore::math::Color {
    match res.get_property("pixels") {
        Some(Variant::Array(a)) => match a.to_vec().first() {
            Some(Variant::Color(c)) => *c,
            other => panic!("first pixel is not a Color: {other:?}"),
        },
        other => panic!("expected a pixels Array, got {other:?}"),
    }
}

/// Writes `contents` next to `asset` as `<asset>.import`.
fn write_sidecar(asset: &Path, params: &str) {
    let mut sidecar = asset.as_os_str().to_os_string();
    sidecar.push(".import");
    std::fs::write(
        Path::new(&sidecar),
        format!("[remap]\n\nimporter=\"texture\"\n\n[params]\n\n{params}\n"),
    )
    .expect("write .import sidecar");
}

fn load(dir: &Path, res_path: &str) -> Arc<Resource> {
    UnifiedLoader::new(dir)
        .load(res_path)
        .unwrap_or_else(|e| panic!("load {res_path}: {e:?}"))
}

// ---------------------------------------------------------------------------
// The acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** Three assets, three options, three observable
/// differences — all through `UnifiedLoader::load`.
#[test]
fn import_params_change_loaded_resource() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // -- svg/scale --------------------------------------------------------
    // Same bytes twice: one asset scaled 1.0, one scaled 2.0. `scale = 2.0`
    // doubles each axis, so the pixel count is 4×.
    let one = root.join("one.svg");
    let two = root.join("two.svg");
    std::fs::write(&one, RED_SVG).expect("write svg");
    std::fs::write(&two, RED_SVG).expect("write svg");
    write_sidecar(&one, "svg/scale=1.0");
    write_sidecar(&two, "svg/scale=2.0");

    let at_1x = load(root, "res://one.svg");
    let at_2x = load(root, "res://two.svg");
    assert_eq!(int_prop(&at_1x, "width"), 32, "scale 1.0 keeps the viewBox");
    assert_eq!(int_prop(&at_2x, "width"), 64, "scale 2.0 doubles each axis");
    assert_eq!(
        pixel_count(&at_2x),
        4 * pixel_count(&at_1x),
        "svg/scale=2.0 must yield 4× the pixels of svg/scale=1.0 — this was \
         hardcoded to 1.0, so both used to come back the same size"
    );

    // -- force/mono -------------------------------------------------------
    let wav = root.join("tone.wav");
    std::fs::write(&wav, stereo_wav(44_100, 1_000)).expect("write wav");
    write_sidecar(&wav, "force/mono=true");

    let mono = load(root, "res://tone.wav");
    assert_eq!(
        int_prop(&mono, "channels"),
        1,
        "force/mono=true must import a stereo source as one channel"
    );
    // The duration is a property of the recording, not of the channel count:
    // mixing down changes the sample count, never the length.
    match mono.get_property("length_seconds") {
        Some(Variant::Float(len)) => assert!(
            (len - 1000.0 / 44_100.0).abs() < 1e-6,
            "force/mono must not change the length, got {len}"
        ),
        other => panic!("length_seconds missing: {other:?}"),
    }

    // -- process/size_limit -----------------------------------------------
    let png = root.join("big.png");
    std::fs::write(&png, rgba_png(64, 64)).expect("write png");
    write_sidecar(&png, "process/size_limit=16");

    let limited = load(root, "res://big.png");
    assert_eq!(
        int_prop(&limited, "width"),
        16,
        "process/size_limit=16 must downscale a 64×64 source"
    );
    assert_eq!(
        int_prop(&limited, "height"),
        16,
        "the aspect ratio is kept, so a square stays square"
    );
    assert_eq!(
        pixel_count(&limited),
        16 * 16,
        "the pixel array is resampled with the dimensions, not left at 64×64"
    );
}

/// The control: the SAME bytes with no sidecar load unchanged. Without this,
/// the assertions above could pass on an importer that always halved things.
#[test]
fn without_a_sidecar_the_defaults_are_used() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(root.join("plain.svg"), RED_SVG).expect("write svg");
    std::fs::write(root.join("plain.wav"), stereo_wav(44_100, 1_000)).expect("write wav");
    std::fs::write(root.join("plain.png"), rgba_png(64, 64)).expect("write png");

    let svg = load(root, "res://plain.svg");
    assert_eq!(int_prop(&svg, "width"), 32, "no sidecar → scale 1.0");
    let wav = load(root, "res://plain.wav");
    assert_eq!(int_prop(&wav, "channels"), 2, "no sidecar → stereo is kept");
    let png = load(root, "res://plain.png");
    assert_eq!(int_prop(&png, "width"), 64, "no sidecar → no size limit");
}

/// `[importer_defaults]` in `project.godot` fills options the asset's own
/// sidecar leaves out, and loses to it where both are set — Godot's precedence.
#[test]
fn importer_defaults_fill_gaps_and_lose_to_the_asset() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("project.godot"),
        "config_version=5\n\n[application]\n\nconfig/name=\"t\"\n\n[importer_defaults]\n\ntexture={\"process/size_limit\": 16}\n",
    )
    .expect("write project.godot");

    // (a) the sidecar says nothing about the size limit → the project default
    //     applies.
    let inherits = root.join("inherits.png");
    std::fs::write(&inherits, rgba_png(64, 64)).expect("write png");
    write_sidecar(&inherits, "mipmaps/generate=false");
    assert_eq!(
        int_prop(&load(root, "res://inherits.png"), "width"),
        16,
        "an option only the project sets must still reach the importer"
    );

    // (b) the sidecar sets it too → the per-asset value wins.
    let overrides = root.join("overrides.png");
    std::fs::write(&overrides, rgba_png(64, 64)).expect("write png");
    write_sidecar(&overrides, "process/size_limit=32");
    assert_eq!(
        int_prop(&load(root, "res://overrides.png"), "width"),
        32,
        "the asset's own value must beat the project default"
    );
}

/// The rest of the texture surface: a channel remap and premultiplied alpha
/// rewrite pixels, and `saturation` desaturates an SVG.
#[test]
fn texture_and_svg_pixel_options_are_applied() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // `process/channel_remap/red = 10` is Godot's "One" — the red channel of
    // every pixel becomes 1.0 regardless of the source.
    let remapped = root.join("remap.png");
    std::fs::write(&remapped, rgba_png(4, 4)).expect("write png");
    write_sidecar(&remapped, "process/channel_remap/red=10");
    let res = load(root, "res://remap.png");
    assert!(
        (first_pixel(&res).r - 1.0).abs() < 1e-6,
        "channel_remap/red=One must force red to 1.0, got {:?}",
        first_pixel(&res)
    );

    // A fully desaturated red square is grey: r == g == b.
    let flat = root.join("flat.svg");
    std::fs::write(&flat, RED_SVG).expect("write svg");
    write_sidecar(&flat, "saturation=0.0");
    let grey = first_pixel(&load(root, "res://flat.svg"));
    assert!(
        (grey.r - grey.g).abs() < 1e-3 && (grey.g - grey.b).abs() < 1e-3,
        "saturation=0.0 must leave the pixel grey, got {grey:?}"
    );
    // Not vacuous: the same SVG without the option is saturated red.
    let plain = root.join("plain.svg");
    std::fs::write(&plain, RED_SVG).expect("write svg");
    let red = first_pixel(&load(root, "res://plain.svg"));
    assert!(
        red.r > 0.5 && red.g < 0.2,
        "the unmodified SVG must still be red, got {red:?}"
    );
}

/// The WAV loop options override what the file's own `smpl` chunk said — the
/// artist's setting wins over the sampler's, which is how Godot lets you loop a
/// sample nothing marked.
#[test]
fn wav_loop_options_override_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let wav = root.join("loop.wav");
    std::fs::write(&wav, stereo_wav(22_050, 500)).expect("write wav");

    // No `smpl` chunk at all, so the file itself declares no loop.
    let unlooped = load(root, "res://loop.wav");
    assert_eq!(
        int_prop(&unlooped, "loop_mode"),
        0,
        "no smpl chunk → no loop"
    );

    write_sidecar(
        &wav,
        "edit/loop_mode=1\nedit/loop_begin=100\nedit/loop_end=400",
    );
    let looped = load(root, "res://loop.wav");
    assert_eq!(int_prop(&looped, "loop_mode"), 1, "edit/loop_mode wins");
    assert_eq!(int_prop(&looped, "loop_begin"), 100);
    assert_eq!(int_prop(&looped, "loop_end"), 400);
}
