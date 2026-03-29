use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use regex::Regex;
use std::sync::OnceLock;

use crate::error::{OrchestratorError, Result};

/// Known command prefixes (case-insensitive) that identify test/build commands.
const KNOWN_PREFIXES: &[&str] = &[
    "cargo test",
    "cargo nextest",
    "cargo check",
    "pnpm test",
    "pnpm lint",
    "pnpm exec",
    "npm test",
    "npm run",
    "yarn test",
    "yarn ",
    "pytest",
    "python -m pytest",
    "uv run pytest",
    "go test",
    "bun test",
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
        let inner = inner
            .strip_prefix("```")
            .unwrap_or(inner);
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
    let strip_fenced =
        STRIP_FENCED.get_or_init(|| Regex::new(r"(?s)```.*?```").unwrap());
    static STRIP_INLINE: OnceLock<Regex> = OnceLock::new();
    let strip_inline =
        STRIP_INLINE.get_or_init(|| Regex::new(r"`[^`]+`").unwrap());
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
    description
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let stripped = trimmed.strip_prefix("Acceptance:")?;
            let cmd = stripped.trim();
            if cmd.is_empty() {
                None
            } else {
                Some(cmd.to_string())
            }
        })
        .collect()
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

/// Run each command via bash and return an error on failure or timeout.
pub fn verify_commands(
    project_root: &Path,
    commands: &[String],
    timeout: Duration,
) -> Result<()> {
    if commands.is_empty() {
        return Err(OrchestratorError::Verification(
            "no test commands to verify".into(),
        ));
    }

    for cmd in commands {
        let cwd = command_workdir(project_root, cmd);
        tracing::info!("[verify] running in {}: {}", cwd.display(), cmd);

        let mut child = Command::new("bash")
            .args(["-lc", cmd])
            .current_dir(&cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                OrchestratorError::Verification(format!("failed to spawn bash: {e}"))
            })?;

        let status = match child.wait_timeout(timeout) {
            Ok(Some(s)) => s,
            Ok(None) => {
                // Timed out — kill the child
                let _ = child.kill();
                let _ = child.wait();
                return Err(OrchestratorError::Verification(format!(
                    "timed out after {}s: {cmd}",
                    timeout.as_secs()
                )));
            }
            Err(e) => {
                return Err(OrchestratorError::Verification(format!(
                    "error waiting for command: {e}"
                )));
            }
        };

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            return Err(OrchestratorError::Verification(format!(
                "failed ({code}): {cmd}"
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
        assert_eq!(cmds, vec!["cargo test --test signal_trace_fixture_parity_test"]);
    }

    #[test]
    fn test_extract_from_inline_backtick() {
        let text = "Run `cargo test --test foo_test` to verify.";
        let cmds = extract_test_commands(text);
        assert_eq!(cmds, vec!["cargo test --test foo_test"]);
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
        assert_eq!(
            cmds,
            vec!["cargo test --test a", "cargo test --test b"]
        );
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

        assert_eq!(
            command_workdir(tmp.path(), "pnpm test"),
            web_dir
        );
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
        assert_eq!(cmds, vec!["cargo test --test signal_trace_fixture_parity_test"]);
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
}
