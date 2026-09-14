//! Const walkers behind the derive's compile-time check of a config file.
//!
//! The macro embeds the parsed file as a [`Node`] tree and emits one `const _: () = check_node(..)` per value, so
//! rustc reports every mismatch as its own const-evaluation error. [`conforms`] is the whole-subtree verdict and
//! follows the same policy as [`FromValue::from_value`](crate::FromValue::from_value); [`check_node`] reports only
//! the problems local to one value, in the same words as [`ConfigError`](crate::ConfigError)'s `Display`.

use crate::{Field, Repr, Shape, Variant, is_datetime};

/// A parsed document as a const tree: the macro's mirror of [`Value`](crate::Value), without spans.
#[derive(Debug)]
pub enum Node {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(&'static str),
    Datetime(&'static str),
    Seq(&'static [Self]),
    Map(&'static [(&'static str, Self)]),
}

impl Node {
    /// The same names as [`Value::kind`](crate::Value::kind).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "integer",
            Self::Float(_) => "float",
            Self::Str(_) => "string",
            Self::Datetime(_) => "datetime",
            Self::Seq(_) => "sequence",
            Self::Map(_) => "table",
        }
    }
}

/// Whether `node` and everything below it converts into a type of shape `shape`.
#[must_use]
pub const fn conforms(shape: &Shape, node: &Node) -> bool {
    match (shape, node) {
        (Shape::Optional(_), Node::Null) => true,
        (Shape::Optional(inner), _) => conforms(inner, node),
        (Shape::Seq(item), Node::Seq(items)) => all_conform(item, items),
        (Shape::Tuple(shapes), Node::Seq(items)) => tuple_conforms(shapes, items),
        (Shape::Map(item), Node::Map(entries)) => values_conform(item, entries),
        (Shape::Table { fields, .. }, Node::Map(entries)) => table_conforms(fields, entries, None),
        (Shape::Enum { repr, variants, .. }, _) => select_variant(*repr, variants, node).is_some(),
        _ => matches!(local(shape, node), Verdict::Ok),
    }
}

/// Checks the value at `path` below `doc` against its shape, found by walking down from `root`.
///
/// Each index in `path` picks a sequence item or a table entry by position. Only problems local to the target are
/// reported (its kind, range or length, a table's required keys, an enum's variant); its children have their own
/// checks. If an ancestor already mismatches, this returns quietly, since the ancestor's own check reports it.
///
/// # Panics
///
/// With a message starting with `prefix` when the value does not fit, which fails the build in const evaluation.
#[allow(clippy::panic, reason = "a const panic is how a compile-time check reports a mismatch")]
pub const fn check_node(root: &Shape, doc: &Node, path: &[usize], prefix: &str) {
    let (mut shape, mut node, mut path) = (root, doc, path);
    loop {
        shape = resolve(shape, node);
        if matches!(shape, Shape::Any) {
            return;
        }
        let [index, rest @ ..] = path else { break };
        // A missing key does not hide the entries that are there, so keep checking below it.
        if !matches!(local(shape, node), Verdict::Ok | Verdict::Missing(_)) {
            return;
        }
        let Some((child_shape, child_node)) = child(shape, node, *index) else { return };
        (shape, node, path) = (child_shape, child_node, rest);
    }

    let mut message = Message::new();
    message.push(prefix);
    match local(shape, node) {
        Verdict::Ok => return,
        Verdict::Type(expected) => {
            message.push(": expected ");
            message.push(expected);
            message.push(", found ");
            message.push(node.kind());
            if matches!(node, Node::Null) {
                message.push(" (the key is present but has no value)");
            }
        }
        Verdict::Range(value, name) => {
            message.push(": ");
            message.push_int(value);
            message.push(" is out of range for ");
            message.push(name);
        }
        Verdict::Length(expected, found) => {
            message.push(": expected a sequence of ");
            message.push_usize(expected);
            message.push(" items, found ");
            message.push_usize(found);
        }
        Verdict::Missing(key) => {
            message.push(": missing required key `");
            message.push(key);
            message.push("`");
        }
        Verdict::UnknownVariant(found, name, variants) => {
            message.push(": unknown variant `");
            message.push(found);
            message.push("` of ");
            message.push(name);
            message.push(", expected one of ");
            let mut rest = variants;
            while let [variant, tail @ ..] = rest {
                message.push("`");
                message.push(variant.name);
                message.push(if tail.is_empty() { "`" } else { "`, " });
                rest = tail;
            }
        }
        Verdict::NoVariant(name) => {
            message.push(": ");
            message.push(node.kind());
            message.push(" matches no variant of ");
            message.push(name);
        }
    }
    panic!("{}", message.as_str())
}

