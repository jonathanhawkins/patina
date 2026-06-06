## Oracle capture: Editor Scene Tree Probe
## Captures the scene-tree ground truth for a scene — exact node hierarchy,
## classes, owners, and instanced scene paths as Godot 4.6.1 sees them.
##
## Usage:
##   godot --headless --path <project> \
##     -s res://tools/oracle/editor_scene_tree_probe.gd -- \
##     --output <out.json> --scene res://main.tscn
##
## Output format (JSON):
##   {
##     "godot_version": "4.6.1.stable",
##     "scene": "res://main.tscn",
##     "tree": {
##       "path": "/root/World",
##       "node_class": "Node2D",
##       "name": "World",
##       "owner": "",
##       "scene_file_path": "",
##       "children": [
##         { "path": "/root/World/Player", "node_class": "CharacterBody2D", ... }
##       ]
##     }
##   }
extends SceneTree

const OUTPUT_ARG := "--output"
const SCENE_ARG := "--scene"

func _init() -> void:
	call_deferred("_capture")

func _capture() -> void:
	var output_path := _get_arg(OUTPUT_ARG)
	if output_path.is_empty():
		output_path = "user://editor_scene_tree_probe.json"
	var scene_path := _get_arg(SCENE_ARG)
	if scene_path.is_empty():
		push_error("missing --scene argument")
		quit(1)
		return

	var scene_resource := load(scene_path) as PackedScene
	if not scene_resource:
		push_error("failed to load scene: %s" % scene_path)
		quit(1)
		return

	var instance := scene_resource.instantiate()
	root.add_child(instance)

	var result := {
		"godot_version": Engine.get_version_info().string,
		"scene": scene_path,
		"tree": _serialize(instance, "/root/%s" % instance.name)
	}

	var json_str := JSON.stringify(result, "\t")
	var file := FileAccess.open(output_path, FileAccess.WRITE)
	if not file:
		push_error("failed to open output: %s" % output_path)
		quit(1)
		return
	file.store_string(json_str)
	file.close()
	print("scene_tree_probe wrote: %s" % output_path)
	quit()

func _serialize(node: Node, path: String) -> Dictionary:
	var entry := {
		"path": path,
		"node_class": node.get_class(),
		"name": String(node.name),
		"owner": (String(node.owner.name) if node.owner else ""),
		"scene_file_path": node.scene_file_path,
		"children": []
	}
	for child in node.get_children():
		entry.children.append(_serialize(child, "%s/%s" % [path, child.name]))
	return entry

func _get_arg(name: String) -> String:
	var args := OS.get_cmdline_args()
	for i in range(args.size() - 1):
		if args[i] == name:
			return args[i + 1]
	return ""
