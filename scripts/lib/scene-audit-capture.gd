## Injected as the FIRST autoload by scripts/scene-audit.mjs into a TEMP COPY
## of a game (never into games/) to record what REAL Godot does with one scene
## run standalone: after `frames` main-loop iterations it writes the scene
## tree (name, class, instanced-scene source, script) as JSON and quits. The
## script errors are read from Godot's stderr by scene-audit.mjs.
##
## Same frame count as `patina-runner <scene> --frames N`: the tree is dumped
## from a deferred call queued by this node's `_process`, which runs LAST, so
## iteration N's physics ticks, every `_process` and the deferred calls they
## queued have all run.
##
## Randomness is pinned like scripts/godot-reference.mjs does (the temp copy's
## `randomize()` / `RandomNumberGenerator.new()` are rewritten to call the
## static helpers of `__godot_reference_capture.gd`, which is copied next to
## this file but NOT registered as an autoload).
extends Node

var out_path := ""
var frames := 3
var done := false


func _init() -> void:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == "--scene-audit-out" and i + 1 < args.size():
			out_path = args[i + 1]
		elif args[i] == "--scene-audit-frames" and i + 1 < args.size():
			frames = int(args[i + 1])
	seed(42)


func _ready() -> void:
	process_mode = Node.PROCESS_MODE_ALWAYS
	process_priority = 2147483000
	if out_path == "":
		printerr("scene-audit: no --scene-audit-out")
		get_tree().quit(3)


func _process(_delta: float) -> void:
	if done:
		return
	# This iteration produces frame `get_process_frames() + 1`.
	if Engine.get_process_frames() + 1 >= frames:
		done = true
		_dump.call_deferred()


func _dump() -> void:
	var root := get_tree().root
	var kids: Array = []
	for c in root.get_children():
		if c != self:
			kids.append(_node(c))
	var cur := get_tree().current_scene
	var data := {
		"engine": "godot",
		"version": Engine.get_version_info()["string"],
		"frames": Engine.get_process_frames() + 1,
		"current_scene": str(cur.get_path()) if cur else null,
		"tree": {"name": "root", "class": root.get_class(), "path": "/root", "children": kids},
	}
	var f := FileAccess.open(out_path, FileAccess.WRITE)
	if f == null:
		printerr("scene-audit: cannot write ", out_path)
		get_tree().quit(4)
		return
	f.store_string(JSON.stringify(data))
	f.close()
	get_tree().quit(0)


func _node(n: Node) -> Dictionary:
	var d := {"name": str(n.name), "class": n.get_class(), "path": str(n.get_path())}
	if n.scene_file_path != "":
		d["instance"] = n.scene_file_path
	var s: Script = n.get_script()
	if s and s.resource_path != "":
		d["script"] = s.resource_path
	var kids: Array = []
	for c in n.get_children():
		kids.append(_node(c))
	d["children"] = kids
	return d
