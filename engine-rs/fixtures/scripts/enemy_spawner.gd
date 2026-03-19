extends Node2D

var spawn_timer = 0.0
var spawn_interval = 2.0

func _process(delta):
    spawn_timer += delta
    if spawn_timer >= spawn_interval:
        spawn_timer = 0.0
        print("Spawn enemy!")
