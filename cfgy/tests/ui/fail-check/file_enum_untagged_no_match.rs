use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(untagged)]
enum Fallback {
    Port(u16),
    Server { host: String },
}

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_enum_untagged_no_match.toml")]
struct Settings {
    fallback: Fallback,
}

fn main() {}
