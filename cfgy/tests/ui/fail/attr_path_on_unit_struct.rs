use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "config.toml")]
struct Marker;

fn main() {}
