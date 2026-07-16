//! GDExtension host slice (pat-z35r2.14).

/// Loaded extension metadata (no native code executed in this slice).
#[derive(Debug, Clone)]
pub struct GdExtensionMeta {
    pub library_path: String,
    pub entry_symbol: String,
}

/// Host that records registered extensions.
#[derive(Debug, Default)]
pub struct GdExtensionHost {
    pub extensions: Vec<GdExtensionMeta>,
}

impl GdExtensionHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a `.gdextension` library path (does not dlopen in this slice).
    pub fn register(&mut self, library_path: impl Into<String>, entry: impl Into<String>) {
        self.extensions.push(GdExtensionMeta {
            library_path: library_path.into(),
            entry_symbol: entry.into(),
        });
    }

    pub fn count(&self) -> usize {
        self.extensions.len()
    }

    /// Parses a `.gdextension` manifest and registers the metadata for the
    /// library matching `platform` (e.g. `"macos"`, `"linux"`, `"windows"`).
    ///
    /// Resolves the platform-specific library path and entry symbol from the
    /// manifest — the metadata half of loading an extension. No native code is
    /// `dlopen`'d or executed in this slice. Returns `true` when a matching
    /// library was found and registered.
    pub fn register_parsed(&mut self, manifest: &str, platform: &str) -> bool {
        let cfg = parse_gdextension(manifest);
        match cfg.library_for_platform(platform) {
            Some(path) if !cfg.entry_symbol.is_empty() => {
                self.register(path.to_string(), cfg.entry_symbol.clone());
                true
            }
            _ => false,
        }
    }
}

/// Parsed contents of a `.gdextension` manifest (Godot's INI-style extension
/// descriptor). Holds the `[configuration]` fields and the `[libraries]`
/// per-platform paths; other sections (icons, dependencies) are ignored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GdExtensionConfig {
    /// The `entry_symbol` — the exported initialization function name.
    pub entry_symbol: String,
    /// The `compatibility_minimum` Godot version string (empty if unspecified).
    pub compatibility_minimum: String,
    /// Whether the extension is `reloadable`.
    pub reloadable: bool,
    /// `(platform_tag, library_path)` pairs from the `[libraries]` section, in
    /// file order. Tags look like `linux.debug.x86_64` or `macos.release`.
    pub libraries: Vec<(String, String)>,
}

impl GdExtensionConfig {
    /// Returns the library path whose platform tag matches `platform` — an exact
    /// tag match, or a tag whose leading dot-segment is `platform` (e.g.
    /// `"macos"` matches `macos.debug`). Returns the first such entry, or `None`.
    ///
    /// This is a simplified match on the leading OS segment; full Godot feature-
    /// tag resolution (debug/release, architecture) is left to the caller.
    pub fn library_for_platform(&self, platform: &str) -> Option<&str> {
        self.libraries
            .iter()
            .find(|(tag, _)| {
                tag.as_str() == platform
                    || tag
                        .split_once('.')
                        .map(|(os, _)| os == platform)
                        .unwrap_or(false)
            })
            .map(|(_, path)| path.as_str())
    }
}

/// Parses a `.gdextension` manifest (INI-style). Recognizes the
/// `[configuration]` keys `entry_symbol`, `compatibility_minimum`, and
/// `reloadable`, and every `key = "path"` line under `[libraries]`. Values may
/// be double-quoted; `;` and `#` begin comments; unknown sections and keys are
/// ignored.
pub fn parse_gdextension(source: &str) -> GdExtensionConfig {
    let mut cfg = GdExtensionConfig::default();
    let mut section = String::new();

    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = inner.trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value.trim());
        match section.as_str() {
            "configuration" => match key {
                "entry_symbol" => cfg.entry_symbol = value,
                "compatibility_minimum" => cfg.compatibility_minimum = value,
                "reloadable" => cfg.reloadable = value.eq_ignore_ascii_case("true"),
                _ => {}
            },
            "libraries" => cfg.libraries.push((key.to_string(), value)),
            _ => {}
        }
    }
    cfg
}

/// Strips a single pair of surrounding double quotes, if present.
fn unquote(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_extension() {
        let mut h = GdExtensionHost::new();
        h.register("res://bin/demo.gdextension", "demo_init");
        assert_eq!(h.count(), 1);
    }

    #[test]
    fn parse_gdextension_manifest() {
        let src = r#"
; a comment
[configuration]
entry_symbol = "demo_library_init"
compatibility_minimum = "4.1"
reloadable = true

[libraries]
linux.debug.x86_64 = "res://bin/libdemo.linux.so"
macos.debug = "res://bin/libdemo.macos.framework"
windows.debug.x86_64 = "res://bin/demo.windows.dll"

[icons]
Demo = "res://icon.svg"
"#;
        let cfg = parse_gdextension(src);
        assert_eq!(cfg.entry_symbol, "demo_library_init");
        assert_eq!(cfg.compatibility_minimum, "4.1");
        assert!(cfg.reloadable);
        assert_eq!(cfg.libraries.len(), 3); // [icons] section ignored
        assert_eq!(
            cfg.library_for_platform("macos"),
            Some("res://bin/libdemo.macos.framework")
        );
        assert_eq!(
            cfg.library_for_platform("linux"),
            Some("res://bin/libdemo.linux.so")
        );
        assert_eq!(cfg.library_for_platform("web"), None);
    }

    #[test]
    fn register_parsed_selects_platform_library() {
        let src = "[configuration]\nentry_symbol = \"init_fn\"\n\
                   [libraries]\nmacos = \"res://m.framework\"\nlinux.release = \"res://l.so\"\n";
        let mut h = GdExtensionHost::new();
        assert!(h.register_parsed(src, "linux"));
        assert_eq!(h.count(), 1);
        assert_eq!(h.extensions[0].library_path, "res://l.so");
        assert_eq!(h.extensions[0].entry_symbol, "init_fn");
        // No matching library for this platform -> nothing registered.
        assert!(!h.register_parsed(src, "android"));
        assert_eq!(h.count(), 1);
    }

    #[test]
    fn parse_gdextension_empty_is_default() {
        let cfg = parse_gdextension("");
        assert_eq!(cfg, GdExtensionConfig::default());
        assert!(!cfg.reloadable);
        assert!(cfg.library_for_platform("linux").is_none());
    }
}
