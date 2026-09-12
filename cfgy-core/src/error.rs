use std::{error::Error, fmt, io, path::PathBuf};

use crate::{PathStack, Value};

/// Everything that can go wrong between a config file on disk and a typed struct.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The file could not be read.
    Io { path: PathBuf, source: io::Error },
    /// No format could be chosen for the file.
    UnknownFormat { path: PathBuf, message: String },
    /// The source text is not well-formed in its format.
    Parse { format: &'static str, message: String, line_col: Option<(usize, usize)> },
    /// A required key is absent.
    Missing { path: String },
    /// A value has the wrong type.
    Type { path: String, expected: &'static str, found: &'static str },
    /// An integer does not fit the target type.
    OutOfRange { path: String, value: i64, target: &'static str },
    /// A sequence has the wrong length for a tuple.
    Length { path: String, expected: usize, found: usize },
}

impl ConfigError {
    #[must_use]
    pub fn missing(path: &PathStack) -> Self {
        Self::Missing { path: path.to_string() }
    }

    #[must_use]
    pub fn type_mismatch(path: &PathStack, expected: &'static str, found: &Value) -> Self {
        Self::Type { path: path.to_string(), expected, found: found.kind() }
    }

    #[must_use]
    pub fn out_of_range(path: &PathStack, value: i64, target: &'static str) -> Self {
        Self::OutOfRange { path: path.to_string(), value, target }
    }

    #[must_use]
    pub fn length(path: &PathStack, expected: usize, found: usize) -> Self {
        Self::Length { path: path.to_string(), expected, found }
    }
}

const fn at(path: &str) -> &str {
    if path.is_empty() { "<root>" } else { path }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "failed to read `{}`: {source}", path.display()),
            Self::UnknownFormat { path, message } => {
                write!(f, "cannot pick a format for `{}`: {message}", path.display())
            }
            Self::Parse { format, message, line_col: Some((line, col)) } => {
                write!(f, "invalid {format} at line {line}, column {col}: {message}")
            }
            Self::Parse { format, message, line_col: None } => write!(f, "invalid {format}: {message}"),
            Self::Missing { path } => write!(f, "missing required key `{}`", at(path)),
            Self::Type { path, expected, found: "null" } => {
                write!(f, "`{}`: expected {expected}, found null (the key is present but has no value)", at(path))
            }
            Self::Type { path, expected, found } => write!(f, "`{}`: expected {expected}, found {found}", at(path)),
            Self::OutOfRange { path, value, target } => {
                write!(f, "`{}`: {value} is out of range for {target}", at(path))
            }
            Self::Length { path, expected, found } => {
                write!(f, "`{}`: expected a sequence of {expected} items, found {found}", at(path))
            }
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        let mut path = PathStack::new();
        assert_eq!(ConfigError::missing(&path).to_string(), "missing required key `<root>`");
        path.with_key("port", |p| {
            assert_eq!(
                ConfigError::type_mismatch(p, "u16", &Value::Null).to_string(),
                "`port`: expected u16, found null (the key is present but has no value)"
            );
            assert_eq!(
                ConfigError::type_mismatch(p, "u16", &Value::Str(String::new())).to_string(),
                "`port`: expected u16, found string"
            );
            assert_eq!(ConfigError::out_of_range(p, 70000, "u16").to_string(), "`port`: 70000 is out of range for u16");
        });
    }
}
