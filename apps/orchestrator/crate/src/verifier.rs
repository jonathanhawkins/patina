use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use regex::Regex;
use std::sync::OnceLock;

use crate::error::{OrchestratorError, Result};

/// Known command prefixes (case-insensitive) that identify test/build commands.
const KNOWN_PREFIXES: &[&str] = &[
    "./scripts/rust_task.sh",
    "scripts/rust_task.sh",
    "bash ./scripts/rust_task.sh",
    "bash scripts/rust_task.sh",
    "cargo test",
    "cargo nextest",
    "cargo check",
    "cargo fmt",
    "cargo build",
    "pnpm test",
    "pnpm lint",
    "pnpm exec",
    "npm test",
    "npm run",
    "yarn test",
    "yarn ",
    "pytest",
    "python -m pytest",
    "python3 ",
    "uv run pytest",
    "go test",
    "bun test",
    "rg ",
    "rustfmt ",
];

/// Compiled regex patterns for each known prefix (case-insensitive).
/// We check the word-boundary guard manually since Rust regex doesn't support look-behind.
fn prefix_patterns() -> &'static Vec<Regex> {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        KNOWN_PREFIXES
            .iter()
            .map(|prefix| {
                let escaped = regex::escape(prefix);
                Regex::new(&format!("(?i){escaped}")).unwrap()
            })
            .collect()
    })
}

/// Check if the character before `pos` in `text` is a word character (alphanumeric or _).
fn has_word_char_before(text: &str, pos: usize) -> bool {
    if pos == 0 {
        return false;
    }
    text[..pos]
        .chars()
        .next_back()
        .map(|c| c.is_ascii_alphanumeric() || c == '_')
        .unwrap_or(false)
}

/// Find all matches of prefix patterns in text, respecting word boundary.
fn find_prefix_matches(text: &str) -> Vec<(usize, usize)> {
    let patterns = prefix_patterns();
    let mut matches = Vec::new();
    for (idx, pat) in patterns.iter().enumerate() {
        for m in pat.find_iter(text) {
            if !has_word_char_before(text, m.start()) {
                matches.push((m.start(), idx));
            }
        }
    }
    matches.sort_by_key(|&(start, _)| start);
    matches
}

/// Strip markdown artifacts and trailing result annotations from a command string.
pub fn clean_command(cmd: &str) -> String {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        vec![
            // Leading bullets and numbering
            Regex::new(r"^[*\-]\s*").unwrap(),
            Regex::new(r"^\d+\.\s*").unwrap(),
            // Bold markers
            Regex::new(r"^\*\*|\*\*$").unwrap(),
            // Trailing annotations: "— **...anything**"
            Regex::new(r"\s+[—\-]\s+\*\*.*$").unwrap(),
            // "— all passed..."
            Regex::new(r"(?i)\s+[—\-]\s+all passed.*$").unwrap(),
            // "— N/M passed..."
            Regex::new(r"(?i)\s+[—\-]\s+\d+/\d+\s+passed.*$").unwrap(),
            // "=> ..."
            Regex::new(r"\s*=>\s*.*$").unwrap(),
            // "test result: ..."
            Regex::new(r"(?i)\s*test result:\s*.*$").unwrap(),
            // "— N passed..."
            Regex::new(r"(?i)\s+[—\-]\s+\d+\s+passed.*$").unwrap(),
            // "(N passed...)" with optional trailing dot
            Regex::new(r"(?i)\s+\(\d+\s+passed[^)]*\)\.?.*$").unwrap(),
            // "N passed..."
            Regex::new(r"(?i)\s+\d+\s+passed.*$").unwrap(),
            // "N failed..."
            Regex::new(r"(?i)\s+\d+\s+failed.*$").unwrap(),
            // "Coverage..."
            Regex::new(r"(?i)\s*Coverage.*$").unwrap(),
        ]
    });

    let mut s = cmd.trim().replace('`', "");
    for pat in patterns {
        s = pat.replace_all(&s, "").trim().to_string();
    }
    // Strip trailing punctuation
    s = s.trim_matches(|c: char| " ,;:-".contains(c)).to_string();
    // Collapse whitespace
    static WS: OnceLock<Regex> = OnceLock::new();
    let ws = WS.get_or_init(|| Regex::new(r"\s+").unwrap());
    ws.replace_all(&s, " ").to_string()
}

/// Split a candidate string when it contains multiple known-prefix commands concatenated.
fn split_embedded_commands(candidate: &str) -> Vec<String> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return vec![];
    }

    let matches = find_prefix_matches(candidate);

    if matches.is_empty() {
        return vec![candidate.to_string()];
    }

    let mut parts = Vec::new();
    for (i, &(start, _)) in matches.iter().enumerate() {
        let end = if i + 1 < matches.len() {
            matches[i + 1].0
        } else {
            candidate.len()
        };
        let piece = candidate[start..end].trim();
        if !piece.is_empty() {
            parts.push(piece.to_string());
        }
    }

    if parts.is_empty() {
        vec![candidate.to_string()]
    } else {
        parts
    }
}

/// Returns true if the cleaned command starts with one of the known prefixes.
fn starts_with_known_prefix(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    KNOWN_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn is_replayable_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    let trimmed = cmd.trim();
    if trimmed.starts_with("...")
        || trimmed.starts_with("... ")
        || trimmed.ends_with(" ...")
        || cmd.contains(" ... ")
        || cmd.contains("...\n")
    {
        return false;
    }
    if lower.contains("direct validation script")
        || lower.contains("assertion script")
        || lower.contains("doc-validation check")
    {
        return false;
    }
    if lower.starts_with("python3 ") {
        return lower.starts_with("python3 -c ")
            || lower.starts_with("python3 -m ")
            || lower.starts_with("python3 ./")
            || lower.starts_with("python3 /")
            || lower.contains(".py");
    }
    true
}

/// Try to add a candidate command (after cleaning and deduplication).
fn maybe_add(found: &mut Vec<String>, candidate: &str) {
    for piece in split_embedded_commands(candidate) {
        let cleaned = clean_command(&piece);
        if cleaned.is_empty() {
            continue;
        }
        if !starts_with_known_prefix(&cleaned) {
            continue;
        }
        if !is_replayable_command(&cleaned) {
            continue;
        }
        // Skip if this is a longer version of an already-found command
        // (e.g. raw line "cargo test --test foo to verify" when backtick already found "cargo test --test foo")
        let dominated = found.iter().any(|existing| {
            cleaned.starts_with(existing.as_str()) && cleaned.len() > existing.len()
        });
        if dominated {
            continue;
        }
        if !found.contains(&cleaned) {
            found.push(cleaned);
        }
    }
}

/// Extract test commands from prose/markdown text.
///
/// Searches fenced code blocks, inline backticks, and raw lines for commands
/// that start with known test/build prefixes.
pub fn extract_test_commands(text: &str) -> Vec<String> {
    let text = text.trim();
    let mut found: Vec<String> = Vec::new();

    // 1. Fenced code blocks
    static FENCED: OnceLock<Regex> = OnceLock::new();
    let fenced = FENCED.get_or_init(|| Regex::new(r"(?s)```.*?```").unwrap());
    for block in fenced.find_iter(text) {
        let inner = block.as_str();
        // Strip opening ``` (with optional language tag) and closing ```
        let inner = inner.strip_prefix("```").unwrap_or(inner);
        let inner = inner.strip_suffix("```").unwrap_or(inner);
        // Skip the language tag line if present
        let inner = if let Some(pos) = inner.find('\n') {
            &inner[pos + 1..]
        } else {
            inner
        };
        for line in inner.lines() {
            maybe_add(&mut found, line);
        }
    }

    // 2. Inline backticks
    static INLINE: OnceLock<Regex> = OnceLock::new();
    let inline = INLINE.get_or_init(|| Regex::new(r"`([^`]+)`").unwrap());
    for cap in inline.captures_iter(text) {
        maybe_add(&mut found, &cap[1]);
    }

    // 3. Raw lines — search for known prefixes
    // Remove fenced and inline code blocks to avoid re-extracting already-handled commands
    static STRIP_FENCED: OnceLock<Regex> = OnceLock::new();
    let strip_fenced = STRIP_FENCED.get_or_init(|| Regex::new(r"(?s)```.*?```").unwrap());
    static STRIP_INLINE: OnceLock<Regex> = OnceLock::new();
    let strip_inline = STRIP_INLINE.get_or_init(|| Regex::new(r"`[^`]+`").unwrap());
    let normalized = text.replace('\r', "");
    let normalized = strip_fenced.replace_all(&normalized, "");
    let normalized = strip_inline.replace_all(&normalized, "");
    for segment in normalized.split(|c| c == '\n' || c == ';') {
        let line = segment.trim();
        if line.is_empty() {
            continue;
        }
        let matches = find_prefix_matches(line);
        if let Some(&(start, _)) = matches.first() {
            maybe_add(&mut found, &line[start..]);
        }
    }

    found
}

/// Extract acceptance test commands from a bead description.
///
/// Lines starting with "Acceptance:" contain the command to run.
/// For example:
/// ```text
/// Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_name
/// ```
pub fn extract_acceptance_commands(description: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in description.lines() {
        let trimmed = line.trim();
        let Some(stripped) = trimmed.strip_prefix("Acceptance:") else {
            continue;
        };
        maybe_add(&mut found, stripped.trim());
    }
    found
}

/// A bead's acceptance criteria, reduced to what a completion must actually run.
///
/// Beads state their acceptance two ways. Older ones put a whole command on an
/// `Acceptance:` line; port-audit ones end their `acceptance_criteria` field
/// with the test's *name* — `(test: \`scene_tree_script_bridge\`)` — and leave
/// the command to the worker. Both are demands on the verify set, so both are
/// captured here (pat-mpqun).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcceptanceDemand {
    /// Runnable commands quoted in the criteria or description.
    pub commands: Vec<String>,
    /// Test names called out without a command, from the `(test: \`name\`)` form.
    pub test_names: Vec<String>,
}

impl AcceptanceDemand {
    /// True when the bead states no acceptance command or test name, so the
    /// completion's own reported commands are all there is to go on.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.test_names.is_empty()
    }
}

/// Parse a bead's `acceptance_criteria` (and/or description) into the commands
/// and test names its completion must be verified against.
pub fn parse_acceptance(text: &str) -> AcceptanceDemand {
    // ONLY `Acceptance:` lines. This deliberately does **not** sweep the whole
    // description with `extract_test_commands`: a bead body is prose written by
    // an audit tool, and the commands quoted in it are documentation, not a
    // script. Harvesting them put a malformed `cargo nextest --test <name>`
    // (no `run` subcommand) at the head of a verify set on 2026-07-25, where it
    // exited 2 and failed the bead before the worker's own correct command ever
    // ran. What executes must be what somebody meant to execute.
    let commands = extract_acceptance_commands(text)
        .into_iter()
        .filter(|cmd| is_runnable_command(cmd))
        .collect();

    static TEST_NAME: OnceLock<Regex> = OnceLock::new();
    let test_name = TEST_NAME.get_or_init(|| {
        Regex::new(r"(?i)\(tests?:\s*`?([A-Za-z0-9_][A-Za-z0-9_:]*)`?\s*\)").unwrap()
    });
    let mut test_names = Vec::new();
    for cap in test_name.captures_iter(text) {
        let name = cap[1].to_string();
        if !test_names.contains(&name) {
            test_names.push(name);
        }
    }

    AcceptanceDemand {
        commands,
        test_names,
    }
}

/// Whether a command quoted in a bead body is actually invocable.
///
/// A bead's `Acceptance:` line is written by hand and is not validated anywhere,
/// so it can carry a typo that cargo rejects at argument-parse time (exit 2)
/// before any test runs. Since these commands are *prepended* to the verify set,
/// one typo fails a bead whose code is fine — so an obviously-broken one is
/// dropped rather than executed.
///
/// Deliberately narrow: this catches the shapes cargo cannot parse at all, not
/// commands that merely might fail.
fn is_runnable_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    // `cargo nextest` needs its subcommand — `cargo nextest --test x` is exit 2.
    if lower.contains("nextest") && !lower.contains("nextest run") {
        return false;
    }
    true
}

