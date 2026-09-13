//! Compile-time check: the config file's parsed [`Value`] is type-checked against the field plan.

use std::path::{Path, PathBuf};
use std::{env, fs};

use cfgy_core::{ConfigError, Datetime, FromValue, PathStack, Value};
use syn::{GenericArgument, LitStr, PathArguments, PathSegment, Type};

use crate::plan::{FieldPlan, SourcePlan, StructPlan};

/// Reads, parses, and type-checks the config file of `plan`, unless it has no `path` or sets `check = false`.
///
/// Returns the resolved path of the checked file, so the caller can register it as a build dependency.
pub fn check(plan: &StructPlan) -> syn::Result<Option<PathBuf>> {
    let Some(source) = plan.source.as_ref().filter(|source| source.check) else {
        return Ok(None);
    };
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| syn::Error::new(source.path.span(), "`CARGO_MANIFEST_DIR` is not set"))?;
    check_source(Path::new(&manifest_dir), source, &plan.fields).map(Some)
}

/// Checks the file `source` points at, relative to `manifest_dir`, with every error spanned on its `path` literal.
///
/// Messages name the file as written in the attribute, not the resolved absolute path, so they read the same on
/// every machine.
pub fn check_source(manifest_dir: &Path, source: &SourcePlan, fields: &[FieldPlan]) -> syn::Result<PathBuf> {
    let file = source.path.value();
    let resolved = manifest_dir.join(&file);
    let error = |message: String| syn::Error::new(source.path.span(), message);

    let text =
        fs::read_to_string(&resolved).map_err(|err| error(format!("cannot read config file `{file}`: {err}")))?;
    let format = cfgy_formats::select(source.format.as_ref().map(LitStr::value).as_deref(), &resolved, &text)
        .map_err(|err| error(format!("config file `{file}`: {err}")))?;
    let value = format.parse(&text).map_err(|err| match err.line_col(&text) {
        Some((line, col)) => error(format!("{file}:{line}:{col}: {}", err.message)),
        None => error(format!("{file}: {}", err.message)),
    })?;

    check_fields(fields, &value)
        .into_iter()
        .map(|message| error(format!("{file}: {message}")))
        .reduce(|mut all, next| {
            all.combine(next);
            all
        })
        .map_or(Ok(resolved), Err)
}

/// Checks `root` against `fields`, returning every error message rather than stopping at the first.
pub fn check_fields(fields: &[FieldPlan], root: &Value) -> Vec<String> {
    let mut path = PathStack::new();
    let Value::Map(map) = root else {
        return vec![ConfigError::type_mismatch(&path, "table", root).to_string()];
    };
    let mut errors = Vec::new();
    for field in fields {
        path.with_key(&field.key, |path| match map.get(&field.key) {
            None if field.required() => errors.push(ConfigError::missing(path).to_string()),
            Some(Value::Null) if field.optional.is_some() => {}
            None => {}
            Some(value) => check_type(field.optional.as_ref().unwrap_or(&field.ty), value, path, &mut errors),
        });
    }
    errors
}

