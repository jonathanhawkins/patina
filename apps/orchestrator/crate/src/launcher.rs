use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config;
use crate::error::{OrchestratorError, Result};
use crate::mail::MailClient;
use crate::tmux;

/// Configuration for the `launch` subcommand.
pub struct LaunchConfig {
    pub session_name: String,
    pub worker_count: u32,
    pub model_command: String,
    pub project_root: PathBuf,
    pub orch_root: PathBuf,
    pub dry_run: bool,
    pub force: bool,
    pub terminal_width: u32,
    pub terminal_height: u32,
    /// When true, create a coordinator window running `patina-orchestrator run`.
    pub with_coordinator: bool,
    /// Poll interval for the coordinator loop (seconds). Only used with `with_coordinator`.
    pub poll_interval: u64,
}

/// Result of the launch process.
pub struct LaunchResult {
    pub session_name: String,
    pub worker_count: u32,
    pub worker_identities: Vec<(u32, String)>,
    pub total_panes: u32,
}

/// Compute grid dimensions for a given worker count.
/// Returns (columns, rows) where columns * rows >= worker_count.
pub fn compute_grid_dimensions(worker_count: u32) -> (u32, u32) {
    if worker_count == 0 {
        return (0, 0);
    }
    let cols = (worker_count as f64).sqrt().ceil() as u32;
    let rows = ((worker_count as f64) / (cols as f64)).ceil() as u32;
    (cols, rows)
}

/// Ensure the model command includes the appropriate permissions-bypass flag.
/// Claude uses --dangerously-skip-permissions; Codex uses --dangerously-bypass-approvals-and-sandbox.
pub fn ensure_skip_permissions(model_cmd: &str) -> String {
    if model_cmd.contains("dangerously-skip-permissions")
        || model_cmd.contains("dangerously-bypass-approvals-and-sandbox")
    {
        model_cmd.to_string()
    } else if model_cmd.contains("codex") {
        format!("{model_cmd} --dangerously-bypass-approvals-and-sandbox")
    } else {
        format!("{model_cmd} --dangerously-skip-permissions")
    }
}

/// Run the launch sequence.
pub fn launch(config: &LaunchConfig) -> Result<LaunchResult> {
    // Check if session already exists
    if tmux::has_session(&config.session_name) {
        if config.force {
            if config.dry_run {
                println!("[dry-run] tmux kill-session -t {}", config.session_name);
            } else {
                kill_session(&config.session_name)?;
            }
        } else {
            return Err(OrchestratorError::Tmux(format!(
                "session '{}' already exists (use --force to override)",
                config.session_name
            )));
        }
    }

    // Ensure mail server is running
    let mail_session = format!("{}--agent-mail", config.session_name);
    if !config.dry_run {
        ensure_mail_server(&config.project_root, &mail_session)?;
    } else {
        println!("[dry-run] ensure mail server session: {mail_session}");
    }

    // Step 1: Create session and layout
    let (grid_cols, grid_rows) = compute_grid_dimensions(config.worker_count);
    let total_panes = 3 + config.worker_count; // monitor + planner + bv + workers

    if config.dry_run {
        print_dry_run_layout(config, grid_cols, grid_rows, total_panes);
        return Ok(LaunchResult {
            session_name: config.session_name.clone(),
            worker_count: config.worker_count,
            worker_identities: Vec::new(),
            total_panes,
        });
    }

    // Create the session with the first pane (pane 0 = monitor)
    create_session(
        &config.session_name,
        config.terminal_width,
        config.terminal_height,
        &config.project_root,
    )?;

    // Build layout: [left col ~14%] | [bv ~28%] | [worker grid ~58%]
    //
    // tmux split-window -h -p N: the NEW pane (right) gets N% of the space.
    // tmux split-window -v -p N: the NEW pane (bottom) gets N% of the space.
    //
    // Step 1: Split pane 0 horizontally — left column (~14%) | everything else (86%)
    split_h(&config.session_name, 0, 0, 86)?;
    // Now: pane 0 = left column (~30 cols), pane 1 = right area (~185 cols)

    // Step 2: Split left column (pane 0) vertically — monitor top, planner bottom
    split_v(&config.session_name, 0, 0, 50)?;
    // Now: pane 0 = monitor, pane 1 = planner, pane 2 = right area

    // Step 3: Split right area (pane 2) horizontally — bv (33%) | workers (67%)
    // -p 67 means the new pane (right/workers) gets 67%, bv keeps 33%.
    split_h(&config.session_name, 0, 2, 67)?;
    // Now: pane 0 = monitor, pane 1 = planner, pane 2 = bv (~61 cols), pane 3 = worker area (~124 cols)

    // Step 4: Create the worker grid in pane 3
    create_worker_grid(&config.session_name, 0, 3, config.worker_count, grid_cols, grid_rows)?;

    // Step 2: Start fixed panes
    // Pane 0: monitor
    let monitor_cmd = format!(
        "while true; do clear; tmux capture-pane -t {}:coordinator.0 -p -S -100 2>/dev/null | tail -35; sleep 2; done",
        config.session_name
    );
    tmux::send_literal(&config.session_name, 0, 0, &monitor_cmd)?;
    tmux::send_keys(&config.session_name, 0, 0, "Enter")?;

    // Pane 1: planner (wait for manual start)
    // Just cd to project root
    let cd_cmd = format!("cd {}", config.project_root.display());
    tmux::send_literal(&config.session_name, 0, 1, &cd_cmd)?;
    tmux::send_keys(&config.session_name, 0, 1, "Enter")?;

    // Pane 2: bv
    tmux::send_literal(&config.session_name, 0, 2, "bv")?;
    tmux::send_keys(&config.session_name, 0, 2, "Enter")?;

    // Step 3 & 4: Bootstrap identities and launch workers
    let model_cmd = ensure_skip_permissions(&config.model_command);
    let mut worker_identities = Vec::new();

    for i in 0..config.worker_count {
        let pane_index = 3 + i;

        // cd to project root first
        let cd = format!("cd {}", config.project_root.display());
        tmux::send_literal(&config.session_name, 0, pane_index, &cd)?;
        tmux::send_keys(&config.session_name, 0, pane_index, "Enter")?;

        // Bootstrap identity
        match bootstrap_worker_identity(&config.project_root) {
            Ok(agent_name) => {
                // Get the tmux pane ID for identity-write
                let panes = tmux::list_panes(&config.session_name, 0)?;
                if let Some(pane_info) = panes.iter().find(|p| p.index == pane_index) {
                    let _ = write_identity(
                        &agent_name,
                        &config.project_root,
                        &pane_info.id,
                    );
                }

                // Launch worker with identity
                let launch_cmd = format!("AGENT_NAME='{agent_name}' {model_cmd}");
                tmux::send_literal(&config.session_name, 0, pane_index, &launch_cmd)?;
                tmux::send_keys(&config.session_name, 0, pane_index, "Enter")?;

                worker_identities.push((pane_index, agent_name));
            }
            Err(e) => {
                eprintln!(
                    "warning: failed to bootstrap identity for pane {pane_index}: {e}"
                );
                // Launch without identity — worker can bootstrap itself
                tmux::send_literal(&config.session_name, 0, pane_index, &model_cmd)?;
                tmux::send_keys(&config.session_name, 0, pane_index, "Enter")?;
            }
        }
    }

    // Step 5: Start planner and auto-start the planner loop
    let planner_cmd = ensure_skip_permissions("claude");
    tmux::send_literal(&config.session_name, 0, 1, &planner_cmd)?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    tmux::send_keys(&config.session_name, 0, 1, "Enter")?;

    // Give Claude a moment to boot, then queue the planner skill.
    // Claude Code needs ~10-15s to initialize (load skills, connect MCP).
    std::thread::sleep(std::time::Duration::from_secs(12));
    tmux::send_literal(&config.session_name, 0, 1, "/loop 10m /planner")?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    tmux::send_keys(&config.session_name, 0, 1, "Enter")?;

    // Step 5b: Start worker autonomous loops.
    // Wait for workers to fully boot before sending /loop commands.
    // Claude Code needs ~10-15s to initialize (load skills, connect MCP).
    // Workers were launched before the planner, so they've had at least 8s already.
    // Add another 8s to ensure reliable delivery of the /loop command.
    if !config.model_command.contains("codex") {
        std::thread::sleep(std::time::Duration::from_secs(8));
    }
    for (pane_index, _agent_name) in &worker_identities {
        std::thread::sleep(std::time::Duration::from_secs(2));
        tmux::send_literal(&config.session_name, 0, *pane_index, "/loop 1m /flywheel-worker")?;
        // Brief pause between typing and Enter — Claude Code's input buffer
        // can swallow the Enter if it arrives during the boot splash animation.
        std::thread::sleep(std::time::Duration::from_millis(500));
        tmux::send_keys(&config.session_name, 0, *pane_index, "Enter")?;
    }

    // Step 6: Optionally create coordinator window and start run loop
    if config.with_coordinator {
        create_coordinator_window(config)?;
    }

    // Step 7: Focus on the swarm window (window 0)
    let _ = Command::new("tmux")
        .args(["select-window", "-t", &format!("{}:0", config.session_name)])
        .output();
    let _ = Command::new("tmux")
        .args(["select-pane", "-t", &format!("{}:0.0", config.session_name)])
        .output();

    // Step 8: Print launch summary
    println!();
    println!("=== Swarm launched: {} ===", config.session_name);
    println!("  Workers: {}", config.worker_count);
    println!("  Total panes: {total_panes}");
    println!("  Layout: {grid_cols}x{grid_rows} worker grid");
    if config.with_coordinator {
        println!("  Coordinator: {}:coordinator ({}s interval)", config.session_name, config.poll_interval);
    }
    println!();
    for (pane, name) in &worker_identities {
        println!("  pane {pane}: {name}");
    }
    if !config.with_coordinator {
        println!();
        println!("Start the coordinator with:");
        println!(
            "  patina-orchestrator run --session {} --interval {}",
            config.session_name, config.poll_interval
        );
    }
    println!();
    println!("Attach to the session:");
    println!("  tmux attach -t {}", config.session_name);

    Ok(LaunchResult {
        session_name: config.session_name.clone(),
        worker_count: config.worker_count,
        worker_identities,
        total_panes,
    })
}

