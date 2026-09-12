//! Front end of the derive: turns a [`DeriveInput`] into a [`StructPlan`].

use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::meta::ParseNestedMeta;
use syn::{
    Attribute, Data, DeriveInput, Error, Expr, Fields, GenericArgument, Generics, Ident, LitBool, LitStr,
    PathArguments, Type,
};

/// Everything codegen needs to know about a derived struct.
pub struct StructPlan {
    pub ident: Ident,
    pub generics: Generics,
    /// `Some` if and only if `#[config(path = "...")]` is present.
    pub source: Option<SourcePlan>,
    pub fields: Vec<FieldPlan>,
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
    /// `Some(inner)` when the field is `Option<inner>`.
    pub optional: Option<Type>,
    pub default: Option<Expr>,
}

impl FieldPlan {
    /// A key absent from the config is an error only for required fields.
    pub const fn required(&self) -> bool {
        self.optional.is_none() && self.default.is_none()
    }
}

/// Builds the plan for `input`, rejecting shapes the derive does not support.
pub fn build(input: &DeriveInput) -> syn::Result<StructPlan> {
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            Fields::Unnamed(fields) => {
                return Err(Error::new_spanned(fields, "`FromConfig` cannot be derived for tuple structs"));
            }
            Fields::Unit => {
                return Err(Error::new(input.ident.span(), "`FromConfig` cannot be derived for unit structs"));
            }
        },
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

    let mut plans: Vec<FieldPlan> = Vec::with_capacity(fields.len());
    for field in fields {
        let Some(ident) = field.ident.clone() else { continue };
        let options = parse_options(&field.attrs, Site::Field, &mut errors);
        let (key, key_span) = options
            .rename
            .map_or_else(|| (ident.unraw().to_string(), ident.span()), |rename| (rename.value(), rename.span()));
        if plans.iter().any(|plan| plan.key == key) {
            errors.push(Error::new(key_span, format!("duplicate config key `{key}`")));
        }
        plans.push(FieldPlan {
            ident,
            key,
            key_span,
            ty: field.ty.clone(),
            optional: option_inner(&field.ty).cloned(),
            default: options.default,
        });
    }

    if let Some(error) = errors.into_iter().reduce(|mut acc, error| {
        acc.combine(error);
        acc
    }) {
        return Err(error);
    }

    Ok(StructPlan { ident: input.ident.clone(), generics: input.generics.clone(), source, fields: plans })
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
    default: Option<Expr>,
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

