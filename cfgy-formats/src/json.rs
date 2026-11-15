use std::{collections::HashMap, ops::Range};

use cfgy_core::{Format, ParseError, Parsed, PathStack, Spans, Value};

/// JSON, via `serde_json`.
///
/// Spans: every value, including objects, arrays, and the root, starts at its first byte (`{`, `[`, `"`, or the
/// literal). For a duplicate key the last occurrence wins, as it does for the value.
#[derive(Debug, Clone, Copy)]
pub struct Json;

impl Format for Json {
    fn name(&self) -> &'static str {
        "json"
    }

    fn parse(&self, source: &str) -> Result<Parsed, ParseError> {
        let value = serde_json::from_str(source).map_err(|err| {
            let message = err.to_string();
            let location = format!(" at line {} column {}", err.line(), err.column());
            ParseError {
                message: message.strip_suffix(&location).unwrap_or(&message).to_owned(),
                offset: offset(source, err.line(), err.column()),
            }
        })?;
        let value = lower(value)?;
        let mut spans = Spans::default();
        Tracker { source, pos: 0 }.node().record(&mut PathStack::new(), &mut spans);
        Ok(Parsed { value, spans })
    }
}

/// Where one value sits in the source, with the positions of its children.
struct Node {
    range: Range<usize>,
    children: Children,
}

enum Children {
    None,
    Seq(Vec<Node>),
    /// Keyed by the decoded key; a later duplicate replaces the earlier subtree, matching `serde_json`.
    Map(HashMap<String, Node>),
}

impl Node {
    fn record(self, path: &mut PathStack, spans: &mut Spans) {
        spans.insert(path, self.range);
        match self.children {
            Children::None => {}
            Children::Seq(items) => {
                for (index, item) in items.into_iter().enumerate() {
                    path.with_index(index, |path| item.record(path, spans));
                }
            }
            Children::Map(members) => {
                for (key, member) in members {
                    path.with_key(&key, |path| member.record(path, spans));
                }
            }
        }
    }
}

/// Scans text `serde_json` has already accepted, so it only needs to find value boundaries, not validate.
///
/// Walks bytes: UTF-8 continuation bytes never equal the ASCII delimiters it looks for. Every step is bounds
/// checked and every value consumes at least one byte, so unexpected input ends the scan rather than panicking
/// or looping.
struct Tracker<'s> {
    source: &'s str,
    pos: usize,
}

impl Tracker<'_> {
    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.pos).copied()
    }

    const fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    fn node(&mut self) -> Node {
        self.skip_whitespace();
        let start = self.pos;
        let children = match self.peek() {
            None => Children::None,
            Some(b'{') => {
                self.bump();
                let mut members = HashMap::new();
                loop {
                    self.skip_whitespace();
                    match self.peek() {
                        None => break,
                        Some(b'}') => {
                            self.bump();
                            break;
                        }
                        Some(b'"') => {
                            let key = self.string();
                            // Reuse serde_json to decode escapes exactly as it did for the value.
                            let key = self.source.get(key).and_then(|raw| serde_json::from_str(raw).ok());
                            self.skip_whitespace();
                            if self.peek() == Some(b':') {
                                self.bump();
                            }
                            members.insert(key.unwrap_or_default(), self.node());
                        }
                        Some(_) => self.bump(), // `,`
                    }
                }
                Children::Map(members)
            }
            Some(b'[') => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_whitespace();
                    match self.peek() {
                        None => break,
                        Some(b']') => {
                            self.bump();
                            break;
                        }
                        Some(b',') => self.bump(),
                        Some(_) => items.push(self.node()),
                    }
                }
                Children::Seq(items)
            }
            Some(b'"') => {
                self.string();
                Children::None
            }
            Some(_) => {
                // A number or `true`/`false`/`null`.
                self.bump();
                while matches!(self.peek(), Some(b) if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'.')) {
                    self.bump();
                }
                Children::None
            }
        };
        Node { range: start..self.pos, children }
    }

    /// Consumes a string starting at its opening quote and returns its range, quotes included.
    fn string(&mut self) -> Range<usize> {
        let start = self.pos;
        self.bump();
        loop {
            match self.peek() {
                None => break,
                Some(b'"') => {
                    self.bump();
                    break;
                }
                Some(b'\\') => {
                    self.bump();
                    self.bump();
                }
                Some(_) => self.bump(),
            }
        }
        start..self.pos
    }
}

/// Byte offset of `serde_json`'s 1-based line and 1-based byte column.
fn offset(source: &str, line: usize, column: usize) -> Option<usize> {
    let line_start: usize = source.split_inclusive('\n').take(line.checked_sub(1)?).map(str::len).sum();
    line_start.checked_add(column.saturating_sub(1))
}

fn lower(value: serde_json::Value) -> Result<Value, ParseError> {
    Ok(match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        // ponytail: serde_json reads integers above u64::MAX as f64, so those arrive as Float, not an error.
        serde_json::Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => Value::Int(i.into()),
            (None, Some(u), _) => Value::Int(u.into()),
            (None, None, Some(f)) => Value::Float(f),
            (None, None, None) => {
                return Err(ParseError { message: format!("number {n} is not representable"), offset: None });
            }
        },
        serde_json::Value::String(s) => Value::Str(s),
        serde_json::Value::Array(items) => Value::Seq(items.into_iter().map(lower).collect::<Result<_, _>>()?),
        serde_json::Value::Object(map) => {
            Value::Map(map.into_iter().map(|(k, v)| Ok((k, lower(v)?))).collect::<Result<_, _>>()?)
        }
    })
}

