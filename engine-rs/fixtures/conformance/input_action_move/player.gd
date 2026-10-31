extends CharacterBody2D

# Pressing the action from code (rather than from a real device) keeps the run
# deterministic on both engines while still going through the project's
# `[input]` action map rather than a direct key check.

var readout: Label

func _ready() -> void:
	readout = get_node("../HUD/Readout")
	Input.action_press("move_right")
	print("[T] pressed=", Input.is_action_pressed("move_right"))

func _physics_process(_delta: float) -> void:
	if Input.is_action_pressed("move_right"):
		velocity.x = 60.0
	else:
		velocity.x = 0.0
	move_and_slide()
	readout.text = "x=%d" % int(position.x)

func _process(_delta: float) -> void:
	pass
