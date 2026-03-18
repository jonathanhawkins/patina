//! Bridge between native engine objects and their attached script instances.
//!
//! `ScriptBridge` is the central registry that maps object IDs to their
//! associated `ScriptInstance`. The engine calls through the bridge to
//! invoke script methods and access script properties.

use std::collections::HashMap;

use gdcore::id::ObjectId;
use gdvariant::Variant;

use crate::bindings::{MethodFlags, MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo};
use gdvariant::variant::VariantType;

/// Central registry mapping engine objects to their script instances.
#[derive(Default)]
pub struct ScriptBridge {
    scripts: HashMap<ObjectId, Box<dyn ScriptInstance>>,
}

impl ScriptBridge {
    /// Creates a new, empty script bridge.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attaches a script instance to the given object.
    ///
    /// If a script was already attached, it is replaced.
    pub fn attach_script(&mut self, object_id: ObjectId, instance: Box<dyn ScriptInstance>) {
        tracing::debug!("attaching script to object {object_id}");
        self.scripts.insert(object_id, instance);
    }

    /// Detaches the script from the given object, if any.
    pub fn detach_script(&mut self, object_id: ObjectId) {
        tracing::debug!("detaching script from object {object_id}");
        self.scripts.remove(&object_id);
    }

    /// Returns `true` if the given object has a script attached.
    pub fn has_script(&self, object_id: ObjectId) -> bool {
        self.scripts.contains_key(&object_id)
    }

    /// Calls a method on the script attached to the given object.
    pub fn call(
        &mut self,
        object_id: ObjectId,
        method: &str,
        args: &[Variant],
    ) -> Result<Variant, ScriptError> {
        let script = self
            .scripts
            .get_mut(&object_id)
            .ok_or(ScriptError::NoScript)?;
        script.call_method(method, args)
    }

    /// Gets a property from the script attached to the given object.
    pub fn get_property(&self, object_id: ObjectId, name: &str) -> Option<Variant> {
        self.scripts.get(&object_id)?.get_property(name)
    }

    /// Sets a property on the script attached to the given object.
    ///
    /// Returns `false` if the object has no script or the property doesn't exist.
    pub fn set_property(&mut self, object_id: ObjectId, name: &str, value: Variant) -> bool {
        match self.scripts.get_mut(&object_id) {
            Some(script) => script.set_property(name, value),
            None => false,
        }
    }
}

/// A method handler closure type for `NativeScript`.
type MethodHandler = Box<dyn FnMut(&[Variant]) -> Result<Variant, ScriptError>>;

/// A Rust-defined script instance for testing and Rust-native scripting.
///
/// Constructed via `NativeScript::builder(name)` using a builder pattern.
pub struct NativeScript {
    name: String,
    methods: HashMap<String, MethodHandler>,
    method_infos: Vec<MethodInfo>,
    properties: HashMap<String, Variant>,
    property_infos: Vec<ScriptPropertyInfo>,
}

impl NativeScript {
    /// Returns a builder for constructing a `NativeScript`.
    pub fn builder(name: impl Into<String>) -> NativeScriptBuilder {
        NativeScriptBuilder {
            name: name.into(),
            methods: HashMap::new(),
            method_infos: Vec::new(),
            properties: HashMap::new(),
            property_infos: Vec::new(),
        }
    }
}

impl ScriptInstance for NativeScript {
    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError> {
        let handler = self
            .methods
            .get_mut(name)
            .ok_or_else(|| ScriptError::MethodNotFound(name.to_owned()))?;
        handler(args)
    }

    fn get_property(&self, name: &str) -> Option<Variant> {
        self.properties.get(name).cloned()
    }

    fn set_property(&mut self, name: &str, value: Variant) -> bool {
        if self.properties.contains_key(name) {
            self.properties.insert(name.to_owned(), value);
            true
        } else {
            false
        }
    }

    fn list_methods(&self) -> Vec<MethodInfo> {
        self.method_infos.clone()
    }

    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        self.property_infos.clone()
    }

    fn get_script_name(&self) -> &str {
        &self.name
    }
}

/// Builder for `NativeScript`.
pub struct NativeScriptBuilder {
    name: String,
    methods: HashMap<String, MethodHandler>,
    method_infos: Vec<MethodInfo>,
    properties: HashMap<String, Variant>,
    property_infos: Vec<ScriptPropertyInfo>,
}

impl NativeScriptBuilder {
    /// Registers a method with the given name and handler closure.
    pub fn method(
        mut self,
        name: impl Into<String>,
        handler: impl FnMut(&[Variant]) -> Result<Variant, ScriptError> + 'static,
    ) -> Self {
        let name = name.into();
        self.method_infos.push(MethodInfo {
            name: name.clone(),
            argument_names: Vec::new(),
            return_type: VariantType::Nil,
            flags: MethodFlags::NORMAL,
        });
        self.methods.insert(name, Box::new(handler));
        self
    }

