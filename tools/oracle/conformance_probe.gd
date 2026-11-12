## Oracle capture: Conformance Probe (pat-mgixk.403)
##
## An AUTOLOAD, not a `-s` script: Godot boots autoloads before the main scene,
## so this probe is already in the tree when the project's own nodes enter it
## and can watch the real boot — `-s res://probe.gd` replaces the main loop and
## has to instance the scene itself, which is not how a game runs.
##
## It is injected by `conformance_capture.py` into a COPY of the project via a
## generated `override.cfg`; the fixture project itself is never edited.
##
## Records, for `--frames` frames:
##   * every signal emission (ordered, with its source path), and
##   * the final flat scene tree (path, class, allow-listed properties).
##
## `print()` output is NOT captured here — Godot gives a script no hook for it.
## The harness reads it off the process stdout instead, which is why every
## fixture prefixes its observable lines with "[T] ".
extends Node

## Built-in signals every node has; recording them would drown the trace and
## they say nothing about a port's behaviour.
const NOISY_SIGNALS := [
	"tree_entered", "tree_exiting", "tree_exited", "ready", "renamed",
	"child_entered_tree", "child_exiting_tree", "child_order_changed",
	"replacing_by", "editor_description_changed", "editor_state_changed",
	"property_list_changed", "script_changed",
	"visibility_changed", "draw", "item_rect_changed",
	"gui_input", "mouse_entered", "mouse_exited", "focus_entered",
	"focus_exited", "minimum_size_changed", "resized", "size_flags_changed",
	"theme_changed", "pre_sort_children", "sort_children",
]

## The only properties compared across engines. A narrow, explicit list is the
## point: a full property dump diverges on dozens of fields that no game
## observes, and the resulting FAIL carries no information.
const TRACKED_PROPERTIES := [
	"visible", "position", "rotation", "scale", "text",
	"wait_time", "one_shot", "autostart", "velocity",
]

var _signals: Array = []
var _frames: int = 0
var _max_frames: int = 12
var _output_path: String = ""
var _written: bool = false

func _ready() -> void:
	_max_frames = int(_arg("--frames", "12"))
	_output_path = _arg("--output", "")
	# Highest priority value runs LAST within a frame, so the dump-and-quit
	# check sees the state the game's own `_process` just produced.
	process_priority = 1000
	get_tree().node_added.connect(_on_node_added)
	_connect_recursive(get_tree().root)

func _on_node_added(node: Node) -> void:
	_connect_node(node)

func _connect_recursive(node: Node) -> void:
	_connect_node(node)
	for child in node.get_children():
		_connect_recursive(child)

func _connect_node(node: Node) -> void:
	# Never trace the probe itself: its own signals are an artifact of
	# measuring, and Patina has no counterpart for them.
	if node == self or is_ancestor_of(node):
		return
	for sig_info in node.get_signal_list():
		var sig_name: String = sig_info["name"]
		if sig_name in NOISY_SIGNALS:
			continue
		var cb := _make_signal_callback(node, sig_name)
		if not node.is_connected(sig_name, cb):
			node.connect(sig_name, cb)

func _make_signal_callback(source: Node, sig_name: String) -> Callable:
	return func(_a1 = null, _a2 = null, _a3 = null, _a4 = null,
			_a5 = null, _a6 = null, _a7 = null, _a8 = null) -> void:
		_signals.append({
			"signal": sig_name,
			"source": _rel_path(source),
			"frame": _frames,
		})

func _process(_delta: float) -> void:
	_frames += 1
	if _frames >= _max_frames and not _written:
		_write_output()
		get_tree().quit(0)

func _rel_path(node: Node) -> String:
	# Paths are reported relative to `/root` so the two engines' differing
	# root-window naming never shows up as a tree-shape diff.
	var p := str(node.get_path())
	if p.begins_with("/root/"):
		return p.substr(6)
	if p == "/root":
		return ""
	return p

func _collect_tree() -> Array:
	var rows: Array = []
	_collect_node(get_tree().root, rows)
	rows.sort_custom(func(a, b): return a["path"] < b["path"])
	return rows

func _collect_node(node: Node, rows: Array) -> void:
	var rel := _rel_path(node)
	# Skip `/root` itself and the probe's own subtree.
	if rel != "" and node != self and not is_ancestor_of(node):
		var props: Dictionary = {}
		for name in TRACKED_PROPERTIES:
			if name in node:
				props[name] = _serialize(node.get(name))
		rows.append({
			"path": rel,
			"class": node.get_class(),
			"properties": props,
		})
	for child in node.get_children():
		_collect_node(child, rows)

func _serialize(value: Variant) -> Variant:
	match typeof(value):
		TYPE_NIL:
			return null
		TYPE_BOOL:
			return value
		TYPE_INT:
			return value
		TYPE_FLOAT:
			return snappedf(value, 0.001)
		TYPE_STRING, TYPE_STRING_NAME:
			return str(value)
		TYPE_VECTOR2:
			return [snappedf(value.x, 0.001), snappedf(value.y, 0.001)]
		TYPE_VECTOR3:
			return [snappedf(value.x, 0.001), snappedf(value.y, 0.001), snappedf(value.z, 0.001)]
		_:
			return str(value)

func _write_output() -> void:
	_written = true
	var envelope := {
		"frames": _frames,
		"signals": _signals,
		"tree": _collect_tree(),
	}
	if _output_path.is_empty():
		print("conformance_probe: no --output given")
		return
	var file := FileAccess.open(_output_path, FileAccess.WRITE)
	if file == null:
		printerr("conformance_probe: cannot open ", _output_path)
		return
	file.store_string(JSON.stringify(envelope, "  "))
	file.close()
	print("conformance_probe: wrote ", _output_path)

func _arg(name: String, fallback: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == name and i + 1 < args.size():
			return args[i + 1]
	return fallback
