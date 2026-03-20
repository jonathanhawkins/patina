//! Binary `.res` resource format parser.
//!
//! Godot's `.res` files are binary-serialized resources. This module
//! provides detection and header parsing for the binary format. Full
//! property deserialization is not yet implemented — attempting to fully
//! load a `.res` file returns an informative error.
//!
//! ## Binary format overview
//!
//! The `.res` binary format starts with a 4-byte magic number (`RSRC`),
//! followed by metadata including the format version, engine version,
//! resource type string, and then serialized properties using Godot's
//! binary variant encoding.

use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};

use crate::resource::Resource;

/// Magic bytes at the start of every Godot `.res` binary resource file.
pub const RES_MAGIC: &[u8; 4] = b"RSRC";

/// Information extracted from a `.res` binary header.
#[derive(Debug, Clone, PartialEq)]
pub struct ResBinaryHeader {
    /// Whether this file uses big-endian byte order.
    pub big_endian: bool,
    /// Whether 64-bit variant encoding is used.
    pub use_64bit: bool,
    /// The engine version major number.
    pub version_major: u32,
    /// The engine version minor number.
    pub version_minor: u32,
    /// The format version.
    pub format_version: u32,
    /// The resource type string (e.g. `"Resource"`, `"PackedScene"`).
    pub resource_type: String,
}

/// Returns `true` if the given bytes start with the `.res` magic number.
pub fn is_res_binary(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == RES_MAGIC
}

