//! Binary resource importers, import-file parsing, format registry, and path resolution.
//!
//! Provides importers for common asset types (PNG, WAV, fonts), a parser for
//! Godot `.import` sidecar files, a [`ResourceFormatLoader`] registry that
//! dispatches by extension, and `res://` path resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdvariant::Variant;

use crate::loader::{ResourceLoader, TresLoader};
use crate::resource::Resource;

// ---------------------------------------------------------------------------
// res:// path resolution
// ---------------------------------------------------------------------------

/// Resolves a `res://` path to an absolute [`PathBuf`].
///
/// Strips the `res://` prefix and joins the remainder with `project_root`.
/// Returns an error if `res_path` does not start with `res://`.
pub fn resolve_res_path(project_root: &Path, res_path: &str) -> EngineResult<PathBuf> {
    let relative = res_path.strip_prefix("res://").ok_or_else(|| {
        EngineError::Parse(format!("path does not start with 'res://': {res_path}"))
    })?;
    Ok(project_root.join(relative))
}

// ---------------------------------------------------------------------------
// ImageImporter — PNG header metadata
// ---------------------------------------------------------------------------

/// PNG magic bytes.
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Reads a PNG file's IHDR chunk and creates a `Texture2D` resource.
///
/// Only the header is parsed — no pixel data is decoded.
pub fn import_image(path: &Path) -> EngineResult<Arc<Resource>> {
    let data = std::fs::read(path).map_err(EngineError::Io)?;

    if data.len() < 29 {
        return Err(EngineError::Parse("PNG file too short".into()));
    }
    if data[..8] != PNG_MAGIC {
        return Err(EngineError::Parse(
            "not a valid PNG file (bad magic)".into(),
        ));
    }

    // IHDR starts at byte 8: 4 bytes length, 4 bytes "IHDR", then 13 bytes of data.
    // Width:  bytes 16..20 (big-endian u32)
    // Height: bytes 20..24 (big-endian u32)
    // Bit depth: byte 24
    // Color type: byte 25
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    let bit_depth = data[24];
    let color_type = data[25];

    let format = match color_type {
        0 => "L", // Grayscale
        2 => "RGB",
        4 => "LA", // Grayscale + Alpha
        6 => "RGBA",
        3 => "Indexed",
        _ => "Unknown",
    };

    let mut res = Resource::new("Texture2D");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("width", Variant::Int(width as i64));
    res.set_property("height", Variant::Int(height as i64));
    res.set_property("format", Variant::String(format.into()));
    res.set_property("bit_depth", Variant::Int(bit_depth as i64));

    Ok(Arc::new(res))
}

// ---------------------------------------------------------------------------
// WavImporter — WAV header metadata
// ---------------------------------------------------------------------------

/// Reads a WAV file's RIFF/fmt chunk and creates an `AudioStreamWAV` resource.
///
/// Only the header is parsed — no sample data is decoded.
pub fn import_wav(path: &Path) -> EngineResult<Arc<Resource>> {
    let data = std::fs::read(path).map_err(EngineError::Io)?;

    if data.len() < 44 {
        return Err(EngineError::Parse("WAV file too short".into()));
    }
    if &data[0..4] != b"RIFF" {
        return Err(EngineError::Parse(
            "not a valid WAV file (missing RIFF)".into(),
        ));
    }
    if &data[8..12] != b"WAVE" {
        return Err(EngineError::Parse(
            "not a valid WAV file (missing WAVE)".into(),
        ));
    }

    // Find the fmt chunk — it usually starts at byte 12 but we'll search for it.
    let (fmt_offset, _fmt_size) = find_chunk(&data, b"fmt ")
        .ok_or_else(|| EngineError::Parse("WAV file missing fmt chunk".into()))?;

    let fmt = &data[fmt_offset..];
    if fmt.len() < 16 {
        return Err(EngineError::Parse("WAV fmt chunk too short".into()));
    }

    let audio_format = u16::from_le_bytes([fmt[0], fmt[1]]);
    let num_channels = u16::from_le_bytes([fmt[2], fmt[3]]);
    let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
    let bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);

    // Find the data chunk to compute length.
    let data_size = find_chunk(&data, b"data")
        .map(|(_, size)| size)
        .unwrap_or(0);

    let bytes_per_sample = (bits_per_sample as u32).max(1) / 8;
    let frame_size = (num_channels as u32) * bytes_per_sample;
    let total_frames = if frame_size > 0 {
        data_size / frame_size
    } else {
        0
    };
    let length_seconds = if sample_rate > 0 {
        total_frames as f64 / sample_rate as f64
    } else {
        0.0
    };

    let mut res = Resource::new("AudioStreamWAV");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("sample_rate", Variant::Int(sample_rate as i64));
    res.set_property("channels", Variant::Int(num_channels as i64));
    res.set_property("bit_depth", Variant::Int(bits_per_sample as i64));
    res.set_property("audio_format", Variant::Int(audio_format as i64));
    res.set_property("length_seconds", Variant::Float(length_seconds));

    Ok(Arc::new(res))
}