/// What is wrong with a value itself, ignoring its children.
enum Verdict {
    Ok,
    /// The expected word, as in `ConfigError::Type`.
    Type(&'static str),
    Range(i128, &'static str),
    /// Expected and found lengths of a tuple.
    Length(usize, usize),
    /// A required key absent from a table.
    Missing(&'static str),
    /// A tag naming no variant: the tag, the enum's name, and its variants.
    UnknownVariant(&'static str, &'static str, &'static [Variant]),
    /// The enum's name, for a value that names no variant at all.
    NoVariant(&'static str),
}

/// Checks `node` against `shape` without looking at its children, except where the kind itself depends on them
/// (an untagged enum's variant, a single character, a datetime string).
const fn local(shape: &Shape, node: &Node) -> Verdict {
    let ok = match (shape, node) {
        (Shape::Any, _)
        | (Shape::Null | Shape::Optional(_), Node::Null)
        | (Shape::Bool, Node::Bool(_))
        | (Shape::String, Node::Str(_) | Node::Datetime(_))
        | (Shape::Path, Node::Str(_))
        | (Shape::Datetime, Node::Datetime(_))
        | (Shape::Float { .. }, Node::Int(_))
        | (Shape::Seq(_), Node::Seq(_))
        | (Shape::Map(_), Node::Map(_)) => true,
        (Shape::Char, Node::Str(s)) => is_one_char(s),
        (Shape::Datetime, Node::Str(s)) => is_datetime(s),
        (Shape::Float { max, .. }, Node::Float(f)) => !f.is_finite() || f.abs() <= *max,
        (Shape::Int { min, max, name }, Node::Int(i)) => {
            return if *min <= *i && *i <= *max { Verdict::Ok } else { Verdict::Range(*i, name) };
        }
        (Shape::Optional(inner), _) => return local(inner, node),
        (Shape::Tuple(shapes), Node::Seq(items)) => {
            return if shapes.len() == items.len() { Verdict::Ok } else { Verdict::Length(shapes.len(), items.len()) };
        }
        (Shape::Table { fields, .. }, Node::Map(entries)) => return missing(fields, entries),
        (Shape::Enum { name, repr, variants }, _) => {
            let Some(variant) = select_variant_locally(*repr, variants, node) else {
                return match (repr, node) {
                    (Repr::Internal { tag } | Repr::Adjacent { tag, .. }, Node::Map(entries))
                        if entry(entries, tag).is_none() =>
                    {
                        Verdict::Missing(tag)
                    }
                    _ => match unknown_tag(*repr, variants, node) {
                        Some(found) => Verdict::UnknownVariant(found, name, variants),
                        None => Verdict::NoVariant(name),
                    },
                };
            };
            return match (repr, variant.payload, node) {
                (Repr::Internal { .. }, Some(Shape::Table { fields, .. }), Node::Map(entries)) => {
                    missing(fields, entries)
                }
                (Repr::Adjacent { content, .. }, Some(payload), Node::Map(entries))
                    if entry(entries, content).is_none() && !accepts_missing(payload) =>
                {
                    Verdict::Missing(content)
                }
                _ => Verdict::Ok,
            };
        }
        _ => false,
    };
    if ok { Verdict::Ok } else { Verdict::Type(expected(shape)) }
}

/// The word runtime conversion errors use for what `shape` expects.
const fn expected(shape: &Shape) -> &'static str {
    match shape {
        Shape::Any => "anything",
        Shape::Null => "null",
        Shape::Bool => "bool",
        Shape::String => "string",
        Shape::Path => "path",
        Shape::Char => "a single character",
        Shape::Datetime => "datetime",
        Shape::Int { name, .. } | Shape::Float { name, .. } | Shape::Enum { name, .. } => name,
        Shape::Optional(inner) => expected(inner),
        Shape::Seq(_) | Shape::Tuple(_) => "sequence",
        Shape::Map(_) | Shape::Table { .. } => "table",
    }
}

/// Peels what applies to `node` itself rather than to a child: `Option` around a non-null value, and the matching
/// variant of an untagged enum.
const fn resolve<'a>(shape: &'a Shape, node: &'a Node) -> &'a Shape {
    match (shape, node) {
        (Shape::Optional(_), Node::Null) => shape,
        (Shape::Optional(inner), _) => resolve(inner, node),
        (Shape::Enum { repr: Repr::Untagged, variants, .. }, _) => match untagged_variant(variants, node) {
            Some(Variant { payload: Some(payload), .. }) => resolve(payload, node),
            _ => shape,
        },
        _ => shape,
    }
}

/// The shape and node of the `index`th child of `node`, a sequence item or a table entry.
const fn child<'a>(shape: &'a Shape, node: &'a Node, index: usize) -> Option<(&'a Shape, &'a Node)> {
    match (shape, node) {
        (Shape::Seq(item), Node::Seq(items)) => match nth(items, index) {
            Some(item_node) => Some((*item, item_node)),
            None => None,
        },
        (Shape::Tuple(shapes), Node::Seq(items)) => match (nth(shapes, index), nth(items, index)) {
            (Some(item), Some(item_node)) => Some((*item, item_node)),
            _ => None,
        },
        (_, Node::Map(entries)) => match nth(entries, index) {
            Some((key, value)) => Some((entry_shape(shape, entries, key), value)),
            None => None,
        },
        _ => None,
    }
}

/// The shape of the entry `key` in a table-like `shape` whose entries are `entries`. Keys the type ignores get
/// [`Shape::Any`].
const fn entry_shape<'a>(shape: &'a Shape, entries: &'a [(&'static str, Node)], key: &str) -> &'a Shape {
    let payload = match shape {
        Shape::Map(item) => return item,
        Shape::Table { fields, .. } => return field_shape(fields, key),
        // A unit variant's payload, if written at all, is null.
        Shape::Enum { repr: Repr::External, variants, .. } => match find_variant(variants, key) {
            Some(Variant { payload: None, .. }) => return &Shape::Null,
            Some(variant) => variant.payload,
            None => None,
        },
        Shape::Enum { repr: Repr::Internal { tag }, variants, .. } if !str_eq(key, tag) => {
            match tagged_variant(variants, entries, tag) {
                Some(Variant { payload: Some(Shape::Table { fields, .. }), .. }) => return field_shape(fields, key),
                _ => None,
            }
        }
        Shape::Enum { repr: Repr::Adjacent { tag, content }, variants, .. } if str_eq(key, content) => {
            match tagged_variant(variants, entries, tag) {
                Some(Variant { payload: None, .. }) => return &Shape::Null,
                Some(variant) => variant.payload,
                None => None,
            }
        }
        _ => None,
    };
    match payload {
        Some(payload) => payload,
        None => &Shape::Any,
    }
}

