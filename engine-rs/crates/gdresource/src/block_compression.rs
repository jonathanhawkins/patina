//! Pure-Rust decoders for GPU block-compressed texture payloads (pat-nzgdu.46).
//!
//! Godot's `Image.decompress` / `load_dds_from_buffer` / `load_ktx_from_buffer`
//! turn S3TC/BCn (and ETC2/ASTC) blocks into RGBA8. This module implements the
//! most common S3TC families — **BC1** (DXT1), **BC2** (DXT3), **BC3**
//! (DXT5), **BC4** (`RGTC_R`) and **BC5** (`RGTC_RG`) — as
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
    //
    // Byte-for-byte a BC4 block, which is why BC4/BC5 reuse this decoder rather
    // than carrying a second copy of the ramp rules (pat-s3rrv.48).
    let mut alpha_block = [0u8; 8];
    alpha_block.copy_from_slice(&block[0..8]);
    let alpha = decode_bc4_channel(&alpha_block);

    // -- Color (BC1, always opaque) -----------------------------------------
    let mut color_block = [0u8; 8];
    color_block.copy_from_slice(&block[8..16]);
    let (color, color_indices) = decode_bc1_color_opaque(&color_block);

    let mut out = [[0u8; 4]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        let ci = ((color_indices >> (2 * i)) & 0x3) as usize;
        let rgb = color[ci];
        *texel = [rgb[0], rgb[1], rgb[2], alpha[i]];
    }
    out
}

/// Decodes the 8-byte BC1 colour half in its **always-opaque** 4-colour mode:
/// the four-entry palette plus the packed 2-bit indices (pat-s3rrv.48).
///
/// Shared by BC2 and BC3. Both carry alpha in a separate half, so the
/// punch-through encoding plain BC1 selects when `c0 <= c1` never applies to
/// them — which is why this is deliberately not the same entry point as
/// [`decode_bc1_block`], and why passing a BC1 block here would be wrong.
fn decode_bc1_color_opaque(block: &[u8; 8]) -> ([[u8; 3]; 4], u32) {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (r0, g0, b0) = rgb565_to_rgb888(c0);
    let (r1, g1, b1) = rgb565_to_rgb888(c1);
    let cmix =
        |a: u8, b: u8, wa: u16, wb: u16| -> u8 { ((a as u16 * wa + b as u16 * wb) / 3) as u8 };
    let palette = [
        [r0, g0, b0],
        [r1, g1, b1],
        [cmix(r0, r1, 2, 1), cmix(g0, g1, 2, 1), cmix(b0, b1, 2, 1)],
        [cmix(r0, r1, 1, 2), cmix(g0, g1, 1, 2), cmix(b0, b1, 1, 2)],
    ];
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    (palette, indices)
}

/// Decodes a single BC2 (DXT3) 16-byte block: **explicit** 4-bit alpha per
/// texel (bytes 0..8) over a BC1 colour block (bytes 8..16).
///
/// Unlike BC3 there is no ramp and no endpoint ordering — every texel simply
/// carries its own nibble, which is why BC2 keeps hard alpha edges that BC3
/// smooths. A 4-bit value scales to 8-bit by replication (`a * 17`), so `15`
/// reaches a true `255` rather than `240`.
fn decode_bc2_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    // 64 bits of 4-bit alpha across bytes 0..8, texel 0 in the low nibble.
    let mut alpha_bits: u64 = 0;
    for i in 0..8 {
        alpha_bits |= (block[i] as u64) << (8 * i);
    }
    let mut color_block = [0u8; 8];
    color_block.copy_from_slice(&block[8..16]);
    let (color, color_indices) = decode_bc1_color_opaque(&color_block);

    let mut out = [[0u8; 4]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        let ci = ((color_indices >> (2 * i)) & 0x3) as usize;
        let a4 = ((alpha_bits >> (4 * i)) & 0xF) as u8;
        let rgb = color[ci];
        *texel = [rgb[0], rgb[1], rgb[2], (a4 << 4) | a4];
    }
    out
}

