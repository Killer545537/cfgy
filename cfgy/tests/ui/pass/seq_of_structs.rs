use cfgy::FromConfig;

#[derive(FromConfig, Debug, PartialEq)]
struct Server {
    host: String,
    port: u16,
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "../../../../cfgy/tests/ui/pass/seq_of_structs.toml")]
struct Cluster {
    servers: Vec<Server>,
}

fn main() {
    let cluster = Cluster::load_str(include_str!("seq_of_structs.toml"), &cfgy::Toml).unwrap();
    let servers =
        vec![Server { host: "10.0.0.2".to_owned(), port: 8081 }, Server { host: "10.0.0.3".to_owned(), port: 8082 }];
    assert_eq!(cluster, Cluster { servers });
}
