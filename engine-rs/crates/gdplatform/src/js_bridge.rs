//! Real `JavaScriptBridge` backend (pat-u3l8c.115).
//!
//! Godot's `JavaScriptBridge` singleton only does anything on the web export
//! (`platform/web/javascript_bridge.cpp`); every other platform links the
//! `JavaScriptBridgeDummy` that returns null/false. Patina matched the *dummy*
//! everywhere, including on wasm32 — [`crate::web::JsBridge`] records calls into
//! a `HashMap` and hands back `NotAvailable`/`Undefined`, so a web build could
//! not reach `localStorage`, `window`, or any other browser API.
//!
//! This module is the real thing, split so the script-facing surface never has
//! to know which half it is talking to:
//!
//! - [`JsBackend`] is the operation set the `JavaScriptBridge` singleton needs.
//! - On `wasm32` the default backend is [`WasmJsBackend`], which evaluates real
//!   JavaScript through `js-sys` and keeps live `wasm_bindgen::JsValue`s in a
//!   handle table so script code can call methods on them across ticks.
//! - Everywhere else the default is [`NullJsBackend`], which mirrors Godot's
//!   dummy: defined, inert return values rather than errors.
//! - [`set_backend`] installs a double, which is how the host tests drive the
//!   JS→script callback path without a browser.
//!
//! ## Callbacks are queued, not re-entrant
//!
//! Godot's `create_callback` hands JavaScript a function that calls straight
//! back into a `Callable`. Doing that literally here would re-enter the
//! interpreter from inside a `js-sys` closure, while the interpreter is already
//! borrowed further up the stack. So a registered callback instead pushes onto
//! [`queue_callback_invocation`], and the interpreter drains it at a safe point
//! (`Interpreter::pump_javascript_callbacks`). The observable contract for
//! script authors is unchanged apart from the callback running at the next pump
//! rather than synchronously inside the JS call.

use std::cell::RefCell;
use std::collections::HashMap;

pub use crate::web::{JsError, JsObjectHandle, JsValue};

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// The operations `JavaScriptBridge` needs from the host environment.
///
/// Every method returns a `Result` so the caller can distinguish "the browser
/// said no" (`JsError::Exception`) from "there is no browser"
/// (`JsError::NotAvailable`); the script surface flattens both to Godot's
/// `null`, but the runner dump and tests want the difference.
pub trait JsBackend {
    /// `JavaScriptBridge.eval(code, use_global_execution_context)`.
    fn eval(&mut self, code: &str, use_global_scope: bool) -> Result<JsValue, JsError>;

    /// `JavaScriptBridge.get_interface(name)` — a global object by name
    /// (`"localStorage"`, `"navigator"`, `"console"`, …).
    fn get_interface(&mut self, name: &str) -> Result<JsValue, JsError>;

    /// `JavaScriptBridge.create_object(class, ...args)` — `new <class>(...)`.
    fn create_object(&mut self, class: &str, args: &[JsValue]) -> Result<JsValue, JsError>;

    /// Calls a method on a previously returned object handle.
    fn object_call(
        &mut self,
        object: &JsObjectHandle,
        method: &str,
        args: &[JsValue],
    ) -> Result<JsValue, JsError>;

    /// Reads a property off an object handle.
    fn object_get(&mut self, object: &JsObjectHandle, property: &str) -> Result<JsValue, JsError>;

    /// Writes a property onto an object handle.
    fn object_set(
        &mut self,
        object: &JsObjectHandle,
        property: &str,
        value: JsValue,
    ) -> Result<(), JsError>;

    /// `JavaScriptBridge.create_callback(callable)` — returns a JS function
    /// object that, when invoked from JavaScript, queues `callback_id` and its
    /// arguments via [`queue_callback_invocation`].
    fn create_callback(&mut self, callback_id: u64) -> Result<JsValue, JsError>;

    /// `JavaScriptBridge.download_buffer(buffer, name, mime)`.
    fn download_buffer(&mut self, data: &[u8], name: &str, mime: &str) -> Result<(), JsError>;

    /// `JavaScriptBridge.is_js_buffer(object)`.
    fn is_buffer(&mut self, value: &JsValue) -> bool;

    /// `JavaScriptBridge.js_buffer_to_packed_byte_array(object)`. `None` when
    /// the value is not an ArrayBuffer/typed array, which the script surface
    /// turns into an empty `PackedByteArray`.
    fn buffer_to_bytes(&mut self, value: &JsValue) -> Option<Vec<u8>>;

