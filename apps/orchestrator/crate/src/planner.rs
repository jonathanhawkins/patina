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
use crate::project_config::{
    self, AnalysisCommand, CompletionCondition, ParserType, ProjectPlannerConfig,
};

// ─── Public types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PlanReport {
    pub timestamp: String,
    pub parity: ParityReport,
    pub gates: GateReport,
    pub queue: QueueReport,
    pub recommendations: Vec<Recommendation>,
    pub phase: Phase,
    /// Label of the phase the planner is currently working on (matches the
    /// `label` field on a `[[phase]]` block in planner.toml). For legacy
    /// single-phase configs this mirrors `config.phase_label`. Surfaces what
    /// the planner is *actually* doing — the `phase` enum above describes
    /// completion state but is still V1-named for backward compatibility.
    #[serde(default)]
    pub active_phase_label: String,
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
    /// V1 (runtime parity) complete. Retained for backward compatibility with
    /// callers that special-case this state. In phase-chain configs this
    /// fires only if the first phase happens to be V1; downstream code
    /// should prefer `active_phase_label` + `AllComplete` for forward logic.
    V1Complete,
    /// Every phase in the phase chain has been resolved (all criteria
    /// checked, gates passed, or otherwise satisfied). Distinct from
    /// `V1Complete` so the planner skill can tell "V1 is done" from "the
    /// entire post-V1 editor/web/agent surface is done".
    AllComplete,
}

// ─── Queue throttle constant ──────────────────────────────────────────────

const QUEUE_THROTTLE: usize = 12;

// ─── Main entry point ─────────────────────────────────────────────────────

/// Run all analysis passes and produce a plan report.
pub fn analyze(project_root: &Path) -> Result<PlanReport> {
    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    // Load project-specific config
    let config = project_config::load(project_root);

    // Phase-chain path: walk phases in order, tick passing criteria, return
    // recommendations from the first incomplete phase.
    if config_uses_phase_chain(&config) {
        return analyze_phase_chain(project_root, &config, &timestamp);
    }

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
            (
                run_parity_pass(&engine_dir),
                run_gate_pass_default(&engine_dir),
            )
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
            db::bead_titles_by_status(&conn, &[db::BeadStatus::Open, db::BeadStatus::InProgress])
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

    // Planner keys across ALL statuses — parity beads dedup on the stable key,
    // since their titles embed a live percentage that changes every cycle.
    let all_keys = match db::open(project_root) {
        Ok(conn) => collect_existing_planner_keys(&conn),
        Err(_) => vec![],
    };

    // Determine phase
    let phase = determine_phase(&gates, &parity, &config);

    let mut recommendations = Vec::new();

    let has_unchecked_criteria = !criteria.is_empty() && criteria.iter().any(|c| !c.checked);

    if has_unchecked_criteria {
        // Active criteria phase — drive beads from the execution map specs and
        // the unchecked criteria checklist. This is the same iteration logic
        // `quick_recommendations` uses; delegate to it so the two callers stay
        // in lockstep on dedup and throttle rules.
        recommendations.extend(quick_recommendations(project_root)?);
    } else if matches!(phase, Phase::V1Complete) {
        // V1 is done AND no unchecked criteria — fall back to next-phase
        // recommendations from a roadmap document, if one is configured.
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
            &all_keys,
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
        active_phase_label: config.phase_label.clone(),
    })
}

/// Phase-chain analyze: walk phases in order, tick passing criteria, return
/// recommendations from the first incomplete phase. Used when the planner
/// config declares `[[phase]]` blocks or AllCriteriaChecked completion.
fn analyze_phase_chain(
    project_root: &Path,
    config: &ProjectPlannerConfig,
    timestamp: &str,
) -> Result<PlanReport> {
    let queue = run_queue_pass(project_root)?;

    let engine_dir = project_root.join("engine-rs");

    // Dedup against all bead titles + planner-keys (used across every phase in
    // the walk). Open once.
    let (existing_titles, existing_keys) = match db::open(project_root) {
        Ok(conn) => (
            db::bead_titles_all(&conn).unwrap_or_default(),
            collect_existing_planner_keys(&conn),
        ),
        Err(e) => {
            eprintln!("[plan] db unavailable: {e}");
            (vec![], vec![])
        }
    };

    let mut active_label = String::from("default");
    let mut active_gates = empty_gates();
    let active_parity = empty_parity();
    let mut recommendations: Vec<Recommendation> = Vec::new();
    let mut all_phases_resolved = true;

    // Walk phases. For each, skip if Complete or Unevaluable, run analysis to
    // refresh state, tick passing criteria, then attempt to seed beads. We
    // commit a phase as "active" only when it actually produces recommendations
    // — a phase whose execution map is fully deduped (closed beads exist for
    // every item) falls through to the next phase rather than blocking the
    // walk. This is the key fix that prevents the old V1-stuck behavior:
    // "incomplete criteria + zero seedable work" is no longer terminal.
    for phase in &config.phases {
        let criteria = load_phase_criteria(project_root, phase);

        // Cheap pre-check: already all-checked → complete with no subprocess work.
        let cheap_complete = !criteria.is_empty() && criteria.iter().all(|c| c.checked);
        if cheap_complete {
            eprintln!(
                "[plan] phase '{}': all criteria already checked, advancing",
                phase.label
            );
            continue;
        }

        // Run the phase's analysis to refresh test status. Empty criteria with
        // FromCriteria analysis is a no-op — we still let the phase be
        // considered, since its execution map may have unseeded items.
        let gates = match &phase.analysis {
            crate::project_config::AnalysisSource::FromCriteria => {
                if engine_dir.exists() && !criteria.is_empty() {
                    run_analysis_from_criteria(
                        &criteria,
                        &phase.test_binaries,
                        &phase.test_packages,
                        phase.lib,
                        &engine_dir,
                    )
                } else {
                    empty_gates()
                }
            }
            crate::project_config::AnalysisSource::Commands(cmds) => {
                if cmds.is_empty() {
                    empty_gates()
                } else {
                    let (_, g) = run_analysis_commands(project_root, cmds);
                    g
                }
            }
        };

        if let Some(ec) = &phase.exit_criteria {
            let path = project_root.join(ec);
            if path.exists() {
                let ticked = tick_criteria_on_pass(&path, &gates).unwrap_or(0);
                if ticked > 0 {
                    eprintln!(
                        "[plan] phase '{}': ticked {ticked} criteria from passing tests",
                        phase.label
                    );
                }
            }
        }
        let criteria_now = load_phase_criteria(project_root, phase);

        // Three-valued completion check. Treat Complete and Unevaluable as
        // "skip this phase" — Unevaluable means we couldn't determine its
        // state (e.g. exit_criteria file not yet authored). Either way, don't
        // block the walk on a phase we can't make progress on.
        match phase_status(phase, &criteria_now, Some(&gates), None) {
            PhaseStatus::Complete => {
                eprintln!("[plan] phase '{}': complete, advancing", phase.label);
                continue;
            }
            PhaseStatus::Unevaluable => {
                // C3: distinguish a benign empty phase from the documented
                // silent-stall config error — criteria carry `(test:)` markers
                // that should auto-tick, but no test_binaries are configured, so
                // they can NEVER tick and the phase is skipped forever.
                if phase_has_unticked_test_markers(
                    &criteria_now,
                    &phase.test_binaries,
                    &phase.test_packages,
                ) {
                    tracing::warn!(
                        phase = %phase.label,
                        "phase has criteria with (test:) markers but no test_binaries configured — \
                         criteria can never auto-tick and the phase will be silently skipped every \
                         cycle; add the cargo test-target names to test_binaries in planner.toml"
                    );
                } else {
                    eprintln!(
                        "[plan] phase '{}': unevaluable (likely empty criteria), advancing",
                        phase.label
                    );
                }
                all_phases_resolved = false;
                continue;
            }
            PhaseStatus::Incomplete => {}
        }

        // Phase is incomplete. Try to produce recommendations from its
        // execution map. If everything is deduped, fall through to the next
        // phase so the walk doesn't stall on a saturated-but-unticked phase.
        let bead_specs = load_phase_execution_map(project_root, phase);
        let phase_recs = phase_recs_from_specs(
            &phase.label,
            &bead_specs,
            &criteria_now,
            &existing_titles,
            &existing_keys,
            &queue,
            0,
        );

        if !phase_recs.is_empty() {
            // This phase has actual seedable work — commit it as active and stop walking.
            active_label = phase.label.clone();
            active_gates = gates;
            recommendations = phase_recs;
            all_phases_resolved = false;
            break;
        }

        // Phase is incomplete but produced no recommendations (everything
        // deduped). Don't block the walk on it — let later phases get a
        // chance. Mark the run as "not all resolved" so the report reflects
        // there is work pending somewhere in the chain.
        all_phases_resolved = false;
        eprintln!(
            "[plan] phase '{}': incomplete but all bead specs already seeded, advancing",
            phase.label
        );
    }

    recommendations.sort_by_key(|r| r.priority);

    let phase_state = if all_phases_resolved {
        Phase::AllComplete
    } else {
        // Walked the entire chain, found incomplete phases. Whether we
        // produced recommendations or not, more work remains — caller
        // should look at `active_phase_label` to see which phase is
        // currently load-bearing.
        Phase::V1Active
    };

    Ok(PlanReport {
        timestamp: timestamp.to_string(),
        parity: active_parity,
        gates: active_gates,
        queue,
        recommendations,
        phase: phase_state,
        active_phase_label: active_label,
    })
}

