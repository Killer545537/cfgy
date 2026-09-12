# Scope

A derive macro `FromConfig` that maps a config file (TOML, JSON, YAML, extensible) onto a Rust struct, Serde-style, with the file's existence and well-formedness checked at compile time and the actual parse happening at runtime.

Fixed decisions from design:

- Nested types derive `FromConfig` themselves. No module-level attribute macro.
- Unknown keys in the file are ignored.
- Maps, sequences, nested structs, and `Option` are all supported targets.
- `#[config(path = ...)]` is what grants a struct `load()`. A struct without it has no loading API at all.
- `#[config(default = <expr>)]` takes an arbitrary expression.

> [!important]
> The compile-time check runs on the build machine against the build-machine filesystem. It is a fast feedback loop for the developer, not a guarantee about the deployment environment. Runtime `load()` must handle every failure independently.

# Crate layout

```
cfgy-core     Value IR, FromValue, ConfigError, Format trait      (no format deps)
cfgy-formats  TOML / JSON / YAML adapters, feature-gated          (depends on core)
cfgy-derive   proc-macro crate: plan + codegen                    (core + formats)
cfgy          facade: re-exports core + the derive                (what users depend on)
```

A `proc-macro = true` crate can export only macros, which forces the split. `cfgy-derive` depends on `cfgy-formats` because it parses the file during expansion; `cfgy` depends on both because the runtime `load()` needs to parse again.

The facade exists so a user writes one dependency and `use cfgy::FromConfig;` resolves both the trait and the derive.

# Value IR

The canonical model every format lowers into. Without it the crate is an $N times M$ matrix of (format $times$ target type); with it, $N$ adapters and one conversion layer.

```rust title="Value"
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Value>),
    Map(IndexMap<String, Value>),
    Datetime(Datetime),
}
```

Decisions baked into this shape:

- `Int` and `Float` are separate. TOML and YAML distinguish them natively. JSON does not, so the JSON adapter emits `Int` when the literal has no fraction and no exponent. Without this every JSON `port: 8080` arrives as a float.
- `Map` is insertion-ordered (`IndexMap`), so error messages and any future round-tripping are deterministic.
- `Datetime` is a real variant rather than a lowered string. Lowering to `Str` means a TOML datetime silently changes type on the way through JSON.
- `Null` exists even though TOML has no null. In TOML, absence is the only way to express it.

Every `Value` carries no span. If you later want "line 14 of config.toml" in a conversion error, that must be added here from the start — retrofitting spans into the IR touches every adapter.

# Format layer

> [!definition]
> A **format** is a strategy: source text in, `Value` out, or a diagnostic carrying a byte offset.

```rust title="Format"
pub trait Format {
    fn name(&self) -> &'static str;
    fn parse(&self, source: &str) -> Result<Value, ParseError>;
}

pub struct ParseError {
    pub message: String,
    pub offset: Option<usize>,
}
```

Each implementation is an **adapter** over a third-party parser (`toml`, `serde_json`, `serde_yaml` or `saphyr`). The adapter's whole job is normalising that crate's value type and error type into ours.

> [!warning]
> The byte offset must be captured at adapter level. Every upstream crate reports position differently, and retrofitting it after three adapters exist means rewriting all three. It is what turns a parse failure into a useful compile error.

## Selection

A small **factory**, resolved in priority order:

1. Explicit `#[config(format = "yaml")]`.
2. File extension: `toml`, `json`, `yaml`/`yml`.
3. Content sniffing on the first non-whitespace bytes.

Unknown extension with no explicit format is an error, never a guess.

## Registry

Inside the macro, a `match` over an enum of compile-time-enabled formats. A dynamic registry buys nothing there — the macro cannot be extended at runtime.

At runtime, the same `match` unless users need to register their own formats, at which point it becomes a `HashMap<&str, Box<dyn Format>>` built at startup. Do not build the dynamic version before something needs it.

Feature flags gate each format so nobody compiles a YAML parser they never use.

# Conversion layer

```rust title="FromValue"
pub trait FromValue: Sized {
    fn from_value(value: &Value, path: &mut PathStack) -> Result<Self, ConfigError>;
}
```

Trait-based dispatch is what makes recursion free. The macro emits `<T as FromValue>::from_value(...)` and never inspects whether `T` is local, imported, generic, or a type alias.

Impls to provide in core:

- Integers, with range-checked narrowing. `Int(70000)` into `u16` is an error, never a wrap.
- `f32`, `f64`, `bool`, `String`, `PathBuf`, `char`.
- `Vec<T>`, `VecDeque<T>`, `HashSet<T>` where `T: FromValue`.
- `HashMap<String, T>`, `BTreeMap<String, T>`, `IndexMap<String, T>`.
- Tuples up to some arity, from `Seq` of matching length.
- `Box<T>`, `Rc<T>`, `Arc<T>` as transparent wrappers.

