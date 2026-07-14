# Export presets vs Godot (pat-z35r2.16)

Patina packaging (`gdplatform::export`) produces Patina-specific manifests and
startup packs. It is **not** bit-compatible with Godot export templates.

Mark Export as **Partial** in COMPAT_MATRIX. Migration path: use Patina packaging
for Patina runtime; Godot export remains for Godot engine binaries.
