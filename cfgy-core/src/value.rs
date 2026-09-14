use indexmap::IndexMap;

/// A datetime as written in the source file.
///
/// ponytail: kept as the original text so core has no date dependency; parse it with a date crate if you need arithmetic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Datetime(pub String);

/// Whether `s` is a TOML datetime: a full date `YYYY-MM-DD`, a local time `HH:MM:SS` with an optional fraction, or a
/// date and a time joined by `T`, `t` or a space, with an optional offset `Z`, `z` or `±HH:MM`.
///
/// Digit counts and field ranges are checked (month 1-12, day 1-31, hour 0-23, minute 0-59, second 0-60) but the
/// calendar is not, so `2023-02-31` passes. A `const fn` so the derive's compile-time check can reuse it.
#[must_use]
pub const fn is_datetime(s: &str) -> bool {
    match date(s.as_bytes()) {
        Some([]) => true,
        Some([b'T' | b't' | b' ', rest @ ..]) => match time(rest) {
            Some(rest) => offset(rest),
            None => false,
        },
        Some(_) => false,
        None => matches!(time(s.as_bytes()), Some([])),
    }
}

/// Parses `YYYY-MM-DD` off the front of `b`, returning the rest.
const fn date(b: &[u8]) -> Option<&[u8]> {
    match b {
        [y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2, rest @ ..]
            if y1.is_ascii_digit()
                && y2.is_ascii_digit()
                && y3.is_ascii_digit()
                && y4.is_ascii_digit()
                && matches!((m1, m2), (b'0', b'1'..=b'9') | (b'1', b'0'..=b'2'))
                && matches!((d1, d2), (b'0', b'1'..=b'9') | (b'1'..=b'2', b'0'..=b'9') | (b'3', b'0'..=b'1')) =>
        {
            Some(rest)
        }
        _ => None,
    }
}

/// Parses `HH:MM:SS` with an optional `.fraction` off the front of `b`, returning the rest.
const fn time(b: &[u8]) -> Option<&[u8]> {
    let rest = match b {
        [h1, h2, b':', m1, m2, b':', s1, s2, rest @ ..]
            if is_hour(*h1, *h2) && is_minute(*m1, *m2) && (is_minute(*s1, *s2) || (*s1 == b'6' && *s2 == b'0')) =>
        {
            rest
        }
        _ => return None,
    };
    let mut rest = match rest {
        [b'.', digit, tail @ ..] if digit.is_ascii_digit() => tail,
        [b'.', ..] => return None,
        _ => return Some(rest),
    };
    while let [digit, tail @ ..] = rest {
        if !digit.is_ascii_digit() {
            break;
        }
        rest = tail;
    }
    Some(rest)
}

/// Whether `b` is exactly nothing, `Z`, `z`, or a `±HH:MM` offset.
const fn offset(b: &[u8]) -> bool {
    match b {
        [] | [b'Z' | b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => is_hour(*h1, *h2) && is_minute(*m1, *m2),
        _ => false,
    }
}

const fn is_hour(tens: u8, ones: u8) -> bool {
    matches!((tens, ones), (b'0'..=b'1', b'0'..=b'9') | (b'2', b'0'..=b'3'))
}

const fn is_minute(tens: u8, ones: u8) -> bool {
    matches!((tens, ones), (b'0'..=b'5', b'0'..=b'9'))
}

/// The canonical model every format lowers into.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Seq(Vec<Self>),
    Map(IndexMap<String, Self>),
    Datetime(Datetime),
}

impl Value {
    /// Human-readable name of the variant, for error messages.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "integer",
            Self::Float(_) => "float",
            Self::Str(_) => "string",
            Self::Seq(_) => "sequence",
            Self::Map(_) => "table",
            Self::Datetime(_) => "datetime",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_toml_datetimes() {
        for s in [
            "1979-05-27",
            "07:32:00",
            "23:59:60.999999",
            "00:00:00.0",
            "1979-05-27T07:32:00Z",
            "1979-05-27t07:32:00z",
            "1979-05-27 07:32:00",
            "1979-05-27T07:32:00.123-08:00",
            "1979-05-27T00:32:00+23:59",
            "0000-12-31T23:59:59",
            "2023-02-31",
        ] {
            assert!(is_datetime(s), "{s}");
        }
    }

    #[test]
    fn rejects_everything_else() {
        for s in [
            "",
            "hello",
            "1979",
            "79-05-27",
            "1979-5-27",
            "1979-00-27",
            "1979-13-27",
            "1979-05-00",
            "1979-05-32",
            "1979-05-27T",
            "1979-05-27 ",
            "1979-05-27X07:32:00",
            "1979-05-27T07:32",
            "7:32:00",
            "07:32",
            "24:00:00",
            "07:60:00",
            "07:32:61",
            "07:32:00.",
            "07:32:00.x",
            "07:32:00Z",
            "1979-05-27T07:32:00+8:00",
            "1979-05-27T07:32:00+24:00",
            "1979-05-27T07:32:00+08:60",
            "1979-05-27T07:32:00+08:00Z",
            "1979-05-27T07:32:00ZZ",
            "1979-05-27T07:32:00 ",
            "\u{661}979-05-27",
        ] {
            assert!(!is_datetime(s), "{s}");
        }
    }

    #[test]
    fn works_in_const() {
        const { assert!(is_datetime("1979-05-27T07:32:00Z")) };
        const { assert!(!is_datetime("yesterday")) };
    }
}