/// Tier-1 fast path for phase-chain configs. Walks phases by checkbox state
/// only (no test subprocess), surfaces recommendations from the first phase
/// with unchecked criteria + unseeded execution-map items.
fn quick_recommendations_phase_chain(
    project_root: &Path,
    config: &ProjectPlannerConfig,
) -> Result<Vec<Recommendation>> {
    let queue = run_queue_pass(project_root)?;

    let existing_titles = match db::open(project_root) {
        Ok(conn) => db::bead_titles_all(&conn).unwrap_or_default(),
        Err(_) => vec![],
    };
    let existing_keys = match db::open(project_root) {
        Ok(conn) => collect_existing_planner_keys(&conn),
        Err(_) => vec![],
    };

    let mut recommendations: Vec<Recommendation> = Vec::new();

    for phase in &config.phases {
        let criteria = load_phase_criteria(project_root, phase);
        // Tier-1 has no analysis context. Skip phases that are complete OR
        // unevaluable (e.g. all_gates_passing without gate context) so the
        // walk doesn't stall on conditions only analyze() can resolve.
        match phase_status(phase, &criteria, None, None) {
            PhaseStatus::Complete | PhaseStatus::Unevaluable => continue,
            PhaseStatus::Incomplete => {}
        }
        let bead_specs = load_phase_execution_map(project_root, phase);
        let phase_recs = phase_recs_from_specs(
            &phase.label,
            &bead_specs,
            &criteria,
            &existing_titles,
            &existing_keys,
            &queue,
            recommendations.len(),
        );
        recommendations.extend(phase_recs);
        // Stop walking only once we've produced recommendations. If the
        // current phase had bead specs but every one was deduped, fall
        // through to later phases — otherwise a saturated phase would
        // permanently mask its successors.
        if !recommendations.is_empty() {
            break;
        }
    }

    recommendations.sort_by_key(|r| r.priority);
    Ok(recommendations)
}

/// Tier 1: Fast recommendations from PRD parsing only (no subprocess calls).
/// Runs in <500ms. Safe to call inline in the coordinator loop.
pub fn quick_recommendations(project_root: &Path) -> Result<Vec<Recommendation>> {
    // Load project-specific config
    let config = project_config::load(project_root);

    // Phase-chain configs use the fast-path walk: parse criteria + execution
    // map of the first incomplete phase (no analysis command — the box state
    // already reflects past test runs), then dedup against existing beads.
    if config_uses_phase_chain(&config) {
        return quick_recommendations_phase_chain(project_root, &config);
    }

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
            )
            .unwrap_or_default(),
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
        let key_match = existing_keys.iter().any(|desc| desc.contains(&key_pattern));

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
            depends_on: spec.depends_on.clone(),
        });
    }

    // For each unchecked criteria item with no matching existing bead → recommend
    for item in &criteria {
        if item.checked {
            continue;
        }

        let title_match = existing_titles.iter().any(|t| t.contains(&item.text));

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
        let key_match = existing_keys.iter().any(|desc| desc.contains(&key_pattern));

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
        Ok(conn) => {
            db::bead_titles_by_status(&conn, &[db::BeadStatus::Open, db::BeadStatus::InProgress])
                .unwrap_or_default()
        }
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

    // C7: surface execution-map specs that lack an acceptance command — these
    // seed beads the verifier can't prove. Warn by key so the source doc gets
    // fixed instead of the problem reappearing as a silent skip downstream.
    let missing_acc = prd_parser::specs_missing_acceptance(&bead_specs);
    if !missing_acc.is_empty() {
        tracing::warn!(
            count = missing_acc.len(),
            keys = %missing_acc.join(", "),
            "execution map: beads with no acceptance command (verifier cannot prove these)"
        );
    }

    // B6: warn on dangling dependencies before they reach the br graph.
    for w in validate_dependencies(&recommendations, &existing_keys) {
        tracing::warn!("{w}");
    }

    // Sort by priority
    recommendations.sort_by_key(|r| r.priority);

    Ok(recommendations)
}

// ─── PRD file loading ─────────────────────────────────────────────────────

fn load_criteria(
    project_root: &Path,
    config: &ProjectPlannerConfig,
) -> Vec<prd_parser::CriteriaItem> {
    let mut all = Vec::new();
    for file in &config.criteria_files {
        let path = project_root.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            all.extend(prd_parser::parse_criteria(&content));
        }
    }
    all
}

pub(crate) fn load_execution_maps(
    project_root: &Path,
    config: &ProjectPlannerConfig,
) -> Vec<prd_parser::BeadSpec> {
    let mut all = Vec::new();
    for file in &config.execution_map_files {
        let path = project_root.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            all.extend(prd_parser::parse_execution_map(&content));
        }
    }
    all
}

/// Read the single execution map associated with one phase.
fn load_phase_execution_map(
    project_root: &Path,
    phase: &crate::project_config::Phase,
) -> Vec<prd_parser::BeadSpec> {
    let rel = match &phase.execution_map {
        Some(p) => p,
        None => return Vec::new(),
    };
    let path = project_root.join(rel);
    match std::fs::read_to_string(&path) {
        Ok(c) => prd_parser::parse_execution_map(&c),
        Err(_) => Vec::new(),
    }
}

/// Read the single exit-criteria file associated with one phase.
fn load_phase_criteria(
    project_root: &Path,
    phase: &crate::project_config::Phase,
) -> Vec<prd_parser::CriteriaItem> {
    let path = match &phase.exit_criteria {
        Some(p) => project_root.join(p),
        None => return Vec::new(),
    };
    match std::fs::read_to_string(&path) {
        Ok(c) => prd_parser::parse_criteria(&c),
        Err(_) => Vec::new(),
    }
}

/// True when the config came from `[[phase]]` blocks in TOML. Set explicitly
/// at parse time so we don't have to sniff phase contents.
fn config_uses_phase_chain(config: &ProjectPlannerConfig) -> bool {
    config.uses_phase_chain
}

/// Build recommendations from a phase's execution-map bead specs, applying the
/// shared dedup + throttle rules. Used by BOTH the slow (`analyze_phase_chain`)
/// and fast (`quick_recommendations_phase_chain`) walks so the two tiers can't
/// drift on dedup/throttle behavior.
///
/// `already_queued` is the count of recommendations already produced this run
/// (e.g. queued depth from earlier in the walk), folded into the throttle.
fn phase_recs_from_specs(
    phase_label: &str,
    bead_specs: &[prd_parser::BeadSpec],
    criteria: &[prd_parser::CriteriaItem],
    existing_titles: &[String],
    existing_keys: &[String],
    queue: &QueueReport,
    already_queued: usize,
) -> Vec<Recommendation> {
    let mut recs: Vec<Recommendation> = Vec::new();
    if queue.ready_unassigned >= QUEUE_THROTTLE {
        return recs;
    }
    for spec in bead_specs {
        // Skip if the criterion this spec addresses is already ticked.
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
        let key_match = existing_keys.iter().any(|d| d.contains(&key_pattern));
        if title_match || key_match {
            continue;
        }
        if queue.ready_unassigned + already_queued + recs.len() >= QUEUE_THROTTLE {
            break;
        }
        recs.push(Recommendation {
            title: format!("{}: {}", spec.section, spec.description),
            priority: spec.priority,
            labels: vec![spec.section.clone(), phase_label.to_string()],
            description: format!(
                "IMPLEMENT: {desc}\n\
                 Phase: {phase}\n\
                 From execution map section: {section}\n\n\
                 [planner-key: {key}]",
                desc = spec.description,
                phase = phase_label,
                section = spec.section,
                key = spec.bead_key,
            ),
            acceptance_command: spec.acceptance_command.clone().unwrap_or_default(),
            gate_key: spec.bead_key.clone(),
            reason: format!("Phase '{phase_label}' execution map item with no existing bead"),
            depends_on: spec.depends_on.clone(),
        });
    }
    recs
}

/// Outcome of a single phase-completion check.
///
/// Distinguishes "actually incomplete" from "can't evaluate right now" so
/// the phase walk can decide whether to stall on a phase or advance past
/// it. The Tier-1 fast path treats `Unevaluable` as "skip" rather than
/// "block forever".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhaseStatus {
    Complete,
    Incomplete,
    /// A condition needs gate/parity data that isn't available in the
    /// current context (e.g. the Tier-1 fast path doesn't run analysis).
    Unevaluable,
}

/// Evaluate a phase's completion conditions. See `PhaseStatus` for the
/// three-valued result.
fn phase_status(
    phase: &crate::project_config::Phase,
    criteria: &[prd_parser::CriteriaItem],
    gates: Option<&GateReport>,
    parity: Option<&ParityReport>,
) -> PhaseStatus {
    if phase.completion.is_empty() {
        // No explicit completion → fall back to "all criteria checked".
        // An empty criteria list is unevaluable rather than incomplete so
        // the walk doesn't stall on a phase that hasn't been authored yet.
        if criteria.is_empty() {
            return PhaseStatus::Unevaluable;
        }
        return if criteria.iter().all(|c| c.checked) {
            PhaseStatus::Complete
        } else {
            PhaseStatus::Incomplete
        };
    }
    for cond in &phase.completion {
        match cond {
            CompletionCondition::AllCriteriaChecked => {
                if criteria.is_empty() {
                    return PhaseStatus::Unevaluable;
                }
                if criteria.iter().any(|c| !c.checked) {
                    return PhaseStatus::Incomplete;
                }
            }
            CompletionCondition::AllGatesPassing => match gates {
                Some(g) if g.total > 0 && g.failing.is_empty() => {}
                Some(g) if !g.failing.is_empty() => return PhaseStatus::Incomplete,
                _ => return PhaseStatus::Unevaluable,
            },
            CompletionCondition::ParityAbove(t) => match parity {
                Some(p) if p.overall >= *t => {}
                Some(_) => return PhaseStatus::Incomplete,
                None => return PhaseStatus::Unevaluable,
            },
        }
    }
    PhaseStatus::Complete
}

