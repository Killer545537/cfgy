use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(rename = "settings")]
struct Settings {
    port: u16,
}

fn main() {}