/// The variant `node` is written as, if any, checking payloads in full.
const fn select_variant<'a>(repr: Repr, variants: &'a [Variant], node: &Node) -> Option<&'a Variant> {
    let Some(variant) = select_variant_locally(repr, variants, node) else { return None };
    let fits = match (repr, variant.payload, node) {
        (Repr::External, None, Node::Map([(_, value)])) => matches!(value, Node::Null),
        (Repr::Adjacent { content, .. }, None, Node::Map(entries)) => {
            matches!(entry(entries, content), None | Some(Node::Null))
        }
        (_, None, _) | (Repr::Untagged, _, _) | (Repr::Internal { .. }, Some(Shape::Any), _) => true,
        (Repr::External, Some(payload), Node::Map([(_, value)])) => conforms(payload, value),
        (Repr::Internal { tag }, Some(Shape::Table { fields, .. }), Node::Map(entries)) => {
            table_conforms(fields, entries, Some(tag))
        }
        (Repr::Adjacent { content, .. }, Some(payload), Node::Map(entries)) => match entry(entries, content) {
            Some(value) => conforms(payload, value),
            None => accepts_missing(payload),
        },
        _ => false,
    };
    if fits { Some(variant) } else { None }
}

/// The variant `node` names, looking only at the tag (or, when untagged, matching payloads in full).
const fn select_variant_locally<'a>(repr: Repr, variants: &'a [Variant], node: &Node) -> Option<&'a Variant> {
    match (repr, node) {
        (Repr::External, Node::Str(name)) => match find_variant(variants, name) {
            Some(variant) if variant.payload.is_none() => Some(variant),
            _ => None,
        },
        (Repr::External, Node::Map([(name, _)])) => find_variant(variants, name),
        (Repr::Internal { tag }, Node::Map(entries)) => match tagged_variant(variants, entries, tag) {
            Some(variant) if matches!(variant.payload, None | Some(Shape::Table { .. } | Shape::Any)) => Some(variant),
            _ => None,
        },
        (Repr::Adjacent { tag, .. }, Node::Map(entries)) => tagged_variant(variants, entries, tag),
        (Repr::Untagged, _) => untagged_variant(variants, node),
        _ => None,
    }
}

/// The tag `node` is written with when it names none of `variants`.
const fn unknown_tag(repr: Repr, variants: &[Variant], node: &Node) -> Option<&'static str> {
    let found = match (repr, node) {
        (Repr::External, Node::Str(name) | Node::Map([(name, _)])) => name,
        (Repr::Internal { tag } | Repr::Adjacent { tag, .. }, Node::Map(entries)) => match entry(entries, tag) {
            Some(Node::Str(name)) => name,
            _ => return None,
        },
        _ => return None,
    };
    if find_variant(variants, found).is_none() { Some(found) } else { None }
}

/// The first variant whose payload `node` conforms to; a unit variant matches null.
const fn untagged_variant<'a>(variants: &'a [Variant], node: &Node) -> Option<&'a Variant> {
    let mut rest = variants;
    while let [variant, tail @ ..] = rest {
        let fits = match variant.payload {
            None => matches!(node, Node::Null),
            Some(payload) => conforms(payload, node),
        };
        if fits {
            return Some(variant);
        }
        rest = tail;
    }
    None
}

/// The variant named by the string entry `tag` of a table.
const fn tagged_variant<'a>(variants: &'a [Variant], entries: &[(&str, Node)], tag: &str) -> Option<&'a Variant> {
    match entry(entries, tag) {
        Some(Node::Str(name)) => find_variant(variants, name),
        _ => None,
    }
}

const fn find_variant<'a>(variants: &'a [Variant], name: &str) -> Option<&'a Variant> {
    let mut rest = variants;
    while let [variant, tail @ ..] = rest {
        if str_eq(variant.name, name) {
            return Some(variant);
        }
        rest = tail;
    }
    None
}

const fn table_conforms(fields: &[Field], entries: &[(&str, Node)], ignore: Option<&str>) -> bool {
    if !matches!(missing(fields, entries), Verdict::Ok) {
        return false;
    }
    let mut rest = entries;
    while let [(key, value), tail @ ..] = rest {
        let ignored = match ignore {
            Some(ignore) => str_eq(key, ignore),
            None => false,
        };
        if !ignored && !conforms(field_shape(fields, key), value) {
            return false;
        }
        rest = tail;
    }
    true
}

/// [`Verdict::Missing`] for the first required field absent from `entries`.
///
/// A field is required unless it has a default or its shape [accepts absence](accepts_missing).
const fn missing(fields: &[Field], entries: &[(&str, Node)]) -> Verdict {
    let mut rest = fields;
    while let [field, tail @ ..] = rest {
        let required = !field.has_default && !accepts_missing(field.shape);
        if required && entry(entries, field.key).is_none() {
            return Verdict::Missing(field.key);
        }
        rest = tail;
    }
    Verdict::Ok
}

