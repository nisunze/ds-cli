//! Thin file/authority bindings for the single native repair owner.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_command_kernel::design_repair;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
const INPUT: Arg = Arg::value(
    "input",
    "<request.json>",
    "Closed native request, bounded to 64 MiB.",
)
.required();
const OUT: Arg = Arg::value(
    "out",
    "<result.json>",
    "New result file; existing outputs refuse.",
)
.required();
const fn refusal(code: &'static str) -> Refusal {
    Refusal {
        code,
        when: "The native owner rejects this request or evidence.",
        remedy: "Inspect the named scope, source, head or selection fence.",
    }
}
const REFUSALS: &[Refusal] = &[
    refusal("repair_document_invalid"),
    refusal("repair_bound_exceeded"),
    refusal("repair_scope_invalid"),
    refusal("repair_scope_mismatch"),
    refusal("repair_source_changed"),
    refusal("repair_target_changed"),
    refusal("repair_target_invalid"),
    refusal("repair_layer_unknown"),
    refusal("repair_crs_required"),
    refusal("repair_geometry_invalid"),
    refusal("repair_field_not_allowed"),
    refusal("repair_value_invalid"),
    refusal("repair_identity_required"),
    refusal("repair_identity_ambiguous"),
    refusal("repair_change_ambiguous"),
    refusal("repair_proposal_changed"),
    refusal("repair_selection_invalid"),
    refusal("repair_effect_unconfirmed"),
    refusal("repair_preservation_refused"),
    refusal("repair_operation_unknown"),
    refusal("repair_output_refused"),
    refusal("repair_input_unreadable"),
    refusal("repair_matching_invalid"),
    refusal("repair_match_ambiguous"),
    refusal("audit_source_incomplete"),
    refusal("repair_root_unknown"),
    refusal("repair_key_invalid"),
    refusal("repair_field_invalid"),
    refusal("repair_source_required"),
    refusal("repair_unit_mismatch"),
];
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    args: &'static [Arg],
    effect: Effect,
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "Run the native repair owner over explicit evidence and retained snapshots. Audit and compare are read-only. Proposal and selected projection never save, process, resize, renumber, rebuild or demote data. Preserve exact revision, scope, source pins and missing/null cells. Local workspace apply and authorized publication are separate declared commands.",
        chapter: Chapter::Design,
        effect,
        authority: Authority::None,
        execution: Execution::Sync,
        args,
        output: "Bounded output receipt with result path, bytes and SHA-256. The complete native result is in the explicit new file.",
        examples: &[],
        refusals: REFUSALS,
        reference: Some("docs/reference/design-repair.md"),
        search: &["repair", "restore", "reconcile", "approved", "evidence"],
        requires: Requires::Server,
        availability: || ds_cli_contract::spec::Availability::Available,
    }
}
pub static DESCRIBE: Command = command(
    "design.repair.describe",
    &["design", "repair", "describe"],
    "Describe evidence, repair operations and native field policy.",
    &[],
    Effect::ReadOnly,
);
pub static AUDIT: Command = command(
    "design.repair.audit",
    &["design", "repair", "audit"],
    "Audit explicit design snapshots without changing them.",
    &[INPUT, OUT],
    Effect::LocalFileWrite,
);
pub static COMPARE: Command = command(
    "design.repair.compare",
    &["design", "repair", "compare"],
    "Compare exact positions or independent typed keys.",
    &[INPUT, OUT],
    Effect::LocalFileWrite,
);
pub static PROPOSE: Command = command(
    "design.repair.propose",
    &["design", "repair", "propose"],
    "Propose allowlisted cells from exact evidence.",
    &[INPUT, OUT],
    Effect::LocalFileWrite,
);
pub static PROJECT: Command = command(
    "design.repair.apply-selected",
    &["design", "repair", "apply-selected"],
    "Project selected repairs without persistence or processing.",
    &[INPUT, OUT],
    Effect::LocalFileWrite,
);
const WORKSPACE_REFUSALS: &[Refusal] = &[
    refusal("repair_workspace_busy"),
    refusal("repair_workspace_unavailable"),
    refusal("repair_operation_id_conflict"),
    refusal("repair_readback_mismatch"),
];
const fn workspace_refusals() -> [Refusal; REFUSALS.len() + WORKSPACE_REFUSALS.len()] {
    let mut result = [REFUSALS[0]; REFUSALS.len() + WORKSPACE_REFUSALS.len()];
    let mut i = 0;
    while i < REFUSALS.len() {
        result[i] = REFUSALS[i];
        i += 1;
    }
    let mut j = 0;
    while j < WORKSPACE_REFUSALS.len() {
        result[i] = WORKSPACE_REFUSALS[j];
        i += 1;
        j += 1;
    }
    result
}
pub static WORKSPACE: Command = Command {
    refusals: &workspace_refusals(),
    ..command(
        "design.project.repair",
        &["design", "project", "repair"],
        "Commit selected evidence-backed cells to an offline workspace.",
        &[
            INPUT,
            Arg::value("workspace", "<dir>", "Private Design workspace.").required(),
        ],
        Effect::LocalFileWrite,
    )
};
const LOCAL_PUBLISH: &[Refusal] = &[
    refusal("repair_server_fence_required"),
    refusal("repair_receipt_mismatch"),
    refusal("repair_readback_unverified"),
    refusal("repair_operation_id_invalid"),
    refusal("repair_response_invalid"),
    refusal("repair_response_failed"),
    refusal("repair_response_too_large"),
    refusal("transformer_repair_invalid"),
    refusal("transformer_repair_source_invalid"),
    refusal("transformer_repair_source_moved"),
    refusal("transformer_repair_identity_invalid"),
    refusal("transformer_repair_preimage_moved"),
    refusal("transformer_repair_conflict"),
    refusal("transformer_repair_operation_conflict"),
    refusal("transformer_version_conflict"),
    refusal("transformer_version_mismatch"),
    refusal("reserved_transformer_name"),
    refusal("transformer_source_changed"),
    refusal("transformer_locked"),
    refusal("transformer_retired"),
    refusal("transformer_deleted"),
    refusal("insufficient_permissions"),
    refusal("project_archived"),
    refusal("project_expired"),
    refusal("transformer_document_too_large"),
];
const fn publish_refusals()
-> [Refusal; REFUSALS.len() + LOCAL_PUBLISH.len() + super::lv::project_save::COMMAND.refusals.len()]
{
    let mut result = [REFUSALS[0];
        REFUSALS.len() + LOCAL_PUBLISH.len() + super::lv::project_save::COMMAND.refusals.len()];
    let mut i = 0;
    while i < REFUSALS.len() {
        result[i] = REFUSALS[i];
        i += 1;
    }
    let mut j = 0;
    while j < LOCAL_PUBLISH.len() {
        result[i] = LOCAL_PUBLISH[j];
        i += 1;
        j += 1;
    }
    j = 0;
    while j < super::lv::project_save::COMMAND.refusals.len() {
        result[i] = super::lv::project_save::COMMAND.refusals[j];
        i += 1;
        j += 1;
    }
    result
}
pub static PUBLISH: Command = Command {
    authority: Authority::HeadlessProject,
    args: &[
        INPUT,
        Arg::value(
            "lane",
            "<stable|canary>",
            "Exact authority lane matching the reviewed proposal.",
        )
        .choices(&["stable", "canary"])
        .required(),
        Arg::value(
            "evidence-file",
            "<file>",
            "Required when evidence declares a raw file pin; exact original bytes.",
        ),
    ],
    refusals: &publish_refusals(),
    availability: ds_cli_auth::native_availability,
    ..command(
        "design.repair.publish",
        &["design", "repair", "publish"],
        "Publish selected property repairs and verify exact saved readback.",
        &[],
        Effect::GlobalWrite,
    )
};
fn read(i: &Inputs) -> Result<Value, Failure> {
    let bytes = ds_design_workspace::read_file(
        Path::new(i.require("input")?),
        ds_command_kernel::design::MAX_BYTES,
    )
    .map_err(|e| Failure::invalid("repair_input_unreadable", e.to_string()))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| Failure::invalid("repair_document_invalid", e.to_string()))
}
fn write(i: &Inputs, result: Value) -> Result<Value, Failure> {
    let bytes = serde_json::to_vec(&result)
        .map_err(|e| Failure::invalid("repair_document_invalid", e.to_string()))?;
    ds_design_workspace::write_new(Path::new(i.require("out")?), &bytes)
        .map_err(|e| Failure::failed("repair_output_refused", e.to_string()))?;
    Ok(
        json!({"out":i.require("out")?,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"schema":result["schema"],"mutated":false,"persisted":false}),
    )
}
fn evaluate(op: &str, i: &Inputs) -> Result<Value, Failure> {
    let result =
        design_repair::evaluate(op, read(i)?).map_err(|e| native_failure(&e.code, e.message))?;
    write(i, result)
}
pub fn describe(_: &Inputs, _: &Context) -> Result<Value, Failure> {
    Ok(design_repair::describe())
}
pub fn audit(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    evaluate("audit", i)
}
pub fn compare(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    evaluate("compare", i)
}
pub fn propose(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    evaluate("propose", i)
}
pub fn project(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    evaluate("apply_selected", i)
}
pub fn workspace(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let input = serde_json::to_vec(&read(i)?).unwrap();
    ds_design_workspace::Workspace::open(Path::new(i.require("workspace")?))
        .and_then(|mut w| w.repair(&input))
        .map_err(workspace_failure)
}
fn workspace_failure(error: ds_design_workspace::Error) -> Failure {
    let message = error.to_string();
    match error {
        ds_design_workspace::Error::Conflict(_) if message.starts_with("operation id already") => {
            Failure::conflict("repair_operation_id_conflict", message)
        }
        ds_design_workspace::Error::Conflict(_) => {
            Failure::conflict("repair_target_changed", message)
        }
        ds_design_workspace::Error::Busy(_) => Failure::conflict("repair_workspace_busy", message),
        ds_design_workspace::Error::Io(_) => {
            Failure::failed("repair_workspace_unavailable", message)
        }
        ds_design_workspace::Error::Invalid(_) => {
            let code = message
                .split(':')
                .next()
                .unwrap_or("repair_document_invalid")
                .to_owned();
            native_failure(&code, message)
        }
    }
}
pub fn publish(i: &Inputs, c: &Context) -> Result<Value, Failure> {
    if !c.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "Review the exact selected proposal and pass --yes.",
        ));
    }
    let publication: ds_client_core::TransformerRepairPublication =
        serde_json::from_value(read(i)?)
            .map_err(|e| Failure::invalid("repair_document_invalid", e.to_string()))?;
    if let Some(source) = &publication.source {
        if let Some(expected) = &source.raw_file_sha256 {
            let path = i.value("evidence-file").ok_or_else(|| {
                Failure::invalid(
                    "repair_source_changed",
                    "Supply --evidence-file for the declared raw file pin.",
                )
            })?;
            let bytes =
                ds_design_workspace::read_file(Path::new(path), design_repair::MAX_SOURCE_BYTES)
                    .map_err(|e| Failure::invalid("repair_input_unreadable", e.to_string()))?;
            let raw: Value = serde_json::from_slice(&bytes)
                .map_err(|e| Failure::invalid("repair_document_invalid", e.to_string()))?;
            let layers = raw.get("layers").cloned().unwrap_or_else(|| raw.clone());
            let same_layers = layers == serde_json::to_value(&source.layers).unwrap()
                || (raw["type"] == "FeatureCollection"
                    && source.layers.len() == 1
                    && source.layers.values().next() == Some(&raw));
            if format!("{:x}", Sha256::digest(&bytes)) != *expected || !same_layers {
                return Err(Failure::invalid(
                    "repair_source_changed",
                    "Raw evidence bytes or their decoded layers differ from the reviewed source.",
                ));
            }
        }
    }
    let receipt = ds_cli_auth::repair_transformer(i.require("lane")?, &publication)?;
    Ok(json!({"project":receipt.project_id(),"lane":receipt.lane(),"result":receipt.result()}))
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn native_failure(code: &str, message: String) -> Failure {
    match code {
        "repair_bound_exceeded"
        | "repair_change_ambiguous"
        | "repair_crs_required"
        | "repair_document_invalid"
        | "repair_effect_unconfirmed"
        | "repair_field_invalid"
        | "repair_field_not_allowed"
        | "repair_geometry_invalid"
        | "repair_identity_ambiguous"
        | "repair_identity_required"
        | "repair_key_invalid"
        | "repair_layer_unknown"
        | "repair_match_ambiguous"
        | "repair_operation_unknown"
        | "repair_preservation_refused"
        | "repair_proposal_changed"
        | "repair_readback_mismatch"
        | "repair_root_unknown"
        | "repair_scope_invalid"
        | "repair_scope_mismatch"
        | "repair_selection_invalid"
        | "repair_source_changed"
        | "repair_source_required"
        | "repair_target_changed"
        | "repair_target_invalid"
        | "repair_unit_mismatch"
        | "repair_value_invalid" => Failure::invalid(code, message),
        _ => Failure::invalid("repair_document_invalid", message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_repair_vocabulary_remains_declared_and_preserved() {
        let source = include_str!("../../../../ds-command-kernel/src/design_repair.rs");
        let mut checked = std::collections::BTreeSet::new();
        for code in source.split('"').filter(|text| {
            (text.starts_with("repair_") || text.starts_with("audit_"))
                && text.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        }) {
            assert!(
                REFUSALS.iter().any(|r| r.code == code),
                "undeclared owner code: {code}"
            );
            assert_eq!(native_failure(code, "owner refusal".into()).code(), code);
            checked.insert(code);
        }
        assert!(
            checked.len() >= 26,
            "the owner vocabulary scan stopped seeing native refusals"
        );
        assert_eq!(
            workspace_failure(ds_design_workspace::Error::Invalid(
                "repair_source_changed: exact evidence moved".into()
            ))
            .code(),
            "repair_source_changed"
        );
        assert_eq!(
            workspace_failure(ds_design_workspace::Error::Conflict("head moved".into())).code(),
            "repair_target_changed"
        );
    }
}
