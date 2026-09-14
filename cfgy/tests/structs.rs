use cfgy::{ConfigError, FromConfig, FromValue, IndexMap, PathStack, Value};

fn table(entries: &[(&str, Value)]) -> Value {
    Value::Map(entries.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect::<IndexMap<_, _>>())
}

fn load<T: FromValue>(value: &Value) -> Result<T, ConfigError> {
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

type MaybePort = Option<u16>;

#[derive(FromConfig, Debug, PartialEq)]
struct Optionals {
    host: String,
    port: MaybePort,
    level: Box<Option<u8>>,
}

#[test]
fn absent_keys_are_optional_through_aliases_and_pointers() {
    let optionals: Optionals = load(&table(&[("host", Value::Str("a".to_owned()))])).unwrap();
    assert_eq!(optionals, Optionals { host: "a".to_owned(), port: None, level: Box::new(None) });

    let value = table(&[("host", Value::Str("a".to_owned())), ("port", Value::Int(80)), ("level", Value::Null)]);
    let optionals: Optionals = load(&value).unwrap();
    assert_eq!(optionals, Optionals { host: "a".to_owned(), port: Some(80), level: Box::new(None) });
}

#[test]
fn absent_required_key_is_missing() {
    let err = load::<Optionals>(&table(&[])).unwrap_err();
    assert!(matches!(err, ConfigError::Missing { ref path, .. } if path == "host"), "{err:?}");
}

#[test]
fn null_into_a_required_field_is_a_type_error() {
    let err = load::<Optionals>(&table(&[("host", Value::Null)])).unwrap_err();
    assert!(matches!(err, ConfigError::Type { ref path, found: "null", .. } if path == "host"), "{err:?}");
    let value = table(&[("host", Value::Str("a".to_owned())), ("port", Value::Bool(true))]);
    let err = load::<Optionals>(&value).unwrap_err();
    assert!(matches!(err, ConfigError::Type { ref path, expected: "u16", .. } if path == "port"), "{err:?}");
}

#[derive(FromConfig, Debug, PartialEq)]
struct Port(u16);

#[derive(FromConfig, Debug, PartialEq)]
struct MaybeLevel(Option<u8>);

#[derive(FromConfig, Debug, PartialEq)]
struct Endpoint(String, Port);

#[derive(FromConfig, Debug, PartialEq)]
struct Marker;

#[derive(FromConfig, Debug, PartialEq)]
struct Shapes {
    port: Port,
    level: MaybeLevel,
    endpoint: Endpoint,
    marker: Option<Marker>,
}

#[test]
fn newtype_is_transparent() {
    assert_eq!(load::<Port>(&Value::Int(8080)).unwrap(), Port(8080));
    let err = load::<Port>(&Value::Int(70_000)).unwrap_err();
    assert!(matches!(err, ConfigError::OutOfRange { target: "u16", .. }), "{err:?}");
    assert_eq!(Port::from_missing(), None);
    assert_eq!(MaybeLevel::from_missing(), Some(MaybeLevel(None)));
}

#[test]
fn tuple_struct_reads_an_exact_length_sequence() {
    let pair = Value::Seq(vec![Value::Str("a".to_owned()), Value::Int(1)]);
    assert_eq!(load::<Endpoint>(&pair).unwrap(), Endpoint("a".to_owned(), Port(1)));

    let err = load::<Endpoint>(&Value::Seq(vec![Value::Str("a".to_owned())])).unwrap_err();
    assert!(matches!(err, ConfigError::Length { expected: 2, found: 1, .. }), "{err:?}");
    let err = load::<Endpoint>(&table(&[])).unwrap_err();
    assert!(matches!(err, ConfigError::Type { expected: "sequence", found: "table", .. }), "{err:?}");
    let bad = Value::Seq(vec![Value::Str("a".to_owned()), Value::Str("b".to_owned())]);
    let err = load::<Endpoint>(&bad).unwrap_err();
    assert!(matches!(err, ConfigError::Type { ref path, expected: "u16", .. } if path == "[1]"), "{err:?}");
}

#[test]
fn unit_struct_reads_null() {
    assert_eq!(load::<Marker>(&Value::Null).unwrap(), Marker);
    let err = load::<Marker>(&Value::Bool(true)).unwrap_err();
    assert!(matches!(err, ConfigError::Type { expected: "null", found: "bool", .. }), "{err:?}");
}

#[test]
fn struct_kinds_nest_inside_named_structs() {
    let value = table(&[
        ("port", Value::Int(80)),
        ("endpoint", Value::Seq(vec![Value::Str("h".to_owned()), Value::Int(81)])),
        ("marker", Value::Null),
    ]);
    let shapes: Shapes = load(&value).unwrap();
    assert_eq!(
        shapes,
        Shapes { port: Port(80), level: MaybeLevel(None), endpoint: Endpoint("h".to_owned(), Port(81)), marker: None }
    );

    let value = table(&[("endpoint", Value::Seq(vec![Value::Str("h".to_owned()), Value::Int(81)]))]);
    let err = load::<Shapes>(&value).unwrap_err();
    assert!(matches!(err, ConfigError::Missing { ref path, .. } if path == "port"), "{err:?}");
}
