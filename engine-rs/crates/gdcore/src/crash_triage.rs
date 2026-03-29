//! Crash triage process for runtime regressions.
//!
//! Provides types and logic for classifying, prioritizing, and reporting
//! runtime crashes and regressions. This module codifies the triage process
//! so that crashes are handled consistently:
//!
//! 1. **Classify** the crash severity (P0–P3)
//! 2. **Identify** whether it's a new crash, known issue, or regression
//! 3. **Generate** a structured crash report with context
//! 4. **Prioritize** based on impact and recurrence

/// Crash severity levels, following standard triage conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// P0: Critical — engine cannot start, data loss, or blocks all testing.
    P0Critical,
    /// P1: High — major subsystem broken, no workaround, blocks CI.
    P1High,
    /// P2: Medium — feature broken but workaround exists, CI not blocked.
    P2Medium,
    /// P3: Low — cosmetic, edge case, or minor inconvenience.
    P3Low,
}

impl Severity {
    /// Returns the numeric priority (0 = highest).
    pub fn priority(&self) -> u8 {
        match self {
            Severity::P0Critical => 0,
            Severity::P1High => 1,
            Severity::P2Medium => 2,
            Severity::P3Low => 3,
        }
    }

    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Severity::P0Critical => "P0-Critical",
            Severity::P1High => "P1-High",
            Severity::P2Medium => "P2-Medium",
            Severity::P3Low => "P3-Low",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Classification of a crash relative to known history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashClassification {
    /// First time this crash has been observed.
    New,
    /// Crash matches a previously known and fixed issue that has returned.
    Regression,
    /// Crash matches a known open issue.
    KnownIssue,
    /// Crash cannot be reproduced.
    Unreproducible,
}

impl CrashClassification {
    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            CrashClassification::New => "New",
            CrashClassification::Regression => "Regression",
            CrashClassification::KnownIssue => "Known Issue",
            CrashClassification::Unreproducible => "Unreproducible",
        }
    }
}

impl std::fmt::Display for CrashClassification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// The subsystem where the crash originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subsystem {
    /// Scene tree (node lifecycle, parent/child management).
    SceneTree,
    /// Physics engine (2D or 3D).
    Physics,
    /// Rendering pipeline.
    Render,
    /// Resource loading and caching.
    Resources,
    /// ClassDB / object model.
    ClassDB,
    /// Script execution (GDScript interop).
    Scripting,
    /// Platform / windowing / input.
    Platform,
    /// Audio subsystem.
    Audio,
    /// Editor.
    Editor,
    /// Other / unknown.
    Other,
}

impl Subsystem {
    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Subsystem::SceneTree => "SceneTree",
            Subsystem::Physics => "Physics",
            Subsystem::Render => "Render",
            Subsystem::Resources => "Resources",
            Subsystem::ClassDB => "ClassDB",
            Subsystem::Scripting => "Scripting",
            Subsystem::Platform => "Platform",
            Subsystem::Audio => "Audio",
            Subsystem::Editor => "Editor",
            Subsystem::Other => "Other",
        }
    }
}

impl std::fmt::Display for Subsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// A structured crash report for triage.
#[derive(Debug, Clone)]
pub struct CrashReport {
    /// Short summary of the crash.
    pub summary: String,
    /// Severity classification.
    pub severity: Severity,
    /// Crash classification (new, regression, known).
    pub classification: CrashClassification,
    /// Subsystem where the crash originated.
    pub subsystem: Subsystem,
    /// Error message or panic message.
    pub error_message: String,
    /// Stack trace or backtrace (if available).
    pub backtrace: Option<String>,
    /// The test or scenario that triggered the crash.
    pub trigger: String,
    /// Number of times this crash has been observed.
    pub occurrence_count: u32,
    /// Whether this crash blocks CI.
    pub blocks_ci: bool,
    /// Related bead ID (if linked to a tracked issue).
    pub bead_id: Option<String>,
}

impl CrashReport {
    /// Creates a new crash report with required fields.
    pub fn new(
        summary: &str,
        severity: Severity,
        subsystem: Subsystem,
        error_message: &str,
        trigger: &str,
    ) -> Self {
        Self {
            summary: summary.to_string(),
            severity,
            classification: CrashClassification::New,
            subsystem,
            error_message: error_message.to_string(),
            backtrace: None,
            trigger: trigger.to_string(),
            occurrence_count: 1,
            blocks_ci: severity <= Severity::P1High,
            bead_id: None,
        }
    }

    /// Sets the crash classification.
    pub fn classify(mut self, classification: CrashClassification) -> Self {
        self.classification = classification;
        self
    }

    /// Sets the backtrace.
    pub fn with_backtrace(mut self, bt: &str) -> Self {
        self.backtrace = Some(bt.to_string());
        self
    }

    /// Sets the occurrence count.
    pub fn with_occurrences(mut self, count: u32) -> Self {
        self.occurrence_count = count;
        self
    }

    /// Links to a bead ID.
    pub fn with_bead(mut self, bead_id: &str) -> Self {
        self.bead_id = Some(bead_id.to_string());
        self
    }

    /// Returns true if this crash should escalate (P0 or regression).
    pub fn should_escalate(&self) -> bool {
        self.severity == Severity::P0Critical
            || self.classification == CrashClassification::Regression
    }

    /// Returns true if this crash blocks CI.
    pub fn is_ci_blocker(&self) -> bool {
        self.blocks_ci
    }
}

/// Triage queue that collects and prioritizes crash reports.
#[derive(Debug, Clone)]
pub struct TriageQueue {
    reports: Vec<CrashReport>,
}

impl TriageQueue {
    /// Creates a new empty triage queue.
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
        }
    }

    /// Adds a crash report to the queue.
    pub fn add(&mut self, report: CrashReport) {
        self.reports.push(report);
    }

    /// Returns the number of reports in the queue.
    pub fn len(&self) -> usize {
        self.reports.len()
    }

    /// Returns true if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }

    /// Returns reports sorted by severity (highest priority first).
    pub fn by_severity(&self) -> Vec<&CrashReport> {
        let mut sorted: Vec<&CrashReport> = self.reports.iter().collect();
        sorted.sort_by_key(|r| r.severity.priority());
        sorted
    }

    /// Returns only reports that block CI.
    pub fn ci_blockers(&self) -> Vec<&CrashReport> {
        self.reports.iter().filter(|r| r.is_ci_blocker()).collect()
    }

    /// Returns only regressions.
    pub fn regressions(&self) -> Vec<&CrashReport> {
        self.reports
            .iter()
            .filter(|r| r.classification == CrashClassification::Regression)
            .collect()
    }

    /// Returns only reports that should escalate.
    pub fn escalations(&self) -> Vec<&CrashReport> {
        self.reports
            .iter()
            .filter(|r| r.should_escalate())
            .collect()
    }

    /// Returns reports filtered by subsystem.
    pub fn by_subsystem(&self, subsystem: Subsystem) -> Vec<&CrashReport> {
        self.reports
            .iter()
            .filter(|r| r.subsystem == subsystem)
            .collect()
    }

    /// Returns a count of reports per severity level.
    pub fn severity_counts(&self) -> [(Severity, usize); 4] {
        let p0 = self
            .reports
            .iter()
            .filter(|r| r.severity == Severity::P0Critical)
            .count();
        let p1 = self
            .reports
            .iter()
            .filter(|r| r.severity == Severity::P1High)
            .count();
        let p2 = self
            .reports
            .iter()
            .filter(|r| r.severity == Severity::P2Medium)
            .count();
        let p3 = self
            .reports
            .iter()
            .filter(|r| r.severity == Severity::P3Low)
            .count();
        [
            (Severity::P0Critical, p0),
            (Severity::P1High, p1),
            (Severity::P2Medium, p2),
            (Severity::P3Low, p3),
        ]
    }

    /// Generates a triage summary report.
    pub fn render_report(&self) -> String {
        let mut out = String::new();

        out.push_str("================ Crash Triage Report ================\n\n");

        // Summary counts
        let counts = self.severity_counts();
        out.push_str("SEVERITY BREAKDOWN\n");
        for (sev, count) in &counts {
            if *count > 0 {
                out.push_str(&format!("  {}: {}\n", sev.label(), count));
            }
        }
        out.push_str(&format!("  Total: {}\n\n", self.len()));

        // CI blockers
        let blockers = self.ci_blockers();
        if !blockers.is_empty() {
            out.push_str(&format!("CI BLOCKERS ({})\n", blockers.len()));
            for r in &blockers {
                out.push_str(&format!(
                    "  [{}] {} — {} ({})\n",
                    r.severity, r.subsystem, r.summary, r.classification
                ));
            }
            out.push('\n');
        }

        // Regressions
        let regressions = self.regressions();
        if !regressions.is_empty() {
            out.push_str(&format!("REGRESSIONS ({})\n", regressions.len()));
            for r in &regressions {
                out.push_str(&format!(
                    "  [{}] {} — {}\n",
                    r.severity, r.subsystem, r.summary
                ));
            }
            out.push('\n');
        }

        // Full list by severity
        out.push_str("ALL REPORTS (by priority)\n");
        out.push_str(&format!(
            "{:<12} {:<12} {:<12} {:<40} {}\n",
            "Severity", "Class", "Subsystem", "Summary", "Trigger"
        ));
        out.push_str(&format!(
            "{:-<12} {:-<12} {:-<12} {:-<40} {:-<30}\n",
            "", "", "", "", ""
        ));
        for r in self.by_severity() {
            out.push_str(&format!(
                "{:<12} {:<12} {:<12} {:<40} {}\n",
                r.severity.label(),
                r.classification.label(),
                r.subsystem.label(),
                if r.summary.len() > 38 {
                    format!("{}...", &r.summary[..35])
                } else {
                    r.summary.clone()
                },
                r.trigger,
            ));
        }

        // Status line
        let has_p0 = counts[0].1 > 0;
        let has_regressions = !regressions.is_empty();
        out.push_str(&format!(
            "\nTriage status: {}\n",
            if has_p0 {
                "CRITICAL — P0 issues require immediate attention"
            } else if has_regressions {
                "ATTENTION — regressions detected, fix before release"
            } else if self.is_empty() {
                "GREEN — no crashes reported"
            } else {
                "YELLOW — issues present but no critical blockers"
            }
        ));

        out
    }
}

