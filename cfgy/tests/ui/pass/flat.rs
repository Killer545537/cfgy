use cfgy::{ConfigError, FromConfig};

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "../../../../cfgy/tests/ui/pass/flat.toml")]
struct Flat {
    name: String,
    port: u16,
    debug: bool,
}

fn main() {
    // `load()` reads relative to the working directory, so only its signature is exercised here.
    let _: Result<Flat, ConfigError> = Flat::load();

    let flat = Flat::load_str(include_str!("flat.toml"), &cfgy::Toml).unwrap();
    assert_eq!(flat, Flat { name: "api".to_owned(), port: 8080, debug: true });
}