    /// `JavaScriptBridge.pwa_needs_update()`.
    fn pwa_needs_update(&mut self) -> bool {
        false
    }

    /// `JavaScriptBridge.pwa_update()` — `OK` (0) or `ERR_UNAVAILABLE` (46).
    fn pwa_update(&mut self) -> Result<(), JsError> {
        Err(JsError::NotAvailable)
    }

    /// `JavaScriptBridge.force_fs_sync()` — flush the IDBFS write-back cache.
    fn force_fs_sync(&mut self) {}
}

// ---------------------------------------------------------------------------
// Null backend (Godot's JavaScriptBridgeDummy)
// ---------------------------------------------------------------------------

/// The non-web backend: every call resolves to a defined, inert value, exactly
/// as Godot's `JavaScriptBridgeDummy` does off the web export. Nothing here
/// errors with `Exception` — there is no JS engine to raise one.
#[derive(Debug, Clone, Default)]
pub struct NullJsBackend;

impl JsBackend for NullJsBackend {
    fn eval(&mut self, _code: &str, _use_global_scope: bool) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }

    fn get_interface(&mut self, _name: &str) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }

    fn create_object(&mut self, _class: &str, _args: &[JsValue]) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }

    fn object_call(
        &mut self,
        _object: &JsObjectHandle,
        _method: &str,
        _args: &[JsValue],
    ) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }

    fn object_get(
        &mut self,
        _object: &JsObjectHandle,
        _property: &str,
    ) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }

    fn object_set(
        &mut self,
        _object: &JsObjectHandle,
        _property: &str,
        _value: JsValue,
    ) -> Result<(), JsError> {
        Ok(())
    }

    fn create_callback(&mut self, _callback_id: u64) -> Result<JsValue, JsError> {
        Ok(JsValue::Null)
    }

    fn download_buffer(&mut self, _data: &[u8], _name: &str, _mime: &str) -> Result<(), JsError> {
        Ok(())
    }

    fn is_buffer(&mut self, _value: &JsValue) -> bool {
        false
    }

    fn buffer_to_bytes(&mut self, _value: &JsValue) -> Option<Vec<u8>> {
        None
    }
}

// ---------------------------------------------------------------------------
// Recording backend (host tests)
// ---------------------------------------------------------------------------

/// A backend that answers from a scripted table and records what it was asked.
///
/// This is how the JS half is doubled in host tests: it hands out real object
/// handles, so the `get_interface(...)` → `object_call(...)` chain that only a
/// browser could otherwise exercise runs on the test runner.
#[derive(Debug, Default)]
pub struct RecordingJsBackend {
    /// Every call in order, as `"eval:1+1"` / `"get_interface:localStorage"` /
    /// `"call:1.setItem"` / `"create_callback:7"` …
    pub log: Vec<String>,
    /// Answers for `eval(code)`, keyed by the code string.
    pub eval_results: HashMap<String, JsValue>,
    /// Answers for `object_call`, keyed by `"<handle id>.<method>"`.
    pub call_results: HashMap<String, JsValue>,
    /// Property values, keyed by `"<handle id>.<property>"`.
    pub properties: HashMap<String, JsValue>,
    /// Global names that resolve, mapped to the type name of the handle handed
    /// back. An unlisted name resolves to `JsValue::Null`, as in a browser.
    pub interfaces: HashMap<String, String>,
    /// Handles minted by `create_callback`, in registration order.
    pub callbacks: Vec<u64>,
    next_handle: u64,
}

impl RecordingJsBackend {
    pub fn new() -> Self {
        Self {
            next_handle: 1,
            ..Default::default()
        }
    }

    /// Makes `get_interface(name)` resolve to a fresh object handle.
    pub fn with_interface(mut self, name: &str, type_name: &str) -> Self {
        self.interfaces
            .insert(name.to_string(), type_name.to_string());
        self
    }

    /// Makes `eval(code)` return `value`.
    pub fn with_eval(mut self, code: &str, value: JsValue) -> Self {
        self.eval_results.insert(code.to_string(), value);
        self
    }

    fn mint(&mut self, type_name: &str) -> JsObjectHandle {
        let id = self.next_handle;
        self.next_handle += 1;
        JsObjectHandle::new(id, type_name)
    }

