# cfgy

Derive config structs from TOML, JSON, or YAML files — with the file checked at
**compile time** and parsed at **runtime**.

> [!warning]
> Early development. Nothing here works yet; the API below is the target design,
> not a description of what is implemented.

## What it does

Point a struct at a config file. The macro reads that file *during compilation*
and type-checks it against your struct, so a typo in `config.toml` is a build
error rather than a startup panic. The actual parse still happens at runtime, so
the file can change without a rebuild.

```rust
use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "config.toml")]
struct Settings {
    host: String,
    port: u16,
    #[config(default = 4)]
    workers: usize,
}

let settings = Settings::load()?;
```

Given a `config.toml` with `port = "eighty"`, this fails to compile.

## Why

Config mistakes usually surface at startup, in the environment least convenient
for finding them. Moving the check to compile time catches the common cases —
missing keys, wrong types, malformed files — while the developer still has the
file open.

The compile-time check runs on the build machine against the build machine's
filesystem. It is a fast feedback loop, not a guarantee about production;
`load()` still handles every failure independently at runtime.

## Workspace layout

| Crate | Role |
|---|---|
| [`cfgy`](cfgy) | Facade. The only crate you depend on. |
| [`cfgy-core`](cfgy-core) | Value IR, `FromValue`, `ConfigError`, the `Format` trait. |
| [`cfgy-formats`](cfgy-formats) | Feature-gated TOML / JSON / YAML adapters. |
| [`cfgy-derive`](cfgy-derive) | The `FromConfig` proc macro. |

## Development

```sh
cargo build --workspace          # build
cargo test --workspace           # test
cargo fmt --all --check          # formatting
cargo clippy --all-targets       # lints (deny-heavy; see [workspace.lints])
cargo deny check                 # dependency licenses and advisories
```

The full design, including the rationale behind the value IR and the plan/codegen
split, lives in [`.docs/DESIGN.md`](.docs/DESIGN.md).

## License

MIT. See [LICENSE](LICENSE).

## Status

| | |
|---|---|
| **MSRV** | 1.85 (Rust 2024 edition) |
