use cfgy::FromConfig;

// The variant is known, but its payload has the wrong type; the path runs through the variant name.
#[derive(FromConfig)]
enum Listener {
    Disabled,
    Port(u16),
}

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_enum_payload_mismatch.toml")]
struct Settings {
    listener: Listener,
}

fn main() {}