/// Convenience predicate that treats `Unevaluable` as "not complete" — used
/// where we want a hard yes/no (e.g. analyze_phase_chain after a real
/// analysis pass, where Unevaluable shouldn't happen).
fn phase_is_complete(
    phase: &crate::project_config::Phase,
    criteria: &[prd_parser::CriteriaItem],
    gates: Option<&GateReport>,
    parity: Option<&ParityReport>,
) -> bool {
    matches!(
        phase_status(phase, criteria, gates, parity),
        PhaseStatus::Complete
    )
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
                        eprintln!(
                            "planner: command '{}' timed out after {}s",
                            cmd,
                            dur.as_secs()
                        );
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
    let re = Regex::new(r"(?m)^\s*(\S+)\s+(\d+)\s+(\d+)\s+(\d+(?:\.\d+)?)%\s*$").unwrap();

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
        eprintln!(
            "planner: parity parser found no scene data in output ({} bytes)",
            text.len()
        );
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

/// Parse test pass/fail from either `cargo test` or `cargo nextest` output.
///
/// - cargo test:    `test <name> ... ok|FAILED|ignored`
/// - cargo nextest: `   PASS|FAIL [   0.0s] (1/1) <pkg> <module::…::name>`
///
/// Names are reduced to the bare final `::` segment so they match the bare fn
/// names that criteria markers carry (the tick set is an exact-match lookup).
pub fn parse_gate_output(text: &str) -> GateReport {
    let cargo_re = Regex::new(r"(?m)^test\s+(\S+)\s+\.\.\.\s+(ok|FAILED|ignored)").unwrap();
    // Status word, then the test path is the final whitespace-delimited token.
    let nextest_re =
        Regex::new(r"(?m)^\s*(PASS|FAIL|TIMEOUT|ABORT|SIGSEGV|LEAK)\b.*\s(\S+)\s*$").unwrap();

    let bare = |s: &str| s.rsplit("::").next().unwrap_or(s).to_string();

    let mut passing = Vec::new();
    let mut failing = Vec::new();

    for cap in cargo_re.captures_iter(text) {
        let name = bare(&cap[1]);
        match &cap[2] {
            "ok" => passing.push(name),
            "FAILED" => failing.push(name),
            _ => {} // ignored entries are not counted
        }
    }
    for cap in nextest_re.captures_iter(text) {
        let name = bare(&cap[2]);
        if &cap[1] == "PASS" {
            passing.push(name);
        } else {
            failing.push(name);
        }
    }

    passing.sort();
    passing.dedup();
    failing.sort();
    failing.dedup();
    // A retried test can report both PASS and FAIL; let any failure win so a
    // flaky pass never ticks a box.
    passing.retain(|p| !failing.contains(p));

    if passing.is_empty() && failing.is_empty() {
        eprintln!(
            "planner: gate parser found no test results in output ({} bytes)",
            text.len()
        );
    }

    let total = passing.len() + failing.len();
    GateReport {
        total,
        passing,
        failing,
    }
}

// ─── Pass B': Criteria-driven analysis ────────────────────────────────────

/// Hard ceiling on a criteria-analysis subprocess. A compile + run of a
/// handful of scoped test binaries should finish well within this; if it
/// hangs (lock contention, runaway test), we kill it and report no passes
/// rather than wedge the coordinator loop.
/// Wall-clock ceiling for a criteria-analysis subprocess. The dominant cost is
/// a COLD compile of the scoped test binary, which links the whole engine — on
/// a loaded machine that can take many minutes. This runs at most once per
/// planner cycle (not per bead), so we budget generously: a too-tight timeout
/// would kill a legitimate cold build and silently stall the phase (criteria
/// never tick). 20 minutes gives ample headroom while still bounding the
/// lock-contention case the timeout exists to prevent.
// 3600s, not 1200s: the editor-parity analysis does a cold gdeditor lib-test
// build (~14 min for the large #[cfg(test)] block) plus 154 test runs on the
// FIRST cycle, which exceeds 20 min. Incremental compilation keeps every later
// cycle to seconds, so this high ceiling only ever applies to the first run.
const CRITERIA_ANALYSIS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3600);

/// Stale-temp-file age threshold. `run_command_with_timeout` sweeps leftover
/// `patina-plan-*` files older than this on entry — they only accumulate if a
/// prior planner process was hard-killed (SIGKILL) mid-run, since every
/// graceful exit path cleans up its own files.
const STALE_TEMP_AGE: std::time::Duration = std::time::Duration::from_secs(3600);

/// Run the tests named in criteria-line markers and return a GateReport keyed
/// by test name. Skips criteria items without a `(test: \`...\`)` marker.
///
/// The build is scoped to `test_binaries` via repeated `--test <name>` flags
/// so cargo compiles ONLY those targets. This is critical: a `--workspace`
/// build would compile all 400+ integration tests and lock the machine. When
/// `test_binaries` is empty, the function returns early WITHOUT running
/// anything — it never falls back to a workspace-wide build.
pub fn run_analysis_from_criteria(
    criteria: &[prd_parser::CriteriaItem],
    test_binaries: &[String],
    test_packages: &[String],
    lib: bool,
    engine_dir: &Path,
) -> GateReport {
    let mut test_names: Vec<String> = criteria
        .iter()
        .filter_map(|c| prd_parser::extract_test_name_from_criteria_line(&c.text))
        .collect();
    test_names.sort();
    test_names.dedup();

    if test_names.is_empty() {
        return empty_gates();
    }

    if test_binaries.is_empty() && test_packages.is_empty() {
        // No scoped targets configured — refuse to run a workspace-wide build.
        // The phase's boxes will only tick if something else (a manual run or
        // a future config with test_binaries/test_packages) checks them.
        eprintln!(
            "planner: criteria analysis skipped — phase has {} test name(s) but no \
             `test_binaries`/`test_packages` configured; refusing a --workspace build. \
             Add the cargo test-target or package names to the phase in planner.toml.",
            test_names.len()
        );
        return empty_gates();
    }

    // Build a nextest filter expression: test(=name1) | test(=name2) | ...
    let nextest_filter = test_names
        .iter()
        // End-anchored regex, NOT `test(=name)`: nextest's `=` is an exact match
        // on the FULL test path (`module::…::name`), but criteria carry only the
        // bare fn name, so `=` matches nothing. `/name$/` matches the test whose
        // path ends in that fn name (precise even when names share a prefix).
        .map(|n| format!("test(/{n}$/)"))
        .collect::<Vec<_>>()
        .join(" | ");

    // cargo nextest run --no-fail-fast --run-ignored all \
    //   -p <pkg>... [--lib] --test <bin>... -E <filter>
    // `-p <pkg>` scopes the build to those packages (lib + their tests); `--lib`
    // narrows it to just the lib test target (where most editor-parity tests
    // live), so the run re-launches one cached binary per package instead of
    // ~100 distinct integration binaries. `--test <bin>` adds the few
    // integration-only targets on top.
    let mut nextest_args: Vec<String> = vec![
        "nextest".into(),
        "run".into(),
        "--no-fail-fast".into(),
        "--run-ignored".into(),
        "all".into(),
    ];
    for pkg in test_packages {
        nextest_args.push("-p".into());
        nextest_args.push(pkg.clone());
    }
    if lib {
        nextest_args.push("--lib".into());
    }
    for bin in test_binaries {
        nextest_args.push("--test".into());
        nextest_args.push(bin.clone());
    }
    nextest_args.push("-E".into());
    nextest_args.push(nextest_filter);

    let combined = match run_command_with_timeout(
        "cargo",
        &nextest_args,
        engine_dir,
        CRITERIA_ANALYSIS_TIMEOUT,
    ) {
        Some((output, code)) if code != Some(101) => output,
        Some(_) => {
            // 101 = compile error in a scoped target. Don't fall back to a
            // workspace build (that's the lock hazard). Report no passes.
            eprintln!("planner: criteria analysis hit a compile error in scoped targets");
            return empty_gates();
        }
        None => {
            eprintln!(
                "planner: criteria analysis timed out after {}s — killed",
                CRITERIA_ANALYSIS_TIMEOUT.as_secs()
            );
            return empty_gates();
        }
    };

    parse_gate_output(&combined)
}

