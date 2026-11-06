extends GutTest

# func test_commented_out() is not a test


func before_each() -> void:
	pass


func test_one() -> void:
	assert_true(true)


func test_two(params = use_parameters([1, 2])) -> void:
	assert_true(params > 0)


func helper_test_not_a_test() -> void:
	pass
