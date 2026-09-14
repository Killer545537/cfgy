use cfgy::FromConfig;

// Sequence elements are checked one by one, and named by index.
#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_vec_out_of_range.toml")]
struct Settings {
    levels: Vec<u8>,
}

fn main() {}
