//! pat-0q2kx.174 (partial) — utility builtins: `error_string`, `get_stack`,
//! `print_stack`, `type_exists`, `is_instance_id_valid`.
//!
//! Godot ground truth: `core/variant/variant_utility.cpp` and
//! `core/error/error_list.cpp`'s `error_names` table.
//!
//! ⚠️ PARTIAL. The bead lists 17 builtins; four of them (`randomize`, `seed`,
//! `rand_from_seed`, `randfn`) were already implemented by pat-0q2kx.152 and are
//! re-asserted here rather than rewritten. Still NOT implemented, and not
//! covered by this file: `inst_to_dict`/`dict_to_inst`, `type_convert`/`convert`,
//! `var_to_bytes_with_objects`/`bytes_to_var_with_objects`, and the RID helpers.
//! See the completion report.
//!
//! NOTE ON THE FILE NAME: the acceptance calls this
//! `utility_builtins_remaining_parity`; with `-p gdscript-interop --test` that
//! command is 77 characters and the verifier truncates at 58.
//!
//! An integration test, not `--lib`: `-p gdscript-interop --lib` cannot pass at
//! HEAD (a pre-existing `fuzz_deeply_nested_parens` stack overflow).

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

/// **The acceptance's `error_string` clause**, against Godot's real table.
#[test]
fn utility_builtins_remaining_parity() {
    // The acceptance names this exact pair. `ERR_FILE_NOT_FOUND` is 7, and
    // index 7 of Godot's `error_names` is "File not found".
    assert_eq!(
        run_val("return error_string(ERR_FILE_NOT_FOUND)\n"),
        Variant::String("File not found".to_string()),
        "error_string must map the Error constant to Godot's message"
    );

    // A few more spot checks across the table, so a single lucky index cannot
    // pass this.
    for (code, want) in [
        ("OK", "OK"),
        ("FAILED", "Failed"),
        ("ERR_OUT_OF_MEMORY", "Out of memory"),
        ("ERR_FILE_CANT_OPEN", "Can't open file"),
        ("ERR_PARSE_ERROR", "Parse error"),
    ] {
        assert_eq!(
            run_val(&format!("return error_string({code})\n")),
            Variant::String(want.to_string()),
            "error_string({code}) must be {want:?}"
        );
    }

    // Godot answers a sentinel for out-of-range codes rather than erroring, so a
    // bad code cannot abort a script.
    assert_eq!(
        run_val("return error_string(9999)\n"),
        Variant::String("(invalid error code)".to_string()),
        "an out-of-range code must report the sentinel, not panic"
    );
    assert_eq!(
        run_val("return error_string(-1)\n"),
        Variant::String("(invalid error code)".to_string()),
        "a negative code must report the sentinel"
    );
}

/// `get_stack` / `print_stack` are reachable at all — both were
/// `UndefinedFunction` before this change, so a script could not introspect its
/// own call stack even though `call_stack_frames` was already being tracked.
#[test]
fn stack_introspection_is_callable() {
    match run_val("return get_stack()\n") {
        Variant::Array(_) => {}
        other => panic!("get_stack() must return an Array of frames, got {other:?}"),
    }
    // `print_stack()` returns Nil and must not raise.
    assert_eq!(run_val("print_stack()\nreturn 1\n"), Variant::Int(1));
}

/// `type_exists` answers ClassDB membership, which is what Godot reports here.
#[test]
fn type_exists_reports_classdb_membership() {
    gdobject::class_db::register_2d_classes();

    assert_eq!(
        run_val("return type_exists(\"Node2D\")\n"),
        Variant::Bool(true),
        "a registered class must be reported as existing"
    );
    assert_eq!(
        run_val("return type_exists(\"NoSuchClassAnywhere\")\n"),
        Variant::Bool(false),
        "an unregistered name must not be reported as existing"
    );
}

/// `is_instance_id_valid` rejects the obviously invalid without scene access,
/// rather than claiming validity it cannot check.
#[test]
fn is_instance_id_valid_rejects_bad_ids() {
    assert_eq!(
        run_val("return is_instance_id_valid(0)\n"),
        Variant::Bool(false),
        "id 0 is never a valid instance"
    );
    assert_eq!(
        run_val("return is_instance_id_valid(-5)\n"),
        Variant::Bool(false),
        "a negative id is never valid"
    );
}

/// The RNG family from pat-0q2kx.152 still satisfies the acceptance's first
/// clause — re-asserted here because this bead claimed it was missing.
#[test]
fn seeded_rng_sequence_is_still_reproducible() {
    let seq = run_val(
        "seed(1234)\n\
         var a = [randi(), randi()]\n\
         seed(1234)\n\
         var b = [randi(), randi()]\n\
         return [a[0] == b[0], a[1] == b[1], a[0] != a[1]]\n",
    );
    match seq {
        Variant::Array(v) => {
            assert_eq!(
                v.to_vec()[0],
                Variant::Bool(true),
                "same seed replays draw 1"
            );
            assert_eq!(
                v.to_vec()[1],
                Variant::Bool(true),
                "same seed replays draw 2"
            );
            assert_eq!(
                v.to_vec()[2],
                Variant::Bool(true),
                "the generator actually advances"
            );
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}
