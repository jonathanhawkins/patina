//! pat-k8hr0: parity-criteria methodology lint.
//!
//! A parity criterion in `prd/EDITOR_PARITY_EXIT.md` may not be ticked `[x]`
//! while the main-screen view its acceptance depends on is still a placeholder
//! (`MainView::is_implemented() == false`). Booting *distinct placeholders* is
//! not the same as implementing the Game preview / AssetLib browser, so a green
//! checkbox on a criterion that claims those views render real, distinct content
//! is a false "parity complete" signal (the bug this lint guards against:
//! `mode_placeholder.rs` reports `GamePreview`/`AssetLibBrowser` unimplemented
//! while their criteria were ticked).
//!
//! This is a pure-data lint — it does not boot the server. It parses the
//! criteria file and cross-checks the *real* `is_implemented()` exported by
//! `gdeditor`, so the check stays honest automatically as views get
//! implemented: ticking `main-screen-e2e-modes` becomes allowed the moment both
//! mapped views report implemented.

use gdeditor::main_screen::MainView;

/// Criteria whose green status requires a specific main-screen view to render a
/// real, distinct surface (not a placeholder). Keyed by criterion id. A criterion
/// listed here may only be `[x]` once every mapped view reports
/// `is_implemented() == true`.
///
/// Keep this list narrow: only criteria whose acceptance literally asserts the
/// view produces a distinct, working central surface belong here. Criteria that
/// merely toggle mode state are not view-dependent.
const VIEW_DEPENDENT_CRITERIA: &[(&str, &[MainView])] = &[
    // "Booting the real server ... makes the SERVED central view actually
    // correspond to that mode — ... game preview / asset library, each distinct"
    // — green here requires both views to be real, not placeholders.
    (
        "main-screen-e2e-modes",
        &[MainView::GamePreview, MainView::AssetLibBrowser],
    ),
    // The app-level mode-switch criterion makes the same over-HTTP claim.
    (
        "mode-switch-renders-view-app",
        &[MainView::GamePreview, MainView::AssetLibBrowser],
    ),
];

/// A parsed criterion row from the parity exit file.
struct Criterion {
    line_no: usize,
    checked: bool,
    id: String,
}

/// Resolves `<repo>/prd/EDITOR_PARITY_EXIT.md` from this crate's manifest dir
/// (`<repo>/engine-rs/crates/gdeditor`).
fn criteria_file() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../prd/EDITOR_PARITY_EXIT.md")
}

/// Parses criterion rows of the form `- [x] \`criterion-id\` …` (or `- [ ] …`).
/// The criterion id is the first backtick-delimited token after the checkbox.
fn parse_criteria(text: &str) -> Vec<Criterion> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_start();
        let parsed = if let Some(rest) = line.strip_prefix("- [x] ") {
            Some((true, rest))
        } else if let Some(rest) = line.strip_prefix("- [ ] ") {
            Some((false, rest))
        } else {
            None
        };
        let Some((checked, rest)) = parsed else {
            continue;
        };
        // First backtick-delimited token is the criterion id.
        let Some(id) = rest.strip_prefix('`').and_then(|r| r.split('`').next()) else {
            continue;
        };
        out.push(Criterion {
            line_no: i + 1,
            checked,
            id: id.to_string(),
        });
    }
    out
}

/// The core lint: no ticked criterion may depend on an unimplemented view.
#[test]
fn parity_criteria_lint_no_ticked_unimplemented_view() {
    let path = criteria_file();
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let criteria = parse_criteria(&text);
    assert!(
        !criteria.is_empty(),
        "parsed zero criteria from {} — parser or file format changed",
        path.display()
    );

    let mut violations = Vec::new();
    for crit in &criteria {
        if !crit.checked {
            continue;
        }
        let Some((_, views)) =
            VIEW_DEPENDENT_CRITERIA.iter().find(|(id, _)| *id == crit.id.as_str())
        else {
            continue;
        };
        for view in *views {
            if !view.is_implemented() {
                violations.push(format!(
                    "line {}: criterion `{}` is ticked [x] but its view {:?} is not implemented \
                     (MainView::{:?}.is_implemented() == false)",
                    crit.line_no, crit.id, view, view
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "parity-criteria lint failed — un-tick these criteria until the underlying view is \
         implemented (a distinct placeholder is not an implemented view):\n{}",
        violations.join("\n")
    );
}

/// Guards the lint itself: every id in `VIEW_DEPENDENT_CRITERIA` must still be a
/// real criterion in the file. A renamed/removed criterion would otherwise
/// silently disable the check above.
#[test]
fn parity_criteria_lint_mapping_ids_exist() {
    let path = criteria_file();
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let ids: Vec<String> = parse_criteria(&text).into_iter().map(|c| c.id).collect();

    let mut missing = Vec::new();
    for (mapped_id, _) in VIEW_DEPENDENT_CRITERIA {
        if !ids.iter().any(|id| id.as_str() == *mapped_id) {
            missing.push(*mapped_id);
        }
    }

    assert!(
        missing.is_empty(),
        "VIEW_DEPENDENT_CRITERIA references criterion ids not present in {}: {:?} — update the \
         mapping when criteria are renamed",
        path.display(),
        missing
    );
}