impl Default for TriageQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Classifies a crash based on known issue signatures.
///
/// Compares the error message against a list of known signatures.
/// Returns `Regression` if the error matches a previously fixed issue,
/// `KnownIssue` if it matches an open issue, or `New` otherwise.
pub fn classify_crash(
    error_message: &str,
    known_fixed: &[&str],
    known_open: &[&str],
) -> CrashClassification {
    for pattern in known_fixed {
        if error_message.contains(pattern) {
            return CrashClassification::Regression;
        }
    }
    for pattern in known_open {
        if error_message.contains(pattern) {
            return CrashClassification::KnownIssue;
        }
    }
    CrashClassification::New
}

/// Determines severity based on crash characteristics.
///
/// Rules:
/// - Panics in scene tree or resource loading → P0
/// - CI test failures → P1
/// - Rendering glitches with workaround → P2
/// - Edge case / cosmetic → P3
pub fn auto_severity(
    subsystem: Subsystem,
    is_panic: bool,
    blocks_ci: bool,
    has_workaround: bool,
) -> Severity {
    if is_panic
        && matches!(
            subsystem,
            Subsystem::SceneTree | Subsystem::Resources | Subsystem::ClassDB
        )
    {
        return Severity::P0Critical;
    }
    if blocks_ci || (is_panic && !has_workaround) {
        return Severity::P1High;
    }
    if has_workaround {
        return Severity::P2Medium;
    }
    Severity::P3Low
}

// ---------------------------------------------------------------------------
// StackFrame — a single parsed frame from a backtrace
// ---------------------------------------------------------------------------

/// A single parsed frame from a stack trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    /// Frame index (0 = innermost).
    pub index: u32,
    /// Function or symbol name (may be mangled).
    pub symbol: String,
    /// Source file path (if available).
    pub file: Option<String>,
    /// Line number in the source file (if available).
    pub line: Option<u32>,
    /// Column number (if available).
    pub column: Option<u32>,
}

impl StackFrame {
    /// Creates a new stack frame.
    pub fn new(index: u32, symbol: &str) -> Self {
        Self {
            index,
            symbol: symbol.to_string(),
            file: None,
            line: None,
            column: None,
        }
    }

    /// Sets the source location.
    pub fn with_location(mut self, file: &str, line: u32) -> Self {
        self.file = Some(file.to_string());
        self.line = Some(line);
        self
    }

    /// Returns the crate name extracted from the symbol (first path segment).
    pub fn crate_name(&self) -> Option<&str> {
        // Rust symbols: "gdcore::crash_triage::foo" → "gdcore"
        self.symbol.split("::").next()
    }

    /// Returns true if this frame is from engine code (gd* crates).
    pub fn is_engine_frame(&self) -> bool {
        self.crate_name()
            .map_or(false, |c| c.starts_with("gd") || c.starts_with("patina"))
    }
}

impl std::fmt::Display for StackFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "  {:>3}: {}", self.index, self.symbol)?;
        if let (Some(file), Some(line)) = (&self.file, self.line) {
            write!(f, "\n       at {}:{}", file, line)?;
            if let Some(col) = self.column {
                write!(f, ":{}", col)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CrashTrace — a complete parsed stack trace
// ---------------------------------------------------------------------------

/// A parsed and structured stack trace from a crash.
#[derive(Debug, Clone)]
pub struct CrashTrace {
    /// The panic/error message that triggered the trace.
    pub message: String,
    /// Parsed stack frames (innermost first).
    pub frames: Vec<StackFrame>,
    /// Thread name where the crash occurred.
    pub thread_name: Option<String>,
}

impl CrashTrace {
    /// Creates a new crash trace with a message and frames.
    pub fn new(message: &str, frames: Vec<StackFrame>) -> Self {
        Self {
            message: message.to_string(),
            frames,
            thread_name: None,
        }
    }

    /// Sets the thread name.
    pub fn with_thread(mut self, name: &str) -> Self {
        self.thread_name = Some(name.to_string());
        self
    }

    /// Returns only engine frames (filtering out std/system frames).
    pub fn engine_frames(&self) -> Vec<&StackFrame> {
        self.frames.iter().filter(|f| f.is_engine_frame()).collect()
    }

    /// Returns the top engine frame (most likely crash origin).
    pub fn top_engine_frame(&self) -> Option<&StackFrame> {
        self.frames.iter().find(|f| f.is_engine_frame())
    }

    /// Auto-detects the subsystem from the top engine frame's crate name.
    pub fn detect_subsystem(&self) -> Subsystem {
        let frame = match self.top_engine_frame() {
            Some(f) => f,
            None => return Subsystem::Other,
        };
        subsystem_from_crate(frame.crate_name().unwrap_or(""))
    }

    /// Generates a crash signature (fingerprint) for deduplication.
    ///
    /// The signature is based on the top N engine frames' symbols,
    /// making it stable across different runs and machines.
    pub fn signature(&self) -> String {
        let engine_frames = self.engine_frames();
        let sig_frames: Vec<&str> = engine_frames
            .iter()
            .take(3)
            .map(|f| f.symbol.as_str())
            .collect();

        if sig_frames.is_empty() {
            // Fallback to first 3 frames of any kind
            let any_frames: Vec<&str> = self
                .frames
                .iter()
                .take(3)
                .map(|f| f.symbol.as_str())
                .collect();
            return simple_hash(&any_frames.join("|"));
        }

        simple_hash(&sig_frames.join("|"))
    }

    /// Formats the trace as a human-readable string.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(ref thread) = self.thread_name {
            out.push_str(&format!("thread '{}' ", thread));
        }
        out.push_str(&format!("panicked at '{}'\n", self.message));
        out.push_str("stack backtrace:\n");
        for frame in &self.frames {
            out.push_str(&format!("{}\n", frame));
        }
        out
    }
}

/// Maps a crate name to a subsystem.
fn subsystem_from_crate(crate_name: &str) -> Subsystem {
    match crate_name {
        "gdscene" => Subsystem::SceneTree,
        "gdphysics2d" | "gdphysics3d" => Subsystem::Physics,
        "gdrender2d" | "gdrender3d" | "gdserver3d" => Subsystem::Render,
        "gdresource" => Subsystem::Resources,
        "gdobject" => Subsystem::ClassDB,
        "gdscript_interop" | "gdscript" => Subsystem::Scripting,
        "gdplatform" => Subsystem::Platform,
        "gdaudio" => Subsystem::Audio,
        "gdeditor" => Subsystem::Editor,
        _ => Subsystem::Other,
    }
}