/// Decodes one BC4-style 8-byte block into 16 row-major 8-bit channel values.
///
/// This is the single-channel primitive underneath three formats: BC3's alpha
/// (bytes 0..8 of its block), BC4/`RGTC_R` on its own, and each half of
/// BC5/`RGTC_RG`.
///
/// Two endpoints then a 3-bit index per texel. When `e0 > e1` the ramp is six
/// interpolated values; otherwise it is four, and the last two slots are hard
/// 0 and 255 — the same either/or split BC1 uses for its colour endpoints, and
/// the reason endpoint ORDER is meaningful rather than cosmetic.
fn decode_bc4_channel(block: &[u8; 8]) -> [u8; 16] {
    let e0 = block[0];
    let e1 = block[1];
    let mut ramp = [0u8; 8];
    ramp[0] = e0;
    ramp[1] = e1;
    let mix =
        |wa: u16, wb: u16, denom: u16| -> u8 { ((e0 as u16 * wa + e1 as u16 * wb) / denom) as u8 };
    if e0 > e1 {
        for i in 1..7 {
            ramp[i + 1] = mix((7 - i) as u16, i as u16, 7);
        }
    } else {
        for i in 1..5 {
            ramp[i + 1] = mix((5 - i) as u16, i as u16, 5);
        }
        ramp[6] = 0;
        ramp[7] = 255;
    }
    // 48 bits of 3-bit indices across bytes 2..8 (little-endian).
    let mut bits: u64 = 0;
    for i in 0..6 {
        bits |= (block[2 + i] as u64) << (8 * i);
    }
    let mut out = [0u8; 16];
    for (i, v) in out.iter_mut().enumerate() {
        *v = ramp[((bits >> (3 * i)) & 0x7) as usize];
    }
    out
}

/// Decodes a BC4 (`RGTC_R`) block: one channel into red, matching Godot's
/// `FORMAT_RGTC_R` -> `FORMAT_R8`, whose `get_pixel` reads `(r, 0, 0, 1)`.
fn decode_bc4_block(block: &[u8; 8]) -> [[u8; 4]; 16] {
    let r = decode_bc4_channel(block);
    let mut out = [[0u8; 4]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        *texel = [r[i], 0, 0, 255];
    }
    out
}

