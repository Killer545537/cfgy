use cfgy::FromConfig;

// Every bad field is reported, not just the first.
#[derive(FromConfig)]
#[config(path = "../../../../cfgy/tests/ui/fail/file_two_field_errors.toml")]
struct Settings {
    host: String,
    port: u16,
    debug: bool,
}

fn main() {}