    /// Registers a property with the given name and default value.
    pub fn property(mut self, name: impl Into<String>, default: Variant) -> Self {
        let name = name.into();
        self.property_infos.push(ScriptPropertyInfo {
            name: name.clone(),
            property_type: default.variant_type(),
            default_value: default.clone(),
        });
        self.properties.insert(name, default);
        self
    }

    /// Builds the `NativeScript` instance.
    pub fn build(self) -> NativeScript {
        NativeScript {
            name: self.name,
            methods: self.methods,
            method_infos: self.method_infos,
            properties: self.properties,
            property_infos: self.property_infos,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn native_script_builder_create() {
        let script = NativeScript::builder("TestScript")
            .method("greet", |_args| Ok(Variant::String("hello".into())))
            .property("health", Variant::Int(100))
            .build();

        assert_eq!(script.get_script_name(), "TestScript");
    }

    #[test]
    fn native_script_call_method() {
        let mut script = NativeScript::builder("TestScript")
            .method("add", |args| {
                if let (Some(Variant::Int(a)), Some(Variant::Int(b))) = (args.first(), args.get(1))
                {
                    Ok(Variant::Int(a + b))
                } else {
                    Err(ScriptError::TypeError("expected two ints".into()))
                }
            })
            .build();

        let result = script
            .call_method("add", &[Variant::Int(3), Variant::Int(4)])
            .unwrap();
        assert_eq!(result, Variant::Int(7));
    }

    #[test]
    fn native_script_get_set_property() {
        let mut script = NativeScript::builder("TestScript")
            .property("health", Variant::Int(100))
            .build();

        assert_eq!(script.get_property("health"), Some(Variant::Int(100)));
        assert!(script.set_property("health", Variant::Int(50)));
        assert_eq!(script.get_property("health"), Some(Variant::Int(50)));
    }

    #[test]
    fn native_script_call_nonexistent_method() {
        let mut script = NativeScript::builder("TestScript").build();

        let err = script.call_method("missing", &[]).unwrap_err();
        assert!(matches!(err, ScriptError::MethodNotFound(ref name) if name == "missing"));
    }

    #[test]
    fn native_script_list_methods() {
        let script = NativeScript::builder("TestScript")
            .method("foo", |_| Ok(Variant::Nil))
            .method("bar", |_| Ok(Variant::Nil))
            .build();

        let methods = script.list_methods();
        assert_eq!(methods.len(), 2);
        let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar"));
    }

    #[test]
    fn native_script_list_properties() {
        let script = NativeScript::builder("TestScript")
            .property("x", Variant::Float(0.0))
            .property("y", Variant::Float(0.0))
            .build();

        let props = script.list_properties();
        assert_eq!(props.len(), 2);
        let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"x"));
        assert!(names.contains(&"y"));
    }

    #[test]
    fn bridge_attach_and_call() {
        let mut bridge = ScriptBridge::new();
        let obj_id = ObjectId::next();

        let script = NativeScript::builder("BridgeTest")
            .method("ping", |_| Ok(Variant::String("pong".into())))
            .build();

        bridge.attach_script(obj_id, Box::new(script));
        assert!(bridge.has_script(obj_id));

        let result = bridge.call(obj_id, "ping", &[]).unwrap();
        assert_eq!(result, Variant::String("pong".into()));
    }

    #[test]
    fn bridge_detach_removes_script() {
        let mut bridge = ScriptBridge::new();
        let obj_id = ObjectId::next();

        let script = NativeScript::builder("DetachTest").build();
        bridge.attach_script(obj_id, Box::new(script));
        assert!(bridge.has_script(obj_id));

        bridge.detach_script(obj_id);
        assert!(!bridge.has_script(obj_id));
    }

    #[test]
    fn bridge_call_on_non_scripted_object() {
        let mut bridge = ScriptBridge::new();
        let obj_id = ObjectId::next();

        let err = bridge.call(obj_id, "anything", &[]).unwrap_err();
        assert!(matches!(err, ScriptError::NoScript));
    }

    #[test]
    fn bridge_property_get_set() {
        let mut bridge = ScriptBridge::new();
        let obj_id = ObjectId::next();

        let script = NativeScript::builder("PropTest")
            .property("score", Variant::Int(0))
            .build();

        bridge.attach_script(obj_id, Box::new(script));

        assert_eq!(bridge.get_property(obj_id, "score"), Some(Variant::Int(0)));
        assert!(bridge.set_property(obj_id, "score", Variant::Int(42)));
        assert_eq!(bridge.get_property(obj_id, "score"), Some(Variant::Int(42)));
    }

