extends Node2D

var reported := false

func _ready() -> void:
	print("[T] main_ready children=", get_child_count())

func _process(_delta: float) -> void:
	if reported:
		return
	var spawned := get_node_or_null("Spawned")
	if spawned != null:
		reported = true
		print("[T] spawned_present children=", get_child_count())
