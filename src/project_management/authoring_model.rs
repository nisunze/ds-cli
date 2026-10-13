//! Voluntary authoring labels, never authentication or cryptographic attestation.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringModel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<String>,
}
impl AuthoringModel {
    pub fn validate(&self) -> Result<(), String> {
        let fields = [&self.model, &self.version, &self.strength];
        if !fields.iter().any(|v| v.is_some())
            || fields.into_iter().any(|v| {
                v.as_ref().is_some_and(|s| {
                    s.is_empty()
                        || s.trim() != s
                        || s.len() > 200
                        || s.chars().any(char::is_control)
                })
            })
        {
            return Err("Supply only known nonempty model/version/strength labels, trimmed within 200 bytes; unknown values stay absent".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn known_labels_are_optional_and_cannot_claim_verified_attestation() {
        let d: AuthoringModel = serde_json::from_value(json!({"model":"GPT-6"})).unwrap();
        d.validate().unwrap();
        let encoded = serde_json::to_value(&d).unwrap();
        assert_eq!(encoded, json!({"model":"GPT-6"}));
        assert!(encoded.get("version").is_none());
        assert!(encoded.get("strength").is_none());
        assert!(
            serde_json::from_value::<AuthoringModel>(json!({"model":"GPT-6","source":"verified"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<AuthoringModel>(
                json!({"model":"GPT-6","kind":"self_declared","signature":"proof"})
            )
            .is_err()
        );
    }
    #[test]
    fn empty_unknown_or_out_of_bound_labels_do_not_become_a_writer() {
        assert!(AuthoringModel::default().validate().is_err());
        for text in [
            "".into(),
            " guess ".into(),
            "a".repeat(201),
            "bad\nlabel".into(),
        ] {
            let d = AuthoringModel {
                model: Some(text),
                ..Default::default()
            };
            assert!(d.validate().is_err());
        }
        AuthoringModel {
            strength: Some("Declared reasoning effort".into()),
            ..Default::default()
        }
        .validate()
        .unwrap();
    }
}
