mod br;
mod config;
mod coordinator;
mod db;
mod error;
mod gate_map;
mod launcher;
mod mail;
mod message;
mod planner;
mod prd_parser;
mod project_config;
mod tmux;
mod verifier;
mod worker;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;

fn print_usage() {
    eprintln!(
        "Usage: patina-orchestrator <command> [options]\n\
         \n\
         Commands:\n\
         \x20 launch         Create tmux swarm session with workers\n\
         \x20 run            Run the coordinator loop (replaces run_loop.sh)\n\
         \x20 poll           Run a single poll cycle\n\
         \x20 assign         Run a single assignment cycle\n\
         \x20 plan           Analyze parity, gates, queue; emit JSON report\n\
         \x20 health         Print swarm health metrics\n\
         \x20 worker-state   Print per-pane worker state as JSON\n\
         \x20 version        Print version\n\
         \n\
         Options:\n\
         \x20 --session NAME     tmux session name\n\
         \x20 --window N         tmux window index (default: 0)\n\
         \x20 --interval N       poll interval in seconds (default: 8)\n\
         \x20 --workers N        number of worker panes (launch only, default: 9)\n\
         \x20 --model CMD        worker model command (launch only, alias for --worker-model)\n\
         \x20 --worker-model CMD worker model command (launch only, default: claude)\n\
         \x20 --planner-model CMD planner model command (launch only, default: worker model)\n\
         \x20 --force            overwrite existing session (launch only)\n\
         \x20 --with-coordinator start coordinator run loop (launch only)\n\
         \x20 --dry-run          print actions without executing\n\
         \x20 --apply            (plan only) create beads for each recommendation\n\
         \x20 --heal             (plan only) backfill acceptance_criteria on broken legacy beads\n\
         \x20 --project-root DIR project root (default: auto-detect)"
    );
}

fn detect_project_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

struct CliArgs {
    command: String,
    session: Option<String>,
    window: Option<u32>,
    interval: Option<u64>,
    workers: Option<u32>,
    worker_model: Option<String>,
    planner_model: Option<String>,
    force: bool,
    dry_run: bool,
    apply: bool,
    heal: bool,
    project_root: Option<PathBuf>,
    with_coordinator: bool,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let command = args[1].clone();
    let mut session = None;
    let mut window = None;
    let mut interval = None;
    let mut workers = None;
    let mut worker_model = None;
    let mut planner_model = None;
    let mut force = false;
    let mut dry_run = false;
    let mut apply = false;
    let mut heal = false;
    let mut project_root = None;
    let mut with_coordinator = false;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--session" => {
                i += 1;
                session = args.get(i).cloned();
            }
            "--window" => {
                i += 1;
                window = args.get(i).and_then(|v| v.parse().ok());
            }
            "--interval" => {
                i += 1;
                interval = args.get(i).and_then(|v| v.parse().ok());
            }
            "--workers" => {
                i += 1;
                workers = args.get(i).and_then(|v| v.parse().ok());
            }
            "--model" | "--worker-model" => {
                i += 1;
                worker_model = args.get(i).cloned();
            }
            "--planner-model" => {
                i += 1;
                planner_model = args.get(i).cloned();
            }
            "--force" => {
                force = true;
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--apply" => {
                apply = true;
            }
            "--heal" => {
                heal = true;
            }
            "--with-coordinator" => {
                with_coordinator = true;
            }
            "--project-root" => {
                i += 1;
                project_root = args.get(i).map(PathBuf::from);
            }
            other => {
                eprintln!("unknown option: {other}");
                print_usage();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    CliArgs {
        command,
        session,
        window,
        interval,
        workers,
        worker_model,
        planner_model,
        force,
        dry_run,
        apply,
        heal,
        project_root,
        with_coordinator,
    }
}

fn resolve_root(cli: &CliArgs) -> PathBuf {
    cli.project_root
        .clone()
        .or_else(|| std::env::var("PROJECT_ROOT").ok().map(PathBuf::from))
        .or_else(detect_project_root)
        .unwrap_or_else(|| {
            eprintln!("error: could not detect project root (no .git found)");
            std::process::exit(1);
        })
}

fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    let cli = parse_args();
    let root = resolve_root(&cli);

    // Set env overrides from CLI flags (safe: done before any threads are spawned)
    if let Some(s) = &cli.session {
        // SAFETY: called before any threads are spawned in main()
        unsafe { std::env::set_var("ORCH_SESSION", s) };
    }
    if let Some(iv) = cli.interval {
        // SAFETY: called before any threads are spawned in main()
        unsafe { std::env::set_var("ORCH_INTERVAL_SECONDS", iv.to_string()) };
    }

    match cli.command.as_str() {
        "version" => println!("patina-orchestrator {}", env!("CARGO_PKG_VERSION")),
        "launch" => cmd_launch(&root, &cli),
        "health" => cmd_health(&root, &cli),
        "worker-state" => cmd_worker_state(&root, &cli),
        "plan" => cmd_plan(&root, &cli),
        "poll" => cmd_poll(&root, cli.dry_run),
        "assign" => cmd_assign(&root, &cli),
        "run" => cmd_run(&root, &cli),
        _ => {
            eprintln!("unknown command: {}", cli.command);
            print_usage();
            std::process::exit(1);
        }
    }
}

// --- Subcommand implementations ---

