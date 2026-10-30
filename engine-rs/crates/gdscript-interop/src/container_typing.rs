//! Typed containers from declarations (pat-s3rrv.265).
//!
//! [`pat-s3rrv.8`] made `Array` and `Dictionary` shared handles that *carry*
//! an element type and a read-only flag. Nothing ever set either, so
//! `is_typed()` was false for every container in the engine and a `const`
//! array was freely mutable. This module is the half that creates them, and it
//! lives outside `interpreter.rs` on purpose: that file is large and
//! frequently contended, so it gets only the call sites.
//!
//! ## What Godot actually does
//!
//! Assigning to a typed variable is **not** a tag applied to the value in
//! place. `gdscript_vm.cpp`'s `OPCODE_ASSIGN_TYPED_ARRAY` builds a *new* typed
//! array and copies the source into it, validating every element; a mismatch is
//! a runtime error. That distinction is forced on us anyway —
//! [`SharedArray::set_typed`] refuses a non-empty array ("type can only be set
//! when the array is empty"), exactly as `array.cpp` does — so
//! [`coerce_to_declared_type`] constructs and assigns rather than mutating.
//!
//! ⚠️ The consequence is visible to scripts and is Godot's behaviour, not an
//! artifact: `var a: Array[int] = b` where `b` is untyped binds a **new**
//! array, so later writes through `b` are not seen through `a`. An
//! already-correctly-typed source is passed through untouched, which preserves
//! the shared identity in the case where Godot preserves it too.
//!
//! ## Type hints arrive as strings, and that is sufficient
//!
//! `parser.rs`'s `parse_type_hint` returns `"Array[int]"` /
//! `"Dictionary[String, int]"` — it already preserves the bracket content, so
//! no parser change is needed; the element type is recovered from the string
//! here.

use gdvariant::{
    ContainerError, ContainerType, SharedArray, SharedDictionary, Variant, VariantType,
};

/// The container constraint a declaration's type hint asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum DeclaredContainer {
    /// `Array[T]`.
    Array(ContainerType),
    /// `Dictionary[K, V]`.
    Dictionary(ContainerType, ContainerType),
}

/// Reads a declaration's type hint as a container constraint.
///
/// Returns `None` for a hint that is not a parameterised container
/// (`"int"`, `"Node2D"`, a bare `"Array"`), which is the common case and must
/// stay free.
///
/// ⚠️ A bare `Array` is deliberately `None` rather than
/// `Array(ContainerType::untyped())`: Godot's `Array` hint constrains the
/// *variable*, not the value's elements, and returning a constraint here would
/// make `var a: Array = [1]` build a pointless new handle and break the
/// reference semantics pat-s3rrv.8 established.
pub fn declared_container(hint: &str) -> Option<DeclaredContainer> {
    let hint = hint.trim();
    let (base, params) = split_generic(hint)?;
    match base {
        "Array" => {
            let parts = split_top_level(params);
            // `Array[int, float]` is not valid GDScript; refusing it is better
            // than silently typing on the first parameter.
            if parts.len() != 1 {
                return None;
            }
            Some(DeclaredContainer::Array(container_type_of(parts[0])))
        }
        "Dictionary" => {
            let parts = split_top_level(params);
            if parts.len() != 2 {
                return None;
            }
            Some(DeclaredContainer::Dictionary(
                container_type_of(parts[0]),
                container_type_of(parts[1]),
            ))
        }
        _ => None,
    }
}

/// Converts `value` to satisfy `hint`, the way an assignment to a typed
/// variable does in Godot.
///
/// Anything that is not a container assignment is returned untouched, so this
/// is safe to call on every declaration.
pub fn coerce_to_declared_type(value: Variant, hint: &str) -> Result<Variant, ContainerError> {
    let Some(declared) = declared_container(hint) else {
        return Ok(value);
    };
    match (declared, value) {
        (DeclaredContainer::Array(want), Variant::Array(source)) => {
            if !want.is_typed() {
                return Ok(Variant::Array(source));
            }
            if source.is_typed() {
                // Already typed: either it matches and keeps its identity, or
                // it is the wrong typed array and Godot refuses the assignment
                // rather than silently re-typing a copy.
                return if same_array_type(&source, &want) {
                    Ok(Variant::Array(source))
                } else {
                    Err(ContainerError::TypeMismatch {
                        expected: name_of(&want),
                        got: array_type_name(&source),
                    })
                };
            }
            let typed = SharedArray::typed(want);
            // `assign` is what validates every element, and it reports the
            // first offender rather than truncating silently.
            typed.assign(&source)?;
            Ok(Variant::Array(typed))
        }
        (DeclaredContainer::Dictionary(key, val), Variant::Dictionary(source)) => {
            if !key.is_typed() && !val.is_typed() {
                return Ok(Variant::Dictionary(source));
            }
            if source.is_typed() {
                return if source.typed_key_builtin() == key.builtin
                    && source.typed_value_builtin() == val.builtin
                {
                    Ok(Variant::Dictionary(source))
                } else {
                    Err(ContainerError::TypeMismatch {
                        expected: format!("Dictionary[{}, {}]", name_of(&key), name_of(&val)),
                        got: "a differently typed Dictionary".to_string(),
                    })
                };
            }
            let typed = SharedDictionary::new();
            // Set the type while it is still empty — the only time `set_typed`
            // is allowed — then let `assign` validate every entry.
            typed.set_typed(key, val)?;
            typed.assign(&source)?;
            Ok(Variant::Dictionary(typed))
        }
        // A hint naming a container with a value that is not one: leave it
        // alone. Declaration-site type *checking* is a separate concern and
        // reporting it here would turn every `var a: Array[int]` with no
        // initialiser into an error.
        (_, other) => Ok(other),
    }
}

