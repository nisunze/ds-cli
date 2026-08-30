//! Pure validation for the linked ds-network compile-time release pin.
//!
//! This file is shared by `build.rs` and the contract tests so malformed
//! release pins cannot drift into an untested second implementation.

pub const RELEASE_PIN_STATE: &str = "release_pin";
pub const DEVELOPMENT_UNPINNED_STATE: &str = "development_unpinned";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsNetworkPin {
    pub source_sha: Option<String>,
    pub state: &'static str,
}

pub fn resolve_ds_network_pin(
    profile: &str,
    release_pin: Option<&str>,
) -> Result<DsNetworkPin, &'static str> {
    if profile != "release" {
        return Ok(DsNetworkPin {
            source_sha: None,
            state: DEVELOPMENT_UNPINNED_STATE,
        });
    }

    let release_pin = release_pin.ok_or(
        "release builds require DS_RELEASE_PIN_DS_NETWORK as one exact lowercase 40-hex SHA",
    )?;
    if release_pin.len() != 40
        || !release_pin
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(
            "release builds require DS_RELEASE_PIN_DS_NETWORK as one exact lowercase 40-hex SHA",
        );
    }
    Ok(DsNetworkPin {
        source_sha: Some(release_pin.to_owned()),
        state: RELEASE_PIN_STATE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_requires_one_exact_lowercase_sha() {
        for invalid in [
            None,
            Some(""),
            Some("a"),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            Some("gggggggggggggggggggggggggggggggggggggggg"),
        ] {
            assert!(resolve_ds_network_pin("release", invalid).is_err());
        }

        let exact = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            resolve_ds_network_pin("release", Some(exact)),
            Ok(DsNetworkPin {
                source_sha: Some(exact.to_owned()),
                state: RELEASE_PIN_STATE,
            })
        );
    }

    #[test]
    fn development_is_explicitly_unpinned_and_ignores_overrides() {
        let exact = "0123456789abcdef0123456789abcdef01234567";
        for supplied in [None, Some(exact), Some("malformed-runtime-value")] {
            assert_eq!(
                resolve_ds_network_pin("debug", supplied),
                Ok(DsNetworkPin {
                    source_sha: None,
                    state: DEVELOPMENT_UNPINNED_STATE,
                })
            );
        }
    }
}
