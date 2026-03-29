//! Generic PRD markdown parser.
//!
//! Extracts criteria checklists, bead specifications, and phase deliverables
//! from any markdown document following common conventions.

use regex::Regex;

// ─── Public types ──────────────────────────────────────────────────────────

/// A single checkbox item from an exit-criteria or checklist document.
#[derive(Debug, Clone)]
pub struct CriteriaItem {
    /// The `## Section` header this item falls under.
    pub section: String,
    /// The checkbox text (without the `- [ ]` / `- [x]` prefix).
    pub text: String,
    /// Whether the checkbox is checked.
    pub checked: bool,
    /// 1-based line number in the source file.
    pub line_number: usize,
}

/// A bead specification extracted from an execution map.
#[derive(Debug, Clone)]
pub struct BeadSpec {
    /// Section header (e.g. "Now", "Next", "Later", or any `##` header).
    pub section: String,
    /// The backtick-delimited key (e.g. `v1-obj-classdb`).
    pub bead_key: String,
    /// Human-readable description.
    pub description: String,
    /// Optional acceptance command (from `Acceptance:` line).
    pub acceptance_command: Option<String>,
    /// Priority derived from section name.
    pub priority: u32,
}

/// A deliverable extracted from a phase description.
#[derive(Debug, Clone)]
pub struct PhaseDeliverable {
    pub title: String,
    /// URL-safe slug auto-generated from the title.
    pub slug: String,
}

// ─── Parsing functions ─────────────────────────────────────────────────────

/// Parse checkbox items from markdown content.
///
/// Finds `- [ ]` and `- [x]` lines, tracking which `## Section` they belong to.
pub fn parse_criteria(content: &str) -> Vec<CriteriaItem> {
    let checkbox_re = Regex::new(r"(?i)^-\s+\[([ xX])\]\s+(.+)$").unwrap();
    let section_re = Regex::new(r"^##\s+(.+)$").unwrap();

    let mut items = Vec::new();
    let mut current_section = String::new();

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(cap) = section_re.captures(trimmed) {
            current_section = cap[1].trim().to_string();
        } else if let Some(cap) = checkbox_re.captures(trimmed) {
            let checked = &cap[1] != " ";
            let text = cap[2].trim().to_string();
            items.push(CriteriaItem {
                section: current_section.clone(),
                text,
                checked,
                line_number: idx + 1,
            });
        }
    }

    items
}

/// Parse bead specs from an execution map markdown.
///
/// Expects numbered items like:
/// ```text
/// 1. `bead-key` Description text
///    Acceptance: some command
/// ```
///
/// Items are grouped under `## Section` headers.
pub fn parse_execution_map(content: &str) -> Vec<BeadSpec> {
    // Match: N. `key` description
    let bead_re = Regex::new(r"^\d+\.\s+`([^`]+)`\s+(.+)$").unwrap();
    let acceptance_re = Regex::new(r"(?i)^\s*Acceptance:\s*(.+)$").unwrap();
    let section_re = Regex::new(r"^##\s+(.+)$").unwrap();
    // Also match ### sub-headers to track team sections, but use ## for priority
    let subsection_re = Regex::new(r"^###\s+(.+)$").unwrap();

    let mut specs = Vec::new();
    let mut current_section = String::new();
    let mut pending_acceptance: Option<usize> = None; // index into specs

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(cap) = section_re.captures(trimmed) {
            current_section = cap[1].trim().to_string();
            pending_acceptance = None;
        } else if subsection_re.is_match(trimmed) {
            // Sub-headers don't change the priority section
            pending_acceptance = None;
        } else if let Some(cap) = bead_re.captures(trimmed) {
            let bead_key = cap[1].to_string();
            let description = cap[2].trim().to_string();
            let priority = section_to_priority(&current_section);
            let idx = specs.len();
            specs.push(BeadSpec {
                section: current_section.clone(),
                bead_key,
                description,
                acceptance_command: None,
                priority,
            });
            pending_acceptance = Some(idx);
        } else if let Some(cap) = acceptance_re.captures(trimmed) {
            if let Some(idx) = pending_acceptance {
                specs[idx].acceptance_command = Some(cap[1].trim().to_string());
            }
        } else if !trimmed.is_empty() && !trimmed.starts_with('-') && !trimmed.starts_with('#') {
            // Non-empty non-structural line might be continuation text; don't clear pending
        }
    }

    specs
}

