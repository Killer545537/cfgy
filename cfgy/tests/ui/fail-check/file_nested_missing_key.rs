use cfgy::FromConfig;

// A required key missing from a nested table is reported at that table.
#[derive(FromConfig)]
struct Database {
    host: String,
    port: u16,
}

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_nested_missing_key.toml")]
struct Settings {
    name: String,
    database: Database,
}

fn main() {}
