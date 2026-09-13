use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    hash::{BuildHasher, Hash},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};

use indexmap::IndexMap;

use crate::{ConfigError, Datetime, PathStack, Shape, Value, is_datetime};

/// Conversion from the value IR into a Rust type.
///
/// `path` locates `value` in the document; implementations push onto it before recursing.
pub trait FromValue: Sized {
    /// What this type accepts, for the derive's compile-time check. Defaults to [`Shape::Any`], so a type that does
    /// not describe itself is never rejected at compile time; `from_value` still decides at runtime.
    const SHAPE: &'static Shape = &Shape::Any;

    /// # Errors
    ///
    /// Returns [`ConfigError`] when `value` does not have the shape `Self` needs.
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError>;

    /// The value for a key absent from its table, or `None` if absence is an error.
    ///
    /// This is serde's `missing_field`: `Option<T>` returns `Some(None)`, so optional fields work through type
    /// aliases instead of depending on the field being spelled `Option<...>`.
    #[must_use]
    fn from_missing() -> Option<Self> {
        None
    }
}

/// The [`Shape::Int`] of a `bits`-wide integer type.
///
/// `From` is not const and `as` is banned, so the bounds come from shifting `i128`'s own, which also gets `isize` and
/// `usize` right on every target. `u128::MAX` does not fit and is clamped to `i128::MAX`, the most a `Value::Int` holds.
const fn int_shape(name: &'static str, bits: u32, signed: bool) -> Shape {
    let shift = 128_u32.saturating_sub(bits);
    let (min, max) = if signed {
        // An arithmetic shift of `i128::MIN` keeps the sign: -2^(bits-1), whose complement is 2^(bits-1) - 1.
        let min = match i128::MIN.checked_shr(shift) {
            Some(min) => min,
            None => i128::MIN,
        };
        (min, !min)
    } else {
        match i128::MAX.checked_shr(shift.saturating_sub(1)) {
            Some(max) => (0, max),
            None => (0, i128::MAX),
        }
    };
    Shape::Int { min, max, name }
}

macro_rules! impl_int {
    ($($t:ty),*) => {$(
        impl FromValue for $t {
            const SHAPE: &'static Shape = &int_shape(stringify!($t), <$t>::BITS, <$t>::MIN != 0);

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

/// Widens any integer into a float, rounding to the nearest representable value like `as` (and serde) do.
///
/// `as` is banned and `From` only covers small integers, so go through the decimal text, which parses correctly rounded.
fn widen<F: std::str::FromStr>(i: i128) -> Option<F> {
    i.to_string().parse().ok()
}

impl FromValue for f64 {
    const SHAPE: &'static Shape = &Shape::Float { max: Self::MAX, name: "f64" };

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Float(f) => Ok(*f),
            Value::Int(i) => widen(*i).ok_or_else(|| ConfigError::out_of_range(path, *i, "f64")),
            _ => Err(ConfigError::type_mismatch(path, "f64", value)),
        }
    }
}

impl FromValue for f32 {
    // Not `f32::MAX`: narrowing rounds to nearest, so finite values a little above it still land on `f32::MAX`. This is
    // 2^128 - 2^103, the largest `f64` that still narrows to a finite `f32` (its shortest decimal sits just below the
    // halfway point, so it rounds down). The float tests pin the boundary on both sides.
    const SHAPE: &'static Shape = &Shape::Float { max: 3.402_823_567_797_336_6e38, name: "f32" };

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            // `as` is banned and std has no checked f64 -> f32, so go through the shortest round-trip decimal, which
            // parses to the nearest f32. A finite value too large for f32 would become infinite, so it is rejected.
            Value::Float(f) => match f.to_string().parse::<Self>() {
                Ok(narrowed) if narrowed.is_finite() || !f.is_finite() => Ok(narrowed),
                _ => Err(ConfigError::type_mismatch(path, "f32", value)),
            },
            // Every i128 is below f32::MAX, so widening never overflows to infinity.
            Value::Int(i) => widen(*i).ok_or_else(|| ConfigError::out_of_range(path, *i, "f32")),
            _ => Err(ConfigError::type_mismatch(path, "f32", value)),
        }
    }
}

