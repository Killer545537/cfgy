//! Derive config types from TOML, JSON, or YAML files, with the file checked at compile time and parsed at
//! runtime.
//!
//! `#[derive(FromConfig)]` maps a config file onto a struct. With `#[config(path = "...")]`, the derive reads that
//! file *during compilation* and type-checks every value in it against the struct, nested structs and enums
//! included, so `port = "eighty"` in `config.toml` is a build error. The struct also gains
//! [`load`](#generated-api), which parses the file again at runtime, so the file can change without a rebuild.
//!
//! ```rust,ignore
//! use cfgy::FromConfig;
//!
//! #[derive(FromConfig)]
//! #[config(path = "config.toml")] // resolved against CARGO_MANIFEST_DIR at compile time
//! struct Settings {
//!     host: String,
//!     port: u16,
//!     #[config(default = 4)]
//!     workers: usize,
//! }
//!
//! let settings = Settings::load()?; // resolved against the working directory at runtime
//! ```
//!
//! The same struct, parsed from a string so it runs without a file on disk:
//!
//! ```rust
//! use cfgy::FromConfig;
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! #[config(path = "config.toml", check = false)] // `check = false`: no file exists at build time here
//! struct Settings {
//!     host: String,
//!     port: u16,
//!     #[config(default = 4)]
//!     workers: usize,
//! }
//!
//! # fn main() -> Result<(), cfgy::ConfigError> {
//! let settings = Settings::load_str("host = \"localhost\"\nport = 8080\n", &cfgy::Toml)?;
//! assert_eq!(settings, Settings { host: "localhost".into(), port: 8080, workers: 4 });
//! # Ok(())
//! # }
//! ```
//!
//! # Attributes
//!
//! On a struct:
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `path = "config.toml"` | The config file. Grants the struct its loading methods. Needs named fields. |
//! | `format = "toml"` | Force a format: `toml`, `json`, `yaml`, or `yml`. Requires `path`. |
//! | `check = false` | Skip the compile-time check, for files that do not exist at build time. Requires `path`. |
//! | `rename_all = "camelCase"` | Convert every field name to a key with one of serde's case rules (below). |
//!
//! On an enum (see [Enums](#enums)):
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `tag = "type"` | Internally tagged: `{ type = "Variant", ..fields }`. |
//! | `tag = "t", content = "c"` | Adjacently tagged: `{ t = "Variant", c = payload }`. |
//! | `untagged` | No tag: the first variant, in declaration order, whose payload matches. Not with `tag`. |
//! | `rename_all = "snake_case"` | Convert every variant name with one of serde's case rules. |
//!
//! On an enum variant:
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `rename = "on"` | Write the variant under a different name. Wins over the enum's `rename_all`. |
//! | `rename_all = "kebab-case"` | Convert the field names of this struct variant. |
//!
//! On a named field, of a struct or of a struct variant:
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `rename = "type"` | Read the field from a different key. Wins over `rename_all`. |
//! | `default` | Use `Default::default()` when the key is absent. |
//! | `default = <expr>` | Use `<expr>` when the key is absent (see [Defaults](#defaults)). |
//! | `unchecked` | Accept any value in the compile-time check; the field is still converted at runtime. |
//!
//! The case rules are serde's: `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`,
//! `SCREAMING_SNAKE_CASE`, `kebab-case`, and `SCREAMING-KEBAB-CASE`. Field names are converted from `snake_case`,
//! variant names from `PascalCase`.
//!
//! The key is the field name unless renamed. Raw identifiers drop their prefix, so `r#type` reads the key `type`.
//! Unknown options, options in the wrong place (a struct option on a field, `path` on an enum, `tag` on a struct),
//! an unknown `format` name or one whose cargo feature is disabled, and two fields or variants mapping to the same
//! name are compile errors. Every such error in the derive is reported, not just the first.
//!
//! ## Defaults
//!
//! When a key is absent, a field with `default` uses it. Otherwise the field type decides through
//! [`FromValue::from_missing`]: `Option<T>` becomes `None`, and every other built-in type is a missing-key error.
//! Because this is decided by the type rather than by how the field is spelled, `type MaybePort = Option<u16>`,
//! `Box<Option<T>>`, and a newtype around an `Option` are all optional too.
//!
//! `default = <expr>` accepts any expression of the field's type, plus two conveniences: a string literal for a
//! `String` or `PathBuf` field, and a `T` for an `Option<T>` field (`default = 5` on an `Option<u64>` is
//! `Some(5)`). Integer and float literals take the field's type, as they would in a `let`.
//!
//! ```rust
//! use cfgy::FromConfig;
//! use std::path::PathBuf;
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! #[config(path = "config.toml", check = false)]
//! struct Settings {
//!     #[config(default = "localhost")]
//!     host: String,
//!     #[config(default = "/var/lib/app")]
//!     data_dir: PathBuf,
//!     #[config(default = 30)]
//!     timeout: Option<u64>,
//!     #[config(default)]
//!     tags: Vec<String>,
//! }
//!
//! let settings = Settings::load_str("", &cfgy::Toml).unwrap();
//! assert_eq!(settings.host, "localhost");
//! assert_eq!(settings.data_dir, PathBuf::from("/var/lib/app"));
//! assert_eq!(settings.timeout, Some(30));
//! assert!(settings.tags.is_empty());
//! ```
//!
//! A string literal does not convert into an `Option<String>`; write `default = String::from("x")` there. An
//! expression whose own type must be inferred, such as `"localhost".into()`, is ambiguous; drop the `.into()`.
//!
//! # Generated API
//!
//! Every derived type implements [`FromValue`], which is what lets it appear as a field, a `Vec` element, or a map
//! value inside another derived type. Nested types derive `FromConfig` without a `path`.
//!
//! A struct with `path` also gets three inherent methods:
//!
//! ```rust,ignore
//! impl Settings {
//!     pub fn load() -> Result<Self, ConfigError>;
//!     pub fn load_from(path: impl AsRef<Path>) -> Result<Self, ConfigError>;
//!     pub fn load_str(source: &str, format: &dyn Format) -> Result<Self, ConfigError>;
//! }
//! ```
//!
//! A type without `path` has none of them. Only a struct with named fields can have a `path`; an enum is always
//! loaded as part of such a struct.
//!
//! - `load()` is `load_from(path)`, with `path` taken **relative to the process working directory**. The
//!   compile-time check resolves the same `path` against `CARGO_MANIFEST_DIR` instead. The two agree when you run
//!   from the crate root, as `cargo run` in a single-crate project does.
//! - `load_from(path)` reads any file, choosing its format as described below. Use it for deployment overrides; the
//!   compile-time check keeps validating the checked-in default:
//!
//!   ```rust,ignore
//!   let settings = match std::env::var("APP_CONFIG") {
//!       Ok(path) => Settings::load_from(path)?,
//!       Err(_) => Settings::load()?,
//!   };
//!   ```
//! - `load_str(source, format)` parses a string with a given [`Format`], such as [`Toml`]. Useful in tests.
//!
//! Nested structs and error paths:
//!
//! ```rust
//! use cfgy::FromConfig;
//!
//! #[derive(FromConfig)]
//! struct Server {
//!     host: String,
//!     port: u16,
//! }
//!
//! #[derive(FromConfig)]
//! #[config(path = "config.toml", check = false)]
//! struct Settings {
//!     primary: Server,
//!     replicas: Vec<Server>,
//! }
//!
//! let source = r#"
//! primary = { host = "10.0.0.1", port = 8080 }
//! replicas = [{ host = "10.0.0.2", port = 8081 }, { host = "10.0.0.3", port = "eighty" }]
//! "#;
//! let err = Settings::load_str(source, &cfgy::Toml).err().unwrap();
//! // Line 3, column 77 of `source`: where `"eighty"` starts.
//! assert_eq!(err.to_string(), "3:77: `replicas[1].port`: expected u16, found string");
//! ```
//!
//! # Formats
//!
//! Each format is a cargo feature: `toml` (default), `json`, and `yaml`. The adapters are re-exported as
//! [`Toml`], `Json`, and `Yaml` when enabled.
//!
//! The format of a file is chosen, in order, by:
//!
//! 1. the `format` attribute;
//! 2. the file extension: `.toml`, `.json`, `.yaml`, or `.yml`, in any letter case;
//! 3. only when the file has no extension, its content: a leading `{` is JSON, a leading `---` is YAML.
//!
//! An unknown extension is an error, never a guess, as is a format whose feature is disabled. An unknown or disabled
//! `format` name is always a build error, even with `check = false`. An unknown extension is a build error from the
//! compile-time check; with `check = false`, or for a `load_from` path, it surfaces at runtime as
//! [`ConfigError::UnknownFormat`].
//!
//! YAML reads the first document of a stream; an empty file is an empty table, as in TOML. A YAML node with a custom
//! tag, `!Name value`, reads as the single-key table `{ Name: value }`, which is how `serde_yaml` writes an
//! externally tagged enum: `!Port 8080` is the variant `Port(8080)` and a bare `!Off` the unit variant `Off`. The
//! core `!!` tags keep their usual meaning.
//!
//! # Field types
//!
//! | Rust type | Accepts |
//! |---|---|
//! | `i8`..`i128`, `isize`, `u8`..`u128`, `usize` | integers, range-checked (`u128` up to `i128::MAX`) |
//! | `f64`, `f32` | floats and integers; `f32` rejects finite values beyond its range |
//! | `bool` | booleans |
//! | `String` | strings and datetimes |
//! | `PathBuf` | strings |
//! | `char` | one-character strings |
//! | [`Datetime`] | TOML datetimes, or strings that are valid datetimes; the text is kept as written |
//! | `Option<T>` | a `T`, an absent key, or null |
//! | `Vec<T>`, `VecDeque<T>`, `HashSet<T>` | sequences |
//! | `HashMap<String, T>`, `BTreeMap<String, T>`, [`IndexMap<String, T>`](IndexMap) | tables |
//! | tuples of up to 4 elements | sequences of exactly that length |
//! | `Box<T>`, `Rc<T>`, `Arc<T>` | whatever `T` accepts |
//! | derived struct with named fields | tables |
//! | derived newtype struct, `struct Port(u16)` | whatever its field accepts (transparent, like serde) |
//! | derived tuple struct, `struct Pair(String, u8)` | sequences of exactly that length |
//! | derived unit struct, `struct Marker;` | null |
//! | derived enum | its variants, written as [Enums](#enums) describes |
//! | any other type implementing [`FromValue`] | whatever its `from_value` accepts |
//!
//! Type aliases are their target type: `type Port = u16` behaves exactly like `u16`, at runtime and in the
//! compile-time check.
//!
//! ```rust
//! use cfgy::FromConfig;
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! struct Port(u16);
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! struct Range(u16, u16);
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! #[config(path = "config.toml", check = false, rename_all = "kebab-case")]
//! struct Settings {
//!     listen_port: Port,
//!     ephemeral_range: Range,
//! }
//!
//! let settings = Settings::load_str("listen-port = 80\nephemeral-range = [49152, 65535]\n", &cfgy::Toml).unwrap();
//! assert_eq!(settings, Settings { listen_port: Port(80), ephemeral_range: Range(49152, 65535) });
//! ```
//!
//! ## Conversion rules
//!
//! Conversion is strict, and the same in every format:
//!
//! - Integers never wrap: `70000` into a `u16` is an error. Integers are held as `i128`, so every integer type up to
//!   `u64` and `i128` accepts its full range, as far as the format can write it: TOML integers are 64-bit signed
//!   by the TOML spec (a larger literal is a parse error), YAML integers above `i64::MAX` are read as floats, and
//!   JSON integers above `u64::MAX` are read as floats, as `serde_json` does.
//! - Integers widen into floats, rounding to the nearest representable value, as serde does.
//! - A float is never an integer, even `1.0`. A string is never a number. An integer is never a bool.
//! - A TOML datetime converts into either a [`Datetime`] or a `String`. JSON and YAML have no datetime type; their
//!   strings convert into a [`Datetime`] when they are valid TOML or RFC 3339 datetimes, so the same config loads
//!   identically from every format. The grammar and field ranges are checked, not the calendar: `2023-02-31` passes.
//! - Keys in the file that no field reads are ignored.
//! - An absent key uses the field's `default`, else its type's [`from_missing`](FromValue::from_missing) (`None`
//!   for an `Option`), else is a [`ConfigError::Missing`].
//! - A key present with a null value (JSON `null`, YAML `~`) is `None` for an `Option` field and a type error for a
//!   required field. TOML has no null.
//!
//! # Enums
//!
//! An enum derives `FromConfig` like a struct and is used as a field type. Unit, newtype, tuple, and struct variants
//! are all supported, in serde's four representations:
//!
//! | Representation | Attribute | Unit variant | Other variants |
//! |---|---|---|---|
//! | externally tagged | none (the default) | `"Off"` | `{ Port = 8080 }`, `{ Serve = { host = "h" } }` |
//! | internally tagged | `tag = "type"` | `{ type = "Off" }` | `{ type = "Serve", host = "h" }` |
//! | adjacently tagged | `tag = "t", content = "c"` | `{ t = "Off" }` | `{ t = "Port", c = 8080 }` |
//! | untagged | `untagged` | null | the payload alone: `8080`, `{ host = "h" }` |
//!
//! - **Externally tagged:** a unit variant may also be written `{ Off = null }` (YAML `!Off`). Any other variant is
//!   a table with exactly one key.
//! - **Internally tagged:** a unit variant ignores the keys beside the tag; a struct variant reads its fields from
//!   them. Tuple variants cannot be internally tagged (a compile error, as in serde), and a struct variant field
//!   whose key equals the tag is a compile error.
//! - **Adjacently tagged:** `content` may be absent for a unit variant and for a newtype variant whose type is
//!   optional.
//! - **Untagged:** the first variant, in declaration order, that converts wins; the errors of the variants tried
//!   before it are dropped, as in serde.
//!
//! A tag naming no variant is a [`ConfigError::UnknownVariant`] listing the valid names.
//!
//! ```rust
//! use cfgy::FromConfig;
//! use std::path::PathBuf;
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! #[config(rename_all = "lowercase")]
//! enum Level {
//!     Debug,
//!     Info,
//!     Warn,
//! }
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! enum Listen {
//!     Port(u16),
//!     Socket { path: PathBuf },
//! }
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! #[config(tag = "kind", rename_all = "snake_case")]
//! enum Storage {
//!     InMemory,
//!     Disk {
//!         path: PathBuf,
//!         #[config(default = 64)]
//!         cache_mb: u32,
//!     },
//! }
//!
//! #[derive(FromConfig, Debug, PartialEq)]
//! #[config(path = "config.toml", check = false)]
//! struct Settings {
//!     level: Level,
//!     listen: Listen,
//!     storage: Storage,
//! }
//!
//! let source = r#"
//! level = "info"
//! listen = { Port = 8080 }
//! storage = { kind = "disk", path = "/var/lib/app" }
//! "#;
//! let settings = Settings::load_str(source, &cfgy::Toml).unwrap();
//! assert_eq!(
//!     settings,
//!     Settings {
//!         level: Level::Info,
//!         listen: Listen::Port(8080),
//!         storage: Storage::Disk { path: "/var/lib/app".into(), cache_mb: 64 },
//!     },
//! );
//!
//! let err = Settings::load_str(&source.replace("\"info\"", "\"trace\""), &cfgy::Toml).unwrap_err();
//! assert_eq!(
//!     err.to_string(),
//!     "2:9: `level`: unknown variant `trace` of Level, expected one of `debug`, `info`, `warn`",
//! );
//! ```
//!
//! Two deviations from serde remain:
//!
//! - An internally tagged newtype variant receives the whole table, tag included. A derived struct ignores the
//!   extra key, so `Wrapped(Server)` reads `{ type = "Wrapped", host = "h" }` as serde does, but a map type would
//!   see the tag as an entry.
//! - An internally tagged newtype variant whose type does not describe itself as a table (a map, another enum) can
//!   load at runtime but is rejected by the compile-time check. Mark the field holding the enum `unchecked`.
//!
//! # Errors
//!
//! Every error is a [`ConfigError`]. Conversion errors name the full key path, such as `` `replicas[1].port` ``,
//! and carry a [`Location`]: `line:col` from `load_str`, `file:line:col` from `load_from` and `load`. Parse errors
//! are located the same way. A missing key points at the table it is missing from.
//!
//! ```rust
//! use cfgy::{ConfigError, FromConfig, Location};
//!
//! #[derive(FromConfig, Debug)]
//! struct Database {
//!     url: String,
//!     pool: u32,
//! }
//!
//! #[derive(FromConfig, Debug)]
//! #[config(path = "config.toml", check = false)]
//! struct Settings {
//!     database: Database,
//! }
//!
//! let err = Settings::load_str("[database]\nurl = 5\npool = 4\n", &cfgy::Toml).unwrap_err();
//! assert_eq!(err.to_string(), "2:7: `database.url`: expected string, found integer");
//! assert!(matches!(err, ConfigError::Type { location: Some(Location { line: 2, column: 7, .. }), .. }));
//!
//! // `pool` is missing: the error points at the `[database]` header.
//! let err = Settings::load_str("[database]\nurl = \"postgres://db\"\n", &cfgy::Toml).unwrap_err();
//! assert_eq!(err.to_string(), "1:1: missing required key `database.pool`");
//!
//! let err = Settings::load_str("[database\n", &cfgy::Toml).unwrap_err();
//! assert!(matches!(err, ConfigError::Parse { location: Some(_), .. }), "{err}");
//! ```
//!
//! Through `load_from`, the same errors read `config/app.toml:2:7: ...`. [`ConfigError::Io`] and
//! [`ConfigError::UnknownFormat`] name the file instead of a position.
//!
//! # Compile-time check
//!
//! For a struct with `path` and without `check = false`, the derive reads the file relative to
//! `CARGO_MANIFEST_DIR` and parses it. A missing or malformed file is a compile error on the `path` literal, with
//! `file:line:col` for a parse error.
//!
//! Every value in the parsed file is then checked against `<Settings as FromValue>::SHAPE`, a compile-time
//! description ([`Shape`]) of what the type accepts, by const evaluation. The compiler resolves the shape, so the
//! check follows real types: nested structs, `Vec`s of structs, maps, enums in every representation, newtypes, and
//! type aliases are all checked to the bottom, with the same rules as runtime conversion. Each wrong value is its
//! own error, and all of them are reported:
//!
//! ```text
//! error[E0080]: evaluation panicked: config.toml:3:8: `primary.port`: expected u16, found string
//! error[E0080]: evaluation panicked: config.toml:7:8: `replica.port`: 70000 is out of range for u16
//! ```
//!
//! A table missing required keys is reported once, at the table, naming the first missing key.
//!
//! Some values are left to runtime:
//!
//! - **Custom [`FromValue`] impls** have the default shape [`Shape::Any`], which accepts anything, so `from_value`
//!   decides at runtime. An impl may set `SHAPE` to have its values checked at build time; the shape must accept
//!   everything `from_value` does, or the check rejects files that would load.
//! - **Recursive types.** A field whose type mentions its own type, such as `children: Vec<Node>` in `Node`, is not
//!   checked at build time; the rest of the struct is. Mutually recursive types (`Team` holds `Member`, which holds
//!   `Team`) fail to compile with a cycle error (E0391) until one of the fields in the cycle is marked
//!   `#[config(unchecked)]`.
//! - **`unchecked` fields** accept any value at build time.
//!
//! ```rust
//! use cfgy::{ConfigError, FromConfig, FromValue, PathStack, Shape, Value};
//!
//! /// A log level written as a string, checked at build time like a `String`.
//! struct Level(String);
//!
//! impl FromValue for Level {
//!     const SHAPE: &'static Shape = &Shape::String;
//!
//!     fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
//!         String::from_value(value, path).map(Level)
//!     }
//! }
//!
//! #[derive(FromConfig)]
//! struct Member {
//!     name: String,
//!     #[config(unchecked)] // breaks the Team -> Member -> Team cycle
//!     team: Option<Box<Team>>,
//! }
//!
//! #[derive(FromConfig)]
//! struct Team {
//!     members: Vec<Member>,
//! }
//!
//! #[derive(FromConfig)]
//! #[config(path = "config.toml", check = false)]
//! struct Settings {
//!     level: Level,
//!     team: Team,
//! }
//!
//! let settings = Settings::load_str("level = \"info\"\nteam = { members = [{ name = \"a\" }] }\n", &cfgy::Toml);
//! assert_eq!(settings.unwrap().team.members[0].name, "a");
//! ```
//!
//! The derive also emits an `include_str!` of the file, so Cargo rebuilds the crate, and re-runs the check, whenever
//! the file changes.
//!
//! The check runs on the build machine against the build machine's file. It is a fast feedback loop, not a guarantee
//! about the file present in production; `load` handles every failure again at runtime.
//!
//! # Validation
//!
//! `load` returns a struct that is well-typed, not one that is semantically valid. Validation is a separate step;
//! `cfgy` knows nothing about it, and pairs with the `validator` crate as an independent derive:
//!
//! ```rust,ignore
//! use cfgy::FromConfig;
//! use validator::Validate;
//!
//! #[derive(FromConfig, Validate)]
//! #[config(path = "config.toml")]
//! struct Settings {
//!     #[validate(length(min = 1))]
//!     host: String,
//!     #[validate(range(min = 1024))]
//!     port: u16,
//! }
//!
//! let settings = Settings::load()?;
//! settings.validate()?;
//! ```
//!
//! # Known limitations
//!
//! - **The check reflects the build machine only.** A file that passes the check can still fail to load in another
//!   environment, and `check = false` structs are only validated at runtime.
//! - **Generic structs and enums are not supported.** Unions are not either.
//! - **Unknown keys are ignored.** There is no `deny_unknown_keys`, so a misspelt optional key goes unnoticed.
//! - **No `flatten`, `alias`, or `skip`**, and no container-level `default`.
//! - **No layered configs or environment-variable overrides.** One file is loaded per call; merge in your own code.
//! - **[`Datetime`] is text only.** It keeps the datetime as written; parse it with a date crate for arithmetic.
//! - **JSON integers above `u64::MAX` become floats**, as in `serde_json` without `arbitrary_precision`, and YAML
//!   integers above `i64::MAX` do too.
//! - **Mutually recursive types need `unchecked`** on one field of the cycle, as described above.
//! - **Compile-time check errors render differently between rustc versions.** Stable prints
//!   `evaluation panicked: <message>`; Rust 1.85 prints `the evaluated program panicked at '<message>'`. The message
//!   itself is the same.
//! - **One const per config value.** Compile time grows with the size of the checked file; this is fine for
//!   hand-written configs but noticeable for files with thousands of values. Use `check = false` for those.

// Lets the derive's `::cfgy::…` paths resolve inside this crate's own tests.
extern crate self as cfgy;

pub use cfgy_core::*;
pub use cfgy_derive::FromConfig;
#[cfg(feature = "json")]
pub use cfgy_formats::Json;
#[cfg(feature = "toml")]
pub use cfgy_formats::Toml;
#[cfg(feature = "yaml")]
pub use cfgy_formats::Yaml;

/// Items used by generated code. Not public API.
#[doc(hidden)]
pub mod __private {
    pub use cfgy_core::DefaultFrom;
    pub use cfgy_core::check::{Node, check_node};
    pub use cfgy_formats::select;
}
