extends GutTest


func test_addition() -> void:
	assert_eq(2 + 2, 4)


func test_multiplication() -> void:
	assert_eq(3 * 4, 12)


func test_division() -> void:
	assert_almost_eq(1.0 / 3.0, 0.333, 0.001)
