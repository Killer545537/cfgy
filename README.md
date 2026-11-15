# cfgy

Derive config types from TOML, JSON, or YAML files — with the file checked at
**compile time** and parsed at **runtime**.

## What it does

Point a struct at a config file. The macro reads that file *during compilation*
and type-checks every value in it against your types (nested structs and enums
included), so a typo in `config.toml` is a build error rather than a startup
panic. The actual parse still happens at runtime, so the file can change without
a rebuild.

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
#[config(rename_all = "lowercase")]
enum Level {
    Debug,
    Info,
    Warn,
}

#[derive(FromConfig)]
#[config(path = "config.toml")]
struct Settings {
    host: String,
    port: u16,
    #[config(default = 4)]
    workers: usize,
    level: Level,
    database: Database,
}

fn main() -> Result<(), cfgy::ConfigError> {
    let settings = Settings::load()?;
    println!("listening on {}:{}", settings.host, settings.port);
    Ok(())
}
```

Given this `config.toml`:

```toml
host = "0.0.0.0"
port = "eighty"
level = "info"

[database]
url = "postgres://localhost/app"
pool_size = -1
```

the build fails with one error per wrong value:

```text
error[E0080]: evaluation panicked: config.toml:2:8: `port`: expected u16, found string
error[E0080]: evaluation panicked: config.toml:7:13: `database.pool_size`: -1 is out of range for u32
```

Nested types derive `FromConfig` without a `path`. Only a struct with `path`
gets loading methods.

## Attributes

Structs:

| Attribute | Meaning |
|---|---|
| `path = "config.toml"` | The config file. Generates `load`, `load_from`, `load_str`. Needs named fields. |
| `format = "yaml"` | Force a format: `toml`, `json`, `yaml`, `yml`. Requires `path`. |
| `check = false` | Skip the compile-time check (file generated in CI, templated at deploy). Requires `path`. |
| `rename_all = "camelCase"` | Convert every field name with one of serde's case rules. |

Fields (of a struct or a struct variant):

| Attribute | Meaning |
|---|---|
| `rename = "type"` | Read the field from a different key. |
| `default` | `Default::default()` when the key is absent. |
| `default = <expr>` | `<expr>` when the key is absent. A string literal works for `String` and `PathBuf`, a `T` for `Option<T>`. |
| `unchecked` | Accept any value in the compile-time check; still converted at runtime. |

Enums and variants:

| Attribute | Where | Meaning |
|---|---|---|
| `tag = "type"` | enum | Internally tagged: `{ type = "Serve", host = "h" }`. |
| `tag = "t", content = "c"` | enum | Adjacently tagged: `{ t = "Port", c = 8080 }`. |
| `untagged` | enum | The first variant whose payload matches. |
| `rename_all = "snake_case"` | enum | Convert every variant name. |
| `rename = "on"` | variant | Write the variant under a different name. |
| `rename_all = "kebab-case"` | variant | Convert the field names of a struct variant. |

The case rules are serde's: `lowercase`, `UPPERCASE`, `PascalCase`,
`camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`,
`SCREAMING-KEBAB-CASE`. Keys default to the field name; raw identifiers drop the
prefix, so `r#type` reads `type`. Unknown keys in the file are ignored.

An absent key uses the field's `default`, else the type decides through
`FromValue::from_missing`: `Option<T>` is `None`, anything else is an error.
That makes `type MaybePort = Option<u16>` optional too.

## Enums

Unit, newtype, tuple, and struct variants, in serde's four representations:

| Representation | Unit variant | Other variants |
|---|---|---|
| externally tagged (default) | `"Off"` | `{ Port = 8080 }` |
| internally tagged, `tag = "type"` | `{ type = "Off" }` | `{ type = "Serve", host = "h" }` |
| adjacently tagged, `tag` + `content` | `{ t = "Off" }` | `{ t = "Port", c = 8080 }` |
| `untagged` | null | the payload alone |

Tuple variants cannot be internally tagged (a compile error, as in serde). In
YAML, `serde_yaml`'s `!Variant value` tags read as externally tagged variants:
`port: !Port 8080`. Two deviations from serde: an internally tagged newtype
variant receives the whole table, tag included; and one whose inner type is not
a table (a map, another enum) can load at runtime but is rejected by the
compile-time check (mark the field `unchecked`).

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

## Located errors

Runtime errors name the full key path and where the value is, as
`file:line:col` from `load` and `load_from`, and `line:col` from `load_str`:

```text
config.toml:7:13: `database.pool_size`: -1 is out of range for u32
config.toml:5:1: missing required key `database.url`
config.toml:3:9: `level`: unknown variant `trace` of Level, expected one of `debug`, `info`, `warn`
```

Type, range, length, and unknown-variant errors point at the value; a missing
key points at the table it is missing from; parse errors point at the syntax
error. The position is `ConfigError`'s `location` field, a `cfgy::Location`.

