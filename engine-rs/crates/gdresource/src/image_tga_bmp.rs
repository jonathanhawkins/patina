//! Dependency-free decoders for Truevision TGA and Windows BMP source images
//! (pat-nzgdu.110).
//!
//! Godot's `Image.load_tga_from_buffer` / `load_bmp_from_buffer` are default
//! source formats for the texture importer. Patina had encode-only BMP (for
//! screenshots) and no TGA at all. These decoders return tightly packed,
//! top-left-origin RGBA8 (`width * height * 4` bytes) so the texture importer
//! can register `.tga`/`.bmp` alongside PNG/JPEG. Covered: uncompressed 24/32-bit
//! BMP (bottom-up and top-down) and 24/32-bit TGA truecolor, both uncompressed
//! (type 2) and run-length encoded (type 10), honoring the origin bit.

/// A decoded image: `width`×`height` RGBA8 pixels, row-major, top-left origin.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedImage {
    /// Image width in pixels.
    pub width: usize,
    /// Image height in pixels.
    pub height: usize,
    /// Tightly packed RGBA8 pixels (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
}

/// Reads one little-endian BGR(A) truecolor pixel (`bpx` = 3 or 4) as `[R,G,B,A]`.
/// `src` must have at least `bpx` bytes.
fn read_pixel_rgba(src: &[u8], bpx: usize) -> [u8; 4] {
    let a = if bpx == 4 { src[3] } else { 255 };
    [src[2], src[1], src[0], a]
}

/// Decodes an uncompressed 24/32-bit Windows BMP (`BITMAPINFOHEADER`, `BI_RGB`).
/// Handles bottom-up (positive height) and top-down (negative height) rows and
/// 4-byte row padding. Returns `None` on unsupported/short input.
pub fn decode_bmp(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return None;
    }
    let pixel_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    let dib_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
    if dib_size < 40 {
        return None; // only BITMAPINFOHEADER or newer
    }
    let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height_raw = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    let bpp = u16::from_le_bytes([data[28], data[29]]);
    let compression = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);
    if compression != 0 || width <= 0 || height_raw == 0 || (bpp != 24 && bpp != 32) {
        return None;
    }
    let top_down = height_raw < 0;
    let width = width as usize;
    let height = height_raw.unsigned_abs() as usize;
    let bpx = (bpp / 8) as usize;
    // Each row is padded up to a 4-byte boundary.
    let row_size = (bpp as usize * width).div_ceil(32) * 4;
    if data.len() < pixel_offset + row_size * height {
        return None;
    }
    let mut rgba = vec![0u8; width * height * 4];
    for file_row in 0..height {
        // BMP rows are bottom-up unless the height is negative.
        let img_row = if top_down {
            file_row
        } else {
            height - 1 - file_row
        };
        let src = pixel_offset + file_row * row_size;
        for col in 0..width {
            let px = read_pixel_rgba(&data[src + col * bpx..], bpx);
            let o = (img_row * width + col) * 4;
            rgba[o..o + 4].copy_from_slice(&px);
        }
    }
    Some(DecodedImage {
        width,
        height,
        rgba,
    })
}

