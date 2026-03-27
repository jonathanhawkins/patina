use std::process::Command;

use crate::error::{OrchestratorError, Result};

/// Information about a tmux pane from `list-panes`.
#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub index: u32,
    pub id: String,
    pub dead: bool,
    pub current_command: String,
}

fn tmux_error(msg: impl Into<String>) -> OrchestratorError {
    OrchestratorError::Tmux(msg.into())
}

fn run_tmux(args: &[&str]) -> Result<String> {
    let output = Command::new("tmux")
        .args(args)
        .output()
        .map_err(|e| tmux_error(format!("failed to spawn tmux: {e}")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(tmux_error(format!(
            "tmux {} failed: {}",
            args.join(" "),
            stderr.trim()
        )))
    }
}

/// Capture the last N lines from a tmux pane.
pub fn capture_pane(session: &str, window: u32, pane: u32, lines: usize) -> Result<String> {
    let target = format!("{session}:{window}.{pane}");
    let full_capture = run_tmux(&["capture-pane", "-p", "-t", &target])?;

    // Return only the last N lines (matching `tail -n N`)
    let all_lines: Vec<&str> = full_capture.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    Ok(all_lines[start..].join("\n"))
}

/// Send keys to a tmux pane (interpreted, e.g. C-u, Enter).
pub fn send_keys(session: &str, window: u32, pane: u32, keys: &str) -> Result<()> {
    let target = format!("{session}:{window}.{pane}");
    run_tmux(&["send-keys", "-t", &target, keys])?;
    Ok(())
}

/// Send literal text to a tmux pane (using -l flag, no key interpretation).
pub fn send_literal(session: &str, window: u32, pane: u32, text: &str) -> Result<()> {
    let target = format!("{session}:{window}.{pane}");
    run_tmux(&["send-keys", "-t", &target, "-l", "--", text])?;
    Ok(())
}

/// List all panes in a window with their metadata.
pub fn list_panes(session: &str, window: u32) -> Result<Vec<PaneInfo>> {
    let target = format!("{session}:{window}");
    let output = run_tmux(&[
        "list-panes",
        "-t",
        &target,
        "-F",
        "#{pane_index}|#{pane_id}|#{pane_dead}|#{pane_current_command}",
    ])?;

    let mut panes = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(4, '|').collect();
        if parts.len() < 4 {
            continue;
        }
        let index = match parts[0].parse::<u32>() {
            Ok(i) => i,
            Err(_) => continue,
        };
        panes.push(PaneInfo {
            index,
            id: parts[1].to_string(),
            dead: parts[2] != "0",
            current_command: parts[3].to_string(),
        });
    }
    Ok(panes)
}

/// Check if a tmux session exists.
pub fn has_session(session: &str) -> bool {
    let target = format!("={session}");
    Command::new("tmux")
        .args(["has-session", "-t", &target])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if the bv pane (index 2) is alive and running `bv`.
/// If bv crashed or was suspended, restart it.
pub fn ensure_bv_alive(session: &str, window: u32, workdir: &str) -> Result<bool> {
    let target = format!("{session}:{window}");
    let panes = list_panes(session, window)?;

    let bv_pane = panes.iter().find(|p| p.index == 2);
    let needs_restart = match bv_pane {
        None => false, // pane doesn't exist at all — layout may be different
        Some(p) if p.dead => true,
        Some(p) if p.current_command != "bv" => true,
        _ => false,
    };

    if needs_restart {
        tracing::info!("bv pane is not running — restarting");
        let bv_cmd = format!("cd {workdir} && exec bv");
        run_tmux(&[
            "respawn-pane",
            "-k",
            "-t",
            &format!("{target}.2"),
            &bv_cmd,
        ])?;
        return Ok(true);
    }

    Ok(false)
}

/// A queued prompt task for batch/parallel submission.
#[derive(Clone)]
pub struct PromptTask {
    pub session: String,
    pub window: u32,
    pub pane: u32,
    pub text: String,
    pub pcfg: PromptConfig,
}

/// Submit multiple prompts in parallel using std::thread::scope.
/// Each prompt runs in its own thread — tmux calls are I/O-bound (subprocess),
/// so parallelism lets us prompt 9 workers in ~4s instead of ~36s serial.
pub fn submit_prompts_parallel(tasks: &[PromptTask]) -> Vec<bool> {
    if tasks.is_empty() {
        return Vec::new();
    }

    tracing::info!(
        count = tasks.len(),
        panes = ?tasks.iter().map(|t| t.pane).collect::<Vec<_>>(),
        "Starting parallel prompt submission"
    );

    // Stagger thread starts by 200ms to avoid overwhelming tmux's single-threaded
    // event loop. Without staggering, 8 simultaneous send-keys can interleave
    // and cause dropped keystrokes.
    std::thread::scope(|s| {
        let handles: Vec<_> = tasks
            .iter()
            .enumerate()
            .map(|(i, task)| {
                s.spawn(move || {
                    // Stagger: each thread waits i * 200ms before starting
                    if i > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(i as u64 * 200));
                    }
                    let t = std::time::Instant::now();
                    let result = prompt_worker_pane(
                        &task.session,
                        task.window,
                        task.pane,
                        &task.text,
                        &task.pcfg,
                    )
                    .unwrap_or(false);
                    tracing::info!(
                        pane = task.pane,
                        ok = result,
                        elapsed_ms = t.elapsed().as_millis() as u64,
                        "Parallel prompt: pane done"
                    );
                    result
                })
            })
            .collect();

        handles.into_iter().map(|h| h.join().unwrap_or(false)).collect()
    })
}

