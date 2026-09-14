//! Front end of the derive: turns a [`DeriveInput`] into a [`StructPlan`].

use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::meta::ParseNestedMeta;
use syn::{Attribute, Data, DeriveInput, Error, Expr, Fields, Generics, Ident, LitBool, LitStr, Token, Type};

/// Everything codegen needs to know about a derived struct.
pub struct StructPlan {
    pub ident: Ident,
    pub generics: Generics,
    /// `Some` if and only if `#[config(path = "...")]` is present.
    pub source: Option<SourcePlan>,
    pub kind: StructKind,
}

/// The field layout of the derived struct.
pub enum StructKind {
    /// `struct S { .. }`: reads a table.
    Named(Vec<FieldPlan>),
    /// `struct S(A, B)`: reads a sequence of exactly this many items, except that one field is a transparent newtype.
    Tuple(Vec<Type>),
    /// `struct S;`: reads null.
    Unit,
}

/// The struct-level `#[config(path, format, check)]` options.
pub struct SourcePlan {
    pub path: LitStr,
    pub format: Option<LitStr>,
    /// Whether the file is checked at compile time; defaults to `true`.
    pub check: bool,
}

/// One named field and the config key it maps to.
pub struct FieldPlan {
    pub ident: Ident,
    pub key: String,
    pub key_span: Span,
    pub ty: Type,
    /// Used for an absent key; without one, the type's `FromValue::from_missing` decides.
    pub default: Option<FieldDefault>,
}

/// A field's `#[config(default)]` or `#[config(default = <expr>)]`.
pub enum FieldDefault {
    /// Bare `default`: `Default::default()`.
    Trait,
    /// `default = <expr>`, converted into the field type through `DefaultFrom`.
    Expr(Expr),
}

/// Builds the plan for `input`, rejecting shapes the derive does not support.
pub fn build(input: &DeriveInput) -> syn::Result<StructPlan> {
    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        Data::Enum(data) => {
            return Err(Error::new(data.enum_token.span, "`FromConfig` can only be derived for structs, not enums"));
        }
        Data::Union(data) => {
            return Err(Error::new(data.union_token.span, "`FromConfig` can only be derived for structs, not unions"));
        }
    };

    if input.generics.lt_token.is_some() {
        return Err(Error::new_spanned(&input.generics, "generic structs are not supported yet"));
    }

    let mut errors = Vec::new();
    let source = source_plan(parse_options(&input.attrs, Site::Struct, &mut errors), &mut errors);

    let kind = match fields {
        Fields::Named(fields) => StructKind::Named(named_fields(&fields.named, &mut errors)),
        Fields::Unnamed(fields) => StructKind::Tuple(tuple_fields(&fields.unnamed, &mut errors)),
        Fields::Unit => StructKind::Unit,
    };
    if let (Some(source), StructKind::Tuple(_) | StructKind::Unit) = (&source, &kind) {
        errors.push(Error::new(source.path.span(), "`path` requires a struct with named fields"));
    }

    if let Some(error) = errors.into_iter().reduce(|mut acc, error| {
        acc.combine(error);
        acc
    }) {
        return Err(error);
    }

    Ok(StructPlan { ident: input.ident.clone(), generics: input.generics.clone(), source, kind })
}

/// Plans each named field, pushing option errors and keys used twice onto `errors`.
fn named_fields<'a>(fields: impl IntoIterator<Item = &'a syn::Field>, errors: &mut Vec<Error>) -> Vec<FieldPlan> {
    let mut plans: Vec<FieldPlan> = Vec::new();
    for field in fields {
        let Some(ident) = field.ident.clone() else { continue };
        let options = parse_options(&field.attrs, Site::Field, errors);
        let (key, key_span) = options
            .rename
            .map_or_else(|| (ident.unraw().to_string(), ident.span()), |rename| (rename.value(), rename.span()));
        if let Some(first) = plans.iter().find(|plan| plan.key == key) {
            let mut error = Error::new(key_span, format!("duplicate config key `{key}`"));
            error.combine(Error::new(first.key_span, "first used here"));
            errors.push(error);
        }
        plans.push(FieldPlan { ident, key, key_span, ty: field.ty.clone(), default: options.default });
    }
    plans
}

