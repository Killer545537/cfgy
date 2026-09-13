use cfgy::FromConfig;

// Rejected at build time even though `check = false` skips reading the file.
#[derive(FromConfig)]
#[config(path = "settings.conf", format = "ini", check = false)]
struct Settings {
    port: u16,
}

fn main() {}