/// Decodes a BC5 (`RGTC_RG`) block: two BC4 channel blocks back to back, red
/// then green, matching Godot's `FORMAT_RGTC_RG` -> `FORMAT_RG8` and its
/// `(r, g, 0, 1)` pixel reads.
fn decode_bc5_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let mut red_block = [0u8; 8];
    red_block.copy_from_slice(&block[0..8]);
    let mut green_block = [0u8; 8];
    green_block.copy_from_slice(&block[8..16]);
    let r = decode_bc4_channel(&red_block);
    let g = decode_bc4_channel(&green_block);
    let mut out = [[0u8; 4]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        *texel = [r[i], g[i], 0, 255];
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

/// Decodes a BC2 (DXT3) stream of `width`x`height` pixels into RGBA8
/// (pat-s3rrv.48).
///
/// Expects `ceil(width/4) * ceil(height/4) * 16` bytes; returns `None` if
/// `data` is shorter than that.
pub fn decode_bc2(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<16>(data, width, height, decode_bc2_block)
}

/// Decodes a BC4 (`RGTC_R`) stream of `width`x`height` pixels into RGBA8
/// (pat-s3rrv.48).
///
/// The single channel lands in red with green and blue zeroed and alpha opaque,
/// matching what Godot's `FORMAT_R8` reports through `get_pixel`.
///
/// Expects `ceil(width/4) * ceil(height/4) * 8` bytes; returns `None` if `data`
/// is shorter than that.
pub fn decode_bc4(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<8>(data, width, height, decode_bc4_block)
}

/// Decodes a BC5 (`RGTC_RG`) stream of `width`x`height` pixels into RGBA8
/// (pat-s3rrv.48).
///
/// Two channels land in red and green with blue zeroed and alpha opaque,
/// matching Godot's `FORMAT_RG8`. Blue is NOT reconstructed as a normal-map Z:
/// Godot does not do it at decompress time either, and a caller that wants a
/// normal has to know the texture is one.
///
/// Expects `ceil(width/4) * ceil(height/4) * 16` bytes; returns `None` if
/// `data` is shorter than that.
pub fn decode_bc5(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<16>(data, width, height, decode_bc5_block)
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

    // BC2 carries an explicit 4-bit alpha per texel — no ramp, no endpoint
    // ordering. The nibbles are little-endian across bytes 0..8 with texel 0 in
    // the LOW nibble, and a 4-bit value scales by replication (a*17), so 15 must
    // reach a true 255 rather than 240.
    #[test]
    fn bc2_explicit_nibble_alpha_over_bc1_color() {
        let mut block = [0u8; 16];
        // px0 alpha nibble 15 -> 255; px1 nibble 0 -> 0; px2 nibble 8 -> 136.
        block[0] = 0x0F; // low nibble = px0 = 15, high nibble = px1 = 0
        block[1] = 0x08; // low nibble = px2 = 8
                         // Colour: c0 = red (0xF800), c1 = blue (0x001F), all indices 0 -> red.
        block[8] = 0x00;
        block[9] = 0xF8;
        block[10] = 0x1F;
        block[11] = 0x00;
        let px = decode_bc2(&block, 4, 4).expect("decodes");
        assert_eq!(px.len(), 4 * 4 * 4);
        assert_eq!(&px[0..4], &[255, 0, 0, 255], "nibble 15 -> 15*17 = 255");
        assert_eq!(&px[4..8], &[255, 0, 0, 0], "nibble 0 -> fully transparent");
        assert_eq!(&px[8..12], &[255, 0, 0, 136], "nibble 8 -> 8*17 = 136");
    }

    // BC2's colour half is read in the always-opaque 4-colour mode even when
    // c0 <= c1 — the ordering that would make plain BC1 punch through. A
    // decoder that reused the BC1 entry point here would return a transparent
    // texel for index 3 instead of an interpolated colour.
    #[test]
    fn bc2_color_half_ignores_bc1_punchthrough_ordering() {
        let mut block = [0u8; 16];
        block[0..8].copy_from_slice(&[0xFF; 8]); // every alpha nibble 15 -> 255
        block[8] = 0x1F; // c0 = blue (0x001F)
        block[9] = 0x00;
        block[10] = 0x00; // c1 = red (0xF800) so c0 < c1
        block[11] = 0xF8;
        block[12] = 0xC0; // px3 index 3, px0..px2 index 0
        let px = decode_bc2(&block, 4, 4).expect("decodes");
        assert_eq!(
            px[3 * 4 + 3],
            255,
            "index 3 must stay opaque: BC2 alpha comes from its own nibbles, \
             not from BC1's c0 <= c1 punch-through rule",
        );
        assert_eq!(
            &px[12..16],
            &[170, 0, 85, 255],
            "index 3 -> 1/3 c0 + 2/3 c1 with c0 = blue, c1 = red",
        );
    }

    #[test]
    fn bc2_too_short_returns_none() {
        assert!(decode_bc2(&[0u8; 15], 4, 4).is_none());
    }

    // BC4 8-value mode (e0 > e1). ramp[0]=255, ramp[1]=0, then six interpolants
    // (255*(7-i))/7 for i in 1..7 with integer division:
    //   i=1 -> 218, i=2 -> 182, i=3 -> 145, i=4 -> 109, i=5 -> 72, i=6 -> 36.
    #[test]
    fn bc4_eight_value_ramp_lands_in_red() {
        // 3-bit indices, little-endian: px0 -> 0, px1 -> 1, px2 -> 2.
        // 0 | 1<<3 | 2<<6 = 0x88.
        let block = [255u8, 0x00, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00];
        let px = decode_bc4(&block, 4, 4).expect("decodes");
        assert_eq!(px.len(), 4 * 4 * 4);
        assert_eq!(&px[0..4], &[255, 0, 0, 255], "index 0 -> e0");
        assert_eq!(&px[4..8], &[0, 0, 0, 255], "index 1 -> e1");
        assert_eq!(&px[8..12], &[218, 0, 0, 255], "index 2 -> (255*6)/7");
    }

    // BC4 6-value mode (e0 <= e1) is the branch that makes endpoint ORDER
    // meaningful: indices 6 and 7 become hard 0 and 255 rather than
    // interpolants, so a decoder that ignored the ordering would return
    // interpolated values here instead.
    #[test]
    fn bc4_six_value_ramp_has_hard_endpoints() {
        // px0 -> index 6, px1 -> index 7: 6 | 7<<3 = 0x3E.
        let block = [0x00u8, 255, 0x3E, 0x00, 0x00, 0x00, 0x00, 0x00];
        let px = decode_bc4(&block, 4, 4).expect("decodes");
        assert_eq!(&px[0..4], &[0, 0, 0, 255], "index 6 -> hard 0");
        assert_eq!(&px[4..8], &[255, 0, 0, 255], "index 7 -> hard 255");
    }

    // BC5 is two BC4 blocks back to back. Distinct constant values per channel
    // prove the ORDER (red first, then green) rather than just that two
    // channels arrived.
    #[test]
    fn bc5_reds_then_greens_with_blue_zero() {
        let mut block = [0u8; 16];
        block[0] = 200; // red e0; all indices 0 -> 200
        block[8] = 50; // green e0; all indices 0 -> 50
        let px = decode_bc5(&block, 4, 4).expect("decodes");
        assert_eq!(px.len(), 4 * 4 * 4);
        for i in 0..16 {
            assert_eq!(
                &px[i * 4..i * 4 + 4],
                &[200, 50, 0, 255],
                "texel {i}: red from the first block, green from the second, \
                 blue zeroed and alpha opaque (Godot's RG8 pixel read)",
            );
        }
    }

    #[test]
    fn bc4_and_bc5_too_short_return_none() {
        // One 4x4 block needs 8 bytes for BC4 and 16 for BC5.
        assert!(decode_bc4(&[0u8; 7], 4, 4).is_none());
        assert!(decode_bc5(&[0u8; 15], 4, 4).is_none());
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