/// Whether `cmd` runs the test called `test_name`.
///
/// Matched as a whole token so `--test scene_tree` does not satisfy a demand
/// for `scene_tree_script_bridge` (or the reverse).
fn command_mentions_test(cmd: &str, test_name: &str) -> bool {
    if test_name.is_empty() {
        return false;
    }
    let haystack = cmd.to_lowercase();
    let needle = test_name.to_lowercase();
    let bytes = haystack.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    haystack.match_indices(&needle).any(|(start, m)| {
        let end = start + m.len();
        let before_ok = start == 0 || !ident(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !ident(bytes[end]);
        before_ok && after_ok
    })
}

/// Choose the commands a completion is verified against, given what the worker
/// reported and what the bead's acceptance criteria demands (pat-mpqun).
///
/// Two rules, both aimed at the same failure: a bead closing on a green
/// `cargo check` while its actual scope was never exercised.
///
/// 1. **A named acceptance test demands an actual test run.** If the criteria
///    names a test and the completion reports only compile checks, it is
///    rejected instead of being verified against a `cargo check`.
/// 2. **A real test beats a compile check.** Once an acceptance test is in the
///    set, generic `cargo check` / `cargo build` commands are dropped: they are
///    not evidence of the bead's scope, and they cost verify-lane time.
///
/// A bead with no stated acceptance keeps the current behavior — the worker's
/// reported commands, unchanged.
///
/// Rule 1 deliberately checks *that a test runs*, not *that the named one does*.
/// The first cut demanded the exact name and produced immediate false rejections
/// on correct completions, because a bead names a test **function**
/// (`cross_object_script_method_call`) while the command selects the **binary**
/// that contains it (`--test cross_object_script_call_test`), or a module filter
/// that covers it (`--lib animation_tree`). Neither string contains the other,
/// and the coordinator cannot know the mapping without reading the test sources.
/// A green run of the wrong test is still caught downstream — `count_tests_run`
/// rejects a vacuous pass, and the test itself must go green.
pub fn select_verify_commands(
    reported: &[String],
    demand: &AcceptanceDemand,
) -> std::result::Result<Vec<String>, String> {
    if demand.is_empty() {
        return Ok(reported.to_vec());
    }

    let runs_a_test = demand
        .commands
        .iter()
        .chain(reported.iter())
        .any(|cmd| is_test_run_command(cmd.as_str()));
    if !demand.test_names.is_empty() && !runs_a_test {
        return Err(format!(
            "acceptance test not run: the bead's acceptance_criteria names `{}`, \
             but the completion reports no test command at all ({}). A compile \
             check is not evidence the scope was delivered — report the \
             acceptance test, e.g. `cargo nextest run -p <package> --test {}`.",
            demand.test_names.join("`, `"),
            if reported.is_empty() {
                "no commands reported".to_string()
            } else {
                reported.join("; ")
            },
            demand.test_names[0],
        ));
    }

    // Not a rejection, but worth saying out loud: nothing in the verify set
    // mentions the demanded name, so it is only *probably* covered by whatever
    // binary or module filter was reported.
    for name in &demand.test_names {
        let mentioned = demand
            .commands
            .iter()
            .chain(reported.iter())
            .any(|cmd| command_mentions_test(cmd.as_str(), name));
        if !mentioned {
            tracing::info!(
                acceptance_test = %name,
                commands = %reported.join("; "),
                "[verify] no reported command names the acceptance test — \
                 trusting the reported test run to cover it"
            );
        }
    }

    let mut selected: Vec<String> = Vec::new();
    for cmd in demand.commands.iter().chain(reported.iter()) {
        if !selected.contains(cmd) {
            selected.push(cmd.clone());
        }
    }

    // Prefer the real tests. Only drop the compile checks when something
    // test-shaped survives — verifying nothing is worse than verifying a check.
    let tests_only: Vec<String> = selected
        .iter()
        .filter(|cmd| is_test_run_command(cmd.as_str()))
        .cloned()
        .collect();
    if tests_only.is_empty() {
        Ok(selected)
    } else {
        Ok(tests_only)
    }
}

/// Determine the working directory for a command based on its prefix.
pub fn command_workdir(project_root: &Path, command: &str) -> PathBuf {
    let lower = command.trim().to_lowercase();
    if lower.starts_with("cargo ") {
        let engine_root = project_root.join("engine-rs");
        if engine_root.join("Cargo.toml").exists() {
            return engine_root;
        }
    }
    if lower.starts_with("pnpm ")
        || lower.starts_with("npm ")
        || lower.starts_with("yarn ")
        || lower.starts_with("bun ")
    {
        let web_root = project_root.join("apps").join("web");
        if web_root.join("package.json").exists() {
            return web_root;
        }
    }
    project_root.to_path_buf()
}

/// Rewrite broad workspace commands to focused ones.
///
/// Workers sometimes report `--workspace`, `-E` filter, or `--no-run` commands
/// that enumerate all 300+ test binaries. Replace with a compile-only check —
/// scoped to the named package when the command carries `-p`/`--package`
/// (a full `cargo check --workspace` compiles every crate, wgpu/metal
/// included), falling back to the workspace check only for unscoped commands.
fn rewrite_broad_command(cmd: &str) -> String {
    let lower = cmd.to_lowercase();
    let fallback_check = || match crate_breaker::extract_crate_from_command(cmd) {
        Some(pkg) => format!("cargo check -p {pkg}"),
        None => "cargo check --workspace".to_string(),
    };

    // `cargo test --workspace --no-run` → just check it compiles
    if lower.contains("--workspace") && lower.contains("--no-run") {
        return fallback_check();
    }

    // `cargo nextest run --workspace -E <expr>` → extract --test from the -E if possible,
    // otherwise fall back to check
    if lower.contains("--workspace") && lower.contains("-e ") {
        return fallback_check();
    }

    // `nextest run -E <expr>` without --test → fall back to check
    // These enumerate all binaries to find matches
    if (lower.contains("nextest run") || lower.contains("nextest  run"))
        && lower.contains("-e ")
        && !lower.contains("--test ")
    {
        return fallback_check();
    }

    // `cargo test --workspace` without --test → check only
    if lower.contains("--workspace")
        && (lower.contains("cargo test") || lower.contains("cargo nextest"))
        && !lower.contains("--test ")
    {
        return fallback_check();
    }

    cmd.to_string()
}

/// One shared warm-up build covering every scoped cargo test target across a
/// drained verify batch, so N beads amortize a single dep-graph build instead
/// of each paying compile+link of the engine chain separately.
///
/// Collects `-p <pkg>` / `--test <target>` pairs from cargo-ish commands
/// (unscoped `--test` targets belong to `default_package`, the workspace-root
/// package). Commands with no `--test` target contribute nothing: including a
/// bare `-p X` would build ALL of X's test binaries — the exact full-build
/// trap this pre-build exists to avoid. Returns `None` unless the union spans
/// at least two distinct targets (a single-target batch gains nothing).
///
/// The result is best-effort by design: one bogus target name fails the whole
/// cargo invocation, so callers must treat a failure as "skip the warm-up and
/// let per-bead verifies build individually", never as a bead failure.
pub fn batch_prebuild_command<'a>(
    commands: impl IntoIterator<Item = &'a str>,
    default_package: &str,
) -> Option<String> {
    static TEST_TARGET: OnceLock<Regex> = OnceLock::new();
    let test_re =
        TEST_TARGET.get_or_init(|| Regex::new(r"(?:^|\s)--test[=\s]+([A-Za-z0-9_-]+)").unwrap());

    let mut packages: Vec<String> = Vec::new();
    let mut targets: Vec<String> = Vec::new();
    for cmd in commands {
        let lower = cmd.to_lowercase();
        // Only cargo-ish commands participate. Broad --workspace commands get
        // rewritten to plain checks at verify time; pre-building them would
        // reintroduce the full-workspace build.
        if !(lower.contains("cargo") || lower.contains("rust_task"))
            || lower.contains("--workspace")
        {
            continue;
        }
        // Ignore everything after ` -- ` — those args go to the test binary,
        // and a `--test` there (e.g. libtest's own flag) is not a cargo target.
        let head = cmd.split(" -- ").next().unwrap_or(cmd);
        let cmd_targets: Vec<String> = test_re
            .captures_iter(head)
            .map(|c| c[1].to_string())
            .collect();
        if cmd_targets.is_empty() {
            continue;
        }
        let pkg = crate_breaker::extract_crate_from_command(head)
            .unwrap_or_else(|| default_package.to_string());
        if !packages.contains(&pkg) {
            packages.push(pkg);
        }
        for t in cmd_targets {
            if !targets.contains(&t) {
                targets.push(t);
            }
        }
    }

    if targets.len() < 2 {
        return None;
    }
    let mut out = String::from("cargo build");
    for p in &packages {
        out.push_str(" -p ");
        out.push_str(p);
    }
    for t in &targets {
        out.push_str(" --test ");
        out.push_str(t);
    }
    Some(out)
}

/// Return the last `max_chars` characters of `s`, prefixed with an ellipsis
/// marker when truncated. Used to attach an actionable output tail to a
/// verification failure so the bounce message tells the worker *why* it failed.
pub fn tail_chars(s: &str, max_chars: usize) -> String {
    let trimmed = s.trim_end();
    let char_count = trimmed.chars().count();
    if char_count <= max_chars {
        return trimmed.to_string();
    }
    let start = char_count - max_chars;
    let tail: String = trimmed.chars().skip(start).collect();
    format!("…{tail}")
}

/// Best-effort one-word classification of a command failure from its output.
/// Gives the worker a hint about the failure category instead of a bare code.
pub fn classify_failure(output: &str) -> &'static str {
    let lower = output.to_lowercase();
    // Order matters: check the most specific signals first.
    if lower.contains("error[e") || lower.contains("could not compile") {
        "compile-error"
    } else if lower.contains("cannot find") && lower.contains("in this scope") {
        "compile-error"
    } else if lower.contains("panicked at") {
        "panic"
    } else if lower.contains("assertion") && lower.contains("failed") {
        "assertion-failure"
    } else if lower.contains("address already in use")
        || lower.contains("connection refused")
        || lower.contains("resource temporarily unavailable")
        || lower.contains("too many open files")
    {
        // Likely environmental/flaky — surfaced so the worker investigates the
        // root cause rather than silently retrying (we never auto-skip flakes).
        "possibly-environmental"
    } else if lower.contains("test result: failed") || lower.contains("tests failed") {
        "test-failure"
    } else {
        "failure"
    }
}

/// Count how many tests a successful test-runner invocation actually executed.
///
/// Returns `Some(0)` when the output proves zero tests ran (a *vacuous pass* —
/// a phantom or empty test that a worker could otherwise claim as proof of a
/// fix), `Some(n>0)` when tests genuinely ran, and `None` when the format is
/// unrecognized (we then make no claim and let the pass stand).
///
/// Recognizes libtest (`test result: ok. N passed`), nextest
/// (`Summary [..] N tests run`), and pytest (`collected N items` /
/// `N passed` / `no tests ran`).
pub fn count_tests_run(output: &str) -> Option<u64> {
    let lower = output.to_lowercase();

    // pytest: explicit "no tests ran" or "collected 0 items"
    if lower.contains("no tests ran") || lower.contains("collected 0 items") {
        return Some(0);
    }

    // nextest: "Summary [   0.123s] 42 tests run, 42 passed, 0 skipped"
    static NEXTEST: OnceLock<Regex> = OnceLock::new();
    let nextest = NEXTEST.get_or_init(|| Regex::new(r"(?i)(\d+)\s+tests?\s+run").unwrap());
    if let Some(cap) = nextest.captures(output) {
        if let Ok(n) = cap[1].parse::<u64>() {
            return Some(n);
        }
    }

    // libtest: one "test result: ok. N passed; M failed" line per binary.
    // Sum the "passed" counts across all binaries; if a binary errored it
    // would have produced a non-zero exit (handled before we get here).
    static LIBTEST: OnceLock<Regex> = OnceLock::new();
    let libtest =
        LIBTEST.get_or_init(|| Regex::new(r"(?i)test result:\s*\w+\.\s*(\d+)\s+passed").unwrap());
    let mut saw_result = false;
    let mut total: u64 = 0;
    for cap in libtest.captures_iter(output) {
        saw_result = true;
        total += cap[1].parse::<u64>().unwrap_or(0);
    }
    if saw_result {
        return Some(total);
    }

    None
}

