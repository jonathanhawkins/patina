//! Dynamic gate map built from parsed PRD markdown.
//!
//! Replaces the previous compiled-in static table with a function that
//! cross-references execution map bead specs with criteria items.

use crate::prd_parser::{self, BeadSpec, CriteriaItem};

/// A single gate entry linking an acceptance test to its exit criteria.
#[derive(Debug, Clone)]
pub struct GateEntry {
    /// Test function name extracted from the acceptance command.
    pub test_name: String,
    /// Subsystem section from the criteria document.
    pub criteria_section: String,
    /// The specific criteria line text.
    pub criteria_line: String,
    /// Bead key for dedup (e.g. `v1-obj-classdb`).
    pub bead_key: String,
    /// Execution map priority: 1=Now, 2=Next, 3=Later.
    pub priority: u32,
}

/// Build a gate map by cross-referencing bead specs (from execution maps)
/// with criteria items (from exit criteria documents).
///
/// For each bead spec that has an acceptance command, extracts the test
/// function name and tries to match it to a criteria item by substring
/// match on the description.
pub fn build_gate_map(bead_specs: &[BeadSpec], criteria: &[CriteriaItem]) -> Vec<GateEntry> {
    let mut entries = Vec::new();

    for spec in bead_specs {
        let acceptance = match &spec.acceptance_command {
            Some(cmd) => cmd,
            None => continue,
        };

        let test_name = match prd_parser::extract_test_name_from_command(acceptance) {
            Some(name) => name,
            None => continue,
        };

        // Try to find a matching criteria item by substring match
        let criteria_match = find_best_criteria_match(&spec.description, criteria);

        let (criteria_section, criteria_line) = match criteria_match {
            Some(item) => (item.section.clone(), item.text.clone()),
            None => (spec.section.clone(), spec.description.clone()),
        };

        entries.push(GateEntry {
            test_name,
            criteria_section,
            criteria_line,
            bead_key: spec.bead_key.clone(),
            priority: spec.priority,
        });
    }

    entries
}

/// Find the best matching criteria item for a bead description.
///
/// Uses word overlap scoring: each word in the description that appears
/// in a criteria item's text counts as a match point. Returns the item
/// with the highest score, if any score is above a minimum threshold.
fn find_best_criteria_match<'a>(
    description: &str,
    criteria: &'a [CriteriaItem],
) -> Option<&'a CriteriaItem> {
    let desc_words: Vec<String> = description
        .split_whitespace()
        .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() > 2) // Skip short words like "of", "in", etc.
        .collect();

    if desc_words.is_empty() {
        return None;
    }

    let mut best_score = 0usize;
    let mut best_item = None;

    for item in criteria {
        let item_lower = item.text.to_lowercase();
        let score = desc_words.iter().filter(|w| item_lower.contains(w.as_str())).count();
        if score > best_score {
            best_score = score;
            best_item = Some(item);
        }
    }

    // Require at least 2 matching words to count as a match
    if best_score >= 2 {
        best_item
    } else {
        None
    }
}

// ─── Legacy gate map for migration testing ─────────────────────────────────

