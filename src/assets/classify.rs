//! Classification (§2.2–§2.4, §6.1): kind / status / sensitivity defaults,
//! folder-default inheritance, and the never-loosen rule.
//!
//! Three rules decide everything here.
//!
//! 1. **The kernel infers, a person decides.** `kind` follows the recognised
//!    format and `status` follows the folder, but only while
//!    `classified_by` is `kernel`. Once a person has classified an asset the
//!    kernel does not re-infer it away (§2.1).
//! 2. **A default may only close.** The class floor is `internal` (D4) and a
//!    folder default is applied with [`Sensitivity::stricter`], so
//!    inheritance can tighten an asset and can never expose one — not even
//!    when the folder itself defaults to `open`. `open` is reachable only as
//!    a deliberate act by a capable caller.
//! 3. **Loosening is capability-gated.** It needs `assets.classify` *and*
//!    the capability that reads the class being loosened *from* (§6.1): a
//!    caller who cannot read confidential documents cannot make one
//!    internal. Tightening is always allowed.
//!
//! Everything the outcome changed is named in `changed`, and any change or
//! any override is audited (§6.4).

use serde::Serialize;

use super::{kind_of, Asset, ClassifiedBy, ClassifyOverride, ClassifyRequest, Sensitivity, Status};

/// §6.1: the capability every classification write needs, whichever field it
/// touches and whichever direction it moves.
const CLASSIFY_CAPABILITY: &str = "assets.classify";

/// The classified asset, the field names that changed, and whether the change
/// must be audited (every classification write is, §6.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClassifyOutcome {
    pub asset: Asset,
    pub changed: Vec<String>,
    pub audit_required: bool,
}

pub fn classify(request: &ClassifyRequest) -> Result<ClassifyOutcome, String> {
    let asset = &request.asset;
    let folder = request.folder.as_ref();
    let overrides = request.r#override.as_ref();
    let capabilities = request.caller_capabilities.as_slice();

    // The folder handed in supplies the defaults, so it has to be the folder
    // the asset ends up in — its own, or the one the override moves it to.
    let destination = overrides
        .and_then(|overrides| overrides.folder.as_deref())
        .unwrap_or(asset.folder.as_str());
    if let Some(folder) = folder {
        if folder.project_id != asset.project_id {
            return Err(format!(
                "folder {} belongs to project {} but the asset belongs to project {}",
                folder.path, folder.project_id, asset.project_id
            ));
        }
        if folder.path != destination {
            return Err(format!(
                "classify was given folder {} but the asset belongs to {destination}; supply the destination folder document",
                folder.path
            ));
        }
    }

    let mut classified = asset.clone();
    let decided_by_a_person = asset.classified_by == ClassifiedBy::User;

    // Inference, which never runs over a person's decision.
    if !decided_by_a_person {
        classified.kind = kind_of(asset.format);
        // D4: an unclassified document is a closed one, never an open one.
        classified.sensitivity = classified.sensitivity.stricter(Sensitivity::default());
        if let Some(default) = folder.and_then(|folder| folder.default_status) {
            if classified.status == Status::Fresh {
                classified.status = default;
            }
        }
    }
    // A folder default can only close, so it is safe to inherit at every
    // point in an asset's life; it can never undo a person's classification
    // in the direction that exposes anything.
    if let Some(default) = folder.and_then(|folder| folder.default_sensitivity) {
        classified.sensitivity = classified.sensitivity.stricter(default);
    }

    if let Some(overrides) = overrides {
        apply_override(&mut classified, overrides, capabilities)?;
    }

    let changed = changed_fields(asset, &classified);
    let audit_required = overrides.is_some() || !changed.is_empty();
    Ok(ClassifyOutcome {
        asset: classified,
        changed,
        audit_required,
    })
}

