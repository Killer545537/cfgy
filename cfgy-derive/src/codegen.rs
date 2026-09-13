//! Back end of the derive: turns a [`StructPlan`] into the emitted impls.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{LitStr, Type};

use crate::plan::{FieldPlan, SourcePlan, StructPlan};

/// Emits the `FromValue` impl for `plan`, plus the loading methods when it has a `path`.
pub fn expand(plan: &StructPlan) -> TokenStream2 {
    let ident = &plan.ident;
    let (impl_generics, ty_generics, where_clause) = plan.generics.split_for_impl();
    let fields = plan.fields.iter().map(field_init);
    let load = plan.source.as_ref().map(|source| load_impl(plan, source));
    quote! {
        #[automatically_derived]
        impl #impl_generics ::cfgy::FromValue for #ident #ty_generics #where_clause {
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

        #load
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
                Self::load_str(&__cfgy_source, __cfgy_format)
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
                let __cfgy_value = format.parse(source).map_err(|__cfgy_error| ::cfgy::ConfigError::Parse {
                    format: format.name(),
                    line_col: __cfgy_error.line_col(source),
                    message: __cfgy_error.message,
                })?;
                <Self as ::cfgy::FromValue>::from_value(&__cfgy_value, &mut ::cfgy::PathStack::new())
            }
        }
    }
}

/// `ident: <lookup>,` for one field, following the design's absent/optional/default/required order.
fn field_init(field: &FieldPlan) -> TokenStream2 {
    let FieldPlan { ident, key, key_span, ty, optional, default, .. } = field;
    let key = LitStr::new(key, *key_span);
    let convert = |ty: &Type| {
        quote! {
            __cfgy_path.with_key(#key, |__cfgy_path| <#ty as ::cfgy::FromValue>::from_value(__cfgy_field, __cfgy_path))?
        }
    };
    let present = optional.as_ref().map_or_else(
        || {
            let convert = convert(ty);
            quote! { ::core::option::Option::Some(__cfgy_field) => #convert, }
        },
        |inner| {
            let convert = convert(inner);
            quote! {
                ::core::option::Option::Some(::cfgy::Value::Null) => ::core::option::Option::None,
                ::core::option::Option::Some(__cfgy_field) => ::core::option::Option::Some(#convert),
            }
        },
    );
    // A default wins over `None` so `#[config(default = ...)]` on an `Option` field is not silently ignored.
    let absent = match default {
        Some(default) => quote! { { #default } },
        None if field.required() => quote! {
            return ::core::result::Result::Err(
                __cfgy_path.with_key(#key, |__cfgy_path| ::cfgy::ConfigError::missing(__cfgy_path)),
            )
        },
        None => quote! { ::core::option::Option::None },
    };
    quote! {
        #ident: match __cfgy_map.get(#key) {
            #present
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
        assert!(out.contains(":: cfgy :: Value :: Null"), "{out}");
        assert!(out.contains("{ 8080 }"), "{out}");
        assert_eq!(out.matches("ConfigError :: missing").count(), 1, "{out}");
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
