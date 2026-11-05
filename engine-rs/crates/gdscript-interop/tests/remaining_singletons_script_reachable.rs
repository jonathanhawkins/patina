//! pat-s3rrv.211 — the eight remaining Godot 4.6.1 singletons are reachable
//! from script with web-target-correct answers.
//!
//! Before this, `IP`, `TextServerManager`, `NativeMenu`,
//! `NavigationMeshGenerator` and the four `*Manager` names appeared nowhere in
//! the interpreter: every one of them failed with `UndefinedVariable`. Two of
//! them had real implementations sitting unreferenced —
//! `gdplatform::native_menu` is 822 lines whose only consumer was `macos.rs`,
//! and `gdrender2d::text_server` is the shipped shaper.
//!
//! ⚠️ **One acceptance clause is wrong and this file deliberately contradicts
//! it.** The bead asserts a queued resolve reaches `RESOLVER_STATUS_DONE`. In
//! `core/io/ip.cpp:105` the terminal status is
//! `response.is_empty() ? RESOLVER_STATUS_ERROR : RESOLVER_STATUS_DONE`, and
//! `IPWeb::_resolve_hostname` (`platform/web/ip_web.h:38`) has an **empty
//! body**, so on the web target an uncached lookup is always empty and always
//! ends in `ERROR`. `DONE` is reachable only through the resolver cache
//! (ip.cpp:171-173), which `a_cached_hostname_reaches_done` covers.
//!
//! Integration tests rather than unit tests: `-p gdscript-interop --lib` cannot
//! pass, because a pre-existing `fuzz_deeply_nested_parens` stack overflow
//! cancels the run.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Serializes the tests in this binary that touch the *Server*Manager singleton registries (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `src` and returns its `return` value.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

/// The eight names this bead adds.
const EIGHT: [&str; 8] = [
    "IP",
    "TextServerManager",
    "NativeMenu",
    "NavigationMeshGenerator",
    "PhysicsServer2DManager",
    "PhysicsServer3DManager",
    "NavigationServer2DManager",
    "NavigationServer3DManager",
];

/// One valid, side-effect-free call per singleton — the direct test that the
/// name resolves at all rather than raising `UndefinedVariable`.
const ONE_CALL_EACH: [(&str, &str); 8] = [
    ("IP", "IP.get_local_addresses()"),
    (
        "TextServerManager",
        "TextServerManager.get_interface_count()",
    ),
    ("NativeMenu", "NativeMenu.has_feature(0)"),
    (
        "NavigationMeshGenerator",
        "NavigationMeshGenerator.is_baking()",
    ),
    (
        "PhysicsServer2DManager",
        "PhysicsServer2DManager.get_servers_count()",
    ),
    (
        "PhysicsServer3DManager",
        "PhysicsServer3DManager.get_servers_count()",
    ),
    (
        "NavigationServer2DManager",
        "NavigationServer2DManager.get_servers_count()",
    ),
    (
        "NavigationServer3DManager",
        "NavigationServer3DManager.get_servers_count()",
    ),
];

#[test]
fn engine_has_singleton_admits_all_eight() {
    for name in EIGHT {
        assert_eq!(
            run_val(&format!("return Engine.has_singleton(\"{name}\")\n")),
            Variant::Bool(true),
            "`Engine.has_singleton(\"{name}\")` must be true — the defensive \
             idiom `if Engine.has_singleton(X): X.foo()` depends on it"
        );
    }
}

#[test]
fn every_name_resolves_instead_of_raising_undefined_variable() {
    for (name, call) in ONE_CALL_EACH {
        let mut interp = Interpreter::new();
        let result = interp.run(&format!("return {call}\n"));
        assert!(
            result.is_ok(),
            "`{call}` must answer — `{name}` used to fail with UndefinedVariable. \
             Got: {:?}",
            result.err()
        );
    }
}

// ---------------------------------------------------------------------------
// IP — present-and-empty, mirroring IPWeb.
// ---------------------------------------------------------------------------

#[test]
fn ip_reports_no_local_addresses_or_interfaces() {
    assert_eq!(
        run_val("return IP.get_local_addresses()\n"),
        Variant::PackedStringArray(vec![]),
        "IPWeb has no local addresses"
    );
    assert_eq!(
        run_val("return IP.get_local_interfaces()\n"),
        Variant::Array(Default::default()),
        "IPWeb::get_local_interfaces has an empty body"
    );
}

#[test]
fn ip_resolve_hostname_is_empty_on_the_web_target() {
    assert_eq!(
        run_val("return IP.resolve_hostname(\"example.com\")\n"),
        Variant::String(String::new())
    );
    assert_eq!(
        run_val("return IP.resolve_hostname_addresses(\"example.com\")\n"),
        Variant::PackedStringArray(vec![])
    );
}

