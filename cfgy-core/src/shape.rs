//! Compile-time description of what a [`FromValue`](crate::FromValue) type accepts.
//!
//! The derive checks a config file against `<Root as FromValue>::SHAPE` in const evaluation, so the check follows
//! real types (aliases, nested structs, custom impls) instead of type names.

/// What a type accepts, as a const-constructible tree.
#[derive(Debug)]
pub enum Shape {
    /// Anything. The default for impls that do not describe themselves.
    Any,
    /// Only null: unit structs.
    Null,
    Bool,
    /// Strings and datetimes.
    String,
    /// Strings only.
    Path,
    /// One-character strings.
    Char,
    /// Datetimes, or strings that are valid datetimes.
    Datetime,
    Int {
        min: i128,
        max: i128,
        name: &'static str,
    },
    /// Floats, and integers (widened). `max` bounds finite values, so `f32` rejects `1e300`.
    Float {
        max: f64,
        name: &'static str,
    },
    /// `Option<T>`: null, an absent key, or a `T`.
    Optional(&'static Self),
    /// `Vec<T>`, `VecDeque<T>`, `HashSet<T>`.
    Seq(&'static Self),
    /// Tuples and tuple structs: a sequence of exactly these element shapes.
    Tuple(&'static [&'static Self]),
    /// `HashMap<String, T>` and friends: a table whose values are `T`.
    Map(&'static Self),
    /// A struct with named fields.
    Table {
        name: &'static str,
        fields: &'static [Field],
    },
    /// An enum in one of serde's four representations.
    Enum {
        name: &'static str,
        repr: Repr,
        variants: &'static [Variant],
    },
}

/// One named field of a [`Shape::Table`].
#[derive(Debug)]
pub struct Field {
    pub key: &'static str,
    pub shape: &'static Shape,
    /// Whether an absent key falls back to a `default` instead of being an error.
    pub has_default: bool,
}

/// How an enum's variant is written in the file, mirroring serde's enum representations.
#[derive(Debug, Clone, Copy)]
pub enum Repr {
    /// `"Unit"` or `{ Variant = payload }`.
    External,
    /// `{ tag = "Variant", ...fields }`.
    Internal { tag: &'static str },
    /// `{ tag = "Variant", content = payload }`.
    Adjacent { tag: &'static str, content: &'static str },
    /// Whichever variant's payload matches.
    Untagged,
}

/// One variant of a [`Shape::Enum`].
#[derive(Debug)]
pub struct Variant {
    pub name: &'static str,
    /// `None` for a unit variant.
    pub payload: Option<&'static Shape>,
}
