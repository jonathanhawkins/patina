## Oracle capture: Signal Tracer
## Connects to all user-defined signals on all nodes, records emissions.
## Runs for a configurable number of frames then outputs ordered trace.
## Usage:
##   godot --headless --path <project> -s res://signal_tracer.gd -- --output <path> --scene res://scenes/main.tscn --frames 60
extends SceneTree

const OUTPUT_ARG := "--output"
const FRAMES_ARG := "--frames"
const SCENE_ARG := "--scene"

var _trace: Array[Dictionary] = []
var _frame_count: int = 0
var _max_frames: int = 60

func _init() -> void:
	call_deferred("_setup")

func _setup() -> void:
	var frames_str := _get_arg(FRAMES_ARG)
	if not frames_str.is_empty():
		_max_frames = int(frames_str)

	_load_scene()

	# Connect to all user signals on all existing nodes.
	_connect_signals_recursive(root)

	# Watch for new nodes added to the tree.
	root.child_entered_tree.connect(_on_node_added)

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

func _on_node_added(node: Node) -> void:
	_connect_signals_recursive(node)

func _connect_signals_recursive(node: Node) -> void:
	_connect_node_signals(node)
	for child in node.get_children():
		_connect_signals_recursive(child)

func _connect_node_signals(node: Node) -> void:
	for sig_info in node.get_signal_list():
		var sig_name: String = sig_info["name"]
		# Skip built-in signals that are too noisy.
		if sig_name in ["tree_entered", "tree_exiting", "tree_exited",
				"child_entered_tree", "child_exiting_tree",
				"child_order_changed", "ready", "renamed",
				"property_list_changed", "script_changed"]:
			continue
		var cb := _make_signal_callback(node, sig_name)
		if not node.is_connected(sig_name, cb):
			node.connect(sig_name, cb)

func _make_signal_callback(source: Node, sig_name: String) -> Callable:
	return func(arg1 = null, arg2 = null, arg3 = null, arg4 = null,
			arg5 = null, arg6 = null, arg7 = null, arg8 = null) -> void:
		var args := []
		for a in [arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8]:
			if a != null:
				args.append(_serialize_arg(a))
			else:
				break
		_trace.append({
			"signal_name": sig_name,
			"source_path": str(source.get_path()),
			"args": args,
			"frame_number": _frame_count,
		})

func _serialize_arg(value: Variant) -> Variant:
	match typeof(value):
		TYPE_NIL:
			return null
		TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING, TYPE_STRING_NAME:
			return value
		TYPE_VECTOR2:
			return {"type": "Vector2", "value": [value.x, value.y]}
		TYPE_VECTOR3:
			return {"type": "Vector3", "value": [value.x, value.y, value.z]}
		TYPE_OBJECT:
			if value is Node:
				return {"type": "Node", "path": str(value.get_path())}
			return {"type": value.get_class()}
		_:
			return str(value)

func _process(delta: float) -> bool:
	_frame_count += 1
	if _frame_count >= _max_frames:
		_write_output()
		quit(0)
		return true
	return false

func _write_output() -> void:
	var output_path := _get_arg(OUTPUT_ARG)
	if output_path.is_empty():
		output_path = "user://signal_trace.json"

	var data := {
		"total_frames": _frame_count,
		"total_signals": _trace.size(),
		"trace": _trace,
	}

	var json_str := JSON.stringify(data, "\t")
	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if file == null:
		printerr("signal_tracer: cannot open ", output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()
	print("signal_tracer: wrote ", output_path, " (", _trace.size(), " signals in ", _frame_count, " frames)")

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
