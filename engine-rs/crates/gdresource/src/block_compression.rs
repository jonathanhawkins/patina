//! Pure-Rust decoders for GPU block-compressed texture payloads (pat-nzgdu.46).
//!
//! Godot's `Image.decompress` / `load_dds_from_buffer` / `load_ktx_from_buffer`
//! turn S3TC/BCn (and ETC2/ASTC) blocks into RGBA8. This module implements the
//! most common S3TC families — **BC1** (DXT1) and **BC3** (DXT5) — as
//! dependency-free block decoders that the `.ctex`/`.dds`/`.ktx` loaders can
//! call to populate a Texture2D's `pixels`. Each decoder takes the raw block
//! stream plus the image dimensions and returns a tightly packed, row-major
//! RGBA8 buffer (`width * height * 4` bytes). Dimensions need not be multiples
//! of 4 — edge blocks are clipped to the image bounds, matching how GPUs store
//! non-multiple-of-4 mip levels.

/// Unpacks a 16-bit RGB565 color to 8-bit `(r, g, b)` channels, replicating the
/// high bits into the low bits so full white maps to `255` (Godot/D3D rule).
fn rgb565_to_rgb888(v: u16) -> (u8, u8, u8) {
    let r5 = ((v >> 11) & 0x1F) as u8;
    let g6 = ((v >> 5) & 0x3F) as u8;
    let b5 = (v & 0x1F) as u8;
    let r8 = (r5 << 3) | (r5 >> 2);
    let g8 = (g6 << 2) | (g6 >> 4);
    let b8 = (b5 << 3) | (b5 >> 2);
    (r8, g8, b8)
}

/// Writes a decoded 4×4 texel block into the RGBA output at block `(bx, by)`,
/// clipping any texels that fall outside the `width`×`height` image.
fn write_block_rgba(
    out: &mut [u8],
    width: usize,
    height: usize,
    bx: usize,
    by: usize,
    texels: &[[u8; 4]; 16],
) {
    for ty in 0..4 {
        let py = by * 4 + ty;
        if py >= height {
            break;
        }
        for tx in 0..4 {
            let px = bx * 4 + tx;
            if px >= width {
                break;
            }
            let o = (py * width + px) * 4;
            out[o..o + 4].copy_from_slice(&texels[ty * 4 + tx]);
        }
    }
}

/// Decodes a single BC1 (S3TC / DXT1) 8-byte block into 16 row-major RGBA
/// texels. When `color0 > color1` the block is opaque (4 interpolated colors);
/// otherwise index 3 is transparent black (1-bit alpha punch-through).
fn decode_bc1_block(block: &[u8; 8]) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (r0, g0, b0) = rgb565_to_rgb888(c0);
    let (r1, g1, b1) = rgb565_to_rgb888(c1);

    let mut palette = [[0u8; 4]; 4];
    palette[0] = [r0, g0, b0, 255];
    palette[1] = [r1, g1, b1, 255];
    let mix = |a: u8, b: u8, wa: u16, wb: u16, denom: u16| -> u8 {
        ((a as u16 * wa + b as u16 * wb) / denom) as u8
    };
    if c0 > c1 {
        palette[2] = [
            mix(r0, r1, 2, 1, 3),
            mix(g0, g1, 2, 1, 3),
            mix(b0, b1, 2, 1, 3),
            255,
        ];
        palette[3] = [
            mix(r0, r1, 1, 2, 3),
            mix(g0, g1, 1, 2, 3),
            mix(b0, b1, 1, 2, 3),
            255,
        ];
    } else {
        palette[2] = [
            mix(r0, r1, 1, 1, 2),
            mix(g0, g1, 1, 1, 2),
            mix(b0, b1, 1, 1, 2),
            255,
        ];
        palette[3] = [0, 0, 0, 0];
    }

    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    let mut out = [[0u8; 4]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        let idx = ((indices >> (2 * i)) & 0x3) as usize;
        *texel = palette[idx];
    }
    out
}