/// Whether a command is a test-runner (so a zero-tests-run result is suspicious).
/// `cargo check` / `cargo build` / `cargo fmt` / `rg` legitimately run no tests
/// and must NOT be flagged as vacuous.
fn is_test_run_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    if lower.starts_with("cargo check")
        || lower.starts_with("cargo build")
        || lower.starts_with("cargo fmt")
        || lower.starts_with("rg ")
        || lower.starts_with("rustfmt")
    {
        return false;
    }
    // rust_task.sh wrappers: "scripts/rust_task.sh check ..." → not a test run
    if lower.contains("rust_task.sh") {
        return lower.contains("test") || lower.contains("nextest");
    }
    lower.contains("test") || lower.contains("pytest")
}

/// Outcome of running a single command. `None` means the command timed out.
type CommandOutcome = Option<(std::process::ExitStatus, String)>;

/// Spawn a command under bash, draining stdout+stderr on reader threads so a
/// verbose build can never deadlock by filling the OS pipe buffer, and return
/// the exit status plus combined output (or `None` on timeout).
fn run_command_capture(
    cwd: &Path,
    cmd: &str,
    timeout: Duration,
) -> std::io::Result<CommandOutcome> {
    use std::io::Read;
    // Lead a fresh process group so a timeout can SIGKILL the whole tree, not
    // just bash. Without this the orphaned cargo-nextest survives, holds the
    // build-dir lock, and wedges every later verify (pat-regax).
    use std::os::unix::process::CommandExt;

    // Snapshot compiles already referencing this workspace BEFORE we start.
    // Anything matching later that is absent here began inside our window, so a
    // timeout may reap it; anything already running is somebody else's and must
    // survive (pat-r46zo).
    let preexisting_compilers = compiler_pids_for_workspace(cwd);

    let mut child = Command::new("bash")
        .args(["-lc", cmd])
        .current_dir(cwd)
        .env("AGENT_NAME", "verifier")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // pgid == this child's pid; descendants inherit it (pat-regax).
        .process_group(0)
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut s) = stdout {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut s) = stderr {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });

    let status = match child.wait_timeout(timeout)? {
        Some(s) => s,
        None => {
            // Timed out. bash leads its own process group (set via
            // `process_group(0)`), so its pid is also the pgid. SIGKILL the
            // WHOLE group — not just bash — so orphaned descendants
            // (cargo-nextest and its spawned test binaries) can't survive and
            // wedge the lane by holding the build-dir lock (pat-regax). Then
            // reap bash so it doesn't linger as a zombie. Don't join the readers
            // (a lingering grandchild could hold the pipe open); let them detach.
            let pgid = child.id() as i32;
            // SAFETY: kill(2) with a negative pid targets the process group
            // `pgid`. `pgid` is this child's pid, which we made a group leader
            // via `process_group(0)`, so the signal only reaches this command's
            // own tree — no unrelated processes share the group.
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
            let _ = child.wait();
            // The group kill cannot reach compiles that sccache's server spawned
            // for us: they are pid-1-parented and never joined our group. Observed
            // live 2026-07-14 — after a platform_suite timeout, its gdrender3d
            // rustc survived 6+ min at 0 CPU holding an sccache slot and starving
            // the lane. Reap them by cmdline instead (pat-r46zo).
            let escaped = reap_escaped_compiler_orphans(cwd, &preexisting_compilers);
            if !escaped.is_empty() {
                tracing::warn!(
                    "[verify] timeout kill reaped {} compiler orphan(s) outside group {pgid}: {escaped:?}",
                    escaped.len()
                );
            }
            // Post-kill assertion: nothing in the group should still be alive.
            // If something is (e.g. a descendant that escaped into its own
            // group), surface it loudly so a wedge is visible, not silent.
            if let Some(survivors) = surviving_group_members(pgid) {
                tracing::error!(
                    "[verify] timeout kill left process(es) alive in group {pgid}: {survivors}"
                );
            }
            return Ok(None);
        }
    };

    let out = out_handle.join().unwrap_or_default();
    let err = err_handle.join().unwrap_or_default();
    Ok(Some((status, format!("{out}{err}"))))
}

/// PIDs of compiler processes whose command line references `workspace`,
/// regardless of who their parent is.
///
/// The process-group kill (pat-regax) only reaps descendants that inherited the
/// verify's pgid. Compiles routed through `RUSTC_WRAPPER=sccache` do NOT: the
/// sccache client hands the job to the long-lived sccache SERVER, which spawns
/// `rustc` as its own (pid-1-parented) child, outside our group entirely. A
/// cmdline match is the only way to find those. Best-effort — an empty set if
/// `ps` is unavailable (pat-r46zo).
fn compiler_pids_for_workspace(workspace: &Path) -> std::collections::HashSet<i32> {
    let workspace = workspace.to_string_lossy();
    let output = match Command::new("ps").args(["-axo", "pid=,command="]).output() {
        Ok(o) => o,
        Err(_) => return std::collections::HashSet::new(),
    };
    let self_pid = std::process::id() as i32;

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim_start().split_once(' ')?;
            let pid: i32 = pid.parse().ok()?;
            // Never signal ourselves, whatever our cmdline happens to contain.
            if pid == self_pid {
                return None;
            }
            if command.contains("rustc") && command.contains(&*workspace) {
                Some(pid)
            } else {
                None
            }
        })
        .collect()
}

/// SIGKILL compiler orphans that escaped the timeout's process-group kill,
/// returning the pids actually signalled (sorted).
///
/// `preexisting` is the same match taken BEFORE the verify started: anything in
/// it belongs to somebody else and is left alone. Everything else appeared
/// inside this verify's window, so it is ours to reap — this is the bead's
/// "start time falls within the verify window" test, done as a pid-set diff
/// rather than by parsing `ps -o lstart` (pat-r46zo).
fn reap_escaped_compiler_orphans(
    workspace: &Path,
    preexisting: &std::collections::HashSet<i32>,
) -> Vec<i32> {
    let mut reaped: Vec<i32> = compiler_pids_for_workspace(workspace)
        .into_iter()
        .filter(|pid| !preexisting.contains(pid))
        .collect();
    reaped.sort_unstable();

    for pid in &reaped {
        // SAFETY: kill(2) with a positive pid signals exactly that process. Each
        // pid matched `rustc` + this verify's workspace and was absent before the
        // verify began, so it is a compile this verify started.
        unsafe {
            libc::kill(*pid, libc::SIGKILL);
        }
    }

    reaped
}

/// PIDs still alive in process group `pgid` (space-separated), or `None` if the
/// group is empty. Used to assert that a timeout-kill actually reaped the whole
/// tree rather than leaving orphans behind (pat-regax). Best-effort: returns
/// `None` if `pgrep` is unavailable.
fn surviving_group_members(pgid: i32) -> Option<String> {
    let out = Command::new("pgrep")
        .args(["-g", &pgid.to_string()])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let pids: Vec<&str> = stdout.split_whitespace().collect();
    if pids.is_empty() {
        None
    } else {
        Some(pids.join(" "))
    }
}

