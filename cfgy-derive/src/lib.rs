//! Proc-macro implementation of cfgy's `FromConfig` derive.
//!
//! This crate is an implementation detail of [cfgy](https://docs.rs/cfgy).
//! Depend on that instead.

// Nothing calls into `plan` until the derive entry point lands in wave 2.
#[allow(dead_code)]
mod plan;
