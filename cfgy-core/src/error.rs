use std::{
    error::Error,
    fmt, io,
    path::{Path, PathBuf},
};

use crate::{PathStack, Spans, Value, format::line_col};

/// Where in which file an error happened. `file` is `None` for sources that did not come from a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Location {
    pub file: Option<PathBuf>,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{}:", file.display())?;
        }
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Everything that can go wrong between a config file on disk and a typed struct.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The file could not be read.
    Io { path: PathBuf, source: io::Error },
    /// No format could be chosen for the file.
    UnknownFormat { path: PathBuf, message: String },
    /// The source text is not well-formed in its format.
    Parse { format: &'static str, message: String, location: Option<Location> },
    /// A required key is absent. `location` is the table it is missing from.
    Missing { path: PathStack, location: Option<Location> },
    /// A value has the wrong type.
    Type { path: PathStack, expected: &'static str, found: &'static str, location: Option<Location> },
    /// An integer does not fit the target type.
    OutOfRange { path: PathStack, value: i128, target: &'static str, location: Option<Location> },
    /// A sequence has the wrong length for a tuple.
    Length { path: PathStack, expected: usize, found: usize, location: Option<Location> },
    /// A tag names no variant of the enum `name`. `expected` lists the variant names, as `` `a`, `b` ``.
    ///
    /// `found` is boxed to keep `ConfigError` within clippy's `result_large_err` limit.
    UnknownVariant {
        path: PathStack,
        name: &'static str,
        found: Box<str>,
        expected: &'static str,
        location: Option<Location>,
    },
}

impl ConfigError {
    #[must_use]
    pub fn missing(path: &PathStack) -> Self {
        Self::Missing { path: path.clone(), location: None }
    }

    #[must_use]
    pub fn type_mismatch(path: &PathStack, expected: &'static str, found: &Value) -> Self {
        Self::Type { path: path.clone(), expected, found: found.kind(), location: None }
    }

    #[must_use]
    pub fn out_of_range(path: &PathStack, value: i128, target: &'static str) -> Self {
        Self::OutOfRange { path: path.clone(), value, target, location: None }
    }

    #[must_use]
    pub fn length(path: &PathStack, expected: usize, found: usize) -> Self {
        Self::Length { path: path.clone(), expected, found, location: None }
    }

    /// `found` names no variant of the enum `name`, whose variants are listed in `expected`.
    #[must_use]
    pub fn unknown_variant(path: &PathStack, name: &'static str, found: &str, expected: &'static str) -> Self {
        Self::UnknownVariant { path: path.clone(), name, found: found.into(), expected, location: None }
    }

    /// Fills in the line and column of a conversion error from the `spans` of the `source` it was parsed from.
    ///
    /// A missing key points at the table it should be in. Errors whose path has no span are left unlocated.
    #[must_use]
    pub fn locate(mut self, source: &str, spans: &Spans) -> Self {
        let (path, missing) = match &self {
            Self::Missing { path, .. } => (path, true),
            Self::Type { path, .. }
            | Self::OutOfRange { path, .. }
            | Self::Length { path, .. }
            | Self::UnknownVariant { path, .. } => (path, false),
            _ => return self,
        };
        let segments = path.segments();
        let segments = if missing { segments.split_last().map_or(segments, |(_, parent)| parent) } else { segments };
        let found = spans.get(segments).and_then(|range| line_col(source, range.start));
        if let (Some((line, column)), Some(location)) = (found, self.location_mut()) {
            *location = Some(Location { file: None, line, column });
        }
        self
    }

    /// Names the file a located error came from, so it renders as `file:line:col`.
    #[must_use]
    pub fn in_file(mut self, file: &Path) -> Self {
        if let Some(Some(location)) = self.location_mut() {
            location.file = Some(file.to_path_buf());
        }
        self
    }

    const fn location_mut(&mut self) -> Option<&mut Option<Location>> {
        match self {
            Self::Parse { location, .. }
            | Self::Missing { location, .. }
            | Self::Type { location, .. }
            | Self::OutOfRange { location, .. }
            | Self::Length { location, .. }
            | Self::UnknownVariant { location, .. } => Some(location),
            Self::Io { .. } | Self::UnknownFormat { .. } => None,
        }
    }

    const fn location(&self) -> Option<&Location> {
        match self {
            Self::Parse { location, .. }
            | Self::Missing { location, .. }
            | Self::Type { location, .. }
            | Self::OutOfRange { location, .. }
            | Self::Length { location, .. }
            | Self::UnknownVariant { location, .. } => location.as_ref(),
            Self::Io { .. } | Self::UnknownFormat { .. } => None,
        }
    }
}

/// Renders a path, naming the root explicitly.
struct At<'a>(&'a PathStack);

