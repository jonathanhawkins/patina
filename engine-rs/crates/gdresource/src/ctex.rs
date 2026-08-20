//! CompressedTexture2D (`.ctex`) reader.
//!
//! Reads Godot's `.ctex` compiled-texture format
//! (`scene/resources/compressed_texture.cpp` `CompressedTexture2D::_load_data`
//! + `load_image_from_file`; writer in
//! `editor/import/resource_importer_texture.cpp`). Little-endian throughout.
//!
//! Exported/imported Godot projects reference `.godot/imported/*.ctex`
//! artifacts (via `.import` sidecars) rather than the source PNGs, so loading
//! them is required to open a real project. This reader produces the same
//! `Texture2D` [`Resource`] shape as the crate's PNG importer
//! ([`crate::importers::import_image`]) — width/height/format/pixels — so
//! downstream consumers are agnostic to whether a texture came from a source
//! image or a compiled `.ctex`.
//!
//! # Layout
//!
//! ```text
//! Outer header:
//!   magic        4 bytes  "GST2"
//!   version      u32      (<= FORMAT_VERSION = 1)
//!   width        u32      original texture width
//!   height       u32      original texture height
//!   df           u32      FORMAT_BIT_* flags (stream / mipmaps / detect)
//!   mipmap_limit u32
//!   reserved     u32 x 3
//! Inner image header:
//!   data_format  u32      DATA_FORMAT_IMAGE(0)/PNG(1)/WEBP(2)/BASIS(3)
//!   w            u16      image width
//!   h            u16      image height
//!   mipmaps      u32      extra mip levels
//!   format       u32      Image::Format enum
//! Data (level 0 becomes the pixels; later levels are validated, not decoded):
//!   PNG/WEBP:    u32 size + `size` bytes (an embedded PNG/WebP file), per level
//!   IMAGE:       raw pixel bytes, per level, back to back with NO size prefix
//! ```
//!
//! # Support
//!
//! - `DATA_FORMAT_PNG` / `DATA_FORMAT_WEBP`: decoded by reusing the crate's
//!   PNG/WebP importers.
//! - `DATA_FORMAT_IMAGE`, uncompressed: `L8`/`LA8`/`RGB8`/`RGBA8`.
//! - `DATA_FORMAT_IMAGE`, VRAM-compressed (pat-s3rrv.48): the S3TC family —
//!   `DXT1` (BC1), `DXT3` (BC2), `DXT5` (BC3), `DXT5_RA_AS_RG`, `RGTC_R` (BC4),
//!   `RGTC_RG` (BC5) — and the ETC2/EAC family Godot's Web export uses: `ETC`
//!   (ETC1), `ETC2_RGB8`, `ETC2_RGBA8`, `ETC2_RA_AS_RG`, `ETC2_R11` and
//!   `ETC2_RG11`. All decode to RGBA8 through [`crate::block_compression`].
//!   `compress/mode = VRAM Compressed` is the default of Godot's "3D" texture
//!   preset, so this is how most 3D textures in a real project are stored.
//! - Still unsupported: BC7/BC6H, ASTC, ETC2's punchthrough `RGB8A1`, the
//!   signed `R11S`/`RG11S` variants, and `DATA_FORMAT_BASIS_UNIVERSAL`. Each
//!   returns an [`EngineError::Unsupported`] that **names the format**, so a
//!   project that fails to load says which encoder produced it.
//! - Layered and volume artifacts (`.ctexarray`, `.ccube`, `.ccubearray`,
//!   `.ctex3d`) share the `GSTL` container — see
//!   [`load_ctex_layered_bytes`].
//!
//! # Mip chains
//!
//! `DATA_FORMAT_IMAGE` stores levels contiguously with no per-level framing, so
//! finding level 1 means computing level 0's exact size — for block formats,
//! `ceil(w/4) * ceil(h/4) * block_bytes`, with the chain flooring at 4x4 rather
//! than 1x1 (`Image::get_format_min_pixel_size`). Level 0 supplies `pixels`;
//! the remaining levels are read to prove the file carries what its header
//! promises, and the total is reported as `mipmap_count`.

use std::path::Path;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::Color;
use gdvariant::Variant;

use crate::block_compression;
use crate::importers::{import_image_bytes, import_webp_bytes};
use crate::resource::Resource;

/// Magic bytes at the start of a `.ctex` CompressedTexture2D file.
pub const CTEX_MAGIC: &[u8; 4] = b"GST2";

/// Highest `.ctex` FORMAT_VERSION this reader accepts.
const CTEX_FORMAT_VERSION_MAX: u32 = 1;

// DataFormat enum (compressed_texture.h).
const CTEX_DATA_FORMAT_IMAGE: u32 = 0;
const CTEX_DATA_FORMAT_PNG: u32 = 1;
const CTEX_DATA_FORMAT_WEBP: u32 = 2;
const CTEX_DATA_FORMAT_BASIS_UNIVERSAL: u32 = 3;

// Image::Format enum values (core/io/image.h) supported for DATA_FORMAT_IMAGE.
const IMG_FORMAT_L8: u32 = 0;
const IMG_FORMAT_LA8: u32 = 1;
const IMG_FORMAT_RGB8: u32 = 4;
const IMG_FORMAT_RGBA8: u32 = 5;

// Block-compressed Image::Format values (pat-s3rrv.48). Ordinals counted from
// `FORMAT_L8 = 0` in upstream/godot/core/io/image.h; `compress/mode = VRAM
// Compressed` is the default of Godot's "3D" texture preset, so these are the
// common storage for any 3D texture in a real project.
const IMG_FORMAT_DXT1: u32 = 17; // BC1
const IMG_FORMAT_DXT3: u32 = 18; // BC2
const IMG_FORMAT_DXT5: u32 = 19; // BC3
const IMG_FORMAT_RGTC_R: u32 = 20; // BC4
const IMG_FORMAT_RGTC_RG: u32 = 21; // BC5
const IMG_FORMAT_BPTC_RGBA: u32 = 22; // BC7
const IMG_FORMAT_BPTC_RGBF: u32 = 23;
const IMG_FORMAT_BPTC_RGBFU: u32 = 24;
const IMG_FORMAT_ETC: u32 = 25;
const IMG_FORMAT_ETC2_R11: u32 = 26;
const IMG_FORMAT_ETC2_R11S: u32 = 27;
const IMG_FORMAT_ETC2_RG11: u32 = 28;
const IMG_FORMAT_ETC2_RG11S: u32 = 29;
const IMG_FORMAT_ETC2_RGB8: u32 = 30;
const IMG_FORMAT_ETC2_RGBA8: u32 = 31;
const IMG_FORMAT_ETC2_RGB8A1: u32 = 32;
const IMG_FORMAT_ETC2_RA_AS_RG: u32 = 33;
const IMG_FORMAT_DXT5_RA_AS_RG: u32 = 34;
const IMG_FORMAT_ASTC_4X4: u32 = 35;
const IMG_FORMAT_ASTC_4X4_HDR: u32 = 36;
const IMG_FORMAT_ASTC_8X8: u32 = 37;
const IMG_FORMAT_ASTC_8X8_HDR: u32 = 38;

