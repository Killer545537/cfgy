//! Derive config structs from TOML, JSON, or YAML files.
//!
//! `#[derive(FromConfig)]` maps a config file onto a struct. Nested types derive it too, and a
//! struct with `#[config(path = "...")]` gains `load`, `load_from`, and `load_str`.
//!
//! Formats are behind cargo features: `toml` (default), `json`, and `yaml`.

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
