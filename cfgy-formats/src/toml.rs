use cfgy_core::{Datetime, Format, ParseError, Value};

/// TOML, via the `toml` crate.
#[derive(Debug, Clone, Copy)]
pub struct Toml;

impl Format for Toml {
    fn name(&self) -> &'static str {
        "toml"
    }

    fn parse(&self, source: &str) -> Result<Value, ParseError> {
        let table = source.parse::<::toml::Table>().map_err(|err| ParseError {
            message: err.message().to_owned(),
            offset: err.span().map(|span| span.start),
        })?;
        Ok(lower_table(table))
    }
}

fn lower_table(table: ::toml::Table) -> Value {
    Value::Map(table.into_iter().map(|(key, value)| (key, lower(value))).collect())
}

fn lower(value: ::toml::Value) -> Value {
    match value {
        ::toml::Value::String(s) => Value::Str(s),
        ::toml::Value::Integer(i) => Value::Int(i),
        ::toml::Value::Float(f) => Value::Float(f),
        ::toml::Value::Boolean(b) => Value::Bool(b),
        ::toml::Value::Datetime(d) => Value::Datetime(Datetime(d.to_string())),
        ::toml::Value::Array(items) => Value::Seq(items.into_iter().map(lower).collect()),
        ::toml::Value::Table(table) => lower_table(table),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Value {
        Toml.parse(source).unwrap()
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
}