/// `FORMAT_BIT_HAS_MIPMAPS` from `scene/resources/compressed_texture.h:56` —
/// set in the OUTER header's `df` word when the file carries a mip chain.
const CTEX_FORMAT_BIT_HAS_MIPMAPS: u32 = 1 << 23;

/// Every block-compressed format is stored as 4x4 texel blocks, and its mip
/// chain floors at 4x4 rather than 1x1 (`Image::get_format_min_pixel_size`,
/// core/io/image.cpp). ASTC 8x8 is the one exception and is not decoded here.
const BLOCK_DIM: u32 = 4;

/// How a `DATA_FORMAT_IMAGE` payload is laid out for one `Image::Format`.
enum ImageLayout {
    /// Tightly packed 8-bit channels: `w * h * bpp` bytes per level.
    Raw {
        bpp: usize,
        format_str: &'static str,
    },
    /// 4x4 block-compressed: `ceil(w/4) * ceil(h/4) * block_bytes` per level.
    /// `decode` turns one level's bytes into RGBA8.
    Block {
        block_bytes: usize,
        decode: fn(&[u8], usize, usize) -> Option<Vec<u8>>,
        name: &'static str,
    },
}

/// Maps an `Image::Format` ordinal onto its payload layout, or names the format
/// in an `Unsupported` error.
///
/// Formats whose decoder does not exist yet are named individually rather than
/// lumped together: "ETC2_RGBA8 is not supported" tells you which encoder the
/// project used, where the old blanket message did not.
fn image_layout(img_format: u32) -> EngineResult<ImageLayout> {
    let unsupported = |name: &str| {
        EngineError::Unsupported(format!(
            ".ctex Image::Format {img_format} ({name}) has no decoder in this build"
        ))
    };
    Ok(match img_format {
        IMG_FORMAT_L8 => ImageLayout::Raw {
            bpp: 1,
            format_str: "L",
        },
        IMG_FORMAT_LA8 => ImageLayout::Raw {
            bpp: 2,
            format_str: "LA",
        },
        IMG_FORMAT_RGB8 => ImageLayout::Raw {
            bpp: 3,
            format_str: "RGB",
        },
        IMG_FORMAT_RGBA8 => ImageLayout::Raw {
            bpp: 4,
            format_str: "RGBA",
        },
        // BC1: 8 bytes per block. Godot writes DXT1 for opaque VRAM textures.
        IMG_FORMAT_DXT1 => ImageLayout::Block {
            block_bytes: 8,
            decode: block_compression::decode_bc1,
            name: "DXT1",
        },
        // BC3: 16 bytes per block (alpha ramp + colour block).
        IMG_FORMAT_DXT5 => ImageLayout::Block {
            block_bytes: 16,
            decode: block_compression::decode_bc3,
            name: "DXT5",
        },
        // BC3 with the channels swizzled for normal maps. The block layout is
        // identical, so it decodes; consumers that care about the swizzle read
        // the `format` property.
        IMG_FORMAT_DXT5_RA_AS_RG => ImageLayout::Block {
            block_bytes: 16,
            decode: block_compression::decode_bc3,
            name: "DXT5_RA_AS_RG",
        },
        // BC2: explicit 4-bit alpha per texel over a BC1 colour block.
        IMG_FORMAT_DXT3 => ImageLayout::Block {
            block_bytes: 16,
            decode: block_compression::decode_bc2,
            name: "DXT3",
        },
        // BC4/BC5: one and two BC4-style channel blocks. Godot decompresses
        // these to R8/RG8, so the unused channels stay zero and alpha opaque
        // (pat-s3rrv.48).
        IMG_FORMAT_RGTC_R => ImageLayout::Block {
            block_bytes: 8,
            decode: block_compression::decode_bc4,
            name: "RGTC_R/BC4",
        },
        IMG_FORMAT_RGTC_RG => ImageLayout::Block {
            block_bytes: 16,
            decode: block_compression::decode_bc5,
            name: "RGTC_RG/BC5",
        },
        IMG_FORMAT_BPTC_RGBA => return Err(unsupported("BPTC_RGBA/BC7")),
        IMG_FORMAT_BPTC_RGBF => return Err(unsupported("BPTC_RGBF/BC6H signed")),
        IMG_FORMAT_BPTC_RGBFU => return Err(unsupported("BPTC_RGBFU/BC6H unsigned")),
        // ETC1 is the ETC2 individual/differential subset, so the same block
        // decoder reads it (pat-s3rrv.48).
        IMG_FORMAT_ETC => ImageLayout::Block {
            block_bytes: 8,
            decode: block_compression::decode_etc2_rgb8,
            name: "ETC1",
        },
        // EAC single- and dual-channel, decompressed to R8/RG8 like Godot does,
        // so the unused channels stay zero and alpha opaque.
        IMG_FORMAT_ETC2_R11 => ImageLayout::Block {
            block_bytes: 8,
            decode: block_compression::decode_eac_r11,
            name: "ETC2_R11",
        },
        IMG_FORMAT_ETC2_RG11 => ImageLayout::Block {
            block_bytes: 16,
            decode: block_compression::decode_eac_rg11,
            name: "ETC2_RG11",
        },
        // ⚠️ The SIGNED EAC variants are a different value mapping (the base is
        // a signed 8-bit value), and etcpak — the decoder this port follows —
        // has no signed path. Left unsupported rather than decoded wrongly.
        IMG_FORMAT_ETC2_R11S => return Err(unsupported("ETC2_R11S (signed)")),
        IMG_FORMAT_ETC2_RG11S => return Err(unsupported("ETC2_RG11S (signed)")),
        IMG_FORMAT_ETC2_RGB8 => ImageLayout::Block {
            block_bytes: 8,
            decode: block_compression::decode_etc2_rgb8,
            name: "ETC2_RGB8",
        },
        IMG_FORMAT_ETC2_RGBA8 => ImageLayout::Block {
            block_bytes: 16,
            decode: block_compression::decode_etc2_rgba8,
            name: "ETC2_RGBA8",
        },
        // ⚠️ Punchthrough alpha: the `diffbit` becomes an opacity flag and
        // index 2 means fully transparent, a different colour path that etcpak
        // does not implement. Decoding it as plain ETC2_RGB8 would silently
        // drop the transparency, so it stays unsupported.
        IMG_FORMAT_ETC2_RGB8A1 => return Err(unsupported("ETC2_RGB8A1 (punchthrough)")),
        // Same block layout as ETC2_RGBA8 with the channels swizzled for normal
        // maps; consumers that care read the `format` property.
        IMG_FORMAT_ETC2_RA_AS_RG => ImageLayout::Block {
            block_bytes: 16,
            decode: block_compression::decode_etc2_rgba8,
            name: "ETC2_RA_AS_RG",
        },
        IMG_FORMAT_ASTC_4X4 => return Err(unsupported("ASTC_4x4")),
        IMG_FORMAT_ASTC_4X4_HDR => return Err(unsupported("ASTC_4x4_HDR")),
        IMG_FORMAT_ASTC_8X8 => return Err(unsupported("ASTC_8x8")),
        IMG_FORMAT_ASTC_8X8_HDR => return Err(unsupported("ASTC_8x8_HDR")),
        other => {
            return Err(EngineError::Unsupported(format!(
                ".ctex uncompressed Image::Format {other} is not supported (only L8/LA8/RGB8/RGBA8 and DXT1/DXT5)"
            )))
        }
    })
}

