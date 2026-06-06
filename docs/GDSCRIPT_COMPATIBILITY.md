# GDScript Compatibility Reference

A detailed reference of which GDScript language features, built-in functions,
and runtime behaviors are supported by the Patina Engine's `gdscript-interop`
crate. This document is aimed at developers porting Godot 4 projects to Patina.

> **Key point**: Patina includes a tree-walk GDScript interpreter. Scripts are
> parsed and executed from AST — there is no JIT or bytecode compilation. For
> performance-critical code, write Rust directly.

---

## Language Features

### Supported

| Feature | Syntax | Notes |
|---------|--------|-------|
| Variable declaration | `var name [: type] [= value]` | Type hints are informational, not enforced |
| Constants | `const NAME = value` | Parsed; limited enforcement |
| Functions | `func name(params):` | Default parameters, recursion (depth limit 64) |
| Static functions | `static func name():` | |
| Lambda functions | `func(x): return x + 1` | Anonymous/inline functions |
| If/elif/else | `if cond:` / `elif:` / `else:` | |
| While loops | `while cond:` | |
| For loops | `for item in iterable:` | Works with arrays, ranges, dictionaries |
| Match/case | `match value:` with patterns | |
| Break/continue | `break` / `continue` | |
| Pass | `pass` | |
| Return | `return [expr]` | |
| Ternary | `value if cond else other` | |
| Await | `await signal` | Parsed; runtime depends on scene context |
| Class declaration | `class_name ClassName` | |
| Inheritance | `extends ParentClass` | String or identifier |
| Inner classes | `class InnerName:` | Nested class definitions |
| Signals | `signal name(params)` | Declaration and emission |
| Enums | `enum Dir { UP, DOWN }` | Auto-numbered from 0 |
| Annotations | `@export`, `@onready`, `@tool` | Stored on variables/functions |
| Setters/getters | `var x: set = _set, get = _get` | Property accessors |
| String interpolation | `"value: {expr}"` | Inline expressions in strings |
| Self reference | `self.prop`, `self.method()` | |
| Array literals | `[1, 2, 3]` | With indexing and slicing |
| Dictionary literals | `{"key": value}` | String keys |

### Operators

| Category | Operators |
|----------|-----------|
| Arithmetic | `+`, `-`, `*`, `/`, `%` |
| Comparison | `==`, `!=`, `<`, `>`, `<=`, `>=` |
| Logical | `and`, `or`, `not` |
| Membership | `in` (array/dict containment) |
| Assignment | `=`, `+=`, `-=` |
| Member access | `obj.field`, `obj.method()` |
| Index access | `arr[0]`, `dict["key"]` |
| Unary | `-` (negation), `not` |

### Not Supported

| Feature | Status | Alternative |
|---------|--------|-------------|
| `preload(path)` | Not implemented | Load resources via Rust `gdresource` API |
| `load(path)` | Not implemented | Use Rust resource loading |
| `yield(obj, signal)` | Not tokenized | Use `await` (parsed) or Rust async |
| `as` type casting | Not tokenized | Use conversion functions (`int()`, `float()`, `str()`) |
| `is` type checking | Not tokenized | Use `typeof()` and compare strings |
| `assert` | Not tokenized | Use Rust `assert!` in native code |
| `try`/`except` | Not supported | Handle errors in Rust; scripts don't throw |
| Operator overloading | Not supported | Implement operators in Rust types |
| Abstract methods | Not supported | Use trait-based dispatch in Rust |
| Multiple inheritance | Not supported | Same as Godot — single inheritance only |
| `const` (full) | Partial | Parsed but not compile-time evaluated |
| Generators / `yield` as expr | Not supported | Use frame-based state machines |
| `remote`/`master`/`puppet` | Not supported | No multiplayer RPC annotations |

---

## Data Types

### Variant Type Mapping

