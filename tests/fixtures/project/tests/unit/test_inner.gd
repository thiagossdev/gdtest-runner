extends GutTest


func test_outer() -> void:
	assert_true(true)


class Helper:
	func test_looks_like_a_test_but_is_not() -> void:
		pass


class TestFirst extends GutTest:
	func test_a() -> void:
		assert_true(true)

	func test_b() -> void:
		assert_true(true)

	func _nested() -> void:
		var inner_lambda := func test_lambda(): pass


class TestSecond:
	extends GutTest

	func test_c() -> void:
		assert_true(true)


class TestNotGut:
	extends RefCounted

	func test_d() -> void:
		pass
