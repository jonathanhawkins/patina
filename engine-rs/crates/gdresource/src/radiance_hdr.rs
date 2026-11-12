//! Dependency-free decoder for Radiance HDR (`.hdr` / RGBE) images (pat-nzgdu.45).
//!
//! Godot's HDR loader decodes Radiance RGBE files (used for HDR panoramas /
//! `PanoramaSkyMaterial` and lightmaps) into linear float RGB. Patina had no HDR
//! decoder. This returns tightly packed linear `f32` RGB (`width * height * 3`),
//! handling the text header, the `-Y H +X W` resolution line, and both flat
//! (old-style) and new-style adaptive-RLE scanlines. Each RGBE pixel expands to
//! `channel * 2^(exponent - 136)` (Radiance's shared-exponent format).

/// A decoded HDR image: `width`×`height` linear RGB in `f32`, row-major
/// (`width * height * 3` values).
#[derive(Debug, Clone, PartialEq)]
pub struct HdrImage {
    /// Image width in pixels.
    pub width: usize,
    /// Image height in pixels.
    pub height: usize,
    /// Tightly packed linear RGB floats (`width * height * 3`).
    pub rgb: Vec<f32>,
}

/// Reads one `\n`-terminated line starting at `*pos`, advancing past it.
fn read_line(data: &[u8], pos: &mut usize) -> Option<String> {
    if *pos >= data.len() {
        return None;
    }
    let start = *pos;
    while *pos < data.len() && data[*pos] != b'\n' {
        *pos += 1;
    }
    let line = String::from_utf8_lossy(&data[start..*pos]).into_owned();
    if *pos < data.len() {
        *pos += 1; // consume the newline
    }
    Some(line)
}

/// Parses the standard `-Y <height> +X <width>` resolution line.
fn parse_resolution(line: &str) -> Option<(usize, usize)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() != 4 || parts[0] != "-Y" || parts[2] != "+X" {
        return None;
    }
    Some((parts[1].parse().ok()?, parts[3].parse().ok()?))
}

/// Expands an RGBE pixel to linear `(r, g, b)`. An exponent of 0 is black.
fn rgbe_to_rgb(px: [u8; 4]) -> (f32, f32, f32) {
    let e = px[3];
    if e == 0 {
        return (0.0, 0.0, 0.0);
    }
    // value = channel * 2^(e - 128 - 8)  (the 8 accounts for the 0..255 byte).
    let f = (e as f32 - 136.0).exp2();
    (px[0] as f32 * f, px[1] as f32 * f, px[2] as f32 * f)
}

/// Decodes one scanline of `width` RGBE pixels into `out`. Detects the new-style
/// adaptive-RLE header (`2, 2, width_hi, width_lo`) and otherwise reads flat
/// RGBE bytes.
fn read_scanline(data: &[u8], pos: &mut usize, width: usize, out: &mut [[u8; 4]]) -> Option<()> {
    if *pos + 4 > data.len() {
        return None;
    }
    let (b0, b1, b2, b3) = (data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]);
    let new_rle = b0 == 2
        && b1 == 2
        && ((b2 as usize) << 8 | b3 as usize) == width
        && (8..=0x7fff).contains(&width);
    if new_rle {
        *pos += 4;
        // Each of the 4 channels (R, G, B, E) is RLE-encoded across the row.
        #[allow(clippy::needless_range_loop)] // `ch` is a column index into `out[x]`
        for ch in 0..4 {
            let mut x = 0;
            while x < width {
                if *pos >= data.len() {
                    return None;
                }
                let count = data[*pos];
                *pos += 1;
                if count > 128 {
                    // Run: (count - 128) copies of the next byte.
                    let run = (count - 128) as usize;
                    if *pos >= data.len() || x + run > width {
                        return None;
                    }
                    let val = data[*pos];
                    *pos += 1;
                    for _ in 0..run {
                        out[x][ch] = val;
                        x += 1;
                    }
                } else {
                    // Literal: `count` bytes copied verbatim.
                    let n = count as usize;
                    if *pos + n > data.len() || x + n > width {
                        return None;
                    }
                    for _ in 0..n {
                        out[x][ch] = data[*pos];
                        *pos += 1;
                        x += 1;
                    }
                }
            }
        }
    } else {
        if *pos + width * 4 > data.len() {
            return None;
        }
        for texel in out.iter_mut().take(width) {
            *texel = [data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]];
            *pos += 4;
        }
    }
    Some(())
}