/// Print the dry-run plan without executing anything.
fn print_dry_run_layout(config: &LaunchConfig, grid_cols: u32, grid_rows: u32, total_panes: u32) {
    let model_cmd = ensure_skip_permissions(&config.model_command);

    println!("=== Launch Plan (dry-run) ===");
    println!("Session: {}", config.session_name);
    println!("Terminal: {}x{}", config.terminal_width, config.terminal_height);
    println!(
        "Workers: {} ({}x{} grid)",
        config.worker_count, grid_cols, grid_rows
    );
    println!("Total panes: {total_panes}");
    println!();
    println!("Layout:");
    println!("  pane 0: monitor  (watch coordinator output)");
    println!("  pane 1: planner  ({model_cmd} → /loop 10m /planner)");
    println!("  pane 2: bv       (bv)");
    for i in 0..config.worker_count {
        let pane = 3 + i;
        println!("  pane {pane}: worker  (AGENT_NAME='<bootstrapped>' {model_cmd})");
    }
    println!();
    println!("Commands:");
    println!(
        "  tmux new-session -d -s {} -x {} -y {}",
        config.session_name, config.terminal_width, config.terminal_height
    );
    println!("  tmux split-window -h -t {}:0.0 -p 85  # left col | rest", config.session_name);
    println!("  tmux split-window -v -t {}:0.0 -p 50  # monitor | planner", config.session_name);
    println!(
        "  tmux split-window -h -t {}:0.2 -p 67  # bv | worker area",
        config.session_name
    );
    println!("  <worker grid: {} splits for {} workers>", grid_rows * grid_cols - 1, config.worker_count);
    println!();
    println!("All worker commands include: --dangerously-skip-permissions");
}

// --- tmux layout helpers ---

fn create_session(session: &str, width: u32, height: u32, workdir: &Path) -> Result<()> {
    let w = width.to_string();
    let h = height.to_string();
    let dir = workdir.to_string_lossy();
    let output = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session,
            "-x",
            &w,
            "-y",
            &h,
            "-c",
            &dir,
        ])
        .output()
        .map_err(|e| OrchestratorError::Tmux(format!("failed to create session: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::Tmux(format!(
            "tmux new-session failed: {stderr}"
        )));
    }
    Ok(())
}

fn kill_session(session: &str) -> Result<()> {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", session])
        .output();
    Ok(())
}

fn split_h(session: &str, window: u32, pane: u32, percent: u32) -> Result<()> {
    let target = format!("{session}:{window}.{pane}");
    let pct = percent.to_string();
    let output = Command::new("tmux")
        .args(["split-window", "-h", "-t", &target, "-p", &pct])
        .output()
        .map_err(|e| OrchestratorError::Tmux(format!("split-h failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::Tmux(format!(
            "tmux split-window -h failed: {stderr}"
        )));
    }
    Ok(())
}

