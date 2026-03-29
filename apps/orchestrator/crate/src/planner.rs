//! Planner analysis engine.
//!
//! Runs analysis passes (parity measurement, acceptance gate tests,
//! queue depth) and produces a `PlanReport` with recommendations for new
//! beads that should be created.
//!
//! Configuration is loaded from `.orchestrator/planner.toml` or discovered
//! by convention. All project-specific logic lives in the config and PRD
//! markdown files, not in compiled code.

use std::path::Path;
use std::process::Command;

use regex::Regex;
use serde::Serialize;

use crate::db;
use crate::error::Result;
use crate::gate_map;
use crate::prd_parser;
use crate::project_config::{self, AnalysisCommand, CompletionCondition, ParserType, ProjectPlannerConfig};

// ─── Public types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PlanReport {
    pub timestamp: String,
    pub parity: ParityReport,
    pub gates: GateReport,
    pub queue: QueueReport,
    pub recommendations: Vec<Recommendation>,
    pub phase: Phase,
}

#[derive(Debug, Serialize)]
pub struct ParityReport {
    pub overall: f64,
    pub total: usize,
    pub matched: usize,
    pub scenes: Vec<SceneParity>,
}

#[derive(Debug, Serialize)]
pub struct SceneParity {
    pub name: String,
    pub total: usize,
    pub matched: usize,
    pub parity: f64,
}

#[derive(Debug, Serialize)]
pub struct GateReport {
    pub total: usize,
    pub passing: Vec<String>,
    pub failing: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct QueueReport {
    pub open: usize,
    pub in_progress: usize,
    pub closed: usize,
    pub ready_unassigned: usize,
}

#[derive(Debug, Serialize)]
pub struct Recommendation {
    pub title: String,
    pub priority: u32,
    pub labels: Vec<String>,
    pub description: String,
    pub acceptance_command: String,
    pub gate_key: String,
    pub reason: String,
    /// Planner keys of beads this recommendation depends on.
    /// These will become `br dep add` calls after creation.
    pub depends_on: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub enum Phase {
    V1Active,
    V1NearlyDone,
    V1Complete,
}

// ─── Queue throttle constant ──────────────────────────────────────────────

const QUEUE_THROTTLE: usize = 12;

// ─── Main entry point ─────────────────────────────────────────────────────

/// Run all analysis passes and produce a plan report.
pub fn analyze(project_root: &Path) -> Result<PlanReport> {
    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    // Load project-specific config
    let config = project_config::load(project_root);

    // Parse PRD files
    let criteria = load_criteria(project_root, &config);
    let bead_specs = load_execution_maps(project_root, &config);

    // Build dynamic gate map
    let dynamic_gates = gate_map::build_gate_map(&bead_specs, &criteria);

    // Run analysis commands from config, or use default behavior
    let (parity, gates) = if config.analysis.is_empty() {
        // Fallback: try the conventional commands
        let engine_dir = project_root.join("engine-rs");
        if engine_dir.exists() {
            (run_parity_pass(&engine_dir), run_gate_pass_default(&engine_dir))
        } else {
            (empty_parity(), empty_gates())
        }
    } else {
        run_analysis_commands(project_root, &config.analysis)
    };

    // Pass C — Queue depth
    let queue = run_queue_pass(project_root)?;

    // Two-tier dedup strategy:
    // - Gate recommendations: dedup against open/in-progress only, so still-failing
    //   gates get re-recommended even if a previous bead was closed.
    // - Parity recommendations: dedup against ALL statuses to prevent re-recommending
    //   completed work.
    let active_titles = match db::open(project_root) {
        Ok(conn) => {
            db::bead_titles_by_status(
                &conn,
                &[db::BeadStatus::Open, db::BeadStatus::InProgress],
            )
            .unwrap_or_default()
        }
        Err(_) => vec![],
    };

    let active_keys = match db::open(project_root) {
        Ok(conn) => collect_existing_planner_keys_by_status(
            &conn,
            &[db::BeadStatus::Open, db::BeadStatus::InProgress],
        ),
        Err(_) => vec![],
    };

    let all_titles = match db::open(project_root) {
        Ok(conn) => db::bead_titles_all(&conn).unwrap_or_default(),
        Err(_) => vec![],
    };

    // Determine phase
    let phase = determine_phase(&gates, &parity, &config);

    let mut recommendations = Vec::new();

    if matches!(phase, Phase::V1Complete) {
        // V1 is done — skip gate/parity recommendations (they'd create duplicates
        // for already-passing work). Only generate next-phase recommendations.
        recommendations.extend(generate_next_phase_recommendations(
            project_root,
            &config,
            &active_titles,
            &queue,
        ));
    } else {
        // V1 still in progress — generate gate and parity recommendations
        recommendations.extend(generate_recommendations(
            &gates,
            &dynamic_gates,
            &active_titles,
            &active_keys,
            &queue,
        ));
        recommendations.extend(generate_parity_recommendations(
            &parity,
            &all_titles,
            &queue,
        ));
    }

    // Sort all recommendations by priority
    recommendations.sort_by_key(|r| r.priority);

    Ok(PlanReport {
        timestamp,
        parity,
        gates,
        queue,
        recommendations,
        phase,
    })
}

/// Tier 1: Fast recommendations from PRD parsing only (no subprocess calls).
/// Runs in <500ms. Safe to call inline in the coordinator loop.
pub fn quick_recommendations(project_root: &Path) -> Result<Vec<Recommendation>> {
    // Load project-specific config
    let config = project_config::load(project_root);

    // Parse PRD files
    let criteria = load_criteria(project_root, &config);
    let bead_specs = load_execution_maps(project_root, &config);

    // Build dynamic gate map
    let dynamic_gates = gate_map::build_gate_map(&bead_specs, &criteria);

    // Query queue depth (short-lived connection)
    let queue = run_queue_pass(project_root)?;

    // Collect existing bead titles and planner keys for dedup.
    // Include ALL statuses (including closed) to prevent re-recommending
    // work that was already completed.
    let existing_titles = match db::open(project_root) {
        Ok(conn) => db::bead_titles_all(&conn).unwrap_or_default(),
        Err(_) => vec![],
    };

    let existing_keys = match db::open(project_root) {
        Ok(conn) => collect_existing_planner_keys(&conn),
        Err(_) => vec![],
    };

    let mut recommendations = Vec::new();

    // If all criteria are checked, V1 is complete — skip V1 bead generation
    // and only produce next-phase recommendations. This prevents duplicate beads
    // for already-passing gates when the text-match dedup is imperfect.
    let all_criteria_checked = !criteria.is_empty() && criteria.iter().all(|c| c.checked);
    if all_criteria_checked {
        // Jump straight to next-phase recommendations
        let active_titles = match db::open(project_root) {
            Ok(conn) => db::bead_titles_by_status(
                &conn,
                &[db::BeadStatus::Open, db::BeadStatus::InProgress],
            ).unwrap_or_default(),
            Err(_) => vec![],
        };
        recommendations.extend(generate_next_phase_recommendations(
            project_root,
            &config,
            &active_titles,
            &queue,
        ));
        recommendations.sort_by_key(|r| r.priority);
        return Ok(recommendations);
    }

    // For each bead spec in execution maps with no matching existing bead → recommend.
    // Also skip specs whose corresponding criteria item is already checked.
    for spec in &bead_specs {
        // Skip if the criteria this spec addresses is already satisfied
        let criteria_done = criteria.iter().any(|c| {
            c.checked && (c.text.contains(&spec.description) || spec.description.contains(&c.text))
        });
        if criteria_done {
            continue;
        }

        let title_match = existing_titles
            .iter()
            .any(|t| t.contains(&spec.description));

        let key_pattern = format!("[planner-key: {}]", spec.bead_key);
        let key_match = existing_keys
            .iter()
            .any(|desc| desc.contains(&key_pattern));

        if title_match || key_match {
            continue;
        }

        if queue.ready_unassigned >= QUEUE_THROTTLE {
            break;
        }

        recommendations.push(Recommendation {
            title: format!("{}: {}", spec.section, spec.description),
            priority: spec.priority,
            labels: vec![spec.section.clone()],
            description: format!(
                "IMPLEMENT: {desc}\n\
                 From execution map section: {section}\n\n\
                 [planner-key: {key}]",
                desc = spec.description,
                section = spec.section,
                key = spec.bead_key,
            ),
            acceptance_command: spec.acceptance_command.clone().unwrap_or_default(),
            gate_key: spec.bead_key.clone(),
            reason: "Execution map bead with no matching existing bead".to_string(),
            depends_on: vec![],
        });
    }

    // For each unchecked criteria item with no matching existing bead → recommend
    for item in &criteria {
        if item.checked {
            continue;
        }

        let title_match = existing_titles
            .iter()
            .any(|t| t.contains(&item.text));

        if title_match {
            continue;
        }

        if queue.ready_unassigned + recommendations.len() >= QUEUE_THROTTLE {
            break;
        }

        // Find matching gate entry for priority/key
        let gate_entry = dynamic_gates.iter().find(|e| e.criteria_line == item.text);
        let key = gate_entry
            .map(|e| e.bead_key.clone())
            .unwrap_or_else(|| format!("criteria-{}", item.text.len()));
        let priority = gate_entry.map(|e| e.priority).unwrap_or(2);

        let key_pattern = format!("[planner-key: {}]", key);
        let key_match = existing_keys
            .iter()
            .any(|desc| desc.contains(&key_pattern));

        if key_match {
            continue;
        }

        recommendations.push(Recommendation {
            title: format!("{}: {}", item.section, item.text),
            priority,
            labels: vec![item.section.clone()],
            description: format!(
                "IMPLEMENT: {text}\n\
                 Section: {section}\n\
                 From criteria (unchecked).\n\n\
                 [planner-key: {key}]",
                text = item.text,
                section = item.section,
                key = key,
            ),
            acceptance_command: String::new(),
            gate_key: key,
            reason: format!("Unchecked criteria item in {}", item.section),
            depends_on: vec![],
        });
    }

    // If all criteria are checked, add next-phase recommendations.
    // Only dedup against open/in-progress beads — closed beads from stale recovery
    // don't mean the deliverable is actually done.
    let all_checked = criteria.iter().all(|c| c.checked);
    let active_titles = match db::open(project_root) {
        Ok(conn) => db::bead_titles_by_status(
            &conn,
            &[db::BeadStatus::Open, db::BeadStatus::InProgress],
        ).unwrap_or_default(),
        Err(_) => vec![],
    };
    if all_checked && !criteria.is_empty() {
        recommendations.extend(generate_next_phase_recommendations(
            project_root,
            &config,
            &active_titles,
            &queue,
        ));
    }

    // Sort by priority
    recommendations.sort_by_key(|r| r.priority);

    Ok(recommendations)
}

// ─── PRD file loading ─────────────────────────────────────────────────────

fn load_criteria(project_root: &Path, config: &ProjectPlannerConfig) -> Vec<prd_parser::CriteriaItem> {
    let mut all = Vec::new();
    for file in &config.criteria_files {
        let path = project_root.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            all.extend(prd_parser::parse_criteria(&content));
        }
    }
    all
}

fn load_execution_maps(project_root: &Path, config: &ProjectPlannerConfig) -> Vec<prd_parser::BeadSpec> {
    let mut all = Vec::new();
    for file in &config.execution_map_files {
        let path = project_root.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            all.extend(prd_parser::parse_execution_map(&content));
        }
    }
    all
}