/// Decodes a Radiance HDR / RGBE image into linear `f32` RGB. Returns `None` on
/// a bad signature, unsupported orientation, or truncated data.
pub fn decode_hdr(data: &[u8]) -> Option<HdrImage> {
    let mut pos = 0;
    let magic = read_line(data, &mut pos)?;
    if !(magic.starts_with("#?RADIANCE") || magic.starts_with("#?RGBE")) {
        return None;
    }
    // Skip header variables up to the blank separator line.
    loop {
        let line = read_line(data, &mut pos)?;
        if line.trim().is_empty() {
            break;
        }
    }
    let (height, width) = parse_resolution(&read_line(data, &mut pos)?)?;
    if width == 0 || height == 0 {
        return None;
    }

    let mut rgb = vec![0.0f32; width * height * 3];
    let mut scanline = vec![[0u8; 4]; width];
    for y in 0..height {
        read_scanline(data, &mut pos, width, &mut scanline)?;
        for (x, &px) in scanline.iter().enumerate() {
            let (r, g, b) = rgbe_to_rgb(px);
            let o = (y * width + x) * 3;
            rgb[o] = r;
            rgb[o + 1] = g;
            rgb[o + 2] = b;
        }
    }
    Some(HdrImage { width, height, rgb })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn header(width: usize, height: usize) -> Vec<u8> {
        format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {height} +X {width}\n").into_bytes()
    }

    #[test]
    fn flat_rgbe_decodes_to_linear_rgb() {
        // Pixel 0: (128,0,0,128) -> 128 * 2^(128-136) = 128/256 = 0.5 red.
        // Pixel 1: exponent 0 -> black.
        let mut data = header(2, 1);
        data.extend_from_slice(&[128, 0, 0, 128]);
        data.extend_from_slice(&[255, 255, 255, 0]);
        let img = decode_hdr(&data).expect("valid HDR");
        assert_eq!((img.width, img.height), (2, 1));
        assert!(approx(img.rgb[0], 0.5) && approx(img.rgb[1], 0.0) && approx(img.rgb[2], 0.0));
        assert!(approx(img.rgb[3], 0.0) && approx(img.rgb[4], 0.0) && approx(img.rgb[5], 0.0));
    }

    #[test]
    fn new_style_rle_expands_runs() {
        // width 8 (min for new RLE), one scanline, every pixel (128,0,0,128).
        // Each channel is a single run: byte 128+8=136, then the value.
        let mut data = header(8, 1);
        data.extend_from_slice(&[2, 2, 0, 8]); // new-RLE header (width hi/lo)
        data.extend_from_slice(&[136, 128]); // R: 8 x 128
        data.extend_from_slice(&[136, 0]); // G: 8 x 0
        data.extend_from_slice(&[136, 0]); // B: 8 x 0
        data.extend_from_slice(&[136, 128]); // E: 8 x 128
        let img = decode_hdr(&data).expect("valid RLE HDR");
        assert_eq!((img.width, img.height), (8, 1));
        for x in 0..8 {
            let o = x * 3;
            assert!(approx(img.rgb[o], 0.5), "px {x} r");
            assert!(approx(img.rgb[o + 1], 0.0) && approx(img.rgb[o + 2], 0.0));
        }
    }

    #[test]
    fn rejects_bad_signature() {
        assert!(decode_hdr(b"not an hdr file\n").is_none());
        assert!(decode_hdr(&[]).is_none());
    }
}
