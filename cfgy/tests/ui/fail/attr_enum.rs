use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "config.toml")]
enum Mode {
    Dev,
    Prod,
}

fn main() {}
