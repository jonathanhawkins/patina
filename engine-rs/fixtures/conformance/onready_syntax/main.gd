extends Node2D

@onready var child = $Child
@onready var label: Label = $Label

var enabled := true
var blocked := false
var ticks := 0

func _ready() -> void:
	print("[T] child_name=", child.name)
	print("[T] child_x=", child.position.x)
	if !blocked && enabled:
		print("[T] bang_and_ok")
	if blocked || enabled:
		print("[T] or_ok")
	var count := 3
	var scale_factor := 0.5
	print("[T] inferred=", count, ",", scale_factor)
	label.text = "ready"

func _process(_delta: float) -> void:
	ticks += 1
	if ticks == 2:
		label.text = "tick2"
		print("[T] label_text=", label.text)