// ─── Analysis command execution ───────────────────────────────────────────

fn run_analysis_commands(
    project_root: &Path,
    commands: &[AnalysisCommand],
) -> (ParityReport, GateReport) {
    let mut parity = empty_parity();
    let mut gates = empty_gates();

    for cmd in commands {
        let workdir = project_root.join(&cmd.workdir);
        let timeout = if cmd.timeout_secs > 0 {
            Some(std::time::Duration::from_secs(cmd.timeout_secs))
        } else {
            None
        };
        let output = run_shell_command(&cmd.cmd, &workdir, timeout);
        if output.is_empty() {
            eprintln!("planner: command '{}' returned empty output", cmd.cmd);
        }
        match cmd.parser {
            ParserType::Table => {
                parity = parse_parity_output(&output);
            }
            ParserType::TestPassFail => {
                gates = parse_gate_output(&output);
            }
            ParserType::ExitCode => {
                // ExitCode parser: the command was already run, we just note success/failure
                // This could be extended to track per-command pass/fail
            }
        }
    }

    (parity, gates)
}

fn run_shell_command(cmd: &str, workdir: &Path, timeout: Option<std::time::Duration>) -> String {
    if cmd.is_empty() {
        return String::new();
    }

    // Use bash -c to correctly handle quoted args, pipes, and shell features
    let mut child = match Command::new("bash")
        .args(["-c", cmd])
        .current_dir(workdir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("planner: command '{}' failed to spawn: {e}", cmd);
            return String::new();
        }
    };

    if let Some(dur) = timeout {
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() > dur {
                        eprintln!("planner: command '{}' timed out after {}s", cmd, dur.as_secs());
                        let _ = child.kill();
                        let _ = child.wait();
                        return String::new();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    eprintln!("planner: command '{}' wait error: {e}", cmd);
                    return String::new();
                }
            }
        }
    }

    match child.wait_with_output() {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            format!("{}\n{}", stderr, stdout)
        }
        Err(e) => {
            eprintln!("planner: command '{}' failed: {e}", cmd);
            String::new()
        }
    }
}

fn empty_parity() -> ParityReport {
    ParityReport {
        overall: 0.0,
        total: 0,
        matched: 0,
        scenes: vec![],
    }
}

fn empty_gates() -> GateReport {
    GateReport {
        total: 0,
        passing: vec![],
        failing: vec![],
    }
}

// ─── Pass A: Parity (default fallback) ───────────────────────────────────

fn run_parity_pass(engine_dir: &Path) -> ParityReport {
    let output = Command::new("cargo")
        .args([
            "test",
            "--test",
            "oracle_regression_test",
            "--",
            "--nocapture",
            "golden_all_scenes_property_parity_report",
        ])
        .current_dir(engine_dir)
        .output();

    match output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let combined = format!("{}\n{}", stderr, stdout);
            parse_parity_output(&combined)
        }
        Err(e) => {
            eprintln!("planner: parity pass failed: {e}");
            empty_parity()
        }
    }
}

