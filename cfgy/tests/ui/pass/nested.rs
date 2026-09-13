use cfgy::FromConfig;

#[derive(FromConfig, Debug, PartialEq)]
struct Database {
    url: String,
    pool: u32,
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "../../../../cfgy/tests/ui/pass/nested.toml")]
struct Settings {
    name: String,
    database: Database,
}

fn main() {
    let settings = Settings::load_str(include_str!("nested.toml"), &cfgy::Toml).unwrap();
    let database = Database { url: "postgres://localhost/app".to_owned(), pool: 8 };
    assert_eq!(settings, Settings { name: "api".to_owned(), database });
}
