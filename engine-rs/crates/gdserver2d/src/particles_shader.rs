//! pat-0q2kx.261 — an interpreted subset of `shader_type particles`.
//!
//! Godot ground truth: the particles stage built-ins in
//! `servers/rendering/shader_language.cpp` and the shading-language particles
//! reference. A particles shader replaces `ParticleProcessMaterial`: `start()`
//! initialises a particle's `TRANSFORM` / `VELOCITY` / `COLOR` / `CUSTOM`, and
//! `process()` integrates them once per simulation step.
//!
//! Patina's shader front-end recognised the type and produced nothing —
//! `shader.rs::compile` maps everything but `canvas_item` to `wgsl_program:
//! None` — and the CPU simulator had no hook at all, so a particles `.gdshader`
//! assigned as `process_material` was inert.
//!
//! # What this is, and what it deliberately is not
//!
//! This is an **interpreter over the CPU particle simulator**, not a shader
//! compiler. Godot runs these on the GPU; Patina's 2D particles are simulated
//! on the CPU, so the honest port is to evaluate the same statements per
//! particle per step. That is the approach the bead asks for and the one the
//! `canvas_item` CPU-eval path already established.
//!
//! The recognised subset is intentionally small and **everything outside it is
//! reported**, never silently skipped — see [`ParticlesProgram::unsupported`].
//! A particles shader that half-runs is worse than one that does not run: the
//! particles move, so the feature looks wired, and only the specific lines that
//! were dropped misbehave.

use gdcore::math::{Color, Vector2};

/// A per-particle value. The subset is scalars and 4-component vectors, which
/// is what `VELOCITY` (a vec3 in Godot, used as vec2 here), `COLOR` (vec4) and
/// float built-ins need.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// A single float.
    Scalar(f32),
    /// Up to four components; shorter vectors leave the tail at zero.
    Vec4(f32, f32, f32, f32),
}

impl Value {
    fn component(self, index: usize) -> f32 {
        match self {
            Value::Scalar(s) => s,
            Value::Vec4(x, y, z, w) => [x, y, z, w][index.min(3)],
        }
    }

    fn as_scalar(self) -> f32 {
        match self {
            Value::Scalar(s) => s,
            Value::Vec4(x, ..) => x,
        }
    }
}

/// One assignment statement in a recognised stage.
#[derive(Debug, Clone, PartialEq)]
struct Assignment {
    /// The built-in being written, e.g. `VELOCITY`.
    target: String,
    /// The component index if the target was swizzled (`VELOCITY.y` -> 1).
    component: Option<usize>,
    /// The compound operator, if any: `+=` / `-=` / `*=` / `/=`.
    op: Option<char>,
    /// Right-hand side.
    expr: Expr,
}

/// The recognised expression grammar: literals, built-ins, `vecN(..)`
/// constructors, and the four arithmetic operators over them.
#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Literal(f32),
    /// A built-in, optionally swizzled: `DELTA`, `VELOCITY.y`.
    Builtin(String, Option<usize>),
    Vec(Vec<Expr>),
    Binary(Box<Expr>, char, Box<Expr>),
}

/// The mutable per-particle state a stage reads and writes.
#[derive(Debug, Clone, Copy)]
pub struct ParticleShaderState {
    /// `VELOCITY` — Godot's is a vec3; the 2D simulator uses xy.
    pub velocity: Vector2,
    /// `COLOR`.
    pub color: Color,
    /// `TRANSFORM[3].xy`, i.e. the particle's position.
    pub position: Vector2,
    /// `DELTA` — the step's duration in seconds.
    pub delta: f32,
    /// `LIFETIME` — the particle's total lifetime.
    pub lifetime: f32,
    /// Godot's `CUSTOM.y`-style age accumulator, exposed as `AGE`.
    pub age: f32,
    /// `RANDOM_SEED`, stable for the particle's whole life.
    pub random_seed: f32,
}