impl ImageLayout {
    /// Byte size of one mip level at `w` x `h`.
    fn level_size(&self, w: u32, h: u32) -> EngineResult<usize> {
        let overflow = || EngineError::Parse(".ctex image size overflow".into());
        match self {
            Self::Raw { bpp, .. } => (w as usize)
                .checked_mul(h as usize)
                .and_then(|n| n.checked_mul(*bpp))
                .ok_or_else(overflow),
            Self::Block { block_bytes, .. } => {
                let bx = (w as usize).div_ceil(BLOCK_DIM as usize);
                let by = (h as usize).div_ceil(BLOCK_DIM as usize);
                bx.checked_mul(by)
                    .and_then(|n| n.checked_mul(*block_bytes))
                    .ok_or_else(overflow)
            }
        }
    }

    /// Dimensions of the next mip level down.
    ///
    /// Godot halves with a floor of `get_format_min_pixel_size` — 1 for packed
    /// formats, 4 for block-compressed ones (`Image::_get_dst_image_size`).
    /// Using 1 for a block format under-counts every small level and desyncs
    /// the cursor for the rest of the chain.
    fn next_level(&self, w: u32, h: u32) -> (u32, u32) {
        let floor = match self {
            Self::Raw { .. } => 1,
            Self::Block { .. } => BLOCK_DIM,
        };
        ((w >> 1).max(floor), (h >> 1).max(floor))
    }
}