/// Whether a type of shape `shape` can be absent: `Option`, or [`Shape::Any`], whose `from_missing` cannot be known
/// here.
const fn accepts_missing(shape: &Shape) -> bool {
    matches!(shape, Shape::Optional(_) | Shape::Any)
}

/// The shape of field `key`, or [`Shape::Any`] for a key the struct ignores.
const fn field_shape<'a>(fields: &'a [Field], key: &str) -> &'a Shape {
    let mut rest = fields;
    while let [field, tail @ ..] = rest {
        if str_eq(field.key, key) {
            return field.shape;
        }
        rest = tail;
    }
    &Shape::Any
}

const fn all_conform(item: &Shape, items: &[Node]) -> bool {
    let mut rest = items;
    while let [node, tail @ ..] = rest {
        if !conforms(item, node) {
            return false;
        }
        rest = tail;
    }
    true
}

const fn values_conform(item: &Shape, entries: &[(&str, Node)]) -> bool {
    let mut rest = entries;
    while let [(_, node), tail @ ..] = rest {
        if !conforms(item, node) {
            return false;
        }
        rest = tail;
    }
    true
}

const fn tuple_conforms(shapes: &[&Shape], items: &[Node]) -> bool {
    let (mut shapes, mut items) = (shapes, items);
    loop {
        match (shapes, items) {
            ([], []) => return true,
            ([shape, shape_tail @ ..], [node, node_tail @ ..]) if conforms(shape, node) => {
                (shapes, items) = (shape_tail, node_tail);
            }
            _ => return false,
        }
    }
}

const fn entry<'a>(entries: &'a [(&str, Node)], key: &str) -> Option<&'a Node> {
    let mut rest = entries;
    while let [(entry_key, value), tail @ ..] = rest {
        if str_eq(entry_key, key) {
            return Some(value);
        }
        rest = tail;
    }
    None
}

const fn nth<T>(items: &[T], index: usize) -> Option<&T> {
    match items.split_at_checked(index) {
        Some((_, [item, ..])) => Some(item),
        _ => None,
    }
}

const fn str_eq(a: &str, b: &str) -> bool {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a, b) {
            ([], []) => return true,
            ([x, a_tail @ ..], [y, b_tail @ ..]) if *x == *y => (a, b) = (a_tail, b_tail),
            _ => return false,
        }
    }
}

/// Whether `s` is exactly one `char`, read from the UTF-8 length of its first byte.
const fn is_one_char(s: &str) -> bool {
    let width = match s.as_bytes() {
        [] => return false,
        [0..=0x7f, ..] => 1,
        [0xc0..=0xdf, ..] => 2,
        [0xe0..=0xef, ..] => 3,
        _ => 4,
    };
    s.len() == width
}

// ponytail: 512 bytes holds any realistic message; a longer one (a huge prefix or key) is cut at a char boundary
// rather than failing for the wrong reason.
const CAPACITY: usize = 512;

/// A fixed buffer to build a panic message in, since const code cannot allocate or use `format!`.
struct Message {
    buf: [u8; CAPACITY],
    len: usize,
}

impl Message {
    const fn new() -> Self {
        Self { buf: [0; CAPACITY], len: 0 }
    }

    const fn push(&mut self, s: &str) {
        self.push_bytes(s.as_bytes());
    }

    /// Appends `bytes`, dropping whatever does not fit.
    const fn push_bytes(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while let [byte, tail @ ..] = rest {
            let Some((_, [slot, ..])) = self.buf.split_at_mut_checked(self.len) else { return };
            *slot = *byte;
            self.len = self.len.saturating_add(1);
            rest = tail;
        }
    }

    const fn push_int(&mut self, value: i128) {
        if value < 0 {
            self.push("-");
        }
        self.push_digits(value.unsigned_abs());
    }

    const fn push_usize(&mut self, value: usize) {
        // `u128::from` is not const, so widen through the little-endian bytes.
        let mut wide = [0; 16];
        let bytes = value.to_le_bytes();
        let mut rest: &[u8] = &bytes;
        let mut index = 0;
        while let [byte, tail @ ..] = rest {
            if let Some((_, [slot, ..])) = wide.split_at_mut_checked(index) {
                *slot = *byte;
            }
            index = index.saturating_add(1);
            rest = tail;
        }
        self.push_digits(u128::from_le_bytes(wide));
    }

    const fn push_digits(&mut self, value: u128) {
        // u128::MAX has 39 digits; fill from the end.
        let mut digits = [0; 39];
        let mut start = digits.len();
        let mut value = value;
        loop {
            let [digit, ..] = value.wrapping_rem(10).to_le_bytes();
            start = start.saturating_sub(1);
            if let Some((_, [slot, ..])) = digits.split_at_mut_checked(start) {
                *slot = b'0'.wrapping_add(digit);
            }
            value = value.wrapping_div(10);
            if value == 0 {
                break;
            }
        }
        if let Some((_, used)) = digits.split_at_checked(start) {
            self.push_bytes(used);
        }
    }

