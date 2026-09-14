//! Front end of the derive: turns a [`DeriveInput`] into a [`StructPlan`].

use crate::case::RenameRule;
use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::meta::ParseNestedMeta;
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Expr, Fields, Generics, Ident, LitBool, LitStr, Token, Type};

/// Everything codegen needs to know about a derived struct or enum.
pub struct StructPlan {
    pub ident: Ident,
    pub generics: Generics,
    /// `Some` if and only if `#[config(path = "...")]` is present; always `None` for an enum.
    pub source: Option<SourcePlan>,
    pub kind: StructKind,
}

/// The field layout of the derived struct, or the variants of the derived enum.
#[allow(dead_code, reason = "read by codegen once enums convert")]
pub enum StructKind {
    /// `struct S { .. }`: reads a table.
    Named(Vec<FieldPlan>),
    /// `struct S(A, B)`: reads a sequence of exactly this many items, except that one field is a transparent newtype.
    Tuple(Vec<Type>),
    /// `struct S;`: reads null.
    Unit,
    /// `enum E { .. }`: reads one of its variants, written as `repr` says.
    Enum { repr: EnumRepr, variants: Vec<VariantPlan> },
}

/// How an enum's variant is written in the file: serde's four enum representations.
#[allow(dead_code, reason = "read by codegen once enums convert")]
pub enum EnumRepr {
    /// The default: `"Unit"`, or a single-key table `{ Variant = payload }`.
    External,
    /// `#[config(tag = "type")]`: `{ type = "Variant", ..fields }`.
    Internal { tag: LitStr },
    /// `#[config(tag = "t", content = "c")]`: `{ t = "Variant", c = payload }`.
    Adjacent { tag: LitStr, content: LitStr },
    /// `#[config(untagged)]`: the first variant whose payload matches.
    Untagged,
}

/// One enum variant and the name it is written as.
#[allow(dead_code, reason = "read by codegen once enums convert")]
pub struct VariantPlan {
    pub ident: Ident,
    /// Its `rename`, else its name converted by the enum's `rename_all`, else its name.
    pub name: String,
    pub name_span: Span,
    pub kind: VariantKind,
}

/// The payload of a variant.
#[allow(dead_code, reason = "read by codegen once enums convert")]
pub enum VariantKind {
    /// `V`: no payload.
    Unit,
    /// `V(T)`: reads exactly as `T`.
    Newtype(Box<Type>),
    /// `V(A, B)` (or `V()`): a sequence of exactly this many items.
    Tuple(Vec<Type>),
    /// `V { .. }`: a table, with the same field options as a struct.
    Struct(Vec<FieldPlan>),
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
    /// `#[config(unchecked)]`: the field's shape is `Shape::Any`, so the compile-time check accepts any value.
    pub unchecked: bool,
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
    let reject_generics = |what: &str| {
        if input.generics.lt_token.is_some() {
            return Err(Error::new_spanned(&input.generics, format!("generic {what} are not supported yet")));
        }
        Ok(())
    };

    let mut errors = Vec::new();
    let (source, kind) = match &input.data {
        Data::Struct(data) => {
            reject_generics("structs")?;
            struct_kind(&input.attrs, &data.fields, &mut errors)
        }
        Data::Enum(data) => {
            reject_generics("enums")?;
            (None, enum_kind(&input.attrs, data, &mut errors))
        }
        Data::Union(data) => {
            return Err(Error::new(data.union_token.span, "`FromConfig` can only be derived for structs, not unions"));
        }
    };

    if let Some(error) = errors.into_iter().reduce(|mut acc, error| {
        acc.combine(error);
        acc
    }) {
        return Err(error);
    }

    Ok(StructPlan { ident: input.ident.clone(), generics: input.generics.clone(), source, kind })
}

