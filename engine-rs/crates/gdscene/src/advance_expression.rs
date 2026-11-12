//! Evaluating a state-machine transition's `advance_expression` (pat-mgixk.82).
//!
//! Godot's `AnimationNodeStateMachineTransition.advance_expression` is a
//! GDScript expression that must ALSO be true for an `ADVANCE_MODE_AUTO` edge to
//! fire (`AnimationNodeStateMachinePlayback::_check_advance_condition`,
//! `scene/animation/animation_node_state_machine.cpp:1119`). Upstream builds an
//! `Expression`, parses the string once, and executes it with the node named by
//! `AnimationTree.advance_expression_base_node` as the **base instance** — which
//! is what makes a bare `speed` in the expression mean "that node's `speed`".
//!
//! # Why this module exists rather than a new entry point in the interpreter
//!
//! `gdscript-interop` already implements the whole of Godot's `Expression` class
//! (pat-0q2kx.207) — but its parts are private to that module:
//! `compile_expression` and `Interpreter::eval_expr` are both private, and the
//! `Expression` surface is reachable only from *inside* a running script, which
//! is the wrong direction: here the engine is the caller.
//!
//! It needs no new seam, though. The crate's existing **public** API is enough:
//! [`Interpreter::run_class`] compiles a synthesized one-method class,
//! `instantiate_class` builds an instance whose `properties` field is public, and
//! [`Interpreter::call_instance_method`] runs it. Seeding those properties with a
//! snapshot of the base node reproduces Godot's `base_instance` semantics
//! exactly, because a bareword read in a class method resolves against
//! `self_instance.properties` before anything else
//! (`gdscript-interop/src/interpreter.rs:3990`).
//!
//! That matters practically as well as aesthetically: evaluating expressions here
//! adds nothing to `gdscript-interop`, so this bead cannot break any script.

use std::collections::HashMap;

use gdvariant::Variant;

/// Name of the synthesized method the expression is compiled into.
///
/// Deliberately `__`-prefixed and spelled out: it shares a scope with the base
/// node's own property names, and a collision would silently shadow a real
/// member rather than fail.
const EVAL_FN: &str = "__patina_advance_expression";

/// Evaluates `code` as a Godot `advance_expression` against `scope`, a snapshot
/// of the `advance_expression_base_node`'s properties.
///
/// Returns:
/// * `Some(true)` — the expression evaluated to a true value, so the edge opens;
/// * `Some(false)` — it evaluated to a false value, so the edge stays shut;
/// * `None` — it could not be evaluated at all (did not tokenize, did not parse,
///   referenced a name the base node does not carry, divided by zero, …).
///
/// ⚠️ `None` and `Some(false)` are deliberately distinct even though the caller
/// treats both as "do not advance". Keeping them apart is what lets the call site
/// spell its gate positively (`== Some(true)`), so a *failure* to evaluate can
/// never be mistaken for a satisfied condition. Godot reaches the same place by a
/// different road: `Expression::execute` returns null and sets
/// `has_execute_failed()`, and `_check_advance_condition` ignores the value when
/// that flag is set.
///
/// Every failure is swallowed rather than propagated, which is the entire point
/// of `Expression` upstream: it evaluates project-authored text, so a bad
/// expression must not take the frame down with it.
pub fn eval_advance_expression(code: &str, scope: &HashMap<String, Variant>) -> Option<bool> {
    if code.trim().is_empty() {
        return None;
    }

    // One method whose body is the expression. `run_class` is the public entry
    // point and it only parses whole scripts, so the expression has to be
    // wrapped in something — a `func` rather than a bare `var`, because only a
    // method body is evaluated with a `self_instance` in scope, and that is what
    // resolves the base node's members.
    //
    // A tab indents the body — the tokenizer counts one as four spaces
    // (`gdscript-interop/src/tokenizer.rs:363`), and unlike leading spaces in a
    // format string it cannot be silently reflowed.
    let source = format!("extends Node\nfunc {EVAL_FN}():\n\treturn ({code})\n");

    let mut interp = gdscript_interop::interpreter::Interpreter::new();
    let class_def = interp.run_class(&source).ok()?;
    let mut instance = interp.instantiate_class(&class_def).ok()?;
    // The base node's state, as the expression's members. `instantiate_class`
    // has already run the class's own `var` initialisers (there are none), so
    // this cannot clobber anything the synthesized source declared.
    for (name, value) in scope {
        instance.properties.insert(name.clone(), value.clone());
    }

    let returned = interp
        .call_instance_method(&mut instance, EVAL_FN, &[])
        .ok()?;
    variant_truth(&returned)
}

