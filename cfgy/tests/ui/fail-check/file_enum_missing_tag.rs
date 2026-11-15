use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(tag = "type")]
enum Backend {
    Offline,
    Server { host: String },
}

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_enum_missing_tag.toml")]
struct Settings {
    backend: Backend,
}

fn main() {}