/// A parsed particles shader: the statements of each recognised stage, plus
/// everything that was not recognised.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticlesProgram {
    start: Vec<Assignment>,
    process: Vec<Assignment>,
    /// Source constructs outside the interpreted subset, in source order.
    ///
    /// Non-empty does **not** mean the program is unusable — the recognised
    /// statements still run. It means those specific lines do nothing, which
    /// the caller should surface rather than let a partially-applied shader
    /// pass for a working one.
    pub unsupported: Vec<String>,
}

impl ParticlesProgram {
    /// Whether either stage has anything to run.
    pub fn is_empty(&self) -> bool {
        self.start.is_empty() && self.process.is_empty()
    }

    /// Whether `start()` has recognised statements.
    pub fn has_start(&self) -> bool {
        !self.start.is_empty()
    }

    /// Run `start()` over a freshly spawned particle.
    pub fn run_start(&self, state: &mut ParticleShaderState) {
        run(&self.start, state);
    }

    /// Run `process()` for one simulation step.
    pub fn run_process(&self, state: &mut ParticleShaderState) {
        run(&self.process, state);
    }
}

/// Parse a particles shader source into its interpretable statements.
///
/// Unrecognised statements are collected rather than causing a parse failure: a
/// real shader mixes supported and unsupported lines, and refusing the whole
/// program would make the feature all-or-nothing.
pub fn parse_particles_shader(source: &str) -> ParticlesProgram {
    let mut program = ParticlesProgram::default();
    for stage in ["start", "process"] {
        let Some(body) = extract_stage_body(source, stage) else {
            continue;
        };
        for raw in body.split(';') {
            let stmt = strip_comments(raw);
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            match parse_assignment(stmt) {
                Some(a) => {
                    if stage == "start" {
                        program.start.push(a);
                    } else {
                        program.process.push(a);
                    }
                }
                None => program
                    .unsupported
                    .push(format!("`{stage}()` statement not interpreted: `{stmt}`")),
            }
        }
    }
    program
}

/// The text between a stage's outermost braces, tracking nesting so a nested
/// block does not end the body early.
fn extract_stage_body(source: &str, stage: &str) -> Option<String> {
    let needle = format!("void {stage}()");
    let start = source.find(&needle).or_else(|| {
        // Tolerate `void process ( )` spacing without a full tokenizer.
        source
            .find(&format!("void {stage} ()"))
            .or_else(|| source.find(&format!("void {stage}( )")))
    })?;
    let open = source[start..].find('{')? + start;
    let bytes: Vec<char> = source[open..].chars().collect();
    let mut depth = 0i32;
    let mut end = None;
    for (i, c) in bytes.iter().enumerate() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    Some(bytes[1..end].iter().collect())
}

