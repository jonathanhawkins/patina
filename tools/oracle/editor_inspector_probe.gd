## Oracle capture: Editor Inspector Probe
## Captures the inspector ground truth for nodes in a scene — the exact set of
## properties, categories, and metadata that Godot 4.6.1's inspector would show
## when a node is selected.
##
## This differs from property_dumper.gd in that it captures *inspector-visible*
## properties (filtered by PROPERTY_USAGE_EDITOR) rather than all runtime props.
##
## Usage:
##   godot --headless --path <project> -s res://editor_inspector_probe.gd -- --output <path> --scene res://scenes/main.tscn
##
## Output format (JSON):
##   {
##     "godot_version": "4.6.1.stable",
##     "scene": "res://scenes/main.tscn",
##     "nodes": [
##       {
##         "path": "/root/Main/Player",
##         "class": "CharacterBody2D",
##         "inspector_properties": [
##           {
##             "name": "motion_mode",
##             "class_name": "CharacterBody2D",
##             "type": 2,
##             "type_name": "int",
##             "hint": 2,
##             "hint_string": "Grounded,Floating",
##             "usage": 4102,
##             "value": 0,
##             "is_default": true
##           },
##           ...
##         ]
##       },
##       ...
##     ]
##   }
extends SceneTree

const OUTPUT_ARG := "--output"
const SCENE_ARG := "--scene"

func _init() -> void:
	call_deferred("_capture")

func _capture() -> void:
	var output_path := _get_arg(OUTPUT_ARG)
	if output_path.is_empty():
		output_path = "user://editor_inspector_probe.json"

	_load_scene()

	var result := {
		"godot_version": Engine.get_version_info().string,
		"scene": _get_arg(SCENE_ARG),
		"nodes": _capture_all_nodes(root)
	}

	var json_str := JSON.stringify(result, "\t")
	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if file == null:
		printerr("editor_inspector_probe: cannot open ", output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()

	print("editor_inspector_probe: wrote ", output_path)
	quit(0)

func _load_scene() -> void:
	var scene_path := _get_arg(SCENE_ARG)
	if not scene_path.is_empty():
		var packed := ResourceLoader.load(scene_path) as PackedScene
		if packed:
			var instance := packed.instantiate()
			root.add_child(instance)

func _capture_all_nodes(node: Node) -> Array:
	var nodes := []
	_walk_tree(node, nodes)
	return nodes

func _walk_tree(node: Node, out: Array) -> void:
	out.append(_capture_node(node))
	for child in node.get_children():
		_walk_tree(child, out)

func _capture_node(node: Node) -> Dictionary:
	var props := []
	var prop_list := node.get_property_list()

	for prop in prop_list:
		# Filter to inspector-visible properties only
		var usage: int = prop.get("usage", 0)
		if not (usage & PROPERTY_USAGE_EDITOR):
			continue

		var name: String = prop.get("name", "")
		if name.is_empty():
			continue

		var value = node.get(name)
		var default_value = ClassDB.class_get_property_default_value(node.get_class(), name)
		var is_default: bool = typeof(value) == typeof(default_value) and value == default_value

		var entry := {
			"name": name,
			"class_name": prop.get("class_name", ""),
			"type": prop.get("type", 0),
			"type_name": type_string(prop.get("type", 0)),
			"hint": prop.get("hint", 0),
			"hint_string": prop.get("hint_string", ""),
			"usage": usage,
			"is_default": is_default,
		}

		# Serialize the value to JSON-safe format
		entry["value"] = _serialize_value(value)

		props.append(entry)

	return {
		"path": str(node.get_path()),
		"class": node.get_class(),
		"inspector_properties": props,
	}

func _serialize_value(value) -> Variant:
	match typeof(value):
		TYPE_NIL:
			return null
		TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING:
			return value
		TYPE_VECTOR2:
			return {"x": value.x, "y": value.y}
		TYPE_VECTOR3:
			return {"x": value.x, "y": value.y, "z": value.z}
		TYPE_COLOR:
			return {"r": value.r, "g": value.g, "b": value.b, "a": value.a}
		TYPE_RECT2:
			return {"position": _serialize_value(value.position), "size": _serialize_value(value.size)}
		TYPE_TRANSFORM2D:
			return {"x": _serialize_value(value.x), "y": _serialize_value(value.y), "origin": _serialize_value(value.origin)}
		TYPE_NODE_PATH:
			return str(value)
		TYPE_STRING_NAME:
			return str(value)
		TYPE_OBJECT:
			if value == null:
				return null
			if value is Resource:
				return {"resource_class": value.get_class(), "resource_path": value.resource_path}
			return {"object_class": value.get_class()}
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
		_:
			return str(value)

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
