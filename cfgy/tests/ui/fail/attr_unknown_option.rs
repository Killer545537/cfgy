use cfgy::FromConfig;

#[derive(FromConfig)]
struct Settings {
    #[config(optional)]
    port: u16,
}

fn main() {}
