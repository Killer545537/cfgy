//! Compile-time check: the config file is embedded as a const [`Node`](cfgy_core::check::Node) document, and every
//! value in it is asserted against `<Root as FromValue>::SHAPE` in const evaluation.
//!
//! The type system resolves the shape, so aliases, nested structs, custom `FromValue` impls and recursive types are
//! checked by what they are rather than by how their names are spelled.

use std::path::Path;
use std::{env, fs, io};

use cfgy_core::{Datetime, PathStack, Spans, Value, line_col};
use proc_macro2::{Ident, Literal, Span, TokenStream as TokenStream2};
use quote::{ToTokens, quote, quote_spanned};
use syn::LitStr;

use crate::plan::{SourcePlan, StructPlan};

/// Reads and parses the config file of `plan` and emits its compile-time check, unless it has no `path` or sets
/// `check = false`.
pub fn check(plan: &StructPlan) -> syn::Result<TokenStream2> {
    let Some(source) = plan.source.as_ref().filter(|source| source.check) else {
        return Ok(TokenStream2::new());
    };
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| syn::Error::new(source.path.span(), "`CARGO_MANIFEST_DIR` is not set"))?;
    check_source(Path::new(&manifest_dir), source, &plan.ident)
}

/// Checks the file `source` points at, relative to `manifest_dir`, against the `FromValue::SHAPE` of `ident`.
///
/// A file that cannot be read or parsed is a `syn` error on the `path` literal. A parsed file becomes one block:
///
/// ```ignore
/// const _: () = {
///     const _: &str = ::core::include_str!("/abs/path/config.toml");
///     const __CFGY_DOC: ::cfgy::__private::Node = ::cfgy::__private::Node::Map(&[("port", ..)]);
///     const _: () = ::cfgy::__private::check_node(
///         <Settings as ::cfgy::FromValue>::SHAPE, &__CFGY_DOC, &[], "config.toml:1:1: `<root>`",
///     );
///     const _: () = ::cfgy::__private::check_node(
///         <Settings as ::cfgy::FromValue>::SHAPE, &__CFGY_DOC, &[0], "config.toml:1:8: `port`",
///     );
/// };
/// ```
///
/// - The `include_str!` is unused, but it makes Cargo rebuild when the file changes; without it the check would
///   validate a stale file.
/// - The outer `const _` scopes `__CFGY_DOC`, so several checked structs can share a module.
/// - rustc evaluates each inner `const _` separately, so every mismatch in the file is reported, not just the first.
///   `check_node` stays quiet below a mismatched ancestor, so one mistake is one error.
/// - A message starts with the value's `file:line:col` and its path, rendered as runtime errors render it:
///   `database.replicas[1].port`, or `<root>` for the whole document. A missing key is reported at its table.
/// - The consts are located at the `path` literal, which is where rustc points their errors.
/// - `__CFGY_DOC` allows `clippy::approx_constant`, the one lint a copied value (`pi = 3.14159`) trips.
///
/// Messages name the file as written in the attribute, not the resolved absolute path, so they read the same on
/// every machine.
///
/// ponytail: one const per value, so compile time grows with the size of the config file; fine for hand-written
/// configs. Upgrade path: one const per table, or a single const returning the first failing index.
pub fn check_source(manifest_dir: &Path, source: &SourcePlan, ident: &Ident) -> syn::Result<TokenStream2> {
    let file = source.path.value();
    let resolved = manifest_dir.join(&file);
    let error = |message: String| syn::Error::new(source.path.span(), message);

    let text = fs::read_to_string(&resolved).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => error(format!("config file `{file}` does not exist")),
        _ => error(format!("cannot read config file `{file}`: {err}")),
    })?;
    let format = cfgy_formats::select(source.format.as_ref().map(LitStr::value).as_deref(), &resolved, &text)
        .map_err(|err| error(format!("config file `{file}`: {err}")))?;
    let parsed = format.parse(&text).map_err(|err| match err.line_col(&text) {
        Some((line, col)) => error(format!("{file}:{line}:{col}: {}", err.message)),
        None => error(format!("{file}: {}", err.message)),
    })?;

    // Located at the literal but resolved at the call site, so lints keep treating the consts as macro output.
    let span = Span::call_site().located_at(source.path.span());
    let resolved = LitStr::new(&resolved.to_string_lossy(), span);
    let document = document(&parsed.value);
    let mut collector =
        Collector { file: &file, text: &text, spans: &parsed.spans, indices: Vec::new(), checks: Vec::new() };
    collector.visit(&parsed.value, &mut PathStack::new());
    let checks = collector.checks.into_iter().map(|Check { indices, prefix }| {
        let indices = indices.into_iter().map(Literal::usize_unsuffixed);
        quote_spanned! {span=>
            const _: () = ::cfgy::__private::check_node(
                <#ident as ::cfgy::FromValue>::SHAPE,
                &__CFGY_DOC,
                &[#(#indices),*],
                #prefix,
            );
        }
    });
    Ok(quote_spanned! {span=>
        const _: () = {
            const _: &str = ::core::include_str!(#resolved);
            #[allow(clippy::approx_constant, reason = "values copied from the config file, such as `pi = 3.14159`")]
            const __CFGY_DOC: ::cfgy::__private::Node = #document;
            #(#checks)*
        };
    })
}

/// One `check_node` assertion: the value's position below the root, and the start of its failure message.
#[derive(Debug, PartialEq, Eq)]
struct Check {
    indices: Vec<usize>,
    prefix: String,
}

/// Walks a parsed file, producing one [`Check`] per value.
struct Collector<'a> {
    file: &'a str,
    text: &'a str,
    spans: &'a Spans,
    /// Positions from the root to the value being visited.
    indices: Vec<usize>,
    checks: Vec<Check>,
}

