//! Batch node renaming (Godot's editor Rename dialog).
//!
//! [`batch_rename`] applies a pattern with a sequential counter across a
//! selection, mirroring Godot's `RenameDialog`: `%d` expands to a per-item
//! counter (configurable start/step/zero-padding) and `%s` to the item's
//! original name, wrapped by an optional prefix/suffix. Renaming a three-node
//! selection with `node_%d` yields `node_1` / `node_2` / `node_3`.
//!
//! This is the pure substitution core; the editor dialog UI and the
//! `/api/node/batch_rename` endpoint that applies it to the current
//! multi-selection are wired separately.

/// Options controlling [`batch_rename`]'s counter and affixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameOptions {
    /// Value the `%d` counter starts at for the first item.
    pub counter_start: i64,
    /// Amount the `%d` counter advances per item.
    pub counter_step: i64,
    /// Minimum digit count for `%d`, left-padded with zeros.
    pub counter_padding: usize,
    /// String prepended to every result.
    pub prefix: String,
    /// String appended to every result.
    pub suffix: String,
}

impl Default for RenameOptions {
    fn default() -> Self {
        Self {
            counter_start: 1,
            counter_step: 1,
            counter_padding: 1,
            prefix: String::new(),
            suffix: String::new(),
        }
    }
}

impl RenameOptions {
    /// Options with the given counter start (step 1, no padding/affixes).
    pub fn starting_at(counter_start: i64) -> Self {
        Self {
            counter_start,
            ..Self::default()
        }
    }
}

/// Renames each name in `names` by substituting `pattern`: `%d` becomes the
/// per-item counter (`counter_start + i * counter_step`, zero-padded to
/// `counter_padding` digits) and `%s` the original name, then wraps the result
/// with `prefix`/`suffix`. Returns one output name per input, in order.
pub fn batch_rename(names: &[String], pattern: &str, options: &RenameOptions) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let counter = options.counter_start + (i as i64) * options.counter_step;
            let counter_str = format!("{:0width$}", counter, width = options.counter_padding);
            // Substitute the counter first so a `%d` inside an original name
            // (inserted by `%s`) is left untouched.
            let body = pattern.replace("%d", &counter_str).replace("%s", name);
            format!("{}{}{}", options.prefix, body, options.suffix)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn batch_rename_counter_pattern() {
        let names = strs(&["A", "B", "C"]);
        let out = batch_rename(&names, "node_%d", &RenameOptions::default());
        assert_eq!(out, strs(&["node_1", "node_2", "node_3"]));
    }

    #[test]
    fn batch_rename_original_name_step_and_padding() {
        let names = strs(&["foo", "bar"]);
        let opts = RenameOptions {
            counter_start: 0,
            counter_step: 2,
            counter_padding: 3,
            ..Default::default()
        };
        let out = batch_rename(&names, "%s_%d", &opts);
        assert_eq!(out, strs(&["foo_000", "bar_002"]));
    }

    #[test]
    fn batch_rename_prefix_suffix() {
        let names = strs(&["x"]);
        let opts = RenameOptions {
            prefix: "pre_".to_string(),
            suffix: "_end".to_string(),
            ..Default::default()
        };
        let out = batch_rename(&names, "%s", &opts);
        assert_eq!(out, strs(&["pre_x_end"]));
    }

    #[test]
    fn batch_rename_name_with_percent_d_is_preserved() {
        // A `%d` inside the original name must survive (only the pattern's
        // counter token is substituted).
        let names = strs(&["a%db"]);
        let out = batch_rename(&names, "%s", &RenameOptions::default());
        assert_eq!(out, strs(&["a%db"]));
    }

    #[test]
    fn batch_rename_starting_at_helper() {
        let names = strs(&["n", "n", "n"]);
        let out = batch_rename(&names, "item_%d", &RenameOptions::starting_at(10));
        assert_eq!(out, strs(&["item_10", "item_11", "item_12"]));
    }
}
