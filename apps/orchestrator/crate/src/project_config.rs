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
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CompletionCondition {
    AllGatesPassing,
    ParityAbove(f64),
}

// ─── TOML schema (intermediate) ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TomlConfig {
    analysis: Option<TomlAnalysis>,
    criteria: Option<TomlFileList>,
    execution_maps: Option<TomlFileList>,
    phases: Option<TomlPhases>,
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

fn load_from_toml(content: &str, project_root: &Path) -> std::result::Result<ProjectPlannerConfig, String> {
    let raw: TomlConfig = toml::from_str(content).map_err(|e| e.to_string())?;

    let analysis = raw
        .analysis
        .map(|a| a.commands)
        .unwrap_or_default();

    let criteria_files: Vec<PathBuf> = raw
        .criteria
        .map(|c| c.files.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default();

    let execution_map_files: Vec<PathBuf> = raw
        .execution_maps
        .map(|c| c.files.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default();

    let phases = raw.phases.unwrap_or(TomlPhases {
        label: None,
        completion: None,
        next_sources: None,
    });

    let phase_label = phases.label.unwrap_or_else(|| "default".to_string());

    let completion_conditions = phases
        .completion
        .unwrap_or_default()
        .into_iter()
        .filter_map(parse_completion_condition)
        .collect();

    let next_sources: Vec<PathBuf> = phases
        .next_sources
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();

    // Verify that referenced files exist (warn only)
    for f in criteria_files.iter().chain(execution_map_files.iter()).chain(next_sources.iter()) {
        let abs = project_root.join(f);
        if !abs.exists() {
            eprintln!("planner: config references missing file: {}", abs.display());
        }
    }

    Ok(ProjectPlannerConfig {
        analysis,
        criteria_files,
        execution_map_files,
        phase_label,
        completion_conditions,
        next_sources,
    })
}

fn parse_completion_condition(val: toml::Value) -> Option<CompletionCondition> {
    match val {
        toml::Value::String(s) if s == "all_gates_passing" => {
            Some(CompletionCondition::AllGatesPassing)
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

    ProjectPlannerConfig {
        analysis: vec![],
        criteria_files,
        execution_map_files,
        phase_label: "default".to_string(),
        completion_conditions: vec![],
        next_sources,
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
    fn test_convention_fallback_empty_dir() {
        let cfg = convention_fallback(Path::new("/tmp/nonexistent-project-abc123"));
        assert!(cfg.criteria_files.is_empty());
        assert!(cfg.execution_map_files.is_empty());
    }
}
