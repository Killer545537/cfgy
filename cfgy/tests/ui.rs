//! trybuild compile-pass and compile-fail cases for `#[derive(FromConfig)]`.
//!
//! # Fixture paths
//!
//! The derive resolves `#[config(path = "...")]` against `CARGO_MANIFEST_DIR` at expansion time. trybuild does not
//! compile the cases inside this crate: it generates a project at `<target-dir>/tests/trybuild/cfgy/` and builds
//! each case as a bin of that project, so `CARGO_MANIFEST_DIR` is that generated directory, not `cfgy/`.
//!
//! Cases therefore reference their fixture files relative to it, climbing out of the target dir and back into this
//! crate: `path = "../../../../cfgy/tests/ui/<dir>/<file>"`. This holds for the default target dir
//! (`<workspace>/target`), which is what local runs and CI use. A custom `CARGO_TARGET_DIR` breaks the checked
//! cases (they fail with "config file ... not found"), because a path literal cannot be made relative to anything
//! else without a library change.
//!
//! Runtime reads in pass cases use `include_str!`, which is relative to the case's source file and so unaffected.
//!
//! # Gated directories
//!
//! trybuild builds the cases with the features this test was built with, so the default run only has TOML.
//!
//! - `fail-json/`, `fail-yaml/`: need their format feature; with it off the file would fail for a different reason.
//! - `fail-unix/`: the missing-file message embeds the OS error text (`No such file or directory (os error 2)`),
//!   which reads differently on Windows.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
    t.compile_fail("tests/ui/fail/*.rs");
    if cfg!(unix) {
        t.compile_fail("tests/ui/fail-unix/*.rs");
    }
    if cfg!(feature = "json") {
        t.compile_fail("tests/ui/fail-json/*.rs");
    }
    if cfg!(feature = "yaml") {
        t.compile_fail("tests/ui/fail-yaml/*.rs");
    }
}
