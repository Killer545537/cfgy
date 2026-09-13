use cfgy::FromConfig;

#[derive(FromConfig)]
struct Settings {
    #[config(path = "config.toml")]
    port: u16,
}

fn main() {}
