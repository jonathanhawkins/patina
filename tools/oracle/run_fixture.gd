## Oracle capture: Run Fixture (Master autoload)
## Runs all captures on a scene and outputs a combined JSON envelope.
## Usage:
##   godot --headless --path <project> -s res://run_fixture.gd -- --output <path> --scene res://scenes/main.tscn --frames 10
extends SceneTree

const OUTPUT_ARG := "--output"
const FRAMES_ARG := "--frames"
const FIXTURE_ID_ARG := "--fixture-id"
const UPSTREAM_VERSION_ARG := "--upstream-version"
const SCENE_ARG := "--scene"

var _trace_signals: Array[Dictionary] = []
var _trace_notifications: Array[Dictionary] = []
var _frame_count: int = 0
var _max_frames: int = 10

func _init() -> void:
	call_deferred("_setup")

func _setup() -> void:
	var frames_str := _get_arg(FRAMES_ARG)
	if not frames_str.is_empty():
		_max_frames = int(frames_str)

	_load_scene()

	# Set up signal tracing and notification tracing on all nodes.
	_instrument_recursive(root)
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
	_instrument_recursive(node)

func _instrument_recursive(node: Node) -> void:
	_instrument_node(node)
	for child in node.get_children():
		_instrument_recursive(child)

func _instrument_node(node: Node) -> void:
	var path_str := str(node.get_path())
	var class_str := node.get_class()

	# Signal tracing: connect to non-builtin signals.
	for sig_info in node.get_signal_list():
		var sig_name: String = sig_info["name"]
		if sig_name in ["tree_entered", "tree_exiting", "tree_exited",
				"child_entered_tree", "child_exiting_tree",
				"child_order_changed", "ready", "renamed",
				"property_list_changed", "script_changed"]:
			continue
		var cb := _make_signal_cb(node, sig_name)
		if not node.is_connected(sig_name, cb):
			node.connect(sig_name, cb)

	# Notification tracing via lifecycle signals.
	_connect_notif_signal(node, "tree_entered", path_str, class_str, "ENTER_TREE", 10)
	_connect_notif_signal(node, "tree_exiting", path_str, class_str, "EXIT_TREE", 11)
	_connect_notif_signal(node, "ready", path_str, class_str, "READY", 13)

func _connect_notif_signal(node: Node, sig_name: String, path_str: String,
		class_str: String, notif_name: String, notif_id: int) -> void:
	var cb := func() -> void:
		_trace_notifications.append({
			"notification_id": notif_id,
			"notification_name": notif_name,
			"node_path": path_str,
			"node_class": class_str,
			"frame": _frame_count,
		})
	if not node.is_connected(sig_name, cb):
		node.connect(sig_name, cb)

func _make_signal_cb(source: Node, sig_name: String) -> Callable:
	return func(arg1 = null, arg2 = null, arg3 = null, arg4 = null,
			arg5 = null, arg6 = null, arg7 = null, arg8 = null) -> void:
		var args := []
		for a in [arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8]:
			if a != null:
				args.append(_serialize_arg(a))
			else:
				break
		_trace_signals.append({
			"signal_name": sig_name,
			"source_path": str(source.get_path()),
			"args": args,
			"frame_number": _frame_count,
		})

func _serialize_arg(value: Variant) -> Variant:
	match typeof(value):
		TYPE_NIL: return null
		TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING, TYPE_STRING_NAME: return value
		TYPE_VECTOR2: return {"type": "Vector2", "value": [value.x, value.y]}
		TYPE_VECTOR3: return {"type": "Vector3", "value": [value.x, value.y, value.z]}
		TYPE_OBJECT:
			if value is Node:
				return {"type": "Node", "path": str(value.get_path())}
			return {"type": value.get_class()}
		_: return str(value)

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
		output_path = "user://fixture_output.json"

	var fixture_id := _get_arg(FIXTURE_ID_ARG)
	if fixture_id.is_empty():
		var scene := root.get_child(0) if root.get_child_count() > 0 else null
		fixture_id = scene.name if scene else "unknown"

	var upstream_version := _get_arg(UPSTREAM_VERSION_ARG)
	if upstream_version.is_empty():
		upstream_version = "4.5.1-stable"

	# Capture scene tree.
	var scene_tree_data := _dump_scene_tree(root)

	# Capture properties.
	var properties_data := _dump_properties(root)

	# Build envelope.
	var envelope := {
		"fixture_id": fixture_id,
		"capture_type": "full",
		"upstream_version": upstream_version,
		"generated_at": Time.get_datetime_string_from_system(true),
		"frames_captured": _frame_count,
		"scene_tree": scene_tree_data,
		"properties": properties_data,
		"signal_trace": _trace_signals,
		"notification_trace": _trace_notifications,
	}

	var json_str := JSON.stringify(envelope, "\t")
	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if file == null:
		printerr("run_fixture: cannot open ", output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()
	print("run_fixture: wrote ", output_path)

## Scene tree capture.
func _dump_scene_tree(node: Node) -> Dictionary:
	var children_arr: Array[Dictionary] = []
	for child in node.get_children():
		children_arr.append(_dump_scene_tree(child))

	var groups: Array[String] = []
	for g in node.get_groups():
		groups.append(str(g))

	var result := {
		"name": node.name,
		"class": node.get_class(),
		"path": str(node.get_path()),
		"groups": groups,
		"children": children_arr,
	}

	var script = node.get_script()
	if script != null and script is Script:
		result["script"] = script.resource_path

	return result

## Property capture.
func _dump_properties(node: Node) -> Dictionary:
	var properties := {}
	for prop_info in node.get_property_list():
		var usage: int = prop_info["usage"]
		if not (usage & PROPERTY_USAGE_SCRIPT_VARIABLE or usage & PROPERTY_USAGE_STORAGE):
			continue
		var prop_name: String = prop_info["name"]
		if prop_name in ["script", ""]:
			continue
		var value = node.get(prop_name)
		properties[prop_name] = {
			"type": type_string(typeof(value)),
			"value": _serialize_property(value),
		}

	var children_arr: Array[Dictionary] = []
	for child in node.get_children():
		children_arr.append(_dump_properties(child))

	return {
		"name": node.name,
		"class": node.get_class(),
		"path": str(node.get_path()),
		"properties": properties,
		"children": children_arr,
	}

func _serialize_property(value: Variant) -> Variant:
	match typeof(value):
		TYPE_NIL: return null
		TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING, TYPE_STRING_NAME: return value
		TYPE_VECTOR2: return {"x": value.x, "y": value.y}
		TYPE_VECTOR3: return {"x": value.x, "y": value.y, "z": value.z}
		TYPE_COLOR: return {"r": value.r, "g": value.g, "b": value.b, "a": value.a}
		TYPE_RECT2: return {"position": _serialize_property(value.position), "size": _serialize_property(value.size)}
		TYPE_NODE_PATH: return str(value)
		TYPE_ARRAY:
			var arr := []
			for item in value:
				arr.append(_serialize_property(item))
			return arr
		TYPE_DICTIONARY:
			var dict := {}
			for key in value:
				dict[str(key)] = _serialize_property(value[key])
			return dict
		TYPE_OBJECT:
			if value == null: return null
			if value is Resource:
				return {"_resource_class": value.get_class(), "_resource_path": value.resource_path}
			return {"_object_class": value.get_class()}
		_: return str(value)

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
