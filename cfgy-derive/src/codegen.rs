//! Back end of the derive: turns a [`StructPlan`] into the emitted impls.

use proc_macro2::{Ident, Literal, TokenStream as TokenStream2, TokenTree};
use quote::{ToTokens, format_ident, quote};
use syn::ext::IdentExt;
use syn::{LitStr, Type};

use crate::plan::{FieldDefault, FieldPlan, SourcePlan, StructKind, StructPlan};

/// Emits the `FromValue` impl for `plan`, plus the loading methods when it has a `path`.
pub fn expand(plan: &StructPlan) -> TokenStream2 {
    let ident = &plan.ident;
    let (impl_generics, ty_generics, where_clause) = plan.generics.split_for_impl();
    let body = match &plan.kind {
        StructKind::Named(fields) => named_body(fields),
        StructKind::Tuple(types) => match types.as_slice() {
            [inner] => newtype_body(inner),
            _ => tuple_body(types),
        },
        StructKind::Unit => unit_body(),
    };
    let shape = shape(plan);
    let load = plan.source.as_ref().map(|source| load_impl(plan, source));
    quote! {
        #[automatically_derived]
        impl #impl_generics ::cfgy::FromValue for #ident #ty_generics #where_clause {
            const SHAPE: &'static ::cfgy::Shape = #shape;

            #body
        }

        #load
    }
}

