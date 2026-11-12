extends Control
## Shown when the last life is lost. Input is ignored for a moment so the key
## that was held when the run ended does not skip the screen.

const INPUT_DELAY := 0.6

var _elapsed := 0.0


func _ready() -> void:
	$Panel/Score.text = "Sparks gathered: %d" % Game.score
	$Panel/Best.text = "Best: %d" % Game.high_score
	if Game.score >= Game.high_score and Game.score > 0:
		$Panel/Best.text += "  (new!)"


func _process(delta: float) -> void:
	_elapsed += delta
	if _elapsed < INPUT_DELAY:
		return
	if Input.is_action_just_pressed("ui_accept") or Input.is_action_just_pressed("pulse"):
		get_tree().change_scene_to_file("res://scenes/title.tscn")
