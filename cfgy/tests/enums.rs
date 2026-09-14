use cfgy::check::{Node, conforms};
use cfgy::{ConfigError, Field, FromConfig, FromValue, IndexMap, PathStack, Repr, Shape, Value, Variant};

fn table(entries: &[(&str, Value)]) -> Value {
    Value::Map(entries.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect::<IndexMap<_, _>>())
}

fn str(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn load<T: FromValue>(value: &Value) -> Result<T, ConfigError> {
    T::from_value(value, &mut PathStack::new())
}

/// The error message for `value`, which must not convert.
#[allow(clippy::unwrap_used, reason = "a test helper that fails the test on success")]
fn message<T: FromValue + std::fmt::Debug>(value: &Value) -> String {
    load::<T>(value).unwrap_err().to_string()
}

#[derive(FromConfig, Debug, PartialEq)]
struct Server {
    host: String,
    #[config(default = 80)]
    port: u16,
}

#[derive(FromConfig, Debug, PartialEq)]
enum External {
    Off,
    Port(u16),
    Pair(String, u8),
    Serve {
        host: String,
        #[config(rename = "p", default = 80)]
        port: u16,
        tls: Option<bool>,
    },
    Wrapped(Server),
}

#[test]
fn externally_tagged() {
    assert_eq!(load::<External>(&str("Off")).unwrap(), External::Off);
    assert_eq!(load::<External>(&table(&[("Off", Value::Null)])).unwrap(), External::Off);
    assert_eq!(load::<External>(&table(&[("Port", Value::Int(8))])).unwrap(), External::Port(8));
    let pair = table(&[("Pair", Value::Seq(vec![str("a"), Value::Int(1)]))]);
    assert_eq!(load::<External>(&pair).unwrap(), External::Pair("a".to_owned(), 1));
    let serve = table(&[("Serve", table(&[("host", str("h"))]))]);
    assert_eq!(load::<External>(&serve).unwrap(), External::Serve { host: "h".to_owned(), port: 80, tls: None });
    let wrapped = table(&[("Wrapped", table(&[("host", str("h")), ("port", Value::Int(1))]))]);
    assert_eq!(load::<External>(&wrapped).unwrap(), External::Wrapped(Server { host: "h".to_owned(), port: 1 }));
}

#[test]
fn externally_tagged_errors() {
    assert_eq!(
        message::<External>(&str("Turbo")),
        "`<root>`: unknown variant `Turbo` of External, expected one of `Off`, `Port`, `Pair`, `Serve`, `Wrapped`"
    );
    assert_eq!(
        message::<External>(&table(&[("Turbo", Value::Null)])),
        "`<root>`: unknown variant `Turbo` of External, expected one of `Off`, `Port`, `Pair`, `Serve`, `Wrapped`"
    );
    assert_eq!(message::<External>(&str("Port")), "`<root>`: expected a variant of External, found string");
    assert_eq!(message::<External>(&Value::Int(1)), "`<root>`: expected a variant of External, found integer");
    assert_eq!(message::<External>(&table(&[])), "`<root>`: expected a variant of External, found table");
    let two = table(&[("Off", Value::Null), ("Port", Value::Int(1))]);
    assert_eq!(message::<External>(&two), "`<root>`: expected a variant of External, found table");
    assert_eq!(message::<External>(&table(&[("Off", Value::Int(1))])), "`Off`: expected null, found integer");
    assert_eq!(message::<External>(&table(&[("Port", str("x"))])), "`Port`: expected u16, found string");
    let pair = table(&[("Pair", Value::Seq(vec![str("a"), str("b")]))]);
    assert_eq!(message::<External>(&pair), "`Pair[1]`: expected u8, found string");
    let pair = table(&[("Pair", Value::Seq(vec![str("a")]))]);
    assert_eq!(message::<External>(&pair), "`Pair`: expected a sequence of 2 items, found 1");
    assert_eq!(message::<External>(&table(&[("Serve", table(&[]))])), "missing required key `Serve.host`");
    let serve = table(&[("Serve", table(&[("host", str("h")), ("p", Value::Bool(true))]))]);
    assert_eq!(message::<External>(&serve), "`Serve.p`: expected u16, found bool");
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(tag = "type")]
enum Internal {
    Off,
    Serve { host: String, port: Option<u16> },
    Wrapped(Server),
}

#[test]
fn internally_tagged() {
    assert_eq!(load::<Internal>(&table(&[("type", str("Off")), ("junk", Value::Null)])).unwrap(), Internal::Off);
    let serve = table(&[("type", str("Serve")), ("host", str("h"))]);
    assert_eq!(load::<Internal>(&serve).unwrap(), Internal::Serve { host: "h".to_owned(), port: None });
    let wrapped = table(&[("host", str("h")), ("type", str("Wrapped"))]);
    assert_eq!(load::<Internal>(&wrapped).unwrap(), Internal::Wrapped(Server { host: "h".to_owned(), port: 80 }));
}

#[test]
fn internally_tagged_errors() {
    assert_eq!(
        message::<Internal>(&table(&[("type", str("On"))])),
        "`<root>`: unknown variant `On` of Internal, expected one of `Off`, `Serve`, `Wrapped`"
    );
    assert_eq!(message::<Internal>(&table(&[])), "missing required key `type`");
    assert_eq!(message::<Internal>(&table(&[("type", Value::Int(1))])), "`type`: expected string, found integer");
    assert_eq!(message::<Internal>(&str("Off")), "`<root>`: expected a variant of Internal, found string");
    assert_eq!(message::<Internal>(&table(&[("type", str("Serve"))])), "missing required key `host`");
    let serve = table(&[("type", str("Serve")), ("host", str("h")), ("port", str("x"))]);
    assert_eq!(message::<Internal>(&serve), "`port`: expected u16, found string");
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(tag = "t", content = "c")]
enum Adjacent {
    Off,
    Port(u16),
    Level(Option<u8>),
    Pair(String, u8),
    Serve { host: String },
}

#[test]
fn adjacently_tagged() {
    assert_eq!(load::<Adjacent>(&table(&[("t", str("Off"))])).unwrap(), Adjacent::Off);
    assert_eq!(load::<Adjacent>(&table(&[("t", str("Off")), ("c", Value::Null)])).unwrap(), Adjacent::Off);
    assert_eq!(load::<Adjacent>(&table(&[("c", Value::Int(8)), ("t", str("Port"))])).unwrap(), Adjacent::Port(8));
    assert_eq!(load::<Adjacent>(&table(&[("t", str("Level"))])).unwrap(), Adjacent::Level(None));
    let pair = table(&[("t", str("Pair")), ("c", Value::Seq(vec![str("a"), Value::Int(2)]))]);
    assert_eq!(load::<Adjacent>(&pair).unwrap(), Adjacent::Pair("a".to_owned(), 2));
    let serve = table(&[("t", str("Serve")), ("c", table(&[("host", str("h"))])), ("other", Value::Null)]);
    assert_eq!(load::<Adjacent>(&serve).unwrap(), Adjacent::Serve { host: "h".to_owned() });
}

#[test]
fn adjacently_tagged_errors() {
    assert_eq!(
        message::<Adjacent>(&table(&[("t", str("x"))])),
        "`<root>`: unknown variant `x` of Adjacent, expected one of `Off`, `Port`, `Level`, `Pair`, `Serve`"
    );
    assert_eq!(message::<Adjacent>(&table(&[("c", Value::Int(1))])), "missing required key `t`");
    assert_eq!(message::<Adjacent>(&table(&[("t", str("Port"))])), "missing required key `c`");
    assert_eq!(message::<Adjacent>(&table(&[("t", str("Serve"))])), "missing required key `c`");
    assert_eq!(
        message::<Adjacent>(&table(&[("t", str("Off")), ("c", Value::Int(1))])),
        "`c`: expected null, found integer"
    );
    assert_eq!(message::<Adjacent>(&table(&[("t", str("Port")), ("c", str("1"))])), "`c`: expected u16, found string");
    let serve = table(&[("t", str("Serve")), ("c", table(&[]))]);
    assert_eq!(message::<Adjacent>(&serve), "missing required key `c.host`");
    assert_eq!(
        message::<Adjacent>(&Value::Null),
        "`<root>`: expected a variant of Adjacent, found null (the key is present but has no value)"
    );
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(untagged)]
enum Untagged {
    Off,
    Port(u16),
    Pair(String, u8),
    Serve { host: String },
    Name(String),
}

#[test]
fn untagged_tries_variants_in_order() {
    assert_eq!(load::<Untagged>(&Value::Null).unwrap(), Untagged::Off);
    assert_eq!(load::<Untagged>(&Value::Int(8)).unwrap(), Untagged::Port(8));
    assert_eq!(
        load::<Untagged>(&Value::Seq(vec![str("a"), Value::Int(1)])).unwrap(),
        Untagged::Pair("a".to_owned(), 1)
    );
    assert_eq!(load::<Untagged>(&table(&[("host", str("h"))])).unwrap(), Untagged::Serve { host: "h".to_owned() });
    assert_eq!(load::<Untagged>(&str("Off")).unwrap(), Untagged::Name("Off".to_owned()));
    assert_eq!(message::<Untagged>(&Value::Int(-1)), "`<root>`: expected a variant of Untagged, found integer");
    assert_eq!(message::<Untagged>(&table(&[])), "`<root>`: expected a variant of Untagged, found table");
    // A failed attempt leaves the path as it was.
    let mut path = PathStack::new();
    let err = path.with_key("mode", |path| Untagged::from_value(&Value::Seq(vec![str("a")]), path)).unwrap_err();
    assert_eq!(err.to_string(), "`mode`: expected a variant of Untagged, found sequence");
    assert_eq!(path, PathStack::new());
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(rename_all = "kebab-case")]
enum Renamed {
    VeryFast,
    #[config(rename = "slow!")]
    Slow,
    #[config(rename_all = "SCREAMING_SNAKE_CASE")]
    WithFields {
        max_conns: u8,
    },
    PlainFields {
        max_conns: u8,
    },
}

#[test]
fn rename_all_renames_variants_and_variant_rename_all_renames_fields() {
    assert_eq!(load::<Renamed>(&str("very-fast")).unwrap(), Renamed::VeryFast);
    assert_eq!(load::<Renamed>(&str("slow!")).unwrap(), Renamed::Slow);
    let fields = table(&[("with-fields", table(&[("MAX_CONNS", Value::Int(3))]))]);
    assert_eq!(load::<Renamed>(&fields).unwrap(), Renamed::WithFields { max_conns: 3 });
    let fields = table(&[("plain-fields", table(&[("max_conns", Value::Int(4))]))]);
    assert_eq!(load::<Renamed>(&fields).unwrap(), Renamed::PlainFields { max_conns: 4 });
    assert_eq!(
        message::<Renamed>(&str("VeryFast")),
        "`<root>`: unknown variant `VeryFast` of Renamed, expected one of `very-fast`, `slow!`, `with-fields`, \
         `plain-fields`"
    );
}

#[derive(FromConfig, Debug, PartialEq)]
struct Modes {
    mode: External,
    fallback: Option<Internal>,
}

#[test]
fn enums_nest_in_structs_with_paths() {
    let value = table(&[("mode", str("Off"))]);
    assert_eq!(load::<Modes>(&value).unwrap(), Modes { mode: External::Off, fallback: None });
    assert_eq!(
        message::<Modes>(&table(&[("mode", str("turbo"))])),
        "`mode`: unknown variant `turbo` of External, expected one of `Off`, `Port`, `Pair`, `Serve`, `Wrapped`"
    );
    let value = table(&[("mode", table(&[("Serve", table(&[("host", Value::Int(1))]))]))]);
    assert_eq!(message::<Modes>(&value), "`mode.Serve.host`: expected string, found integer");
    let value = table(&[("mode", str("Off")), ("fallback", table(&[("type", str("Serve"))]))]);
    assert_eq!(message::<Modes>(&value), "missing required key `fallback.host`");
}

#[derive(FromConfig, Debug, PartialEq)]
#[allow(clippy::use_self, reason = "the derive must detect recursion spelled with the type name")]
enum Expr {
    Lit(i64),
    Add(Box<Expr>, Box<Expr>),
    Neg { inner: Box<Self> },
}

#[derive(FromConfig, Debug)]
enum Empty {}

#[test]
fn recursive_and_empty_enums() {
    let value = table(&[(
        "Add",
        Value::Seq(vec![
            table(&[("Lit", Value::Int(1))]),
            table(&[("Neg", table(&[("inner", table(&[("Lit", Value::Int(2))]))]))]),
        ]),
    )]);
    assert_eq!(
        load::<Expr>(&value).unwrap(),
        Expr::Add(Box::new(Expr::Lit(1)), Box::new(Expr::Neg { inner: Box::new(Expr::Lit(2)) }))
    );
    assert_eq!(message::<Empty>(&str("A")), "`<root>`: unknown variant `A` of Empty, expected one of ");
    assert_eq!(
        message::<Empty>(&Value::Null),
        "`<root>`: expected a variant of Empty, found null (the key is present but has no value)"
    );
}

/// `value` as a const-check [`Node`], leaked so it is `'static` like the macro's embedded tree.
fn node(value: &Value) -> Node {
    let leak = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
    match value {
        Value::Null => Node::Null,
        Value::Bool(b) => Node::Bool(*b),
        Value::Int(i) => Node::Int(*i),
        Value::Float(f) => Node::Float(*f),
        Value::Str(s) => Node::Str(leak(s)),
        Value::Datetime(datetime) => Node::Datetime(leak(&datetime.0)),
        Value::Seq(items) => Node::Seq(Box::leak(items.iter().map(node).collect())),
        Value::Map(entries) => {
            Node::Map(Box::leak(entries.iter().map(|(key, value)| (leak(key), node(value))).collect()))
        }
    }
}

/// Asserts that the shape of `T` accepts exactly the `values` that `T::from_value` converts, and that both
/// verdicts occur.
fn assert_parity<T: FromValue>(values: &[Value]) {
    let mut verdicts = [false; 2];
    for value in values {
        let converts = load::<T>(value).is_ok();
        assert_eq!(conforms(T::SHAPE, &node(value)), converts, "{value:?}");
        if let Some(seen) = verdicts.get_mut(usize::from(converts)) {
            *seen = true;
        }
    }
    assert_eq!(verdicts, [true, true], "cover both accepted and rejected values");
}

#[test]
fn shapes_accept_what_from_value_accepts() {
    let server = table(&[("host", str("h"))]);
    assert_parity::<External>(&[
        str("Off"),
        str("Port"),
        str("Nope"),
        Value::Null,
        Value::Int(1),
        table(&[]),
        table(&[("Off", Value::Null)]),
        table(&[("Off", Value::Int(1))]),
        table(&[("Port", Value::Int(8))]),
        table(&[("Port", Value::Int(-8))]),
        table(&[("Port", Value::Int(8)), ("Off", Value::Null)]),
        table(&[("Pair", Value::Seq(vec![str("a"), Value::Int(1)]))]),
        table(&[("Pair", Value::Seq(vec![str("a")]))]),
        table(&[("Serve", server.clone())]),
        table(&[("Serve", table(&[("host", str("h")), ("p", str("80"))]))]),
        table(&[("Serve", table(&[]))]),
        table(&[("Wrapped", server.clone())]),
        table(&[("Wrapped", Value::Seq(vec![]))]),
        table(&[("Nope", Value::Null)]),
    ]);
    assert_parity::<Internal>(&[
        table(&[("type", str("Off")), ("junk", Value::Null)]),
        table(&[("type", str("Serve")), ("host", str("h"))]),
        table(&[("type", str("Serve")), ("host", str("h")), ("port", Value::Null)]),
        table(&[("type", str("Serve")), ("host", str("h")), ("port", str("x"))]),
        table(&[("type", str("Serve"))]),
        table(&[("type", str("Wrapped")), ("host", str("h"))]),
        table(&[("type", str("Wrapped")), ("host", Value::Int(1))]),
        table(&[("type", str("Nope"))]),
        table(&[("type", Value::Int(1))]),
        table(&[]),
        str("Off"),
    ]);
    assert_parity::<Adjacent>(&[
        table(&[("t", str("Off"))]),
        table(&[("t", str("Off")), ("c", Value::Null)]),
        table(&[("t", str("Off")), ("c", Value::Int(1))]),
        table(&[("t", str("Port")), ("c", Value::Int(1))]),
        table(&[("t", str("Port")), ("c", str("1"))]),
        table(&[("t", str("Port"))]),
        table(&[("t", str("Level"))]),
        table(&[("t", str("Level")), ("c", Value::Int(300))]),
        table(&[("t", str("Pair")), ("c", Value::Seq(vec![str("a"), Value::Int(2)]))]),
        table(&[("t", str("Pair")), ("c", Value::Seq(vec![]))]),
        table(&[("c", server.clone()), ("t", str("Serve"))]),
        table(&[("t", str("Serve"))]),
        table(&[("t", str("Nope")), ("c", Value::Null)]),
        table(&[("c", Value::Null)]),
        Value::Seq(vec![]),
    ]);
    assert_parity::<Untagged>(&[
        Value::Null,
        Value::Int(8),
        Value::Int(-8),
        Value::Seq(vec![str("a"), Value::Int(1)]),
        Value::Seq(vec![str("a")]),
        server,
        table(&[("port", Value::Int(1))]),
        str("Off"),
        Value::Bool(true),
        Value::Float(1.5),
    ]);
    assert_parity::<Renamed>(&[
        str("very-fast"),
        str("VeryFast"),
        str("slow!"),
        table(&[("with-fields", table(&[("MAX_CONNS", Value::Int(3))]))]),
        table(&[("with-fields", table(&[("max_conns", Value::Int(3))]))]),
        table(&[("plain-fields", table(&[("max_conns", Value::Int(4))]))]),
    ]);
    assert_parity::<Modes>(&[
        table(&[("mode", str("Off"))]),
        table(&[("mode", str("Off")), ("fallback", table(&[("type", str("Off"))]))]),
        table(&[("mode", str("Off")), ("fallback", table(&[("type", str("Serve"))]))]),
        table(&[("mode", str("turbo"))]),
        table(&[]),
    ]);
}

#[test]
fn derived_enums_describe_their_shape() {
    let Shape::Enum { name: "External", repr: Repr::External, variants } = External::SHAPE else {
        panic!("expected an enum, got {:?}", External::SHAPE);
    };
    let [
        Variant { name: "Off", payload: None },
        Variant { name: "Port", payload: Some(Shape::Int { name: "u16", .. }) },
        Variant { name: "Pair", payload: Some(Shape::Tuple([Shape::String, Shape::Int { name: "u8", .. }])) },
        Variant {
            name: "Serve",
            payload:
                Some(Shape::Table {
                    name: "External::Serve",
                    fields:
                        [
                            Field { key: "host", shape: Shape::String, has_default: false },
                            Field { key: "p", shape: Shape::Int { .. }, has_default: true },
                            Field { key: "tls", shape: Shape::Optional(Shape::Bool), has_default: false },
                        ],
                }),
        },
        Variant { name: "Wrapped", payload: Some(Shape::Table { name: "Server", .. }) },
    ] = variants
    else {
        panic!("unexpected variants: {variants:?}");
    };
    assert!(matches!(Internal::SHAPE, Shape::Enum { repr: Repr::Internal { tag: "type" }, .. }));
    assert!(matches!(Adjacent::SHAPE, Shape::Enum { repr: Repr::Adjacent { tag: "t", content: "c" }, .. }));
    assert!(matches!(Untagged::SHAPE, Shape::Enum { repr: Repr::Untagged, .. }));
    assert!(matches!(
        Renamed::SHAPE,
        Shape::Enum { variants: [Variant { name: "very-fast", .. }, Variant { name: "slow!", .. }, ..], .. }
    ));
    // Recursion through the enum's own name is cut to `Any`, as for structs.
    assert!(matches!(
        Expr::SHAPE,
        Shape::Enum {
            variants: [
                Variant { payload: Some(Shape::Int { .. }), .. },
                Variant { payload: Some(Shape::Tuple([Shape::Any, Shape::Any])), .. },
                Variant { payload: Some(Shape::Table { fields: [Field { shape: Shape::Any, .. }], .. }), .. },
            ],
            ..
        }
    ));
    assert!(matches!(Empty::SHAPE, Shape::Enum { name: "Empty", variants: [], .. }));
}