/// Godot's `Variant::operator bool()` for the cases an advance expression can
/// reasonably produce.
///
/// ⚠️ Anything else — `Nil`, a string, an object, an array — yields `None`
/// rather than a guess. Upstream would coerce some of those, but an engine that
/// guessed here would fire a transition a project deliberately gated, and
/// "cannot decide" already has a safe meaning at the call site. Widen this only
/// with a test that pins the coercion.
fn variant_truth(value: &Variant) -> Option<bool> {
    match value {
        Variant::Bool(b) => Some(*b),
        Variant::Int(i) => Some(*i != 0),
        Variant::Float(f) => Some(*f != 0.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(pairs: &[(&str, Variant)]) -> HashMap<String, Variant> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn a_member_comparison_reads_the_base_nodes_property() {
        let s = scope(&[("speed", Variant::Float(2.0))]);
        assert_eq!(eval_advance_expression("speed > 1.0", &s), Some(true));

        let s = scope(&[("speed", Variant::Float(0.0))]);
        assert_eq!(eval_advance_expression("speed > 1.0", &s), Some(false));
    }

    /// The operand comes from the scope, so the SAME expression text must give
    /// opposite answers for different base-node state. A test that only ever
    /// passes one scope cannot tell evaluation from a constant.
    #[test]
    fn the_same_expression_tracks_the_scope() {
        for (speed, expected) in [(0.5_f64, false), (1.5, true)] {
            let s = scope(&[("speed", Variant::Float(speed))]);
            assert_eq!(
                eval_advance_expression("speed > 1.0", &s),
                Some(expected),
                "speed = {speed}",
            );
        }
    }

    #[test]
    fn compound_expressions_and_int_members_work() {
        let s = scope(&[
            ("speed", Variant::Float(3.0)),
            ("on_floor", Variant::Bool(true)),
        ]);
        assert_eq!(
            eval_advance_expression("on_floor and speed > 1.0", &s),
            Some(true)
        );
        assert_eq!(
            eval_advance_expression("not on_floor and speed > 1.0", &s),
            Some(false)
        );

        let s = scope(&[("hits", Variant::Int(3))]);
        assert_eq!(eval_advance_expression("hits >= 3", &s), Some(true));
    }

    /// An int/float result is Godot's truthiness, not an error.
    #[test]
    fn numeric_results_coerce() {
        let s = scope(&[("hits", Variant::Int(2))]);
        assert_eq!(eval_advance_expression("hits", &s), Some(true));
        let s = scope(&[("hits", Variant::Int(0))]);
        assert_eq!(eval_advance_expression("hits", &s), Some(false));
    }

    /// ⚠️ The distinction the call site's gate depends on: an expression that
    /// cannot be evaluated is `None`, never `Some(true)`.
    #[test]
    fn unevaluatable_expressions_are_none_not_true() {
        // References a member the base node does not carry.
        let s = scope(&[("speed", Variant::Float(2.0))]);
        assert_eq!(eval_advance_expression("velocity.x > 1.0", &s), None);
        // Not an expression at all.
        assert_eq!(eval_advance_expression("speed >", &s), None);
        // Empty, which is also how "no expression authored" reaches here if a
        // caller ever stops checking.
        assert_eq!(eval_advance_expression("", &s), None);
        assert_eq!(eval_advance_expression("   ", &s), None);
        // A result of a type this refuses to guess at.
        assert_eq!(eval_advance_expression("\"yes\"", &s), None);
    }
}