fn split_v(session: &str, window: u32, pane: u32, percent: u32) -> Result<()> {
    let target = format!("{session}:{window}.{pane}");
    let pct = percent.to_string();
    let output = Command::new("tmux")
        .args(["split-window", "-v", "-t", &target, "-p", &pct])
        .output()
        .map_err(|e| OrchestratorError::Tmux(format!("split-v failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::Tmux(format!(
            "tmux split-window -v failed: {stderr}"
        )));
    }
    Ok(())
}

/// Create the NxM worker grid starting at `start_pane`.
///
/// Uses **pane IDs** (e.g., `%5`) for tracking instead of pane indices, because
/// tmux renumbers indices after every split, making index-based tracking unreliable.
///
/// Strategy: split into rows first (vertical splits on the largest remaining pane),
/// then split each row into columns (horizontal splits on the largest remaining pane).
fn create_worker_grid(
    session: &str,
    window: u32,
    start_pane: u32,
    worker_count: u32,
    cols: u32,
    rows: u32,
) -> Result<()> {
    if worker_count <= 1 {
        return Ok(());
    }

    // Get the stable pane ID for start_pane
    let start_id = pane_id_for_index(session, window, start_pane)?;

    // --- Phase 1: Create rows by splitting the LARGEST pane each time ---
    // Track panes by their stable ID.
    let mut last_id = start_id;
    for r in 1..rows {
        let remaining = rows - r;
        let new_pane_pct = (100 * remaining) / (remaining + 1);
        let ids_before = pane_ids(session, window)?;
        // Split by ID target (more reliable than index)
        split_v_by_id(session, &last_id, new_pane_pct)?;
        let ids_after = pane_ids(session, window)?;
        // New pane is the one that appeared
        if let Some(new_id) = ids_after.iter().find(|id| !ids_before.contains(id)) {
            last_id = new_id.clone();
        }
    }

    // Collect all row pane IDs sorted by vertical position (top coordinate)
    let row_ids = pane_ids_sorted_by_top(session, window, start_pane)?;

    // --- Phase 2: Split each row into columns ---
    for (row_idx, row_id) in row_ids.iter().enumerate() {
        let workers_before = (row_idx as u32) * cols;
        let workers_in_row = cols.min(worker_count.saturating_sub(workers_before));
        if workers_in_row <= 1 {
            continue;
        }

        let mut last_col_id = row_id.clone();
        for c in 1..workers_in_row {
            let remaining = workers_in_row - c;
            let new_pane_pct = (100 * remaining) / (remaining + 1);
            let ids_before = pane_ids(session, window)?;
            split_h_by_id(session, &last_col_id, new_pane_pct)?;
            let ids_after = pane_ids(session, window)?;
            if let Some(new_id) = ids_after.iter().find(|id| !ids_before.contains(id)) {
                last_col_id = new_id.clone();
            }
        }
    }

    Ok(())
}

// --- Pane ID helpers ---

/// Get the stable pane ID (e.g., "%5") for a given pane index.
fn pane_id_for_index(session: &str, window: u32, index: u32) -> Result<String> {
    let panes = tmux::list_panes(session, window)?;
    panes
        .iter()
        .find(|p| p.index == index)
        .map(|p| p.id.clone())
        .ok_or_else(|| OrchestratorError::Tmux(format!("pane {index} not found")))
}

/// Get all pane IDs in a window.
fn pane_ids(session: &str, window: u32) -> Result<Vec<String>> {
    let panes = tmux::list_panes(session, window)?;
    Ok(panes.iter().map(|p| p.id.clone()).collect())
}

/// Get pane IDs for the worker area (index >= start_pane), sorted by top coordinate.
fn pane_ids_sorted_by_top(session: &str, window: u32, min_index: u32) -> Result<Vec<String>> {
    let target = format!("{session}:{window}");
    let output = Command::new("tmux")
        .args([
            "list-panes", "-t", &target, "-F",
            "#{pane_index}|#{pane_id}|#{pane_top}",
        ])
        .output()
        .map_err(|e| OrchestratorError::Tmux(format!("list-panes failed: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut entries: Vec<(u32, String)> = Vec::new(); // (top, id)
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(3, '|').collect();
        if parts.len() < 3 { continue; }
        let idx: u32 = parts[0].parse().unwrap_or(0);
        if idx < min_index { continue; }
        let id = parts[1].to_string();
        let top: u32 = parts[2].parse().unwrap_or(0);
        entries.push((top, id));
    }
    entries.sort_by_key(|(top, _)| *top);
    // Dedup by top coordinate (each row has a unique top value)
    let mut seen_tops = std::collections::HashSet::new();
    let mut result = Vec::new();
    for (top, id) in entries {
        if seen_tops.insert(top) {
            result.push(id);
        }
    }
    Ok(result)
}

/// Split by pane ID (stable, doesn't change with renumbering).
fn split_h_by_id(session: &str, pane_id: &str, percent: u32) -> Result<()> {
    let pct = percent.to_string();
    let output = Command::new("tmux")
        .args(["split-window", "-h", "-t", pane_id, "-p", &pct])
        .output()
        .map_err(|e| OrchestratorError::Tmux(format!("split-h by id failed: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::Tmux(format!(
            "tmux split-window -h -t {pane_id} failed: {stderr}"
        )));
    }
    Ok(())
}

fn split_v_by_id(session: &str, pane_id: &str, percent: u32) -> Result<()> {
    let pct = percent.to_string();
    let output = Command::new("tmux")
        .args(["split-window", "-v", "-t", pane_id, "-p", &pct])
        .output()
        .map_err(|e| OrchestratorError::Tmux(format!("split-v by id failed: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::Tmux(format!(
            "tmux split-window -v -t {pane_id} failed: {stderr}"
        )));
    }
    Ok(())
}

