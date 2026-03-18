# Godot 4.x Scene Format Compatibility Notes

Format quirks and parser behavior discovered during real Godot 4.3 compatibility testing.

## Header Attributes

- `[gd_scene load_steps=N format=3 uid="uid://..."]` — `load_steps` and `format` are unquoted integer attributes. The parser handles these via the generic unquoted-value branch in `extract_header_attrs`.
- `uid` values use the format `uid://base62chars`. The `parse_uid_string` function hashes these to produce a stable numeric `ResourceUid`.

## ext_resource uid Format

- Godot 4.x ext_resource lines use `uid="uid://..."` alongside `path` and `id` attributes.
- The `id` field uses a `"N_hash"` format (e.g., `"1_abc12"`) rather than plain integers as in Godot 3.x.

## Packed Typed Arrays

Godot 4.x uses typed packed arrays that serialize differently from generic `Array`:

| Type | Format | Notes |
|------|--------|-------|
| `PackedByteArray()` | Comma-separated ints | Empty parens = empty array |
| `PackedInt32Array(0, 1, 2)` | Comma-separated ints | Stored as `Variant::Int` elements |
| `PackedInt64Array(...)` | Same as Int32 | No distinct 32/64 in our Variant |
| `PackedFloat32Array(0, 0, 1024, 640)` | Comma-separated floats | Stored as `Variant::Float` |
| `PackedFloat64Array(...)` | Same as Float32 | No distinct 32/64 in our Variant |
| `PackedStringArray("a", "b")` | Quoted strings | Each element must be quoted |
| `PackedVector2Array(x1, y1, x2, y2)` | Flat float pairs | Stored as `Variant::Vector2` elements |
| `PackedVector3Array(x1, y1, z1, ...)` | Flat float triples | Stored as `Variant::Vector3` elements |
| `PackedColorArray(r, g, b, a, ...)` | Flat float quads | Stored as `Variant::Color` elements |

All packed arrays are represented as `Variant::Array` internally. The type distinction is lost at parse time (matches Godot's runtime behavior where packed arrays convert to generic arrays freely).

## Property Key Paths

- Godot uses `/`-separated property paths for metadata: `metadata/key = value`.
- These are stored verbatim as property keys (e.g., `"metadata/move_speed"`).
- No special handling needed — `split_once('=')` correctly captures the full key.

## Integer Variant Types

- `Vector2i(x, y)` and `Vector3i(x, y, z)` are parsed as regular `Vector2`/`Vector3` (float).
- This matches Godot's behavior where integer vectors auto-convert to float vectors in most contexts.

## Null Values

- Godot uses `null` in property values (e.g., `physics_material_override = null`).
- Parser accepts `null`, `nil`, and `Nil` — all map to `Variant::Nil`.

## Connection Sections

- `[connection signal="..." from="..." to="." method="..." flags=N]`
- `flags` is optional (defaults to 0).
- Paths can use `/` separators for nested nodes (e.g., `from="VBoxContainer/StartButton"`).

## Sub-Resource References in Properties

- `shape = SubResource("RectangleShape2D_rect1")` — stored as `Variant::String("SubResource:RectangleShape2D_rect1")`.
- `script = ExtResource("1_abc12")` — stored as `Variant::String("ExtResource:1_abc12")`.
- The parser doesn't resolve these references — that's left to the runtime.

## Dictionary Values in Properties

- `libraries = {"": SubResource("AnimationLibrary_anim1")}` — nested function calls inside dictionaries work correctly because `split_args` respects parenthesis depth.

## Known Limitations

1. **Multi-line values**: Not supported. Godot sometimes splits long arrays across lines; our parser requires each property on a single line.
2. **Type annotations**: `Array[int]` typed arrays in the header are not distinguished from generic arrays.
3. **Resource embedding**: Inline resource definitions (rare in `.tscn`) are not handled.
4. **Comments in values**: Inline `;` comments after values are not stripped.