/// Verify-lane circuit breaker for the host dyld-wedge (pat-8zkm3).
///
/// When the kernel AMFI/code-sign wedge sets in, every freshly launched test
/// binary hangs at `_dyld_start`. The verifier would otherwise keep launching a
/// new 900s verify every cycle, each timing out and spawning ~26 more wedged
/// processes — deepening the wedge. (Root cause per 2026-07-14 diagnosis: each
/// NEW binary's first launch pays a serialized syspolicyd/Gatekeeper
/// evaluation, ~5-25s for big engine binaries; concurrent launches of many
/// never-seen binaries saturate that queue so launches back up indefinitely.
/// It drains once the launch storm stops — no reboot needed.) This breaker
/// watches for that signature: after `N` consecutive wall-clock timeouts with
/// no compile error in the output it trips, the lane stops launching new
/// verifies, an operator alert is emitted, and the lane auto-resumes once a
/// cheap probe launch succeeds (proof the host can run binaries again).
///
/// The state machine is pure and synchronous so it is unit-testable by
/// injecting outcomes and probe results — no real 900s waits required.
///
/// WIRING: live as of pat-f6ndj — `spawn_verifier_worker` (coordinator.rs) holds
/// a `VerifyCircuitBreaker`, gates each launch on `should_launch()`, feeds every
/// result through `classify_verify_outcome()` + `record()`, emits the
/// [`WedgeAlert`] (HOST-ISSUE log line + operator agent-mail) on trip, and
/// auto-resumes via [`probe_recent_test_binary`]. A couple of read-only getters
/// (`is_tripped`/`consecutive_timeouts`) remain test-only, so the module keeps
/// `allow(dead_code)`.
#[allow(dead_code)]
pub mod circuit_breaker {
    use super::{classify_failure, run_command_capture};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    /// Outcome of a single verify run, as seen by [`VerifyCircuitBreaker`].
    ///
    /// Only a bare wall-clock timeout with no compile error counts toward the
    /// dyld-wedge breaker; every other terminal result resets the streak.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VerifyOutcome {
        /// The verify finished with a terminal status (pass / test failure /
        /// compile error). Resets the consecutive-timeout streak.
        Finished,
        /// The verify hit the wall-clock timeout with no compile error in its
        /// output — the dyld-wedge signature (binaries hung at `_dyld_start`).
        TimedOut,
    }

    /// Classify a completed verify attempt for the circuit breaker.
    ///
    /// `timed_out` is true when [`run_command_capture`](super::run_command_capture)
    /// returned `None`. A timeout whose captured output already shows a compile
    /// error is NOT treated as a wedge — a real build problem is the worker's to
    /// fix, not a host issue — so it maps to [`VerifyOutcome::Finished`].
    pub fn classify_verify_outcome(timed_out: bool, output: &str) -> VerifyOutcome {
        if timed_out && classify_failure(output) != "compile-error" {
            VerifyOutcome::TimedOut
        } else {
            VerifyOutcome::Finished
        }
    }

    /// Operator alert emitted exactly once when the breaker trips. Carries
    /// everything needed to write the HOST-ISSUE log line and an agent-mail
    /// message to the coordinator/operator.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct WedgeAlert {
        /// How many consecutive timeouts tripped the breaker.
        pub consecutive_timeouts: u32,
    }

    impl WedgeAlert {
        /// One-line, grep-friendly banner for verifier.log.
        pub fn host_issue_log_line(&self) -> String {
            format!(
                "HOST-ISSUE: dyld-wedge suspected after {} consecutive verify timeouts \
                 (no compile error) — freshly launched test binaries are hanging at \
                 _dyld_start (saturated syspolicyd first-launch evaluation queue). \
                 Verify lane HALTED so the queue can drain; probing periodically to \
                 auto-resume. Reboot is a last resort, not the fix.",
                self.consecutive_timeouts
            )
        }

        /// Subject line for the operator agent-mail.
        pub fn mail_subject(&self) -> String {
            "[verifier] HOST-ISSUE: dyld-wedge — verify lane halted".to_string()
        }

        /// Body for the operator agent-mail: the signature, how to confirm with
        /// `sample <pid> 1`, and the auto-resume behavior.
        pub fn mail_body(&self) -> String {
            format!(
                "The verifier lane halted after {n} consecutive verify timeouts with no \
                 compile error in the output.\n\n\
                 Signature: every freshly launched test binary hangs at `_dyld_start`. \
                 Root cause (2026-07-14 diagnosis): ad-hoc-signed (never-notarized) \
                 binaries pay a serialized syspolicyd/Gatekeeper evaluation per LAUNCH at \
                 limited sustained throughput. A high process-spawn rate (nextest \
                 process-per-test fan-out, verify retry storms) saturates that queue, so \
                 every launch on the host backs up indefinitely. Each new verify deepens \
                 the backlog, so the lane has stopped launching to let it drain.\n\n\
                 Confirm: pick a hung child PID and run `sample <pid> 1`; if the stack \
                 bottoms out at `_dyld_start` the diagnosis is confirmed.\n\n\
                 Recovery WITHOUT reboot (see the /dyld-unwedge skill): kill the hung \
                 launches, wait ~5-10 quiet minutes, then resume at a sustainable spawn \
                 rate — NEXTEST_TEST_THREADS=1, or in-process `cargo test` for bulk runs. \
                 Only if a freshly compiled hello-world still hangs after ~10 quiet \
                 minutes does a reboot make sense.\n\n\
                 Auto-resume: the lane re-probes by launching a tiny prebuilt test binary \
                 with `--list` under a 10s timeout; when that returns promptly the queue \
                 has drained and verification resumes automatically.",
                n = self.consecutive_timeouts
            )
        }
    }

    /// Circuit breaker that halts the verify lane on a host dyld-wedge.
    #[derive(Debug)]
    pub struct VerifyCircuitBreaker {
        threshold: u32,
        consecutive_timeouts: u32,
        tripped: bool,
    }

    impl VerifyCircuitBreaker {
        /// New breaker that trips after `threshold` consecutive timeouts. The
        /// pat-8zkm3 default is `N = 2`; `threshold` is clamped to at least 1.
        pub fn new(threshold: u32) -> Self {
            Self {
                threshold: threshold.max(1),
                consecutive_timeouts: 0,
                tripped: false,
            }
        }

        /// Whether the lane may launch the next verify. False once tripped,
        /// until a successful probe resumes it.
        pub fn should_launch(&self) -> bool {
            !self.tripped
        }

        /// Whether the breaker is currently tripped (lane halted).
        pub fn is_tripped(&self) -> bool {
            self.tripped
        }

        /// Current consecutive-timeout streak (test/diagnostic visibility).
        pub fn consecutive_timeouts(&self) -> u32 {
            self.consecutive_timeouts
        }

        /// Record one verify outcome. Returns `Some(alert)` exactly once — on
        /// the transition that trips the breaker — so the caller emits the
        /// HOST-ISSUE log line and operator mail a single time. A non-timeout
        /// outcome resets the streak (but does not by itself un-trip the
        /// breaker — only a probe does, since while tripped no verifies run to
        /// produce a `Finished` result).
        pub fn record(&mut self, outcome: VerifyOutcome) -> Option<WedgeAlert> {
            match outcome {
                VerifyOutcome::TimedOut => {
                    self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
                    if !self.tripped && self.consecutive_timeouts >= self.threshold {
                        self.tripped = true;
                        return Some(WedgeAlert {
                            consecutive_timeouts: self.consecutive_timeouts,
                        });
                    }
                    None
                }
                VerifyOutcome::Finished => {
                    self.consecutive_timeouts = 0;
                    None
                }
            }
        }

        /// While tripped, run `probe` (a cheap launch — e.g. a prebuilt test
        /// binary with `--list` under a short timeout). On success, reset and
        /// resume the lane; returns whether the lane is now running. A no-op
        /// returning `true` when not tripped. The probe is injected so the
        /// state machine stays unit-testable without a real launch.
        pub fn probe_and_maybe_resume(&mut self, probe: impl FnOnce() -> bool) -> bool {
            if !self.tripped {
                return true;
            }
            if probe() {
                self.tripped = false;
                self.consecutive_timeouts = 0;
                true
            } else {
                false
            }
        }
    }

    /// Default probe for [`VerifyCircuitBreaker::probe_and_maybe_resume`]:
    /// launch a prebuilt test binary with `--list` under a short timeout.
    /// Returns true when it completes promptly with success — proof the host can
    /// launch binaries again (the dyld-wedge has cleared). Never triggers a
    /// build, so it is safe to call while the lane is halted.
    pub fn probe_prebuilt_binary(test_binary: &Path, timeout: Duration) -> bool {
        let cwd = test_binary.parent().unwrap_or_else(|| Path::new("."));
        let cmd = format!("'{}' --list", test_binary.display());
        matches!(run_command_capture(cwd, &cmd, timeout), Ok(Some((status, _))) if status.success())
    }

    /// The most-recently-modified executable regular file across `dirs`, if any.
    /// Used to find a freshly-built local test binary for the resume probe.
    fn newest_executable(dirs: &[PathBuf]) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                // Regular files with any execute bit set — skips .rlib/.d sidecars
                // and .dSYM directories that share the deps/ dir.
                if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
                    continue;
                }
                let Ok(mtime) = meta.modified() else {
                    continue;
                };
                if best.as_ref().map_or(true, |(t, _)| mtime > *t) {
                    best = Some((mtime, entry.path()));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// Auto-resume probe (pat-f6ndj, reworked 2026-07-14): compile a 1-line C
    /// program to a FRESH path and launch it under a 10s timeout. A fresh
    /// ad-hoc-signed binary is exactly what the launch-queue saturation blocks
    /// (it must pass a serialized syspolicyd/Gatekeeper evaluation), yet on a
    /// healthy host it evaluates in well under a second — so this cleanly
    /// separates "queue drained, resume" from "still saturated, hold".
    ///
    /// The previous probe launched the most-recently-built *test* binary with
    /// `--list`, but big engine binaries legitimately take 20-120s on their
    /// first launch even on a healthy host — the 10s probe could never pass,
    /// so a tripped breaker stayed open forever (observed live 2026-07-14:
    /// lane halted 2h+ after the queue had drained). Deliberately does NOT
    /// probe a system/cached binary: those launch from the dyld shared cache
    /// even mid-saturation and would falsely resume the lane.
    pub fn probe_recent_test_binary(project_root: &Path) -> bool {
        let dir = std::env::temp_dir().join("patina-verifier-probe");
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }
        // Unique source/binary per probe so every attempt is a brand-new
        // inode + ad-hoc signature (a reused binary would be pre-evaluated).
        let tag = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let src = dir.join(format!("probe-{tag}.c"));
        let bin = dir.join(format!("probe-{tag}"));
        if std::fs::write(&src, "int main(void){return 0;}\n").is_err() {
            return false;
        }
        // Compile with the system clang (an old, cached binary — compiling is
        // unaffected by the launch-queue saturation). ~0.3s on a healthy host.
        let compiled = Command::new("/usr/bin/clang")
            .arg(&src)
            .arg("-o")
            .arg(&bin)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        let result = if compiled {
            let cmd = format!("'{}'", bin.display());
            matches!(
                run_command_capture(project_root, &cmd, Duration::from_secs(10)),
                Ok(Some((status, _))) if status.success()
            )
        } else {
            // No compiler → fall back to the old newest-binary probe rather
            // than resuming blind.
            let candidates = [
                project_root.join("engine-rs/target/debug/deps"),
                project_root.join("engine-rs/target/release/deps"),
            ];
            newest_executable(&candidates)
                .map(|b| probe_prebuilt_binary(&b, Duration::from_secs(10)))
                .unwrap_or(false)
        };
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&bin);
        result
    }
}

/// ─── Crate-breakage circuit breaker ──────────────────────────────────────
///
/// A worker's uncommitted WIP that breaks a shared crate makes every verify
/// whose build graph includes that crate fail with the same `could not
/// compile` error. Without a breaker the single verify lane re-runs the same
/// broken build for each queued bead — burning the build slot for zero new
/// information (observed 2026-07-14: broken gdscene → four consecutive
/// identical failures). This breaker counts consecutive compile failures per
/// *broken* crate (the crate rustc names, not the crate the verify targets —
/// a gdeditor test failing on its gdscene dep attributes to gdscene). At the
/// threshold it alerts the operator once (rustc excerpt + dirty files) and
/// halts the lane behind a throttled `cargo check -p <crate>` pre-flight
/// until the crate builds again. Skipped verifies stay pending (the B3
/// reaper is the backstop) — workers are never bounced for someone else's
/// breakage.
pub mod crate_breaker {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use regex::Regex;

