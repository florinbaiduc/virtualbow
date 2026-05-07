# Legacy example bows

These `.bow` files are kept for reference but **excluded from the automated
test suite** because they have pre-existing schema issues that pre-date the
v5 (asymmetric-bow) refactor:

- `recurve.bow`: declares `version = "0.10.0"` (= v4 schema) but contains a
  `dimensions` block (v3 schema layout) and is missing v4 fields such as
  `static_iteration_tolerance`. It cannot be deserialized as v4.
- `screenshot_example_1.bow`: declares `version = "0.9"` but contains an
  array where a scalar is expected ("invalid type: sequence, expected f64").

If/when these are migrated to the v5 schema, move them back to `../examples/`
to re-enable end-to-end testing.