/// Parses the binary header from a `.res` file.
///
/// This extracts metadata (endianness, version, resource type) without
/// deserializing the full resource. Returns an error if the data is too
/// short or doesn't start with the expected magic bytes.
pub fn parse_res_header(data: &[u8]) -> EngineResult<ResBinaryHeader> {
    if data.len() < 4 {
        return Err(EngineError::Parse(
            "data too short to be a .res file".into(),
        ));
    }

    if &data[..4] != RES_MAGIC {
        return Err(EngineError::Parse(format!(
            "invalid .res magic bytes: expected {:?}, got {:?}",
            RES_MAGIC,
            &data[..4]
        )));
    }

    // After magic: 4 bytes for big_endian flag, 4 bytes for use_64bit flag.
    if data.len() < 12 {
        return Err(EngineError::Parse(
            ".res header truncated: missing endian/64bit flags".into(),
        ));
    }

    let big_endian = read_u32_le(&data[4..8]) != 0;
    let use_64bit = read_u32_le(&data[8..12]) != 0;

    let read_u32 = if big_endian { read_u32_be } else { read_u32_le };

    // Version info: major (4), minor (4), format_version (4).
    if data.len() < 24 {
        return Err(EngineError::Parse(
            ".res header truncated: missing version info".into(),
        ));
    }

    let version_major = read_u32(&data[12..16]);
    let version_minor = read_u32(&data[16..20]);
    let format_version = read_u32(&data[20..24]);

    // Resource type string: 4 bytes length, then UTF-8 data (padded to 4-byte alignment).
    let resource_type = if data.len() >= 28 {
        let type_len = read_u32(&data[24..28]) as usize;
        let padded_len = (type_len + 3) & !3; // align to 4 bytes
        if data.len() >= 28 + padded_len {
            String::from_utf8_lossy(&data[28..28 + type_len])
                .trim_end_matches('\0')
                .to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    Ok(ResBinaryHeader {
        big_endian,
        use_64bit,
        version_major,
        version_minor,
        format_version,
        resource_type,
    })
}

/// Attempts to load a `.res` binary resource file.
///
/// Currently, this parses the header to extract the resource type, then
/// returns an error indicating that full binary deserialization is not
/// yet supported.
pub fn load_res_binary(data: &[u8], path: &str) -> EngineResult<Arc<Resource>> {
    let header = parse_res_header(data)?;

    Err(EngineError::Unsupported(format!(
        "binary .res format not yet supported (resource type: '{}', format version: {}, path: {})",
        header.resource_type, header.format_version, path
    )))
}

/// Reads a little-endian `u32` from a 4-byte slice.
fn read_u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Reads a big-endian `u32` from a 4-byte slice.
fn read_u32_be(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal .res binary header for testing.
    fn build_test_header(
        big_endian: bool,
        use_64bit: bool,
        major: u32,
        minor: u32,
        format_ver: u32,
        resource_type: &str,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(RES_MAGIC);

        // Endian and 64bit flags are always written as LE at offset 4 and 8.
        data.extend_from_slice(&(big_endian as u32).to_le_bytes());
        data.extend_from_slice(&(use_64bit as u32).to_le_bytes());

        let write_u32 = if big_endian {
            |v: u32| v.to_be_bytes()
        } else {
            |v: u32| v.to_le_bytes()
        };

        data.extend_from_slice(&write_u32(major));
        data.extend_from_slice(&write_u32(minor));
        data.extend_from_slice(&write_u32(format_ver));

        // Resource type string with length prefix.
        let type_bytes = resource_type.as_bytes();
        data.extend_from_slice(&write_u32(type_bytes.len() as u32));
        data.extend_from_slice(type_bytes);
        // Pad to 4-byte alignment.
        let padding = (4 - (type_bytes.len() % 4)) % 4;
        data.extend(std::iter::repeat(0u8).take(padding));

        data
    }

    #[test]
    fn is_res_binary_detects_magic() {
        let data = build_test_header(false, false, 4, 3, 5, "Resource");
        assert!(is_res_binary(&data));
    }

    #[test]
    fn is_res_binary_rejects_non_res() {
        assert!(!is_res_binary(b"[gd_resource"));
        assert!(!is_res_binary(b"RSR"));
        assert!(!is_res_binary(b""));
        assert!(!is_res_binary(b"RSRD")); // close but wrong
    }

    #[test]
    fn is_res_binary_rejects_tres_text() {
        let tres = b"[gd_resource type=\"Theme\" format=3]";
        assert!(!is_res_binary(tres));
    }

    #[test]
    fn parse_header_little_endian() {
        let data = build_test_header(false, false, 4, 3, 5, "Resource");
        let header = parse_res_header(&data).unwrap();
        assert!(!header.big_endian);
        assert!(!header.use_64bit);
        assert_eq!(header.version_major, 4);
        assert_eq!(header.version_minor, 3);
        assert_eq!(header.format_version, 5);
        assert_eq!(header.resource_type, "Resource");
    }

    #[test]
    fn parse_header_big_endian() {
        let data = build_test_header(true, false, 4, 2, 3, "PackedScene");
        let header = parse_res_header(&data).unwrap();
        assert!(header.big_endian);
        assert!(!header.use_64bit);
        assert_eq!(header.version_major, 4);
        assert_eq!(header.version_minor, 2);
        assert_eq!(header.format_version, 3);
        assert_eq!(header.resource_type, "PackedScene");
    }

    #[test]
    fn parse_header_64bit_flag() {
        let data = build_test_header(false, true, 4, 0, 1, "Texture2D");
        let header = parse_res_header(&data).unwrap();
        assert!(header.use_64bit);
        assert_eq!(header.resource_type, "Texture2D");
    }

    #[test]
    fn parse_header_empty_type() {
        let data = build_test_header(false, false, 4, 0, 1, "");
        let header = parse_res_header(&data).unwrap();
        assert_eq!(header.resource_type, "");
    }

    #[test]
    fn parse_header_too_short() {
        let result = parse_res_header(b"RS");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("too short"));
    }

    #[test]
    fn parse_header_wrong_magic() {
        let result = parse_res_header(b"NOTRSRC\x00\x00\x00\x00\x00");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid .res magic"));
    }

    #[test]
    fn parse_header_truncated_after_magic() {
        let mut data = Vec::new();
        data.extend_from_slice(RES_MAGIC);
        data.extend_from_slice(&[0u8; 4]); // only endian flag, missing 64bit
        let result = parse_res_header(&data);
        assert!(result.is_err());
    }

    #[test]
    fn load_res_binary_returns_unsupported_error() {
        let data = build_test_header(false, false, 4, 3, 5, "Theme");
        let result = load_res_binary(&data, "res://theme.res");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("binary .res format not yet supported"));
        assert!(err.contains("Theme"));
        assert!(err.contains("res://theme.res"));
    }

    #[test]
    fn load_res_binary_invalid_data() {
        let result = load_res_binary(b"not a res file", "test.res");
        assert!(result.is_err());
    }

    #[test]
    fn parse_header_long_resource_type() {
        let long_type = "VeryLongResourceTypeName";
        let data = build_test_header(false, false, 4, 0, 1, long_type);
        let header = parse_res_header(&data).unwrap();
        assert_eq!(header.resource_type, long_type);
    }
}