## Formats

| Feature | Format | Default |
|---|---|---|
| `toml` | TOML | yes |
| `json` | JSON | no |
| `yaml` | YAML (via `saphyr`) | no |

A file's format is chosen by, in order: the `format` attribute; the extension
(`.toml`, `.json`, `.yaml`, `.yml`, in any letter case); and, only for files
with no extension, content sniffing (leading `{` is JSON, leading `---` is
YAML). An unknown extension is an error, never a guess. An unknown `format` name,
or one whose feature is disabled, is a build error.

## Conversion

Supported field types: all integer types, `f32`, `f64`, `bool`, `String`,
`PathBuf`, `char`, `cfgy::Datetime`, `Option<T>`, `Vec<T>`, `VecDeque<T>`,
`HashSet<T>`, `HashMap<String, T>`, `BTreeMap<String, T>`,
`IndexMap<String, T>`, tuples up to 4, `Box`/`Rc`/`Arc<T>`, type aliases of
any of these, and any type implementing `FromValue`: every derived struct and
enum does. Derived newtype structs are transparent, tuple structs read a
sequence of exactly their length, and unit structs read null.

Coercion is strict:

- Integers are range-checked, never wrapped (`70000` into `u16` is an error).
  Values are held as `i128`, so `u64` accepts its full range where the format
  can write it (TOML integers are 64-bit signed by spec).
- Integers widen into floats, rounding like serde.
- No float → integer (even `1.0`), string → number, or integer → bool.
- A TOML datetime loads into `Datetime` or `String`. A string loads into
  `Datetime` only if it is a valid TOML/RFC 3339 datetime (the grammar is
  checked, not the calendar).
- A present `null` is `None` for `Option` and a type error for a required field.

## Typed compile-time check

The derive parses the file at build time and checks every value against
`<Settings as FromValue>::SHAPE`, a const description of what the type accepts,
by const evaluation. The compiler resolves that shape, so nested structs, enums,
newtypes, and type aliases are checked by what they are, with the same rules as
runtime conversion. Every wrong value is reported as its own
``file:line:col: `path`: message`` error (see the quick start).

Left to runtime:

- Custom `FromValue` impls default to `Shape::Any` (accept anything); an impl
  can set `SHAPE` to be checked.
- A field whose type mentions its own struct (`children: Vec<Node>`) is not
  checked, so recursive types build. Mutually recursive types fail with a cycle
  error (E0391) until one field of the cycle is marked `#[config(unchecked)]`.
- `#[config(unchecked)]` fields.

Editing the config file triggers a rebuild (the derive emits an `include_str!`
of it), so the check never validates a stale file.

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

- **The check reflects the build machine only.** It is a fast feedback loop,
  not a guarantee about production; `load()` re-checks everything at runtime.
- **No generic structs or enums.**
- **Unknown keys are ignored.** There is no `deny_unknown_keys`.
- **No `flatten`, `alias`, or `skip`.**
- **No layered configs or environment-variable overrides.**
- **`Datetime` is text only.** Parse it with a date crate for arithmetic.
- **Large integers become floats.** JSON integers above `u64::MAX` (as in
  `serde_json`) and YAML integers above `i64::MAX`.
- **Mutual recursion needs `#[config(unchecked)]`** on one field of the cycle.
- **Check errors render differently across rustc versions.** Stable prints
  `evaluation panicked: <message>`, Rust 1.85 `the evaluated program panicked
  at '<message>'`.
- **One const per config value.** Compile time grows with very large checked
  files; use `check = false` for generated configs with thousands of values.

## Workspace layout

| Crate | Role |
|---|---|
| [`cfgy`](cfgy) | Facade. The only crate you depend on. |
| [`cfgy-core`](cfgy-core) | Value IR, `FromValue`, `Shape`, `ConfigError`, the `Format` trait, the const check. |
| [`cfgy-formats`](cfgy-formats) | Feature-gated TOML / JSON / YAML adapters, with value spans. |
| [`cfgy-derive`](cfgy-derive) | The `FromConfig` proc macro. |

## Development

```sh
cargo build --workspace --all-features --all-targets          # build
cargo nextest run --workspace --all-features                  # tests
cargo test --workspace --all-features --doc                   # doctests
cargo +stable test -p cfgy --test ui                          # trybuild UI tests
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check                                              # licenses and advisories
```

The UI cases for the compile-time check's const-evaluation errors are blessed
on stable and run only with a stable rustc, since their rendering differs
between versions.

The full design, including the rationale behind the value IR and the plan/codegen
split, lives in [`.docs/DESIGN.md`](.docs/DESIGN.md).

## License

MIT. See [LICENSE](LICENSE).

## Status

| | |
|---|---|
| **MSRV** | 1.85 (Rust 2024 edition) |