    #[test]
    fn method_info_display_and_flags() {
        let info = MethodInfo {
            name: "update".into(),
            argument_names: vec!["delta".into()],
            return_type: VariantType::Nil,
            flags: MethodFlags::NORMAL | MethodFlags::VIRTUAL,
        };

        let display = format!("{info}");
        assert!(display.contains("update"));
        assert!(display.contains("delta"));
        assert!(display.contains("NORMAL"));
        assert!(display.contains("VIRTUAL"));
        assert!(info.flags.contains(MethodFlags::NORMAL));
        assert!(info.flags.contains(MethodFlags::VIRTUAL));
        assert!(!info.flags.contains(MethodFlags::CONST));
    }

    #[test]
    fn script_error_messages() {
        let e1 = ScriptError::MethodNotFound("foo".into());
        assert_eq!(e1.to_string(), "method not found: 'foo'");

        let e2 = ScriptError::InvalidArgCount {
            expected: 2,
            got: 3,
        };
        assert_eq!(e2.to_string(), "invalid argument count: expected 2, got 3");

        let e3 = ScriptError::PropertyNotFound("bar".into());
        assert_eq!(e3.to_string(), "property not found: 'bar'");

        let e4 = ScriptError::TypeError("expected int".into());
        assert_eq!(e4.to_string(), "type error: expected int");

        let e5 = ScriptError::NoScript;
        assert_eq!(e5.to_string(), "no script attached to object");
    }

    #[test]
    fn method_modifies_state_counter() {
        let counter = Rc::new(RefCell::new(0i64));
        let counter_clone = counter.clone();

        let mut script = NativeScript::builder("CounterScript")
            .method("increment", move |_args| {
                let mut c = counter_clone.borrow_mut();
                *c += 1;
                Ok(Variant::Int(*c))
            })
            .build();

        assert_eq!(
            script.call_method("increment", &[]).unwrap(),
            Variant::Int(1)
        );
        assert_eq!(
            script.call_method("increment", &[]).unwrap(),
            Variant::Int(2)
        );
        assert_eq!(
            script.call_method("increment", &[]).unwrap(),
            Variant::Int(3)
        );
        assert_eq!(*counter.borrow(), 3);
    }

    #[test]
    fn method_with_arguments() {
        let mut script = NativeScript::builder("ArgsScript")
            .method("multiply", |args| {
                if args.len() != 2 {
                    return Err(ScriptError::InvalidArgCount {
                        expected: 2,
                        got: args.len(),
                    });
                }
                match (&args[0], &args[1]) {
                    (Variant::Int(a), Variant::Int(b)) => Ok(Variant::Int(a * b)),
                    _ => Err(ScriptError::TypeError("expected two ints".into())),
                }
            })
            .build();

        let result = script
            .call_method("multiply", &[Variant::Int(6), Variant::Int(7)])
            .unwrap();
        assert_eq!(result, Variant::Int(42));
    }

    #[test]
    fn method_with_wrong_arg_count() {
        let mut script = NativeScript::builder("ArgsScript")
            .method("needs_two", |args| {
                if args.len() != 2 {
                    return Err(ScriptError::InvalidArgCount {
                        expected: 2,
                        got: args.len(),
                    });
                }
                Ok(Variant::Nil)
            })
            .build();

        let err = script
            .call_method("needs_two", &[Variant::Int(1)])
            .unwrap_err();
        match err {
            ScriptError::InvalidArgCount { expected, got } => {
                assert_eq!(expected, 2);
                assert_eq!(got, 1);
            }
            _ => panic!("expected InvalidArgCount"),
        }
    }

    #[test]
    fn get_nonexistent_property_returns_none() {
        let script = NativeScript::builder("TestScript").build();
        assert_eq!(script.get_property("missing"), None);
    }

    #[test]
    fn set_nonexistent_property_returns_false() {
        let mut script = NativeScript::builder("TestScript").build();
        assert!(!script.set_property("missing", Variant::Int(1)));
    }

    #[test]
    fn bridge_get_property_no_script_returns_none() {
        let bridge = ScriptBridge::new();
        let obj_id = ObjectId::next();
        assert_eq!(bridge.get_property(obj_id, "anything"), None);
    }

    #[test]
    fn bridge_set_property_no_script_returns_false() {
        let mut bridge = ScriptBridge::new();
        let obj_id = ObjectId::next();
        assert!(!bridge.set_property(obj_id, "anything", Variant::Nil));
    }
}