#[cfg(test)]
mod tests {
    use cfgy_core::Segment;

    use super::*;

    fn parse(source: &str) -> Value {
        Json.parse(source).unwrap().value
    }

    #[test]
    fn numbers() {
        assert_eq!(parse("8080"), Value::Int(8080));
        assert_eq!(parse("-1"), Value::Int(-1));
        assert_eq!(parse("8080.0"), Value::Float(8080.0));
        assert_eq!(parse("1e3"), Value::Float(1000.0));
        assert_eq!(parse("18446744073709551615"), Value::Int(i128::from(u64::MAX)));
        assert_eq!(parse("-9223372036854775808"), Value::Int(i128::from(i64::MIN)));
        // Above u64::MAX serde_json yields a float, as serde does.
        assert_eq!(parse("18446744073709551616"), Value::Float(18_446_744_073_709_551_616.0));
    }

    #[test]
    fn scalars_and_order() {
        assert_eq!(parse("null"), Value::Null);
        let Value::Map(map) = parse(r#"{"z": [true, "s"], "a": {}}"#) else { panic!("expected a map") };
        assert_eq!(map.keys().collect::<Vec<_>>(), ["z", "a"]);
        assert_eq!(map["z"], Value::Seq(vec![Value::Bool(true), Value::Str("s".to_owned())]));
        assert_eq!(map["a"], Value::Map(cfgy_core::IndexMap::new()));
    }

    #[test]
    fn malformed_offset() {
        let source = "{\n  \"a\": 1,\n  \"b\": x\n}";
        let err = Json.parse(source).unwrap_err();
        assert_eq!(err.offset, source.find('x'));
        assert_eq!(err.line_col(source), Some((3, 8)));
        assert_eq!(err.message, "expected value");
    }

    fn key(k: &str) -> Segment {
        Segment::Key(k.to_owned())
    }

    /// The source text each path's span covers.
    fn text<'s>(source: &'s str, spans: &Spans, path: &[Segment]) -> Option<&'s str> {
        source.get(spans.get(path)?.clone())
    }

    #[test]
    fn spans() {
        let source =
            "  {\n  \"server\": {\"port\": 8080, \"hosts\": [\"a\", [true, null], {}]},\n  \"ratio\": -1.5e3 }\n";
        let Parsed { spans, .. } = Json.parse(source).unwrap();
        let root = spans.get(&[]).unwrap();
        assert_eq!(root.start, 2);
        assert!(text(source, &spans, &[]).unwrap().ends_with("-1.5e3 }"));
        assert_eq!(cfgy_core::line_col(source, spans.get(&[key("server")]).unwrap().start), Some((2, 13)));
        assert_eq!(text(source, &spans, &[key("server"), key("port")]), Some("8080"));
        let hosts = [key("server"), key("hosts")];
        assert_eq!(text(source, &spans, &hosts), Some("[\"a\", [true, null], {}]"));
        let at = |i: usize, rest: &[Segment]| [&hosts[..], &[Segment::Index(i)], rest].concat();
        assert_eq!(text(source, &spans, &at(0, &[])), Some("\"a\""));
        assert_eq!(text(source, &spans, &at(1, &[Segment::Index(1)])), Some("null"));
        assert_eq!(text(source, &spans, &at(2, &[])), Some("{}"));
        assert_eq!(text(source, &spans, &[key("ratio")]), Some("-1.5e3"));
        assert_eq!(spans.get(&[key("missing")]), None);
    }

    #[test]
    fn spans_after_escapes_and_multibyte() {
        let source = r#"{"café": "a \"quoted\" \\ é😀", "😀": {"😀\n": [1]}, "\u00e9\ud83d\ude00": 5, "b": "ü"}"#;
        let Parsed { spans, .. } = Json.parse(source).unwrap();
        assert_eq!(text(source, &spans, &[key("é😀")]), Some("5"));
        assert_eq!(text(source, &spans, &[key("café")]), Some(r#""a \"quoted\" \\ é😀""#));
        assert_eq!(text(source, &spans, &[key("😀"), key("😀\n"), Segment::Index(0)]), Some("1"));
        assert_eq!(text(source, &spans, &[key("b")]), Some("\"ü\""));
        let b = spans.get(&[key("b")]).unwrap().start;
        assert_eq!(cfgy_core::line_col(source, b), Some((1, source.get(..b).unwrap().chars().count() + 1)));
    }

    #[test]
    fn duplicate_keys_use_the_last_occurrence() {
        let source = r#"{"a": {"x": 1}, "b": 2, "a": {"y": 3}}"#;
        let Parsed { value, spans } = Json.parse(source).unwrap();
        let Value::Map(map) = value else { panic!("expected a map") };
        assert_eq!(map["a"], Value::Map(std::iter::once(("y".to_owned(), Value::Int(3))).collect()));
        assert_eq!(text(source, &spans, &[key("a")]), Some(r#"{"y": 3}"#));
        assert_eq!(text(source, &spans, &[key("a"), key("y")]), Some("3"));
        assert_eq!(spans.get(&[key("a"), key("x")]), None);
    }

    #[test]
    fn tracker_never_panics_on_unexpected_text() {
        for source in ["", "{", "[", "\"", "\"\\", "{\"a\"", "{\"a\":", "[1,", "}", "]", ":", "é", "{é}", "[é]"] {
            let mut spans = Spans::default();
            Tracker { source, pos: 0 }.node().record(&mut PathStack::new(), &mut spans);
        }
    }
}
