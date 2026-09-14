use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_missing_key.toml")]
struct Settings {
    host: String,
    port: u16,
}

fn main() {}
