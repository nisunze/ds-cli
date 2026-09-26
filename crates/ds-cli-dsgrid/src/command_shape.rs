//! Where in one typed engine command a serde refusal applies.
//!
//! `GridCommand` is internally tagged by `command_kind`, so serde buffers the
//! whole command before it picks a variant, and every error it then raises —
//! `missing field `verification``, `invalid type: …` — has lost its place in
//! the document. On 2026-09-25 that turned authoring one design policy into
//! 177 blind guesses.
//!
//! The engine derives each command's JSON Schema from the same types serde
//! deserializes, so this module walks the failing command against that
//! schema, in document order, and names the first location whose violation
//! is the one serde reported. It adds a place to serde's own sentence; it
//! never replaces it, and it keeps no field list of its own. When the schema
//! walk cannot corroborate serde's error, nothing is located rather than a
//! guess.

use ds_grid_engine::GridCommand;
use serde_json::{Map, Value};

/// The place a command-level serde error applies, relative to the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Located {
    /// `duty_profiles[0]`, `row.voltage_class`; empty for the command itself.
    pub path: String,
    /// The innermost named engine type on the way to that place, as
    /// `ds dsgrid describe --kind types --id <type>` publishes it.
    pub type_name: Option<String>,
}

/// What serde said went wrong, reduced to what the schema can check.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Issue {
    Missing(String),
    Unknown(String),
    Type,
    Value,
}

impl Issue {
    fn from_serde(message: &str) -> Option<Self> {
        let quoted = |prefix: &str| {
            message
                .strip_prefix(prefix)
                .and_then(|rest| rest.split('`').next())
                .map(str::to_owned)
        };
        if let Some(field) = quoted("missing field `") {
            Some(Self::Missing(field))
        } else if let Some(field) = quoted("unknown field `") {
            Some(Self::Unknown(field))
        } else if message.starts_with("invalid type:") {
            Some(Self::Type)
        } else if message.starts_with("invalid value:") || message.starts_with("unknown variant") {
            Some(Self::Value)
        } else {
            None
        }
    }
}

/// One schema violation found by the walk.
struct Violation {
    path: Vec<Segment>,
    type_name: Option<String>,
    issue: Issue,
}

#[derive(Clone)]
enum Segment {
    Key(String),
    Index(usize),
}

/// The most violations one walk collects; the first match is all a refusal
/// reports, and a pathological command must not make the refusal expensive.
const MAX_VIOLATIONS: usize = 64;

/// Locate `serde_error`, raised deserializing `command` as a `GridCommand`,
/// against the engine's derived schema for that command.
pub(crate) fn locate(command: &Value, serde_error: &str) -> Option<Located> {
    let issue = Issue::from_serde(serde_error)?;
    let schema = serde_json::to_value(
        schemars::generate::SchemaSettings::draft2020_12()
            .into_generator()
            .into_root_schema_for::<GridCommand>(),
    )
    .ok()?;
    let empty = Map::new();
    let walk = Walk {
        defs: schema
            .get("$defs")
            .and_then(Value::as_object)
            .unwrap_or(&empty),
    };
    let mut found = Vec::new();
    walk.value(command, &schema, &mut Vec::new(), None, &mut found);
    found
        .into_iter()
        .find(|violation| match (&issue, &violation.issue) {
            (Issue::Missing(a), Issue::Missing(b)) | (Issue::Unknown(a), Issue::Unknown(b)) => {
                a == b
            }
            (Issue::Type, Issue::Type) | (Issue::Value, Issue::Value) => true,
            _ => false,
        })
        .map(|violation| Located {
            path: render(&violation.path),
            type_name: violation.type_name,
        })
}

fn render(path: &[Segment]) -> String {
    let mut text = String::new();
    for segment in path {
        match segment {
            Segment::Key(key) if text.is_empty() => text.push_str(key),
            Segment::Key(key) => {
                text.push('.');
                text.push_str(key);
            }
            Segment::Index(index) => text.push_str(&format!("[{index}]")),
        }
    }
    text
}

