use cfgy::FromConfig;

// Mistakes in different nested tables are all reported, not just the first.
#[derive(FromConfig)]
struct Server {
    host: String,
    port: u16,
}

#[derive(FromConfig)]
#[config(path = "fixtures/fail-check/file_nested_two_errors.toml")]
struct Cluster {
    primary: Server,
    replica: Server,
}

fn main() {}
