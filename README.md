# cfgy

Derive config structs from TOML, JSON, or YAML files — with the file checked at
**compile time** and parsed at **runtime**.

## What it does

Point a struct at a config file. The macro reads that file *during compilation*
and type-checks it against your struct, so a typo in `config.toml` is a build
error rather than a startup panic. The actual parse still happens at runtime, so
the file can change without a rebuild.

```toml
# Cargo.toml
[dependencies]
cfgy = { git = "https://github.com/Killer545537/cfgy" }   # TOML only
# cfgy = { git = "https://github.com/Killer545537/cfgy", features = ["json", "yaml"] }
```

```rust
use cfgy::FromConfig;

#[derive(FromConfig)]
struct Database {
    url: String,
    pool_size: Option<u32>,
}

#[derive(FromConfig)]
#[config(path = "config.toml")]
struct Settings {
    host: String,
    port: u16,
    #[config(default = 4)]
    workers: usize,
    database: Database,
}

fn main() -> Result<(), cfgy::ConfigError> {
    let settings = Settings::load()?;
    println!("listening on {}:{}", settings.host, settings.port);
    Ok(())
}
```

Given a `config.toml` with `port = "eighty"`, this fails to compile:

```text
error: config.toml: `port`: expected u16, found string
```

Nested types derive `FromConfig` without a `path`. Only a struct with `path`
gets loading methods.

## Attributes

| Attribute | Where | Meaning |
|---|---|---|
| `path = "config.toml"` | struct | The config file. Generates `load`, `load_from`, `load_str`. |
| `format = "yaml"` | struct | Force a format: `toml`, `json`, `yaml`, `yml`. Requires `path`. |
| `check = false` | struct | Skip the compile-time check (file generated in CI, templated at deploy). Requires `path`. |
| `rename = "type"` | field | Read the field from a different key. |
| `default = <expr>` | field | Evaluate `<expr>` when the key is absent. |

Keys default to the field name; raw identifiers drop the prefix, so `r#type`
reads `type`. Unknown keys in the file are ignored.

## Loading

```rust
impl Settings {
    pub fn load() -> Result<Self, ConfigError>;                      // `path`, relative to the working directory
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, ConfigError>;
    pub fn load_str(source: &str, format: &dyn Format) -> Result<Self, ConfigError>;
}
```

`load()` resolves `path` against the **process working directory**; the
compile-time check resolves it against `CARGO_MANIFEST_DIR`. Override the file
at deploy time with `load_from` — the build still checks the repo's default:

```rust
let settings = match std::env::var("APP_CONFIG") {
    Ok(path) => Settings::load_from(path)?,
    Err(_) => Settings::load()?,
};
```

`load_str` parses a string with a given format (`&cfgy::Toml`, `&cfgy::Json`,
`&cfgy::Yaml`), which keeps tests off the filesystem.

Runtime errors name the full key path, e.g.
`` `replicas[1].port`: expected u16, found string ``.

## Formats

| Feature | Format | Default |
|---|---|---|
| `toml` | TOML | yes |
| `json` | JSON | no |
| `yaml` | YAML (via `saphyr`) | no |

A file's format is chosen by, in order: the `format` attribute; the extension
(`.toml`, `.json`, `.yaml`, `.yml`); and, only for files with no extension,
content sniffing (leading `{` is JSON, leading `---` is YAML). An unknown
extension is an error, never a guess.

## Conversion

Supported field types: all integer types, `f32`, `f64`, `bool`, `String`,
`PathBuf`, `char`, `cfgy::Datetime`, `Option<T>`, `Vec<T>`, `VecDeque<T>`,
`HashSet<T>`, `HashMap<String, T>`, `BTreeMap<String, T>`,
`IndexMap<String, T>`, tuples up to 4, `Box`/`Rc`/`Arc<T>`, and any type
implementing `FromValue` (every derived struct does).

Coercion is strict:

- Integers are range-checked, never wrapped (`70000` into `u16` is an error).
- Integers widen to floats only when exact: within `i32` for `f64`, `i16` for `f32`.
- No float → integer (even `1.0`), string → number, or integer → bool.
- An absent key is `None` for `Option`, the default for `default`, otherwise an error.
- A present `null` is `None` for `Option` and a type error for a required field.

## Validation

`load()` returns a well-typed struct, not a validated one. Pair it with the
[`validator`](https://crates.io/crates/validator) crate — the two derives are
independent:

```rust
#[derive(FromConfig, Validate)]
#[config(path = "config.toml")]
struct Settings {
    #[validate(length(min = 1))]
    host: String,
    #[validate(range(min = 1024))]
    port: u16,
}

let settings = Settings::load()?;
settings.validate()?;
```

## Known limitations

- **The compile-time check is shallow.** Scalars, collections, `Option`, and
  smart pointers are checked fully. Any other type — a nested struct, a type
  alias like `type Port = u16`, a custom `FromValue` impl — is only checked to
  be a table. Mistakes inside nested structs surface at runtime, and a struct
  with a custom non-table field type needs `check = false`.
- **The check reflects the build machine only.** It is a fast feedback loop,
  not a guarantee about production; `load()` re-checks everything at runtime.
- **`Option` is recognised syntactically.** `type MaybePort = Option<u16>` is a
  required field.
- **`default` is a plain expression.** `default = "localhost"` on a `String`
  does not compile; write `default = "localhost".into()`.
- **Structs only.** Enums and generic structs are not supported.
- **No line numbers in conversion errors.** Parse errors carry line and column;
  type errors after parsing carry only the key path.

Editing the config file triggers a rebuild (the derive emits an `include_str!`
of it), so the check never validates a stale file.

## Workspace layout

| Crate | Role |
|---|---|
| [`cfgy`](cfgy) | Facade. The only crate you depend on. |
| [`cfgy-core`](cfgy-core) | Value IR, `FromValue`, `ConfigError`, the `Format` trait. |
| [`cfgy-formats`](cfgy-formats) | Feature-gated TOML / JSON / YAML adapters. |
| [`cfgy-derive`](cfgy-derive) | The `FromConfig` proc macro. |

## Development

```sh
cargo build --workspace --all-features --all-targets          # build
cargo nextest run --workspace --all-features                  # tests
cargo test --workspace --all-features --doc                   # doctests
cargo test -p cfgy --test ui                                  # trybuild UI tests
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check                                              # licenses and advisories
```

The full design, including the rationale behind the value IR and the plan/codegen
split, lives in [`.docs/DESIGN.md`](.docs/DESIGN.md).

## License

MIT. See [LICENSE](LICENSE).

## Status

| | |
|---|---|
| **MSRV** | 1.85 (Rust 2024 edition) |
