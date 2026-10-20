//! The small value shapes every preference contract is built from: bounded
//! text, finite numbers in a range, JavaScript's rounding and truthiness (the
//! readers they replace were JavaScript), and closed members.
use serde_json::{Map, Value};

/// The largest integer a JSON number carries exactly into JavaScript.
pub(in crate::ui_preferences) const MAX_SAFE: u64 = 9_007_199_254_740_991;
/// The longest map key or list entry a preference keeps (a layer id, a
/// collapse path, a source address).
pub(in crate::ui_preferences) const MAX_ENTRY_BYTES: usize = 1024;

/// Text with no control character, 1..=`max` bytes.
pub(in crate::ui_preferences) fn text(value: &Value, max: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|text| !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control))
}

/// An identity: bounded text with no surrounding whitespace.
pub(in crate::ui_preferences) fn identity(value: &Value, max: usize) -> Option<&str> {
    text(value, max).filter(|text| text.trim() == *text)
}

/// Optional free text (a label, a name): any string within the bound.
pub(in crate::ui_preferences) fn label(value: &Value, max: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|text| text.len() <= max && !text.chars().any(char::is_control))
}

/// A map key or list entry.
pub(in crate::ui_preferences) fn entry(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_ENTRY_BYTES && !key.chars().any(char::is_control)
}

/// A finite number within `[min, max]`.
pub(in crate::ui_preferences) fn ranged(value: &Value, min: f64, max: f64) -> Option<f64> {
    value.as_f64().filter(|number| (min..=max).contains(number))
}

/// A whole number of milliseconds a JavaScript clock can hold.
pub(in crate::ui_preferences) fn millis(value: &Value) -> Option<u64> {
    value.as_u64().filter(|ms| *ms <= MAX_SAFE)
}

/// `Math.round`: halves round toward positive infinity.
pub(in crate::ui_preferences) fn js_round(number: f64) -> f64 {
    (number + 0.5).floor()
}

/// A number as JavaScript writes it: integral values without a fraction.
pub(in crate::ui_preferences) fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < MAX_SAFE as f64 {
        Value::from(value as i64)
    } else {
        Value::from(value)
    }
}

/// JavaScript truthiness of one stored field.
pub(in crate::ui_preferences) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `Number(value)` for the stored shapes the old readers accepted.
pub(in crate::ui_preferences) fn js_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok().filter(|n| n.is_finite()),
        _ => None,
    }
}

/// The one member of a closed vocabulary this value names.
pub(in crate::ui_preferences) fn member(
    value: &Value,
    members: &[&'static str],
) -> Option<&'static str> {
    let name = value.as_str()?;
    members.iter().copied().find(|member| *member == name)
}

/// An object whose keys are all in `allowed`.
pub(in crate::ui_preferences) fn fields<'a>(
    value: &'a Value,
    allowed: &[&str],
) -> Option<&'a Map<String, Value>> {
    let map = value.as_object()?;
    map.keys()
        .all(|key| allowed.contains(&key.as_str()))
        .then_some(map)
}

/// `[x, y, ...]` of exactly `N` finite numbers.
pub(in crate::ui_preferences) fn finite_tuple<const N: usize>(value: &Value) -> Option<[f64; N]> {
    let items = value.as_array().filter(|items| items.len() == N)?;
    let mut out = [0.0; N];
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = item.as_f64()?;
    }
    Some(out)
}

/// The size of a value's stored JSON text.
pub(in crate::ui_preferences) fn stored_len(value: &Value) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |text| text.len())
}

/// `new Date(ms).toISOString()`.
pub(in crate::ui_preferences) fn iso_from_ms(ms: u64) -> String {
    let at = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.millisecond()
    )
}
