//! Serialize generated `Value` trees to each format, parse them back with the adapter, and compare.
#![cfg(any(feature = "toml", feature = "json", feature = "yaml"))]

use cfgy_core::{Format, IndexMap, Value};
use proptest::prelude::*;

/// A document root: a map whose values nest up to three levels deep.
fn document(null: bool) -> impl Strategy<Value = IndexMap<String, Value>> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Int),
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
        let value = Value::Map(map);
        prop_assert_eq!(cfgy_formats::Toml.parse(&text), Ok(value), "{}", text);
    }

    #[test]
    #[cfg(feature = "json")]
    fn json(map in document(true)) {
        let value = Value::Map(map);
        let text = inline(&value, ": ");
        prop_assert_eq!(cfgy_formats::Json.parse(&text), Ok(value), "{}", text);
    }

    #[test]
    #[cfg(feature = "yaml")]
    fn yaml(map in document(true)) {
        let value = Value::Map(map);
        let text = inline(&value, ": ");
        prop_assert_eq!(cfgy_formats::Yaml.parse(&text), Ok(value), "{}", text);
    }
}
