use std::ops::Range;

use ::toml::{
    Spanned,
    de::{DeTable, DeValue},
};
use cfgy_core::{Datetime, Format, ParseError, Parsed, PathStack, Spans, Value};

/// TOML, via the `toml` crate.
///
/// Spans: the root covers the whole source; a `[table]` or `[[array]]` element starts at its header, an
/// implicit parent table (`a` in `[a.b]` or `a.b = 1`) at its key, and every other value at its first byte.
#[derive(Debug, Clone, Copy)]
pub struct Toml;

impl Format for Toml {
    fn name(&self) -> &'static str {
        "toml"
    }

    fn parse(&self, source: &str) -> Result<Parsed, ParseError> {
        let table = DeTable::parse(source).map_err(|err| ParseError {
            message: err.message().to_owned(),
            offset: err.span().map(|span| span.start),
        })?;
        let mut spans = Spans::default();
        let mut path = PathStack::new();
        spans.insert(&path, 0..source.len());
        let value = lower_table(table.into_inner(), &mut path, &mut spans)?;
        Ok(Parsed { value, spans })
    }
}

fn lower_table(table: DeTable<'_>, path: &mut PathStack, spans: &mut Spans) -> Result<Value, ParseError> {
    table
        .into_iter()
        .map(|(key, value)| {
            let value = path.with_key(key.get_ref(), |path| lower(value, path, spans))?;
            Ok((key.into_inner().into_owned(), value))
        })
        .collect::<Result<_, _>>()
        .map(Value::Map)
}

fn lower(value: Spanned<DeValue<'_>>, path: &mut PathStack, spans: &mut Spans) -> Result<Value, ParseError> {
    let span = value.span();
    spans.insert(path, span.clone());
    Ok(match value.into_inner() {
        DeValue::String(s) => Value::Str(s.into_owned()),
        // Integers and floats are range-checked with the same messages as deserializing into `toml::Table`.
        DeValue::Integer(i) => match i64::from_str_radix(i.as_str(), i.radix()) {
            Ok(i) => Value::Int(i128::from(i)),
            Err(_) if u64::from_str_radix(i.as_str(), i.radix()).is_ok() => {
                return Err(error("u64 value was too large", &span));
            }
            Err(_) => return Err(error("integer number overflowed", &span)),
        },
        DeValue::Float(f) => match f.as_str().parse::<f64>() {
            Ok(parsed) if parsed.is_finite() || parsed.is_nan() || f.as_str().contains("inf") => Value::Float(parsed),
            _ => return Err(error("floating-point number overflowed", &span)),
        },
        DeValue::Boolean(b) => Value::Bool(b),
        DeValue::Datetime(d) => Value::Datetime(Datetime(d.to_string())),
        DeValue::Array(items) => Value::Seq(
            items
                .into_iter()
                .enumerate()
                .map(|(index, item)| path.with_index(index, |path| lower(item, path, spans)))
                .collect::<Result<_, _>>()?,
        ),
        DeValue::Table(table) => lower_table(table, path, spans)?,
    })
}

fn error(message: &str, span: &Range<usize>) -> ParseError {
    ParseError { message: message.to_owned(), offset: Some(span.start) }
}

#[cfg(test)]
mod tests {
    use cfgy_core::Segment;

    use super::*;

    fn parse(source: &str) -> Value {
        Toml.parse(source).unwrap().value
    }

    fn map(source: &str) -> cfgy_core::IndexMap<String, Value> {
        match parse(source) {
            Value::Map(map) => map,
            other => panic!("expected a map, got {other:?}"),
        }
    }

    #[test]
    fn int_vs_float() {
        assert_eq!(map("port = 8080")["port"], Value::Int(8080));
        assert_eq!(map("ratio = 1.0")["ratio"], Value::Float(1.0));
    }

    #[test]
    fn datetime() {
        assert_eq!(
            map("at = 1979-05-27T07:32:00Z")["at"],
            Value::Datetime(Datetime("1979-05-27T07:32:00Z".to_owned()))
        );
    }

    #[test]
    fn nested_and_ordered() {
        let map = map("z = 1\na = [true, \"s\"]\n[m]\ny = 2\nb = 3");
        assert_eq!(map.keys().collect::<Vec<_>>(), ["z", "a", "m"]);
        assert_eq!(map["a"], Value::Seq(vec![Value::Bool(true), Value::Str("s".to_owned())]));
        let Value::Map(m) = &map["m"] else { panic!("expected a table") };
        assert_eq!(m.keys().collect::<Vec<_>>(), ["y", "b"]);
    }

