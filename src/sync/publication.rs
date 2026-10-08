//! One interpretation of the record's answer, independent of producing engine.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationBlock {
    EngineUnavailable,
    MissingSource,
}

impl PublicationBlock {
    pub fn verdict(self) -> PublicationVerdict {
        match self {
            Self::EngineUnavailable => PublicationVerdict::Held,
            Self::MissingSource => PublicationVerdict::Refused,
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::EngineUnavailable => "this publication has no registered engine policy; its bytes remain held",
            Self::MissingSource => "this publication names no source inputs, so the record cannot tell whether it is current; produce it again",
        }
    }
}

/// Source admission is engine-table data, evaluated before any gateway call.
/// An aggregate may have device-local inputs without a server source fence.
pub fn publication_preflight(
    identity: &super::Identity,
    input_base_fingerprint: Option<&str>,
) -> Result<(), PublicationBlock> {
    let engine =
        super::engines::engine(&identity.engine).ok_or(PublicationBlock::EngineUnavailable)?;
    if engine.requires_source(&identity.operation)
        && input_base_fingerprint.is_none_or(str::is_empty)
    {
        return Err(PublicationBlock::MissingSource);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStage {
    Open,
    Transfer,
    Finalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationVerdict {
    Published,
    StoredStale,
    Conflict,
    Refused,
    CredentialRefused,
    Held,
}

impl PublicationVerdict {
    /// Existing native receipt vocabulary; a failed attempt is still held.
    pub fn outcome(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::StoredStale => "stored_stale",
            Self::Conflict => "conflict",
            Self::Refused => "refused",
            Self::CredentialRefused => "credential_refused",
            Self::Held => "failed",
        }
    }

    /// Only these verdicts settle a losing publication. Credential and
    /// transport failures preserve bytes for the next authorized trigger.
    pub fn settled(self) -> Option<&'static str> {
        match self {
            Self::StoredStale | Self::Conflict | Self::Refused => Some(self.outcome()),
            _ => None,
        }
    }
}

/// The record wins. An HTTP refusal takes precedence over any response state;
/// a successful response that has not committed stays held. No engine argument
/// exists, so two engines cannot receive different verdicts for the same facts.
pub fn publication_verdict(
    stage: PublicationStage,
    status: Option<u16>,
    state: Option<&str>,
) -> PublicationVerdict {
    use PublicationVerdict as V;
    match status {
        Some(401) => return V::CredentialRefused,
        Some(409) => {
            return if stage == PublicationStage::Finalize {
                V::StoredStale
            } else {
                V::Conflict
            };
        }
        Some(403) => return V::Refused,
        Some(400) if stage == PublicationStage::Open => return V::Refused,
        Some(status) if !(200..300).contains(&status) => return V::Held,
        _ => {}
    }
    match state {
        Some("published") => V::Published,
        Some("stored_stale") => V::StoredStale,
        Some("conflict") => V::Conflict,
        Some("refused") => V::Refused,
        Some("credential_refused") => V::CredentialRefused,
        _ => V::Held,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_without_a_fingerprint_is_refused_for_every_fenced_engine() {
        for engine in crate::sync::engines::ENGINES {
            let identity = crate::sync::Identity {
                engine: engine.name.into(),
                operation: "individual-resource".into(),
                variant: "default".into(),
            };
            for source in [None, Some("")] {
                assert!(publication_preflight(&identity, source).is_err());
            }
            assert!(publication_preflight(&identity, Some("fenced-inputs")).is_ok());
            for operation in engine.fingerprint_free_operations {
                let aggregate = crate::sync::Identity {
                    operation: (*operation).into(),
                    ..identity.clone()
                };
                assert!(publication_preflight(&aggregate, None).is_ok());
            }
        }
    }

    #[test]
    fn record_answers_have_one_verdict_and_only_publication_verdicts_settle() {
        use PublicationStage::{Finalize, Open, Transfer};
        use PublicationVerdict as V;
        for (stage, status, state, expected) in [
            (Open, Some(401), None, V::CredentialRefused),
            (Finalize, Some(401), None, V::CredentialRefused),
            (Open, Some(403), None, V::Refused),
            (Finalize, Some(403), None, V::Refused),
            (Open, Some(409), None, V::Conflict),
            (Finalize, Some(409), None, V::StoredStale),
            (Open, Some(400), None, V::Refused),
            (Finalize, Some(400), None, V::Held),
            (Open, Some(503), None, V::Held),
            (Finalize, None, None, V::Held),
            (Transfer, None, Some("incomplete"), V::Held),
            (Finalize, Some(200), Some("published"), V::Published),
            (Finalize, None, Some("stored_stale"), V::StoredStale),
            (Finalize, Some(401), Some("published"), V::CredentialRefused),
            (Finalize, Some(503), Some("published"), V::Held),
            (Finalize, Some(200), Some("unknown"), V::Held),
        ] {
            let verdict = publication_verdict(stage, status, state);
            assert_eq!(verdict, expected, "{stage:?} {status:?} {state:?}");
            assert_eq!(
                verdict.settled().is_some(),
                matches!(expected, V::StoredStale | V::Conflict | V::Refused)
            );
        }
    }
}