/// Configurable timing/key parameters for prompt submission.
#[derive(Clone)]
pub struct PromptConfig {
    pub queue_prompt_marker: String,
    pub shell_prompt_char: char,
    pub clear_line_key: String,
    pub submit_key: String,
    pub prompt_submit_delay_ms: u64,
    pub shell_prompt_wait_ms: u64,
    pub post_clear_delay_ms: u64,
    pub post_type_delay_ms: u64,
    pub submit_retry_attempts: u32,
    pub shell_prompt_timeout_secs: u64,
    pub capture_lines: usize,
}

/// Check if orchestrator prompt text is still visible in a pane capture.
/// Used for **post-submit verification** — after typing prompt + Enter, this
/// checks whether Enter went through or the text is still in the input buffer.
///
/// IMPORTANT: Only uses START markers (`/skill flywheel-worker` + `Work bead`),
/// NOT end markers. After successful submission, the prompt becomes part of
/// the conversation history and end markers (at the tail) persist longer in
/// the visible area, causing false positives that make the coordinator think
/// Enter failed when it actually succeeded.
///
/// For **stuck pane detection** (where false positives are less costly),
/// `worker::has_stuck_input` uses BOTH start and end markers to catch
/// narrow-pane cases where start markers scroll off.
fn prompt_text_visible_in_capture(capture: &str) -> bool {
    let flat = capture.replace('\n', "");
    flat.contains("/skill flywheel-worker") && flat.contains("Work bead")
}

