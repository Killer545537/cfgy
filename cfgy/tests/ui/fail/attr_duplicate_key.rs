use cfgy::FromConfig;

#[derive(FromConfig)]
struct Settings {
    port: u16,
    #[config(rename = "port")]
    listen_port: u16,
}

fn main() {}
