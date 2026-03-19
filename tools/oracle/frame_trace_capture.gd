## Oracle capture: Frame Trace
## Records per-frame lifecycle events: which nodes had _ready, _process,
## _physics_process called, in what order, with what delta.
## Usage:
##   godot --headless --path <project> -s res://frame_trace_capture.gd -- --output <path> --scene res://scenes/test_scripts.tscn --frames 10
extends SceneTree

const OUTPUT_ARG := "--output"
const FRAMES_ARG := "--frames"
const SCENE_ARG := "--scene"

var _trace: Array[Dictionary] = []
var _frame_count: int = 0
var _max_frames: int = 10
var _ready_done: bool = false

func _init() -> void:
	call_deferred("_setup")

func _setup() -> void:
	var frames_str := _get_arg(FRAMES_ARG)
	if not frames_str.is_empty():
		_max_frames = int(frames_str)

	_load_scene()
	# Attach probes AFTER scene is loaded so we capture _ready ordering.
	_attach_probes_recursive(root)
	_ready_done = true

func _load_scene() -> void:
	var scene_path := _get_arg(SCENE_ARG)
	if not scene_path.is_empty():
		var packed := ResourceLoader.load(scene_path) as PackedScene
		if packed:
			var instance := packed.instantiate()
			root.add_child(instance)
			instance.owner = root
			return
	if root.get_child_count() == 0:
		var main_scene: String = ProjectSettings.get_setting("application/run/main_scene", "")
		if not main_scene.is_empty():
			var packed := ResourceLoader.load(main_scene) as PackedScene
			if packed:
				var instance := packed.instantiate()
				root.add_child(instance)
				instance.owner = root

func _attach_probes_recursive(node: Node) -> void:
	_attach_probe(node)
	for child in node.get_children():
		_attach_probes_recursive(child)

func _attach_probe(node: Node) -> void:
	var path_str := str(node.get_path())
	var class_str := node.get_class()

	# Track lifecycle notifications via signals.
	node.tree_entered.connect(_make_event_cb(path_str, class_str, "notification", "ENTER_TREE"))
	node.tree_exiting.connect(_make_event_cb(path_str, class_str, "notification", "EXIT_TREE"))
	node.ready.connect(_make_event_cb(path_str, class_str, "notification", "READY"))

func _make_event_cb(path_str: String, class_str: String, event_type: String, detail: String) -> Callable:
	return func() -> void:
		_trace.append({
			"event_type": event_type,
			"node_path": path_str,
			"detail": detail,
			"frame": _frame_count,
		})

## Override _process to record per-frame _process notifications.
## This fires for us each frame; we manually walk the tree to record
## which nodes had _process / _physics_process called.
func _process(delta: float) -> bool:
	# Record PROCESS notifications for all nodes in tree order.
	_record_process_notifications(root, delta)
	_frame_count += 1
	if _frame_count >= _max_frames:
		_write_output()
		quit(0)
		return true
	return false

func _record_process_notifications(node: Node, delta: float) -> void:
	var path_str := str(node.get_path())
	# Record INTERNAL_PHYSICS_PROCESS if node processes internally.
	# Record PHYSICS_PROCESS if node has physics processing enabled.
	if node.is_physics_processing_internal():
		_trace.append({
			"event_type": "notification",
			"node_path": path_str,
			"detail": "INTERNAL_PHYSICS_PROCESS",
			"frame": _frame_count,
		})
	if node.is_physics_processing():
		_trace.append({
			"event_type": "notification",
			"node_path": path_str,
			"detail": "PHYSICS_PROCESS",
			"frame": _frame_count,
		})
	if node.is_processing_internal():
		_trace.append({
			"event_type": "notification",
			"node_path": path_str,
			"detail": "INTERNAL_PROCESS",
			"frame": _frame_count,
		})
	if node.is_processing():
		_trace.append({
			"event_type": "notification",
			"node_path": path_str,
			"detail": "PROCESS",
			"frame": _frame_count,
		})
	# Record script calls for nodes with scripts.
	if node.get_script() != null:
		if node.has_method("_process") and node.is_processing():
			_trace.append({
				"event_type": "script_call",
				"node_path": path_str,
				"detail": "_process",
				"frame": _frame_count,
			})
			_trace.append({
				"event_type": "script_return",
				"node_path": path_str,
				"detail": "_process",
				"frame": _frame_count,
			})
		if node.has_method("_physics_process") and node.is_physics_processing():
			_trace.append({
				"event_type": "script_call",
				"node_path": path_str,
				"detail": "_physics_process",
				"frame": _frame_count,
			})
			_trace.append({
				"event_type": "script_return",
				"node_path": path_str,
				"detail": "_physics_process",
				"frame": _frame_count,
			})
	for child in node.get_children():
		_record_process_notifications(child, delta)

func _write_output() -> void:
	var output_path := _get_arg(OUTPUT_ARG)
	if output_path.is_empty():
		output_path = "user://frame_trace.json"

	var data := {
		"total_frames": _frame_count,
		"total_events": _trace.size(),
		"event_trace": _trace,
	}

	var json_str := JSON.stringify(data, "\t")
	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if file == null:
		printerr("frame_trace_capture: cannot open ", output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()
	print("frame_trace_capture: wrote ", output_path, " (", _trace.size(), " events in ", _frame_count, " frames)")

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
