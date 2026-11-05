## Injected as an autoload by scripts/godot-reference.mjs (pat-xggzs) into a
## TEMP COPY of a game — never into games/. Replays a key timeline at fixed
## physics frames, records watched properties every frame, saves PNG frames.
##
## Frame numbering matches Patina's MCP `play` tool exactly:
## - frame 0 is the state before anything steps (after every `_ready`);
## - frame f is the state after f main-loop iterations (each runs the
##   project's physics ticks for one frame — TWO at the 3D platformer's
##   physics_ticks_per_second=120 — then `_process`, then the render);
## - an input {start, end} is pressed before frame `start` steps and released
##   after frame `end` steps.
## This node runs FIRST in the first physics tick of every iteration (lowest
## priority), so at iteration f+1 it sees the complete result of frame f
## (scripts, move_and_slide, the physics server step, `_process` and that
## frame's render) and records it as frame f, then applies the inputs for
## frame f+1 before any game script runs — where a real key press lands.
extends Node

var cfg: Dictionary = {}
var out_dir := ""
var last_iteration := -1
var samples: Array = []
var shots: Dictionary = {}
var written := false
var finished := false


func _ready() -> void:
	process_mode = Node.PROCESS_MODE_ALWAYS
	process_physics_priority = -2147483000
	var path := ""
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == "--godot-reference-config" and i + 1 < args.size():
			path = args[i + 1]
	var f := FileAccess.open(path, FileAccess.READ)
	if f == null:
		printerr("godot-reference: cannot read config ", path)
		get_tree().quit(3)
		return
	cfg = JSON.parse_string(f.get_as_text())
	out_dir = cfg["out_dir"]
	for s in cfg.get("shots", []):
		shots[int(s)] = true
	DirAccess.make_dir_recursive_absolute(out_dir.path_join("frames"))
	print("godot-reference: ", RenderingServer.get_current_rendering_method(), " on ",
		RenderingServer.get_current_rendering_driver_name(), ", window ", get_viewport().size,
		", ", Engine.physics_ticks_per_second, " physics ticks/s")


func _physics_process(_delta: float) -> void:
	if finished:
		return
	# During iteration k's physics ticks Engine.get_process_frames() is k-1:
	# the number of COMPLETED frames. Only the first tick of an iteration acts.
	var frame := Engine.get_process_frames()
	if frame == last_iteration:
		return
	last_iteration = frame
	_record(frame)
	if shots.has(frame) and frame > 0:
		_shoot(frame)
	for inp in cfg["inputs"]:
		if int(inp["end"]) == frame:
			_key(inp, false)
	if frame >= int(cfg["frames"]):
		finished = true
		_write()
		get_tree().quit(0)
		return
	for inp in cfg["inputs"]:
		if int(inp["start"]) == frame + 1:
			_key(inp, true)
	Input.flush_buffered_events()


func _key(inp: Dictionary, down: bool) -> void:
	var name: String = inp["key"]
	var code := OS.find_keycode_from_string(name)
	if code == KEY_NONE:
		printerr("godot-reference: unknown key ", name)
		return
	var ev := InputEventKey.new()
	ev.keycode = code
	ev.physical_keycode = code
	ev.key_label = code
	ev.pressed = down
	if down and name.length() == 1:
		ev.unicode = name.to_lower().unicode_at(0)
	elif down and name == "Space":
		ev.unicode = 32
	Input.parse_input_event(ev)


func _record(frame: int) -> void:
	var values := {}
	var errors := {}
	for w in cfg["watches"]:
		var node := get_node_or_null(NodePath(w["path"]))
		if node == null:
			errors[w["id"]] = "no node at " + str(w["path"])
			values[w["id"]] = null
			continue
		values[w["id"]] = _plain(node.get(w["prop"]))
	var sample := {
		"frame": frame,
		"values": values,
		"physics_frames": Engine.get_physics_frames(),
		"process_frames": Engine.get_process_frames(),
	}
	if not errors.is_empty():
		sample["errors"] = errors
	samples.append(sample)


func _plain(v: Variant) -> Variant:
	match typeof(v):
		TYPE_VECTOR2, TYPE_VECTOR2I:
			return [v.x, v.y]
		TYPE_VECTOR3, TYPE_VECTOR3I:
			return [v.x, v.y, v.z]
		TYPE_VECTOR4, TYPE_VECTOR4I, TYPE_QUATERNION:
			return [v.x, v.y, v.z, v.w]
		TYPE_COLOR:
			return [v.r, v.g, v.b, v.a]
		TYPE_BASIS:
			return [v.x.x, v.x.y, v.x.z, v.y.x, v.y.y, v.y.z, v.z.x, v.z.y, v.z.z]
		TYPE_TRANSFORM3D:
			var b: Basis = v.basis
			return [b.x.x, b.x.y, b.x.z, b.y.x, b.y.y, b.y.z, b.z.x, b.z.y, b.z.z, v.origin.x, v.origin.y, v.origin.z]
		TYPE_TRANSFORM2D:
			return [v.x.x, v.x.y, v.y.x, v.y.y, v.origin.x, v.origin.y]
		TYPE_BOOL:
			return 1 if v else 0
		TYPE_INT, TYPE_FLOAT:
			return v
		TYPE_NIL:
			return null
	return str(v)


func _shoot(frame: int) -> void:
	# The texture holds the last DRAWN frame: the render that ended iteration
	# `frame`, i.e. the state after `frame` steps.
	var img := get_viewport().get_texture().get_image()
	if img == null:
		printerr("godot-reference: no image for frame ", frame)
		return
	img.save_png(out_dir.path_join("frames").path_join("f%05d.png" % frame))


func _write() -> void:
	if written:
		return
	written = true
	var data := {
		"engine": "godot",
		"version": Engine.get_version_info()["string"],
		"rendering_method": RenderingServer.get_current_rendering_method(),
		"rendering_driver": RenderingServer.get_current_rendering_driver_name(),
		"window": [get_viewport().size.x, get_viewport().size.y],
		"physics_ticks_per_second": Engine.physics_ticks_per_second,
		"physics_interpolation": ProjectSettings.get_setting("physics/common/physics_interpolation", false),
		"frames": samples,
		"quit_early": not finished,
	}
	var f := FileAccess.open(out_dir.path_join("trace.json"), FileAccess.WRITE)
	f.store_string(JSON.stringify(data, "\t"))
	f.close()


func _exit_tree() -> void:
	# The game quit itself (or crashed out of the tree) before the timeline
	# ended: keep what was recorded.
	_write()
