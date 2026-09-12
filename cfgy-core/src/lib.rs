//! Value IR, conversion traits, and error types for cfgy.
//!
//! Format-agnostic. You probably want the [cfgy](https://docs.rs/cfgy) crate instead.

mod format;
mod value;

pub use format::{Format, ParseError};
pub use indexmap::IndexMap;
pub use value::{Datetime, Value};
