use cfgy::FromConfig;

// Optional through an alias: the shape is `Optional`, so an absent key is not "missing".
type MaybePort = Option<u16>;

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "fixtures/pass/option_alias_absent.toml")]
struct Server {
    host: String,
    port: MaybePort,
}

fn main() {
    let server = Server::load_str(include_str!("option_alias_absent.toml"), &cfgy::Toml).unwrap();
    assert_eq!(server, Server { host: "localhost".to_owned(), port: None });
}
