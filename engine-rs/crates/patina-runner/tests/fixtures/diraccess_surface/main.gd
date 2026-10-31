extends Node2D

# Declared defaults are all impossible results, so no assertion can pass on a
# variable the script failed to assign.
var made = -1
var deep_exists = false
var copy_ret = -1
var files_n = -1
var files_first = "UNSET"
var cur_dir = "UNSET"
var sub_exists = false
var nope_is_null = false
var nope_err = -1
var reached_end = false

func _ready():
    # The standard save-folder idiom. Raised UndefinedFunction before this bead:
    # gdcore implemented make_dir_recursive, but nothing bound it to script.
    self.made = DirAccess.make_dir_recursive_absolute("user://a/b/c")
    self.deep_exists = DirAccess.dir_exists_absolute("user://a/b/c")

    # Something to copy.
    var w = FileAccess.open("user://x.txt", FileAccess.WRITE)
    w.store_line("payload")
    w.close()

    var d = DirAccess.open("user://a")
    self.copy_ret = d.copy("user://x.txt", "user://a/b/y.txt")

    # get_files_at lists FILES only, so the `c` directory inside `b` must not
    # appear — one entry, not two.
    var files = DirAccess.get_files_at("user://a/b")
    self.files_n = files.size()
    self.files_first = files[0]

    # change_dir then get_current_dir: the handle must actually move.
    d.change_dir("b")
    self.cur_dir = d.get_current_dir()
    # Resolved against the handle's NEW directory, so a bare name finds the
    # child of `b` rather than of the old `a`.
    self.sub_exists = d.dir_exists("c")

    var nope = DirAccess.open("user://nope")
    self.nope_is_null = nope == null
    self.nope_err = DirAccess.get_open_error()
    self.reached_end = true
