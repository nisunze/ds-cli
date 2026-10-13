//! Thin local native-account adapter for the shared captured-page preview.
use ds_cli_contract::spec::Refusal;
use ds_cli_contract::{Failure, Inputs};
use ds_cli_desktop::{bridge, ops};
use ds_report_host::{layout_preview, preview_artifact};
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf};

pub(crate) const REFUSALS: &[Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::SIGNED_OUT,
    ops::TARGET_NOT_LIVE,
    ops::TARGET_MISMATCH,
    ops::UNKNOWN_TARGET,
    ops::PROJECT_NOT_OPEN,
    Refusal {
        code: "project_context_changed",
        when: "the paired project or published identity moved",
        remedy: "retain the capture and deliberately select the intended paired project",
    },
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
/// One closed semantic operation; callers cannot name an arbitrary command.
pub const PAIRED_OP: ops::BridgeOp = ops::BridgeOp {
    operation: "printing.layout.render",
    arguments: &["project", "request", "format", "retention"],
};
fn paired_mode(inputs: &Inputs) -> Result<bool, Failure> {
    let named = inputs
        .value("target")
        .map(str::to_owned)
        .or_else(ops::env_target);
    match named.as_deref() {
        None => Ok(inputs.value("desktop-descriptor").is_some()),
        Some(target) => match ops::host(Some(target))? {
            ops::Host::Desktop(_) => Ok(true),
            ops::Host::Server if inputs.value("desktop-descriptor").is_some() => {
                Err(invalid("server cannot use a Desktop descriptor"))
            }
            ops::Host::Server => Ok(false),
        },
    }
}
fn invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid("printing_invalid", message.to_string())
        .remedy("use one inline native RenderRequest and correct the named layout constraint")
}
pub(crate) fn preflight(inputs: &Inputs) -> Result<(), Failure> {
    if paired_mode(inputs)? && !inputs.switch("preview") {
        return Err(invalid(
            "paired captured layouts require --preview; ordinary delivery is headless",
        ));
    }
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
    if paired_mode(inputs)? {
        return run_paired(inputs);
    }
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
fn run_paired(inputs: &Inputs) -> Result<Value, Failure> {
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    let fence = bridge::IdentityFence::from_session(&bridge::session(&descriptor)?)?;
    let project = inputs.require("project")?;
    if fence.project.as_deref() != Some(project) {
        return Err(Failure::conflict(
            "project_context_changed",
            "Captured preview project differs from the paired window.",
        )
        .remedy("use the current paired project; this command never switches it"));
    }
    let mut raw = Vec::new();
    // The declared loopback bridge is bounded independently of the headless
    // capture. Reject before sending; never raise its 8 MiB metadata limit.
    let max = (8 * 1024 * 1024 - 4096).min(layout_preview::MAX_CAPTURE_BYTES);
    std::fs::File::open(inputs.require("request")?)
        .map_err(invalid)?
        .take(max + 1)
        .read_to_end(&mut raw)
        .map_err(invalid)?;
    if raw.len() as u64 > max {
        return Err(invalid("paired capture exceeds the bridge request bound"));
    }
    let capture: Value = serde_json::from_slice(&raw).map_err(invalid)?;
    let mut args = json!({"project":project,"request":capture});
    if let Some(format) = inputs.value("preview-format") {
        args["format"] = json!(format);
    }
    if let Some(retention) = inputs.value("preview-retention") {
        args["retention"] = json!(retention);
    }
    // Explicit Desktop mode adopts its published window authority. A separate
    // headless profile is not restored for this offline command. Preserve any
    // caller requirement already supplied; ops admits it against the window.
    let _guard = ops::scoped_requirement().is_none().then(|| {
        ops::scope_headless_identity(Some(ops::HeadlessIdentity {
            uid: fence.uid.clone(),
            lane: fence.lane.clone(),
            credential_audience_sha256: fence.credential_audience_sha256.clone(),
            project: Some(project.into()),
            command_authority: ds_cli_contract::spec::Authority::Project,
        }))
    });
    let result = ops::invoke(
        &descriptor,
        &PAIRED_OP,
        args,
        std::time::Duration::from_secs(30 * 60),
    )
    .map_err(ops::classify_signed_out)?;
    if bridge::IdentityFence::from_session(&bridge::session(&descriptor)?)? != fence {
        return Err(Failure::conflict(
            "project_context_changed",
            "Paired identity changed during captured preview.",
        )
        .remedy("retain the capture and consume its result under the original paired identity"));
    }
    let answer: layout_preview::Answer = serde_json::from_value(result.clone()).map_err(|_| {
        Failure::failed(
            "desktop_unreadable",
            "Paired captured preview lacks its closed native file manifest.",
        )
        .remedy("install matching CLI/Desktop; no inline page bytes are accepted")
    })?;
    if answer.project != project
        || answer.lane != fence.lane
        || answer.layout_id != capture["render"]["layout"]["id"].as_str().unwrap_or("")
        || answer.page.content_type
            != if inputs.value("preview-format") == Some("pdf") {
                "application/pdf"
            } else {
                "image/svg+xml"
            }
    {
        return Err(Failure::failed(
            "desktop_unreadable",
            "Paired preview differs from captured project, layout or media.",
        )
        .remedy("retain the capture and install matching CLI/Desktop"));
    }
    let room = ds_project_data::preview_room::Room {
        root: ds_layer_store::default_root().map_err(invalid)?,
        scope: ds_command_kernel::project_dataset_cache::Scope {
            principal: fence.uid,
            project: project.into(),
        },
        lane: fence.lane,
        credential_audience_sha256: fence.credential_audience_sha256,
    };
    // Same native reader verifies the exact scoped file/digest/expiry. Bytes
    // are consumed here only, and never enter the CLI or MCP response.
    preview_artifact::read(&room, &answer.page).map_err(host_failure)?;
    Ok(result)
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
        assert!(
            ds_cli_contract::args::parse(
                &crate::layout::RENDER,
                &[
                    "--request".into(),
                    "unread.json".into(),
                    "--preview-format".into(),
                    "png".into()
                ]
            )
            .is_err()
        );
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
    #[test]
    fn paired_mode_is_explicit_and_has_one_closed_operation_without_headless_profile_fallback() {
        let inputs = parse(&[
            "--request",
            "unread.json",
            "--preview",
            "--project",
            "project",
            "--target",
            "desktop",
        ]);
        assert!(preflight(&inputs).is_ok());
        assert!(paired_mode(&inputs).unwrap());
        assert!(
            !paired_mode(&parse(&[
                "--request",
                "unread.json",
                "--preview",
                "--project",
                "project",
                "--target",
                "server"
            ]))
            .unwrap()
        );
        assert_eq!(
            preflight(&parse(&["--request", "unread.json", "--target", "desktop"]))
                .unwrap_err()
                .code(),
            "printing_invalid"
        );
        assert_eq!(
            preflight(&parse(&[
                "--request",
                "unread.json",
                "--preview",
                "--project",
                "project",
                "--target",
                "server",
                "--desktop-descriptor",
                "unused"
            ]))
            .unwrap_err()
            .code(),
            "printing_invalid"
        );
        assert_eq!(
            paired_mode(&parse(&["--request", "unread.json", "--target", "unknown"]))
                .unwrap_err()
                .code(),
            "unknown_target"
        );
        assert_eq!(PAIRED_OP.operation, "printing.layout.render");
        assert!(ops::undeclared_key(&PAIRED_OP,&json!({"project":"project","request":{"source":"original"},"format":"pdf","retention":"7d"})).is_none());
        for key in [
            "command",
            "force",
            "seed",
            "model",
            "transformer",
            "uid",
            "path",
        ] {
            let mut bad = json!({});
            bad[key] = json!("never-consumed");
            assert_eq!(ops::undeclared_key(&PAIRED_OP, &bad).as_deref(), Some(key));
        }
    }
}
