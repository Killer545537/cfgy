use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(default = 8080)]
struct Settings {
    port: u16,
}

fn main() {}
