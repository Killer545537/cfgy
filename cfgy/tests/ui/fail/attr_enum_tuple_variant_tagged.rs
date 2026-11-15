use cfgy::FromConfig;

// An internally tagged tuple variant has no key to hold the tag, as in serde.
#[derive(FromConfig)]
#[config(tag = "type")]
enum Endpoint {
    Local,
    Remote(String, u16),
}

fn main() {}