fn cmd_launch(project_root: &Path, cli: &CliArgs) {
    let session = cli.session.as_deref().unwrap_or_else(|| {
        eprintln!("error: --session required for launch");
        std::process::exit(1);
    });

    let orch_root = project_root.join("apps/orchestrator");

    let worker_model = cli
        .worker_model
        .clone()
        .unwrap_or_else(|| "claude".to_string());
    let planner_model = cli
        .planner_model
        .clone()
        .unwrap_or_else(|| worker_model.clone());

    let config = launcher::LaunchConfig {
        session_name: session.to_string(),
        worker_count: cli.workers.unwrap_or(9),
        worker_model_command: worker_model,
        planner_model_command: planner_model,
        project_root: project_root.to_path_buf(),
        orch_root,
        dry_run: cli.dry_run,
        force: cli.force,
        terminal_width: 200,
        terminal_height: 50,
        with_coordinator: cli.with_coordinator,
        poll_interval: cli.interval.unwrap_or(8),
    };

    match launcher::launch(&config) {
        Ok(_result) => {}
        Err(e) => {
            eprintln!("launch error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_plan(project_root: &Path, cli: &CliArgs) {
    let report = planner::analyze(project_root).unwrap_or_else(|e| {
        eprintln!("plan error: {e}");
        std::process::exit(1);
    });

    if cli.dry_run {
        eprintln!("=== Planner Report (dry-run) ===");
        eprintln!(
            "Parity: {:.1}% ({}/{})",
            report.parity.overall, report.parity.matched, report.parity.total
        );
        eprintln!(
            "Gates: {}/{} passing, {} failing",
            report.gates.passing.len(),
            report.gates.total,
            report.gates.failing.len()
        );
        for f in &report.gates.failing {
            eprintln!("  FAIL: {f}");
        }
        eprintln!(
            "Queue: open={}, in_progress={}, closed={}, ready={}",
            report.queue.open,
            report.queue.in_progress,
            report.queue.closed,
            report.queue.ready_unassigned
        );
        eprintln!("Recommendations: {}", report.recommendations.len());
        for r in &report.recommendations {
            let acc = if r.acceptance_command.trim().is_empty() {
                "⚠ MISSING".to_string()
            } else {
                r.acceptance_command.clone()
            };
            let deps = if r.depends_on.is_empty() {
                String::new()
            } else {
                format!(" depends_on={}", r.depends_on.join(","))
            };
            eprintln!(
                "  P{}: {} (key={}){}\n        acceptance: {}",
                r.priority, r.title, r.gate_key, deps, acc
            );
        }

        // C4: lint findings — broken seeds an operator should fix before --apply.
        let findings = planner::audit_report(project_root, &report);
        if findings.is_empty() {
            eprintln!("Lint: OK (no missing acceptance, no dangling dependencies)");
        } else {
            eprintln!("Lint: {} finding(s)", findings.len());
            for f in &findings {
                eprintln!("  {f}");
            }
        }

        eprintln!("Phase: {:?}", report.phase);
        eprintln!();
    }

    if cli.heal && !cli.dry_run {
        heal_broken_beads(project_root);
    }

    if cli.apply && !cli.dry_run {
        apply_recommendations(&report);
    }

    // Always print JSON to stdout for the skill to parse
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

/// Backfill `acceptance_criteria` on legacy beads created before `plan --apply`
/// populated the structured field. Scans open + in_progress beads, looks up
/// their `[planner-key: ...]` marker against the phase deliverable templates,
/// and runs `br update --acceptance-criteria` for matches.
///
/// Workers without concrete acceptance criteria invent their own scope or go
/// idle (root-cause incident pat-03sbm, May 2026). This unblocks them.
fn heal_broken_beads(project_root: &Path) {
    let acceptance_map = planner::build_planner_key_acceptance_map(project_root);
    eprintln!(
        "[plan --heal] acceptance_map has {} entries",
        acceptance_map.len()
    );
    if acceptance_map.is_empty() {
        eprintln!("[plan --heal] no planner-key templates found — nothing to heal against");
        return;
    }

    let list_args = vec![
        "list",
        "--status",
        "open",
        "--status",
        "in_progress",
        "--json",
    ];
    let list_raw = match br::run_br_public(&list_args) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("[plan --heal] FAIL list: {e}");
            return;
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&list_raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[plan --heal] FAIL parse br list JSON: {e}");
            return;
        }
    };
    let issues = match parsed.get("issues").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => {
            eprintln!(
                "[plan --heal] br list JSON missing 'issues' array (top-level type: {})",
                match &parsed {
                    serde_json::Value::Array(_) => "array",
                    serde_json::Value::Object(_) => "object",
                    _ => "other",
                }
            );
            return;
        }
    };
    eprintln!("[plan --heal] scanning {} issues", issues.len());

    let key_re = Regex::new(r"\[planner-key:\s*([^\]]+)\]").expect("static regex");

    let mut healed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for issue in issues {
        let has_acceptance = match issue.get("acceptance_criteria") {
            Some(v) if !v.is_null() => true,
            _ => false,
        };
        if has_acceptance {
            continue;
        }
        let id = match issue.get("id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let title = issue.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let description = issue
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let key = match key_re.captures(description).and_then(|c| c.get(1)) {
            Some(m) => m.as_str().trim().to_string(),
            None => {
                eprintln!("[plan --heal] SKIP {id} '{title}': no [planner-key:] marker");
                skipped += 1;
                continue;
            }
        };

        let acceptance = match acceptance_map.get(&key) {
            Some(a) => a,
            None => {
                eprintln!("[plan --heal] SKIP {id} '{title}': planner-key '{key}' has no template");
                skipped += 1;
                continue;
            }
        };

        let update_args = vec!["update", id, "--acceptance-criteria", acceptance.as_str()];
        if let Err(e) = br::run_br_public(&update_args) {
            eprintln!("[plan --heal] FAIL update {id} '{title}': {e}");
            failed += 1;
            continue;
        }
        eprintln!("[plan --heal] HEALED {id} (key={key}): {title}");
        healed += 1;
    }

    eprintln!("[plan --heal] summary: healed={healed} skipped={skipped} failed={failed}");
}

/// Create one bead per recommendation by shelling out to `br create` + `br update
/// --acceptance-criteria`. The two-step pattern is required because `br create`
/// has no `--acceptance-criteria` flag — without the follow-up update, the
/// structured `acceptance_criteria` field stays null and workers invent their
/// own scope (root-cause incident pat-03sbm, May 2026).
///
/// Skips recommendations whose `acceptance_command` is empty rather than
/// creating a broken bead. Logs every action to stderr.
fn apply_recommendations(report: &planner::PlanReport) {
    let mut created = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for rec in &report.recommendations {
        let acceptance = rec.acceptance_command.trim();
        if acceptance.is_empty() {
            eprintln!(
                "[plan --apply] SKIP '{}' — empty acceptance_command (gate_key={})",
                rec.title, rec.gate_key
            );
            skipped += 1;
            continue;
        }

        let labels = rec.labels.join(",");
        let priority = rec.priority.to_string();
        let description = format!(
            "IMPLEMENT the feature: {}\n\nAcceptance: {}\n\n[planner-key: {}]",
            rec.title, acceptance, rec.gate_key
        );

        let create_args = vec![
            "create",
            "--title",
            rec.title.as_str(),
            "--type",
            "task",
            "--priority",
            priority.as_str(),
            "--labels",
            labels.as_str(),
            "--description",
            description.as_str(),
            "--silent",
        ];

        let id_raw = match br::run_br_public(&create_args) {
            Ok(out) => out,
            Err(e) => {
                eprintln!("[plan --apply] FAIL create '{}': {e}", rec.title);
                failed += 1;
                continue;
            }
        };
        let id = id_raw.trim().to_string();
        if id.is_empty() {
            eprintln!(
                "[plan --apply] FAIL create '{}': br returned no ID",
                rec.title
            );
            failed += 1;
            continue;
        }

        // A2: `br create` has no --acceptance-criteria flag, so the field is set
        // in a follow-up `br update`. If that update is lost (crash, transient br
        // error) the bead stays OPEN with null acceptance_criteria and a worker
        // picks it up and invents scope (pat-03sbm). Retry the update; if it
        // still fails, DEFER the bead so it can't be claimed as ready work, and
        // log loudly. A deferred-but-scopeless bead is recoverable; an
        // open-but-scopeless one is the actual incident.
        let set_ok = set_acceptance_with_retry(
            ACCEPTANCE_UPDATE_ATTEMPTS,
            || {
                br::run_br_public(&["update", id.as_str(), "--acceptance-criteria", acceptance])
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
            |attempt, err| {
                eprintln!(
                    "[plan --apply] acceptance update attempt {attempt}/{ACCEPTANCE_UPDATE_ATTEMPTS} for {id} ('{}') failed: {err}",
                    rec.title
                );
            },
        );

        if !set_ok {
            // Best-effort: park the bead so no worker claims a scopeless task.
            match br::run_br_public(&["update", id.as_str(), "--status", "deferred"]) {
                Ok(_) => eprintln!(
                    "[plan --apply] DEFERRED {id} ('{}') — acceptance_criteria could not be set; parked out of the ready queue for manual repair",
                    rec.title
                ),
                Err(e) => eprintln!(
                    "[plan --apply] CRITICAL {id} ('{}') is OPEN with null acceptance_criteria and could not be deferred: {e}",
                    rec.title
                ),
            }
            failed += 1;
            continue;
        }

        eprintln!("[plan --apply] CREATED {id}: {}", rec.title);
        created += 1;
    }

    eprintln!("[plan --apply] summary: created={created} skipped={skipped} failed={failed}");
}

/// Number of times to attempt the `br update --acceptance-criteria` follow-up
/// before parking the bead (A2).
const ACCEPTANCE_UPDATE_ATTEMPTS: u32 = 3;

/// Run `set` (the acceptance-criteria update) up to `attempts` times, invoking
/// `on_fail(attempt, err)` after each failure. Returns true on the first
/// success, false if all attempts fail. Pure control flow so it is unit-testable
/// without invoking `br`.
fn set_acceptance_with_retry<S, L>(attempts: u32, mut set: S, mut on_fail: L) -> bool
where
    S: FnMut() -> std::result::Result<(), String>,
    L: FnMut(u32, &str),
{
    for attempt in 1..=attempts {
        match set() {
            Ok(()) => return true,
            Err(e) => on_fail(attempt, &e),
        }
    }
    false
}

fn cmd_health(project_root: &Path, cli: &CliArgs) {
    let mut cfg = match config::Config::from_env(project_root.to_path_buf()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(1);
        }
    };
    if let Some(session) = &cli.session {
        apply_session_worker_defaults(&mut cfg, session, cli.window.unwrap_or(0));
    }

    let conn = match db::open(project_root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let open = db::count_by_status(&conn, db::BeadStatus::Open).unwrap_or(0);
    let in_progress = db::count_by_status(&conn, db::BeadStatus::InProgress).unwrap_or(0);
    let closed = db::count_by_status(&conn, db::BeadStatus::Closed).unwrap_or(0);
    let ready = db::count_ready_unassigned(&conn).unwrap_or(0);

    println!("=== Orchestrator Health ===");
    println!("open={open}");
    println!("in_progress={in_progress}");
    println!("closed={closed}");
    println!("ready_unassigned={ready}");

    // If session provided, add swarm health
    if let Some(session) = &cli.session {
        let window = cli.window.unwrap_or(0);
        match worker::worker_info_list_with_config(session, window, &conn, &cfg) {
            Ok(workers) => match worker::swarm_health(&workers, &conn) {
                Ok(health) => {
                    println!("worker_panes={}", health.worker_panes);
                    println!("assigned_worker_panes={}", health.assigned_worker_panes);
                    println!("unassigned_worker_panes={}", health.unassigned_worker_panes);
                    println!("idle_assigned_panes={}", health.idle_assigned_panes);
                    println!("active_assigned_panes={}", health.active_assigned_panes);
                    println!(
                        "missing_worker_assignments={}",
                        health.missing_worker_assignments
                    );
                }
                Err(e) => eprintln!("swarm health error: {e}"),
            },
            Err(e) => {
                eprintln!("swarm health error: {e}");
            }
        }
    }
}

fn cmd_worker_state(project_root: &Path, cli: &CliArgs) {
    let session = cli.session.as_deref().unwrap_or_else(|| {
        eprintln!("error: --session required for worker-state");
        std::process::exit(1);
    });
    let window = cli.window.unwrap_or(0);

    let mut cfg = match config::Config::from_env(project_root.to_path_buf()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(1);
        }
    };
    apply_session_worker_defaults(&mut cfg, session, window);

    let conn = match db::open(project_root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    match worker::worker_info_list_with_config(session, window, &conn, &cfg) {
        Ok(workers) => {
            for w in &workers {
                let json = serde_json::json!({
                    "pane_index": w.pane_index,
                    "pane_id": w.pane_id,
                    "worker_name": w.worker_name,
                    "idle": w.state == worker::WorkerState::Idle,
                    "completed_waiting": w.completed_waiting,
                    "assigned_bead": w.assigned_bead,
                    "assigned_status": w.assigned_status,
                    "assignment_age_seconds": w.assignment_age_secs,
                });
                println!("{}", serde_json::to_string(&json).unwrap_or_default());
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_poll(project_root: &Path, dry_run: bool) {
    let config = match config::Config::from_env(project_root.to_path_buf()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(1);
        }
    };

    let coord = match coordinator::Coordinator::new(config, dry_run) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    println!("=== Coordinator Poll ===");
    println!("Dry run: {dry_run}");
    println!();

    match coord.poll() {
        Ok(result) => {
            println!("=== Poll Complete ===");
            println!(
                "Processed: {}, Rejected: {}, Errors: {}, Prompts: {}",
                result.processed,
                result.rejected,
                result.errors,
                result.prompt_tasks.len()
            );
            // Submit any prompt tasks from reassignments
            if !result.prompt_tasks.is_empty() {
                println!(
                    "Submitting {} prompt(s) from reassignment...",
                    result.prompt_tasks.len()
                );
                tmux::submit_prompts_parallel(&result.prompt_tasks);
            }
        }
        Err(e) => {
            eprintln!("poll error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_assign(project_root: &Path, cli: &CliArgs) {
    let session = cli.session.as_deref().unwrap_or_else(|| {
        eprintln!("error: --session required for assign");
        std::process::exit(1);
    });
    let window = cli.window.unwrap_or(0);

    let config = match config::Config::from_env(project_root.to_path_buf()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(1);
        }
    };

    let coord = match coordinator::Coordinator::new(config, cli.dry_run) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    println!("=== Coordinator Idle Fill ===");
    println!("Session: {session}:{window}");
    println!("Dry run: {}", cli.dry_run);
    println!();

    match coord.assign_idle_workers(session, window) {
        Ok((assigned, prompts)) => {
            // Submit prompts immediately in this one-shot command
            if !prompts.is_empty() {
                tmux::submit_prompts_parallel(&prompts);
            }
            println!();
            println!("Assigned: {assigned}");
        }
        Err(e) => {
            eprintln!("assign error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_run(project_root: &Path, cli: &CliArgs) {
    let config = match config::Config::from_env(project_root.to_path_buf()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(1);
        }
    };

    let interval = Duration::from_secs(config.interval_seconds);
    let session_name = config.session_name.clone();
    let mail_server_session = config.mail_server_session();
    let seed_script = config.seed_script();
    let stall_consecutive = config.stall_consecutive_polls;
    let stall_reclaim_secs = config.stall_reclaim_seconds;
    let max_stall = config.max_stall_cycles;
    let window = cli.window.unwrap_or(config.window_index);

    // Ensure lock directory exists
    let lock_path = config.lock_file();
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let coord = match coordinator::Coordinator::new(config, cli.dry_run) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    // Startup sync: prefer the live DB state and only import if flush detects staleness.
    // Import-only at startup can clobber fresh in_progress assignments when JSONL lags DB.
    tracing::info!("Startup: reconciling DB ↔ JSONL state");
    if let Err(e) = br::sync() {
        tracing::warn!(error = %e, "Startup sync failed — DB/JSONL may be stale");
    }
    let reconciled_deps = reconcile_active_planner_dependencies(&coord.config.project_root);
    if reconciled_deps > 0 {
        tracing::info!(
            added = reconciled_deps,
            "Startup: reconciled planner dependency edges"
        );
    }

    let mut stall_counter: usize = 0;
    let mut deadlock_counter: usize = 0; // D5: consecutive zero-progress polls
    let mut bg_planner = BackgroundPlannerState::new();
    let mut planner_recovery = PlannerRecoveryState::new();
    let mut last_mail_check = Instant::now(); // Fix #5: cache mail server check
    let deep_cooldown = Duration::from_secs(coord.config.deep_planner_cooldown_secs);
    let planner_restart_cooldown = Duration::from_secs(coord.config.planner_restart_cooldown_secs);
    let fast_planner_enabled = coord.config.fast_planner_enabled;
    let deep_planner_enabled = coord.config.deep_planner_enabled;

    loop {
        let mut all_prompts: Vec<tmux::PromptTask> = Vec::new();
        println!(
            "\n=== orchestrator poll {} ===",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );

        // Acquire exclusive lock
        let lock_file = match std::fs::File::create(&lock_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("lock file error: {e}");
                thread::sleep(interval);
                continue;
            }
        };

        if !try_flock_exclusive(&lock_file) {
            eprintln!("coordinator: another instance is running, skipping cycle");
            thread::sleep(interval);
            continue;
        }

        // Ensure mail server is running (only check every 60s, not every cycle)
        if let Some(mail_session) = &mail_server_session {
            if last_mail_check.elapsed() >= Duration::from_secs(60) {
                if !ensure_mail_server(&coord.config.orch_root, mail_session) {
                    eprintln!("mail server check failed; continuing after sleep");
                    thread::sleep(interval);
                    continue;
                }
                last_mail_check = Instant::now();
            }
        }

        // Recover any workers with stuck (unsubmitted) input.
        // Skip in pull mode — workers manage their own prompts.
        if !coord.config.pull_mode {
            if let Some(session) = &session_name {
                match coord.recover_stuck_workers(session, window) {
                    Ok(n) if n > 0 => {
                        tracing::info!(recovered = n, "Recovered stuck input workers")
                    }
                    Err(e) => tracing::debug!(error = %e, "stuck input recovery skipped"),
                    _ => {}
                }
            }
        }

        if let Some(session) = &session_name {
            maintain_planner_pane(
                session,
                window,
                &coord.config.project_root,
                &coord.config.planner_command,
                planner_restart_cooldown,
                &mut planner_recovery,
            );
        }

        // Poll FIRST — process completions before reclaiming idle assignments.
        // This prevents the race where idle-fill reclaims a bead from worker A
        // and reassigns to worker B, then poll sees worker A's completion message
        // and skips it due to assignee mismatch.
        let mut poll_progress = 0usize; // completions processed + rejected (D5)
        let poll_ok = match coord.poll() {
            Ok(result) => {
                tracing::info!(
                    processed = result.processed,
                    rejected = result.rejected,
                    errors = result.errors,
                    poll_prompts = result.prompt_tasks.len(),
                    "poll complete"
                );
                poll_progress = result.processed + result.rejected;
                // Collect prompt tasks from reassignments so workers get
                // prompted immediately instead of waiting for idle-fill.
                all_prompts.extend(result.prompt_tasks);
                true
            }
            Err(e) => {
                eprintln!("coordinator poll failed: {e}");
                false
            }
        };

        if poll_ok {
            if let Some(session) = &session_name {
                // Post-poll idle fill — collect prompts for parallel submission.
                // Skip in pull mode — workers discover and claim their own work.
                if !coord.config.pull_mode {
                    thread::sleep(Duration::from_secs(1));
                    all_prompts.extend(run_idle_fill(&coord, session, window));
                }

                // Seed check + stall detection — single short-lived DB connection.
                // Worker list is captured once and reused for count + health check.
                // IMPORTANT: connection must be DROPPED before any br subprocess
                // calls (seed, planner, br create), otherwise the orchestrator's
                // open connection blocks br writes with "database is busy".
                let (worker_list, open, in_progress, ready, workers) = {
                    let conn = db::open(&coord.config.project_root).ok();
                    let wl = conn
                        .as_ref()
                        .and_then(|c| {
                            worker::worker_info_list_with_config(session, window, c, &coord.config)
                                .ok()
                        })
                        .unwrap_or_default();
                    let counts = conn
                        .as_ref()
                        .map(|c| {
                            let o = db::count_by_status(c, db::BeadStatus::Open).unwrap_or(0);
                            let ip =
                                db::count_by_status(c, db::BeadStatus::InProgress).unwrap_or(0);
                            let r = db::count_ready_unassigned(c).unwrap_or(0);
                            (o, ip, r, wl.len())
                        })
                        .unwrap_or((0, 0, 0, 0));
                    // conn dropped here — frees the DB for br subprocess writes
                    (wl, counts.0, counts.1, counts.2, counts.3)
                };

                // Reset work_exhausted if there's active work
                if in_progress > 0 || open > 0 {
                    bg_planner.work_exhausted = false;
                }

                let ready_target = coord.config.desired_ready_backlog(workers);
                let open_target = coord.config.desired_open_backlog(workers);
                let should_seed = planner_replenishment_needed(
                    open,
                    in_progress,
                    ready,
                    ready_target,
                    open_target,
                    workers,
                );

                if should_seed {
                    println!(
                        "backlog low (open={open}, in_progress={in_progress}, \
                         ready_unassigned={ready}, target_ready={ready_target}, \
                         target_open={open_target}); attempting automatic seed"
                    );

                    // Step 1: Run seed script
                    if seed_script.exists() {
                        let _ = Command::new("bash")
                            .arg(&seed_script)
                            .current_dir(&coord.config.project_root)
                            .status();
                        all_prompts.extend(run_idle_fill(&coord, session, window));
                    }

                    // Re-check queue after seed (short-lived connection)
                    let (open2, ip2, ready2) = {
                        let c = db::open(&coord.config.project_root).ok();
                        c.as_ref()
                            .map(|c| {
                                (
                                    db::count_by_status(c, db::BeadStatus::Open).unwrap_or(0),
                                    db::count_by_status(c, db::BeadStatus::InProgress).unwrap_or(0),
                                    db::count_ready_unassigned(c).unwrap_or(0),
                                )
                            })
                            .unwrap_or((0, 0, 0))
                    };

                    if planner_replenishment_needed(
                        open2,
                        ip2,
                        ready2,
                        ready_target,
                        open_target,
                        workers,
                    ) && !bg_planner.pending_recommendations.is_empty()
                    {
                        let pending = std::mem::take(&mut bg_planner.pending_recommendations);
                        let created = auto_create_beads_from_planner(&pending);
                        if created > 0 {
                            println!("deferred planner recommendations created {created} bead(s)");
                            all_prompts.extend(run_idle_fill(&coord, session, window));
                        }
                    }

                    // Step 2: Tier 1 — fast planner (no subprocesses, <500ms)
                    if planner_replenishment_needed(
                        open2,
                        ip2,
                        ready2,
                        ready_target,
                        open_target,
                        workers,
                    ) && fast_planner_enabled
                        && !bg_planner.work_exhausted
                    {
                        println!(
                            "queue still low after seed (open={open2}, in_progress={ip2}, ready={ready2}); running Tier 1 fast planner"
                        );
                        match planner::quick_recommendations(&coord.config.project_root) {
                            Ok(recs) => {
                                let created = auto_create_beads_from_planner(&recs);
                                if created > 0 {
                                    println!("Tier 1 planner created {created} bead(s)");
                                    all_prompts.extend(run_idle_fill(&coord, session, window));
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Tier 1 fast planner failed");
                            }
                        }
                    }

                    // Re-check again after Tier 1 (short-lived connection)
                    let (open3, ip3, ready3) = {
                        let c = db::open(&coord.config.project_root).ok();
                        c.as_ref()
                            .map(|c| {
                                (
                                    db::count_by_status(c, db::BeadStatus::Open).unwrap_or(0),
                                    db::count_by_status(c, db::BeadStatus::InProgress).unwrap_or(0),
                                    db::count_ready_unassigned(c).unwrap_or(0),
                                )
                            })
                            .unwrap_or((0, 0, 0))
                    };

                    // Step 3: Tier 2 — spawn deep planner in background thread
                    let bg_running = bg_planner
                        .handle
                        .as_ref()
                        .map_or(false, |h| !h.is_finished());
                    let cooldown_elapsed = bg_planner
                        .last_deep_run
                        .map_or(true, |t| t.elapsed() >= deep_cooldown);

                    if planner_replenishment_needed(
                        open3,
                        ip3,
                        ready3,
                        ready_target,
                        open_target,
                        workers,
                    ) && deep_planner_enabled
                        && !bg_running
                        && cooldown_elapsed
                        && !bg_planner.work_exhausted
                    {
                        let project_root_owned = coord.config.project_root.clone();
                        println!(
                            "queue still starved (open={open3}, in_progress={ip3}, \
                             ready={ready3}); spawning Tier 2 deep planner in background"
                        );
                        bg_planner.handle = Some(std::thread::spawn(move || {
                            planner::analyze(&project_root_owned)
                        }));
                        bg_planner.last_deep_run = Some(Instant::now());
                    }

                    // Check for work exhaustion
                    if ready3 == 0 && open3 == 0 && ip3 == 0 && !bg_running {
                        bg_planner.work_exhausted = true;
                        coord.notify_boss(
                            "No open or in-progress beads remain. \
                             Planner found no new gaps to fill.",
                        );
                    }
                } else {
                    tracing::info!(
                        open,
                        in_progress,
                        ready,
                        ready_target,
                        open_target,
                        workers,
                        "planner skipped: queue healthy or swarm saturated"
                    );
                }

                // Step 4: Harvest completed background planner thread
                if let Some(ref handle) = bg_planner.handle {
                    if handle.is_finished() {
                        let handle = bg_planner.handle.take().unwrap();
                        match handle.join() {
                            Ok(Ok(report)) => {
                                let (open4, ip4, ready4) = {
                                    let c = db::open(&coord.config.project_root).ok();
                                    c.as_ref()
                                        .map(|c| {
                                            (
                                                db::count_by_status(c, db::BeadStatus::Open)
                                                    .unwrap_or(0),
                                                db::count_by_status(c, db::BeadStatus::InProgress)
                                                    .unwrap_or(0),
                                                db::count_ready_unassigned(c).unwrap_or(0),
                                            )
                                        })
                                        .unwrap_or((0, 0, 0))
                                };
                                if planner_replenishment_needed(
                                    open4,
                                    ip4,
                                    ready4,
                                    ready_target,
                                    open_target,
                                    workers,
                                ) {
                                    let created =
                                        auto_create_beads_from_planner(&report.recommendations);
                                    if created > 0 {
                                        println!("Tier 2 deep planner created {created} bead(s)");
                                        all_prompts.extend(run_idle_fill(&coord, session, window));
                                    } else {
                                        tracing::info!(
                                            "Tier 2 deep planner returned 0 recommendations"
                                        );
                                    }
                                } else {
                                    tracing::info!(
                                        open = open4,
                                        in_progress = ip4,
                                        ready = ready4,
                                        "Tier 2 deep planner finished but queue is healthy; deferring bead creation"
                                    );
                                    bg_planner.pending_recommendations = report.recommendations;
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::warn!(error = %e, "Tier 2 deep planner failed");
                            }
                            Err(_) => {
                                tracing::warn!("Tier 2 deep planner thread panicked");
                            }
                        }
                    }
                }

                // Recovery / stall detection (short-lived connection, reuse cached worker_list).
                // CRITICAL: scope the rusqlite Connection to just the health query — drop it
                // BEFORE `assign_idle_workers_with_stale_override` spawns `br update`, otherwise
                // the child writer deadlocks on our own reader for the full 30s timeout.
                let health_opt = match db::open(&coord.config.project_root) {
                    Ok(stall_conn) => {
                        // C5: refresh the worker list immediately before the health
                        // query. The list captured at the top of the cycle can be
                        // ~seconds stale by now (seeding, br create, idle-fill all
                        // ran in between), so workers may have died or spawned —
                        // using the stale list misclassifies the swarm state.
                        let fresh_workers = worker::worker_info_list_with_config(
                            session,
                            window,
                            &stall_conn,
                            &coord.config,
                        )
                        .unwrap_or_else(|_| worker_list.clone());
                        match worker::swarm_health(&fresh_workers, &stall_conn) {
                            Ok(h) => Some(h),
                            Err(e) => {
                                tracing::warn!(error = %e, "swarm health check failed");
                                None
                            }
                        }
                    }
                    Err(_) => None,
                };
                if let Some(health) = health_opt {
                    if let Some(reason) = coord.recovery_reason(&health) {
                        stall_counter += 1;
                        if stall_counter >= stall_consecutive && !coord.config.pull_mode {
                            println!(
                                "recovery detector: forcing recovery idle-fill with \
                                 stale_assignment_seconds={stall_reclaim_secs}"
                            );
                            coord.notify_boss(&format!(
                                "Auto-recovery triggered after {stall_counter} poll(s): {reason}"
                            ));

                            // Use recovery-specific stale threshold via parameter
                            let recovery_result = coord.assign_idle_workers_with_stale_override(
                                session,
                                window,
                                stall_reclaim_secs,
                            );

                            match &recovery_result {
                                Ok((assigned, prompts)) if *assigned > 0 => {
                                    all_prompts.extend(prompts.clone());
                                    stall_counter = 0;
                                }
                                _ => {
                                    if stall_counter > 1 {
                                        stall_counter -= 1;
                                    }
                                }
                            }

                            if stall_counter >= max_stall {
                                eprintln!(
                                    "CRITICAL: stall counter reached {stall_counter} \
                                     (max={max_stall}), recovery is not making progress"
                                );
                                coord.notify_boss(&format!(
                                    "CRITICAL: Coordinator stall counter hit {stall_counter} \
                                     cycles (max={max_stall}). Recovery is not making progress. \
                                     Manual intervention may be needed. Last reason: {reason}"
                                ));
                            }
                        }
                    } else {
                        stall_counter = 0;
                    }
                }

                // D5: deadlock watchdog. Alerts when work is assigned to live
                // workers but no completions move across consecutive polls —
                // catches wedges (dead verifier, all deps blocked, stuck workers)
                // even in pull mode, where the recovery above is disabled. This
                // is alert-only and never takes risky automatic action.
                let (dc, alert) = coordinator::deadlock_watchdog_tick(
                    poll_progress > 0,
                    in_progress > 0,
                    workers > 0,
                    deadlock_counter,
                    coord.config.deadlock_watchdog_polls,
                );
                deadlock_counter = dc;
                if alert {
                    eprintln!(
                        "CRITICAL: deadlock watchdog — {deadlock_counter} consecutive polls, \
                         {in_progress} bead(s) assigned to {workers} worker(s), zero completions"
                    );
                    coord.notify_boss(&format!(
                        "Deadlock watchdog: {deadlock_counter} consecutive zero-progress polls with \
                         {in_progress} bead(s) in_progress across {workers} worker(s) and no \
                         completions. Likely wedged (dead verifier, all deps blocked, or stuck \
                         workers). Manual check advised."
                    ));
                }
            }
        }

        // Submit all collected prompts in parallel (9 workers in ~4s instead of ~36s serial).
        // In pull mode, all_prompts should be empty — workers prompt themselves.
        if !all_prompts.is_empty() && !coord.config.pull_mode {
            let t = std::time::Instant::now();
            let results = tmux::submit_prompts_parallel(&all_prompts);
            let submitted = results.iter().filter(|&&ok| ok).count();
            tracing::info!(
                total = all_prompts.len(),
                submitted,
                elapsed_ms = t.elapsed().as_millis() as u64,
                "Parallel prompt submission complete"
            );
        }

        // End-of-cycle: flush DB → JSONL so the source of truth stays current.
        // This is ONE sync per cycle (not per-bead) — catches all close/update/
        // reopen/create changes made during the cycle. Runs after the lock is
        // released so it doesn't block the next cycle if bv holds WAL.
        if let Err(e) = br::sync() {
            tracing::warn!(error = %e, "end-of-cycle sync failed — will retry next cycle");
        }

        // Release lock (dropped when lock_file goes out of scope) and sleep
        drop(lock_file);
        thread::sleep(interval);
    }
}

// --- Helper functions ---

/// Run idle fill and return prompt tasks for parallel submission.
/// In pull mode, returns empty — workers discover their own work.
fn run_idle_fill(
    coord: &coordinator::Coordinator,
    session: &str,
    window: u32,
) -> Vec<tmux::PromptTask> {
    if coord.config.pull_mode {
        return Vec::new();
    }
    let max_attempts = coord.config.idle_fill_retry_attempts;
    for attempt in 0..max_attempts {
        match coord.assign_idle_workers(session, window) {
            Ok((assigned, prompts)) => {
                if attempt > 0 {
                    tracing::info!(attempt = attempt + 1, "idle fill succeeded after retry");
                }
                if assigned > 0 {
                    tracing::info!(
                        assigned,
                        prompts = prompts.len(),
                        "idle fill queued prompts"
                    );
                }
                return prompts;
            }
            Err(e) => {
                if attempt < max_attempts - 1 {
                    tracing::warn!(attempt = attempt + 1, error = %e, "idle fill retry");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }
    Vec::new()
}

fn normalized_worker_command(current_command: &str) -> String {
    if current_command.contains("codex") {
        "codex".to_string()
    } else if current_command.contains("claude") {
        "claude".to_string()
    } else if config::is_version_string(current_command) {
        // tmux's `pane_current_command` reports the Claude CLI's `process.title`,
        // which is the version string (e.g. "2.1.121"), not the literal "claude"
        // binary name. Without this branch, the worker command would be set to
        // the version string and `matches_worker_command` would fail to recognize
        // any pane — assignment would silently report `workers=0`.
        "claude".to_string()
    } else {
        current_command.to_string()
    }
}

fn apply_session_worker_defaults(cfg: &mut config::Config, session: &str, window: u32) {
    if std::env::var("ORCH_WORKER_COMMAND").is_ok() {
        return;
    }
    let Ok(panes) = tmux::list_panes(session, window) else {
        return;
    };
    let Some(worker_pane) = panes
        .iter()
        .find(|pane| pane.index >= cfg.min_worker_pane_index && !pane.dead)
    else {
        return;
    };
    let normalized = normalized_worker_command(&worker_pane.current_command);
    cfg.worker_command = normalized.clone();
    if std::env::var("ORCH_AGENT_TYPE").is_err() {
        cfg.agent_type = if normalized.contains("codex") {
            "codex".to_string()
        } else {
            "claude".to_string()
        };
    }
}

/// Try to acquire an exclusive flock on a file. Returns true if acquired.
fn try_flock_exclusive(file: &std::fs::File) -> bool {
    use std::os::unix::io::AsFd;
    // rustix provides a safe wrapper around flock() without raw fd manipulation
    let fd = file.as_fd();
    rustix::fs::flock(fd, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_ok()
}

/// State for the background deep-planner thread.
struct BackgroundPlannerState {
    handle: Option<std::thread::JoinHandle<error::Result<planner::PlanReport>>>,
    pending_recommendations: Vec<planner::Recommendation>,
    last_deep_run: Option<Instant>,
    work_exhausted: bool,
}

impl BackgroundPlannerState {
    fn new() -> Self {
        Self {
            handle: None,
            pending_recommendations: Vec::new(),
            last_deep_run: None,
            work_exhausted: false,
        }
    }
}

struct PlannerRecoveryState {
    last_restart: Option<Instant>,
    loop_requeue_at: Option<Instant>,
}

impl PlannerRecoveryState {
    fn new() -> Self {
        Self {
            last_restart: None,
            loop_requeue_at: None,
        }
    }
}

fn planner_loop_command(planner_command: &str) -> Option<&'static str> {
    if launcher::supports_auto_loop(planner_command) {
        Some("/loop 10m /planner")
    } else {
        None
    }
}

fn planner_boot_command(planner_command: &str) -> String {
    launcher::ensure_skip_permissions(planner_command)
}

fn planner_capture_shows_rate_limit(capture: &str) -> bool {
    let lower = capture.to_ascii_lowercase();
    lower.contains("api error: rate limit")
        || lower.contains("rate limit reached")
        || lower.contains("you've hit your limit")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
}

fn maintain_planner_pane(
    session: &str,
    window: u32,
    project_root: &Path,
    planner_command: &str,
    restart_cooldown: Duration,
    state: &mut PlannerRecoveryState,
) {
    if let Some(when) = state.loop_requeue_at {
        if Instant::now() >= when {
            if let Some(loop_command) = planner_loop_command(planner_command) {
                if let Err(e) = tmux::send_literal(session, window, 1, loop_command) {
                    tracing::warn!(error = %e, "planner loop requeue failed");
                } else if let Err(e) = tmux::send_keys(session, window, 1, "Enter") {
                    tracing::warn!(error = %e, "planner loop submit failed");
                } else {
                    tracing::info!("planner loop requeued after restart");
                }
            } else {
                tracing::debug!(
                    planner_command,
                    "planner loop requeue skipped for non-auto-loop planner"
                );
            }
            state.loop_requeue_at = None;
        }
    }

    let capture = match tmux::capture_pane(session, window, 1, 80) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "planner pane capture skipped");
            return;
        }
    };

    if !planner_capture_shows_rate_limit(&capture) {
        return;
    }

    let cooldown_elapsed = state
        .last_restart
        .map_or(true, |t| t.elapsed() >= restart_cooldown);
    if !cooldown_elapsed {
        tracing::info!(
            cooldown_secs = restart_cooldown.as_secs(),
            "planner pane is rate-limited but restart cooldown has not elapsed"
        );
        return;
    }

    let planner_cmd = planner_boot_command(planner_command);
    let workdir = project_root.to_string_lossy().to_string();
    match tmux::restart_planner_pane(session, window, &workdir, &planner_cmd) {
        Ok(()) => {
            tracing::warn!("planner pane rate-limited — restarted planner pane");
            state.last_restart = Some(Instant::now());
            state.loop_requeue_at = planner_loop_command(planner_command)
                .map(|_| Instant::now() + Duration::from_secs(12));
        }
        Err(e) => {
            tracing::warn!(error = %e, "planner pane restart failed");
        }
    }
}

/// Decide whether queue replenishment should run at all.
///
/// The planner should stay idle when the swarm is already saturated with
/// in-progress work or when total active backlog is healthy, even if the
/// ready queue is temporarily low. This avoids fighting the coordinator's
/// write path for the same SQLite/WAL lock while the swarm is busy.
fn planner_replenishment_needed(
    open: usize,
    in_progress: usize,
    ready: usize,
    ready_target: usize,
    open_target: usize,
    workers: usize,
) -> bool {
    let total_active = open.saturating_add(in_progress);
    let queue_ready_enough = ready >= ready_target;
    let total_backlog_healthy = total_active >= open_target;
    let swarm_saturated = workers > 0 && in_progress >= workers;

    !(queue_ready_enough || total_backlog_healthy || swarm_saturated)
}

/// Create beads from planner recommendations using `br create`.
/// Returns the number of beads successfully created.
fn auto_create_beads_from_planner(recommendations: &[planner::Recommendation]) -> usize {
    // Pre-load existing planner keys to avoid creating duplicates.
    // Check open/in-progress beads only — if a bead was closed, the planner
    // may legitimately want to re-create it (e.g., work wasn't actually done).
    let existing_keys: Vec<String> = match db::open(std::path::Path::new(".")) {
        Ok(conn) => db::bead_descriptions_containing_by_status(
            &conn,
            "[planner-key:",
            &[db::BeadStatus::Open, db::BeadStatus::InProgress],
        )
        .unwrap_or_default(),
        Err(_) => {
            // Try from project root
            match db::open(std::path::Path::new("/Users/bone/dev/games/patina")) {
                Ok(conn) => db::bead_descriptions_containing_by_status(
                    &conn,
                    "[planner-key:",
                    &[db::BeadStatus::Open, db::BeadStatus::InProgress],
                )
                .unwrap_or_default(),
                Err(_) => vec![],
            }
        }
    };

    let mut created = 0;
    let mut planner_key_index = load_active_planner_key_index();
    for rec in recommendations {
        // Skip if an open/in-progress bead already has this planner key
        let key_pattern = format!("[planner-key: {}]", rec.gate_key);
        if existing_keys.iter().any(|desc| desc.contains(&key_pattern)) {
            continue;
        }

        // Check for a CLOSED bead with the same planner-key. If one exists,
        // we must reactivate it (not create a duplicate). If reactivation
        // fails due to writer contention / timeout, we MUST skip — falling
        // through to `br create` produces a duplicate open bead. This was
        // the observed failure mode behind pat-v1vs3 duplicating pat-pumyh
        // (planner-key ci-export-matrix).
        let closed_existing_id: Option<String> = match db::open(std::path::Path::new(".")) {
            Ok(conn) => db::bead_id_by_description_and_status(
                &conn,
                &key_pattern,
                &[db::BeadStatus::Closed],
            )
            .unwrap_or(None),
            Err(_) => match db::open(std::path::Path::new("/Users/bone/dev/games/patina")) {
                Ok(conn) => db::bead_id_by_description_and_status(
                    &conn,
                    &key_pattern,
                    &[db::BeadStatus::Closed],
                )
                .unwrap_or(None),
                Err(_) => None,
            },
        };
        if let Some(existing_id) = closed_existing_id.as_ref() {
            // Attempt fast-path reactivate (shorter timeout so we fail-fast
            // instead of getting killed by the 30s guardrail).
            let args = vec![
                "update",
                existing_id.as_str(),
                "--status",
                "open",
                "--assignee",
                "",
            ];
            match br::reactivate_bead(&args) {
                Ok(_) => {
                    tracing::info!(
                        bead = %existing_id,
                        planner_key = %rec.gate_key,
                        "planner reactivated closed bead (reused instead of creating duplicate)"
                    );
                    planner_key_index.insert(rec.gate_key.clone(), existing_id.clone());
                    created += 1;
                    continue;
                }
                Err(e) => {
                    // CRITICAL: do NOT fall through to `br create`. A closed
                    // bead with this planner-key exists; creating a new one
                    // would duplicate it. Retry on next planner cycle.
                    tracing::warn!(
                        bead = %existing_id,
                        planner_key = %rec.gate_key,
                        error = %e,
                        "skipping create — closed bead exists but reactivation failed; will retry next cycle"
                    );
                    continue;
                }
            }
        }

        let description = format!(
            "IMPLEMENT the feature: {title}\n\
             Acceptance: {acceptance}\n\
             [planner-key: {key}]",
            title = rec.title,
            acceptance = rec.acceptance_command,
            key = rec.gate_key,
        );
        // Sanitize labels: br only allows alphanumeric, hyphen, underscore, colon
        let labels: String = rec
            .labels
            .iter()
            .map(|l| {
                l.chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == ':')
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(",");
        let priority = rec.priority.to_string();

        let mut args = vec![
            "create",
            "--title",
            &rec.title,
            "--type",
            "task",
            "--priority",
            &priority,
            "--description",
            &description,
        ];
        if !labels.is_empty() {
            args.push("--labels");
            args.push(&labels);
        }

        match br::run_br_public(&args) {
            Ok(output) => {
                tracing::info!(title = %rec.title, "planner auto-created bead");
                created += 1;

                // Extract the bead ID from the create output and add dependencies
                if let Some(bead_id) = output.split_whitespace().find(|w| w.starts_with("pat-")) {
                    planner_key_index.insert(rec.gate_key.clone(), bead_id.to_string());
                    for dep_key in &rec.depends_on {
                        if let Some(dep_bead_id) = planner_key_index.get(dep_key) {
                            if let Err(e) = br::dep_add(bead_id, dep_bead_id) {
                                tracing::debug!(
                                    bead = bead_id,
                                    blocker = dep_bead_id,
                                    dep_key = dep_key,
                                    error = %e,
                                    "failed to add dependency edge"
                                );
                            }
                        } else {
                            tracing::debug!(
                                bead = bead_id,
                                dep_key = dep_key,
                                "skipping dependency edge because blocker bead is not active"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                let msg = format!("{e}");
                if msg.to_lowercase().contains("already exists")
                    || msg.to_lowercase().contains("duplicate")
                {
                    // Expected: bead already exists from previous cycle
                } else {
                    tracing::warn!(title = %rec.title, error = %e, "failed to create bead");
                }
            }
        }
    }
    created
}

fn load_active_planner_key_index() -> std::collections::HashMap<String, String> {
    let statuses = [db::BeadStatus::Open, db::BeadStatus::InProgress];
    match db::open(std::path::Path::new(".")) {
        Ok(conn) => db::planner_key_to_bead_id_by_status(&conn, &statuses).unwrap_or_default(),
        Err(_) => match db::open(std::path::Path::new("/Users/bone/dev/games/patina")) {
            Ok(conn) => db::planner_key_to_bead_id_by_status(&conn, &statuses).unwrap_or_default(),
            Err(_) => std::collections::HashMap::new(),
        },
    }
}

fn reconcile_active_planner_dependencies(project_root: &Path) -> usize {
    let config = project_config::load(project_root);
    let specs = planner::load_execution_maps(project_root, &config);
    if specs.is_empty() {
        return 0;
    }

    let statuses = [db::BeadStatus::Open, db::BeadStatus::InProgress];
    let planner_key_index = match db::open(project_root) {
        Ok(conn) => db::planner_key_to_bead_id_by_status(&conn, &statuses).unwrap_or_default(),
        Err(_) => return 0,
    };

    let mut added = 0;
    for (blocked_id, blocker_id, blocked_key, dep_key) in
        resolve_dependency_edges(&specs, &planner_key_index)
    {
        match br::dep_add(&blocked_id, &blocker_id) {
            Ok(_) => {
                added += 1;
            }
            Err(e) => {
                let msg = format!("{e}").to_ascii_lowercase();
                if !(msg.contains("already")
                    || msg.contains("exists")
                    || msg.contains("duplicate")
                    || msg.contains("constraint"))
                {
                    tracing::debug!(
                        blocked_id,
                        blocker_id,
                        blocked_key,
                        dep_key,
                        error = %e,
                        "failed to reconcile planner dependency edge"
                    );
                }
            }
        }
    }

    added
}

fn resolve_dependency_edges(
    specs: &[prd_parser::BeadSpec],
    planner_key_index: &std::collections::HashMap<String, String>,
) -> Vec<(String, String, String, String)> {
    let mut edges = Vec::new();
    for spec in specs {
        let Some(blocked_id) = planner_key_index.get(&spec.bead_key) else {
            continue;
        };
        for dep_key in &spec.depends_on {
            let Some(blocker_id) = planner_key_index.get(dep_key) else {
                continue;
            };
            edges.push((
                blocked_id.clone(),
                blocker_id.clone(),
                spec.bead_key.clone(),
                dep_key.clone(),
            ));
        }
    }
    edges
}

fn ensure_mail_server(orch_root: &std::path::Path, mail_session: &str) -> bool {
    let script = orch_root.join("swarm/ensure_mail_server.sh");
    if !script.exists() {
        return true;
    }
    Command::new("bash")
        .arg(&script)
        .arg(mail_session)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        normalized_worker_command, planner_boot_command, planner_capture_shows_rate_limit,
        planner_loop_command, planner_replenishment_needed, resolve_dependency_edges,
        set_acceptance_with_retry,
    };
    use crate::prd_parser::BeadSpec;
    use std::cell::Cell;

    // ─── A2: acceptance-criteria update retry ────────────────────────────

    #[test]
    fn test_set_acceptance_succeeds_first_try() {
        let calls = Cell::new(0);
        let fails = Cell::new(0);
        let ok = set_acceptance_with_retry(
            3,
            || {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |_, _| fails.set(fails.get() + 1),
        );
        assert!(ok);
        assert_eq!(calls.get(), 1, "should not retry after success");
        assert_eq!(fails.get(), 0);
    }

    #[test]
    fn test_set_acceptance_succeeds_after_transient_failure() {
        let calls = Cell::new(0);
        let fails = Cell::new(0);
        let ok = set_acceptance_with_retry(
            3,
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 2 {
                    Err("transient br error".to_string())
                } else {
                    Ok(())
                }
            },
            |_, _| fails.set(fails.get() + 1),
        );
        assert!(ok);
        assert_eq!(calls.get(), 2);
        assert_eq!(fails.get(), 1, "one failure callback before success");
    }

    #[test]
    fn test_set_acceptance_gives_up_after_all_attempts() {
        let calls = Cell::new(0);
        let fails = Cell::new(0);
        let ok = set_acceptance_with_retry(
            3,
            || {
                calls.set(calls.get() + 1);
                Err("persistent failure".to_string())
            },
            |attempt, err| {
                fails.set(fails.get() + 1);
                assert!(attempt >= 1 && attempt <= 3);
                assert_eq!(err, "persistent failure");
            },
        );
        assert!(!ok, "must report failure so caller defers the bead");
        assert_eq!(calls.get(), 3);
        assert_eq!(fails.get(), 3);
    }

    // Regression: tmux's `pane_current_command` reports the Claude CLI's
    // `process.title` ("2.1.121"), not the literal "claude" binary name. Before
    // the fix, `normalized_worker_command` returned the version string verbatim,
    // which then clobbered `cfg.worker_command` and caused
    // `matches_worker_command` to fail for every Claude pane (workers=0).
    #[test]
    fn normalized_worker_command_handles_claude_version_strings() {
        assert_eq!(normalized_worker_command("claude"), "claude");
        assert_eq!(normalized_worker_command("claude --model opus"), "claude");
        assert_eq!(normalized_worker_command("codex"), "codex");
        assert_eq!(normalized_worker_command("codex --model gpt-5"), "codex");
        // The bug: tmux reports the Claude version, not the program name.
        assert_eq!(normalized_worker_command("2.1.121"), "claude");
        assert_eq!(normalized_worker_command("2.1"), "claude");
        // Non-version, non-known commands are passed through unchanged.
        assert_eq!(normalized_worker_command("bash"), "bash");
    }

    #[test]
    fn planner_skips_when_swarm_is_saturated() {
        assert!(
            !planner_replenishment_needed(18, 21, 0, 21, 42, 21),
            "21 in-progress workers should suppress planner replenishment even if ready is empty"
        );
    }

    #[test]
    fn planner_skips_when_total_backlog_is_already_healthy() {
        assert!(
            !planner_replenishment_needed(31, 8, 0, 9, 18, 9),
            "healthy total backlog should suppress planner replenishment"
        );
    }

    #[test]
    fn planner_runs_when_ready_and_total_backlog_are_both_low() {
        assert!(
            planner_replenishment_needed(3, 2, 0, 9, 18, 9),
            "low ready backlog plus low total active work should trigger replenishment"
        );
    }

    #[test]
    fn planner_skips_when_ready_queue_is_already_healthy() {
        assert!(
            !planner_replenishment_needed(2, 1, 9, 9, 18, 9),
            "healthy ready queue should suppress planner replenishment"
        );
    }

    #[test]
    fn planner_rate_limit_detection_matches_real_signals() {
        assert!(planner_capture_shows_rate_limit(
            "API Error: Rate limit reached"
        ));
        assert!(planner_capture_shows_rate_limit("You've hit your limit"));
        assert!(planner_capture_shows_rate_limit("429 Too many requests"));
        assert!(!planner_capture_shows_rate_limit("Running scheduled task"));
    }

    #[test]
    fn planner_loop_command_is_stable() {
        assert_eq!(planner_loop_command("claude"), Some("/loop 10m /planner"));
        assert_eq!(planner_loop_command("codex --model gpt-5.4"), None);
    }

    #[test]
    fn planner_boot_command_adds_permission_flag() {
        let claude = planner_boot_command("claude");
        assert!(claude.contains("claude"));
        assert!(claude.contains("dangerously-skip-permissions"));

        let codex = planner_boot_command("codex --model gpt-5.4");
        assert!(codex.contains("codex"));
        assert!(codex.contains("dangerously-bypass-approvals-and-sandbox"));
    }

    #[test]
    fn resolve_dependency_edges_uses_planner_keys() {
        let specs = vec![
            BeadSpec {
                section: "Now".to_string(),
                subsection: Some("Lane A".to_string()),
                bead_key: "phase6-a".to_string(),
                description: "A".to_string(),
                acceptance_command: None,
                depends_on: vec![],
                priority: 1,
            },
            BeadSpec {
                section: "Now".to_string(),
                subsection: Some("Lane A".to_string()),
                bead_key: "phase6-b".to_string(),
                description: "B".to_string(),
                acceptance_command: None,
                depends_on: vec!["phase6-a".to_string(), "missing".to_string()],
                priority: 1,
            },
        ];
        let index = std::collections::HashMap::from([
            ("phase6-a".to_string(), "pat-a".to_string()),
            ("phase6-b".to_string(), "pat-b".to_string()),
        ]);

        let edges = resolve_dependency_edges(&specs, &index);
        assert_eq!(
            edges,
            vec![(
                "pat-b".to_string(),
                "pat-a".to_string(),
                "phase6-b".to_string(),
                "phase6-a".to_string()
            )]
        );
    }

    /// Regression: auto_create_beads_from_planner must call br::dep_add for
    /// each depends_on entry. Verify the dep-add code path exists.
    #[test]
    fn test_auto_create_wires_dependencies() {
        let source = include_str!("main.rs");
        let fn_start = source
            .find("fn auto_create_beads_from_planner")
            .expect("auto_create_beads_from_planner must exist");
        let body = &source[fn_start..std::cmp::min(fn_start + 6000, source.len())];

        assert!(
            body.contains("dep_add"),
            "auto_create_beads_from_planner must call br::dep_add to wire dependency edges"
        );
        assert!(
            body.contains("depends_on"),
            "auto_create_beads_from_planner must iterate rec.depends_on"
        );
        assert!(
            body.contains("planner_key_index"),
            "must maintain a planner_key_index to resolve gate_key → bead_id for dep edges"
        );
    }

    /// Regression: auto_create_beads_from_planner must reactivate existing
    /// CLOSED planner-key beads instead of creating duplicates.
    ///
    /// Context: the planner burst-creates ~22 beads every 10 minutes. When a
    /// planner-key matches a closed bead, the code must call `br update`
    /// (via br::reactivate_bead) to reopen that bead. Without this check,
    /// each planner cycle produces new duplicate beads with the same
    /// planner-key (e.g. pat-v1vs3 duplicating pat-pumyh, planner-key
    /// ci-export-matrix).
    #[test]
    fn test_auto_create_reactivates_closed_planner_keys() {
        let source = include_str!("main.rs");
        let fn_start = source
            .find("fn auto_create_beads_from_planner")
            .expect("auto_create_beads_from_planner must exist");
        let body = &source[fn_start..std::cmp::min(fn_start + 6000, source.len())];

        assert!(
            body.contains("BeadStatus::Closed"),
            "must query for CLOSED beads with the same planner-key before creating"
        );
        assert!(
            body.contains("bead_id_by_description_and_status"),
            "must use bead_id_by_description_and_status to locate closed planner-key beads"
        );
        assert!(
            body.contains("reactivate_bead"),
            "must call br::reactivate_bead (short-timeout path) to reopen closed beads"
        );
    }

    /// Regression: when reactivation fails (timeout/DB contention) the
    /// function MUST skip the recommendation — it must NOT fall through
    /// to `br create`. Fall-through produces duplicate open beads with
    /// the same planner-key.
    ///
    /// Static inspection: the Err branch for reactivate_bead must `continue`
    /// the loop, not fall through. We verify by confirming the expected
    /// log message and a `continue` appear inside the Err match arm, and
    /// that `run_br_public` for `create` is NOT reachable from the
    /// reactivate-Err branch.
    #[test]
    fn test_auto_create_skips_on_reactivation_failure() {
        let source = include_str!("main.rs");
        let fn_start = source
            .find("fn auto_create_beads_from_planner")
            .expect("auto_create_beads_from_planner must exist");
        let body = &source[fn_start..std::cmp::min(fn_start + 6000, source.len())];

        // The warn message must exist and be associated with the skip path.
        assert!(
            body.contains("reactivation failed"),
            "must emit a warn log on reactivation failure before skipping"
        );
        assert!(
            body.contains("will retry next cycle"),
            "warn log must note that the planner will retry on the next cycle"
        );

        // Locate the reactivate_bead(&args) call and verify its Err arm
        // contains `continue` BEFORE any `run_br_public` / `br create` call.
        let reactivate_idx = body
            .find("reactivate_bead(&args)")
            .expect("reactivate_bead(&args) must be called");
        let tail = &body[reactivate_idx..];
        let err_idx = tail.find("Err(e)").expect("must handle reactivate Err(e)");
        let err_tail = &tail[err_idx..];
        // Within ~1200 chars of the Err arm, `continue;` must appear before
        // any br::run_br_public call (the br create fallback).
        let continue_pos = err_tail.find("continue;");
        let fallback_pos = err_tail.find("run_br_public");
        assert!(
            continue_pos.is_some(),
            "Err arm of reactivate_bead must contain `continue;` to skip the recommendation"
        );
        // If run_br_public appears in the Err tail, it must come AFTER the
        // closing brace of the loop iteration (i.e. after continue), not as
        // a fall-through. Enforce: `continue;` precedes any run_br_public
        // within the immediate Err arm context.
        if let (Some(cp), Some(fp)) = (continue_pos, fallback_pos) {
            assert!(
                cp < fp,
                "`continue;` must precede any `run_br_public` call in the reactivate-Err branch — \
                 otherwise a timed-out reactivation falls through to `br create` and produces a duplicate"
            );
        }
    }

    /// Regression: the stall-detection rusqlite Connection must be scoped to
    /// a short-lived expression that drops the Connection BEFORE the code
    /// spawns `br update` via `assign_idle_workers_with_stale_override`.
    ///
    /// Root cause we fixed: holding a rusqlite Connection open while a child
    /// `br update` writer tries to acquire the WAL lock deadlocks the writer
    /// against our own reader, for the full 30s `spawn_br_with_deadline`
    /// timeout. The fix is to open the Connection inside a `match` block,
    /// compute `health_opt`, and let the Connection drop before the writer
    /// spawns.
    ///
    /// This test enforces the invariant statically: in the coordinator loop,
    /// `db::open(&coord.config.project_root)` (the stall-detection path)
    /// must be followed by a `match` that binds the Connection to a name
    /// that goes out of scope (producing `Option<Health>`), and the next
    /// `assign_idle_workers_with_stale_override` call must appear AFTER
    /// that match (i.e., the Connection has been dropped).
    #[test]
    fn test_stall_detection_connection_is_scoped() {
        let source = include_str!("main.rs");

        // Constrain the search to the runtime code above the tests module, so
        // the assertions don't self-match the anti-pattern strings embedded
        // in this very test.
        let tests_marker = "#[cfg(test)]\nmod tests";
        let runtime = match source.find(tests_marker) {
            Some(idx) => &source[..idx],
            None => source,
        };

        // The health-check open call must exist and be written as a `match`
        // expression. (Binding the Connection by reference from
        // `db::open(...).ok()` extends the borrow across the rest of the
        // scope and reintroduces the 30s self-deadlock.)
        let scoped_open = concat!(
            "let health_opt = match db::open(",
            "&coord.config.project_root)"
        );
        assert!(
            runtime.contains(scoped_open),
            "stall-detection path must open the Connection inside a scoped `match` expression \
             that drops the Connection before `assign_idle_workers_with_stale_override` runs"
        );

        // The anti-pattern must not appear in runtime code.
        let anti_pattern = concat!("if let Some(ref ", "stall_conn) = db::open");
        assert!(
            !runtime.contains(anti_pattern),
            "must not bind rusqlite Connection by reference from db::open(...).ok() — \
             the Connection must be dropped before any child `br update` spawns"
        );

        // The CRITICAL comment documenting this invariant must stay in place
        // so a future refactor can't silently drop it.
        assert!(
            runtime.contains("scope the rusqlite Connection to just the health query"),
            "the invariant comment explaining the Connection-scoping rule must remain in source"
        );

        // Ordering: the scoped match must appear BEFORE
        // `assign_idle_workers_with_stale_override` in the coordinator loop.
        let match_pos = runtime
            .find(scoped_open)
            .expect("stall-detection match must exist");
        let call_pos = runtime
            .find("assign_idle_workers_with_stale_override(")
            .expect("assign_idle_workers_with_stale_override call must exist");
        assert!(
            match_pos < call_pos,
            "scoped Connection match must precede the stale-override assign call"
        );
    }
}
