//! RFC 8785 JSON Canonicalization Scheme, decided once for every host.
//!
//! Three implementations of the same canonical form already exist around the
//! product: ds-brain's `content_digest.go` (the transformer `content_digest`
//! ds-web mirrors in `content-digest.ts`), and the desktop shell's
//! `ds-edge-authority::jcs` (the Network Reporter room digest). They agree by
//! construction — UTF-16 key order, ECMAScript number spelling,
//! `JSON.stringify` escaping — and this module is the kernel's own copy so a
//! headless host can produce the same digests without linking the desktop's
//! signing crate. It performs no IO and formats numbers with the edge authority's
//! same Ryu shortest round-trip algorithm, reshaped into ECMAScript's
//! fixed/scientific thresholds.

use serde_json::{Number, Value};
use sha2::{Digest, Sha256};

const MAX_DEPTH: usize = 128;

trait Sink {
    fn append(&mut self, bytes: &[u8]);
}

impl Sink for Vec<u8> {
    fn append(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

struct DigestSink(Sha256);

impl Sink for DigestSink {
    fn append(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

/// RFC 8785 §3.2.3: object members sort by their UTF-16 code units, which
/// differs from scalar-value order for astral characters.
fn utf16_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn write_string(sink: &mut impl Sink, value: &str) {
    sink.append(b"\"");
    for character in value.chars() {
        match character {
            '\u{0008}' => sink.append(br"\b"),
            '\u{0009}' => sink.append(br"\t"),
            '\u{000a}' => sink.append(br"\n"),
            '\u{000c}' => sink.append(br"\f"),
            '\u{000d}' => sink.append(br"\r"),
            '"' => sink.append(br#"\""#),
            '\\' => sink.append(br"\\"),
            '\u{0000}'..='\u{001f}' => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = character as u8;
                sink.append(&[
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    HEX[(code >> 4) as usize],
                    HEX[(code & 0xf) as usize],
                ]);
            }
            _ => {
                let mut buffer = [0_u8; 4];
                sink.append(character.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    sink.append(b"\"");
}

/// Every JSON number must be one exact binary64: an integer above 2^53 has no
/// ECMAScript identity, and a digest over a value the other side cannot hold
/// would silently disagree.
fn exact_binary64(number: &Number) -> Result<f64, String> {
    if let Some(value) = number.as_i64() {
        let binary = value as f64;
        if binary as i128 != i128::from(value) {
            return Err("RFC 8785 number is not exactly representable as binary64".to_string());
        }
        return Ok(binary);
    }
    if let Some(value) = number.as_u64() {
        let binary = value as f64;
        if binary as u128 != u128::from(value) {
            return Err("RFC 8785 number is not exactly representable as binary64".to_string());
        }
        return Ok(binary);
    }
    number
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| "RFC 8785 number is not finite binary64".to_string())
}

/// ECMAScript `Number::toString`: the shortest digit string that round-trips,
/// in fixed notation between 1e-6 (inclusive) and 1e21 (exclusive), otherwise
/// scientific with a signed exponent. The digits use the same Ryu formatter as the native edge authority.
pub(crate) fn ecmascript_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let negative = value.is_sign_negative();
    let magnitude = value.abs();
    let mut buffer = ryu::Buffer::new();
    let raw = buffer.format_finite(magnitude);
    let (mantissa, exponent) = match raw.split_once('e') {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<i32>()
                .expect("ryu always emits a bounded decimal exponent"),
        ),
        None => (raw, 0),
    };
    let decimal_position = mantissa.find('.').unwrap_or(mantissa.len()) as i32 + exponent;
    let mut digits = mantissa
        .bytes()
        .filter(|byte| *byte != b'.')
        .collect::<Vec<_>>();
    let leading = digits.iter().take_while(|byte| **byte == b'0').count();
    let decimal_position = decimal_position - leading as i32;
    digits.drain(..leading);
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    debug_assert!(!digits.is_empty());

    let mut rendered = String::new();
    if negative {
        rendered.push('-');
    }
    if !(1e-6..1e21).contains(&magnitude) {
        rendered.push(digits[0] as char);
        if digits.len() > 1 {
            rendered.push('.');
            rendered.extend(digits[1..].iter().map(|byte| *byte as char));
        }
        let scientific_exponent = decimal_position - 1;
        rendered.push('e');
        if scientific_exponent >= 0 {
            rendered.push('+');
        }
        rendered.push_str(&scientific_exponent.to_string());
    } else if decimal_position <= 0 {
        rendered.push_str("0.");
        rendered.extend(std::iter::repeat_n('0', (-decimal_position) as usize));
        rendered.extend(digits.iter().map(|byte| *byte as char));
    } else if decimal_position as usize >= digits.len() {
        rendered.extend(digits.iter().map(|byte| *byte as char));
        rendered.extend(std::iter::repeat_n(
            '0',
            decimal_position as usize - digits.len(),
        ));
    } else {
        let split = decimal_position as usize;
        rendered.extend(digits[..split].iter().map(|byte| *byte as char));
        rendered.push('.');
        rendered.extend(digits[split..].iter().map(|byte| *byte as char));
    }
    rendered
}

fn write_object<'a, I>(sink: &mut impl Sink, entries: I, depth: usize) -> Result<(), String>
where
    I: IntoIterator<Item = (&'a String, &'a Value)>,
{
    let mut entries = entries.into_iter().collect::<Vec<_>>();
    entries.sort_unstable_by(|left, right| utf16_cmp(left.0, right.0));
    sink.append(b"{");
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index > 0 {
            sink.append(b",");
        }
        write_string(sink, key);
        sink.append(b":");
        write_value(sink, value, depth + 1)?;
    }
    sink.append(b"}");
    Ok(())
}

fn write_value(sink: &mut impl Sink, value: &Value, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "RFC 8785 value exceeds its {MAX_DEPTH}-level nesting bound"
        ));
    }
    match value {
        Value::Null => sink.append(b"null"),
        Value::Bool(false) => sink.append(b"false"),
        Value::Bool(true) => sink.append(b"true"),
        Value::Number(number) => sink.append(ecmascript_number(exact_binary64(number)?).as_bytes()),
        Value::String(value) => write_string(sink, value),
        Value::Array(values) => {
            sink.append(b"[");
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    sink.append(b",");
                }
                write_value(sink, value, depth + 1)?;
            }
            sink.append(b"]");
        }
        Value::Object(values) => write_object(sink, values.iter(), depth)?,
    }
    Ok(())
}

