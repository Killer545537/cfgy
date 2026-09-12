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

macro_rules! impl_int {
    ($($t:ty),*) => {$(
        impl FromValue for $t {
            fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
                let Value::Int(i) = value else {
                    return Err(ConfigError::type_mismatch(path, stringify!($t), value));
                };
                Self::try_from(*i).map_err(|_| ConfigError::out_of_range(path, *i, stringify!($t)))
            }
        }
    )*};
}

impl_int!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

#[cfg(test)]
mod tests {
    use super::*;

    fn conv<T: FromValue>(value: &Value) -> Result<T, ConfigError> {
        T::from_value(value, &mut PathStack::new())
    }

    #[test]
    fn integers() {
        assert!(matches!(conv::<u16>(&Value::Int(i64::MAX)), Err(ConfigError::OutOfRange { target: "u16", .. })));
        assert!(matches!(conv::<u32>(&Value::Int(-1)), Err(ConfigError::OutOfRange { value: -1, .. })));
        assert_eq!(conv::<u16>(&Value::Int(i64::from(u16::MAX))).unwrap(), u16::MAX);
        assert_eq!(conv::<i128>(&Value::Int(i64::MIN)).unwrap(), i128::from(i64::MIN));
        assert!(matches!(
            conv::<i32>(&Value::Float(1.0)),
            Err(ConfigError::Type { expected: "i32", found: "float", .. })
        ));
        assert!(matches!(conv::<i32>(&Value::Str("1".into())), Err(ConfigError::Type { found: "string", .. })));
    }
}
