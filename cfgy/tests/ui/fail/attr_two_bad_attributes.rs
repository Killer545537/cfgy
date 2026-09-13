use cfgy::FromConfig;

// Both errors must be reported, not just the first.
#[derive(FromConfig)]
#[config(rename = "settings")]
struct Settings {
    #[config(path = "config.toml")]
    port: u16,
}

fn main() {}
