use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(tag = "type", untagged)]
enum Mode {
    Dev,
    Prod,
}

fn main() {}
