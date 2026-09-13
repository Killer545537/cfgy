use cfgy::FromConfig;

#[derive(FromConfig)]
enum Mode {
    Dev,
    Prod,
}

fn main() {}