#[test]
fn an_uncached_queue_item_ends_in_error_not_done() {
    // ip.cpp:105 — the bead's acceptance text says DONE, which is wrong for the
    // web target. See the module header.
    let status = run_val(
        "var id = IP.resolve_hostname_queue_item(\"example.com\")\n\
         return IP.get_resolve_item_status(id)\n",
    );
    assert_eq!(
        status,
        Variant::Int(3),
        "an empty response is RESOLVER_STATUS_ERROR (3), not DONE"
    );
    assert_eq!(
        status,
        run_val("return IP.RESOLVER_STATUS_ERROR\n"),
        "the literal and the class constant must agree"
    );
}

#[test]
fn a_queued_item_yields_a_usable_id_and_can_be_erased() {
    // A negative control for the status assertions above: the id must be a real
    // slot, not the invalid sentinel, or every status check would pass vacuously.
    let id = run_val("return IP.resolve_hostname_queue_item(\"example.com\")\n");
    assert_ne!(
        id,
        Variant::Int(-1),
        "RESOLVER_INVALID_ID means nothing was queued"
    );
    assert_eq!(
        run_val(
            "var id = IP.resolve_hostname_queue_item(\"example.com\")\n\
             IP.erase_resolve_item(id)\n\
             return IP.get_resolve_item_status(id)\n"
        ),
        Variant::Int(0),
        "an erased slot reports RESOLVER_STATUS_NONE"
    );
}

#[test]
fn ip_class_constants_match_godots_values() {
    for (member, want) in [
        ("RESOLVER_STATUS_NONE", 0),
        ("RESOLVER_STATUS_WAITING", 1),
        ("RESOLVER_STATUS_DONE", 2),
        ("RESOLVER_STATUS_ERROR", 3),
        ("RESOLVER_MAX_QUERIES", 256),
        ("RESOLVER_INVALID_ID", -1),
        ("TYPE_NONE", 0),
        ("TYPE_IPV4", 1),
        ("TYPE_IPV6", 2),
        ("TYPE_ANY", 3),
    ] {
        assert_eq!(
            run_val(&format!("return IP.{member}\n")),
            Variant::Int(want),
            "IP.{member} — transcribed from core/io/ip.h"
        );
    }
}

// ---------------------------------------------------------------------------
// TextServerManager
// ---------------------------------------------------------------------------

#[test]
fn text_server_manager_ships_a_primary_interface() {
    match run_val("return TextServerManager.get_interface_count()\n") {
        Variant::Int(n) => assert!(n >= 1, "at least one interface must ship, got {n}"),
        other => panic!("expected an int, got {other:?}"),
    }
    let primary = run_val("return TextServerManager.get_primary_interface()\n");
    assert_ne!(
        primary,
        Variant::Nil,
        "the primary interface must be non-null"
    );
    assert_eq!(
        run_val("return TextServerManager.find_interface(\"TextServerLite\")\n"),
        Variant::Int(0),
        "the shipped shaper is findable by name"
    );
}

// ---------------------------------------------------------------------------
// NativeMenu — no global menu on web, but faithful bookkeeping.
// ---------------------------------------------------------------------------

#[test]
fn native_menu_reports_no_global_menu_on_web() {
    assert_eq!(
        run_val("return NativeMenu.has_feature(NativeMenu.FEATURE_GLOBAL_MENU)\n"),
        Variant::Bool(false)
    );
    assert_eq!(
        run_val("return NativeMenu.has_system_menu(NativeMenu.MAIN_MENU_ID)\n"),
        Variant::Bool(false)
    );
}

#[test]
fn native_menu_items_round_trip() {
    assert_eq!(
        run_val(
            "var m = NativeMenu.create_menu()\n\
             NativeMenu.add_item(m, \"Open\")\n\
             return NativeMenu.get_item_count(m)\n"
        ),
        Variant::Int(1),
        "one added item must be counted"
    );
    assert_eq!(
        run_val(
            "var m = NativeMenu.create_menu()\n\
             NativeMenu.add_item(m, \"Open\")\n\
             return NativeMenu.get_item_text(m, 0)\n"
        ),
        Variant::String("Open".into()),
        "the item's label must survive the round-trip"
    );
}

#[test]
fn native_menu_handles_stay_distinct() {
    // A negative control: if create_menu handed back the same handle twice, the
    // round-trip test above would pass while the model was badly broken.
    assert_eq!(
        run_val(
            "var a = NativeMenu.create_menu()\n\
             var b = NativeMenu.create_menu()\n\
             NativeMenu.add_item(a, \"OnlyInA\")\n\
             return NativeMenu.get_item_count(b)\n"
        ),
        Variant::Int(0),
        "an item added to one menu must not appear in another"
    );
}

