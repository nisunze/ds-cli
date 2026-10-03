//! Domain-free formatting and validation shared by CLI hosts.

use std::time::{SystemTime, UNIX_EPOCH};

/// Shorten by Unicode scalar values, reserving the last position for an ellipsis.
/// Retains the CLI's historical ellipsis for nonempty text at width zero.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// ASCII case policy: digest consumers may normalize mixed case or require the
/// canonical lowercase spelling. Prefix and whitespace policies belong to callers.
#[derive(Clone, Copy, Debug)]
pub enum HexCase {
    Any,
    Lower,
}

pub fn is_hex(value: &str, digits: usize, case: HexCase) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == digits
        && bytes.iter().all(|byte| {
            byte.is_ascii_hexdigit() && (matches!(case, HexCase::Any) || !byte.is_ascii_uppercase())
        })
}

pub fn is_sha256_hex(value: &str, case: HexCase) -> bool {
    is_hex(value, 64, case)
}

/// A full catalogue digest, requiring its exact lowercase prefix.
pub fn is_sha256_digest(value: &str, case: HexCase) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| is_sha256_hex(hex, case))
}

/// A nonempty slash-separated relative inventory path with no traversal or
/// backslashes. Callers retain any additional length/control-character rules.
pub fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Host epoch milliseconds; clock-before-epoch returns zero, overflow saturates.
pub fn now_ms() -> u64 {
    epoch_ms().unwrap_or(0)
}

/// Callers that refuse a clock before the epoch retain their typed error.
pub fn epoch_ms() -> Result<u64, std::time::SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
}

/// Signed epoch fields retain their existing Rust and JSON representation.
pub fn now_ms_i64() -> i64 {
    i64::try_from(now_ms()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_keeps_unicode_and_historical_small_widths() {
        assert_eq!(truncate("é🙂a", 2), "é…");
        assert_eq!(truncate("é🙂", 2), "é🙂");
        assert_eq!(truncate("x", 1), "x");
        assert_eq!(truncate("xy", 1), "…");
        assert_eq!(truncate("x", 0), "…");
        assert_eq!(truncate("", 0), "");
    }

    #[test]
    fn hex_policies_keep_exact_ascii_widths_and_prefixes() {
        let lower = "0123456789abcdef".repeat(4);
        let upper = lower.to_uppercase();
        assert!(is_sha256_hex(&lower, HexCase::Lower));
        assert!(is_sha256_hex(&upper, HexCase::Any));
        assert!(!is_sha256_hex(&upper, HexCase::Lower));
        for value in [
            "a".repeat(63),
            "a".repeat(65),
            "g".repeat(64),
            "é".repeat(32),
        ] {
            assert!(!is_sha256_hex(&value, HexCase::Any));
        }
        assert!(is_sha256_digest(&format!("sha256:{lower}"), HexCase::Lower));
        for value in [
            lower.clone(),
            format!("SHA256:{lower}"),
            format!(" sha256:{lower}"),
        ] {
            assert!(!is_sha256_digest(&value, HexCase::Any));
        }
        assert!(is_hex("abcdef", 6, HexCase::Lower));
    }

    #[test]
    fn inventory_paths_keep_the_existing_grammar() {
        for path in ["member", "nested/member.ext", "colon:name", "with spaces"] {
            assert!(safe_relative(path));
        }
        for path in ["", "/member", "a\\b", "a//b", "a/./b", "a/../b", "a/"] {
            assert!(!safe_relative(path));
        }
    }
}