/// Parse deliverables from phase sections in a plan document.
///
/// Looks for `## Phase N` or `## section_prefix` headers, then extracts
/// list items from the `### Deliverables` subsection.
pub fn parse_phase_deliverables(content: &str, section_prefix: &str) -> Vec<PhaseDeliverable> {
    let section_re = Regex::new(r"^##\s+(.+)$").unwrap();
    let subsection_re = Regex::new(r"^###\s+(.+)$").unwrap();
    let list_re = Regex::new(r"^-\s+(.+)$").unwrap();

    let prefix_lower = section_prefix.to_lowercase();
    let mut deliverables = Vec::new();
    let mut in_matching_phase = false;
    let mut in_deliverables = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(cap) = section_re.captures(trimmed) {
            let header = cap[1].trim().to_lowercase();
            in_matching_phase = header.contains(&prefix_lower);
            in_deliverables = false;
        } else if let Some(cap) = subsection_re.captures(trimmed) {
            let sub = cap[1].trim().to_lowercase();
            if in_matching_phase {
                in_deliverables = sub.contains("deliverable");
            } else {
                in_deliverables = false;
            }
        } else if in_matching_phase && in_deliverables {
            if let Some(cap) = list_re.captures(trimmed) {
                let title = cap[1].trim().to_string();
                // Strip trailing commas and periods from list items
                let title = title.trim_end_matches([',', '.']).trim().to_string();
                let slug = slugify(&title);
                deliverables.push(PhaseDeliverable { title, slug });
            }
        }
    }

    deliverables
}

/// Map a section name to a priority number.
///
/// "Now" -> 1, "Next" -> 2, "Later" -> 3, anything else -> 2.
pub fn section_to_priority(section: &str) -> u32 {
    let lower = section.to_lowercase();
    if lower.starts_with("now") {
        1
    } else if lower.starts_with("next") {
        2
    } else if lower.starts_with("later") || lower.starts_with("do not") {
        3
    } else {
        2
    }
}