    /// Crate a single command targets (`-p X` / `--package X`), if any.
    pub fn extract_crate_from_command(command: &str) -> Option<String> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE
            .get_or_init(|| Regex::new(r"(?:^|\s)(?:-p|--package)[=\s]+([A-Za-z0-9_-]+)").unwrap());
        re.captures(command).map(|cap| cap[1].to_string())
    }

    /// Crate a verify request targets, from its command list
    /// (`-p X` / `--package X`). None for non-cargo or unscoped commands.
    pub fn extract_request_crate(commands: &[String]) -> Option<String> {
        commands.iter().find_map(|c| extract_crate_from_command(c))
    }

    /// Crate named in a ``could not compile `X` `` failure output.
    pub fn parse_broken_crate(failure_output: &str) -> Option<String> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"could not compile `([A-Za-z0-9_-]+)`").unwrap());
        re.captures(failure_output).map(|cap| cap[1].to_string())
    }

    /// Operator alert payload, emitted exactly once per trip transition.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct BreakageAlert {
        pub crate_name: String,
        pub consecutive_failures: u32,
        pub error_excerpt: String,
    }

    impl BreakageAlert {
        pub fn log_line(&self) -> String {
            format!(
                "CRATE-BROKEN: `{}` failed to compile in {} consecutive verifies — halting \
                 verifies behind a `cargo check -p {}` pre-flight until it builds again",
                self.crate_name, self.consecutive_failures, self.crate_name
            )
        }
    }

    /// Consecutive same-crate compile-failure tracking with a throttled
    /// recovery pre-check. `threshold == 0` disables tripping entirely.
    pub struct CrateBreaker {
        threshold: u32,
        precheck_interval: Duration,
        streaks: HashMap<String, u32>,
        /// Tripped crates → last pre-check attempt (throttling).
        tripped: HashMap<String, Instant>,
    }

    impl CrateBreaker {
        pub fn new(threshold: u32, precheck_interval: Duration) -> Self {
            Self {
                threshold,
                precheck_interval,
                streaks: HashMap::new(),
                tripped: HashMap::new(),
            }
        }

        /// Record a completed verify. `broken_crate` is the crate rustc blamed
        /// (None for passes and non-compile failures); `request_crate` is the
        /// crate the verify targeted. Returns the alert exactly when a crate's
        /// streak crosses the threshold.
        pub fn record(
            &mut self,
            request_crate: Option<&str>,
            broken_crate: Option<&str>,
            error_excerpt: &str,
        ) -> Option<BreakageAlert> {
            if let Some(broken) = broken_crate {
                let n = self.streaks.entry(broken.to_string()).or_insert(0);
                *n += 1;
                if self.threshold > 0 && *n == self.threshold {
                    self.tripped.insert(broken.to_string(), Instant::now());
                    return Some(BreakageAlert {
                        crate_name: broken.to_string(),
                        consecutive_failures: *n,
                        error_excerpt: error_excerpt.to_string(),
                    });
                }
                return None;
            }
            // Completed without a compile error: the targeted crate built, so
            // its streak (if any) is stale.
            if let Some(rc) = request_crate {
                self.clear(rc);
            }
            None
        }

        /// A crate proved buildable (pre-check or clean verify) — forget it.
        pub fn clear(&mut self, crate_name: &str) {
            self.streaks.remove(crate_name);
            self.tripped.remove(crate_name);
        }

        pub fn any_tripped(&self) -> bool {
            !self.tripped.is_empty()
        }

        pub fn is_tripped(&self, crate_name: &str) -> bool {
            self.tripped.contains_key(crate_name)
        }

        pub fn tripped_crates(&self) -> Vec<String> {
            let mut v: Vec<String> = self.tripped.keys().cloned().collect();
            v.sort();
            v
        }

        /// Run the recovery pre-check for every tripped crate whose throttle
        /// window has elapsed; crates whose probe passes are cleared. Returns
        /// the crates that recovered.
        pub fn run_due_prechecks(&mut self, mut probe: impl FnMut(&str) -> bool) -> Vec<String> {
            let due: Vec<String> = self
                .tripped
                .iter_mut()
                .filter_map(|(krate, last)| {
                    if last.elapsed() >= self.precheck_interval {
                        *last = Instant::now();
                        Some(krate.clone())
                    } else {
                        None
                    }
                })
                .collect();
            let mut recovered = Vec::new();
            for krate in due {
                if probe(&krate) {
                    self.clear(&krate);
                    recovered.push(krate);
                }
            }
            recovered
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn extracts_request_crate_from_commands() {
            let cmds = vec![
                "cd somewhere".to_string(),
                "cargo nextest run -p gdscene --lib foo".to_string(),
            ];
            assert_eq!(extract_request_crate(&cmds).as_deref(), Some("gdscene"));
            let pkg = vec!["cargo test --package gd-render2d --test t".to_string()];
            assert_eq!(extract_request_crate(&pkg).as_deref(), Some("gd-render2d"));
            assert_eq!(extract_request_crate(&["make check".to_string()]), None);
        }

        #[test]
        fn parses_broken_crate_from_rustc_output() {
            let out = "…\nerror: could not compile `gdscene` (lib) due to 2 previous errors";
            assert_eq!(parse_broken_crate(out).as_deref(), Some("gdscene"));
            assert_eq!(parse_broken_crate("test result: FAILED"), None);
        }

        /// Trips exactly once at the threshold, attributing to the BROKEN
        /// crate even when the request targeted a dependent crate.
        #[test]
        fn trips_once_on_broken_crate_not_request_crate() {
            let mut b = CrateBreaker::new(3, Duration::from_secs(120));
            assert!(b.record(Some("gdeditor"), Some("gdscene"), "e1").is_none());
            assert!(b
                .record(Some("gdrender2d"), Some("gdscene"), "e2")
                .is_none());
            let alert = b.record(Some("gdeditor"), Some("gdscene"), "e3");
            assert_eq!(alert.unwrap().crate_name, "gdscene");
            assert!(b.any_tripped());
            // Trip is crate-scoped: only the broken crate's verifies are held.
            assert!(b.is_tripped("gdscene"));
            assert!(!b.is_tripped("gdaudio"));
            // Further failures don't re-alert.
            assert!(b.record(Some("gdeditor"), Some("gdscene"), "e4").is_none());
            assert_eq!(b.tripped_crates(), vec!["gdscene".to_string()]);
        }

        #[test]
        fn threshold_zero_never_trips() {
            let mut b = CrateBreaker::new(0, Duration::from_secs(120));
            for _ in 0..10 {
                assert!(b.record(None, Some("gdscene"), "e").is_none());
            }
            assert!(!b.any_tripped());
        }

        /// A clean completion for the targeted crate clears its streak.
        #[test]
        fn clean_verify_clears_streak() {
            let mut b = CrateBreaker::new(3, Duration::from_secs(120));
            b.record(Some("gdscene"), Some("gdscene"), "e1");
            b.record(Some("gdscene"), Some("gdscene"), "e2");
            b.record(Some("gdscene"), None, ""); // passed (or failed non-compile)
            let alert = b.record(Some("gdscene"), Some("gdscene"), "e3");
            assert!(alert.is_none(), "streak must restart after a clean verify");
        }

        /// Pre-checks are throttled: the trip stamps the clock, so a probe
        /// only runs after the interval — and a passing probe clears the trip.
        #[test]
        fn precheck_throttles_then_recovers() {
            let mut b = CrateBreaker::new(1, Duration::from_secs(3600));
            b.record(None, Some("gdscene"), "e");
            assert!(b.any_tripped());
            let mut probes = 0;
            let recovered = b.run_due_prechecks(|_| {
                probes += 1;
                true
            });
            assert_eq!(probes, 0, "within the throttle window no probe runs");
            assert!(recovered.is_empty());
            assert!(b.any_tripped());

            let mut b = CrateBreaker::new(1, Duration::from_secs(0));
            b.record(None, Some("gdscene"), "e");
            let recovered = b.run_due_prechecks(|krate| krate == "gdscene");
            assert_eq!(recovered, vec!["gdscene".to_string()]);
            assert!(!b.any_tripped());
        }
    }
}

/// Run each command via bash and return an error on failure or timeout.
///
/// On failure the error carries a one-word category (see [`classify_failure`])
/// and an output tail so the worker bounce is actionable. A *successful* test
/// command that ran zero tests is rejected as a vacuous pass (see
/// [`count_tests_run`]) to prevent phantom/empty tests being accepted as proof.
pub fn verify_commands(project_root: &Path, commands: &[String], timeout: Duration) -> Result<()> {
    if commands.is_empty() {
        return Err(OrchestratorError::Verification(
            "no test commands to verify".into(),
        ));
    }

    for cmd in commands {
        // Rewrite broad workspace commands to focused ones.
        // Workers sometimes report `--workspace` or `-E` filter commands that
        // enumerate all 300+ test binaries and take 10+ minutes. Strip these
        // and fall back to a focused check.
        let cmd = rewrite_broad_command(cmd);

        // Wrap cargo commands with rust_task.sh so verification serializes against
        // other swarm builds via the directory mutex and Agent Mail build slot.
        // rust_task.sh handles `cd engine-rs` internally, so cwd stays at project root.
        let (effective_cmd, effective_cwd) = if cmd.trim().to_lowercase().starts_with("cargo ") {
            let cargo_args = cmd.trim().strip_prefix("cargo ").unwrap();
            (
                format!("./scripts/rust_task.sh {cargo_args}"),
                project_root.to_path_buf(),
            )
        } else {
            (cmd.clone(), command_workdir(project_root, &cmd))
        };
        let cwd = effective_cwd;
        tracing::info!("[verify] running in {}: {}", cwd.display(), cmd);

        let outcome = run_command_capture(&cwd, &effective_cmd, timeout)
            .map_err(|e| OrchestratorError::Verification(format!("failed to spawn bash: {e}")))?;

        let (status, output) = match outcome {
            Some(pair) => pair,
            None => {
                return Err(OrchestratorError::Verification(format!(
                    "timed out after {}s: {cmd}",
                    timeout.as_secs()
                )));
            }
        };

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            let category = classify_failure(&output);
            let tail = tail_chars(&output, 800);
            return Err(OrchestratorError::Verification(format!(
                "failed ({code}, {category}): {cmd}\n--- output tail ---\n{tail}"
            )));
        }

        // A "passing" test command that executed zero tests is not proof of a
        // fix — reject it so phantom/empty tests can't be claimed as evidence.
        if is_test_run_command(&cmd) && count_tests_run(&output) == Some(0) {
            let tail = tail_chars(&output, 400);
            return Err(OrchestratorError::Verification(format!(
                "vacuous pass (0 tests ran — phantom or empty test, not proof of a fix): {cmd}\n--- output tail ---\n{tail}"
            )));
        }
    }

    tracing::info!("[verify] reported test commands passed");
    Ok(())
}