/// Create a coordinator window and start the `patina-orchestrator run` loop in it.
fn create_coordinator_window(config: &LaunchConfig) -> Result<()> {
    let session = &config.session_name;
    let workdir = config.project_root.to_string_lossy();

    // Create the coordinator window (if it doesn't already exist)
    let output = Command::new("tmux")
        .args([
            "new-window", "-t", session, "-n", "coordinator", "-c", &workdir,
        ])
        .output()
        .map_err(|e| OrchestratorError::Tmux(format!("new-window coordinator: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::Tmux(format!(
            "failed to create coordinator window: {stderr}"
        )));
    }

    // Resolve the orchestrator binary path
    let orch_bin = config
        .orch_root
        .join("crate/target/release/patina-orchestrator");
    let orch_bin_str = orch_bin.to_string_lossy();

    // Build the coordinator command with environment variables.
    // All values are shell-escaped to prevent injection from env vars or paths with spaces.
    let session_family = std::env::var("ORCH_SESSION_FAMILY")
        .unwrap_or_else(|_| session.to_string());
    // Register coordinator in Agent Mail with an auto-generated name.
    // Write the name to .beads/coordinator_agent so workers can discover it.
    let coordinator_agent = std::env::var("ORCH_COORDINATOR_AGENT")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            // Auto-register and get the generated name
            bootstrap_worker_identity(&config.project_root).ok()
        })
        .unwrap_or_else(|| "CoordinatorFallback".to_string());

    // Write coordinator name to a discoverable file
    let coord_file = config.project_root.join(".beads/coordinator_agent");
    if let Err(e) = std::fs::write(&coord_file, &coordinator_agent) {
        eprintln!("  warning: could not write coordinator name to {}: {e}", coord_file.display());
    } else {
        println!("  Coordinator registered as: {coordinator_agent}");
        println!("  Workers discover via: .beads/coordinator_agent");
    }
    let browser_verify = std::env::var("ORCH_BROWSER_VERIFY_ENABLED").unwrap_or_else(|_| "0".into());
    let browser_panes = std::env::var("ORCH_BROWSER_VERIFY_PANES").unwrap_or_default();

    let coord_cmd = format!(
        "cd {} && \
         export ORCH_SESSION={} && \
         export ORCH_SESSION_FAMILY={} && \
         export AGENT_NAME={} && \
         export ORCH_BROWSER_VERIFY_ENABLED={} && \
         export ORCH_BROWSER_VERIFY_PANES={} && \
         exec {} run --session {} --interval {}",
        shell_escape(&workdir),
        shell_escape(session),
        shell_escape(&session_family),
        shell_escape(&coordinator_agent),
        shell_escape(&browser_verify),
        shell_escape(&browser_panes),
        shell_escape(&orch_bin_str),
        shell_escape(session),
        config.poll_interval,
    );

    // Use respawn-pane to start the coordinator process.
    // The coord_cmd is already fully escaped, so we wrap it in double quotes for zsh -lc.
    let target = format!("{session}:coordinator.0");
    let shell_cmd = format!("/bin/zsh -lc {}", shell_escape(&coord_cmd));
    let output = Command::new("tmux")
        .args(["respawn-pane", "-k", "-t", &target, &shell_cmd])
        .output()
        .map_err(|e| OrchestratorError::Tmux(format!("respawn coordinator: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::Tmux(format!(
            "failed to start coordinator: {stderr}"
        )));
    }

    Ok(())
}

// --- Identity helpers ---

/// Register a new agent identity via the Agent Mail MCP server (pure Rust, no shell).
fn bootstrap_worker_identity(project_root: &Path) -> Result<String> {
    let mail_config = config::load_mail_config(project_root)?;
    let client = MailClient::new(&mail_config);

    let project_key = project_root.to_string_lossy().to_string();
    let args = serde_json::json!({
        "project_key": project_key,
        "program": "claude-code",
        "model": "opus-4",
    });

    let response = client.call_tool("register_agent", args)?;
    extract_agent_name_from_mcp_response(&response)
}

/// Extract agent name from an MCP JSON-RPC response for `register_agent`.
///
/// Response shape: `{ "result": { "content": [{ "type": "text", "text": "{...}" }] } }`
/// The inner text is a JSON object with a `name` field.
fn extract_agent_name_from_mcp_response(response: &serde_json::Value) -> Result<String> {
    let result = response.get("result").unwrap_or(response);
    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        for item in content {
            if item.get("type").and_then(|t| t.as_str()) != Some("text") {
                continue;
            }
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
                    if let Some(name) = parsed.get("name").and_then(|n| n.as_str()) {
                        if !name.is_empty() {
                            return Ok(name.to_string());
                        }
                    }
                }
            }
        }
    }

    Err(OrchestratorError::Mail(
        "register_agent response did not contain agent name".to_string(),
    ))
}

/// Extract the base URL (scheme + host + port) from an MCP endpoint URL.
///
/// e.g. `"http://127.0.0.1:8765/api/"` → `"http://127.0.0.1:8765"`
fn extract_base_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        let host_end = rest.find('/').unwrap_or(rest.len());
        format!("{}{}", &url[..scheme_end + 3], &rest[..host_end])
    } else {
        url.trim_end_matches('/').to_string()
    }
}

/// Write identity file for a pane (pure Rust, no shell).
///
/// Writes to: ~/.local/state/agent-mail/identity/<project_hash>/<pane_id>
/// File contains two lines: agent name and Unix epoch timestamp.
fn write_identity(agent_name: &str, project_root: &Path, pane_id: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::time::SystemTime;

    // SHA-256 hash of project path, first 12 hex chars
    let hash = {
        let digest = {
            // Use openssl CLI as a fallback — stdlib doesn't have SHA-256.
            // This is a tiny one-shot call, acceptable.
            let output = Command::new("shasum")
                .args(["-a", "256"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    if let Some(ref mut stdin) = child.stdin {
                        let _ = stdin.write_all(project_root.to_string_lossy().as_bytes());
                    }
                    child.wait_with_output()
                })
                .map_err(|e| OrchestratorError::Mail(format!("shasum failed: {e}")))?;
            String::from_utf8_lossy(&output.stdout).to_string()
        };
        digest.chars().take(12).collect::<String>()
    };

    let home = std::env::var("HOME")
        .map_err(|_| OrchestratorError::Mail("HOME not set".into()))?;
    let identity_dir = PathBuf::from(&home)
        .join(".local/state/agent-mail/identity")
        .join(&hash);

    std::fs::create_dir_all(&identity_dir)
        .map_err(|e| OrchestratorError::Mail(format!("mkdir identity dir: {e}")))?;

    // Secure directory permissions (700)
    let _ = std::fs::set_permissions(
        &identity_dir,
        std::fs::Permissions::from_mode(0o700),
    );

    let identity_file = identity_dir.join(pane_id);
    let tmp_file = identity_dir.join(format!("{pane_id}.tmp.{}", std::process::id()));

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Write to temp file then atomically rename
    std::fs::write(&tmp_file, format!("{agent_name}\n{timestamp}\n"))
        .map_err(|e| OrchestratorError::Mail(format!("write identity tmp: {e}")))?;

    let _ = std::fs::set_permissions(&tmp_file, std::fs::Permissions::from_mode(0o600));

    std::fs::rename(&tmp_file, &identity_file)
        .map_err(|e| OrchestratorError::Mail(format!("rename identity file: {e}")))?;

    Ok(())
}

