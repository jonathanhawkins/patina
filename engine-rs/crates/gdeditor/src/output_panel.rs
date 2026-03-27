//! Output panel with log filtering and search.
//!
//! Implements Godot's Output panel which displays engine and script log
//! messages with filtering by severity level and full-text search.
//!
//! - **Log levels**: Error, Warning, Info, Debug (matching Godot's output categories).
//! - **Filtering**: Show/hide messages by level, toggle timestamps.
//! - **Search**: Case-insensitive substring search across message text.
//! - **Clear**: Clear all or filtered messages.
//! - **Capacity**: Configurable max message count with oldest-first eviction.

use std::collections::VecDeque;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Log level
// ---------------------------------------------------------------------------

/// Severity level for output messages.
///
/// Maps to Godot's output categories (Errors, Warnings, Messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LogLevel {
    /// Debug-level trace output.
    Debug,
    /// Informational messages.
    Info,
    /// Warnings (non-fatal issues).
    Warning,
    /// Errors (failures that may affect functionality).
    Error,
}

impl LogLevel {
    /// Returns the display label for this level.
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }

    /// Returns the Godot-style integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Debug => 0,
            Self::Info => 1,
            Self::Warning => 2,
            Self::Error => 3,
        }
    }

    /// Creates from a Godot-style integer.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Debug,
            1 => Self::Info,
            2 => Self::Warning,
            3 => Self::Error,
            _ => Self::Info,
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// LogMessage
// ---------------------------------------------------------------------------

/// A single output message with metadata.
#[derive(Debug, Clone)]
pub struct LogMessage {
    /// Unique sequential ID for this message.
    pub id: u64,
    /// Severity level.
    pub level: LogLevel,
    /// The message text.
    pub text: String,
    /// Optional source location (e.g. `"res://main.gd:42"`).
    pub source: Option<String>,
    /// Timestamp when the message was logged.
    pub timestamp: SystemTime,
}

impl LogMessage {
    /// Returns a formatted display string.
    ///
    /// Format: `[LEVEL] text` or `[LEVEL] source: text` if source is present.
    pub fn formatted(&self, show_timestamp: bool) -> String {
        let mut s = String::new();
        if show_timestamp {
            if let Ok(dur) = self.timestamp.duration_since(SystemTime::UNIX_EPOCH) {
                let secs = dur.as_secs();
                let hours = (secs / 3600) % 24;
                let mins = (secs / 60) % 60;
                let sec = secs % 60;
                s.push_str(&format!("[{:02}:{:02}:{:02}] ", hours, mins, sec));
            }
        }
        s.push('[');
        s.push_str(self.level.label());
        s.push_str("] ");
        if let Some(ref src) = self.source {
            s.push_str(src);
            s.push_str(": ");
        }
        s.push_str(&self.text);
        s
    }

