//! Value IR, conversion traits, and error types for cfgy.
//!
//! Format-agnostic. You probably want the [cfgy](https://docs.rs/cfgy) crate instead.

mod error;
mod format;
mod from_value;
mod path;
mod value;

pub use error::ConfigError;
pub use format::{Format, ParseError};
pub use from_value::FromValue;
pub use indexmap::IndexMap;
pub use path::{PathStack, Segment};
pub use value::{Datetime, Value};
