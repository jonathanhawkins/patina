extends Control
## Title screen: shows the best score and starts a run on Space/Enter or a
## click on Start.

var _leaving := false


func _ready() -> void:
	$Menu/Best.text = "Best: %d" % Game.high_score
	$Menu/Start.pressed.connect(_start)


func _process(_delta: float) -> void:
	if Input.is_action_just_pressed("ui_accept") or Input.is_action_just_pressed("pulse"):
		_start()


func _start() -> void:
	if _leaving:
		return
	_leaving = true
	get_tree().change_scene_to_file("res://scenes/main.tscn")