    /// Returns whether this message's text contains the query (case-insensitive).
    pub fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let lower_text = self.text.to_lowercase();
        let lower_query = query.to_lowercase();
        lower_text.contains(&lower_query)
            || self
                .source
                .as_ref()
                .map(|s| s.to_lowercase().contains(&lower_query))
                .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// OutputFilter
// ---------------------------------------------------------------------------

/// Filter configuration for the output panel.
#[derive(Debug, Clone)]
pub struct OutputFilter {
    /// Whether to show debug messages.
    pub show_debug: bool,
    /// Whether to show info messages.
    pub show_info: bool,
    /// Whether to show warning messages.
    pub show_warnings: bool,
    /// Whether to show error messages.
    pub show_errors: bool,
    /// Search query (empty = show all).
    pub search_query: String,
    /// Whether to show timestamps in output.
    pub show_timestamps: bool,
}

impl Default for OutputFilter {
    fn default() -> Self {
        Self {
            show_debug: false,
            show_info: true,
            show_warnings: true,
            show_errors: true,
            search_query: String::new(),
            show_timestamps: true,
        }
    }
}

impl OutputFilter {
    /// Returns whether a message passes this filter.
    pub fn matches(&self, msg: &LogMessage) -> bool {
        let level_ok = match msg.level {
            LogLevel::Debug => self.show_debug,
            LogLevel::Info => self.show_info,
            LogLevel::Warning => self.show_warnings,
            LogLevel::Error => self.show_errors,
        };
        level_ok && msg.matches_search(&self.search_query)
    }
}

// ---------------------------------------------------------------------------
// OutputPanel
// ---------------------------------------------------------------------------

/// The editor output panel — a ring buffer of log messages with filtering
/// and search capabilities.
///
/// Mirrors Godot's Output panel behavior:
/// - Messages are stored in a bounded ring buffer (oldest evicted first).
/// - Filtering by level and search query.
/// - Counts per level for the filter toggle buttons.
/// - Clear all or just filtered messages.
#[derive(Debug)]
pub struct OutputPanel {
    /// All stored messages (bounded ring buffer).
    messages: VecDeque<LogMessage>,
    /// Maximum number of messages to retain.
    max_messages: usize,
    /// Next message ID.
    next_id: u64,
    /// Current filter configuration.
    pub filter: OutputFilter,
    /// Running counts per log level (including evicted messages for UI display).
    error_count: u64,
    warning_count: u64,
    info_count: u64,
    debug_count: u64,
}

impl Default for OutputPanel {
    fn default() -> Self {
        Self::new(10_000)
    }
}

impl OutputPanel {
    /// Creates a new output panel with the given maximum message capacity.
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: VecDeque::with_capacity(max_messages.min(1024)),
            max_messages,
            next_id: 1,
            filter: OutputFilter::default(),
            error_count: 0,
            warning_count: 0,
            info_count: 0,
            debug_count: 0,
        }
    }

    /// Pushes a new log message into the panel.
    ///
    /// If the buffer is full, the oldest message is evicted.
    pub fn push(&mut self, level: LogLevel, text: impl Into<String>) {
        self.push_with_source(level, text, None);
    }

    /// Pushes a log message with an optional source location.
    pub fn push_with_source(
        &mut self,
        level: LogLevel,
        text: impl Into<String>,
        source: Option<String>,
    ) {
        let msg = LogMessage {
            id: self.next_id,
            level,
            text: text.into(),
            source,
            timestamp: SystemTime::now(),
        };
        self.next_id += 1;

        match level {
            LogLevel::Error => self.error_count += 1,
            LogLevel::Warning => self.warning_count += 1,
            LogLevel::Info => self.info_count += 1,
            LogLevel::Debug => self.debug_count += 1,
        }

        if self.max_messages == 0 {
            return; // No storage capacity — count only.
        }
        if self.messages.len() >= self.max_messages {
            self.messages.pop_front();
        }
        self.messages.push_back(msg);
    }

    /// Convenience: push an error message.
    pub fn error(&mut self, text: impl Into<String>) {
        self.push(LogLevel::Error, text);
    }

    /// Convenience: push a warning message.
    pub fn warning(&mut self, text: impl Into<String>) {
        self.push(LogLevel::Warning, text);
    }

    /// Convenience: push an info message.
    pub fn info(&mut self, text: impl Into<String>) {
        self.push(LogLevel::Info, text);
    }

    /// Convenience: push a debug message.
    pub fn debug(&mut self, text: impl Into<String>) {
        self.push(LogLevel::Debug, text);
    }

    /// Returns all messages that pass the current filter.
    pub fn filtered_messages(&self) -> Vec<&LogMessage> {
        self.messages
            .iter()
            .filter(|m| self.filter.matches(m))
            .collect()
    }

    /// Returns the total number of stored messages (unfiltered).
    pub fn total_count(&self) -> usize {
        self.messages.len()
    }

    /// Returns the number of messages that pass the current filter.
    pub fn filtered_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| self.filter.matches(m))
            .count()
    }

    /// Returns the cumulative error count (including evicted messages).
    pub fn error_count(&self) -> u64 {
        self.error_count
    }

    /// Returns the cumulative warning count.
    pub fn warning_count(&self) -> u64 {
        self.warning_count
    }

    /// Returns the cumulative info count.
    pub fn info_count(&self) -> u64 {
        self.info_count
    }

    /// Returns the cumulative debug count.
    pub fn debug_count(&self) -> u64 {
        self.debug_count
    }

    /// Clears all messages and resets counts.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.error_count = 0;
        self.warning_count = 0;
        self.info_count = 0;
        self.debug_count = 0;
    }

    /// Sets the search query for filtering.
    pub fn set_search(&mut self, query: impl Into<String>) {
        self.filter.search_query = query.into();
    }

    /// Clears the search query.
    pub fn clear_search(&mut self) {
        self.filter.search_query.clear();
    }

    /// Toggles visibility of a specific log level.
    pub fn toggle_level(&mut self, level: LogLevel) {
        match level {
            LogLevel::Debug => self.filter.show_debug = !self.filter.show_debug,
            LogLevel::Info => self.filter.show_info = !self.filter.show_info,
            LogLevel::Warning => self.filter.show_warnings = !self.filter.show_warnings,
            LogLevel::Error => self.filter.show_errors = !self.filter.show_errors,
        }
    }

    /// Returns whether a level is currently visible.
    pub fn is_level_visible(&self, level: LogLevel) -> bool {
        match level {
            LogLevel::Debug => self.filter.show_debug,
            LogLevel::Info => self.filter.show_info,
            LogLevel::Warning => self.filter.show_warnings,
            LogLevel::Error => self.filter.show_errors,
        }
    }

    /// Toggles timestamp display.
    pub fn toggle_timestamps(&mut self) {
        self.filter.show_timestamps = !self.filter.show_timestamps;
    }

    /// Returns formatted output lines for the current filter.
    pub fn formatted_output(&self) -> Vec<String> {
        self.filtered_messages()
            .iter()
            .map(|m| m.formatted(self.filter.show_timestamps))
            .collect()
    }

    /// Returns the maximum message capacity.
    pub fn max_messages(&self) -> usize {
        self.max_messages
    }

    /// Searches messages and returns matching entries.
    ///
    /// This applies the search on top of the current level filters.
    pub fn search(&self, query: &str) -> Vec<&LogMessage> {
        self.messages
            .iter()
            .filter(|m| {
                let level_ok = match m.level {
                    LogLevel::Debug => self.filter.show_debug,
                    LogLevel::Info => self.filter.show_info,
                    LogLevel::Warning => self.filter.show_warnings,
                    LogLevel::Error => self.filter.show_errors,
                };
                level_ok && m.matches_search(query)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── LogLevel ─────────────────────────────────────────────────────

    #[test]
    fn log_level_labels() {
        assert_eq!(LogLevel::Debug.label(), "DEBUG");
        assert_eq!(LogLevel::Info.label(), "INFO");
        assert_eq!(LogLevel::Warning.label(), "WARNING");
        assert_eq!(LogLevel::Error.label(), "ERROR");
    }

    #[test]
    fn log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warning);
        assert!(LogLevel::Warning < LogLevel::Error);
    }

    #[test]
    fn log_level_godot_int_roundtrip() {
        for level in [
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warning,
            LogLevel::Error,
        ] {
            assert_eq!(LogLevel::from_godot_int(level.to_godot_int()), level);
        }
    }

    #[test]
    fn log_level_unknown_int_defaults_info() {
        assert_eq!(LogLevel::from_godot_int(99), LogLevel::Info);
    }

    #[test]
    fn log_level_display() {
        assert_eq!(format!("{}", LogLevel::Error), "ERROR");
    }

    // ── LogMessage ───────────────────────────────────────────────────

    #[test]
    fn message_formatted_without_source() {
        let msg = LogMessage {
            id: 1,
            level: LogLevel::Error,
            text: "something broke".into(),
            source: None,
            timestamp: SystemTime::UNIX_EPOCH,
        };
        let s = msg.formatted(false);
        assert_eq!(s, "[ERROR] something broke");
    }

    #[test]
    fn message_formatted_with_source() {
        let msg = LogMessage {
            id: 1,
            level: LogLevel::Warning,
            text: "unused var".into(),
            source: Some("res://main.gd:42".into()),
            timestamp: SystemTime::UNIX_EPOCH,
        };
        let s = msg.formatted(false);
        assert_eq!(s, "[WARNING] res://main.gd:42: unused var");
    }

    #[test]
    fn message_formatted_with_timestamp() {
        let msg = LogMessage {
            id: 1,
            level: LogLevel::Info,
            text: "hello".into(),
            source: None,
            timestamp: SystemTime::UNIX_EPOCH,
        };
        let s = msg.formatted(true);
        assert!(s.starts_with("[00:00:00]"), "got: {}", s);
        assert!(s.contains("[INFO] hello"));
    }

    #[test]
    fn message_search_case_insensitive() {
        let msg = LogMessage {
            id: 1,
            level: LogLevel::Info,
            text: "Hello World".into(),
            source: None,
            timestamp: SystemTime::UNIX_EPOCH,
        };
        assert!(msg.matches_search("hello"));
        assert!(msg.matches_search("WORLD"));
        assert!(msg.matches_search(""));
        assert!(!msg.matches_search("goodbye"));
    }

    #[test]
    fn message_search_matches_source() {
        let msg = LogMessage {
            id: 1,
            level: LogLevel::Info,
            text: "error occurred".into(),
            source: Some("res://player.gd".into()),
            timestamp: SystemTime::UNIX_EPOCH,
        };
        assert!(msg.matches_search("player"));
        assert!(msg.matches_search("error"));
    }

    // ── OutputFilter ─────────────────────────────────────────────────

    #[test]
    fn filter_default_shows_info_warn_error() {
        let f = OutputFilter::default();
        assert!(!f.show_debug);
        assert!(f.show_info);
        assert!(f.show_warnings);
        assert!(f.show_errors);
    }

    #[test]
    fn filter_matches_by_level() {
        let f = OutputFilter {
            show_debug: false,
            show_info: true,
            show_warnings: false,
            show_errors: true,
            search_query: String::new(),
            show_timestamps: true,
        };
        let make = |level| LogMessage {
            id: 1,
            level,
            text: "test".into(),
            source: None,
            timestamp: SystemTime::UNIX_EPOCH,
        };
        assert!(!f.matches(&make(LogLevel::Debug)));
        assert!(f.matches(&make(LogLevel::Info)));
        assert!(!f.matches(&make(LogLevel::Warning)));
        assert!(f.matches(&make(LogLevel::Error)));
    }

    #[test]
    fn filter_matches_by_search() {
        let f = OutputFilter {
            search_query: "needle".into(),
            ..Default::default()
        };
        let msg_yes = LogMessage {
            id: 1,
            level: LogLevel::Info,
            text: "found the needle here".into(),
            source: None,
            timestamp: SystemTime::UNIX_EPOCH,
        };
        let msg_no = LogMessage {
            id: 2,
            level: LogLevel::Info,
            text: "nothing to see".into(),
            source: None,
            timestamp: SystemTime::UNIX_EPOCH,
        };
        assert!(f.matches(&msg_yes));
        assert!(!f.matches(&msg_no));
    }

    #[test]
    fn filter_level_and_search_combined() {
        let f = OutputFilter {
            show_debug: false,
            search_query: "hello".into(),
            ..Default::default()
        };
        let msg = LogMessage {
            id: 1,
            level: LogLevel::Debug,
            text: "hello world".into(),
            source: None,
            timestamp: SystemTime::UNIX_EPOCH,
        };
        // Matches search but not level filter.
        assert!(!f.matches(&msg));
    }

    // ── OutputPanel ──────────────────────────────────────────────────

    #[test]
    fn panel_default() {
        let panel = OutputPanel::default();
        assert_eq!(panel.total_count(), 0);
        assert_eq!(panel.max_messages(), 10_000);
    }

    #[test]
    fn panel_push_and_count() {
        let mut panel = OutputPanel::new(100);
        panel.info("hello");
        panel.warning("warn");
        panel.error("err");
        panel.debug("dbg");

        assert_eq!(panel.total_count(), 4);
        assert_eq!(panel.info_count(), 1);
        assert_eq!(panel.warning_count(), 1);
        assert_eq!(panel.error_count(), 1);
        assert_eq!(panel.debug_count(), 1);
    }

    #[test]
    fn panel_filtered_messages() {
        let mut panel = OutputPanel::new(100);
        panel.info("info msg");
        panel.debug("debug msg");
        panel.error("error msg");

        // Default filter: show info, warning, error; hide debug.
        let filtered = panel.filtered_messages();
        assert_eq!(filtered.len(), 2); // info + error
    }

    #[test]
    fn panel_toggle_level() {
        let mut panel = OutputPanel::new(100);
        panel.debug("dbg");
        panel.info("info");

        assert!(!panel.is_level_visible(LogLevel::Debug));
        assert_eq!(panel.filtered_count(), 1); // only info

        panel.toggle_level(LogLevel::Debug);
        assert!(panel.is_level_visible(LogLevel::Debug));
        assert_eq!(panel.filtered_count(), 2); // info + debug

        panel.toggle_level(LogLevel::Debug);
        assert!(!panel.is_level_visible(LogLevel::Debug));
        assert_eq!(panel.filtered_count(), 1);
    }

    #[test]
    fn panel_search() {
        let mut panel = OutputPanel::new(100);
        panel.info("loading scene");
        panel.info("loading texture");
        panel.error("failed to load shader");

        let results = panel.search("load");
        assert_eq!(results.len(), 3);

        let results = panel.search("texture");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "loading texture");
    }

    #[test]
    fn panel_set_search_filter() {
        let mut panel = OutputPanel::new(100);
        panel.info("alpha");
        panel.info("beta");
        panel.info("gamma");

        panel.set_search("beta");
        assert_eq!(panel.filtered_count(), 1);

        panel.clear_search();
        assert_eq!(panel.filtered_count(), 3);
    }

    #[test]
    fn panel_clear() {
        let mut panel = OutputPanel::new(100);
        panel.info("a");
        panel.error("b");
        assert_eq!(panel.total_count(), 2);

        panel.clear();
        assert_eq!(panel.total_count(), 0);
        assert_eq!(panel.error_count(), 0);
        assert_eq!(panel.info_count(), 0);
    }

    #[test]
    fn panel_max_capacity_evicts_oldest() {
        let mut panel = OutputPanel::new(3);
        panel.info("first");
        panel.info("second");
        panel.info("third");
        panel.info("fourth");

        assert_eq!(panel.total_count(), 3);
        let msgs = panel.filtered_messages();
        assert_eq!(msgs[0].text, "second");
        assert_eq!(msgs[2].text, "fourth");
    }

    #[test]
    fn panel_counts_survive_eviction() {
        let mut panel = OutputPanel::new(2);
        panel.error("e1");
        panel.error("e2");
        panel.error("e3");

        assert_eq!(panel.total_count(), 2); // Only 2 in buffer.
        assert_eq!(panel.error_count(), 3); // But count tracks all.
    }

    #[test]
    fn panel_push_with_source() {
        let mut panel = OutputPanel::new(100);
        panel.push_with_source(
            LogLevel::Error,
            "null reference",
            Some("res://enemy.gd:15".into()),
        );

        let msgs = panel.filtered_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].source.as_deref(), Some("res://enemy.gd:15"));
    }

    #[test]
    fn panel_formatted_output() {
        let mut panel = OutputPanel::new(100);
        panel.filter.show_timestamps = false;
        panel.info("hello");
        panel.error("world");

        let lines = panel.formatted_output();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "[INFO] hello");
        assert_eq!(lines[1], "[ERROR] world");
    }

    #[test]
    fn panel_toggle_timestamps() {
        let mut panel = OutputPanel::new(100);
        assert!(panel.filter.show_timestamps);
        panel.toggle_timestamps();
        assert!(!panel.filter.show_timestamps);
        panel.toggle_timestamps();
        assert!(panel.filter.show_timestamps);
    }

    #[test]
    fn panel_sequential_ids() {
        let mut panel = OutputPanel::new(100);
        panel.info("a");
        panel.info("b");
        panel.info("c");

        let msgs = panel.filtered_messages();
        assert_eq!(msgs[0].id, 1);
        assert_eq!(msgs[1].id, 2);
        assert_eq!(msgs[2].id, 3);
    }

    #[test]
    fn panel_convenience_methods() {
        let mut panel = OutputPanel::new(100);
        panel.error("e");
        panel.warning("w");
        panel.info("i");
        panel.debug("d");

        assert_eq!(panel.error_count(), 1);
        assert_eq!(panel.warning_count(), 1);
        assert_eq!(panel.info_count(), 1);
        assert_eq!(panel.debug_count(), 1);
    }

    #[test]
    fn panel_empty_search_matches_all() {
        let mut panel = OutputPanel::new(100);
        panel.info("a");
        panel.info("b");
        panel.set_search("");
        assert_eq!(panel.filtered_count(), 2);
    }

    #[test]
    fn panel_search_across_levels() {
        let mut panel = OutputPanel::new(100);
        panel.filter.show_debug = true; // Enable debug for this test.
        panel.debug("loading debug data");
        panel.info("loading level");
        panel.warning("loading slow");
        panel.error("loading failed");

        panel.set_search("loading");
        assert_eq!(panel.filtered_count(), 4);

        panel.filter.show_debug = false;
        assert_eq!(panel.filtered_count(), 3);
    }

    #[test]
    fn panel_zero_capacity() {
        let mut panel = OutputPanel::new(0);
        panel.info("msg"); // Should not panic.
        assert_eq!(panel.total_count(), 0); // Evicted immediately.
        assert_eq!(panel.info_count(), 1); // Count still tracked.
    }
}
