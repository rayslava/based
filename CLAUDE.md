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
  after introducing changes into sources files.

# Build process

Only use the `make` command.

# Tests

Run `cargo test`
