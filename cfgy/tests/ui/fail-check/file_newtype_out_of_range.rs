use cfgy::FromConfig;

// A newtype is checked as its inner type, so the range error names `u16`.
#[derive(FromConfig)]
struct Port(u16);

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_newtype_out_of_range.toml")]
struct Settings {
    port: Port,
}

fn main() {}
