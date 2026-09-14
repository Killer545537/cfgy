use cfgy::FromConfig;

// A present key with a null value is not an absent key: a required field rejects it.
#[derive(FromConfig)]
#[config(path = "fixtures/fail-check-json/file_null_required.json")]
struct Settings {
    host: String,
    port: Option<u16>,
}

fn main() {}
