use cfgy::FromConfig;

#[derive(FromConfig, Debug, PartialEq)]
struct Server {
    host: String,
    #[config(default = 80)]
    port: u16,
}

/// Externally tagged: a bare string for a unit variant, else a single-key table.
#[derive(FromConfig, Debug, PartialEq)]
#[config(rename_all = "snake_case")]
enum Listener {
    Disabled,
    LocalPort(u16),
    RemoteHost { host: String },
}

/// Internally tagged: the tag sits beside the variant's fields.
#[derive(FromConfig, Debug, PartialEq)]
#[config(tag = "type", rename_all = "kebab-case")]
enum Backend {
    Offline,
    StaticServer(Server),
    DynamicServer { discovery_url: String },
}

/// Adjacently tagged: the tag and the payload sit under two keys.
#[derive(FromConfig, Debug, PartialEq)]
#[config(tag = "t", content = "c", rename_all = "SCREAMING_SNAKE_CASE")]
enum Probe {
    NoProbe,
    TcpPort(u16),
    HttpGet { path: String },
}

/// Untagged: the first variant whose shape fits wins. A unit variant matches only null, which TOML cannot write.
#[derive(FromConfig, Debug, PartialEq)]
#[config(untagged)]
enum Fallback {
    Nothing,
    Port(u16),
    Server { host: String },
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "fixtures/pass/enums.toml")]
struct Settings {
    listeners: Vec<Listener>,
    backends: Vec<Backend>,
    probes: Vec<Probe>,
    fallbacks: Vec<Fallback>,
}

fn main() {
    let settings = Settings::load_str(include_str!("enums.toml"), &cfgy::Toml).unwrap();
    assert_eq!(
        settings,
        Settings {
            listeners: vec![
                Listener::Disabled,
                Listener::LocalPort(8080),
                Listener::RemoteHost { host: "example.com".to_owned() },
            ],
            backends: vec![
                Backend::Offline,
                Backend::StaticServer(Server { host: "10.0.0.1".to_owned(), port: 80 }),
                Backend::DynamicServer { discovery_url: "http://consul".to_owned() },
            ],
            probes: vec![Probe::NoProbe, Probe::TcpPort(9000), Probe::HttpGet { path: "/health".to_owned() }],
            fallbacks: vec![Fallback::Port(443), Fallback::Server { host: "db".to_owned() }],
        }
    );
}
