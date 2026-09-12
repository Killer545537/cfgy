use crate::{ConfigError, PathStack, Value};

/// Conversion from the value IR into a Rust type.
///
/// `path` locates `value` in the document; implementations push onto it before recursing.
pub trait FromValue: Sized {
    /// # Errors
    ///
    /// Returns [`ConfigError`] when `value` does not have the shape `Self` needs.
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError>;
}
