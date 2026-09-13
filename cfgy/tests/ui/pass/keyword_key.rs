use cfgy::FromConfig;

// Two ways to read the keyword key `type`.
#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "../../../../cfgy/tests/ui/pass/keyword_key.toml")]
struct Renamed {
    #[config(rename = "type")]
    kind: String,
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "../../../../cfgy/tests/ui/pass/keyword_key.toml")]
struct RawIdent {
    r#type: String,
}

fn main() {
    let source = include_str!("keyword_key.toml");
    assert_eq!(Renamed::load_str(source, &cfgy::Toml).unwrap(), Renamed { kind: "web".to_owned() });
    assert_eq!(RawIdent::load_str(source, &cfgy::Toml).unwrap(), RawIdent { r#type: "web".to_owned() });
}
