use super::MAX_SURVEY_FIELDS;
use serde_json::{json, Map, Value};

/// The evaluator's operator vocabulary, also used by authoring clients.
#[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Operator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    Gt,
    Gte,
    Lt,
    Lte,
    IsTrue,
    IsFalse,
    IsEmpty,
    NotEmpty,
    InArray,
    NotInArray,
}
impl Operator {
    const ALL: [Self; 14] = [
        Self::Equals,
        Self::NotEquals,
        Self::Contains,
        Self::NotContains,
        Self::Gt,
        Self::Gte,
        Self::Lt,
        Self::Lte,
        Self::IsTrue,
        Self::IsFalse,
        Self::IsEmpty,
        Self::NotEmpty,
        Self::InArray,
        Self::NotInArray,
    ];
    fn requires_value(self) -> bool {
        !matches!(
            self,
            Self::IsTrue | Self::IsFalse | Self::IsEmpty | Self::NotEmpty
        )
    }
}
pub(super) fn operators() -> Value {
    json!(Operator::ALL.map(|op| {
        let value = serde_json::to_value(op).unwrap();
        let label = match op { Operator::Gt => ">", Operator::Gte => ">=", Operator::Lt => "<", Operator::Lte => "<=", _ => "" };
        json!({"value":value,"requires_value":op.requires_value(),"label":label,"label_key":format!("ff_op_{}",value.as_str().unwrap())})
    }))
}

