//! Pure-Rust decoders for GPU block-compressed texture payloads (pat-nzgdu.46).
//!
//! Godot's `Image.decompress` / `load_dds_from_buffer` / `load_ktx_from_buffer`
//! turn S3TC/BCn (and ETC2/ASTC) blocks into RGBA8. This module implements the
//! most common S3TC families — **BC1** (DXT1), **BC2** (DXT3), **BC3**
//! (DXT5), **BC4** (`RGTC_R`) and **BC5** (`RGTC_RG`) — plus the **ETC2/EAC**
//! family Godot's Web export uses (**ETC1**, **ETC2_RGB8**, **ETC2_RGBA8**,
//! **EAC R11/RG11**), as dependency-free block decoders that the
//! `.ctex`/`.dds`/`.ktx` loaders can call to populate a Texture2D's `pixels`. Each decoder takes the raw block
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

// ---------------------------------------------------------------------------
// ETC2 / EAC (pat-s3rrv.48)
// ---------------------------------------------------------------------------
//
// Ported line-by-line from `upstream/godot/thirdparty/etcpak/DecodeRGB.cpp`,
// the decoder Godot itself ships — not from the Khronos spec. ETC2's failure
// mode is silently wrong pixels rather than a compile error, so the port is
// deliberately a transcription of working code, and every table below is copied
// from `etcpak/Tables.cpp` rather than recomputed.
//
// Covered: ETC1/ETC2 RGB (individual, differential, T, H and planar blocks),
// ETC2 RGBA8 (the same colour block plus an EAC alpha word), and the EAC
// R11/RG11 single- and dual-channel formats. Punchthrough (`ETC2_RGB8A1`) and
// the signed R11S/RG11S variants are NOT here: etcpak carries no decoder for
// them, so implementing them would mean working from the spec, which is the
// failure mode this port exists to avoid.

/// `table59T58H` — the T/H mode distance table (etcpak/DecodeRGB.cpp:27).
const ETC2_TH_DISTANCE: [i32; 8] = [3, 6, 11, 16, 23, 32, 41, 64];

/// `g_table` — the ETC1 intensity modifier table (etcpak/Tables.cpp:3).
const ETC_INTENSITY: [[i32; 4]; 8] = [
    [2, 8, -2, -8],
    [5, 17, -5, -17],
    [9, 29, -9, -29],
    [13, 42, -13, -42],
    [18, 60, -18, -60],
    [24, 80, -24, -80],
    [33, 106, -33, -106],
    [47, 183, -47, -183],
];

/// `g_alpha` — the EAC modifier table (etcpak/Tables.cpp:70).
const EAC_MODIFIER: [[i32; 8]; 16] = [
    [-3, -6, -9, -15, 2, 5, 8, 14],
    [-3, -7, -10, -13, 2, 6, 9, 12],
    [-2, -5, -8, -13, 1, 4, 7, 12],
    [-2, -4, -6, -13, 1, 3, 5, 12],
    [-3, -6, -8, -12, 2, 5, 7, 11],
    [-3, -7, -9, -11, 2, 6, 8, 10],
    [-4, -7, -8, -11, 3, 6, 7, 10],
    [-3, -5, -8, -11, 2, 4, 7, 10],
    [-2, -6, -8, -10, 1, 5, 7, 9],
    [-2, -5, -8, -10, 1, 4, 7, 9],
    [-2, -4, -8, -10, 1, 3, 7, 9],
    [-2, -5, -7, -10, 1, 4, 6, 9],
    [-3, -4, -7, -10, 2, 3, 6, 9],
    [-1, -2, -3, -10, 0, 1, 2, 9],
    [-4, -6, -8, -9, 3, 5, 7, 8],
    [-3, -5, -7, -9, 2, 4, 6, 8],
];

/// `g_alpha11Mul` — the R11/RG11 multiplier table (etcpak/Tables.cpp:89).
const EAC_R11_MUL: [i32; 16] = [
    1, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120,
];

