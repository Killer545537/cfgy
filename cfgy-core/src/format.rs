use crate::Value;

/// A strategy that lowers source text into a [`Value`].
pub trait Format {
    fn name(&self) -> &'static str;

    /// # Errors
    ///
    /// Returns [`ParseError`] when `source` is not well-formed in this format.
    fn parse(&self, source: &str) -> Result<Value, ParseError>;
}

/// A parse failure, with the byte offset into the source when the parser reports one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub offset: Option<usize>,
}

impl ParseError {
    /// 1-based `(line, column)` of [`Self::offset`] in `source`, with the column counted in chars.
    ///
    /// `None` when there is no offset or it does not land on a char boundary inside `source`.
    #[must_use]
    pub fn line_col(&self, source: &str) -> Option<(usize, usize)> {
        let before = source.get(..self.offset?)?;
        let line = before.matches('\n').count().saturating_add(1);
        let last_line = before.rsplit('\n').next().unwrap_or_default();
        Some((line, last_line.chars().count().saturating_add(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(offset: usize, source: &str) -> Option<(usize, usize)> {
        ParseError { message: String::new(), offset: Some(offset) }.line_col(source)
    }

    #[test]
    fn line_col() {
        assert_eq!(at(0, "a = 1"), Some((1, 1)));
        assert_eq!(at(4, "a = 1"), Some((1, 5)));
        assert_eq!(at(6, "a = 1\nb = x"), Some((2, 1)));
        assert_eq!(at(10, "a = 1\nb = x"), Some((2, 5)));
        assert_eq!(at(5, "a = 1"), Some((1, 6)));
        assert_eq!(at(6, "a = 1"), None);
        assert_eq!(ParseError { message: String::new(), offset: None }.line_col("a"), None);
    }

    #[test]
    fn line_col_multibyte() {
        // "é" is two bytes but one column.
        assert_eq!(at(3, "é x"), Some((1, 3)));
        assert_eq!(at(1, "é x"), None);
    }
}
