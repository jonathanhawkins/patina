//! Image export utilities for [`FrameBuffer`].
//!
//! Supports BMP (uncompressed 32-bit BGRA), PPM (binary P6), and PNG
//! (deflate-compressed via `miniz_oxide`) output formats.

use crate::renderer::FrameBuffer;

// ---------------------------------------------------------------------------
// BMP
// ---------------------------------------------------------------------------

/// Encodes the framebuffer as an uncompressed 32-bit BMP image.
pub fn encode_bmp(fb: &FrameBuffer) -> Vec<u8> {
    let pixel_data_size = fb.width * fb.height * 4;
    let file_size = 14 + 40 + pixel_data_size;
    let mut buf = Vec::with_capacity(file_size as usize);

    // -- File header (14 bytes) --
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes()); // reserved1
    buf.extend_from_slice(&0u16.to_le_bytes()); // reserved2
    buf.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset

    // -- DIB header (BITMAPINFOHEADER, 40 bytes) --
    buf.extend_from_slice(&40u32.to_le_bytes()); // header size
    buf.extend_from_slice(&(fb.width as i32).to_le_bytes());
    buf.extend_from_slice(&(fb.height as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // planes
    buf.extend_from_slice(&32u16.to_le_bytes()); // bpp
    buf.extend_from_slice(&0u32.to_le_bytes()); // compression (BI_RGB)
    buf.extend_from_slice(&pixel_data_size.to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes()); // h-res (pixels/m)
    buf.extend_from_slice(&2835u32.to_le_bytes()); // v-res
    buf.extend_from_slice(&0u32.to_le_bytes()); // colors used
    buf.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // -- Pixel data (bottom-up, BGRA) --
    for y in (0..fb.height).rev() {
        for x in 0..fb.width {
            let c = fb.get_pixel(x, y);
            buf.push((c.b.clamp(0.0, 1.0) * 255.0) as u8);
            buf.push((c.g.clamp(0.0, 1.0) * 255.0) as u8);
            buf.push((c.r.clamp(0.0, 1.0) * 255.0) as u8);
            buf.push((c.a.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }

    buf
}

/// Saves the framebuffer as a BMP file.
pub fn save_bmp(fb: &FrameBuffer, path: &str) -> std::io::Result<()> {
    std::fs::write(path, encode_bmp(fb))
}

// ---------------------------------------------------------------------------
// PPM (binary P6)
// ---------------------------------------------------------------------------

/// Encodes the framebuffer as a binary PPM (P6) image.
pub fn encode_ppm(fb: &FrameBuffer) -> Vec<u8> {
    let header = format!("P6\n{} {}\n255\n", fb.width, fb.height);
    let mut buf = Vec::with_capacity(header.len() + (fb.width * fb.height * 3) as usize);
    buf.extend_from_slice(header.as_bytes());
    for pixel in &fb.pixels {
        buf.push((pixel.r.clamp(0.0, 1.0) * 255.0) as u8);
        buf.push((pixel.g.clamp(0.0, 1.0) * 255.0) as u8);
        buf.push((pixel.b.clamp(0.0, 1.0) * 255.0) as u8);
    }
    buf
}

/// Saves the framebuffer as a binary PPM file.
pub fn save_ppm(fb: &FrameBuffer, path: &str) -> std::io::Result<()> {
    std::fs::write(path, encode_ppm(fb))
}

// ---------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------

/// Encodes the framebuffer as a PNG image.
///
/// Uses filter type 0 (None) for all scanlines and deflate compression
/// via `miniz_oxide`.
pub fn encode_png(fb: &FrameBuffer) -> Vec<u8> {
    use miniz_oxide::deflate::compress_to_vec_zlib;

    let mut buf = Vec::new();

    // -- PNG signature --
    buf.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    // -- IHDR --
    let mut ihdr_data = Vec::with_capacity(13);
    ihdr_data.extend_from_slice(&fb.width.to_be_bytes());
    ihdr_data.extend_from_slice(&fb.height.to_be_bytes());
    ihdr_data.push(8); // bit depth
    ihdr_data.push(6); // color type: RGBA
    ihdr_data.push(0); // compression
    ihdr_data.push(0); // filter
    ihdr_data.push(0); // interlace
    write_png_chunk(&mut buf, b"IHDR", &ihdr_data);

    // -- IDAT --
    // Build raw scanline data: filter byte (0) + RGBA pixels per row.
    let row_bytes = 1 + fb.width as usize * 4;
    let mut raw = Vec::with_capacity(row_bytes * fb.height as usize);
    for y in 0..fb.height {
        raw.push(0); // filter type: None
        for x in 0..fb.width {
            let c = fb.get_pixel(x, y);
            raw.push((c.r.clamp(0.0, 1.0) * 255.0) as u8);
            raw.push((c.g.clamp(0.0, 1.0) * 255.0) as u8);
            raw.push((c.b.clamp(0.0, 1.0) * 255.0) as u8);
            raw.push((c.a.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    let compressed = compress_to_vec_zlib(&raw, 6);
    write_png_chunk(&mut buf, b"IDAT", &compressed);

    // -- IEND --
    write_png_chunk(&mut buf, b"IEND", &[]);

    buf
}

/// Saves the framebuffer as a PNG file.
pub fn save_png(fb: &FrameBuffer, path: &str) -> std::io::Result<()> {
    std::fs::write(path, encode_png(fb))
}

/// Writes a PNG chunk: length (4 bytes BE) + type (4 bytes) + data + CRC-32.
fn write_png_chunk(buf: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
    buf.extend_from_slice(chunk_type);
    buf.extend_from_slice(data);
    let crc = png_crc32(chunk_type, data);
    buf.extend_from_slice(&crc.to_be_bytes());
}

/// Computes the CRC-32 over the chunk type + data (ISO 3309 / PNG spec).
fn png_crc32(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in chunk_type.iter().chain(data.iter()) {
        let idx = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = CRC_TABLE[idx] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Pre-computed CRC-32 lookup table (polynomial 0xEDB88320).
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = 0xEDB8_8320 ^ (crc >> 1);
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;

    fn red_2x2() -> FrameBuffer {
        FrameBuffer::new(2, 2, Color::rgb(1.0, 0.0, 0.0))
    }

    fn single_pixel(color: Color) -> FrameBuffer {
        FrameBuffer::new(1, 1, color)
    }

    // -- BMP tests --

    #[test]
    fn bmp_header_signature() {
        let data = encode_bmp(&red_2x2());
        assert_eq!(&data[0..2], b"BM");
    }

    #[test]
    fn bmp_header_file_size() {
        let fb = red_2x2();
        let data = encode_bmp(&fb);
        let file_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
        assert_eq!(file_size as usize, data.len());
        assert_eq!(file_size, 14 + 40 + 2 * 2 * 4);
    }

    #[test]
    fn bmp_header_dimensions() {
        let data = encode_bmp(&red_2x2());
        let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        let height = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        assert_eq!(width, 2);
        assert_eq!(height, 2);
    }

    #[test]
    fn bmp_pixel_data_bgra() {
        let fb = single_pixel(Color::rgb(1.0, 0.0, 0.0));
        let data = encode_bmp(&fb);
        // Pixel starts at offset 54, format is BGRA.
        assert_eq!(data[54], 0); // B
        assert_eq!(data[55], 0); // G
        assert_eq!(data[56], 255); // R
        assert_eq!(data[57], 255); // A
    }

    #[test]
    fn bmp_bottom_up_row_order() {
        let mut fb = FrameBuffer::new(1, 2, Color::BLACK);
        fb.set_pixel(0, 0, Color::rgb(1.0, 0.0, 0.0)); // top row = red
        fb.set_pixel(0, 1, Color::rgb(0.0, 0.0, 1.0)); // bottom row = blue
        let data = encode_bmp(&fb);
        // BMP is bottom-up, so first pixel in data is bottom row (blue).
        assert_eq!(data[54], 255); // B channel of blue pixel
        assert_eq!(data[56], 0); // R channel of blue pixel
                                 // Second pixel is top row (red).
        assert_eq!(data[58], 0); // B channel of red pixel
        assert_eq!(data[60], 255); // R channel of red pixel
    }

    #[test]
    fn bmp_1x1_edge_case() {
        let data = encode_bmp(&single_pixel(Color::WHITE));
        assert_eq!(data.len(), 14 + 40 + 4);
        assert_eq!(&data[0..2], b"BM");
    }

    // -- PPM tests --

    #[test]
    fn ppm_header_p6() {
        let data = encode_ppm(&red_2x2());
        assert!(data.starts_with(b"P6\n"));
    }

    #[test]
    fn ppm_pixel_data() {
        let fb = single_pixel(Color::rgb(0.0, 1.0, 0.0));
        let data = encode_ppm(&fb);
        let header = b"P6\n1 1\n255\n";
        assert_eq!(&data[..header.len()], header);
        assert_eq!(data[header.len()], 0); // R
        assert_eq!(data[header.len() + 1], 255); // G
        assert_eq!(data[header.len() + 2], 0); // B
    }

    // -- PNG tests --

    #[test]
    fn png_magic_bytes() {
        let data = encode_png(&red_2x2());
        assert_eq!(&data[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[test]
    fn png_ihdr_chunk() {
        let data = encode_png(&red_2x2());
        // After 8-byte signature: 4-byte length + "IHDR"
        let len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        assert_eq!(len, 13);
        assert_eq!(&data[12..16], b"IHDR");
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(data[24], 8); // bit depth
        assert_eq!(data[25], 6); // color type RGBA
    }

    #[test]
    fn png_ends_with_iend() {
        let data = encode_png(&red_2x2());
        let tail = &data[data.len() - 12..];
        let len = u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]);
        assert_eq!(len, 0);
        assert_eq!(&tail[4..8], b"IEND");
    }

    #[test]
    fn png_1x1_edge_case() {
        let data = encode_png(&single_pixel(Color::TRANSPARENT));
        assert_eq!(&data[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(&data[12..16] == b"IHDR");
    }

    #[test]
    fn png_roundtrip_decompresses() {
        use miniz_oxide::inflate::decompress_to_vec_zlib;

        let fb = red_2x2();
        let data = encode_png(&fb);

        // Find IDAT chunk (after signature + IHDR chunk).
        // IHDR: 4 len + 4 type + 13 data + 4 crc = 25 bytes after signature.
        let idat_offset = 8 + 25;
        let idat_len = u32::from_be_bytes([
            data[idat_offset],
            data[idat_offset + 1],
            data[idat_offset + 2],
            data[idat_offset + 3],
        ]) as usize;
        assert_eq!(&data[idat_offset + 4..idat_offset + 8], b"IDAT");
        let compressed = &data[idat_offset + 8..idat_offset + 8 + idat_len];
        let raw = decompress_to_vec_zlib(compressed).expect("decompression failed");

        // Each row: 1 filter byte + width * 4 RGBA bytes.
        let expected_len = (1 + 2 * 4) * 2;
        assert_eq!(raw.len(), expected_len);

        // First row, first pixel: filter=0, then R=255, G=0, B=0, A=255.
        assert_eq!(raw[0], 0); // filter byte
        assert_eq!(raw[1], 255); // R
        assert_eq!(raw[2], 0); // G
        assert_eq!(raw[3], 0); // B
        assert_eq!(raw[4], 255); // A
    }

    // -- Save to temp file tests --

    #[test]
    fn save_bmp_creates_file() {
        let fb = red_2x2();
        let path = "/tmp/patina_test_export.bmp";
        save_bmp(&fb, path).expect("failed to write BMP");
        let data = std::fs::read(path).expect("failed to read BMP");
        assert_eq!(&data[0..2], b"BM");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn save_png_creates_file() {
        let fb = red_2x2();
        let path = "/tmp/patina_test_export.png";
        save_png(&fb, path).expect("failed to write PNG");
        let data = std::fs::read(path).expect("failed to read PNG");
        assert_eq!(&data[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn save_ppm_creates_file() {
        let fb = red_2x2();
        let path = "/tmp/patina_test_export.ppm";
        save_ppm(&fb, path).expect("failed to write PPM");
        let data = std::fs::read(path).expect("failed to read PPM");
        assert!(data.starts_with(b"P6\n"));
        let _ = std::fs::remove_file(path);
    }
}