/// The source options and field layout of a struct.
fn struct_kind(attrs: &[Attribute], fields: &Fields, errors: &mut Vec<Error>) -> (Option<SourcePlan>, StructKind) {
    let mut options = parse_options(attrs, Site::Struct, errors);
    let rename_all = rename_rule(options.rename_all.take(), matches!(fields, Fields::Named(_)), "struct", errors);
    let source = source_plan(options, errors);

    let kind = match fields {
        Fields::Named(fields) => StructKind::Named(named_fields(&fields.named, rename_all, errors)),
        Fields::Unnamed(fields) => StructKind::Tuple(tuple_fields(&fields.unnamed, "tuple struct", errors)),
        Fields::Unit => StructKind::Unit,
    };
    if let (Some(source), StructKind::Tuple(_) | StructKind::Unit) = (&source, &kind) {
        errors.push(Error::new(source.path.span(), "`path` requires a struct with named fields"));
    }
    (source, kind)
}

/// The representation and variants of an enum.
///
/// As in serde, the enum's `rename_all` renames its variants, and a variant's own `rename_all` renames the fields of
/// that variant.
fn enum_kind(attrs: &[Attribute], data: &DataEnum, errors: &mut Vec<Error>) -> StructKind {
    let mut options = parse_options(attrs, Site::Enum, errors);
    let rename_all = rename_rule(options.rename_all.take(), true, "enum", errors);
    let repr = enum_repr(options, errors);

    let mut variants: Vec<VariantPlan> = Vec::new();
    for variant in &data.variants {
        let mut options = parse_options(&variant.attrs, Site::Variant, errors);
        let ident = variant.ident.clone();
        let (name, name_span) = options.rename.take().map_or_else(
            || {
                let name = ident.unraw().to_string();
                (rename_all.map_or_else(|| name.clone(), |rule| rule.apply_to_variant(&name)), ident.span())
            },
            |rename| (rename.value(), rename.span()),
        );
        if let Some(first) = variants.iter().find(|plan| plan.name == name) {
            let mut error = Error::new(name_span, format!("duplicate variant name `{name}`"));
            error.combine(Error::new(first.name_span, "first used here"));
            errors.push(error);
        }

        let named = matches!(variant.fields, Fields::Named(_));
        let field_rule = rename_rule(options.rename_all.take(), named, "variant", errors);
        let kind = match &variant.fields {
            Fields::Named(fields) => VariantKind::Struct(named_fields(&fields.named, field_rule, errors)),
            Fields::Unnamed(fields) => {
                let mut types = tuple_fields(&fields.unnamed, "tuple variant", errors);
                match (types.pop(), types.is_empty()) {
                    (Some(inner), true) => VariantKind::Newtype(Box::new(inner)),
                    (last, _) => {
                        types.extend(last);
                        VariantKind::Tuple(types)
                    }
                }
            }
            Fields::Unit => VariantKind::Unit,
        };

        if let EnumRepr::Internal { tag } = &repr {
            internal_variant_errors(tag, &ident, &kind, errors);
        }
        variants.push(VariantPlan { ident, name, name_span, kind });
    }
    StructKind::Enum { repr, variants }
}

/// Variants serde also rejects under internal tagging: a tuple variant has nowhere to put the tag, and a field keyed
/// like the tag would be shadowed by it.
fn internal_variant_errors(tag: &LitStr, ident: &Ident, kind: &VariantKind, errors: &mut Vec<Error>) {
    match kind {
        VariantKind::Tuple(_) => {
            errors.push(Error::new(ident.span(), "tuple variants cannot be internally tagged with `tag`"));
        }
        VariantKind::Struct(fields) => {
            let tag = tag.value();
            errors.extend(
                fields.iter().filter(|field| field.key == tag).map(|field| {
                    Error::new(field.key_span, format!("field key `{tag}` conflicts with the enum's `tag`"))
                }),
            );
        }
        VariantKind::Unit | VariantKind::Newtype(_) => {}
    }
}