| GDScript Type | Patina Variant | Rust Backing Type |
|---------------|---------------|-------------------|
| `null` | `Variant::Nil` | `()` |
| `bool` | `Variant::Bool` | `bool` |
| `int` | `Variant::Int` | `i64` |
| `float` | `Variant::Float` | `f64` |
| `String` | `Variant::String` | `String` |
| `StringName` | `Variant::StringName` | `StringName` (interned) |
| `NodePath` | `Variant::NodePath` | `NodePath` |
| `Vector2` | `Variant::Vector2` | `Vector2` (f32, f32) |
| `Vector3` | `Variant::Vector3` | `Vector3` (f32, f32, f32) |
| `Rect2` | `Variant::Rect2` | `Rect2` |
| `Transform2D` | `Variant::Transform2D` | `Transform2D` |
| `Transform3D` | `Variant::Transform3D` | `Transform3D` |
| `Basis` | `Variant::Basis` | `Basis` |
| `Quaternion` | `Variant::Quaternion` | `Quaternion` |
| `AABB` | `Variant::Aabb` | `Aabb` |
| `Plane` | `Variant::Plane` | `Plane` |
| `Color` | `Variant::Color` | `Color` (f32 x 4) |
| `Array` | `Variant::Array` | `Vec<Variant>` |
| `Dictionary` | `Variant::Dictionary` | `HashMap<String, Variant>` |
| `Callable` | `Variant::Callable` | Function reference |
| `Object` | `Variant::ObjectId` | `ObjectId` |
| `Resource` | `Variant::Resource` | Resource metadata |

### Type Conversion Rules

Follows Godot semantics:

- **Int <-> Float**: Lossless within range
- **String <-> Numeric**: Via `int()`, `float()`, `str()` functions
- **Type mismatches**: Return `TypeError` (not silent coercion)

### Truthiness

| Value | Truthy? |
|-------|---------|
| `null` | false |
| `false` | false |
| `0` / `0.0` | false |
| `""` (empty string) | false |
| `[]` (empty array) | false |
| `{}` (empty dict) | false |
| Everything else | true |

---

## Built-in Functions

### Type Conversion & Inspection

| Function | Signature | Notes |
|----------|-----------|-------|
| `print(...)` | Variadic | Outputs to stdout |
| `str(value)` | Any -> String | String representation |
| `int(value)` | Numeric/String -> Int | |
| `float(value)` | Numeric/String -> Float | |
| `len(x)` | Array/Dict/String -> Int | |
| `typeof(value)` | Any -> String | Returns type name |
| `range(...)` | 1-2 args -> Array | `range(n)` or `range(start, end)` |

### Math Functions

| Function | Signature | Notes |
|----------|-----------|-------|
| `abs(x)` | Numeric -> Numeric | Absolute value |
| `sign(x)` | Numeric -> Int | Returns -1, 0, or 1 |
| `floor(x)` | Float -> Int | Round toward -inf |
| `ceil(x)` | Float -> Int | Round toward +inf |
| `round(x)` | Float -> Int | Round to nearest |
| `sqrt(x)` | Float -> Float | Square root |
| `pow(base, exp)` | Float, Float -> Float | |
| `sin(x)` / `cos(x)` | Float -> Float | Radians |
| `min(a, b)` | Numeric -> Numeric | |
| `max(a, b)` | Numeric -> Numeric | |
| `clamp(val, min, max)` | Numeric -> Numeric | |
| `lerp(a, b, t)` | Float -> Float | Linear interpolation |
| `deg_to_rad(deg)` | Float -> Float | |
| `rad_to_deg(rad)` | Float -> Float | |
| `move_toward(cur, tgt, delta)` | Float -> Float | Step toward target |

### Random Numbers

| Function | Signature | Notes |
|----------|-----------|-------|
| `randi()` | -> Int | Deterministic PRNG |
| `randf()` | -> Float | Range [0, 1) |
| `randi_range(min, max)` | Int, Int -> Int | Inclusive range |
| `randf_range(min, max)` | Float, Float -> Float | |

