use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(format = "toml")]
struct Settings {
    port: u16,
}

fn main() {}
