//! Serialize generated `Value` trees to each format, parse them back with the adapter, and compare.
#![cfg(any(feature = "toml", feature = "json", feature = "yaml"))]

use cfgy_core::{Format, IndexMap, Parsed, PathStack, Value};
use proptest::prelude::*;

/// Parses `text` and checks the value matches and every path in it, the root included, has a span.
fn check(format: &dyn Format, text: &str, expected: &Value) -> Result<(), TestCaseError> {
    fn first_unspanned(parsed: &Parsed, value: &Value, path: &mut PathStack) -> Option<String> {
        if parsed.spans.get(path.segments()).is_none() {
            return Some(format!("`{path}`"));
        }
        match value {
            Value::Seq(items) => items
                .iter()
                .enumerate()
                .find_map(|(i, item)| path.with_index(i, |path| first_unspanned(parsed, item, path))),
            Value::Map(map) => map.iter().find_map(|(k, v)| path.with_key(k, |path| first_unspanned(parsed, v, path))),
            _ => None,
        }
    }
    let parsed = format.parse(text).map_err(|err| TestCaseError::fail(format!("{err:?}\n{text}")))?;
    prop_assert_eq!(&parsed.value, expected, "{}", text);
    let missing = first_unspanned(&parsed, &parsed.value, &mut PathStack::new());
    prop_assert!(missing.is_none(), "{} has no span in\n{}", missing.unwrap_or_default(), text);
    Ok(())
}

/// A document root: a map whose values nest up to three levels deep.
fn document(null: bool) -> impl Strategy<Value = IndexMap<String, Value>> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|i| Value::Int(i.into())),
        {
            use proptest::num::f64::{NEGATIVE, NORMAL, POSITIVE, SUBNORMAL, ZERO};
            (POSITIVE | NEGATIVE | NORMAL | SUBNORMAL | ZERO).prop_map(Value::Float)
        },
        // Integral floats must come back as floats, not integers.
        (-1000..1000).prop_map(|i| Value::Float(f64::from(i))),
        "[ -~]{0,12}".prop_map(Value::Str),
    ]
    .boxed();
    let leaf = if null { prop_oneof![leaf, Just(Value::Null)].boxed() } else { leaf };
    let value = leaf.prop_recursive(2, 24, 4, |inner| {
        prop_oneof![prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Seq), map(inner).prop_map(Value::Map)]
    });
    map(value)
}

fn map(value: impl Strategy<Value = Value>) -> impl Strategy<Value = IndexMap<String, Value>> {
    prop::collection::vec(("[a-z][a-z0-9_]{0,6}", value), 0..4).prop_map(|entries| entries.into_iter().collect())
}

/// A double-quoted string; TOML, JSON, and YAML share these two escapes for printable ASCII.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Inline rendering shared by all three formats, which differ only in the key separator (TOML never gets `Null`).
fn inline(value: &Value, separator: &str) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        // `{:?}` always keeps a `.` or an exponent, so integral floats stay floats.
        Value::Float(f) => format!("{f:?}"),
        Value::Str(s) => quoted(s),
        Value::Seq(items) => {
            format!("[{}]", items.iter().map(|v| inline(v, separator)).collect::<Vec<_>>().join(", "))
        }
        Value::Map(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}{separator}{}", quoted(k), inline(v, separator)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Datetime(d) => quoted(&d.0), // never generated
    }
}

#[cfg(feature = "toml")]
fn to_toml(map: &IndexMap<String, Value>) -> String {
    map.iter().map(|(k, v)| format!("{} = {}", quoted(k), inline(v, " = "))).collect::<Vec<_>>().join("\n")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    #[cfg(feature = "toml")]
    fn toml(map in document(false)) {
        let text = to_toml(&map);
        check(&cfgy_formats::Toml, &text, &Value::Map(map))?;
    }

    #[test]
    #[cfg(feature = "json")]
    fn json(map in document(true)) {
        let value = Value::Map(map);
        check(&cfgy_formats::Json, &inline(&value, ": "), &value)?;
    }

    #[test]
    #[cfg(feature = "yaml")]
    fn yaml(map in document(true)) {
        let value = Value::Map(map);
        check(&cfgy_formats::Yaml, &inline(&value, ": "), &value)?;
    }
}
