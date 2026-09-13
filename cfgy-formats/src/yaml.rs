use std::ops::Range;

use cfgy_core::{Format, IndexMap, ParseError, Parsed, PathStack, Spans, Value};
use saphyr::{LoadableYamlNode, MarkedYaml, Marker, Scalar, YamlData};

/// YAML 1.2, via `saphyr`.
///
/// Only the first document of a stream is read. A stream with no documents (an empty or comment-only file)
/// lowers to an empty map, matching an empty TOML file; an explicit `~` or bare `---` document lowers to `Null`.
///
/// Spans: every value starts where saphyr's marker puts it: a block mapping or sequence at its first key or
/// `-`, a flow collection at its bracket, a scalar at its first byte. A stream with no documents spans the
/// whole source.
#[derive(Debug, Clone, Copy)]
pub struct Yaml;

impl Format for Yaml {
    fn name(&self) -> &'static str {
        "yaml"
    }

    fn parse(&self, source: &str) -> Result<Parsed, ParseError> {
        let offsets = Offsets::new(source);
        let docs = MarkedYaml::load_from_str(source)
            .map_err(|err| ParseError { message: err.info().to_owned(), offset: offsets.byte(err.marker()) })?;
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
            YamlData::Value(Scalar::Null) => Value::Null,
            YamlData::Value(Scalar::Boolean(b)) => Value::Bool(b),
            YamlData::Value(Scalar::Integer(i)) => Value::Int(i128::from(i)),
            YamlData::Value(Scalar::FloatingPoint(f)) => Value::Float(f.0),
            YamlData::Value(Scalar::String(s)) => Value::Str(s.into_owned()),
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
            // ponytail: user-defined tags are dropped and the node lowered as if untagged.
            YamlData::Tagged(_, inner) => self.node(*inner, path)?,
            YamlData::BadValue => return Err(error("scalar does not match its tag")),
            YamlData::Representation(..) | YamlData::Alias(_) => return Err(error("unresolved YAML node")),
        })
    }

    /// Lowers a mapping key, which must come out as a string. Keys are not values, so they get no span.
    fn key(&self, key: MarkedYaml<'_>) -> Result<String, ParseError> {
        let offset = self.range(&key).map(|range| range.start);
        match key.data {
            YamlData::Value(Scalar::String(s)) => Ok(s.into_owned()),
            data => {
                let key = MarkedYaml { span: key.span, data };
                match (Lower { offsets: self.offsets, spans: Spans::default() }).node(key, &mut PathStack::new())? {
                    Value::Str(s) => Ok(s),
                    other => Err(ParseError {
                        message: format!("mapping keys must be strings, found {}", other.kind()),
                        offset,
                    }),
                }
            }
        }
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
        assert_eq!(parse("!custom 3"), Value::Int(3));
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
}
