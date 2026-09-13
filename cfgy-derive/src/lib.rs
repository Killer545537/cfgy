//! Proc-macro implementation of cfgy's `FromConfig` derive.
//!
//! This crate is an implementation detail of [cfgy](https://docs.rs/cfgy).
//! Depend on that instead.

mod check;
mod codegen;
mod plan;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, LitStr, parse_macro_input};

/// Derives `cfgy::FromValue`, plus `load`, `load_from`, and `load_str` when `#[config(path = "...")]` is set.
#[proc_macro_derive(FromConfig, attributes(config))]
pub fn derive_from_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    plan::build(&input).map_or_else(syn::Error::into_compile_error, |plan| expand(&plan)).into()
}

/// Codegen plus the compile-time file check.
///
/// Check errors are emitted alongside the generated impls rather than instead of them, so a bad config file
/// reports only its own errors and not a cascade of "no method `load`" at every call site.
fn expand(plan: &plan::StructPlan) -> TokenStream2 {
    let mut tokens = codegen::expand(plan);
    match check::check(plan) {
        // Unused, but it makes Cargo rebuild when the config file changes; without it the check validates a
        // stale file.
        Ok(Some(resolved)) => {
            let resolved = LitStr::new(&resolved.to_string_lossy(), proc_macro2::Span::call_site());
            tokens.extend(quote! { const _: &str = ::core::include_str!(#resolved); });
        }
        Ok(None) => {}
        Err(error) => tokens.extend(error.into_compile_error()),
    }
    tokens
}
