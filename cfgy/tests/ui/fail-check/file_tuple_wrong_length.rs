use cfgy::FromConfig;

#[derive(FromConfig)]
struct Endpoint(String, u16);

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_tuple_wrong_length.toml")]
struct Settings {
    upstream: Endpoint,
}

fn main() {}
