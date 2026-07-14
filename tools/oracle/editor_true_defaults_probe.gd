#!/usr/bin/env -S godot --headless --script
## pat-ghfv0.42: Dump ClassDB.class_get_property_default_value for editor-visible
## properties of representative node classes. Output is written as JSON to
## fixtures/editor_oracle/true_defaults.json (or --path=).
##
## Usage (from repo root, with Godot 4.6.1 on PATH):
##   godot --headless --path . --script tools/oracle/editor_true_defaults_probe.gd
##   godot --headless --script tools/oracle/editor_true_defaults_probe.gd -- --path=fixtures/editor_oracle/true_defaults.json

extends SceneTree

const CLASSES := [
	"Node", "Node2D", "Node3D", "Control", "Label", "VBoxContainer",
	"Camera2D", "Camera3D", "Sprite2D", "MeshInstance3D",
	"CollisionShape2D", "CollisionShape3D", "StaticBody2D", "StaticBody3D",
	"RigidBody2D", "RigidBody3D", "CharacterBody2D", "CharacterBody3D",
	"Area2D", "Area3D", "DirectionalLight3D", "OmniLight3D", "SpotLight3D",
	"AnimationPlayer", "Timer", "WorldEnvironment",
]

func _init() -> void:
	var out_path := "fixtures/editor_oracle/true_defaults.json"
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--path="):
			out_path = arg.substr(7)

	var classes := {}
	for cls in CLASSES:
		if not ClassDB.class_exists(cls):
			continue
		var props := {}
		for p in ClassDB.class_get_property_list(cls, true):
			var usage: int = p.get("usage", 0)
			if not (usage & PROPERTY_USAGE_EDITOR):
				continue
			var name: String = p.get("name", "")
			if name.is_empty():
				continue
			var dflt = ClassDB.class_get_property_default_value(cls, name)
			props[name] = {
				"type": type_string(typeof(dflt)),
				"value": _serialize(dflt),
			}
		if not props.is_empty():
			classes[cls] = props

	var doc := {
		"source": "ClassDB.class_get_property_default_value (Godot probe)",
		"probe": "tools/oracle/editor_true_defaults_probe.gd",
		"classes": classes,
	}
	var f := FileAccess.open(out_path, FileAccess.WRITE)
	if f == null:
		push_error("cannot write %s" % out_path)
		quit(1)
	f.store_string(JSON.stringify(doc, "\t"))
	f.close()
	print("wrote true_defaults for %d classes -> %s" % [classes.size(), out_path])
	quit(0)

func _serialize(value) -> Variant:
	match typeof(value):
		TYPE_NIL:
			return null
		TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING:
			return value
		TYPE_VECTOR2:
			return [value.x, value.y]
		TYPE_VECTOR2I:
			return [value.x, value.y]
		TYPE_VECTOR3:
			return [value.x, value.y, value.z]
		TYPE_COLOR:
			return [value.r, value.g, value.b, value.a]
		TYPE_RECT2:
			return {
				"position": [value.position.x, value.position.y],
				"size": [value.size.x, value.size.y],
			}
		_:
			return str(value)
