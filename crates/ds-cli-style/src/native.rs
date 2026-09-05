use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::BridgeOp;
use serde_json::{Value, json};
pub const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);
pub const STYLE_REFUSED: Refusal = Refusal {
    code: "style_refused",
    when: "the guided style instruction violates the backend document contract",
    remedy: "inspect ds style read for the exact fields, channels, and property bounds",
};
macro_rules! native_refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        pub const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}

native_refusal!(
    NATIVE_PROFILE,
    "native_profile_not_configured",
    "the exact packaged native profile is unavailable",
    "install one complete ds release"
);
native_refusal!(
    NATIVE_PROFILE_DIGEST,
    "native_profile_digest_mismatch",
    "the packaged catalogue differs from the build pin",
    "reinstall one complete ds release"
);
native_refusal!(
    NATIVE_PROFILE_UNSAFE,
    "native_profile_unsafe",
    "the packaged native catalogue is unsafe or malformed",
    "reinstall one complete ds release"
);
native_refusal!(
    HEADLESS_SIGNED_OUT,
    "headless_signed_out",
    "the selected lane has no restorable native user",
    "run ds auth login --email <address>"
);
native_refusal!(
    HEADLESS_NO_PROJECT,
    "headless_project_not_selected",
    "the user has no audience-fenced selected project",
    "run ds auth project use --project <exact-id>"
);
native_refusal!(
    PROJECT_CONTEXT_STALE,
    "project_context_stale",
    "the saved project belongs to another identity, lane, or audience",
    "select the project again with ds auth project use"
);
native_refusal!(
    PROJECT_CONTEXT_CHANGED,
    "project_context_changed",
    "the selected project changed between the status and preflight reads",
    "run ds tile plan again against the current selected project"
);
native_refusal!(
    NATIVE_STATE_UNSAFE,
    "native_state_unsafe",
    "protected native state is unsafe or unreadable",
    "repair the owner-only DS config directory"
);
native_refusal!(
    NATIVE_STATE_UNAVAILABLE,
    "native_state_unavailable",
    "protected native state cannot be accessed",
    "repair the owner-only DS config directory"
);
native_refusal!(
    NATIVE_STATE_PROTECTION,
    "native_state_protection_unavailable",
    "this build has no protected-state adapter",
    "install a supported native ds build"
);
native_refusal!(
    NATIVE_STATE_ROOT,
    "native_state_root_invalid",
    "the configured state root is not absolute",
    "unset it or provide an absolute path"
);
native_refusal!(
    NATIVE_STATE_CONFLICT,
    "native_state_conflict",
    "another native operation holds the state lease",
    "retry after that operation finishes"
);
native_refusal!(
    NATIVE_CLEANUP,
    "native_cleanup_required",
    "revoked identity cleanup could not clear context",
    "repair protected state and run auth logout"
);
native_refusal!(
    AUTH_CONTEXT_MISMATCH,
    "auth_context_mismatch",
    "the protected native providers disagree on identity or selected project",
    "sign out or revoke the unintended provider before retrying"
);
native_refusal!(
    AUTH_INPUT,
    "auth_input_invalid",
    "the selected project identity violates the fixed request bound",
    "select a freshly visible project again"
);
native_refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the fixed gateway rejects the verified request",
    "verify the account and its selected-project access"
);
native_refusal!(
    AUTH_REVOKED,
    "auth_revoked",
    "the native session was permanently revoked",
    "sign in again interactively"
);
native_refusal!(
    AUTH_IDENTITY_MISMATCH,
    "auth_identity_mismatch",
    "the restored identity differs from the bound native session",
    "sign in again and report a repeated mismatch"
);
native_refusal!(
    AUTH_TRANSIENT,
    "auth_transient",
    "the fixed native service is temporarily unavailable",
    "retry without changing local state"
);
native_refusal!(
    AUTH_UNREADABLE,
    "auth_response_unreadable",
    "the style response violates its closed bounded contract",
    "retry once, then update ds if it persists"
);

pub const REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
    HEADLESS_NO_PROJECT,
    PROJECT_CONTEXT_STALE,
    NATIVE_STATE_UNSAFE,
    NATIVE_STATE_UNAVAILABLE,
    NATIVE_STATE_PROTECTION,
    NATIVE_STATE_ROOT,
    NATIVE_STATE_CONFLICT,
    NATIVE_CLEANUP,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    AUTH_REJECTED,
    AUTH_REVOKED,
    AUTH_IDENTITY_MISMATCH,
    AUTH_TRANSIENT,
    AUTH_UNREADABLE,
    crate::INVALID_NUMBER,
    crate::INVALID_VALUE_SPEC,
    crate::INVALID_COLOR,
    crate::INVALID_APPEARANCE,
    crate::INVALID_CARTOGRAPHY,
    STYLE_REFUSED,
    crate::CONFIRMATION_REQUIRED,
];

pub fn execute(
    inputs: &Inputs,
    _context: &Context,
    operation: &BridgeOp,
    mut args: Value,
) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    if operation.operation == "style.list" || operation.operation == "style.read" {
        let snapshot = ds_cli_auth::layer_config(lane, false)?;
        let result = if operation.operation == "style.list" {
            ds_command_kernel::style_plan::list_styles(
                snapshot.result().document(),
                args["query"].as_str(),
                args["limit"].as_u64().unwrap_or(100) as usize,
            )
        } else {
            ds_command_kernel::style_plan::describe_style(
                snapshot.result().document(),
                inputs.require("ref")?,
            )
        };
        return result
            .map(|mut data| {
                data["lane"] = json!(snapshot.lane());
                data
            })
            .map_err(refused);
    }
    let reference = inputs.require("ref")?;
    let apply = args["apply"].as_bool().unwrap_or(false);
    let instruction = match operation.operation {
        "style.appearance.set" => ds_cli_auth::StyleInstruction::Appearance {
            color: args["color"].as_str().map(str::to_owned),
            icon: args["icon"].as_str().map(str::to_owned),
            size: args["size"].as_f64(),
        },
        "style.dimension.set" => ds_cli_auth::StyleInstruction::Dimension {
            field: args["field"].as_str().unwrap_or_default().to_owned(),
            channel: args["channel"].as_str().unwrap_or("halo").to_owned(),
            values: serde_json::from_value(args["values"].clone())
                .map_err(|_| refused("invalid dimension values"))?,
            other: args["other"].as_f64(),
            color: args["color"].as_str().map(str::to_owned),
            field_type: inputs.value("field-type").map(str::to_owned),
        },
        "style.dimension.clear" => ds_cli_auth::StyleInstruction::ClearDimension,
        "style.cartography.set" => {
            let obj = args
                .as_object_mut()
                .ok_or_else(|| refused("invalid cartography arguments"))?;
            obj.remove("ref");
            obj.remove("apply");
            ds_cli_auth::StyleInstruction::Cartography {
                change: serde_json::from_value(args)
                    .map_err(|_| refused("invalid cartography arguments"))?,
            }
        }
        _ => return Err(refused("unsupported guided style operation")),
    };
    let receipt = ds_cli_auth::style_edit(lane, reference, &instruction, apply)?;
    let mut data = receipt.result().data().clone();
    data["lane"] = json!(receipt.lane());
    Ok(data)
}
fn refused(message: impl Into<String>) -> Failure {
    Failure::invalid("style_refused", message).remedy(STYLE_REFUSED.remedy)
}