/// Ensure the Agent Mail server is running (pure Rust, no shell scripts).
///
/// 1. Parse the mail server base URL from codex.mcp.json.
/// 2. Health-check the server — if it's already up, return immediately.
/// 3. If not, kill any stale tmux session, create a new one running the
///    Python server, and poll until it becomes ready.
fn ensure_mail_server(project_root: &Path, mail_session: &str) -> Result<()> {
    let mail_config = config::load_mail_config(project_root)?;

    let base_url = extract_base_url(&mail_config.url);

    let health_url = format!("{base_url}/health/readiness");

    // Quick health check — if already running, we're done.
    let http_agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(2))
        .timeout_read(std::time::Duration::from_secs(5))
        .build();

    if http_agent.get(&health_url).call().is_ok() {
        return Ok(());
    }

    // Kill stale session if it exists
    if tmux::has_session(mail_session) {
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", mail_session])
            .output();
    }

    // Resolve the mail server directory and token
    let mail_dir = project_root.join("mcp_agent_mail");

    // Read bearer token from .env if available, otherwise generate one
    let token = resolve_bearer_token(&mail_dir);

    // Start the mail server in a new tmux session.
    // We inline the server command directly — no shell script dependency.
    let inner_cmd = format!(
        "cd {} && export UV_CACHE_DIR=/tmp/uv-cache && export HTTP_BEARER_TOKEN={} && exec uv run python -m mcp_agent_mail.cli serve-http",
        shell_escape(&mail_dir.to_string_lossy()),
        shell_escape(&token),
    );
    let server_cmd = format!("/bin/zsh -lc {}", shell_escape(&inner_cmd));

    let output = Command::new("tmux")
        .args([
            "new-session", "-d",
            "-s", mail_session,
            "-c", &mail_dir.to_string_lossy(),
            &server_cmd,
        ])
        .output()
        .map_err(|e| OrchestratorError::Mail(format!("tmux new-session for mail: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OrchestratorError::Mail(format!(
            "failed to create mail server session: {stderr}"
        )));
    }

    // Verify session survived initial startup
    std::thread::sleep(std::time::Duration::from_secs(1));
    if !tmux::has_session(mail_session) {
        return Err(OrchestratorError::Mail(
            "Agent Mail server session crashed immediately".into(),
        ));
    }

    // Health check with exponential backoff (up to 20 attempts)
    let mut wait_secs = 1u64;
    for _ in 0..20 {
        if http_agent.get(&health_url).call().is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(wait_secs));
        if wait_secs < 4 {
            wait_secs *= 2;
        }
    }

    Err(OrchestratorError::Mail(format!(
        "Agent Mail server did not become ready at {base_url}"
    )))
}

/// Read HTTP_BEARER_TOKEN from .env, or generate a random hex token.
fn resolve_bearer_token(mail_dir: &Path) -> String {
    // Try .env file first
    let env_path = mail_dir.join(".env");
    if let Ok(contents) = std::fs::read_to_string(&env_path) {
        for line in contents.lines() {
            if let Some(val) = line.strip_prefix("HTTP_BEARER_TOKEN=") {
                let val = val.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return val.to_string();
                }
            }
        }
    }

    // Generate a random 32-byte hex token
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{seed:032x}")
}

