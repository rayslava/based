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
- Use the `make size` command to check that release build succeeds and does not
  increase resulting file dramatically.
- Under any circumstances do not use or import `std` and `core` crates. Do not
  use types from there, do not write code using such types.
- It is possible to use `std` in test module though. But only inside the test
  module and nowhere else.
- Write all the code in a statement-oriented style.
- Run `make test` after finishing changes to check that tests still pass.
- Never exclude the newly created functions from the test coverage.
- Never disable any clippy checks and warnings, only fix the code.
- Never ever add #[allow(clippy::*)] for the code that is used out of the
  tests. Never allow any warnings in production code.
- Never do some additional or supplementary tasks, only complete the explicit
  requests.
- Never use the #[allow(dead_code)]. Eliminate all the unneded code.
- Always write the most idiomatic and short Rust code, use pattern matching,
  write the statement-based code. Split the code into small and easy-readble
  functions.

# Build process

Only use the `make` command.

# Tests

Run `cargo test`
