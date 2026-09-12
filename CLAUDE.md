# Development Standards

## Toolchain

`cargo build` is the primary tool for building the project. It compiles the Rust code and produces the necessary binaries.
`cargo nextest` is used for running tests. It executes the test suite and provides feedback on the results.

## Git

- Commit messages should be clear and descriptive, following the format: `type(scope): description`. For example, `feat(parser): add support for new syntax`.
- Never add co-authored commits. Each commit should be authored by a single individual to maintain clarity in the project's history.