/// etcpak's `clampu8`: negatives clamp to 0, anything over 255 to 255.
fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// `expand6` / `expand7` — replicate the high bits of a 6- or 7-bit planar
/// component into the low bits (etcpak/DecodeRGB.cpp:32-40).
fn expand6(v: u32) -> i32 {
    ((v << 2) | (v >> 4)) as i32
}

fn expand7(v: u32) -> i32 {
    ((v << 1) | (v >> 6)) as i32
}

/// A three-bit two's-complement differential component, the value etcpak
/// extracts with `(int32_t(d) << 5) >> 29`.
fn sign_extend_3(v: u64) -> i32 {
    let v = (v & 0x7) as i32;
    if v & 0x4 != 0 {
        v - 8
    } else {
        v
    }
}

/// etcpak's `ConvertByteOrder` applied to a raw 8-byte block: the low word is a
/// big-endian read of bytes 0..3 and the high word of bytes 4..7.
fn etc_block_word(block: &[u8]) -> u64 {
    let lo = u32::from_be_bytes([block[0], block[1], block[2], block[3]]) as u64;
    let hi = u32::from_be_bytes([block[4], block[5], block[6], block[7]]) as u64;
    lo | (hi << 32)
}

/// One EAC alpha sample for texel `(i, j)` — the `45 - j*3 - i*12` bit walk
/// shared by every alpha-carrying etcpak decoder.
fn eac_sample(alpha: u64, base: i32, mul: i32, table: &[i32; 8], i: usize, j: usize) -> u8 {
    let shift = 45 - (j as i32) * 3 - (i as i32) * 12;
    let modifier = table[((alpha >> shift) & 0x7) as usize];
    clamp_u8(base + modifier * mul)
}

/// Fills in the alpha channel of an already-decoded T/H/planar block, which is
/// the only thing etcpak's `Decode*Alpha` variants do differently.
fn apply_eac_alpha(texels: &mut [[u8; 4]; 16], alpha: u64) {
    let base = (alpha >> 56) as i32;
    let mul = ((alpha >> 52) & 0xF) as i32;
    let table = &EAC_MODIFIER[((alpha >> 48) & 0xF) as usize];
    for j in 0..4 {
        for i in 0..4 {
            texels[j * 4 + i][3] = eac_sample(alpha, base, mul, table, i, j);
        }
    }
}

/// The two-bit-per-texel index plane shared by the T and H modes.
fn th_index(indexes: u32, i: usize, j: usize) -> usize {
    let hi = (indexes >> (j + i * 4 + 16)) & 0x1;
    let lo = (indexes >> (j + i * 4)) & 0x1;
    ((hi << 1) | lo) as usize
}

/// ETC2 "T" block (etcpak `DecodeT`).
fn decode_etc2_t(block: u64) -> [[u8; 4]; 16] {
    let r0 = (block >> 24) & 0x1B;
    let rh0 = (r0 >> 3) & 0x3;
    let rl0 = r0 & 0x3;
    let g0 = (block >> 20) & 0xF;
    let b0 = (block >> 16) & 0xF;

    let r1 = (block >> 12) & 0xF;
    let g1 = (block >> 8) & 0xF;
    let b1 = (block >> 4) & 0xF;

    let cr0 = ((rh0 << 6) | (rl0 << 4) | (rh0 << 2) | rl0) as i32;
    let cg0 = ((g0 << 4) | g0) as i32;
    let cb0 = ((b0 << 4) | b0) as i32;
    let cr1 = ((r1 << 4) | r1) as i32;
    let cg1 = ((g1 << 4) | g1) as i32;
    let cb1 = ((b1 << 4) | b1) as i32;

    let codeword = ((((block >> 2) & 0x3) << 1) | (block & 0x1)) as usize;
    let d = ETC2_TH_DISTANCE[codeword];

    let palette = [
        [cr0 as u8, cg0 as u8, cb0 as u8, 255],
        [clamp_u8(cr1 + d), clamp_u8(cg1 + d), clamp_u8(cb1 + d), 255],
        [cr1 as u8, cg1 as u8, cb1 as u8, 255],
        [clamp_u8(cr1 - d), clamp_u8(cg1 - d), clamp_u8(cb1 - d), 255],
    ];

    let indexes = ((block >> 32) & 0xFFFF_FFFF) as u32;
    let mut out = [[0u8; 4]; 16];
    for j in 0..4 {
        for i in 0..4 {
            out[j * 4 + i] = palette[th_index(indexes, i, j)];
        }
    }
    out
}

