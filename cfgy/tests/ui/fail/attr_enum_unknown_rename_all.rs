use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(rename_all = "Title Case")]
enum Mode {
    Dev,
    Prod,
}

fn main() {}
