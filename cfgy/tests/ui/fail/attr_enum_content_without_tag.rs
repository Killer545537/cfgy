use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(content = "value")]
enum Mode {
    Dev,
    Port(u16),
}

fn main() {}