> [!note]
> `Option<T>` deliberately has **no** blanket impl used by the macro. Only the parent knows whether a key was absent versus present-and-null, so `Option` is peeled syntactically in the field plan and handled at the lookup site. An impl may still exist for use inside `Vec<Option<T>>`, but codegen does not route through it.

Coercion policy must be written down and then held to. Suggested: `Int -> Float` widening yes; `Float -> Int` no, even when exact; `Str -> Int` no; `Int -> Bool` no. Lax coercion is the kind of convenience that produces bug reports for years.

## Error paths

`PathStack` is a breadcrumb stack pushed on each recursion step, producing `database.replicas[2].port` in the message. Cheap now, miserable to retrofit, and with nesting it is the difference between a usable error and a useless one.

# Field plan

The front end. One `StructPlan` per derived struct, consumed by codegen.

```rust title="Plan"
pub struct StructPlan {
    pub ident: Ident,
    pub generics: Generics,
    pub source: Option<SourcePlan>,   // Some <=> #[config(path)] present
    pub fields: Vec<FieldPlan>,
}

pub struct FieldPlan {
    pub ident: Ident,
    pub key: String,
    pub key_span: Span,
    pub ty: Type,
    pub optional: Option<Type>,       // Some(inner) when the field is Option<inner>
    pub default: Option<DefaultPlan>,
}
```

`source: Option<SourcePlan>` is the single point where the "no path, no `load()`" rule lives. Codegen emits the inherent impl if and only if it is `Some`. A `format` with no `path` is a compile error, not a silent no-op.

`required()` is `optional.is_none() && default.is_none()` — the only place missing-key policy is decided.

Attribute errors accumulate via `syn::Error::combine` so a struct with four bad attributes reports four errors, not one per rebuild.

# Codegen

Per field, in order:

1. Look up `key` in the parent `Map`.
2. Absent and `optional` — `None`.
3. Absent and `default` — evaluate the expression.
4. Absent and required — `ConfigError::Missing` with the accumulated path.
5. Present — push the key onto `PathStack`, call `FromValue::from_value`, pop.

Present-and-`Null` into an `Option` field is `None`. Present-and-`Null` into a required field is a type error, not a missing-key error, and the message should say so.

Generated for a struct with a `path`:

```rust title="Root API"
impl Settings {
    pub fn load() -> Result<Self, ConfigError>;
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, ConfigError>;
    pub fn load_str(source: &str, format: &dyn Format) -> Result<Self, ConfigError>;
}
```

`load_from` exists because operators want an env-var override. `load_str` exists because it makes the whole crate testable without touching a filesystem.

# Compile-time behaviour

During expansion the macro:

1. Resolves `path` against `CARGO_MANIFEST_DIR`, never the process CWD. The CWD during macro expansion is not what anyone expects.
2. Confirms the file exists and reads it.
3. Selects a format and parses it into a `Value`.
4. Type-checks the `Value` against the field plan, so `port = "eighty"` fails the build. This is the main payoff of the whole design.
5. Discards the result and emits runtime code.

> [!important]
> Emit `const _: &str = include_str!("<resolved path>");` into the expansion. The string is unused. It exists so Cargo registers the config file as a build dependency. Without it, editing the config does not re-run the macro and the compile-time check silently validates a stale file.

All three failure classes become a `syn::Error` spanned on the `path` literal. Since you cannot span into a foreign file, the file's line and column go in the message text.

An escape hatch is worth having: `#[config(path = "...", check = false)]` for configs that legitimately do not exist at build time (generated by CI, templated at deploy). Without it the crate is unusable in those setups.

# Runtime API and validation

`load()` returns a parsed, type-correct, semantically unvalidated struct. The user calls `.validate()` from the `validator` crate themselves.

```rust title="Usage"
#[derive(FromConfig, Validate)]
#[config(path = "config.toml")]
struct Settings {
    #[validate(length(min = 1))]
    host: String,
    #[validate(range(min = 1024))]
    port: u16,
}

let settings = Settings::load()?;
settings.validate()?;
```

The two derives are independent — nothing in `cfgy` knows `validator` exists, which is the sign the boundary is in the right place.

`load_validated()` behind a feature flag is acceptable sugar, returning an enum over `ConfigError` and `ValidationErrors`. It must not be the default path; coupling the error type to another crate for every user is a bad trade for saving one line.

# Use cases

> [!example] Flat service config
> Single struct, `path` set, scalars only. The 80% case. Must be one derive and one `load()` call with no ceremony.

> [!example] Nested config
> `Settings { server: Server, database: Database }`. Each nested type derives `FromConfig` and has no `path`, so neither gains a `load()`. Errors report `database.port`, not `port`.

