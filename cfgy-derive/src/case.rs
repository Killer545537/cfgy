//! serde's `rename_all` case conversions, applied to field and variant names.

/// One `rename_all` rule, converting exactly as serde's `RenameRule` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

/// Every rule with the name it is written as, in serde's order.
const RULES: [(&str, RenameRule); 8] = [
    ("lowercase", RenameRule::Lower),
    ("UPPERCASE", RenameRule::Upper),
    ("PascalCase", RenameRule::Pascal),
    ("camelCase", RenameRule::Camel),
    ("snake_case", RenameRule::Snake),
    ("SCREAMING_SNAKE_CASE", RenameRule::ScreamingSnake),
    ("kebab-case", RenameRule::Kebab),
    ("SCREAMING-KEBAB-CASE", RenameRule::ScreamingKebab),
];

impl RenameRule {
    /// Parses a rule name, or returns the error message listing the valid names.
    pub fn from_name(name: &str) -> Result<Self, String> {
        RULES.iter().find(|(rule_name, _)| *rule_name == name).map(|(_, rule)| *rule).ok_or_else(|| {
            let names: Vec<String> = RULES.iter().map(|(rule_name, _)| format!("\"{rule_name}\"")).collect();
            format!("unknown `rename_all` rule \"{name}\", expected one of {}", names.join(", "))
        })
    }

    /// Converts a `snake_case` field name.
    pub fn apply_to_field(self, field: &str) -> String {
        match self {
            Self::Lower | Self::Snake => field.to_owned(),
            Self::Upper | Self::ScreamingSnake => field.to_ascii_uppercase(),
            Self::Pascal => {
                let mut pascal = String::with_capacity(field.len());
                let mut capitalize = true;
                for ch in field.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        pascal.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        pascal.push(ch);
                    }
                }
                pascal
            }
            Self::Camel => lowercase_first(&Self::Pascal.apply_to_field(field)),
            Self::Kebab => field.replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake.apply_to_field(field).replace('_', "-"),
        }
    }

    /// Converts a `PascalCase` variant name.
    // `allow`, not `expect`: rustc 1.85 does not fulfil a `dead_code` expectation here.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once enums derive `FromConfig`"))]
    pub fn apply_to_variant(self, variant: &str) -> String {
        match self {
            Self::Pascal => variant.to_owned(),
            Self::Lower => variant.to_ascii_lowercase(),
            Self::Upper => variant.to_ascii_uppercase(),
            Self::Camel => lowercase_first(variant),
            Self::Snake => {
                let mut snake = String::with_capacity(variant.len());
                for (index, ch) in variant.char_indices() {
                    if index > 0 && ch.is_uppercase() {
                        snake.push('_');
                    }
                    snake.push(ch.to_ascii_lowercase());
                }
                snake
            }
            Self::ScreamingSnake => Self::Snake.apply_to_variant(variant).to_ascii_uppercase(),
            Self::Kebab => Self::Snake.apply_to_variant(variant).replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake.apply_to_variant(variant).replace('_', "-"),
        }
    }
}

/// `name` with its first character ASCII-lowercased.
fn lowercase_first(name: &str) -> String {
    let mut chars = name.chars();
    let mut lowered = String::with_capacity(name.len());
    if let Some(first) = chars.next() {
        lowered.push(first.to_ascii_lowercase());
    }
    lowered.push_str(chars.as_str());
    lowered
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [RenameRule; 8] = [
        RenameRule::Lower,
        RenameRule::Upper,
        RenameRule::Pascal,
        RenameRule::Camel,
        RenameRule::Snake,
        RenameRule::ScreamingSnake,
        RenameRule::Kebab,
        RenameRule::ScreamingKebab,
    ];

    #[test]
    fn parses_every_serde_name() {
        for (name, rule) in RULES {
            assert_eq!(RenameRule::from_name(name), Ok(rule));
        }
        let err = RenameRule::from_name("Title Case").unwrap_err();
        assert_eq!(
            err,
            "unknown `rename_all` rule \"Title Case\", expected one of \"lowercase\", \"UPPERCASE\", \"PascalCase\", \
             \"camelCase\", \"snake_case\", \"SCREAMING_SNAKE_CASE\", \"kebab-case\", \"SCREAMING-KEBAB-CASE\""
        );
    }

    // The expected columns are serde's own `rename_fields` and `rename_variants` test tables.
    #[test]
    fn fields() {
        let cases = [
            ("outcome", ["outcome", "OUTCOME", "Outcome", "outcome", "outcome", "OUTCOME", "outcome", "OUTCOME"]),
            (
                "very_tasty",
                [
                    "very_tasty",
                    "VERY_TASTY",
                    "VeryTasty",
                    "veryTasty",
                    "very_tasty",
                    "VERY_TASTY",
                    "very-tasty",
                    "VERY-TASTY",
                ],
            ),
            ("a", ["a", "A", "A", "a", "a", "A", "a", "A"]),
            ("z42", ["z42", "Z42", "Z42", "z42", "z42", "Z42", "z42", "Z42"]),
        ];
        for (field, expected) in cases {
            for (rule, expected) in ALL.into_iter().zip(expected) {
                assert_eq!(rule.apply_to_field(field), expected, "{rule:?} on {field}");
            }
        }
    }

    #[test]
    fn variants() {
        let cases = [
            ("Outcome", ["outcome", "OUTCOME", "Outcome", "outcome", "outcome", "OUTCOME", "outcome", "OUTCOME"]),
            (
                "VeryTasty",
                [
                    "verytasty",
                    "VERYTASTY",
                    "VeryTasty",
                    "veryTasty",
                    "very_tasty",
                    "VERY_TASTY",
                    "very-tasty",
                    "VERY-TASTY",
                ],
            ),
            ("A", ["a", "A", "A", "a", "a", "A", "a", "A"]),
            ("Z42", ["z42", "Z42", "Z42", "z42", "z42", "Z42", "z42", "Z42"]),
        ];
        for (variant, expected) in cases {
            for (rule, expected) in ALL.into_iter().zip(expected) {
                assert_eq!(rule.apply_to_variant(variant), expected, "{rule:?} on {variant}");
            }
        }
    }
}