#[test]
fn native_menu_constants_match_godots_values() {
    for (member, want) in [
        ("FEATURE_GLOBAL_MENU", 0),
        ("FEATURE_POPUP_MENU", 1),
        ("FEATURE_OPEN_CLOSE_CALLBACK", 2),
        ("FEATURE_HOVER_CALLBACK", 3),
        ("FEATURE_KEY_CALLBACK", 4),
        ("INVALID_MENU_ID", 0),
        ("MAIN_MENU_ID", 1),
        ("APPLICATION_MENU_ID", 2),
        ("WINDOW_MENU_ID", 3),
        ("HELP_MENU_ID", 4),
        ("DOCK_MENU_ID", 5),
    ] {
        assert_eq!(
            run_val(&format!("return NativeMenu.{member}\n")),
            Variant::Int(want),
            "NativeMenu.{member} — transcribed from servers/display/native_menu.h"
        );
    }
}

// ---------------------------------------------------------------------------
// The four *Manager singletons
// ---------------------------------------------------------------------------

#[test]
fn every_manager_names_the_server_patina_ships() {
    let _guard = serial();
    for (singleton, shipped) in [
        ("PhysicsServer2DManager", "PatinaPhysics2D"),
        ("PhysicsServer3DManager", "PatinaPhysics3D"),
        ("NavigationServer2DManager", "PatinaNavigation2D"),
        ("NavigationServer3DManager", "PatinaNavigation3D"),
    ] {
        match run_val(&format!("return {singleton}.get_servers_count()\n")) {
            Variant::Int(n) => assert!(n >= 1, "{singleton} must list a server, got {n}"),
            other => panic!("expected an int, got {other:?}"),
        }
        assert_eq!(
            run_val(&format!("return {singleton}.get_server_name(0)\n")),
            Variant::String(shipped.into()),
            "{singleton} must name the server Patina actually ships"
        );
        assert_eq!(
            run_val(&format!("return {singleton}.get_default_server_name()\n")),
            Variant::String(shipped.into()),
            "{singleton}'s default is the shipped server"
        );
    }
}

#[test]
fn a_registered_server_becomes_findable_and_default() {
    let _guard = serial();
    assert_eq!(
        run_val(
            "PhysicsServer3DManager.register_server(\"Custom3D\")\n\
             PhysicsServer3DManager.set_default_server(\"Custom3D\")\n\
             return PhysicsServer3DManager.get_default_server_name()\n"
        ),
        Variant::String("Custom3D".into()),
        "register_server + set_default_server must move the default"
    );
    // The registry is process-global and outlives this test: put the shipped
    // default back, or `every_manager_names_the_server_patina_ships` reads
    // "Custom3D" whenever it runs after this one in the same process.
    run_val("PhysicsServer3DManager.set_default_server(\"PatinaPhysics3D\")\nreturn 0\n");
}

// ---------------------------------------------------------------------------
// NavigationMeshGenerator
// ---------------------------------------------------------------------------

#[test]
fn baking_source_geometry_produces_polygons() {
    let baked = run_val(
        "var geo = {\"vertices\": [Vector3(0, 0, 0), Vector3(1, 0, 0), Vector3(0, 0, 1)], \
         \"indices\": [0, 1, 2]}\n\
         return NavigationMeshGenerator.bake_from_source_geometry_data(geo)\n",
    );
    let Variant::Dictionary(d) = baked else {
        panic!("expected a navmesh dictionary, got {baked:?}");
    };
    match d.get(Variant::String("polygons".into())) {
        Some(Variant::Array(polys)) => {
            assert_eq!(polys.len(), 1, "one triangle in, one polygon out");
            assert_eq!(
                polys.to_vec()[0],
                Variant::PackedInt32Array(vec![0, 1, 2]),
                "the polygon must carry the source triangle's corners"
            );
        }
        other => panic!("expected a polygons array, got {other:?}"),
    }
    match d.get(Variant::String("vertices".into())) {
        Some(Variant::PackedVector3Array(vs)) => {
            assert_eq!(vs.len(), 3, "the source vertices must survive the bake")
        }
        other => panic!("expected a vertex array, got {other:?}"),
    }
}

#[test]
fn baking_is_synchronous_so_nothing_is_ever_in_flight() {
    assert_eq!(
        run_val("return NavigationMeshGenerator.is_baking()\n"),
        Variant::Bool(false)
    );
}
