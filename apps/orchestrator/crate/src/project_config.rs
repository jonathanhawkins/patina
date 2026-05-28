//! Project-specific planner configuration.
//!
//! Loaded from `.orchestrator/planner.toml` at the project root.
//! Falls back to convention-based discovery when the config file is absent.

use std::path::{Path, PathBuf};

use serde::Deserialize;

// ─── Public types ──────────────────────────────────────────────────────────

/// How to parse output from an analysis command.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParserType {
    /// Tabular parity output (scene/total/match/parity columns).
    Table,
    /// `test NAME ... ok/FAILED/ignored` lines.
    TestPassFail,
    /// Just check whether the process exited 0.
    ExitCode,
}

/// A single analysis command the planner should run.
#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisCommand {
    pub name: String,
    pub cmd: String,
    /// Working directory relative to project root.
    pub workdir: String,
    pub parser: ParserType,
    /// Timeout in seconds (0 = no timeout).
    #[serde(default)]
    pub timeout_secs: u64,
}

/// Top-level planner configuration.
#[derive(Debug, Clone)]
pub struct ProjectPlannerConfig {
    pub analysis: Vec<AnalysisCommand>,
    pub criteria_files: Vec<PathBuf>,
    pub execution_map_files: Vec<PathBuf>,
    pub phase_label: String,
    pub completion_conditions: Vec<CompletionCondition>,
    pub next_sources: Vec<PathBuf>,
    /// Ordered phase chain. Always non-empty: when the TOML uses the legacy
    /// single-phase form, this contains a single synthesized Phase mirroring
    /// the legacy fields. When the TOML uses [[phase]] blocks, this contains
    /// one entry per block in declaration order.
    pub phases: Vec<Phase>,
    /// True when the TOML actually declared `[[phase]]` blocks. Distinguishes
    /// the new schema from synthesized-from-legacy-fields configs, so the
    /// planner can route schema-aware code paths without sniffing phase
    /// content.
    pub uses_phase_chain: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CompletionCondition {
    AllGatesPassing,
    AllCriteriaChecked,
    ParityAbove(f64),
}

/// One phase in the planner's phase chain.
#[derive(Debug, Clone)]
pub struct Phase {
    pub label: String,
    /// `None` when the phase has no execution map (rare; only used by the
    /// legacy-fallback synthesis when no maps are configured at all).
    pub execution_map: Option<PathBuf>,
    pub exit_criteria: Option<PathBuf>,
    pub analysis: AnalysisSource,
    pub completion: Vec<CompletionCondition>,
    /// Cargo test-target (binary) names this phase's criteria tests live in,
    /// e.g. `editor_parity_bootstrap_map_test`. The planner passes these as
    /// `--test <name>` flags so criteria analysis compiles ONLY these targets
    /// instead of the whole workspace (which has 400+ integration tests and
    /// would lock the machine). When empty with `analysis = { source =
    /// "criteria" }`, the planner skips the analysis subprocess entirely
    /// rather than fall back to a `--workspace` build.
    pub test_binaries: Vec<String>,
}

/// How to determine whether each criterion is met.
#[derive(Debug, Clone)]
pub enum AnalysisSource {
    /// Derive test names from the criteria file's `(test: \`...\`)` markers
    /// and run them via cargo nextest in a single invocation.
    FromCriteria,
    /// Hand-authored shell commands. Used by the legacy single-phase config.
    Commands(Vec<AnalysisCommand>),
}

// ─── TOML schema (intermediate) ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TomlConfig {
    analysis: Option<TomlAnalysis>,
    criteria: Option<TomlFileList>,
    execution_maps: Option<TomlFileList>,
    phases: Option<TomlPhases>,
    /// New schema: ordered list of [[phase]] blocks. When present, takes
    /// precedence over the legacy single-phase fields above.
    #[serde(default)]
    phase: Vec<TomlPhaseV2>,
}

#[derive(Debug, Deserialize)]
struct TomlAnalysis {
    commands: Vec<AnalysisCommand>,
}