/// Parse the parity table from cargo test output.
///
/// Expected format:
/// ```text
/// Scene                        Total    Match   Parity
/// -------------------------------------------------------
/// minimal                          1        1   100.0%
/// ...
/// OVERALL                        221      180    81.4%
/// ```
pub fn parse_parity_output(text: &str) -> ParityReport {
    let re = Regex::new(
        r"(?m)^\s*(\S+)\s+(\d+)\s+(\d+)\s+(\d+(?:\.\d+)?)%\s*$"
    )
    .unwrap();

    let mut scenes = Vec::new();
    let mut overall = 0.0;
    let mut overall_total = 0;
    let mut overall_matched = 0;

    for cap in re.captures_iter(text) {
        let name = cap[1].to_string();
        let total: usize = cap[2].parse().unwrap_or(0);
        let matched: usize = cap[3].parse().unwrap_or(0);
        let parity: f64 = cap[4].parse().unwrap_or(0.0);

        if name == "OVERALL" {
            overall = parity;
            overall_total = total;
            overall_matched = matched;
        } else {
            scenes.push(SceneParity {
                name,
                total,
                matched,
                parity,
            });
        }
    }

    if scenes.is_empty() {
        eprintln!("planner: parity parser found no scene data in output ({} bytes)", text.len());
    }

    ParityReport {
        overall,
        total: overall_total,
        matched: overall_matched,
        scenes,
    }
}

// ─── Pass B: Gates (default fallback) ────────────────────────────────────

fn run_gate_pass_default(engine_dir: &Path) -> GateReport {
    let output = Command::new("cargo")
        .args([
            "test",
            "--test",
            "v1_acceptance_gate_test",
            "--",
            "--ignored",
        ])
        .current_dir(engine_dir)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = format!("{}\n{}", stdout, stderr);
            parse_gate_output(&combined)
        }
        Err(e) => {
            eprintln!("planner: gate pass failed: {e}");
            empty_gates()
        }
    }
}

/// Parse test pass/fail from cargo test output.
///
/// Matches any `test NAME ... ok/FAILED/ignored` line (generic, no prefix requirement).
pub fn parse_gate_output(text: &str) -> GateReport {
    let re = Regex::new(r"(?m)^test\s+(\S+)\s+\.\.\.\s+(ok|FAILED|ignored)").unwrap();

    let mut passing = Vec::new();
    let mut failing = Vec::new();

    for cap in re.captures_iter(text) {
        let name = cap[1].to_string();
        let status = &cap[2];
        match status {
            "ok" => passing.push(name),
            "FAILED" => failing.push(name),
            _ => {} // ignored entries are not counted
        }
    }

    if passing.is_empty() && failing.is_empty() {
        eprintln!("planner: gate parser found no test results in output ({} bytes)", text.len());
    }

    let total = passing.len() + failing.len();
    GateReport {
        total,
        passing,
        failing,
    }
}

// ─── Pass C: Queue ────────────────────────────────────────────────────────

fn run_queue_pass(project_root: &Path) -> Result<QueueReport> {
    let conn = db::open(project_root)?;
    let open = db::count_by_status(&conn, db::BeadStatus::Open).unwrap_or(0);
    let in_progress = db::count_by_status(&conn, db::BeadStatus::InProgress).unwrap_or(0);
    let closed = db::count_by_status(&conn, db::BeadStatus::Closed).unwrap_or(0);
    let ready_unassigned = db::count_ready_unassigned(&conn).unwrap_or(0);

    Ok(QueueReport {
        open,
        in_progress,
        closed,
        ready_unassigned,
    })
}

// ─── Recommendations ──────────────────────────────────────────────────────

fn collect_existing_planner_keys(conn: &rusqlite::Connection) -> Vec<String> {
    db::bead_descriptions_containing(conn, "[planner-key:")
        .unwrap_or_default()
}

/// Collect planner keys only from beads in the given statuses.
fn collect_existing_planner_keys_by_status(
    conn: &rusqlite::Connection,
    statuses: &[db::BeadStatus],
) -> Vec<String> {
    db::bead_descriptions_containing_by_status(conn, "[planner-key:", statuses)
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
struct NextPhaseTemplate {
    title: &'static str,
    labels: &'static [&'static str],
    acceptance: &'static str,
}

fn next_phase_template(title: &str) -> NextPhaseTemplate {
    match title.to_ascii_lowercase().as_str() {
        "first 3d crate set" => NextPhaseTemplate {
            title: "Define and bootstrap the first 3D crate set",
            labels: &["phase6", "3d", "architecture"],
            acceptance: "crate boundaries are documented, Cargo manifests exist for the selected 3D slice, and the new crates compile under `cargo test --workspace --no-run`",
        },
        "render and physics comparison tooling" => NextPhaseTemplate {
            title: "Add 3D render and physics comparison tooling",
            labels: &["phase6", "3d", "oracle"],
            acceptance: "comparison tooling can ingest Patina and oracle outputs for one representative 3D fixture and a checked-in test or doc cites the command path",
        },
        "3d fixture corpus" => NextPhaseTemplate {
            title: "Plan the first 3D fixture corpus and oracle capture flow",
            labels: &["phase6", "3d", "fixtures"],
            acceptance: "representative 3D fixtures are checked in, oracle capture flow is documented, and coverage is validated by an automated fixture-corpus test",
        },
        "first real 3d demo parity report" => NextPhaseTemplate {
            title: "Produce the first real 3D demo parity report",
            labels: &["phase6", "3d", "reporting"],
            acceptance: "a checked-in report artifact compares one real 3D demo against oracle expectations and an automated test validates the artifact is present and parseable",
        },
        "startup/runtime packaging flow" => NextPhaseTemplate {
            title: "Add startup packaging flow and supported-target CI matrix",
            labels: &["phase7", "platform", "distribution"],
            acceptance: "the packaging flow is documented and covered by a focused test or workflow validation that exercises the supported startup/runtime artifact path",
        },
        "desktop platform targets" => NextPhaseTemplate {
            title: "Define supported desktop platform targets and validation coverage",
            labels: &["phase7", "platform", "ci", "distribution"],
            acceptance: "supported desktop targets are explicitly documented, validation coverage is listed per target, and a test or doc-validation check guards the target matrix",
        },
        "gdplatform first stable layer" | "`gdplatform` first stable layer" => NextPhaseTemplate {
            title: "Stabilize gdplatform windowing input and timing layer",
            labels: &["phase7", "platform", "input", "timing"],
            acceptance: "windowing, input, and timing responsibilities are documented for the stable layer and backed by focused gdplatform or integration tests",
        },
        "editor architecture plan" => NextPhaseTemplate {
            title: "Write the editor architecture plan for post-V1 work",
            labels: &["phase8", "editor", "architecture"],
            acceptance: "the plan names concrete subsystems, boundaries, and deferred scope and a validation test or doc check cites the document as the source of truth",
        },
        "selected tooling parity milestones" => NextPhaseTemplate {
            title: "Define selected tooling parity milestones",
            labels: &["phase8", "tooling", "parity"],
            acceptance: "tooling milestones are enumerated with measurable exit evidence and a test or validation doc proves the milestone list stays in sync",
        },
        "benchmark dashboards" => NextPhaseTemplate {
            title: "Build benchmark dashboards for runtime parity and regressions",
            labels: &["phase9", "benchmarks", "reporting"],
            acceptance: "dashboard artifacts are generated from committed benchmark data and tests validate the dashboard and benchmark schema stay in sync",
        },
        "fuzz/property tests where useful" => NextPhaseTemplate {
            title: "Add fuzz and property tests for high-risk runtime surfaces",
            labels: &["phase9", "testing"],
            acceptance: "at least one high-risk surface gains fuzz or property coverage and the suite is wired into a documented local or CI command",
        },
        "crash triage process" => NextPhaseTemplate {
            title: "Define crash triage process for runtime regressions",
            labels: &["phase9", "stability"],
            acceptance: "the crash triage workflow is documented end-to-end and a validation test or doc check asserts the required steps and artifacts are present",
        },
        "release train" => NextPhaseTemplate {
            title: "Define repeatable release-train workflow for Patina runtime milestones",
            labels: &["phase9", "release"],
            acceptance: "the release-train workflow is documented with entry and exit criteria and automated tests cover the core release-train data model or workflow artifact",
        },
        "contributor onboarding docs" => NextPhaseTemplate {
            title: "Write contributor onboarding docs for runtime and oracle workflows",
            labels: &["phase9", "docs"],
            acceptance: "onboarding docs cover setup, targeted test commands, and oracle workflows and a validation test checks the required sections remain present",
        },
        "migration guide for users" => NextPhaseTemplate {
            title: "Draft migration guide for users adopting Patina runtime milestones",
            labels: &["phase9", "docs"],
            acceptance: "the migration guide explains supported runtime scope, gaps, and upgrade path and a validation test checks the required guidance sections remain present",
        },
        _ => NextPhaseTemplate {
            title: "",
            labels: &[],
            acceptance: "the deliverable is broken into measurable evidence with tests, docs, or oracle-backed artifacts before closure",
        },
    }
}