/// Run a command with a wall-clock timeout. Returns `Some((combined_output,
/// exit_code))` on completion, or `None` if the timeout elapsed (the child is
/// killed). stdout+stderr are redirected to temp files so a full pipe buffer
/// can't deadlock the child while we poll. Polls `try_wait` rather than
/// pulling in a dependency.
fn run_command_with_timeout(
    program: &str,
    args: &[String],
    workdir: &Path,
    timeout: std::time::Duration,
) -> Option<(String, Option<i32>)> {
    let base = std::env::temp_dir();

    // Best-effort sweep of files leaked by a hard-killed prior run. Graceful
    // exits clean up their own files; this only catches SIGKILL leftovers.
    sweep_stale_temp_files(&base);

    // Unique temp paths in the system temp dir (no extra crate dependency).
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let out_path = base.join(format!("patina-plan-{pid}-{nanos}.out"));
    let err_path = base.join(format!("patina-plan-{pid}-{nanos}.err"));

    let stdout = match std::fs::File::create(&out_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "planner: failed to create temp file {}: {e}",
                out_path.display()
            );
            return Some((String::new(), Some(-1)));
        }
    };
    let stderr = match std::fs::File::create(&err_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "planner: failed to create temp file {}: {e}",
                err_path.display()
            );
            return Some((String::new(), Some(-1)));
        }
    };

    let spawn = Command::new(program)
        .args(args)
        .current_dir(workdir)
        .stdout(stdout)
        .stderr(stderr)
        .spawn();

    let mut child = match spawn {
        Ok(c) => c,
        Err(e) => {
            eprintln!("planner: failed to spawn {program}: {e}");
            let _ = std::fs::remove_file(&out_path);
            let _ = std::fs::remove_file(&err_path);
            return Some((String::new(), Some(-1)));
        }
    };

    let start = std::time::Instant::now();
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&out_path);
                    let _ = std::fs::remove_file(&err_path);
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => {
                eprintln!("planner: error waiting on {program}: {e}");
                let _ = std::fs::remove_file(&out_path);
                let _ = std::fs::remove_file(&err_path);
                return Some((String::new(), Some(-1)));
            }
        }
    };

    let stdout_text = std::fs::read_to_string(&out_path).unwrap_or_default();
    let stderr_text = std::fs::read_to_string(&err_path).unwrap_or_default();
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);
    Some((format!("{stdout_text}\n{stderr_text}"), exit_code))
}

/// Remove `patina-plan-*.{out,err}` files in `dir` older than `STALE_TEMP_AGE`.
/// Best-effort: ignores all errors. These only exist if a prior planner
/// process was SIGKILL'd between spawning a child and its cleanup.
fn sweep_stale_temp_files(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("patina-plan-") {
            continue;
        }
        if !(name.ends_with(".out") || name.ends_with(".err")) {
            continue;
        }
        let age_ok = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mtime| now.duration_since(mtime).ok())
            .map(|age| age >= STALE_TEMP_AGE)
            .unwrap_or(false);
        if age_ok {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Rewrite a criteria markdown file so that every criterion whose named test
/// passed in `gates` becomes `- [x]`. Already-checked lines are left as-is.
/// Lines without a `(test: \`...\`)` marker are untouched.
///
/// Writes are atomic: the new content lands in a `.tmp` file next to the
/// criteria file, then is renamed into place. A SIGKILL mid-write leaves
/// either the original file or the new file — never a truncated artifact.
///
/// Returns the number of newly-checked criteria.
pub fn tick_criteria_on_pass(criteria_path: &Path, gates: &GateReport) -> Result<usize> {
    let content = std::fs::read_to_string(criteria_path).map_err(|e| {
        crate::error::OrchestratorError::Io(std::io::Error::new(
            e.kind(),
            format!("read {}: {e}", criteria_path.display()),
        ))
    })?;

    let passing_set: std::collections::HashSet<&str> =
        gates.passing.iter().map(String::as_str).collect();

    // Capture the original's trailing-newline state once, before any splicing.
    // This decides whether the output should keep its final '\n' at the end.
    let original_had_trailing_newline = content.ends_with('\n');

    let mut newly_checked = 0usize;
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        // Only touch lines that start with `- [ ]` (unchecked) and have a
        // test marker whose test passed.
        let trimmed = line.trim_start();
        let unchecked = trimmed.starts_with("- [ ]");
        if unchecked {
            if let Some(test_name) = prd_parser::extract_test_name_from_criteria_line(trimmed) {
                if passing_set.contains(test_name.as_str()) {
                    // Splice in `[x]` preserving leading whitespace.
                    let lead_len = line.len() - trimmed.len();
                    out.push_str(&line[..lead_len]);
                    out.push_str("- [x]");
                    out.push_str(&trimmed["- [ ]".len()..]);
                    out.push('\n');
                    newly_checked += 1;
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    // Honor the original's trailing-newline state. content.lines() drops the
    // final newline if there was one, and we appended '\n' to every emitted
    // line — so if the original did NOT end with '\n', strip the one we added.
    if !original_had_trailing_newline && out.ends_with('\n') {
        out.pop();
    }

    if newly_checked > 0 {
        // Atomic write: stage to a sibling `.tmp` then rename. Both files
        // live on the same filesystem so the rename is a single inode swap.
        let tmp_path = criteria_path.with_extension("md.tmp");
        std::fs::write(&tmp_path, &out).map_err(|e| {
            crate::error::OrchestratorError::Io(std::io::Error::new(
                e.kind(),
                format!("write {}: {e}", tmp_path.display()),
            ))
        })?;
        std::fs::rename(&tmp_path, criteria_path).map_err(|e| {
            // Best-effort cleanup so a failed rename doesn't leave .tmp behind.
            let _ = std::fs::remove_file(&tmp_path);
            crate::error::OrchestratorError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "rename {} → {}: {e}",
                    tmp_path.display(),
                    criteria_path.display()
                ),
            ))
        })?;
    }
    Ok(newly_checked)
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
    db::bead_descriptions_containing(conn, "[planner-key:").unwrap_or_default()
}

/// C4: lint a plan report and return human-readable audit findings — beads that
/// would be seeded without an acceptance command, and dependencies that resolve
/// to neither a sibling recommendation nor an existing bead. Used by
/// `plan --dry-run` so operators see *why* a seed would be broken before
/// `--apply` creates anything.
pub fn audit_report(project_root: &Path, report: &PlanReport) -> Vec<String> {
    let mut findings = Vec::new();
    for r in &report.recommendations {
        if r.acceptance_command.trim().is_empty() {
            findings.push(format!(
                "MISSING-ACCEPTANCE  P{} '{}' (key={}) — verifier cannot prove this bead",
                r.priority, r.title, r.gate_key
            ));
        }
    }
    let existing_keys = match db::open(project_root) {
        Ok(conn) => collect_existing_planner_keys(&conn),
        Err(_) => vec![],
    };
    for w in validate_dependencies(&report.recommendations, &existing_keys) {
        findings.push(format!("DANGLING-DEP  {w}"));
    }
    findings
}

/// C3: detect the silent-stall config error — a phase whose criteria carry
/// `(test: ...)` markers (so they are *meant* to auto-tick from passing tests)
/// but which has no `test_binaries` configured, meaning analysis is skipped and
/// the criteria can never tick. Such a phase is reported Unevaluable and
/// silently skipped on every cycle.
fn phase_has_unticked_test_markers(
    criteria: &[prd_parser::CriteriaItem],
    test_binaries: &[String],
    test_packages: &[String],
) -> bool {
    test_binaries.is_empty()
        && test_packages.is_empty()
        && criteria.iter().any(|c| {
            !c.checked && prd_parser::extract_test_name_from_criteria_line(&c.text).is_some()
        })
}

/// Extract the bare planner key from a description containing
/// `[planner-key: <key>]`. Returns `None` when no well-formed marker is present.
fn extract_planner_key(desc: &str) -> Option<String> {
    let start = desc.find("[planner-key: ")? + "[planner-key: ".len();
    let rest = &desc[start..];
    let end = rest.find(']')?;
    let key = rest[..end].trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_lowercase())
    }
}

/// Validate that every `depends_on` of a to-be-seeded recommendation resolves
/// to either another recommendation in this batch or an existing bead (B6).
///
/// Returns one warning string per dangling dependency. A dep pointing at a key
/// that was deduped/closed/never-seeded would otherwise create a broken edge in
/// the `br` dependency graph that needs manual repair.
fn validate_dependencies(recs: &[Recommendation], existing_key_descs: &[String]) -> Vec<String> {
    use std::collections::HashSet;
    let mut known: HashSet<String> = recs.iter().map(|r| r.gate_key.to_lowercase()).collect();
    for desc in existing_key_descs {
        if let Some(k) = extract_planner_key(desc) {
            known.insert(k);
        }
    }
    let mut warnings = Vec::new();
    for rec in recs {
        for dep in &rec.depends_on {
            let dep_norm = dep.to_lowercase();
            if !known.contains(&dep_norm) {
                warnings.push(format!(
                    "bead '{}' depends on '{}' which is neither being seeded nor an existing bead — dependency will be dropped",
                    rec.gate_key, dep
                ));
            }
        }
    }
    warnings
}

/// Collect planner keys only from beads in the given statuses.
fn collect_existing_planner_keys_by_status(
    conn: &rusqlite::Connection,
    statuses: &[db::BeadStatus],
) -> Vec<String> {
    db::bead_descriptions_containing_by_status(conn, "[planner-key:", statuses).unwrap_or_default()
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
        "editor security hardening" => NextPhaseTemplate {
            title: "Harden editor HTTP server with auth, sandbox, audit log, and rate limit",
            labels: &["editor-agent", "security"],
            acceptance: "bearer token auth, filesystem sandbox, append-only audit log, and per-token rate limit are each covered by a dedicated integration test under engine-rs/tests",
        },
        "editor agent capabilities and openapi" => NextPhaseTemplate {
            title: "Expose editor capabilities and OpenAPI spec for agent driveability",
            labels: &["editor-agent", "api"],
            acceptance: "GET /api/capabilities returns a deterministic schema of every route and a checked-in OpenAPI 3 spec validates against the live capabilities output",
        },
        "editor agent realtime events" => NextPhaseTemplate {
            title: "Add realtime event stream and concurrency control to the editor server",
            labels: &["editor-agent", "realtime"],
            acceptance: "a /api/events WebSocket broadcasts ordered scene-tree mutations to all clients and concurrent edits to the same node return HTTP 409 with a node version conflict",
        },
        "editor wgpu default renderer" => NextPhaseTemplate {
            title: "Make wgpu the default editor renderer and validate viewport latency",
            labels: &["editor-agent", "quality"],
            acceptance: "default cargo build links wgpu, software rasterizer is gated behind a feature flag, and /api/viewport p99 is under 50 ms over 1000 calls on the CI baseline",
        },
        "editor visual parity audit" => NextPhaseTemplate {
            title: "Audit editor visual parity against Godot on reference scenes",
            labels: &["editor-agent", "visual"],
            acceptance: "DOM parity tests cover Inspector, Scene Tree, and bottom panels on five reference scenes and a viewport pixel-diff against Godot stays within 2 percent mean error",
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
        let key_match = existing_keys.iter().any(|desc| desc.contains(&key_pattern));

        if title_match || key_match {
            continue;
        }

        let title = format!(
            "{} gate: {} — {}",
            "V1", // Could come from config.phase_label in the future
            entry.criteria_section,
            entry.criteria_line
        );

        let description = format!(
            "Acceptance gate `{}` is failing.\n\n\
             Criteria: {}\n\
             Section: {}\n\n\
             [planner-key: {}]",
            entry.test_name, entry.criteria_line, entry.criteria_section, entry.bead_key,
        );

        let acceptance_command = entry.test_name.clone();

        recs.push(Recommendation {
            title,
            priority: entry.priority,
            labels: vec!["gate".to_string(), entry.criteria_section.clone()],
            description,
            acceptance_command,
            gate_key: entry.bead_key.clone(),
            reason: format!("Gate {} still fails, no open bead found", entry.test_name),
            depends_on: vec![],
        });
    }

    recs.sort_by_key(|r| r.priority);
    recs
}

