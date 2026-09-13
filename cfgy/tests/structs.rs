use cfgy::{FromConfig, FromValue, IndexMap, PathStack, Value};

fn table(entries: &[(&str, Value)]) -> Value {
    Value::Map(entries.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect::<IndexMap<_, _>>())
}

fn load<T: FromValue>(value: &Value) -> Result<T, cfgy::ConfigError> {
    T::from_value(value, &mut PathStack::new())
}

#[derive(FromConfig, Debug, PartialEq)]
struct Defaults {
    #[config(default = "localhost")]
    host: String,
    #[config(default)]
    tags: Vec<String>,
    #[config(default = 30)]
    timeout_secs: u64,
    #[config(default = 30)]
    retry_secs: Option<u64>,
}

#[test]
fn defaults_fill_absent_keys() {
    let defaults: Defaults = load(&table(&[])).unwrap();
    assert_eq!(
        defaults,
        Defaults { host: "localhost".to_owned(), tags: Vec::new(), timeout_secs: 30, retry_secs: Some(30) }
    );
}

#[test]
fn present_keys_override_defaults() {
    let value = table(&[
        ("host", Value::Str("example.com".to_owned())),
        ("tags", Value::Seq(vec![Value::Str("a".to_owned())])),
        ("timeout_secs", Value::Int(5)),
        ("retry_secs", Value::Null),
    ]);
    let defaults: Defaults = load(&value).unwrap();
    assert_eq!(
        defaults,
        Defaults { host: "example.com".to_owned(), tags: vec!["a".to_owned()], timeout_secs: 5, retry_secs: None }
    );
}