impl fmt::Display for At<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.segments().is_empty() { f.write_str("<root>") } else { self.0.fmt(f) }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(location) = self.location() {
            write!(f, "{location}: ")?;
        }
        match self {
            Self::Io { path, source } => write!(f, "failed to read `{}`: {source}", path.display()),
            Self::UnknownFormat { path, message } => {
                write!(f, "cannot pick a format for `{}`: {message}", path.display())
            }
            Self::Parse { format, message, .. } => write!(f, "invalid {format}: {message}"),
            Self::Missing { path, .. } => write!(f, "missing required key `{}`", At(path)),
            Self::Type { path, expected, found: "null", .. } => {
                write!(f, "`{}`: expected {expected}, found null (the key is present but has no value)", At(path))
            }
            Self::Type { path, expected, found, .. } => write!(f, "`{}`: expected {expected}, found {found}", At(path)),
            Self::OutOfRange { path, value, target, .. } => {
                write!(f, "`{}`: {value} is out of range for {target}", At(path))
            }
            Self::Length { path, expected, found, .. } => {
                write!(f, "`{}`: expected a sequence of {expected} items, found {found}", At(path))
            }
            Self::UnknownVariant { path, name, found, expected, .. } => {
                write!(f, "`{}`: unknown variant `{found}` of {name}, expected one of {expected}", At(path))
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
        path.with_key("mode", |p| {
            assert_eq!(
                ConfigError::unknown_variant(p, "Mode", "turbo", "`fast`, `slow`").to_string(),
                "`mode`: unknown variant `turbo` of Mode, expected one of `fast`, `slow`"
            );
        });
    }

    #[test]
    fn locate_points_an_unknown_variant_at_its_value() {
        let source = "mode = \"turbo\"\n";
        let mut spans = Spans::default();
        let mut path = PathStack::new();
        path.with_key("mode", |p| spans.insert(p, 7..14));
        let err = path.with_key("mode", |p| ConfigError::unknown_variant(p, "Mode", "turbo", "`fast`, `slow`"));
        let err = err.locate(source, &spans).in_file(Path::new("config.toml"));
        assert_eq!(
            err.to_string(),
            "config.toml:1:8: `mode`: unknown variant `turbo` of Mode, expected one of `fast`, `slow`"
        );
    }

    #[test]
    fn locate_uses_the_value_span_or_the_parent_table_for_missing_keys() {
        let source = "[server]\nport = \"x\"\n";
        let mut spans = Spans::default();
        let mut path = PathStack::new();
        path.with_key("server", |p| {
            spans.insert(p, 0..8);
            p.with_key("port", |p| spans.insert(p, 16..19));
        });

        let err =
            path.with_key("server", |p| p.with_key("port", |p| ConfigError::type_mismatch(p, "u16", &Value::Null)));
        let err = err.locate(source, &spans).in_file(Path::new("config.toml"));
        assert_eq!(
            err.to_string(),
            "config.toml:2:8: `server.port`: expected u16, found null (the key is present but has no value)"
        );

        let err = path.with_key("server", |p| p.with_key("host", |p| ConfigError::missing(p))).locate(source, &spans);
        assert_eq!(err.to_string(), "1:1: missing required key `server.host`");

        let err = path.with_key("other", |p| ConfigError::missing(p)).locate(source, &spans);
        assert_eq!(err.to_string(), "missing required key `other`");
    }
}