/// Simple string hash for crash signatures.
/// Produces a hex string from a basic FNV-1a-like hash.
fn simple_hash(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

// ---------------------------------------------------------------------------
// parse_backtrace — parse a Rust backtrace string
// ---------------------------------------------------------------------------

/// Parses a Rust-style backtrace string into structured [`StackFrame`]s.
///
/// Handles the common format:
/// ```text
///    0: module::function
///              at /path/to/file.rs:42:5
///    1: another::function
/// ```
pub fn parse_backtrace(backtrace: &str) -> Vec<StackFrame> {
    let mut frames = Vec::new();
    let mut current_frame: Option<StackFrame> = None;

    for line in backtrace.lines() {
        let trimmed = line.trim();

        // Try to match a frame line: "N: symbol_name"
        if let Some(frame) = parse_frame_line(trimmed) {
            if let Some(prev) = current_frame.take() {
                frames.push(prev);
            }
            current_frame = Some(frame);
        } else if trimmed.starts_with("at ") {
            // Location line: "at /path/to/file.rs:42:5"
            if let Some(ref mut frame) = current_frame {
                if let Some((file, line, col)) = parse_location(trimmed) {
                    frame.file = Some(file);
                    frame.line = Some(line);
                    frame.column = col;
                }
            }
        }
    }

    if let Some(frame) = current_frame {
        frames.push(frame);
    }

    frames
}

/// Attempts to parse a frame header line like "  3: gdcore::crash_triage::foo".
fn parse_frame_line(line: &str) -> Option<StackFrame> {
    // Match pattern: optional whitespace, digits, colon, space, symbol
    let line = line.trim();
    let colon_pos = line.find(':')?;
    let index_str = line[..colon_pos].trim();
    let index: u32 = index_str.parse().ok()?;
    let symbol = line[colon_pos + 1..].trim();
    if symbol.is_empty() {
        return None;
    }
    Some(StackFrame::new(index, symbol))
}

/// Parses a location line like "at /path/file.rs:42:5".
fn parse_location(line: &str) -> Option<(String, u32, Option<u32>)> {
    let rest = line.strip_prefix("at ")?;
    // Find the last colon-separated numbers
    // Format: path:line or path:line:column
    let mut parts = rest.rsplitn(3, ':');
    let last = parts.next()?;

    // Try parsing as column number
    if let Ok(col) = last.parse::<u32>() {
        if let Some(line_str) = parts.next() {
            if let Ok(line_num) = line_str.parse::<u32>() {
                let file = parts.next().unwrap_or("").to_string();
                if !file.is_empty() {
                    return Some((file, line_num, Some(col)));
                }
            }
        }
    }

    // Try parsing as line number (no column)
    if let Ok(line_num) = last.parse::<u32>() {
        let file = parts
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(":");
        if !file.is_empty() {
            return Some((file, line_num, None));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// CrashReport from CrashTrace — automated report creation
// ---------------------------------------------------------------------------

impl CrashReport {
    /// Creates a crash report from a structured crash trace.
    ///
    /// Automatically detects the subsystem, generates a backtrace string,
    /// and classifies against known issue patterns.
    pub fn from_trace(trace: &CrashTrace, known_fixed: &[&str], known_open: &[&str]) -> Self {
        let subsystem = trace.detect_subsystem();
        let is_panic = trace.message.contains("panic")
            || trace.message.contains("unwrap")
            || trace.message.contains("index out of bounds")
            || trace.message.contains("overflow");

        let severity = auto_severity(subsystem, is_panic, false, false);
        let classification = classify_crash(&trace.message, known_fixed, known_open);

        let trigger = trace
            .top_engine_frame()
            .map(|f| f.symbol.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let mut report = CrashReport::new(
            &format!("[{}] {}", trace.signature(), &trace.message),
            severity,
            subsystem,
            &trace.message,
            &trigger,
        )
        .classify(classification);

        report.backtrace = Some(trace.render());
        report
    }
}

// ===========================================================================
// Auto-issue creation
// ===========================================================================

/// A generated issue from a crash report, ready to be filed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueTemplate {
    /// Issue title.
    pub title: String,
    /// Issue body in markdown.
    pub body: String,
    /// Suggested priority (maps to bead priority).
    pub priority: u8,
    /// Labels to apply.
    pub labels: Vec<String>,
    /// The crash signature used for deduplication.
    pub signature: String,
}

impl IssueTemplate {
    /// Renders the issue as a CLI command for `br create`.
    pub fn to_br_command(&self) -> String {
        let labels = self.labels.join(",");
        format!(
            "br create --title {:?} --priority {} --labels {} --description {:?}",
            self.title, self.priority, labels, self.body,
        )
    }
}

/// Generates an [`IssueTemplate`] from a [`CrashReport`].
///
/// The template includes a structured markdown body with crash details,
/// backtrace, and reproduction info.
pub fn generate_issue(report: &CrashReport) -> IssueTemplate {
    let title = format!(
        "[Crash][{}] {} — {}",
        report.severity.label(),
        report.subsystem.label(),
        truncate(&report.summary, 60),
    );

    let mut body = String::new();
    body.push_str("## Crash Report\n\n");
    body.push_str(&format!("**Severity:** {}\n", report.severity.label()));
    body.push_str(&format!(
        "**Classification:** {}\n",
        report.classification.label()
    ));
    body.push_str(&format!("**Subsystem:** {}\n", report.subsystem.label()));
    body.push_str(&format!("**Trigger:** `{}`\n", report.trigger));
    body.push_str(&format!("**Occurrences:** {}\n", report.occurrence_count));
    if report.blocks_ci {
        body.push_str("**CI Blocker:** Yes\n");
    }
    if let Some(ref bead) = report.bead_id {
        body.push_str(&format!("**Related bead:** {}\n", bead));
    }
    body.push('\n');

    body.push_str("### Error Message\n\n");
    body.push_str("```\n");
    body.push_str(&report.error_message);
    body.push_str("\n```\n\n");

    if let Some(ref bt) = report.backtrace {
        body.push_str("### Backtrace\n\n");
        body.push_str("```\n");
        body.push_str(bt);
        body.push_str("\n```\n\n");
    }

    body.push_str("### Acceptance Criteria\n\n");
    body.push_str("- [ ] Root cause identified\n");
    body.push_str("- [ ] Fix implemented with regression test\n");
    body.push_str("- [ ] No new crashes in related subsystem\n");
    if report.blocks_ci {
        body.push_str("- [ ] CI green after fix\n");
    }

    let priority = report.severity.priority();

    let mut labels = vec![
        "crash".to_string(),
        format!("subsystem:{}", report.subsystem.label().to_lowercase()),
    ];
    if report.classification == CrashClassification::Regression {
        labels.push("regression".to_string());
    }
    if report.blocks_ci {
        labels.push("ci-blocker".to_string());
    }

    // Generate signature from summary + error for dedup.
    let signature = simple_hash(&format!(
        "{}:{}:{}",
        report.subsystem.label(),
        report.error_message,
        report.trigger,
    ));

    IssueTemplate {
        title,
        body,
        priority,
        labels,
        signature,
    }
}

/// Truncates a string to `max_len` characters, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// An issue tracker that deduplicates crash-generated issues.
///
/// Tracks which crash signatures have already been filed so the same
/// crash doesn't generate multiple issues.
#[derive(Debug, Clone)]
pub struct AutoIssueTracker {
    /// Set of crash signatures that have already been filed.
    filed_signatures: std::collections::HashSet<String>,
    /// Generated issues (pending or filed).
    issues: Vec<IssueTemplate>,
}

impl AutoIssueTracker {
    /// Creates a new empty tracker.
    pub fn new() -> Self {
        Self {
            filed_signatures: std::collections::HashSet::new(),
            issues: Vec::new(),
        }
    }

    /// Attempts to file an issue from a crash report.
    ///
    /// Returns `Some(template)` if the issue is new (not a duplicate),
    /// or `None` if an issue with the same signature was already filed.
    pub fn file_from_report(&mut self, report: &CrashReport) -> Option<&IssueTemplate> {
        let template = generate_issue(report);
        if self.filed_signatures.contains(&template.signature) {
            return None;
        }
        self.filed_signatures.insert(template.signature.clone());
        self.issues.push(template);
        self.issues.last()
    }

    /// Processes all reports in a triage queue, filing new issues for each.
    ///
    /// Returns the number of new issues filed (excluding duplicates).
    pub fn process_queue(&mut self, queue: &TriageQueue) -> usize {
        let mut count = 0;
        for report in queue.by_severity() {
            if self.file_from_report(report).is_some() {
                count += 1;
            }
        }
        count
    }

    /// Returns all generated issues.
    pub fn issues(&self) -> &[IssueTemplate] {
        &self.issues
    }

    /// Returns the number of filed issues.
    pub fn filed_count(&self) -> usize {
        self.issues.len()
    }

    /// Returns true if a crash with the given signature has already been filed.
    pub fn is_duplicate(&self, signature: &str) -> bool {
        self.filed_signatures.contains(signature)
    }

    /// Marks a signature as already filed (e.g. loaded from persistence).
    pub fn mark_filed(&mut self, signature: &str) {
        self.filed_signatures.insert(signature.to_string());
    }

    /// Returns issues filtered by priority.
    pub fn issues_by_priority(&self, priority: u8) -> Vec<&IssueTemplate> {
        self.issues
            .iter()
            .filter(|i| i.priority == priority)
            .collect()
    }

    /// Renders a summary of all filed issues.
    pub fn render_summary(&self) -> String {
        if self.issues.is_empty() {
            return "No crash issues filed.\n".to_string();
        }
        let mut out = format!("Auto-filed crash issues: {}\n\n", self.issues.len());
        for (i, issue) in self.issues.iter().enumerate() {
            out.push_str(&format!(
                "  {}. [P{}] {} (sig: {})\n",
                i + 1,
                issue.priority,
                issue.title,
                &issue.signature[..8],
            ));
        }
        out
    }
}

impl Default for AutoIssueTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Automated stack trace collection
// ===========================================================================

/// A timestamped record of a collected crash.
#[derive(Debug, Clone)]
pub struct CrashRecord {
    /// Monotonic sequence number (assigned by the collector).
    pub seq: u64,
    /// Timestamp as seconds since an arbitrary epoch (e.g. process start).
    pub timestamp_secs: f64,
    /// The parsed crash trace.
    pub trace: CrashTrace,
    /// The crash signature (cached from trace).
    pub signature: String,
    /// Detected subsystem.
    pub subsystem: Subsystem,
}

/// Frequency statistics for a particular crash signature.
#[derive(Debug, Clone)]
pub struct CrashFrequency {
    /// The crash signature.
    pub signature: String,
    /// Total number of occurrences.
    pub count: u64,
    /// Timestamp of the first occurrence.
    pub first_seen: f64,
    /// Timestamp of the most recent occurrence.
    pub last_seen: f64,
    /// Representative error message.
    pub message: String,
    /// Detected subsystem.
    pub subsystem: Subsystem,
}

impl CrashFrequency {
    /// Returns the average interval between crashes in seconds.
    /// Returns `None` if only one occurrence.
    pub fn avg_interval_secs(&self) -> Option<f64> {
        if self.count <= 1 {
            return None;
        }
        let span = self.last_seen - self.first_seen;
        if span <= 0.0 {
            return None;
        }
        Some(span / (self.count - 1) as f64)
    }

    /// Returns true if the crash is accelerating (last interval < average).
    /// Requires at least 3 occurrences to be meaningful.
    pub fn is_accelerating(&self, timestamps: &[f64]) -> bool {
        if timestamps.len() < 3 {
            return false;
        }
        let avg = match self.avg_interval_secs() {
            Some(a) => a,
            None => return false,
        };
        let last_interval = timestamps[timestamps.len() - 1] - timestamps[timestamps.len() - 2];
        last_interval < avg * 0.8 // 20% faster than average
    }
}

/// Trend assessment for a crash signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashTrend {
    /// Single occurrence, no trend.
    Isolated,
    /// Stable — occurring at a roughly constant rate.
    Stable,
    /// Accelerating — getting more frequent.
    Accelerating,
    /// Decelerating — getting less frequent.
    Decelerating,
}

impl CrashTrend {
    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            CrashTrend::Isolated => "Isolated",
            CrashTrend::Stable => "Stable",
            CrashTrend::Accelerating => "Accelerating",
            CrashTrend::Decelerating => "Decelerating",
        }
    }
}

impl std::fmt::Display for CrashTrend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Automated crash collector that accumulates traces, deduplicates them,
/// and feeds them into the triage pipeline.
#[derive(Debug, Clone)]
pub struct CrashCollector {
    /// All collected crash records in order.
    records: Vec<CrashRecord>,
    /// Per-signature timestamp lists for frequency analysis.
    sig_timestamps: std::collections::HashMap<String, Vec<f64>>,
    /// Next sequence number.
    next_seq: u64,
    /// Maximum number of records to retain (0 = unlimited).
    max_records: usize,
    /// Known-fixed patterns for classification.
    known_fixed: Vec<String>,
    /// Known-open patterns for classification.
    known_open: Vec<String>,
}

impl CrashCollector {
    /// Creates a new collector with default settings.
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            sig_timestamps: std::collections::HashMap::new(),
            next_seq: 0,
            max_records: 10_000,
            known_fixed: Vec::new(),
            known_open: Vec::new(),
        }
    }

    /// Sets the maximum number of records to retain.
    pub fn with_max_records(mut self, max: usize) -> Self {
        self.max_records = max;
        self
    }

    /// Adds a known-fixed pattern for regression detection.
    pub fn add_known_fixed(&mut self, pattern: &str) {
        self.known_fixed.push(pattern.to_string());
    }

    /// Adds a known-open pattern for classification.
    pub fn add_known_open(&mut self, pattern: &str) {
        self.known_open.push(pattern.to_string());
    }

    /// Collects a crash trace at the given timestamp.
    ///
    /// Returns the sequence number assigned to the record.
    pub fn collect(&mut self, trace: CrashTrace, timestamp_secs: f64) -> u64 {
        let signature = trace.signature();
        let subsystem = trace.detect_subsystem();
        let seq = self.next_seq;
        self.next_seq += 1;

        let record = CrashRecord {
            seq,
            timestamp_secs,
            trace,
            signature: signature.clone(),
            subsystem,
        };

        self.sig_timestamps
            .entry(signature)
            .or_default()
            .push(timestamp_secs);

        self.records.push(record);

        // Evict oldest records if over capacity.
        if self.max_records > 0 && self.records.len() > self.max_records {
            self.records.remove(0);
        }

        seq
    }

    /// Collects a crash from a raw backtrace string and panic message.
    pub fn collect_from_backtrace(
        &mut self,
        message: &str,
        backtrace: &str,
        timestamp_secs: f64,
    ) -> u64 {
        let frames = parse_backtrace(backtrace);
        let trace = CrashTrace::new(message, frames);
        self.collect(trace, timestamp_secs)
    }

    /// Returns the total number of collected records.
    pub fn total_records(&self) -> usize {
        self.records.len()
    }

    /// Returns the number of unique crash signatures seen.
    pub fn unique_signatures(&self) -> usize {
        self.sig_timestamps.len()
    }

    /// Returns all records (in collection order).
    pub fn records(&self) -> &[CrashRecord] {
        &self.records
    }

    /// Returns records matching a specific signature.
    pub fn records_by_signature(&self, signature: &str) -> Vec<&CrashRecord> {
        self.records
            .iter()
            .filter(|r| r.signature == signature)
            .collect()
    }

    /// Returns records within a time window.
    pub fn records_in_window(&self, start: f64, end: f64) -> Vec<&CrashRecord> {
        self.records
            .iter()
            .filter(|r| r.timestamp_secs >= start && r.timestamp_secs <= end)
            .collect()
    }

    /// Returns frequency statistics for all observed signatures.
    pub fn frequencies(&self) -> Vec<CrashFrequency> {
        let mut result = Vec::new();
        for (sig, timestamps) in &self.sig_timestamps {
            if timestamps.is_empty() {
                continue;
            }
            // Find a representative record for message/subsystem.
            let rep = self
                .records
                .iter()
                .rfind(|r| r.signature == *sig);
            let (message, subsystem) = match rep {
                Some(r) => (r.trace.message.clone(), r.subsystem),
                None => continue,
            };
            result.push(CrashFrequency {
                signature: sig.clone(),
                count: timestamps.len() as u64,
                first_seen: timestamps[0],
                last_seen: *timestamps.last().unwrap(),
                message,
                subsystem,
            });
        }
        // Sort by count descending.
        result.sort_by(|a, b| b.count.cmp(&a.count));
        result
    }

    /// Returns the frequency data for a specific signature.
    pub fn frequency_of(&self, signature: &str) -> Option<CrashFrequency> {
        self.frequencies().into_iter().find(|f| f.signature == signature)
    }

    /// Determines the trend for a given crash signature.
    pub fn trend(&self, signature: &str) -> CrashTrend {
        let timestamps = match self.sig_timestamps.get(signature) {
            Some(ts) => ts,
            None => return CrashTrend::Isolated,
        };
        if timestamps.len() <= 1 {
            return CrashTrend::Isolated;
        }
        if timestamps.len() < 3 {
            return CrashTrend::Stable;
        }

        // Compare the last interval to the average interval.
        let total_span = timestamps.last().unwrap() - timestamps.first().unwrap();
        if total_span <= 0.0 {
            return CrashTrend::Stable;
        }
        let avg_interval = total_span / (timestamps.len() - 1) as f64;
        let last_interval = timestamps[timestamps.len() - 1] - timestamps[timestamps.len() - 2];

        if last_interval < avg_interval * 0.8 {
            CrashTrend::Accelerating
        } else if last_interval > avg_interval * 1.2 {
            CrashTrend::Decelerating
        } else {
            CrashTrend::Stable
        }
    }

    /// Builds a `TriageQueue` from all collected records, deduplicating by
    /// signature and setting occurrence counts from frequency data.
    pub fn build_triage_queue(&self) -> TriageQueue {
        let mut queue = TriageQueue::new();
        let known_fixed: Vec<&str> = self.known_fixed.iter().map(|s| s.as_str()).collect();
        let known_open: Vec<&str> = self.known_open.iter().map(|s| s.as_str()).collect();

        // One report per unique signature, with occurrence count.
        let mut seen_sigs = std::collections::HashSet::new();
        for record in &self.records {
            if !seen_sigs.insert(record.signature.clone()) {
                continue; // already processed this signature
            }
            let count = self
                .sig_timestamps
                .get(&record.signature)
                .map_or(1, |ts| ts.len() as u32);
            let report = CrashReport::from_trace(&record.trace, &known_fixed, &known_open)
                .with_occurrences(count);
            queue.add(report);
        }
        queue
    }

    /// Runs the full pipeline: collect → deduplicate → triage → issue generation.
    ///
    /// Returns the `AutoIssueTracker` with any newly filed issues.
    pub fn run_pipeline(&self) -> (TriageQueue, AutoIssueTracker) {
        let queue = self.build_triage_queue();
        let mut tracker = AutoIssueTracker::new();
        tracker.process_queue(&queue);
        (queue, tracker)
    }

    /// Returns a summary of the collector state.
    pub fn summary(&self) -> CollectorSummary {
        let freqs = self.frequencies();
        let top_crashers: Vec<(String, u64)> = freqs
            .iter()
            .take(5)
            .map(|f| (f.signature.clone(), f.count))
            .collect();
        let accelerating = freqs
            .iter()
            .filter(|f| {
                self.sig_timestamps
                    .get(&f.signature)
                    .map_or(false, |ts| f.is_accelerating(ts))
            })
            .count();

        CollectorSummary {
            total_records: self.records.len() as u64,
            unique_signatures: self.sig_timestamps.len() as u64,
            top_crashers,
            accelerating_count: accelerating as u64,
        }
    }

    /// Renders a human-readable collection report.
    pub fn render_report(&self) -> String {
        let summary = self.summary();
        let mut out = String::new();
        out.push_str("============ Crash Collection Report ============\n\n");
        out.push_str(&format!(
            "Total crashes collected: {}\n",
            summary.total_records
        ));
        out.push_str(&format!(
            "Unique crash signatures: {}\n",
            summary.unique_signatures
        ));
        out.push_str(&format!(
            "Accelerating crashes:    {}\n\n",
            summary.accelerating_count
        ));

        if !summary.top_crashers.is_empty() {
            out.push_str("TOP CRASHERS\n");
            for (i, (sig, count)) in summary.top_crashers.iter().enumerate() {
                let trend = self.trend(sig);
                out.push_str(&format!(
                    "  {}. sig:{} — {} occurrences ({})\n",
                    i + 1,
                    &sig[..8.min(sig.len())],
                    count,
                    trend,
                ));
            }
            out.push('\n');
        }

        out
    }

    /// Clears all collected data.
    pub fn clear(&mut self) {
        self.records.clear();
        self.sig_timestamps.clear();
        self.next_seq = 0;
    }
}

