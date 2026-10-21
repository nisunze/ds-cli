//! Test-only linked oracles: the former in-process target opening and owner
//! refusal mapping, kept so process-port answers are compared against an
//! independent computation. Never a runtime fallback; dev-dependencies only.
use crate::mutation::Target;
use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_grid_engine::{
    ClearanceSetError, GridSession, StandardError, feature_code_standard::VOLTAGE_CLASSES,
};
use serde_json::{Value, json};

pub(crate) struct LinkedOpened {
    pub session: GridSession,
}

/// The former `feature_codes::open_read_only` over the linked owners.
pub(crate) fn open_read_only(inputs: &Inputs) -> Result<(Value, LinkedOpened), Failure> {
    let target = Target::resolve(inputs, false)?;
    let path = target.path();
    let bytes = crate::package::read_bytes(&path)?;
    let package = ds_grid_exchange::package::unpack(&bytes).map_err(|error| {
        Failure::failed("package_decode_failed", format!("`{path}` did not decode"))
            .detail(json!({ "detail": error.to_string() }))
    })?;
    let session = ds_grid_exchange::linked_models::open_session(&package);
    let head = session.current_revision().revision_id.clone();
    if let Some(pinned) = inputs.value("revision").map(str::trim)
        && pinned != head.as_str()
    {
        return Err(Failure::conflict(
            "revision_conflict",
            "the head moved since the revision you observed",
        )
        .detail(json!({ "expected_revision": pinned, "actual_revision": head.as_str() })));
    }
    let describe = json!({
        "path": path,
        "model_id": package.manifest.model.model_id.as_str(),
        "package_revision": package.manifest.model.model_revision,
        "authored_revision": head.as_str(),
    });
    Ok((describe, LinkedOpened { session }))
}

pub(crate) fn map_standard_error(error: StandardError) -> Failure {
    match error {
        StandardError::Unreadable(detail) => {
            Failure::invalid("standard_invalid", "the standard is not readable JSON")
                .remedy("issue the standard with its build tool")
                .detail(json!({ "detail": detail }))
        }
        StandardError::WrongSchema(detail) => Failure::invalid(
            "standard_invalid",
            "the file does not carry the feature-code standard schema",
        )
        .remedy("pass an issued ds.pls-feature-codes/v1 file")
        .detail(json!({ "detail": detail })),
        StandardError::DigestMismatch {
            version,
            expected,
            found,
        } => Failure::invalid(
            "standard_digest_mismatch",
            format!("standard version {version} was edited outside the owner's build"),
        )
        .remedy("use the bundled standard or the owner's issued file; report a wrong value as a finding")
        .detail(json!({ "version": version, "expected": expected, "found": found })),
        StandardError::VoltageClassUnknown(class) => Failure::invalid(
            "voltage_class_unknown",
            format!("`{class}` is not a voltage class of the standard"),
        )
        .remedy(format!("pass one of {}", VOLTAGE_CLASSES.join(", "))),
    }
}

/// The former `criteria::clearance_set::map_error`.
pub(crate) fn map_set_error(error: ClearanceSetError) -> Failure {
    match error {
        ClearanceSetError::Standard(error) => map_standard_error(error),
        ClearanceSetError::CriterionSetMissing => {
            Failure::invalid("criterion_set_missing", "the model has no criterion set")
                .remedy("convert the workspace with its CRI member first")
        }
        ClearanceSetError::CriterionSetAmbiguous { available } => Failure::invalid(
            "criterion_set_ambiguous",
            "the model has several criterion sets",
        )
        .remedy("pass --criterion-set <id>")
        .detail(json!({ "available": available })),
        ClearanceSetError::CriterionSetNotFound(id) => Failure::invalid(
            "criterion_set_not_found",
            format!("criterion set `{id}` is not in the model"),
        )
        .remedy("pass an id from `ds dsgrid criteria show`"),
        ClearanceSetError::ClearanceCaseMissing {
            which,
            label,
            available,
        } => Failure::invalid(
            "clearance_case_missing",
            format!("weather case {label:?} for the {which} clearance is not in the model"),
        )
        .remedy("use one of the model's weather labels")
        .detail(json!({ "which": which, "label": label, "available": available })),
    }
}
