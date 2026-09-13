use cfgy::FromConfig;

// `check = false` skips the compile-time read, so the file need not exist; `load` still exists and fails at runtime.
#[derive(FromConfig, Debug)]
#[config(path = "does/not/exist.toml", check = false)]
struct Unchecked {
    port: u16,
}

fn main() {
    assert!(Unchecked::load().is_err());
    let unchecked = Unchecked::load_str("port = 8080", &cfgy::Toml).unwrap();
    assert_eq!(unchecked.port, 8080);
}