fn string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
        _ => v.to_string(),
    }
}
fn number(v: &Value) -> Option<f64> {
    let s = string(v);
    if s.trim().is_empty() {
        return None;
    }
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}
fn empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}
fn condition(c: &Value, values: &Value) -> bool {
    let key = c["field"].as_str().unwrap_or("").replace('.', "_");
    let raw = &values[&key];
    let actual = string(raw).to_lowercase();
    let target = string(&c["value"]).to_lowercase();
    let token = c["operator"]
        .as_str()
        .or(c["op"].as_str())
        .unwrap_or("equals");
    let Ok(op) = serde_json::from_value::<Operator>(json!(token)) else {
        return false;
    };
    match op {
        Operator::Equals => actual == target,
        Operator::NotEquals => actual != target,
        Operator::Contains => actual.contains(&target),
        Operator::NotContains => !actual.contains(&target),
        Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte => {
            let (Some(a), Some(b)) = (number(raw), number(&c["value"])) else {
                return false;
            };
            match c["operator"].as_str().or(c["op"].as_str()).unwrap_or("") {
                "gt" => a > b,
                "gte" => a >= b,
                "lt" => a < b,
                _ => a <= b,
            }
        }
        Operator::IsTrue => actual == "true",
        Operator::IsFalse => raw.is_null() || actual == "false",
        Operator::IsEmpty => empty(raw),
        Operator::NotEmpty => !empty(raw),
        op @ (Operator::InArray | Operator::NotInArray) => {
            let choices = c["values"]
                .as_array()
                .map(|a| a.iter().map(string).collect::<Vec<_>>())
                .unwrap_or_else(|| {
                    target
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect()
                });
            let found = choices.iter().any(|s| s.to_lowercase() == actual);
            if matches!(op, Operator::InArray) {
                found
            } else {
                !found
            }
        }
    }
}
fn combine(items: &[Value], mode: &str, test: impl Fn(&Value) -> bool) -> bool {
    if items.is_empty() {
        return false;
    }
    match mode {
        "or" | "any" => items.iter().any(test),
        "and" | "all" => items.iter().all(test),
        _ => false,
    }
}
fn conditions(rule: &Value, values: &Value) -> bool {
    combine(
        rule["conditions"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        rule["combinator"].as_str().unwrap_or("and"),
        |c| condition(c, values),
    )
}
fn visible(field: &Value, values: &Value) -> bool {
    let rule = &field["visibility"];
    if rule.is_null() {
        return true;
    }
    if let Some(groups) = rule["groups"].as_array().filter(|g| !g.is_empty()) {
        return combine(
            groups,
            rule["groups_mode"]
                .as_str()
                .or(rule["groupsMode"].as_str())
                .unwrap_or("all"),
            |g| conditions(g, values),
        );
    }
    conditions(rule, values)
}
fn validate(field: &Value, value: &Value) -> Option<String> {
    let kind = field["type"].as_str().unwrap_or("");
    if matches!(kind, "textBlock" | "separator") {
        return None;
    }
    let media = matches!(
        kind,
        "photo" | "photos" | "video" | "videos" | "document" | "documents" | "audio" | "audios"
    );
    let absent = |v: &Value| v.is_null() || v.as_str() == Some("");
    let count = value
        .as_array()
        .map(|a| a.iter().filter(|v| !absent(v)).count())
        .unwrap_or(usize::from(!absent(value)));
    let missing = if media { count == 0 } else { absent(value) };
    if missing {
        return (field["required"] == true)
            .then(|| format!("{} is required", string(&field["label"])));
    }
    let text = string(value);
    let validation = &field["validation"];
    match kind {
        "email" | "phone" | "website" => {
            let (pattern, error) = match kind {
                "email" => (r"^[^\s@]+@[^\s@]+\.[^\s@]+$", "Invalid email address"),
                "phone" => (r"^[+]?[0-9\s()-]{6,20}$", "Invalid phone number"),
                _ => (r"^https?://.+", "Invalid URL (must start with http)"),
            };
            if !regex::Regex::new(pattern)
                .expect("static pattern")
                .is_match(&text)
            {
                return Some(error.into());
            }
        }
        "number" | "slider" => {
            let Some(n) = number(value) else {
                return Some("Must be a number".into());
            };
            for (key, message) in [("min", "Minimum"), ("max", "Maximum")] {
                if let Some(bound) = validation[key].as_f64() {
                    if (key == "min" && n < bound) || (key == "max" && n > bound) {
                        return Some(format!("{message} is {}", validation[key]));
                    }
                }
            }
        }
        "text" | "multiline" => {
            let n = text.encode_utf16().count() as f64;
            for (key, message) in [("min", "Minimum"), ("max", "Maximum")] {
                if let Some(bound) = validation[key].as_f64() {
                    if (key == "min" && n < bound) || (key == "max" && n > bound) {
                        return Some(format!("{message} {} characters", validation[key]));
                    }
                }
            }
            if let Some(pattern) = validation["pattern"].as_str().filter(|s| !s.is_empty()) {
                match regex::RegexBuilder::new(pattern)
                    .size_limit(1024 * 1024)
                    .build()
                {
                    Ok(re) if !re.is_match(&text) => return Some("Invalid format".into()),
                    Err(_) => return Some("Invalid validation pattern in form schema".into()),
                    _ => {}
                }
            }
        }
        _ if media => {
            if field["minCount"]
                .as_u64()
                .is_some_and(|n| (count as u64) < n)
            {
                return Some(format!("At least {} required", field["minCount"]));
            }
            if field["maxCount"]
                .as_u64()
                .is_some_and(|n| (count as u64) > n)
            {
                return Some(format!("Maximum {} allowed", field["maxCount"]));
            }
        }
        _ => {}
    }
    None
}
pub(super) fn evaluate(request: &Value) -> Result<Value, String> {
    let fields = request["fields"]
        .as_array()
        .ok_or("Survey fields must be an array")?;
    if fields.len() > MAX_SURVEY_FIELDS {
        return Err("Survey form exceeds 512 fields".into());
    }
    let values = request["values"]
        .as_object()
        .ok_or("Survey values must be an object")?;
    let mut seen = std::collections::HashSet::new();
    let mut keys = Vec::new();
    let mut data = Map::new();
    let mut errors = Map::new();
    for field in fields {
        let key = field["key"].as_str().ok_or("Survey field has no key")?;
        if key.is_empty() || !seen.insert(key) {
            return Err("Survey field keys must be nonempty and unique".into());
        }
        if request["visible_only"] != false && !visible(field, &request["values"]) {
            continue;
        }
        keys.push(key);
        let value = values.get(key).unwrap_or(&Value::Null);
        if let Some(value) = values.get(key) {
            data.insert(key.into(), value.clone());
        }
        if request["validate"] != false {
            if let Some(error) = validate(field, value) {
                errors.insert(key.into(), json!(error));
            }
        }
    }
    Ok(json!({"visible_keys":keys,"data":data,"errors":errors}))
}

#[cfg(test)]
mod vocabulary_tests {
    use super::*;
    #[test]
    fn exported_vocabulary_is_the_evaluators_enum() {
        let rows = operators();
        assert_eq!(rows.as_array().unwrap().len(), 14);
        for row in rows.as_array().unwrap() {
            assert!(serde_json::from_value::<Operator>(row["value"].clone()).is_ok());
        }
        assert!(condition(
            &json!({"field":"x","operator":"in_array","values":["a","b"]}),
            &json!({"x":"a"})
        ));
    }
}