/// Parses one `key = value` option. Misplaced and duplicate options are pushed to `errors`
/// so parsing continues; unknown options and malformed values abort the attribute.
fn parse_option(options: &mut Options, meta: &ParseNestedMeta, site: Site, errors: &mut Vec<Error>) -> syn::Result<()> {
    let Some(key) = meta.path.get_ident().cloned() else {
        return Err(meta.error("unknown `config` option"));
    };
    let (expected, duplicate) = match key.to_string().as_str() {
        "path" => (Site::Struct, options.path.replace(meta.value()?.parse()?).is_some()),
        "format" => (Site::Struct, options.format.replace((key.clone(), meta.value()?.parse()?)).is_some()),
        "check" => (Site::Struct, options.check.replace((key.clone(), meta.value()?.parse()?)).is_some()),
        "rename" => (Site::Field, options.rename.replace(meta.value()?.parse()?).is_some()),
        "default" => (Site::Field, options.default.replace(meta.value()?.parse()?).is_some()),
        _ => return Err(meta.error(format!("unknown `config` option `{key}`"))),
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

/// Turns struct-level options into a [`SourcePlan`]; `format` and `check` require `path`.
fn source_plan(options: Options, errors: &mut Vec<Error>) -> Option<SourcePlan> {
    let Some(path) = options.path else {
        let keys = options.format.map(|(key, _)| key).into_iter().chain(options.check.map(|(key, _)| key));
        errors.extend(keys.map(|key| Error::new(key.span(), format!("`{key}` requires `path`"))));
        return None;
    };
    Some(SourcePlan {
        path,
        format: options.format.map(|(_, format)| format),
        check: options.check.is_none_or(|(_, check)| check.value),
    })
}

/// Returns `T` if `ty` is syntactically `Option<T>`, `std::option::Option<T>` or `core::option::Option<T>`.
fn option_inner(ty: &Type) -> Option<&Type> {
    let path = match ty {
        Type::Group(group) => return option_inner(&group.elem),
        Type::Path(type_path) if type_path.qself.is_none() => &type_path.path,
        _ => return None,
    };
    let idents: Vec<String> = path.segments.iter().map(|segment| segment.ident.to_string()).collect();
    let idents: Vec<&str> = idents.iter().map(String::as_str).collect();
    let prefix_is_bare = path.segments.iter().rev().skip(1).all(|segment| segment.arguments.is_none());
    if !prefix_is_bare || !matches!(idents.as_slice(), ["Option"] | ["std" | "core", "option", "Option"]) {
        return None;
    }
    let PathArguments::AngleBracketed(generics) = &path.segments.last()?.arguments else { return None };
    let mut args = generics.args.iter();
    match (args.next(), args.next()) {
        (Some(GenericArgument::Type(inner)), None) => Some(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn error(input: &DeriveInput) -> String {
        build(input).err().expect("expected an error").to_string()
    }

    #[test]
    fn named_struct_builds() {
        let plan = build(&parse_quote! {
            struct Settings { port: u16, host: String }
        })
        .unwrap();
        assert_eq!(plan.ident, "Settings");
        assert!(plan.source.is_none());
        let keys: Vec<_> = plan.fields.iter().map(|f| f.key.as_str()).collect();
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
    fn rejects_tuple_struct() {
        assert!(error(&parse_quote! { struct T(u8); }).contains("tuple structs"));
    }

    #[test]
    fn rejects_unit_struct() {
        assert!(error(&parse_quote! { struct U; }).contains("unit structs"));
    }

    fn messages(input: &DeriveInput) -> Vec<String> {
        build(input).err().expect("expected an error").into_iter().map(|e| e.to_string()).collect()
    }

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
        assert_eq!(plan.fields[0].default, Some(expected));
    }

    #[test]
    fn rejects_unknown_option() {
        assert!(
            error(&parse_quote! { #[config(nope = 1)] struct S { a: u8 } }).contains("unknown `config` option `nope`")
        );
        assert!(error(&parse_quote! { struct S { #[config(nope)] a: u8 } }).contains("unknown `config` option `nope`"));
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
        plan.fields.iter().find(|f| f.ident == name).unwrap()
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
        let ty = &plan.fields[0];
        assert_eq!(ty.ident, "r#type");
        assert_eq!(ty.key, "type");
    }

    #[test]
    fn rejects_duplicate_key_after_rename() {
        let msg = error(&parse_quote! {
            struct S {
                port: u16,
                #[config(rename = "port")]
                other: u16,
            }
        });
        assert_eq!(msg, "duplicate config key `port`");
    }

    #[test]
    fn peels_option() {
        let plan = build(&parse_quote! {
            struct S {
                a: Option<u8>,
                b: std::option::Option<u8>,
                c: ::core::option::Option<u8>,
                d: Option<Vec<u8>>,
                e: Vec<Option<u8>>,
                f: u8,
                g: foo::Option<u8>,
            }
        })
        .unwrap();
        let u8_ty: Type = parse_quote!(u8);
        let vec_ty: Type = parse_quote!(Vec<u8>);
        assert_eq!(field(&plan, "a").optional.as_ref(), Some(&u8_ty));
        assert_eq!(field(&plan, "b").optional.as_ref(), Some(&u8_ty));
        assert_eq!(field(&plan, "c").optional.as_ref(), Some(&u8_ty));
        assert_eq!(field(&plan, "d").optional.as_ref(), Some(&vec_ty));
        assert!(field(&plan, "e").optional.is_none());
        assert!(field(&plan, "f").optional.is_none());
        assert!(field(&plan, "g").optional.is_none());
    }

    #[test]
    fn required_only_without_option_or_default() {
        let plan = build(&parse_quote! {
            struct S {
                a: u8,
                b: Option<u8>,
                #[config(default = 1)]
                c: u8,
            }
        })
        .unwrap();
        assert!(field(&plan, "a").required());
        assert!(!field(&plan, "b").required());
        assert!(!field(&plan, "c").required());
    }

    #[test]
    fn rejects_generics() {
        let msg = "generic structs are not supported yet";
        assert_eq!(error(&parse_quote! { struct G<T> { t: T } }), msg);
        assert_eq!(error(&parse_quote! { struct L<'a> { s: &'a str } }), msg);
    }
}