/// §2.3: the five fields a person may set, each needing `assets.classify`,
/// and sensitivity needing the never-loosen check on top of it.
fn apply_override(
    asset: &mut Asset,
    overrides: &ClassifyOverride,
    capabilities: &[String],
) -> Result<(), String> {
    // An override that names no field still pins the row against future
    // inference, so it is still a classification act and still needs the
    // capability for one.
    if overrides.kind.is_none()
        && overrides.status.is_none()
        && overrides.owner.is_none()
        && overrides.folder.is_none()
        && overrides.sensitivity.is_none()
    {
        require_classify(capabilities, "classification")?;
    }
    if let Some(kind) = overrides.kind {
        require_classify(capabilities, "kind")?;
        asset.kind = kind;
    }
    if let Some(status) = overrides.status {
        require_classify(capabilities, "status")?;
        asset.status = status;
    }
    if let Some(owner) = &overrides.owner {
        require_classify(capabilities, "owner")?;
        if owner.kind.trim().is_empty() || owner.id.trim().is_empty() {
            return Err("an owner override must name both a kind and an id".into());
        }
        asset.owner = Some(owner.clone());
    }
    if let Some(folder) = &overrides.folder {
        require_classify(capabilities, "folder")?;
        asset.folder = folder.clone();
    }
    if let Some(sensitivity) = overrides.sensitivity {
        require_classify(capabilities, "sensitivity")?;
        // The class it is loosening *from* is the class the asset holds now,
        // folder inheritance included — never the looser one it arrived with.
        may_loosen(asset.sensitivity, sensitivity, capabilities)?;
        asset.sensitivity = sensitivity;
    }
    // §2.1: from here the values are a person's, and are never re-inferred.
    asset.classified_by = ClassifiedBy::User;
    Ok(())
}

/// Loosening `from` to `to` needs `assets.classify` and the capability that
/// reads `from` (§6.1); tightening is always allowed.
pub fn may_loosen(
    from: Sensitivity,
    to: Sensitivity,
    caller_capabilities: &[String],
) -> Result<(), String> {
    if !from.loosens_to(to) {
        return Ok(());
    }
    if !holds(caller_capabilities, CLASSIFY_CAPABILITY) {
        return Err(format!(
            "loosening {from} to {to} needs the {CLASSIFY_CAPABILITY} capability"
        ));
    }
    match capability_for(from) {
        Some(capability) if !holds(caller_capabilities, capability) => Err(format!(
            "loosening {from} to {to} needs the {capability} capability: a caller who cannot read {from} documents cannot make one {to}"
        )),
        _ => Ok(()),
    }
}

/// §2.4: the capability that reads bytes of this class; `open` needs none
/// beyond project membership.
pub fn capability_for(sensitivity: Sensitivity) -> Option<&'static str> {
    match sensitivity {
        Sensitivity::Open => None,
        Sensitivity::Internal => Some("assets.read"),
        Sensitivity::Restricted => Some("assets.read.commercial"),
        Sensitivity::Confidential => Some("assets.read.confidential"),
    }
}

fn require_classify(capabilities: &[String], field: &str) -> Result<(), String> {
    if holds(capabilities, CLASSIFY_CAPABILITY) {
        return Ok(());
    }
    Err(format!(
        "changing {field} needs the {CLASSIFY_CAPABILITY} capability"
    ))
}

fn holds(capabilities: &[String], wanted: &str) -> bool {
    capabilities
        .iter()
        .any(|capability| capability.as_str() == wanted)
}

