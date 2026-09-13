use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Segment {
    Key(String),
    Index(usize),
}

/// Breadcrumbs from the root table to the value being converted, rendered as `database.replicas[2].port`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PathStack(Vec<Segment>);

/// Compares against the rendered form, so `path == "database.port"` reads naturally in tests and matches.
impl PartialEq<str> for PathStack {
    fn eq(&self, other: &str) -> bool {
        /// Consumes the expected text as the path renders, failing on the first difference.
        struct Rest<'a>(&'a str);
        impl fmt::Write for Rest<'_> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.0 = self.0.strip_prefix(s).ok_or(fmt::Error)?;
                Ok(())
            }
        }
        let mut rest = Rest(other);
        fmt::write(&mut rest, format_args!("{self}")).is_ok() && rest.0.is_empty()
    }
}

impl PathStack {
    #[must_use]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Runs `f` with `key` pushed, popping afterwards so callers cannot leave the stack unbalanced.
    pub fn with_key<T>(&mut self, key: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scoped(Segment::Key(key.to_owned()), f)
    }

    /// Runs `f` with `index` pushed, popping afterwards.
    pub fn with_index<T>(&mut self, index: usize, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scoped(Segment::Index(index), f)
    }

    fn scoped<T>(&mut self, segment: Segment, f: impl FnOnce(&mut Self) -> T) -> T {
        self.0.push(segment);
        let out = f(self);
        self.0.pop();
        out
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.0
    }
}

impl fmt::Display for PathStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.0.iter().enumerate() {
            match segment {
                Segment::Key(key) if i == 0 => f.write_str(key)?,
                Segment::Key(key) => write!(f, ".{key}")?,
                Segment::Index(index) => write!(f, "[{index}]")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_nested_path() {
        let mut path = PathStack::new();
        let rendered = path.with_key("database", |p| {
            p.with_key("replicas", |p| p.with_index(2, |p| p.with_key("port", |p| p.to_string())))
        });
        assert_eq!(rendered, "database.replicas[2].port");
        assert_eq!(path, PathStack::new());
        path.with_key("a", |p| {
            p.with_index(1, |p| {
                assert_eq!(*p, *"a[1]");
                assert_ne!(*p, *"a[1].b");
                assert_ne!(*p, *"a");
            });
        });
    }

    #[test]
    fn renders_root_index_and_empty() {
        let mut path = PathStack::new();
        assert_eq!(path.to_string(), "");
        assert_eq!(path.with_index(0, |p| p.with_key("a", |p| p.to_string())), "[0].a");
    }
}