> **Note**: Random number generation uses a deterministic seed by default for
> reproducible behavior in tests.

### Array Methods

| Method | Returns | Notes |
|--------|---------|-------|
| `.append(value)` | void | Add to end |
| `.push_back(value)` | void | Same as append |
| `.size()` / `.length()` | Int | Element count |
| `.reverse()` | void | In-place |
| `.sort()` | void | In-place |
| `.erase(value)` | void | Remove first occurrence |
| `.insert(idx, value)` | void | Insert at position |
| `.slice(from, to)` | Array | Sub-array |
| `.find(value)` | Int | Index or -1 |
| `.has(value)` | Bool | Containment check |
| `.front()` / `.back()` | Variant | First/last element |

### Dictionary Methods

| Method | Returns | Notes |
|--------|---------|-------|
| `.keys()` | Array | All keys |
| `.values()` | Array | All values |
| `.has(key)` | Bool | Key exists |
| `.get(key, default)` | Variant | With fallback |
| `.erase(key)` | void | Remove key |
| `.merge(other)` | void | Combine dictionaries |
| `.size()` / `.length()` | Int | Entry count |

### String Methods

| Method | Returns | Notes |
|--------|---------|-------|
| `.length()` / `.size()` | Int | Character count |
| `.substr(from, len)` | String | Substring |
| `.slice(from, to)` | String | Substring |
| `.to_lower()` / `.to_upper()` | String | Case conversion |
| `.split(delim)` | Array | Split into parts |
| `.strip_edges()` | String | Trim whitespace |
| `.lstrip()` / `.rstrip()` | String | Left/right trim |
| `.begins_with(prefix)` | Bool | |
| `.ends_with(suffix)` | Bool | |
| `.find(substr)` | Int | Index or -1 |
| `.contains(substr)` | Bool | |

### Math Type Constructors

| Constructor | Notes |
|-------------|-------|
| `Vector2(x, y)` | Component access: `.x`, `.y` |
| `Vector3(x, y, z)` | Component access: `.x`, `.y`, `.z` |
| `Color(r, g, b)` / `Color(r, g, b, a)` | Component access: `.r`, `.g`, `.b`, `.a` |

**Vector methods**: `.dot()`, `.cross()`, `.length()`, `.normalized()`

**Static constants**: `Vector2.ZERO`, `Vector2.ONE`, `Vector3.UP`, `Color.WHITE`, etc.

---

## Scene Tree Access

These functions require a `SceneAccess` implementation to be injected into the
interpreter context. They are available when scripts run within an active scene.

| Function | Signature | Notes |
|----------|-----------|-------|
| `get_node(path)` | String -> ObjectId | Resolve NodePath |
| `get_parent()` | -> ObjectId or nil | Parent node |
| `get_children()` | -> Array | Child ObjectIds |
| `emit_signal(name, ...)` | String, ... -> void | Emit signal on self |
| `get_node_property(id, prop)` | ObjectId, String -> Variant | Read property |
| `set_node_property(id, prop, val)` | ObjectId, String, Variant -> void | Write property |

### Input Functions (via SceneAccess)

| Function | Signature | Notes |
|----------|-----------|-------|
| `Input.is_action_pressed(action)` | String -> Bool | |
| `Input.is_action_just_pressed(action)` | String -> Bool | |
| `Input.is_key_pressed(key)` | String -> Bool | |
| `Input.get_global_mouse_position()` | -> (Float, Float) | |
| `Input.is_mouse_button_pressed(btn)` | Int -> Bool | |
| `Input.get_vector(neg_x, pos_x, neg_y, pos_y)` | 4x String -> (Float, Float) | Normalized |

---

## Class & Instance Model

### Supported Class Features