impl FromValue for bool {
    const SHAPE: &'static Shape = &Shape::Bool;

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Bool(b) => Ok(*b),
            _ => Err(ConfigError::type_mismatch(path, "bool", value)),
        }
    }
}

impl FromValue for String {
    const SHAPE: &'static Shape = &Shape::String;

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            // A TOML datetime is text in JSON and YAML, so accept it here too or the formats would disagree.
            Value::Str(s) | Value::Datetime(Datetime(s)) => Ok(s.clone()),
            _ => Err(ConfigError::type_mismatch(path, "string", value)),
        }
    }
}

impl FromValue for PathBuf {
    const SHAPE: &'static Shape = &Shape::Path;

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Str(s) => Ok(Self::from(s)),
            _ => Err(ConfigError::type_mismatch(path, "path", value)),
        }
    }
}

impl FromValue for char {
    const SHAPE: &'static Shape = &Shape::Char;

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        if let Value::Str(s) = value {
            let mut chars = s.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                return Ok(c);
            }
        }
        Err(ConfigError::type_mismatch(path, "a single character", value))
    }
}

/// Converts every item of a `Seq`, pushing its index onto `path`.
fn seq<T: FromValue, C: FromIterator<T>>(value: &Value, path: &mut PathStack) -> Result<C, ConfigError> {
    let Value::Seq(items) = value else {
        return Err(ConfigError::type_mismatch(path, "sequence", value));
    };
    items.iter().enumerate().map(|(i, item)| path.with_index(i, |p| T::from_value(item, p))).collect()
}

impl<T: FromValue> FromValue for Vec<T> {
    const SHAPE: &'static Shape = &Shape::Seq(T::SHAPE);

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        seq(value, path)
    }
}

impl<T: FromValue> FromValue for VecDeque<T> {
    const SHAPE: &'static Shape = &Shape::Seq(T::SHAPE);

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        seq(value, path)
    }
}

impl<T: FromValue + Eq + Hash, S: BuildHasher + Default> FromValue for HashSet<T, S> {
    const SHAPE: &'static Shape = &Shape::Seq(T::SHAPE);

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        seq(value, path)
    }
}

/// Converts every entry of a `Map`, pushing its key onto `path`.
fn map<T: FromValue, C: FromIterator<(String, T)>>(value: &Value, path: &mut PathStack) -> Result<C, ConfigError> {
    let Value::Map(entries) = value else {
        return Err(ConfigError::type_mismatch(path, "table", value));
    };
    entries.iter().map(|(key, item)| Ok((key.clone(), path.with_key(key, |p| T::from_value(item, p))?))).collect()
}

impl<T: FromValue, S: BuildHasher + Default> FromValue for HashMap<String, T, S> {
    const SHAPE: &'static Shape = &Shape::Map(T::SHAPE);

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        map(value, path)
    }
}

impl<T: FromValue> FromValue for BTreeMap<String, T> {
    const SHAPE: &'static Shape = &Shape::Map(T::SHAPE);

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        map(value, path)
    }
}

impl<T: FromValue, S: BuildHasher + Default> FromValue for IndexMap<String, T, S> {
    const SHAPE: &'static Shape = &Shape::Map(T::SHAPE);

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        map(value, path)
    }
}

macro_rules! impl_tuple {
    ($len:literal => $(($t:ident $item:ident $i:literal)),+) => {
        impl<$($t: FromValue),+> FromValue for ($($t,)+) {
            const SHAPE: &'static Shape = &Shape::Tuple(&[$($t::SHAPE),+]);

            fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
                let Value::Seq(items) = value else {
                    return Err(ConfigError::type_mismatch(path, "sequence", value));
                };
                let [$($item),+] = items.as_slice() else {
                    return Err(ConfigError::length(path, $len, items.len()));
                };
                Ok(($(path.with_index($i, |p| $t::from_value($item, p))?,)+))
            }
        }
    };
}