/// The types of a tuple struct's fields; `config` attributes on them are pushed onto `errors`.
fn tuple_fields<'a>(fields: impl IntoIterator<Item = &'a syn::Field>, errors: &mut Vec<Error>) -> Vec<Type> {
    let mut types = Vec::new();
    for field in fields {
        for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("config")) {
            errors.push(Error::new_spanned(attr, "`config` attributes are not supported on tuple struct fields"));
        }
        types.push(field.ty.clone());
    }
    types
}

/// Where a `#[config(...)]` attribute is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Site {
    Struct,
    Field,
}

/// Raw `#[config(...)]` options gathered from every attribute at one site.
///
/// `format` and `check` keep their key so a missing `path` can be reported on them.
#[derive(Default)]
struct Options {
    path: Option<LitStr>,
    format: Option<(Ident, LitStr)>,
    check: Option<(Ident, LitBool)>,
    rename: Option<LitStr>,
    default: Option<FieldDefault>,
}

/// Parses every `#[config(...)]` in `attrs`, pushing errors instead of stopping at the first.
fn parse_options(attrs: &[Attribute], site: Site, errors: &mut Vec<Error>) -> Options {
    let mut options = Options::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("config")) {
        if let Err(error) = attr.parse_nested_meta(|meta| parse_option(&mut options, &meta, site, errors)) {
            errors.push(error);
        }
    }
    options
}

/// Parses one `key = value` option. Unknown, misplaced, and duplicate options are pushed to `errors` so parsing
/// continues; only a malformed value aborts the attribute.
fn parse_option(options: &mut Options, meta: &ParseNestedMeta, site: Site, errors: &mut Vec<Error>) -> syn::Result<()> {
    let Some(key) = meta.path.get_ident().cloned() else {
        errors.push(meta.error("unknown `config` option"));
        return skip_value(meta);
    };
    let (expected, duplicate) = match key.to_string().as_str() {
        "path" => (Site::Struct, options.path.replace(meta.value()?.parse()?).is_some()),
        "format" => (Site::Struct, options.format.replace((key.clone(), meta.value()?.parse()?)).is_some()),
        "check" => (Site::Struct, options.check.replace((key.clone(), meta.value()?.parse()?)).is_some()),
        "rename" => (Site::Field, options.rename.replace(meta.value()?.parse()?).is_some()),
        "default" => {
            // Bare `default` means `Default::default()`, as with serde.
            let default = if meta.input.is_empty() || meta.input.peek(Token![,]) {
                FieldDefault::Trait
            } else {
                FieldDefault::Expr(meta.value()?.parse()?)
            };
            (Site::Field, options.default.replace(default).is_some())
        }
        _ => {
            errors.push(meta.error(format!("unknown `config` option `{key}`")));
            return skip_value(meta);
        }
    };
    match (site, expected) {
        (Site::Field, Site::Struct) => {
            errors.push(Error::new(
                key.span(),
                format!("`{key}` is a struct-level option and cannot be used on a field"),
            ));
        }
        (Site::Struct, Site::Field) => {
            errors.push(Error::new(
                key.span(),
                format!("`{key}` is a field-level option and cannot be used on the struct"),
            ));
        }
        _ => {}
    }
    if duplicate {
        errors.push(Error::new(key.span(), format!("duplicate `{key}` option")));
    }
    Ok(())
}

/// Consumes the `= <expr>` or `(...)` after an unknown option, so the options after it are still parsed.
fn skip_value(meta: &ParseNestedMeta) -> syn::Result<()> {
    if meta.input.peek(Token![=]) {
        meta.value()?.parse::<Expr>()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.input.parse::<proc_macro2::TokenTree>()?;
    }
    Ok(())
}

