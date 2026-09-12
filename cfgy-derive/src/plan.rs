//! Front end of the derive: turns a [`DeriveInput`] into a [`StructPlan`].

use proc_macro2::Span;
use syn::{Data, DeriveInput, Error, Expr, Fields, Generics, Ident, LitStr, Type};

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

    let fields = fields
        .iter()
        .filter_map(|field| {
            let ident = field.ident.clone()?;
            Some(FieldPlan {
                key: ident.to_string(),
                key_span: ident.span(),
                ident,
                ty: field.ty.clone(),
                optional: None,
                default: None,
            })
        })
        .collect();

    Ok(StructPlan { ident: input.ident.clone(), generics: input.generics.clone(), source: None, fields })
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

    #[test]
    fn rejects_generics() {
        let msg = "generic structs are not supported yet";
        assert_eq!(error(&parse_quote! { struct G<T> { t: T } }), msg);
        assert_eq!(error(&parse_quote! { struct L<'a> { s: &'a str } }), msg);
    }
}
