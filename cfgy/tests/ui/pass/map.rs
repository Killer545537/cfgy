use std::collections::HashMap;

use cfgy::FromConfig;

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "../../../../cfgy/tests/ui/pass/map.toml")]
struct Features {
    features: HashMap<String, bool>,
}

fn main() {
    let features = Features::load_str(include_str!("map.toml"), &cfgy::Toml).unwrap();
    let expected = HashMap::from([("metrics".to_owned(), true), ("tracing".to_owned(), false)]);
    assert_eq!(features, Features { features: expected });
}
