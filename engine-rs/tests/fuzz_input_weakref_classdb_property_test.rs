//! Property-based tests for gdobject (ClassDB) and gdplatform (Input).
//!
//! Exercises ClassDB registration, inheritance, property/method metadata,
//! and InputState / InputMap consistency under randomized inputs.
//! Also covers edge cases: empty class names, very long names, special characters.

use std::sync::Mutex;

use proptest::prelude::*;

use gdobject::class_db::{
    class_count, class_exists, class_has_method, clear_for_testing, get_class_info,
    get_class_info_by_id, get_method_list, get_property_list, inheritance_chain, instantiate,
    is_parent_class, register_class, ClassRegistration, MethodInfo, PropertyInfo,
};
use gdobject::GodotObject;
use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, InputState, Key, MouseButton,
};
use gdvariant::Variant;

/// ClassDB is global state -- all ClassDB tests must hold this lock.
static CLASS_DB_LOCK: Mutex<()> = Mutex::new(());

fn setup_classdb() -> std::sync::MutexGuard<'static, ()> {
    let guard = CLASS_DB_LOCK.lock().expect("test lock poisoned");
    clear_for_testing();
    guard
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Valid class name: starts with a letter, 1..64 alphanumeric/underscore chars.
fn class_name_strategy() -> impl Strategy<Value = String> {
    "[A-Za-z][A-Za-z0-9_]{0,63}".prop_filter("non-empty", |s| !s.is_empty())
}

fn property_name_strategy() -> impl Strategy<Value = String> {
    "[a-z_][a-z0-9_]{0,31}"
}

fn method_name_strategy() -> impl Strategy<Value = String> {
    "[a-z_][a-z0-9_]{0,31}"
}

fn arb_key() -> impl Strategy<Value = Key> {
    prop_oneof![
        Just(Key::A),
        Just(Key::B),
        Just(Key::C),
        Just(Key::D),
        Just(Key::W),
        Just(Key::S),
        Just(Key::Space),
        Just(Key::Enter),
        Just(Key::Escape),
        Just(Key::Shift),
        Just(Key::Ctrl),
        Just(Key::Alt),
        Just(Key::Up),
        Just(Key::Down),
        Just(Key::Left),
        Just(Key::Right),
        Just(Key::F1),
        Just(Key::F12),
    ]
}

fn arb_mouse_button() -> impl Strategy<Value = MouseButton> {
    prop_oneof![
        Just(MouseButton::Left),
        Just(MouseButton::Right),
        Just(MouseButton::Middle),
        Just(MouseButton::WheelUp),
        Just(MouseButton::WheelDown),
    ]
}

fn arb_gamepad_button() -> impl Strategy<Value = GamepadButton> {
    prop_oneof![
        Just(GamepadButton::FaceA),
        Just(GamepadButton::FaceB),
        Just(GamepadButton::FaceX),
        Just(GamepadButton::FaceY),
        Just(GamepadButton::DPadUp),
        Just(GamepadButton::DPadDown),
        Just(GamepadButton::Start),
        Just(GamepadButton::Select),
    ]
}

/// Builds a linear hierarchy of `depth` classes (Base -> Level1 -> ... -> LevelN-1),
/// each level adding one property and one method.
fn setup_hierarchy(depth: usize) -> Vec<String> {
    clear_for_testing();

    let mut names = Vec::new();
    let base = "Base".to_string();
    register_class(ClassRegistration::new(&base));
    names.push(base);

    for i in 1..depth {
        let name = format!("Level{i}");
        let parent = &names[i - 1];
        register_class(
            ClassRegistration::new(&name)
                .parent(parent)
                .property(PropertyInfo::new(
                    format!("prop_{i}"),
                    Variant::Int(i as i64),
                ))
                .method(MethodInfo::new(format!("method_{i}"), i)),
        );
        names.push(name);
    }
    names
}