#[derive(Debug, Deserialize)]
struct TomlFileList {
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TomlPhases {
    label: Option<String>,
    completion: Option<Vec<toml::Value>>,
    next_sources: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TomlPhaseV2 {
    label: String,
    execution_map: String,
    #[serde(default)]
    exit_criteria: Option<String>,
    #[serde(default)]
    analysis: Option<TomlPhaseAnalysis>,
    #[serde(default)]
    completion: Option<Vec<toml::Value>>,
    #[serde(default)]
    test_binaries: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TomlPhaseAnalysis {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    commands: Option<Vec<AnalysisCommand>>,
}

// ─── Loading ───────────────────────────────────────────────────────────────

const CONFIG_PATH: &str = ".orchestrator/planner.toml";

/// Load planner config from `.orchestrator/planner.toml`, or fall back to
/// convention-based discovery.
pub fn load(project_root: &Path) -> ProjectPlannerConfig {
    let config_file = project_root.join(CONFIG_PATH);
    if config_file.exists() {
        match std::fs::read_to_string(&config_file) {
            Ok(content) => match load_from_toml(&content, project_root) {
                Ok(cfg) => return cfg,
                Err(e) => {
                    eprintln!(
                        "planner: failed to parse {}: {e}; using convention fallback",
                        config_file.display()
                    );
                }
            },
            Err(e) => {
                eprintln!(
                    "planner: failed to read {}: {e}; using convention fallback",
                    config_file.display()
                );
            }
        }
    }
    convention_fallback(project_root)
}

fn load_from_toml(
    content: &str,
    project_root: &Path,
) -> std::result::Result<ProjectPlannerConfig, String> {
    let raw: TomlConfig = toml::from_str(content).map_err(|e| e.to_string())?;

    let analysis = raw.analysis.map(|a| a.commands).unwrap_or_default();

    let criteria_files: Vec<PathBuf> = raw
        .criteria
        .map(|c| c.files.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default();

    let execution_map_files: Vec<PathBuf> = raw
        .execution_maps
        .map(|c| c.files.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default();

    let phases_legacy = raw.phases.unwrap_or(TomlPhases {
        label: None,
        completion: None,
        next_sources: None,
    });

    let phase_label = phases_legacy.label.unwrap_or_else(|| "default".to_string());

    let completion_conditions: Vec<CompletionCondition> = phases_legacy
        .completion
        .unwrap_or_default()
        .into_iter()
        .filter_map(parse_completion_condition)
        .collect();

    let next_sources: Vec<PathBuf> = phases_legacy
        .next_sources
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();

    // Build the phases vector. Two paths:
    //   1. New schema: [[phase]] blocks present → one Phase per block.
    //   2. Legacy schema: synthesize a single Phase from the flat fields so
    //      analyze() can walk a uniform Vec<Phase> regardless of source.
    let uses_phase_chain = !raw.phase.is_empty();
    let phases: Vec<Phase> = if uses_phase_chain {
        raw.phase
            .into_iter()
            .map(|p| {
                let analysis_source = match p.analysis {
                    Some(a) => {
                        if let Some(cmds) = a.commands {
                            AnalysisSource::Commands(cmds)
                        } else if a.source.as_deref() == Some("criteria") {
                            AnalysisSource::FromCriteria
                        } else {
                            AnalysisSource::Commands(vec![])
                        }
                    }
                    None => AnalysisSource::Commands(vec![]),
                };
                let completion = p
                    .completion
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(parse_completion_condition)
                    .collect();
                Phase {
                    label: p.label,
                    execution_map: Some(PathBuf::from(p.execution_map)),
                    exit_criteria: p.exit_criteria.map(PathBuf::from),
                    analysis: analysis_source,
                    completion,
                    test_binaries: p.test_binaries,
                }
            })
            .collect()
    } else {
        // Synthesize one phase from legacy fields. The execution map is
        // optional — when no maps are configured the phase still exists
        // but produces no recommendations.
        let exec_map = execution_map_files.first().cloned();
        let exit_crit = criteria_files.first().cloned();
        let synth_analysis = AnalysisSource::Commands(analysis.clone());
        vec![Phase {
            label: phase_label.clone(),
            execution_map: exec_map,
            exit_criteria: exit_crit,
            analysis: synth_analysis,
            completion: completion_conditions.clone(),
            test_binaries: vec![],
        }]
    };

    // Verify that referenced files exist (warn only)
    for f in criteria_files
        .iter()
        .chain(execution_map_files.iter())
        .chain(next_sources.iter())
    {
        let abs = project_root.join(f);
        if !abs.exists() {
            eprintln!("planner: config references missing file: {}", abs.display());
        }
    }
    for ph in &phases {
        if let Some(em) = &ph.execution_map {
            let emp = project_root.join(em);
            if !emp.exists() {
                eprintln!(
                    "planner: phase '{}' references missing execution_map: {}",
                    ph.label,
                    emp.display()
                );
            }
        }
        if let Some(ec) = &ph.exit_criteria {
            let ecp = project_root.join(ec);
            if !ecp.exists() {
                eprintln!(
                    "planner: phase '{}' references missing exit_criteria: {}",
                    ph.label,
                    ecp.display()
                );
            }
        }
    }

    Ok(ProjectPlannerConfig {
        analysis,
        criteria_files,
        execution_map_files,
        phase_label,
        completion_conditions,
        next_sources,
        phases,
        uses_phase_chain,
    })
}

fn parse_completion_condition(val: toml::Value) -> Option<CompletionCondition> {
    match val {
        toml::Value::String(s) if s == "all_gates_passing" => {
            Some(CompletionCondition::AllGatesPassing)
        }
        toml::Value::String(s) if s == "all_criteria_checked" => {
            Some(CompletionCondition::AllCriteriaChecked)
        }
        toml::Value::Table(t) => {
            if let Some(toml::Value::Float(f)) = t.get("parity_threshold") {
                Some(CompletionCondition::ParityAbove(*f))
            } else if let Some(toml::Value::Integer(i)) = t.get("parity_threshold") {
                Some(CompletionCondition::ParityAbove(*i as f64))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Convention-based fallback: scan `prd/` for well-known file patterns.
pub fn convention_fallback(project_root: &Path) -> ProjectPlannerConfig {
    let prd_dir = project_root.join("prd");
    let mut criteria_files = Vec::new();
    let mut execution_map_files = Vec::new();
    let mut next_sources = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&prd_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_uppercase();
            let rel = PathBuf::from("prd").join(entry.file_name());
            if name.contains("EXIT_CRITERIA") {
                criteria_files.push(rel);
            } else if name.contains("BEAD_EXECUTION_MAP") {
                execution_map_files.push(rel);
            } else if name.contains("PLAN") && !name.contains("AGENT") {
                next_sources.push(rel);
            }
        }
    }

    // Sort for deterministic ordering
    criteria_files.sort();
    execution_map_files.sort();
    next_sources.sort();

    let phases = vec![Phase {
        label: "default".to_string(),
        execution_map: execution_map_files.first().cloned(),
        exit_criteria: criteria_files.first().cloned(),
        analysis: AnalysisSource::Commands(vec![]),
        completion: vec![],
        test_binaries: vec![],
    }];

    ProjectPlannerConfig {
        analysis: vec![],
        criteria_files,
        execution_map_files,
        phase_label: "default".to_string(),
        completion_conditions: vec![],
        next_sources,
        phases,
        uses_phase_chain: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_from_toml_basic() {
        let toml_str = r#"
[analysis]
commands = [
  { name = "parity", cmd = "cargo test --test oracle_test", workdir = "engine", parser = "table" },
  { name = "gates", cmd = "cargo test --test gate_test -- --ignored", workdir = "engine", parser = "test_pass_fail" },
]

[criteria]
files = ["prd/EXIT_CRITERIA.md"]

[execution_maps]
files = ["prd/EXECUTION_MAP.md"]

[phases]
label = "v1"
completion = ["all_gates_passing", { parity_threshold = 98.0 }]
next_sources = ["prd/PLAN.md"]
"#;
        let cfg = load_from_toml(toml_str, Path::new("/tmp/fake")).unwrap();
        assert_eq!(cfg.analysis.len(), 2);
        assert_eq!(cfg.analysis[0].name, "parity");
        assert_eq!(cfg.analysis[0].parser, ParserType::Table);
        assert_eq!(cfg.analysis[1].parser, ParserType::TestPassFail);
        assert_eq!(cfg.criteria_files.len(), 1);
        assert_eq!(cfg.execution_map_files.len(), 1);
        assert_eq!(cfg.phase_label, "v1");
        assert_eq!(cfg.completion_conditions.len(), 2);
        assert_eq!(
            cfg.completion_conditions[0],
            CompletionCondition::AllGatesPassing
        );
        assert_eq!(
            cfg.completion_conditions[1],
            CompletionCondition::ParityAbove(98.0)
        );
        assert_eq!(cfg.next_sources.len(), 1);
    }

    #[test]
    fn test_load_from_toml_minimal() {
        let toml_str = "";
        let cfg = load_from_toml(toml_str, Path::new("/tmp/fake")).unwrap();
        assert!(cfg.analysis.is_empty());
        assert!(cfg.criteria_files.is_empty());
        assert_eq!(cfg.phase_label, "default");
    }

    #[test]
    fn test_convention_fallback_finds_prd_files() {
        // Use the real project root to verify convention discovery
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let prd_dir = project_root.join("prd");
        if !prd_dir.exists() {
            return; // Skip if not in the full repo
        }
        let cfg = convention_fallback(project_root);
        // Should find V1_EXIT_CRITERIA.md
        assert!(
            cfg.criteria_files
                .iter()
                .any(|f| f.to_string_lossy().contains("EXIT_CRITERIA")),
            "should discover exit criteria files, got: {:?}",
            cfg.criteria_files
        );
        // Should find the live execution map, not the V1 handoff doc
        assert!(
            cfg.execution_map_files
                .iter()
                .any(|f| f.to_string_lossy().contains("BEAD_EXECUTION_MAP")),
            "should discover BEAD_EXECUTION_MAP.md, got: {:?}",
            cfg.execution_map_files
        );
        assert!(
            cfg.execution_map_files
                .iter()
                .all(|f| !f.to_string_lossy().contains("V1_EXIT_EXECUTION_MAP")),
            "should not use V1_EXIT_EXECUTION_MAP.md as an active execution map: {:?}",
            cfg.execution_map_files
        );
    }

    #[test]
    fn test_legacy_config_synthesizes_single_phase() {
        // The single-phase legacy form must still produce a one-Phase chain
        // so the new phase-walk in analyze() can iterate uniformly.
        let toml_str = r#"
[criteria]
files = ["prd/EXIT.md"]

[execution_maps]
files = ["prd/MAP.md"]

[phases]
label = "v1"
completion = ["all_gates_passing"]
"#;
        let cfg = load_from_toml(toml_str, Path::new("/tmp/fake")).unwrap();
        assert!(
            !cfg.uses_phase_chain,
            "legacy form must NOT set uses_phase_chain"
        );
        assert_eq!(cfg.phases.len(), 1);
        assert_eq!(cfg.phases[0].label, "v1");
        assert_eq!(
            cfg.phases[0].execution_map,
            Some(PathBuf::from("prd/MAP.md"))
        );
        assert_eq!(
            cfg.phases[0].exit_criteria,
            Some(PathBuf::from("prd/EXIT.md"))
        );
        assert_eq!(
            cfg.phases[0].completion,
            vec![CompletionCondition::AllGatesPassing]
        );
    }

    #[test]
    fn test_new_phase_chain_form() {
        // The new [[phase]] form parses into the same Phase shape as the
        // legacy synthesis. Two ordered phases, each with its own criteria,
        // execution map, analysis source, and completion condition.
        let toml_str = r#"
[[phase]]
label = "editor-agent"
execution_map = "prd/EDITOR_AGENT_EXECUTION_MAP.md"
exit_criteria = "prd/EDITOR_AGENT_EXIT.md"
analysis = { source = "criteria" }
completion = ["all_criteria_checked"]

[[phase]]
label = "editor-parity-bootstrap"
execution_map = "prd/EDITOR_PARITY_BOOTSTRAP_MAP.md"
exit_criteria = "prd/EDITOR_PARITY_BOOTSTRAP_EXIT.md"
analysis = { source = "criteria" }
completion = ["all_criteria_checked"]
"#;
        let cfg = load_from_toml(toml_str, Path::new("/tmp/fake")).unwrap();
        assert!(
            cfg.uses_phase_chain,
            "[[phase]] form MUST set uses_phase_chain"
        );
        assert_eq!(cfg.phases.len(), 2);
        assert_eq!(cfg.phases[0].label, "editor-agent");
        assert!(matches!(
            cfg.phases[0].analysis,
            AnalysisSource::FromCriteria
        ));
        assert_eq!(
            cfg.phases[0].completion,
            vec![CompletionCondition::AllCriteriaChecked]
        );
        assert_eq!(cfg.phases[1].label, "editor-parity-bootstrap");
        assert_eq!(
            cfg.phases[1].execution_map,
            Some(PathBuf::from("prd/EDITOR_PARITY_BOOTSTRAP_MAP.md"))
        );
    }

    #[test]
    fn test_phase_test_binaries_parsed() {
        // The lock-fix relies on test_binaries scoping the analysis build.
        // Confirm the field round-trips from TOML.
        let toml_str = r#"
[[phase]]
label = "editor-parity-bootstrap"
execution_map = "prd/MAP.md"
exit_criteria = "prd/EXIT.md"
analysis = { source = "criteria" }
test_binaries = ["editor_parity_bootstrap_map_test"]
completion = ["all_criteria_checked"]

[[phase]]
label = "no-binaries"
execution_map = "prd/MAP2.md"
analysis = { source = "criteria" }
completion = ["all_criteria_checked"]
"#;
        let cfg = load_from_toml(toml_str, Path::new("/tmp/fake")).unwrap();
        assert_eq!(
            cfg.phases[0].test_binaries,
            vec!["editor_parity_bootstrap_map_test".to_string()]
        );
        // Omitted test_binaries defaults to empty (→ analysis skipped, never --workspace).
        assert!(cfg.phases[1].test_binaries.is_empty());
    }

    #[test]
    fn test_phase_with_inline_analysis_commands() {
        // Legacy-style hand-authored commands inside a [[phase]] block.
        let toml_str = r#"
[[phase]]
label = "v1-legacy"
execution_map = "prd/MAP.md"
exit_criteria = "prd/EXIT.md"
completion = ["all_gates_passing", { parity_threshold = 98.0 }]

[[phase.analysis.commands]]
name = "parity"
cmd = "cargo test --test oracle"
workdir = "engine-rs"
parser = "table"
"#;
        let cfg = load_from_toml(toml_str, Path::new("/tmp/fake")).unwrap();
        assert_eq!(cfg.phases.len(), 1);
        match &cfg.phases[0].analysis {
            AnalysisSource::Commands(cmds) => {
                assert_eq!(cmds.len(), 1);
                assert_eq!(cmds[0].name, "parity");
            }
            AnalysisSource::FromCriteria => {
                panic!("expected Commands analysis source");
            }
        }
        assert_eq!(cfg.phases[0].completion.len(), 2);
    }

    #[test]
    fn test_convention_fallback_empty_dir() {
        let cfg = convention_fallback(Path::new("/tmp/nonexistent-project-abc123"));
        assert!(cfg.criteria_files.is_empty());
        assert!(cfg.execution_map_files.is_empty());
    }
}
