//! A count read from a host-supplied value — the rule, stated once.
//!
//! Every count the kernel types is a `u64`, and every host hands it whatever
//! the wire handed them: a browser's `JSON.parse` yields `-1`, `2.5`, `null`,
//! `"7"` or a missing member with equal ease, and `u64` decodes none of them.
//! So each caller clamped before it called — the browser's Solar seeding card
//! with `Math.max(0, Math.trunc(value))`, the collision card with the same
//! expression — and the clamp, which is a decision, lived in TypeScript where
//! two copies could disagree about what a malformed count means.
//!
//! It means this, and the kernel says so:
//!
//! * a JSON number reads as `max(0, trunc(value))`, saturating at `u64::MAX`;
//! * everything else reads as `0` — a non-number (a numeric STRING included:
//!   nothing is coerced), a non-finite number, `null`, an absent member.
//!
//! Clamping rather than refusing is deliberate: refusing would be a NEW
//! behaviour. A card handed a reporter reply carrying `-1` showed a zero; it
//! never showed the operator a refusal, and a count is never the fact a
//! decision turns on — the plan's cities are.
//!
//! This is what a value a host was handed MEANS, not a bound on what a count
//! may be. A member whose absence means something other than zero, or one
//! allowed to be negative, does not belong to this rule.
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// The rule, applied to one value.
pub fn count_of(value: &Value) -> u64 {
    let Value::Number(number) = value else {
        return 0;
    };
    if let Some(whole) = number.as_u64() {
        return whole;
    }
    if let Some(signed) = number.as_i64() {
        return u64::try_from(signed).unwrap_or(0);
    }
    match number.as_f64() {
        Some(float) if float.is_finite() && float >= 1.0 => truncate(float),
        _ => 0,
    }
}

/// The rule, as a `#[serde(default, deserialize_with = "…")]` reader.
pub fn count<'de, D>(input: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(count_of(&Value::deserialize(input)?))
}

fn truncate(float: f64) -> u64 {
    let whole = float.trunc();
    if whole >= u64::MAX as f64 {
        u64::MAX
    } else {
        whole as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Row {
        #[serde(default, deserialize_with = "count")]
        n: u64,
    }

    fn read(json: &str) -> u64 {
        serde_json::from_str::<Row>(json)
            .expect("a count never fails to decode")
            .n
    }

    /// The whole rule as a table: this is the answer every card that used to
    /// clamp in TypeScript now reads.
    #[test]
    fn the_rule_is_one_table() {
        for (json, expected) in [
            (r#"{"n":0}"#, 0),
            (r#"{"n":7}"#, 7),
            (r#"{"n":18446744073709551615}"#, u64::MAX),
            // Negative, fractional, and both at once.
            (r#"{"n":-1}"#, 0),
            (r#"{"n":-9223372036854775808}"#, 0),
            (r#"{"n":2.5}"#, 2),
            (r#"{"n":0.9}"#, 0),
            (r#"{"n":-0.5}"#, 0),
            // Beyond u64 saturates rather than wrapping or refusing.
            (r#"{"n":1e30}"#, u64::MAX),
            // Everything that is not a number is not a count.
            (r#"{"n":null}"#, 0),
            (r#"{"n":"7"}"#, 0),
            (r#"{"n":true}"#, 0),
            (r#"{"n":[7]}"#, 0),
            (r#"{"n":{"n":7}}"#, 0),
            (r#"{}"#, 0),
        ] {
            assert_eq!(read(json), expected, "{json}");
        }
    }

    /// The float path is reached only for a number `as_u64` and `as_i64` both
    /// refused; it truncates toward zero there too, and saturates rather than
    /// wrapping.
    #[test]
    fn the_float_path_truncates_and_saturates() {
        let fractional = serde_json::Number::from_f64(2.99).expect("finite");
        assert_eq!(count_of(&Value::Number(fractional)), 2);
        assert_eq!(truncate(f64::MAX), u64::MAX);
    }
}