// ===========================================================================
// ClassDB property tests (14 proptest cases)
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // 1. Register + lookup roundtrip
    #[test]
    fn classdb_register_lookup_roundtrip(name in class_name_strategy()) {
        let _g = setup_classdb();
        let id = register_class(ClassRegistration::new(&name));
        prop_assert!(class_exists(&name));
        let info = get_class_info(&name).unwrap();
        prop_assert_eq!(info.class_name, name.clone());
        prop_assert_eq!(info.class_id, id);
    }

    // 2. Lookup by ClassId roundtrip
    #[test]
    fn classdb_lookup_by_id_roundtrip(name in class_name_strategy()) {
        let _g = setup_classdb();
        let id = register_class(ClassRegistration::new(&name));
        let info = get_class_info_by_id(id).unwrap();
        prop_assert_eq!(info.class_name, name);
    }

    // 3. Double register is idempotent (count stays 1)
    #[test]
    fn classdb_double_register_idempotent(name in class_name_strategy()) {
        let _g = setup_classdb();
        let _id1 = register_class(ClassRegistration::new(&name));
        let id2 = register_class(ClassRegistration::new(&name));
        prop_assert_eq!(class_count(), 1);
        let info = get_class_info(&name).unwrap();
        prop_assert_eq!(info.class_id, id2);
    }

    // 4. Two-level inheritance chain
    #[test]
    fn classdb_two_level_inheritance(
        parent_name in class_name_strategy(),
        child_suffix in "[A-Z][a-z]{1,8}",
    ) {
        let _g = setup_classdb();
        let child_name = format!("{}_{}", parent_name, child_suffix);
        register_class(ClassRegistration::new(&parent_name));
        register_class(ClassRegistration::new(&child_name).parent(&parent_name));

        let chain = inheritance_chain(&child_name);
        prop_assert_eq!(chain.len(), 2);
        prop_assert_eq!(&chain[0], &child_name);
        prop_assert_eq!(&chain[1], &parent_name);
        prop_assert!(is_parent_class(&child_name, &parent_name));
        prop_assert!(is_parent_class(&child_name, &child_name));
    }

    // 5. Deep inheritance chain transitivity
    #[test]
    fn classdb_deep_chain_transitivity(depth in 3usize..8) {
        let _g = setup_classdb();
        let names = setup_hierarchy(depth);
        let leaf = names.last().unwrap();
        for ancestor in &names {
            prop_assert!(is_parent_class(leaf, ancestor),
                "{} should inherit from {}", leaf, ancestor);
        }
        // Base must NOT inherit from leaf
        if depth > 1 {
            prop_assert!(!is_parent_class(&names[0], names.last().unwrap()));
        }
    }

    // 6. Property registration and retrieval
    #[test]
    fn classdb_property_registration(
        cname in class_name_strategy(),
        pname in property_name_strategy(),
        default_val in -1000i64..1000,
    ) {
        let _g = setup_classdb();
        register_class(
            ClassRegistration::new(&cname)
                .property(PropertyInfo::new(&pname, Variant::Int(default_val))),
        );
        let props = get_property_list(&cname);
        prop_assert_eq!(props.len(), 1);
        prop_assert_eq!(&props[0].name, &pname);
        if let Variant::Int(v) = &props[0].default_value {
            prop_assert_eq!(*v, default_val);
        } else {
            prop_assert!(false, "expected Int variant");
        }
    }

    // 7. Inherited properties accumulate (base-first order)
    #[test]
    fn classdb_inherited_properties(
        base_prop in property_name_strategy(),
        child_prop in property_name_strategy(),
    ) {
        let _g = setup_classdb();
        register_class(
            ClassRegistration::new("PropBase")
                .property(PropertyInfo::new(&base_prop, Variant::Bool(true))),
        );
        register_class(
            ClassRegistration::new("PropChild")
                .parent("PropBase")
                .property(PropertyInfo::new(&child_prop, Variant::Float(1.0))),
        );
        let props = get_property_list("PropChild");
        prop_assert!(props.len() >= 2 || base_prop == child_prop);
        prop_assert_eq!(&props[0].name, &base_prop);
    }

    // 8. Method registration and retrieval
    #[test]
    fn classdb_method_registration(
        cname in class_name_strategy(),
        mname in method_name_strategy(),
        argc in 0usize..10,
    ) {
        let _g = setup_classdb();
        register_class(
            ClassRegistration::new(&cname)
                .method(MethodInfo::new(&mname, argc)),
        );
        let methods = get_method_list(&cname);
        prop_assert_eq!(methods.len(), 1);
        prop_assert_eq!(&methods[0].name, &mname);
        prop_assert_eq!(methods[0].argument_count, argc);
    }

    // 9. Method metadata consistency (multiple methods)
    #[test]
    fn classdb_method_argc_consistent(
        cname in class_name_strategy(),
        argc1 in 0usize..5,
        argc2 in 0usize..5,
    ) {
        let _g = setup_classdb();
        register_class(
            ClassRegistration::new(&cname)
                .method(MethodInfo::new("alpha", argc1))
                .method(MethodInfo::new("beta", argc2)),
        );
        let methods = get_method_list(&cname);
        prop_assert_eq!(methods.len(), 2);
        let alpha = methods.iter().find(|m| m.name == "alpha").unwrap();
        let beta = methods.iter().find(|m| m.name == "beta").unwrap();
        prop_assert_eq!(alpha.argument_count, argc1);
        prop_assert_eq!(beta.argument_count, argc2);
    }

    // 10. class_has_method walks the inheritance chain
    #[test]
    fn classdb_has_method_inheritance(mname in method_name_strategy()) {
        let _g = setup_classdb();
        register_class(
            ClassRegistration::new("HMBase")
                .method(MethodInfo::new(&mname, 0)),
        );
        register_class(ClassRegistration::new("HMChild").parent("HMBase"));
        prop_assert!(class_has_method("HMChild", &mname));
        prop_assert!(class_has_method("HMBase", &mname));
    }

    // 11. Instantiate produces an object with correct defaults
    #[test]
    fn classdb_instantiate_defaults(
        pname in property_name_strategy(),
        val in -500i64..500,
    ) {
        let _g = setup_classdb();
        register_class(
            ClassRegistration::new("InstClass")
                .property(PropertyInfo::new(&pname, Variant::Int(val))),
        );
        let obj = instantiate("InstClass").unwrap();
        prop_assert_eq!(obj.get_class(), "InstClass");
        prop_assert_eq!(obj.get_property(&pname), Variant::Int(val));
    }

    // 12. Property accumulation grows with depth
    #[test]
    fn classdb_property_accumulation(depth in 2usize..8) {
        let _g = setup_classdb();
        let names = setup_hierarchy(depth);
        let mut prev_count = 0;
        for (i, name) in names.iter().enumerate() {
            let props = get_property_list(name);
            if i > 0 {
                prop_assert!(props.len() > prev_count);
            }
            prev_count = props.len();
        }
    }

    // 13. Unregistered class has empty chain
    #[test]
    fn classdb_unregistered_chain(name in class_name_strategy()) {
        let _g = setup_classdb();
        let chain = inheritance_chain(&name);
        prop_assert!(chain.is_empty());
    }

    // 14. PropertyInfo hint preserved
    #[test]
    fn classdb_property_hint_preserved(hint in 0i32..100) {
        let _g = setup_classdb();
        register_class(
            ClassRegistration::new("HintClass")
                .property(PropertyInfo::new("hp", Variant::Int(10)).with_hint(hint)),
        );
        let props = get_property_list("HintClass");
        prop_assert_eq!(props[0].hint, hint);
    }
}

