//! trybuild compile-pass and compile-fail cases for `#[derive(FromConfig)]`.
//!
//! # Fixture paths
//!
//! The derive resolves `#[config(path = "...")]` against `CARGO_MANIFEST_DIR` at expansion time. trybuild does not
//! compile the cases inside this crate: it generates a project at `<target-dir>/tests/trybuild/cfgy/` and builds
//! each case as a bin of that project, so `CARGO_MANIFEST_DIR` is that generated directory, not `cfgy/`.
//!
//! Before running the cases, this test copies every `*.toml`, `*.json` and `*.yaml` under `tests/ui/` into
//! `<target-dir>/tests/trybuild/cfgy/fixtures/`, keeping its subdirectory, and cases reference them as
//! `path = "fixtures/<dir>/<file>"`. `<target-dir>` is the parent of `CARGO_TARGET_TMPDIR`, so this works with a
//! custom `CARGO_TARGET_DIR` too. trybuild never cleans its project directory, so the copies survive until the
//! cases are built.
//!
//! Runtime reads in pass cases use `include_str!`, which is relative to the case's source file and so unaffected.
//!
//! # Gated directories
//!
//! trybuild builds the cases with the features this test was built with, so the default run only has TOML.
//!
//! - `fail-json/`, `fail-yaml/`: need their format feature; with it off the file would fail for a different reason.

use std::path::Path;
use std::{fs, io};

/// Extensions of the config files copied into the trybuild project.
const FIXTURE_EXTENSIONS: [&str; 3] = ["toml", "json", "yaml"];

/// Copies every fixture file under `src` into `dst`, preserving relative subdirectories.
fn copy_fixtures(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let path = entry?.path();
        let target = dst.join(path.file_name().unwrap_or_default());
        if path.is_dir() {
            copy_fixtures(&path, &target)?;
        } else if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| FIXTURE_EXTENSIONS.contains(&ext)) {
            fs::create_dir_all(dst)?;
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

#[test]
fn ui() {
    let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("CARGO_TARGET_TMPDIR has a parent");
    let fixtures = target_dir.join("tests/trybuild/cfgy/fixtures");
    copy_fixtures(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui"), &fixtures)
        .expect("copy ui fixtures into the trybuild project");

    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
    t.compile_fail("tests/ui/fail/*.rs");
    if cfg!(feature = "json") {
        t.compile_fail("tests/ui/fail-json/*.rs");
    }
    if cfg!(feature = "yaml") {
        t.compile_fail("tests/ui/fail-yaml/*.rs");
    }
}
