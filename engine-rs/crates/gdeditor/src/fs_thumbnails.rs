//! FileSystem dock **thumbnail previews** (pat-y6e8m).
//!
//! The file list shows generated thumbnails for previewable assets — textures
//! and scene/resource files — while non-previewable files get none. Thumbnails
//! are cached per path and tagged with the source version they were generated
//! from; when a source file is edited (its content/version changes) the cache
//! regenerates that file's thumbnail so the dock always shows a current preview.

use std::collections::HashMap;

/// The kind of a file in the dock, which decides whether it gets a thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// An image/texture asset (png, jpg, …).
    Texture,
    /// A scene file (`.tscn`/`.scn`).
    Scene,
    /// Another previewable resource (`.tres`/`.res`).
    Resource,
    /// A non-previewable file (scripts, text, …).
    Other,
}

impl FileKind {
    /// Whether files of this kind get a generated thumbnail.
    pub fn previewable(self) -> bool {
        !matches!(self, FileKind::Other)
    }
}

/// A generated thumbnail for a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    /// The kind of source the thumbnail was generated from.
    pub kind: FileKind,
    /// The source version this thumbnail reflects.
    pub source_version: u64,
    /// A deterministic signature of the rendered pixels (derived from source).
    pub signature: u64,
    /// The thumbnail size in pixels (width, height).
    pub size: (u32, u32),
}

/// FNV-1a 64-bit hash — a small, deterministic content signature.
fn signature_of(content: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in content {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// A cache of file thumbnails keyed by path, refreshing on source change.
#[derive(Debug, Clone)]
pub struct ThumbnailCache {
    size: (u32, u32),
    entries: HashMap<String, Thumbnail>,
    generations: u64,
}

impl ThumbnailCache {
    /// Creates a cache that renders thumbnails at `size` pixels.
    pub fn new(size: (u32, u32)) -> Self {
        Self {
            size,
            entries: HashMap::new(),
            generations: 0,
        }
    }

    /// The number of thumbnails currently cached.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many thumbnails have been generated over the cache's lifetime
    /// (renders + refreshes). Re-requesting an up-to-date thumbnail does not
    /// increment this.
    pub fn generation_count(&self) -> u64 {
        self.generations
    }

    /// The cached thumbnail for `path`, if any.
    pub fn get(&self, path: &str) -> Option<&Thumbnail> {
        self.entries.get(path)
    }

    /// Requests the thumbnail for `path` (a `kind` file whose current source is
    /// `content` at `version`). Previewable kinds render on first request and
    /// re-render whenever `version` changes; non-previewable kinds return
    /// `None`. Returns the up-to-date thumbnail.
    pub fn thumbnail(
        &mut self,
        path: &str,
        kind: FileKind,
        content: &[u8],
        version: u64,
    ) -> Option<&Thumbnail> {
        if !kind.previewable() {
            return None;
        }
        let up_to_date = self
            .entries
            .get(path)
            .is_some_and(|t| t.source_version == version);
        if !up_to_date {
            let thumb = Thumbnail {
                kind,
                source_version: version,
                signature: signature_of(content),
                size: self.size,
            };
            self.entries.insert(path.to_string(), thumb);
            self.generations += 1;
        }
        self.entries.get(path)
    }

    /// Drops the cached thumbnail for `path` (e.g. when the file is deleted).
    pub fn invalidate(&mut self, path: &str) -> bool {
        self.entries.remove(path).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-y6e8m): texture and scene files show generated
    /// thumbnails and editing a source file refreshes its thumbnail.
    #[test]
    fn fs_dock_thumbnails_render_and_refresh() {
        let mut cache = ThumbnailCache::new((64, 64));
        assert!(cache.is_empty());

        // A texture renders a thumbnail.
        let tex = cache
            .thumbnail("icon.png", FileKind::Texture, b"\x89PNG-v1", 1)
            .expect("texture is previewable");
        assert_eq!(tex.kind, FileKind::Texture);
        assert_eq!(tex.size, (64, 64));
        assert_eq!(tex.source_version, 1);
        let tex_sig_v1 = tex.signature;
        assert_eq!(cache.generation_count(), 1);

        // A scene renders too.
        let scene = cache
            .thumbnail("level.tscn", FileKind::Scene, b"[gd_scene]", 1)
            .expect("scene is previewable");
        assert_eq!(scene.kind, FileKind::Scene);
        assert_eq!(cache.generation_count(), 2);
        assert_eq!(cache.len(), 2);

        // A non-previewable file gets no thumbnail and isn't cached.
        assert!(cache
            .thumbnail("readme.md", FileKind::Other, b"# hi", 1)
            .is_none());
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.generation_count(), 2);

        // Re-requesting an up-to-date thumbnail returns the cache without
        // regenerating.
        let again = cache
            .thumbnail("icon.png", FileKind::Texture, b"\x89PNG-v1", 1)
            .unwrap();
        assert_eq!(again.signature, tex_sig_v1);
        assert_eq!(cache.generation_count(), 2);

        // Editing the source (new content + bumped version) refreshes the
        // thumbnail: the signature changes and a new generation is counted.
        let refreshed = cache
            .thumbnail("icon.png", FileKind::Texture, b"\x89PNG-v2-edited", 2)
            .unwrap();
        assert_eq!(refreshed.source_version, 2);
        assert_ne!(refreshed.signature, tex_sig_v1, "edit refreshes the thumbnail");
        assert_eq!(cache.generation_count(), 3);

        // The dock reads the refreshed thumbnail back.
        assert_eq!(cache.get("icon.png").unwrap().source_version, 2);
    }

    /// Non-previewable kinds never cache; invalidation drops an entry.
    #[test]
    fn previewable_and_invalidate() {
        assert!(FileKind::Texture.previewable());
        assert!(FileKind::Scene.previewable());
        assert!(FileKind::Resource.previewable());
        assert!(!FileKind::Other.previewable());

        let mut cache = ThumbnailCache::new((32, 32));
        cache.thumbnail("a.tres", FileKind::Resource, b"data", 1);
        assert_eq!(cache.len(), 1);
        assert!(cache.invalidate("a.tres"));
        assert!(!cache.invalidate("a.tres"));
        assert!(cache.get("a.tres").is_none());
    }
}
