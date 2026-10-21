//! Bounded Settings evidence. Complete values remain available to explicit file
//! export; a rule-set entry must never smuggle thousands of rows into one item.
use super::editor::{apply, Request};
use serde_json::{json, Map, Value};
fn preview(value: &Value, depth: usize, truncated: &mut bool) -> Value {
    match value {
        Value::String(s) if s.chars().count() > 240 => {
            *truncated = true;
            json!(s.chars().take(240).collect::<String>())
        }
        Value::Array(_) | Value::Object(_) if depth == 0 => {
            *truncated = true;
            json!({"omitted":true})
        }
        Value::Array(a) => {
            *truncated |= a.len() > 12;
            json!(a
                .iter()
                .take(12)
                .map(|v| preview(v, depth - 1, truncated))
                .collect::<Vec<_>>())
        }
        Value::Object(m) => {
            *truncated |= m.len() > 12;
            let mut out = Map::new();
            for (k, v) in m.iter().take(12) {
                if k.chars().count() > 240 {
                    *truncated = true;
                    continue;
                }
                out.insert(k.clone(), preview(v, depth - 1, truncated));
            }
            Value::Object(out)
        }
        _ => value.clone(),
    }
}
pub fn bounded_page(items: &[Value], offset: usize, limit: usize) -> Result<Value, String> {
    if !(1..=100).contains(&limit) {
        return Err("config_page_invalid".into());
    }
    let mut truncated = false;
    let values: Vec<_> = items
        .iter()
        .skip(offset)
        .take(limit)
        .map(|v| preview(v, 4, &mut truncated))
        .collect();
    Ok(
        json!({"items":values,"total":items.len(),"more":items.len().saturating_sub(offset.saturating_add(limit)),"truncated":truncated}),
    )
}
pub fn read_sheet(
    document: &Value,
    key: &str,
    rule_set: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<Value, String> {
    let value = document["sheets"].get(key).ok_or("config_sheet_unknown")?;
    let is_rules = apply(Request::IsRuleSetSheet {
        document: document.clone(),
        sheet_key: key.into(),
    })?
    .as_bool()
    .unwrap_or(false);
    let (rows, name, metadata) = if is_rules {
        let names = apply(Request::RuleSetNames {
            document: document.clone(),
            sheet_key: key.into(),
            value: value.clone(),
        })?;
        let name = rule_set
            .or_else(|| names[0].as_str())
            .ok_or("config_rule_set_source_invalid")?;
        if !names
            .as_array()
            .is_some_and(|a| a.iter().any(|n| n == name))
        {
            return Err("config_rule_set_source_invalid".into());
        }
        (
            apply(Request::RuleSetRows {
                value: value.clone(),
                name: name.into(),
            })?,
            Some(name.to_owned()),
            apply(Request::RuleSetMetadata {
                value: value.clone(),
                name: name.into(),
            })?,
        )
    } else {
        if rule_set.is_some() {
            return Err("config_rule_set_source_invalid".into());
        }
        (value.clone(), None, json!({}))
    };
    let mut page = bounded_page(
        rows.as_array().ok_or("config_sheet_shape_invalid")?,
        offset,
        limit,
    )?;
    let mut truncated = page["truncated"].as_bool().unwrap_or(false);
    page["metadata"] = preview(&metadata, 3, &mut truncated);
    page["truncated"] = json!(truncated);
    page["sheet"] = json!(key);
    page["rule_set"] = json!(name);
    Ok(page)
}
