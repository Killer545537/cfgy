use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "../../../../cfgy/tests/ui/fail/file_malformed_toml.toml")]
struct Settings {
    host: String,
    port: u16,
}

fn main() {}