impl Default for CrashCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of collector state.
#[derive(Debug, Clone)]
pub struct CollectorSummary {
    /// Total number of crash records.
    pub total_records: u64,
    /// Number of unique crash signatures.
    pub unique_signatures: u64,
    /// Top crashers by frequency: (signature, count).
    pub top_crashers: Vec<(String, u64)>,
    /// Number of crash signatures with accelerating trend.
    pub accelerating_count: u64,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Severity -----------------------------------------------------------

    #[test]
    fn severity_ordering() {
        assert!(Severity::P0Critical < Severity::P1High);
        assert!(Severity::P1High < Severity::P2Medium);
        assert!(Severity::P2Medium < Severity::P3Low);
    }

    #[test]
    fn severity_priority_numbers() {
        assert_eq!(Severity::P0Critical.priority(), 0);
        assert_eq!(Severity::P1High.priority(), 1);
        assert_eq!(Severity::P2Medium.priority(), 2);
        assert_eq!(Severity::P3Low.priority(), 3);
    }

    #[test]
    fn severity_display() {
        assert_eq!(format!("{}", Severity::P0Critical), "P0-Critical");
        assert_eq!(format!("{}", Severity::P3Low), "P3-Low");
    }

    // -- CrashClassification ------------------------------------------------

    #[test]
    fn classification_labels() {
        assert_eq!(CrashClassification::New.label(), "New");
        assert_eq!(CrashClassification::Regression.label(), "Regression");
        assert_eq!(CrashClassification::KnownIssue.label(), "Known Issue");
        assert_eq!(
            CrashClassification::Unreproducible.label(),
            "Unreproducible"
        );
    }