/// Finds a RIFF chunk by its 4-byte ID. Returns `(data_offset, data_size)`.
fn find_chunk(data: &[u8], id: &[u8; 4]) -> Option<(usize, u32)> {
    let mut offset = 12; // skip RIFF header (4 id + 4 size + 4 format)
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        if chunk_id == id {
            return Some((offset + 8, chunk_size));
        }
        // Advance past chunk (size is padded to even).
        offset += 8 + ((chunk_size as usize + 1) & !1);
    }
    None
}

// ---------------------------------------------------------------------------
// FontImporter — stub
// ---------------------------------------------------------------------------

/// Creates a stub `FontFile` resource for font files.
///
/// Does not parse the font — just records the path and a default size.
pub fn import_font(path: &Path) -> EngineResult<Arc<Resource>> {
    if !path.exists() {
        return Err(EngineError::NotFound(format!(
            "font file not found: {}",
            path.display()
        )));
    }

    let mut res = Resource::new("FontFile");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("path", Variant::String(path.to_string_lossy().into()));
    res.set_property("size", Variant::Int(16));

    Ok(Arc::new(res))
}

// ---------------------------------------------------------------------------
// ImportFileParser — .import sidecar files
// ---------------------------------------------------------------------------

/// Parsed contents of a Godot `.import` sidecar file.
#[derive(Debug, Clone, Default)]
pub struct ImportFile {
    /// `[remap]` section entries.
    pub remap: HashMap<String, String>,
    /// `[deps]` section entries.
    pub deps: HashMap<String, String>,
    /// Any other sections, keyed by section name.
    pub other_sections: HashMap<String, HashMap<String, String>>,
}

impl ImportFile {
    /// Convenience: the `importer` value from `[remap]`.
    pub fn importer(&self) -> Option<&str> {
        self.remap.get("importer").map(|s| s.as_str())
    }

    /// Convenience: the `type` value from `[remap]`.
    pub fn resource_type(&self) -> Option<&str> {
        self.remap.get("type").map(|s| s.as_str())
    }

    /// Convenience: the `source_file` value from `[deps]`.
    pub fn source_file(&self) -> Option<&str> {
        self.deps.get("source_file").map(|s| s.as_str())
    }

    /// Convenience: the `uid` value from `[remap]`.
    pub fn uid(&self) -> Option<&str> {
        self.remap.get("uid").map(|s| s.as_str())
    }

    /// Convenience: the `path` value from `[remap]`.
    pub fn import_path(&self) -> Option<&str> {
        self.remap.get("path").map(|s| s.as_str())
    }
}

/// Parses a `.import` sidecar file (INI-like format).
pub fn parse_import_file(contents: &str) -> EngineResult<ImportFile> {
    let mut result = ImportFile::default();
    let mut current_section = String::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        // Section header: [name]
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].to_string();
            continue;
        }

        // Key=value (or key = "value")
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let mut value = value.trim().to_string();

            // Strip surrounding quotes.
            if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                value = value[1..value.len() - 1].to_string();
            }

            match current_section.as_str() {
                "remap" => {
                    result.remap.insert(key, value);
                }
                "deps" => {
                    result.deps.insert(key, value);
                }
                section => {
                    result
                        .other_sections
                        .entry(section.to_string())
                        .or_default()
                        .insert(key, value);
                }
            }
        }
    }

    Ok(result)
}

/// Parses a `.import` sidecar file from disk.
pub fn load_import_file(path: &Path) -> EngineResult<ImportFile> {
    let contents = std::fs::read_to_string(path).map_err(EngineError::Io)?;
    parse_import_file(&contents)
}

// ---------------------------------------------------------------------------
// ResourceFormatLoader — extension-based registry
// ---------------------------------------------------------------------------

/// A loader function signature: takes a path, returns a resource.
pub type LoaderFn = fn(&Path) -> EngineResult<Arc<Resource>>;