struct Walk<'a> {
    defs: &'a Map<String, Value>,
}

impl Walk<'_> {
    fn value(
        &self,
        value: &Value,
        schema: &Value,
        path: &mut Vec<Segment>,
        type_name: Option<&str>,
        out: &mut Vec<Violation>,
    ) {
        if out.len() >= MAX_VIOLATIONS {
            return;
        }
        let mut type_name = type_name.map(str::to_owned);
        let mut schema = schema;
        while let Some(name) = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix("#/$defs/"))
        {
            let Some(definition) = self.defs.get(name) else {
                return;
            };
            type_name = Some(name.to_owned());
            schema = definition;
        }
        if let Some(branches) = schema
            .get("oneOf")
            .or_else(|| schema.get("anyOf"))
            .and_then(Value::as_array)
        {
            self.choice(value, branches, path, type_name.as_deref(), out);
            return;
        }
        if let Some(expected) = schema.get("type")
            && !type_admits(expected, value)
        {
            out.push(violation(Issue::Type, path, &type_name));
            return;
        }
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
            && !allowed.contains(value)
        {
            out.push(violation(Issue::Value, path, &type_name));
            return;
        }
        if let Some(constant) = schema.get("const")
            && constant != value
        {
            out.push(violation(Issue::Value, path, &type_name));
            return;
        }
        if !number_in_range(schema, value) {
            out.push(violation(Issue::Value, path, &type_name));
            return;
        }
        match value {
            Value::Object(object) => {
                let empty = Map::new();
                let properties = schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .unwrap_or(&empty);
                let additional = schema.get("additionalProperties");
                for (key, member) in object {
                    path.push(Segment::Key(key.clone()));
                    match (properties.get(key), additional) {
                        (Some(property), _) => {
                            self.value(member, property, path, type_name.as_deref(), out)
                        }
                        (None, Some(Value::Bool(false))) => {
                            path.pop();
                            out.push(violation(Issue::Unknown(key.clone()), path, &type_name));
                            continue;
                        }
                        (None, Some(entry @ Value::Object(_))) => {
                            self.value(member, entry, path, type_name.as_deref(), out)
                        }
                        (None, _) => {}
                    }
                    path.pop();
                }
                for required in schema
                    .get("required")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if !object.contains_key(required) {
                        out.push(violation(
                            Issue::Missing(required.to_owned()),
                            path,
                            &type_name,
                        ));
                    }
                }
            }
            Value::Array(items) => {
                if let Some(item_schema) = schema.get("items") {
                    for (index, item) in items.iter().enumerate() {
                        path.push(Segment::Index(index));
                        self.value(item, item_schema, path, type_name.as_deref(), out);
                        path.pop();
                    }
                }
            }
            _ => {}
        }
    }

    /// A `oneOf`/`anyOf`: an internally tagged enum is chosen by its tag, an
    /// `Option` by null, and anything else by the first branch that admits
    /// the value — or, when none does, by the branch whose type matches.
    fn choice(
        &self,
        value: &Value,
        branches: &[Value],
        path: &mut Vec<Segment>,
        type_name: Option<&str>,
        out: &mut Vec<Violation>,
    ) {
        let resolved: Vec<&Value> = branches.iter().map(|branch| self.resolve(branch)).collect();
        if value.is_null() && resolved.iter().any(|branch| type_admits_null(branch)) {
            return;
        }
        if let Value::Object(object) = value
            && let Some(tag) = tag_key(&resolved)
        {
            let selected = object.get(tag).and_then(|actual| {
                branches.iter().zip(&resolved).find(|(_, branch)| {
                    branch
                        .get("properties")
                        .and_then(|properties| properties.get(tag))
                        .and_then(|property| property.get("const"))
                        == Some(actual)
                })
            });
            match selected {
                Some((branch, _)) => self.value(value, branch, path, type_name, out),
                None if object.contains_key(tag) => {
                    path.push(Segment::Key(tag.to_owned()));
                    out.push(Violation {
                        path: path.clone(),
                        type_name: type_name.map(str::to_owned),
                        issue: Issue::Value,
                    });
                    path.pop();
                }
                None => out.push(Violation {
                    path: path.clone(),
                    type_name: type_name.map(str::to_owned),
                    issue: Issue::Missing(tag.to_owned()),
                }),
            }
            return;
        }
        // Untagged: a branch the value satisfies ends the question.
        let mut first_typed = None;
        for branch in branches {
            let mut trial = Vec::new();
            self.value(value, branch, &mut path.clone(), type_name, &mut trial);
            if trial.is_empty() {
                return;
            }
            if first_typed.is_none()
                && !trial
                    .iter()
                    .any(|v| v.issue == Issue::Type && v.path.len() == path.len())
            {
                first_typed = Some(trial);
            }
        }
        match first_typed {
            Some(trial) => out.extend(trial),
            None => out.push(Violation {
                path: path.clone(),
                type_name: type_name.map(str::to_owned),
                issue: Issue::Type,
            }),
        }
    }

    fn resolve<'s>(&'s self, mut schema: &'s Value) -> &'s Value {
        while let Some(definition) = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix("#/$defs/"))
            .and_then(|name| self.defs.get(name))
        {
            schema = definition;
        }
        schema
    }
}