/// Checks `value` against `ty` by the type's syntax, pushing a message per mismatch onto `errors`.
///
/// Leaf scalars go through the runtime [`FromValue`] impls, so both phases share one coercion policy.
fn check_type(ty: &Type, value: &Value, path: &mut PathStack, errors: &mut Vec<String>) {
    let mut push = |result: Result<(), ConfigError>| {
        if let Err(error) = result {
            errors.push(error.to_string());
        }
    };
    let segment = match ty {
        Type::Group(group) => return check_type(&group.elem, value, path, errors),
        Type::Paren(paren) => return check_type(&paren.elem, value, path, errors),
        Type::Tuple(tuple) => {
            let Value::Seq(items) = value else {
                return push(Err(ConfigError::type_mismatch(path, "sequence", value)));
            };
            if items.len() != tuple.elems.len() {
                return push(Err(ConfigError::length(path, tuple.elems.len(), items.len())));
            }
            for (index, (elem, item)) in tuple.elems.iter().zip(items).enumerate() {
                path.with_index(index, |path| check_type(elem, item, path, errors));
            }
            return;
        }
        Type::Path(type_path) if type_path.qself.is_none() => match type_path.path.segments.last() {
            Some(segment) => segment,
            None => return,
        },
        // References, arrays, slices, trait objects, `<T as Trait>::Assoc`, ...: no runtime impl to mirror, so
        // leave them to rustc.
        _ => return,
    };
    match segment.ident.to_string().as_str() {
        "i8" => push(leaf::<i8>(value, path)),
        "i16" => push(leaf::<i16>(value, path)),
        "i32" => push(leaf::<i32>(value, path)),
        "i64" => push(leaf::<i64>(value, path)),
        "i128" => push(leaf::<i128>(value, path)),
        "isize" => push(leaf::<isize>(value, path)),
        "u8" => push(leaf::<u8>(value, path)),
        "u16" => push(leaf::<u16>(value, path)),
        "u32" => push(leaf::<u32>(value, path)),
        "u64" => push(leaf::<u64>(value, path)),
        "u128" => push(leaf::<u128>(value, path)),
        "usize" => push(leaf::<usize>(value, path)),
        "f32" => push(leaf::<f32>(value, path)),
        "f64" => push(leaf::<f64>(value, path)),
        "bool" => push(leaf::<bool>(value, path)),
        "String" => push(leaf::<String>(value, path)),
        "PathBuf" => push(leaf::<PathBuf>(value, path)),
        "char" => push(leaf::<char>(value, path)),
        "Datetime" => push(leaf::<Datetime>(value, path)),
        "Box" | "Rc" | "Arc" => {
            if let Some(inner) = type_arg(segment, 0) {
                check_type(inner, value, path, errors);
            }
        }
        "Option" => match (value, type_arg(segment, 0)) {
            (Value::Null, _) | (_, None) => {}
            (_, Some(inner)) => check_type(inner, value, path, errors),
        },
        "Vec" | "VecDeque" | "HashSet" => {
            let Value::Seq(items) = value else {
                return push(Err(ConfigError::type_mismatch(path, "sequence", value)));
            };
            if let Some(inner) = type_arg(segment, 0) {
                for (index, item) in items.iter().enumerate() {
                    path.with_index(index, |path| check_type(inner, item, path, errors));
                }
            }
        }
        "HashMap" | "BTreeMap" | "IndexMap" => {
            let Value::Map(entries) = value else {
                return push(Err(ConfigError::type_mismatch(path, "table", value)));
            };
            if let Some(inner) = type_arg(segment, 1) {
                for (key, item) in entries {
                    path.with_key(key, |path| check_type(inner, item, path, errors));
                }
            }
        }
        // ponytail: any other type path is assumed to be a struct deriving `FromConfig` and is only required to be a
        // table; its fields are not checked, so nested struct errors (and type aliases of scalars) surface at runtime
        // or wrongly here. Upgrade path: have the derive emit a per-type `const SHAPE` and check against that instead.
        _ => {
            if !matches!(value, Value::Map(_)) {
                push(Err(ConfigError::type_mismatch(path, "table", value)));
            }
        }
    }
}

/// Converts `value` with the runtime impl for `T`, keeping only the error.
fn leaf<T: FromValue>(value: &Value, path: &mut PathStack) -> Result<(), ConfigError> {
    T::from_value(value, path).map(drop)
}

