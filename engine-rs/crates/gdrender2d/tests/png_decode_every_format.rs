//! `decode_png` must read every PNG a Godot project can ship. It accepted only
//! 8-bit RGB/RGBA/gray and returned `None` for anything else, so a sprite drew
//! NOTHING — found importing the official Godot demo "Dodge the Creeps", whose
//! enemy sprites are 8-bit palette PNGs: every mob was invisible. An Adam7
//! interlaced file was worse: the flag was never read, so it decoded scrambled.
//!
//! Each case builds its file here, byte by byte, so the expected pixels are
//! exact. The decoder ignores CRCs (as before), so they are written as zero.

use gdrender2d::texture::decode_png;
use miniz_oxide::deflate::compress_to_vec_zlib;

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&[0, 0, 0, 0]);
}

/// A PNG of `w`x`h`; `scanlines` are already filter-prefixed rows (for Adam7,
/// the passes' rows back to back).
fn png(
    w: u32,
    h: u32,
    depth: u8,
    color_type: u8,
    interlace: u8,
    extra: &[(&[u8; 4], Vec<u8>)],
    scanlines: &[u8],
) -> Vec<u8> {
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[depth, color_type, 0, 0, interlace]);
    chunk(&mut out, b"IHDR", &ihdr);
    for (kind, body) in extra {
        chunk(&mut out, kind, body);
    }
    chunk(&mut out, b"IDAT", &compress_to_vec_zlib(scanlines, 6));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Unfiltered rows: a 0 filter byte before each.
fn rows(rows: &[&[u8]]) -> Vec<u8> {
    rows.iter().flat_map(|r| std::iter::once(0u8).chain(r.iter().copied())).collect()
}

fn rgba(t: &gdrender2d::texture::Texture2D, x: u32, y: u32) -> [u8; 4] {
    let c = t.pixels[(y * t.width + x) as usize];
    let q = |v: f32| (v * 255.0).round() as u8;
    [q(c.r), q(c.g), q(c.b), q(c.a)]
}

#[test]
fn an_8_bit_palette_png_decodes_with_its_trns_alpha() {
    let plte = vec![255, 0, 0, 0, 255, 0, 0, 0, 255];
    // Index 0 transparent, 1 half, 2 past the end of tRNS -> opaque.
    let trns = vec![0, 128];
    let file = png(
        3,
        1,
        8,
        3,
        0,
        &[(b"PLTE", plte), (b"tRNS", trns)],
        &rows(&[&[0, 1, 2]]),
    );
    let t = decode_png(&file).expect("a palette PNG decodes");
    assert_eq!((t.width, t.height), (3, 1));
    assert_eq!(rgba(&t, 0, 0), [255, 0, 0, 0]);
    assert_eq!(rgba(&t, 1, 0), [0, 255, 0, 128]);
    assert_eq!(rgba(&t, 2, 0), [0, 0, 255, 255]);
}

#[test]
fn a_2_bit_palette_png_unpacks_and_rounds_its_stride_up() {
    // 3 pixels x 2 bits = 6 bits -> 1 byte per row, indices packed MSB first.
    let plte = vec![0, 0, 0, 10, 10, 10, 20, 20, 20, 30, 30, 30];
    let packed = (3 << 6) | (1 << 4) | (2 << 2);
    let file = png(3, 2, 2, 3, 0, &[(b"PLTE", plte)], &rows(&[&[packed], &[0]]));
    let t = decode_png(&file).expect("a 2-bit palette PNG decodes");
    assert_eq!(rgba(&t, 0, 0), [30, 30, 30, 255]);
    assert_eq!(rgba(&t, 1, 0), [10, 10, 10, 255]);
    assert_eq!(rgba(&t, 2, 0), [20, 20, 20, 255]);
    assert_eq!(rgba(&t, 2, 1), [0, 0, 0, 255]);
}

#[test]
fn sixteen_bit_rgba_and_keyed_gray_decode() {
    let file = png(1, 1, 16, 6, 0, &[], &rows(&[&[0xFF, 0xFF, 0x80, 0x00, 0, 0, 0xFF, 0xFF]]));
    let t = decode_png(&file).expect("16-bit RGBA decodes");
    assert_eq!(rgba(&t, 0, 0), [255, 128, 0, 255]);

    // A 4-bit gray image whose tRNS keys value 5 to transparent.
    let file = png(2, 1, 4, 0, 0, &[(b"tRNS", vec![0, 5])], &rows(&[&[(5 << 4) | 15]]));
    let t = decode_png(&file).expect("4-bit gray decodes");
    assert_eq!(rgba(&t, 0, 0)[3], 0, "the keyed gray value is transparent");
    assert_eq!(rgba(&t, 1, 0), [255, 255, 255, 255]);
}

#[test]
fn an_adam7_interlaced_png_decodes_to_the_same_pixels_as_a_plain_one() {
    // 5x3 RGB, every pixel distinct.
    let (w, h) = (5usize, 3usize);
    let px = |x: usize, y: usize| [(x * 40) as u8, (y * 90) as u8, (x * 7 + y * 11) as u8];
    let plain: Vec<Vec<u8>> = (0..h)
        .map(|y| (0..w).flat_map(|x| px(x, y)).collect())
        .collect();
    let plain_file = png(
        w as u32,
        h as u32,
        8,
        2,
        0,
        &[],
        &rows(&plain.iter().map(Vec::as_slice).collect::<Vec<_>>()),
    );

    const ADAM7: [(usize, usize, usize, usize); 7] = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let mut interlaced = Vec::new();
    for (x0, y0, dx, dy) in ADAM7 {
        let mut y = y0;
        while y < h {
            if x0 < w {
                interlaced.push(0);
                let mut x = x0;
                while x < w {
                    interlaced.extend_from_slice(&px(x, y));
                    x += dx;
                }
            }
            y += dy;
        }
    }
    let laced_file = png(w as u32, h as u32, 8, 2, 1, &[], &interlaced);

    let a = decode_png(&plain_file).expect("plain decodes");
    let b = decode_png(&laced_file).expect("interlaced decodes");
    assert_eq!(a.pixels, b.pixels, "Adam7 must reassemble the same image");
    assert_eq!(rgba(&b, 4, 2), [160, 180, 50, 255]);
}

#[test]
fn malformed_files_are_rejected_rather_than_guessed() {
    // A palette image with no PLTE chunk.
    assert!(decode_png(&png(1, 1, 8, 3, 0, &[], &rows(&[&[0]]))).is_none());
    // An illegal depth for the colour type (RGB at 4 bits).
    assert!(decode_png(&png(1, 1, 4, 2, 0, &[], &rows(&[&[0, 0]]))).is_none());
    // Truncated image data.
    assert!(decode_png(&png(2, 2, 8, 6, 0, &[], &rows(&[&[0; 8]]))).is_none());
}