/// The enum representation chosen by `tag`, `content`, and `untagged`.
fn enum_repr(options: Options, errors: &mut Vec<Error>) -> EnumRepr {
    if let (Some(untagged), Some(_)) = (&options.untagged, &options.tag) {
        errors.push(Error::new(untagged.span(), "`untagged` cannot be combined with `tag`"));
    }
    match (options.tag, options.content) {
        (Some(tag), Some((_, content))) => {
            if tag.value() == content.value() {
                errors.push(Error::new(content.span(), "`tag` and `content` must be different keys"));
            }
            EnumRepr::Adjacent { tag, content }
        }
        (Some(tag), None) => EnumRepr::Internal { tag },
        (None, content) => {
            if let Some((key, _)) = content {
                errors.push(Error::new(key.span(), "`content` requires `tag`"));
            }
            if options.untagged.is_some() { EnumRepr::Untagged } else { EnumRepr::External }
        }
    }
}

/// Parses a `rename_all` rule, which only applies to a struct, enum, or variant that has names to rename.
fn rename_rule(rule: Option<LitStr>, applies: bool, site: &str, errors: &mut Vec<Error>) -> Option<RenameRule> {
    rule.and_then(|rule| {
        if !applies {
            errors.push(Error::new(rule.span(), format!("`rename_all` requires a {site} with named fields")));
        }
        RenameRule::from_name(&rule.value()).map_err(|message| errors.push(Error::new(rule.span(), message))).ok()
    })
}

/// Plans each named field, pushing option errors and keys used twice (after renaming) onto `errors`.
///
/// A field's key is its `rename` if given, else its name converted by `rename_all`, else its name.
fn named_fields<'a>(
    fields: impl IntoIterator<Item = &'a syn::Field>,
    rename_all: Option<RenameRule>,
    errors: &mut Vec<Error>,
) -> Vec<FieldPlan> {
    let mut plans: Vec<FieldPlan> = Vec::new();
    for field in fields {
        let Some(ident) = field.ident.clone() else { continue };
        let options = parse_options(&field.attrs, Site::Field, errors);
        let (key, key_span) = options.rename.map_or_else(
            || {
                let name = ident.unraw().to_string();
                let key = match rename_all {
                    Some(rule) => rule.apply_to_field(&name),
                    None => name,
                };
                (key, ident.span())
            },
            |rename| (rename.value(), rename.span()),
        );
        if let Some(first) = plans.iter().find(|plan| plan.key == key) {
            let mut error = Error::new(key_span, format!("duplicate config key `{key}`"));
            error.combine(Error::new(first.key_span, "first used here"));
            errors.push(error);
        }
        plans.push(FieldPlan {
            ident,
            key,
            key_span,
            ty: field.ty.clone(),
            default: options.default,
            unchecked: options.unchecked.is_some(),
        });
    }
    plans
}

/// The types of a tuple struct's or tuple variant's fields; `config` attributes on them are pushed onto `errors`.
fn tuple_fields<'a>(
    fields: impl IntoIterator<Item = &'a syn::Field>,
    owner: &str,
    errors: &mut Vec<Error>,
) -> Vec<Type> {
    let mut types = Vec::new();
    for field in fields {
        for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("config")) {
            errors.push(Error::new_spanned(attr, format!("`config` attributes are not supported on {owner} fields")));
        }
        types.push(field.ty.clone());
    }
    types
}

/// Where a `#[config(...)]` attribute is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Site {
    Struct,
    Enum,
    Variant,
    Field,
}

impl Site {
    /// How an error names the place an option was misused.
    const fn noun(self) -> &'static str {
        match self {
            Self::Struct => "the struct",
            Self::Enum => "the enum",
            Self::Variant => "a variant",
            Self::Field => "a field",
        }
    }
}

/// Raw `#[config(...)]` options gathered from every attribute at one site.
///
/// `format`, `check`, and `content` keep their key so a missing `path` or `tag` can be reported on them.
#[derive(Default)]
struct Options {
    path: Option<LitStr>,
    format: Option<(Ident, LitStr)>,
    check: Option<(Ident, LitBool)>,
    rename_all: Option<LitStr>,
    tag: Option<LitStr>,
    content: Option<(Ident, LitStr)>,
    untagged: Option<Ident>,
    rename: Option<LitStr>,
    default: Option<FieldDefault>,
    unchecked: Option<Ident>,
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
    const STRUCT: &[Site] = &[Site::Struct];
    const CONTAINER: &[Site] = &[Site::Struct, Site::Enum, Site::Variant];
    const ENUM: &[Site] = &[Site::Enum];
    const NAMED: &[Site] = &[Site::Field, Site::Variant];
    const FIELD: &[Site] = &[Site::Field];

