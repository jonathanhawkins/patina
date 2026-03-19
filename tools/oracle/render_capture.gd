## Oracle capture: Render Capture
## Captures a viewport frame as PNG after N frames.
## Usage:
##   godot --path <project> -s res://render_capture.gd -- --output <path> --frame 5
extends SceneTree

const OUTPUT_ARG := "--output"
const FRAME_ARG := "--frame"

var _frame_count: int = 0
var _capture_frame: int = 5

func _init() -> void:
	call_deferred("_setup")

func _setup() -> void:
	var frame_str := _get_arg(FRAME_ARG)
	if not frame_str.is_empty():
		_capture_frame = int(frame_str)

func _process(delta: float) -> bool:
	_frame_count += 1
	if _frame_count >= _capture_frame:
		call_deferred("_do_capture")
		return true
	return false

func _do_capture() -> void:
	var output_path := _get_arg(OUTPUT_ARG)
	if output_path.is_empty():
		output_path = "user://render_capture.png"

	var viewport := root
	if viewport == null:
		printerr("render_capture: no root viewport")
		quit(1)
		return

	# Wait one more frame for rendering to complete.
	await process_frame

	var image := viewport.get_texture().get_image()
	if image == null:
		printerr("render_capture: failed to get viewport image")
		quit(1)
		return

	var err := image.save_png(output_path)
	if err != OK:
		printerr("render_capture: failed to save PNG: ", err)
		quit(1)
		return

	# Also save metadata JSON alongside the PNG.
	var meta_path := output_path.replace(".png", "_meta.json")
	var meta := {
		"output_path": output_path,
		"capture_frame": _capture_frame,
		"viewport_size": {"width": viewport.size.x, "height": viewport.size.y},
		"image_size": {"width": image.get_width(), "height": image.get_height()},
		"image_format": image.get_format(),
	}
	var json_str := JSON.stringify(meta, "\t")
	var file := FileAccess.open(meta_path, FileAccess.WRITE)
	if file != null:
		file.store_string(json_str)
		file.close()

	print("render_capture: saved ", output_path, " (", image.get_width(), "x", image.get_height(), ")")
	quit(0)

func _get_arg(arg_name: String) -> String:
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == arg_name and i + 1 < args.size():
			return args[i + 1]
	return ""
