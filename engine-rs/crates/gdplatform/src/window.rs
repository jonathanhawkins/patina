//! Window creation and management.
//!
//! Provides `WindowConfig` with builder-pattern construction, mirroring
//! Godot's project-settings window configuration.

/// Configuration for the application window.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowConfig {
    /// Window width in pixels.
    pub width: u32,
    /// Window height in pixels.
    pub height: u32,
    /// Window title.
    pub title: String,
    /// Whether the window is fullscreen.
    pub fullscreen: bool,
    /// Whether vsync is enabled.
    pub vsync: bool,
    /// Whether the window can be resized.
    pub resizable: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            title: "Patina Engine".to_string(),
            fullscreen: false,
            vsync: true,
            resizable: true,
        }
    }
}

impl WindowConfig {
    /// Creates a new `WindowConfig` with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the window width.
    pub fn with_width(mut self, width: u32) -> Self {
        self.width = width;
        self
    }

    /// Sets the window height.
    pub fn with_height(mut self, height: u32) -> Self {
        self.height = height;
        self
    }

    /// Sets both width and height.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Sets the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets fullscreen mode.
    pub fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Sets vsync.
    pub fn with_vsync(mut self, vsync: bool) -> Self {
        self.vsync = vsync;
        self
    }

    /// Sets whether the window is resizable.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_config_default_values() {
        let cfg = WindowConfig::default();
        assert_eq!(cfg.width, 1280);
        assert_eq!(cfg.height, 720);
        assert_eq!(cfg.title, "Patina Engine");
        assert!(!cfg.fullscreen);
        assert!(cfg.vsync);
        assert!(cfg.resizable);
    }

    #[test]
    fn window_config_builder_pattern() {
        let cfg = WindowConfig::new()
            .with_size(1920, 1080)
            .with_title("My Game")
            .with_fullscreen(true)
            .with_vsync(false)
            .with_resizable(false);

        assert_eq!(cfg.width, 1920);
        assert_eq!(cfg.height, 1080);
        assert_eq!(cfg.title, "My Game");
        assert!(cfg.fullscreen);
        assert!(!cfg.vsync);
        assert!(!cfg.resizable);
    }
}