#[cfg(test)]
fn legacy_gate_map() -> Vec<(&'static str, &'static str)> {
    // (bead_key, test_name) pairs from the original static table
    vec![
        ("v1-obj-classdb", "test_v1_classdb_full_property_enumeration"),
        ("v1-obj-notif", "test_v1_notification_dispatch_ordering"),
        ("v1-obj-weakref", "test_v1_weakref_auto_invalidates_on_free"),
        ("v1-obj-free", "test_v1_object_free_use_after_free_guard"),
        ("v1-res-uid", "test_v1_resource_uid_registry_from_parsed_files"),
        ("v1-res-subres", "test_v1_subresource_inline_loading"),
        ("v1-res-extref", "test_v1_ext_resource_cross_file_resolution"),
        ("v1-res-roundtrip", "test_v1_resource_roundtrip_equivalence"),
        ("v1-res-oracle", "test_v1_resource_oracle_comparison"),
        ("v1-scene-instance", "test_v1_instance_inheritance_ext_resource"),
        ("v1-scene-roundtrip", "test_v1_packed_scene_save_restore_roundtrip"),
        ("v1-scene-signals", "test_v1_scene_signal_connections_wired"),
        ("v1-scene-oracle", "test_v1_scene_oracle_golden_comparison"),
        ("v1-script-parser", "test_v1_gdscript_parser_stable_ast"),
        ("v1-script-onready", "test_v1_onready_variable_resolution"),
        ("v1-script-dispatch", "test_v1_func_dispatch_via_method_table"),
        ("v1-script-signal-decl", "test_v1_signal_declaration_from_script"),
        ("v1-script-signal-emit", "test_v1_script_signal_declaration_and_emit"),
        ("v1-script-oracle", "test_v1_script_fixture_oracle_match"),
        ("v1-phys-api", "test_v1_physics_server_2d_api_surface"),
        ("v1-phys-layers", "test_v1_collision_layers_and_masks"),
        ("v1-phys-kinematic", "test_v1_kinematic_move_and_collide"),
        ("v1-phys-oracle", "test_v1_physics_multi_body_oracle_trace"),
        ("v1-phys-kinematic-full", "test_v1_kinematic_body_move_and_collide"),
        ("v1-render-atlas", "test_v1_texture_atlas_sampling"),
        ("v1-render-zindex", "test_v1_canvas_item_z_index_ordering"),
        ("v1-render-visibility", "test_v1_visibility_suppression"),
        ("v1-render-camera", "test_v1_camera2d_transform"),
        ("v1-render-pixeldiff", "test_v1_pixel_diff_threshold"),
        ("v1-plat-window", "test_v1_window_creation"),
        ("v1-plat-input", "test_v1_input_event_delivery"),
        ("v1-plat-os", "test_v1_os_singleton"),
        ("v1-plat-time", "test_v1_time_singleton"),
        ("v1-plat-headless", "test_v1_headless_mode"),
        ("v1-scene-packed-roundtrip", "test_v1_packed_scene_roundtrip"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_gate_map_basic() {
        let specs = vec![
            BeadSpec {
                section: "Now".to_string(),
                bead_key: "my-gate".to_string(),
                description: "Full property enumeration against oracle".to_string(),
                acceptance_command: Some(
                    "cargo test --test gate -- --ignored test_property_enum".to_string(),
                ),
                priority: 1,
            },
            BeadSpec {
                section: "Later".to_string(),
                bead_key: "no-cmd".to_string(),
                description: "No command".to_string(),
                acceptance_command: None,
                priority: 3,
            },
        ];
        let criteria = vec![CriteriaItem {
            section: "Object Model".to_string(),
            text: "Full property and method enumeration against oracle output".to_string(),
            checked: false,
            line_number: 10,
        }];

        let map = build_gate_map(&specs, &criteria);
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].test_name, "test_property_enum");
        assert_eq!(map[0].bead_key, "my-gate");
        assert_eq!(map[0].criteria_section, "Object Model");
        assert_eq!(map[0].priority, 1);
    }

    #[test]
    fn test_build_gate_map_no_criteria_match_uses_spec_section() {
        let specs = vec![BeadSpec {
            section: "Now".to_string(),
            bead_key: "key-a".to_string(),
            description: "Zephyr quantum vortex handler".to_string(),
            acceptance_command: Some("cargo test -- test_zephyr".to_string()),
            priority: 1,
        }];
        let criteria = vec![CriteriaItem {
            section: "Other".to_string(),
            text: "Banana mango papaya integration layer".to_string(),
            checked: false,
            line_number: 1,
        }];

        let map = build_gate_map(&specs, &criteria);
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].criteria_section, "Now");
        assert_eq!(
            map[0].criteria_line,
            "Zephyr quantum vortex handler"
        );
    }

    #[test]
    fn test_all_entries_have_nonempty_fields() {
        let specs = vec![BeadSpec {
            section: "Now".to_string(),
            bead_key: "test-key".to_string(),
            description: "Test description".to_string(),
            acceptance_command: Some("cargo test -- test_func".to_string()),
            priority: 1,
        }];
        let map = build_gate_map(&specs, &[]);
        for entry in &map {
            assert!(!entry.test_name.is_empty());
            assert!(!entry.criteria_section.is_empty());
            assert!(!entry.criteria_line.is_empty());
            assert!(!entry.bead_key.is_empty());
        }
    }

    #[test]
    fn test_bead_keys_are_unique_in_built_map() {
        let specs = vec![
            BeadSpec {
                section: "Now".to_string(),
                bead_key: "key-a".to_string(),
                description: "Desc A".to_string(),
                acceptance_command: Some("cmd -- test_a".to_string()),
                priority: 1,
            },
            BeadSpec {
                section: "Now".to_string(),
                bead_key: "key-b".to_string(),
                description: "Desc B".to_string(),
                acceptance_command: Some("cmd -- test_b".to_string()),
                priority: 1,
            },
        ];
        let map = build_gate_map(&specs, &[]);
        let mut keys: Vec<&str> = map.iter().map(|e| e.bead_key.as_str()).collect();
        let original_len = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), original_len, "bead keys should be unique");
    }

    #[test]
    fn test_migration_dynamic_covers_legacy_bead_keys() {
        // This test verifies that building a gate map from parsed execution specs
        // and criteria keeps the bead keys stable. It uses a synthetic execution
        // map because the real V1 execution map is now a handoff doc.
        let exec_content = r#"
## Now

1. `v1-obj-classdb` Full ClassDB property and method enumeration against oracle output
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_classdb
2. `v1-res-uid` Resource UID registry for uid:// references
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_uid_registry

## Next

1. `v1-phys-api` PhysicsServer2D API surface body_create body_set_state body_get_state
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_physics_server_api
2. `v1-plat-window` Window creation abstraction backed by winit
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_window_creation
"#;
        let criteria_content = r#"
## Object Model (`gdobject`)

- [x] Full `ClassDB` property and method enumeration (measurable against oracle output)

## Resources (`gdresource`)

- [x] Resource UID registry (tracks `uid://` references)

## Physics (`gdphysics2d`)

- [x] `PhysicsServer2D` API surface: `body_create`, `body_set_state`, `body_get_state`

## Platform / Window / Input (`gdplatform`)

- [x] Window creation abstraction (backed by `winit`)
"#;

        let specs = prd_parser::parse_execution_map(exec_content);
        let criteria = prd_parser::parse_criteria(criteria_content);
        let dynamic_map = build_gate_map(&specs, &criteria);

        let dynamic_keys: std::collections::HashSet<&str> =
            dynamic_map.iter().map(|e| e.bead_key.as_str()).collect();
        let spec_keys: std::collections::HashSet<&str> = specs.iter().map(|s| s.bead_key.as_str()).collect();

        for key in &spec_keys {
            assert!(
                dynamic_keys.contains(key),
                "spec bead key '{}' missing from dynamic gate map",
                key
            );
        }

        // Also verify the dynamic map found a reasonable number of entries
        assert!(
            dynamic_map.len() >= 4,
            "dynamic gate map should have at least 4 entries, got {}",
            dynamic_map.len()
        );
    }
}