/// Decodes a single BC3 (DXT5) 16-byte block into 16 row-major RGBA texels:
/// an 8-value interpolated alpha ramp (bytes 0..8, BC4-style) combined with a
/// BC1 color block (bytes 8..16, always opaque 4-color mode).
fn decode_bc3_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    // -- Alpha ramp (BC4) ---------------------------------------------------
    let a0 = block[0];
    let a1 = block[1];
    let mut alpha = [0u8; 8];
    alpha[0] = a0;
    alpha[1] = a1;
    let amix =
        |wa: u16, wb: u16, denom: u16| -> u8 { ((a0 as u16 * wa + a1 as u16 * wb) / denom) as u8 };
    if a0 > a1 {
        for i in 1..7 {
            alpha[i + 1] = amix((7 - i) as u16, i as u16, 7);
        }
    } else {
        for i in 1..5 {
            alpha[i + 1] = amix((5 - i) as u16, i as u16, 5);
        }
        alpha[6] = 0;
        alpha[7] = 255;
    }
    // 48 bits of 3-bit alpha indices across bytes 2..8 (little-endian).
    let mut alpha_bits: u64 = 0;
    for i in 0..6 {
        alpha_bits |= (block[2 + i] as u64) << (8 * i);
    }

    // -- Color (BC1, always opaque) -----------------------------------------
    let c0 = u16::from_le_bytes([block[8], block[9]]);
    let c1 = u16::from_le_bytes([block[10], block[11]]);
    let (r0, g0, b0) = rgb565_to_rgb888(c0);
    let (r1, g1, b1) = rgb565_to_rgb888(c1);
    let cmix =
        |a: u8, b: u8, wa: u16, wb: u16| -> u8 { ((a as u16 * wa + b as u16 * wb) / 3) as u8 };
    let color = [
        [r0, g0, b0],
        [r1, g1, b1],
        [cmix(r0, r1, 2, 1), cmix(g0, g1, 2, 1), cmix(b0, b1, 2, 1)],
        [cmix(r0, r1, 1, 2), cmix(g0, g1, 1, 2), cmix(b0, b1, 1, 2)],
    ];
    let color_indices = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);

    let mut out = [[0u8; 4]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        let ci = ((color_indices >> (2 * i)) & 0x3) as usize;
        let ai = ((alpha_bits >> (3 * i)) & 0x7) as usize;
        let rgb = color[ci];
        *texel = [rgb[0], rgb[1], rgb[2], alpha[ai]];
    }
    out
}

/// Decodes a BC1 (DXT1) stream of `width`×`height` pixels into RGBA8.
///
/// Expects `ceil(width/4) * ceil(height/4) * 8` bytes; returns `None` if `data`
/// is shorter than that.
pub fn decode_bc1(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<8>(data, width, height, decode_bc1_block)
}

/// Decodes a BC3 (DXT5) stream of `width`×`height` pixels into RGBA8.
///
/// Expects `ceil(width/4) * ceil(height/4) * 16` bytes; returns `None` if
/// `data` is shorter than that.
pub fn decode_bc3(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<16>(data, width, height, decode_bc3_block)
}

