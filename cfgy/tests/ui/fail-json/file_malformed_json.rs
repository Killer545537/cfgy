use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "fixtures/fail-json/file_malformed_json.json")]
struct Settings {
    host: String,
    port: u16,
}

fn main() {}