/// Registry that maps file extensions to loader functions.
///
/// Call [`register`](ResourceFormatLoader::register) to add extension mappings,
/// then [`load_resource`](ResourceFormatLoader::load_resource) to dispatch.
#[derive(Default)]
pub struct ResourceFormatLoader {
    loaders: HashMap<String, LoaderFn>,
}

impl ResourceFormatLoader {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry pre-populated with the built-in loaders.
    pub fn with_defaults() -> Self {
        let mut rfl = Self::new();
        rfl.register(".png", import_image);
        rfl.register(".wav", import_wav);
        rfl.register(".ttf", import_font);
        rfl.register(".otf", import_font);
        rfl.register(".tres", tres_loader);
        rfl.register(".tscn", tscn_loader);
        rfl.register(".vs", vs_loader);
        rfl
    }

    /// Registers a loader for the given extension (including the dot, e.g. `".png"`).
    pub fn register(&mut self, extension: &str, loader: LoaderFn) {
        self.loaders.insert(extension.to_lowercase(), loader);
    }

    /// Returns the number of registered extensions.
    pub fn extension_count(&self) -> usize {
        self.loaders.len()
    }

    /// Returns `true` if a loader is registered for the extension.
    ///
    /// Accepts extensions with or without a leading dot (e.g. both `".vs"` and `"vs"`).
    pub fn can_load(&self, extension: &str) -> bool {
        let ext = extension.to_lowercase();
        if self.loaders.contains_key(&ext) {
            return true;
        }
        // Try with a leading dot if one wasn't provided
        if !ext.starts_with('.') {
            self.loaders.contains_key(&format!(".{ext}"))
        } else {
            false
        }
    }

    /// Loads a resource by dispatching to the registered loader for its extension.
    pub fn load_resource(&self, path: &Path) -> EngineResult<Arc<Resource>> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();

        let loader = self.loaders.get(&ext).ok_or_else(|| {
            EngineError::NotFound(format!("no loader registered for extension '{ext}'"))
        })?;

        loader(path)
    }
}

