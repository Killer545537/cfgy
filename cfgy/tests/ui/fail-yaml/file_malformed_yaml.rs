use cfgy::FromConfig;

#[derive(FromConfig)]
#[config(path = "../../../../cfgy/tests/ui/fail-yaml/file_malformed_yaml.yaml")]
struct Settings {
    host: String,
    port: u16,
}

fn main() {}
