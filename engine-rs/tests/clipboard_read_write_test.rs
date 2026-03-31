//! pat-pipjq: Clipboard read and write support.
//!
//! Validates:
//! 1. HeadlessClipboard text read/write round-trip
//! 2. ClipboardContent variants (Text, RichText, Image, Empty)
//! 3. Clear and overwrite behavior
//! 4. Primary selection (X11) with fallback
//! 5. Clipboard trait object usage
//! 6. Unicode and multiline text handling
//! 7. Content type switching (text -> image -> text)
//! 8. ClassDB registration of clipboard methods on DisplayServer
//! 9. Edge cases: empty strings, large text, repeated operations

use gdplatform::clipboard::{Clipboard, ClipboardContent, HeadlessClipboard};

// ── Basic read/write ──────────────────────────────────────────────────

#[test]
fn clipboard_starts_empty() {
    let cb = HeadlessClipboard::new();
    assert!(!cb.has_content());
    assert!(cb.get().is_empty());
}

#[test]
fn set_text_and_read_back() {
    let mut cb = HeadlessClipboard::new();
    cb.set_text("hello");
    assert!(cb.has_content());
    assert_eq!(cb.get().as_text(), Some("hello"));
}

#[test]
fn overwrite_text() {
    let mut cb = HeadlessClipboard::new();
    cb.set_text("first");
    cb.set_text("second");
    assert_eq!(cb.get().as_text(), Some("second"));
}

#[test]
fn clear_removes_content() {
    let mut cb = HeadlessClipboard::new();
    cb.set_text("data");
    cb.clear();
    assert!(!cb.has_content());
    assert!(cb.get().is_empty());
}

// ── Content variants ──────────────────────────────────────────────────

#[test]
fn text_content_classification() {
    let c = ClipboardContent::Text("test".to_string());
    assert!(c.is_text());
    assert!(!c.is_image());
    assert!(!c.is_empty());
    assert_eq!(c.as_text(), Some("test"));
}

#[test]
fn rich_text_exposes_plain_fallback() {
    let c = ClipboardContent::RichText {
        html: "<b>bold</b>".to_string(),
        plain: "bold".to_string(),
    };
    assert!(c.is_text());
    assert_eq!(c.as_text(), Some("bold"));
}

#[test]
fn image_content_classification() {
    let c = ClipboardContent::Image {
        width: 4,
        height: 4,
        rgba: vec![0u8; 64],
    };
    assert!(c.is_image());
    assert!(!c.is_text());
    assert!(c.as_text().is_none());
}

#[test]
fn empty_content_classification() {
    let c = ClipboardContent::Empty;
    assert!(c.is_empty());
    assert!(!c.is_text());
    assert!(!c.is_image());
}

// ── Rich text and image via set() ─────────────────────────────────────

#[test]
fn set_rich_text_content() {
    let mut cb = HeadlessClipboard::new();
    cb.set(ClipboardContent::RichText {
        html: "<em>italic</em>".to_string(),
        plain: "italic".to_string(),
    });
    assert!(cb.has_content());
    assert!(cb.get().is_text());
    assert_eq!(cb.get().as_text(), Some("italic"));
}

#[test]
fn set_image_content() {
    let mut cb = HeadlessClipboard::new();
    cb.set(ClipboardContent::Image {
        width: 2,
        height: 2,
        rgba: vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    });
    assert!(cb.has_content());
    assert!(cb.get().is_image());
}

// ── Primary selection ─────────────────────────────────────────────────

#[test]
fn no_primary_selection_by_default() {
    let cb = HeadlessClipboard::new();
    assert!(!cb.has_primary_selection());
}

#[test]
fn primary_fallback_to_clipboard_when_unsupported() {
    let mut cb = HeadlessClipboard::new();
    cb.set_text("clipboard");
    // Without primary support, get_primary returns clipboard content.
    assert_eq!(cb.get_primary().as_text(), Some("clipboard"));
}

#[test]
fn primary_selection_independent_from_clipboard() {
    let mut cb = HeadlessClipboard::with_primary_selection();
    assert!(cb.has_primary_selection());

    cb.set_text("clipboard content");
    cb.set_primary_text("primary content");

    assert_eq!(cb.get().as_text(), Some("clipboard content"));
    assert_eq!(cb.get_primary().as_text(), Some("primary content"));
}

#[test]
fn primary_set_ignored_without_support() {
    let mut cb = HeadlessClipboard::new();
    cb.set_primary_text("ignored");
    assert!(cb.get_primary().is_empty());
}

#[test]
fn primary_with_empty_clipboard() {
    let mut cb = HeadlessClipboard::with_primary_selection();
    cb.set_primary_text("selected");
    assert!(!cb.has_content()); // Clipboard is empty.
    assert_eq!(cb.get_primary().as_text(), Some("selected"));
}

// ── Trait object usage ────────────────────────────────────────────────

#[test]
fn clipboard_trait_object() {
    let mut cb: Box<dyn Clipboard> = Box::new(HeadlessClipboard::new());
    cb.set_text("via trait");
    assert_eq!(cb.get().as_text(), Some("via trait"));
    cb.clear();
    assert!(!cb.has_content());
}

// ── Unicode and special text ──────────────────────────────────────────

#[test]
fn unicode_text_roundtrip() {
    let mut cb = HeadlessClipboard::new();
    cb.set_text("Hello 🌍 世界 مرحبا Привет");
    assert_eq!(cb.get().as_text(), Some("Hello 🌍 世界 مرحبا Привет"));
}

#[test]
fn multiline_text_roundtrip() {
    let mut cb = HeadlessClipboard::new();
    let text = "line 1\nline 2\nline 3\n";
    cb.set_text(text);
    assert_eq!(cb.get().as_text(), Some(text));
}

#[test]
fn empty_string_is_still_content() {
    let mut cb = HeadlessClipboard::new();
    cb.set_text("");
    // Empty string is Text(""), not Empty.
    assert!(cb.has_content());
    assert!(cb.get().is_text());
    assert_eq!(cb.get().as_text(), Some(""));
}

#[test]
fn large_text() {
    let mut cb = HeadlessClipboard::new();
    let big = "x".repeat(100_000);
    cb.set_text(&big);
    assert_eq!(cb.get().as_text().unwrap().len(), 100_000);
}

// ── Content type switching ────────────────────────────────────────────

#[test]
fn switch_text_to_image_to_text() {
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
    assert_eq!(cb.get().as_text(), Some("back to text"));
}

#[test]
fn set_clear_set_cycle() {
    let mut cb = HeadlessClipboard::new();
    cb.set_text("a");
    cb.clear();
    assert!(cb.get().is_empty());
    cb.set_text("b");
    assert_eq!(cb.get().as_text(), Some("b"));
}

// ── ClassDB registration ──────────────────────────────────────────────

#[test]
fn classdb_display_server_has_clipboard_methods() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method(
        "DisplayServer",
        "clipboard_get"
    ));
    assert!(gdobject::class_db::class_has_method(
        "DisplayServer",
        "clipboard_set"
    ));
    assert!(gdobject::class_db::class_has_method(
        "DisplayServer",
        "clipboard_has"
    ));
    assert!(gdobject::class_db::class_has_method(
        "DisplayServer",
        "clipboard_get_primary"
    ));
    assert!(gdobject::class_db::class_has_method(
        "DisplayServer",
        "clipboard_set_primary"
    ));
}
