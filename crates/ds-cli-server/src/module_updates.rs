//! Thin CLI/MCP effects over the native module updater and its opaque grant.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::module_release::ModuleComponent;
use ds_update_runtime::modules::ModuleStore;
use serde_json::Value;

const COMPONENT: Arg = Arg::value(
    "component",
    "<solar|reporter|network>",
    "Exact fixed native addon owner.",
)
.choices(&["solar", "reporter", "network"])
.required();

const SCHEMA_INVALID: Refusal = Refusal {
    code: "module_release_schema_invalid",
    when: "module metadata names an unsupported schema",
    remedy: "obtain the fixed owner's supported module catalog schema",
};
const IDENTITY_MISMATCH: Refusal = Refusal {
    code: "module_release_identity_mismatch",
    when: "module metadata crosses component, lane or product identity",
    remedy: "retain the selected lane and obtain its exact owner publication",
};
const PROTOCOL_UNSUPPORTED: Refusal = Refusal {
    code: "module_release_protocol_unsupported",
    when: "module metadata names an unsupported owner protocol",
    remedy: "obtain a release using the fixed owner's supported native protocol",
};
const TARGET_UNSUPPORTED: Refusal = Refusal {
    code: "module_release_target_unsupported",
    when: "module metadata names an unsupported or repeated platform target",
    remedy: "obtain a qualified Linux or Windows x86_64 owner publication",
};
const ASSET_INVALID: Refusal = Refusal {
    code: "module_release_asset_invalid",
    when: "module metadata has invalid executable identity or bounds",
    remedy: "obtain a corrected qualified module executable publication",
};
const PROVENANCE_INVALID: Refusal = Refusal {
    code: "module_release_provenance_invalid",
    when: "module metadata has incomplete or crossed producer source closure",
    remedy: "obtain the owner's complete qualified producer provenance",
};
const URL_INVALID: Refusal = Refusal {
    code: "module_release_url_invalid",
    when: "module metadata names a mutable or unadmitted artifact URL",
    remedy: "obtain an immutable public HTTPS owner artifact publication",
};
const CHANNEL: Arg = Arg::value(
    "channel",
    "<stable|canary>",
    "Exact installed release lane.",
)
.choices(&["stable", "canary"])
.required();

