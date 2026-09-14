use cfgy::FromConfig;

// The check reads `<Port as FromValue>::SHAPE`, which is `u16`'s: an alias is its target, not a table.
type Port = u16;

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "fixtures/pass/alias_scalar.toml")]
struct Server {
    port: Port,
}

fn main() {
    let server = Server::load_str(include_str!("alias_scalar.toml"), &cfgy::Toml).unwrap();
    assert_eq!(server, Server { port: 8080 });
}
