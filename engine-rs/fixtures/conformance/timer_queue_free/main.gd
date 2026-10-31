extends Node2D

# Every observable line is prefixed with "[T] " so the conformance harness can
# pick the fixture's own output out of the engine's log on either side.

func _ready() -> void:
	print("[T] ready target=", get_node_or_null("Target") != null)
	# Fires the Button's `pressed` signal, which the .tscn connects to
	# Label.hide — a NATIVE method on a scriptless node.
	$UI/Button.pressed.emit()
	print("[T] label_visible_after_press=", $UI/Label.visible)

func _process(_delta: float) -> void:
	if get_node_or_null("Target") == null:
		print("[T] target_freed")
		set_process(false)
