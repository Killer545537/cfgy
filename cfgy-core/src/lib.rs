//! Value IR, conversion traits, and error types for cfgy.
//!
//! Format-agnostic. You probably want the [cfgy](https://docs.rs/cfgy) crate instead.

mod value;

pub use indexmap::IndexMap;
pub use value::{Datetime, Value};
