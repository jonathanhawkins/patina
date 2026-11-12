//! pat-rp4bp — a multi-line lambda inside call parentheses, and a lambda
//! return type, parse and run.
//!
//! truck_town's `vehicles/vehicle.gd:122` (the official Godot demo, unmodified):
//!
//! ```gdscript
//! t.finished.connect(func() -> void:
//!     node.visible = false
//! )
//! ```
//!
//! failed with "unexpected token -> … expected :" (no lambda return type),
//! and with that fixed, "unexpected token =" on line 123: the tokenizer
//! suppresses NEWLINE and indentation inside brackets, so the lambda's
//! indented body arrived as one run-on expression. Godot's parser switches the
//! tokenizer back to indentation mode for a lambda body
//! (`gdscript_parser.cpp` `parse_lambda` → `push_expression_indented_block`)
//! and restores the bracket mode after it; Patina's tokenizer now does the same
//! by recognising the body itself.

use gdscript_interop::interpreter::Interpreter;
use gdscript_interop::parser::Parser;
use gdscript_interop::tokenizer::tokenize;
use gdvariant::Variant;

fn repo_games() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games")
}

fn parse(src: &str) -> Result<(), String> {
    let tokens = tokenize(src).map_err(|e| format!("lex: {e:?}"))?;
    Parser::new(tokens, src)
        .parse_script()
        .map(|_| ())
        .map_err(|e| format!("parse: {e}"))
}

/// Runs `body` as a method and returns its value.
fn run(body: &str) -> Variant {
    let src = format!("extends Node\nfunc run():\n{body}");
    let mut interp = Interpreter::new();
    let class_def = interp
        .run_class(&src)
        .unwrap_or_else(|e| panic!("script parses: {e}\n{src}"));
    let mut inst = interp.instantiate_class(&class_def).expect("instantiates");
    interp
        .call_instance_method(&mut inst, "run", &[])
        .unwrap_or_else(|e| panic!("run() executes: {e}\n{src}"))
}

fn ints(v: Variant) -> Vec<i64> {
    match v {
        Variant::Array(a) => a
            .to_vec()
            .into_iter()
            .map(|x| match x {
                Variant::Int(i) => i,
                other => panic!("expected Int, got {other:?}"),
            })
            .collect(),
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn truck_town_vehicle_gd_parses() {
    let path = repo_games().join("truck_town/vehicles/vehicle.gd");
    let src = std::fs::read_to_string(&path).expect("truck_town fixture");
    assert!(
        src.contains("t.finished.connect(func() -> void:"),
        "fixture still holds the multi-line lambda this test is about"
    );
    parse(&src).unwrap_or_else(|e| panic!("vehicle.gd must parse, got {e}"));
}

#[test]
fn every_gd_file_in_every_game_parses() {
    let mut files = Vec::new();
    let mut stack = vec![repo_games()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if p.is_dir() && !name.starts_with('.') {
                stack.push(p);
            } else if name.ends_with(".gd") {
                files.push(p);
            }
        }
    }
    files.sort();
    assert!(
        files.len() >= 30,
        "expected the games/* scripts, found {}",
        files.len()
    );
    let failures: Vec<String> = files
        .iter()
        .filter_map(|p| {
            let src = std::fs::read_to_string(p).unwrap();
            parse(&src).err().map(|e| format!("{}: {e}", p.display()))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "scripts that fail to parse:\n{}",
        failures.join("\n")
    );
}

#[test]
fn multi_line_lambda_body_inside_call_parens_runs_every_statement() {
    let v = run(concat!(
        "\tvar out = [1, 2].map(func(v) -> int:\n",
        "\t\tvar d = v * 10\n",
        "\t\treturn d + 1\n",
        "\t)\n",
        "\treturn out\n",
    ));
    assert_eq!(ints(v), vec![11, 21]);
}

#[test]
fn statement_after_the_closing_paren_keeps_the_outer_indentation() {
    // The indent stack must be restored after the body: the `if` after the
    // call is still inside `run`, and its own block still indents.
    let v = run(concat!(
        "\tvar log = []\n",
        "\tvar f = (func() -> void:\n",
        "\t\tlog.append(1)\n",
        "\t\tlog.append(2)\n",
        "\t)\n",
        "\tf.call()\n",
        "\tif log.size() == 2:\n",
        "\t\tlog.append(3)\n",
        "\treturn log\n",
    ));
    assert_eq!(ints(v), vec![1, 2, 3]);
}

#[test]
fn closing_line_may_carry_further_arguments() {
    let v = run(concat!(
        "\tvar arr = [3, 1, 2]\n",
        "\tvar total = [1, 2, 3].reduce(func(acc, x):\n",
        "\t\treturn acc + x\n",
        "\t, 100)\n",
        "\tarr.sort_custom(func(a, b): return a > b)\n",
        "\treturn [total] + arr\n",
    ));
    assert_eq!(ints(v), vec![106, 3, 2, 1]);
}

#[test]
fn one_line_lambda_followed_by_another_argument() {
    let v = run("\treturn [[1, 2, 3].map(func(x): return x * 2), 7]\n");
    match v {
        Variant::Array(a) => {
            let a = a.to_vec();
            assert_eq!(ints(a[0].clone()), vec![2, 4, 6]);
            assert_eq!(a[1], Variant::Int(7));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn one_line_statement_body_and_named_lambda() {
    // `func(): x = …` (an assignment, not an expression) and Godot's optional
    // lambda name.
    let v = run(concat!(
        "\tvar box = [0]\n",
        "\tvar set_it = (func named() -> void: box[0] = 5)\n",
        "\tset_it.call()\n",
        "\treturn box\n",
    ));
    assert_eq!(ints(v), vec![5]);
}

#[test]
fn brackets_inside_the_body_suppress_newlines_again_and_nested_lambdas_work() {
    let v = run(concat!(
        "\tvar out = [1, 2].map(func(v):\n",
        "\t\tvar d = {\n",
        "\t\t\t\"k\": v,\n",
        "\t\t}\n",
        "\t\tvar inner = [d[\"k\"]].map(func(w):\n",
        "\t\t\treturn w + 100\n",
        "\t\t)\n",
        "\t\treturn inner[0]\n",
        "\t)\n",
        "\treturn out\n",
    ));
    assert_eq!(ints(v), vec![101, 102]);
}

#[test]
fn closing_paren_on_the_last_body_line() {
    // `x)` — the closer ends the body without a line of its own.
    let v = run(concat!(
        "\tvar out = [1].map(func(v):\n",
        "\t\treturn v + 1)\n",
        "\treturn out\n",
    ));
    assert_eq!(ints(v), vec![2]);
}
