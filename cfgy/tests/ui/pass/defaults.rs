use cfgy::FromConfig;

// Every defaulted key is absent from the file, so the compile-time check must accept them as optional.
#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "fixtures/pass/defaults.toml")]
struct Settings {
    name: String,
    #[config(default)]
    tags: Vec<String>,
    #[config(default = "localhost")]
    host: String,
    #[config(default = "replica")]
    fallback: Option<String>,
    #[config(default = 30)]
    timeout_secs: u64,
}

fn main() {
    let settings = Settings::load_str(include_str!("defaults.toml"), &cfgy::Toml).unwrap();
    assert_eq!(
        settings,
        Settings {
            name: "api".to_owned(),
            tags: Vec::new(),
            host: "localhost".to_owned(),
            fallback: Some("replica".to_owned()),
            timeout_secs: 30,
        }
    );
}