/// Remove `//` line comments; block comments are reported as unsupported by the
/// statement parser rather than being stripped, because a `/* */` spanning a
/// `;` would silently merge two statements.
fn strip_comments(s: &str) -> String {
    s.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_assignment(stmt: &str) -> Option<Assignment> {
    if stmt.contains("/*") || stmt.contains("*/") {
        return None;
    }
    // Longest operators first: `+=` must not be read as `=`.
    let (lhs, op, rhs) = ["+=", "-=", "*=", "/="]
        .iter()
        .find_map(|o| {
            stmt.split_once(*o)
                .map(|(l, r)| (l, Some(o.chars().next().unwrap()), r))
        })
        .or_else(|| {
            // A bare `=` that is not part of `==`, `<=`, `>=`, `!=`.
            let i = stmt.find('=')?;
            let prev = stmt[..i].chars().last();
            let next = stmt[i + 1..].chars().next();
            if matches!(prev, Some('<' | '>' | '!' | '=')) || next == Some('=') {
                return None;
            }
            Some((&stmt[..i], None, &stmt[i + 1..]))
        })?;

    let (target, component) = parse_target(lhs.trim())?;
    let expr = parse_expr(rhs.trim())?;
    Some(Assignment {
        target,
        component,
        op,
        expr,
    })
}

/// Split `VELOCITY.y` into its built-in and component index.
fn parse_target(lhs: &str) -> Option<(String, Option<usize>)> {
    let (name, swizzle) = match lhs.split_once('.') {
        Some((n, s)) => (n.trim(), Some(s.trim())),
        None => (lhs, None),
    };
    if !is_builtin_name(name) {
        return None;
    }
    let component = match swizzle {
        None => None,
        Some(s) => Some(component_index(s)?),
    };
    Some((name.to_string(), component))
}

/// `x/r/s` -> 0, `y/g/t` -> 1, `z/b/p` -> 2, `w/a/q` -> 3.
///
/// Only single-component swizzles are recognised. A multi-component one like
/// `COLOR.rgb` returns `None` so the statement is reported rather than being
/// applied to the wrong single channel — writing `rgb` as if it were `r` would
/// change the colour in a plausible-looking way.
fn component_index(s: &str) -> Option<usize> {
    match s {
        "x" | "r" | "s" => Some(0),
        "y" | "g" | "t" => Some(1),
        "z" | "b" | "p" => Some(2),
        "w" | "a" | "q" => Some(3),
        _ => None,
    }
}

fn is_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "VELOCITY" | "COLOR" | "TRANSFORM" | "DELTA" | "LIFETIME" | "AGE" | "RANDOM_SEED"
    )
}

// ---------------------------------------------------------------------------
// Expression parsing — precedence climbing over `+ -` then `* /`.
// ---------------------------------------------------------------------------

fn parse_expr(s: &str) -> Option<Expr> {
    let (e, rest) = parse_additive(s.trim())?;
    if !rest.trim().is_empty() {
        return None;
    }
    Some(e)
}

fn parse_additive(s: &str) -> Option<(Expr, &str)> {
    let (mut lhs, mut rest) = parse_multiplicative(s)?;
    loop {
        let t = rest.trim_start();
        let Some(op) = t.chars().next().filter(|c| *c == '+' || *c == '-') else {
            return Some((lhs, rest));
        };
        let (rhs, r) = parse_multiplicative(&t[1..])?;
        lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        rest = r;
    }
}

fn parse_multiplicative(s: &str) -> Option<(Expr, &str)> {
    let (mut lhs, mut rest) = parse_primary(s)?;
    loop {
        let t = rest.trim_start();
        let Some(op) = t.chars().next().filter(|c| *c == '*' || *c == '/') else {
            return Some((lhs, rest));
        };
        let (rhs, r) = parse_primary(&t[1..])?;
        lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        rest = r;
    }
}

fn parse_primary(s: &str) -> Option<(Expr, &str)> {
    let t = s.trim_start();

    if let Some(after) = t.strip_prefix('(') {
        let (e, rest) = parse_additive(after)?;
        let rest = rest.trim_start().strip_prefix(')')?;
        return Some((e, rest));
    }
    if let Some(after) = t.strip_prefix('-') {
        let (e, rest) = parse_primary(after)?;
        return Some((
            Expr::Binary(Box::new(Expr::Literal(0.0)), '-', Box::new(e)),
            rest,
        ));
    }

    // vec2/vec3/vec4 constructors.
    for ctor in ["vec4", "vec3", "vec2"] {
        if let Some(after) = t.strip_prefix(ctor) {
            let after = after.trim_start().strip_prefix('(')?;
            let mut args = Vec::new();
            let mut rest = after;
            loop {
                let (e, r) = parse_additive(rest)?;
                args.push(e);
                let r = r.trim_start();
                if let Some(r2) = r.strip_prefix(',') {
                    rest = r2;
                    continue;
                }
                rest = r.strip_prefix(')')?;
                break;
            }
            return Some((Expr::Vec(args), rest));
        }
    }

    // Identifier (built-in, optionally swizzled).
    let ident_len = t
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
        .count();
    if ident_len > 0 && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        let name = &t[..ident_len];
        let mut rest = &t[ident_len..];
        let mut component = None;
        if let Some(after_dot) = rest.strip_prefix('.') {
            let sw_len = after_dot
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_alphabetic())
                .count();
            component = Some(component_index(&after_dot[..sw_len])?);
            rest = &after_dot[sw_len..];
        }
        if !is_builtin_name(name) {
            return None;
        }
        return Some((Expr::Builtin(name.to_string(), component), rest));
    }

    // Numeric literal.
    let num_len = t
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == '.')
        .count();
    if num_len > 0 {
        let v: f32 = t[..num_len].parse().ok()?;
        return Some((Expr::Literal(v), &t[num_len..]));
    }

    None
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

