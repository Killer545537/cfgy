//! Feature-gated TOML, JSON, and YAML adapters that lower source text into the cfgy value IR.
//!
//! You probably want the [cfgy](https://docs.rs/cfgy) crate instead.

use std::{error::Error, fmt, path::Path};

use cfgy_core::Format;

/// No format could be chosen for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectError {
    pub message: String,
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for SelectError {}

/// Picks the format for `path`: the `explicit` name, else the extension, else (only when `path` has no
/// extension) a sniff of the first non-whitespace bytes of `source`.
///
/// # Errors
///
/// Returns [`SelectError`] when the name or extension is unknown, the content cannot be sniffed, or the
/// chosen format's cargo feature is disabled.
pub fn select(explicit: Option<&str>, path: &Path, source: &str) -> Result<&'static dyn Format, SelectError> {
    enabled(feature_name(explicit, path, source)?)
}

fn feature_name(explicit: Option<&str>, path: &Path, source: &str) -> Result<&'static str, SelectError> {
    if let Some(name) = explicit {
        return by_name(name).ok_or_else(|| error(format!("unknown format `{name}`, expected toml, json, or yaml")));
    }
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy();
        return by_name(&ext)
            .ok_or_else(|| error(format!("unknown extension `.{ext}`, expected .toml, .json, .yaml, or .yml")));
    }
    let trimmed = source.trim_start();
    if trimmed.starts_with('{') {
        Ok("json")
    } else if trimmed.starts_with("---") {
        Ok("yaml")
    } else {
        Err(error("no file extension and the content does not look like JSON or YAML".to_owned()))
    }
}

fn by_name(name: &str) -> Option<&'static str> {
    match name {
        "toml" => Some("toml"),
        "json" => Some("json"),
        "yaml" | "yml" => Some("yaml"),
        _ => None,
    }
}

fn enabled(feature: &'static str) -> Result<&'static dyn Format, SelectError> {
    Err(error(format!("the {feature} format is disabled, enable the `{feature}` cargo feature")))
}

const fn error(message: String) -> SelectError {
    SelectError { message }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(explicit: Option<&str>, path: &str, source: &str) -> Result<&'static str, String> {
        feature_name(explicit, Path::new(path), source).map_err(|e| e.message)
    }

    #[test]
    fn precedence() {
        assert_eq!(name(Some("yml"), "app.toml", ""), Ok("yaml"));
        assert_eq!(name(None, "app.toml", "{"), Ok("toml"));
        assert_eq!(
            name(None, "dir.d/app.JSON", ""),
            Err("unknown extension `.JSON`, expected .toml, .json, .yaml, or .yml".to_owned())
        );
    }

    #[test]
    fn unknown_is_an_error() {
        assert!(name(Some("ini"), "app.toml", "").unwrap_err().contains("unknown format `ini`"));
        assert!(name(None, "app.ini", "{").unwrap_err().contains("unknown extension `.ini`"));
    }

    #[test]
    fn sniffs_only_without_extension() {
        assert_eq!(name(None, "config", "  \n{\"a\": 1}"), Ok("json"));
        assert_eq!(name(None, "config", "\n---\na: 1"), Ok("yaml"));
        assert!(name(None, "config", "a = 1").unwrap_err().contains("does not look like"));
    }

    #[test]
    #[cfg(not(feature = "yaml"))]
    fn disabled_feature_is_named() {
        let err = select(Some("yml"), Path::new("app"), "").map(Format::name).unwrap_err();
        assert_eq!(err.to_string(), "the yaml format is disabled, enable the `yaml` cargo feature");
    }
}
