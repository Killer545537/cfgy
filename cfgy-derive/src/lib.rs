//! Proc-macro implementation of cfgy's `FromConfig` derive.
//!
//! This crate is an implementation detail of [cfgy](https://docs.rs/cfgy).
//! Depend on that instead.

mod codegen;
// `SourcePlan::check` is unused until the compile-time check is wired in.
#[allow(dead_code)]
mod plan;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Derives `cfgy::FromValue`, plus `load`, `load_from`, and `load_str` when `#[config(path = "...")]` is set.
#[proc_macro_derive(FromConfig, attributes(config))]
pub fn derive_from_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    plan::build(&input).map_or_else(syn::Error::into_compile_error, |plan| codegen::expand(&plan)).into()
}