/// Send a prompt to a worker pane: clear input, type text, submit.
/// Returns Ok(true) if submitted, Ok(false) if prompt remained queued.
pub fn prompt_worker_pane(
    session: &str,
    window: u32,
    pane: u32,
    prompt_text: &str,
    pcfg: &PromptConfig,
) -> Result<bool> {
    let queue_pattern = &pcfg.queue_prompt_marker;

    // Check current state first
    let capture = capture_pane(session, window, pane, pcfg.capture_lines)?;

    // If there's already a queued prompt, do NOT add another one.
    // Just try to submit what's there.
    if capture.contains(queue_pattern.as_str()) {
        tracing::info!(pane = pane, "existing queued prompt found, submitting it");
        for _ in 0..pcfg.submit_retry_attempts {
            send_keys(session, window, pane, &pcfg.submit_key)?;
            std::thread::sleep(std::time::Duration::from_millis(pcfg.prompt_submit_delay_ms));
            let c = capture_pane(session, window, pane, pcfg.capture_lines)?;
            if !c.contains(queue_pattern.as_str()) {
                return Ok(true);
            }
        }
        // Enter didn't work — escalate: Escape to dismiss, then re-paste fresh
        tracing::warn!(pane = pane, "existing queued prompt stuck — Escape + re-paste");
        send_keys(session, window, pane, "Escape")?;
        std::thread::sleep(std::time::Duration::from_millis(300));
        // Fall through to the normal paste-and-submit flow below
    }

    // Wait for shell prompt (up to configured timeout)
    let has_prompt = capture.contains(pcfg.shell_prompt_char);
    if !has_prompt {
        for _ in 0..pcfg.shell_prompt_timeout_secs {
            std::thread::sleep(std::time::Duration::from_millis(pcfg.shell_prompt_wait_ms));
            let c = capture_pane(session, window, pane, pcfg.capture_lines)?;
            if c.contains(pcfg.shell_prompt_char) {
                break;
            }
        }
    }

    // Clear line, then paste prompt text atomically via tmux paste-buffer.
    // Using send-keys -l for long text is character-by-character delivery which
    // races with tmux's event loop when multiple panes are targeted in parallel.
    // paste-buffer delivers the ENTIRE text as a single block, then we send Enter.
    send_keys(session, window, pane, &pcfg.clear_line_key)?;
    std::thread::sleep(std::time::Duration::from_millis(pcfg.post_clear_delay_ms));

    let target = format!("{session}:{window}.{pane}");

    // Paste text atomically via tmux buffer, then send Enter separately.
    // paste-buffer delivers the whole text as one block (not char-by-char like send-keys -l).
    let buf_name = format!("prompt-{pane}");
    run_tmux(&["set-buffer", "-b", &buf_name, "--", prompt_text])?;
    run_tmux(&["paste-buffer", "-b", &buf_name, "-t", &target, "-d"])?;

    // Send Enter with a small delay, then a backup Enter 200ms later.
    // The backup is harmless if the first succeeds (Claude ignores blank submissions).
    std::thread::sleep(std::time::Duration::from_millis(150));
    run_tmux(&["send-keys", "-t", &target, &pcfg.submit_key])?;
    std::thread::sleep(std::time::Duration::from_millis(200));
    run_tmux(&["send-keys", "-t", &target, &pcfg.submit_key])?;

    // Wait for Claude to process
    std::thread::sleep(std::time::Duration::from_millis(pcfg.prompt_submit_delay_ms));

    // Check if it submitted
    let capture = capture_pane(session, window, pane, pcfg.capture_lines)?;
    if !capture.contains(queue_pattern.as_str()) {
        // No queue marker — but verify the prompt text is gone (Enter actually went through).
        // If prompt text is still visible, Enter likely failed silently.
        if !prompt_text_visible_in_capture(&capture) {
            return Ok(true);
        }
        // Prompt text still visible — retry Enter
        tracing::warn!(pane = pane, "prompt text still visible after submit — retrying Enter");
        for _ in 0..pcfg.submit_retry_attempts {
            send_keys(session, window, pane, &pcfg.submit_key)?;
            std::thread::sleep(std::time::Duration::from_millis(pcfg.prompt_submit_delay_ms));
            let c = capture_pane(session, window, pane, pcfg.capture_lines)?;
            if !prompt_text_visible_in_capture(&c) {
                return Ok(true);
            }
        }
        tracing::warn!(pane = pane, "prompt text persists after Enter retries");
        return Ok(false);
    }

    // Got queued — try submitting a few more times
    for _ in 0..pcfg.submit_retry_attempts {
        send_keys(session, window, pane, &pcfg.submit_key)?;
        std::thread::sleep(std::time::Duration::from_millis(pcfg.prompt_submit_delay_ms));
        let c = capture_pane(session, window, pane, pcfg.capture_lines)?;
        if !c.contains(queue_pattern.as_str()) {
            return Ok(true);
        }
    }

    // Enter retries exhausted — escalate: dismiss the queue with Escape,
    // then re-paste and submit the prompt fresh.
    tracing::warn!(pane = pane, "prompt queued after Enter retries — escalating with Escape + re-paste");
    send_keys(session, window, pane, "Escape")?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Re-paste and submit
    let buf_name = format!("prompt-{pane}");
    run_tmux(&["set-buffer", "-b", &buf_name, "--", prompt_text])?;
    run_tmux(&["paste-buffer", "-b", &buf_name, "-t", &target, "-d"])?;
    std::thread::sleep(std::time::Duration::from_millis(150));
    run_tmux(&["send-keys", "-t", &target, &pcfg.submit_key])?;
    std::thread::sleep(std::time::Duration::from_millis(200));
    run_tmux(&["send-keys", "-t", &target, &pcfg.submit_key])?;
    std::thread::sleep(std::time::Duration::from_millis(pcfg.prompt_submit_delay_ms));

    let c = capture_pane(session, window, pane, pcfg.capture_lines)?;
    if !c.contains(queue_pattern.as_str()) {
        tracing::info!(pane = pane, "prompt submitted after Escape escalation");
        return Ok(true);
    }

    tracing::warn!(pane = pane, "prompt still queued after Escape escalation");
    Ok(false)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Expose prompt_text_visible_in_capture for cross-module contract tests.
    pub fn prompt_text_visible_for_test(capture: &str) -> bool {
        prompt_text_visible_in_capture(capture)
    }

    #[test]
    fn test_prompt_text_visible_when_present() {
        let capture = "❯ Use /skill flywheel-worker. Work bead pat-123: Fix bug. Your Agent Mail identity is WorkerA.";
        assert!(prompt_text_visible_in_capture(capture));
    }

    #[test]
    fn test_prompt_text_visible_with_garbled_wrapping() {
        // Realistic: tmux wraps the long prompt across lines
        let capture = "+loa\n+t\" 2069 +\nUse /skill flywheel-worker. Work bead pat-456: Add feature.";
        assert!(prompt_text_visible_in_capture(capture));
    }

    #[test]
    fn test_prompt_text_not_visible_in_normal_output() {
        assert!(!prompt_text_visible_in_capture("Compiling patina v0.1"));
        assert!(!prompt_text_visible_in_capture("❯ "));
        assert!(!prompt_text_visible_in_capture("✢ Searching for files..."));
        assert!(!prompt_text_visible_in_capture("completion sent via ./apps/orchestrator/mail/complete"));
    }

    #[test]
    fn test_prompt_text_needs_both_substrings() {
        // Only one substring → not detected
        assert!(!prompt_text_visible_in_capture("Use /skill flywheel-worker. Do something."));
        assert!(!prompt_text_visible_in_capture("Work bead pat-123: Fix bug."));
        // Both present → detected
        assert!(prompt_text_visible_in_capture("/skill flywheel-worker stuff Work bead pat-123"));
    }

    // ========================================================================
    // Integration tests: wrapping robustness, post-submit verification edge
    // cases, and cross-module contract with worker::has_stuck_input.
    // ========================================================================

    /// Helper: simulate tmux visual line wrapping at a given column width.
    fn wrap_at_cols(text: &str, cols: usize) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut lines = Vec::new();
        for chunk in chars.chunks(cols) {
            lines.push(chunk.iter().collect::<String>());
        }
        lines.join("\n")
    }

    /// Helper: generate a realistic orchestrator prompt.
    fn fake_prompt(bead_id: &str, title: &str, worker: &str) -> String {
        format!(
            "Use /skill flywheel-worker. Work bead {bead_id}: {title}. \
             Your Agent Mail identity is {worker}. Read your inbox for full \
             assignment details. Close out with /skill mail-complete, \
             not a freehand completion message."
        )
    }

    // --- Wrapping robustness for prompt_text_visible_in_capture ---

    #[test]
    fn test_prompt_visible_at_all_realistic_pane_widths() {
        let prompt = fake_prompt("pat-tw1", "Implement rendering pipeline", "PearlFox");

        for cols in (20..=200).step_by(5) {
            let wrapped = wrap_at_cols(&prompt, cols);
            assert!(
                prompt_text_visible_in_capture(&wrapped),
                "prompt_text_visible_in_capture failed at {cols} cols.\nWrapped:\n{wrapped}"
            );
        }
    }

    #[test]
    fn test_prompt_visible_with_prefix_at_all_widths() {
        let prompt = fake_prompt("pat-tw2", "Add caching", "GrayBay");
        let full = format!("❯ {prompt}");

        for cols in (20..=200).step_by(5) {
            let wrapped = wrap_at_cols(&full, cols);
            assert!(
                prompt_text_visible_in_capture(&wrapped),
                "failed with prefix at {cols} cols"
            );
        }
    }

    #[test]
    fn test_prompt_not_visible_after_claude_consumes_it() {
        // After Claude Code accepts the prompt, the pane shows Claude's UI,
        // not the raw prompt text. Verify these realistic post-submit captures
        // are NOT detected as having visible prompt text.
        let post_submit_captures = vec![
            "❯\n✢ Searching for files...\nReading 3 files\n",
            "✻ Clauding… (2m 30s)\n  Running cargo test\n",
            "  Applied patch to src/main.rs\n  Compiling patina v0.1\n",
            "Press up to edit\n❯ ",
            "❯ ",
            "",
        ];

        for capture in &post_submit_captures {
            assert!(
                !prompt_text_visible_in_capture(capture),
                "false positive on post-submit capture: '{capture}'"
            );
        }
    }

    // --- Cross-module contract: tmux and worker detection must agree ---

    #[test]
    fn test_tmux_and_worker_detection_agree_on_prompts() {
        // Both functions should detect the same prompts at the same widths.
        let prompts = vec![
            fake_prompt("pat-xm1", "Fix bug", "W1"),
            fake_prompt("pat-xm2", "Very long title with many words to test wrapping", "LongWorkerName"),
            fake_prompt("pat-xm3", "Short", "W"),
        ];

        for prompt in &prompts {
            for cols in &[20, 25, 30, 40, 60, 80, 120] {
                let wrapped = wrap_at_cols(prompt, *cols);
                let tmux_sees = prompt_text_visible_in_capture(&wrapped);
                let worker_sees = crate::worker::has_stuck_input(&wrapped);
                assert_eq!(
                    tmux_sees, worker_sees,
                    "tmux/worker disagreement at {cols} cols for '{}'",
                    &prompt[..prompt.len().min(40)]
                );
            }
        }
    }

    #[test]
    fn test_tmux_and_worker_detection_agree_on_non_prompts() {
        let non_prompts = vec![
            "Compiling patina v0.1",
            "❯ ",
            "test result: ok. 42 passed",
            "✢ Searching for files...",
            "completion sent via ./apps/orchestrator/mail/complete",
            "/skill flywheel-worker but no work bead",
            "Work bead pat-123 but no flywheel-worker",
        ];

        for text in &non_prompts {
            assert_eq!(
                prompt_text_visible_in_capture(text),
                crate::worker::has_stuck_input(text),
                "tmux/worker disagreement on non-prompt: '{text}'"
            );
        }
    }

    // --- Post-submit verification edge cases ---

    #[test]
    fn test_post_submit_correctly_identifies_stuck_vs_consumed() {
        // Simulate the two outcomes after typing prompt + Enter:
        //
        // Outcome A: Enter went through → Claude consumed it, pane shows activity
        let consumed = "✢ Searching for files...\nReading 3 files\nRunning cargo test";
        assert!(!prompt_text_visible_in_capture(consumed), "consumed prompt should not be visible");

        // Outcome B: Enter didn't go through → raw prompt text still in pane
        let stuck = fake_prompt("pat-ps1", "Fix", "W");
        assert!(prompt_text_visible_in_capture(&stuck), "stuck prompt should be visible");
    }

    #[test]
    fn test_post_submit_with_queue_marker_and_prompt_text() {
        // If prompt got queued, both "Press up to edit" AND prompt text are visible.
        // The queue-marker check in prompt_worker_pane handles this path,
        // but prompt_text_visible_in_capture should still return true.
        let prompt = fake_prompt("pat-ps2", "Add feature", "W");
        let capture = format!("Press up to edit\n{prompt}");
        assert!(prompt_text_visible_in_capture(&capture));
    }

    // --- Extreme edge cases ---

    #[test]
    fn test_prompt_visible_with_unicode_in_title() {
        let prompt = fake_prompt("pat-u1", "Fix émoji 🎮 rendering", "Worker");
        assert!(prompt_text_visible_in_capture(&prompt));

        let wrapped = wrap_at_cols(&prompt, 30);
        assert!(prompt_text_visible_in_capture(&wrapped));
    }

    #[test]
    fn test_prompt_visible_with_extremely_narrow_pane() {
        // 15-column pane — each marker substring gets split multiple times
        let prompt = fake_prompt("pat-en1", "Fix", "W");
        let wrapped = wrap_at_cols(&prompt, 15);
        assert!(
            prompt_text_visible_in_capture(&wrapped),
            "should detect even at 15 cols"
        );
    }

    #[test]
    fn test_empty_capture_not_detected() {
        assert!(!prompt_text_visible_in_capture(""));
        assert!(!prompt_text_visible_in_capture("\n\n\n"));
    }

    // ========================================================================
    // Regression: prompt_text_visible_in_capture must NOT use end markers.
    //
    // Bug: Adding end markers to post-submit verification caused false
    // positives — after Enter succeeded, the prompt text persisted in
    // conversation history, and end markers (at the tail) stayed visible
    // longer than start markers, making the coordinator think Enter failed.
    // ========================================================================

    /// Regression: post-submit check must NOT detect end-of-prompt alone.
    /// After Enter goes through, the prompt becomes conversation history
    /// and end markers persist. Only start markers should be checked here.
    #[test]
    fn test_prompt_visible_does_not_use_end_markers() {
        // Tail-only capture (no start markers) — must NOT be detected
        let tail = "details. Close out with /skill mail-complete, \
                    not a freehand completion message.";
        assert!(
            !prompt_text_visible_in_capture(tail),
            "post-submit check must NOT use end markers (causes false positives)"
        );
    }

    /// Verify start markers still work for post-submit verification.
    #[test]
    fn test_prompt_visible_start_markers_still_work() {
        let prompt = fake_prompt("pat-post1", "Fix rendering", "Worker");
        assert!(prompt_text_visible_in_capture(&prompt));

        // After Claude consumes, start markers should be gone
        let post_submit = "✢ Searching for files...\nReading 3 files\nRunning cargo test";
        assert!(!prompt_text_visible_in_capture(post_submit));
    }
}
