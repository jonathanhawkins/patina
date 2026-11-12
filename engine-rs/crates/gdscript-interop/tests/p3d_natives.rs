//! pat-v3co1 — interpreter-level natives the official Godot 3D platformer demo
//! calls, pinned by value. The demo's own scripts run end to end in
//! `gdscene/tests/p3d_real_scripts.rs`; this file pins each native's Godot
//! semantics in isolation, including the branches the demo does not take.
//!
//! Short file name on purpose: `gdscript-interop` is a long package name and a
//! long binary name pushes the verify command past its truncation width.

use gdscript_interop::interpreter::{reset_project_settings, seed_project_settings, Interpreter};
use gdscript_interop::{tokenize, Expr, Parser, Stmt};
use gdvariant::Variant;

/// Runs `check()` of `src` through real VM dispatch (no scene attached).
fn call_check(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() raised: {e:?}"))
}

const RENDERING_METHOD: &str = "\
extends Node

func check():
    return RenderingServer.get_current_rendering_method()
";

// -- RenderingServer.get_current_rendering_method ---------------------------

/// No project seeded at all answers Godot's default.
#[test]
fn rendering_method_defaults_to_forward_plus() {
    reset_project_settings();
    assert_eq!(
        call_check(RENDERING_METHOD),
        Variant::String("forward_plus".into())
    );
}

/// 🔑 It answers the PROJECT's choice, not a constant: a Compatibility project
/// must take its scripts' `gl_compatibility` branches as it would in Godot.
#[test]
fn rendering_method_follows_the_project_setting() {
    for method in ["gl_compatibility", "mobile", "forward_plus"] {
        reset_project_settings();
        seed_project_settings([(
            "rendering/renderer/rendering_method",
            Variant::String(method.into()),
        )]);
        assert_eq!(
            call_check(RENDERING_METHOD),
            Variant::String(method.into()),
            "a project declaring {method} must report it"
        );
    }
}

/// A name Godot does not know reads as the default rather than being echoed —
/// Godot itself never reports a method it cannot run.
#[test]
fn an_unknown_rendering_method_reads_as_forward_plus() {
    reset_project_settings();
    seed_project_settings([(
        "rendering/renderer/rendering_method",
        Variant::String("vulkan_please".into()),
    )]);
    assert_eq!(
        call_check(RENDERING_METHOD),
        Variant::String("forward_plus".into())
    );
}

// -- DisplayServer.is_touchscreen_available ---------------------------------

const TOUCH: &str = "\
extends Node

func check():
    return DisplayServer.is_touchscreen_available()
";

/// A desktop with no platform backend and no touch emulation has none.
#[test]
fn touchscreen_is_unavailable_on_a_plain_desktop() {
    reset_project_settings();
    assert_eq!(call_check(TOUCH), Variant::Bool(false));
}

/// Godot's base implementation IS `Input.is_emulating_touch_from_mouse()`,
/// seeded from this project setting.
#[test]
fn touchscreen_is_available_when_the_project_emulates_touch() {
    reset_project_settings();
    seed_project_settings([(
        "input_devices/pointing/emulate_touch_from_mouse",
        Variant::Bool(true),
    )]);
    assert_eq!(call_check(TOUCH), Variant::Bool(true));
}

// -- `%UniqueName` --------------------------------------------------------

fn parse(src: &str) -> Vec<Stmt> {
    let tokens = tokenize(src).expect("tokenizes");
    Parser::new(tokens, src)
        .parse_script()
        .unwrap_or_else(|e| panic!("`{src}` must parse: {e:?}"))
}

fn var_value(src: &str) -> Expr {
    match parse(src).into_iter().next() {
        Some(Stmt::VarDecl { value: Some(v), .. }) => v,
        other => panic!("expected a var with a value, got {other:?}"),
    }
}

/// `%Name` is `get_node("%Name")` — the exact form player.gd uses. Before the
/// fix this was "unexpected token %" and the whole script failed to load.
#[test]
fn percent_unique_name_parses_as_a_node_path() {
    assert!(matches!(var_value("var n = %CoinCount\n"), Expr::GetNode(ref p) if p == "%CoinCount"));
    assert!(
        matches!(var_value("var n = %CoinCount/Parallax\n"), Expr::GetNode(ref p) if p == "%CoinCount/Parallax"),
        "a unique name may lead a longer path"
    );
    assert!(
        matches!(var_value("var n = $%CoinCount\n"), Expr::GetNode(ref p) if p == "%CoinCount"),
        "`$%Name` is the same lookup"
    );
    assert!(
        matches!(var_value("var n = %\"Coin Count\"\n"), Expr::GetNode(ref p) if p == "%Coin Count"),
        "the quoted form keeps its spaces"
    );
    // player.gd line 47, verbatim: a member assignment through the shorthand.
    parse("func f():\n    %CoinCount.text = str(1)\n");
}

/// GUARD: binary `%` is still modulo.
#[test]
fn binary_percent_is_still_modulo() {
    assert!(matches!(
        var_value("var n = 7 % 3\n"),
        Expr::BinaryOp {
            op: gdscript_interop::BinOp::Mod,
            ..
        }
    ));
    assert_eq!(
        call_check("extends Node\nfunc check():\n    var a = 7\n    return a % 3\n"),
        Variant::Int(1)
    );
}

// -- Input.get_vector with StringNames --------------------------------------

/// The demo writes `Input.get_vector(&"move_left", ...)`. Without a scene the
/// answer is the zero vector; before the fix it was a TypeError every frame.
#[test]
fn input_get_vector_accepts_stringname_actions() {
    assert_eq!(
        call_check(
            "extends Node\nfunc check():\n    return Input.get_vector(&\"move_left\", &\"move_right\", &\"move_forward\", &\"move_back\")\n"
        ),
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0))
    );
}

// -- PhysicsRayQueryParameters3D.create -------------------------------------

/// `create(from, to, mask, exclude)` sets all four, and the omitted optional
/// arguments take Godot's defaults (all layers, nothing excluded).
#[test]
fn ray_query_parameters_3d_create_sets_its_arguments() {
    gdobject::class_db::register_all_classes();
    let full = call_check(
        "extends Node\n\
         func check():\n\
         \x20   var q = PhysicsRayQueryParameters3D.create(Vector3(1, 2, 3), Vector3(4, 5, 6), 5, [RID()])\n\
         \x20   return [q.from, q.to, q.collision_mask, q.exclude.size(), q.hit_from_inside]\n",
    );
    let Variant::Array(full) = full else {
        panic!("expected an Array, got {full:?}")
    };
    let full = full.to_vec();
    assert_eq!(
        full[0],
        Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(
        full[1],
        Variant::Vector3(gdcore::math::Vector3::new(4.0, 5.0, 6.0))
    );
    assert_eq!(full[2], Variant::Int(5));
    assert_eq!(full[3], Variant::Int(1), "the exclude list is carried");
    assert_eq!(
        full[4],
        Variant::Bool(false),
        "unset properties keep their ClassDB defaults, as `.new()` does"
    );

    let short = call_check(
        "extends Node\n\
         func check():\n\
         \x20   var q = PhysicsRayQueryParameters3D.create(Vector3(), Vector3(0, -1, 0))\n\
         \x20   return [q.collision_mask, q.exclude.size()]\n",
    );
    let Variant::Array(short) = short else {
        panic!("expected an Array, got {short:?}")
    };
    assert_eq!(
        short.to_vec(),
        vec![Variant::Int(0xFFFF_FFFF), Variant::Int(0)],
        "Godot's defaults: every layer, nothing excluded"
    );
}
