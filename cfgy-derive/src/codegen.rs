//! Back end of the derive: turns a [`StructPlan`] into the emitted impls.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{LitStr, Type};

use crate::plan::{FieldPlan, StructPlan};

/// Emits the `FromValue` impl for `plan`.
pub fn expand(plan: &StructPlan) -> TokenStream2 {
    let ident = &plan.ident;
    let (impl_generics, ty_generics, where_clause) = plan.generics.split_for_impl();
    let fields = plan.fields.iter().map(field_init);
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
}
