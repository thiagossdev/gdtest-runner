extends GutTest

## Fails on purpose: the CI job checks that the runner reports it.


func test_passes() -> void:
	assert_true(true)


func test_fails_on_purpose() -> void:
	assert_eq(1, 2, "this failure is expected")