/// Generate recommendations for scenes that are below 100% parity.
///
/// Dedup is by the stable `parity-gap-<scene>` planner key, NOT by title: the
/// title embeds the live parity percentage, which changes every cycle as parity
/// improves, so a title-only check re-seeds the same bead repeatedly (the
/// documented "parity beads re-seeded every cycle" stall). `existing_titles` is
/// still consulted as a fallback so legacy beads created before key-dedup are
/// also recognized.
pub fn generate_parity_recommendations(
    parity: &ParityReport,
    existing_titles: &[String],
    existing_keys: &[String],
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
        let missing = scene.total.saturating_sub(scene.matched);
        let title = format!(
            "Close parity gap in {} (currently {:.1}%)",
            scene.name, scene.parity
        );
        let key = format!("parity-gap-{}", scene.name);

        // Primary dedup: stable planner key (title is volatile).
        let key_pattern = format!("[planner-key: {key}]");
        if existing_keys.iter().any(|d| d.contains(&key_pattern)) {
            continue;
        }
        // Fallback dedup: legacy parity beads created before key-based dedup.
        if existing_titles
            .iter()
            .any(|t| t.contains(&scene.name) && t.contains("parity"))
        {
            continue;
        }

        let acceptance_command = format!(
            "cargo test --test oracle_regression_test -- golden_{}_full_property_parity",
            scene.name
        );

        recs.push(Recommendation {
            title: title.clone(),
            priority: 2,
            labels: vec!["parity-gap".to_string()],
            description: format!(
                "IMPLEMENT engine fixes so the `{name}` scene reaches 100% property parity \
                 with the Godot oracle ({missing} of {total} properties currently mismatch; \
                 {matched}/{total} = {pct:.1}% match).\n\
                 The acceptance test below fails one assertion per mismatching property and \
                 names the expected vs actual value — make every assertion pass by correcting \
                 the engine's output (do not edit the test or the golden fixture).\n\
                 Acceptance: {cmd}\n\n\
                 [planner-key: {key}]",
                name = scene.name,
                missing = missing,
                matched = scene.matched,
                total = scene.total,
                pct = scene.parity,
                cmd = acceptance_command,
                key = key,
            ),
            acceptance_command,
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

/// Build a `planner-key -> acceptance text` map covering every phase 5-9
/// deliverable in the project's `next_sources`. Used by `plan --heal` to
/// backfill `acceptance_criteria` on legacy beads created before `--apply`
/// populated the structured field.
///
/// Unlike `generate_next_phase_recommendations`, this does NOT dedup against
/// existing beads — it indexes every potential template so heal can look up
/// already-created beads by their planner-key.
pub fn build_planner_key_acceptance_map(
    project_root: &Path,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let config = project_config::load(project_root);
    for source_file in &config.next_sources {
        let path = project_root.join(source_file);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for phase_num in 5..=9 {
            let prefix = format!("Phase {}", phase_num);
            let deliverables = prd_parser::parse_phase_deliverables(&content, &prefix);
            for d in deliverables {
                let template = next_phase_template(&d.title);
                let key = format!("phase{}-{}", phase_num, d.slug);
                map.insert(key, template.acceptance.to_string());
            }
        }
    }
    map
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

            // Chain sequential deliverables within the same phase so each
            // depends on the previous one — mirrors the execution map parser.
            let mut prev_key_in_phase: Option<String> = None;

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
                    || existing_titles
                        .iter()
                        .any(|t| t.eq_ignore_ascii_case(&d.title) || t.eq_ignore_ascii_case(&title))
                {
                    // Even if we skip creating this bead, it's still in the
                    // chain for dependency purposes.
                    prev_key_in_phase = Some(key);
                    continue;
                }

                let labels = if template.labels.is_empty() {
                    vec![format!("phase{}", phase_num)]
                } else {
                    template.labels.iter().map(|l| (*l).to_string()).collect()
                };

                let depends_on = prev_key_in_phase
                    .as_ref()
                    .map(|k| vec![k.clone()])
                    .unwrap_or_default();

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
                    gate_key: key.clone(),
                    reason: format!("Phase {} deliverable from port plan, no existing bead", phase_num),
                    depends_on,
                });
                prev_key_in_phase = Some(key);
                seen_titles.insert(normalized_title);
                seen_keys.insert(key_pattern);
            }
        }
    }
    recs
}

// ─── Phase determination ──────────────────────────────────────────────────

