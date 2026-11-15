use cfgy::FromConfig;

// Distinct in Rust, but both become `readonly` after `rename_all`.
#[derive(FromConfig)]
#[config(rename_all = "lowercase")]
enum Access {
    ReadOnly,
    Readonly,
}

fn main() {}