/// The names of the fields this classification moved, in document order.
fn changed_fields(before: &Asset, after: &Asset) -> Vec<String> {
    let mut changed = Vec::new();
    let mut note = |field: &str, moved: bool| {
        if moved {
            changed.push(field.to_string());
        }
    };
    note("folder", before.folder != after.folder);
    note("kind", before.kind != after.kind);
    note("status", before.status != after.status);
    note("sensitivity", before.sensitivity != after.sensitivity);
    note("owner", before.owner != after.owner);
    note("classified_by", before.classified_by != after.classified_by);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{evaluate, Folder, Kind, Owner, REQUEST_SCHEMA};
    use serde_json::{json, Value};

    fn merged(mut base: Value, fields: Value) -> Value {
        if let (Some(base), Some(fields)) = (base.as_object_mut(), fields.as_object()) {
            for (key, value) in fields {
                base.insert(key.clone(), value.clone());
            }
        }
        base
    }

    fn asset(fields: Value) -> Asset {
        let base = json!({
            "asset_id": "a_7Kq3nR2v",
            "project_id": "arjgpydw_survey_test",
            "folder": "contracts/2026/epc",
            "name": "EPC-Lot3-signed.pdf",
            "kind": "doc",
            "format": "pdf",
            "bytes": 2_847_113,
            "source": {"kind": "upload", "detail": "assets tab"}
        });
        serde_json::from_value(merged(base, fields)).unwrap()
    }

    fn folder(fields: Value) -> Folder {
        let base = json!({
            "folder_id": "f_contracts_2026_epc",
            "project_id": "arjgpydw_survey_test",
            "path": "contracts/2026/epc",
            "name": "epc"
        });
        serde_json::from_value(merged(base, fields)).unwrap()
    }

    fn overrides(fields: Value) -> ClassifyOverride {
        serde_json::from_value(fields).unwrap()
    }

    fn request(
        asset: Asset,
        folder: Option<Folder>,
        over: Option<ClassifyOverride>,
        capabilities: &[&str],
    ) -> ClassifyRequest {
        ClassifyRequest {
            schema: REQUEST_SCHEMA.to_string(),
            asset,
            folder,
            r#override: over,
            caller_capabilities: capabilities
                .iter()
                .map(|capability| (*capability).to_string())
                .collect(),
        }
    }

    fn classified(
        asset: Asset,
        folder: Option<Folder>,
        over: Option<ClassifyOverride>,
        capabilities: &[&str],
    ) -> Result<ClassifyOutcome, String> {
        classify(&request(asset, folder, over, capabilities))
    }

    const ALL: [&str; 4] = [
        "assets.classify",
        "assets.read",
        "assets.read.commercial",
        "assets.read.confidential",
    ];

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_classification_never_loosens() {
        // The default class is internal, and inference can never reach open.
        let outcome = classified(asset(json!({})), None, None, &[]).unwrap();
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Internal);
        assert_eq!(outcome.asset.classified_by, ClassifiedBy::Kernel);
        assert!(outcome.changed.is_empty());
        assert!(!outcome.audit_required);
        let outcome = classified(asset(json!({"sensitivity": "open"})), None, None, &[]).unwrap();
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Internal);
        assert_eq!(outcome.changed, vec!["sensitivity".to_string()]);
        assert!(outcome.audit_required);

        // A folder default tightens; it never loosens what it inherits.
        let folder_default = |class: &str| Some(folder(json!({"default_sensitivity": class})));
        let outcome =
            classified(asset(json!({})), folder_default("confidential"), None, &[]).unwrap();
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Confidential);
        let outcome = classified(
            asset(json!({"sensitivity": "confidential"})),
            folder_default("internal"),
            None,
            &[],
        )
        .unwrap();
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Confidential);
        assert!(outcome.changed.is_empty());
        // Not even a folder that defaults to open can open an asset.
        let outcome = classified(asset(json!({})), folder_default("open"), None, &[]).unwrap();
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Internal);

        // A person's classification is not re-inferred, in either direction.
        let outcome = classified(
            asset(json!({"sensitivity": "open", "classified_by": "user", "kind": "other"})),
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Open);
        assert_eq!(outcome.asset.kind, Kind::Other);
        assert!(outcome.changed.is_empty());

        // Loosening explicitly: refused without assets.classify, refused
        // without the capability that reads the class it loosens from.
        let confidential = asset(json!({"sensitivity": "confidential"}));
        let to_internal = || Some(overrides(json!({"sensitivity": "internal"})));
        let error = classified(confidential.clone(), None, to_internal(), &[]).unwrap_err();
        assert_eq!(
            error,
            "changing sensitivity needs the assets.classify capability"
        );
        let error = classified(
            confidential.clone(),
            None,
            to_internal(),
            &["assets.classify", "assets.read", "assets.read.commercial"],
        )
        .unwrap_err();
        assert_eq!(
            error,
            "loosening confidential to internal needs the assets.read.confidential capability: a caller who cannot read confidential documents cannot make one internal"
        );
        let outcome = classified(confidential.clone(), None, to_internal(), &ALL).unwrap();
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Internal);
        assert_eq!(outcome.asset.classified_by, ClassifiedBy::User);
        assert_eq!(
            outcome.changed,
            vec!["sensitivity".to_string(), "classified_by".to_string()]
        );
        assert!(outcome.audit_required);

        // Tightening is always allowed, and needs no reading capability.
        let outcome = classified(
            asset(json!({})),
            None,
            Some(overrides(json!({"sensitivity": "confidential"}))),
            &["assets.classify"],
        )
        .unwrap();
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Confidential);

        // A folder default is applied before the override, so the class the
        // override loosens *from* is the inherited one, not the one it
        // arrived with.
        let error = classified(
            asset(json!({})),
            folder_default("confidential"),
            to_internal(),
            &["assets.classify", "assets.read"],
        )
        .unwrap_err();
        assert!(error.contains("assets.read.confidential"), "{error}");

        // The rule itself, over every ordered pair.
        let all: Vec<String> = ALL
            .iter()
            .map(|capability| capability.to_string())
            .collect();
        let classify_only = vec![CLASSIFY_CAPABILITY.to_string()];
        for from in Sensitivity::ALL {
            for to in Sensitivity::ALL {
                if from.loosens_to(*to) {
                    assert!(may_loosen(*from, *to, &[]).is_err());
                    assert!(may_loosen(*from, *to, &classify_only).is_err());
                    assert!(may_loosen(*from, *to, &all).is_ok());
                    assert_eq!(capability_for(*from).is_some(), *from != Sensitivity::Open);
                } else {
                    assert!(may_loosen(*from, *to, &[]).is_ok());
                }
            }
        }
        assert_eq!(capability_for(Sensitivity::Open), None);
        assert_eq!(capability_for(Sensitivity::Internal), Some("assets.read"));
        assert_eq!(
            capability_for(Sensitivity::Restricted),
            Some("assets.read.commercial")
        );
        assert_eq!(
            capability_for(Sensitivity::Confidential),
            Some("assets.read.confidential")
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_classification_infers_kind_status_and_owner() {
        // kind follows the recognised format while the kernel owns the row.
        let outcome = classified(
            asset(json!({"kind": "other", "format": "xlsx"})),
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(outcome.asset.kind, Kind::Sheet);
        assert_eq!(outcome.changed, vec!["kind".to_string()]);
        assert!(outcome.audit_required);
        let outcome = classified(
            asset(json!({"format": "unknown", "kind": "doc"})),
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(outcome.asset.kind, Kind::Other);

        // status defaults to fresh and takes the folder's default.
        assert_eq!(
            classified(asset(json!({})), None, None, &[])
                .unwrap()
                .asset
                .status,
            Status::Fresh
        );
        let durable = Some(folder(json!({"default_status": "durable"})));
        let outcome = classified(asset(json!({})), durable.clone(), None, &[]).unwrap();
        assert_eq!(outcome.asset.status, Status::Durable);
        // An explicit archive is not reset to the folder's default.
        let outcome = classified(asset(json!({"status": "archive"})), durable, None, &[]).unwrap();
        assert_eq!(outcome.asset.status, Status::Archive);

        // Every override field needs assets.classify, and sets classified_by.
        for (field, over) in [
            ("kind", json!({"kind": "note"})),
            ("status", json!({"status": "durable"})),
            (
                "owner",
                json!({"owner": {"kind": "user", "id": "nisunze@gmail.com"}}),
            ),
            ("folder", json!({"folder": "contracts/2026"})),
            ("sensitivity", json!({"sensitivity": "restricted"})),
        ] {
            let error =
                classified(asset(json!({})), None, Some(overrides(over.clone())), &[]).unwrap_err();
            assert_eq!(
                error,
                format!("changing {field} needs the {CLASSIFY_CAPABILITY} capability")
            );
            let outcome = classified(asset(json!({})), None, Some(overrides(over)), &ALL).unwrap();
            assert_eq!(outcome.asset.classified_by, ClassifiedBy::User);
            assert!(outcome.changed.contains(&field.to_string()), "{field}");
            assert!(outcome.audit_required);
        }

        // An owner override names an accountable party, or it is refused.
        let error = classified(
            asset(json!({})),
            None,
            Some(overrides(json!({"owner": {"kind": "user", "id": "  "}}))),
            &ALL,
        )
        .unwrap_err();
        assert_eq!(error, "an owner override must name both a kind and an id");
        let outcome = classified(
            asset(json!({})),
            None,
            Some(overrides(
                json!({"owner": {"kind": "team", "id": "commercial", "display": "Commercial"}}),
            )),
            &ALL,
        )
        .unwrap();
        assert_eq!(
            outcome.asset.owner,
            Some(Owner {
                kind: "team".into(),
                id: "commercial".into(),
                display: Some("Commercial".into())
            })
        );

        // A move takes the destination folder's defaults with it.
        let outcome = classified(
            asset(json!({})),
            Some(folder(json!({
                "path": "contracts/2026/payroll",
                "default_sensitivity": "confidential",
                "default_status": "durable"
            }))),
            Some(overrides(json!({"folder": "contracts/2026/payroll"}))),
            &ALL,
        )
        .unwrap();
        assert_eq!(outcome.asset.folder, "contracts/2026/payroll");
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Confidential);
        assert_eq!(outcome.asset.status, Status::Durable);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_classification_refuses_unknown_vocabulary_and_wrong_folders() {
        // Unknown vocabulary is refused by name, before classification runs.
        for (field, word) in [
            ("sensitivity", "secret"),
            ("status", "deleted"),
            ("kind", "folder"),
        ] {
            let request = serde_json::to_vec(&json!({
                "schema": REQUEST_SCHEMA,
                "action": "classify",
                "asset": {
                    "asset_id": "a_1", "project_id": "p", "folder": "", "name": "x.pdf",
                    "bytes": 1, "source": {"kind": "upload"}
                },
                "override": {field: word},
                "caller_capabilities": ["assets.classify"]
            }))
            .unwrap();
            let error = evaluate(&request).unwrap_err();
            assert!(error.contains(word), "{error}");
            assert!(error.starts_with("invalid assets request"), "{error}");
        }
        assert!(
            serde_json::from_value::<ClassifyOverride>(json!({"owner": {"kind": "user"}}))
                .unwrap_err()
                .to_string()
                .contains("id")
        );

        // The folder handed in must be the folder the asset ends up in.
        let error = classified(
            asset(json!({})),
            Some(folder(json!({"path": "contracts/2026"}))),
            None,
            &ALL,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "classify was given folder contracts/2026 but the asset belongs to contracts/2026/epc; supply the destination folder document"
        );
        let error = classified(
            asset(json!({})),
            Some(folder(json!({"project_id": "another_project"}))),
            None,
            &ALL,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "folder contracts/2026/epc belongs to project another_project but the asset belongs to project arjgpydw_survey_test"
        );

        // The minimal asset of §2.1 classifies without a folder or an owner.
        let minimal: Asset = serde_json::from_value(json!({
            "asset_id": "a_1", "project_id": "p", "folder": "", "name": "x.bin",
            "bytes": 1, "source": {"kind": "upload"}
        }))
        .unwrap();
        let outcome = classified(minimal, None, None, &[]).unwrap();
        assert_eq!(outcome.asset.kind, Kind::Other);
        assert_eq!(outcome.asset.sensitivity, Sensitivity::Internal);
        assert!(!outcome.audit_required);

        // An override that names no field is still a classification act.
        let error = classified(
            asset(json!({})),
            None,
            Some(overrides(json!({"reason": "checked"}))),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error,
            "changing classification needs the assets.classify capability"
        );

        // An override that changes nothing is still a person's act.
        let outcome = classified(
            asset(json!({"kind": "doc"})),
            None,
            Some(overrides(json!({"kind": "doc", "reason": "checked"}))),
            &ALL,
        )
        .unwrap();
        assert_eq!(outcome.changed, vec!["classified_by".to_string()]);
        assert!(outcome.audit_required);
    }
}