> [!example] Homogeneous map
> `features: HashMap<String, bool>` where keys are unknown at compile time. Distinct from a struct, whose key set is fixed and known.

> [!example] Sequence of structs
> `replicas: Vec<Server>`. Index appears in the error path.

> [!example] Partially-specified config
> `Option` fields and `default` expressions, with the file supplying a subset. Compile-time type check must respect defaults and not demand keys that have them.

> [!example] Format migration
> The same config expressed as TOML, JSON, and YAML deserialises into an identical struct value. This is the contract the IR exists to provide.

> [!example] Deployment override
> `load_from(env::var("APP_CONFIG")?)` at runtime, compile-time validation still pointing at the repo's checked-in default.

> [!example] Renamed and reserved keys
> `#[config(rename = "type")]` onto a field named `kind`. Config keys that are Rust keywords must be reachable.

# Design patterns in use

| Pattern | Where | Why |
|---|---|---|
| Intermediate representation | `Value` | Collapses format $times$ target from $N times M$ to $N + M$ |
| Strategy | `Format` trait | Formats are interchangeable behind one interface |
| Adapter | Each format impl | Normalises a third-party value and error type into ours |
| Factory | Format selection | Extension, attribute, and sniffing resolve to one strategy |
| Registry | Format lookup table | Enum at compile time, map at runtime only if users extend |
| Trait dispatch | `FromValue` | Recursion into nested types without the macro resolving names |
| Compiler pipeline | plan $->$ codegen | Front end is reusable by a future module-attribute front end |
| Facade | `cfgy` crate | One dependency, one import, internals free to move |

The pipeline split is the one that pays off later. A module-level attribute macro, if ever wanted, is a second front end producing $N$ `StructPlan`s and handing them to the same codegen.

# Testing

## Compile-fail — the highest value tests

`trybuild`, one `.rs` and one `.stderr` per case. This is where a proc-macro crate's real quality lives, because the error message *is* the user interface.

- Missing file at the given path.
- File present but malformed, per format.
- Type mismatch between file and struct — `port = "eighty"` for a `u16`.
- Required key absent from the file.
- `format` given without `path`.
- `path` or `format` on a field; `rename` or `default` on the struct.
- Duplicate config key after `rename` collides with another field.
- Derive on an enum, a union, a tuple struct, a unit struct.
- Unknown option inside `#[config(...)]`.
- Two bad attributes in one struct, asserting **both** errors appear.
- `load()` called on a struct with no `path` — asserting the method does not exist.

That last one is the direct test of the rule you specified, and it is easy to break accidentally.

## Expansion snapshots

`macrotest` or committed `cargo expand` output for a representative struct. Catches unintended changes to generated code, including hygiene regressions and a dropped `include_str!`.

## Conversion unit tests

Pure `Value -> T`, no files, no macro:

- Integer narrowing at boundaries: `i64::MAX` into `u16`, `-1` into `u32`, exactly `u16::MAX`.
- `Float` into an integer field rejected even when the value is integral.
- `Null` into `Option<T>` versus into a required field.
- Empty `Seq` into `Vec<T>`; empty `Map` into `HashMap`.
- Deeply nested error path string is exactly right, including the array index form.

## Cross-format equivalence

The single most valuable integration test. One fixture config expressed in all supported formats, asserting all deserialise to an identical struct value. Every IR lowering bug shows up here.

## Property tests

`proptest` on a generated `Value` tree: serialise to each format, parse back, assert equality. Bounds the depth to keep it fast. This is what catches the JSON int-versus-float issue and datetime lowering.

## Attribute parsing

Unit tests on `plan::build` alone, asserting the produced `StructPlan` for representative inputs. Faster than compiling, and where `rename`, `default`, and `Option`-peeling behaviour get pinned down.

## Rebuild behaviour

Not automatable in a normal test suite, but verify manually once and note it: touch the config file, rebuild, confirm the macro re-runs. If `include_str!` is missing this silently passes every other test.

# Deferred

- Enums. Unit variants from a string are easy; tagged variants are a rabbit hole.
- Layered configs — merging a base file with an environment overlay.
- Env-var overlay per field (`#[config(env = "APP_PORT")]`).
- `deny_unknown_keys` as an opt-in. Worth adding eventually; a typo'd key silently doing nothing is a classic support burden.
- Spans in `Value`, for "line 14" in runtime conversion errors.
- Generic structs. Reject with a clear error in v1 rather than emit subtly broken impls.

# Known limitations to document

- `type MaybePort = Option<u16>;` is not recognised as optional. `Option` peeling is syntactic. Serde has the same hole.
- `#[config(default = "localhost")]` on a `String` field does not compile — the expression is `&'static str`. Users write `.to_string()` or `.into()`. A `default = path::to::fn` form can be added later if this grates.
- Compile-time validation reflects the build machine only.