    /// Appends to both this backend's own `log` and the module-level
    /// [`recorded_calls`] mirror.
    ///
    /// The mirror exists because [`with_backend`] hands out `&mut dyn
    /// JsBackend`: once installed, a caller cannot get the concrete
    /// `RecordingJsBackend` back to read `log` without downcasting. A test that
    /// drives the bridge *through the interpreter* — which is the only way to
    /// prove a script-registered callback ran — has no other observation point.
    fn record(&mut self, entry: String) {
        RECORDED.with(|r| r.borrow_mut().push(entry.clone()));
        self.log.push(entry);
    }
}

impl JsBackend for RecordingJsBackend {
    fn eval(&mut self, code: &str, use_global_scope: bool) -> Result<JsValue, JsError> {
        self.record(format!("eval:{code}:{use_global_scope}"));
        Ok(self
            .eval_results
            .get(code)
            .cloned()
            .unwrap_or(JsValue::Null))
    }

    fn get_interface(&mut self, name: &str) -> Result<JsValue, JsError> {
        self.record(format!("get_interface:{name}"));
        match self.interfaces.get(name).cloned() {
            Some(type_name) => Ok(JsValue::Object(self.mint(&type_name))),
            None => Ok(JsValue::Null),
        }
    }

    fn create_object(&mut self, class: &str, args: &[JsValue]) -> Result<JsValue, JsError> {
        self.record(format!("create_object:{class}:{}", args.len()));
        Ok(JsValue::Object(self.mint(class)))
    }

    fn object_call(
        &mut self,
        object: &JsObjectHandle,
        method: &str,
        args: &[JsValue],
    ) -> Result<JsValue, JsError> {
        self.record(format!("call:{}.{}:{}", object.id, method, args.len()));
        Ok(self
            .call_results
            .get(&format!("{}.{}", object.id, method))
            .cloned()
            .unwrap_or(JsValue::Undefined))
    }

    fn object_get(&mut self, object: &JsObjectHandle, property: &str) -> Result<JsValue, JsError> {
        self.record(format!("get:{}.{}", object.id, property));
        Ok(self
            .properties
            .get(&format!("{}.{}", object.id, property))
            .cloned()
            .unwrap_or(JsValue::Undefined))
    }

    fn object_set(
        &mut self,
        object: &JsObjectHandle,
        property: &str,
        value: JsValue,
    ) -> Result<(), JsError> {
        self.record(format!("set:{}.{}", object.id, property));
        self.properties
            .insert(format!("{}.{}", object.id, property), value);
        Ok(())
    }

    fn create_callback(&mut self, callback_id: u64) -> Result<JsValue, JsError> {
        self.record(format!("create_callback:{callback_id}"));
        self.callbacks.push(callback_id);
        Ok(JsValue::Object(self.mint("Function")))
    }

    fn download_buffer(&mut self, data: &[u8], name: &str, mime: &str) -> Result<(), JsError> {
        self.record(format!("download:{name}:{mime}:{}", data.len()));
        Ok(())
    }

    fn is_buffer(&mut self, value: &JsValue) -> bool {
        matches!(value, JsValue::ArrayBuffer(_))
    }

