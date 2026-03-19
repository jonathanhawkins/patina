## Oracle capture: Property Dumper
## For every node, dumps all non-default properties with type information.
## Usage:
##   godot --headless --path <project> -s res://property_dumper.gd -- --output <path> --scene res://scenes/main.tscn
extends SceneTree

const OUTPUT_ARG := "--output"
const SCENE_ARG := "--scene"

func _init() -> void:
	call_deferred("_capture")

func _capture() -> void:
	var output_path := _get_arg(OUTPUT_ARG)
	if output_path.is_empty():
		output_path = "user://property_dump.json"

	_load_scene()

	var data := _dump_node_properties(root)
	var json_str := JSON.stringify(data, "\t")

	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if file == null:
		printerr("property_dumper: cannot open ", output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()

	print("property_dumper: wrote ", output_path)
	quit(0)

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

func _dump_node_properties(node: Node) -> Dictionary:
	var properties := {}
	for prop_info in node.get_property_list():
		var usage: int = prop_info["usage"]
		if not (usage & PROPERTY_USAGE_SCRIPT_VARIABLE or usage & PROPERTY_USAGE_STORAGE):
			continue
		var prop_name: String = prop_info["name"]
		if prop_name in ["script", "metadata/", ""]:
			continue

		var value = node.get(prop_name)
		var default_value = _get_class_default(node, prop_name)

		if not (usage & PROPERTY_USAGE_SCRIPT_VARIABLE) and _values_equal(value, default_value):
			continue

		properties[prop_name] = {
			"type": type_string(typeof(value)),
			"value": _serialize_value(value),
			"class_type": prop_info.get("class_name", ""),
			"hint": prop_info.get("hint", 0),
		}

	var children_arr: Array[Dictionary] = []
	for child in node.get_children():
		children_arr.append(_dump_node_properties(child))

	return {
		"name": node.name,
		"class": node.get_class(),
		"path": str(node.get_path()),
		"properties": properties,
		"children": children_arr,
	}

func _get_class_default(node: Node, prop_name: String) -> Variant:
	var class_name_str := node.get_class()
	if ClassDB.can_instantiate(class_name_str):
		var temp = ClassDB.instantiate(class_name_str)
		if temp != null:
			var val = temp.get(prop_name)
			if temp is Node:
				temp.free()
			return val
	return null

func _values_equal(a: Variant, b: Variant) -> bool:
	if typeof(a) != typeof(b):
		return false
	return a == b

func _serialize_value(value: Variant) -> Variant:
	match typeof(value):
		TYPE_NIL:
			return null
		TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING, TYPE_STRING_NAME:
			return value
		TYPE_VECTOR2:
			return {"x": value.x, "y": value.y}
		TYPE_VECTOR2I:
			return {"x": value.x, "y": value.y}
		TYPE_VECTOR3:
			return {"x": value.x, "y": value.y, "z": value.z}
		TYPE_VECTOR3I:
			return {"x": value.x, "y": value.y, "z": value.z}
		TYPE_RECT2:
			return {"position": _serialize_value(value.position), "size": _serialize_value(value.size)}
		TYPE_COLOR:
			return {"r": value.r, "g": value.g, "b": value.b, "a": value.a}
		TYPE_TRANSFORM2D:
			return {"x": _serialize_value(value.x), "y": _serialize_value(value.y), "origin": _serialize_value(value.origin)}
		TYPE_TRANSFORM3D:
			return {"basis": {"x": _serialize_value(value.basis.x), "y": _serialize_value(value.basis.y), "z": _serialize_value(value.basis.z)}, "origin": _serialize_value(value.origin)}
		TYPE_NODE_PATH:
			return str(value)
		TYPE_ARRAY:
			var arr := []
			for item in value:
				arr.append(_serialize_value(item))
			return arr
		TYPE_DICTIONARY:
			var dict := {}
			for key in value:
				dict[str(key)] = _serialize_value(value[key])
			return dict
		TYPE_OBJECT:
			if value == null:
				return null
			if value is Resource:
				return {"_resource_class": value.get_class(), "_resource_path": value.resource_path}
			return {"_object_class": value.get_class()}
		_:
			return str(value)

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