/// A bounds-checked little-endian cursor for `.ctex` parsing.
struct CtexCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> CtexCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> EngineResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| EngineError::Parse(".ctex length overflow".into()))?;
        if end > self.data.len() {
            return Err(EngineError::Parse(format!(
                ".ctex truncated: need {n} byte(s) at offset {}",
                self.pos
            )));
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u16(&mut self) -> EngineResult<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> EngineResult<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// Loads a `.ctex` CompressedTexture2D file into a `Texture2D` [`Resource`].
///
/// `path` supplies the resource's `res://` path (from its filename). All reads
/// are bounds-checked, so truncated or corrupt input yields an error rather
/// than a panic.
pub fn load_ctex_bytes(data: &[u8], path: &Path) -> EngineResult<Arc<Resource>> {
    let mut c = CtexCursor::new(data);

    if c.take(4)? != CTEX_MAGIC {
        return Err(EngineError::Parse(
            "not a .ctex file (expected GST2 magic)".into(),
        ));
    }
    let version = c.u32()?;
    if version > CTEX_FORMAT_VERSION_MAX {
        return Err(EngineError::Unsupported(format!(
            ".ctex format version {version} is newer than supported ({CTEX_FORMAT_VERSION_MAX})"
        )));
    }
    let _width = c.u32()?; // original texture width
    let _height = c.u32()?; // original texture height
                            // `df` carries FORMAT_BIT_HAS_MIPMAPS. The blob below
                            // states its own level count, which is what upstream
                            // reads, so this is informational here.
    let _df = c.u32()?;
    let _mipmap_limit = c.u32()?;
    c.u32()?; // reserved
    c.u32()?;
    c.u32()?;

    let (tex, levels) = read_image_blob(&mut c, path)?;
    Ok(with_mipmap_count(tex, levels))
}

/// Reads ONE self-delimiting image blob at the cursor and returns it plus the
/// number of mip levels it carried (level 0 included).
///
/// This is Godot's `CompressedTexture2D::load_image_from_file`
/// (`scene/resources/compressed_texture.cpp`), and it is a separate function for
/// the same reason it is upstream: a `.ctexarray` / `.ccube` / `.ctex3d` is a
/// short header followed by **several** of these blobs back to back, so reading
/// one has to leave the cursor exactly at the start of the next (pat-s3rrv.48).
///
/// ⚠️ The outer file's `FORMAT_BIT_HAS_MIPMAPS` is deliberately not consulted:
/// upstream never passes it here, and the blob's own `mipmaps` field is what
/// says how many levels follow.
fn read_image_blob(c: &mut CtexCursor<'_>, path: &Path) -> EngineResult<(Arc<Resource>, u32)> {
    let data_format = c.u32()?;
    let w = c.u16()? as u32;
    let h = c.u16()? as u32;
    let mipmaps = c.u32()?; // mip levels BEYOND level 0
    let img_format = c.u32()?;

    match data_format {
        CTEX_DATA_FORMAT_PNG => {
            // Every level is length-framed here, and level 0 is the one that
            // becomes pixels — but the rest still have to be stepped over so a
            // following layer starts where it should.
            let size = c.u32()? as usize;
            let png = c.take(size)?;
            let tex = import_image_bytes(png, path)?;
            skip_framed_levels(c, mipmaps)?;
            Ok((tex, mipmaps.saturating_add(1)))
        }
        CTEX_DATA_FORMAT_WEBP => {
            // Same layout as PNG. Godot's default 2D texture importer stores
            // textures this way, so this is the common path for a real project
            // (pat-nzgdu.8).
            let size = c.u32()? as usize;
            let webp = c.take(size)?;
            let tex = import_webp_bytes(webp, path)?;
            skip_framed_levels(c, mipmaps)?;
            Ok((tex, mipmaps.saturating_add(1)))
        }
        CTEX_DATA_FORMAT_BASIS_UNIVERSAL => Err(EngineError::Unsupported(
            "Basis Universal .ctex is not supported".into(),
        )),
        CTEX_DATA_FORMAT_IMAGE => {
            let layout = image_layout(img_format)?;

            // Levels are stored back to back with NO per-level size prefix —
            // unlike the PNG/WebP arms above, where each level is length-framed.
            // `load_image_from_file` reads the whole chain as one run, so the
            // only way to find level 1 is to compute level 0's exact length.
            let level0_size = layout.level_size(w, h)?;
            let level0 = c.take(level0_size)?;

            // Walk the rest of the chain. Nothing here needs the bytes yet —
            // level 0 is what becomes pixels — but reading them proves the file
            // actually carries the levels its header advertises, so a truncated
            // artifact is an error instead of a texture with a silent hole, and
            // leaves the cursor on the next layer.
            let (mut lw, mut lh) = layout.next_level(w, h);
            for _ in 0..mipmaps {
                let size = layout.level_size(lw, lh)?;
                c.take(size)?;
                let (nw, nh) = layout.next_level(lw, lh);
                lw = nw;
                lh = nh;
            }

            let tex = match &layout {
                ImageLayout::Raw { bpp, format_str } => {
                    build_raw_texture(w, h, level0, *bpp, *format_str, path)
                }
                ImageLayout::Block { decode, name, .. } => {
                    // The block decoders hand back tightly packed RGBA8, which
                    // is exactly what `build_raw_texture` consumes, so a
                    // VRAM-compressed texture ends up the same Resource shape as
                    // a PNG-sourced one — the point of the bead.
                    let rgba = (*decode)(level0, w as usize, h as usize).ok_or_else(|| {
                        EngineError::Parse(format!(".ctex {name} payload is too short for {w}x{h}"))
                    })?;
                    build_raw_texture(w, h, rgba.as_slice(), 4, "RGBA", path)
                }
            };
            // Godot counts `mipmaps` as levels BEYOND level 0, so a texture with
            // one extra level has two in total. `mipmap_count` reports the
            // total, which is what a renderer allocating storage needs.
            Ok((tex, mipmaps.saturating_add(1)))
        }
        other => Err(EngineError::Parse(format!(
            "unknown .ctex data format {other}"
        ))),
    }
}

/// Steps over `count` length-framed sub-images (the PNG/WebP level chain).
fn skip_framed_levels(c: &mut CtexCursor<'_>, count: u32) -> EngineResult<()> {
    for _ in 0..count {
        let size = c.u32()? as usize;
        c.take(size)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Layered / volume artifacts — .ctexarray, .ccube, .ccubearray, .ctex3d
// ---------------------------------------------------------------------------

/// `GSTL` — the magic every layered and volume artifact starts with.
///
/// Both `CompressedTextureLayered::_load_data` and
/// `CompressedTexture3D::_load_data` check for this same four-byte tag
/// (`scene/resources/compressed_texture.cpp`), which is why the file extension —
/// not the header — is what says which of the four resource types a file is.
pub const CTEX_LAYERED_MAGIC: &[u8; 4] = b"GSTL";

/// Which layered/volume resource an artifact holds.
///
/// The discriminants are Godot's `ResourceImporterLayeredTexture::Mode`
/// (`editor/import/resource_importer_layered_texture.h:65`), which the importer
/// writes into the header's third word and the loader checks against
/// `TextureLayered::LayeredType` — the two enums share their first three values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayeredKind {
    /// `.ctexarray` → `CompressedTexture2DArray`.
    Texture2DArray,
    /// `.ccube` → `CompressedCubemap` (six layers).
    Cubemap,
    /// `.ccubearray` → `CompressedCubemapArray`.
    CubemapArray,
    /// `.ctex3d` → `CompressedTexture3D`, where the layer count is the volume's
    /// depth and extra volume mipmaps follow the slices.
    Texture3D,
}

impl LayeredKind {
    /// The `mode` value the importer stamps into the header.
    fn mode(self) -> u32 {
        match self {
            Self::Texture2DArray => 0,
            Self::Cubemap => 1,
            Self::CubemapArray => 2,
            Self::Texture3D => 3,
        }
    }

    /// The Godot class the artifact loads as.
    fn class_name(self) -> &'static str {
        match self {
            Self::Texture2DArray => "CompressedTexture2DArray",
            Self::Cubemap => "CompressedCubemap",
            Self::CubemapArray => "CompressedCubemapArray",
            Self::Texture3D => "CompressedTexture3D",
        }
    }

    /// `depth` for a volume, `layers` for everything else — the property name
    /// Godot exposes for the count in the header's second word.
    fn count_property(self) -> &'static str {
        match self {
            Self::Texture3D => "depth",
            _ => "layers",
        }
    }
}

impl LayeredKind {
    /// The [`LayeredKind`] a Godot `.import` sidecar's `importer=` names
    /// (pat-s3rrv.138).
    ///
    /// These are the `ResourceImporterLayeredTexture` subclasses'
    /// `get_importer_name()` values, which is what a project's sidecar records
    /// — the file extension of the ARTIFACT (`.ctexarray`) is a different
    /// thing and only appears in `[remap] path=`.
    pub fn from_importer_name(name: &str) -> Option<Self> {
        match name {
            "2d_array_texture" => Some(Self::Texture2DArray),
            "cubemap_texture" => Some(Self::Cubemap),
            "cubemap_array_texture" => Some(Self::CubemapArray),
            "3d_texture" => Some(Self::Texture3D),
            _ => None,
        }
    }

    /// The `(hslices, vslices)` grid this kind slices its source image into.
    ///
    /// ⚠️ A cubemap IGNORES `slices/horizontal` and `slices/vertical` entirely
    /// and derives the grid from `slices/arrangement`
    /// (`resource_importer_layered_texture.cpp:312-329`): `0`→1x6, `1`→2x3,
    /// `2`→3x2, `3`→6x1. Feeding it the h/v options instead — they are present
    /// in the same sidecar — produces a plausible grid with the wrong face
    /// count.
    ///
    /// ⚠️ A cubemap ARRAY then multiplies one axis by `slices/amount`, chosen
    /// by `slices/layout`: `0` (Horizontal) scales the horizontal axis,
    /// anything else scales the vertical one.
    pub fn slice_grid(
        self,
        hslices: u32,
        vslices: u32,
        arrangement: i64,
        layout: i64,
        amount: u32,
    ) -> (u32, u32) {
        match self {
            Self::Cubemap | Self::CubemapArray => {
                let (mut h, mut v) = match arrangement {
                    0 => (1, 6),
                    2 => (3, 2),
                    3 => (6, 1),
                    // Godot's default arrangement is 1 (2x3), and an
                    // out-of-range value falls here rather than erroring.
                    _ => (2, 3),
                };
                if self == Self::CubemapArray {
                    let amount = amount.max(1);
                    if layout == 0 {
                        h *= amount;
                    } else {
                        v *= amount;
                    }
                }
                (h, v)
            }
            _ => (hslices.max(1), vslices.max(1)),
        }
    }
}

