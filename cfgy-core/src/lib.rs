//! Value IR, conversion traits, and error types for cfgy.
//!
//! Format-agnostic. You probably want the [cfgy](https://docs.rs/cfgy) crate instead.

#[doc(hidden)]
pub mod check;
mod default_from;
mod error;
mod format;
mod from_value;
mod path;
mod shape;
mod value;

#[doc(hidden)]
pub use default_from::DefaultFrom;
pub use error::{ConfigError, Location};
pub use format::{Format, ParseError, Parsed, Spans, line_col};
pub use from_value::FromValue;
pub use indexmap::IndexMap;
pub use path::{PathStack, Segment};
pub use shape::{Field, Repr, Shape, Variant};
pub use value::{Datetime, Value, is_datetime};