/// Extension trait for wait with timeout on std::process::Child.
trait WaitTimeout {
    fn wait_timeout(
        &mut self,
        timeout: Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(
        &mut self,
        timeout: Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        use std::thread;
        use std::time::Instant;

        let start = Instant::now();
        let poll_interval = Duration::from_millis(100);

        loop {
            match self.try_wait()? {
                Some(status) => return Ok(Some(status)),
                None => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    thread::sleep(poll_interval);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// pat-regax: a verify timeout must SIGKILL the entire process group, not
    /// just the bash parent. Spawns a fake long-running command tree
    /// (`sleep 1000 & sleep 1000 & wait`) through the verifier timeout path with
    /// a 2s timeout and asserts that every descendant the shell spawned is dead
    /// afterwards — i.e. no orphaned process survives to wedge the lane. With
    /// the old `child.kill()` (bash only) the backgrounded sleeps would survive
    /// and this test would fail.
    #[cfg(unix)]
    #[test]
    fn verifier_timeout_kills_process_group() {
        let dir = TempDir::new().unwrap();
        let pidfile = dir.path().join("pids.txt");
        let pf = pidfile.display();
        // Record bash's own pid (the group leader) plus the two backgrounded
        // sleep children, then block forever so the call must time out.
        let cmd = format!(
            "echo $$ > '{pf}'; sleep 1000 & echo $! >> '{pf}'; sleep 1000 & echo $! >> '{pf}'; wait"
        );

        let outcome =
            run_command_capture(dir.path(), &cmd, Duration::from_secs(2)).expect("spawn ok");
        assert!(outcome.is_none(), "command should have timed out");

        let recorded = std::fs::read_to_string(&pidfile).unwrap_or_default();
        let pids: Vec<String> = recorded.split_whitespace().map(|s| s.to_string()).collect();
        assert!(
            pids.len() >= 3,
            "expected bash + 2 sleep pids recorded, got: {recorded:?}"
        );

        // Every recorded pid must be dead after the timeout kill. Poll briefly
        // to absorb the small teardown/reap window.
        let pid_alive = |pid: &str| -> bool {
            Command::new("kill")
                .args(["-0", pid])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        for pid in &pids {
            let mut alive = true;
            for _ in 0..30 {
                if !pid_alive(pid) {
                    alive = false;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            assert!(
                !alive,
                "pid {pid} survived the timeout kill — process group was not fully reaped"
            );
        }
    }

    /// Poll `kill -0 <pid>` until the process is gone, up to ~3s. Returns true
    /// if it died. Absorbs the small teardown/reap window after a SIGKILL.
    #[cfg(unix)]
    fn died_within_grace(pid: &str) -> bool {
        for _ in 0..30 {
            let alive = Command::new("kill")
                .args(["-0", pid])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !alive {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// pat-r46zo: `kill(-pgid)` cannot reach a compile that sccache's long-lived
    /// server spawned on our behalf — it is pid-1-parented and never joined the
    /// verify's process group, so it survives the timeout and holds an sccache
    /// slot (observed live: a gdrender3d rustc lingering 6+ min at 0 CPU).
    ///
    /// Stands in a fake `rustc` for the real one and detaches it into its OWN
    /// process group via bash job control (`set -m`), which is precisely the
    /// escape the sccache server performs. With only the pat-regax group kill,
    /// the fake rustc survives and this test fails.
    #[cfg(unix)]
    #[test]
    fn verifier_timeout_reaps_wrapper_spawned_orphan_outside_group() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path();

        // A stand-in compiler: its path carries both "rustc" and the workspace,
        // which is exactly how the reaper identifies this verify's compiles.
        let fake_rustc = ws.join("rustc");
        std::fs::write(&fake_rustc, "#!/bin/sh\nsleep 20\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_rustc, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let pidfile = ws.join("orphan.pid");
        // `set -m` gives each background job its own process group — the same
        // escape sccache's server performs. Then block so the call times out.
        let cmd = format!(
            "set -m; '{rustc}' & echo $! > '{pf}'; sleep 1000",
            rustc = fake_rustc.display(),
            pf = pidfile.display()
        );

        let outcome = run_command_capture(ws, &cmd, Duration::from_secs(3)).expect("spawn ok");
        assert!(outcome.is_none(), "command should have timed out");

        let orphan = std::fs::read_to_string(&pidfile).unwrap_or_default();
        let orphan = orphan.trim();
        assert!(
            !orphan.is_empty(),
            "fake rustc pid should have been recorded"
        );

        assert!(
            died_within_grace(orphan),
            "pid {orphan} (wrapper-spawned rustc outside the process group) survived the \
             timeout — it would hold an sccache slot and starve the verify lane"
        );
    }

    /// The reaper must not shoot down a compile that was already running when the
    /// verify started: that one belongs to another lane (or the operator), and
    /// killing it would turn one lane's timeout into everyone else's failure.
    #[cfg(unix)]
    #[test]
    fn reaper_spares_compilers_that_predate_the_verify() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path();

        let fake_rustc = ws.join("rustc");
        std::fs::write(&fake_rustc, "#!/bin/sh\nsleep 20\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_rustc, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Start it BEFORE any snapshot, standing in for a foreign in-flight compile.
        let mut foreign = Command::new(&fake_rustc)
            .spawn()
            .expect("spawn foreign rustc");
        let foreign_pid = foreign.id() as i32;

        // Give ps a moment to see it, then snapshot as run_command_capture would.
        std::thread::sleep(Duration::from_millis(300));
        let preexisting = compiler_pids_for_workspace(ws);
        assert!(
            preexisting.contains(&foreign_pid),
            "snapshot must see the in-flight compile at {foreign_pid}"
        );

        let reaped = reap_escaped_compiler_orphans(ws, &preexisting);

        assert!(
            !reaped.contains(&foreign_pid),
            "reaper killed a compile that predates the verify window: {reaped:?}"
        );
        assert!(
            Command::new("kill")
                .args(["-0", &foreign_pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false),
            "pid {foreign_pid} must still be alive — it was never ours to kill"
        );

        let _ = foreign.kill();
        let _ = foreign.wait();
    }

    // ─── pat-8zkm3: dyld-wedge circuit breaker ───────────────────────────
    use super::circuit_breaker::{
        classify_verify_outcome, probe_prebuilt_binary, VerifyCircuitBreaker, VerifyOutcome,
    };

    /// pat-8zkm3: after N=2 consecutive verify timeouts the breaker must halt
    /// the lane (the third verify is NOT launched) and emit the operator alert
    /// exactly once; a successful probe must resume the lane. Outcomes are
    /// injected through the state machine — no real 900s waits.
    #[test]
    fn verifier_timeout_circuit_breaker() {
        let mut cb = VerifyCircuitBreaker::new(2);
        assert!(cb.should_launch(), "lane should launch before any timeout");

        // First timeout: streak=1, not tripped, no alert, lane still runs.
        assert_eq!(cb.record(VerifyOutcome::TimedOut), None);
        assert!(cb.should_launch(), "one timeout must not halt the lane");

        // Second consecutive timeout: trips and emits the alert exactly once.
        let alert = cb
            .record(VerifyOutcome::TimedOut)
            .expect("breaker must trip on the 2nd consecutive timeout");
        assert_eq!(alert.consecutive_timeouts, 2);
        assert!(alert.host_issue_log_line().contains("HOST-ISSUE"));
        assert!(alert.host_issue_log_line().to_lowercase().contains("dyld"));
        assert!(
            alert.mail_body().contains("_dyld_start") && alert.mail_body().contains("sample"),
            "operator mail must describe the _dyld_start signature and the `sample` check"
        );

        // Lane is halted: the THIRD verify must not be launched.
        assert!(cb.is_tripped());
        assert!(
            !cb.should_launch(),
            "tripped breaker must stop launching new verifies"
        );

        // Further timeouts while tripped do not re-emit the alert (fires once).
        assert_eq!(cb.record(VerifyOutcome::TimedOut), None);
        assert!(!cb.should_launch());

        // A failing probe does NOT resume the lane.
        assert!(!cb.probe_and_maybe_resume(|| false));
        assert!(
            !cb.should_launch(),
            "lane must stay halted while the probe keeps failing"
        );

        // A successful probe resumes the lane and clears the streak.
        assert!(cb.probe_and_maybe_resume(|| true));
        assert!(cb.should_launch(), "successful probe must resume the lane");
        assert!(!cb.is_tripped());
        assert_eq!(cb.consecutive_timeouts(), 0, "streak resets on resume");
    }

    /// A timeout whose output already shows a compile error is a real build
    /// problem, not a host wedge — it must NOT count toward the breaker.
    #[test]
    fn circuit_breaker_compile_error_timeout_is_not_a_wedge() {
        let mut cb = VerifyCircuitBreaker::new(2);
        let ce = classify_verify_outcome(true, "error[E0599]: no method named `foo`");
        assert_eq!(ce, VerifyOutcome::Finished);
        assert_eq!(cb.record(ce), None);

        // A bare timeout (no output) is the wedge candidate.
        let bare = classify_verify_outcome(true, "");
        assert_eq!(bare, VerifyOutcome::TimedOut);
        assert_eq!(cb.record(bare), None); // streak=1, not yet tripped
        assert!(cb.should_launch());
    }

    /// Any non-timeout terminal result resets the consecutive-timeout streak.
    #[test]
    fn circuit_breaker_finished_outcome_resets_streak() {
        let mut cb = VerifyCircuitBreaker::new(2);
        assert_eq!(cb.record(VerifyOutcome::TimedOut), None); // streak=1
        assert_eq!(cb.record(VerifyOutcome::Finished), None); // reset
        assert_eq!(cb.consecutive_timeouts(), 0);
        // It now takes two fresh consecutive timeouts again to trip.
        assert_eq!(cb.record(VerifyOutcome::TimedOut), None); // streak=1
        assert!(
            cb.record(VerifyOutcome::TimedOut).is_some(),
            "two fresh consecutive timeouts must trip the breaker"
        );
    }

    /// The auto-resume probe must compile-and-launch a FRESH tiny binary and
    /// return true on a healthy host. (On a host mid-launch-queue-saturation
    /// this test fails by design — that is the condition the probe exists to
    /// detect; wait for the drain and re-run.)
    #[cfg(unix)]
    #[test]
    fn circuit_breaker_probe_compiles_fresh_binary() {
        use super::circuit_breaker::probe_recent_test_binary;
        let tmp = TempDir::new().unwrap();
        assert!(
            probe_recent_test_binary(tmp.path()),
            "fresh-binary probe failed: either /usr/bin/clang is unavailable \
             or the host launch queue is saturated (see /dyld-unwedge)"
        );
    }

    /// The real probe helper returns true for a binary that launches and exits
    /// 0, false for one that exits non-zero — exercising the actual launch path
    /// (no 900s wait) that the lane uses to auto-resume.
    #[cfg(unix)]
    #[test]
    fn circuit_breaker_probe_helper_runs_prebuilt_binary() {
        assert!(
            probe_prebuilt_binary(Path::new("/bin/echo"), Duration::from_secs(10)),
            "a binary that launches and exits 0 must probe true"
        );
        assert!(
            !probe_prebuilt_binary(Path::new("/usr/bin/false"), Duration::from_secs(10)),
            "a binary that exits non-zero must probe false"
        );
    }

    #[test]
    fn test_extract_from_fenced_block() {
        let text = r#"```text
cargo test --test signal_trace_fixture_parity_test
test result: ok. 10 passed; 0 failed
```"#;
        let cmds = extract_test_commands(text);
        assert_eq!(
            cmds,
            vec!["cargo test --test signal_trace_fixture_parity_test"]
        );
    }

    #[test]
    fn test_extract_from_inline_backtick() {
        let text = "Run `cargo test --test foo_test` to verify.";
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["cargo test --test foo_test"]);
    }

    #[test]
    fn test_extract_rust_task_wrapper_command() {
        let text =
            "Tests run: `./scripts/rust_task.sh nextest run -p patina-engine comparison_tooling_3d_test`";
        let cmds = extract_test_commands(text);
        assert_eq!(
            cmds,
            vec!["scripts/rust_task.sh nextest run -p patina-engine comparison_tooling_3d_test"]
        );
    }

    #[test]
    fn test_extract_from_prose() {
        // In prose without backticks, raw extraction picks up from the prefix to end of segment.
        // The trailing "and it passed" isn't a known annotation pattern, so it remains.
        // This is acceptable — the command will still run (bash ignores "and" as a syntax error
        // or the test binary ignores unknown args).
        let text = "I ran cargo test --test foo_test and it passed";
        let cmds = extract_test_commands(text);
        assert!(!cmds.is_empty());
        assert!(cmds[0].starts_with("cargo test --test foo_test"));
    }

    #[test]
    fn test_extract_splits_embedded() {
        let text = "cargo test --test a cargo test --test b";
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["cargo test --test a", "cargo test --test b"]);
    }

    #[test]
    fn test_clean_command_strips_annotations() {
        assert_eq!(
            clean_command("cargo test --test foo — **all passed**"),
            "cargo test --test foo"
        );
        assert_eq!(
            clean_command("cargo test --test foo — 5/5 passed"),
            "cargo test --test foo"
        );
        assert_eq!(
            clean_command("cargo test --test foo (3 passed)"),
            "cargo test --test foo"
        );
        assert_eq!(
            clean_command("cargo test --test foo test result: ok. 10 passed"),
            "cargo test --test foo"
        );
        assert_eq!(
            clean_command("cargo test --test foo Coverage: 80%"),
            "cargo test --test foo"
        );
    }

    #[test]
    fn test_clean_command_strips_markdown() {
        assert_eq!(
            clean_command("* `cargo test --test foo`"),
            "cargo test --test foo"
        );
        assert_eq!(
            clean_command("- `cargo test --test foo`"),
            "cargo test --test foo"
        );
        assert_eq!(
            clean_command("1. cargo test --test foo"),
            "cargo test --test foo"
        );
    }

    #[test]
    fn test_command_workdir_cargo() {
        let tmp = TempDir::new().unwrap();
        let engine_dir = tmp.path().join("engine-rs");
        std::fs::create_dir_all(&engine_dir).unwrap();
        std::fs::write(engine_dir.join("Cargo.toml"), "").unwrap();

        assert_eq!(
            command_workdir(tmp.path(), "cargo test --test foo"),
            engine_dir
        );
    }

    #[test]
    fn test_command_workdir_pnpm() {
        let tmp = TempDir::new().unwrap();
        let web_dir = tmp.path().join("apps").join("web");
        std::fs::create_dir_all(&web_dir).unwrap();
        std::fs::write(web_dir.join("package.json"), "{}").unwrap();

        assert_eq!(command_workdir(tmp.path(), "pnpm test"), web_dir);
    }

    #[test]
    fn test_command_workdir_default() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            command_workdir(tmp.path(), "python -m pytest tests/"),
            tmp.path().to_path_buf()
        );
    }

    #[test]
    fn test_extract_empty_input() {
        assert!(extract_test_commands("").is_empty());
        assert!(extract_test_commands("   \n\n  ").is_empty());
    }

    #[test]
    fn test_extract_no_known_prefixes() {
        let text = "I checked the logs and everything looks fine. No tests needed.";
        assert!(extract_test_commands(text).is_empty());
    }

    // --- Ported from test_coordinator_poll_command_extraction.sh ---

    #[test]
    fn test_ported_fenced_block_plus_prose() {
        let text = r#"```text
cargo test --test signal_trace_fixture_parity_test
test result: ok. 10 passed; 0 failed
```

Tests cover:
1. Registration order trace matches fixture
"#;
        let cmds = extract_test_commands(text);
        assert_eq!(
            cmds,
            vec!["cargo test --test signal_trace_fixture_parity_test"]
        );
    }

    #[test]
    fn test_ported_markdown_bullets_with_backticks() {
        let text = r#"- `cargo test --test probe_output_schema_test` — 55 passed (was 45, +10 new)
- `cargo test --test api_extraction_automation_test` — 33 passed (all green)"#;
        let cmds = extract_test_commands(text);
        assert_eq!(
            cmds,
            vec![
                "cargo test --test probe_output_schema_test",
                "cargo test --test api_extraction_automation_test",
            ]
        );
    }

    #[test]
    fn test_ported_inline_prose_with_paren_counts() {
        let text = "cargo test --test collision_shape_registration_overlap_test (38 passed) ; cargo test --test collision_overlap_extended_parity_test (36 passed). Total 74 tests, all green.";
        let cmds = extract_test_commands(text);
        assert_eq!(
            cmds,
            vec![
                "cargo test --test collision_shape_registration_overlap_test",
                "cargo test --test collision_overlap_extended_parity_test",
            ]
        );
    }

    #[test]
    fn test_ported_markdown_bullets_with_emphasis() {
        let text = r#"- `cargo test --test node3d_transform_propagation_parity_test` — **30/30 passed**
- `cargo test --test transform3d_camera_light_contract_test` — **44/44 passed**
- `cargo test -p gdscene` — all passed

All 17 core classes recognized."#;
        let cmds = extract_test_commands(text);
        assert_eq!(
            cmds,
            vec![
                "cargo test --test node3d_transform_propagation_parity_test",
                "cargo test --test transform3d_camera_light_contract_test",
                "cargo test -p gdscene",
            ]
        );
    }

    #[test]
    fn test_clean_strips_arrow_annotation() {
        assert_eq!(
            clean_command("cargo test --test foo => ok"),
            "cargo test --test foo"
        );
    }

    #[test]
    fn test_clean_strips_failed_count() {
        assert_eq!(
            clean_command("cargo test --test foo 2 failed"),
            "cargo test --test foo"
        );
    }

    #[test]
    fn test_multiple_inline_backticks() {
        let text = "Ran `cargo test -p gdscene` and `pnpm lint` successfully.";
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["cargo test -p gdscene", "pnpm lint"]);
    }

    #[test]
    fn test_deduplication() {
        let text = r#"```
cargo test --test foo
```
Also ran `cargo test --test foo` again."#;
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["cargo test --test foo"]);
    }

    #[test]
    fn test_case_insensitive_prefix() {
        let text = "`Cargo Test --test foo`";
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["Cargo Test --test foo"]);
    }

    #[test]
    fn test_yarn_prefix() {
        let text = "Run `yarn test` to verify.";
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["yarn test"]);
    }

    #[test]
    fn test_pytest_prefix() {
        let text = "Verified with `pytest tests/unit -v`";
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["pytest tests/unit -v"]);
    }

    #[test]
    fn test_go_test_prefix() {
        let text = "`go test ./...`";
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["go test ./..."]);
    }

    #[test]
    fn test_extract_acceptance_commands_parses_lines() {
        let desc = "Fix the rendering bug.\n\
                     Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_render_fix\n\
                     Some other notes.\n\
                     Acceptance: cargo test --test smoke_test\n";
        let cmds = extract_acceptance_commands(desc);
        assert_eq!(
            cmds,
            vec![
                "cargo test --test v1_acceptance_gate_test -- --ignored test_render_fix",
                "cargo test --test smoke_test",
            ]
        );
    }

    // -- Acceptance-driven command selection (pat-mpqun) --------------------

    /// The port-audit shape: the criteria names a test and nothing else.
    #[test]
    fn test_parse_acceptance_picks_up_named_test() {
        let criteria = "A scene script can construct Transform2D and apply it \
                        via `*` end-to-end (test: `variant_affine_types_runtime`)";
        let demand = parse_acceptance(criteria);
        assert_eq!(demand.test_names, vec!["variant_affine_types_runtime"]);
        assert!(demand.commands.is_empty(), "no command was stated");
        assert!(!demand.is_empty(), "a named test is still a demand");
    }

    /// The older shape: a whole command on an `Acceptance:` line.
    #[test]
    fn test_parse_acceptance_picks_up_command() {
        let demand = parse_acceptance("Acceptance: cargo test --test smoke_test\n");
        assert_eq!(demand.commands, vec!["cargo test --test smoke_test"]);
        assert!(demand.test_names.is_empty());
    }

    /// Regression: a command merely *mentioned* in the description is
    /// documentation, not something to execute. Harvesting prose put a malformed
    /// `cargo nextest --test <name>` (no `run`) at the head of a verify set on
    /// 2026-07-25; cargo exited 2 at argument-parse time and failed a bead whose
    /// code was fine, before the worker's own correct command ever ran.
    #[test]
    fn test_parse_acceptance_ignores_commands_quoted_in_prose() {
        let body = "Patina's shape_from_node returns None, so the authored shape \
                    is lost. Verify with `cargo nextest --test collision_wiring` \
                    once fixed. (test: `collision_wiring`)";
        let demand = parse_acceptance(body);
        assert!(
            demand.commands.is_empty(),
            "a command in prose must not become an executed command: {:?}",
            demand.commands
        );
        // The named test is still picked up — the demand survives, only the
        // unvetted command is dropped.
        assert_eq!(demand.test_names, vec!["collision_wiring"]);
    }

    /// ...and even on a real `Acceptance:` line, a shape cargo cannot parse is
    /// dropped rather than run, since these commands lead the verify set.
    #[test]
    fn test_parse_acceptance_drops_an_uninvocable_command() {
        // `cargo nextest` without its `run` subcommand is exit 2, always.
        let demand = parse_acceptance("Acceptance: cargo nextest --test smoke_test\n");
        assert!(
            demand.commands.is_empty(),
            "an uninvocable command must not be executed: {:?}",
            demand.commands
        );
        let ok = parse_acceptance("Acceptance: cargo nextest run --test smoke_test\n");
        assert_eq!(ok.commands, vec!["cargo nextest run --test smoke_test"]);
    }

    /// A bead with no stated acceptance keeps today's behavior: the worker's
    /// reported commands, unchanged and unfiltered.
    #[test]
    fn test_select_verify_commands_without_acceptance_is_unchanged() {
        let reported = vec![
            "cargo check -p gdscene".to_string(),
            "cargo test --test foo".to_string(),
        ];
        let demand = parse_acceptance("Just a description with no acceptance.");
        assert!(demand.is_empty());
        assert_eq!(
            select_verify_commands(&reported, &demand).unwrap(),
            reported,
            "with nothing demanded, the reported set passes through verbatim"
        );
    }

    /// The bug this exists for: a bead names a test, the worker reports only a
    /// compile check, and the bead closes green with its scope undelivered.
    #[test]
    fn test_select_verify_commands_rejects_missing_acceptance_test() {
        let demand = parse_acceptance("Prove it: ... (test: `scene_tree_script_bridge`)");
        let reported = vec!["cargo check -p gdscene".to_string()];
        let err = select_verify_commands(&reported, &demand)
            .expect_err("a compile check is not proof of the named test");
        assert!(
            err.contains("scene_tree_script_bridge"),
            "the rejection must name the test still owed: {err}"
        );
    }

    /// A completion that DOES run the named test verifies via that command —
    /// and the generic compile check alongside it is dropped.
    #[test]
    fn test_select_verify_commands_prefers_the_acceptance_test() {
        let demand = parse_acceptance("Prove it: ... (test: `scene_tree_script_bridge`)");
        let reported = vec![
            "cargo check -p gdscene".to_string(),
            "cargo nextest run -p gdscene --test scene_tree_script_bridge".to_string(),
        ];
        assert_eq!(
            select_verify_commands(&reported, &demand).unwrap(),
            vec!["cargo nextest run -p gdscene --test scene_tree_script_bridge"],
            "the named test supersedes the compile check"
        );
    }

    /// An `Acceptance:` command leads the verify set even when the worker
    /// reported something else, and compile-only commands still drop out.
    #[test]
    fn test_select_verify_commands_runs_the_stated_command_first() {
        let demand = parse_acceptance("Acceptance: cargo test --test gate_test\n");
        let reported = vec![
            "cargo build -p gdcore".to_string(),
            "cargo test --test worker_choice".to_string(),
        ];
        assert_eq!(
            select_verify_commands(&reported, &demand).unwrap(),
            vec![
                "cargo test --test gate_test",
                "cargo test --test worker_choice"
            ],
            "the bead's own command leads; the build check is not evidence"
        );
    }

    /// Regression: a bead names a test **function**, but the command selects the
    /// **binary** that contains it — the two names do not match as strings, and
    /// demanding that they do rejected correct completions the moment this
    /// landed (`cross_object_script_method_call` lives in
    /// `--test cross_object_script_call_test`; `animation_tree_blend_tree_nodes_runtime`
    /// is covered by `--lib animation_tree`). Both must be accepted.
    #[test]
    fn test_select_verify_commands_accepts_a_binary_that_contains_the_test() {
        let by_binary =
            vec!["cargo nextest run -p gdscene --test cross_object_script_call_test".to_string()];
        let demand = parse_acceptance("(test: `cross_object_script_method_call`)");
        assert_eq!(
            select_verify_commands(&by_binary, &demand).unwrap(),
            by_binary,
            "a test binary whose name differs from the acceptance function must pass"
        );

        let by_module = vec!["cargo nextest run -p gdscene --lib animation_tree".to_string()];
        let demand = parse_acceptance("(test: `animation_tree_blend_tree_nodes_runtime`)");
        assert!(
            select_verify_commands(&by_module, &demand).is_ok(),
            "a module filter that covers the acceptance test must pass"
        );
    }

    /// Never verify nothing: if the demand is met but every surviving command
    /// is a compile check, keep them rather than emptying the set.
    #[test]
    fn test_select_verify_commands_keeps_checks_when_no_test_survives() {
        let demand = AcceptanceDemand {
            commands: vec!["cargo check -p patina-engine".to_string()],
            test_names: vec![],
        };
        assert_eq!(
            select_verify_commands(&[], &demand).unwrap(),
            vec!["cargo check -p patina-engine"]
        );
    }

    #[test]
    fn test_extract_acceptance_commands_empty_description() {
        assert!(extract_acceptance_commands("").is_empty());
        assert!(extract_acceptance_commands("No acceptance lines here.").is_empty());
        assert!(extract_acceptance_commands("  \n  \n").is_empty());
        // "Acceptance:" with no command should be skipped
        assert!(extract_acceptance_commands("Acceptance:   \n").is_empty());
    }

    #[test]
    fn test_extract_acceptance_commands_skips_prose_contracts() {
        let desc = "Acceptance: `prd/PHASE6_3D_PARITY_AUDIT.md` maps fixture coverage to the measured 3D slice\n";
        assert!(extract_acceptance_commands(desc).is_empty());
    }

    #[test]
    fn test_extract_test_commands_accepts_doc_validation_rg() {
        let text = r#"Tests run:
- `rg -n "desktop targets" /tmp/a.md /tmp/b.rs`
"#;
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec![r#"rg -n "desktop targets" /tmp/a.md /tmp/b.rs"#]);
    }

    #[test]
    fn test_extract_test_commands_rejects_unreplayable_python_summary() {
        let text = r#"Tests run:
- `python3 direct validation script`
- `python3 - <<'PY' ... matrix-ok ... PY`
"#;
        assert!(extract_test_commands(text).is_empty());
    }

    // ─── rewrite_broad_command tests ─────────────────────────────────────

    #[test]
    fn test_rewrite_workspace_no_run() {
        assert_eq!(
            rewrite_broad_command("cargo test --workspace --no-run"),
            "cargo check --workspace"
        );
    }

    #[test]
    fn test_rewrite_scripts_wrapper_workspace_no_run() {
        assert_eq!(
            rewrite_broad_command("scripts/rust_task.sh test --workspace --no-run"),
            "cargo check --workspace",
        );
    }

    #[test]
    fn test_rewrite_workspace_with_e_filter() {
        assert_eq!(
            rewrite_broad_command("cargo nextest run --workspace -E 'test(foo)'"),
            "cargo check --workspace"
        );
    }

    #[test]
    fn test_rewrite_e_filter_without_test_flag() {
        assert_eq!(
            rewrite_broad_command("cargo nextest run -E 'test(phase6_3d_fixture_corpus)'"),
            "cargo check --workspace"
        );
    }

    #[test]
    fn test_rewrite_workspace_test_without_specific_test() {
        assert_eq!(
            rewrite_broad_command("cargo test --workspace"),
            "cargo check --workspace"
        );
    }

    #[test]
    fn test_rewrite_preserves_focused_test() {
        // --test <name> is focused — should pass through unchanged
        let cmd = "cargo nextest run --test my_specific_test";
        assert_eq!(rewrite_broad_command(cmd), cmd);
    }

    #[test]
    fn test_rewrite_preserves_focused_test_with_workspace() {
        // --test with --workspace is still focused enough — the --test flag
        // means only that one binary is compiled
        let cmd = "cargo nextest run --workspace --test my_specific_test";
        assert_eq!(rewrite_broad_command(cmd), cmd);
    }

    #[test]
    fn test_rewrite_preserves_non_cargo_commands() {
        let cmd = "pnpm test";
        assert_eq!(rewrite_broad_command(cmd), cmd);

        let cmd2 = "python3 -m pytest tests/";
        assert_eq!(rewrite_broad_command(cmd2), cmd2);
    }

    #[test]
    fn test_rewrite_preserves_cargo_check() {
        // check is already fast — don't rewrite
        let cmd = "cargo check -p my_crate";
        assert_eq!(rewrite_broad_command(cmd), cmd);
    }

    #[test]
    fn test_rewrite_case_insensitive() {
        assert_eq!(
            rewrite_broad_command("Cargo Test --Workspace --No-Run"),
            "cargo check --workspace"
        );
    }

    /// Broad commands that carry `-p <pkg>` must rewrite to a check scoped to
    /// that package, not a full-workspace check (which compiles every crate,
    /// wgpu/metal included).
    #[test]
    fn test_rewrite_scopes_check_to_named_package() {
        assert_eq!(
            rewrite_broad_command("cargo test -p gdscene --workspace --no-run"),
            "cargo check -p gdscene"
        );
        assert_eq!(
            rewrite_broad_command("cargo nextest run --workspace -p gdeditor -E 'test(foo)'"),
            "cargo check -p gdeditor"
        );
        assert_eq!(
            rewrite_broad_command("cargo nextest run -p gdrender2d -E 'test(bar)'"),
            "cargo check -p gdrender2d"
        );
        assert_eq!(
            rewrite_broad_command("cargo nextest run --package gdvariant --workspace"),
            "cargo check -p gdvariant"
        );
    }

    // ─── batch_prebuild_command (shared warm-up build) ───────────────────

    /// The union build covers every scoped `-p`/`--test` pair across the
    /// batch, assigns unscoped `--test` targets to the root package, and
    /// dedupes repeats.
    #[test]
    fn test_batch_prebuild_unions_packages_and_targets() {
        let cmds = [
            "cargo nextest run -p gdscene --test navigation_runtime_test",
            "cargo test --test editor_api_test",
            "scripts/rust_task.sh nextest run -p gdscene --test navigation_runtime_test",
            "cargo test -p gdrender2d --test=draw_test -- --ignored slow_case",
        ];
        let cmd = batch_prebuild_command(cmds.iter().copied(), "patina-engine").unwrap();
        assert_eq!(
            cmd,
            "cargo build -p gdscene -p patina-engine -p gdrender2d \
             --test navigation_runtime_test --test editor_api_test --test draw_test"
        );
    }

    /// A single distinct target (or none) gains nothing from a shared
    /// pre-build — the per-bead verify builds it anyway.
    #[test]
    fn test_batch_prebuild_needs_two_distinct_targets() {
        let one = ["cargo test --test only_test"];
        assert_eq!(batch_prebuild_command(one.iter().copied(), "root"), None);
        let dup = [
            "cargo test --test same_test",
            "cargo nextest run --test same_test",
        ];
        assert_eq!(batch_prebuild_command(dup.iter().copied(), "root"), None);
        let none: [&str; 0] = [];
        assert_eq!(batch_prebuild_command(none.iter().copied(), "root"), None);
    }

    /// Non-cargo commands, bare `-p` without `--test` (which would build ALL
    /// of the package's test binaries), `--workspace` commands, and `--test`
    /// after the ` -- ` separator must all be excluded from the union.
    #[test]
    fn test_batch_prebuild_skips_unsafe_contributors() {
        let cmds = [
            "pnpm test",
            "pytest tests/",
            "cargo nextest run -p gdeditor",
            "cargo test --workspace --test broad_test",
            "cargo test --test real_a",
            "cargo test --test real_b -- --test not_a_target",
        ];
        let cmd = batch_prebuild_command(cmds.iter().copied(), "patina-engine").unwrap();
        assert_eq!(
            cmd,
            "cargo build -p patina-engine --test real_a --test real_b"
        );
    }

    #[test]
    fn test_extract_crate_from_command_forms() {
        use super::crate_breaker::extract_crate_from_command;
        assert_eq!(
            extract_crate_from_command("cargo test -p gdscene --test t").as_deref(),
            Some("gdscene")
        );
        assert_eq!(
            extract_crate_from_command("cargo test --package gd-render2d").as_deref(),
            Some("gd-render2d")
        );
        assert_eq!(extract_crate_from_command("cargo test --test t"), None);
    }

    /// Regression: verify_commands calls rewrite_broad_command before execution.
    #[test]
    fn test_verify_commands_uses_rewrite() {
        let source = include_str!("verifier.rs");
        let fn_start = source
            .find("pub fn verify_commands(")
            .expect("verify_commands must exist");
        let body = &source[fn_start..std::cmp::min(fn_start + 1000, source.len())];
        assert!(
            body.contains("rewrite_broad_command"),
            "verify_commands must call rewrite_broad_command to prevent slow workspace builds"
        );
    }

    // ─── count_tests_run / vacuous-pass detection (A3) ───────────────────

    #[test]
    fn test_count_tests_run_libtest_nonzero() {
        let out = "running 3 tests\ntest a ... ok\ntest result: ok. 3 passed; 0 failed; 0 ignored";
        assert_eq!(count_tests_run(out), Some(3));
    }

    #[test]
    fn test_count_tests_run_libtest_zero_is_vacuous() {
        let out = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured";
        assert_eq!(count_tests_run(out), Some(0));
    }

    #[test]
    fn test_count_tests_run_libtest_sums_multiple_binaries() {
        let out = "test result: ok. 2 passed; 0 failed\n\
                   test result: ok. 5 passed; 0 failed";
        assert_eq!(count_tests_run(out), Some(7));
    }

    #[test]
    fn test_count_tests_run_nextest() {
        let out = "    Summary [   0.123s] 42 tests run, 42 passed, 0 skipped";
        assert_eq!(count_tests_run(out), Some(42));
    }

    #[test]
    fn test_count_tests_run_nextest_zero() {
        let out = "    Summary [   0.001s] 0 tests run, 0 passed, 0 skipped";
        assert_eq!(count_tests_run(out), Some(0));
    }

    #[test]
    fn test_count_tests_run_pytest_collected_zero() {
        assert_eq!(
            count_tests_run("collected 0 items\n\nno tests ran in 0.01s"),
            Some(0)
        );
    }

    #[test]
    fn test_count_tests_run_unrecognized_is_none() {
        // No test-runner summary → we make no claim (don't flag as vacuous).
        assert_eq!(count_tests_run("Finished dev profile in 1.2s"), None);
        assert_eq!(count_tests_run(""), None);
    }

    #[test]
    fn test_is_test_run_command_excludes_check_and_build() {
        assert!(!is_test_run_command("cargo check --workspace"));
        assert!(!is_test_run_command("cargo build -p foo"));
        assert!(!is_test_run_command("cargo fmt --check"));
        assert!(!is_test_run_command("rg -n pattern file"));
        assert!(is_test_run_command("cargo test --test foo"));
        assert!(is_test_run_command("cargo nextest run -p bar"));
        assert!(is_test_run_command("pytest tests/"));
        assert!(is_test_run_command(
            "scripts/rust_task.sh nextest run -p crate"
        ));
        // rust_task.sh check wrapper is not a test run
        assert!(!is_test_run_command(
            "scripts/rust_task.sh check --workspace"
        ));
    }

    // ─── classify_failure (C2) ───────────────────────────────────────────

    #[test]
    fn test_classify_failure_categories() {
        assert_eq!(
            classify_failure("error[E0425]: cannot find value `x` in this scope"),
            "compile-error"
        );
        assert_eq!(
            classify_failure("error: could not compile `gdscene` due to 2 errors"),
            "compile-error"
        );
        assert_eq!(
            classify_failure("thread 'main' panicked at src/lib.rs:10:5"),
            "panic"
        );
        assert_eq!(
            classify_failure("assertion `left == right` failed\n  left: 1\n right: 2"),
            "assertion-failure"
        );
        assert_eq!(
            classify_failure("Error: Address already in use (os error 48)"),
            "possibly-environmental"
        );
        assert_eq!(classify_failure("some other unexpected output"), "failure");
    }

    // ─── tail_chars (C2 output tail) ─────────────────────────────────────

    #[test]
    fn test_tail_chars_short_passthrough() {
        assert_eq!(tail_chars("hello", 100), "hello");
        assert_eq!(tail_chars("hello\n\n  ", 100), "hello");
    }

    #[test]
    fn test_tail_chars_truncates_with_marker() {
        let s = "abcdefghij";
        assert_eq!(tail_chars(s, 4), "…ghij");
    }

    #[test]
    fn test_tail_chars_handles_multibyte() {
        // Must not panic on a char boundary mid-truncation.
        let s = "αβγδεζηθ";
        let out = tail_chars(s, 3);
        assert_eq!(out, "…ζηθ");
    }

    // ─── E3: adversarial negative tests for extract_test_commands ────────
    // Prose, comments, and diffs that merely *mention* test-shaped words must
    // not be extracted as runnable verification commands.

    #[test]
    fn test_extract_rejects_prose_mentioning_tests_without_command() {
        let samples = [
            "I added test coverage for the new code path.",
            "The cargo workspace builds fine now.",
            "// run the tests before committing",
            "We should test this more thoroughly.",
            "diff --git a/tests/foo.rs b/tests/foo.rs\n+    // testing the thing",
            "See the testing guide in docs/.",
            "This feature needs a regression test.",
        ];
        for s in samples {
            assert!(
                extract_test_commands(s).is_empty(),
                "must not extract a command from prose: {s:?} → {:?}",
                extract_test_commands(s)
            );
        }
    }

    #[test]
    fn test_extract_rejects_test_word_inside_identifier() {
        // "latest" / "greatest" contain "test" but are not commands.
        assert!(extract_test_commands("the latest greatest build is ready").is_empty());
    }

    /// Regression: rust_task.sh worker guard must exist.
    #[test]
    fn test_rust_task_sh_has_worker_guard() {
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/rust_task.sh"),
        )
        .expect("rust_task.sh must exist");
        assert!(
            script.contains("BLOCKED: worker"),
            "rust_task.sh must block non-verifier agents from running Rust builds"
        );
        assert!(
            script.contains("AGENT_NAME") && script.contains("verifier"),
            "rust_task.sh must allow the verifier agent through"
        );
    }

    /// Regression (B2): rust_task.sh must reclaim an abandoned build slot so a
    /// SIGKILL'd verifier (which skips the EXIT trap) can't wedge all builds.
    #[test]
    fn test_rust_task_sh_reaps_stale_lock() {
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/rust_task.sh"),
        )
        .expect("rust_task.sh must exist");
        assert!(
            script.contains("lock_is_stale") && script.contains("kill -0"),
            "rust_task.sh must detect a dead lock owner"
        );
        assert!(
            script.contains("reclaiming abandoned build slot"),
            "rust_task.sh must reclaim a stale lock instead of waiting forever"
        );
    }

    /// Regression (D1): rust_task.sh must enable build caching (sccache when
    /// available, otherwise Cargo incremental) to amortize per-bead compiles.
    #[test]
    fn test_rust_task_sh_enables_build_cache() {
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/rust_task.sh"),
        )
        .expect("rust_task.sh must exist");
        assert!(
            script.contains("CARGO_INCREMENTAL"),
            "must set CARGO_INCREMENTAL"
        );
        assert!(
            script.contains("sccache") && script.contains("RUSTC_WRAPPER"),
            "must prefer sccache when present"
        );
    }
}