    fn buffer_to_bytes(&mut self, value: &JsValue) -> Option<Vec<u8>> {
        match value {
            JsValue::ArrayBuffer(bytes) => Some(bytes.clone()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Installed backend
// ---------------------------------------------------------------------------

thread_local! {
    /// The backend the `JavaScriptBridge` singleton talks to. `None` until the
    /// first access, which installs the per-target default.
    static BACKEND: RefCell<Option<Box<dyn JsBackend>>> = const { RefCell::new(None) };

    /// Callback invocations JavaScript has requested but the interpreter has
    /// not drained yet. See the module docs for why this is not re-entrant.
    static PENDING: RefCell<Vec<(u64, Vec<JsValue>)>> = const { RefCell::new(Vec::new()) };

    /// Mirror of [`RecordingJsBackend::log`], readable without holding the
    /// backend. See `RecordingJsBackend::record`.
    static RECORDED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Every call a [`RecordingJsBackend`] has served since the last
/// [`reset_backend`], in order.
///
/// This is the observation point for tests that drive the bridge through the
/// interpreter rather than calling the backend directly — notably the one that
/// proves a script `Callable` registered via `create_callback` really ran.
pub fn recorded_calls() -> Vec<String> {
    RECORDED.with(|r| r.borrow().clone())
}

/// The backend to use when nothing has been installed: the browser one on
/// wasm32, Godot's dummy semantics everywhere else.
fn default_backend() -> Box<dyn JsBackend> {
    // `return` in the cfg'd arm, not a trailing block: with two `#[cfg]` blocks
    // the first one is a *statement* on the target that keeps it, which only
    // type-checks if it evaluates to `()`. Same shape as `os.rs::shell_open`.
    #[cfg(target_arch = "wasm32")]
    {
        return Box::new(wasm::WasmJsBackend::new());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Box::new(NullJsBackend)
    }
}

/// Installs `backend`, replacing whatever was there. Tests use this to double
/// the browser; the web runtime may use it to inject a preconfigured one.
pub fn set_backend(backend: Box<dyn JsBackend>) {
    BACKEND.with(|b| *b.borrow_mut() = Some(backend));
}

/// Drops the installed backend and every queued callback invocation, so the
/// next access re-installs the per-target default. Tests must call this to stay
/// isolated — the backend is thread-local, not per-test.
pub fn reset_backend() {
    BACKEND.with(|b| *b.borrow_mut() = None);
    PENDING.with(|p| p.borrow_mut().clear());
    RECORDED.with(|r| r.borrow_mut().clear());
}

/// Runs `f` against the installed backend, installing the default first if
/// needed.
///
/// The backend is *moved out* of the thread-local for the duration of the call
/// and put back afterwards. That is what makes a backend safe to call
/// re-entrantly: a `RefCell` borrow held across `f` would panic the moment a
/// backend implementation touched the bridge again (which the wasm closure path
/// does whenever JS calls back into us).
pub fn with_backend<R>(f: impl FnOnce(&mut dyn JsBackend) -> R) -> R {
    let mut backend = BACKEND
        .with(|b| b.borrow_mut().take())
        .unwrap_or_else(default_backend);
    let result = f(backend.as_mut());
    BACKEND.with(|b| {
        // A backend installed *during* `f` wins: it is the more recent intent.
        let mut slot = b.borrow_mut();
        if slot.is_none() {
            *slot = Some(backend);
        }
    });
    result
}

// ---------------------------------------------------------------------------
// Callback queue
// ---------------------------------------------------------------------------

/// Records that JavaScript invoked the function created for `callback_id`.
///
/// Called from the `js-sys` closure on wasm, and directly from tests to stand
/// in for the browser.
pub fn queue_callback_invocation(callback_id: u64, args: Vec<JsValue>) {
    PENDING.with(|p| p.borrow_mut().push((callback_id, args)));
}

/// Drains every queued invocation in arrival order.
///
/// The interpreter calls this from `pump_javascript_callbacks` and dispatches
/// each one onto the `Callable` registered under that id.
pub fn take_callback_invocations() -> Vec<(u64, Vec<JsValue>)> {
    PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()))
}

/// Number of invocations waiting to be drained (diagnostics / tests).
pub fn pending_callback_count() -> usize {
    PENDING.with(|p| p.borrow().len())
}

// ---------------------------------------------------------------------------
// Browser backend (wasm32 only)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::{queue_callback_invocation, JsBackend, JsError, JsObjectHandle, JsValue};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::{JsCast, JsValue as RawJsValue};

    thread_local! {
        /// Live JS values behind the handles handed to script code. A handle
        /// outlives the call that produced it, so the value has to be owned
        /// here rather than passed back and forth across the boundary.
        static OBJECTS: RefCell<HashMap<u64, RawJsValue>> = RefCell::new(HashMap::new());
        /// Closures backing `create_callback`. Kept alive for the lifetime of
        /// the module: JS holds the function, so dropping the `Closure` would
        /// leave it dangling.
        static CLOSURES: RefCell<Vec<Closure<dyn FnMut(RawJsValue)>>> = RefCell::new(Vec::new());
    }

    fn js_error(e: RawJsValue) -> JsError {
        JsError::Exception(
            e.as_string()
                .or_else(|| js_sys::JSON::stringify(&e).ok().map(String::from))
                .unwrap_or_else(|| "unknown JavaScript error".to_string()),
        )
    }

    /// `target[property]`, as a plain reflective read.
    fn get(target: &RawJsValue, property: &str) -> Result<RawJsValue, JsError> {
        js_sys::Reflect::get(target, &RawJsValue::from_str(property)).map_err(js_error)
    }

    /// `target.method(...args)`, with `target` as the receiver.
    fn call_method(
        target: &RawJsValue,
        method: &str,
        args: &[RawJsValue],
    ) -> Result<RawJsValue, JsError> {
        let f: js_sys::Function = get(target, method)?
            .dyn_into()
            .map_err(|_| JsError::PropertyNotFound(method.to_string()))?;
        let arr = js_sys::Array::new();
        for a in args {
            arr.push(a);
        }
        js_sys::Reflect::apply(&f, target, &arr).map_err(js_error)
    }

    /// `new scope[class](...args)`.
    fn construct(
        scope: &RawJsValue,
        class: &str,
        args: &js_sys::Array,
    ) -> Result<RawJsValue, JsError> {
        let ctor: js_sys::Function = get(scope, class)?
            .dyn_into()
            .map_err(|_| JsError::PropertyNotFound(class.to_string()))?;
        js_sys::Reflect::construct(&ctor, args).map_err(js_error)
    }

    /// The browser-backed `JavaScriptBridge`.
    pub struct WasmJsBackend {
        next_handle: u64,
    }

    impl WasmJsBackend {
        pub fn new() -> Self {
            Self { next_handle: 1 }
        }

        /// Stores `raw` and returns the handle script code will see.
        fn store(&mut self, raw: RawJsValue, type_name: &str) -> JsObjectHandle {
            let id = self.next_handle;
            self.next_handle += 1;
            OBJECTS.with(|o| o.borrow_mut().insert(id, raw));
            JsObjectHandle::new(id, type_name)
        }

        fn lookup(object: &JsObjectHandle) -> Result<RawJsValue, JsError> {
            OBJECTS
                .with(|o| o.borrow().get(&object.id).cloned())
                .ok_or_else(|| JsError::PropertyNotFound(format!("JsObject({})", object.id)))
        }

        /// Converts a browser value into the engine's `JsValue`, minting a
        /// handle for anything that is not a primitive.
        fn from_raw(&mut self, raw: RawJsValue) -> JsValue {
            if raw.is_undefined() {
                return JsValue::Undefined;
            }
            if raw.is_null() {
                return JsValue::Null;
            }
            if let Some(b) = raw.as_bool() {
                return JsValue::Bool(b);
            }
            if let Some(n) = raw.as_f64() {
                return JsValue::Number(n);
            }
            if let Some(s) = raw.as_string() {
                return JsValue::String(s);
            }
            // `ArrayBuffer.isView` covers every typed array and `DataView`.
            // Reading them as `Uint8Array` gives element-wise bytes, which is
            // what `js_buffer_to_packed_byte_array` promises.
            if raw.is_instance_of::<js_sys::ArrayBuffer>() || js_sys::ArrayBuffer::is_view(&raw) {
                let bytes = js_sys::Uint8Array::new(&raw).to_vec();
                return JsValue::ArrayBuffer(bytes);
            }
            let type_name = raw
                .js_typeof()
                .as_string()
                .unwrap_or_else(|| "object".to_string());
            JsValue::Object(self.store(raw, &type_name))
        }

        /// Converts an engine value into a browser one. An `Object` resolves
        /// through the handle table; a stale handle becomes `undefined`, which
        /// is what JS would see for a collected reference.
        fn to_raw(value: &JsValue) -> RawJsValue {
            match value {
                JsValue::Undefined => RawJsValue::UNDEFINED,
                JsValue::Null => RawJsValue::NULL,
                JsValue::Bool(b) => RawJsValue::from_bool(*b),
                JsValue::Number(n) => RawJsValue::from_f64(*n),
                JsValue::String(s) => RawJsValue::from_str(s),
                JsValue::Object(h) => OBJECTS
                    .with(|o| o.borrow().get(&h.id).cloned())
                    .unwrap_or(RawJsValue::UNDEFINED),
                JsValue::ArrayBuffer(bytes) => {
                    js_sys::Uint8Array::from(bytes.as_slice()).buffer().into()
                }
            }
        }

        fn args_to_array(args: &[JsValue]) -> js_sys::Array {
            let arr = js_sys::Array::new();
            for a in args {
                arr.push(&Self::to_raw(a));
            }
            arr
        }
    }

    impl JsBackend for WasmJsBackend {
        fn eval(&mut self, code: &str, use_global_scope: bool) -> Result<JsValue, JsError> {
            // Godot: `use_global_execution_context` runs the code at global
            // scope (`eval` on `window`); otherwise it runs in the engine's own
            // scope, which here means inside a fresh function body so `var`
            // declarations do not leak onto `globalThis`.
            let raw = if use_global_scope {
                js_sys::eval(code).map_err(js_error)?
            } else {
                let f = js_sys::Function::new_no_args(&format!("return ({code});"));
                f.call0(&RawJsValue::UNDEFINED).map_err(js_error)?
            };
            Ok(self.from_raw(raw))
        }

        fn get_interface(&mut self, name: &str) -> Result<JsValue, JsError> {
            let global = js_sys::global();
            let raw =
                js_sys::Reflect::get(&global, &RawJsValue::from_str(name)).map_err(js_error)?;
            Ok(self.from_raw(raw))
        }

        fn create_object(&mut self, class: &str, args: &[JsValue]) -> Result<JsValue, JsError> {
            let global = js_sys::global();
            let ctor =
                js_sys::Reflect::get(&global, &RawJsValue::from_str(class)).map_err(js_error)?;
            let ctor: js_sys::Function = ctor
                .dyn_into()
                .map_err(|_| JsError::PropertyNotFound(class.to_string()))?;
            let raw =
                js_sys::Reflect::construct(&ctor, &Self::args_to_array(args)).map_err(js_error)?;
            Ok(self.from_raw(raw))
        }

        fn object_call(
            &mut self,
            object: &JsObjectHandle,
            method: &str,
            args: &[JsValue],
        ) -> Result<JsValue, JsError> {
            let target = Self::lookup(object)?;
            let f =
                js_sys::Reflect::get(&target, &RawJsValue::from_str(method)).map_err(js_error)?;
            let f: js_sys::Function = f
                .dyn_into()
                .map_err(|_| JsError::PropertyNotFound(method.to_string()))?;
            let raw = js_sys::Reflect::apply(&f, &target, &Self::args_to_array(args))
                .map_err(js_error)?;
            Ok(self.from_raw(raw))
        }

        fn object_get(
            &mut self,
            object: &JsObjectHandle,
            property: &str,
        ) -> Result<JsValue, JsError> {
            let target = Self::lookup(object)?;
            let raw =
                js_sys::Reflect::get(&target, &RawJsValue::from_str(property)).map_err(js_error)?;
            Ok(self.from_raw(raw))
        }

        fn object_set(
            &mut self,
            object: &JsObjectHandle,
            property: &str,
            value: JsValue,
        ) -> Result<(), JsError> {
            let target = Self::lookup(object)?;
            js_sys::Reflect::set(
                &target,
                &RawJsValue::from_str(property),
                &Self::to_raw(&value),
            )
            .map_err(js_error)?;
            Ok(())
        }

        fn create_callback(&mut self, callback_id: u64) -> Result<JsValue, JsError> {
            // The closure only *queues*; the interpreter runs the Callable at
            // the next pump. See the module docs for why.
            let closure = Closure::wrap(Box::new(move |arg: RawJsValue| {
                let mut args = Vec::new();
                if !arg.is_undefined() {
                    if arg.is_instance_of::<js_sys::Array>() {
                        let arr = js_sys::Array::from(&arg);
                        for item in arr.iter() {
                            args.push(raw_to_plain(item));
                        }
                    } else {
                        args.push(raw_to_plain(arg));
                    }
                }
                queue_callback_invocation(callback_id, args);
            }) as Box<dyn FnMut(RawJsValue)>);
            let function: RawJsValue = closure.as_ref().clone();
            CLOSURES.with(|c| c.borrow_mut().push(closure));
            Ok(self.from_raw(function))
        }

        fn download_buffer(&mut self, data: &[u8], name: &str, mime: &str) -> Result<(), JsError> {
            // Reached entirely through `js_sys::Reflect` rather than `web-sys`
            // typed bindings. `web-sys` gates each DOM type behind a feature
            // and has moved the `BlobPropertyBag` API across 0.3.x releases, so
            // a typed binding here would couple gdplatform's build to whichever
            // `web-sys` version the rest of the tree resolves.
            let global = js_sys::global();

            // new Blob([bytes], { type: mime })
            let parts = js_sys::Array::new();
            parts.push(&js_sys::Uint8Array::from(data).into());
            let options = js_sys::Object::new();
            js_sys::Reflect::set(
                &options,
                &RawJsValue::from_str("type"),
                &RawJsValue::from_str(mime),
            )
            .map_err(js_error)?;
            let blob_args = js_sys::Array::new();
            blob_args.push(&parts);
            blob_args.push(&options);
            let blob = construct(&global, "Blob", &blob_args)?;

            // URL.createObjectURL(blob)
            let url_ns = get(&global, "URL")?;
            let url = call_method(&url_ns, "createObjectURL", &[blob])?;

            // document.createElement("a") -> href/download -> click()
            let document = get(&global, "document")?;
            let anchor = call_method(&document, "createElement", &[RawJsValue::from_str("a")])?;
            js_sys::Reflect::set(&anchor, &RawJsValue::from_str("href"), &url).map_err(js_error)?;
            js_sys::Reflect::set(
                &anchor,
                &RawJsValue::from_str("download"),
                &RawJsValue::from_str(name),
            )
            .map_err(js_error)?;
            call_method(&anchor, "click", &[])?;

            // Revoking immediately is safe: the click has already handed the
            // blob to the browser's download machinery.
            let _ = call_method(&url_ns, "revokeObjectURL", &[url]);
            Ok(())
        }

        fn is_buffer(&mut self, value: &JsValue) -> bool {
            match value {
                JsValue::ArrayBuffer(_) => true,
                JsValue::Object(h) => match Self::lookup(h) {
                    Ok(raw) => {
                        raw.is_instance_of::<js_sys::ArrayBuffer>()
                            || js_sys::ArrayBuffer::is_view(&raw)
                    }
                    Err(_) => false,
                },
                _ => false,
            }
        }

        fn buffer_to_bytes(&mut self, value: &JsValue) -> Option<Vec<u8>> {
            match value {
                JsValue::ArrayBuffer(bytes) => Some(bytes.clone()),
                JsValue::Object(h) => {
                    let raw = Self::lookup(h).ok()?;
                    if raw.is_instance_of::<js_sys::ArrayBuffer>()
                        || js_sys::ArrayBuffer::is_view(&raw)
                    {
                        Some(js_sys::Uint8Array::new(&raw).to_vec())
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }

        fn pwa_needs_update(&mut self) -> bool {
            // The service worker sets this flag on the shell; absent shell
            // support the answer is "no update", as in Godot.
            js_sys::Reflect::get(
                &js_sys::global(),
                &RawJsValue::from_str("__patina_pwa_update_pending"),
            )
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        }

        fn pwa_update(&mut self) -> Result<(), JsError> {
            let global = js_sys::global();
            let f = js_sys::Reflect::get(&global, &RawJsValue::from_str("__patina_pwa_update"))
                .map_err(js_error)?;
            let f: js_sys::Function = f.dyn_into().map_err(|_| JsError::NotAvailable)?;
            f.call0(&RawJsValue::UNDEFINED).map_err(js_error)?;
            Ok(())
        }

        fn force_fs_sync(&mut self) {
            let global = js_sys::global();
            if let Ok(f) = js_sys::Reflect::get(&global, &RawJsValue::from_str("__patina_fs_sync"))
            {
                if let Ok(f) = f.dyn_into::<js_sys::Function>() {
                    let _ = f.call0(&RawJsValue::UNDEFINED);
                }
            }
        }
    }

    /// Converts a callback argument without minting a handle.
    ///
    /// A callback fires from inside a JS closure, where the backend is checked
    /// out of its thread-local slot, so there is no `&mut self` to store a
    /// handle in. Non-primitive arguments therefore arrive as their JSON text
    /// rather than as an object handle — enough for the event payloads
    /// callbacks actually carry, and it never drops the invocation.
    fn raw_to_plain(raw: RawJsValue) -> JsValue {
        if raw.is_undefined() {
            JsValue::Undefined
        } else if raw.is_null() {
            JsValue::Null
        } else if let Some(b) = raw.as_bool() {
            JsValue::Bool(b)
        } else if let Some(n) = raw.as_f64() {
            JsValue::Number(n)
        } else if let Some(s) = raw.as_string() {
            JsValue::String(s)
        } else {
            match js_sys::JSON::stringify(&raw) {
                Ok(s) => JsValue::String(String::from(s)),
                Err(_) => JsValue::Undefined,
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm::WasmJsBackend;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test shares one thread-local backend, so each has to start clean.
    fn install(backend: RecordingJsBackend) {
        reset_backend();
        set_backend(Box::new(backend));
    }

    #[test]
    fn default_backend_off_web_matches_godots_dummy() {
        reset_backend();
        // Godot's JavaScriptBridgeDummy: null, never an error.
        assert_eq!(with_backend(|b| b.eval("1 + 1", false)), Ok(JsValue::Null));
        assert_eq!(
            with_backend(|b| b.get_interface("localStorage")),
            Ok(JsValue::Null)
        );
        assert!(!with_backend(|b| b.pwa_needs_update()));
        reset_backend();
    }

    #[test]
    fn eval_returns_the_backends_value_and_records_the_scope_flag() {
        install(RecordingJsBackend::new().with_eval("1 + 1", JsValue::Number(2.0)));
        assert_eq!(
            with_backend(|b| b.eval("1 + 1", true)),
            Ok(JsValue::Number(2.0))
        );
        // An unmapped expression is `null`, not an error — a browser `eval`
        // that yields nothing is not a failure.
        assert_eq!(with_backend(|b| b.eval("noop()", false)), Ok(JsValue::Null));
        reset_backend();
    }

    #[test]
    fn get_interface_hands_back_a_handle_that_object_call_can_use() {
        install(RecordingJsBackend::new().with_interface("localStorage", "Storage"));
        let iface = with_backend(|b| b.get_interface("localStorage")).unwrap();
        let handle = match iface {
            JsValue::Object(h) => h,
            other => panic!("expected an object handle, got {other}"),
        };
        assert_eq!(handle.type_name, "Storage");

        with_backend(|b| {
            b.object_call(
                &handle,
                "setItem",
                &[JsValue::String("save".into()), JsValue::String("{}".into())],
            )
        })
        .unwrap();
        with_backend(|b| b.object_set(&handle, "length", JsValue::Number(1.0))).unwrap();
        assert_eq!(
            with_backend(|b| b.object_get(&handle, "length")),
            Ok(JsValue::Number(1.0))
        );
        reset_backend();
    }

    #[test]
    fn unknown_interface_is_null_not_an_error() {
        install(RecordingJsBackend::new());
        assert_eq!(
            with_backend(|b| b.get_interface("definitelyNotAGlobal")),
            Ok(JsValue::Null)
        );
        reset_backend();
    }

    #[test]
    fn callbacks_queue_in_arrival_order_and_drain_once() {
        install(RecordingJsBackend::new());
        with_backend(|b| b.create_callback(7)).unwrap();
        with_backend(|b| b.create_callback(9)).unwrap();

        assert_eq!(pending_callback_count(), 0, "registration must not fire");

        // Stand in for JavaScript invoking the returned functions.
        queue_callback_invocation(7, vec![JsValue::Number(1.0)]);
        queue_callback_invocation(9, vec![JsValue::String("hi".into())]);
        queue_callback_invocation(7, vec![]);
        assert_eq!(pending_callback_count(), 3);

        let drained = take_callback_invocations();
        assert_eq!(
            drained,
            vec![
                (7, vec![JsValue::Number(1.0)]),
                (9, vec![JsValue::String("hi".into())]),
                (7, vec![]),
            ]
        );
        assert!(
            take_callback_invocations().is_empty(),
            "a drained invocation must not run twice"
        );
        reset_backend();
    }

    #[test]
    fn buffer_helpers_round_trip_bytes() {
        install(RecordingJsBackend::new());
        let buf = JsValue::ArrayBuffer(vec![1, 2, 3]);
        assert!(with_backend(|b| b.is_buffer(&buf)));
        assert_eq!(
            with_backend(|b| b.buffer_to_bytes(&buf)),
            Some(vec![1, 2, 3])
        );
        assert!(!with_backend(|b| b.is_buffer(&JsValue::Number(1.0))));
        assert_eq!(
            with_backend(|b| b.buffer_to_bytes(&JsValue::Number(1.0))),
            None
        );
        reset_backend();
    }

    #[test]
    fn with_backend_is_reentrant() {
        // The backend is moved out for the duration of the call, so a nested
        // access must not panic on a double borrow.
        install(RecordingJsBackend::new().with_eval("inner", JsValue::Bool(true)));
        let inner = with_backend(|_| with_backend(|b| b.eval("inner", false)));
        assert_eq!(inner, Ok(JsValue::Bool(true)));
        reset_backend();
    }

    #[test]
    fn reset_backend_drops_queued_invocations() {
        install(RecordingJsBackend::new());
        queue_callback_invocation(1, vec![]);
        assert_eq!(pending_callback_count(), 1);
        reset_backend();
        assert_eq!(pending_callback_count(), 0);
    }
}
