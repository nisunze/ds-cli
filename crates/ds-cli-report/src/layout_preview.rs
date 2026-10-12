//! Thin local native-account adapter for the shared captured-page preview.
use ds_cli_contract::spec::Refusal;
use ds_cli_contract::{Failure, Inputs};
use ds_report_host::{layout_preview, preview_artifact};
use serde_json::{json, Value};
use std::{io::Read, path::PathBuf};

pub(crate) const REFUSALS: &[Refusal] = &[
    crate::project::NATIVE_PROFILE,
    crate::project::NATIVE_PROFILE_DIGEST,
    crate::project::NATIVE_PROFILE_UNSAFE,
    crate::project::HEADLESS_SIGNED_OUT,
    crate::project::NATIVE_STATE_UNSAFE,
    crate::project::NATIVE_STATE_UNAVAILABLE,
    crate::project::NATIVE_STATE_PROTECTION,
    crate::project::NATIVE_STATE_ROOT,
    crate::project::NATIVE_STATE_CONFLICT,
    crate::project::NATIVE_CLEANUP,
    crate::project::AUTH_CONTEXT_MISMATCH,
    Refusal {
        code: "printing_invalid",
        when: "the bounded capture or preview mode is invalid",
        remedy: "use one inline native RenderRequest and correct the named layout constraint",
    },
    Refusal {
        code: "project_required",
        when: "--preview has no explicit project",
        remedy: "pass the exact --project and --lane for this native account",
    },
    Refusal {
        code: "report_inputs_invalid",
        when: "the capture, held renderer policy or native source scope disagrees",
        remedy: "correct the exact captured inputs; no defaults or source are substituted",
    },
    Refusal {
        code: "report_staging_failed",
        when: "private disk scratch cannot be created",
        remedy: "repair the writable DS data root and disk capacity",
    },
    Refusal {
        code: "report_result_invalid",
        when: "the producer manifest or bytes differ from the requested page",
        remedy: "preserve the capture and install matching CLI/reporter artifacts",
    },
    Refusal {
        code: "report_engine_identity_changed",
        when: "the reporter changed during rendering",
        remedy: "retry after installation has finished",
    },
    Refusal {
        code: "report_engine_unsupported",
        when: "the engine cannot render captured layouts",
        remedy: "install a reporter advertising render_print_layout",
    },
    Refusal {
        code: "export_blocked",
        when: "the fixed layout renderer refused this capture",
        remedy: "correct its reported capture/renderer blockers",
    },
    Refusal {
        code: "print_preview_artifact_unavailable",
        when: "the common preview manifest or retained bytes fail admission",
        remedy: "preserve the file room and inspect the native refusal",
    },
    Refusal {
        code: "print_preview_artifact_expired",
        when: "the original preview lifetime ended",
        remedy: "request this intentional preview again",
    },
    Refusal {
        code: "print_preview_metadata_limit",
        when: "the retained diagnostics exceed the common metadata bound",
        remedy: "narrow the captured page; never inline page bytes",
    },
    Refusal {
        code: "reporter_engine_missing",
        when: "the installed reporter is unavailable",
        remedy: "install the native ds-report component",
    },
    Refusal {
        code: "callee_contract_mismatch",
        when: "the installed reporter task contract differs",
        remedy: "install matching CLI and reporter artifacts",
    },
    Refusal {
        code: "callee_timed_out",
        when: "the fixed local renderer exceeded its execution deadline",
        remedy: "preserve the capture and inspect the renderer failure",
    },
    Refusal {
        code: "engine_refused",
        when: "the fixed local task refused execution",
        remedy: "inspect the reporter's exact refusal and install matching artifacts",
    },
];
fn invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid("printing_invalid", message.to_string())
        .remedy("use one inline native RenderRequest and correct the named layout constraint")
}
pub(crate) fn preflight(inputs: &Inputs) -> Result<(), Failure> {
    if inputs.switch("preview") {
        if inputs
            .value("project")
            .is_none_or(|p| p.trim().is_empty() || p != p.trim() || p.len() > 128)
        {
            return Err(Failure::invalid(
                "project_required",
                "Preview requires one exact explicit project.",
            )
            .remedy("pass the exact --project and --lane for this native account"));
        }
    } else if inputs.value("preview-format").is_some()
        || inputs.value("preview-retention").is_some()
        || inputs.value("project").is_some()
    {
        return Err(invalid(
            "--preview-format, --preview-retention and --project require --preview for this local render command",
        ));
    }
    Ok(())
}
pub(crate) fn run(inputs: &Inputs) -> Result<Value, Failure> {
    preflight(inputs)?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
    let root = ds_layer_store::default_root().map_err(invalid)?;
    let room = ds_project_data::preview_room::Room {
        root: root.clone(),
        scope: ds_command_kernel::project_dataset_cache::Scope {
            principal: identity.uid().into(),
            project: project.into(),
        },
        lane: identity.lane().into(),
        credential_audience_sha256: identity.credential_audience_sha256().into(),
    };
    let path = PathBuf::from(inputs.require("request")?);
    let mut raw = Vec::new();
    std::fs::File::open(&path)
        .map_err(invalid)?
        .take(layout_preview::MAX_CAPTURE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(invalid)?;
    if raw.len() as u64 > layout_preview::MAX_CAPTURE_BYTES {
        return Err(invalid("capture exceeds its byte bound"));
    }
    let capture = serde_json::from_slice(&raw).map_err(invalid)?;
    let retention: preview_artifact::Retention = inputs
        .value("preview-retention")
        .map(|v| serde_json::from_value(json!(v)).map_err(invalid))
        .transpose()?
        .unwrap_or_default();
    let current = ds_cli_auth::headless_identity_for_named_project(lane)?;
    if current != identity {
        return Err(Failure::conflict(
            "auth_context_mismatch",
            "Native preview account changed.",
        ));
    }
    let format = inputs
        .value("preview-format")
        .map(|value| serde_json::from_value(json!(value)).map_err(invalid))
        .transpose()?
        .unwrap_or_default();
    let answer = layout_preview::execute_format(
        &crate::project::export::CliEngine,
        &room,
        &capture,
        retention,
        &root.join("preview-scratch"),
        format,
    )
    .map_err(host_failure)?;
    if ds_cli_auth::headless_identity_for_named_project(lane)? != identity {
        return Err(Failure::conflict(
            "auth_context_mismatch",
            "Native preview account changed.",
        ));
    }
    serde_json::to_value(answer).map_err(invalid)
}
fn host_failure(error: ds_report_host::HostFailure) -> Failure {
    let code = error.code.clone();
    let classified = ds_report_host::PreviewRefusal::Host(error);
    use ds_cli_contract::outcome::ExitClass;
    use ds_command_kernel::envelope::Class;
    let class = match classified.class() {
        Class::Internal => ExitClass::Internal,
        Class::InvalidInput => ExitClass::InvalidInput,
        Class::Unavailable => ExitClass::Unavailable,
        Class::Unauthorized => ExitClass::Unauthorized,
        Class::Conflict => ExitClass::Conflict,
        Class::Failed => ExitClass::Failed,
    };
    let mut failure = Failure::new(class, &code, classified.message());
    if let Some(refusal) = REFUSALS.iter().find(|r| r.code == code) {
        failure = failure.remedy(refusal.remedy);
    }
    if let Some(detail) = classified.detail() {
        failure = failure.detail(detail);
    }
    failure
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parse(tokens: &[&str]) -> Inputs {
        ds_cli_contract::args::parse(
            &crate::layout::RENDER,
            &tokens.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn preview_admission_is_declared_and_refuses_before_identity_or_input_file_reads() {
        let inputs = parse(&["--request", "unread.json", "--preview"]);
        assert_eq!(preflight(&inputs).unwrap_err().code(), "project_required");
        let inputs = parse(&["--request", "unread.json", "--preview-retention", "7d"]);
        assert_eq!(preflight(&inputs).unwrap_err().code(), "printing_invalid");
        let inputs = parse(&["--request", "unread.json", "--preview-format", "pdf"]);
        assert_eq!(preflight(&inputs).unwrap_err().code(), "printing_invalid");
        assert!(ds_cli_contract::args::parse(
            &crate::layout::RENDER,
            &[
                "--request".into(),
                "unread.json".into(),
                "--preview-format".into(),
                "png".into()
            ]
        )
        .is_err());
        let inputs = parse(&[
            "--request",
            "unread.json",
            "--preview",
            "--project",
            "p",
            "--lane",
            "canary",
            "--preview-retention",
            "7d",
        ]);
        assert!(preflight(&inputs).is_ok());
        assert_eq!(inputs.require("lane").unwrap(), "canary");
        // Existing delivery remains local and needs no native account.
        assert!(preflight(&parse(&["--request", "unread.json"])).is_ok());
    }
}