fn determine_phase(
    gates: &GateReport,
    parity: &ParityReport,
    config: &ProjectPlannerConfig,
) -> Phase {
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
            // The phase-walk path evaluates AllCriteriaChecked directly
            // against the criteria items; the V1-centric legacy path uses
            // gate/parity signals only, so this condition is a no-op here.
            CompletionCondition::AllCriteriaChecked => {}
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
    fn test_tick_criteria_on_pass_writes_x_for_passing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EXIT.md");
        let original = "\
# Exit Criteria

## Security

- [ ] Editor HTTP server requires bearer token auth (test: `editor_auth_required_test`)
- [ ] Filesystem endpoints reject paths outside project root (test: `editor_filesystem_sandbox_test`)
- [ ] Already done (test: `editor_audit_log_test`)
- [ ] Criterion without test marker

## Other

- [x] Pre-existing checked item stays checked (test: `editor_other_test`)
";
        fs::write(&path, original).unwrap();
        let gates = GateReport {
            total: 3,
            passing: vec![
                "editor_auth_required_test".to_string(),
                "editor_audit_log_test".to_string(),
            ],
            failing: vec!["editor_filesystem_sandbox_test".to_string()],
        };
        let n = tick_criteria_on_pass(&path, &gates).unwrap();
        assert_eq!(n, 2, "should tick auth and audit, not sandbox");
        let after = fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("- [x] Editor HTTP server requires bearer token auth"),
            "auth should be ticked:\n{after}"
        );
        assert!(
            after.contains("- [ ] Filesystem endpoints"),
            "failing test should leave criterion unchecked"
        );
        assert!(
            after.contains("- [x] Already done"),
            "passing test should tick the criterion"
        );
        assert!(
            after.contains("- [ ] Criterion without test marker"),
            "no test marker → no tick"
        );
        assert!(
            after.contains("- [x] Pre-existing checked item"),
            "already-checked should stay"
        );
    }

    #[test]
    fn test_tick_criteria_on_pass_atomic_no_partial_file() {
        // After a successful tick, no `.tmp` sibling should remain.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EXIT.md");
        fs::write(&path, "- [ ] X (test: `t1`)\n").unwrap();
        let gates = GateReport {
            total: 1,
            passing: vec!["t1".to_string()],
            failing: vec![],
        };
        let n = tick_criteria_on_pass(&path, &gates).unwrap();
        assert_eq!(n, 1);
        let tmp_path = path.with_extension("md.tmp");
        assert!(
            !tmp_path.exists(),
            "tmp file should be renamed away, not left behind"
        );
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("- [x] X"));
    }

    #[test]
    fn test_tick_criteria_on_pass_preserves_no_trailing_newline() {
        // Edge case W5: input without final newline should stay that way.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EXIT.md");
        fs::write(&path, "- [ ] X (test: `t1`)").unwrap();
        let gates = GateReport {
            total: 1,
            passing: vec!["t1".to_string()],
            failing: vec![],
        };
        tick_criteria_on_pass(&path, &gates).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, "- [x] X (test: `t1`)");
    }

    /// Regression for the V1-stuck behavior the user explicitly called out:
    /// if phase 1 has unchecked criteria but every spec is already deduped
    /// (existing closed beads), the planner must walk to phase 2 instead of
    /// reporting "incomplete + zero recommendations" forever.
    #[test]
    fn test_quick_recommendations_falls_through_saturated_phase() {
        use crate::project_config::{AnalysisSource, Phase};
        use std::path::PathBuf;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Phase 1: unchecked criteria + execution map whose only item is
        // already represented by an "existing title" (simulating a closed
        // bead in production).
        fs::write(root.join("p1_exit.md"), "- [ ] Unchecked (test: `t1`)\n").unwrap();
        fs::write(
            root.join("p1_map.md"),
            "## Now\n\n1. `p1-saturated` Already-implemented thing\n   Acceptance: (test: `t1`)\n",
        )
        .unwrap();
        // Phase 2: fresh work.
        fs::write(root.join("p2_exit.md"), "- [ ] Fresh (test: `t2`)\n").unwrap();
        fs::write(
            root.join("p2_map.md"),
            "## Now\n\n1. `p2-new` Implement\n   Acceptance: (test: `t2`)\n",
        )
        .unwrap();

        let phases = vec![
            Phase {
                label: "p1".into(),
                execution_map: Some(PathBuf::from("p1_map.md")),
                exit_criteria: Some(PathBuf::from("p1_exit.md")),
                analysis: AnalysisSource::FromCriteria,
                completion: vec![CompletionCondition::AllCriteriaChecked],
                test_binaries: vec![],
                test_packages: vec![],
                lib: false,
            },
            Phase {
                label: "p2".into(),
                execution_map: Some(PathBuf::from("p2_map.md")),
                exit_criteria: Some(PathBuf::from("p2_exit.md")),
                analysis: AnalysisSource::FromCriteria,
                completion: vec![CompletionCondition::AllCriteriaChecked],
                test_binaries: vec![],
                test_packages: vec![],
                lib: false,
            },
        ];

        // Walk emulation: check phase status, look at specs. Phase 1's only
        // spec matches an existing title — should be skipped, walk continues
        // to phase 2.
        let p1_specs = load_phase_execution_map(root, &phases[0]);
        assert_eq!(p1_specs.len(), 1);
        let p1_crit = load_phase_criteria(root, &phases[0]);
        assert!(
            !phase_is_complete(&phases[0], &p1_crit, None, None),
            "p1 unchecked criteria → not complete"
        );
        // Simulated "already-implemented" title.
        let existing_titles = vec!["Now: Already-implemented thing".to_string()];
        let p1_dedup = p1_specs
            .iter()
            .all(|s| existing_titles.iter().any(|t| t.contains(&s.description)));
        assert!(p1_dedup, "p1 should be fully deduped");

        // Phase 2 has unseeded specs.
        let p2_specs = load_phase_execution_map(root, &phases[1]);
        assert_eq!(p2_specs.len(), 1);
        let p2_dedup = p2_specs
            .iter()
            .all(|s| existing_titles.iter().any(|t| t.contains(&s.description)));
        assert!(!p2_dedup, "p2 should NOT be deduped");
        // The walk should reach p2.
    }

    #[test]
    fn test_phase_enum_has_all_complete_variant() {
        // S4 follow-up: AllComplete distinguishes "V1 done" from "all phases done".
        // Compile-time check; the value is set in analyze_phase_chain.
        let all = Phase::AllComplete;
        assert_eq!(all, Phase::AllComplete);
        assert_ne!(Phase::AllComplete, Phase::V1Complete);
    }

    #[test]
    fn test_sweep_stale_temp_files_removes_only_old_planner_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();

        // A fresh planner temp file (should survive — too new).
        let fresh = p.join("patina-plan-999-123.out");
        fs::write(&fresh, "x").unwrap();
        // An unrelated file (should survive — wrong prefix).
        let unrelated = p.join("some-other-file.out");
        fs::write(&unrelated, "x").unwrap();
        // An old planner temp file (should be swept). Backdate its mtime past
        // the threshold via filetime-free trick: set via utimensat is awkward
        // without a dep, so assert the prefix/suffix matching + age logic by
        // confirming the fresh file is NOT removed (age branch is exercised
        // separately in integration).
        sweep_stale_temp_files(p);

        assert!(fresh.exists(), "fresh planner temp file must not be swept");
        assert!(unrelated.exists(), "non-planner file must not be swept");
    }

    #[test]
    fn test_run_analysis_from_criteria_skips_without_test_binaries() {
        // The critical machine-lock guard: when a phase names criteria tests
        // but configures no test_binaries, analysis must return empty (no
        // passes) WITHOUT spawning any cargo build. We can't easily assert
        // "no subprocess" directly, but we assert the early-return contract:
        // empty test_binaries → empty GateReport, fast, no panic.
        let criteria = vec![prd_parser::CriteriaItem {
            section: "S".into(),
            text: "Some criterion (test: `editor_auth_required_test`)".into(),
            checked: false,
            line_number: 1,
        }];
        let report =
            run_analysis_from_criteria(&criteria, &[], &[], false, Path::new("/nonexistent-engine"));
        assert!(report.passing.is_empty());
        assert!(report.failing.is_empty());
        assert_eq!(report.total, 0);
    }

    #[test]
    fn test_phase_status_unevaluable_for_missing_gates() {
        // W1: a phase using AllGatesPassing without gate context should
        // return Unevaluable so the walk advances rather than stalling.
        use crate::project_config::{AnalysisSource, Phase};
        use std::path::PathBuf;

        let phase = Phase {
            label: "needs-gates".into(),
            execution_map: Some(PathBuf::from("MAP.md")),
            exit_criteria: None,
            analysis: AnalysisSource::Commands(vec![]),
            completion: vec![CompletionCondition::AllGatesPassing],
            test_binaries: vec![],
            test_packages: vec![],
            lib: false,
        };
        assert_eq!(
            phase_status(&phase, &[], None, None),
            PhaseStatus::Unevaluable
        );
        // With gates context (all passing), it should be Complete.
        let gates_ok = GateReport {
            total: 3,
            passing: vec!["a".into(), "b".into(), "c".into()],
            failing: vec![],
        };
        assert_eq!(
            phase_status(&phase, &[], Some(&gates_ok), None),
            PhaseStatus::Complete
        );
        // With failing gates, Incomplete.
        let gates_fail = GateReport {
            total: 3,
            passing: vec!["a".into()],
            failing: vec!["b".into(), "c".into()],
        };
        assert_eq!(
            phase_status(&phase, &[], Some(&gates_fail), None),
            PhaseStatus::Incomplete
        );
    }

    #[test]
    fn test_tick_criteria_on_pass_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EXIT.md");
        fs::write(&path, "- [x] All set (test: `my_test`)\n").unwrap();
        let gates = GateReport {
            total: 1,
            passing: vec!["my_test".to_string()],
            failing: vec![],
        };
        let n = tick_criteria_on_pass(&path, &gates).unwrap();
        assert_eq!(n, 0, "already-checked items should not re-trigger writes");
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, "- [x] All set (test: `my_test`)\n");
    }

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
        assert!(report
            .passing
            .contains(&"test_v1_classdb_full_property_enumeration".to_string()));
        assert!(report
            .failing
            .contains(&"test_v1_notification_dispatch_ordering".to_string()));
        assert!(report
            .failing
            .contains(&"test_v1_weakref_auto_invalidates_on_free".to_string()));
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
            failing: vec!["test_notif".to_string(), "test_weakref".to_string()],
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
            "V1 gate: Object Model — Object.notification() dispatch with correct ordering"
                .to_string(),
        ];
        let existing_keys: Vec<String> = vec![];

        let queue = QueueReport {
            open: 5,
            in_progress: 2,
            closed: 100,
            ready_unassigned: 3,
        };

        let recs = generate_recommendations(
            &gates,
            &dynamic_gates,
            &existing_titles,
            &existing_keys,
            &queue,
        );
        assert_eq!(recs.len(), 1, "should skip bead with matching title");
        assert!(recs[0].gate_key.contains("weakref"));
    }

    // ─── C4: plan audit / lint ──────────────────────────────────────────

    #[test]
    fn test_audit_report_flags_missing_acceptance_and_dangling_deps() {
        // A non-existent project root → no DB → existing_keys empty, so a dep
        // pointing outside the batch is reported dangling.
        let report = PlanReport {
            timestamp: "t".into(),
            parity: empty_parity(),
            gates: empty_gates(),
            queue: QueueReport {
                open: 0,
                in_progress: 0,
                closed: 0,
                ready_unassigned: 0,
            },
            recommendations: vec![
                Recommendation {
                    title: "no acceptance".into(),
                    priority: 1,
                    labels: vec![],
                    description: String::new(),
                    acceptance_command: String::new(), // missing
                    gate_key: "key-a".into(),
                    reason: String::new(),
                    depends_on: vec![],
                },
                Recommendation {
                    title: "dangling dep".into(),
                    priority: 1,
                    labels: vec![],
                    description: String::new(),
                    acceptance_command: "cargo test --test t".into(),
                    gate_key: "key-b".into(),
                    depends_on: vec!["key-ghost".into()],
                    reason: String::new(),
                },
            ],
            phase: Phase::V1Active,
            active_phase_label: String::new(),
        };
        let findings = audit_report(std::path::Path::new("/nonexistent-xyz"), &report);
        assert!(findings
            .iter()
            .any(|f| f.contains("MISSING-ACCEPTANCE") && f.contains("key-a")));
        assert!(findings
            .iter()
            .any(|f| f.contains("DANGLING-DEP") && f.contains("key-ghost")));
    }

    // ─── C3: silent-stall detection ─────────────────────────────────────

    #[test]
    fn test_phase_has_unticked_test_markers() {
        use crate::prd_parser::CriteriaItem;
        let with_marker = vec![CriteriaItem {
            section: "S".into(),
            text: "Auth required (test: `editor_auth_required_test`)".into(),
            checked: false,
            line_number: 1,
        }];
        // Markers present + no test_binaries → silent-stall risk.
        assert!(phase_has_unticked_test_markers(&with_marker, &[], &[]));
        // test_binaries configured → analysis runs → no silent stall.
        assert!(!phase_has_unticked_test_markers(
            &with_marker,
            &["editor_agent_integration_test".to_string()],
            &[]
        ));
        // Already-checked markers don't count.
        let checked = vec![CriteriaItem {
            section: "S".into(),
            text: "Done (test: `t`)".into(),
            checked: true,
            line_number: 1,
        }];
        assert!(!phase_has_unticked_test_markers(&checked, &[], &[]));
        // No markers at all → benign empty/prose phase, not a stall.
        let no_marker = vec![CriteriaItem {
            section: "S".into(),
            text: "Some prose criterion".into(),
            checked: false,
            line_number: 1,
        }];
        assert!(!phase_has_unticked_test_markers(&no_marker, &[], &[]));
    }

    // ─── B6: planner-key extraction + dependency validation ─────────────

    #[test]
    fn test_extract_planner_key() {
        assert_eq!(
            extract_planner_key("IMPLEMENT: foo\n\n[planner-key: v1-obj-foo]").as_deref(),
            Some("v1-obj-foo")
        );
        // Case-normalized
        assert_eq!(
            extract_planner_key("[planner-key: V1-OBJ-Bar]").as_deref(),
            Some("v1-obj-bar")
        );
        // Malformed / missing
        assert_eq!(extract_planner_key("no marker here"), None);
        assert_eq!(extract_planner_key("[planner-key: ]"), None);
    }

    fn rec_with(key: &str, deps: Vec<&str>) -> Recommendation {
        Recommendation {
            title: format!("t-{key}"),
            priority: 1,
            labels: vec![],
            description: String::new(),
            acceptance_command: String::new(),
            gate_key: key.to_string(),
            reason: String::new(),
            depends_on: deps.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn test_validate_dependencies_resolves_within_batch() {
        let recs = vec![rec_with("key-a", vec![]), rec_with("key-b", vec!["key-a"])];
        assert!(validate_dependencies(&recs, &[]).is_empty());
    }

    #[test]
    fn test_validate_dependencies_resolves_against_existing() {
        let recs = vec![rec_with("key-b", vec!["key-a"])];
        let existing = vec!["...[planner-key: key-a]...".to_string()];
        assert!(validate_dependencies(&recs, &existing).is_empty());
    }

    #[test]
    fn test_validate_dependencies_flags_dangling() {
        let recs = vec![rec_with("key-b", vec!["key-missing"])];
        let warnings = validate_dependencies(&recs, &[]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("key-missing"));
        assert!(warnings[0].contains("key-b"));
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
        let existing_keys =
            vec!["some bead with [planner-key: v1-obj-notif] in description".to_string()];

        let queue = QueueReport {
            open: 5,
            in_progress: 2,
            closed: 100,
            ready_unassigned: 3,
        };

        let recs = generate_recommendations(
            &gates,
            &dynamic_gates,
            &existing_titles,
            &existing_keys,
            &queue,
        );
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
            phases: vec![],
            uses_phase_chain: false,
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
            phases: vec![],
            uses_phase_chain: false,
        };
        assert_eq!(
            determine_phase(&gates, &parity, &config),
            Phase::V1NearlyDone
        );
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
            phases: vec![],
            uses_phase_chain: false,
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
            phases: vec![],
            uses_phase_chain: false,
        };
        assert_eq!(
            determine_phase(&gates, &parity, &config),
            Phase::V1NearlyDone
        );
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
            phases: vec![],
            uses_phase_chain: false,
        };
        assert_eq!(
            determine_phase(&gates, &parity, &config),
            Phase::V1NearlyDone
        );
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
        let root =
            std::env::temp_dir().join(format!("patina-planner-next-phase-{}", std::process::id()));
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
            phases: vec![],
            uses_phase_chain: false,
        };
        let queue = QueueReport {
            open: 0,
            in_progress: 0,
            closed: 0,
            ready_unassigned: 0,
        };

        let recs = generate_next_phase_recommendations(&root, &config, &[], &queue);
        assert_eq!(
            recs.len(),
            2,
            "duplicate sources should not duplicate beads"
        );
        assert_eq!(
            recs.iter()
                .filter(|r| r.gate_key == "phase9-benchmark-dashboards")
                .count(),
            1
        );
        assert!(recs.iter().all(|r| !r.acceptance_command.is_empty()));
        assert!(recs
            .iter()
            .all(|r| r.labels.iter().any(|l| l.starts_with("phase"))));

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
            phases: vec![],
            uses_phase_chain: false,
        };
        let queue = QueueReport {
            open: 0,
            in_progress: 0,
            closed: 0,
            ready_unassigned: 0,
        };
        let existing_titles = vec!["Produce the first real 3D demo parity report".to_string()];

        let recs = generate_next_phase_recommendations(&root, &config, &existing_titles, &queue);
        assert!(
            recs.is_empty(),
            "existing active title should suppress recommendation"
        );

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
        assert!(
            (report.overall - 0.0).abs() < f64::EPSILON,
            "empty input → 0% parity"
        );
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
            input.push_str(&format!(
                "scene_{i:03}    50    {matched}    {pct:.1}%\n",
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
            "scene 10 5 50.0", // missing % sign
            "OVERALL",         // incomplete OVERALL line
            "\n\n\n\n",        // only newlines
        ];
        for input in &inputs {
            let report = parse_parity_output(input);
            // Must not panic — zero-value report is fine
            assert!(
                report.overall >= 0.0,
                "garbled input must not produce negative parity"
            );
        }
    }

    /// Negative: garbled gate input must not panic.
    #[test]
    fn test_gate_parser_garbled_input_no_panic() {
        let inputs = [
            "test ... ok",             // missing test name
            "test_foo ... maybe",      // unknown status
            "testing test_bar ... ok", // wrong prefix
            "💥\0garbage\n",
            "test  ... FAILED", // empty name
        ];
        for input in &inputs {
            let report = parse_gate_output(input);
            // Must not panic
            assert!(
                report.total < 1000,
                "garbled input should not produce huge totals"
            );
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
                SceneParity {
                    name: "perfect".into(),
                    total: 25,
                    matched: 25,
                    parity: 100.0,
                },
                SceneParity {
                    name: "also_perfect".into(),
                    total: 25,
                    matched: 25,
                    parity: 100.0,
                },
            ],
        };
        let queue = QueueReport {
            open: 2,
            in_progress: 1,
            closed: 50,
            ready_unassigned: 1,
        };
        let recs = generate_parity_recommendations(&parity, &[], &[], &queue);
        assert!(
            recs.is_empty(),
            "100% scenes should not generate recommendations"
        );
    }

    /// Parity recommendations: scenes below 100% SHOULD generate recs.
    #[test]
    fn test_parity_recs_for_imperfect_scenes() {
        let parity = ParityReport {
            overall: 95.0,
            total: 100,
            matched: 95,
            scenes: vec![
                SceneParity {
                    name: "good".into(),
                    total: 50,
                    matched: 50,
                    parity: 100.0,
                },
                SceneParity {
                    name: "needs_work".into(),
                    total: 50,
                    matched: 45,
                    parity: 90.0,
                },
            ],
        };
        let queue = QueueReport {
            open: 2,
            in_progress: 1,
            closed: 50,
            ready_unassigned: 1,
        };
        let recs = generate_parity_recommendations(&parity, &[], &[], &queue);
        assert_eq!(recs.len(), 1);
        assert!(recs[0].title.contains("needs_work"));
        assert!(recs[0].gate_key.contains("parity-gap-needs_work"));
        // Description must be implementation-directive and carry the scoped test.
        assert!(recs[0]
            .description
            .contains("[planner-key: parity-gap-needs_work]"));
        assert!(recs[0]
            .description
            .contains("golden_needs_work_full_property_parity"));
    }

    /// A1 regression: parity beads dedup on the stable planner key, NOT the
    /// title. The title embeds the live percentage, so when parity ticks up
    /// (90.0% → 92.0%) a title-only check would re-seed the same bead. With
    /// key dedup, an existing bead suppresses re-seeding regardless of the
    /// percentage in its title.
    #[test]
    fn test_parity_recs_dedup_by_key_not_volatile_title() {
        let parity = ParityReport {
            overall: 92.0,
            total: 50,
            matched: 46,
            scenes: vec![SceneParity {
                name: "needs_work".into(),
                total: 50,
                matched: 46, // parity improved since the bead was created
                parity: 92.0,
            }],
        };
        let queue = QueueReport {
            open: 1,
            in_progress: 0,
            closed: 0,
            ready_unassigned: 1,
        };
        // Existing bead's title says 90.0% (stale), but its key is stable.
        let existing_titles = vec!["Close parity gap in needs_work (currently 90.0%)".to_string()];
        let existing_keys = vec!["IMPLEMENT ...\n[planner-key: parity-gap-needs_work]".to_string()];

        // Key-based dedup suppresses re-seeding even though the title % differs.
        let recs = generate_parity_recommendations(&parity, &[], &existing_keys, &queue);
        assert!(
            recs.is_empty(),
            "stable key must dedup despite volatile title %"
        );

        // And the legacy title-fallback still works when no key is present.
        let recs2 = generate_parity_recommendations(&parity, &existing_titles, &[], &queue);
        assert!(recs2.is_empty(), "legacy title fallback must still dedup");
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
        let next_fn = source
            .find("fn generate_next_phase_recommendations(")
            .unwrap();
        let fn_end = source[next_fn..].find("\n}").unwrap_or(1000);
        let body = &source[next_fn..next_fn + fn_end];

        // The function receives existing_titles — the caller must pass active-only titles
        assert!(
            body.contains("existing_titles"),
            "must accept existing_titles for dedup"
        );

        // Verify callers pass active_titles, not all_titles
        let all_callers = source
            .matches("generate_next_phase_recommendations(")
            .count();
        let active_callers = source
            .matches("&active_titles,\n            &queue,")
            .count();
        assert!(
            active_callers >= 2,
            "all callers must pass active_titles (open/in-progress only), found {active_callers} of {all_callers}"
        );
    }

    /// Verify that port-plan deliverable recommendations chain dependencies
    /// within the same phase so sequential items depend on the previous one.
    #[test]
    fn test_next_phase_recommendations_chain_dependencies() {
        let tmpdir = tempfile::tempdir().unwrap();
        let plan_path = tmpdir.path().join("PORT_PLAN.md");

        // Minimal port plan with three Phase 6 deliverables in order.
        // The parser requires a `### Deliverables` subsection under `## Phase 6`.
        fs::write(
            &plan_path,
            r#"# Port Plan

## Phase 6

### Deliverables

- first 3D crate set,
- 3D fixture corpus,
- 3D demo parity report
"#,
        )
        .unwrap();

        let config = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "V1".to_string(),
            completion_conditions: vec![],
            next_sources: vec![plan_path.clone()],
            phases: vec![],
            uses_phase_chain: false,
        };

        let queue = QueueReport {
            open: 0,
            in_progress: 0,
            closed: 0,
            ready_unassigned: 0,
        };

        let recs = generate_next_phase_recommendations(
            tmpdir.path(),
            &config,
            &[], // no existing titles
            &queue,
        );

        // We should get at least 2 recommendations (the deliverables)
        assert!(
            recs.len() >= 2,
            "expected at least 2 port-plan recommendations, got {}",
            recs.len()
        );

        // The first recommendation should have no dependencies (or only previous
        // phase deps). Subsequent ones should depend on the previous gate_key.
        let first = &recs[0];
        assert!(
            first.depends_on.is_empty(),
            "first deliverable in phase should have no intra-phase dependency, got {:?}",
            first.depends_on
        );

        for i in 1..recs.len() {
            let prev_key = &recs[i - 1].gate_key;
            assert!(
                recs[i].depends_on.contains(prev_key),
                "rec[{}] ({}) should depend on rec[{}] ({}), but depends_on = {:?}",
                i,
                recs[i].gate_key,
                i - 1,
                prev_key,
                recs[i].depends_on
            );
        }
    }

    /// Regression: verify the planner source code has no `depends_on: vec![]`
    /// in the port-plan deliverable generator — it must use the chained value.
    #[test]
    fn test_port_plan_generator_does_not_hardcode_empty_depends_on() {
        let source = include_str!("planner.rs");
        // Find the deliverable loop body (between "for d in deliverables" and closing brace)
        let marker = "for d in deliverables";
        let start = source
            .find(marker)
            .expect("deliverable loop must exist in planner.rs");
        // Look at the next 1500 chars which covers the loop body
        let body = &source[start..std::cmp::min(start + 1500, source.len())];
        assert!(
            !body.contains("depends_on: vec![]"),
            "port-plan deliverable generator must chain dependencies, not hardcode empty vec. \
             Found 'depends_on: vec![]' in the deliverable loop body."
        );
    }

    // ─── Phase-chain integration ──────────────────────────────────────────

    #[test]
    fn test_phase_is_complete_all_criteria_checked() {
        use crate::project_config::{AnalysisSource, Phase};
        use std::path::PathBuf;

        let phase = Phase {
            label: "test".to_string(),
            execution_map: Some(PathBuf::from("MAP.md")),
            exit_criteria: Some(PathBuf::from("EXIT.md")),
            analysis: AnalysisSource::FromCriteria,
            completion: vec![CompletionCondition::AllCriteriaChecked],
            test_binaries: vec![],
            test_packages: vec![],
            lib: false,
        };

        // No criteria → not complete (avoid false-positive on empty file).
        assert!(!phase_is_complete(&phase, &[], None, None));

        // All checked → complete.
        let all_checked = vec![
            prd_parser::CriteriaItem {
                section: "S".into(),
                text: "a".into(),
                checked: true,
                line_number: 1,
            },
            prd_parser::CriteriaItem {
                section: "S".into(),
                text: "b".into(),
                checked: true,
                line_number: 2,
            },
        ];
        assert!(phase_is_complete(&phase, &all_checked, None, None));

        // Any unchecked → not complete.
        let mut one_unchecked = all_checked.clone();
        one_unchecked[0].checked = false;
        assert!(!phase_is_complete(&phase, &one_unchecked, None, None));
    }

    #[test]
    fn test_config_uses_phase_chain_reads_explicit_flag() {
        use crate::project_config::{AnalysisSource, Phase};
        use std::path::PathBuf;

        // Legacy: uses_phase_chain = false.
        let legacy = ProjectPlannerConfig {
            analysis: vec![],
            criteria_files: vec![],
            execution_map_files: vec![],
            phase_label: "v1".to_string(),
            completion_conditions: vec![CompletionCondition::AllGatesPassing],
            next_sources: vec![],
            phases: vec![Phase {
                label: "v1".into(),
                execution_map: None,
                exit_criteria: None,
                analysis: AnalysisSource::Commands(vec![]),
                completion: vec![CompletionCondition::AllGatesPassing],
                test_binaries: vec![],
                test_packages: vec![],
                lib: false,
            }],
            uses_phase_chain: false,
        };
        assert!(!config_uses_phase_chain(&legacy));

        // New: explicit flag set, even if analysis/completion are "legacy-shaped".
        // This is the test the reviewer flagged in S3 — a [[phase]] block using
        // only Commands + all_gates_passing must still route through the new path.
        let new = ProjectPlannerConfig {
            phases: vec![Phase {
                label: "v2-only-commands".into(),
                execution_map: Some(PathBuf::from("MAP.md")),
                exit_criteria: Some(PathBuf::from("EXIT.md")),
                analysis: AnalysisSource::Commands(vec![]),
                completion: vec![CompletionCondition::AllGatesPassing],
                test_binaries: vec![],
                test_packages: vec![],
                lib: false,
            }],
            uses_phase_chain: true,
            ..legacy.clone()
        };
        assert!(config_uses_phase_chain(&new));
    }

    /// Phase walk picks the first incomplete phase. Tests the
    /// load + completion logic directly (no DB).
    #[test]
    fn test_phase_walk_selects_first_incomplete_phase() {
        use crate::project_config::{AnalysisSource, Phase};
        use std::path::PathBuf;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Phase 1: all checked (complete).
        fs::write(
            root.join("p1_exit.md"),
            "- [x] Done (test: `t1`)\n- [x] Also done (test: `t2`)\n",
        )
        .unwrap();
        fs::write(
            root.join("p1_map.md"),
            "## Now\n\n1. `p1-a` Done\n   Acceptance: (test: `t1`)\n",
        )
        .unwrap();
        // Phase 2: unchecked (incomplete).
        fs::write(
            root.join("p2_exit.md"),
            "- [ ] Pending (test: `t3`)\n- [ ] More (test: `t4`)\n",
        )
        .unwrap();
        fs::write(
            root.join("p2_map.md"),
            "## Now\n\n1. `p2-a` Implement\n   Acceptance: (test: `t3`)\n",
        )
        .unwrap();

        let phases = vec![
            Phase {
                label: "p1".into(),
                execution_map: Some(PathBuf::from("p1_map.md")),
                exit_criteria: Some(PathBuf::from("p1_exit.md")),
                analysis: AnalysisSource::FromCriteria,
                completion: vec![CompletionCondition::AllCriteriaChecked],
                test_binaries: vec![],
                test_packages: vec![],
                lib: false,
            },
            Phase {
                label: "p2".into(),
                execution_map: Some(PathBuf::from("p2_map.md")),
                exit_criteria: Some(PathBuf::from("p2_exit.md")),
                analysis: AnalysisSource::FromCriteria,
                completion: vec![CompletionCondition::AllCriteriaChecked],
                test_binaries: vec![],
                test_packages: vec![],
                lib: false,
            },
        ];

        // The walk: skip complete, return first incomplete.
        let mut active: Option<&Phase> = None;
        for ph in &phases {
            let crit = load_phase_criteria(root, ph);
            if !phase_is_complete(ph, &crit, None, None) {
                active = Some(ph);
                break;
            }
        }
        assert!(active.is_some(), "expected an incomplete phase");
        assert_eq!(active.unwrap().label, "p2");

        // Phase 2's execution map is non-empty.
        let p2_specs = load_phase_execution_map(root, active.unwrap());
        assert_eq!(p2_specs.len(), 1);
        assert_eq!(p2_specs[0].bead_key, "p2-a");
    }

    #[test]
    fn test_tick_criteria_on_pass_writes_x_for_passing_phase_2() {
        // Sanity: when tick_criteria_on_pass runs against a phase 2 criteria
        // file whose test now passes, the box flips and the phase becomes
        // closer to completion. Mirrors the auto-advance flow at runtime.
        let tmp = tempfile::tempdir().unwrap();
        let exit = tmp.path().join("EXIT.md");
        fs::write(
            &exit,
            "- [ ] Item A (test: `passes_now`)\n- [ ] Item B (test: `still_failing`)\n",
        )
        .unwrap();
        let gates = GateReport {
            total: 2,
            passing: vec!["passes_now".to_string()],
            failing: vec!["still_failing".to_string()],
        };
        let n = tick_criteria_on_pass(&exit, &gates).unwrap();
        assert_eq!(n, 1, "exactly one should tick");
        let after = fs::read_to_string(&exit).unwrap();
        assert!(after.contains("- [x] Item A"));
        assert!(after.contains("- [ ] Item B"));
    }
}
