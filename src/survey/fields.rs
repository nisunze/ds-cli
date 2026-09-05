use super::MAX_SURVEY_FIELDS;
use serde_json::{json, Map, Value};

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
    match c["operator"]
        .as_str()
        .or(c["op"].as_str())
        .unwrap_or("equals")
    {
        "equals" => actual == target,
        "not_equals" => actual != target,
        "contains" => actual.contains(&target),
        "not_contains" => !actual.contains(&target),
        "gt" | "gte" | "lt" | "lte" => {
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
        "is_true" => actual == "true",
        "is_false" => raw.is_null() || actual == "false",
        "is_empty" => empty(raw),
        "not_empty" => !empty(raw),
        op @ ("in_array" | "not_in_array") => {
            let choices = c["values"]
                .as_array()
                .map(|a| a.iter().map(string).collect::<Vec<_>>())
                .unwrap_or_else(|| target.split(',').map(|s| s.trim().into()).collect());
            let found = choices.iter().any(|s| s.to_lowercase() == actual);
            if op == "in_array" {
                found
            } else {
                !found
            }
        }
        _ => false,
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
    let mut keys = Vec::new();
    let mut data = Map::new();
    let mut errors = Map::new();
    for field in fields {
        let key = field["key"].as_str().ok_or("Survey field has no key")?;
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