/// ETC2 "H" block (etcpak `DecodeH`).
fn decode_etc2_h(block: u64) -> [[u8; 4]; 16] {
    let indexes = ((block >> 32) & 0xFFFF_FFFF) as u32;

    let r0444 = (block >> 27) & 0xF;
    let g0444 = ((block >> 20) & 0x1) | (((block >> 24) & 0x7) << 1);
    let b0444 = ((block >> 15) & 0x7) | (((block >> 19) & 0x1) << 3);
    let r1444 = (block >> 11) & 0xF;
    let g1444 = (block >> 7) & 0xF;
    let b1444 = (block >> 3) & 0xF;

    let r0 = ((r0444 << 4) | r0444) as i32;
    let g0 = ((g0444 << 4) | g0444) as i32;
    let b0 = ((b0444 << 4) | b0444) as i32;
    let r1 = ((r1444 << 4) | r1444) as i32;
    let g1 = ((g1444 << 4) | g1444) as i32;
    let b1 = ((b1444 << 4) | b1444) as i32;

    // ⚠️ `block & 0x4` is used UNSHIFTED upstream, so it contributes 4 to the
    // codeword while `block & 0x1` contributes 2 — the two high bits arrive
    // from bit 2 and bit 0 respectively. Copied as written.
    let codeword_hi = ((block & 0x1) << 1) | (block & 0x4);
    let c0 = (r0444 << 8) | (g0444 << 4) | b0444;
    let c1 = (block >> 3) & ((1 << 12) - 1);
    let codeword = (codeword_hi | u64::from(c0 >= c1)) as usize;
    let d = ETC2_TH_DISTANCE[codeword];

    let palette = [
        [clamp_u8(r0 + d), clamp_u8(g0 + d), clamp_u8(b0 + d), 255],
        [clamp_u8(r0 - d), clamp_u8(g0 - d), clamp_u8(b0 - d), 255],
        [clamp_u8(r1 + d), clamp_u8(g1 + d), clamp_u8(b1 + d), 255],
        [clamp_u8(r1 - d), clamp_u8(g1 - d), clamp_u8(b1 - d), 255],
    ];

    let mut out = [[0u8; 4]; 16];
    for j in 0..4 {
        for i in 0..4 {
            out[j * 4 + i] = palette[th_index(indexes, i, j)];
        }
    }
    out
}

/// ETC2 planar block (etcpak `DecodePlanar`, scalar path).
fn decode_etc2_planar(block: u64) -> [[u8; 4]; 16] {
    let bv = expand6(((block >> 32) & 0x3F) as u32);
    let gv = expand7(((block >> 38) & 0x7F) as u32);
    let rv = expand6(((block >> 45) & 0x3F) as u32);
    let bh = expand6(((block >> 51) & 0x3F) as u32);
    let gh = expand7(((block >> 57) & 0x7F) as u32);

    let rh0 = block & 0x01;
    let rh1 = ((block >> 2) & 0x1F) << 1;
    let rh = expand6((rh0 | rh1) as u32);

    let bo0 = (block >> 7) & 0x07;
    let bo1 = ((block >> 11) & 0x3) << 3;
    let bo2 = ((block >> 16) & 0x1) << 5;
    let bo = expand6((bo0 | bo1 | bo2) as u32);
    let go0 = (block >> 17) & 0x3F;
    let go1 = ((block >> 24) & 0x01) << 6;
    let go = expand7((go0 | go1) as u32);
    let ro = expand6(((block >> 25) & 0x3F) as u32);

    let mut out = [[0u8; 4]; 16];
    for j in 0..4i32 {
        for i in 0..4i32 {
            let r = (i * (rh - ro) + j * (rv - ro) + 4 * ro + 2) >> 2;
            let g = (i * (gh - go) + j * (gv - go) + 4 * go + 2) >> 2;
            let b = (i * (bh - bo) + j * (bv - bo) + 4 * bo + 2) >> 2;
            out[(j * 4 + i) as usize] = [clamp_u8(r), clamp_u8(g), clamp_u8(b), 255];
        }
    }
    out
}

