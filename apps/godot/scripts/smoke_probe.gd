extends Node

func _ready() -> void:
	var probe := PatinaSmokeProbe.new()
	probe.name = "PatinaSmokeProbe"
	add_child(probe)
	probe.run_smoke_probe()
	get_tree().quit()
