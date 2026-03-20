extends Node

func _ready() -> void:
	var probe := PatinaSmokeProbe.new()
	probe.name = "PatinaSmokeProbe"
	add_child(probe)

	# Run smoke probes (scene tree, properties, signals)
	probe.run_smoke_probe()

	# Run ClassDB probe for 17 core classes
	probe.run_classdb_probe()

	# Probe resources (add fixture .tres files to res://fixtures/ for more coverage)
	var fixtures := [
		"res://scenes/smoke_probe.tscn",
	]
	for path in fixtures:
		if ResourceLoader.exists(path):
			probe.run_resource_probe(path)

	get_tree().quit()
