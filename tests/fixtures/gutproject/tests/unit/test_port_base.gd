extends GutTest


func test_port_base_is_set() -> void:
	var base := OS.get_environment("GDTEST_PORT_BASE")
	assert_true(base.is_valid_int(), "GDTEST_PORT_BASE is '%s'" % base)