impl Collector<'_> {
    /// Pushes a check for `value`, at `path`, then for everything below it; parents come before their children.
    fn visit(&mut self, value: &Value, path: &mut PathStack) {
        self.checks.push(Check { indices: self.indices.clone(), prefix: self.prefix(path) });
        match value {
            Value::Seq(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.indices.push(index);
                    path.with_index(index, |path| self.visit(item, path));
                    self.indices.pop();
                }
            }
            Value::Map(entries) => {
                for (index, (key, item)) in entries.iter().enumerate() {
                    self.indices.push(index);
                    path.with_key(key, |path| self.visit(item, path));
                    self.indices.pop();
                }
            }
            Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_) | Value::Datetime(_) => {}
        }
    }

    /// `` file:line:col: `path` ``, or `` file: `path` `` for a value without a span.
    fn prefix(&self, path: &PathStack) -> String {
        let file = self.file;
        let rendered = if path.segments().is_empty() { "<root>".to_owned() } else { path.to_string() };
        match self.spans.get(path.segments()).and_then(|range| line_col(self.text, range.start)) {
            Some((line, col)) => format!("{file}:{line}:{col}: `{rendered}`"),
            None => format!("{file}: `{rendered}`"),
        }
    }
}

/// `value` as an expression building a `::cfgy::__private::Node`, for a const initializer.
///
/// Nested slices are written as `&[..]` and promoted to `'static` by the const they initialize.
pub fn document(value: &Value) -> TokenStream2 {
    match value {
        Value::Null => quote! { ::cfgy::__private::Node::Null },
        Value::Bool(b) => quote! { ::cfgy::__private::Node::Bool(#b) },
        Value::Int(i) => {
            let i = Literal::i128_suffixed(*i);
            quote! { ::cfgy::__private::Node::Int(#i) }
        }
        Value::Float(f) => {
            let f = float(*f);
            quote! { ::cfgy::__private::Node::Float(#f) }
        }
        Value::Str(s) => quote! { ::cfgy::__private::Node::Str(#s) },
        Value::Datetime(Datetime(s)) => quote! { ::cfgy::__private::Node::Datetime(#s) },
        Value::Seq(items) => {
            let items = items.iter().map(document);
            quote! { ::cfgy::__private::Node::Seq(&[#(#items),*]) }
        }
        Value::Map(entries) => {
            let entries = entries.iter().map(|(key, value)| {
                let value = document(value);
                quote! { (#key, #value) }
            });
            quote! { ::cfgy::__private::Node::Map(&[#(#entries),*]) }
        }
    }
}

/// An `f64` expression for `f`: a suffixed literal, or the named constant for NaN and the infinities, which have no
/// literal form.
fn float(f: f64) -> TokenStream2 {
    if f.is_nan() {
        quote! { ::core::primitive::f64::NAN }
    } else if f == f64::INFINITY {
        quote! { ::core::primitive::f64::INFINITY }
    } else if f == f64::NEG_INFINITY {
        quote! { ::core::primitive::f64::NEG_INFINITY }
    } else {
        Literal::f64_suffixed(f).into_token_stream()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use cfgy_core::IndexMap;
    use std::path::PathBuf;
    use syn::parse_quote;

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(entries.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect::<IndexMap<_, _>>())
    }

    /// A fresh directory holding `name` with `contents`, unique per test process and name.
    fn manifest_dir(name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cfgy-check-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), contents).unwrap();
        dir
    }

    fn source(path: &str) -> SourcePlan {
        SourcePlan { path: LitStr::new(path, Span::call_site()), format: None, check: true }
    }

    fn ident() -> Ident {
        Ident::new("Settings", Span::call_site())
    }

    /// The tokens emitted for a file `name` holding `contents`, or the error messages.
    fn check_file(name: &str, contents: &str) -> Result<String, Vec<String>> {
        check_source(&manifest_dir(name, contents), &source(name), &ident())
            .map(|tokens| tokens.to_string())
            .map_err(|err| err.into_iter().map(|err| err.to_string()).collect())
    }

    /// The checks for a value parsed from `text` with `spans`.
    fn checks(value: &Value, text: &str, spans: &Spans) -> Vec<(Vec<usize>, String)> {
        let mut collector = Collector { file: "c.toml", text, spans, indices: Vec::new(), checks: Vec::new() };
        collector.visit(value, &mut PathStack::new());
        collector.checks.into_iter().map(|check| (check.indices, check.prefix)).collect()
    }

    #[test]
    fn missing_file_fails() {
        let dir = manifest_dir("other.txt", "");
        let err = check_source(&dir, &source("missing.toml"), &ident()).unwrap_err().to_string();
        assert_eq!(err, "config file `missing.toml` does not exist");
    }

    #[test]
    fn unknown_extension_fails() {
        let errors = check_file("config.ini", "a = 1").unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("unknown extension `.ini`"), "{errors:?}");
    }

    #[test]
    fn no_path_or_check_false_emits_nothing() {
        let input = parse_quote! {
            #[config(path = "does/not/exist.toml", check = false)]
            struct S { port: u16 }
        };
        assert!(check(&plan::build(&input).unwrap()).unwrap().is_empty());
        assert!(check(&plan::build(&parse_quote! { struct S { port: u16 } }).unwrap()).unwrap().is_empty());
    }

    #[cfg(feature = "toml")]
    #[test]
    fn malformed_toml_reports_line_and_col() {
        let errors = check_file("bad.toml", "a = 1\nb = = 2").unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].starts_with("bad.toml:2:5: "), "{errors:?}");
    }

    #[cfg(feature = "toml")]
    #[test]
    fn emits_the_document_and_one_check_per_value() {
        let dir = manifest_dir("ok.toml", "port = \"x\"\n[database]\nurl = 1\n");
        let out = check_source(&dir, &source("ok.toml"), &ident()).unwrap().to_string();
        let resolved = LitStr::new(&dir.join("ok.toml").to_string_lossy(), Span::call_site()).to_token_stream();
        let check = |indices: &str, prefix: &str| {
            format!(
                "const _ : () = :: cfgy :: __private :: check_node (< Settings as :: cfgy :: FromValue > :: SHAPE , \
                 & __CFGY_DOC , & [{indices}] , \"{prefix}\" ,) ;"
            )
        };
        let expected = format!(
            "const _ : () = {{ const _ : & str = :: core :: include_str ! ({resolved}) ; # [allow (clippy :: \
             approx_constant , reason = \"values copied from the config file, such as `pi = 3.14159`\")] const \
             __CFGY_DOC : :: cfgy :: __private :: Node = {doc} ; {} {} {} {} }} ;",
            check("", "ok.toml:1:1: `<root>`"),
            check("0", "ok.toml:1:8: `port`"),
            check("1", "ok.toml:2:1: `database`"),
            check("1 , 0", "ok.toml:3:7: `database.url`"),
            doc = doc(&map(&[("port", Value::Str("x".into())), ("database", map(&[("url", Value::Int(1))]))])),
        );
        assert_eq!(out, expected);
        syn::parse2::<syn::File>(check_source(&dir, &source("ok.toml"), &ident()).unwrap()).unwrap();
    }

    #[cfg(feature = "toml")]
    #[test]
    fn check_paths_follow_sequences_and_tables() {
        let text = "[[servers]]\nport = 1\n[[servers]]\nhost = \"a\"\nport = \"x\"\n";
        let parsed = cfgy_core::Format::parse(&cfgy_formats::Toml, text).unwrap();
        assert_eq!(
            checks(&parsed.value, text, &parsed.spans),
            [
                (vec![], "c.toml:1:1: `<root>`".to_owned()),
                (vec![0], "c.toml:1:1: `servers`".to_owned()),
                (vec![0, 0], "c.toml:1:1: `servers[0]`".to_owned()),
                (vec![0, 0, 0], "c.toml:2:8: `servers[0].port`".to_owned()),
                (vec![0, 1], "c.toml:3:1: `servers[1]`".to_owned()),
                (vec![0, 1, 0], "c.toml:4:8: `servers[1].host`".to_owned()),
                (vec![0, 1, 1], "c.toml:5:8: `servers[1].port`".to_owned()),
            ]
        );
    }

    #[test]
    fn check_without_a_span_names_only_the_file_and_path() {
        let value = map(&[("a", Value::Seq(vec![Value::Null]))]);
        assert_eq!(
            checks(&value, "", &Spans::default()),
            [
                (vec![], "c.toml: `<root>`".to_owned()),
                (vec![0], "c.toml: `a`".to_owned()),
                (vec![0, 0], "c.toml: `a[0]`".to_owned()),
            ]
        );
    }

    #[cfg(feature = "json")]
    #[test]
    fn explicit_format_overrides_extension() {
        let mut source = source("config.txt");
        source.format = Some(LitStr::new("json", Span::call_site()));
        let dir = manifest_dir("config.txt", "{\n  \"port\": true\n}");
        let out = check_source(&dir, &source, &ident()).unwrap().to_string();
        assert!(out.contains("\"config.txt:2:11: `port`\""), "{out}");
    }

    fn doc(value: &Value) -> String {
        document(value).to_string()
    }

    #[test]
    fn document_scalars() {
        assert_eq!(doc(&Value::Null), ":: cfgy :: __private :: Node :: Null");
        assert_eq!(doc(&Value::Bool(true)), ":: cfgy :: __private :: Node :: Bool (true)");
        assert_eq!(doc(&Value::Int(8080)), ":: cfgy :: __private :: Node :: Int (8080i128)");
        assert_eq!(doc(&Value::Int(-3)), ":: cfgy :: __private :: Node :: Int (- 3i128)");
        assert_eq!(
            doc(&Value::Int(i128::from(u64::MAX))),
            ":: cfgy :: __private :: Node :: Int (18446744073709551615i128)"
        );
        assert_eq!(doc(&Value::Str("a \"b\"\n".into())), r#":: cfgy :: __private :: Node :: Str ("a \"b\"\n")"#);
        assert_eq!(
            doc(&Value::Datetime(Datetime("1979-05-27T07:32:00Z".into()))),
            r#":: cfgy :: __private :: Node :: Datetime ("1979-05-27T07:32:00Z")"#
        );
    }

    #[test]
    fn document_floats() {
        assert_eq!(doc(&Value::Float(1.5)), ":: cfgy :: __private :: Node :: Float (1.5f64)");
        assert_eq!(doc(&Value::Float(-0.25)), ":: cfgy :: __private :: Node :: Float (- 0.25f64)");
        assert_eq!(doc(&Value::Float(3.0)), ":: cfgy :: __private :: Node :: Float (3f64)");
        assert_eq!(
            doc(&Value::Float(f64::NAN)),
            ":: cfgy :: __private :: Node :: Float (:: core :: primitive :: f64 :: NAN)"
        );
        assert_eq!(
            doc(&Value::Float(f64::INFINITY)),
            ":: cfgy :: __private :: Node :: Float (:: core :: primitive :: f64 :: INFINITY)"
        );
        assert_eq!(
            doc(&Value::Float(f64::NEG_INFINITY)),
            ":: cfgy :: __private :: Node :: Float (:: core :: primitive :: f64 :: NEG_INFINITY)"
        );
        // Shortest round-trip text, so the const holds exactly the parsed value.
        let parsed: f64 = Literal::f64_suffixed(0.1 + 0.2).to_string().trim_end_matches("f64").parse().unwrap();
        assert_eq!(parsed.to_bits(), (0.1_f64 + 0.2).to_bits());
    }

    #[test]
    fn document_containers_keep_order() {
        let value = map(&[
            ("b", Value::Seq(vec![Value::Int(1), map(&[("x", Value::Null)])])),
            ("a", Value::Seq(vec![])),
            ("c", map(&[])),
        ]);
        assert_eq!(
            doc(&value),
            ":: cfgy :: __private :: Node :: Map (& [(\"b\" , :: cfgy :: __private :: Node :: Seq (& [:: cfgy :: \
             __private :: Node :: Int (1i128) , :: cfgy :: __private :: Node :: Map (& [(\"x\" , :: cfgy :: __private \
             :: Node :: Null)])])) , (\"a\" , :: cfgy :: __private :: Node :: Seq (& [])) , (\"c\" , :: cfgy :: \
             __private :: Node :: Map (& []))])"
        );
        // Parses as an expression.
        syn::parse2::<syn::Expr>(document(&value)).unwrap();
    }
}
