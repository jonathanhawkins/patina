//! **Go-to-method** navigation for signal connections (pat-yukkm).
//!
//! Activating a signal connection opens the receiver's script at the connected
//! method's definition. This module resolves a connection to a navigation
//! target — the target script and the 1-based line of the method's `func`
//! definition — which the editor uses to open and scroll the script.

/// If `line` begins with `func` followed by whitespace, returns the function
/// name that follows.
fn func_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("func")?;
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// The 1-based line where `method` is defined in `source`, if present.
pub fn method_line(source: &str, method: &str) -> Option<usize> {
    source
        .lines()
        .enumerate()
        .find(|(_, line)| func_name(line) == Some(method))
        .map(|(i, _)| i + 1)
}

/// A signal connection from an emitter to a receiver method in a target script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The signal name on the emitter.
    pub signal: String,
    /// The receiver's script path.
    pub target_script: String,
    /// The receiver method the signal is connected to.
    pub method: String,
}

/// Where the editor should navigate: a script and a line within it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavTarget {
    /// The script to open.
    pub script: String,
    /// The 1-based line of the method definition.
    pub line: usize,
}

/// Resolves the navigation target for `conn`, given the target script's
/// `source`. Returns `None` if the connected method isn't defined in the script.
pub fn navigate(conn: &Connection, source: &str) -> Option<NavTarget> {
    let line = method_line(source, &conn.method)?;
    Some(NavTarget {
        script: conn.target_script.clone(),
        line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str =
        "extends Button\n\nfunc _ready():\n\tpass\n\nfunc _on_button_pressed():\n\tprint(\"hi\")\n";

    /// Acceptance (pat-yukkm): activating a connection navigates the script
    /// editor to the receiver method definition.
    #[test]
    fn signals_navigate_to_connected_method() {
        let conn = Connection {
            signal: "pressed".to_string(),
            target_script: "res://ui/menu.gd".to_string(),
            method: "_on_button_pressed".to_string(),
        };

        // Activating the connection navigates to the method's script and line.
        let target = navigate(&conn, SCRIPT).expect("method is defined");
        assert_eq!(target.script, "res://ui/menu.gd");
        assert_eq!(target.line, 6); // `func _on_button_pressed` is line 6

        // A connection to a method that isn't defined yields no navigation.
        let missing = Connection {
            method: "_on_missing".to_string(),
            ..conn.clone()
        };
        assert!(navigate(&missing, SCRIPT).is_none());

        // The method-line lookup is correct for other methods too.
        assert_eq!(method_line(SCRIPT, "_ready"), Some(3));
        assert_eq!(method_line(SCRIPT, "_on_button_pressed"), Some(6));
        assert_eq!(method_line(SCRIPT, "nope"), None);
    }

    /// Indented (inner-class) methods resolve to their own line; calls don't
    /// count as definitions.
    #[test]
    fn indented_methods_and_calls() {
        let src = "class Inner:\n\tfunc tick():\n\t\tpass\n\nfunc run():\n\ttick()\n";
        assert_eq!(method_line(src, "tick"), Some(2));
        assert_eq!(method_line(src, "run"), Some(5));
        // The `tick()` call on the last line is not a definition.
        assert_ne!(method_line(src, "tick"), Some(6));
    }
}