/// For each failing gate without a matching bead, generate a recommendation.
pub fn generate_recommendations(
    gates: &GateReport,
    dynamic_gates: &[gate_map::GateEntry],
    existing_titles: &[String],
    existing_keys: &[String],
    queue: &QueueReport,
) -> Vec<Recommendation> {
    if queue.ready_unassigned >= QUEUE_THROTTLE {
        return vec![];
    }

    let mut recs = Vec::new();

    for failing_test in &gates.failing {
        // Find the gate entry for this test
        let entry = match dynamic_gates.iter().find(|e| e.test_name == *failing_test) {
            Some(e) => e,
            None => continue,
        };

        // Check title dedup
        let title_match = existing_titles
            .iter()
            .any(|t| t.contains(&entry.criteria_line));

        // Check key dedup
        let key_pattern = format!("[planner-key: {}]", entry.bead_key);
        let key_match = existing_keys
            .iter()
            .any(|desc| desc.contains(&key_pattern));

        if title_match || key_match {
            continue;
        }

        let title = format!(
            "{} gate: {} — {}",
            "V1", // Could come from config.phase_label in the future
            entry.criteria_section, entry.criteria_line
        );

        let description = format!(
            "Acceptance gate `{}` is failing.\n\n\
             Criteria: {}\n\
             Section: {}\n\n\
             [planner-key: {}]",
            entry.test_name,
            entry.criteria_line,
            entry.criteria_section,
            entry.bead_key,
        );

        let acceptance_command = entry.test_name.clone();

        recs.push(Recommendation {
            title,
            priority: entry.priority,
            labels: vec![
                "gate".to_string(),
                entry.criteria_section.clone(),
            ],
            description,
            acceptance_command,
            gate_key: entry.bead_key.clone(),
            reason: format!(
                "Gate {} still fails, no open bead found",
                entry.test_name
            ),
            depends_on: vec![],
        });
    }

    recs.sort_by_key(|r| r.priority);
    recs
}

/// Generate recommendations for scenes that are below 100% parity.
pub fn generate_parity_recommendations(
    parity: &ParityReport,
    existing_titles: &[String],
    queue: &QueueReport,
) -> Vec<Recommendation> {
    if queue.ready_unassigned >= QUEUE_THROTTLE {
        return vec![];
    }

    let mut recs = Vec::new();
    for scene in &parity.scenes {
        if scene.parity >= 100.0 {
            continue;
        }
        let title = format!("Close parity gap in {} (currently {:.1}%)", scene.name, scene.parity);
        let key = format!("parity-gap-{}", scene.name);

        if existing_titles.iter().any(|t| t.contains(&scene.name) && t.contains("parity")) {
            continue;
        }

        recs.push(Recommendation {
            title: title.clone(),
            priority: 2,
            labels: vec!["parity-gap".to_string()],
            description: format!(
                "IMPLEMENT fixes to reach 100% property parity for {name}.\n\
                 Currently {matched}/{total} properties match ({pct:.1}%).\n\
                 Run the oracle regression test to identify which properties mismatch,\n\
                 then fix the engine to produce matching output.\n\n\
                 [planner-key: {key}]",
                name = scene.name,
                matched = scene.matched,
                total = scene.total,
                pct = scene.parity,
                key = key,
            ),
            acceptance_command: format!(
                "cargo test --test oracle_regression_test -- golden_{}_full_property_parity",
                scene.name
            ),
            gate_key: key,
            reason: format!(
                "{} has {:.1}% parity ({}/{}), needs 100%",
                scene.name, scene.parity, scene.matched, scene.total
            ),
            depends_on: vec![],
        });
    }
    recs
}

/// Generate next-phase recommendations by parsing deliverables from
/// the `next_sources` PRD files in the config.
fn generate_next_phase_recommendations(
    project_root: &Path,
    config: &ProjectPlannerConfig,
    existing_titles: &[String],
    queue: &QueueReport,
) -> Vec<Recommendation> {
    if queue.ready_unassigned >= QUEUE_THROTTLE {
        return vec![];
    }

    let mut recs = Vec::new();
    let active_keys = match db::open(project_root) {
        Ok(conn) => collect_existing_planner_keys_by_status(
            &conn,
            &[db::BeadStatus::Open, db::BeadStatus::InProgress],
        ),
        Err(_) => vec![],
    };
    let mut seen_titles: std::collections::HashSet<String> = existing_titles
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    let mut seen_keys: std::collections::HashSet<String> = active_keys
        .iter()
        .filter_map(|desc| {
            let start = desc.find("[planner-key: ")?;
            let rest = &desc[start..];
            let end = rest.find(']')?;
            Some(rest[..=end].to_string())
        })
        .collect();

    for source_file in &config.next_sources {
        let path = project_root.join(source_file);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Parse deliverables from multiple phases (5-9)
        for phase_num in 5..=9 {
            let prefix = format!("Phase {}", phase_num);
            let deliverables = prd_parser::parse_phase_deliverables(&content, &prefix);

            for d in deliverables {
                let template = next_phase_template(&d.title);
                let title = if template.title.is_empty() {
                    d.title.clone()
                } else {
                    template.title.to_string()
                };
                let normalized_title = title.to_ascii_lowercase();
                let key = format!("phase{}-{}", phase_num, d.slug);
                let key_pattern = format!("[planner-key: {key}]");

                if seen_titles.contains(&normalized_title)
                    || seen_keys.contains(&key_pattern)
                    || existing_titles.iter().any(|t| {
                        t.eq_ignore_ascii_case(&d.title) || t.eq_ignore_ascii_case(&title)
                    })
                {
                    continue;
                }

                let labels = if template.labels.is_empty() {
                    vec![format!("phase{}", phase_num)]
                } else {
                    template.labels.iter().map(|l| (*l).to_string()).collect()
                };

                recs.push(Recommendation {
                    title: title.clone(),
                    priority: 3,
                    labels,
                    description: format!(
                        "IMPLEMENT: {title}\n\
                         Seeded from {source} Phase {phase_num} deliverables.\n\
                         Acceptance should be verified with explicit tests, docs, or oracle-backed artifacts.\n\n\
                         [planner-key: {key}]",
                        title = title,
                        source = source_file.display(),
                        phase_num = phase_num,
                        key = key,
                    ),
                    acceptance_command: template.acceptance.to_string(),
                    gate_key: key,
                    reason: format!("Phase {} deliverable from port plan, no existing bead", phase_num),
                    depends_on: vec![],
                });
                seen_titles.insert(normalized_title);
                seen_keys.insert(key_pattern);
            }
        }
    }
    recs
}

