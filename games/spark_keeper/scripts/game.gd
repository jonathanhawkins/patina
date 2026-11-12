extends Node
## Global game state: score, lives, the saved high score, and the seeded RNG
## every spawner draws from (a fixed seed keeps playtests reproducible).

signal score_changed(score: int)
signal lives_changed(lives: int)
signal game_over(final_score: int)

const SAVE_PATH := "user://spark_keeper.cfg"
const START_LIVES := 3

var score := 0
var lives := START_LIVES
var high_score := 0
var rng := RandomNumberGenerator.new()


func _ready() -> void:
	rng.set_seed(7)
	_load_high_score()


func reset() -> void:
	score = 0
	lives = START_LIVES
	score_changed.emit(score)
	lives_changed.emit(lives)


func add_score(points: int) -> void:
	score += points
	score_changed.emit(score)


func lose_life() -> void:
	if lives <= 0:
		return
	lives -= 1
	lives_changed.emit(lives)
	if lives == 0:
		if score > high_score:
			high_score = score
			_save_high_score()
		game_over.emit(score)


func _load_high_score() -> void:
	if not FileAccess.file_exists(SAVE_PATH):
		return
	var cfg := ConfigFile.new()
	if cfg.load(SAVE_PATH) == OK:
		high_score = cfg.get_value("scores", "high", 0)


func _save_high_score() -> void:
	var cfg := ConfigFile.new()
	cfg.set_value("scores", "high", high_score)
	cfg.save(SAVE_PATH)