/// The canonical bytes of one already-parsed JSON value.
pub fn canonicalize(value: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    write_value(&mut bytes, value, 0)?;
    Ok(bytes)
}

/// Lowercase hex SHA-256 of the canonical bytes, without materializing them.
pub fn sha256(value: &Value) -> Result<String, String> {
    let mut sink = DigestSink(Sha256::new());
    write_value(&mut sink, value, 0)?;
    Ok(format!("{:x}", sink.0.finalize()))
}

/// Lowercase hex SHA-256 of the canonical `{"layers": …}` room document —
/// the Network Reporter `room_content_sha256` — streamed straight from the
/// layer map so a large room is never cloned into a wrapper object.
pub fn room_content_sha256(
    layers: &std::collections::BTreeMap<String, Value>,
) -> Result<String, String> {
    let mut sink = DigestSink(Sha256::new());
    sink.append(br#"{"layers":"#);
    write_object(&mut sink, layers.iter(), 1)?;
    sink.append(b"}");
    Ok(format!("{:x}", sink.0.finalize()))
}

/// Lowercase hex SHA-256 of the canonical layer map itself — the transformer
/// `content_digest` ds-brain stores in `metadata.content_digest` and answers
/// "unchanged" against (`internal/bulk/services/content_digest.go`).
pub fn layers_content_digest(
    layers: &std::collections::BTreeMap<String, Value>,
) -> Result<String, String> {
    let mut sink = DigestSink(Sha256::new());
    write_object(&mut sink, layers.iter(), 0)?;
    Ok(format!("{:x}", sink.0.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn canonical(value: Value) -> String {
        String::from_utf8(canonicalize(&value).unwrap()).unwrap()
    }

    #[test]
    fn numbers_spell_as_ecmascript_number_to_string() {
        for (value, expected) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.3, "0.3"),
            (100.0, "100"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (0.1 + 0.2, "0.30000000000000004"),
            (29.5678, "29.5678"),
            (-1.91234, "-1.91234"),
            (9007199254740992.0, "9007199254740992"),
            (1e16, "10000000000000000"),
            (123.456e5, "12345600"),
        ] {
            assert_eq!(ecmascript_number(value), expected, "{value:?}");
        }
    }

    #[test]
    #[allow(clippy::excessive_precision)]
    fn rfc_8785_test_vectors() {
        // The RFC's own example (§3.2.3 / Appendix), key order by UTF-16; the
        // over-precise literal is the RFC's, and the point of the vector.
        let value = json!({
            "numbers": [333333333.33333329, 1E30, 4.50, 2e-3, 0.000000000000000000000000001],
            "string": "\u{20ac}$\u{000F}\u{000a}A'\u{0042}\u{0022}\u{005c}\\\"\u{002f}",
            "literals": [null, true, false]
        });
        let expected = String::from(
            "{\"literals\":[null,true,false],\"numbers\":[333333333.3333333,1e+30,4.5,0.002,1e-27],\"string\":\"\u{20ac}$\\u000f\\nA'B\\\"\\\\\\\\\\\"/\"}",
        );
        assert_eq!(canonical(value), expected);
        // UTF-16 order: the surrogate pairs (D800…, D834…) sort BEFORE U+FF61,
        // the opposite of scalar-value order. This is the observable
        // difference RFC 8785 §3.2.3 calls out.
        let astral = json!({"\u{1d11e}": 1, "\u{ff61}": 2, "z": 3, "\u{10000}": 4});
        assert_eq!(
            canonical(astral),
            "{\"z\":3,\"\u{10000}\":4,\"\u{1d11e}\":1,\"\u{ff61}\":2}"
        );
    }

    #[test]
    fn integers_above_binary64_precision_are_refused() {
        let value = json!({"n": 9007199254740993_u64});
        assert!(canonicalize(&value).unwrap_err().contains("binary64"));
        let exact = json!({"n": 9007199254740992_u64});
        assert_eq!(canonical(exact), r#"{"n":9007199254740992}"#);
    }

    #[test]
    fn the_room_digest_is_the_canonical_wrapper_and_the_content_digest_the_bare_map() {
        let mut layers = std::collections::BTreeMap::new();
        layers.insert(
            "poles".to_string(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"b":2,"a":1.50},"geometry":{"type":"Point","coordinates":[30.0612345,-1.94]}}]}),
        );
        layers.insert(
            "cables".to_string(),
            json!({"type":"FeatureCollection","features":[]}),
        );
        let wrapper = json!({"layers": layers.clone()});
        assert_eq!(
            room_content_sha256(&layers).unwrap(),
            sha256(&wrapper).unwrap()
        );
        let bare = Value::Object(
            layers
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        assert_eq!(
            layers_content_digest(&layers).unwrap(),
            sha256(&bare).unwrap()
        );
        assert_ne!(
            room_content_sha256(&layers).unwrap(),
            layers_content_digest(&layers).unwrap()
        );
        assert_eq!(
            canonical(bare),
            r#"{"cables":{"features":[],"type":"FeatureCollection"},"poles":{"features":[{"geometry":{"coordinates":[30.0612345,-1.94],"type":"Point"},"properties":{"a":1.5,"b":2},"type":"Feature"}],"type":"FeatureCollection"}}"#
        );
    }

    #[test]
    fn nesting_is_bounded() {
        let mut value = json!(1);
        for _ in 0..130 {
            value = json!([value]);
        }
        assert!(canonicalize(&value).unwrap_err().contains("nesting bound"));
    }
}
