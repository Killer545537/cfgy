use cfgy::{ConfigError, FromConfig, FromValue, PathStack, Value};

// A hand-written impl keeps the default `SHAPE`, `Shape::Any`, so the check accepts the string and leaves the
// verdict to `from_value` at runtime.
#[derive(Debug, PartialEq)]
enum Level {
    Info,
    Debug,
}

impl FromValue for Level {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError> {
        match value {
            Value::Str(s) if s == "info" => Ok(Self::Info),
            Value::Str(s) if s == "debug" => Ok(Self::Debug),
            _ => Err(ConfigError::type_mismatch(path, "level", value)),
        }
    }
}

#[derive(FromConfig, Debug, PartialEq)]
#[config(path = "fixtures/pass/custom_from_value.toml")]
struct Logging {
    level: Level,
    fallback: Level,
}

fn main() {
    let logging = Logging::load_str(include_str!("custom_from_value.toml"), &cfgy::Toml).unwrap();
    assert_eq!(logging, Logging { level: Level::Info, fallback: Level::Debug });
}