impl_tuple!(1 => (A a 0));
impl_tuple!(2 => (A a 0), (B b 1));
impl_tuple!(3 => (A a 0), (B b 1), (C c 2));
impl_tuple!(4 => (A a 0), (B b 1), (C c 2), (D d 3));

macro_rules! impl_pointer {
    ($($p:ident),*) => {$(
        impl<T: FromValue> FromValue for $p<T> {
            const SHAPE: &'static Shape = T::SHAPE;

            fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
                T::from_value(value, path).map(Self::new)
            }

            fn from_missing() -> Option<Self> {
                T::from_missing().map(Self::new)
            }
        }
    )*};
}

impl_pointer!(Box, Rc, Arc);

impl FromValue for Datetime {
    const SHAPE: &'static Shape = &Shape::Datetime;

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Datetime(d) => Ok(d.clone()),
            // JSON and YAML have no datetime type, so text stands in, but only if it is one (toml checks this too).
            Value::Str(s) if is_datetime(s) => Ok(Self(s.clone())),
            _ => Err(ConfigError::type_mismatch(path, "datetime", value)),
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    const SHAPE: &'static Shape = &Shape::Optional(T::SHAPE);

    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Null => Ok(None),
            _ => T::from_value(value, path).map(Some),
        }
    }

    fn from_missing() -> Option<Self> {
        Some(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv<T: FromValue>(value: &Value) -> Result<T, ConfigError> {
        T::from_value(value, &mut PathStack::new())
    }

    #[test]
    fn integers() {
        assert!(matches!(
            conv::<u16>(&Value::Int(i128::from(i64::MAX))),
            Err(ConfigError::OutOfRange { target: "u16", .. })
        ));
        assert!(matches!(conv::<u32>(&Value::Int(-1)), Err(ConfigError::OutOfRange { value: -1, .. })));
        assert_eq!(conv::<u16>(&Value::Int(i128::from(u16::MAX))).unwrap(), u16::MAX);
        assert_eq!(conv::<i128>(&Value::Int(i128::MIN)).unwrap(), i128::MIN);
        assert_eq!(conv::<u64>(&Value::Int(i128::from(u64::MAX))).unwrap(), u64::MAX);
        assert!(matches!(
            conv::<i32>(&Value::Float(1.0)),
            Err(ConfigError::Type { expected: "i32", found: "float", .. })
        ));
        assert!(matches!(conv::<i32>(&Value::Str("1".into())), Err(ConfigError::Type { found: "string", .. })));
    }

    #[test]
    fn floats() {
        assert!((conv::<f64>(&Value::Float(1.5)).unwrap() - 1.5).abs() < f64::EPSILON);
        assert!((conv::<f64>(&Value::Int(-3)).unwrap() + 3.0).abs() < f64::EPSILON);
        // Every integer widens, rounding like `as`.
        assert_eq!(
            conv::<f64>(&Value::Int(i128::from(i64::MAX))).unwrap().to_bits(),
            9_223_372_036_854_775_807_f64.to_bits()
        );
        assert_eq!(conv::<f64>(&Value::Int(i128::MAX)).unwrap().to_bits(), 1.701_411_834_604_692_3e38_f64.to_bits());
        assert_eq!(conv::<f64>(&Value::Int(i128::MIN)).unwrap().to_bits(), (-1.701_411_834_604_692_3e38_f64).to_bits());
        assert_eq!(conv::<f64>(&Value::Int((1 << 53) + 1)).unwrap().to_bits(), 9_007_199_254_740_992_f64.to_bits());
        assert!(matches!(conv::<f64>(&Value::Str("1.0".into())), Err(ConfigError::Type { found: "string", .. })));
        assert!((conv::<f32>(&Value::Float(0.1)).unwrap() - 0.1).abs() < f32::EPSILON);
        assert!((conv::<f32>(&Value::Int(7)).unwrap() - 7.0).abs() < f32::EPSILON);
        assert!(conv::<f32>(&Value::Float(f64::INFINITY)).unwrap().is_infinite());
        assert!(matches!(conv::<f32>(&Value::Float(1.0e300)), Err(ConfigError::Type { expected: "f32", .. })));
        assert!((conv::<f32>(&Value::Int(40_000)).unwrap() - 40_000.0).abs() < f32::EPSILON);
        assert_eq!(conv::<f32>(&Value::Int(16_777_217)).unwrap().to_bits(), 16_777_216_f32.to_bits());
        assert!(conv::<f32>(&Value::Int(i128::MAX)).unwrap().is_finite());
        assert!(conv::<f32>(&Value::Int(i128::MIN)).unwrap().is_finite());
    }

    #[test]
    fn scalars() {
        assert!(conv::<bool>(&Value::Bool(true)).unwrap());
        assert!(matches!(
            conv::<bool>(&Value::Int(1)),
            Err(ConfigError::Type { expected: "bool", found: "integer", .. })
        ));
        assert_eq!(conv::<String>(&Value::Str("hi".into())).unwrap(), "hi");
        assert!(matches!(conv::<String>(&Value::Int(1)), Err(ConfigError::Type { found: "integer", .. })));
        assert_eq!(conv::<PathBuf>(&Value::Str("a/b".into())).unwrap(), PathBuf::from("a/b"));
        assert!(matches!(conv::<PathBuf>(&Value::Bool(false)), Err(ConfigError::Type { found: "bool", .. })));
        assert_eq!(conv::<char>(&Value::Str("é".into())).unwrap(), 'é');
        assert!(conv::<char>(&Value::Str("ab".into())).is_err());
        assert!(conv::<char>(&Value::Str(String::new())).is_err());
        assert!(conv::<char>(&Value::Int(97)).is_err());
    }

    #[test]
    fn sequences() {
        let ints = Value::Seq(vec![Value::Int(1), Value::Int(2), Value::Int(1)]);
        assert_eq!(conv::<Vec<u8>>(&ints).unwrap(), [1, 2, 1]);
        assert_eq!(conv::<VecDeque<u8>>(&ints).unwrap(), [1, 2, 1]);
        assert_eq!(conv::<HashSet<u8>>(&ints).unwrap(), HashSet::from([1, 2]));
        assert_eq!(conv::<Vec<u8>>(&Value::Seq(vec![])).unwrap(), Vec::<u8>::new());
        assert!(matches!(conv::<Vec<u8>>(&Value::Int(1)), Err(ConfigError::Type { expected: "sequence", .. })));

        let bad = Value::Seq(vec![Value::Int(1), Value::Str("x".into())]);
        let mut path = PathStack::new();
        let err = path.with_key("ports", |p| Vec::<u8>::from_value(&bad, p)).unwrap_err();
        assert!(matches!(err, ConfigError::Type { ref path, .. } if path == "ports[1]"));
        assert_eq!(path, PathStack::new());
    }

    #[test]
    fn maps() {
        let table = Value::Map(IndexMap::from([("b".to_owned(), Value::Int(2)), ("a".to_owned(), Value::Int(1))]));
        let expected = [("b".to_owned(), 2), ("a".to_owned(), 1)];
        assert_eq!(conv::<HashMap<String, u8>>(&table).unwrap(), HashMap::from(expected.clone()));
        assert_eq!(conv::<BTreeMap<String, u8>>(&table).unwrap(), BTreeMap::from(expected.clone()));
        assert!(conv::<IndexMap<String, u8>>(&table).unwrap().into_iter().eq(expected));
        assert_eq!(conv::<HashMap<String, u8>>(&Value::Map(IndexMap::new())).unwrap(), HashMap::new());
        assert!(matches!(
            conv::<BTreeMap<String, u8>>(&Value::Seq(vec![])),
            Err(ConfigError::Type { expected: "table", .. })
        ));

        let bad = Value::Map(IndexMap::from([("port".to_owned(), Value::Int(-1))]));
        let err = conv::<HashMap<String, u8>>(&bad).unwrap_err();
        assert!(matches!(err, ConfigError::OutOfRange { ref path, .. } if path == "port"));
    }

    #[test]
    fn tuples() {
        let pair = Value::Seq(vec![Value::Str("a".into()), Value::Int(1)]);
        assert_eq!(conv::<(String, u8)>(&pair).unwrap(), ("a".to_owned(), 1));
        assert_eq!(conv::<(u8,)>(&Value::Seq(vec![Value::Int(9)])).unwrap(), (9,));
        let four = Value::Seq(vec![Value::Int(1), Value::Bool(true), Value::Float(0.5), Value::Str("x".into())]);
        assert_eq!(conv::<(u8, bool, f64, char)>(&four).unwrap(), (1, true, 0.5, 'x'));
        assert!(matches!(conv::<(u8, u8, u8)>(&pair), Err(ConfigError::Length { expected: 3, found: 2, .. })));
        assert!(matches!(conv::<(u8,)>(&Value::Int(1)), Err(ConfigError::Type { expected: "sequence", .. })));
        let err = conv::<(String, String)>(&pair).unwrap_err();
        assert!(matches!(err, ConfigError::Type { ref path, .. } if path == "[1]"));
    }

    #[test]
    fn pointers_and_datetime() {
        assert_eq!(*conv::<Box<u8>>(&Value::Int(1)).unwrap(), 1);
        assert_eq!(*conv::<Rc<String>>(&Value::Str("s".into())).unwrap(), "s");
        assert_eq!(*conv::<Arc<Vec<bool>>>(&Value::Seq(vec![Value::Bool(false)])).unwrap(), [false]);
        assert!(conv::<Box<u8>>(&Value::Null).is_err());

        let stamp = "1979-05-27T07:32:00Z";
        assert_eq!(conv::<Datetime>(&Value::Datetime(Datetime(stamp.into()))).unwrap(), Datetime(stamp.into()));
        assert_eq!(conv::<Datetime>(&Value::Str(stamp.into())).unwrap(), Datetime(stamp.into()));
        assert_eq!(conv::<Datetime>(&Value::Str("07:32:00".into())).unwrap(), Datetime("07:32:00".into()));
        assert!(matches!(conv::<Datetime>(&Value::Int(0)), Err(ConfigError::Type { expected: "datetime", .. })));
        assert!(matches!(
            conv::<Datetime>(&Value::Str("yesterday".into())),
            Err(ConfigError::Type { expected: "datetime", found: "string", .. })
        ));
        assert!(conv::<Datetime>(&Value::Str("1979-13-27".into())).is_err());
        assert_eq!(conv::<String>(&Value::Datetime(Datetime(stamp.into()))).unwrap(), stamp);
    }

    #[test]
    fn options() {
        let items = Value::Seq(vec![Value::Int(1), Value::Null, Value::Int(3)]);
        assert_eq!(conv::<Vec<Option<u8>>>(&items).unwrap(), [Some(1), None, Some(3)]);
        assert!(matches!(conv::<Option<u8>>(&Value::Str("x".into())), Err(ConfigError::Type { expected: "u8", .. })));
    }

    #[test]
    fn from_missing_follows_the_type_not_its_spelling() {
        type MaybePort = Option<u16>;
        assert_eq!(MaybePort::from_missing(), Some(None));
        assert_eq!(<Box<Option<u8>>>::from_missing(), Some(Box::new(None)));
        assert_eq!(u16::from_missing(), None);
        assert_eq!(<Vec<Option<u8>>>::from_missing(), None);
    }

    #[test]
    fn integer_shapes_match_the_type_bounds() {
        fn bounds<T: FromValue>() -> (i128, i128, &'static str) {
            let Shape::Int { min, max, name } = T::SHAPE else { panic!("{:?}", T::SHAPE) };
            (*min, *max, name)
        }
        assert_eq!(bounds::<i8>(), (-128, 127, "i8"));
        assert_eq!(bounds::<u8>(), (0, 255, "u8"));
        assert_eq!(bounds::<u16>(), (0, i128::from(u16::MAX), "u16"));
        assert_eq!(bounds::<i32>(), (i128::from(i32::MIN), i128::from(i32::MAX), "i32"));
        assert_eq!(bounds::<u64>(), (0, i128::from(u64::MAX), "u64"));
        assert_eq!(bounds::<i128>(), (i128::MIN, i128::MAX, "i128"));
        assert_eq!(bounds::<u128>(), (0, i128::MAX, "u128"));
        assert_eq!(
            bounds::<isize>(),
            (i128::try_from(isize::MIN).unwrap(), i128::try_from(isize::MAX).unwrap(), "isize")
        );
        assert_eq!(bounds::<usize>(), (0, i128::try_from(usize::MAX).unwrap(), "usize"));
    }

    #[test]
    fn float_shapes_bound_what_narrows_finitely() {
        let Shape::Float { max, name: "f64" } = f64::SHAPE else { panic!() };
        assert_eq!(max.to_bits(), f64::MAX.to_bits());
        let Shape::Float { max, name: "f32" } = f32::SHAPE else { panic!() };
        assert!(conv::<f32>(&Value::Float(*max)).unwrap().is_finite());
        assert!(conv::<f32>(&Value::Float(-*max)).unwrap().is_finite());
        assert!(conv::<f32>(&Value::Float(f64::from_bits(max.to_bits() + 1))).is_err());
        assert!(*max > f64::from(f32::MAX));
    }

    #[test]
    fn composite_shapes() {
        // A type without its own shape still accepts anything.
        struct Custom;
        impl FromValue for Custom {
            fn from_value(_: &Value, _: &mut PathStack) -> Result<Self, ConfigError> {
                Ok(Self)
            }
        }
        assert!(matches!(<Option<Custom>>::SHAPE, Shape::Optional(Shape::Any)));

        assert!(matches!(bool::SHAPE, Shape::Bool));
        assert!(matches!(String::SHAPE, Shape::String));
        assert!(matches!(PathBuf::SHAPE, Shape::Path));
        assert!(matches!(char::SHAPE, Shape::Char));
        assert!(matches!(Datetime::SHAPE, Shape::Datetime));
        assert!(matches!(<Vec<Option<u16>>>::SHAPE, Shape::Seq(Shape::Optional(Shape::Int { name: "u16", .. }))));
        assert!(matches!(<HashSet<String>>::SHAPE, Shape::Seq(Shape::String)));
        assert!(matches!(<BTreeMap<String, f32>>::SHAPE, Shape::Map(Shape::Float { name: "f32", .. })));
        assert!(matches!(<Arc<Box<char>>>::SHAPE, Shape::Char));
        assert!(matches!(<(u8, String)>::SHAPE, Shape::Tuple([Shape::Int { name: "u8", .. }, Shape::String])));
    }

    #[test]
    fn deep_error_path() {
        let table = |key: &str, value: Value| Value::Map(IndexMap::from([(key.to_owned(), value)]));
        let replicas = Value::Seq(vec![
            table("port", Value::Int(5432)),
            table("port", Value::Int(5433)),
            table("port", Value::Int(70_000)),
        ]);
        let doc = table("database", table("replicas", replicas));

        let err = conv::<HashMap<String, HashMap<String, Vec<HashMap<String, u16>>>>>(&doc).unwrap_err();
        assert!(matches!(err, ConfigError::OutOfRange { ref path, .. } if path == "database.replicas[2].port"));
        assert_eq!(err.to_string(), "`database.replicas[2].port`: 70000 is out of range for u16");
    }
}