// ===========================================================================
// Edge-case tests (3 regular tests)
// ===========================================================================

#[test]
fn classdb_empty_class_name() {
    let _g = setup_classdb();
    let id = register_class(ClassRegistration::new(""));
    assert!(class_exists(""));
    let info = get_class_info("").unwrap();
    assert_eq!(info.class_id, id);
}

#[test]
fn classdb_very_long_name() {
    let _g = setup_classdb();
    let long_name = "A".repeat(1024);
    let id = register_class(ClassRegistration::new(&long_name));
    assert!(class_exists(&long_name));
    let info = get_class_info(&long_name).unwrap();
    assert_eq!(info.class_id, id);
}

#[test]
fn classdb_special_characters_in_name() {
    let _g = setup_classdb();
    let names = [
        "Node<T>",
        "my::class",
        "hello world",
        "foo/bar",
        "emoji\u{1F600}",
    ];
    for name in &names {
        clear_for_testing();
        let id = register_class(ClassRegistration::new(*name));
        assert!(class_exists(name), "failed for name: {:?}", name);
        let info = get_class_info(name).unwrap();
        assert_eq!(info.class_id, id);
    }
}

// ===========================================================================
// Input system property tests (10 proptest cases)
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // 15. Key press/release cycle
    #[test]
    fn input_key_press_release(key in arb_key()) {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key, pressed: true, shift: false, ctrl: false, alt: false,
        });
        prop_assert!(state.is_key_pressed(key));
        prop_assert!(state.is_key_just_pressed(key));

        state.flush_frame();
        prop_assert!(state.is_key_pressed(key));
        prop_assert!(!state.is_key_just_pressed(key));

        state.process_event(InputEvent::Key {
            key, pressed: false, shift: false, ctrl: false, alt: false,
        });
        prop_assert!(!state.is_key_pressed(key));
        prop_assert!(state.is_key_just_released(key));
    }

    // 16. Key held transition: pressed -> held -> released
    #[test]
    fn input_key_held_transition(key in arb_key()) {
        let mut state = InputState::new();

        // Frame 1: press
        state.process_event(InputEvent::Key {
            key, pressed: true, shift: false, ctrl: false, alt: false,
        });
        prop_assert!(state.is_key_just_pressed(key));
        prop_assert!(state.is_key_pressed(key));

        // Frame 2: held (flush, no new events)
        state.flush_frame();
        prop_assert!(state.is_key_pressed(key));
        prop_assert!(!state.is_key_just_pressed(key));
        prop_assert!(!state.is_key_just_released(key));

        // Frame 3: release
        state.process_event(InputEvent::Key {
            key, pressed: false, shift: false, ctrl: false, alt: false,
        });
        prop_assert!(!state.is_key_pressed(key));
        prop_assert!(state.is_key_just_released(key));
    }

    // 17. Action binding and query via InputMap
    #[test]
    fn input_action_binding_query(key in arb_key()) {
        let mut map = InputMap::new();
        map.add_action("test_action", 0.0);
        map.action_add_event("test_action", ActionBinding::KeyBinding(key));

        let event = InputEvent::Key {
            key, pressed: true, shift: false, ctrl: false, alt: false,
        };
        prop_assert!(map.event_matches_action(&event, "test_action"));
        prop_assert!(!map.event_matches_action(&event, "nonexistent"));
    }

    // 18. InputMap add/get bindings consistency
    #[test]
    fn input_map_bindings_consistent(key in arb_key()) {
        let mut map = InputMap::new();
        map.add_action("act", 0.1);
        map.action_add_event("act", ActionBinding::KeyBinding(key));

        let bindings = map.get_bindings("act").unwrap();
        prop_assert_eq!(bindings.len(), 1);
        prop_assert_eq!(bindings[0].clone(), ActionBinding::KeyBinding(key));
        prop_assert!((map.get_deadzone("act") - 0.1).abs() < f32::EPSILON);
    }

    // 19. InputState with InputMap: bound key fires action
    #[test]
    fn input_state_action_from_key(key in arb_key()) {
        let mut map = InputMap::new();
        map.add_action("jump", 0.0);
        map.action_add_event("jump", ActionBinding::KeyBinding(key));

        let mut state = InputState::new();
        state.set_input_map(map);
        state.process_event(InputEvent::Key {
            key, pressed: true, shift: false, ctrl: false, alt: false,
        });
        prop_assert!(state.is_action_pressed("jump"));
        prop_assert!(state.is_action_just_pressed("jump"));

        state.flush_frame();
        prop_assert!(state.is_action_pressed("jump"));
        prop_assert!(!state.is_action_just_pressed("jump"));

        state.process_event(InputEvent::Key {
            key, pressed: false, shift: false, ctrl: false, alt: false,
        });
        prop_assert!(!state.is_action_pressed("jump"));
        prop_assert!(state.is_action_just_released("jump"));
    }

    // 20. Mouse button press/release tracking
    #[test]
    fn input_mouse_press_release(btn in arb_mouse_button()) {
        let mut state = InputState::new();
        state.process_event(InputEvent::MouseButton {
            button: btn, pressed: true, position: gdcore::math::Vector2::ZERO,
        });
        prop_assert!(state.is_mouse_button_pressed(btn));
        prop_assert!(state.is_mouse_button_just_pressed(btn));

        state.flush_frame();
        prop_assert!(state.is_mouse_button_pressed(btn));
        prop_assert!(!state.is_mouse_button_just_pressed(btn));

        state.process_event(InputEvent::MouseButton {
            button: btn, pressed: false, position: gdcore::math::Vector2::ZERO,
        });
        prop_assert!(!state.is_mouse_button_pressed(btn));
        prop_assert!(state.is_mouse_button_just_released(btn));
    }

    // 21. Gamepad button tracking
    #[test]
    fn input_gamepad_button_press(btn in arb_gamepad_button(), pad_id in 0u32..4) {
        let mut state = InputState::new();
        state.process_event(InputEvent::GamepadButton {
            button: btn, pressed: true, gamepad_id: pad_id,
        });
        prop_assert!(state.is_gamepad_button_pressed(pad_id, btn));

        state.process_event(InputEvent::GamepadButton {
            button: btn, pressed: false, gamepad_id: pad_id,
        });
        prop_assert!(!state.is_gamepad_button_pressed(pad_id, btn));
    }

    // 22. Gamepad axis values stored and retrieved
    #[test]
    fn input_gamepad_axis_value(val in -1.0f32..1.0) {
        let mut state = InputState::new();
        state.process_event(InputEvent::GamepadAxis {
            axis: GamepadAxis::LeftStickX, value: val, gamepad_id: 0,
        });
        let stored = state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX);
        prop_assert!((stored - val).abs() < f32::EPSILON);
    }

    // 23. Digital action strength is 1.0 pressed, 0.0 released
    #[test]
    fn input_action_strength_digital(key in arb_key()) {
        let mut map = InputMap::new();
        map.add_action("fire", 0.0);
        map.action_add_event("fire", ActionBinding::KeyBinding(key));

        let mut state = InputState::new();
        state.set_input_map(map);

        state.process_event(InputEvent::Key {
            key, pressed: true, shift: false, ctrl: false, alt: false,
        });
        prop_assert!((state.get_action_strength("fire") - 1.0).abs() < f32::EPSILON);

        state.process_event(InputEvent::Key {
            key, pressed: false, shift: false, ctrl: false, alt: false,
        });
        prop_assert!(state.get_action_strength("fire").abs() < f32::EPSILON);
    }

    // 24. Unbound action never reports pressed
    #[test]
    fn input_unbound_action_never_pressed(key in arb_key()) {
        let mut state = InputState::new();
        state.process_event(InputEvent::Key {
            key, pressed: true, shift: false, ctrl: false, alt: false,
        });
        prop_assert!(!state.is_action_pressed("unbound_action"));
    }
}
