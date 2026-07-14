//! pat-nzgdu.9 — Input singleton polling dispatched from GDScript.
//!
//! Before this bead the interpreter's `Input.*` handler knew only 6 methods;
//! `get_axis` / `get_action_strength` / `is_action_just_released` hit the
//! `UndefinedFunction` catch-all. This test drives a scripted class through the
//! real interpreter dispatch (`call_instance_method`, the path `_process`
//! takes), flipping a mock `SceneAccess`'s input state frame-by-frame, and
//! asserts the three new methods return Godot-correct values. The mock only
//! implements `is_input_action_pressed` / `is_input_action_just_released`; the
//! `get_axis` / `get_action_strength` results therefore exercise the trait's
//! default derivations (`strength(pos) - strength(neg)`), proving the whole
//! chain end-to-end.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gdscript_interop::bindings::SceneAccess;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

#[derive(Default)]
struct InputMockState {
    pressed: HashSet<String>,
    just_released: HashSet<String>,
}

struct InputMock {
    state: Rc<RefCell<InputMockState>>,
}

impl SceneAccess for InputMock {
    fn get_node(&self, _from: u64, _path: &str) -> Option<u64> {
        None
    }
    fn get_parent(&self, _node: u64) -> Option<u64> {
        None
    }
    fn get_children(&self, _node: u64) -> Vec<u64> {
        Vec::new()
    }
    fn get_node_property(&self, _node: u64, _prop: &str) -> Variant {
        Variant::Nil
    }
    fn set_node_property(&mut self, _node: u64, _prop: &str, _value: Variant) {}
    fn emit_signal(&mut self, _node: u64, _signal: &str, _args: &[Variant]) {}
    fn connect_signal(&mut self, _source: u64, _signal: &str, _target: u64, _method: &str) {}
    fn get_node_name(&self, _node: u64) -> Option<String> {
        None
    }
    fn is_input_action_pressed(&self, action: &str) -> bool {
        self.state.borrow().pressed.contains(action)
    }
    fn is_input_action_just_released(&self, action: &str) -> bool {
        self.state.borrow().just_released.contains(action)
    }
}

/// Poll the three new Input methods once, returning `[axis, strength, released]`.
const SCRIPT: &str = "\
extends Node
func poll() -> Array:
    return [Input.get_axis(\"ui_left\", \"ui_right\"), \
Input.get_action_strength(\"ui_right\"), \
Input.is_action_just_released(\"ui_accept\")]
";

fn num(v: &Variant) -> f64 {
    match v {
        Variant::Float(f) => *f,
        Variant::Int(i) => *i as f64,
        o => panic!("not a number: {o:?}"),
    }
}

fn poll(
    interp: &mut Interpreter,
    inst: &mut gdscript_interop::interpreter::ClassInstance,
) -> (f64, f64, bool) {
    let r = interp
        .call_instance_method(inst, "poll", &[])
        .expect("poll() dispatches");
    match r {
        Variant::Array(a) => (num(&a[0]), num(&a[1]), matches!(a[2], Variant::Bool(true))),
        o => panic!("poll() must return an Array, got {o:?}"),
    }
}

#[test]
fn input_singleton_polling_frames() {
    let state = Rc::new(RefCell::new(InputMockState::default()));
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SCRIPT).unwrap();
    let mut inst = interp.instantiate_class(&class_def).unwrap();
    interp.set_scene_access(Box::new(InputMock { state: state.clone() }), 1);

    // Frame 1: ui_right held.
    state.borrow_mut().pressed = ["ui_right".to_string()].into_iter().collect();
    let (axis, strength, released) = poll(&mut interp, &mut inst);
    assert_eq!(axis, 1.0, "get_axis == +1 while only ui_right is held");
    assert_eq!(strength, 1.0, "get_action_strength == 1.0 while held");
    assert!(!released);

    // Frame 2: both directions held → axis cancels to 0, strength still 1.
    state.borrow_mut().pressed = ["ui_left".to_string(), "ui_right".to_string()]
        .into_iter()
        .collect();
    let (axis, strength, _) = poll(&mut interp, &mut inst);
    assert_eq!(axis, 0.0, "opposing actions cancel to 0");
    assert_eq!(strength, 1.0);

    // Frame 3: only ui_left → axis == -1, ui_right strength 0.
    state.borrow_mut().pressed = ["ui_left".to_string()].into_iter().collect();
    let (axis, strength, _) = poll(&mut interp, &mut inst);
    assert_eq!(axis, -1.0, "get_axis == -1 while only ui_left is held");
    assert_eq!(strength, 0.0);

    // Frame 4: ui_accept released *this* frame → is_action_just_released true.
    {
        let mut s = state.borrow_mut();
        s.pressed.clear();
        s.just_released = ["ui_accept".to_string()].into_iter().collect();
    }
    let (_, _, released) = poll(&mut interp, &mut inst);
    assert!(released, "is_action_just_released true on the release frame");

    // Frame 5: released set cleared → false the next frame.
    state.borrow_mut().just_released.clear();
    let (_, _, released) = poll(&mut interp, &mut inst);
    assert!(!released, "is_action_just_released false the frame after release");
}