/// Slices one source image into the layers of a layered or volume texture
/// (pat-s3rrv.138).
///
/// Godot ground truth: `resource_importer_layered_texture.cpp:370-385`.
/// `slice_w = width / hslices` and `slice_h = height / vslices` (integer
/// division — a source that does not divide evenly loses the remainder, which
/// is upstream's behaviour, not a rounding bug), and the slices are collected
/// **row-major**: the outer loop walks `vslices`, the inner walks `hslices`, so
/// layer `i * hslices + j` is row `i`, column `j`.
///
/// ⚠️ **Row-major is load-bearing and not guessable.** A 3x1 strip and a 1x3
/// column both produce three layers, so a column-major implementation passes
/// every count assertion and silently reorders the faces of a cubemap. The
/// order is asserted against distinctly-coloured slices in the tests.
///
/// The result mirrors [`load_ctex_layered_bytes`]'s shape exactly — same class
/// name, same `layers`/`depth` property, same `layer_N` subresources — so a
/// consumer cannot tell whether the layers arrived from a pre-baked
/// `.ctexarray` or from slicing a source strip.
pub fn slice_source_into_layers(
    source: &Resource,
    kind: LayeredKind,
    hslices: u32,
    vslices: u32,
    path: &Path,
) -> EngineResult<Arc<Resource>> {
    let int_of = |key: &str| match source.get_property(key) {
        Some(Variant::Int(v)) => *v as i64,
        _ => 0,
    };
    let width = int_of("width");
    let height = int_of("height");
    let pixels: &Vec<Variant> = match source.get_property("pixels") {
        Some(Variant::Array(a)) => a,
        _ => {
            return Err(EngineError::Parse(format!(
                "{} has no decoded pixels to slice",
                path.display()
            )))
        }
    };
    if width <= 0 || height <= 0 {
        return Err(EngineError::Parse(format!(
            "{} has no dimensions to slice",
            path.display()
        )));
    }
    if pixels.len() as i64 != width * height {
        return Err(EngineError::Parse(format!(
            "{} declares {width}x{height} but carries {} pixels",
            path.display(),
            pixels.len()
        )));
    }

    let hs = hslices.max(1) as i64;
    let vs = vslices.max(1) as i64;
    let slice_w = width / hs;
    let slice_h = height / vs;
    if slice_w == 0 || slice_h == 0 {
        return Err(EngineError::Parse(format!(
            "{} is {width}x{height}, too small for a {hs}x{vs} slice grid",
            path.display()
        )));
    }

    let mut layers: Vec<Arc<Resource>> = Vec::with_capacity((hs * vs) as usize);
    for i in 0..vs {
        for j in 0..hs {
            let ox = slice_w * j;
            let oy = slice_h * i;
            let mut out = Vec::with_capacity((slice_w * slice_h) as usize);
            for y in 0..slice_h {
                let row = (oy + y) * width + ox;
                for x in 0..slice_w {
                    out.push(pixels[(row + x) as usize].clone());
                }
            }
            let mut layer = Resource::new("Texture2D");
            layer.set_property("width", Variant::Int(slice_w));
            layer.set_property("height", Variant::Int(slice_h));
            layer.set_property("has_pixel_data", Variant::Bool(true));
            layer.set_property("pixels", Variant::Array(out));
            layers.push(Arc::new(layer));
        }
    }

    let count = layers.len() as i64;
    let first = layers[0].clone();
    let mut res = Resource::new(kind.class_name());
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(slice_w));
    res.set_property("height", Variant::Int(slice_h));
    res.set_property("has_pixel_data", Variant::Bool(true));
    if let Some(pixels) = first.get_property("pixels") {
        res.set_property("pixels", pixels.clone());
    }
    if let Some(format) = source.get_property("format") {
        res.set_property("format", format.clone());
    }
    res.set_property(kind.count_property(), Variant::Int(count));
    res.set_property("layer_count", Variant::Int(count));
    res.set_property("mipmap_count", Variant::Int(1));
    for (i, layer) in layers.into_iter().enumerate() {
        res.subresources.insert(format!("layer_{i}"), layer);
    }
    Ok(Arc::new(res))
}

/// Loads a layered or volume `.ctex` artifact.
///
/// The header is 36 bytes, written by
/// `ResourceImporterLayeredTexture::_save_tex`
/// (`editor/import/resource_importer_layered_texture.cpp:170`):
///
/// ```text
/// "GSTL"  version  count  mode  0  0  extra_mipmaps  0  0
/// ```
///
/// then `count + extra_mipmaps` image blobs back to back, each in exactly the
/// format a plain `.ctex` carries after its own header. `count` is the layer
/// count for an array or cubemap and the depth for a volume; `extra_mipmaps` is
/// non-zero only for volumes, whose mip chain is a set of whole smaller slices
/// rather than a chain inside each blob.
///
/// Every layer is decoded — not just layer 0 — and stored as a subresource
/// keyed `layer_<i>`, with layer 0's pixels also placed on the returned
/// resource so a consumer that only knows how to read a `Texture2D` still sees
/// an image rather than an empty one (pat-s3rrv.48).
pub fn load_ctex_layered_bytes(
    data: &[u8],
    path: &Path,
    kind: LayeredKind,
) -> EngineResult<Arc<Resource>> {
    let mut c = CtexCursor::new(data);

    if c.take(4)? != CTEX_LAYERED_MAGIC {
        return Err(EngineError::Parse(format!(
            "not a layered .ctex file (expected GSTL magic) for {}",
            path.display()
        )));
    }
    let version = c.u32()?;
    if version > CTEX_FORMAT_VERSION_MAX {
        return Err(EngineError::Unsupported(format!(
            "layered .ctex format version {version} is newer than supported \
             ({CTEX_FORMAT_VERSION_MAX})"
        )));
    }

    let count = c.u32()?;
    let mode = c.u32()?;
    c.u32()?; // reserved (0)
    c.u32()?; // reserved (0)
    let extra_mipmaps = c.u32()?;
    c.u32()?; // reserved (0)
    c.u32()?; // reserved (0)

    // Upstream fails the load outright on a mismatch rather than guessing, and
    // so does this: a `.ccube` whose header says "2D array" is a mislabelled
    // file, and silently treating six layers as an array would put a broken
    // cubemap on screen instead of an error in the log.
    if mode != kind.mode() {
        return Err(EngineError::Parse(format!(
            "{} declares mode {mode}, expected {} for {}",
            path.display(),
            kind.mode(),
            kind.class_name()
        )));
    }
    if count == 0 {
        return Err(EngineError::Parse(format!(
            "{} declares zero layers",
            path.display()
        )));
    }

    let total = count.saturating_add(extra_mipmaps);
    let mut layers: Vec<Arc<Resource>> = Vec::with_capacity(total as usize);
    let mut levels_in_layer0 = 1;
    for i in 0..total {
        let (tex, levels) = read_image_blob(&mut c, path)?;
        if i == 0 {
            levels_in_layer0 = levels;
        }
        layers.push(tex);
    }

    let first = layers.first().expect("count > 0 was checked above").clone();
    let mut res = Resource::new(kind.class_name());
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    // Layer 0's own properties describe the whole set: every layer of a Godot
    // layered texture shares one width, height and format.
    for key in ["width", "height", "format", "bit_depth", "has_pixel_data"] {
        if let Some(v) = first.get_property(key) {
            res.set_property(key, v.clone());
        }
    }
    if let Some(pixels) = first.get_property("pixels") {
        res.set_property("pixels", pixels.clone());
    }
    res.set_property(kind.count_property(), Variant::Int(count as i64));
    res.set_property("layer_count", Variant::Int(count as i64));
    res.set_property("mipmap_count", Variant::Int(levels_in_layer0 as i64));
    if kind == LayeredKind::Texture3D {
        res.set_property("mipmap_slices", Variant::Int(extra_mipmaps as i64));
    }
    for (i, layer) in layers.into_iter().enumerate() {
        res.subresources.insert(format!("layer_{i}"), layer);
    }
    Ok(Arc::new(res))
}

