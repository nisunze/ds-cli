//! Host-neutral local recovery pins. Wire fields match the engine checkpoint
//! pin, but this module neither links the engine nor validates its journal.

use super::{refusal, LocalModel, Refusal, MALFORMED};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_DRAFT_BYTES: u64 = 512 * 1024 * 1024;
pub const DRAFT_CONFLICT: &str = "local_model_draft_conflict";
pub const DRAFT_PENDING: &str = "local_model_draft_pending";
pub const DRAFT_CORRUPT: &str = "local_model_draft_corrupt";

/// Exact identity of the retained validated package, independent of drafts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageIdentity {
    pub sha256: String,
    pub bytes: u64,
    pub model_revision: u64,
    pub head_revision: Option<String>,
}

impl LocalModel {
    pub fn package_identity(&self) -> PackageIdentity {
        PackageIdentity {
            sha256: self.sha256.clone(),
            bytes: self.bytes,
            model_revision: self.model_revision,
            head_revision: self.head_revision.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftBinding {
    pub local_model_id: String,
    pub base_artifact_sha256: String,
}

/// Serde-compatible with ds-grid-engine's DraftCheckpointPin. The authored
/// revision is kept as its wire string so there is no Cargo edge to the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftCheckpointPin {
    pub binding: DraftBinding,
    pub head_revision: String,
    pub sequence: u64,
    pub content_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalDraft {
    pub pin: DraftCheckpointPin,
    pub bytes: u64,
}

pub fn draft_digest_is_safe(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Engine digest domain: domain bytes, NUL, u64 LE byte length, exact bytes.
/// Integrity only; it grants no exchange, export or publication authority.
pub fn draft_content_digest(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"ds.grid.local-draft.v1\0");
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
    format!("{:x}", hash.finalize())
}

impl LocalDraft {
    /// Validate only host-local metadata and binding, never engineering state.
    pub fn validate_for(&self, model: &LocalModel) -> Result<(), Refusal> {
        let pin = &self.pin;
        if self.bytes == 0
            || self.bytes > MAX_DRAFT_BYTES
            || pin.sequence == 0
            || pin.sequence == u64::MAX
            || !draft_digest_is_safe(&pin.content_digest)
            || !draft_digest_is_safe(&pin.binding.base_artifact_sha256)
            || !pin
                .head_revision
                .strip_prefix("rev:")
                .is_some_and(draft_digest_is_safe)
        {
            return refusal(
                MALFORMED,
                "a local draft needs bounded bytes and an engine checkpoint pin",
            );
        }
        if pin.binding.local_model_id != model.id
            || !pin
                .binding
                .base_artifact_sha256
                .eq_ignore_ascii_case(&model.sha256)
        {
            return refusal(
                DRAFT_CONFLICT,
                "the local draft is bound to another model or base package",
            );
        }
        Ok(())
    }
}

pub(super) fn advance(
    row: &mut LocalModel,
    expected_base: &PackageIdentity,
    expected_draft: Option<&DraftCheckpointPin>,
    draft: LocalDraft,
) -> Result<(), Refusal> {
    if row.package_identity() != *expected_base
        || row.draft.as_ref().map(|draft| &draft.pin) != expected_draft
    {
        return refusal(
            DRAFT_CONFLICT,
            "the base package or prior local draft pin moved",
        );
    }
    draft.validate_for(row)?;
    if expected_draft.is_some_and(|previous| draft.pin.sequence <= previous.sequence) {
        return refusal(
            DRAFT_CONFLICT,
            "a local draft checkpoint sequence must advance",
        );
    }
    row.draft = Some(draft);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_models::{evaluate, Catalogue, Op, Origin, Scope};

    fn initial() -> Catalogue {
        let scope = Scope {
            lane: "stable".into(),
            uid: "u-1".into(),
        };
        evaluate(
            &Catalogue::empty(scope.clone()),
            &scope,
            Op::Register {
                id: "m-1".into(),
                display_name: "A".into(),
                origin: Origin::Created,
                crs: "EPSG:32635".into(),
                model_revision: 1,
                bytes: 4,
                sha256: "a".repeat(64),
                created_at: Some("2026-09-30T00:00:00Z".into()),
                project: None,
                head_revision: None,
                activate: false,
            },
        )
        .unwrap()
        .catalogue
    }

    fn stage(catalogue: &Catalogue, sequence: u64) -> Op {
        let row = &catalogue.models[0];
        Op::StageDraft {
            id: row.id.clone(),
            expected_base: row.package_identity(),
            expected_draft: row.draft.as_ref().map(|draft| draft.pin.clone()),
            draft: LocalDraft {
                pin: DraftCheckpointPin {
                    binding: DraftBinding {
                        local_model_id: row.id.clone(),
                        base_artifact_sha256: row.sha256.clone(),
                    },
                    head_revision: format!("rev:{}", "b".repeat(64)),
                    sequence,
                    content_digest: draft_content_digest(b"draft"),
                },
                bytes: 5,
            },
        }
    }

    #[test]
    fn stage_and_advance_only_change_the_independent_draft_pin() {
        let old = initial();
        let first = evaluate(&old, &old.scope, stage(&old, 1)).unwrap();
        let mut expected = old.clone();
        expected.models[0].draft = first.model.unwrap().draft;
        assert_eq!(first.catalogue, expected);
        assert!(!first.active_changed);
        let next = evaluate(&first.catalogue, &old.scope, stage(&first.catalogue, 2)).unwrap();
        assert_eq!(
            next.catalogue.models[0].package_identity(),
            old.models[0].package_identity()
        );
        assert_eq!(
            next.catalogue.models[0]
                .draft
                .as_ref()
                .unwrap()
                .pin
                .sequence,
            2
        );
    }

    #[test]
    fn cas_fences_every_base_field_and_the_whole_previous_pin() {
        let old = initial();
        let current = evaluate(&old, &old.scope, stage(&old, 1))
            .unwrap()
            .catalogue;
        assert_eq!(
            evaluate(&current, &old.scope, stage(&old, 2))
                .unwrap_err()
                .code,
            DRAFT_CONFLICT
        );
        for field in 0..4 {
            let mut op = stage(&current, 2);
            if let Op::StageDraft { expected_base, .. } = &mut op {
                match field {
                    0 => expected_base.sha256 = "c".repeat(64),
                    1 => expected_base.bytes += 1,
                    2 => expected_base.model_revision += 1,
                    _ => expected_base.head_revision = Some("different".into()),
                }
            }
            assert_eq!(
                evaluate(&current, &old.scope, op).unwrap_err().code,
                DRAFT_CONFLICT
            );
        }
        for field in 0..5 {
            let mut op = stage(&current, 2);
            if let Op::StageDraft {
                expected_draft: Some(pin),
                ..
            } = &mut op
            {
                match field {
                    0 => pin.sequence += 1,
                    1 => pin.head_revision.push('x'),
                    2 => pin.content_digest = "c".repeat(64),
                    3 => pin.binding.local_model_id = "m-2".into(),
                    _ => pin.binding.base_artifact_sha256 = "c".repeat(64),
                }
            }
            assert_eq!(
                evaluate(&current, &old.scope, op).unwrap_err().code,
                DRAFT_CONFLICT
            );
        }
        assert_eq!(
            evaluate(&current, &old.scope, stage(&current, 1))
                .unwrap_err()
                .code,
            DRAFT_CONFLICT
        );
    }

    #[test]
    fn malformed_and_foreign_pins_refuse_and_normal_revise_preserves_drafts() {
        let old = initial();
        for field in 0..7 {
            let mut op = stage(&old, 1);
            if let Op::StageDraft { draft, .. } = &mut op {
                match field {
                    0 => draft.bytes = 0,
                    1 => draft.bytes = MAX_DRAFT_BYTES + 1,
                    2 => draft.pin.sequence = 0,
                    3 => draft.pin.sequence = u64::MAX,
                    4 => draft.pin.content_digest = "../escape".into(),
                    5 => draft.pin.head_revision = "invalid".into(),
                    _ => draft.pin.binding.base_artifact_sha256 = "A".repeat(64),
                }
            }
            assert_eq!(evaluate(&old, &old.scope, op).unwrap_err().code, MALFORMED);
        }
        for field in 0..2 {
            let mut op = stage(&old, 1);
            if let Op::StageDraft { draft, .. } = &mut op {
                if field == 0 {
                    draft.pin.binding.local_model_id = "m-2".into();
                } else {
                    draft.pin.binding.base_artifact_sha256 = "b".repeat(64);
                }
            }
            assert_eq!(
                evaluate(&old, &old.scope, op).unwrap_err().code,
                DRAFT_CONFLICT
            );
        }
        let current = evaluate(&old, &old.scope, stage(&old, 1))
            .unwrap()
            .catalogue;
        for expected_sha256 in [None, Some(old.models[0].sha256.clone())] {
            let op = Op::Revise {
                id: "m-1".into(),
                expected_sha256,
                model_revision: 2,
                bytes: 6,
                sha256: "c".repeat(64),
                head_revision: None,
                revised_at: Some("2026-09-30T01:00:00Z".into()),
            };
            assert_eq!(
                evaluate(&current, &old.scope, op).unwrap_err().code,
                DRAFT_PENDING
            );
        }
    }

    #[test]
    fn old_catalogues_reopen_without_a_schema_change_or_added_null_field() {
        let old = initial();
        let bytes = serde_json::to_vec(&old).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("draft"));
        let reopened: Catalogue = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reopened, old);
        let current = evaluate(&old, &old.scope, stage(&old, 1))
            .unwrap()
            .catalogue;
        assert_eq!(
            serde_json::from_slice::<Catalogue>(&serde_json::to_vec(&current).unwrap()).unwrap(),
            current
        );
    }

    #[test]
    fn engine_wire_shape_and_digest_domain_are_exact() {
        let wire = serde_json::json!({
            "binding": {"local_model_id": "m-1", "base_artifact_sha256": "a".repeat(64)},
            "head_revision": format!("rev:{}", "b".repeat(64)),
            "sequence": 1, "content_digest": draft_content_digest(b"draft")
        });
        let pin: DraftCheckpointPin = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(pin).unwrap(), wire);
        // Independently computed from the engine domain's specified byte layout.
        assert_eq!(
            draft_content_digest(b"draft"),
            "dfe1bc26ed280cfb1765ff1cf3d331f62ffc81a7477da0519e0690e64218fe14"
        );
    }
}
