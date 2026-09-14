//! The compile-time walkers must agree with runtime conversion: for generated values, a type's `SHAPE` accepts
//! exactly what its `FromValue` impl accepts, and `check_node` reports a problem somewhere exactly when it does not.
#![allow(clippy::arithmetic_side_effects, reason = "edge-value literals; an overflow there fails to compile")]

use std::{collections::HashMap, panic, path::PathBuf};

use cfgy_core::{
    Datetime, FromValue, IndexMap, PathStack, Shape, Value,
    check::{Node, check_node, conforms},
};
use proptest::prelude::*;

fn leak<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}

fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

/// The const mirror of `value`, leaked so it lives as long as a macro-emitted `static` would.
fn node(value: &Value) -> Node {
    match value {
        Value::Null => Node::Null,
        Value::Bool(b) => Node::Bool(*b),
        Value::Int(i) => Node::Int(*i),
        Value::Float(f) => Node::Float(*f),
        Value::Str(s) => Node::Str(leak_str(s)),
        Value::Datetime(Datetime(s)) => Node::Datetime(leak_str(s)),
        Value::Seq(items) => Node::Seq(leak(items.iter().map(node).collect())),
        Value::Map(entries) => Node::Map(leak(entries.iter().map(|(k, v)| (leak_str(k), node(v))).collect())),
    }
}

/// Every index path in `node`, the root included.
fn paths(node: &Node, prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    out.push(prefix.clone());
    let children: Vec<&Node> = match node {
        Node::Seq(items) => items.iter().collect(),
        Node::Map(entries) => entries.iter().map(|(_, v)| v).collect(),
        _ => Vec::new(),
    };
    for (i, child) in children.into_iter().enumerate() {
        prefix.push(i);
        paths(child, prefix, out);
        prefix.pop();
    }
}

fn int() -> impl Strategy<Value = i128> {
    let edges = [
        i128::MIN,
        i128::MIN + 1,
        i128::MAX,
        -i128::from(u64::MAX),
        i128::from(i64::MIN) - 1,
        i128::from(i64::MIN),
        i128::from(i64::MAX),
        i128::from(u64::MAX),
        i128::from(u64::MAX) + 1,
        i128::from(i8::MIN) - 1,
        i128::from(i8::MIN),
        i128::from(i8::MAX),
        i128::from(i8::MAX) + 1,
        255,
        256,
        i128::from(u16::MAX),
        i128::from(u16::MAX) + 1,
        (1 << 53) + 1,
        -1,
        0,
    ];
    prop_oneof![proptest::sample::select(edges.to_vec()), -300_i128..70_000, any::<i128>()]
}

fn float() -> impl Strategy<Value = f64> {
    let edges = [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::MAX,
        f64::from(f32::MAX),
        // The largest f64 narrowing to a finite f32, and the next one up.
        3.402_823_567_797_336_6e38,
        3.402_823_567_797_337e38,
        -3.402_823_567_797_337e38,
        1e300,
        0.5,
        -0.0,
    ];
    prop_oneof![proptest::sample::select(edges.to_vec()), any::<f64>(), -1e3..1e3]
}

fn string() -> impl Strategy<Value = String> {
    prop_oneof![
        "[ -~]{0,4}",
        any::<char>().prop_map(String::from),
        Just("é".to_owned()),
        // Datetime-looking text with random digits, so field ranges pass and fail.
        "[0-9]{4}-[01][0-9]-[0-3][0-9]",
        "[0-2][0-9]:[0-6][0-9]:[0-6][0-9](\\.[0-9]{1,3})?",
        "[0-9]{4}-[01][0-9]-[0-3][0-9][Tt ][0-2][0-9]:[0-6][0-9]:[0-6][0-9](Z|z|[+-][0-2][0-9]:[0-6][0-9])?",
        Just("1979-05-27T07:32:00Z".to_owned()),
    ]
}

fn leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        int().prop_map(Value::Int),
        float().prop_map(Value::Float),
        string().prop_map(Value::Str),
        prop_oneof![Just("1979-05-27".to_owned()), Just("07:32:00".to_owned()), string()]
            .prop_map(|s| Value::Datetime(Datetime(s))),
    ]
}

fn value() -> impl Strategy<Value = Value> {
    leaf().prop_recursive(3, 32, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Seq),
            prop::collection::vec(("[a-c]", inner), 0..4)
                .prop_map(|entries| Value::Map(entries.into_iter().collect::<IndexMap<_, _>>())),
        ]
    })
}

fn assert_parity<T: FromValue>(value: &Value, node: &'static Node) -> Result<(), TestCaseError> {
    let shape: &'static Shape = T::SHAPE;
    let runtime = T::from_value(value, &mut PathStack::new()).is_ok();
    prop_assert_eq!(conforms(shape, node), runtime, "{} on {:?}", std::any::type_name::<T>(), value);

    let mut all = Vec::new();
    paths(node, &mut Vec::new(), &mut all);
    // The walker's panics are expected; keep them out of the output, but restore the hook so failures still print.
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let reported = all.into_iter().any(|path| panic::catch_unwind(|| check_node(shape, node, &path, "")).is_err());
    panic::set_hook(hook);
    prop_assert_eq!(reported, !runtime, "check_node for {} on {:?}", std::any::type_name::<T>(), value);
    Ok(())
}

fn all_types(value: &Value) -> Result<(), TestCaseError> {
    let node: &'static Node = Box::leak(Box::new(node(value)));
    assert_parity::<u8>(value, node)?;
    assert_parity::<i64>(value, node)?;
    assert_parity::<u64>(value, node)?;
    assert_parity::<u128>(value, node)?;
    assert_parity::<f32>(value, node)?;
    assert_parity::<f64>(value, node)?;
    assert_parity::<bool>(value, node)?;
    assert_parity::<String>(value, node)?;
    assert_parity::<PathBuf>(value, node)?;
    assert_parity::<char>(value, node)?;
    assert_parity::<Datetime>(value, node)?;
    assert_parity::<Option<u16>>(value, node)?;
    assert_parity::<Vec<Option<i8>>>(value, node)?;
    assert_parity::<HashMap<String, u16>>(value, node)?;
    assert_parity::<(u8, String)>(value, node)?;
    assert_parity::<Box<f32>>(value, node)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    // Scalars at the root, so edge values meet the scalar types often.
    #[test]
    fn scalar_shapes_accept_what_from_value_accepts(value in leaf()) {
        all_types(&value)?;
    }

    #[test]
    fn shapes_accept_what_from_value_accepts(value in value()) {
        all_types(&value)?;
    }
}
