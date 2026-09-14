//! One document written in each format: the values agree and each path's span starts on the expected line.
#![cfg(all(feature = "toml", feature = "json", feature = "yaml"))]

use cfgy_core::{Format, Parsed, Segment, line_col};

const TOML: &str = r#"name = "app"
[server]
host = "localhost"
ports = [80, 443]
[[workers]]
id = 1
[[workers]]
id = 2
"#;

const JSON: &str = r#"{
  "name": "app",
  "server": {
    "host": "localhost",
    "ports": [80, 443]
  },
  "workers": [
    {"id": 1},
    {"id": 2}
  ]
}
"#;

const YAML: &str = "name: app
server:
  host: localhost
  ports: [80, 443]
workers:
  - id: 1
  - id: 2
";

fn key(k: &str) -> Segment {
    Segment::Key(k.to_owned())
}

#[test]
fn spans_point_at_the_same_values_in_every_format() {
    let toml: Parsed = cfgy_formats::Toml.parse(TOML).unwrap();
    let json = cfgy_formats::Json.parse(JSON).unwrap();
    let yaml = cfgy_formats::Yaml.parse(YAML).unwrap();
    assert_eq!(toml.value, json.value);
    assert_eq!(toml.value, yaml.value);

    // (path, TOML line, JSON line, YAML line)
    let cases: [(Vec<Segment>, usize, usize, usize); 9] = [
        (vec![], 1, 1, 1),
        (vec![key("name")], 1, 2, 1),
        (vec![key("server")], 2, 3, 3),
        (vec![key("server"), key("host")], 3, 4, 3),
        (vec![key("server"), key("ports"), Segment::Index(1)], 4, 5, 4),
        (vec![key("workers")], 5, 7, 6),
        (vec![key("workers"), Segment::Index(0)], 5, 8, 6),
        (vec![key("workers"), Segment::Index(1)], 7, 9, 7),
        (vec![key("workers"), Segment::Index(1), key("id")], 8, 9, 7),
    ];
    for (path, toml_line, json_line, yaml_line) in cases {
        for (name, parsed, source, line) in
            [("toml", &toml, TOML, toml_line), ("json", &json, JSON, json_line), ("yaml", &yaml, YAML, yaml_line)]
        {
            let span = parsed.spans.get(&path).unwrap_or_else(|| panic!("{name}: no span for {path:?}"));
            let (actual, _) = line_col(source, span.start).unwrap();
            assert_eq!(actual, line, "{name}: {path:?} starts at {span:?}");
        }
    }
}

#[test]
fn value_errors_point_into_the_source() {
    let source = "server:\n  é: 1\n  2: x\n";
    let err = cfgy_formats::Yaml.parse(source).unwrap_err();
    assert_eq!(err.line_col(source), Some((3, 3)), "{err:?}");
    let source = "[server]\n\"é\" = 1\nport = 99999999999999999999\n";
    let err = cfgy_formats::Toml.parse(source).unwrap_err();
    assert_eq!(err.line_col(source), Some((3, 8)), "{err:?}");
}
