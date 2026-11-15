//! Conversion of a field's `#[config(default = <expr>)]` into the field type.

use std::path::PathBuf;

/// Converts a `default` expression into the field's type, so `default = "localhost"` works on a `String`.
///
/// The derive emits `<FieldTy as DefaultFrom<_>>::default_from(expr)`. Each target type has exactly one impl a given
/// literal can satisfy, so integer and float literals still infer their type from the field. Not public API.
#[doc(hidden)]
pub trait DefaultFrom<T> {
    fn default_from(value: T) -> Self;
}

impl<T> DefaultFrom<T> for T {
    fn default_from(value: T) -> Self {
        value
    }
}

impl DefaultFrom<&'static str> for String {
    fn default_from(value: &'static str) -> Self {
        value.to_owned()
    }
}

impl DefaultFrom<&'static str> for PathBuf {
    fn default_from(value: &'static str) -> Self {
        Self::from(value)
    }
}

impl<T> DefaultFrom<T> for Option<T> {
    fn default_from(value: T) -> Self {
        Some(value)
    }
}

impl DefaultFrom<&'static str> for Option<String> {
    fn default_from(value: &'static str) -> Self {
        Some(value.to_owned())
    }
}

impl DefaultFrom<&'static str> for Option<PathBuf> {
    fn default_from(value: &'static str) -> Self {
        Some(PathBuf::from(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_infer_the_field_type() {
        assert_eq!(<u64 as DefaultFrom<_>>::default_from(30), 30_u64);
        assert!((<f64 as DefaultFrom<_>>::default_from(1.5) - 1.5).abs() < f64::EPSILON);
        assert_eq!(<String as DefaultFrom<_>>::default_from("x"), "x");
        assert_eq!(<String as DefaultFrom<_>>::default_from(String::from("y")), "y");
        assert_eq!(<PathBuf as DefaultFrom<_>>::default_from("a/b"), PathBuf::from("a/b"));
        assert_eq!(<Option<u64> as DefaultFrom<_>>::default_from(5), Some(5));
        assert_eq!(<Option<u64> as DefaultFrom<_>>::default_from(None), None);
        assert_eq!(<Option<String> as DefaultFrom<_>>::default_from(String::from("z")), Some("z".to_owned()));
        assert_eq!(<Option<String> as DefaultFrom<_>>::default_from("w"), Some("w".to_owned()));
        assert_eq!(<Option<PathBuf> as DefaultFrom<_>>::default_from("c"), Some(PathBuf::from("c")));
        assert_eq!(<Option<i8> as DefaultFrom<_>>::default_from(-1), Some(-1));
        assert_eq!(<Vec<u8> as DefaultFrom<_>>::default_from(vec![1, 2]), [1, 2]);
    }
}