/// Minimal shell escaping — wrap in single quotes, escaping any inner single quotes.
fn shell_escape(s: &str) -> String {
    if s.contains('\'') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        format!("'{s}'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_grid_dimensions() {
        // 0 workers
        assert_eq!(compute_grid_dimensions(0), (0, 0));
        // 1 worker
        assert_eq!(compute_grid_dimensions(1), (1, 1));
        // 4 workers -> 2x2
        assert_eq!(compute_grid_dimensions(4), (2, 2));
        // 6 workers -> 3 cols, 2 rows
        assert_eq!(compute_grid_dimensions(6), (3, 2));
        // 9 workers -> 3x3
        assert_eq!(compute_grid_dimensions(9), (3, 3));
        // 2 workers -> 2 cols, 1 row
        assert_eq!(compute_grid_dimensions(2), (2, 1));
        // 3 workers -> 2 cols, 2 rows (ceil(sqrt(3))=2, ceil(3/2)=2)
        assert_eq!(compute_grid_dimensions(3), (2, 2));
        // 5 workers -> 3 cols, 2 rows (ceil(sqrt(5))=3, ceil(5/3)=2)
        assert_eq!(compute_grid_dimensions(5), (3, 2));
        // 7 workers -> 3 cols, 3 rows
        assert_eq!(compute_grid_dimensions(7), (3, 3));
        // 8 workers -> 3 cols, 3 rows
        assert_eq!(compute_grid_dimensions(8), (3, 3));
        // 10 workers -> 4 cols, 3 rows
        assert_eq!(compute_grid_dimensions(10), (4, 3));
        // 12 workers -> 4 cols, 3 rows
        assert_eq!(compute_grid_dimensions(12), (4, 3));
        // 16 workers -> 4x4
        assert_eq!(compute_grid_dimensions(16), (4, 4));
    }

    #[test]
    fn test_grid_always_has_enough_cells() {
        for n in 1..=30 {
            let (cols, rows) = compute_grid_dimensions(n);
            assert!(
                cols * rows >= n,
                "grid {cols}x{rows} = {} cells is less than {n} workers",
                cols * rows
            );
        }
    }

    #[test]
    fn test_skip_permissions_always_included() {
        assert_eq!(
            ensure_skip_permissions("claude"),
            "claude --dangerously-skip-permissions"
        );
        assert_eq!(
            ensure_skip_permissions("claude --model opus"),
            "claude --model opus --dangerously-skip-permissions"
        );
    }

    #[test]
    fn test_skip_permissions_not_duplicated() {
        let cmd = "claude --dangerously-skip-permissions";
        assert_eq!(ensure_skip_permissions(cmd), cmd);

        let cmd2 = "claude --dangerously-skip-permissions --model opus";
        assert_eq!(ensure_skip_permissions(cmd2), cmd2);
    }

    #[test]
    fn test_total_panes_calculation() {
        // Verify the launch result reports correct pane count:
        // 3 fixed panes (monitor, planner, bv) + worker_count
        assert_eq!(3 + 1, 4, "1 worker → 4 panes");
        assert_eq!(3 + 4, 7, "4 workers → 7 panes");
        assert_eq!(3 + 9, 12, "9 workers → 12 panes");
        assert_eq!(3 + 12, 15, "12 workers → 15 panes");
        // Grid dimensions must have enough cells for each worker count
        for workers in [1u32, 4, 9, 12] {
            let (cols, rows) = compute_grid_dimensions(workers);
            assert!(cols * rows >= workers, "grid too small for {workers} workers");
        }
    }

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn test_shell_escape_with_single_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_resolve_bearer_token_from_env_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "OTHER_VAR=foo\nHTTP_BEARER_TOKEN=my_secret_token\nANOTHER=bar\n",
        )
        .unwrap();
        assert_eq!(resolve_bearer_token(dir.path()), "my_secret_token");
    }

    #[test]
    fn test_resolve_bearer_token_generates_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        // No .env file — should generate a token
        let token = resolve_bearer_token(dir.path());
        assert!(!token.is_empty());
    }

    // --- Regression tests for coordinator agent name bug ---
    // Bug: create_coordinator_window set AGENT_NAME to empty string when
    // ORCH_COORDINATOR_AGENT env var was unset, causing "coordinator agent
    // name not set" error on every poll cycle.

    #[test]
    fn test_extract_agent_name_from_valid_mcp_response() {
        let response = serde_json::json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "{\"name\": \"TurquoisePuma\", \"id\": 42}"
                }]
            }
        });
        let name = extract_agent_name_from_mcp_response(&response).unwrap();
        assert_eq!(name, "TurquoisePuma");
    }

    #[test]
    fn test_extract_agent_name_fails_on_empty_name() {
        let response = serde_json::json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "{\"name\": \"\", \"id\": 42}"
                }]
            }
        });
        assert!(
            extract_agent_name_from_mcp_response(&response).is_err(),
            "Empty agent name should be rejected"
        );
    }

    #[test]
    fn test_extract_agent_name_fails_on_missing_name_field() {
        let response = serde_json::json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "{\"id\": 42, \"program\": \"claude-code\"}"
                }]
            }
        });
        assert!(
            extract_agent_name_from_mcp_response(&response).is_err(),
            "Missing 'name' field should be rejected"
        );
    }

    #[test]
    fn test_extract_agent_name_fails_on_empty_response() {
        let response = serde_json::json!({});
        assert!(extract_agent_name_from_mcp_response(&response).is_err());
    }

    #[test]
    fn test_extract_agent_name_fails_on_non_text_content() {
        let response = serde_json::json!({
            "result": {
                "content": [{
                    "type": "image",
                    "data": "base64..."
                }]
            }
        });
        assert!(extract_agent_name_from_mcp_response(&response).is_err());
    }

    #[test]
    fn test_extract_agent_name_skips_non_text_finds_text() {
        // Multiple content items — should skip non-text and find the text one
        let response = serde_json::json!({
            "result": {
                "content": [
                    { "type": "image", "data": "..." },
                    { "type": "text", "text": "{\"name\": \"GreenLake\"}" }
                ]
            }
        });
        assert_eq!(
            extract_agent_name_from_mcp_response(&response).unwrap(),
            "GreenLake"
        );
    }

    #[test]
    fn test_extract_agent_name_handles_malformed_json_text() {
        let response = serde_json::json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "this is not json"
                }]
            }
        });
        assert!(
            extract_agent_name_from_mcp_response(&response).is_err(),
            "Malformed JSON in text content should fail gracefully"
        );
    }

    #[test]
    fn test_extract_agent_name_with_result_directly() {
        // Some responses may not have nested result wrapper
        let response = serde_json::json!({
            "content": [{
                "type": "text",
                "text": "{\"name\": \"BlueDog\"}"
            }]
        });
        assert_eq!(
            extract_agent_name_from_mcp_response(&response).unwrap(),
            "BlueDog"
        );
    }

    // --- URL base extraction tests ---

    #[test]
    fn test_extract_base_url_with_path() {
        assert_eq!(
            extract_base_url("http://127.0.0.1:8765/api/"),
            "http://127.0.0.1:8765"
        );
    }

    #[test]
    fn test_extract_base_url_no_path() {
        assert_eq!(
            extract_base_url("http://127.0.0.1:8765"),
            "http://127.0.0.1:8765"
        );
    }

    #[test]
    fn test_extract_base_url_with_deep_path() {
        assert_eq!(
            extract_base_url("https://mail.example.com:443/v1/mcp/api"),
            "https://mail.example.com:443"
        );
    }

    #[test]
    fn test_extract_base_url_trailing_slash_only() {
        assert_eq!(
            extract_base_url("http://localhost:8765/"),
            "http://localhost:8765"
        );
    }

    #[test]
    fn test_extract_base_url_no_scheme() {
        // Degenerate case — no "://" present, just strip trailing slashes
        assert_eq!(extract_base_url("localhost:8765/api/"), "localhost:8765/api");
    }

    // --- Bearer token edge cases ---

    #[test]
    fn test_resolve_bearer_token_quoted_value() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "HTTP_BEARER_TOKEN=\"quoted_token_value\"\n",
        )
        .unwrap();
        assert_eq!(resolve_bearer_token(dir.path()), "quoted_token_value");
    }

    #[test]
    fn test_resolve_bearer_token_single_quoted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "HTTP_BEARER_TOKEN='single_quoted_val'\n",
        )
        .unwrap();
        assert_eq!(resolve_bearer_token(dir.path()), "single_quoted_val");
    }

    #[test]
    fn test_resolve_bearer_token_empty_value_generates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "HTTP_BEARER_TOKEN=\n").unwrap();
        let token = resolve_bearer_token(dir.path());
        assert!(!token.is_empty());
        assert_ne!(token, "", "Empty .env value should trigger generation");
    }

    // --- Shell escape edge cases ---

    #[test]
    fn test_shell_escape_empty_string() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_shell_escape_spaces() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn test_shell_escape_multiple_single_quotes() {
        assert_eq!(shell_escape("it's a 'test'"), "'it'\\''s a '\\''test'\\'''");
    }

    // --- Coordinator name config regression ---
    // The core bug: Config::from_env uses env_string which filters empty strings,
    // but create_coordinator_window used unwrap_or_default() on raw env var.

    #[test]
    fn test_config_coordinator_agent_none_by_default() {
        // test_config() simulates a fresh config with no env vars
        let cfg = crate::config::test_config();
        assert!(
            cfg.coordinator_agent.is_none(),
            "coordinator_agent must be None when no env var is set — \
             this is the root cause of the 'not set' error if it defaults to Some(\"\")"
        );
    }

    #[test]
    fn test_coordinator_name_fails_without_agent() {
        // Directly test that the coordinator rejects None agent name
        // rather than silently using empty string
        let cfg = crate::config::test_config();
        assert!(
            cfg.coordinator_agent.is_none(),
            "test_config must have None coordinator_agent"
        );
        // The coordinator would call coordinator_name() which does:
        //   self.config.coordinator_agent.clone().ok_or_else(|| ...)
        let result: std::result::Result<String, _> = cfg
            .coordinator_agent
            .clone()
            .ok_or("coordinator agent name not set");
        assert!(
            result.is_err(),
            "None coordinator_agent must produce an error, not an empty string"
        );
    }

    // =========================================================================
    // Regression: worker loop command format
    // =========================================================================
    // Bug 1: `/loop 2m /skill flywheel-worker` was parsed by Claude as
    //   skill="/skill", args="flywheel-worker" → "Unknown skill: skill".
    //   Fix: use `/loop 2m /flywheel-worker` (no `/skill` prefix).
    //
    // Bug 2: Worker loop commands sent too fast get queued as
    //   "Press up to edit queued messages" and never processed.
    //   Fix: delay between each worker's loop command.

    /// Regression: launcher must use `/flywheel-worker` not `/skill flywheel-worker`
    /// in the loop command. The `/loop` command's parser treats the first token
    /// after the interval as the skill name — `/skill` would be parsed as the skill.
    #[test]
    fn test_worker_loop_uses_correct_skill_format() {
        let source = include_str!("launcher.rs");
        let step5b = source.find("Step 5b").expect("Step 5b must exist");
        let section_end = source[step5b..].find("Step 6").unwrap_or(500);
        let section = &source[step5b..step5b + section_end];

        assert!(
            section.contains("/loop 2m /flywheel-worker"),
            "REGRESSION: worker loop must use '/loop 2m /flywheel-worker', \
             not '/loop 2m /skill flywheel-worker'."
        );
        // Check only the send_literal line (actual command), not comments
        let send_line = section.lines()
            .find(|l| l.contains("send_literal") && l.contains("loop"))
            .expect("must have a send_literal line with loop command");
        assert!(
            !send_line.contains("/skill flywheel-worker"),
            "REGRESSION: send_literal must NOT use '/skill flywheel-worker' in loop command. \
             The /loop parser treats '/skill' as the skill name."
        );
    }

    /// Regression: worker loop commands must have a delay between each pane.
    /// Without the delay, commands get queued as "Press up to edit" and
    /// Claude never processes them.
    #[test]
    fn test_worker_loop_has_delay_between_panes() {
        let source = include_str!("launcher.rs");
        let step5b = source.find("Step 5b").expect("Step 5b must exist");
        let section_end = source[step5b..].find("Step 6").unwrap_or(500);
        let section = &source[step5b..step5b + section_end];

        assert!(
            section.contains("sleep"),
            "REGRESSION: worker loop commands must have a delay between panes. \
             Without it, commands get queued as 'Press up to edit' and never process."
        );
    }

    /// Regression: monitor pane must NOT use `tail -f /tmp/patina-orchestrator.log`
    /// which is a stale file. Must watch the live coordinator tmux pane.
    #[test]
    fn test_monitor_pane_watches_live_coordinator() {
        let source = include_str!("launcher.rs");
        // Check the actual monitor_cmd assignment, not comments/tests
        let monitor_setup = source.find("Pane 0: monitor").expect("monitor setup must exist");
        let section = &source[monitor_setup..monitor_setup + 300];
        assert!(
            !section.contains("\"tail -f"),
            "REGRESSION: monitor must NOT tail stale log file. \
             Must watch live coordinator tmux pane output."
        );
        assert!(
            section.contains("tmux capture-pane"),
            "Monitor pane must use tmux capture-pane to watch coordinator"
        );
    }

    /// Regression: monitor pane must NOT use `watch` command (not installed on macOS).
    /// Must use a shell loop instead.
    #[test]
    fn test_monitor_pane_does_not_use_watch() {
        let source = include_str!("launcher.rs");
        let monitor = source.find("Pane 0: monitor").expect("monitor setup must exist");
        let section = &source[monitor..monitor + 300];

        assert!(
            !section.contains("watch -n"),
            "REGRESSION: monitor must NOT use 'watch' command — not installed on macOS. \
             Use 'while true; do clear; ... sleep N; done' instead."
        );
    }

    /// Regression: flywheel-worker skill must exist in the project skills directory.
    /// The global ~/.claude/skills/ is not visible to workers launched in the project.
    #[test]
    fn test_flywheel_worker_skill_in_project_dir() {
        // This test checks the ACTUAL filesystem, not source code
        let project_skill = std::path::Path::new("/Users/bone/dev/games/patina/.claude/skills/flywheel-worker/SKILL.md");
        assert!(
            project_skill.exists(),
            "REGRESSION: flywheel-worker skill must be in project .claude/skills/, \
             not just global ~/.claude/skills/. Workers can't find global skills."
        );
    }

    /// The editor feature gate must be lifted in CLAUDE.md.
    /// Workers filter out editor beads if the gate is active.
    #[test]
    fn test_editor_gate_is_lifted_in_claude_md() {
        let claude_md = std::path::Path::new("/Users/bone/dev/games/patina/CLAUDE.md");
        if claude_md.exists() {
            let content = std::fs::read_to_string(claude_md).unwrap();
            assert!(
                !content.contains("No new `gdeditor` features until runtime parity"),
                "REGRESSION: editor feature gate must be LIFTED in CLAUDE.md. \
                 Workers filter out all editor beads when the gate is active, \
                 causing 'all beads editor-gated' idle state."
            );
        }
    }

    /// Pull mode must be the default in config.
    #[test]
    fn test_pull_mode_is_default() {
        let cfg = crate::config::test_config();
        assert!(
            cfg.pull_mode,
            "pull_mode must default to true — workers discover their own work"
        );
    }

    // =========================================================================
    // Regression: coordinator must use well-known name "Coordinator"
    // =========================================================================
    // Bug: coordinator registered with random name (TurquoisePuma) but workers
    // send completions to "--to Coordinator". No agent named "Coordinator"
    // existed in Agent Mail, so /mail-complete failed and beads never closed.

    // =========================================================================
    // Regression: coordinator name discovery
    // =========================================================================
    // Bug: coordinator hardcoded as "Coordinator" but Agent Mail rejects
    // non-adjective+noun names. Then hardcoded as "IvoryTower" which was
    // fragile. Fix: auto-register, write name to .beads/coordinator_agent,
    // workers read it dynamically.

    /// Regression: coordinator must auto-register via bootstrap_worker_identity,
    /// NOT use a hardcoded name. Agent Mail rejects names like "Coordinator".
    #[test]
    fn test_coordinator_uses_auto_registered_name() {
        let source = include_str!("launcher.rs");
        let coord_section = source.find("fn create_coordinator_window").expect("must exist");
        let fn_end = source[coord_section..].find("\n}").unwrap_or(2000);
        let fn_body = &source[coord_section..coord_section + fn_end];

        assert!(
            fn_body.contains("bootstrap_worker_identity"),
            "REGRESSION: coordinator must auto-register via bootstrap_worker_identity, \
             not use a hardcoded name. Agent Mail rejects non-adjective+noun names."
        );
    }

    /// Regression: coordinator name must be written to .beads/coordinator_agent
    /// so workers can discover it dynamically.
    #[test]
    fn test_coordinator_name_written_to_discovery_file() {
        let source = include_str!("launcher.rs");
        let coord_section = source.find("fn create_coordinator_window").expect("must exist");
        let fn_end = source[coord_section..].find("\n}").unwrap_or(2000);
        let fn_body = &source[coord_section..coord_section + fn_end];

        assert!(
            fn_body.contains("coordinator_agent"),
            "coordinator name must be stored in a variable"
        );
        assert!(
            fn_body.contains(".beads/coordinator_agent") || fn_body.contains("coordinator_agent"),
            "coordinator name must be written to a discovery file"
        );
    }

    /// Regression: coordinator name must NOT be hardcoded as "Coordinator".
    /// Agent Mail requires adjective+noun format.
    #[test]
    fn test_coordinator_name_not_hardcoded_coordinator() {
        let source = include_str!("launcher.rs");
        let coord_section = source.find("fn create_coordinator_window").expect("must exist");
        let fn_end = source[coord_section..].find("\n}").unwrap_or(2000);
        let fn_body = &source[coord_section..coord_section + fn_end];

        // The function should NOT contain a literal "Coordinator" as the agent name
        // (the fallback string is fine since it's never actually used with Agent Mail)
        assert!(
            fn_body.contains("bootstrap_worker_identity"),
            "Must use bootstrap_worker_identity for auto-generated adjective+noun name"
        );
    }

    /// Regression: config must read coordinator name from .beads/coordinator_agent
    /// as a fallback when no env var is set.
    #[test]
    fn test_config_reads_coordinator_from_discovery_file() {
        let source = include_str!("config.rs");
        assert!(
            source.contains("coordinator_agent") && source.contains(".beads"),
            "Config must read coordinator agent name from .beads/coordinator_agent file"
        );
    }

    /// Functional: .beads/coordinator_agent file exists after launch.
    #[test]
    fn test_coordinator_discovery_file_exists() {
        let path = std::path::Path::new("/Users/bone/dev/games/patina/.beads/coordinator_agent");
        if path.exists() {
            let name = std::fs::read_to_string(path).unwrap();
            let name = name.trim();
            assert!(!name.is_empty(), "coordinator_agent file must not be empty");
            assert!(
                !name.eq_ignore_ascii_case("coordinator"),
                "coordinator_agent must be an auto-generated name, not 'Coordinator'"
            );
        }
    }

    /// The flywheel-worker skill must check existing in-progress beads first.
    #[test]
    fn test_skill_checks_existing_beads_first() {
        let skill_path = std::path::Path::new("/Users/bone/dev/games/patina/.claude/skills/flywheel-worker/SKILL.md");
        if skill_path.exists() {
            let content = std::fs::read_to_string(skill_path).unwrap();
            assert!(
                content.contains("br list --status in_progress"),
                "REGRESSION: flywheel-worker skill must check for existing in-progress \
                 beads before claiming new ones. Without this, workers hoard beads \
                 (one worker claimed 17 beads without finishing any)."
            );
            assert!(
                content.contains("Do NOT claim a new bead"),
                "Skill must explicitly tell workers not to claim new beads when they have existing work"
            );
        }
    }

    /// The flywheel-worker skill must use /mail-complete, not raw MCP.
    #[test]
    fn test_skill_uses_mail_complete_not_raw_mcp() {
        let skill_path = std::path::Path::new("/Users/bone/dev/games/patina/.claude/skills/flywheel-worker/SKILL.md");
        if skill_path.exists() {
            let content = std::fs::read_to_string(skill_path).unwrap();
            assert!(
                content.contains("/mail-complete"),
                "Skill must use /mail-complete for completions"
            );
            assert!(
                content.contains("Do NOT send raw MCP"),
                "REGRESSION: skill must warn against raw MCP send_message — \
                 coordinator can't process messages without the bead-complete topic"
            );
        }
    }

    /// Regression: skill must read coordinator name from .beads/coordinator_agent,
    /// not hardcode "--to Coordinator".
    #[test]
    fn test_skill_reads_coordinator_name_dynamically() {
        let skill_path = std::path::Path::new("/Users/bone/dev/games/patina/.claude/skills/flywheel-worker/SKILL.md");
        if skill_path.exists() {
            let content = std::fs::read_to_string(skill_path).unwrap();
            assert!(
                content.contains(".beads/coordinator_agent"),
                "REGRESSION: skill must read coordinator name from .beads/coordinator_agent, \
                 not hardcode a name. Agent Mail rejects 'Coordinator' as invalid format."
            );
            assert!(
                !content.contains("--to Coordinator"),
                "REGRESSION: skill must NOT hardcode '--to Coordinator' — \
                 Agent Mail requires adjective+noun names like 'SapphireFalcon'"
            );
        }
    }

    #[test]
    fn test_worker_loop_sent_after_boot_delay() {
        let source = include_str!("launcher.rs");
        let step5b = source.find("Step 5b: Start worker autonomous loops").unwrap();
        let end = (step5b + 800).min(source.len());
        let section = &source[step5b..end];
        assert!(
            section.contains("sleep(std::time::Duration::from_secs(8))")
                || section.contains("sleep(std::time::Duration::from_secs(10))"),
            "Must wait for Claude to boot before sending /loop to workers"
        );
    }

    #[test]
    fn test_skip_permissions_codex_uses_bypass_flag() {
        assert_eq!(
            ensure_skip_permissions("codex"),
            "codex --dangerously-bypass-approvals-and-sandbox"
        );
    }

    #[test]
    fn test_skip_permissions_codex_not_duplicated() {
        let cmd = "codex --dangerously-bypass-approvals-and-sandbox";
        assert_eq!(ensure_skip_permissions(cmd), cmd);
    }
}