/// Expands the two 16-bit index planes into one 2-bit-per-texel word, the
/// bit-interleave etcpak does before its texel loop.
fn etc_index_plane(d: u64) -> u32 {
    fn spread(v: u32) -> u32 {
        let v = (v | (v << 8)) & 0x00FF_00FF;
        let v = (v | (v << 4)) & 0x0F0F_0F0F;
        let v = (v | (v << 2)) & 0x3333_3333;
        (v | (v << 1)) & 0x5555_5555
    }
    let b1 = ((d >> 32) & 0xFFFF) as u32;
    let b2 = ((d >> 48) & 0xFFFF) as u32;
    spread(b1) | (spread(b2) << 1)
}

/// The shared ETC1/ETC2 colour block (etcpak `DecodeRGBPart` /
/// `DecodeRGBAPart`). `alpha` is the big-endian EAC word for an RGBA stream.
fn decode_etc2_color(d: u64, alpha: Option<u64>) -> [[u8; 4]; 16] {
    let (br, bg, bb);
    if d & 0x2 != 0 {
        let r0 = ((d & 0xF800_0000) >> 27) as i32;
        let g0 = ((d & 0x00F8_0000) >> 19) as i32;
        let b0 = ((d & 0x0000_F800) >> 11) as i32;
        let r1 = r0 + sign_extend_3(d >> 24);
        let g1 = g0 + sign_extend_3(d >> 16);
        let b1 = b0 + sign_extend_3(d >> 8);

        // The three ETC2 escape modes, in etcpak's order: an out-of-range
        // differential on R selects T, on G selects H, on B selects planar.
        if !(0..=31).contains(&r1) {
            let mut t = decode_etc2_t(d);
            if let Some(a) = alpha {
                apply_eac_alpha(&mut t, a);
            }
            return t;
        }
        if !(0..=31).contains(&g1) {
            let mut h = decode_etc2_h(d);
            if let Some(a) = alpha {
                apply_eac_alpha(&mut h, a);
            }
            return h;
        }
        if !(0..=31).contains(&b1) {
            let mut p = decode_etc2_planar(d);
            if let Some(a) = alpha {
                apply_eac_alpha(&mut p, a);
            }
            return p;
        }

        br = [(r0 << 3) | (r0 >> 2), (r1 << 3) | (r1 >> 2)];
        bg = [(g0 << 3) | (g0 >> 2), (g1 << 3) | (g1 >> 2)];
        bb = [(b0 << 3) | (b0 >> 2), (b1 << 3) | (b1 >> 2)];
    } else {
        br = [
            (((d & 0xF000_0000) >> 24) | ((d & 0xF000_0000) >> 28)) as i32,
            (((d & 0x0F00_0000) >> 20) | ((d & 0x0F00_0000) >> 24)) as i32,
        ];
        bg = [
            (((d & 0x00F0_0000) >> 16) | ((d & 0x00F0_0000) >> 20)) as i32,
            (((d & 0x000F_0000) >> 12) | ((d & 0x000F_0000) >> 16)) as i32,
        ];
        bb = [
            (((d & 0x0000_F000) >> 8) | ((d & 0x0000_F000) >> 12)) as i32,
            (((d & 0x0000_0F00) >> 4) | ((d & 0x0000_0F00) >> 8)) as i32,
        ];
    }

    let tcw = [((d & 0xE0) >> 5) as usize, ((d & 0x1C) >> 2) as usize];
    let mut idx = etc_index_plane(d);

    let eac = alpha.map(|a| {
        (
            (a >> 56) as i32,
            ((a >> 52) & 0xF) as i32,
            &EAC_MODIFIER[((a >> 48) & 0xF) as usize],
        )
    });

    let flip = d & 0x1 != 0;
    let mut out = [[0u8; 4]; 16];
    for i in 0..4 {
        for j in 0..4 {
            // Flipped blocks split into two horizontal halves (by row),
            // unflipped into two vertical halves (by column).
            let sub = if flip { j / 2 } else { i / 2 };
            let modifier = ETC_INTENSITY[tcw[sub]][(idx & 0x3) as usize];
            let a = match (alpha, eac) {
                (Some(word), Some((base, mul, table))) => eac_sample(word, base, mul, table, i, j),
                _ => 255,
            };
            out[j * 4 + i] = [
                clamp_u8(br[sub] + modifier),
                clamp_u8(bg[sub] + modifier),
                clamp_u8(bb[sub] + modifier),
                a,
            ];
            idx >>= 2;
        }
    }
    out
}

