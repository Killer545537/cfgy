use cfgy_core::{Format, ParseError, Parsed, Value};

/// JSON, via `serde_json`.
#[derive(Debug, Clone, Copy)]
pub struct Json;

impl Format for Json {
    fn name(&self) -> &'static str {
        "json"
    }

    fn parse(&self, source: &str) -> Result<Parsed, ParseError> {
        let value = serde_json::from_str(source).map_err(|err| {
            let message = err.to_string();
            let location = format!(" at line {} column {}", err.line(), err.column());
            ParseError {
                message: message.strip_suffix(&location).unwrap_or(&message).to_owned(),
                offset: offset(source, err.line(), err.column()),
            }
        })?;
        lower(value).map(Parsed::from)
    }
}

/// Byte offset of `serde_json`'s 1-based line and 1-based byte column.
fn offset(source: &str, line: usize, column: usize) -> Option<usize> {
    let line_start: usize = source.split_inclusive('\n').take(line.checked_sub(1)?).map(str::len).sum();
    line_start.checked_add(column.saturating_sub(1))
}

fn lower(value: serde_json::Value) -> Result<Value, ParseError> {
    Ok(match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        // ponytail: serde_json reads integers above u64::MAX as f64, so those arrive as Float, not an error.
        serde_json::Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => Value::Int(i.into()),
            (None, Some(u), _) => Value::Int(u.into()),
            (None, None, Some(f)) => Value::Float(f),
            (None, None, None) => {
                return Err(ParseError { message: format!("number {n} is not representable"), offset: None });
            }
        },
        serde_json::Value::String(s) => Value::Str(s),
        serde_json::Value::Array(items) => Value::Seq(items.into_iter().map(lower).collect::<Result<_, _>>()?),
        serde_json::Value::Object(map) => {
            Value::Map(map.into_iter().map(|(k, v)| Ok((k, lower(v)?))).collect::<Result<_, _>>()?)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Value {
        Json.parse(source).unwrap().value
    }

    #[test]
    fn numbers() {
        assert_eq!(parse("8080"), Value::Int(8080));
        assert_eq!(parse("-1"), Value::Int(-1));
        assert_eq!(parse("8080.0"), Value::Float(8080.0));
        assert_eq!(parse("1e3"), Value::Float(1000.0));
        assert_eq!(parse("18446744073709551615"), Value::Int(i128::from(u64::MAX)));
        assert_eq!(parse("-9223372036854775808"), Value::Int(i128::from(i64::MIN)));
        // Above u64::MAX serde_json yields a float, as serde does.
        assert_eq!(parse("18446744073709551616"), Value::Float(18_446_744_073_709_551_616.0));
    }

    #[test]
    fn scalars_and_order() {
        assert_eq!(parse("null"), Value::Null);
        let Value::Map(map) = parse(r#"{"z": [true, "s"], "a": {}}"#) else { panic!("expected a map") };
        assert_eq!(map.keys().collect::<Vec<_>>(), ["z", "a"]);
        assert_eq!(map["z"], Value::Seq(vec![Value::Bool(true), Value::Str("s".to_owned())]));
        assert_eq!(map["a"], Value::Map(cfgy_core::IndexMap::new()));
    }

    #[test]
    fn malformed_offset() {
        let source = "{\n  \"a\": 1,\n  \"b\": x\n}";
        let err = Json.parse(source).unwrap_err();
        assert_eq!(err.offset, source.find('x'));
        assert_eq!(err.line_col(source), Some((3, 8)));
        assert_eq!(err.message, "expected value");
    }
}
