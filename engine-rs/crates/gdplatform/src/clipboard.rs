//! Clipboard read and write support.
//!
//! Provides a platform-agnostic clipboard interface mirroring Godot's
//! `DisplayServer.clipboard_*` methods. Supports text content and tracks
//! clipboard ownership for paste operations.
//!
//! The [`HeadlessClipboard`] implementation stores content in-memory for
//! testing. Platform-specific backends (winit, macOS, Linux) can implement
//! the [`Clipboard`] trait to interact with the system clipboard.

// ---------------------------------------------------------------------------
// ClipboardContent
// ---------------------------------------------------------------------------

/// The type of content stored on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardContent {
    /// Plain text content.
    Text(String),
    /// Rich text / HTML content with a plain-text fallback.
    RichText {
        /// HTML or rich text markup.
        html: String,
        /// Plain text fallback.
        plain: String,
    },
    /// Image data as raw RGBA bytes with dimensions.
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// The clipboard is empty.
    Empty,
}

impl ClipboardContent {
    /// Returns the content as plain text, if available.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ClipboardContent::Text(s) => Some(s),
            ClipboardContent::RichText { plain, .. } => Some(plain),
            _ => None,
        }
    }

    /// Returns `true` if the clipboard is empty.
    pub fn is_empty(&self) -> bool {
        matches!(self, ClipboardContent::Empty)
    }

    /// Returns `true` if the content is text (plain or rich).
    pub fn is_text(&self) -> bool {
        matches!(
            self,
            ClipboardContent::Text(_) | ClipboardContent::RichText { .. }
        )
    }

    /// Returns `true` if the content is an image.
    pub fn is_image(&self) -> bool {
        matches!(self, ClipboardContent::Image { .. })
    }
}

// ---------------------------------------------------------------------------
// Clipboard trait
// ---------------------------------------------------------------------------

/// Platform-agnostic clipboard interface.
///
/// Mirrors Godot's `DisplayServer` clipboard methods:
/// - `clipboard_get` / `clipboard_set`
/// - `clipboard_get_primary` / `clipboard_set_primary` (X11 primary selection)
/// - `clipboard_has`
pub trait Clipboard {
    /// Returns the current clipboard content.
    fn get(&self) -> ClipboardContent;

    /// Sets the clipboard content to the given text.
    fn set_text(&mut self, text: &str);

    /// Sets the clipboard to the given content.
    fn set(&mut self, content: ClipboardContent);

    /// Returns `true` if the clipboard has content.
    fn has_content(&self) -> bool;

    /// Clears the clipboard.
    fn clear(&mut self);

    /// Returns the primary selection content (X11 middle-click paste).
    /// Falls back to the regular clipboard on platforms without primary selection.
    fn get_primary(&self) -> ClipboardContent;

    /// Sets the primary selection content.
    fn set_primary_text(&mut self, text: &str);

    /// Returns `true` if this clipboard supports primary selection (X11).
    fn has_primary_selection(&self) -> bool;
}

// ---------------------------------------------------------------------------
// HeadlessClipboard
// ---------------------------------------------------------------------------

/// In-memory clipboard implementation for testing and headless mode.
///
/// Stores clipboard and primary selection content without interacting with
/// the OS. Useful for unit tests and CI environments.
#[derive(Debug, Clone)]
pub struct HeadlessClipboard {
    content: ClipboardContent,
    primary: ClipboardContent,
    /// Whether to simulate primary selection support (X11 behavior).
    primary_selection_supported: bool,
}

impl HeadlessClipboard {
    /// Creates a new empty headless clipboard.
    pub fn new() -> Self {
        Self {
            content: ClipboardContent::Empty,
            primary: ClipboardContent::Empty,
            primary_selection_supported: false,
        }
    }

    /// Creates a headless clipboard that simulates X11 primary selection.
    pub fn with_primary_selection() -> Self {
        Self {
            content: ClipboardContent::Empty,
            primary: ClipboardContent::Empty,
            primary_selection_supported: true,
        }
    }
}

impl Default for HeadlessClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for HeadlessClipboard {
    fn get(&self) -> ClipboardContent {
        self.content.clone()
    }

    fn set_text(&mut self, text: &str) {
        self.content = ClipboardContent::Text(text.to_string());
    }

    fn set(&mut self, content: ClipboardContent) {
        self.content = content;
    }

    fn has_content(&self) -> bool {
        !self.content.is_empty()
    }

    fn clear(&mut self) {
        self.content = ClipboardContent::Empty;
    }

    fn get_primary(&self) -> ClipboardContent {
        if self.primary_selection_supported {
            self.primary.clone()
        } else {
            self.content.clone()
        }
    }

    fn set_primary_text(&mut self, text: &str) {
        if self.primary_selection_supported {
            self.primary = ClipboardContent::Text(text.to_string());
        }
        // On platforms without primary selection, this is a no-op.
    }

