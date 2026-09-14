use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_type_mismatch.toml")]
struct Settings {
    port: u16,
}

fn main() {}