/// One EAC 11-bit channel, decoded to 8 bits (etcpak `DecodeRPart`).
fn decode_eac_channel(word: u64) -> [u8; 16] {
    let base = (word >> 56) as i32 * 8 + 4;
    let mul = ((word >> 52) & 0xF) as usize;
    let table = &EAC_MODIFIER[((word >> 48) & 0xF) as usize];
    let mut out = [0u8; 16];
    for i in 0..4 {
        for j in 0..4 {
            let shift = 45 - (j as i32) * 3 - (i as i32) * 12;
            let modifier = table[((word >> shift) & 0x7) as usize];
            out[j * 4 + i] = clamp_u8((base + modifier * EAC_R11_MUL[mul]) / 8);
        }
    }
    out
}

fn decode_etc2_rgb_block(block: &[u8; 8]) -> [[u8; 4]; 16] {
    decode_etc2_color(etc_block_word(block), None)
}

fn decode_etc2_rgba_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let alpha = u64::from_be_bytes(block[0..8].try_into().expect("8 bytes"));
    decode_etc2_color(etc_block_word(&block[8..16]), Some(alpha))
}

fn decode_eac_r11_block(block: &[u8; 8]) -> [[u8; 4]; 16] {
    let r = decode_eac_channel(u64::from_be_bytes(*block));
    let mut out = [[0u8; 4]; 16];
    for (texel, red) in out.iter_mut().zip(r) {
        *texel = [red, 0, 0, 255];
    }
    out
}

fn decode_eac_rg11_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let r = decode_eac_channel(u64::from_be_bytes(block[0..8].try_into().expect("8 bytes")));
    let g = decode_eac_channel(u64::from_be_bytes(
        block[8..16].try_into().expect("8 bytes"),
    ));
    let mut out = [[0u8; 4]; 16];
    for (n, texel) in out.iter_mut().enumerate() {
        *texel = [r[n], g[n], 0, 255];
    }
    out
}

/// Decodes an ETC2 RGB8 (or ETC1) stream of `width`x`height` pixels into RGBA8.
///
/// Expects `ceil(width/4) * ceil(height/4) * 8` bytes; returns `None` if `data`
/// is shorter than that.
pub fn decode_etc2_rgb8(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<8>(data, width, height, decode_etc2_rgb_block)
}

/// Decodes an ETC2 RGBA8 stream (EAC alpha word + colour word per block).
pub fn decode_etc2_rgba8(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<16>(data, width, height, decode_etc2_rgba_block)
}

/// Decodes an EAC R11 stream into RGBA8 with the value in red, matching how
/// Godot decompresses single-channel formats to R8.
pub fn decode_eac_r11(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<8>(data, width, height, decode_eac_r11_block)
}

/// Decodes an EAC RG11 stream into RGBA8 with the two channels in red/green.
pub fn decode_eac_rg11(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    decode_blocks::<16>(data, width, height, decode_eac_rg11_block)
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