    fn has_primary_selection(&self) -> bool {
        self.primary_selection_supported
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- ClipboardContent -----------------------------------------------------

    #[test]
    fn empty_content() {
        let c = ClipboardContent::Empty;
        assert!(c.is_empty());
        assert!(!c.is_text());
        assert!(!c.is_image());
        assert!(c.as_text().is_none());
    }

    #[test]
    fn text_content() {
        let c = ClipboardContent::Text("hello".to_string());
        assert!(!c.is_empty());
        assert!(c.is_text());
        assert!(!c.is_image());
        assert_eq!(c.as_text(), Some("hello"));
    }

    #[test]
    fn rich_text_content() {
        let c = ClipboardContent::RichText {
            html: "<b>bold</b>".to_string(),
            plain: "bold".to_string(),
        };
        assert!(c.is_text());
        assert_eq!(c.as_text(), Some("bold"));
    }

    #[test]
    fn image_content() {
        let c = ClipboardContent::Image {
            width: 2,
            height: 2,
            rgba: vec![0u8; 16],
        };
        assert!(c.is_image());
        assert!(!c.is_text());
        assert!(c.as_text().is_none());
    }

    // -- HeadlessClipboard basic ops ------------------------------------------

    #[test]
    fn default_is_empty() {
        let cb = HeadlessClipboard::new();
        assert!(!cb.has_content());
        assert!(cb.get().is_empty());
    }

    #[test]
    fn set_and_get_text() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("hello world");
        assert!(cb.has_content());
        assert_eq!(cb.get().as_text(), Some("hello world"));
    }

    #[test]
    fn set_empty_string() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("");
        // Empty string is still text content, not Empty.
        assert!(cb.has_content());
        assert_eq!(cb.get().as_text(), Some(""));
    }

    #[test]
    fn overwrite_content() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("first");
        cb.set_text("second");
        assert_eq!(cb.get().as_text(), Some("second"));
    }

    #[test]
    fn clear_clipboard() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("data");
        assert!(cb.has_content());
        cb.clear();
        assert!(!cb.has_content());
        assert!(cb.get().is_empty());
    }

    #[test]
    fn set_rich_text() {
        let mut cb = HeadlessClipboard::new();
        cb.set(ClipboardContent::RichText {
            html: "<em>italic</em>".to_string(),
            plain: "italic".to_string(),
        });
        assert!(cb.has_content());
        assert_eq!(cb.get().as_text(), Some("italic"));
    }

    #[test]
    fn set_image_content() {
        let mut cb = HeadlessClipboard::new();
        cb.set(ClipboardContent::Image {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        });
        assert!(cb.has_content());
        assert!(cb.get().is_image());
    }

    // -- Primary selection (X11) ----------------------------------------------

    #[test]
    fn no_primary_selection_by_default() {
        let cb = HeadlessClipboard::new();
        assert!(!cb.has_primary_selection());
    }

    #[test]
    fn primary_fallback_to_clipboard() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("clipboard data");
        // Without primary selection support, get_primary returns clipboard content.
        let primary = cb.get_primary();
        assert_eq!(primary.as_text(), Some("clipboard data"));
    }

    #[test]
    fn primary_selection_independent() {
        let mut cb = HeadlessClipboard::with_primary_selection();
        assert!(cb.has_primary_selection());

        cb.set_text("clipboard");
        cb.set_primary_text("primary");

        assert_eq!(cb.get().as_text(), Some("clipboard"));
        assert_eq!(cb.get_primary().as_text(), Some("primary"));
    }

    #[test]
    fn primary_set_ignored_without_support() {
        let mut cb = HeadlessClipboard::new();
        cb.set_primary_text("ignored");
        // Primary is not supported, so get_primary falls back to clipboard (empty).
        assert!(cb.get_primary().is_empty());
    }

    #[test]
    fn primary_selection_with_clipboard_empty() {
        let mut cb = HeadlessClipboard::with_primary_selection();
        cb.set_primary_text("selected text");
        // Clipboard is empty, primary has content.
        assert!(!cb.has_content());
        assert_eq!(cb.get_primary().as_text(), Some("selected text"));
    }

    // -- Trait object usage ---------------------------------------------------

    #[test]
    fn clipboard_as_trait_object() {
        let mut cb: Box<dyn Clipboard> = Box::new(HeadlessClipboard::new());
        cb.set_text("trait object");
        assert_eq!(cb.get().as_text(), Some("trait object"));
        assert!(cb.has_content());
        cb.clear();
        assert!(!cb.has_content());
    }

    // -- Round-trip and edge cases --------------------------------------------

    #[test]
    fn unicode_text() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("Hello 🌍 世界 مرحبا");
        assert_eq!(cb.get().as_text(), Some("Hello 🌍 世界 مرحبا"));
    }

    #[test]
    fn multiline_text() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("line 1\nline 2\nline 3");
        assert_eq!(cb.get().as_text(), Some("line 1\nline 2\nline 3"));
    }

    #[test]
    fn set_then_clear_then_set() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("first");
        cb.clear();
        cb.set_text("second");
        assert_eq!(cb.get().as_text(), Some("second"));
    }

    #[test]
    fn content_type_switching() {
        let mut cb = HeadlessClipboard::new();
        cb.set_text("text");
        assert!(cb.get().is_text());

        cb.set(ClipboardContent::Image {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        });
        assert!(cb.get().is_image());
        assert!(!cb.get().is_text());

        cb.set_text("back to text");
        assert!(cb.get().is_text());
    }
}