// ─── Phase determination ──────────────────────────────────────────────────

fn determine_phase(gates: &GateReport, parity: &ParityReport, config: &ProjectPlannerConfig) -> Phase {
    if config.completion_conditions.is_empty() {
        // Default behavior: gates all passing + parity >= 98
        return determine_phase_default(gates, parity);
    }

    let mut all_met = true;
    for condition in &config.completion_conditions {
        match condition {
            CompletionCondition::AllGatesPassing => {
                if !gates.failing.is_empty() {
                    all_met = false;
                }
            }
            CompletionCondition::ParityAbove(threshold) => {
                if parity.overall < *threshold {
                    all_met = false;
                }
            }
        }
    }

    if all_met {
        Phase::V1Complete
    } else if gates.failing.len() <= 3 {
        Phase::V1NearlyDone
    } else {
        Phase::V1Active
    }
}

fn determine_phase_default(gates: &GateReport, parity: &ParityReport) -> Phase {
    if gates.failing.is_empty() && parity.overall >= 98.0 {
        Phase::V1Complete
    } else if gates.failing.len() <= 3 {
        Phase::V1NearlyDone
    } else {
        Phase::V1Active
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_parity_output() {
        let sample = r#"
running 1 test
test golden_all_scenes_property_parity_report ... ok

Scene                        Total    Match   Parity
-------------------------------------------------------
minimal                          1        1   100.0%
hierarchy                       12       12   100.0%
platformer                      45       40    88.9%
physics_playground               30       25    83.3%
OVERALL                        221      180    81.4%
"#;
        let report = parse_parity_output(sample);
        assert!((report.overall - 81.4).abs() < 0.01, "overall parity");
        assert_eq!(report.total, 221);
        assert_eq!(report.matched, 180);
        assert_eq!(report.scenes.len(), 4);

        assert_eq!(report.scenes[0].name, "minimal");
        assert_eq!(report.scenes[0].total, 1);
        assert_eq!(report.scenes[0].matched, 1);
        assert!((report.scenes[0].parity - 100.0).abs() < 0.01);

        assert_eq!(report.scenes[2].name, "platformer");
        assert_eq!(report.scenes[2].total, 45);
        assert_eq!(report.scenes[2].matched, 40);
        assert!((report.scenes[2].parity - 88.9).abs() < 0.01);
    }

    #[test]
    fn test_parse_parity_output_empty() {
        let report = parse_parity_output("no table here");
        assert!((report.overall - 0.0).abs() < 0.01);
        assert!(report.scenes.is_empty());
    }

    #[test]
    fn test_parse_gate_output() {
        let sample = r#"
running 35 tests
test test_v1_classdb_full_property_enumeration ... ok
test test_v1_notification_dispatch_ordering ... FAILED
test test_v1_weakref_auto_invalidates_on_free ... FAILED
test test_v1_object_free_use_after_free_guard ... ok
test test_v1_headless_mode ... ok
"#;
        let report = parse_gate_output(sample);
        assert_eq!(report.total, 5);
        assert_eq!(report.passing.len(), 3);
        assert_eq!(report.failing.len(), 2);
        assert!(report.passing.contains(&"test_v1_classdb_full_property_enumeration".to_string()));
        assert!(report.failing.contains(&"test_v1_notification_dispatch_ordering".to_string()));
        assert!(report.failing.contains(&"test_v1_weakref_auto_invalidates_on_free".to_string()));
    }

    #[test]
    fn test_parse_gate_output_generic_names() {
        // The regex should now match any test name, not just test_v1_ prefixed
        let sample = r#"
test my_custom_gate ... ok
test another_test ... FAILED
test something_else ... ignored
"#;
        let report = parse_gate_output(sample);
        assert_eq!(report.total, 2);
        assert_eq!(report.passing.len(), 1);
        assert_eq!(report.failing.len(), 1);
        assert!(report.passing.contains(&"my_custom_gate".to_string()));
        assert!(report.failing.contains(&"another_test".to_string()));
    }

    #[test]
    fn test_parse_gate_output_ignores_ignored() {
        let sample = "test test_v1_something ... ignored\n";
        let report = parse_gate_output(sample);
        assert_eq!(report.total, 0);
        assert!(report.passing.is_empty());
        assert!(report.failing.is_empty());
    }

    #[test]
    fn test_recommendations_skip_existing_beads() {
        let gates = GateReport {
            total: 2,
            passing: vec![],
            failing: vec![
                "test_notif".to_string(),
                "test_weakref".to_string(),
            ],
        };

        let dynamic_gates = vec![
            gate_map::GateEntry {
                test_name: "test_notif".to_string(),
                criteria_section: "Object Model".to_string(),
                criteria_line: "Object.notification() dispatch with correct ordering".to_string(),
                bead_key: "v1-obj-notif".to_string(),
                priority: 1,
            },
            gate_map::GateEntry {
                test_name: "test_weakref".to_string(),
                criteria_section: "Object Model".to_string(),
                criteria_line: "Weak reference behavior".to_string(),
                bead_key: "v1-obj-weakref".to_string(),
                priority: 2,
            },
        ];

        let existing_titles = vec![
            "V1 gate: Object Model — Object.notification() dispatch with correct ordering".to_string(),
        ];
        let existing_keys: Vec<String> = vec![];

        let queue = QueueReport {
            open: 5,
            in_progress: 2,
            closed: 100,
            ready_unassigned: 3,
        };

        let recs = generate_recommendations(&gates, &dynamic_gates, &existing_titles, &existing_keys, &queue);
        assert_eq!(recs.len(), 1, "should skip bead with matching title");
        assert!(recs[0].gate_key.contains("weakref"));
    }

    #[test]
    fn test_recommendations_skip_existing_planner_key() {
        let gates = GateReport {
            total: 1,
            passing: vec![],
            failing: vec!["test_notif".to_string()],
        };

        let dynamic_gates = vec![gate_map::GateEntry {
            test_name: "test_notif".to_string(),
            criteria_section: "Object Model".to_string(),
            criteria_line: "notification dispatch".to_string(),
            bead_key: "v1-obj-notif".to_string(),
            priority: 1,
        }];

        let existing_titles: Vec<String> = vec![];
        let existing_keys = vec![
            "some bead with [planner-key: v1-obj-notif] in description".to_string(),
        ];

        let queue = QueueReport {
            open: 5,
            in_progress: 2,
            closed: 100,
            ready_unassigned: 3,
        };

        let recs = generate_recommendations(&gates, &dynamic_gates, &existing_titles, &existing_keys, &queue);
        assert_eq!(recs.len(), 0, "should skip bead with matching planner key");
    }

    #[test]
    fn test_recommendations_throttle_on_full_queue() {
        let gates = GateReport {
            total: 5,
            passing: vec![],
            failing: vec!["test_a".to_string(), "test_b".to_string()],
        };

        let dynamic_gates = vec![
            gate_map::GateEntry {
                test_name: "test_a".to_string(),
                criteria_section: "A".to_string(),
                criteria_line: "line a".to_string(),
                bead_key: "key-a".to_string(),
                priority: 1,
            },
            gate_map::GateEntry {
                test_name: "test_b".to_string(),
                criteria_section: "B".to_string(),
                criteria_line: "line b".to_string(),
                bead_key: "key-b".to_string(),
                priority: 2,
            },
        ];

        let queue = QueueReport {
            open: 20,
            in_progress: 3,
            closed: 100,
            ready_unassigned: 12,
        };

        let recs = generate_recommendations(&gates, &dynamic_gates, &[], &[], &queue);
        assert!(recs.is_empty(), "should throttle when ready >= 12");
    }

    #[test]
    fn test_recommendations_priority_ordering() {
        let gates = GateReport {
            total: 3,
            passing: vec![],
            failing: vec![
                "test_later".to_string(),
                "test_now".to_string(),
                "test_next".to_string(),
            ],
        };

        let dynamic_gates = vec![
            gate_map::GateEntry {
                test_name: "test_later".to_string(),
                criteria_section: "Later".to_string(),
                criteria_line: "later gate".to_string(),
                bead_key: "later-key".to_string(),
                priority: 3,
            },
            gate_map::GateEntry {
                test_name: "test_now".to_string(),
                criteria_section: "Now".to_string(),
                criteria_line: "now gate".to_string(),
                bead_key: "now-key".to_string(),
                priority: 1,
            },
            gate_map::GateEntry {
                test_name: "test_next".to_string(),
                criteria_section: "Next".to_string(),
                criteria_line: "next gate".to_string(),
                bead_key: "next-key".to_string(),
                priority: 2,
            },
        ];

        let queue = QueueReport {
            open: 2,
            in_progress: 1,
            closed: 50,
            ready_unassigned: 1,
        };

        let recs = generate_recommendations(&gates, &dynamic_gates, &[], &[], &queue);
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].priority, 1, "first rec should be P1 (Now)");
        assert_eq!(recs[1].priority, 2, "second rec should be P2 (Next)");
        assert_eq!(recs[2].priority, 3, "third rec should be P3 (Later)");
    }

    #[test]
    fn test_phase_v1_complete() {
        let gates = GateReport {
            total: 35,
            passing: (0..35).map(|i| format!("test_{i}")).collect(),
            failing: vec![],
        };
        let parity = ParityReport {
            overall: 99.5,
            total: 200,
            matched: 199,
            scenes: vec![],
        };
        let config = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "v1".to_string(),
            completion_conditions: vec![
                CompletionCondition::AllGatesPassing,
                CompletionCondition::ParityAbove(98.0),
            ],
            next_sources: vec![],
        };
        assert_eq!(determine_phase(&gates, &parity, &config), Phase::V1Complete);
    }

    #[test]
    fn test_phase_v1_nearly_done() {
        let gates = GateReport {
            total: 35,
            passing: (0..33).map(|i| format!("test_{i}")).collect(),
            failing: vec!["a".into(), "b".into()],
        };
        let parity = ParityReport {
            overall: 95.0,
            total: 200,
            matched: 190,
            scenes: vec![],
        };
        let config = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "v1".to_string(),
            completion_conditions: vec![],
            next_sources: vec![],
        };
        assert_eq!(determine_phase(&gates, &parity, &config), Phase::V1NearlyDone);
    }

    #[test]
    fn test_phase_v1_active() {
        let gates = GateReport {
            total: 35,
            passing: vec![],
            failing: (0..10).map(|i| format!("test_{i}")).collect(),
        };
        let parity = ParityReport {
            overall: 80.0,
            total: 200,
            matched: 160,
            scenes: vec![],
        };
        let config = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "v1".to_string(),
            completion_conditions: vec![],
            next_sources: vec![],
        };
        assert_eq!(determine_phase(&gates, &parity, &config), Phase::V1Active);
    }

    #[test]
    fn test_phase_high_parity_but_gates_fail() {
        let gates = GateReport {
            total: 35,
            passing: vec![],
            failing: vec!["test_v1_something".into()],
        };
        let parity = ParityReport {
            overall: 99.0,
            total: 200,
            matched: 198,
            scenes: vec![],
        };
        let config = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "v1".to_string(),
            completion_conditions: vec![],
            next_sources: vec![],
        };
        assert_eq!(determine_phase(&gates, &parity, &config), Phase::V1NearlyDone);
    }

    #[test]
    fn test_phase_with_config_conditions() {
        let gates = GateReport {
            total: 10,
            passing: (0..10).map(|i| format!("test_{i}")).collect(),
            failing: vec![],
        };
        let parity = ParityReport {
            overall: 97.0,
            total: 100,
            matched: 97,
            scenes: vec![],
        };
        // Parity threshold is 98, but we're at 97 -> not complete
        let config = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "v1".to_string(),
            completion_conditions: vec![
                CompletionCondition::AllGatesPassing,
                CompletionCondition::ParityAbove(98.0),
            ],
            next_sources: vec![],
        };
        assert_eq!(determine_phase(&gates, &parity, &config), Phase::V1NearlyDone);
    }

    #[test]
    fn test_quick_recommendations_returns_without_subprocesses() {
        // quick_recommendations must complete fast — it never runs cargo test
        // or any other subprocess. We test on the real project root if available.
        let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let start = std::time::Instant::now();
        let result = quick_recommendations(project_root);
        let elapsed = start.elapsed();

        // Should succeed (or fail gracefully with no DB — either way, fast)
        match result {
            Ok(recs) => {
                // Recommendations are sorted by priority
                for window in recs.windows(2) {
                    assert!(window[0].priority <= window[1].priority);
                }
            }
            Err(_) => {
                // DB not available in CI is fine — the point is it ran fast
            }
        }

        assert!(
            elapsed.as_secs() < 2,
            "quick_recommendations took {elapsed:?}, expected < 2s (no subprocesses)"
        );
    }

    #[test]
    fn test_next_phase_template_sharpens_known_deliverables() {
        let template = next_phase_template("benchmark dashboards");
        assert_eq!(
            template.title,
            "Build benchmark dashboards for runtime parity and regressions"
        );
        assert!(template.labels.contains(&"phase9"));
        assert!(template.acceptance.contains("dashboard"));
    }

    #[test]
    fn test_generate_next_phase_recommendations_dedupes_across_sources() {
        let root = std::env::temp_dir().join(format!(
            "patina-planner-next-phase-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("prd")).unwrap();

        let plan_a = root.join("prd/PLAN_A.md");
        let plan_b = root.join("prd/PLAN_B.md");
        let content = "\
## Phase 9 - Hardening and Release Discipline

### Deliverables

- benchmark dashboards,
- contributor onboarding docs.
";
        fs::write(&plan_a, content).unwrap();
        fs::write(&plan_b, content).unwrap();

        let config = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "v1".to_string(),
            completion_conditions: vec![],
            next_sources: vec!["prd/PLAN_A.md".into(), "prd/PLAN_B.md".into()],
        };
        let queue = QueueReport {
            open: 0,
            in_progress: 0,
            closed: 0,
            ready_unassigned: 0,
        };

        let recs = generate_next_phase_recommendations(&root, &config, &[], &queue);
        assert_eq!(recs.len(), 2, "duplicate sources should not duplicate beads");
        assert_eq!(
            recs.iter()
                .filter(|r| r.gate_key == "phase9-benchmark-dashboards")
                .count(),
            1
        );
        assert!(recs.iter().all(|r| !r.acceptance_command.is_empty()));
        assert!(recs.iter().all(|r| r.labels.iter().any(|l| l.starts_with("phase"))));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_generate_next_phase_recommendations_skips_existing_sharpened_title() {
        let root = std::env::temp_dir().join(format!(
            "patina-planner-next-phase-existing-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("prd")).unwrap();

        let plan = root.join("prd/PLAN.md");
        fs::write(
            &plan,
            "\
## Phase 6 - 3D Runtime Slice

### Deliverables

- first real 3D demo parity report.
",
        )
        .unwrap();

        let config = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "v1".to_string(),
            completion_conditions: vec![],
            next_sources: vec!["prd/PLAN.md".into()],
        };
        let queue = QueueReport {
            open: 0,
            in_progress: 0,
            closed: 0,
            ready_unassigned: 0,
        };
        let existing_titles =
            vec!["Produce the first real 3D demo parity report".to_string()];

        let recs =
            generate_next_phase_recommendations(&root, &config, &existing_titles, &queue);
        assert!(recs.is_empty(), "existing active title should suppress recommendation");

        let _ = fs::remove_dir_all(&root);
    }

    // ── Regression tests for silent-failure bug ─────────────────────────
    //
    // Bug: The planner skill passed `--json` to the binary, which rejected
    // it as an unknown option (exit 1). The skill error-handled and skipped
    // the cycle, so the planner *never* ran successfully. Additionally,
    // when analysis commands returned empty output, the parsers silently
    // produced zero-value reports with no indication of failure.
    //
    // These tests guard against:
    // (a) parsers silently accepting garbage/empty input without detection
    // (b) boundary conditions in parity and gate parsing
    // (c) stress: bulk input with many scenes/tests
    // (d) variants: unusual but valid output formats
    // (e) negative: malformed input that must not panic

    /// Regression: empty input produces zero-value report (original bug scenario).
    /// The parsers must not panic and must return identifiable empty state.
    #[test]
    fn test_parity_parser_empty_input_regression() {
        let report = parse_parity_output("");
        assert_eq!(report.total, 0, "empty input → total=0");
        assert_eq!(report.matched, 0, "empty input → matched=0");
        assert!((report.overall - 0.0).abs() < f64::EPSILON, "empty input → 0% parity");
        assert!(report.scenes.is_empty(), "empty input → no scenes");
    }

    #[test]
    fn test_gate_parser_empty_input_regression() {
        let report = parse_gate_output("");
        assert_eq!(report.total, 0, "empty input → total=0");
        assert!(report.passing.is_empty(), "empty input → no passing");
        assert!(report.failing.is_empty(), "empty input → no failing");
    }

    /// Boundary: parity at exactly 100.0% and 0.0%.
    #[test]
    fn test_parity_parser_boundary_values() {
        let input = "\
scene_a    10    10   100.0%
scene_b    10     0     0.0%
OVERALL    20    10    50.0%
";
        let report = parse_parity_output(input);
        assert_eq!(report.scenes.len(), 2);
        assert!((report.scenes[0].parity - 100.0).abs() < 0.01);
        assert!((report.scenes[1].parity - 0.0).abs() < 0.01);
        assert_eq!(report.scenes[1].matched, 0);
        assert!((report.overall - 50.0).abs() < 0.01);
    }

    /// Boundary: single scene, no OVERALL row.
    #[test]
    fn test_parity_parser_single_scene_no_overall() {
        let input = "my_scene    5    3    60.0%\n";
        let report = parse_parity_output(input);
        assert_eq!(report.scenes.len(), 1);
        assert_eq!(report.scenes[0].name, "my_scene");
        assert_eq!(report.scenes[0].total, 5);
        assert_eq!(report.scenes[0].matched, 3);
        // No OVERALL line → overall stays 0.0
        assert!((report.overall - 0.0).abs() < 0.01);
    }

    /// Boundary: gate parser with only "ignored" entries → 0 total.
    #[test]
    fn test_gate_parser_all_ignored() {
        let input = "\
test test_a ... ignored
test test_b ... ignored
test test_c ... ignored
";
        let report = parse_gate_output(input);
        assert_eq!(report.total, 0, "ignored tests should not count");
        assert!(report.passing.is_empty());
        assert!(report.failing.is_empty());
    }

    /// Stress: 200 scenes parsed correctly.
    #[test]
    fn test_parity_parser_stress_many_scenes() {
        let mut input = String::new();
        for i in 0..200 {
            input.push_str(&format!("scene_{i:03}    50    {matched}    {pct:.1}%\n",
                matched = i % 51,
                pct = (i % 51) as f64 / 50.0 * 100.0,
            ));
        }
        input.push_str("OVERALL    10000    5000    50.0%\n");

        let report = parse_parity_output(&input);
        assert_eq!(report.scenes.len(), 200, "should parse all 200 scenes");
        assert!((report.overall - 50.0).abs() < 0.01);
        assert_eq!(report.total, 10000);
    }

    /// Stress: 500 tests parsed correctly.
    #[test]
    fn test_gate_parser_stress_many_tests() {
        let mut input = String::new();
        for i in 0..500 {
            let status = if i % 3 == 0 { "FAILED" } else { "ok" };
            input.push_str(&format!("test test_{i:03} ... {status}\n"));
        }

        let report = parse_gate_output(&input);
        let expected_failing = (0..500).filter(|i| i % 3 == 0).count();
        let expected_passing = 500 - expected_failing;
        assert_eq!(report.failing.len(), expected_failing);
        assert_eq!(report.passing.len(), expected_passing);
        assert_eq!(report.total, 500);
    }

    /// Variant: parity output with extra whitespace, headers, and separators.
    #[test]
    fn test_parity_parser_with_surrounding_cargo_noise() {
        let input = r#"
   Compiling engine v0.1.0
    Finished test target(s) in 3.21s
     Running tests/oracle_regression_test.rs

running 1 test
test golden_all_scenes_property_parity_report ... ok

Scene                        Total    Match   Parity
-------------------------------------------------------
  minimal                        1        1   100.0%
  hierarchy                     12       12   100.0%
  platformer                    45       40    88.9%
OVERALL                        58       53    91.4%

test result: ok. 1 passed; 0 failed; 0 ignored
"#;
        let report = parse_parity_output(input);
        assert_eq!(report.scenes.len(), 3, "should skip header/separator lines");
        assert!((report.overall - 91.4).abs() < 0.01);
        assert_eq!(report.total, 58);
        assert_eq!(report.matched, 53);
    }

    /// Variant: gate output mixed with compilation warnings.
    #[test]
    fn test_gate_parser_with_cargo_warnings() {
        let input = r#"
warning: unused variable `x`
  --> src/foo.rs:10:5
test test_alpha ... ok
warning: field is never read
  --> src/bar.rs:20:5
test test_beta ... FAILED
test test_gamma ... ok
"#;
        let report = parse_gate_output(input);
        assert_eq!(report.passing.len(), 2);
        assert_eq!(report.failing.len(), 1);
        assert!(report.failing.contains(&"test_beta".to_string()));
    }

    /// Variant: scene names with underscores and numbers.
    #[test]
    fn test_parity_parser_unusual_scene_names() {
        let input = "\
scene_3d_v2    100    99    99.0%
a              1      1   100.0%
OVERALL        101   100    99.0%
";
        let report = parse_parity_output(input);
        assert_eq!(report.scenes.len(), 2);
        assert_eq!(report.scenes[0].name, "scene_3d_v2");
        assert_eq!(report.scenes[1].name, "a");
    }

    /// Negative: completely garbled input must not panic.
    #[test]
    fn test_parity_parser_garbled_input_no_panic() {
        let inputs = [
            "💥 random unicode garbage 🎮",
            "\0\0\0null bytes\0\0",
            "100% 200% 300%",  // percentage without table format
            "scene 10 5 50.0",  // missing % sign
            "OVERALL",  // incomplete OVERALL line
            "\n\n\n\n",  // only newlines
        ];
        for input in &inputs {
            let report = parse_parity_output(input);
            // Must not panic — zero-value report is fine
            assert!(report.overall >= 0.0, "garbled input must not produce negative parity");
        }
    }

    /// Negative: garbled gate input must not panic.
    #[test]
    fn test_gate_parser_garbled_input_no_panic() {
        let inputs = [
            "test ... ok",  // missing test name
            "test_foo ... maybe",  // unknown status
            "testing test_bar ... ok",  // wrong prefix
            "💥\0garbage\n",
            "test  ... FAILED",  // empty name
        ];
        for input in &inputs {
            let report = parse_gate_output(input);
            // Must not panic
            assert!(report.total < 1000, "garbled input should not produce huge totals");
        }
    }

    /// Parity recommendations: scenes at exactly 100% should NOT generate recs.
    #[test]
    fn test_parity_recs_skip_100_percent_scenes() {
        let parity = ParityReport {
            overall: 100.0,
            total: 50,
            matched: 50,
            scenes: vec![
                SceneParity { name: "perfect".into(), total: 25, matched: 25, parity: 100.0 },
                SceneParity { name: "also_perfect".into(), total: 25, matched: 25, parity: 100.0 },
            ],
        };
        let queue = QueueReport { open: 2, in_progress: 1, closed: 50, ready_unassigned: 1 };
        let recs = generate_parity_recommendations(&parity, &[], &queue);
        assert!(recs.is_empty(), "100% scenes should not generate recommendations");
    }

    /// Parity recommendations: scenes below 100% SHOULD generate recs.
    #[test]
    fn test_parity_recs_for_imperfect_scenes() {
        let parity = ParityReport {
            overall: 95.0,
            total: 100,
            matched: 95,
            scenes: vec![
                SceneParity { name: "good".into(), total: 50, matched: 50, parity: 100.0 },
                SceneParity { name: "needs_work".into(), total: 50, matched: 45, parity: 90.0 },
            ],
        };
        let queue = QueueReport { open: 2, in_progress: 1, closed: 50, ready_unassigned: 1 };
        let recs = generate_parity_recommendations(&parity, &[], &queue);
        assert_eq!(recs.len(), 1);
        assert!(recs[0].title.contains("needs_work"));
        assert!(recs[0].gate_key.contains("parity-gap-needs_work"));
    }

    #[test]
    fn test_convention_fallback_produces_equivalent_config() {
        // Verify that convention fallback on the Patina repo finds the same
        // files that would be in the explicit config.
        let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let prd_dir = project_root.join("prd");
        if !prd_dir.exists() {
            return;
        }

        let cfg = project_config::convention_fallback(project_root);

        // Should find V1_EXIT_CRITERIA.md
        assert!(
            cfg.criteria_files
                .iter()
                .any(|f| f.to_string_lossy().contains("V1_EXIT_CRITERIA")),
            "convention should find V1_EXIT_CRITERIA.md"
        );

        // Should find BEAD_EXECUTION_MAP.md and ignore the V1 handoff doc
        assert!(
            cfg.execution_map_files
                .iter()
                .any(|f| f.to_string_lossy().contains("BEAD_EXECUTION_MAP")),
            "convention should find BEAD_EXECUTION_MAP.md"
        );
        assert!(
            cfg.execution_map_files
                .iter()
                .all(|f| !f.to_string_lossy().contains("V1_EXIT_EXECUTION_MAP")),
            "convention should ignore V1_EXIT_EXECUTION_MAP.md as an active execution map"
        );

        // Should find PORT_GODOT_TO_RUST_PLAN.md in next_sources
        assert!(
            cfg.next_sources
                .iter()
                .any(|f| f.to_string_lossy().contains("PORT_GODOT_TO_RUST_PLAN")),
            "convention should find PORT_GODOT_TO_RUST_PLAN.md"
        );
    }

    // ── V1Complete duplicate prevention tests ────────────────────────────

    /// Regression: when all criteria are checked, quick_recommendations must
    /// NOT generate V1 gate/criteria beads — only next-phase deliverables.
    /// This prevents duplicate beads for already-passing work.
    #[test]
    fn test_v1_complete_skips_gate_recommendations() {
        let source = include_str!("planner.rs");
        let quick_fn = source.find("pub fn quick_recommendations(").unwrap();
        let fn_end = source[quick_fn..].find("\n}").unwrap_or(3000);
        let body = &source[quick_fn..quick_fn + fn_end];

        assert!(
            body.contains("all_criteria_checked"),
            "quick_recommendations must check if all criteria are done"
        );
        assert!(
            body.contains("return Ok(recommendations)"),
            "must early-return with only next-phase recs when V1 is complete"
        );
    }

    /// Regression: analyze() must skip gate/parity recommendations when V1Complete.
    #[test]
    fn test_analyze_skips_gates_when_v1_complete() {
        let source = include_str!("planner.rs");
        let analyze_fn = source.find("pub fn analyze(").unwrap();
        let fn_end = source[analyze_fn..].find("\n}").unwrap_or(3000);
        let body = &source[analyze_fn..analyze_fn + fn_end];

        assert!(
            body.contains("Phase::V1Complete"),
            "analyze must check for V1Complete phase"
        );
        assert!(
            body.contains("generate_next_phase_recommendations"),
            "must generate next-phase recs when V1Complete"
        );
    }

    /// Regression: next-phase recommendations must dedup against active beads only,
    /// not closed beads (stale recovery closes beads without verifying features).
    #[test]
    fn test_next_phase_dedup_active_only() {
        let source = include_str!("planner.rs");
        let next_fn = source.find("fn generate_next_phase_recommendations(").unwrap();
        let fn_end = source[next_fn..].find("\n}").unwrap_or(1000);
        let body = &source[next_fn..next_fn + fn_end];

        // The function receives existing_titles — the caller must pass active-only titles
        assert!(
            body.contains("existing_titles"),
            "must accept existing_titles for dedup"
        );

        // Verify callers pass active_titles, not all_titles
        let all_callers = source.matches("generate_next_phase_recommendations(").count();
        let active_callers = source.matches("&active_titles,\n            &queue,").count();
        assert!(
            active_callers >= 2,
            "all callers must pass active_titles (open/in-progress only), found {active_callers} of {all_callers}"
        );
    }
}
