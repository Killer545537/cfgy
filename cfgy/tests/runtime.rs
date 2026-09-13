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