```gdscript
class_name Player
extends Node2D
signal health_changed(new_hp)

@export var speed: float = 100.0
@onready var sprite = $Sprite
var direction = Vector2()

func _ready():
    print("Ready!")

func _process(delta):
    position += direction * speed * delta

static func create():
    return Player.new()

class Weapon:
    var damage = 10

enum State { IDLE, RUNNING, JUMPING }
```

### Lifecycle Methods

The following GDScript lifecycle methods are dispatched via the notification
system when scripts are attached to scene tree nodes:

| Method | When Called |
|--------|------------|
| `_ready()` | Node enters tree for the first time |
| `_process(delta)` | Every idle frame |
| `_physics_process(delta)` | Every physics step |
| `_enter_tree()` | Node added to tree |
| `_exit_tree()` | Node removed from tree |
| `_input(event)` | Unhandled input event |
| `_unhandled_input(event)` | Input not consumed by UI |

### `@onready` Resolution

Variables annotated with `@onready` are resolved after `_ready()` is called,
before the first `_process()`. The `$NodePath` shorthand resolves via
`get_node()` at that time.

### `@export` Variables

Variables annotated with `@export` are:
- Listed in script metadata via `list_properties()`
- Editable in the Patina editor inspector
- Serialized/deserialized with the scene

---

## Integration Architecture

```
  GDScript Source (.gd)
        │
        ▼
  ┌─────────────┐
  │  Tokenizer   │  gdscript-interop/src/tokenizer.rs
  └──────┬──────┘
         ▼
  ┌─────────────┐
  │   Parser     │  gdscript-interop/src/parser.rs
  └──────┬──────┘
         ▼
  ┌─────────────┐
  │ Interpreter  │  gdscript-interop/src/interpreter.rs
  └──────┬──────┘
         │
    SceneAccess trait
         │
    ┌────┴─────┐
    ▼          ▼
  gdscene   gdobject    (scene tree, signals, notifications)
```

**ScriptBridge** (`bridge.rs`) maps `ObjectId` -> `ScriptInstance`, allowing
the engine to dispatch notifications and access script variables on any node.

---

## Deprecated Features

### VisualScript (.vs files)

VisualScript was removed in Godot 4. Patina provides a `VisualScriptStub` that:
- Allows scenes referencing `.vs` files to load without crashing
- Returns `MethodNotFound` for all method calls
- Logs a deprecation warning on first access

No migration path is needed — VisualScript projects should have already migrated
to GDScript or C# before upgrading to Godot 4.

---

## Performance Notes

- **Execution model**: Tree-walk interpreter (no compilation step)
- **Recursion limit**: 64 frames (configurable via `MAX_RECURSION_DEPTH`)
- **Deterministic RNG**: Seeded for reproducible test results
- **Scope overhead**: Lexical scopes stored as `Vec<HashMap<String, Variant>>`
- **No JIT**: For performance-critical paths, write Rust and call from scripts
  via the `SceneAccess` trait

For hot loops or math-heavy code, consider moving the logic to Rust and exposing
it as a scene tree operation or custom built-in function.

---

## Missing Built-in Functions

The following Godot built-in functions are **not** available:

| Function | Category | Alternative |
|----------|----------|-------------|
| `preload()` | Resources | Rust `gdresource` API |
| `load()` | Resources | Rust `gdresource` API |
| `weakref()` | References | Rust `Weak<T>` |
| `instance_from_id()` | Object lookup | `get_node()` with NodePath |
| `is_instance_valid()` | Object safety | Check via `ObjectId` validity |
| `push_error()` / `push_warning()` | Logging | `print()` or Rust `tracing` |
| `var_to_str()` / `str_to_var()` | Serialization | Rust serde |
| `var_to_bytes()` / `bytes_to_var()` | Binary serialization | Rust bincode/postcard |

---

## See Also

- [Migration Guide](migration-guide.md) — Full porting walkthrough
- [Node Type Compatibility Table](migration-guide.md#node-type-compatibility-table) — Per-node support status
- [Known Limitations](migration-guide.md#known-limitations-and-workarounds) — Workarounds for missing features
