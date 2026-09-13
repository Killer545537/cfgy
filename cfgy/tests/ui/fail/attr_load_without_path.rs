use cfgy::FromConfig;

// Without `#[config(path = "...")]` there is no file to load, so `load` must not exist.
#[derive(FromConfig)]
struct Settings {
    port: u16,
}

fn main() {
    let _ = Settings::load();
}
