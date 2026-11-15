use cfgy::FromConfig;

// A mistake inside a nested struct is a build error, with its line and full path.
#[derive(FromConfig)]
struct Database {
    host: String,
    port: u16,
}

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_nested_mismatch.toml")]
struct Settings {
    name: String,
    database: Database,
}

fn main() {}
