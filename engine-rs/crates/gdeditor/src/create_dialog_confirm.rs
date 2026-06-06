//! Confirm/instantiate behavior for the Create Node dialog.
//!
//! These helpers sit on top of [`crate::create_dialog::CreateNodeDialog`] and
//! implement the two ways a user commits a choice:
//!
//! - **Enter** confirms the best-matching type — the top-ranked entry in the
//!   dialog's filtered, ranked search results — without first clicking a row.
//! - **Double-clicking a type** or **activating the Create button** instantiates
//!   the selected type and closes the dialog.
//!
//! They are kept in their own module (rather than methods on the dialog) so this
//! lifecycle layer can evolve without churning the large dialog file.

use crate::create_dialog::{CreateDialogResult, CreateNodeDialog};

/// The top-ranked search match: the first class in the dialog's filtered, ranked
/// result list (favorites first, then alphabetical), or `None` if nothing
/// matches the current search.
pub fn best_match(dialog: &CreateNodeDialog) -> Option<String> {
    dialog
        .filtered_classes()
        .into_iter()
        .next()
        .map(|entry| entry.class_name)
}

/// Handles the **Enter** key: if no row is selected yet, selects the top-ranked
/// search match, then instantiates the selection and closes the dialog. Returns
/// the instantiated type, or `None` when there is no match to confirm (in which
/// case the dialog stays open).
pub fn confirm_best_match(dialog: &mut CreateNodeDialog) -> Option<CreateDialogResult> {
    if dialog.selected().is_none() {
        let best = best_match(dialog)?;
        dialog.select(&best);
    }
    dialog.confirm()
}

/// Double-clicking a type selects it and immediately instantiates it, closing
/// the dialog. Returns `None` (leaving the dialog open) if the type is unknown.
pub fn double_click(dialog: &mut CreateNodeDialog, class_name: &str) -> Option<CreateDialogResult> {
    if !dialog.select(class_name) {
        return None;
    }
    dialog.confirm()
}

/// Activating the **Create** button instantiates the current selection and
/// closes the dialog. Returns `None` (leaving the dialog open) if nothing is
/// selected.
pub fn activate_create(dialog: &mut CreateNodeDialog) -> Option<CreateDialogResult> {
    dialog.confirm()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create_node_catalog::ensure_registered;

    /// Acceptance (pat-pahv4): pressing Enter selects the top-ranked search
    /// match, and double-clicking a type or activating Create instantiates the
    /// selected type and closes the dialog.
    #[test]
    fn create_node_best_match_confirm() {
        ensure_registered();

        // --- Enter confirms the best-matching type ---
        let mut dialog = CreateNodeDialog::new();
        dialog.open();
        dialog.set_search("Body2D");

        // The best match is the top-ranked entry of the ranked results; the user
        // hasn't clicked a row yet.
        let expected = best_match(&dialog).expect("the search yields at least one match");
        assert!(dialog.selected().is_none(), "no row selected before Enter");
        assert!(dialog.is_visible());

        let result = confirm_best_match(&mut dialog).expect("Enter confirms the best match");
        assert_eq!(
            result.class_name, expected,
            "Enter selects the top-ranked search match"
        );
        assert!(!dialog.is_visible(), "confirming closes the dialog");

        // --- Double-clicking a type instantiates it ---
        let mut dialog = CreateNodeDialog::new();
        dialog.open();
        dialog.set_search("Camera");
        let target = best_match(&dialog).expect("Camera search has a match");

        let result = double_click(&mut dialog, &target).expect("double-click instantiates");
        assert_eq!(result.class_name, target, "double-click instantiates that type");
        assert!(!dialog.is_visible(), "double-click closes the dialog");

        // --- The Create button instantiates the current selection ---
        let mut dialog = CreateNodeDialog::new();
        dialog.open();
        dialog.set_search("Area");
        let selection = best_match(&dialog).expect("Area search has a match");
        assert!(dialog.select(&selection), "the selected type exists");

        let result = activate_create(&mut dialog).expect("Create instantiates the selection");
        assert_eq!(result.class_name, selection);
        assert!(!dialog.is_visible(), "Create closes the dialog");
    }

    /// Enter with no matching search leaves the dialog open and instantiates
    /// nothing.
    #[test]
    fn enter_with_no_match_does_nothing() {
        ensure_registered();

        let mut dialog = CreateNodeDialog::new();
        dialog.open();
        dialog.set_search("ZzNoSuchTypeZz");

        assert!(best_match(&dialog).is_none(), "no match for a gibberish query");
        assert!(confirm_best_match(&mut dialog).is_none(), "nothing to confirm");
        assert!(dialog.is_visible(), "the dialog stays open with no match");
    }
}