/// Decodes a 24/32-bit Truevision TGA truecolor image, uncompressed (image type
/// 2) or run-length encoded (type 10), honoring the origin bit. Color-mapped and
/// grayscale TGAs are rejected. Returns `None` on unsupported/short input.
pub fn decode_tga(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 18 {
        return None;
    }
    let id_length = data[0] as usize;
    let color_map_type = data[1];
    let image_type = data[2];
    let width = u16::from_le_bytes([data[12], data[13]]) as usize;
    let height = u16::from_le_bytes([data[14], data[15]]) as usize;
    let bpp = data[16];
    let descriptor = data[17];
    if color_map_type != 0
        || (image_type != 2 && image_type != 10)
        || (bpp != 24 && bpp != 32)
        || width == 0
        || height == 0
    {
        return None;
    }
    let bpx = (bpp / 8) as usize;
    let top_origin = descriptor & 0x20 != 0;
    let data_start = 18 + id_length; // color map length is 0 here
    let num_pixels = width * height;

    // First decode pixels in file order (left-to-right, row order per origin).
    let mut pixels: Vec<[u8; 4]> = Vec::with_capacity(num_pixels);
    if image_type == 2 {
        if data.len() < data_start + num_pixels * bpx {
            return None;
        }
        for i in 0..num_pixels {
            pixels.push(read_pixel_rgba(&data[data_start + i * bpx..], bpx));
        }
    } else {
        // Run-length encoded (type 10).
        let mut p = data_start;
        while pixels.len() < num_pixels {
            if p >= data.len() {
                return None;
            }
            let header = data[p];
            p += 1;
            let count = (header & 0x7F) as usize + 1;
            if header & 0x80 != 0 {
                // RLE packet: one pixel repeated `count` times.
                if p + bpx > data.len() {
                    return None;
                }
                let px = read_pixel_rgba(&data[p..], bpx);
                p += bpx;
                for _ in 0..count {
                    if pixels.len() >= num_pixels {
                        break;
                    }
                    pixels.push(px);
                }
            } else {
                // Raw packet: `count` distinct pixels follow.
                if p + count * bpx > data.len() {
                    return None;
                }
                for _ in 0..count {
                    if pixels.len() >= num_pixels {
                        break;
                    }
                    pixels.push(read_pixel_rgba(&data[p..], bpx));
                    p += bpx;
                }
            }
        }
    }

    // Emit RGBA with the origin flip (rows are bottom-up unless the origin bit
    // marks a top origin).
    let mut rgba = vec![0u8; num_pixels * 4];
    for row in 0..height {
        let img_row = if top_origin { row } else { height - 1 - row };
        for col in 0..width {
            let px = pixels[row * width + col];
            let o = (img_row * width + col) * 4;
            rgba[o..o + 4].copy_from_slice(&px);
        }
    }
    Some(DecodedImage {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Builds a minimal uncompressed 24-bit bottom-up BMP from top-left-origin
    // RGB rows (each pixel `[r, g, b]`).
    fn build_bmp24(width: usize, height: usize, rows_top_down: &[Vec<[u8; 3]>]) -> Vec<u8> {
        let row_size = ((24 * width + 31) / 32) * 4;
        let pixel_offset = 54usize;
        let mut out = vec![0u8; pixel_offset + row_size * height];
        out[0] = b'B';
        out[1] = b'M';
        out[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        out[14..18].copy_from_slice(&40u32.to_le_bytes());
        out[18..22].copy_from_slice(&(width as i32).to_le_bytes());
        out[22..26].copy_from_slice(&(height as i32).to_le_bytes()); // bottom-up
        out[26..28].copy_from_slice(&1u16.to_le_bytes());
        out[28..30].copy_from_slice(&24u16.to_le_bytes());
        // Bottom-up: file row 0 is the last top-down row.
        for (file_row, td_row) in (0..height).rev().enumerate() {
            let base = pixel_offset + file_row * row_size;
            for (col, rgb) in rows_top_down[td_row].iter().enumerate() {
                let p = base + col * 3;
                out[p] = rgb[2]; // B
                out[p + 1] = rgb[1]; // G
                out[p + 2] = rgb[0]; // R
            }
        }
        out
    }

    #[test]
    fn bmp_bottom_up_24bit_decodes_top_left_origin() {
        // Top-down image rows: row0 = [red, green], row1 = [blue, white].
        let red = [255, 0, 0];
        let green = [0, 255, 0];
        let blue = [0, 0, 255];
        let white = [255, 255, 255];
        let bmp = build_bmp24(2, 2, &[vec![red, green], vec![blue, white]]);
        let img = decode_bmp(&bmp).expect("valid BMP");
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]); // (0,0) red
        assert_eq!(&img.rgba[4..8], &[0, 255, 0, 255]); // (1,0) green
        assert_eq!(&img.rgba[8..12], &[0, 0, 255, 255]); // (0,1) blue
        assert_eq!(&img.rgba[12..16], &[255, 255, 255, 255]); // (1,1) white
    }

    #[test]
    fn bmp_rejects_compressed_and_bad_magic() {
        assert!(decode_bmp(&[0u8; 54]).is_none()); // no "BM"
        let mut bmp = build_bmp24(1, 1, &[vec![[1, 2, 3]]]);
        bmp[30] = 1; // compression = BI_RLE8
        assert!(decode_bmp(&bmp).is_none());
    }

    // Builds an 18-byte-header uncompressed (type 2) 24-bit bottom-origin TGA.
    fn build_tga24_uncompressed(
        width: usize,
        height: usize,
        rows_top_down: &[Vec<[u8; 3]>],
    ) -> Vec<u8> {
        let mut out = vec![0u8; 18 + width * height * 3];
        out[2] = 2; // uncompressed truecolor
        out[12..14].copy_from_slice(&(width as u16).to_le_bytes());
        out[14..16].copy_from_slice(&(height as u16).to_le_bytes());
        out[16] = 24;
        out[17] = 0; // bottom origin
                     // Bottom origin: file row 0 is the last top-down row.
        for (file_row, td_row) in (0..height).rev().enumerate() {
            let base = 18 + file_row * width * 3;
            for (col, rgb) in rows_top_down[td_row].iter().enumerate() {
                let p = base + col * 3;
                out[p] = rgb[2]; // B
                out[p + 1] = rgb[1]; // G
                out[p + 2] = rgb[0]; // R
            }
        }
        out
    }

    #[test]
    fn tga_uncompressed_bottom_origin_decodes() {
        let red = [255, 0, 0];
        let blue = [0, 0, 255];
        let tga = build_tga24_uncompressed(2, 1, &[vec![red, blue]]);
        let img = decode_tga(&tga).expect("valid TGA");
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]); // (0,0) red
        assert_eq!(&img.rgba[4..8], &[0, 0, 255, 255]); // (1,0) blue
    }

    #[test]
    fn tga_rle_expands_runs() {
        // 4x1, 24-bit, top origin. One RLE packet: green x4.
        let mut tga = vec![0u8; 18];
        tga[2] = 10; // RLE truecolor
        tga[12..14].copy_from_slice(&4u16.to_le_bytes());
        tga[14..16].copy_from_slice(&1u16.to_le_bytes());
        tga[16] = 24;
        tga[17] = 0x20; // top origin
                        // RLE packet header: 0x80 | (4-1) = 0x83, then one green pixel (BGR).
        tga.push(0x83);
        tga.extend_from_slice(&[0, 255, 0]); // B,G,R for green
        let img = decode_tga(&tga).expect("valid RLE TGA");
        assert_eq!((img.width, img.height), (4, 1));
        for i in 0..4 {
            assert_eq!(&img.rgba[i * 4..i * 4 + 4], &[0, 255, 0, 255], "pixel {i}");
        }
    }
}
