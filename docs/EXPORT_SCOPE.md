# Export presets vs Godot (pat-z35r2.16)

Patina packaging (`gdplatform::export`) produces Patina-specific manifests and
startup packs. It is **not** bit-compatible with Godot export templates.

For the web target specifically, [WEB_EXPORT.md](WEB_EXPORT.md) documents the
one-command flow, the folder it produces and the two orderings the generated
HTML shell has to get right (pat-u3l8c.114).

Mark Export as **Partial** in COMPAT_MATRIX. Migration path: use Patina packaging
for Patina runtime; Godot export remains for Godot engine binaries.