    let Some(key) = meta.path.get_ident().cloned() else {
        errors.push(meta.error("unknown `config` option"));
        return skip_value(meta);
    };
    // Where each option may appear, the level an error names when it is misplaced, and whether it was already given.
    let (sites, level, duplicate) = match key.to_string().as_str() {
        "path" => (STRUCT, "a struct-level", options.path.replace(meta.value()?.parse()?).is_some()),
        "format" => (STRUCT, "a struct-level", options.format.replace((key.clone(), meta.value()?.parse()?)).is_some()),
        "check" => (STRUCT, "a struct-level", options.check.replace((key.clone(), meta.value()?.parse()?)).is_some()),
        "rename_all" => (CONTAINER, "a struct-level", options.rename_all.replace(meta.value()?.parse()?).is_some()),
        "tag" => (ENUM, "an enum-level", options.tag.replace(meta.value()?.parse()?).is_some()),
        "content" => (ENUM, "an enum-level", options.content.replace((key.clone(), meta.value()?.parse()?)).is_some()),
        "untagged" => (ENUM, "an enum-level", options.untagged.replace(key.clone()).is_some()),
        "rename" => (NAMED, "a field-level", options.rename.replace(meta.value()?.parse()?).is_some()),
        "default" => {
            // Bare `default` means `Default::default()`, as with serde.
            let default = if meta.input.is_empty() || meta.input.peek(Token![,]) {
                FieldDefault::Trait
            } else {
                FieldDefault::Expr(meta.value()?.parse()?)
            };
            (FIELD, "a field-level", options.default.replace(default).is_some())
        }
        "unchecked" => (FIELD, "a field-level", options.unchecked.replace(key.clone()).is_some()),
        _ => {
            errors.push(meta.error(format!("unknown `config` option `{key}`")));
            return skip_value(meta);
        }
    };
    if !sites.contains(&site) {
        errors.push(Error::new(key.span(), format!("`{key}` is {level} option and cannot be used on {}", site.noun())));
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

    fn enum_parts(plan: &StructPlan) -> (&EnumRepr, &[VariantPlan]) {
        match &plan.kind {
            StructKind::Enum { repr, variants } => (repr, variants),
            _ => panic!("expected an enum"),
        }
    }

    fn names(plan: &StructPlan) -> Vec<&str> {
        enum_parts(plan).1.iter().map(|variant| variant.name.as_str()).collect()
    }

    #[test]
    fn enum_builds_every_variant_kind() {
        let plan = build(&parse_quote! {
            enum Mode {
                Off,
                Port(u16),
                Pair(String, u8),
                Empty(),
                Serve { host: String, #[config(default = 80, rename = "p")] port: u16, #[config(unchecked)] x: u8 },
            }
        })
        .unwrap();
        assert!(plan.source.is_none());
        let (repr, variants) = enum_parts(&plan);
        assert!(matches!(repr, EnumRepr::External));
        assert_eq!(names(&plan), ["Off", "Port", "Pair", "Empty", "Serve"]);
        let u16_ty: Type = parse_quote!(u16);
        assert!(matches!(&variants[0].kind, VariantKind::Unit));
        assert!(matches!(&variants[1].kind, VariantKind::Newtype(ty) if **ty == u16_ty));
        assert!(matches!(&variants[2].kind, VariantKind::Tuple(types) if types.len() == 2));
        assert!(matches!(&variants[3].kind, VariantKind::Tuple(types) if types.is_empty()));
        let VariantKind::Struct(fields) = &variants[4].kind else { panic!("expected a struct variant") };
        let keys: Vec<_> = fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["host", "p", "x"]);
        assert!(matches!(fields[1].default, Some(FieldDefault::Expr(_))));
        assert!(fields[2].unchecked);
    }

    #[test]
    fn enum_representations() {
        let plan = build(&parse_quote! { #[config(tag = "type")] enum E { A } }).unwrap();
        assert!(matches!(enum_parts(&plan).0, EnumRepr::Internal { tag } if tag.value() == "type"));
        let plan = build(&parse_quote! { #[config(tag = "t")] #[config(content = "c")] enum E { A } }).unwrap();
        assert!(matches!(
            enum_parts(&plan).0,
            EnumRepr::Adjacent { tag, content } if tag.value() == "t" && content.value() == "c"
        ));
        let plan = build(&parse_quote! { #[config(untagged)] enum E { A } }).unwrap();
        assert!(matches!(enum_parts(&plan).0, EnumRepr::Untagged));
        let plan = build(&parse_quote! { enum E {} }).unwrap();
        assert!(enum_parts(&plan).1.is_empty());
    }

    #[test]
    fn rename_all_renames_variants_but_not_their_fields() {
        let plan = build(&parse_quote! {
            #[config(rename_all = "snake_case")]
            enum E {
                VeryFast,
                #[config(rename = "Slow")]
                SlowMode,
                WithFields { max_conns: u8 },
                #[config(rename_all = "kebab-case")]
                Renamed { max_conns: u8, #[config(rename = "X")] other: u8 },
                r#Type,
            }
        })
        .unwrap();
        assert_eq!(names(&plan), ["very_fast", "Slow", "with_fields", "renamed", "type"]);
        let keys = |index: usize| match &enum_parts(&plan).1[index].kind {
            VariantKind::Struct(fields) => fields.iter().map(|f| f.key.clone()).collect::<Vec<_>>(),
            _ => panic!("expected a struct variant"),
        };
        assert_eq!(keys(2), ["max_conns"]);
        assert_eq!(keys(3), ["max-conns", "X"]);
    }

    #[test]
    fn rejects_conflicting_representations() {
        assert_eq!(
            messages(&parse_quote! { #[config(tag = "t", untagged)] enum E { A } }),
            ["`untagged` cannot be combined with `tag`"]
        );
        assert_eq!(messages(&parse_quote! { #[config(content = "c")] enum E { A } }), ["`content` requires `tag`"]);
        assert_eq!(
            messages(&parse_quote! { #[config(untagged, content = "c")] enum E { A } }),
            ["`content` requires `tag`"]
        );
        assert_eq!(
            messages(&parse_quote! { #[config(tag = "t", content = "t")] enum E { A } }),
            ["`tag` and `content` must be different keys"]
        );
    }

    #[test]
    fn rejects_tuple_variants_and_tag_conflicts_under_internal_tagging() {
        let errors = messages(&parse_quote! {
            #[config(tag = "type")]
            enum E {
                Unit,
                Newtype(Inner),
                Pair(u8, u8),
                Empty(),
                Fields { r#type: String, #[config(rename = "type")] kind: String },
            }
        });
        assert_eq!(
            errors,
            [
                "tuple variants cannot be internally tagged with `tag`",
                "tuple variants cannot be internally tagged with `tag`",
                "duplicate config key `type`",
                "first used here",
                "field key `type` conflicts with the enum's `tag`",
                "field key `type` conflicts with the enum's `tag`",
            ]
        );
        // Adjacent tagging keeps the payload apart from the tag, so anything goes.
        assert!(build(&parse_quote! { #[config(tag = "type", content = "c")] enum E { Pair(u8, u8) } }).is_ok());
    }

    #[test]
    fn rejects_duplicate_variant_names_after_renaming() {
        let errors = messages(&parse_quote! {
            #[config(rename_all = "lowercase")]
            enum E { Fast, #[config(rename = "fast")] Quick, FAST }
        });
        assert_eq!(
            errors,
            ["duplicate variant name `fast`", "first used here", "duplicate variant name `fast`", "first used here"]
        );
    }

    #[test]
    fn rejects_misplaced_options_on_enums_and_variants() {
        let errors = messages(&parse_quote! {
            #[config(path = "a.toml", format = "toml", check = false, rename = "e", default)]
            enum E {
                #[config(tag = "t", default, unchecked, rename_all = "lowercase")]
                A,
                B(#[config(rename = "x")] u8),
                C { #[config(untagged, rename_all = "lowercase")] field: u8 },
            }
        });
        assert_eq!(
            errors,
            [
                "`path` is a struct-level option and cannot be used on the enum",
                "`format` is a struct-level option and cannot be used on the enum",
                "`check` is a struct-level option and cannot be used on the enum",
                "`rename` is a field-level option and cannot be used on the enum",
                "`default` is a field-level option and cannot be used on the enum",
                "`tag` is an enum-level option and cannot be used on a variant",
                "`default` is a field-level option and cannot be used on a variant",
                "`unchecked` is a field-level option and cannot be used on a variant",
                "`rename_all` requires a variant with named fields",
                "`config` attributes are not supported on tuple variant fields",
                "`untagged` is an enum-level option and cannot be used on a field",
                "`rename_all` is a struct-level option and cannot be used on a field",
            ]
        );
        let msg = error(&parse_quote! { #[config(tag = "t")] struct S { a: u8 } });
        assert_eq!(msg, "`tag` is an enum-level option and cannot be used on the struct");
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
    fn rename_all_converts_keys_and_rename_wins() {
        let plan = build(&parse_quote! {
            #[config(rename_all = "kebab-case")]
            struct S {
                listen_port: u16,
                r#type: String,
                #[config(rename = "MAX")]
                max_conns: u32,
            }
        })
        .unwrap();
        let keys: Vec<_> = named(&plan).iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["listen-port", "type", "MAX"]);

        let plan = build(&parse_quote! { #[config(rename_all = "camelCase")] struct S { listen_port: u16 } }).unwrap();
        assert_eq!(named(&plan)[0].key, "listenPort");
    }

    #[test]
    fn rejects_unknown_rename_all_rule() {
        let msg = error(&parse_quote! { #[config(rename_all = "Title Case")] struct S { a: u8 } });
        assert!(msg.starts_with("unknown `rename_all` rule \"Title Case\", expected one of \"lowercase\""), "{msg}");
    }

    #[test]
    fn rejects_rename_all_in_the_wrong_place() {
        let msg = error(&parse_quote! { struct S { #[config(rename_all = "camelCase")] a: u8 } });
        assert_eq!(msg, "`rename_all` is a struct-level option and cannot be used on a field");
        let msg = error(&parse_quote! { #[config(rename_all = "camelCase")] struct S(u8, u8); });
        assert_eq!(msg, "`rename_all` requires a struct with named fields");
    }

    #[test]
    fn detects_duplicate_keys_after_rename_all() {
        let errors = messages(&parse_quote! {
            #[config(rename_all = "UPPERCASE")]
            struct S {
                port: u16,
                #[config(rename = "PORT")]
                other: u16,
            }
        });
        assert_eq!(errors, ["duplicate config key `PORT`", "first used here"]);
    }

    #[test]
    fn unchecked_is_a_field_flag() {
        let plan = build(&parse_quote! { struct S { #[config(unchecked)] a: u8, b: u8 } }).unwrap();
        assert!(named(&plan)[0].unchecked);
        assert!(!named(&plan)[1].unchecked);
        let msg = error(&parse_quote! { #[config(unchecked)] struct S { a: u8 } });
        assert_eq!(msg, "`unchecked` is a field-level option and cannot be used on the struct");
        let msg = error(&parse_quote! { struct S(#[config(unchecked)] u8); });
        assert_eq!(msg, "`config` attributes are not supported on tuple struct fields");
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
        assert_eq!(error(&parse_quote! { enum G<T> { A(T) } }), "generic enums are not supported yet");
    }
}
