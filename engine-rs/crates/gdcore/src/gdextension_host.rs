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
}
