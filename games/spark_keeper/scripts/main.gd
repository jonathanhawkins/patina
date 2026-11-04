extends Node2D
## The arena: spawns sparks and gloom, routes the player's pulse to every
## gloom, keeps the HUD current, and ends the run when the last life goes.

const SPARK := preload("res://scenes/spark.tscn")
const GLOOM := preload("res://scenes/gloom.tscn")
const MAX_SPARKS := 6
const SIZE := Vector2(640, 360)

@onready var player: CharacterBody2D = $Player
@onready var sparks: Node2D = $Sparks
@onready var glooms: Node2D = $Glooms
@onready var score_label: Label = $HUD/Bar/Score
@onready var lives_label: Label = $HUD/Bar/Lives
@onready var pulse_bar: ProgressBar = $HUD/Bar/Pulse


func _ready() -> void:
	Game.reset()
	Game.score_changed.connect(_on_score_changed)
	Game.lives_changed.connect(_on_lives_changed)
	Game.game_over.connect(_on_game_over)
	player.pulsed.connect(_on_player_pulsed)
	$SparkTimer.timeout.connect(_spawn_spark)
	$GloomTimer.timeout.connect(_spawn_gloom)
	_on_score_changed(Game.score)
	_on_lives_changed(Game.lives)
	for i in 3:
		_spawn_spark()


func _process(_delta: float) -> void:
	pulse_bar.value = 1.0 - player.cooldown / player.PULSE_COOLDOWN


func _spawn_spark() -> void:
	if sparks.get_child_count() >= MAX_SPARKS:
		return
	var spark := SPARK.instantiate()
	spark.position = Vector2(Game.rng.randf_range(48.0, SIZE.x - 48.0), Game.rng.randf_range(48.0, SIZE.y - 48.0))
	sparks.add_child(spark)


func _spawn_gloom() -> void:
	var gloom := GLOOM.instantiate()
	var edge := Game.rng.randi_range(0, 3)
	var along := Game.rng.randf()
	match edge:
		0:
			gloom.position = Vector2(along * SIZE.x, -16.0)
		1:
			gloom.position = Vector2(SIZE.x + 16.0, along * SIZE.y)
		2:
			gloom.position = Vector2(along * SIZE.x, SIZE.y + 16.0)
		_:
			gloom.position = Vector2(-16.0, along * SIZE.y)
	gloom.speed = 50.0 + Game.score * 2.5
	glooms.add_child(gloom)
	$GloomTimer.wait_time = maxf(0.8, 3.0 - Game.score * 0.08)


func _on_player_pulsed(origin: Vector2, radius: float) -> void:
	for gloom in glooms.get_children():
		gloom.push_from(origin, radius)
	$PulseRing.show_at(origin, radius)


func _on_score_changed(score: int) -> void:
	score_label.text = "Sparks %d" % score


func _on_lives_changed(lives: int) -> void:
	lives_label.text = "Lives %d" % lives


func _on_game_over(_final_score: int) -> void:
	call_deferred("_show_game_over")


func _show_game_over() -> void:
	get_tree().change_scene_to_file("res://scenes/game_over.tscn")