/// Reads a layered artifact off disk. One helper per extension so each can be
/// registered as a plain [`LoaderFn`](crate::importers::LoaderFn).
fn load_layered_file(path: &Path, kind: LayeredKind) -> EngineResult<Arc<Resource>> {
    let data = std::fs::read(path).map_err(EngineError::Io)?;
    load_ctex_layered_bytes(&data, path, kind)
}

/// Loads a `.ctexarray` (`CompressedTexture2DArray`).
pub fn load_ctexarray_file(path: &Path) -> EngineResult<Arc<Resource>> {
    load_layered_file(path, LayeredKind::Texture2DArray)
}

/// Loads a `.ccube` (`CompressedCubemap`).
pub fn load_ccube_file(path: &Path) -> EngineResult<Arc<Resource>> {
    load_layered_file(path, LayeredKind::Cubemap)
}

/// Loads a `.ccubearray` (`CompressedCubemapArray`).
pub fn load_ccubearray_file(path: &Path) -> EngineResult<Arc<Resource>> {
    load_layered_file(path, LayeredKind::CubemapArray)
}

/// Loads a `.ctex3d` (`CompressedTexture3D`).
pub fn load_ctex3d_file(path: &Path) -> EngineResult<Arc<Resource>> {
    load_layered_file(path, LayeredKind::Texture3D)
}

/// Records how many mip levels the artifact carries, including level 0
/// (pat-s3rrv.48).
///
/// Takes the `Arc` by value and mutates through `Arc::get_mut`: the resource has
/// just been built and is not shared yet, so this always succeeds. If it ever
/// were shared, the count is simply omitted rather than the load failing —
/// a missing hint is recoverable, a panic is not.
fn with_mipmap_count(res: Arc<Resource>, levels: u32) -> Arc<Resource> {
    let mut res = res;
    if let Some(r) = Arc::get_mut(&mut res) {
        r.set_property("mipmap_count", Variant::Int(levels as i64));
    }
    res
}

/// Builds a `Texture2D` [`Resource`] from tightly-packed 8-bit pixel data,
/// matching the shape [`crate::importers::import_image`] produces.
///
/// `raw` holds exactly `w * h * bpp` bytes (guaranteed by the caller).
fn build_raw_texture(
    w: u32,
    h: u32,
    raw: &[u8],
    bpp: usize,
    format_str: &str,
    path: &Path,
) -> Arc<Resource> {
    let mut res = Resource::new("Texture2D");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(w as i64));
    res.set_property("height", Variant::Int(h as i64));
    res.set_property("format", Variant::String(format_str.into()));
    res.set_property("bit_depth", Variant::Int(8));

    let count = w as usize * h as usize;
    let mut colors = Vec::with_capacity(count);
    for i in 0..count {
        let idx = i * bpp;
        let color = match bpp {
            1 => {
                let g = raw[idx] as f32 / 255.0;
                Color::new(g, g, g, 1.0)
            }
            2 => {
                let g = raw[idx] as f32 / 255.0;
                let a = raw[idx + 1] as f32 / 255.0;
                Color::new(g, g, g, a)
            }
            3 => {
                let r = raw[idx] as f32 / 255.0;
                let g = raw[idx + 1] as f32 / 255.0;
                let b = raw[idx + 2] as f32 / 255.0;
                Color::new(r, g, b, 1.0)
            }
            _ => {
                let r = raw[idx] as f32 / 255.0;
                let g = raw[idx + 1] as f32 / 255.0;
                let b = raw[idx + 2] as f32 / 255.0;
                let a = raw[idx + 3] as f32 / 255.0;
                Color::new(r, g, b, a)
            }
        };
        colors.push(Variant::Color(color));
    }
    res.set_property("has_pixel_data", Variant::Bool(!colors.is_empty()));
    res.set_property("pixels", Variant::Array(colors));
    Arc::new(res)
}

