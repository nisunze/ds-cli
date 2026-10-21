//! RFC 8785 JSON Canonicalization Scheme support for native room authority.
//!
//! The webview is intentionally not a digest authority.  Network Reporter
//! hashes the `serde_json::Value` produced by Tauri's native deserializer so a
//! browser-authored byte spelling, property order, or number representation
//! cannot widen a signed room capability.

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

const MAX_JCS_DEPTH: usize = 128;

trait CanonicalSink {
    fn append(&mut self, bytes: &[u8]);
}

impl CanonicalSink for Vec<u8> {
    fn append(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

struct DigestSink(Sha256);

impl CanonicalSink for DigestSink {
    fn append(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

fn utf16_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    // RFC 8785 section 3.2.3 requires raw UTF-16 code-unit ordering. This is
    // observably different from UTF-8/scalar ordering for astral characters.
    left.encode_utf16().cmp(right.encode_utf16())
}

fn write_string(sink: &mut impl CanonicalSink, value: &str) {
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

fn ecmascript_number(value: f64) -> String {
    // RFC 8785 delegates number serialization to ECMAScript's shortest
    // binary64 representation (including its fixed/scientific thresholds).
    if value == 0.0 {
        return "0".to_string();
    }
    let negative = value.is_sign_negative();
    let magnitude = value.abs();
    let mut buffer = ryu::Buffer::new();
    let raw = buffer.format_finite(magnitude);

    let (mantissa, exponent) = match raw.split_once(['e', 'E']) {
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
    if magnitude >= 1e21 || magnitude < 1e-6 {
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

fn write_object<'a, I>(
    sink: &mut impl CanonicalSink,
    entries: I,
    depth: usize,
) -> Result<(), String>
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

fn write_value(sink: &mut impl CanonicalSink, value: &Value, depth: usize) -> Result<(), String> {
    if depth > MAX_JCS_DEPTH {
        return Err(format!(
            "RFC 8785 value exceeds its {MAX_JCS_DEPTH}-level nesting bound"
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

/// Canonicalize one already-deserialized JSON value according to RFC 8785.
pub fn rfc8785_canonicalize(value: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    write_value(&mut bytes, value, 0)?;
    Ok(bytes)
}

/// Hash one already-deserialized JSON value according to RFC 8785.
pub fn rfc8785_sha256(value: &Value) -> Result<String, String> {
    let mut sink = DigestSink(Sha256::new());
    write_value(&mut sink, value, 0)?;
    Ok(format!("{:x}", sink.0.finalize()))
}

/// Hash the exact Network Reporter room object without cloning its potentially
/// large layer tree. The object being authorized is exactly `{ "layers": … }`.
pub fn network_reporter_room_content_sha256(
    layers: &BTreeMap<String, Value>,
) -> Result<String, String> {
    let mut sink = DigestSink(Sha256::new());
    sink.append(br#"{"layers":"#);
    write_object(&mut sink, layers.iter(), 1)?;
    sink.append(b"}");
    Ok(format!("{:x}", sink.0.finalize()))
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrictVisitor;

        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an I-JSON value without duplicate object names")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(value)))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let number = Number::from(value);
                exact_binary64(&number).map_err(E::custom)?;
                Ok(StrictValue(Value::Number(number)))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let number = Number::from(value);
                exact_binary64(&number).map_err(E::custom)?;
                Ok(StrictValue(Value::Number(number)))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let number = Number::from_f64(value)
                    .ok_or_else(|| E::custom("RFC 8785 number is not finite binary64"))?;
                Ok(StrictValue(Value::Number(number)))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StrictValue(Value::String(value.to_string())))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(value)))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }

            fn visit_seq<A>(self, mut values: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut result = Vec::new();
                while let Some(value) = values.next_element::<StrictValue>()? {
                    result.push(value.0);
                }
                Ok(StrictValue(Value::Array(result)))
            }

            fn visit_map<A>(self, mut values: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut result = Map::new();
                while let Some(key) = values.next_key::<String>()? {
                    if result.contains_key(&key) {
                        return Err(de::Error::custom(format!(
                            "RFC 8785 object repeats property {key:?}"
                        )));
                    }
                    let value = values.next_value::<StrictValue>()?;
                    result.insert(key, value.0);
                }
                Ok(StrictValue(Value::Object(result)))
            }
        }

        deserializer.deserialize_any(StrictVisitor)
    }
}

/// Strictly parse and canonicalize raw I-JSON. This is used by the shared
/// adversarial conformance vectors; production Tauri input is already a value.
pub fn rfc8785_canonicalize_json(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(raw);
    let value = StrictValue::deserialize(&mut deserializer)
        .map_err(|error| format!("RFC 8785 input JSON is invalid: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("RFC 8785 input JSON has trailing data: {error}"))?;
    rfc8785_canonicalize(&value.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct SharedVectors {
        schema: u8,
        algorithm: String,
        room_envelope: String,
        vectors: Vec<SharedVector>,
        rejections: Vec<SharedRejection>,
    }

    #[derive(Deserialize)]
    struct SharedVector {
        name: String,
        input: String,
        canonical: String,
        sha256: String,
    }

    #[derive(Deserialize)]
    struct SharedRejection {
        name: String,
        input: String,
    }

    #[test]
    fn shared_server_native_vectors_are_byte_pinned_and_all_conform() {
        let raw = include_bytes!("../testdata/network-reporter-room-jcs-v1.json");
        assert_eq!(
            format!("{:x}", Sha256::digest(raw)),
            "2e9f93cba45d9dc74f438fece0be39a71092f7c1556168e0bd1203d35e44379f"
        );
        let fixture: SharedVectors = serde_json::from_slice(raw).unwrap();
        assert_eq!(fixture.schema, 1);
        assert_eq!(fixture.algorithm, "RFC8785");
        assert_eq!(
            fixture.room_envelope,
            "{\"layers\":<server-read-decompressed-sanitized-layer-object>}"
        );
        for vector in fixture.vectors {
            let canonical = rfc8785_canonicalize_json(vector.input.as_bytes())
                .unwrap_or_else(|error| panic!("{}: {error}", vector.name));
            assert_eq!(canonical, vector.canonical.as_bytes(), "{}", vector.name);
            assert_eq!(
                format!("{:x}", Sha256::digest(&canonical)),
                vector.sha256,
                "{}",
                vector.name
            );
        }
        for rejection in fixture.rejections {
            assert!(
                rfc8785_canonicalize_json(rejection.input.as_bytes()).is_err(),
                "{}",
                rejection.name
            );
        }
    }

    #[test]
    fn rfc_example_uses_ecmascript_numbers_and_minimal_string_escaping() {
        let raw = br#"{
          "numbers":[333333333.33333329,1E30,4.50,2e-3,0.000000000000000000000000001],
          "string":"\u20ac$\u000f\nA'B\"\\\"/",
          "literals":[null,true,false]
        }"#;
        assert_eq!(
            String::from_utf8(rfc8785_canonicalize_json(raw).unwrap()).unwrap(),
            "{\"literals\":[null,true,false],\"numbers\":[333333333.3333333,1e+30,4.5,0.002,1e-27],\"string\":\"€$\\u000f\\nA'B\\\"\\\\\\\"/\"}"
        );
    }

    #[test]
    fn keys_are_sorted_as_utf16_and_number_thresholds_match_ecmascript() {
        let value: Value = serde_json::from_str(
            r#"{"\ufb33":1,"😀":2,"€":3,"ö":4,"\u0080":5,"1":6,"\r":7,
                "numbers":[-0.0,0.000001,1e-7,100000000000000000000,1e21]}"#,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(rfc8785_canonicalize(&value).unwrap()).unwrap(),
            "{\"\\r\":7,\"1\":6,\"numbers\":[0,0.000001,1e-7,100000000000000000000,1e+21],\"\u{80}\":5,\"ö\":4,\"€\":3,\"😀\":2,\"דּ\":1}"
        );
    }

    #[test]
    fn strict_raw_parser_rejects_duplicates_and_non_binary64_integers() {
        assert!(rfc8785_canonicalize_json(br#"{"a":1,"a":2}"#).is_err());
        assert!(rfc8785_canonicalize_json(b"9007199254740993").is_err());
        assert!(rfc8785_canonicalize_json(b"9223372036854775807").is_err());
        assert!(rfc8785_canonicalize_json(b"18446744073709551615").is_err());
        assert_eq!(
            rfc8785_canonicalize_json(b"9007199254740992").unwrap(),
            b"9007199254740992"
        );
    }

    #[test]
    fn number_serialization_matches_every_rfc8785_appendix_b_finite_vector() {
        let vectors = [
            (0x0000_0000_0000_0000, "0"),
            (0x8000_0000_0000_0000, "0"),
            (0x0000_0000_0000_0001, "5e-324"),
            (0x8000_0000_0000_0001, "-5e-324"),
            (0x7fef_ffff_ffff_ffff, "1.7976931348623157e+308"),
            (0xffef_ffff_ffff_ffff, "-1.7976931348623157e+308"),
            (0x4340_0000_0000_0000, "9007199254740992"),
            (0xc340_0000_0000_0000, "-9007199254740992"),
            (0x4430_0000_0000_0000, "295147905179352830000"),
            (0x44b5_2d02_c7e1_4af5, "9.999999999999997e+22"),
            (0x44b5_2d02_c7e1_4af6, "1e+23"),
            (0x44b5_2d02_c7e1_4af7, "1.0000000000000001e+23"),
            (0x444b_1ae4_d6e2_ef4e, "999999999999999700000"),
            (0x444b_1ae4_d6e2_ef4f, "999999999999999900000"),
            (0x444b_1ae4_d6e2_ef50, "1e+21"),
            (0x3eb0_c6f7_a0b5_ed8c, "9.999999999999997e-7"),
            (0x3eb0_c6f7_a0b5_ed8d, "0.000001"),
            (0x41b3_de43_5555_5553, "333333333.3333332"),
            (0x41b3_de43_5555_5554, "333333333.33333325"),
            (0x41b3_de43_5555_5555, "333333333.3333333"),
            (0x41b3_de43_5555_5556, "333333333.3333334"),
            (0x41b3_de43_5555_5557, "333333333.33333343"),
            (0xbecb_f647_612f_3696, "-0.0000033333333333333333"),
            (0x4314_3ff3_c1cb_0959, "1424953923781206.2"),
        ];
        for (bits, expected) in vectors {
            assert_eq!(
                ecmascript_number(f64::from_bits(bits)),
                expected,
                "{bits:016x}"
            );
        }
        for bits in [0x7fff_ffff_ffff_ffff, 0x7ff0_0000_0000_0000] {
            assert!(Number::from_f64(f64::from_bits(bits)).is_none());
        }
    }

    #[test]
    fn room_helper_hashes_exact_layers_wrapper_without_cloning() {
        let layers = BTreeMap::from([(
            "lv".to_string(),
            serde_json::json!({"type":"FeatureCollection","features":[]}),
        )]);
        let wrapped = serde_json::json!({"layers": layers});
        let layers = wrapped["layers"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            network_reporter_room_content_sha256(&layers).unwrap(),
            rfc8785_sha256(&wrapped).unwrap()
        );
    }
}
