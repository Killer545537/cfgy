use std::collections::HashMap;
use std::path::PathBuf;

use cfgy::{ConfigError, FromConfig, FromValue, IndexMap, PathStack, Value};

#[derive(FromConfig, Debug, PartialEq)]
struct Flat {
    name: String,
    port: u16,
}

#[test]
fn derived_from_value_reads_a_flat_map() {
    let value = Value::Map(IndexMap::from([
        ("name".to_owned(), Value::Str("api".to_owned())),
        ("port".to_owned(), Value::Int(8080)),
    ]));
    let flat = Flat::from_value(&value, &mut PathStack::new()).unwrap();
    assert_eq!(flat, Flat { name: "api".to_owned(), port: 8080 });
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "tests/fixtures/flat.toml", check = false)]
struct Loaded {
    name: String,
    port: u16,
}

#[test]
fn load_str_parses_with_the_given_format() {
    let loaded = Loaded::load_str("name = \"api\"\nport = 8080\n", &cfgy::Toml).unwrap();
    assert_eq!(loaded, Loaded { name: "api".to_owned(), port: 8080 });
}

#[test]
fn load_from_reads_a_file() {
    let path = std::env::temp_dir().join(format!("cfgy-load-from-{}.toml", std::process::id()));
    std::fs::write(&path, "name = \"api\"\nport = 8080\n").unwrap();
    let loaded = Loaded::load_from(&path);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(loaded.unwrap(), Loaded { name: "api".to_owned(), port: 8080 });
}

#[derive(FromConfig, Debug, PartialEq)]
struct Server {
    host: String,
    port: u16,
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "tests/fixtures/settings.toml")]
struct Settings {
    #[config(rename = "type")]
    kind: String,
    server: Server,
    replicas: Vec<Server>,
    features: HashMap<String, bool>,
    description: Option<String>,
    /// A native datetime in TOML, a string in JSON and YAML.
    deployed_at: String,
    log_file: Option<PathBuf>,
    #[config(default = 30)]
    timeout_secs: u64,
}

fn server(host: &str, port: u16) -> Server {
    Server { host: host.to_owned(), port }
}

fn expected_settings() -> Settings {
    Settings {
        kind: "web".to_owned(),
        server: server("0.0.0.0", 8080),
        replicas: vec![server("10.0.0.2", 8081), server("10.0.0.3", 8082)],
        features: HashMap::from([("metrics".to_owned(), true), ("tracing".to_owned(), false)]),
        description: Some("primary API".to_owned()),
        deployed_at: "1979-05-27T07:32:00Z".to_owned(),
        log_file: None,
        timeout_secs: 30,
    }
}

#[test]
fn load_reads_the_path_relative_to_the_working_directory() {
    assert_eq!(Settings::load().unwrap(), expected_settings());
}

#[test]
fn toml_fixture_loads() {
    assert_eq!(Settings::load_from("tests/fixtures/settings.toml").unwrap(), expected_settings());
}

#[cfg(feature = "json")]
#[test]
fn json_fixture_matches() {
    assert_eq!(Settings::load_from("tests/fixtures/settings.json").unwrap(), expected_settings());
}

#[cfg(feature = "yaml")]
#[test]
fn yaml_fixture_matches() {
    assert_eq!(Settings::load_from("tests/fixtures/settings.yaml").unwrap(), expected_settings());
}

#[test]
fn missing_file_is_an_io_error() {
    let err = Settings::load_from("tests/fixtures/does-not-exist.toml").unwrap_err();
    assert!(matches!(err, ConfigError::Io { ref path, .. } if path.ends_with("does-not-exist.toml")), "{err:?}");
}

#[test]
fn unknown_extension_is_an_unknown_format_error() {
    let path = std::env::temp_dir().join(format!("cfgy-unknown-format-{}.ini", std::process::id()));
    std::fs::write(&path, "type = web\n").unwrap();
    let err = Settings::load_from(&path);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(err, Err(ConfigError::UnknownFormat { .. })), "{err:?}");
}

#[test]
fn malformed_source_is_a_parse_error_with_line_and_column() {
    let err = Settings::load_str("type = \"web\"\nport = = 1\n", &cfgy::Toml).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { format: "toml", line_col: Some((2, _)), .. }), "{err:?}");
}

const VALID_HEAD: &str = "type = \"web\"\nfeatures = {}\n";

#[test]
fn nested_error_path_includes_the_index() {
    let source = format!(
        "{VALID_HEAD}server = {{ host = \"a\", port = 1 }}\n\
         replicas = [{{ host = \"b\", port = 2 }}, {{ host = \"c\", port = \"eighty\" }}]\n"
    );
    let err = Settings::load_str(&source, &cfgy::Toml).unwrap_err();
    assert_eq!(err.to_string(), "`replicas[1].port`: expected u16, found string");
}

#[test]
fn absent_nested_required_key_is_missing_with_its_full_path() {
    let source = format!("{VALID_HEAD}replicas = []\nserver = {{ host = \"a\" }}\n");
    let err = Settings::load_str(&source, &cfgy::Toml).unwrap_err();
    assert!(matches!(err, ConfigError::Missing { ref path } if path == "server.port"), "{err:?}");
}

#[derive(FromConfig, Debug, PartialEq)]
struct Nullable {
    host: String,
    port: Option<u16>,
}

#[test]
fn null_is_none_for_option_and_a_type_error_for_required() {
    let map = |host: Value| Value::Map(IndexMap::from([("host".to_owned(), host), ("port".to_owned(), Value::Null)]));
    let ok = Nullable::from_value(&map(Value::Str("a".to_owned())), &mut PathStack::new()).unwrap();
    assert_eq!(ok, Nullable { host: "a".to_owned(), port: None });

    let err = Nullable::from_value(&map(Value::Null), &mut PathStack::new()).unwrap_err();
    assert!(matches!(err, ConfigError::Type { ref path, found: "null", .. } if path == "host"), "{err:?}");
}