fn violation(issue: Issue, path: &[Segment], type_name: &Option<String>) -> Violation {
    Violation {
        path: path.to_vec(),
        type_name: type_name.clone(),
        issue,
    }
}

/// The property every object branch pins with a `const`: the enum's tag.
fn tag_key<'s>(branches: &[&'s Value]) -> Option<&'s str> {
    let first = branches.first()?.get("properties")?.as_object()?;
    first
        .iter()
        .filter(|(_, property)| property.get("const").is_some())
        .map(|(key, _)| key.as_str())
        .find(|key| {
            branches.iter().all(|branch| {
                branch
                    .get("properties")
                    .and_then(|properties| properties.get(*key))
                    .and_then(|property| property.get("const"))
                    .is_some()
            })
        })
}

fn type_admits_null(schema: &Value) -> bool {
    match schema.get("type") {
        Some(Value::String(name)) => name == "null",
        Some(Value::Array(names)) => names.iter().any(|name| name == "null"),
        _ => false,
    }
}

fn type_admits(expected: &Value, value: &Value) -> bool {
    let admits = |name: &str| match name {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "string" => value.is_string(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => true,
    };
    match expected {
        Value::String(name) => admits(name),
        Value::Array(names) => names.iter().filter_map(Value::as_str).any(admits),
        _ => true,
    }
}

/// The integer bounds serde enforces for a fixed-width field.
fn number_in_range(schema: &Value, value: &Value) -> bool {
    let Some(number) = value.as_number() else {
        return true;
    };
    if let (Some(minimum), Some(actual)) = (
        schema.get("minimum").and_then(Value::as_f64),
        number.as_f64(),
    ) && actual < minimum
    {
        return false;
    }
    let bounds: Option<(i128, i128)> = match schema.get("format").and_then(Value::as_str) {
        Some("uint8") => Some((0, u8::MAX.into())),
        Some("uint16") => Some((0, u16::MAX.into())),
        Some("uint32") => Some((0, u32::MAX.into())),
        Some("int8") => Some((i8::MIN.into(), i8::MAX.into())),
        Some("int16") => Some((i16::MIN.into(), i16::MAX.into())),
        Some("int32") => Some((i32::MIN.into(), i32::MAX.into())),
        _ => None,
    };
    match (
        bounds,
        number
            .as_i64()
            .map(i128::from)
            .or(number.as_u64().map(i128::from)),
    ) {
        (Some((low, high)), Some(actual)) => (low..=high).contains(&actual),
        _ => true,
    }
}
