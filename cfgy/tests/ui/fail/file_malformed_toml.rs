use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "fixtures/fail/file_malformed_toml.toml")]
struct Settings {
    host: String,
    port: u16,
}

fn main() {}
