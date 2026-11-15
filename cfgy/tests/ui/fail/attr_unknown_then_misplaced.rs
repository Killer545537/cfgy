use cfgy::FromConfig;

// Parsing continues past an unknown option, so the misplaced option after it is reported too.
#[derive(FromConfig)]
struct Settings {
    #[config(optional, path = "config.toml")]
    port: u16,
}

fn main() {}
