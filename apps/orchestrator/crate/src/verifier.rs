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
/// that enumerate all 300+ test binaries. Replace with `check --workspace` for
/// compile-only checks, or strip `-E`/`--workspace` from test commands.
fn rewrite_broad_command(cmd: &str) -> String {
    let lower = cmd.to_lowercase();

    // `cargo test --workspace --no-run` → just check it compiles
    if lower.contains("--workspace") && lower.contains("--no-run") {
        return "cargo check --workspace".to_string();
    }

    // `cargo nextest run --workspace -E <expr>` → extract --test from the -E if possible,
    // otherwise fall back to check
    if lower.contains("--workspace") && lower.contains("-e ") {
        return "cargo check --workspace".to_string();
    }

    // `nextest run -E <expr>` without --test → fall back to check
    // These enumerate all binaries to find matches
    if (lower.contains("nextest run") || lower.contains("nextest  run"))
        && lower.contains("-e ")
        && !lower.contains("--test ")
    {
        return "cargo check --workspace".to_string();
    }

    // `cargo test --workspace` without --test → check only
    if lower.contains("--workspace")
        && (lower.contains("cargo test") || lower.contains("cargo nextest"))
        && !lower.contains("--test ")
    {
        return "cargo check --workspace".to_string();
    }

    cmd.to_string()
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

    let mut child = Command::new("bash")
        .args(["-lc", cmd])
        .current_dir(cwd)
        .env("AGENT_NAME", "verifier")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
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
            // Timed out — kill and reap bash. Don't join the readers (a lingering
            // grandchild could hold the pipe open); let them detach harmlessly.
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
    };

    let out = out_handle.join().unwrap_or_default();
    let err = err_handle.join().unwrap_or_default();
    Ok(Some((status, format!("{out}{err}"))))
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