/// Freezes a `const`'s container value, so `const DIRS = [...]` cannot be
/// appended to (Godot's analyzer calls `make_read_only()` on every constant).
///
/// Shallow, matching `Array::make_read_only` — Godot freezes the constant
/// itself, not transitively through nested handles.
pub fn freeze_const_value(value: &Variant) {
    match value {
        Variant::Array(array) => array.make_read_only(),
        Variant::Dictionary(dict) => dict.make_read_only(),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Hint parsing
// ---------------------------------------------------------------------------

/// Splits `"Array[int]"` into `("Array", "int")`, or `None` when the hint is
/// not parameterised.
fn split_generic(hint: &str) -> Option<(&str, &str)> {
    let open = hint.find('[')?;
    let inner = hint.strip_suffix(']')?;
    Some((hint[..open].trim(), inner[open + 1..].trim()))
}

/// Splits a parameter list on commas that are not inside brackets, so
/// `Dictionary[String, Array[int]]` yields two parameters rather than three.
fn split_top_level(params: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in params.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(params[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(params[start..].trim());
    parts
}

/// The element constraint a type-hint parameter names.
///
/// Delegates to [`ContainerType::from_hint_name`] rather than carrying its own
/// table: the `.tscn` loader needs the identical mapping for
/// `Array[int]([1, 2])`, and two copies could disagree about what
/// `Array[AABB]` means with nothing to catch it.
fn container_type_of(param: &str) -> ContainerType {
    ContainerType::from_hint_name(param)
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Whether `source` already carries exactly the constraint `want` describes.
fn same_array_type(source: &SharedArray, want: &ContainerType) -> bool {
    source.typed_builtin() == want.builtin && source.typed_class_name() == want.class_name
}

/// How a constraint is named in an error message.
fn name_of(want: &ContainerType) -> String {
    match want.builtin {
        None => "Variant".to_string(),
        Some(VariantType::ObjectId) if !want.class_name.is_empty() => want.class_name.clone(),
        Some(builtin) => format!("{builtin:?}"),
    }
}

/// How an already-typed array's constraint is named in an error message.
fn array_type_name(source: &SharedArray) -> String {
    let class_name = source.typed_class_name();
    match source.typed_builtin() {
        None => "an untyped Array".to_string(),
        Some(VariantType::ObjectId) if !class_name.is_empty() => format!("Array[{class_name}]"),
        Some(builtin) => format!("Array[{builtin:?}]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_array_hint_yields_its_element_type() {
        assert_eq!(
            declared_container("Array[int]"),
            Some(DeclaredContainer::Array(ContainerType::builtin(
                VariantType::Int
            )))
        );
    }

    #[test]
    fn a_class_element_becomes_an_object_constraint() {
        let Some(DeclaredContainer::Array(want)) = declared_container("Array[Node2D]") else {
            panic!("Array[Node2D] must parse as an array constraint");
        };
        assert_eq!(want.builtin, Some(VariantType::ObjectId));
        assert_eq!(want.class_name, "Node2D");
    }

    #[test]
    fn a_dictionary_hint_splits_on_the_top_level_comma_only() {
        // ⚠️ The nested `Array[int]` contains no comma, but `Dictionary[String,
        // Array[int]]` must still split into exactly two parameters; a naive
        // `split(',')` on a nested `Dictionary[String, Dictionary[int, int]]`
        // would produce three.
        let Some(DeclaredContainer::Dictionary(key, val)) =
            declared_container("Dictionary[String, Array[int]]")
        else {
            panic!("a two-parameter Dictionary hint must parse");
        };
        assert_eq!(key.builtin, Some(VariantType::String));
        assert_eq!(
            val.builtin,
            Some(VariantType::Array),
            "a parameterised parameter constrains on its outer type"
        );
    }

    #[test]
    fn a_bare_container_hint_is_not_a_constraint() {
        // Otherwise `var a: Array = [1]` would rebind a fresh handle and
        // silently break reference semantics.
        assert_eq!(declared_container("Array"), None);
        assert_eq!(declared_container("Dictionary"), None);
        assert_eq!(declared_container("int"), None);
        assert_eq!(declared_container("Node2D"), None);
    }

    #[test]
    fn transcribed_spellings_that_a_derived_mapping_would_miss() {
        // `AABB`/`Aabb` and `RID`/`Rid` differ between GDScript and the variant
        // enum; derived mapping would make these object constraints.
        assert_eq!(
            ContainerType::from_hint_name("AABB").builtin,
            Some(VariantType::Aabb)
        );
        assert_eq!(
            ContainerType::from_hint_name("RID").builtin,
            Some(VariantType::Rid)
        );
        // `Aabb` is not a GDScript spelling, so it is a class name, not a
        // builtin — which is exactly the silent failure the table prevents.
        let misspelled = ContainerType::from_hint_name("Aabb");
        assert_eq!(misspelled.builtin, Some(VariantType::ObjectId));
        assert_eq!(misspelled.class_name, "Aabb");
    }

    #[test]
    fn coercing_an_untyped_literal_builds_a_typed_array() {
        let source = Variant::Array(SharedArray::from_vec(vec![
            Variant::Int(1),
            Variant::Int(2),
        ]));
        let coerced =
            coerce_to_declared_type(source, "Array[int]").expect("ints satisfy Array[int]");
        let Variant::Array(array) = coerced else {
            panic!("an Array stays an Array");
        };
        assert!(array.is_typed());
        assert_eq!(array.typed_builtin(), Some(VariantType::Int));
        assert_eq!(array.len(), 2, "the elements must be carried over");
    }

    #[test]
    fn coercing_rejects_an_element_that_does_not_fit() {
        let source = Variant::Array(SharedArray::from_vec(vec![Variant::String("x".into())]));
        let err = coerce_to_declared_type(source, "Array[int]")
            .expect_err("a String does not satisfy Array[int]");
        assert!(
            matches!(err, ContainerError::TypeMismatch { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn an_already_correctly_typed_array_keeps_its_identity() {
        // ⚠️ This is the clause that makes typed assignment safe for
        // pat-s3rrv.8's reference semantics: re-typing through a copy here
        // would silently detach the two handles.
        let typed = SharedArray::typed(ContainerType::builtin(VariantType::Int));
        let original = typed.clone();
        let coerced =
            coerce_to_declared_type(Variant::Array(typed), "Array[int]").expect("same type");
        let Variant::Array(array) = coerced else {
            panic!("an Array stays an Array");
        };
        assert!(
            array.is_same_instance(&original),
            "an already-correct typed array must not be rebuilt"
        );
    }

    #[test]
    fn a_differently_typed_array_is_refused_rather_than_retyped() {
        let typed = SharedArray::typed(ContainerType::builtin(VariantType::String));
        let err = coerce_to_declared_type(Variant::Array(typed), "Array[int]")
            .expect_err("Array[String] does not satisfy Array[int]");
        assert!(
            matches!(err, ContainerError::TypeMismatch { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn a_non_container_value_passes_through_untouched() {
        let coerced = coerce_to_declared_type(Variant::Int(7), "Array[int]").expect("passes");
        assert_eq!(coerced, Variant::Int(7));
    }

    #[test]
    fn freezing_a_const_container_refuses_later_writes() {
        let array = SharedArray::from_vec(vec![Variant::Int(1)]);
        freeze_const_value(&Variant::Array(array.clone()));
        assert!(array.is_read_only());
        assert_eq!(array.push(Variant::Int(2)), Err(ContainerError::ReadOnly));
    }

    #[test]
    fn coercing_an_untyped_dictionary_builds_a_typed_one() {
        let mut map = gdvariant::Dictionary::new();
        map.insert(Variant::String("a".into()), Variant::Int(1));
        let coerced = coerce_to_declared_type(
            Variant::Dictionary(SharedDictionary::from_dict(map)),
            "Dictionary[String, int]",
        )
        .expect("the entry satisfies the constraint");
        let Variant::Dictionary(dict) = coerced else {
            panic!("a Dictionary stays a Dictionary");
        };
        assert!(dict.is_typed());
        assert_eq!(dict.typed_key_builtin(), Some(VariantType::String));
        assert_eq!(dict.typed_value_builtin(), Some(VariantType::Int));
        assert_eq!(dict.len(), 1);
    }
}