impl std::fmt::Debug for ResourceFormatLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceFormatLoader")
            .field("extensions", &self.loaders.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Loader function for `.tres` files.
fn tres_loader(path: &Path) -> EngineResult<Arc<Resource>> {
    TresLoader::new().load(path.to_str().unwrap_or(""))
}

/// Loader function for `.tscn` files.
///
/// Re-uses the `.tres` parser since `.tscn` uses the same text format.
fn tscn_loader(path: &Path) -> EngineResult<Arc<Resource>> {
    TresLoader::new().load(path.to_str().unwrap_or(""))
}

/// Loader function for `.vs` (VisualScript) files.
///
/// VisualScript was deprecated in Godot 4. This loader creates a stub
/// resource so that scenes referencing `.vs` files can load without errors.
fn vs_loader(path: &Path) -> EngineResult<Arc<Resource>> {
    if !path.exists() {
        return Err(EngineError::NotFound(format!(
            "VisualScript file not found: {}",
            path.display()
        )));
    }
    let mut res = Resource::new("VisualScript");
    res.path = format!(
        "res://{}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    res.set_property("_deprecated", Variant::Bool(true));
    Ok(Arc::new(res))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    // -- Minimal valid PNG ---------------------------------------------------

    /// Creates a minimal valid PNG with the given dimensions.
    fn make_minimal_png(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::new();
        // PNG magic
        data.extend_from_slice(&PNG_MAGIC);
        // IHDR chunk: length=13, "IHDR", 4 bytes width, 4 bytes height,
        // 1 byte bit_depth, 1 byte color_type, 3 bytes (compression, filter, interlace)
        // then 4 bytes CRC (we use zeros — not validated here).
        data.extend_from_slice(&13u32.to_be_bytes()); // chunk length
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.push(8); // bit depth
        data.push(6); // color type: RGBA
        data.push(0); // compression
        data.push(0); // filter
        data.push(0); // interlace
        data.extend_from_slice(&[0u8; 4]); // CRC placeholder
                                           // IEND chunk
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(b"IEND");
        data.extend_from_slice(&[0u8; 4]); // CRC placeholder
        data
    }

    // -- Minimal valid WAV ---------------------------------------------------

    /// Creates a minimal valid WAV with the given params.
    fn make_minimal_wav(
        sample_rate: u32,
        num_channels: u16,
        bits_per_sample: u16,
        num_samples: u32,
    ) -> Vec<u8> {
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = num_samples * num_channels as u32 * bits_per_sample as u32 / 8;
        let file_size = 36 + data_size;

        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&file_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        // fmt chunk
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        // data chunk
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        // Silence samples
        buf.resize(buf.len() + data_size as usize, 0);
        buf
    }

    // -- res:// path resolution -----------------------------------------------

    #[test]
    fn resolve_res_path_basic() {
        let root = PathBuf::from("/project");
        let resolved = resolve_res_path(&root, "res://icon.png").unwrap();
        assert_eq!(resolved, PathBuf::from("/project/icon.png"));
    }

    #[test]
    fn resolve_res_path_nested() {
        let root = PathBuf::from("/project");
        let resolved = resolve_res_path(&root, "res://assets/sprites/hero.png").unwrap();
        assert_eq!(resolved, PathBuf::from("/project/assets/sprites/hero.png"));
    }

    #[test]
    fn resolve_res_path_no_prefix_fails() {
        let root = PathBuf::from("/project");
        let result = resolve_res_path(&root, "/absolute/path.png");
        assert!(result.is_err());
    }

    // -- ImageImporter --------------------------------------------------------

    #[test]
    fn import_png_reads_dimensions() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.png");
        std::fs::write(&path, make_minimal_png(128, 64)).unwrap();

        let res = import_image(&path).unwrap();
        assert_eq!(res.class_name, "Texture2D");
        assert_eq!(res.get_property("width"), Some(&Variant::Int(128)));
        assert_eq!(res.get_property("height"), Some(&Variant::Int(64)));
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGBA".into()))
        );
        assert_eq!(res.get_property("bit_depth"), Some(&Variant::Int(8)));
    }

    #[test]
    fn import_png_rgb_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rgb.png");
        let mut data = make_minimal_png(32, 32);
        data[25] = 2; // color_type = RGB
        std::fs::write(&path, &data).unwrap();

        let res = import_image(&path).unwrap();
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("RGB".into()))
        );
    }

    #[test]
    fn import_png_grayscale_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("gray.png");
        let mut data = make_minimal_png(16, 16);
        data[25] = 0; // color_type = Grayscale
        std::fs::write(&path, &data).unwrap();

        let res = import_image(&path).unwrap();
        assert_eq!(
            res.get_property("format"),
            Some(&Variant::String("L".into()))
        );
    }

    #[test]
    fn import_png_too_short_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tiny.png");
        std::fs::write(&path, &[0x89, 0x50, 0x4E, 0x47]).unwrap();

        assert!(import_image(&path).is_err());
    }

    #[test]
    fn import_png_bad_magic_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.png");
        let mut data = make_minimal_png(1, 1);
        data[0] = 0x00; // corrupt magic
        std::fs::write(&path, &data).unwrap();

        assert!(import_image(&path).is_err());
    }

    // -- WavImporter ----------------------------------------------------------

    #[test]
    fn import_wav_reads_metadata() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wav");
        std::fs::write(&path, make_minimal_wav(44100, 2, 16, 44100)).unwrap();

        let res = import_wav(&path).unwrap();
        assert_eq!(res.class_name, "AudioStreamWAV");
        assert_eq!(res.get_property("sample_rate"), Some(&Variant::Int(44100)));
        assert_eq!(res.get_property("channels"), Some(&Variant::Int(2)));
        assert_eq!(res.get_property("bit_depth"), Some(&Variant::Int(16)));
        assert_eq!(res.get_property("audio_format"), Some(&Variant::Int(1)));
    }

    #[test]
    fn import_wav_length_seconds() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("onesec.wav");
        // 44100 samples at 44100 Hz = 1 second, mono 16-bit
        std::fs::write(&path, make_minimal_wav(44100, 1, 16, 44100)).unwrap();

        let res = import_wav(&path).unwrap();
        let length = match res.get_property("length_seconds") {
            Some(Variant::Float(f)) => *f,
            other => panic!("expected Float, got {other:?}"),
        };
        assert!((length - 1.0).abs() < 0.001);
    }

    #[test]
    fn import_wav_too_short_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tiny.wav");
        std::fs::write(&path, b"RIFF").unwrap();

        assert!(import_wav(&path).is_err());
    }

    #[test]
    fn import_wav_bad_riff_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.wav");
        let mut data = make_minimal_wav(44100, 1, 16, 100);
        data[0..4].copy_from_slice(b"NOPE");
        std::fs::write(&path, &data).unwrap();

        assert!(import_wav(&path).is_err());
    }

    // -- FontImporter ---------------------------------------------------------

    #[test]
    fn import_font_creates_resource() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.ttf");
        std::fs::write(&path, b"fake font data").unwrap();

        let res = import_font(&path).unwrap();
        assert_eq!(res.class_name, "FontFile");
        assert_eq!(res.get_property("size"), Some(&Variant::Int(16)));
        assert!(res.get_property("path").is_some());
    }

    #[test]
    fn import_font_nonexistent_fails() {
        assert!(import_font(Path::new("/nonexistent/font.ttf")).is_err());
    }

    // -- ImportFileParser -----------------------------------------------------

    #[test]
    fn parse_import_file_basic() {
        let contents = r#"[remap]

importer="texture"
type="CompressedTexture2D"
uid="uid://abc"
path="res://.godot/imported/icon.ctex"

[deps]

source_file="res://icon.png"
"#;
        let import = parse_import_file(contents).unwrap();
        assert_eq!(import.importer(), Some("texture"));
        assert_eq!(import.resource_type(), Some("CompressedTexture2D"));
        assert_eq!(import.uid(), Some("uid://abc"));
        assert_eq!(
            import.import_path(),
            Some("res://.godot/imported/icon.ctex")
        );
        assert_eq!(import.source_file(), Some("res://icon.png"));
    }

    #[test]
    fn parse_import_file_with_comments() {
        let contents = r#"; Godot import file
# Another comment

[remap]
importer="wav"
type="AudioStreamWAV"
"#;
        let import = parse_import_file(contents).unwrap();
        assert_eq!(import.importer(), Some("wav"));
    }

    #[test]
    fn parse_import_file_extra_sections() {
        let contents = r#"[remap]
importer="font"

[params]
antialiased=true
"#;
        let import = parse_import_file(contents).unwrap();
        assert_eq!(import.importer(), Some("font"));
        assert_eq!(
            import
                .other_sections
                .get("params")
                .and_then(|s| s.get("antialiased")),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn parse_import_file_empty() {
        let import = parse_import_file("").unwrap();
        assert!(import.remap.is_empty());
        assert!(import.deps.is_empty());
    }

    // -- ResourceFormatLoader -------------------------------------------------

    #[test]
    fn format_loader_register_and_dispatch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("icon.png");
        std::fs::write(&path, make_minimal_png(64, 64)).unwrap();

        let mut rfl = ResourceFormatLoader::new();
        rfl.register(".png", import_image);
        assert!(rfl.can_load(".png"));
        assert!(!rfl.can_load(".jpg"));

        let res = rfl.load_resource(&path).unwrap();
        assert_eq!(res.class_name, "Texture2D");
    }

    #[test]
    fn format_loader_with_defaults() {
        let rfl = ResourceFormatLoader::with_defaults();
        assert!(rfl.can_load(".png"));
        assert!(rfl.can_load(".wav"));
        assert!(rfl.can_load(".ttf"));
        assert!(rfl.can_load(".otf"));
        assert!(rfl.can_load(".tres"));
        assert!(rfl.can_load(".tscn"));
        assert_eq!(rfl.extension_count(), 7);
    }

    #[test]
    fn format_loader_unknown_extension_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("file.xyz");
        std::fs::write(&path, b"data").unwrap();

        let rfl = ResourceFormatLoader::new();
        assert!(rfl.load_resource(&path).is_err());
    }

    #[test]
    fn format_loader_loads_wav() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sound.wav");
        std::fs::write(&path, make_minimal_wav(22050, 1, 8, 1000)).unwrap();

        let rfl = ResourceFormatLoader::with_defaults();
        let res = rfl.load_resource(&path).unwrap();
        assert_eq!(res.class_name, "AudioStreamWAV");
        assert_eq!(res.get_property("sample_rate"), Some(&Variant::Int(22050)));
    }

    #[test]
    fn format_loader_loads_tres() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.tres");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[gd_resource type=\"Theme\" format=3]").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "[resource]").unwrap();
        writeln!(f, "name = \"TestTheme\"").unwrap();

        let rfl = ResourceFormatLoader::with_defaults();
        let res = rfl.load_resource(&path).unwrap();
        assert_eq!(res.class_name, "Theme");
    }

    #[test]
    fn format_loader_case_insensitive() {
        let mut rfl = ResourceFormatLoader::new();
        rfl.register(".png", import_image);
        assert!(rfl.can_load(".PNG"));
        assert!(rfl.can_load(".Png"));
    }
}