/// The `index`th type argument of `segment`, e.g. `V` at index 1 of `HashMap<K, V>`.
fn type_arg(segment: &PathSegment, index: usize) -> Option<&Type> {
    let PathArguments::AngleBracketed(generics) = &segment.arguments else { return None };
    generics
        .args
        .iter()
        .filter_map(|arg| match arg {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .nth(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use cfgy_core::IndexMap;
    use syn::{DeriveInput, parse_quote};

    const NONE: [String; 0] = [];

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(entries.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect::<IndexMap<_, _>>())
    }

    fn errors(input: &DeriveInput, root: &Value) -> Vec<String> {
        check_fields(&plan::build(input).unwrap().fields, root)
    }

    fn one_error(input: &DeriveInput, root: &Value) -> String {
        let errors = errors(input, root);
        assert_eq!(errors.len(), 1, "{errors:?}");
        errors.into_iter().next().unwrap()
    }

    /// A fresh directory holding `name` with `contents`, unique per test process and name.
    fn manifest_dir(name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cfgy-check-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), contents).unwrap();
        dir
    }

    fn source(path: &str) -> SourcePlan {
        SourcePlan { path: LitStr::new(path, proc_macro2::Span::call_site()), format: None, check: true }
    }

    fn check_file(name: &str, contents: &str, input: &DeriveInput) -> Vec<String> {
        let fields = plan::build(input).unwrap().fields;
        match check_source(&manifest_dir(name, contents), &source(name), &fields) {
            Ok(_) => Vec::new(),
            Err(err) => err.into_iter().map(|err| err.to_string()).collect(),
        }
    }
    #[test]
    fn string_into_u16_fails_with_key() {
        let msg = one_error(&parse_quote! { struct S { port: u16 } }, &map(&[("port", Value::Str("eighty".into()))]));
        assert!(msg.contains("port"), "{msg}");
        assert!(msg.contains("found string"), "{msg}");
    }

    #[test]
    fn out_of_range_fails() {
        let msg = one_error(&parse_quote! { struct S { port: u16 } }, &map(&[("port", Value::Int(70000))]));
        assert_eq!(msg, "`port`: 70000 is out of range for u16");
    }

    #[test]
    fn float_into_int_fails_but_int_into_float_passes() {
        let input = parse_quote! { struct S { a: i32, b: f64 } };
        let msg = one_error(&input, &map(&[("a", Value::Float(1.0)), ("b", Value::Int(1))]));
        assert_eq!(msg, "`a`: expected i32, found float");
    }

    #[test]
    fn missing_required_key() {
        let msg = one_error(&parse_quote! { struct S { port: u16 } }, &map(&[]));
        assert_eq!(msg, "missing required key `port`");
    }

    #[test]
    fn missing_key_with_default_or_option_passes() {
        let input = parse_quote! {
            struct S {
                a: Option<u16>,
                #[config(default = 1)]
                b: u16,
            }
        };
        assert_eq!(errors(&input, &map(&[])), NONE);
    }

    #[test]
    fn null_is_an_error_only_for_non_option_fields() {
        let input = parse_quote! { struct S { a: Option<u16>, b: u16 } };
        let msg = one_error(&input, &map(&[("a", Value::Null), ("b", Value::Null)]));
        assert!(msg.starts_with("`b`:") && msg.contains("null"), "{msg}");
    }

    #[test]
    fn vec_element_error_has_index() {
        let root = map(&[("ports", Value::Seq(vec![Value::Int(1), Value::Str("x".into())]))]);
        let msg = one_error(&parse_quote! { struct S { ports: Vec<u16> } }, &root);
        assert!(msg.contains("ports[1]"), "{msg}");
    }

    #[test]
    fn nested_option_accepts_null() {
        let root = map(&[("ports", Value::Seq(vec![Value::Null, Value::Int(1)]))]);
        assert_eq!(errors(&parse_quote! { struct S { ports: Vec<Option<u16>> } }, &root), NONE);
    }

    #[test]
    fn map_value_error_has_key() {
        let root = map(&[("features", map(&[("a", Value::Bool(true)), ("b", Value::Int(1))]))]);
        let msg = one_error(&parse_quote! { struct S { features: std::collections::HashMap<String, bool> } }, &root);
        assert_eq!(msg, "`features.b`: expected bool, found integer");
    }

    #[test]
    fn tuple_checks_length_and_elements() {
        let input = parse_quote! { struct S { t: (u8, Box<String>) } };
        assert_eq!(errors(&input, &map(&[("t", Value::Seq(vec![Value::Int(1), Value::Str("a".into())]))])), NONE);
        let msg = one_error(&input, &map(&[("t", Value::Seq(vec![Value::Int(1)]))]));
        assert!(msg.contains("expected a sequence of 2 items, found 1"), "{msg}");
        let msg = one_error(&input, &map(&[("t", Value::Seq(vec![Value::Int(1), Value::Int(2)]))]));
        assert_eq!(msg, "`t[1]`: expected string, found integer");
    }

    #[test]
    fn unknown_struct_type_only_requires_a_table() {
        let input = parse_quote! { struct S { database: Database } };
        assert_eq!(errors(&input, &map(&[("database", map(&[("port", Value::Str("x".into()))]))])), NONE);
        let msg = one_error(&input, &map(&[("database", Value::Str("x".into()))]));
        assert_eq!(msg, "`database`: expected table, found string");
    }

    #[test]
    fn root_must_be_a_table() {
        assert_eq!(
            one_error(&parse_quote! { struct S { a: u8 } }, &Value::Seq(vec![])),
            "`<root>`: expected table, found sequence"
        );
    }

    #[test]
    fn collects_every_error() {
        let input = parse_quote! { struct S { host: String, port: u16 } };
        let errors = errors(&input, &map(&[("host", Value::Int(1)), ("port", Value::Bool(true))]));
        assert_eq!(errors, ["`host`: expected string, found integer", "`port`: expected u16, found bool"]);
    }

    #[test]
    fn missing_file_fails() {
        let dir = manifest_dir("other.txt", "");
        let err = check_source(&dir, &source("missing.toml"), &[]).unwrap_err().to_string();
        assert!(err.starts_with("cannot read config file `missing.toml`: "), "{err}");
    }

    #[test]
    fn unknown_extension_fails() {
        let errors = check_file("config.ini", "a = 1", &parse_quote! { struct S {} });
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("unknown extension `.ini`"), "{errors:?}");
    }

    #[test]
    fn check_false_skips_missing_file() {
        let input = parse_quote! {
            #[config(path = "does/not/exist.toml", check = false)]
            struct S { port: u16 }
        };
        assert!(check(&plan::build(&input).unwrap()).is_ok());
        assert!(check(&plan::build(&parse_quote! { struct S { port: u16 } }).unwrap()).is_ok());
    }

    #[cfg(feature = "toml")]
    #[test]
    fn malformed_toml_reports_line_and_col() {
        let dir = manifest_dir("bad.toml", "a = 1\nb = = 2");
        let err = check_source(&dir, &source("bad.toml"), &[]).unwrap_err().to_string();
        assert!(err.starts_with("bad.toml:2:5: "), "{err}");
    }

    #[cfg(feature = "toml")]
    #[test]
    fn valid_file_passes() {
        let input = parse_quote! { struct S { host: String, port: u16, database: Database } };
        let errors = check_file("ok.toml", "host = \"localhost\"\nport = 8080\n[database]\nurl = 1\n", &input);
        assert_eq!(errors, NONE);
        let dir = manifest_dir("ok.toml", "host = \"a\"\nport = 1\n[database]\n");
        let fields = plan::build(&input).unwrap().fields;
        assert_eq!(check_source(&dir, &source("ok.toml"), &fields).unwrap(), dir.join("ok.toml"));
    }

    #[cfg(feature = "toml")]
    #[test]
    fn type_errors_in_file_fail() {
        let input = parse_quote! { struct S { host: String, port: u16 } };
        let errors = check_file("types.toml", "port = \"eighty\"\n", &input);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].ends_with("types.toml: missing required key `host`"), "{errors:?}");
        assert!(errors[1].ends_with("types.toml: `port`: expected u16, found string"), "{errors:?}");
    }

    #[cfg(feature = "json")]
    #[test]
    fn explicit_format_overrides_extension() {
        let mut source = source("config.txt");
        source.format = Some(LitStr::new("json", proc_macro2::Span::call_site()));
        let fields = plan::build(&parse_quote! { struct S { port: u16 } }).unwrap().fields;
        let dir = manifest_dir("config.txt", r#"{"port": true}"#);
        let err = check_source(&dir, &source, &fields).unwrap_err().to_string();
        assert!(err.ends_with("`port`: expected u16, found bool"), "{err}");
    }
}
