extends GutTest


func test_upper() -> void:
	assert_eq("abc".to_upper(), "ABC")


func test_split() -> void:
	var parts := "a,b,c".split(",")
	assert_eq(parts.size(), 3)
	assert_eq(parts[1], "b")


class TestPadding:
	extends GutTest

	func test_pad_zeros() -> void:
		assert_eq("7".pad_zeros(3), "007")
