use std::collections::BTreeMap;

pub fn config_sha<'a>(
    profile: &str,
    stamp: Option<&'a str>,
) -> Result<Option<&'a str>, &'static str> {
    match stamp {
        Some(value)
            if value.len() == 64
                && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) => Ok(Some(value)),
        None if profile != "release" => Ok(None),
        _ => Err("release UI identity requires DS_UI_BUILD_CONFIG_SHA256 from the captured producer configuration"),
    }
}

pub fn source_sha<'a>(profile: &str, source: Option<&'a str>) -> Result<&'a str, &'static str> {
    match source {
        Some(value)
            if value.len() == 40
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
        {
            Ok(value)
        }
        None if profile != "release" => Ok("development"),
        _ => Err(
            "release UI identity requires DS_INTERACTIONS_SOURCE_SHA as a 40-character lowercase Git SHA; a supplied SHA must always be valid",
        ),
    }
}

pub fn encode_config(inputs: &BTreeMap<String, Option<Vec<u8>>>) -> Vec<u8> {
    let mut encoded = b"ds.interactions-ui-config/v1\0".to_vec();
    for (name, bytes) in inputs {
        encoded.extend_from_slice(&(name.len() as u64).to_be_bytes());
        encoded.extend_from_slice(name.as_bytes());
        encoded.push(u8::from(bytes.is_some()));
        if let Some(bytes) = bytes {
            encoded.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
            encoded.extend_from_slice(bytes);
        }
    }
    encoded
}
