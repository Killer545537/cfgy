use std::{borrow::Cow, ops::Range};

use cfgy_core::{Format, IndexMap, ParseError, Parsed, PathStack, Spans, Value};
use saphyr::{MarkedYaml, Marker, Scalar, ScalarStyle, Tag, YamlData, YamlLoader};
use saphyr_parser::{BufferedInput, Parser};

/// YAML 1.2, via `saphyr`.
///
/// Only the first document of a stream is read. A stream with no documents (an empty or comment-only file)
/// lowers to an empty map, matching an empty TOML file; an explicit `~` or bare `---` document lowers to `Null`.
///
/// A node with a custom tag lowers to a single-key map from the tag name to the node, the externally tagged
/// enum encoding `serde_yaml` uses: `!Variant 3` becomes `{Variant: 3}`. Core `!!` tags keep their meaning.
///
/// Spans: every value starts where saphyr's marker puts it: a block mapping or sequence at its first key or
/// `-`, a flow collection at its bracket, a scalar at its first byte after any tag. A custom tag's map shares
/// its node's span. A stream with no documents spans the whole source.
#[derive(Debug, Clone, Copy)]
pub struct Yaml;

impl Format for Yaml {
    fn name(&self) -> &'static str {
        "yaml"
    }

    fn parse(&self, source: &str) -> Result<Parsed, ParseError> {
        let offsets = Offsets::new(source);
        let mut loader = YamlLoader::<MarkedYaml<'_>>::default();
        // Resolving scalars while loading drops their custom tags, so keep the text and resolve in `Lower`.
        loader.early_parse(false);
        Parser::new(BufferedInput::new(source.chars()))
            .load(&mut loader, true)
            .map_err(|err| ParseError { message: err.info().to_owned(), offset: offsets.byte(err.marker()) })?;
        let docs = loader.into_documents();
        let mut lower = Lower { offsets: &offsets, spans: Spans::default() };
        let mut path = PathStack::new();
        let value = if let Some(doc) = docs.into_iter().next() {
            lower.node(doc, &mut path)?
        } else {
            lower.spans.insert(&path, 0..source.len());
            Value::Map(IndexMap::new())
        };
        Ok(Parsed { value, spans: lower.spans })
    }
}

/// Converts saphyr's markers, which count chars, into byte offsets.
enum Offsets {
    /// Chars and bytes coincide; holds the source length.
    Ascii(usize),
    /// The byte offset of every char, then the source length.
    Table(Vec<usize>),
}

impl Offsets {
    fn new(source: &str) -> Self {
        if source.is_ascii() {
            Self::Ascii(source.len())
        } else {
            Self::Table(source.char_indices().map(|(i, _)| i).chain(std::iter::once(source.len())).collect())
        }
    }

    fn byte(&self, marker: &Marker) -> Option<usize> {
        let chars = marker.index();
        match self {
            Self::Ascii(len) => (chars <= *len).then_some(chars),
            Self::Table(table) => table.get(chars).copied(),
        }
    }
}

/// Lowers a document into a [`Value`], recording each node's span as it goes.
struct Lower<'o> {
    offsets: &'o Offsets,
    spans: Spans,
}

