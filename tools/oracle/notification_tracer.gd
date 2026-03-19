## Oracle capture: Notification Tracer
## Records notification-like lifecycle events dispatched to nodes during scene lifecycle.
## Uses signal connections to track lifecycle notifications.
## Usage:
##   godot --headless --path <project> -s res://notification_tracer.gd -- --output <path> --scene res://scenes/main.tscn --frames 10
extends SceneTree

const OUTPUT_ARG := "--output"
const FRAMES_ARG := "--frames"
const SCENE_ARG := "--scene"

var _trace: Array[Dictionary] = []
var _frame_count: int = 0
var _max_frames: int = 10

# Notification ID to name mapping for readability.
const NOTIFICATION_NAMES := {
	0: "POSTINITIALIZE",
	1: "PREDELETE",
	10: "ENTER_TREE",
	11: "EXIT_TREE",
	12: "MOVED_IN_PARENT",
	13: "READY",
	14: "PAUSED",
	15: "UNPAUSED",
	16: "PHYSICS_PROCESS",
	17: "PROCESS",
	18: "PARENTED",
	19: "UNPARENTED",
	20: "SCENE_INSTANTIATED",
	27: "PATH_RENAMED",
	30: "CHILD_ORDER_CHANGED",
	35: "INTERNAL_PROCESS",
	36: "INTERNAL_PHYSICS_PROCESS",
	40: "POST_ENTER_TREE",
}

func _init() -> void:
	call_deferred("_setup")

func _setup() -> void:
	var frames_str := _get_arg(FRAMES_ARG)
	if not frames_str.is_empty():
		_max_frames = int(frames_str)

	_load_scene()
	_attach_probes_recursive(root)

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

	# Record lifecycle signals that correspond to notifications.
	node.tree_entered.connect(_make_notif_cb(path_str, class_str, "ENTER_TREE", 10))
	node.tree_exiting.connect(_make_notif_cb(path_str, class_str, "EXIT_TREE", 11))
	node.ready.connect(_make_notif_cb(path_str, class_str, "READY", 13))
	node.child_order_changed.connect(_make_notif_cb(path_str, class_str, "CHILD_ORDER_CHANGED", 30))

func _make_notif_cb(path_str: String, class_str: String, notif_name: String, notif_id: int) -> Callable:
	return func() -> void:
		_trace.append({
			"notification_id": notif_id,
			"notification_name": notif_name,
			"node_path": path_str,
			"node_class": class_str,
			"frame": _frame_count,
		})

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
		output_path = "user://notification_trace.json"

	var data := {
		"total_frames": _frame_count,
		"total_notifications": _trace.size(),
		"notification_names": NOTIFICATION_NAMES,
		"trace": _trace,
	}

	var json_str := JSON.stringify(data, "\t")
	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if file == null:
		printerr("notification_tracer: cannot open ", output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()
	print("notification_tracer: wrote ", output_path, " (", _trace.size(), " notifications in ", _frame_count, " frames)")

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
