## Oracle capture: Resource Roundtrip
## Loads a .tres file, serializes to JSON, saves, reloads, compares.
## Usage:
##   godot --headless --path <project> -s res://resource_roundtrip.gd -- --input <tres_path> --output <json_path>
extends SceneTree

const INPUT_ARG := "--input"
const OUTPUT_ARG := "--output"

func _init() -> void:
	call_deferred("_capture")

func _capture() -> void:
	var input_path := _get_arg(INPUT_ARG)
	var output_path := _get_arg(OUTPUT_ARG)

	if input_path.is_empty():
		printerr("resource_roundtrip: --input required")
		quit(1)
		return
	if output_path.is_empty():
		output_path = "user://resource_roundtrip.json"

	# Load the resource.
	var resource := ResourceLoader.load(input_path)
	if resource == null:
		printerr("resource_roundtrip: failed to load ", input_path)
		quit(1)
		return

	# Serialize to JSON.
	var original_data := _serialize_resource(resource)

	# Save to a temp path and reload.
	var temp_path := "user://roundtrip_temp.tres"
	var save_err := ResourceSaver.save(resource, temp_path)
	if save_err != OK:
		printerr("resource_roundtrip: failed to save to temp: ", save_err)
		quit(1)
		return

	var reloaded := ResourceLoader.load(temp_path)
	if reloaded == null:
		printerr("resource_roundtrip: failed to reload from temp")
		quit(1)
		return

	var reloaded_data := _serialize_resource(reloaded)

	# Compare.
	var differences := _compare_dicts(original_data, reloaded_data, "")

	var data := {
		"input_path": input_path,
		"class_name": resource.get_class(),
		"original": original_data,
		"reloaded": reloaded_data,
		"roundtrip_identical": differences.is_empty(),
		"differences": differences,
	}

	var json_str := JSON.stringify(data, "\t")
	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if file == null:
		printerr("resource_roundtrip: cannot open ", output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()

	if differences.is_empty():
		print("resource_roundtrip: PASS - roundtrip identical for ", input_path)
	else:
		print("resource_roundtrip: DIFF - ", differences.size(), " differences found for ", input_path)
	quit(0)

func _serialize_resource(res: Resource) -> Dictionary:
	var properties := {}
	for prop_info in res.get_property_list():
		var usage: int = prop_info["usage"]
		if not (usage & PROPERTY_USAGE_STORAGE):
			continue
		var prop_name: String = prop_info["name"]
		if prop_name in ["script", "resource_local_to_scene", "resource_name", ""]:
			continue
		var value = res.get(prop_name)
		properties[prop_name] = {
			"type": type_string(typeof(value)),
			"value": _serialize_value(value),
		}

	var subresources := {}
	# Check properties that are themselves resources.
	for prop_name in properties:
		var value = res.get(prop_name)
		if value is Resource and value != res:
			subresources[prop_name] = {
				"class_name": value.get_class(),
				"properties": _serialize_resource(value).get("properties", {}),
			}

	return {
		"class_name": res.get_class(),
		"properties": properties,
		"subresources": subresources,
	}

func _serialize_value(value: Variant) -> Variant:
	match typeof(value):
		TYPE_NIL:
			return null
		TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING, TYPE_STRING_NAME:
			return value
		TYPE_VECTOR2:
			return [value.x, value.y]
		TYPE_VECTOR3:
			return [value.x, value.y, value.z]
		TYPE_COLOR:
			return [value.r, value.g, value.b, value.a]
		TYPE_RECT2:
			return [value.position.x, value.position.y, value.size.x, value.size.y]
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
			return str(value)
		_:
			return str(value)

func _compare_dicts(a: Variant, b: Variant, path: String) -> Array[Dictionary]:
	var diffs: Array[Dictionary] = []
	if typeof(a) != typeof(b):
		diffs.append({"path": path, "type_a": type_string(typeof(a)), "type_b": type_string(typeof(b))})
		return diffs
	if a is Dictionary and b is Dictionary:
		var all_keys := {}
		for k in a:
			all_keys[k] = true
		for k in b:
			all_keys[k] = true
		for k in all_keys:
			var sub_path := path + "." + str(k) if not path.is_empty() else str(k)
			if not a.has(k):
				diffs.append({"path": sub_path, "issue": "missing_in_original"})
			elif not b.has(k):
				diffs.append({"path": sub_path, "issue": "missing_in_reloaded"})
			else:
				diffs.append_array(_compare_dicts(a[k], b[k], sub_path))
	elif a is Array and b is Array:
		if a.size() != b.size():
			diffs.append({"path": path, "issue": "array_size", "size_a": a.size(), "size_b": b.size()})
		for i in range(min(a.size(), b.size())):
			diffs.append_array(_compare_dicts(a[i], b[i], path + "[" + str(i) + "]"))
	elif a != b:
		# Float tolerance.
		if a is float and b is float and absf(a - b) < 1e-6:
			pass
		else:
			diffs.append({"path": path, "value_a": str(a), "value_b": str(b)})
	return diffs

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
