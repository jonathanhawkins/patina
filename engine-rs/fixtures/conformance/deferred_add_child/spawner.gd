extends Node2D

func _ready() -> void:
	var n := Node2D.new()
	n.name = "Spawned"
	n.position = Vector2(5, 7)
	# `add_child` is a NATIVE method reached through `call_deferred`, so the
	# node must land on the parent one frame later, not during `_ready`.
	get_parent().call_deferred("add_child", n)
	print("[T] deferred_queued")
