//! pat-0q2kx.232 (partial) — the particle nodes' METHOD surface in ClassDB.
//!
//! Godot ground truth: `doc/classes/GPUParticles2D.xml` and
//! `CPUParticles2D.xml`.
//!
//! ⚠️ The bead says these node controls are "absent in Patina", but its
//! verifier grepped `gdscene/src` + `patina-runner/src` — the SIMULATOR, not
//! ClassDB. All eight properties it names (preprocess, use_fixed_seed, seed,
//! amount_ratio, interp_to_end, visibility_rect, interpolate, fract_delta) were
//! already registered. What was genuinely missing from ClassDB is the method
//! surface, which is what this file covers.
//!
//! ⚠️ SCOPE: declarations only. The behaviour — preprocess burn-in, seed
//! override, amount_ratio gating, and dispatching `restart()` through to the
//! simulator — is NOT delivered, so the acceptance test
//! `particle_node_methods_and_seed_control` does not exist. See the report.

use gdobject::class_db::{get_class_info, register_2d_classes};

fn methods_of(class: &str) -> Vec<(String, usize)> {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .methods
        .iter()
        .map(|m| (m.name.clone(), m.argument_count))
        .collect()
}

fn assert_method(class: &str, name: &str, arity: usize) {
    let ms = methods_of(class);
    let found = ms
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`; has {ms:?}"));
    assert_eq!(
        found.1, arity,
        "{class}.{name} takes {arity} argument(s) per the doc XML"
    );
}

/// GPUParticles2D's five methods. A script calling `$P.restart()` previously
/// had no declaration to resolve against.
#[test]
fn gpu_particles_2d_declares_its_method_surface() {
    register_2d_classes();

    // ⚠️ restart takes ONE argument (keep_seed), not zero. Every pre-4.x
    // example calls it bare, which makes 0 the natural wrong guess.
    assert_method("GPUParticles2D", "restart", 1);
    assert_method("GPUParticles2D", "emit_particle", 5);
    assert_method("GPUParticles2D", "capture_rect", 0);
    assert_method("GPUParticles2D", "convert_from_particles", 1);
    assert_method("GPUParticles2D", "request_particles_process", 1);
}

/// CPUParticles2D shares only three of them — it has no `emit_particle` or
/// `capture_rect`, and declaring them would advertise a surface Godot lacks.
#[test]
fn cpu_particles_2d_shares_only_the_three_common_methods() {
    register_2d_classes();

    assert_method("CPUParticles2D", "restart", 1);
    assert_method("CPUParticles2D", "convert_from_particles", 1);
    assert_method("CPUParticles2D", "request_particles_process", 1);

    let names: Vec<String> = methods_of("CPUParticles2D")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    for absent in ["emit_particle", "capture_rect"] {
        assert!(
            !names.contains(&absent.to_string()),
            "CPUParticles2D has no `{absent}` in Godot — declaring it would \
             advertise a method that cannot exist"
        );
    }
}

/// `EmitFlags` is a BITMASK. Numbering the constants 0..4 would make
/// `POSITION|VELOCITY` indistinguishable from other combinations, and the flags
/// are the 5th argument of `emit_particle`.
#[test]
fn emit_flags_are_powers_of_two() {
    register_2d_classes();

    let info = get_class_info("GPUParticles2D").expect("registered");
    let e = info
        .enums
        .iter()
        .find(|e| e.name == "EmitFlags")
        .expect("GPUParticles2D must expose EmitFlags");

    for (name, value) in [
        ("EMIT_FLAG_POSITION", 1i64),
        ("EMIT_FLAG_ROTATION_SCALE", 2),
        ("EMIT_FLAG_VELOCITY", 4),
        ("EMIT_FLAG_COLOR", 8),
        ("EMIT_FLAG_CUSTOM", 16),
    ] {
        let c = e
            .constants
            .iter()
            .find(|c| c.0 == name)
            .unwrap_or_else(|| panic!("EmitFlags must define {name}"));
        assert_eq!(c.1, value, "{name} is a bit, not an ordinal");
    }

    // Every value distinct and a power of two — the property that makes an
    // OR-combination decodable.
    let vals: Vec<i64> = e.constants.iter().map(|c| c.1).collect();
    for v in &vals {
        assert!(
            *v > 0 && (*v & (*v - 1)) == 0,
            "EmitFlags value {v} is not a power of two"
        );
    }
    let mut sorted = vals.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        vals.len(),
        "EmitFlags values must be distinct"
    );
}

/// The properties the bead calls missing were already present. Pinned so a
/// future "add the missing properties" pass does not duplicate them — the
/// property list concatenates the chain without deduping.
#[test]
fn the_named_properties_were_already_registered_and_appear_once() {
    register_2d_classes();

    let props = gdobject::class_db::get_property_list("GPUParticles2D");
    for name in [
        "preprocess",
        "use_fixed_seed",
        "seed",
        "amount_ratio",
        "interp_to_end",
        "visibility_rect",
        "interpolate",
        "fract_delta",
    ] {
        assert_eq!(
            props.iter().filter(|p| p.name == name).count(),
            1,
            "GPUParticles2D must report `{name}` exactly once"
        );
    }
}
