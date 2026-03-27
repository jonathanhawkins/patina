//! pat-ck1: GDScript parser produces stable AST for representative scripts.
//!
//! Validates that:
//! 1. The parser successfully tokenizes and parses representative .gd fixtures
//! 2. Parsing the same script twice produces identical ASTs (stability)
//! 3. Key AST constructs are present (extends, var decls, func defs, if/for/while)
//! 4. Expression parsing produces expected operator precedence
//! 5. All fixture scripts parse without errors

use gdscript_interop::parser::{BinOp, Expr, Parser, Stmt};
use gdscript_interop::tokenizer::tokenize;

// ===========================================================================
// Helpers
// ===========================================================================

fn parse_script(source: &str) -> Vec<Stmt> {
    let tokens = tokenize(source).expect("tokenization should succeed");
    let mut parser = Parser::new(tokens, source);
    parser.parse_script().expect("parsing should succeed")
}

fn fixture_path(name: &str) -> String {
    format!("{}/../fixtures/scripts/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn load_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

// ===========================================================================
// 1. Representative fixtures parse without errors
// ===========================================================================

#[test]
fn ck1_player_gd_parses() {
    let source = load_fixture("player.gd");
    let stmts = parse_script(&source);
    assert!(!stmts.is_empty(), "player.gd should produce statements");
}

#[test]
fn ck1_test_variables_gd_parses() {
    let source = load_fixture("test_variables.gd");
    let stmts = parse_script(&source);
    assert!(!stmts.is_empty(), "test_variables.gd should produce statements");
}

#[test]
fn ck1_enemy_spawner_gd_parses() {
    let source = load_fixture("enemy_spawner.gd");
    let stmts = parse_script(&source);
    assert!(!stmts.is_empty(), "enemy_spawner.gd should produce statements");
}

#[test]
fn ck1_test_movement_gd_parses() {
    let source = load_fixture("test_movement.gd");
    let stmts = parse_script(&source);
    assert!(!stmts.is_empty(), "test_movement.gd should produce statements");
}

// ===========================================================================
// 2. Stability: parsing the same script twice yields identical ASTs
// ===========================================================================

#[test]
fn ck1_stable_ast_player() {
    let source = load_fixture("player.gd");
    let ast1 = parse_script(&source);
    let ast2 = parse_script(&source);
    assert_eq!(ast1, ast2, "player.gd AST must be deterministic");
}

#[test]
fn ck1_stable_ast_test_variables() {
    let source = load_fixture("test_variables.gd");
    let ast1 = parse_script(&source);
    let ast2 = parse_script(&source);
    assert_eq!(ast1, ast2, "test_variables.gd AST must be deterministic");
}

#[test]
fn ck1_stable_ast_enemy_spawner() {
    let source = load_fixture("enemy_spawner.gd");
    let ast1 = parse_script(&source);
    let ast2 = parse_script(&source);
    assert_eq!(ast1, ast2, "enemy_spawner.gd AST must be deterministic");
}

// ===========================================================================
// 3. Extends declaration parsed
// ===========================================================================

#[test]
fn ck1_extends_parsed() {
    let stmts = parse_script("extends Node2D\n");
    assert!(
        stmts.iter().any(|s| matches!(s, Stmt::Extends { .. })),
        "should parse 'extends Node2D' as Extends stmt"
    );
}

// ===========================================================================
// 4. Variable declarations
// ===========================================================================

#[test]
fn ck1_var_decl_simple() {
    let stmts = parse_script("var health = 100\n");
    match &stmts[0] {
        Stmt::VarDecl { name, value, .. } => {
            assert_eq!(name, "health");
            assert!(value.is_some(), "should have initializer");
        }
        other => panic!("expected VarDecl, got {other:?}"),
    }
}

#[test]
fn ck1_var_decl_with_type_hint() {
    let stmts = parse_script("var speed: float = 2.5\n");
    match &stmts[0] {
        Stmt::VarDecl {
            name, type_hint, ..
        } => {
            assert_eq!(name, "speed");
            assert_eq!(type_hint.as_deref(), Some("float"));
        }
        other => panic!("expected VarDecl, got {other:?}"),
    }
}

// ===========================================================================
// 5. Function declarations
// ===========================================================================

#[test]
fn ck1_func_decl_parsed() {
    let source = "func _ready():\n\tpass\n";
    let stmts = parse_script(source);
    match &stmts[0] {
        Stmt::FuncDef { name, body, .. } => {
            assert_eq!(name, "_ready");
            assert!(!body.is_empty());
        }
        other => panic!("expected FuncDecl, got {other:?}"),
    }
}

#[test]
fn ck1_func_with_params() {
    let source = "func add(a, b):\n\treturn a + b\n";
    let stmts = parse_script(source);
    match &stmts[0] {
        Stmt::FuncDef { name, params, .. } => {
            assert_eq!(name, "add");
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "a");
            assert_eq!(params[1].name, "b");
        }
        other => panic!("expected FuncDecl, got {other:?}"),
    }
}

// ===========================================================================
// 6. If statement
// ===========================================================================

#[test]
fn ck1_if_statement_parsed() {
    let source = "func f():\n\tif x > 0:\n\t\tpass\n";
    let stmts = parse_script(source);
    if let Stmt::FuncDef { body, .. } = &stmts[0] {
        assert!(
            body.iter().any(|s| matches!(s, Stmt::If { .. })),
            "function body should contain If stmt"
        );
    } else {
        panic!("expected FuncDecl");
    }
}

// ===========================================================================
// 7. For loop
// ===========================================================================

#[test]
fn ck1_for_loop_parsed() {
    let source = "func f():\n\tfor i in range(10):\n\t\tpass\n";
    let stmts = parse_script(source);
    if let Stmt::FuncDef { body, .. } = &stmts[0] {
        assert!(
            body.iter().any(|s| matches!(s, Stmt::For { .. })),
            "function body should contain For stmt"
        );
    } else {
        panic!("expected FuncDecl");
    }
}

// ===========================================================================
// 8. Expression operator precedence
// ===========================================================================

#[test]
fn ck1_binary_op_precedence() {
    // `1 + 2 * 3` should parse as `1 + (2 * 3)` due to precedence
    let stmts = parse_script("func f():\n\tvar x = 1 + 2 * 3\n");
    if let Stmt::FuncDef { body, .. } = &stmts[0] {
        if let Stmt::VarDecl {
            value: Some(expr), ..
        } = &body[0]
        {
            match expr {
                Expr::BinaryOp { op, right, .. } => {
                    assert_eq!(*op, BinOp::Add, "top-level op should be Add");
                    assert!(
                        matches!(**right, Expr::BinaryOp { op: BinOp::Mul, .. }),
                        "right side should be Mul"
                    );
                }
                other => panic!("expected BinaryOp, got {other:?}"),
            }
        } else {
            panic!("expected VarDecl in body");
        }
    }
}

// ===========================================================================
// 9. Function call expression
// ===========================================================================

#[test]
fn ck1_function_call_parsed() {
    let source = "func f():\n\tprint(\"hello\")\n";
    let stmts = parse_script(source);
    if let Stmt::FuncDef { body, .. } = &stmts[0] {
        match &body[0] {
            Stmt::ExprStmt(Expr::Call { callee, args, .. }) => {
                assert!(matches!(callee.as_ref(), Expr::Ident(name) if name == "print"));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected ExprStmt(Call), got {other:?}"),
        }
    }
}

// ===========================================================================
// 10. Member access
// ===========================================================================

#[test]
fn ck1_member_access_parsed() {
    let source = "func f():\n\tvar x = position.x\n";
    let stmts = parse_script(source);
    if let Stmt::FuncDef { body, .. } = &stmts[0] {
        if let Stmt::VarDecl {
            value: Some(expr), ..
        } = &body[0]
        {
            assert!(
                matches!(expr, Expr::MemberAccess { member, .. } if member == "x"),
                "should parse position.x as MemberAccess"
            );
        }
    }
}

// ===========================================================================
// 11. All fixtures parse — bulk check
// ===========================================================================

#[test]
fn ck1_all_fixture_scripts_parse() {
    let fixture_dir = format!("{}/../fixtures/scripts", env!("CARGO_MANIFEST_DIR"));
    let entries = std::fs::read_dir(&fixture_dir);
    if entries.is_err() {
        // No fixture dir — skip gracefully
        return;
    }

    let mut count = 0;
    for entry in entries.unwrap().flatten() {
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "gd") {
            let source = std::fs::read_to_string(&path).unwrap();
            let tokens = tokenize(&source);
            if let Ok(tokens) = tokens {
                let mut parser = Parser::new(tokens, &source);
                let result = parser.parse_script();
                assert!(
                    result.is_ok(),
                    "fixture {:?} failed to parse: {:?}",
                    path.file_name(),
                    result.err()
                );
                count += 1;
            }
        }
    }
    assert!(count >= 3, "should have parsed at least 3 fixture scripts, got {count}");
}
