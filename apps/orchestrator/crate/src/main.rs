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
         \x20 --model CMD        model command (launch only, default: claude)\n\
         \x20 --force            overwrite existing session (launch only)\n\
         \x20 --with-coordinator start coordinator run loop (launch only)\n\
         \x20 --dry-run          print actions without executing\n\
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
    model: Option<String>,
    force: bool,
    dry_run: bool,
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
    let mut model = None;
    let mut force = false;
    let mut dry_run = false;
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
            "--model" => {
                i += 1;
                model = args.get(i).cloned();
            }
            "--force" => {
                force = true;
            }
            "--dry-run" => {
                dry_run = true;
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
        model,
        force,
        dry_run,
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

    let config = launcher::LaunchConfig {
        session_name: session.to_string(),
        worker_count: cli.workers.unwrap_or(9),
        model_command: cli.model.clone().unwrap_or_else(|| "claude".to_string()),
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
            eprintln!("  P{}: {}", r.priority, r.title);
        }
        eprintln!("Phase: {:?}", report.phase);
        eprintln!();
    }

    // Always print JSON to stdout for the skill to parse
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

fn cmd_health(project_root: &Path, cli: &CliArgs) {
    let cfg = match config::Config::from_env(project_root.to_path_buf()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(1);
        }
    };

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

    let cfg = match config::Config::from_env(project_root.to_path_buf()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(1);
        }
    };

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
                result.processed, result.rejected, result.errors, result.prompt_tasks.len()
            );
            // Submit any prompt tasks from reassignments
            if !result.prompt_tasks.is_empty() {
                println!("Submitting {} prompt(s) from reassignment...", result.prompt_tasks.len());
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

    // Startup sync: import JSONL → DB so we start with a complete database.
    // This catches beads created by other sessions, git pulls, or manual edits.
    tracing::info!("Startup: importing JSONL → DB");
    if let Err(e) = br::force_sync() {
        tracing::warn!(error = %e, "Startup sync failed — DB may be stale");
    }

    let mut stall_counter: usize = 0;
    let mut bg_planner = BackgroundPlannerState::new();
    let mut planner_recovery = PlannerRecoveryState::new();
    let mut last_mail_check = Instant::now(); // Fix #5: cache mail server check
    let deep_cooldown = Duration::from_secs(coord.config.deep_planner_cooldown_secs);
    let planner_restart_cooldown =
        Duration::from_secs(coord.config.planner_restart_cooldown_secs);
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
                    Ok(n) if n > 0 => tracing::info!(recovered = n, "Recovered stuck input workers"),
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
                coord.config.is_codex(),
                planner_restart_cooldown,
                &mut planner_recovery,
            );
        }

        // Poll FIRST — process completions before reclaiming idle assignments.
        // This prevents the race where idle-fill reclaims a bead from worker A
        // and reassigns to worker B, then poll sees worker A's completion message
        // and skips it due to assignee mismatch.
        let poll_ok = match coord.poll() {
            Ok(result) => {
                tracing::info!(
                    processed = result.processed,
                    rejected = result.rejected,
                    errors = result.errors,
                    poll_prompts = result.prompt_tasks.len(),
                    "poll complete"
                );
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
                    let wl = conn.as_ref()
                        .and_then(|c| worker::worker_info_list_with_config(session, window, c, &coord.config).ok())
                        .unwrap_or_default();
                    let counts = conn.as_ref()
                        .map(|c| {
                            let o = db::count_by_status(c, db::BeadStatus::Open).unwrap_or(0);
                            let ip = db::count_by_status(c, db::BeadStatus::InProgress).unwrap_or(0);
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
                        c.as_ref().map(|c| (
                            db::count_by_status(c, db::BeadStatus::Open).unwrap_or(0),
                            db::count_by_status(c, db::BeadStatus::InProgress).unwrap_or(0),
                            db::count_ready_unassigned(c).unwrap_or(0),
                        )).unwrap_or((0, 0, 0))
                    };

                    if planner_replenishment_needed(
                        open2,
                        ip2,
                        ready2,
                        ready_target,
                        open_target,
                        workers,
                    ) && !bg_planner.pending_recommendations.is_empty() {
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
                    ) && fast_planner_enabled && !bg_planner.work_exhausted {
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
                        c.as_ref().map(|c| (
                            db::count_by_status(c, db::BeadStatus::Open).unwrap_or(0),
                            db::count_by_status(c, db::BeadStatus::InProgress).unwrap_or(0),
                            db::count_ready_unassigned(c).unwrap_or(0),
                        )).unwrap_or((0, 0, 0))
                    };

                    // Step 3: Tier 2 — spawn deep planner in background thread
                    let bg_running = bg_planner.handle.as_ref().map_or(false, |h| !h.is_finished());
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
                    )
                        && deep_planner_enabled
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
                                    c.as_ref().map(|c| (
                                        db::count_by_status(c, db::BeadStatus::Open).unwrap_or(0),
                                        db::count_by_status(c, db::BeadStatus::InProgress).unwrap_or(0),
                                        db::count_ready_unassigned(c).unwrap_or(0),
                                    )).unwrap_or((0, 0, 0))
                                };
                                if planner_replenishment_needed(
                                    open4,
                                    ip4,
                                    ready4,
                                    ready_target,
                                    open_target,
                                    workers,
                                ) {
                                    let created = auto_create_beads_from_planner(&report.recommendations);
                                    if created > 0 {
                                        println!("Tier 2 deep planner created {created} bead(s)");
                                        all_prompts.extend(run_idle_fill(&coord, session, window));
                                    } else {
                                        tracing::info!("Tier 2 deep planner returned 0 recommendations");
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

                // Recovery / stall detection (short-lived connection, reuse cached worker_list)
                if let Some(ref stall_conn) = db::open(&coord.config.project_root).ok() {
                    let health = match worker::swarm_health(&worker_list, stall_conn) {
                        Ok(h) => h,
                        Err(e) => {
                            tracing::warn!(error = %e, "swarm health check failed");
                            continue;
                        }
                    };

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
                            let recovery_result =
                                coord.assign_idle_workers_with_stale_override(session, window, stall_reclaim_secs);

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
fn run_idle_fill(coord: &coordinator::Coordinator, session: &str, window: u32) -> Vec<tmux::PromptTask> {
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
                    tracing::info!(assigned, prompts = prompts.len(), "idle fill queued prompts");
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

fn planner_loop_command() -> &'static str {
    "/loop 10m /planner"
}

fn planner_boot_command(is_codex: bool) -> String {
    if is_codex {
        launcher::ensure_skip_permissions("codex")
    } else {
        launcher::ensure_skip_permissions("claude")
    }
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
    is_codex: bool,
    restart_cooldown: Duration,
    state: &mut PlannerRecoveryState,
) {
    if let Some(when) = state.loop_requeue_at {
        if Instant::now() >= when {
            if let Err(e) = tmux::send_literal(session, window, 1, planner_loop_command()) {
                tracing::warn!(error = %e, "planner loop requeue failed");
            } else if let Err(e) = tmux::send_keys(session, window, 1, "Enter") {
                tracing::warn!(error = %e, "planner loop submit failed");
            } else {
                tracing::info!("planner loop requeued after restart");
                state.loop_requeue_at = None;
            }
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

    let planner_cmd = planner_boot_command(is_codex);
    let workdir = project_root.to_string_lossy().to_string();
    match tmux::restart_planner_pane(session, window, &workdir, &planner_cmd) {
        Ok(()) => {
            tracing::warn!("planner pane rate-limited — restarted planner pane");
            state.last_restart = Some(Instant::now());
            state.loop_requeue_at = Some(Instant::now() + Duration::from_secs(12));
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
        ).unwrap_or_default(),
        Err(_) => {
            // Try from project root
            match db::open(std::path::Path::new("/Users/bone/dev/games/patina")) {
                Ok(conn) => db::bead_descriptions_containing_by_status(
                    &conn,
                    "[planner-key:",
                    &[db::BeadStatus::Open, db::BeadStatus::InProgress],
                ).unwrap_or_default(),
                Err(_) => vec![],
            }
        }
    };

    let mut created = 0;
    for rec in recommendations {
        // Skip if an open/in-progress bead already has this planner key
        let key_pattern = format!("[planner-key: {}]", rec.gate_key);
        if existing_keys.iter().any(|desc| desc.contains(&key_pattern)) {
            continue;
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
        let labels: String = rec.labels.iter()
            .map(|l| l.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == ':').collect::<String>())
            .collect::<Vec<_>>()
            .join(",");
        let priority = rec.priority.to_string();

        let mut args = vec![
            "create",
            "--title", &rec.title,
            "--type", "task",
            "--priority", &priority,
            "--description", &description,
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
                if !rec.depends_on.is_empty() {
                    // br create output contains the bead ID (e.g., "Created pat-abc123")
                    if let Some(bead_id) = output.split_whitespace()
                        .find(|w| w.starts_with("pat-"))
                    {
                        for dep_key in &rec.depends_on {
                            if let Err(e) = br::dep_add(bead_id, dep_key) {
                                tracing::debug!(
                                    bead = bead_id, dep = dep_key, error = %e,
                                    "failed to add dependency (dep bead may not exist yet)"
                                );
                            }
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
        planner_boot_command,
        planner_capture_shows_rate_limit,
        planner_loop_command,
        planner_replenishment_needed,
    };

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
        assert!(planner_capture_shows_rate_limit("API Error: Rate limit reached"));
        assert!(planner_capture_shows_rate_limit("You've hit your limit"));
        assert!(planner_capture_shows_rate_limit("429 Too many requests"));
        assert!(!planner_capture_shows_rate_limit("Running scheduled task"));
    }

    #[test]
    fn planner_loop_command_is_stable() {
        assert_eq!(planner_loop_command(), "/loop 10m /planner");
    }

    #[test]
    fn planner_boot_command_adds_permission_flag() {
        let claude = planner_boot_command(false);
        assert!(claude.contains("claude"));
        assert!(claude.contains("dangerously-skip-permissions"));

        let codex = planner_boot_command(true);
        assert!(codex.contains("codex"));
        assert!(codex.contains("dangerously-bypass-approvals-and-sandbox"));
    }
}
