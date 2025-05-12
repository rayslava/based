# The idea

The project is intended do be the very minimalistic editor, so no std is used
and no libc is used either. Only Linux syscalls.

# Design and Code Style

- The program is developed to be as small as possible in terms of resulting
  binary, so the code volume should be reduced.
- The logic that could be done in arithmetic should be done that way.
- Obvious one-line comments are strictly forbidden.
- Do not create "fake" tests that do not test the actual implementation, but
  create new test-only one.
- The test should call the function it tests, not just do empty comparison.
- The function signature though can be placed under the #[cfg(test)] and be
  changed for testing purposes if needed.
- The test module must be in the end of file. All the newly created tests are
  appended to the end of the test module.
- Always use `make fix` after introducing changes into source files.
- Always use `make check` to verify that no issues happened in source code
  after introducing changes into sources files. Always use `make fix` before
  running `make check`.
- Do not add allows for clippy in any case. Rework the code, refactor the
  functions, but do not turn off any clippy checks. Do not ever add any
  `#allow` directive to the code, this is forbidden under any circumstances.
- Do not use allocator, do not use any allocator-based features, do not add any
  features that might use allocator. Rework code in such cases so the allocator
  is not required.
- Do not ever implement `memset` or `memcpy` or other functions from this list,
  rework the code so they're not needed.
- Under any circumstances do not use or import `std` and `core` crates. Do not
  use types from there, do not write code using such types.

# Build process

Only use the `make` command.

# Tests

Run `cargo test`
