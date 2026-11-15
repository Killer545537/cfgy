use cfgy::FromConfig;

/// A newtype reads as its inner value.
#[derive(FromConfig, Debug, PartialEq)]
struct Port(u16);

/// A tuple struct reads from a sequence of exactly its length.
#[derive(FromConfig, Debug, PartialEq)]
struct Endpoint(String, Port);

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "fixtures/pass/struct_kinds.toml")]
struct Settings {
    port: Port,
    upstream: Endpoint,
}

fn main() {
    let settings = Settings::load_str(include_str!("struct_kinds.toml"), &cfgy::Toml).unwrap();
    assert_eq!(settings, Settings { port: Port(8080), upstream: Endpoint("db.local".to_owned(), Port(5432)) });
}
