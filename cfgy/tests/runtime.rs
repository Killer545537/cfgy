use cfgy::{FromConfig, FromValue, IndexMap, PathStack, Value};

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
