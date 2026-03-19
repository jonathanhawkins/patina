## Oracle capture: Scene Tree Dumper
## Walks entire scene tree and outputs JSON with node hierarchy.
## Usage:
##   godot --headless --path <project> -s res://scene_tree_dumper.gd -- --output <path> --scene res://scenes/main.tscn
extends SceneTree

const OUTPUT_ARG := "--output"
const SCENE_ARG := "--scene"

func _init() -> void:
	call_deferred("_capture")

func _capture() -> void:
	var output_path := _get_arg(OUTPUT_ARG)
	if output_path.is_empty():
		output_path = "user://scene_tree_dump.json"

	# Load the target scene if specified, otherwise use project main scene.
	var scene_path := _get_arg(SCENE_ARG)
	if not scene_path.is_empty():
		var packed := ResourceLoader.load(scene_path) as PackedScene
		if packed == null:
			printerr("scene_tree_dumper: cannot load scene ", scene_path)
			quit(1)
			return
		var instance := packed.instantiate()
		root.add_child(instance)
		instance.owner = root
	elif root.get_child_count() == 0:
		# Try to load main scene from project settings.
		var main_scene: String = ProjectSettings.get_setting("application/run/main_scene", "")
		if not main_scene.is_empty():
			var packed := ResourceLoader.load(main_scene) as PackedScene
			if packed != null:
				var instance := packed.instantiate()
				root.add_child(instance)
				instance.owner = root

	var data := _dump_node(root)
	var json_str := JSON.stringify(data, "\t")

	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if file == null:
		printerr("scene_tree_dumper: cannot open ", output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()

	print("scene_tree_dumper: wrote ", output_path)
	quit(0)

func _dump_node(node: Node) -> Dictionary:
	var children_arr: Array[Dictionary] = []
	for child in node.get_children():
		children_arr.append(_dump_node(child))

	var groups: Array[String] = []
	for g in node.get_groups():
		groups.append(str(g))

	var result := {
		"name": node.name,
		"class": node.get_class(),
		"path": str(node.get_path()),
		"owner": str(node.owner.get_path()) if node.owner else "",
		"groups": groups,
		"children": children_arr,
	}

	var script = node.get_script()
	if script != null and script is Script:
		result["script"] = script.resource_path

	return result

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