/// Extract a test function name from an acceptance command string.
///
/// Looks for the last token after `--ignored` or `--` in a cargo test command.
/// Falls back to the last whitespace-delimited token.
pub fn extract_test_name_from_command(cmd: &str) -> Option<String> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    // Find the position of "--ignored" or bare "--"
    let after_flag = parts
        .iter()
        .rposition(|&p| p == "--ignored" || p == "--")
        .map(|i| i + 1);

    if let Some(idx) = after_flag {
        if idx < parts.len() {
            return Some(parts[idx].to_string());
        }
    }
    // Fallback: last token
    parts.last().map(|s| s.to_string())
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_criteria_basic() {
        let md = "\
## Section A

- [x] Done item
- [ ] Todo item
- [X] Also done

## Section B

- [ ] Another todo
";
        let items = parse_criteria(md);
        assert_eq!(items.len(), 4);
        assert!(items[0].checked);
        assert_eq!(items[0].text, "Done item");
        assert_eq!(items[0].section, "Section A");
        assert!(!items[1].checked);
        assert_eq!(items[1].text, "Todo item");
        assert!(items[2].checked);
        assert_eq!(items[3].section, "Section B");
        assert!(!items[3].checked);
    }

    #[test]
    fn test_parse_criteria_empty() {
        let items = parse_criteria("# No checkboxes here\n\nJust text.");
        assert!(items.is_empty());
    }

    #[test]
    fn test_parse_execution_map_basic() {
        let md = "\
## Now

1. `key-a` Description of A
   Acceptance: cargo test --test gate -- --ignored test_a
2. `key-b` Description of B
   Acceptance: cargo test --test gate -- --ignored test_b

## Next

1. `key-c` Description of C

## Later

1. `key-d` Description of D
   Acceptance: cargo test --test other -- --ignored test_d
";
        let specs = parse_execution_map(md);
        assert_eq!(specs.len(), 4);

        assert_eq!(specs[0].bead_key, "key-a");
        assert_eq!(specs[0].description, "Description of A");
        assert_eq!(specs[0].priority, 1);
        assert_eq!(
            specs[0].acceptance_command.as_deref(),
            Some("cargo test --test gate -- --ignored test_a")
        );

        assert_eq!(specs[1].bead_key, "key-b");
        assert_eq!(specs[1].priority, 1);

        assert_eq!(specs[2].bead_key, "key-c");
        assert_eq!(specs[2].priority, 2);
        assert!(specs[2].acceptance_command.is_none());

        assert_eq!(specs[3].bead_key, "key-d");
        assert_eq!(specs[3].priority, 3);
    }

    #[test]
    fn test_parse_execution_map_real_patina_handoff_doc() {
        let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let exec_map = project_root.join("prd/V1_EXIT_EXECUTION_MAP.md");
        if !exec_map.exists() {
            return;
        }
        let content = std::fs::read_to_string(&exec_map).unwrap();
        let specs = parse_execution_map(&content);

        assert!(
            specs.is_empty(),
            "handoff execution map should not seed legacy V1 bead specs, got {} entries",
            specs.len()
        );
    }

    #[test]
    fn test_parse_criteria_real_patina() {
        let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let criteria_file = project_root.join("prd/V1_EXIT_CRITERIA.md");
        if !criteria_file.exists() {
            return;
        }
        let content = std::fs::read_to_string(&criteria_file).unwrap();
        let items = parse_criteria(&content);

        // Current Patina V1 criteria should be fully checked.
        let checked: Vec<_> = items.iter().filter(|i| i.checked).collect();
        let unchecked: Vec<_> = items.iter().filter(|i| !i.checked).collect();

        assert!(
            !checked.is_empty(),
            "should have some checked criteria items"
        );
        assert!(
            unchecked.is_empty(),
            "expected no unchecked criteria items, got {}",
            unchecked.len()
        );

        // Verify sections exist
        let sections: std::collections::HashSet<&str> =
            items.iter().map(|i| i.section.as_str()).collect();
        assert!(
            sections.iter().any(|s| s.contains("gdobject") || s.contains("Object")),
            "should have object model section"
        );
    }

    #[test]
    fn test_parse_phase_deliverables() {
        let md = "\
## Phase 5 - Broader Runtime and 3D Prep

### Objectives

Expand coverage.

### Deliverables

- improved compatibility matrix,
- broader integration fixtures,
- first audio test harness,
- initial 3D architecture spec.

### Exit Criteria

- core runtime stable.

## Phase 6 - 3D Runtime Slice

### Deliverables

- first 3D crate set,
- 3D fixture corpus.
";
        let phase5 = parse_phase_deliverables(md, "Phase 5");
        assert_eq!(phase5.len(), 4);
        assert_eq!(phase5[0].title, "improved compatibility matrix");
        assert_eq!(phase5[0].slug, "improved-compatibility-matrix");

        let phase6 = parse_phase_deliverables(md, "Phase 6");
        assert_eq!(phase6.len(), 2);
        assert_eq!(phase6[0].title, "first 3D crate set");
    }

    #[test]
    fn test_parse_phase_deliverables_real_patina() {
        let project_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let plan_file = project_root.join("prd/PORT_GODOT_TO_RUST_PLAN.md");
        if !plan_file.exists() {
            return;
        }
        let content = std::fs::read_to_string(&plan_file).unwrap();
        let phase5 = parse_phase_deliverables(&content, "Phase 5");
        assert!(
            !phase5.is_empty(),
            "should find Phase 5 deliverables in PORT_GODOT_TO_RUST_PLAN.md"
        );

        let phase6 = parse_phase_deliverables(&content, "Phase 6");
        assert!(
            !phase6.is_empty(),
            "should find Phase 6 deliverables in PORT_GODOT_TO_RUST_PLAN.md"
        );
    }

    #[test]
    fn test_section_to_priority() {
        assert_eq!(section_to_priority("Now"), 1);
        assert_eq!(section_to_priority("Now — Urgent"), 1);
        assert_eq!(section_to_priority("Next"), 2);
        assert_eq!(section_to_priority("Later"), 3);
        assert_eq!(section_to_priority("Do Not Do Yet"), 3);
        assert_eq!(section_to_priority("Something Else"), 2);
    }

    #[test]
    fn test_extract_test_name_from_command() {
        assert_eq!(
            extract_test_name_from_command(
                "cargo test --test v1_acceptance_gate_test -- --ignored test_v1_classdb"
            ),
            Some("test_v1_classdb".to_string())
        );
        assert_eq!(
            extract_test_name_from_command("cargo test -- test_something"),
            Some("test_something".to_string())
        );
        assert_eq!(
            extract_test_name_from_command("just run it"),
            Some("it".to_string())
        );
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World!"), "hello-world");
        assert_eq!(slugify("first 3D crate set"), "first-3d-crate-set");
        assert_eq!(slugify("A -- B"), "a-b");
    }
}