pub static STAGE: Command = Command {
    id: "core.module-stage",
    path: &["core", "module-stage"],
    contract: 1,
    summary: "Stage one exact published native addon without activating it.",
    purpose: "Capture the packaged production lane's release authority, intentionally read its fixed public catalog once, and require the exact selected native asset SHA-256. The shared native updater performs bounded verified transfer and one fixed build-info compatibility probe, then retains an immutable executable and private ready receipt. Staging changes no active selection and acquires no project data. Held JSON, development catalogs, caller URLs and paths grant no installation trust. Requires --yes; never exits or activates the application.",
    chapter: Chapter::Design,
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        COMPONENT,
        CHANNEL,
        Arg::value(
            "sha256",
            "<sha256>",
            "Exact lowercase SHA-256 of the selected published native executable.",
        )
        .required(),
    ],
    output: "Native ready receipt with state, receiptId, component, channel, version and sha256. Retains staged bytes only; active/previous selection and captured jobs are unchanged. Progress is diagnostic stderr, separate from the result envelope.",
    examples: &[],
    refusals: &[
        SCHEMA_INVALID,
        IDENTITY_MISMATCH,
        PROTOCOL_UNSUPPORTED,
        TARGET_UNSUPPORTED,
        ASSET_INVALID,
        PROVENANCE_INVALID,
        URL_INVALID,
        Refusal {
            code: "module_release_invalid",
            when: "the selected digest or authority metadata is malformed, excessive or crossed",
            remedy: "select the exact component, lane and lowercase asset SHA-256",
        },
        Refusal {
            code: "module_authority_unavailable",
            when: "the native profile is a development catalog or local-Go selection",
            remedy: "use the exact packaged production native profile for this installed lane",
        },
        Refusal {
            code: "native_profile_not_configured",
            when: "this binary has no complete packaged native client catalog",
            remedy: "install a complete release with its digest-pinned native client profiles",
        },
        Refusal {
            code: "native_profile_unsafe",
            when: "the packaged native catalog is missing, unsafe, excessive or malformed",
            remedy: "reinstall one complete release with its native client profiles",
        },
        Refusal {
            code: "native_profile_digest_mismatch",
            when: "the packaged native catalog bytes differ from this binary's pin",
            remedy: "reinstall one complete release with its exact native client profiles",
        },
        Refusal {
            code: "module_release_unavailable",
            when: "the one intentional authority read fails",
            remedy: "retain the installed unit and retry only after connectivity or authority is restored",
        },
        Refusal {
            code: "module_release_unpublished",
            when: "the selected module has no public release",
            remedy: "retain the installed unit until its owner publishes a qualified release",
        },
        Refusal {
            code: "module_release_inactive",
            when: "the release is withdrawn, stopped or blocked",
            remedy: "retain the installed unit and select an active qualified release",
        },
        Refusal {
            code: "module_release_changed",
            when: "the public native asset differs from the explicitly selected digest",
            remedy: "refresh the catalog explicitly before choosing the exact asset again",
        },
        Refusal {
            code: "module_platform_unsupported",
            when: "the release has no admitted native asset for this host",
            remedy: "use a supported Linux or Windows x86_64 module asset",
        },
        Refusal {
            code: "module_update_busy",
            when: "another native update owns this component and lane",
            remedy: "wait for that component's current native update to finish",
        },
        Refusal {
            code: "module_store_invalid",
            when: "the native store is linked, reparsed, unavailable or unsafe",
            remedy: "retain the current selection and repair its native owned store",
        },
        Refusal {
            code: "module_receipt_invalid",
            when: "retained native receipt facts are corrupt or crossed",
            remedy: "retain the current selection and repair its exact native installation",
        },
        Refusal {
            code: "module_engine_incompatible",
            when: "verified bytes fail the fixed identity or required native compatibility check",
            remedy: "retain the current selection and obtain a qualified owner executable",
        },
        Refusal {
            code: "module_stage_failed",
            when: "verified transfer or durable staging cannot complete",
            remedy: "retain the current selection and resolve the named native staging failure",
        },
        Refusal {
            code: "module_update_cancelled",
            when: "native cancellation interrupts staging",
            remedy: "retain the current selection and start a new explicit stage when ready",
        },
    ],
    reference: Some("docs/reference/core.md"),
    search: &["addons", "update", "install", "ready"],
    requires: Requires::Server,
    availability: native_module_availability,
};

pub(crate) fn native_module_availability() -> Availability {
    match ds_update_runtime::modules::current_platform() {
        Ok(_) => Availability::Available,
        Err(reason) => Availability::unavailable(
            "module_platform_unsupported",
            reason,
            "use an admitted Linux or Windows x86_64 native host",
        ),
    }
}

fn component(inputs: &Inputs) -> Result<ModuleComponent, Failure> {
    serde_json::from_value(serde_json::json!(inputs.require("component")?)).map_err(|_| {
        Failure::invalid(
            "module_component_invalid",
            "select exactly solar, reporter or network",
        )
    })
}

fn native_failure(message: String) -> Failure {
    native_failure_or(message, "module_stage_failed")
}

fn native_failure_or(message: String, fallback: &str) -> Failure {
    let code = message.split(':').next().unwrap_or_default();
    let declared = STAGE.refusals.iter().any(|refusal| refusal.code == code);
    let code = if declared { code } else { fallback };
    let bounded: String = message.chars().take(1024).collect();
    let failure = match code {
        "module_release_invalid"
        | "module_release_schema_invalid"
        | "module_release_identity_mismatch"
        | "module_release_protocol_unsupported"
        | "module_release_target_unsupported"
        | "module_release_asset_invalid"
        | "module_release_provenance_invalid"
        | "module_release_url_invalid" => Failure::invalid(code, bounded),
        "module_release_changed" | "module_update_busy" => Failure::conflict(code, bounded),
        "module_authority_unavailable"
        | "module_release_unavailable"
        | "module_release_unpublished"
        | "module_release_inactive"
        | "module_platform_unsupported" => Failure::unavailable(code, bounded),
        _ => Failure::failed(code, bounded),
    };
    failure.remedy(
        STAGE
            .refusals
            .iter()
            .find(|refusal| refusal.code == code)
            .expect("declared native failure")
            .remedy,
    )
}