    // -- CrashReport --------------------------------------------------------

    #[test]
    fn crash_report_defaults() {
        let r = CrashReport::new(
            "scene tree panic",
            Severity::P0Critical,
            Subsystem::SceneTree,
            "index out of bounds",
            "lifecycle_test",
        );
        assert_eq!(r.classification, CrashClassification::New);
        assert!(r.blocks_ci);
        assert!(r.backtrace.is_none());
        assert_eq!(r.occurrence_count, 1);
    }

    #[test]
    fn crash_report_builder() {
        let r = CrashReport::new(
            "render glitch",
            Severity::P2Medium,
            Subsystem::Render,
            "pixel mismatch",
            "golden_test",
        )
        .classify(CrashClassification::KnownIssue)
        .with_backtrace("at render.rs:42")
        .with_occurrences(3)
        .with_bead("pat-abc1");

        assert_eq!(r.classification, CrashClassification::KnownIssue);
        assert_eq!(r.backtrace.as_deref(), Some("at render.rs:42"));
        assert_eq!(r.occurrence_count, 3);
        assert_eq!(r.bead_id.as_deref(), Some("pat-abc1"));
    }

    #[test]
    fn p0_should_escalate() {
        let r = CrashReport::new(
            "critical",
            Severity::P0Critical,
            Subsystem::SceneTree,
            "panic",
            "test",
        );
        assert!(r.should_escalate());
    }

    #[test]
    fn regression_should_escalate() {
        let r = CrashReport::new(
            "regressed",
            Severity::P2Medium,
            Subsystem::Render,
            "err",
            "test",
        )
        .classify(CrashClassification::Regression);
        assert!(r.should_escalate());
    }

    #[test]
    fn p3_new_should_not_escalate() {
        let r = CrashReport::new("minor", Severity::P3Low, Subsystem::Other, "warn", "test");
        assert!(!r.should_escalate());
    }

    #[test]
    fn p0_p1_block_ci() {
        let r0 = CrashReport::new("a", Severity::P0Critical, Subsystem::SceneTree, "e", "t");
        let r1 = CrashReport::new("b", Severity::P1High, Subsystem::Physics, "e", "t");
        let r2 = CrashReport::new("c", Severity::P2Medium, Subsystem::Render, "e", "t");
        assert!(r0.is_ci_blocker());
        assert!(r1.is_ci_blocker());
        assert!(!r2.is_ci_blocker());
    }

    // -- classify_crash -----------------------------------------------------

    #[test]
    fn classify_new_crash() {
        let result = classify_crash("never seen before", &["old bug"], &["open bug"]);
        assert_eq!(result, CrashClassification::New);
    }

    #[test]
    fn classify_regression() {
        let result = classify_crash("old bug resurfaces", &["old bug"], &["open bug"]);
        assert_eq!(result, CrashClassification::Regression);
    }

    #[test]
    fn classify_known_issue() {
        let result = classify_crash("open bug still there", &["fixed thing"], &["open bug"]);
        assert_eq!(result, CrashClassification::KnownIssue);
    }

    #[test]
    fn classify_regression_takes_priority_over_known() {
        // If error matches both fixed and open, regression wins.
        let result = classify_crash("shared pattern", &["shared"], &["shared"]);
        assert_eq!(result, CrashClassification::Regression);
    }

    // -- auto_severity ------------------------------------------------------

    #[test]
    fn auto_severity_scene_tree_panic_is_p0() {
        assert_eq!(
            auto_severity(Subsystem::SceneTree, true, false, false),
            Severity::P0Critical
        );
    }

    #[test]
    fn auto_severity_ci_blocker_is_p1() {
        assert_eq!(
            auto_severity(Subsystem::Render, false, true, false),
            Severity::P1High
        );
    }

    #[test]
    fn auto_severity_with_workaround_is_p2() {
        assert_eq!(
            auto_severity(Subsystem::Render, false, false, true),
            Severity::P2Medium
        );
    }

    #[test]
    fn auto_severity_minor_is_p3() {
        assert_eq!(
            auto_severity(Subsystem::Other, false, false, false),
            Severity::P3Low
        );
    }

    // -- TriageQueue --------------------------------------------------------

