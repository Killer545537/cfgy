use cfgy_core::{Format, IndexMap, ParseError, Value};
use saphyr::{LoadableYamlNode, Scalar, Yaml as Node};

/// YAML 1.2, via `saphyr`.
///
/// Only the first document of a stream is read. A stream with no documents (an empty or comment-only file)
/// lowers to an empty map, matching an empty TOML file; an explicit `~` or bare `---` document lowers to `Null`.
#[derive(Debug, Clone, Copy)]
pub struct Yaml;

impl Format for Yaml {
    fn name(&self) -> &'static str {
        "yaml"
    }

    fn parse(&self, source: &str) -> Result<Value, ParseError> {
        let docs = Node::load_from_str(source).map_err(|err| ParseError {
            message: err.info().to_owned(),
            offset: byte_offset(source, err.marker().index()),
        })?;
        docs.into_iter().next().map_or_else(|| Ok(Value::Map(IndexMap::new())), lower)
    }
}

/// saphyr markers count chars; convert to a byte offset.
fn byte_offset(source: &str, chars: usize) -> Option<usize> {
    source.char_indices().map(|(i, _)| i).chain(std::iter::once(source.len())).nth(chars)
}

// ponytail: nodes carry no marker once loaded, so errors found while lowering have no offset; switch to
// `MarkedYaml` if those need a position.
fn lower(node: Node<'_>) -> Result<Value, ParseError> {
    Ok(match node {
        Node::Value(Scalar::Null) => Value::Null,
        Node::Value(Scalar::Boolean(b)) => Value::Bool(b),
        Node::Value(Scalar::Integer(i)) => Value::Int(i),
        Node::Value(Scalar::FloatingPoint(f)) => Value::Float(f.0),
        Node::Value(Scalar::String(s)) => Value::Str(s.into_owned()),
        Node::Sequence(items) => Value::Seq(items.into_iter().map(lower).collect::<Result<_, _>>()?),
        Node::Mapping(map) => Value::Map(
            map.into_iter()
                .map(|(key, value)| match lower(key)? {
                    Value::Str(key) => Ok((key, lower(value)?)),
                    other => Err(error(format!("mapping keys must be strings, found {}", other.kind()))),
                })
                .collect::<Result<_, _>>()?,
        ),
        // ponytail: user-defined tags are dropped and the node lowered as if untagged.
        Node::Tagged(_, node) => lower(*node)?,
        Node::BadValue => return Err(error("scalar does not match its tag".to_owned())),
        Node::Representation(..) | Node::Alias(_) => return Err(error("unresolved YAML node".to_owned())),
    })
}

const fn error(message: String) -> ParseError {
    ParseError { message, offset: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Value {
        Yaml.parse(source).unwrap()
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
        assert_eq!(byte_offset("éa", 1), Some(2));
        assert_eq!(byte_offset("éa", 2), Some(3));
        assert_eq!(byte_offset("éa", 3), None);
    }
}
