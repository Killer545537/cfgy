use cfgy::FromConfig;

#[derive(FromConfig)]
enum Mode {
    Dev,
    Prod,
}

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_enum_unknown_variant.toml")]
struct Settings {
    mode: Mode,
}

fn main() {}
