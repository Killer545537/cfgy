use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    hash::{BuildHasher, Hash},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};

use indexmap::IndexMap;

use crate::{ConfigError, Datetime, PathStack, Value};

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

// ponytail: integers widen into floats only through a lossless `From` (i32 for f64, i16 for f32); larger ones are
// rejected as out of range rather than silently rounded. Write `1.0e10` for big float literals.
impl FromValue for f64 {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Float(f) => Ok(*f),
            Value::Int(i) => i32::try_from(*i).map(Self::from).map_err(|_| ConfigError::out_of_range(path, *i, "f64")),
            _ => Err(ConfigError::type_mismatch(path, "f64", value)),
        }
    }
}

impl FromValue for f32 {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            // `as` is banned and std has no checked f64 -> f32, so go through the shortest round-trip decimal, which
            // parses to the nearest f32. A finite value too large for f32 would become infinite, so it is rejected.
            Value::Float(f) => match f.to_string().parse::<Self>() {
                Ok(narrowed) if narrowed.is_finite() || !f.is_finite() => Ok(narrowed),
                _ => Err(ConfigError::type_mismatch(path, "f32", value)),
            },
            Value::Int(i) => i16::try_from(*i).map(Self::from).map_err(|_| ConfigError::out_of_range(path, *i, "f32")),
            _ => Err(ConfigError::type_mismatch(path, "f32", value)),
        }
    }
}

impl FromValue for bool {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Bool(b) => Ok(*b),
            _ => Err(ConfigError::type_mismatch(path, "bool", value)),
        }
    }
}

impl FromValue for String {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Str(s) => Ok(s.clone()),
            _ => Err(ConfigError::type_mismatch(path, "string", value)),
        }
    }
}

impl FromValue for PathBuf {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Str(s) => Ok(Self::from(s)),
            _ => Err(ConfigError::type_mismatch(path, "path", value)),
        }
    }
}

impl FromValue for char {
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
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        seq(value, path)
    }
}

impl<T: FromValue> FromValue for VecDeque<T> {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        seq(value, path)
    }
}

impl<T: FromValue + Eq + Hash, S: BuildHasher + Default> FromValue for HashSet<T, S> {
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
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        map(value, path)
    }
}

impl<T: FromValue> FromValue for BTreeMap<String, T> {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        map(value, path)
    }
}

impl<T: FromValue, S: BuildHasher + Default> FromValue for IndexMap<String, T, S> {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        map(value, path)
    }
}

macro_rules! impl_tuple {
    ($len:literal => $(($t:ident $item:ident $i:literal)),+) => {
        impl<$($t: FromValue),+> FromValue for ($($t,)+) {
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
            fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
                T::from_value(value, path).map(Self::new)
            }
        }
    )*};
}

impl_pointer!(Box, Rc, Arc);

impl FromValue for Datetime {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Datetime(d) => Ok(d.clone()),
            Value::Str(s) => Ok(Self(s.clone())),
            _ => Err(ConfigError::type_mismatch(path, "datetime", value)),
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Null => Ok(None),
            _ => T::from_value(value, path).map(Some),
        }
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

    #[test]
    fn floats() {
        assert!((conv::<f64>(&Value::Float(1.5)).unwrap() - 1.5).abs() < f64::EPSILON);
        assert!((conv::<f64>(&Value::Int(-3)).unwrap() + 3.0).abs() < f64::EPSILON);
        assert!(matches!(conv::<f64>(&Value::Int(i64::MAX)), Err(ConfigError::OutOfRange { target: "f64", .. })));
        assert!(matches!(conv::<f64>(&Value::Str("1.0".into())), Err(ConfigError::Type { found: "string", .. })));
        assert!((conv::<f32>(&Value::Float(0.1)).unwrap() - 0.1).abs() < f32::EPSILON);
        assert!((conv::<f32>(&Value::Int(7)).unwrap() - 7.0).abs() < f32::EPSILON);
        assert!(conv::<f32>(&Value::Float(f64::INFINITY)).unwrap().is_infinite());
        assert!(matches!(conv::<f32>(&Value::Float(1.0e300)), Err(ConfigError::Type { expected: "f32", .. })));
        assert!(matches!(conv::<f32>(&Value::Int(40_000)), Err(ConfigError::OutOfRange { target: "f32", .. })));
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
        assert!(matches!(conv::<Datetime>(&Value::Int(0)), Err(ConfigError::Type { expected: "datetime", .. })));
    }

    #[test]
    fn options() {
        let items = Value::Seq(vec![Value::Int(1), Value::Null, Value::Int(3)]);
        assert_eq!(conv::<Vec<Option<u8>>>(&items).unwrap(), [Some(1), None, Some(3)]);
        assert!(matches!(conv::<Option<u8>>(&Value::Str("x".into())), Err(ConfigError::Type { expected: "u8", .. })));
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