    #[test]
    fn empty_queue() {
        let q = TriageQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn queue_sorts_by_severity() {
        let mut q = TriageQueue::new();
        q.add(CrashReport::new(
            "low",
            Severity::P3Low,
            Subsystem::Other,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "critical",
            Severity::P0Critical,
            Subsystem::SceneTree,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "medium",
            Severity::P2Medium,
            Subsystem::Render,
            "e",
            "t",
        ));

        let sorted = q.by_severity();
        assert_eq!(sorted[0].severity, Severity::P0Critical);
        assert_eq!(sorted[1].severity, Severity::P2Medium);
        assert_eq!(sorted[2].severity, Severity::P3Low);
    }

    #[test]
    fn queue_filters_ci_blockers() {
        let mut q = TriageQueue::new();
        q.add(CrashReport::new(
            "a",
            Severity::P0Critical,
            Subsystem::SceneTree,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "b",
            Severity::P3Low,
            Subsystem::Other,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "c",
            Severity::P1High,
            Subsystem::Physics,
            "e",
            "t",
        ));

        let blockers = q.ci_blockers();
        assert_eq!(blockers.len(), 2); // P0 and P1
    }

    #[test]
    fn queue_filters_regressions() {
        let mut q = TriageQueue::new();
        q.add(
            CrashReport::new("a", Severity::P2Medium, Subsystem::Render, "e", "t")
                .classify(CrashClassification::Regression),
        );
        q.add(CrashReport::new(
            "b",
            Severity::P3Low,
            Subsystem::Other,
            "e",
            "t",
        ));

        let regressions = q.regressions();
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].summary, "a");
    }

    #[test]
    fn queue_filters_by_subsystem() {
        let mut q = TriageQueue::new();
        q.add(CrashReport::new(
            "a",
            Severity::P1High,
            Subsystem::Physics,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "b",
            Severity::P2Medium,
            Subsystem::Physics,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "c",
            Severity::P3Low,
            Subsystem::Render,
            "e",
            "t",
        ));

        assert_eq!(q.by_subsystem(Subsystem::Physics).len(), 2);
        assert_eq!(q.by_subsystem(Subsystem::Render).len(), 1);
        assert_eq!(q.by_subsystem(Subsystem::SceneTree).len(), 0);
    }

    #[test]
    fn severity_counts_tracked() {
        let mut q = TriageQueue::new();
        q.add(CrashReport::new(
            "a",
            Severity::P0Critical,
            Subsystem::SceneTree,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "b",
            Severity::P1High,
            Subsystem::Physics,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "c",
            Severity::P1High,
            Subsystem::Render,
            "e",
            "t",
        ));
        q.add(CrashReport::new(
            "d",
            Severity::P3Low,
            Subsystem::Other,
            "e",
            "t",
        ));

        let counts = q.severity_counts();
        assert_eq!(counts[0], (Severity::P0Critical, 1));
        assert_eq!(counts[1], (Severity::P1High, 2));
        assert_eq!(counts[2], (Severity::P2Medium, 0));
        assert_eq!(counts[3], (Severity::P3Low, 1));
    }

    #[test]
    fn render_report_contains_sections() {
        let mut q = TriageQueue::new();
        q.add(CrashReport::new(
            "scene crash",
            Severity::P0Critical,
            Subsystem::SceneTree,
            "panic",
            "lifecycle_test",
        ));
        q.add(
            CrashReport::new(
                "render glitch",
                Severity::P2Medium,
                Subsystem::Render,
                "mismatch",
                "golden_test",
            )
            .classify(CrashClassification::Regression),
        );

        let report = q.render_report();
        assert!(report.contains("Crash Triage Report"));
        assert!(report.contains("P0-Critical: 1"));
        assert!(report.contains("CI BLOCKERS"));
        assert!(report.contains("REGRESSIONS"));
        assert!(report.contains("CRITICAL"));
    }

    #[test]
    fn render_report_green_when_empty() {
        let q = TriageQueue::new();
        let report = q.render_report();
        assert!(report.contains("GREEN"));
    }

    // -- StackFrame ---------------------------------------------------------

    #[test]
    fn stack_frame_basic() {
        let f = StackFrame::new(0, "gdcore::crash_triage::foo");
        assert_eq!(f.index, 0);
        assert_eq!(f.symbol, "gdcore::crash_triage::foo");
        assert!(f.file.is_none());
    }

    #[test]
    fn stack_frame_with_location() {
        let f = StackFrame::new(1, "gdscene::node::add_child")
            .with_location("crates/gdscene/src/node.rs", 42);
        assert_eq!(f.file.as_deref(), Some("crates/gdscene/src/node.rs"));
        assert_eq!(f.line, Some(42));
    }

    #[test]
    fn stack_frame_crate_name() {
        let f = StackFrame::new(0, "gdscene::node::add_child");
        assert_eq!(f.crate_name(), Some("gdscene"));
    }

    #[test]
    fn stack_frame_is_engine_frame() {
        assert!(StackFrame::new(0, "gdscene::node::foo").is_engine_frame());
        assert!(StackFrame::new(0, "gdcore::math::bar").is_engine_frame());
        assert!(StackFrame::new(0, "patina_runner::main").is_engine_frame());
        assert!(!StackFrame::new(0, "std::thread::spawn").is_engine_frame());
        assert!(!StackFrame::new(0, "core::panicking::panic").is_engine_frame());
    }

    #[test]
    fn stack_frame_display() {
        let f = StackFrame::new(3, "gdcore::math::add").with_location("src/math.rs", 10);
        let s = format!("{}", f);
        assert!(s.contains("3: gdcore::math::add"));
        assert!(s.contains("at src/math.rs:10"));
    }

    // -- CrashTrace ---------------------------------------------------------

    #[test]
    fn crash_trace_basic() {
        let frames = vec![
            StackFrame::new(0, "core::panicking::panic"),
            StackFrame::new(1, "gdscene::node::add_child"),
            StackFrame::new(2, "gdscene::scene_tree::process"),
        ];
        let trace = CrashTrace::new("index out of bounds", frames);
        assert_eq!(trace.message, "index out of bounds");
        assert_eq!(trace.frames.len(), 3);
    }

    #[test]
    fn crash_trace_engine_frames() {
        let frames = vec![
            StackFrame::new(0, "core::panicking::panic"),
            StackFrame::new(1, "std::vec::Vec::index"),
            StackFrame::new(2, "gdscene::node::add_child"),
            StackFrame::new(3, "gdscene::scene_tree::process"),
        ];
        let trace = CrashTrace::new("panic", frames);
        let engine = trace.engine_frames();
        assert_eq!(engine.len(), 2);
        assert_eq!(engine[0].symbol, "gdscene::node::add_child");
    }

    #[test]
    fn crash_trace_top_engine_frame() {
        let frames = vec![
            StackFrame::new(0, "core::panicking::panic"),
            StackFrame::new(1, "gdscene::node::add_child"),
        ];
        let trace = CrashTrace::new("panic", frames);
        let top = trace.top_engine_frame().unwrap();
        assert_eq!(top.symbol, "gdscene::node::add_child");
    }

    #[test]
    fn crash_trace_detect_subsystem() {
        let frames = vec![
            StackFrame::new(0, "core::panicking::panic"),
            StackFrame::new(1, "gdphysics2d::world::step"),
        ];
        let trace = CrashTrace::new("panic", frames);
        assert_eq!(trace.detect_subsystem(), Subsystem::Physics);
    }

    #[test]
    fn crash_trace_detect_subsystem_render() {
        let frames = vec![StackFrame::new(0, "gdrender2d::draw::rect")];
        let trace = CrashTrace::new("err", frames);
        assert_eq!(trace.detect_subsystem(), Subsystem::Render);
    }

    #[test]
    fn crash_trace_detect_subsystem_unknown() {
        let frames = vec![StackFrame::new(0, "std::thread::spawn")];
        let trace = CrashTrace::new("err", frames);
        assert_eq!(trace.detect_subsystem(), Subsystem::Other);
    }

    #[test]
    fn crash_trace_signature_stable() {
        let frames = vec![
            StackFrame::new(0, "gdscene::node::add_child"),
            StackFrame::new(1, "gdscene::scene_tree::process"),
        ];
        let t1 = CrashTrace::new("panic", frames.clone());
        let t2 = CrashTrace::new("panic", frames);
        assert_eq!(t1.signature(), t2.signature());
    }

    #[test]
    fn crash_trace_signature_differs_for_different_stacks() {
        let t1 = CrashTrace::new(
            "panic",
            vec![StackFrame::new(0, "gdscene::node::add_child")],
        );
        let t2 = CrashTrace::new(
            "panic",
            vec![StackFrame::new(0, "gdscene::node::remove_child")],
        );
        assert_ne!(t1.signature(), t2.signature());
    }

    #[test]
    fn crash_trace_render() {
        let frames =
            vec![StackFrame::new(0, "gdscene::node::add_child").with_location("src/node.rs", 42)];
        let trace = CrashTrace::new("index out of bounds", frames).with_thread("main");
        let rendered = trace.render();
        assert!(rendered.contains("thread 'main'"));
        assert!(rendered.contains("index out of bounds"));
        assert!(rendered.contains("gdscene::node::add_child"));
        assert!(rendered.contains("at src/node.rs:42"));
    }

    // -- parse_backtrace ----------------------------------------------------

    #[test]
    fn parse_backtrace_basic() {
        let bt = "\
   0: core::panicking::panic
             at /rustc/abc123/library/core/src/panicking.rs:100:5
   1: gdscene::node::add_child
             at crates/gdscene/src/node.rs:42:9
   2: gdscene::scene_tree::process
             at crates/gdscene/src/scene_tree.rs:200";
        let frames = parse_backtrace(bt);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].symbol, "core::panicking::panic");
        assert_eq!(frames[0].line, Some(100));
        assert_eq!(frames[1].symbol, "gdscene::node::add_child");
        assert_eq!(
            frames[1].file.as_deref(),
            Some("crates/gdscene/src/node.rs")
        );
        assert_eq!(frames[1].line, Some(42));
        assert_eq!(frames[1].column, Some(9));
        assert_eq!(frames[2].symbol, "gdscene::scene_tree::process");
    }

    #[test]
    fn parse_backtrace_no_locations() {
        let bt = "0: foo::bar\n1: baz::qux";
        let frames = parse_backtrace(bt);
        assert_eq!(frames.len(), 2);
        assert!(frames[0].file.is_none());
        assert!(frames[1].file.is_none());
    }

    #[test]
    fn parse_backtrace_empty() {
        let frames = parse_backtrace("");
        assert!(frames.is_empty());
    }

    // -- CrashReport::from_trace -------------------------------------------

    #[test]
    fn crash_report_from_trace_auto_subsystem() {
        let frames = vec![
            StackFrame::new(0, "core::panicking::panic"),
            StackFrame::new(1, "gdscene::node::add_child"),
        ];
        let trace = CrashTrace::new("index out of bounds", frames);
        let report = CrashReport::from_trace(&trace, &[], &[]);
        assert_eq!(report.subsystem, Subsystem::SceneTree);
        assert_eq!(report.severity, Severity::P0Critical); // panic in SceneTree = P0
        assert!(report.backtrace.is_some());
    }

    #[test]
    fn crash_report_from_trace_classification() {
        let frames = vec![StackFrame::new(0, "gdrender2d::draw::rect")];
        let trace = CrashTrace::new("pixel mismatch in draw::rect", frames);
        let report = CrashReport::from_trace(&trace, &["pixel mismatch"], &[]);
        assert_eq!(report.classification, CrashClassification::Regression);
    }

    #[test]
    fn crash_report_from_trace_signature_in_summary() {
        let frames = vec![StackFrame::new(0, "gdcore::math::add")];
        let trace = CrashTrace::new("overflow", frames);
        let report = CrashReport::from_trace(&trace, &[], &[]);
        // Summary should contain the hex signature
        assert!(report.summary.starts_with('['));
        assert!(report.summary.contains("overflow"));
    }

    // -- subsystem_from_crate -----------------------------------------------

    #[test]
    fn subsystem_mapping() {
        assert_eq!(subsystem_from_crate("gdscene"), Subsystem::SceneTree);
        assert_eq!(subsystem_from_crate("gdphysics2d"), Subsystem::Physics);
        assert_eq!(subsystem_from_crate("gdphysics3d"), Subsystem::Physics);
        assert_eq!(subsystem_from_crate("gdrender2d"), Subsystem::Render);
        assert_eq!(subsystem_from_crate("gdrender3d"), Subsystem::Render);
        assert_eq!(subsystem_from_crate("gdserver3d"), Subsystem::Render);
        assert_eq!(subsystem_from_crate("gdresource"), Subsystem::Resources);
        assert_eq!(subsystem_from_crate("gdobject"), Subsystem::ClassDB);
        assert_eq!(
            subsystem_from_crate("gdscript_interop"),
            Subsystem::Scripting
        );
        assert_eq!(subsystem_from_crate("gdplatform"), Subsystem::Platform);
        assert_eq!(subsystem_from_crate("gdaudio"), Subsystem::Audio);
        assert_eq!(subsystem_from_crate("gdeditor"), Subsystem::Editor);
        assert_eq!(subsystem_from_crate("unknown"), Subsystem::Other);
    }

    // -- simple_hash --------------------------------------------------------

    #[test]
    fn hash_deterministic() {
        let h1 = simple_hash("test input");
        let h2 = simple_hash("test input");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16); // 16 hex chars
    }

    #[test]
    fn hash_differs_for_different_inputs() {
        assert_ne!(simple_hash("a"), simple_hash("b"));
    }

    // -- Auto-issue creation ------------------------------------------------

    fn make_test_report(severity: Severity, subsystem: Subsystem, msg: &str) -> CrashReport {
        CrashReport::new(msg, severity, subsystem, msg, "test_trigger")
    }

    #[test]
    fn generate_issue_has_title_body_priority() {
        let report = make_test_report(Severity::P0Critical, Subsystem::SceneTree, "null deref");
        let issue = generate_issue(&report);
        assert!(issue.title.contains("P0-Critical"));
        assert!(issue.title.contains("SceneTree"));
        assert!(issue.body.contains("null deref"));
        assert_eq!(issue.priority, 0);
    }

    #[test]
    fn generate_issue_labels() {
        let report = make_test_report(Severity::P1High, Subsystem::Render, "gpu crash")
            .classify(CrashClassification::Regression);
        // blocks_ci is true for P1
        let issue = generate_issue(&report);
        assert!(issue.labels.contains(&"crash".to_string()));
        assert!(issue.labels.contains(&"subsystem:render".to_string()));
        assert!(issue.labels.contains(&"regression".to_string()));
        assert!(issue.labels.contains(&"ci-blocker".to_string()));
    }

    #[test]
    fn generate_issue_body_has_acceptance_criteria() {
        let report = make_test_report(Severity::P2Medium, Subsystem::Audio, "buffer underrun");
        let issue = generate_issue(&report);
        assert!(issue.body.contains("Acceptance Criteria"));
        assert!(issue.body.contains("Root cause identified"));
        assert!(issue.body.contains("regression test"));
    }

    #[test]
    fn generate_issue_ci_blocker_label_only_when_blocking() {
        let mut report = make_test_report(Severity::P3Low, Subsystem::Editor, "minor glitch");
        report.blocks_ci = false;
        let issue = generate_issue(&report);
        assert!(!issue.labels.contains(&"ci-blocker".to_string()));
    }

    #[test]
    fn generate_issue_includes_backtrace() {
        let report = make_test_report(Severity::P0Critical, Subsystem::SceneTree, "panic")
            .with_backtrace("frame 0: foo\nframe 1: bar");
        let issue = generate_issue(&report);
        assert!(issue.body.contains("Backtrace"));
        assert!(issue.body.contains("frame 0: foo"));
    }

    #[test]
    fn generate_issue_signature_deterministic() {
        let r1 = make_test_report(Severity::P0Critical, Subsystem::SceneTree, "same error");
        let r2 = make_test_report(Severity::P0Critical, Subsystem::SceneTree, "same error");
        assert_eq!(generate_issue(&r1).signature, generate_issue(&r2).signature);
    }

    #[test]
    fn generate_issue_signature_differs_for_different_errors() {
        let r1 = make_test_report(Severity::P0Critical, Subsystem::SceneTree, "error A");
        let r2 = make_test_report(Severity::P0Critical, Subsystem::SceneTree, "error B");
        assert_ne!(generate_issue(&r1).signature, generate_issue(&r2).signature);
    }

    #[test]
    fn issue_template_to_br_command() {
        let report = make_test_report(Severity::P1High, Subsystem::Physics, "collision bug");
        let issue = generate_issue(&report);
        let cmd = issue.to_br_command();
        assert!(cmd.starts_with("br create"));
        assert!(cmd.contains("--priority 1"));
        assert!(cmd.contains("--labels"));
    }

    // -- AutoIssueTracker ---------------------------------------------------

    #[test]
    fn tracker_files_new_issue() {
        let mut tracker = AutoIssueTracker::new();
        let report = make_test_report(Severity::P1High, Subsystem::Render, "shader compile fail");
        let result = tracker.file_from_report(&report);
        assert!(result.is_some());
        assert_eq!(tracker.filed_count(), 1);
    }

    #[test]
    fn tracker_deduplicates_same_crash() {
        let mut tracker = AutoIssueTracker::new();
        let report = make_test_report(Severity::P1High, Subsystem::Render, "shader compile fail");
        assert!(tracker.file_from_report(&report).is_some());
        assert!(tracker.file_from_report(&report).is_none()); // duplicate
        assert_eq!(tracker.filed_count(), 1);
    }

    #[test]
    fn tracker_files_different_crashes() {
        let mut tracker = AutoIssueTracker::new();
        let r1 = make_test_report(Severity::P0Critical, Subsystem::SceneTree, "null deref");
        let r2 = make_test_report(Severity::P2Medium, Subsystem::Audio, "buffer underrun");
        assert!(tracker.file_from_report(&r1).is_some());
        assert!(tracker.file_from_report(&r2).is_some());
        assert_eq!(tracker.filed_count(), 2);
    }

    #[test]
    fn tracker_process_queue() {
        let mut queue = TriageQueue::new();
        queue.add(make_test_report(
            Severity::P0Critical,
            Subsystem::SceneTree,
            "panic A",
        ));
        queue.add(make_test_report(
            Severity::P1High,
            Subsystem::Render,
            "crash B",
        ));
        queue.add(make_test_report(
            Severity::P0Critical,
            Subsystem::SceneTree,
            "panic A",
        )); // dup

        let mut tracker = AutoIssueTracker::new();
        let filed = tracker.process_queue(&queue);
        assert_eq!(filed, 2); // only 2 unique crashes
        assert_eq!(tracker.filed_count(), 2);
    }

    #[test]
    fn tracker_is_duplicate() {
        let mut tracker = AutoIssueTracker::new();
        let report = make_test_report(Severity::P1High, Subsystem::Physics, "collision");
        let template = generate_issue(&report);
        assert!(!tracker.is_duplicate(&template.signature));
        tracker.file_from_report(&report);
        assert!(tracker.is_duplicate(&template.signature));
    }

    #[test]
    fn tracker_mark_filed() {
        let mut tracker = AutoIssueTracker::new();
        tracker.mark_filed("abc123");
        assert!(tracker.is_duplicate("abc123"));
    }

    #[test]
    fn tracker_issues_by_priority() {
        let mut tracker = AutoIssueTracker::new();
        tracker.file_from_report(&make_test_report(
            Severity::P0Critical,
            Subsystem::SceneTree,
            "crash A",
        ));
        tracker.file_from_report(&make_test_report(
            Severity::P2Medium,
            Subsystem::Audio,
            "crash B",
        ));
        tracker.file_from_report(&make_test_report(
            Severity::P0Critical,
            Subsystem::Resources,
            "crash C",
        ));

        let p0 = tracker.issues_by_priority(0);
        assert_eq!(p0.len(), 2);
        let p2 = tracker.issues_by_priority(2);
        assert_eq!(p2.len(), 1);
    }

    #[test]
    fn tracker_render_summary() {
        let mut tracker = AutoIssueTracker::new();
        tracker.file_from_report(&make_test_report(
            Severity::P1High,
            Subsystem::Render,
            "gpu hang",
        ));
        let summary = tracker.render_summary();
        assert!(summary.contains("Auto-filed crash issues: 1"));
        assert!(summary.contains("[P1]"));
    }

    #[test]
    fn tracker_render_summary_empty() {
        let tracker = AutoIssueTracker::new();
        assert_eq!(tracker.render_summary(), "No crash issues filed.\n");
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let result = truncate("this is a very long string that should be truncated", 20);
        assert!(result.len() <= 20);
        assert!(result.ends_with("..."));
    }

    // -- CrashCollector -----------------------------------------------------

    fn make_trace(symbol: &str, message: &str) -> CrashTrace {
        CrashTrace::new(
            message,
            vec![
                StackFrame::new(0, "core::panicking::panic"),
                StackFrame::new(1, symbol),
            ],
        )
    }

    #[test]
    fn collector_empty() {
        let c = CrashCollector::new();
        assert_eq!(c.total_records(), 0);
        assert_eq!(c.unique_signatures(), 0);
    }

    #[test]
    fn collector_collect_basic() {
        let mut c = CrashCollector::new();
        let trace = make_trace("gdscene::node::add_child", "panic");
        let seq = c.collect(trace, 1.0);
        assert_eq!(seq, 0);
        assert_eq!(c.total_records(), 1);
        assert_eq!(c.unique_signatures(), 1);
    }

    #[test]
    fn collector_sequence_numbers_increment() {
        let mut c = CrashCollector::new();
        let s0 = c.collect(make_trace("gdscene::a", "p"), 1.0);
        let s1 = c.collect(make_trace("gdscene::b", "p"), 2.0);
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
    }

    #[test]
    fn collector_dedup_signatures() {
        let mut c = CrashCollector::new();
        // Same trace twice → same signature
        c.collect(make_trace("gdscene::node::add_child", "panic"), 1.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 2.0);
        assert_eq!(c.total_records(), 2);
        assert_eq!(c.unique_signatures(), 1);
    }

    #[test]
    fn collector_different_signatures() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::node::add_child", "panic"), 1.0);
        c.collect(make_trace("gdphysics2d::world::step", "overflow"), 2.0);
        assert_eq!(c.unique_signatures(), 2);
    }

    #[test]
    fn collector_max_records_eviction() {
        let mut c = CrashCollector::new().with_max_records(3);
        for i in 0..5 {
            c.collect(make_trace(&format!("gd{}::fn", i), "p"), i as f64);
        }
        assert_eq!(c.total_records(), 3);
        // Oldest records evicted
        assert_eq!(c.records()[0].seq, 2);
    }

    #[test]
    fn collector_collect_from_backtrace() {
        let mut c = CrashCollector::new();
        let bt = "0: core::panicking::panic\n1: gdscene::node::add_child";
        let seq = c.collect_from_backtrace("index out of bounds", bt, 1.0);
        assert_eq!(seq, 0);
        assert_eq!(c.total_records(), 1);
        assert_eq!(c.records()[0].trace.frames.len(), 2);
    }

    #[test]
    fn collector_records_by_signature() {
        let mut c = CrashCollector::new();
        let t1 = make_trace("gdscene::node::add_child", "panic");
        let sig = t1.signature();
        c.collect(t1, 1.0);
        c.collect(make_trace("gdphysics2d::world::step", "overflow"), 2.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 3.0);

        let matches = c.records_by_signature(&sig);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn collector_records_in_window() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::a", "p"), 1.0);
        c.collect(make_trace("gdscene::b", "p"), 5.0);
        c.collect(make_trace("gdscene::c", "p"), 10.0);

        let window = c.records_in_window(3.0, 8.0);
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].seq, 1);
    }

    #[test]
    fn collector_frequencies() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::node::add_child", "panic"), 1.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 2.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 3.0);
        c.collect(make_trace("gdphysics2d::world::step", "overflow"), 4.0);

        let freqs = c.frequencies();
        assert_eq!(freqs.len(), 2);
        // First should be the most frequent
        assert_eq!(freqs[0].count, 3);
        assert_eq!(freqs[1].count, 1);
    }

    #[test]
    fn collector_frequency_of() {
        let mut c = CrashCollector::new();
        let t = make_trace("gdscene::node::add_child", "panic");
        let sig = t.signature();
        c.collect(t, 1.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 5.0);

        let freq = c.frequency_of(&sig).unwrap();
        assert_eq!(freq.count, 2);
        assert!((freq.first_seen - 1.0).abs() < f64::EPSILON);
        assert!((freq.last_seen - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn crash_frequency_avg_interval() {
        let freq = CrashFrequency {
            signature: "abc".to_string(),
            count: 4,
            first_seen: 0.0,
            last_seen: 9.0,
            message: "p".to_string(),
            subsystem: Subsystem::Other,
        };
        let avg = freq.avg_interval_secs().unwrap();
        assert!((avg - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn crash_frequency_avg_interval_single() {
        let freq = CrashFrequency {
            signature: "abc".to_string(),
            count: 1,
            first_seen: 5.0,
            last_seen: 5.0,
            message: "p".to_string(),
            subsystem: Subsystem::Other,
        };
        assert!(freq.avg_interval_secs().is_none());
    }

    #[test]
    fn crash_frequency_is_accelerating() {
        let freq = CrashFrequency {
            signature: "abc".to_string(),
            count: 4,
            first_seen: 0.0,
            last_seen: 7.0,
            message: "p".to_string(),
            subsystem: Subsystem::Other,
        };
        // Timestamps: 0, 3, 6, 7 — avg interval ~2.33, last interval 1 (< 2.33*0.8)
        let ts = vec![0.0, 3.0, 6.0, 7.0];
        assert!(freq.is_accelerating(&ts));
    }

    #[test]
    fn crash_frequency_not_accelerating() {
        let freq = CrashFrequency {
            signature: "abc".to_string(),
            count: 3,
            first_seen: 0.0,
            last_seen: 10.0,
            message: "p".to_string(),
            subsystem: Subsystem::Other,
        };
        // Even spacing: 0, 5, 10
        let ts = vec![0.0, 5.0, 10.0];
        assert!(!freq.is_accelerating(&ts));
    }

    // -- CrashTrend ---------------------------------------------------------

    #[test]
    fn trend_isolated_single_occurrence() {
        let mut c = CrashCollector::new();
        let t = make_trace("gdscene::node::add_child", "panic");
        let sig = t.signature();
        c.collect(t, 1.0);
        assert_eq!(c.trend(&sig), CrashTrend::Isolated);
    }

    #[test]
    fn trend_stable_even_spacing() {
        let mut c = CrashCollector::new();
        let t = make_trace("gdscene::node::add_child", "panic");
        let sig = t.signature();
        // Evenly spaced: 0, 5, 10, 15
        for i in 0..4 {
            c.collect(
                make_trace("gdscene::node::add_child", "panic"),
                i as f64 * 5.0,
            );
        }
        assert_eq!(c.trend(&sig), CrashTrend::Stable);
    }

    #[test]
    fn trend_accelerating() {
        let mut c = CrashCollector::new();
        let t = make_trace("gdscene::node::add_child", "panic");
        let sig = t.signature();
        // Accelerating: 0, 10, 18, 20 — avg 6.67, last interval 2
        for ts in &[0.0, 10.0, 18.0, 20.0] {
            c.collect(make_trace("gdscene::node::add_child", "panic"), *ts);
        }
        assert_eq!(c.trend(&sig), CrashTrend::Accelerating);
    }

    #[test]
    fn trend_decelerating() {
        let mut c = CrashCollector::new();
        let t = make_trace("gdscene::node::add_child", "panic");
        let sig = t.signature();
        // Decelerating: 0, 1, 2, 20 — avg 6.67, last interval 18
        for ts in &[0.0, 1.0, 2.0, 20.0] {
            c.collect(make_trace("gdscene::node::add_child", "panic"), *ts);
        }
        assert_eq!(c.trend(&sig), CrashTrend::Decelerating);
    }

    #[test]
    fn trend_unknown_signature() {
        let c = CrashCollector::new();
        assert_eq!(c.trend("nonexistent"), CrashTrend::Isolated);
    }

    // -- Build triage queue from collector ----------------------------------

    #[test]
    fn build_triage_queue_deduplicates() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::node::add_child", "panic"), 1.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 2.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 3.0);

        let queue = c.build_triage_queue();
        assert_eq!(queue.len(), 1); // deduplicated
        let reports = queue.by_severity();
        assert_eq!(reports[0].occurrence_count, 3);
    }

    #[test]
    fn build_triage_queue_multiple_signatures() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::node::add_child", "panic"), 1.0);
        c.collect(make_trace("gdphysics2d::world::step", "overflow"), 2.0);

        let queue = c.build_triage_queue();
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn build_triage_queue_with_known_patterns() {
        let mut c = CrashCollector::new();
        c.add_known_fixed("old_bug");
        c.collect(make_trace("gdscene::node::add_child", "old_bug resurfaced"), 1.0);

        let queue = c.build_triage_queue();
        let reports = queue.by_severity();
        assert_eq!(reports[0].classification, CrashClassification::Regression);
    }

    // -- Full pipeline ------------------------------------------------------

    #[test]
    fn run_pipeline_end_to_end() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::node::add_child", "panic"), 1.0);
        c.collect(make_trace("gdphysics2d::world::step", "overflow"), 2.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 3.0);

        let (queue, tracker) = c.run_pipeline();
        assert_eq!(queue.len(), 2);
        assert_eq!(tracker.filed_count(), 2);
    }

    // -- CollectorSummary ---------------------------------------------------

    #[test]
    fn collector_summary_basic() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::node::add_child", "panic"), 1.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 2.0);
        c.collect(make_trace("gdphysics2d::world::step", "overflow"), 3.0);

        let s = c.summary();
        assert_eq!(s.total_records, 3);
        assert_eq!(s.unique_signatures, 2);
        assert_eq!(s.top_crashers.len(), 2);
        assert_eq!(s.top_crashers[0].1, 2); // most frequent
    }

    #[test]
    fn collector_render_report_contains_sections() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::node::add_child", "panic"), 1.0);
        c.collect(make_trace("gdscene::node::add_child", "panic"), 2.0);

        let report = c.render_report();
        assert!(report.contains("Crash Collection Report"));
        assert!(report.contains("Total crashes collected: 2"));
        assert!(report.contains("Unique crash signatures: 1"));
        assert!(report.contains("TOP CRASHERS"));
    }

    #[test]
    fn collector_clear() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdscene::a", "p"), 1.0);
        c.collect(make_trace("gdscene::b", "q"), 2.0);
        c.clear();
        assert_eq!(c.total_records(), 0);
        assert_eq!(c.unique_signatures(), 0);
    }

    #[test]
    fn crash_trend_labels() {
        assert_eq!(CrashTrend::Isolated.label(), "Isolated");
        assert_eq!(CrashTrend::Stable.label(), "Stable");
        assert_eq!(CrashTrend::Accelerating.label(), "Accelerating");
        assert_eq!(CrashTrend::Decelerating.label(), "Decelerating");
    }

    #[test]
    fn crash_trend_display() {
        assert_eq!(format!("{}", CrashTrend::Accelerating), "Accelerating");
    }

    #[test]
    fn collector_subsystem_detected_in_record() {
        let mut c = CrashCollector::new();
        c.collect(make_trace("gdphysics2d::world::step", "overflow"), 1.0);
        assert_eq!(c.records()[0].subsystem, Subsystem::Physics);
    }
}
