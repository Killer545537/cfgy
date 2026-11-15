use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "fixtures/fail/file_missing.toml")]
struct Settings {
    port: u16,
}

fn main() {}
