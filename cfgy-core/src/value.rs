use indexmap::IndexMap;

/// A datetime as written in the source file.
///
/// ponytail: kept as the original text so core has no date dependency; parse it with a date crate if you need arithmetic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Datetime(pub String);

/// The canonical model every format lowers into.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Self>),
    Map(IndexMap<String, Self>),
    Datetime(Datetime),
}

impl Value {
    /// Human-readable name of the variant, for error messages.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "integer",
            Self::Float(_) => "float",
            Self::Str(_) => "string",
            Self::Seq(_) => "sequence",
            Self::Map(_) => "table",
            Self::Datetime(_) => "datetime",
        }
    }
}
