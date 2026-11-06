extends GutTest

## user:// must live under the data dir the runner gave this process, and
## start empty: no file leaks in from another process.

const MARKER := "user://gdtest_ci_marker.txt"


func test_user_dir_is_under_the_isolated_data_dir() -> void:
	var data_dir := OS.get_environment("XDG_DATA_HOME")
	assert_ne(data_dir, "")
	assert_true(
		ProjectSettings.globalize_path("user://").begins_with(data_dir),
		"user:// is %s, XDG_DATA_HOME is %s" % [ProjectSettings.globalize_path("user://"), data_dir]
	)


func test_user_dir_starts_empty() -> void:
	assert_false(FileAccess.file_exists(MARKER))
	var file := FileAccess.open(MARKER, FileAccess.WRITE)
	assert_not_null(file)
	file.store_string("written by one process")
	file.close()
	assert_true(FileAccess.file_exists(MARKER))