impl Lower<'_> {
    fn range(&self, node: &MarkedYaml<'_>) -> Option<Range<usize>> {
        let start = self.offsets.byte(&node.span.start)?;
        let end = self.offsets.byte(&node.span.end).unwrap_or(start);
        Some(start..end.max(start))
    }

    fn node(&mut self, node: MarkedYaml<'_>, path: &mut PathStack) -> Result<Value, ParseError> {
        let range = self.range(&node);
        if let Some(range) = &range {
            self.spans.insert(path, range.clone());
        }
        let error = |message: &str| ParseError { message: message.to_owned(), offset: range.map(|r| r.start) };
        Ok(match node.data {
            YamlData::Representation(text, style, Some(tag)) if !tag.is_yaml_core_schema() => {
                let untagged = MarkedYaml { span: node.span, data: YamlData::Representation(text, style, None) };
                self.tagged(&tag, untagged, path)?
            }
            YamlData::Representation(text, style, tag) => {
                resolve(text, style, tag.as_ref()).ok_or_else(|| error("scalar does not match its tag"))?
            }
            YamlData::Value(value) => scalar(value),
            YamlData::Sequence(items) => Value::Seq(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(index, item)| path.with_index(index, |path| self.node(item, path)))
                    .collect::<Result<_, _>>()?,
            ),
            YamlData::Mapping(map) => Value::Map(
                map.into_iter()
                    .map(|(key, value)| {
                        let key = self.key(key)?;
                        let value = path.with_key(&key, |path| self.node(value, path))?;
                        Ok((key, value))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            // The loader only wraps collections, and only in custom tags; peel a core tag should one appear.
            YamlData::Tagged(tag, inner) if tag.is_yaml_core_schema() => self.node(*inner, path)?,
            YamlData::Tagged(tag, inner) => self.tagged(&tag, *inner, path)?,
            YamlData::BadValue => return Err(error("scalar does not match its tag")),
            YamlData::Alias(_) => return Err(error("unresolved YAML node")),
        })
    }

    /// Lowers `!name node` into `{name: node}`; the inner node gets the same span as the map.
    fn tagged(&mut self, tag: &Tag, inner: MarkedYaml<'_>, path: &mut PathStack) -> Result<Value, ParseError> {
        let name = if tag.handle == "!" { tag.suffix.clone() } else { format!("{}{}", tag.handle, tag.suffix) };
        let value = path.with_key(&name, |path| self.node(inner, path))?;
        Ok(Value::Map(IndexMap::from([(name, value)])))
    }

    /// Lowers a mapping key, which must come out as a string. Keys are not values, so they get no span.
    fn key(&self, key: MarkedYaml<'_>) -> Result<String, ParseError> {
        let offset = self.range(&key).map(|range| range.start);
        let value = match key.data {
            YamlData::Representation(text, style, tag) if tag.as_ref().is_none_or(|tag| tag.is_yaml_core_schema()) => {
                resolve(text, style, tag.as_ref())
                    .ok_or_else(|| ParseError { message: "scalar does not match its tag".to_owned(), offset })?
            }
            data => {
                let key = MarkedYaml { span: key.span, data };
                (Lower { offsets: self.offsets, spans: Spans::default() }).node(key, &mut PathStack::new())?
            }
        };
        match value {
            Value::Str(s) => Ok(s),
            other => {
                Err(ParseError { message: format!("mapping keys must be strings, found {}", other.kind()), offset })
            }
        }
    }
}

/// Resolves a scalar's text as the loader would have, honouring core tags; `None` when it contradicts its tag.
///
/// An empty plain scalar is null, as in the core schema, so a bare `!Unit` lowers to `{Unit: null}`.
fn resolve(text: Cow<'_, str>, style: ScalarStyle, tag: Option<&Cow<'_, Tag>>) -> Option<Value> {
    if text.is_empty() && style == ScalarStyle::Plain && tag.is_none() {
        return Some(Value::Null);
    }
    Scalar::parse_from_cow_and_metadata(text, style, tag).map(scalar)
}

fn scalar(value: Scalar<'_>) -> Value {
    match value {
        Scalar::Null => Value::Null,
        Scalar::Boolean(b) => Value::Bool(b),
        Scalar::Integer(i) => Value::Int(i128::from(i)),
        Scalar::FloatingPoint(f) => Value::Float(f.0),
        Scalar::String(s) => Value::Str(s.into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use cfgy_core::Segment;

    use super::*;

    fn parse(source: &str) -> Value {
        Yaml.parse(source).unwrap().value
    }

    #[test]
    fn scalars() {
        assert_eq!(parse("8080"), Value::Int(8080));
        assert_eq!(parse("1.0"), Value::Float(1.0));
        assert_eq!(parse("true"), Value::Bool(true));
        assert_eq!(parse("~"), Value::Null);
        assert_eq!(parse("'1'"), Value::Str("1".to_owned()));
        assert_eq!(parse("!custom 3"), single("custom", Value::Int(3)));
    }

    #[test]
    fn nested_and_ordered() {
        let Value::Map(map) = parse("z: [a, 2]\na:\n  y: 1\n  b: null\n") else { panic!("expected a map") };
        assert_eq!(map.keys().collect::<Vec<_>>(), ["z", "a"]);
        assert_eq!(map["z"], Value::Seq(vec![Value::Str("a".to_owned()), Value::Int(2)]));
        let Value::Map(inner) = &map["a"] else { panic!("expected a map") };
        assert_eq!(inner.keys().collect::<Vec<_>>(), ["y", "b"]);
        assert_eq!(inner["b"], Value::Null);
    }

    #[test]
    fn documents() {
        assert_eq!(parse(""), Value::Map(IndexMap::new()));
        assert_eq!(parse("# nothing\n"), Value::Map(IndexMap::new()));
        assert_eq!(parse("---\n"), Value::Null);
        assert_eq!(parse("--- 1\n--- 2\n"), Value::Int(1));
    }

    #[test]
    fn rejects_non_string_keys_and_bad_values() {
        assert_eq!(Yaml.parse("1: a").unwrap_err().message, "mapping keys must be strings, found integer");
        assert_eq!(Yaml.parse("!!int foo").unwrap_err().message, "scalar does not match its tag");
    }

    #[test]
    fn malformed_offset() {
        // The marker counts chars, so the multibyte `é` checks the byte conversion.
        let source = "é: \"x\ny: '";
        let err = Yaml.parse(source).unwrap_err();
        assert_eq!(err.line_col(source), Some((1, 4)), "{err:?}");
    }

    #[test]
    fn char_index_to_byte_offset() {
        let at = |source: &str, chars: usize| Offsets::new(source).byte(&Marker::new(chars, 1, 1));
        assert_eq!(at("éa", 1), Some(2));
        assert_eq!(at("éa", 2), Some(3));
        assert_eq!(at("éa", 3), None);
        assert_eq!(at("ab", 2), Some(2));
        assert_eq!(at("ab", 3), None);
    }

    #[test]
    fn lowering_errors_carry_offsets() {
        let source = "é: 1\nlist:\n  - [1, 2]: x\n";
        let err = Yaml.parse(source).unwrap_err();
        assert_eq!(err.message, "mapping keys must be strings, found sequence");
        assert_eq!(err.line_col(source), Some((3, 5)), "{err:?}");
        let source = "a:\n  b: !!int foo\n";
        let err = Yaml.parse(source).unwrap_err();
        assert_eq!(err.message, "scalar does not match its tag");
        // saphyr marks a tagged scalar at its text, after the tag.
        assert_eq!(err.line_col(source), Some((2, 12)), "{err:?}");
    }

    fn key(k: &str) -> Segment {
        Segment::Key(k.to_owned())
    }

    fn start(source: &str, spans: &Spans, path: &[Segment]) -> Option<(usize, usize)> {
        cfgy_core::line_col(source, spans.get(path)?.start)
    }

    #[test]
    fn spans() {
        let source = "\
# comment
café: ünï
server:
  port: 8080
  hosts:
    - a
    - {name: b, tags: [x, y]}
";
        let Parsed { spans, .. } = Yaml.parse(source).unwrap();
        assert_eq!(start(source, &spans, &[]), Some((2, 1)));
        assert_eq!(start(source, &spans, &[key("café")]), Some((2, 7)));
        assert_eq!(start(source, &spans, &[key("server")]), Some((4, 3)));
        assert_eq!(start(source, &spans, &[key("server"), key("port")]), Some((4, 9)));
        let hosts = [key("server"), key("hosts")];
        assert_eq!(start(source, &spans, &hosts), Some((6, 5)));
        let at = |i: usize, rest: &[Segment]| [&hosts[..], &[Segment::Index(i)], rest].concat();
        assert_eq!(start(source, &spans, &at(0, &[])), Some((6, 7)));
        assert_eq!(start(source, &spans, &at(1, &[])), Some((7, 7)));
        assert_eq!(start(source, &spans, &at(1, &[key("tags"), Segment::Index(1)])), Some((7, 27)));
        let port = spans.get(&[key("server"), key("port")]).unwrap().clone();
        assert_eq!(source.get(port), Some("8080"));
        assert_eq!(spans.get(&[key("missing")]), None);
    }

    #[test]
    fn empty_stream_spans_the_source() {
        let Parsed { spans, .. } = Yaml.parse("# nothing\n").unwrap();
        assert_eq!(spans.get(&[]), Some(&(0..10)));
    }

    fn single(name: &str, value: Value) -> Value {
        Value::Map(IndexMap::from([(name.to_owned(), value)]))
    }

    #[test]
    fn custom_tags_become_single_key_maps() {
        assert_eq!(parse("!Port 3"), single("Port", Value::Int(3)));
        assert_eq!(parse("!Name \"3\""), single("Name", Value::Str("3".to_owned())));
        assert_eq!(parse("!Unit"), single("Unit", Value::Null));
        assert_eq!(parse("!List [1, a]"), single("List", Value::Seq(vec![Value::Int(1), Value::Str("a".to_owned())])));
        assert_eq!(parse("!Tcp {port: 80}"), single("Tcp", single("port", Value::Int(80))));
        assert_eq!(
            parse("- !A\n  - !B x\n"),
            Value::Seq(vec![single("A", Value::Seq(vec![single("B", Value::Str("x".to_owned()))]))])
        );
        assert_eq!(parse("!<tag:example.com,2000:app/x> 1"), single("tag:example.com,2000:app/x", Value::Int(1)));
    }

    #[test]
    fn core_tags_keep_their_meaning() {
        assert_eq!(parse("!!str 1"), Value::Str("1".to_owned()));
        assert_eq!(parse("!!int 1"), Value::Int(1));
        assert_eq!(parse("!!float 1"), Value::Float(1.0));
        assert_eq!(parse("!!map {a: 1}"), single("a", Value::Int(1)));
        assert_eq!(parse("!!seq [1]"), Value::Seq(vec![Value::Int(1)]));
        assert_eq!(parse("!!str 1: a"), single("1", Value::Str("a".to_owned())));
        assert_eq!(
            parse("a: &x 1\nb: *x\n"),
            Value::Map(IndexMap::from([("a".to_owned(), Value::Int(1)), ("b".to_owned(), Value::Int(1))]))
        );
        assert_eq!(Yaml.parse("!K a: 1").unwrap_err().message, "mapping keys must be strings, found table");
    }

    /// Without custom tags, resolving scalars in `Lower` must agree with saphyr resolving them while loading.
    #[test]
    fn matches_early_parse_without_custom_tags() {
        use saphyr::{LoadableYamlNode, Yaml as Node};
        fn reference(node: Node<'_>) -> Result<Value, String> {
            Ok(match node {
                Node::Value(value) => scalar(value),
                Node::Sequence(items) => Value::Seq(items.into_iter().map(reference).collect::<Result<_, _>>()?),
                Node::Mapping(map) => Value::Map(
                    map.into_iter()
                        .map(|(k, v)| match reference(k)? {
                            Value::Str(k) => Ok((k, reference(v)?)),
                            other => Err(format!("mapping keys must be strings, found {}", other.kind())),
                        })
                        .collect::<Result<_, _>>()?,
                ),
                Node::BadValue => return Err("scalar does not match its tag".to_owned()),
                other => return Err(format!("unexpected {other:?}")),
            })
        }
        let cases = [
            "a:",
            "a: ''",
            "a: ~",
            "- \n- x",
            "0x1f",
            "0o17",
            ".inf",
            "-.inf",
            "1e3",
            "'1'",
            "\"true\"",
            "True",
            "yes",
            "a: 1\na: 2",
            "? [1]\n: x",
            "a: &x {b: 1}\nc: *x",
            "!!int 3",
            "!!bool true",
            "!!null ~",
            "!!str",
            "!!int",
            "1: a",
            "null: a",
            "---\n",
            "- !!str\n- x",
        ];
        for source in cases {
            let ours = Yaml.parse(source).map(|parsed| parsed.value).map_err(|err| err.message);
            let doc = Node::load_from_str(source).unwrap().into_iter().next().unwrap();
            assert_eq!(ours, reference(doc), "{source}");
        }
    }

    #[test]
    fn custom_tag_spans() {
        let source = "\
listen: !Tcp
  port: 80
mode: !Fast 3
";
        let Parsed { spans, .. } = Yaml.parse(source).unwrap();
        let listen = [key("listen")];
        assert_eq!(start(source, &spans, &listen), Some((2, 3)));
        assert_eq!(start(source, &spans, &[key("listen"), key("Tcp")]), Some((2, 3)));
        assert_eq!(start(source, &spans, &[key("listen"), key("Tcp"), key("port")]), Some((2, 9)));
        assert_eq!(start(source, &spans, &[key("mode")]), Some((3, 13)));
        assert_eq!(start(source, &spans, &[key("mode"), key("Fast")]), Some((3, 13)));
    }
}