    #[test]
    fn malformed_offset() {
        let source = "a = 1\nb = = 2";
        let err = Toml.parse(source).unwrap_err();
        assert_eq!(err.line_col(source), Some((2, 5)), "{err:?}");
    }

    /// The previous lowering, through `toml::Table`, as the reference for values and errors.
    fn reference(source: &str) -> Result<Value, (String, Option<usize>)> {
        fn lower(value: ::toml::Value) -> Value {
            match value {
                ::toml::Value::String(s) => Value::Str(s),
                ::toml::Value::Integer(i) => Value::Int(i128::from(i)),
                ::toml::Value::Float(f) => Value::Float(f),
                ::toml::Value::Boolean(b) => Value::Bool(b),
                ::toml::Value::Datetime(d) => Value::Datetime(Datetime(d.to_string())),
                ::toml::Value::Array(items) => Value::Seq(items.into_iter().map(lower).collect()),
                ::toml::Value::Table(table) => Value::Map(table.into_iter().map(|(k, v)| (k, lower(v))).collect()),
            }
        }
        source
            .parse::<::toml::Table>()
            .map(|table| lower(::toml::Value::Table(table)))
            .map_err(|err| (err.message().to_owned(), err.span().map(|span| span.start)))
    }

    #[test]
    fn matches_toml_table() {
        let cases = [
            "a = 1\nb = = 2",
            "a = 1\na = 2",
            "[a]\n[a]",
            "a.b = 1\na = 2",
            "a = 9223372036854775807\nb = -9223372036854775808\nc = 0x7fffffffffffffff",
            "a = 9223372036854775808",
            "a = 0xffffffffffffffff",
            "a = 99999999999999999999999999999999999999999",
            "a = 1e999",
            "a = inf\nb = -inf\nc = 1_000.5\nd = 0o17\ne = 0b101\nf = 'lit'\ng = 1979-05-27",
            "[[a]]\nx = 1\n[a.b]\ny = 2\n[[a]]\n",
        ];
        for source in cases {
            let ours = Toml.parse(source).map(|parsed| parsed.value).map_err(|err| (err.message, err.offset));
            assert_eq!(ours, reference(source), "{source}");
        }
        let Value::Map(nan) = parse("n = nan") else { panic!("expected a map") };
        assert!(matches!(nan["n"], Value::Float(f) if f.is_nan()));
    }

    fn start(spans: &Spans, source: &str, path: &[Segment]) -> Option<(usize, usize)> {
        cfgy_core::line_col(source, spans.get(path)?.start)
    }

    fn key(k: &str) -> Segment {
        Segment::Key(k.to_owned())
    }

    #[test]
    fn spans() {
        let source = "\
title = \"x\"
inline = { host = \"h\", ports = [1, 2] }
[server.tls]
port = 8080
[[workers]]
name = \"a\"
[[workers]]
name = \"b\"
";
        let Parsed { spans, .. } = Toml.parse(source).unwrap();
        assert_eq!(spans.get(&[]), Some(&(0..source.len())));
        assert_eq!(start(&spans, source, &[key("title")]), Some((1, 9)));
        // Inline tables and their values start at their first byte.
        assert_eq!(start(&spans, source, &[key("inline")]), Some((2, 10)));
        assert_eq!(start(&spans, source, &[key("inline"), key("host")]), Some((2, 19)));
        assert_eq!(start(&spans, source, &[key("inline"), key("ports"), Segment::Index(1)]), Some((2, 36)));
        // A header table starts at its header; its implicit parent at the parent's key inside that header.
        assert_eq!(start(&spans, source, &[key("server"), key("tls")]), Some((3, 1)));
        assert_eq!(start(&spans, source, &[key("server")]), Some((3, 2)));
        assert_eq!(start(&spans, source, &[key("server"), key("tls"), key("port")]), Some((4, 8)));
        // Array-of-tables: the array at its first header, each element at its own header.
        assert_eq!(start(&spans, source, &[key("workers")]), Some((5, 1)));
        assert_eq!(start(&spans, source, &[key("workers"), Segment::Index(1)]), Some((7, 1)));
        assert_eq!(start(&spans, source, &[key("workers"), Segment::Index(1), key("name")]), Some((8, 8)));
        assert_eq!(spans.get(&[key("missing")]), None);
    }

    #[test]
    fn value_errors_carry_offsets() {
        let source = "a = 1\nb = 9223372036854775808";
        let err = Toml.parse(source).unwrap_err();
        assert_eq!(err.line_col(source), Some((2, 5)), "{err:?}");
    }
}