pub fn stage(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_update_runtime::modules::current_platform().map_err(native_failure)?;
    let component = component(inputs)?;
    let lane = ds_cli_auth::Lane::parse(inputs.require("channel")?)?;
    let sha256 = inputs.require("sha256")?;
    if !ds_cli_contract::util::is_sha256_hex(sha256, ds_cli_contract::util::HexCase::Lower) {
        return Err(native_failure(
            "module_release_invalid: name the exact selected lowercase asset SHA-256".into(),
        ));
    }
    let cancelled = || false;
    let authority = ds_cli_auth::module_updates::capture_release_authority(lane)?;
    let grant = authority
        .acquire(component, sha256, &cancelled)
        .map_err(native_failure)?;
    let store = ModuleStore::managed(component, lane.token()).map_err(native_failure)?;
    let ready = store
        .stage(
            grant,
            |progress| {
                ds_cli_contract::output::note(&format!(
                    "{}: {} {}% ({}/{})",
                    component.as_str(),
                    progress.status,
                    progress.progress,
                    progress.bytes_downloaded,
                    progress.bytes_total,
                ))
            },
            &cancelled,
        )
        .map_err(native_failure)?;
    serde_json::to_value(ready)
        .map_err(|error| Failure::internal("module_result_invalid", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_components::SELECTION;
    use ds_cli_contract::{Format, Output};

    #[test]
    fn stage_has_closed_confirmed_inputs_and_selection_has_no_effect() {
        assert!(STAGE.requires_confirmation());
        assert!(!SELECTION.requires_confirmation());
        assert_eq!(STAGE.authority, Authority::None);
        assert_eq!(SELECTION.effect, Effect::ReadOnly);
        assert_eq!(
            STAGE.args.iter().map(|arg| arg.name).collect::<Vec<_>>(),
            ["component", "channel", "sha256"]
        );
        assert_eq!(
            SELECTION
                .args
                .iter()
                .map(|arg| arg.name)
                .collect::<Vec<_>>(),
            ["component", "channel"]
        );
        for command in [&STAGE, &SELECTION] {
            for forbidden in ["file", "url", "path", "probe", "authority", "idle", "dirty"] {
                assert!(!command.args.iter().any(|arg| arg.name == forbidden));
            }
        }
    }

    #[test]
    fn invalid_digest_refuses_before_capturing_authority_or_creating_store() {
        let context = Context {
            confirmed: true,
            output: Output::resolve(Format::Json, false, true),
        };
        for digest in ["short".to_owned(), "A".repeat(64), "g".repeat(64)] {
            let args = [
                "--component",
                "network",
                "--channel",
                "canary",
                "--sha256",
                &digest,
            ]
            .map(str::to_owned);
            let inputs = ds_cli_contract::parse(&STAGE, &args).unwrap();
            let failure = stage(&inputs, &context).unwrap_err();
            assert_eq!(failure.code(), "module_release_invalid");
        }
    }

    #[test]
    fn native_refusal_codes_stay_declared_and_messages_bounded() {
        let refusal = native_failure("module_release_changed: exact asset changed".into());
        assert_eq!(refusal.code(), "module_release_changed");
        let unknown = native_failure("x".repeat(4096));
        assert_eq!(unknown.code(), "module_stage_failed");
        assert_eq!(unknown.message().chars().count(), 1024);
        for refusal in [
            SCHEMA_INVALID,
            IDENTITY_MISMATCH,
            PROTOCOL_UNSUPPORTED,
            TARGET_UNSUPPORTED,
            ASSET_INVALID,
            PROVENANCE_INVALID,
            URL_INVALID,
        ] {
            assert!(STAGE.refusals.iter().any(|item| item.code == refusal.code));
            assert!(
                SELECTION
                    .refusals
                    .iter()
                    .any(|item| item.code == refusal.code)
            );
            assert_eq!(
                native_failure(format!("{}: owner refusal", refusal.code)).code(),
                refusal.code
            );
            assert_eq!(
                native_failure_or(
                    format!("{}: owner refusal", refusal.code),
                    "module_store_invalid"
                )
                .code(),
                refusal.code
            );
        }
    }
}
