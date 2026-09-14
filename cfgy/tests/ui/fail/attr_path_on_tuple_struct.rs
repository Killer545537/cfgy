use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "config.toml")]
struct Port(u16);

fn main() {}