fn read_builtin(name: &str, state: &ParticleShaderState) -> Value {
    match name {
        "VELOCITY" => Value::Vec4(state.velocity.x, state.velocity.y, 0.0, 0.0),
        "COLOR" => Value::Vec4(state.color.r, state.color.g, state.color.b, state.color.a),
        "TRANSFORM" => Value::Vec4(state.position.x, state.position.y, 0.0, 0.0),
        "DELTA" => Value::Scalar(state.delta),
        "LIFETIME" => Value::Scalar(state.lifetime),
        "AGE" => Value::Scalar(state.age),
        "RANDOM_SEED" => Value::Scalar(state.random_seed),
        _ => Value::Scalar(0.0),
    }
}

fn eval(expr: &Expr, state: &ParticleShaderState) -> Value {
    match expr {
        Expr::Literal(v) => Value::Scalar(*v),
        Expr::Builtin(name, comp) => {
            let v = read_builtin(name, state);
            match comp {
                Some(i) => Value::Scalar(v.component(*i)),
                None => v,
            }
        }
        Expr::Vec(args) => {
            let mut c = [0.0f32; 4];
            for (i, a) in args.iter().take(4).enumerate() {
                c[i] = eval(a, state).as_scalar();
            }
            Value::Vec4(c[0], c[1], c[2], c[3])
        }
        Expr::Binary(l, op, r) => {
            let a = eval(l, state);
            let b = eval(r, state);
            // Scalar-vector mixing broadcasts the scalar, as GLSL does.
            match (a, b) {
                (Value::Scalar(x), Value::Scalar(y)) => Value::Scalar(apply(x, *op, y)),
                (Value::Vec4(x0, x1, x2, x3), Value::Scalar(y)) => Value::Vec4(
                    apply(x0, *op, y),
                    apply(x1, *op, y),
                    apply(x2, *op, y),
                    apply(x3, *op, y),
                ),
                (Value::Scalar(x), Value::Vec4(y0, y1, y2, y3)) => Value::Vec4(
                    apply(x, *op, y0),
                    apply(x, *op, y1),
                    apply(x, *op, y2),
                    apply(x, *op, y3),
                ),
                (Value::Vec4(x0, x1, x2, x3), Value::Vec4(y0, y1, y2, y3)) => Value::Vec4(
                    apply(x0, *op, y0),
                    apply(x1, *op, y1),
                    apply(x2, *op, y2),
                    apply(x3, *op, y3),
                ),
            }
        }
    }
}

fn apply(a: f32, op: char, b: f32) -> f32 {
    match op {
        '+' => a + b,
        '-' => a - b,
        '*' => a * b,
        // A shader dividing by zero yields inf in GLSL too; not special-cased,
        // because substituting 0 would hide the authoring bug.
        '/' => a / b,
        _ => a,
    }
}

