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

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
    t.compile_fail("tests/ui/fail/*.rs");
}
