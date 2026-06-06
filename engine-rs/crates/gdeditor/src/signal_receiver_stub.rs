//! **Auto-create receiver method stub** when connecting a signal (pat-wbmaq).
//!
//! When the user confirms a signal connection whose target method does not yet
//! exist in the receiver's script, the editor appends a correctly-signatured
//! method stub so the connection has something to call. The stub's parameter
//! list mirrors the signal's parameters and its body is a single `pass`.
//!
//! Existing methods are left untouched — connecting to a method that already
//! exists makes no change to the script.

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

/// Whether `source` already defines a `func` named `method`.
pub fn method_exists(source: &str, method: &str) -> bool {
    source.lines().any(|line| func_name(line) == Some(method))
}

/// Builds a method stub: `func <method>(<params>):` with a `pass` body. Params
/// are the signal's parameter names. Indentation uses a tab, matching Godot.
pub fn stub_for(method: &str, params: &[&str]) -> String {
    format!("func {}({}):\n\tpass\n", method, params.join(", "))
}

/// Appends a receiver stub for `method` (taking `params`) to `source`, unless
/// the method already exists. Returns the new script text, or `None` if no stub
/// was needed because the method is already defined.
///
/// The stub is separated from the existing content by a blank line.
pub fn append_receiver(source: &str, method: &str, params: &[&str]) -> Option<String> {
    if method_exists(source, method) {
        return None;
    }
    let mut out = source.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&stub_for(method, params));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-wbmaq): confirming a connection to a non-existent method
    /// appends a correctly-signatured method stub to the target script.
    #[test]
    fn signals_autocreate_receiver_method_stub() {
        let src = "extends Node\n\nfunc _ready():\n\tpass\n";

        // The receiver method doesn't exist yet.
        assert!(!method_exists(src, "_on_button_pressed"));

        // Confirming the connection appends a parameterless stub.
        let after = append_receiver(src, "_on_button_pressed", &[]).expect("stub appended");
        assert!(after.starts_with(src), "existing content is preserved");
        assert!(after.contains("func _on_button_pressed():\n\tpass"));
        // The new method now exists.
        assert!(method_exists(&after, "_on_button_pressed"));
        // It's separated from the prior content by a blank line.
        assert!(after.ends_with("\n\nfunc _on_button_pressed():\n\tpass\n"));

        // A signal with parameters produces a correctly-signatured stub.
        let after2 =
            append_receiver(&after, "_on_area_entered", &["area"]).expect("stub appended");
        assert!(after2.contains("func _on_area_entered(area):\n\tpass"));

        // Multiple parameters are comma-separated.
        let after3 = append_receiver(&after2, "_on_hit", &["damage", "source"]).unwrap();
        assert!(after3.contains("func _on_hit(damage, source):\n\tpass"));

        // Connecting to an already-existing method changes nothing.
        assert!(append_receiver(&after3, "_ready", &[]).is_none());
    }

    /// The generated stub has the exact expected shape.
    #[test]
    fn stub_shape() {
        assert_eq!(stub_for("_on_x", &[]), "func _on_x():\n\tpass\n");
        assert_eq!(stub_for("_on_x", &["a"]), "func _on_x(a):\n\tpass\n");
        assert_eq!(
            stub_for("_on_x", &["a", "b", "c"]),
            "func _on_x(a, b, c):\n\tpass\n"
        );
    }

    /// `func_name` matches definitions but not uses or look-alikes.
    #[test]
    fn func_name_detection() {
        assert!(method_exists("func go():\n\tpass\n", "go"));
        assert!(method_exists("\tfunc nested():\n\t\tpass\n", "nested"));
        // A call, not a definition.
        assert!(!method_exists("\tgo()\n", "go"));
        // `function` is not `func`.
        assert!(!method_exists("var function = 1\n", "function"));
    }
}