/// The `FromValue::SHAPE` of the derived type, mirroring what its `from_value` accepts.
fn shape(plan: &StructPlan) -> TokenStream2 {
    let ident = &plan.ident;
    match &plan.kind {
        StructKind::Named(fields) => {
            let name = LitStr::new(&ident.unraw().to_string(), ident.span());
            let fields = fields.iter().map(|field| {
                let key = LitStr::new(&field.key, field.key_span);
                let shape = field_shape(&field.ty, field.unchecked, ident);
                let has_default = field.default.is_some();
                quote! { ::cfgy::Field { key: #key, shape: #shape, has_default: #has_default } }
            });
            quote! { &::cfgy::Shape::Table { name: #name, fields: &[#(#fields),*] } }
        }
        StructKind::Tuple(types) => {
            let mut shapes = types.iter().map(|ty| field_shape(ty, false, ident));
            match (shapes.next(), shapes.next()) {
                (Some(inner), None) => inner,
                (first, second) => {
                    let shapes = first.into_iter().chain(second).chain(shapes);
                    quote! { &::cfgy::Shape::Tuple(&[#(#shapes),*]) }
                }
            }
        }
        StructKind::Unit => quote! { &::cfgy::Shape::Null },
    }
}

/// `<ty as FromValue>::SHAPE`, or `Shape::Any` for an `unchecked` field or one whose type names the struct itself.
///
/// A shape that reaches its own type through consts is a cycle error (E0391), so recursion is cut at the field that
/// spells it. Mutual recursion through another type is not visible here; `#[config(unchecked)]` breaks that cycle.
fn field_shape(ty: &Type, unchecked: bool, ident: &Ident) -> TokenStream2 {
    if unchecked || mentions(ty.to_token_stream(), ident) {
        quote! { &::cfgy::Shape::Any }
    } else {
        quote! { <#ty as ::cfgy::FromValue>::SHAPE }
    }
}

/// Whether `tokens` contain `Self` or `ident` anywhere, including inside generic arguments and macro invocations.
fn mentions(tokens: TokenStream2, ident: &Ident) -> bool {
    tokens.into_iter().any(|tree| match tree {
        TokenTree::Ident(found) => found == "Self" || found.unraw() == ident.unraw(),
        TokenTree::Group(group) => mentions(group.stream(), ident),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

/// A struct with named fields reads a table.
fn named_body(fields: &[FieldPlan]) -> TokenStream2 {
    let fields = fields.iter().map(field_init);
    quote! {
        fn from_value(
            __cfgy_value: &::cfgy::Value,
            __cfgy_path: &mut ::cfgy::PathStack,
        ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
            let ::cfgy::Value::Map(__cfgy_map) = __cfgy_value else {
                return ::core::result::Result::Err(
                    ::cfgy::ConfigError::type_mismatch(__cfgy_path, "table", __cfgy_value),
                );
            };
            ::core::result::Result::Ok(Self { #(#fields)* })
        }
    }
}

/// A newtype is transparent: it reads, and is optional, exactly as its inner type.
fn newtype_body(inner: &Type) -> TokenStream2 {
    quote! {
        fn from_value(
            __cfgy_value: &::cfgy::Value,
            __cfgy_path: &mut ::cfgy::PathStack,
        ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
            <#inner as ::cfgy::FromValue>::from_value(__cfgy_value, __cfgy_path).map(Self)
        }

        fn from_missing() -> ::core::option::Option<Self> {
            <#inner as ::cfgy::FromValue>::from_missing().map(Self)
        }
    }
}

/// A tuple struct of any other arity reads a sequence of exactly that many items, like a tuple.
fn tuple_body(types: &[Type]) -> TokenStream2 {
    let len = Literal::usize_unsuffixed(types.len());
    let items: Vec<_> = (0..types.len()).map(|index| format_ident!("__cfgy_item{index}")).collect();
    let converts = types.iter().zip(&items).enumerate().map(|(index, (ty, item))| {
        let index = Literal::usize_unsuffixed(index);
        quote! {
            __cfgy_path.with_index(#index, |__cfgy_path| <#ty as ::cfgy::FromValue>::from_value(#item, __cfgy_path))?
        }
    });
    quote! {
        fn from_value(
            __cfgy_value: &::cfgy::Value,
            __cfgy_path: &mut ::cfgy::PathStack,
        ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
            let ::cfgy::Value::Seq(__cfgy_items) = __cfgy_value else {
                return ::core::result::Result::Err(
                    ::cfgy::ConfigError::type_mismatch(__cfgy_path, "sequence", __cfgy_value),
                );
            };
            let [#(#items),*] = __cfgy_items.as_slice() else {
                return ::core::result::Result::Err(
                    ::cfgy::ConfigError::length(__cfgy_path, #len, __cfgy_items.len()),
                );
            };
            ::core::result::Result::Ok(Self(#(#converts),*))
        }
    }
}

/// A unit struct reads only null.
fn unit_body() -> TokenStream2 {
    quote! {
        fn from_value(
            __cfgy_value: &::cfgy::Value,
            __cfgy_path: &mut ::cfgy::PathStack,
        ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
            if ::core::matches!(__cfgy_value, ::cfgy::Value::Null) {
                ::core::result::Result::Ok(Self)
            } else {
                ::core::result::Result::Err(::cfgy::ConfigError::type_mismatch(__cfgy_path, "null", __cfgy_value))
            }
        }
    }
}

/// The inherent `load`, `load_from`, and `load_str` for a struct with `#[config(path = "...")]`.
fn load_impl(plan: &StructPlan, source: &SourcePlan) -> TokenStream2 {
    let ident = &plan.ident;
    let (impl_generics, ty_generics, where_clause) = plan.generics.split_for_impl();
    let path = &source.path;
    let format = source.format.as_ref().map_or_else(
        || quote! { ::core::option::Option::None },
        |format| quote! { ::core::option::Option::Some(#format) },
    );
    let load_doc = format!(" Loads `{ident}` from `{}`, relative to the current working directory.", path.value());
    quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            #[doc = #load_doc]
            ///
            /// # Errors
            ///
            /// See [`Self::load_from`].
            pub fn load() -> ::core::result::Result<Self, ::cfgy::ConfigError> {
                Self::load_from(#path)
            }

            /// Loads the config file at `path`, choosing its format from the extension unless one was
            /// given in `#[config(format = "...")]`.
            ///
            /// # Errors
            ///
            /// Returns [`ConfigError::Io`](::cfgy::ConfigError::Io) if the file cannot be read,
            /// [`ConfigError::UnknownFormat`](::cfgy::ConfigError::UnknownFormat) if no format can be chosen,
            /// and otherwise any error from [`Self::load_str`].
            pub fn load_from(
                path: impl ::core::convert::AsRef<::std::path::Path>,
            ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
                let __cfgy_path = path.as_ref();
                let __cfgy_source = ::std::fs::read_to_string(__cfgy_path).map_err(|__cfgy_error| {
                    ::cfgy::ConfigError::Io { path: __cfgy_path.to_path_buf(), source: __cfgy_error }
                })?;
                let __cfgy_format = ::cfgy::__private::select(#format, __cfgy_path, &__cfgy_source).map_err(
                    |__cfgy_error| ::cfgy::ConfigError::UnknownFormat {
                        path: __cfgy_path.to_path_buf(),
                        message: __cfgy_error.message,
                    },
                )?;
                Self::load_str(&__cfgy_source, __cfgy_format).map_err(|__cfgy_error| __cfgy_error.in_file(__cfgy_path))
            }

            /// Parses `source` as `format` and converts it.
            ///
            /// # Errors
            ///
            /// Returns [`ConfigError::Parse`](::cfgy::ConfigError::Parse) if `source` is malformed, and a
            /// conversion error if it does not match the struct.
            pub fn load_str(
                source: &str,
                format: &dyn ::cfgy::Format,
            ) -> ::core::result::Result<Self, ::cfgy::ConfigError> {
                let __cfgy_parsed = format.parse(source).map_err(|__cfgy_error| ::cfgy::ConfigError::Parse {
                    format: format.name(),
                    location: __cfgy_error.line_col(source).map(|(line, column)| ::cfgy::Location {
                        file: ::core::option::Option::None,
                        line,
                        column,
                    }),
                    message: __cfgy_error.message,
                })?;
                <Self as ::cfgy::FromValue>::from_value(&__cfgy_parsed.value, &mut ::cfgy::PathStack::new())
                    .map_err(|__cfgy_error| __cfgy_error.locate(source, &__cfgy_parsed.spans))
            }
        }
    }
}

/// `ident: <lookup>,` for one field: a present value converts; an absent key takes the `default` if one is given,
/// else `FromValue::from_missing` (so `Option` and aliases of it are optional), else is a missing-key error.
fn field_init(field: &FieldPlan) -> TokenStream2 {
    let FieldPlan { ident, key, key_span, ty, default, .. } = field;
    let key = LitStr::new(key, *key_span);
    let absent = match default {
        Some(FieldDefault::Trait) => quote! { ::core::default::Default::default() },
        // `DefaultFrom` lets `default = "localhost"` fill a `String` while integer literals still infer the field type.
        Some(FieldDefault::Expr(expr)) => quote! { <#ty as ::cfgy::__private::DefaultFrom<_>>::default_from(#expr) },
        None => quote! {
            match <#ty as ::cfgy::FromValue>::from_missing() {
                ::core::option::Option::Some(__cfgy_missing) => __cfgy_missing,
                ::core::option::Option::None => {
                    return ::core::result::Result::Err(
                        __cfgy_path.with_key(#key, |__cfgy_path| ::cfgy::ConfigError::missing(__cfgy_path)),
                    );
                }
            }
        },
    };
    quote! {
        #ident: match __cfgy_map.get(#key) {
            ::core::option::Option::Some(__cfgy_field) => __cfgy_path.with_key(
                #key,
                |__cfgy_path| <#ty as ::cfgy::FromValue>::from_value(__cfgy_field, __cfgy_path),
            )?,
            ::core::option::Option::None => #absent,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use syn::parse_quote;

    fn expand_str(input: &syn::DeriveInput) -> String {
        expand(&plan::build(input).unwrap()).to_string()
    }

    #[test]
    fn from_value_impl_uses_absolute_paths_and_keys() {
        let out = expand_str(&parse_quote! {
            struct Settings {
                #[config(rename = "type")]
                kind: String,
                name: Option<String>,
                #[config(default = 8080)]
                port: u16,
            }
        });
        assert!(out.contains("impl :: cfgy :: FromValue for Settings"), "{out}");
        assert!(out.contains("__cfgy_map . get (\"type\")"), "{out}");
        assert!(out.contains("< String as :: cfgy :: FromValue >"), "{out}");
        assert!(out.contains("< Option < String > as :: cfgy :: FromValue > :: from_value"), "{out}");
        assert!(!out.contains("Value :: Null"), "{out}");
        assert!(out.contains("< u16 as :: cfgy :: __private :: DefaultFrom < _ >> :: default_from (8080)"), "{out}");
        // Only the fields without a default fall back to `from_missing`.
        assert_eq!(out.matches("from_missing").count(), 2, "{out}");
        assert_eq!(out.matches("ConfigError :: missing").count(), 2, "{out}");
    }

    #[test]
    fn bare_default_calls_default_trait() {
        let out = expand_str(&parse_quote! {
            struct Settings {
                #[config(default)]
                tags: Vec<String>,
            }
        });
        assert!(out.contains(":: core :: default :: Default :: default ()"), "{out}");
        assert!(!out.contains("DefaultFrom"), "{out}");
        assert!(!out.contains("ConfigError :: missing"), "{out}");
    }

    #[test]
    fn named_struct_shape_lists_its_fields() {
        let out = expand_str(&parse_quote! {
            struct Settings {
                #[config(rename = "type")]
                kind: String,
                #[config(default)]
                tags: Vec<String>,
                #[config(unchecked)]
                extra: u8,
            }
        });
        let expected = quote! {
            const SHAPE: &'static ::cfgy::Shape = &::cfgy::Shape::Table {
                name: "Settings",
                fields: &[
                    ::cfgy::Field { key: "type", shape: <String as ::cfgy::FromValue>::SHAPE, has_default: false },
                    ::cfgy::Field { key: "tags", shape: <Vec<String> as ::cfgy::FromValue>::SHAPE, has_default: true },
                    ::cfgy::Field { key: "extra", shape: &::cfgy::Shape::Any, has_default: false }
                ]
            };
        };
        assert!(out.contains(&expected.to_string()), "{out}");
    }

    #[test]
    fn self_referencing_fields_have_any_shape() {
        let out = expand_str(&parse_quote! {
            struct Node {
                children: Vec<Node>,
                parent: Option<Box<Self>>,
                weights: std::collections::HashMap<String, (u8, crate::Node)>,
                name: String,
            }
        });
        assert_eq!(out.matches("shape : & :: cfgy :: Shape :: Any").count(), 3, "{out}");
        assert!(out.contains("shape : < String as :: cfgy :: FromValue > :: SHAPE"), "{out}");
        assert!(!out.contains("< Vec < Node > as :: cfgy :: FromValue > :: SHAPE"), "{out}");

        let out = expand_str(&parse_quote! { struct Tree(Vec<Tree>); });
        assert!(out.contains("const SHAPE : & 'static :: cfgy :: Shape = & :: cfgy :: Shape :: Any ;"), "{out}");
        assert!(!mentions(quote!(Vec<Nodes>), &format_ident!("Node")));
        assert!(mentions(quote!(r#Node), &format_ident!("Node")));
    }

    #[test]
    fn tuple_newtype_and_unit_shapes() {
        let out = expand_str(&parse_quote! { struct Port(u16); });
        assert!(out.contains("const SHAPE : & 'static :: cfgy :: Shape = < u16 as :: cfgy :: FromValue > :: SHAPE ;"));
        let out = expand_str(&parse_quote! { struct Pair(String, u8); });
        let tuple = "& :: cfgy :: Shape :: Tuple (& [< String as :: cfgy :: FromValue > :: SHAPE , < u8 as :: cfgy \
                     :: FromValue > :: SHAPE])";
        assert!(out.contains(tuple), "{out}");
        let out = expand_str(&parse_quote! { struct Marker; });
        assert!(out.contains("const SHAPE : & 'static :: cfgy :: Shape = & :: cfgy :: Shape :: Null ;"), "{out}");
    }

    #[test]
    fn newtype_delegates_to_its_inner_type() {
        let out = expand_str(&parse_quote! { struct Port(u16); });
        let from_value = "< u16 as :: cfgy :: FromValue > :: from_value (__cfgy_value , __cfgy_path) . map (Self)";
        assert!(out.contains(from_value), "{out}");
        assert!(out.contains("< u16 as :: cfgy :: FromValue > :: from_missing () . map (Self)"), "{out}");
        assert!(!out.contains("Value :: Map"), "{out}");
    }

    #[test]
    fn tuple_struct_reads_an_exact_length_sequence() {
        let out = expand_str(&parse_quote! { struct Pair(String, u8); });
        assert!(out.contains("let [__cfgy_item0 , __cfgy_item1] = __cfgy_items . as_slice ()"), "{out}");
        assert!(out.contains("ConfigError :: length (__cfgy_path , 2 , __cfgy_items . len ())"), "{out}");
        assert!(out.contains("with_index (1 , | __cfgy_path | < u8 as :: cfgy :: FromValue >"), "{out}");
        assert!(!out.contains("from_missing"), "{out}");
    }

    #[test]
    fn unit_struct_reads_null() {
        let out = expand_str(&parse_quote! { struct Marker; });
        assert!(out.contains(":: cfgy :: Value :: Null"), "{out}");
        assert!(out.contains("Ok (Self)"), "{out}");
    }

    // The plan rejects `format = "yaml"` unless the feature is enabled.
    #[cfg(feature = "yaml")]
    #[test]
    fn expansion_snapshot() {
        let input = parse_quote! {
            #[config(path = "config/settings.yaml", format = "yaml")]
            struct Settings {
                #[config(rename = "type")]
                kind: String,
                server: Server,
                replicas: Vec<Server>,
                description: Option<String>,
                #[config(default = 30)]
                timeout_secs: u64,
            }
        };
        let file = syn::parse2::<syn::File>(expand(&plan::build(&input).unwrap())).unwrap();
        let actual = prettyplease::unparse(&file);
        if std::env::var("CFGY_BLESS").is_ok_and(|bless| bless == "1") {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/settings.expanded.rs");
            std::fs::write(path, actual).unwrap();
            return;
        }
        let expected = include_str!("../tests/snapshots/settings.expanded.rs");
        assert_eq!(actual, expected, "expansion changed; rerun with CFGY_BLESS=1 to update the snapshot");
    }
}
