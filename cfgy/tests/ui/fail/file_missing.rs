use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "../../../../cfgy/tests/ui/fail/file_missing.toml")]
struct Settings {
    port: u16,
}

fn main() {}
