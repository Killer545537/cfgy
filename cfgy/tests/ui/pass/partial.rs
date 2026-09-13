use cfgy::FromConfig;

// Only `name` is in the file: `description` is optional and `timeout_secs` has a default.
#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "../../../../cfgy/tests/ui/pass/partial.toml")]
struct Partial {
    name: String,
    description: Option<String>,
    #[config(default = 30)]
    timeout_secs: u64,
}

fn main() {
    let partial = Partial::load_str(include_str!("partial.toml"), &cfgy::Toml).unwrap();
    assert_eq!(partial, Partial { name: "api".to_owned(), description: None, timeout_secs: 30 });
}