/// Turns struct-level options into a [`SourcePlan`]; `format` and `check` require `path`.
fn source_plan(options: Options, errors: &mut Vec<Error>) -> Option<SourcePlan> {
    let Some(path) = options.path else {
        let keys = options.format.map(|(key, _)| key).into_iter().chain(options.check.map(|(key, _)| key));
        errors.extend(keys.map(|key| Error::new(key.span(), format!("`{key}` requires `path`"))));
        return None;
    };
    // Validated here rather than in the file check so it also holds with `check = false`. An explicit name wins over
    // path and content in `select`, so the empty arguments are never consulted.
    if let Some((_, format)) = &options.format {
        if let Err(error) = cfgy_formats::select(Some(&format.value()), std::path::Path::new(""), "") {
            errors.push(Error::new(format.span(), error.message));
        }
    }
    Some(SourcePlan {
        path,
        format: options.format.map(|(_, format)| format),
        check: options.check.is_none_or(|(_, check)| check.value),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn error(input: &DeriveInput) -> String {
        build(input).err().expect("expected an error").to_string()
    }

    fn named(plan: &StructPlan) -> &[FieldPlan] {
        match &plan.kind {
            StructKind::Named(fields) => fields,
            _ => panic!("expected a struct with named fields"),
        }
    }

    #[test]
    fn named_struct_builds() {
        let plan = build(&parse_quote! {
            struct Settings { port: u16, host: String }
        })
        .unwrap();
        assert_eq!(plan.ident, "Settings");
        assert!(plan.source.is_none());
        let keys: Vec<_> = named(&plan).iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["port", "host"]);
    }

    #[test]
    fn rejects_enum() {
        assert!(error(&parse_quote! { enum E { A } }).contains("not enums"));
    }

    #[test]
    fn rejects_union() {
        assert!(error(&parse_quote! { union U { a: u8 } }).contains("not unions"));
    }

    #[test]
    fn tuple_and_unit_structs_build() {
        let plan = build(&parse_quote! { struct Port(u16); }).unwrap();
        let u16_ty: Type = parse_quote!(u16);
        assert!(matches!(&plan.kind, StructKind::Tuple(types) if *types == [u16_ty]));
        let plan = build(&parse_quote! { struct Pair(String, u8); }).unwrap();
        assert!(matches!(&plan.kind, StructKind::Tuple(types) if types.len() == 2));
        let plan = build(&parse_quote! { struct Marker; }).unwrap();
        assert!(matches!(plan.kind, StructKind::Unit));
    }

    #[test]
    fn rejects_path_without_named_fields() {
        let msg = "`path` requires a struct with named fields";
        assert_eq!(error(&parse_quote! { #[config(path = "a.toml")] struct Port(u16); }), msg);
        assert_eq!(error(&parse_quote! { #[config(path = "a.toml", check = false)] struct Marker; }), msg);
    }

    #[test]
    fn rejects_config_attributes_on_tuple_fields() {
        let errors = messages(&parse_quote! { struct Pair(#[config(default)] u8, #[config(rename = "b")] u8); });
        assert_eq!(errors, ["`config` attributes are not supported on tuple struct fields"; 2]);
    }

    fn messages(input: &DeriveInput) -> Vec<String> {
        build(input).err().expect("expected an error").into_iter().map(|e| e.to_string()).collect()
    }

    #[cfg(feature = "json")]
    #[test]
    fn parses_source_options() {
        let plan = build(&parse_quote! {
            #[config(path = "config.json", format = "json", check = false)]
            struct Settings { port: u16 }
        })
        .unwrap();
        let source = plan.source.unwrap();
        assert_eq!(source.path.value(), "config.json");
        assert_eq!(source.format.unwrap().value(), "json");
        assert!(!source.check);
    }

    #[test]
    fn options_merge_across_attributes_and_check_defaults_to_true() {
        let plan = build(&parse_quote! {
            #[derive(Debug)]
            #[config(path = "config.toml")]
            struct Settings { port: u16 }
        })
        .unwrap();
        let source = plan.source.unwrap();
        assert_eq!(source.path.value(), "config.toml");
        assert!(source.format.is_none());
        assert!(source.check);
    }

    #[test]
    fn keeps_default_expr() {
        let plan = build(&parse_quote! {
            struct Settings {
                #[config(default = 8080 + 1)]
                port: u16,
            }
        })
        .unwrap();
        let expected: Expr = parse_quote!(8080 + 1);
        assert!(matches!(&named(&plan)[0].default, Some(FieldDefault::Expr(expr)) if *expr == expected));
    }

    #[test]
    fn bare_default_is_the_default_trait() {
        let plan = build(&parse_quote! {
            struct Settings {
                #[config(default)]
                tags: Vec<String>,
                #[config(default, rename = "n")]
                count: u8,
            }
        })
        .unwrap();
        assert!(matches!(named(&plan)[0].default, Some(FieldDefault::Trait)));
        assert!(matches!(named(&plan)[1].default, Some(FieldDefault::Trait)));
        assert_eq!(named(&plan)[1].key, "n");
    }

    #[test]
    fn rejects_duplicate_bare_default() {
        let msg = error(&parse_quote! { struct S { #[config(default, default = 1)] a: u8 } });
        assert_eq!(msg, "duplicate `default` option");
    }

    #[test]
    fn rejects_unknown_option() {
        assert!(
            error(&parse_quote! { #[config(nope = 1)] struct S { a: u8 } }).contains("unknown `config` option `nope`")
        );
        assert!(error(&parse_quote! { struct S { #[config(nope)] a: u8 } }).contains("unknown `config` option `nope`"));
    }

    #[test]
    fn keeps_parsing_after_an_unknown_option() {
        let errors = messages(&parse_quote! {
            #[config(bogus = 1 + 2, nested(a, b = 3), a::b, path = "a.toml", rename = "x")]
            struct S {
                #[config(nope, path = "b.toml")]
                a: u8,
            }
        });
        assert_eq!(
            errors,
            [
                "unknown `config` option `bogus`",
                "unknown `config` option `nested`",
                "unknown `config` option",
                "`rename` is a field-level option and cannot be used on the struct",
                "unknown `config` option `nope`",
                "`path` is a struct-level option and cannot be used on a field",
            ]
        );
    }

    #[test]
    fn rejects_struct_option_on_field() {
        let msg = error(&parse_quote! { struct S { #[config(path = "a.toml")] a: u8 } });
        assert_eq!(msg, "`path` is a struct-level option and cannot be used on a field");
    }

    #[test]
    fn rejects_field_option_on_struct() {
        let msg = error(&parse_quote! { #[config(path = "a.toml", rename = "b")] struct S { a: u8 } });
        assert_eq!(msg, "`rename` is a field-level option and cannot be used on the struct");
    }

    #[test]
    fn rejects_unknown_format_even_without_check() {
        let msg = error(&parse_quote! { #[config(path = "a.conf", format = "ini", check = false)] struct S { a: u8 } });
        assert_eq!(msg, "unknown format `ini`, expected toml, json, or yaml");
    }

    #[cfg(not(feature = "yaml"))]
    #[test]
    fn rejects_format_with_disabled_feature() {
        let msg = error(&parse_quote! { #[config(path = "a.yml", format = "yml")] struct S { a: u8 } });
        assert!(msg.contains("enable the `yaml` cargo feature"), "{msg}");
    }

    #[test]
    fn rejects_format_and_check_without_path() {
        let errors = messages(&parse_quote! { #[config(format = "toml", check = false)] struct S { a: u8 } });
        assert_eq!(errors, ["`format` requires `path`", "`check` requires `path`"]);
    }

    #[test]
    fn rejects_duplicate_option() {
        let msg = error(&parse_quote! {
            #[config(path = "a.toml")]
            #[config(path = "b.toml")]
            struct S { a: u8 }
        });
        assert_eq!(msg, "duplicate `path` option");
    }

    #[test]
    fn accumulates_errors_across_attributes_and_fields() {
        let errors = messages(&parse_quote! {
            #[config(bogus = 1)]
            struct S {
                #[config(path = "a.toml")]
                a: u8,
                #[config(other)]
                b: u8,
            }
        });
        assert!(errors.len() >= 2, "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("`bogus`")), "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("struct-level option")), "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("`other`")), "{errors:?}");
    }

    fn field<'a>(plan: &'a StructPlan, name: &str) -> &'a FieldPlan {
        named(plan).iter().find(|f| f.ident == name).unwrap()
    }

    #[test]
    fn rename_sets_key() {
        let plan = build(&parse_quote! {
            struct S {
                #[config(rename = "listen-port")]
                port: u16,
            }
        })
        .unwrap();
        assert_eq!(field(&plan, "port").key, "listen-port");
    }

    #[test]
    fn raw_ident_key_is_unraw() {
        let plan = build(&parse_quote! { struct S { r#type: String } }).unwrap();
        let ty = &named(&plan)[0];
        assert_eq!(ty.ident, "r#type");
        assert_eq!(ty.key, "type");
    }

    #[test]
    fn rejects_duplicate_key_after_rename_pointing_at_the_first_use() {
        let errors = messages(&parse_quote! {
            struct S {
                port: u16,
                #[config(rename = "port")]
                other: u16,
            }
        });
        assert_eq!(errors, ["duplicate config key `port`", "first used here"]);
    }

    #[test]
    fn rejects_generics() {
        let msg = "generic structs are not supported yet";
        assert_eq!(error(&parse_quote! { struct G<T> { t: T } }), msg);
        assert_eq!(error(&parse_quote! { struct L<'a> { s: &'a str } }), msg);
    }
}