/// Encodes tightly-packed RGBA8 pixels as an uncompressed `.ctex`
/// (`DATA_FORMAT_IMAGE` / `IMG_FORMAT_RGBA8`) — the inverse of
/// [`load_ctex_bytes`]'s image arm (pat-s3rrv.2).
///
/// Nothing in the engine could *write* an import artifact before this: every
/// `.godot/imported/*.ctex` Patina resolved had to have been produced by a real
/// Godot editor, so a project authored in Patina had no artifacts at all. The
/// header is the same 48 bytes the reader consumes, so a round-trip through
/// `load_ctex_bytes` returns the pixels it was given.
///
/// Errors when `rgba` is not exactly `width * height * 4` bytes, rather than
/// writing a header whose declared extent the payload cannot satisfy.
pub fn save_ctex_image_rgba8(width: u32, height: u32, rgba: &[u8]) -> EngineResult<Vec<u8>> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| EngineError::Parse(".ctex image size overflow".into()))?;
    if rgba.len() != expected {
        return Err(EngineError::Parse(format!(
            ".ctex RGBA8 payload is {} bytes but {width}x{height} needs {expected}",
            rgba.len()
        )));
    }
    // `w`/`h` are u16 in the inner image header, so the format itself caps a
    // texture at 65535 on a side.
    if width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err(EngineError::Unsupported(format!(
            ".ctex dimensions {width}x{height} exceed the format's 65535 limit"
        )));
    }

    let mut d = Vec::with_capacity(48 + rgba.len());
    d.extend_from_slice(CTEX_MAGIC);
    d.extend_from_slice(&1u32.to_le_bytes()); // format version
    d.extend_from_slice(&width.to_le_bytes()); // original width
    d.extend_from_slice(&height.to_le_bytes()); // original height
    d.extend_from_slice(&0u32.to_le_bytes()); // FORMAT_BIT_* flags
    d.extend_from_slice(&0u32.to_le_bytes()); // mipmap limit
    d.extend_from_slice(&[0u8; 12]); // reserved (3 x u32)

    d.extend_from_slice(&CTEX_DATA_FORMAT_IMAGE.to_le_bytes());
    d.extend_from_slice(&(width as u16).to_le_bytes());
    d.extend_from_slice(&(height as u16).to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // no extra mip levels
    d.extend_from_slice(&IMG_FORMAT_RGBA8.to_le_bytes());
    d.extend_from_slice(rgba);
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny valid 1x1 red PNG (crate test fixture).
    fn tiny_red_png() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/red1x1.png"))
            .expect("fixture red1x1.png")
    }

    /// Outer .ctex header (magic, version 1, width/height, flags, mipmap_limit, 3 reserved).
    fn ctex_header(width: u32, height: u32) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(CTEX_MAGIC);
        d.extend_from_slice(&1u32.to_le_bytes()); // version
        d.extend_from_slice(&width.to_le_bytes());
        d.extend_from_slice(&height.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // df flags
        d.extend_from_slice(&0u32.to_le_bytes()); // mipmap_limit
        d.extend_from_slice(&[0u8; 12]); // 3 reserved u32
        d
    }

    /// Inner image header (data_format, w16, h16, mipmaps=0, image format).
    fn ctex_inner(d: &mut Vec<u8>, data_format: u32, w: u16, h: u16, img_format: u32) {
        d.extend_from_slice(&data_format.to_le_bytes());
        d.extend_from_slice(&w.to_le_bytes());
        d.extend_from_slice(&h.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // mipmaps
        d.extend_from_slice(&img_format.to_le_bytes());
    }

    fn build_ctex_png(width: u32, height: u32, png: &[u8]) -> Vec<u8> {
        let mut d = ctex_header(width, height);
        ctex_inner(
            &mut d,
            CTEX_DATA_FORMAT_PNG,
            width as u16,
            height as u16,
            IMG_FORMAT_RGBA8,
        );
        d.extend_from_slice(&(png.len() as u32).to_le_bytes()); // level-0 size
        d.extend_from_slice(png);
        d
    }

    /// A `.ctex` wrapping a real WebP file, mirroring [`build_ctex_png`]'s
    /// layout — level 0 is a `u32` size followed by the embedded file.
    fn build_ctex_webp(width: u32, height: u32, webp: &[u8]) -> Vec<u8> {
        let mut d = ctex_header(width, height);
        ctex_inner(
            &mut d,
            CTEX_DATA_FORMAT_WEBP,
            width as u16,
            height as u16,
            IMG_FORMAT_RGBA8,
        );
        d.extend_from_slice(&(webp.len() as u32).to_le_bytes()); // level-0 size
        d.extend_from_slice(webp);
        d
    }

    fn build_ctex_image(width: u32, height: u32, img_format: u32, raw: &[u8]) -> Vec<u8> {
        let mut d = ctex_header(width, height);
        ctex_inner(
            &mut d,
            CTEX_DATA_FORMAT_IMAGE,
            width as u16,
            height as u16,
            img_format,
        );
        d.extend_from_slice(raw);
        d
    }

    fn pixels_of(res: &Resource) -> &[Variant] {
        match res.get_property("pixels") {
            Some(Variant::Array(a)) => a,
            other => panic!("expected pixels array, got {other:?}"),
        }
    }

    #[test]
    fn ctex_png_fixture_decodes_to_texture2d() {
        // Embed the 1x1 red PNG fixture inside a .ctex (DATA_FORMAT_PNG).
        let png = tiny_red_png();
        let ctex = build_ctex_png(1, 1, &png);
        let res = load_ctex_bytes(&ctex, Path::new("icon.ctex")).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(1)));
        let px = pixels_of(&res);
        assert_eq!(px.len(), 1);
        match &px[0] {
            Variant::Color(c) => assert!(c.r > 0.9 && c.g < 0.1),
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn ctex_raw_rgba8_decodes() {
        // 2x1: red pixel, green pixel.
        let raw = [255u8, 0, 0, 255, 0, 255, 0, 255];
        let ctex = build_ctex_image(2, 1, IMG_FORMAT_RGBA8, &raw);
        let res = load_ctex_bytes(&ctex, Path::new("art.ctex")).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(2)));
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGBA".into()))
        );
        let px = pixels_of(&res);
        assert_eq!(px.len(), 2);
        match (&px[0], &px[1]) {
            (Variant::Color(a), Variant::Color(b)) => {
                assert!(a.r > 0.9 && a.g < 0.1);
                assert!(b.g > 0.9 && b.r < 0.1);
            }
            _ => panic!("expected colors"),
        }
    }

    #[test]
    fn ctex_raw_l8_decodes() {
        let ctex = build_ctex_image(1, 1, IMG_FORMAT_L8, &[128u8]);
        let res = load_ctex_bytes(&ctex, Path::new("g.ctex")).unwrap();
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("L".into()))
        );
        match &pixels_of(&res)[0] {
            Variant::Color(c) => {
                assert!((c.r - 128.0 / 255.0).abs() < 0.01);
                assert_eq!(c.r, c.g);
                assert!((c.a - 1.0).abs() < f32::EPSILON);
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn ctex_raw_la8_and_rgb8_decode() {
        let la = load_ctex_bytes(
            &build_ctex_image(1, 1, IMG_FORMAT_LA8, &[200, 100]),
            Path::new("la.ctex"),
        )
        .unwrap();
        assert_eq!(
            la.get_property("format"),
            Some(&Variant::String("LA".into()))
        );

        let rgb = load_ctex_bytes(
            &build_ctex_image(1, 1, IMG_FORMAT_RGB8, &[0, 255, 0]),
            Path::new("rgb.ctex"),
        )
        .unwrap();
        assert_eq!(
            rgb.get_property("format"),
            Some(&Variant::String("RGB".into()))
        );
        match &pixels_of(&rgb)[0] {
            Variant::Color(c) => assert!(c.g > 0.9 && c.r < 0.1 && c.b < 0.1),
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn ctex_bad_magic_errors() {
        let mut d = build_ctex_image(1, 1, IMG_FORMAT_RGBA8, &[0, 0, 0, 0]);
        d[0] = b'X';
        assert!(load_ctex_bytes(&d, Path::new("x.ctex")).is_err());
    }

    #[test]
    fn ctex_newer_version_unsupported() {
        let mut d = build_ctex_image(1, 1, IMG_FORMAT_RGBA8, &[0, 0, 0, 0]);
        d[4..8].copy_from_slice(&99u32.to_le_bytes()); // version field
        let err = load_ctex_bytes(&d, Path::new("v.ctex"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("version"), "got: {err}");
    }

    #[test]
    fn ctex_webp_payload_decodes_to_pixels() {
        // Replaces `ctex_webp_unsupported`, which was RED AT HEAD and pinned a
        // limitation that no longer exists (pat-s3rrv.48): WebP `.ctex` was
        // implemented by pat-nzgdu.8 — it is the format Godot's DEFAULT 2D
        // texture importer writes — but the test kept asserting an error whose
        // message contained "WebP". Nothing produces that message any more, so
        // the assertion could only ever fail. A stale test is worse than a
        // missing one: it reads as coverage while pinning the opposite of the
        // contract.
        let webp = include_bytes!("../fixtures/icon.webp");
        let d = build_ctex_webp(256, 256, webp);
        let res = load_ctex_bytes(&d, Path::new("w.ctex")).expect("WebP .ctex decodes");
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(256)));
        assert_eq!(res.get_property("height"), Some(&Variant::Int(256)));
        // Pixels, not just a header: the whole point of the WebP arm is that a
        // `.ctex`-sourced texture matches a source-image one.
        match res.get_property("pixels") {
            Some(Variant::Array(px)) => assert_eq!(
                px.len(),
                256 * 256,
                "one Color per pixel, or the payload was not really decoded",
            ),
            other => panic!("expected a pixels array, got {other:?}"),
        }
    }

    #[test]
    fn ctex_webp_truncated_payload_errors() {
        // The honest half of the old test: a WebP `.ctex` whose level-0 payload
        // is missing must fail on the BOUNDS CHECK rather than be mistaken for
        // an unsupported format. Every cursor read is length-checked, and this
        // pins that the WebP arm is no exception.
        let mut d = ctex_header(1, 1);
        ctex_inner(&mut d, CTEX_DATA_FORMAT_WEBP, 1, 1, IMG_FORMAT_RGBA8);
        let err = load_ctex_bytes(&d, Path::new("w.ctex"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("truncated"), "got: {err}");
    }

    #[test]
    fn ctex_basis_unsupported() {
        let mut d = ctex_header(1, 1);
        ctex_inner(
            &mut d,
            CTEX_DATA_FORMAT_BASIS_UNIVERSAL,
            1,
            1,
            IMG_FORMAT_RGBA8,
        );
        let err = load_ctex_bytes(&d, Path::new("b.ctex"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Basis"), "got: {err}");
    }

    #[test]
    fn ctex_unsupported_image_format_errors() {
        // FORMAT_RG8 (3) is a valid Godot format but unsupported here.
        let d = build_ctex_image(1, 1, 3, &[0, 0]);
        let err = load_ctex_bytes(&d, Path::new("rg.ctex"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not supported"), "got: {err}");
    }

    #[test]
    fn ctex_truncated_never_panics() {
        let full = build_ctex_png(1, 1, &tiny_red_png());
        assert!(load_ctex_bytes(&full, Path::new("f.ctex")).is_ok());
        for len in 0..full.len() {
            let _ = load_ctex_bytes(&full[..len], Path::new("t.ctex")); // must not panic
        }
        // Raw-image fixture too.
        let raw_full = build_ctex_image(2, 2, IMG_FORMAT_RGBA8, &[0u8; 16]);
        for len in 0..raw_full.len() {
            let _ = load_ctex_bytes(&raw_full[..len], Path::new("t.ctex"));
        }
    }

    // -- save_ctex_image_rgba8 (pat-s3rrv.2) ---------------------------------

    /// The writer's output must be readable by this module's own loader, with
    /// every pixel intact — that round-trip is the whole contract, since the
    /// artifacts it produces are what `import_image`'s remap path later reads.
    #[test]
    fn saved_ctex_round_trips_through_the_loader() {
        let raw: [u8; 16] = [
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            255, 255, 255, 0, // transparent white
        ];
        let encoded = save_ctex_image_rgba8(2, 2, &raw).expect("2x2 RGBA8 encodes");
        assert_eq!(&encoded[..4], CTEX_MAGIC, "artifact must carry GST2 magic");

        let res = load_ctex_bytes(&encoded, Path::new("out.ctex")).expect("round-trips");
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(2)));
        assert_eq!(res.get_property("height"), Some(&Variant::Int(2)));
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGBA".into()))
        );

        let pixels = match res.get_property("pixels") {
            Some(Variant::Array(px)) => px.clone(),
            other => panic!("expected a pixels Array, got {other:?}"),
        };
        assert_eq!(pixels.len(), 4);
        let at = |i: usize| match &pixels[i] {
            Variant::Color(c) => *c,
            other => panic!("pixel {i} is not a Color: {other:?}"),
        };
        // Channel-exact within one 8-bit step, in order, including alpha —
        // a writer that transposed or dropped a channel fails here.
        let near = |a: f32, b: f32| (a - b).abs() < 0.005;
        assert!(near(at(0).r, 1.0) && near(at(0).g, 0.0) && near(at(0).b, 0.0));
        assert!(near(at(1).r, 0.0) && near(at(1).g, 1.0) && near(at(1).b, 0.0));
        assert!(near(at(2).r, 0.0) && near(at(2).g, 0.0) && near(at(2).b, 1.0));
        assert!(
            near(at(3).a, 0.0),
            "alpha must survive the round-trip, got {}",
            at(3).a
        );
    }

    /// A payload that does not match the declared extent is rejected rather
    /// than written — a short artifact would fail far away, inside the loader.
    #[test]
    fn saved_ctex_rejects_a_mismatched_payload() {
        let err = save_ctex_image_rgba8(2, 2, &[0u8; 12])
            .unwrap_err()
            .to_string();
        assert!(err.contains("12 bytes"), "got: {err}");
        assert!(
            save_ctex_image_rgba8(1, 1, &[0u8; 4]).is_ok(),
            "1x1 is fine"
        );
        // Zero-sized is consistent (0 bytes) and must encode rather than error.
        assert!(save_ctex_image_rgba8(0, 0, &[]).is_ok());
    }

    /// The inner header stores the extent as u16, so anything larger cannot be
    /// represented — say so instead of silently truncating the dimensions.
    #[test]
    fn saved_ctex_rejects_dimensions_beyond_u16() {
        // Correctly sized, so this reaches the dimension check rather than
        // tripping the payload-length guard first.
        let payload = vec![0u8; 70_000 * 4];
        let err = save_ctex_image_rgba8(70_000, 1, &payload)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("65535"),
            "oversized dimensions must be refused by the u16 guard, got: {err}"
        );
    }
}