fn run(stmts: &[Assignment], state: &mut ParticleShaderState) {
    for a in stmts {
        let rhs = eval(&a.expr, state);
        let current = read_builtin(&a.target, state);
        let new = match a.component {
            // Whole-target write.
            None => match a.op {
                None => rhs,
                Some(op) => match (current, rhs) {
                    (Value::Vec4(c0, c1, c2, c3), r) => Value::Vec4(
                        apply(c0, op, r.component(0)),
                        apply(c1, op, r.component(1)),
                        apply(c2, op, r.component(2)),
                        apply(c3, op, r.component(3)),
                    ),
                    (Value::Scalar(c), r) => Value::Scalar(apply(c, op, r.as_scalar())),
                },
            },
            // Single-component write: leave the other components alone.
            Some(i) => {
                let mut c = [
                    current.component(0),
                    current.component(1),
                    current.component(2),
                    current.component(3),
                ];
                let v = rhs.as_scalar();
                c[i] = match a.op {
                    None => v,
                    Some(op) => apply(c[i], op, v),
                };
                Value::Vec4(c[0], c[1], c[2], c[3])
            }
        };
        write_builtin(&a.target, new, state);
    }
}

fn write_builtin(name: &str, v: Value, state: &mut ParticleShaderState) {
    match name {
        "VELOCITY" => state.velocity = Vector2::new(v.component(0), v.component(1)),
        "COLOR" => {
            state.color = Color::new(
                v.component(0),
                v.component(1),
                v.component(2),
                v.component(3),
            )
        }
        "TRANSFORM" => state.position = Vector2::new(v.component(0), v.component(1)),
        // DELTA / LIFETIME / RANDOM_SEED are read-only in Godot; a write is
        // dropped rather than corrupting the step's own timing.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ParticleShaderState {
        ParticleShaderState {
            velocity: Vector2::new(0.0, 0.0),
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            position: Vector2::new(0.0, 0.0),
            delta: 0.5,
            lifetime: 2.0,
            age: 0.0,
            random_seed: 0.25,
        }
    }

    /// The bead's own shader body.
    #[test]
    fn the_acceptance_shader_parses_and_runs() {
        let src = "shader_type particles;\nvoid process() {\n    VELOCITY.y += 100.0 * DELTA;\n    COLOR = vec4(1.0, 0.0, 0.0, 1.0);\n}\n";
        let p = parse_particles_shader(src);
        assert!(p.unsupported.is_empty(), "{:?}", p.unsupported);

        let mut s = state();
        p.run_process(&mut s);
        assert_eq!(s.velocity.y, 50.0, "100 * 0.5 delta");
        assert_eq!(s.velocity.x, 0.0, "a .y write must not touch x");
        assert_eq!(s.color, Color::new(1.0, 0.0, 0.0, 1.0));
    }

    /// Repeated steps accumulate — the difference between "assigns once" and
    /// "integrates", which a single-step assertion cannot tell apart.
    #[test]
    fn process_accumulates_across_steps() {
        let p = parse_particles_shader(
            "shader_type particles;\nvoid process() { VELOCITY.y += 100.0 * DELTA; }\n",
        );
        let mut s = state();
        for _ in 0..4 {
            p.run_process(&mut s);
        }
        assert_eq!(s.velocity.y, 200.0, "4 steps x 100 x 0.5");
    }

    #[test]
    fn start_and_process_are_separate_stages() {
        let p = parse_particles_shader(
            "shader_type particles;\nvoid start() { VELOCITY = vec2(10.0, -20.0); }\nvoid process() { VELOCITY.x += 1.0; }\n",
        );
        assert!(p.has_start());
        let mut s = state();
        p.run_start(&mut s);
        assert_eq!((s.velocity.x, s.velocity.y), (10.0, -20.0));
        p.run_process(&mut s);
        assert_eq!((s.velocity.x, s.velocity.y), (11.0, -20.0));
    }

    /// Precedence, parentheses and unary minus — an expression evaluator that
    /// got these wrong would still produce motion, just the wrong motion.
    #[test]
    fn arithmetic_respects_precedence() {
        let p = parse_particles_shader(
            "shader_type particles;\nvoid process() { VELOCITY.x = 2.0 + 3.0 * 4.0; VELOCITY.y = (2.0 + 3.0) * 4.0; }\n",
        );
        let mut s = state();
        p.run_process(&mut s);
        assert_eq!(s.velocity.x, 14.0, "* binds tighter than +");
        assert_eq!(s.velocity.y, 20.0, "parentheses override");

        let p = parse_particles_shader(
            "shader_type particles;\nvoid process() { VELOCITY.x = -5.0 + 1.0; }\n",
        );
        let mut s = state();
        p.run_process(&mut s);
        assert_eq!(s.velocity.x, -4.0);
    }

    /// ⚠️ Unrecognised statements are REPORTED, not silently skipped. A
    /// half-applied shader still moves particles, so it looks wired while the
    /// dropped lines do nothing.
    #[test]
    fn unsupported_statements_are_reported_and_the_rest_still_runs() {
        let p = parse_particles_shader(
            "shader_type particles;\nvoid process() {\n    VELOCITY.y += 1.0;\n    emit_subparticle(TRANSFORM, VELOCITY, COLOR, CUSTOM, 0);\n}\n",
        );
        assert_eq!(p.unsupported.len(), 1, "{:?}", p.unsupported);
        assert!(
            p.unsupported[0].contains("emit_subparticle"),
            "the report must name the statement: {:?}",
            p.unsupported
        );
        let mut s = state();
        p.run_process(&mut s);
        assert_eq!(s.velocity.y, 1.0, "the recognised statement still ran");
    }

    /// A multi-component swizzle is rejected rather than applied to one
    /// channel — `COLOR.rgb *= 0.5` treated as `COLOR.r` would dim red only,
    /// which looks like a tint bug rather than an unsupported construct.
    #[test]
    fn multi_component_swizzles_are_rejected_not_misapplied() {
        let p = parse_particles_shader(
            "shader_type particles;\nvoid process() { COLOR.rgb = vec3(0.0, 0.0, 0.0); }\n",
        );
        assert_eq!(p.unsupported.len(), 1);
        let mut s = state();
        p.run_process(&mut s);
        assert_eq!(s.color, Color::new(1.0, 1.0, 1.0, 1.0), "unchanged");
    }

    /// Read-only built-ins stay read-only; writing DELTA must not corrupt the
    /// step's own timing for the statements after it.
    #[test]
    fn writes_to_read_only_builtins_are_dropped() {
        let p = parse_particles_shader(
            "shader_type particles;\nvoid process() { DELTA = 100.0; VELOCITY.y += DELTA; }\n",
        );
        let mut s = state();
        p.run_process(&mut s);
        assert_eq!(s.delta, 0.5, "DELTA is read-only");
        assert_eq!(s.velocity.y, 0.5, "the later statement saw the real DELTA");
    }

    /// A shader with no recognised stage is empty rather than half-parsed.
    #[test]
    fn a_shader_without_stages_is_empty() {
        let p = parse_particles_shader("shader_type particles;\nuniform float speed;\n");
        assert!(p.is_empty());
        assert!(p.unsupported.is_empty(), "nothing to report if nothing ran");
    }

    /// Nested braces must not end the stage body early.
    #[test]
    fn nested_blocks_do_not_truncate_the_stage() {
        let p = parse_particles_shader(
            "shader_type particles;\nvoid process() {\n    if (true) { VELOCITY.x = 1.0; }\n    VELOCITY.y = 2.0;\n}\n",
        );
        let mut s = state();
        p.run_process(&mut s);
        // The `if` is not interpreted, but the statement AFTER the nested block
        // must still be seen — a naive `find('}')` would have stopped there.
        assert_eq!(s.velocity.y, 2.0, "statement after a nested block was lost");
        assert!(!p.unsupported.is_empty(), "the `if` is reported");
    }
}