    /// The text so far, cut back to the last whole char if the buffer filled mid-char.
    const fn as_str(&self) -> &str {
        let bytes = match self.buf.split_at_checked(self.len) {
            Some((bytes, _)) => bytes,
            None => self.buf.as_slice(),
        };
        match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(error) => match bytes.split_at_checked(error.valid_up_to()) {
                Some((valid, _)) => match core::str::from_utf8(valid) {
                    Ok(s) => s,
                    Err(_) => "",
                },
                None => "",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, panic::catch_unwind, path::PathBuf};

    use super::*;
    use crate::{Datetime, FromValue};

    const U16: &Shape = u16::SHAPE;

    const SERVER: Shape = Shape::Table {
        name: "Server",
        fields: &[
            Field { key: "host", shape: String::SHAPE, has_default: false },
            Field { key: "port", shape: U16, has_default: false },
            Field { key: "tls", shape: <Option<bool>>::SHAPE, has_default: false },
            Field { key: "workers", shape: u8::SHAPE, has_default: true },
        ],
    };

    const fn variants(repr: Repr) -> Shape {
        Shape::Enum {
            name: "Mode",
            repr,
            variants: &[
                Variant { name: "Off", payload: None },
                Variant { name: "Port", payload: Some(U16) },
                Variant { name: "Serve", payload: Some(&SERVER) },
            ],
        }
    }

    const EXTERNAL: Shape = variants(Repr::External);
    const INTERNAL: Shape = Shape::Enum {
        name: "Mode",
        repr: Repr::Internal { tag: "type" },
        variants: &[Variant { name: "Off", payload: None }, Variant { name: "Serve", payload: Some(&SERVER) }],
    };
    const ADJACENT: Shape = variants(Repr::Adjacent { tag: "t", content: "c" });
    const UNTAGGED: Shape = variants(Repr::Untagged);

    const SERVER_NODE: Node = Node::Map(&[("host", Node::Str("a")), ("port", Node::Int(80)), ("extra", Node::Null)]);

    #[test]
    fn scalars() {
        assert!(conforms(&Shape::Any, &Node::Seq(&[Node::Null])));
        assert!(conforms(&Shape::Null, &Node::Null));
        assert!(!conforms(&Shape::Null, &Node::Bool(false)));
        assert!(conforms(bool::SHAPE, &Node::Bool(true)));
        assert!(!conforms(bool::SHAPE, &Node::Int(1)));
        assert!(conforms(String::SHAPE, &Node::Str("x")));
        assert!(conforms(String::SHAPE, &Node::Datetime("1979-05-27")));
        assert!(!conforms(String::SHAPE, &Node::Null));
        assert!(conforms(PathBuf::SHAPE, &Node::Str("a/b")));
        assert!(!conforms(PathBuf::SHAPE, &Node::Datetime("1979-05-27")));
        assert!(conforms(char::SHAPE, &Node::Str("é")));
        assert!(conforms(char::SHAPE, &Node::Str("🦀")));
        assert!(!conforms(char::SHAPE, &Node::Str("ab")));
        assert!(!conforms(char::SHAPE, &Node::Str("")));
        assert!(conforms(Datetime::SHAPE, &Node::Datetime("07:32:00")));
        assert!(conforms(Datetime::SHAPE, &Node::Str("1979-05-27T07:32:00Z")));
        assert!(!conforms(Datetime::SHAPE, &Node::Str("tomorrow")));
    }

    #[test]
    fn numbers() {
        assert!(conforms(U16, &Node::Int(65_535)));
        assert!(!conforms(U16, &Node::Int(65_536)));
        assert!(!conforms(U16, &Node::Int(-1)));
        assert!(!conforms(U16, &Node::Float(1.0)));
        assert!(conforms(i128::SHAPE, &Node::Int(i128::MIN)));
        assert!(conforms(f32::SHAPE, &Node::Int(i128::MAX)));
        assert!(conforms(f32::SHAPE, &Node::Float(f64::NAN)));
        assert!(conforms(f32::SHAPE, &Node::Float(f64::NEG_INFINITY)));
        assert!(!conforms(f32::SHAPE, &Node::Float(-1e300)));
        assert!(conforms(f64::SHAPE, &Node::Float(1e300)));
        assert!(!conforms(f64::SHAPE, &Node::Str("1.0")));
    }

    #[test]
    fn containers() {
        assert!(conforms(<Option<u16>>::SHAPE, &Node::Null));
        assert!(conforms(<Option<u16>>::SHAPE, &Node::Int(1)));
        assert!(!conforms(<Option<u16>>::SHAPE, &Node::Str("1")));
        assert!(conforms(<Vec<Option<u16>>>::SHAPE, &Node::Seq(&[Node::Int(1), Node::Null])));
        assert!(!conforms(<Vec<u16>>::SHAPE, &Node::Seq(&[Node::Int(1), Node::Null])));
        assert!(!conforms(<Vec<u16>>::SHAPE, &Node::Map(&[])));
        assert!(conforms(<(u8, String)>::SHAPE, &Node::Seq(&[Node::Int(1), Node::Str("a")])));
        assert!(!conforms(<(u8, String)>::SHAPE, &Node::Seq(&[Node::Int(1)])));
        assert!(!conforms(<(u8, String)>::SHAPE, &Node::Seq(&[Node::Str("a"), Node::Int(1)])));
        assert!(conforms(<HashMap<String, u16>>::SHAPE, &Node::Map(&[("a", Node::Int(1))])));
        assert!(!conforms(<HashMap<String, u16>>::SHAPE, &Node::Map(&[("a", Node::Int(-1))])));
    }

    #[test]
    fn tables() {
        assert!(conforms(&SERVER, &SERVER_NODE));
        assert!(!conforms(&SERVER, &Node::Map(&[("host", Node::Str("a"))])));
        assert!(!conforms(&SERVER, &Node::Map(&[("host", Node::Str("a")), ("port", Node::Int(-1))])));
        assert!(!conforms(
            &SERVER,
            &Node::Map(&[("host", Node::Str("a")), ("port", Node::Int(1)), ("tls", Node::Int(1))])
        ));
        assert!(!conforms(&SERVER, &Node::Seq(&[])));
    }

    #[test]
    fn external_enums() {
        assert!(conforms(&EXTERNAL, &Node::Str("Off")));
        assert!(!conforms(&EXTERNAL, &Node::Str("Port")));
        assert!(!conforms(&EXTERNAL, &Node::Str("Nope")));
        assert!(conforms(&EXTERNAL, &Node::Map(&[("Port", Node::Int(8))])));
        assert!(!conforms(&EXTERNAL, &Node::Map(&[("Port", Node::Str("8"))])));
        assert!(conforms(&EXTERNAL, &Node::Map(&[("Serve", SERVER_NODE)])));
        // A unit variant may be written as a table with a null payload, as YAML's `!Off` lowers to.
        assert!(conforms(&EXTERNAL, &Node::Map(&[("Off", Node::Null)])));
        assert!(!conforms(&EXTERNAL, &Node::Map(&[("Off", Node::Int(1))])));
        assert!(!conforms(&EXTERNAL, &Node::Map(&[("Port", Node::Int(8)), ("Off", Node::Null)])));
        assert!(!conforms(&EXTERNAL, &Node::Null));
    }

    #[test]
    fn internal_enums() {
        assert!(conforms(&INTERNAL, &Node::Map(&[("type", Node::Str("Off")), ("junk", Node::Null)])));
        assert!(conforms(
            &INTERNAL,
            &Node::Map(&[("type", Node::Str("Serve")), ("host", Node::Str("a")), ("port", Node::Int(1))])
        ));
        assert!(!conforms(&INTERNAL, &Node::Map(&[("type", Node::Str("Serve")), ("host", Node::Str("a"))])));
        assert!(!conforms(&INTERNAL, &Node::Map(&[("type", Node::Str("Nope"))])));
        assert!(!conforms(&INTERNAL, &Node::Map(&[("type", Node::Int(0))])));
        assert!(!conforms(&INTERNAL, &Node::Str("Off")));
        // A non-table payload cannot be internally tagged.
        assert!(!conforms(&variants(Repr::Internal { tag: "type" }), &Node::Map(&[("type", Node::Str("Port"))])));
    }

    #[test]
    fn adjacent_enums() {
        const MAYBE: Shape = Shape::Enum {
            name: "Maybe",
            repr: Repr::Adjacent { tag: "t", content: "c" },
            variants: &[Variant { name: "Level", payload: Some(<Option<u8>>::SHAPE) }],
        };
        assert!(conforms(&ADJACENT, &Node::Map(&[("t", Node::Str("Off"))])));
        assert!(conforms(&ADJACENT, &Node::Map(&[("t", Node::Str("Port")), ("c", Node::Int(1))])));
        assert!(!conforms(&ADJACENT, &Node::Map(&[("t", Node::Str("Port")), ("c", Node::Str("1"))])));
        assert!(!conforms(&ADJACENT, &Node::Map(&[("t", Node::Str("Port"))])));
        assert!(conforms(&ADJACENT, &Node::Map(&[("c", SERVER_NODE), ("t", Node::Str("Serve"))])));
        assert!(!conforms(&ADJACENT, &Node::Map(&[("c", Node::Int(1))])));
        assert!(conforms(&ADJACENT, &Node::Map(&[("t", Node::Str("Off")), ("c", Node::Null)])));
        assert!(!conforms(&ADJACENT, &Node::Map(&[("t", Node::Str("Off")), ("c", Node::Int(1))])));
        // An absent content is fine for a payload that may be missing.
        assert!(conforms(&MAYBE, &Node::Map(&[("t", Node::Str("Level"))])));
        check_node(&MAYBE, &Node::Map(&[("t", Node::Str("Level"))]), &[], "at");
    }

    #[test]
    fn untagged_enums() {
        assert!(conforms(&UNTAGGED, &Node::Null));
        assert!(conforms(&UNTAGGED, &Node::Int(80)));
        assert!(conforms(&UNTAGGED, &SERVER_NODE));
        assert!(!conforms(&UNTAGGED, &Node::Int(-1)));
        assert!(!conforms(&UNTAGGED, &Node::Str("Off")));
    }

    const DOC_SHAPE: Shape = Shape::Table {
        name: "Config",
        fields: &[
            Field { key: "server", shape: &SERVER, has_default: false },
            Field { key: "replicas", shape: <Vec<Option<u16>>>::SHAPE, has_default: false },
            Field { key: "pair", shape: <(u8, char)>::SHAPE, has_default: true },
            Field { key: "mode", shape: &EXTERNAL, has_default: true },
            Field { key: "inner", shape: &INTERNAL, has_default: true },
            Field { key: "adj", shape: &ADJACENT, has_default: true },
            Field { key: "any", shape: &UNTAGGED, has_default: true },
            Field { key: "custom", shape: &Shape::Any, has_default: false },
        ],
    };

    const DOC: Node = Node::Map(&[
        ("server", SERVER_NODE),
        ("replicas", Node::Seq(&[Node::Int(1), Node::Null])),
        ("pair", Node::Seq(&[Node::Int(1), Node::Str("x")])),
        ("mode", Node::Map(&[("Port", Node::Int(8))])),
        ("inner", Node::Map(&[("type", Node::Str("Serve")), ("host", Node::Str("h")), ("port", Node::Int(2))])),
        ("adj", Node::Map(&[("t", Node::Str("Port")), ("c", Node::Int(3))])),
        ("any", SERVER_NODE),
        ("unknown", Node::Seq(&[Node::Bool(true)])),
    ]);

    // Every value of a good document passes, in const evaluation.
    const _: () = {
        let paths: &[&[usize]] = &[
            &[],
            &[0],
            &[0, 0],
            &[0, 1],
            &[0, 2],
            &[1],
            &[1, 0],
            &[1, 1],
            &[2],
            &[2, 0],
            &[2, 1],
            &[3],
            &[3, 0],
            &[4],
            &[4, 0],
            &[4, 1],
            &[4, 2],
            &[5],
            &[5, 0],
            &[5, 1],
            &[6],
            &[6, 0],
            &[6, 1],
            &[6, 2],
            &[7],
            &[7, 0],
        ];
        let mut rest = paths;
        while let [path, tail @ ..] = rest {
            check_node(&DOC_SHAPE, &DOC, path, "config.toml");
            rest = tail;
        }
    };

    #[test]
    fn conforming_document() {
        assert!(conforms(&DOC_SHAPE, &DOC));
        check_node(&DOC_SHAPE, &DOC, &[6, 1], "config.toml");
        // Paths that do not exist are not this check's business.
        check_node(&DOC_SHAPE, &DOC, &[9], "config.toml");
        check_node(&DOC_SHAPE, &DOC, &[1, 5], "config.toml");
    }

    /// The panic message of `check_node(shape, doc, path, "at")`, or `None` if it passes.
    fn failure(shape: &'static Shape, doc: &'static Node, path: &'static [usize]) -> Option<String> {
        let result = catch_unwind(|| check_node(shape, doc, path, "at"));
        result.err().map(|payload| payload.downcast_ref::<String>().cloned().expect("a formatted message"))
    }

    fn fails(shape: &'static Shape, doc: &'static Node, path: &'static [usize], message: &str) {
        assert_eq!(failure(shape, doc, path).as_deref(), Some(message));
    }

    #[test]
    fn type_and_range_messages() {
        fails(U16, &Node::Str("x"), &[], "at: expected u16, found string");
        fails(U16, &Node::Int(70_000), &[], "at: 70000 is out of range for u16");
        fails(
            i8::SHAPE,
            &Node::Int(i128::MIN),
            &[],
            "at: -170141183460469231731687303715884105728 is out of range for i8",
        );
        fails(u8::SHAPE, &Node::Int(-1), &[], "at: -1 is out of range for u8");
        fails(<Option<u16>>::SHAPE, &Node::Bool(true), &[], "at: expected u16, found bool");
        fails(U16, &Node::Null, &[], "at: expected u16, found null (the key is present but has no value)");
        fails(f32::SHAPE, &Node::Float(1e300), &[], "at: expected f32, found float");
        fails(bool::SHAPE, &Node::Datetime("07:32:00"), &[], "at: expected bool, found datetime");
        fails(String::SHAPE, &Node::Seq(&[]), &[], "at: expected string, found sequence");
        fails(PathBuf::SHAPE, &Node::Datetime("07:32:00"), &[], "at: expected path, found datetime");
        fails(char::SHAPE, &Node::Str("ab"), &[], "at: expected a single character, found string");
        fails(Datetime::SHAPE, &Node::Str("soon"), &[], "at: expected datetime, found string");
        fails(<Vec<u8>>::SHAPE, &Node::Map(&[]), &[], "at: expected sequence, found table");
        fails(<HashMap<String, u8>>::SHAPE, &Node::Int(1), &[], "at: expected table, found integer");
        fails(&SERVER, &Node::Float(0.5), &[], "at: expected table, found float");
        fails(&Shape::Null, &Node::Int(0), &[], "at: expected null, found integer");
    }

    #[test]
    fn structure_messages() {
        fails(<(u8, u8, u8)>::SHAPE, &Node::Seq(&[Node::Int(1)]), &[], "at: expected a sequence of 3 items, found 1");
        fails(<(u8,)>::SHAPE, &Node::Str("x"), &[], "at: expected sequence, found string");
        fails(&SERVER, &Node::Map(&[("port", Node::Int(1))]), &[], "at: missing required key `host`");
        fails(
            &EXTERNAL,
            &Node::Str("Nope"),
            &[],
            "at: unknown variant `Nope` of Mode, expected one of `Off`, `Port`, `Serve`",
        );
        fails(
            &EXTERNAL,
            &Node::Map(&[("Nope", Node::Null)]),
            &[],
            "at: unknown variant `Nope` of Mode, expected one of `Off`, `Port`, `Serve`",
        );
        // A known name written in the wrong form, or no name at all.
        fails(&EXTERNAL, &Node::Str("Port"), &[], "at: string matches no variant of Mode");
        fails(&EXTERNAL, &Node::Map(&[]), &[], "at: table matches no variant of Mode");
        fails(&EXTERNAL, &Node::Int(1), &[], "at: integer matches no variant of Mode");
        fails(&INTERNAL, &Node::Map(&[("type", Node::Str("Serve"))]), &[], "at: missing required key `host`");
        fails(
            &INTERNAL,
            &Node::Map(&[("type", Node::Str("On"))]),
            &[],
            "at: unknown variant `On` of Mode, expected one of `Off`, `Serve`",
        );
        fails(&INTERNAL, &Node::Map(&[]), &[], "at: missing required key `type`");
        fails(&INTERNAL, &Node::Map(&[("type", Node::Int(1))]), &[], "at: table matches no variant of Mode");
        fails(&INTERNAL, &Node::Str("Off"), &[], "at: string matches no variant of Mode");
        fails(
            &ADJACENT,
            &Node::Map(&[("t", Node::Str("x")), ("c", Node::Null)]),
            &[],
            "at: unknown variant `x` of Mode, expected one of `Off`, `Port`, `Serve`",
        );
        fails(&ADJACENT, &Node::Map(&[("c", Node::Null)]), &[], "at: missing required key `t`");
        // A unit variant's payload is checked as null.
        fails(&EXTERNAL, &Node::Map(&[("Off", Node::Int(1))]), &[0], "at: expected null, found integer");
        fails(
            &ADJACENT,
            &Node::Map(&[("t", Node::Str("Off")), ("c", Node::Int(1))]),
            &[1],
            "at: expected null, found integer",
        );
        assert_eq!(failure(&EXTERNAL, &Node::Map(&[("Off", Node::Int(1))]), &[]), None);
        fails(&ADJACENT, &Node::Map(&[("t", Node::Str("Port"))]), &[], "at: missing required key `c`");
        fails(&UNTAGGED, &Node::Bool(true), &[], "at: bool matches no variant of Mode");
        fails(&UNTAGGED, &Node::Str("Nope"), &[], "at: string matches no variant of Mode");
    }

    #[test]
    fn only_the_target_is_reported() {
        const BAD: Node = Node::Map(&[
            ("server", Node::Map(&[("port", Node::Str("80"))])),
            ("replicas", Node::Seq(&[Node::Null, Node::Int(70_000)])),
            ("pair", Node::Seq(&[Node::Int(1), Node::Str("xy")])),
            ("mode", Node::Map(&[("Port", Node::Bool(false))])),
            ("inner", Node::Map(&[("type", Node::Str("Serve")), ("host", Node::Int(1)), ("port", Node::Int(2))])),
            ("adj", Node::Map(&[("t", Node::Str("Serve")), ("c", Node::Map(&[("host", Node::Str("h"))]))])),
            ("any", Node::Map(&[("wrong", Node::Null)])),
            ("custom", Node::Float(1.0)),
        ]);
        // The root and the containers are fine locally; each child reports its own problem.
        assert_eq!(failure(&DOC_SHAPE, &BAD, &[]), None);
        assert_eq!(failure(&DOC_SHAPE, &BAD, &[1]), None);
        assert_eq!(failure(&DOC_SHAPE, &BAD, &[1, 0]), None);
        fails(&DOC_SHAPE, &BAD, &[0], "at: missing required key `host`");
        fails(&DOC_SHAPE, &BAD, &[0, 0], "at: expected u16, found string");
        fails(&DOC_SHAPE, &BAD, &[1, 1], "at: 70000 is out of range for u16");
        fails(&DOC_SHAPE, &BAD, &[2, 1], "at: expected a single character, found string");
        fails(&DOC_SHAPE, &BAD, &[3, 0], "at: expected u16, found bool");
        fails(&DOC_SHAPE, &BAD, &[4, 1], "at: expected string, found integer");
        assert_eq!(failure(&DOC_SHAPE, &BAD, &[4, 0]), None);
        fails(&DOC_SHAPE, &BAD, &[5, 1], "at: missing required key `port`");
        assert_eq!(failure(&DOC_SHAPE, &BAD, &[5, 1, 0]), None);
        // An untagged enum that matches nothing is reported once, at the enum.
        fails(&DOC_SHAPE, &BAD, &[6], "at: table matches no variant of Mode");
        assert_eq!(failure(&DOC_SHAPE, &BAD, &[6, 0]), None);
        assert_eq!(failure(&DOC_SHAPE, &BAD, &[7]), None);
    }

    #[test]
    fn a_mismatched_ancestor_silences_its_children() {
        const BAD: Node = Node::Map(&[("server", Node::Seq(&[Node::Str("x")])), ("replicas", Node::Int(1))]);
        const TUPLE: Node = Node::Seq(&[Node::Str("x")]);
        const ENUM: Node = Node::Map(&[("Nope", Node::Str("x"))]);
        const ANY: Node = Node::Seq(&[Node::Seq(&[Node::Str("x")])]);
        fails(&DOC_SHAPE, &BAD, &[0], "at: expected table, found sequence");
        assert_eq!(failure(&DOC_SHAPE, &BAD, &[0, 0]), None);
        assert_eq!(failure(<(u8, u8)>::SHAPE, &TUPLE, &[0]), None);
        assert_eq!(failure(&EXTERNAL, &ENUM, &[0]), None);
        assert_eq!(failure(&Shape::Any, &ANY, &[0, 0]), None);
    }

    #[test]
    fn long_messages_are_cut_at_a_char_boundary() {
        // One ASCII byte, then two-byte chars: the 512th byte would split a char.
        let prefix = format!("a{}", "é".repeat(400));
        let message = catch_unwind(|| check_node(U16, &Node::Null, &[], &prefix)).unwrap_err();
        let message = message.downcast_ref::<String>().unwrap();
        assert_eq!(message.len(), 511);
        assert!(prefix.starts_with(message.as_str()));
    }
}