/// Shared block-loop driver: walks the `BLOCK`-byte block stream in row-major
/// block order, decodes each with `decode`, and writes it into the RGBA output.
fn decode_blocks<const BLOCK: usize>(
    data: &[u8],
    width: usize,
    height: usize,
    decode: fn(&[u8; BLOCK]) -> [[u8; 4]; 16],
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return Some(Vec::new());
    }
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);
    if data.len() < blocks_x * blocks_y * BLOCK {
        return None;
    }
    let mut out = vec![0u8; width * height * 4];
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let off = (by * blocks_x + bx) * BLOCK;
            let block: [u8; BLOCK] = data[off..off + BLOCK].try_into().ok()?;
            let texels = decode(&block);
            write_block_rgba(&mut out, width, height, bx, by, &texels);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // color0 = pure red (RGB565 0xF800), color1 = pure blue (RGB565 0x001F).
    // Red  -> (255, 0, 0); Blue -> (0, 0, 255).
    // c0 > c1 so the block is opaque; the 4 palette entries are:
    //   0: red   1: blue   2: 2/3·red+1/3·blue = (170,0,85)
    //                       3: 1/3·red+2/3·blue = (85,0,170)
    #[test]
    fn bc1_opaque_palette_and_indices() {
        // indices: px0=0, px1=1, px2=2, px3=3 -> byte4 = 0b11_10_01_00 = 0xE4.
        let block = [0x00u8, 0xF8, 0x1F, 0x00, 0xE4, 0x00, 0x00, 0x00];
        let px = decode_bc1(&block, 4, 4).expect("decodes");
        assert_eq!(px.len(), 4 * 4 * 4);
        assert_eq!(&px[0..4], &[255, 0, 0, 255], "index 0 -> color0 (red)");
        assert_eq!(&px[4..8], &[0, 0, 255, 255], "index 1 -> color1 (blue)");
        assert_eq!(&px[8..12], &[170, 0, 85, 255], "index 2 -> 2/3 c0 + 1/3 c1");
        assert_eq!(
            &px[12..16],
            &[85, 0, 170, 255],
            "index 3 -> 1/3 c0 + 2/3 c1"
        );
    }

    #[test]
    fn bc1_punchthrough_index3_is_transparent() {
        // c0 = blue (0x001F) <= c1 = red (0xF800) -> 3-color + transparent mode.
        // px0 index 3 -> transparent black.
        let block = [0x1Fu8, 0x00, 0x00, 0xF8, 0x03, 0x00, 0x00, 0x00];
        let px = decode_bc1(&block, 4, 4).expect("decodes");
        assert_eq!(
            &px[0..4],
            &[0, 0, 0, 0],
            "index 3 in low-color mode is transparent"
        );
        // px1..px15 use index 0 -> color0 (blue), opaque.
        assert_eq!(&px[4..8], &[0, 0, 255, 255]);
    }

    #[test]
    fn bc1_too_short_returns_none() {
        assert!(decode_bc1(&[0u8; 4], 4, 4).is_none());
    }

    #[test]
    fn bc3_alpha_ramp_modulates_bc1_color() {
        // Alpha endpoints a0=255 > a1=0 (8-value mode); alpha[0]=255, alpha[1]=0.
        // px0 alpha index 1 -> alpha 0 (transparent); px1.. alpha index 0 -> 255.
        // Color block: red/blue opaque as in the BC1 test; all color indices 0
        // -> red.
        let block = [
            255u8, 0x00, // a0, a1
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, // alpha indices: px0 -> 1
            0x00, 0xF8, 0x1F, 0x00, // color0 red, color1 blue
            0x00, 0x00, 0x00, 0x00, // color indices all 0 -> red
        ];
        let px = decode_bc3(&block, 4, 4).expect("decodes");
        assert_eq!(px.len(), 4 * 4 * 4);
        assert_eq!(
            &px[0..4],
            &[255, 0, 0, 0],
            "px0 red with alpha index 1 -> a1=0"
        );
        assert_eq!(
            &px[4..8],
            &[255, 0, 0, 255],
            "px1 red with alpha index 0 -> a0=255"
        );
    }

    #[test]
    fn non_multiple_of_four_dimensions_are_clipped() {
        // A single 4x4 block covering a 3x2 image: 24 bytes out, no panic.
        let block = [0x00u8, 0xF8, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00];
        let px = decode_bc1(&block, 3, 2).expect("decodes");
        assert_eq!(px.len(), 3 * 2 * 4);
        assert_eq!(&px[0..4], &[255, 0, 0, 255]);
    }
}
