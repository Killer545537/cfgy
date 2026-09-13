//! Derive config structs from TOML, JSON, or YAML files, with the file checked at compile time and parsed at
//! runtime.
//!
//! `#[derive(FromConfig)]` maps a config file onto a struct. With `#[config(path = "...")]`, the derive reads that
//! file *during compilation* and type-checks it against the struct, so `port = "eighty"` in `config.toml` is a build
//! error. The struct also gains [`load`](#generated-api), which parses the file again at runtime, so the file can
//! change without a rebuild.
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
//! On the struct:
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `path = "config.toml"` | The config file. Grants the struct its loading methods. |
//! | `format = "toml"` | Force a format: `toml`, `json`, `yaml`, or `yml`. Requires `path`. |
//! | `check = false` | Skip the compile-time check, for files that do not exist at build time. Requires `path`. |
//!
//! On a field:
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `rename = "type"` | Read the field from a different key. |
//! | `default = <expr>` | Evaluate `<expr>` when the key is absent. Any expression of the field's type. |
//!
//! The key is the field name unless renamed. Raw identifiers drop their prefix, so `r#type` reads the key `type`.
//! Unknown options, struct options on a field (or the reverse), and two fields mapping to the same key are compile
//! errors.
//!
//! # Generated API
//!
//! Every derived struct implements [`FromValue`], which is what lets it appear as a field, a `Vec` element, or a map
//! value inside another derived struct. Nested types derive `FromConfig` without a `path`.
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
//! A struct without `path` has none of them.
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
//! assert_eq!(err.to_string(), "`replicas[1].port`: expected u16, found string");
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
//! 2. the file extension: `.toml`, `.json`, `.yaml`, or `.yml` (case-sensitive);
//! 3. only when the file has no extension, its content: a leading `{` is JSON, a leading `---` is YAML.
//!
//! An unknown extension is an error, never a guess, as is a format whose feature is disabled. The compile-time check
//! reports these as build errors; with `check = false` they surface from `load` as [`ConfigError::UnknownFormat`].
//!
//! # Compile-time check
//!
//! For a struct with `path` and without `check = false`, the derive reads the file relative to
//! `CARGO_MANIFEST_DIR`, parses it, and checks it against the fields. A missing file, a malformed file (reported with
//! `file:line:col`), a missing required key, and a mistyped value are compile errors on the `path` literal. All field
//! errors are reported at once.
//!
//! The derive also emits an `include_str!` of the file, so Cargo rebuilds the crate, and re-runs the check, whenever
//! the file changes.
//!
//! The check runs on the build machine against the build machine's file. It is a fast feedback loop, not a guarantee
//! about the file present in production; `load` handles every failure again at runtime.
//!
//! # Field types
//!
//! | Rust type | Accepts |
//! |---|---|
//! | `i8`..`i128`, `isize`, `u8`..`u128`, `usize` | integers, range-checked |
//! | `f64`, `f32` | floats; integers within `i32` (for `f64`) or `i16` (for `f32`) |
//! | `bool` | booleans |
//! | `String` | strings and datetimes |
//! | `PathBuf` | strings |
//! | `char` | one-character strings |
//! | [`Datetime`] | TOML datetimes, or strings; the text is kept as written |
//! | `Option<T>` | a `T`, an absent key, or null |
//! | `Vec<T>`, `VecDeque<T>`, `HashSet<T>` | sequences |
//! | `HashMap<String, T>`, `BTreeMap<String, T>`, [`IndexMap<String, T>`](IndexMap) | tables |
//! | tuples of up to 4 elements | sequences of exactly that length |
//! | `Box<T>`, `Rc<T>`, `Arc<T>` | whatever `T` accepts |
//! | any type implementing [`FromValue`], including derived structs | tables, for derived structs |
//!
//! Conversion is strict:
//!
//! - Integers never wrap: `70000` into a `u16` is an error. All integers pass through `i64`, so `u64` and wider
//!   fields accept only values up to `i64::MAX`.
//! - Integers widen to floats only when exact (see the table); write `1.0e10` for large float values.
//! - A float is never an integer, even `1.0`. A string is never a number. An integer is never a bool.
//! - A TOML datetime converts into either a [`Datetime`] or a `String`. JSON and YAML have no datetime type; their
//!   strings also convert into either, so the same config loads identically from every format.
//! - Keys in the file that no field reads are ignored.
//! - An absent key is `None` for an `Option` field, the default for a `default` field, and an error otherwise.
//! - A key present with a null value (JSON `null`, YAML `~`) is `None` for an `Option` field and a type error for a
//!   required field. TOML has no null.
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
//! - **The compile-time check is shallow.** It fully checks the types listed above, but any other type path is
//!   only checked to be a table. Mistakes inside a nested struct therefore surface at runtime, not at compile time.
//!   A type alias of a scalar (`type Port = u16;`) or a custom [`FromValue`] type read from a non-table value is
//!   rejected by the check even though it would load; use `check = false` for such structs.
//! - **`Option` is recognised by syntax.** A field must be written `Option<T>` (or `std::option::Option<T>`) to be
//!   optional. `type MaybePort = Option<u16>;` makes the key required.
//! - **`default` is a plain expression.** `#[config(default = "localhost")]` on a `String` field does not compile
//!   because the literal is a `&str`; write `default = "localhost".into()`.
//! - **The check reflects the build machine only.** A file that passes the check can still fail to load in another
//!   environment, and `check = false` structs are only validated at runtime.
//! - **Structs only.** Enums, unions, tuple structs, unit structs, and generic structs cannot derive `FromConfig`.
//! - **No line numbers in conversion errors.** Parse errors carry a line and column, but a type error found after
//!   parsing reports only the key path, such as `` `database.port` ``.
//!
//! ```rust
//! use cfgy::FromConfig;
//!
//! #[derive(FromConfig)]
//! #[config(path = "config.toml", check = false)]
//! struct Settings {
//!     #[config(default = "localhost".into())]
//!     host: String,
//! }
//!
//! assert_eq!(Settings::load_str("", &cfgy::Toml).map(|s| s.host).ok().as_deref(), Some("localhost"));
//! ```

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
    pub use cfgy_formats::select;
}
