//! `ds design lv project-export` — one governed snapshot to one native request.

use std::path::PathBuf;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network::network::native_fast_lv::{
    NativeFastLvError, encode_native_fast_lv_request_from_layers,
};
use serde_json::{Value, json};

use super::artifact::{PROJECT_REQUEST, ensure_absent, sha256, write_new};

const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<name>",
    "One exact transformer in the selected headless project.",
)
.required();
const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "Absent path for one ds.fast-lv.request/v1 document.",
)
.required();
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

macro_rules! refusal {
    ($code:literal, $when:literal, $remedy:literal) => {
        Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        }
    };
}

pub static COMMAND: Command = Command {
    id: "design.lv.project-export",
    path: &["design", "lv", "project-export"],
    contract: 1,
    summary: "Export one governed transformer as a native Fast LV request.",
    purpose: "Restores the native user, reads one exact transformer from the audience-fenced selected project through the fixed context call, requires server-supplied revision and content-digest fences, and writes one validated ds.fast-lv.request/v1 document to an absent path. ds-network—not the CLI—maps the authoritative layers to its explicit owner-default settings and empty project config. This is a safe mapless handoff into `ds design lv process`, not a claim of Desktop preset or project-config parity. No Desktop descriptor, project override, arbitrary request, browser store, or processing-lane value is accepted.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER, OUT, LANE],
    output: "The absent output path, request SHA-256 and byte count; selected lane/project/transformer; exact server version and content digest; layer/job counts; and explicit owner-default-settings/no-project-config handoff state. No layer payload is printed.",
    examples: &[Example {
        command: "ds design lv project-export --transformer T-1042 --out ./T-1042.fast-lv.json --output json",
        note: "Create a fenced one-transformer request for later native processing.",
        runnable: false,
    }],
    refusals: &[
        refusal!(
            "native_profile_not_configured",
            "the exact packaged native profile is unavailable",
            "install one complete ds release"
        ),
        refusal!(
            "native_profile_digest_mismatch",
            "the packaged catalog differs from the build pin",
            "reinstall one complete ds release"
        ),
        refusal!(
            "native_profile_unsafe",
            "the packaged native catalog is unsafe or malformed",
            "reinstall one complete ds release"
        ),
        refusal!(
            "headless_signed_out",
            "the lane has no restorable native user",
            "run ds auth login --email <address>"
        ),
        refusal!(
            "headless_project_not_selected",
            "the restored user has no audience-fenced project selection",
            "run ds auth project use --project <exact-id>"
        ),
        refusal!(
            "project_context_stale",
            "the context belongs to another user, lane, or audience",
            "select the project again with ds auth project use"
        ),
        refusal!(
            "native_state_unsafe",
            "protected native state is unsafe or unreadable",
            "repair the owner-only DS config directory"
        ),
        refusal!(
            "native_state_unavailable",
            "protected native state cannot be accessed",
            "repair the owner-only DS config directory"
        ),
        refusal!(
            "native_state_protection_unavailable",
            "this build has no protected-state adapter",
            "install a supported native ds build"
        ),
        refusal!(
            "native_state_root_invalid",
            "the configured state root is not absolute",
            "unset it or provide an absolute path"
        ),
        refusal!(
            "native_state_conflict",
            "another native operation holds the state lease",
            "retry after that operation finishes"
        ),
        refusal!(
            "native_cleanup_required",
            "revoked identity cleanup could not clear context",
            "repair protected state and run auth logout"
        ),
        refusal!(
            "auth_input_invalid",
            "the transformer or bound identity input is invalid",
            "pass one exact bounded transformer name"
        ),
        refusal!(
            "auth_rejected",
            "the fixed gateway rejects the verified request",
            "verify the account and its project access"
        ),
        refusal!(
            "auth_revoked",
            "Firebase permanently revoked the session",
            "sign in again interactively"
        ),
        refusal!(
            "auth_identity_mismatch",
            "Firebase returned another identity",
            "sign in again and report a repeated mismatch"
        ),
        refusal!(
            "auth_transient",
            "the fixed native service is temporarily unavailable",
            "retry without changing local state"
        ),
        refusal!(
            "auth_response_unreadable",
            "the transformer reply violates its closed contract",
            "retry once, then update ds if it persists"
        ),
        refusal!(
            "transformer_not_found",
            "the transformer does not exist in the selected project",
            "pass one exact transformer name from that project"
        ),
        refusal!(
            "transformer_context_unfenced",
            "the server omitted either the snapshot version or content digest",
            "refresh or migrate the transformer until the context call returns both fences"
        ),
        refusal!(
            "fast_lv_input_too_large",
            "the generated request exceeds 64 MiB",
            "use an owner-supported narrower project snapshot"
        ),
        refusal!(
            "fast_lv_input_invalid",
            "the governed layers cannot form the closed native request",
            "update ds and report the authoritative transformer context"
        ),
        refusal!(
            "fast_lv_bound_refused",
            "the transformer context exceeds a native name, layer, or feature bound",
            "narrow or repair the authoritative transformer context"
        ),
        refusal!(
            "fast_lv_request_output_exists",
            "--out already exists",
            "choose a new request path; this command never overwrites"
        ),
        refusal!(
            "fast_lv_request_output_write_failed",
            "the request cannot be durably written at --out",
            "choose a writable absent path and retry the unchanged transformer export"
        ),
    ],
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let output_path = PathBuf::from(inputs.require("out")?);
    // Do not rotate credentials or call the project service when this local
    // precondition already makes the requested effect impossible.
    ensure_absent(&output_path, &PROJECT_REQUEST)?;

    let transformer = inputs.require("transformer")?;
    let headless = ds_cli_auth::transformer_context(inputs.require("lane")?, transformer)?;
    let snapshot = headless.snapshot();
    let (Some(version), Some(content_digest)) = (
        snapshot.metadata().version(),
        snapshot.metadata().content_digest(),
    ) else {
        return Err(Failure::conflict(
            "transformer_context_unfenced",
            "The transformer context has no complete server version/content-digest fence.",
        )
        .remedy("Refresh or migrate the transformer until the context call returns both fences."));
    };

    let request =
        encode_native_fast_lv_request_from_layers(snapshot.transformer_name(), snapshot.layers())
            .map_err(map_owner_error)?;
    let request_sha256 = sha256(&request);
    write_new(&output_path, &request, &PROJECT_REQUEST)?;

    Ok(json!({
        "out": output_path,
        "request_sha256": request_sha256,
        "byte_count": request.len(),
        "lane": headless.lane(),
        "project": {
            "ds_project": snapshot.ds_project(),
            "project_name": headless.project_name(),
            "status": headless.project_status(),
        },
        "transformer": snapshot.transformer_name(),
        "source": {
            "state": "fenced",
            "version": version,
            "content_digest": content_digest,
        },
        "jobs": 1,
        "layers": snapshot.layers().len(),
        "process_settings": "ds-network-owner-defaults",
        "project_config": "not-included",
    }))
}

fn map_owner_error(error: NativeFastLvError) -> Failure {
    let code = error.code();
    match code {
        "fast_lv_input_too_large" => Failure::invalid(code, error.to_string())
            .remedy("Use an owner-supported narrower project snapshot."),
        "fast_lv_input_invalid" => Failure::failed(code, error.to_string())
            .remedy("Update ds and report the authoritative transformer context."),
        "fast_lv_bound_refused" => Failure::invalid(code, error.to_string())
            .remedy("Narrow or repair the authoritative transformer context."),
        _ => Failure::internal("fast_lv_input_invalid", error.to_string()),
    }
}

pub fn render(value: &Value) -> String {
    format!(
        "Fast LV request exported for {} ({} layer(s), owner defaults, no project config).\nRequest: {}\nSHA-256: {}",
        value["transformer"].as_str().unwrap_or(""),
        value["layers"].as_u64().unwrap_or(0),
        value["out"].as_str().unwrap_or(""),
        value["request_sha256"].as_str().unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn layers() -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                "tr".to_owned(),
                json!({ "type": "FeatureCollection", "features": [] }),
            ),
            (
                "customers".to_owned(),
                json!({ "type": "FeatureCollection", "features": [] }),
            ),
        ])
    }

    #[test]
    fn contract_is_one_fenced_project_read_and_local_file_write() {
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert_eq!(COMMAND.path, ["design", "lv", "project-export"]);
        assert!(COMMAND.args.iter().all(|arg| arg.name != "project"));
        assert!(
            COMMAND
                .args
                .iter()
                .all(|arg| arg.name != "desktop-descriptor")
        );
    }

    #[test]
    fn owner_builds_the_request_and_the_cli_does_not_shape_engine_fields() {
        let bytes = encode_native_fast_lv_request_from_layers("T-1", &layers()).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(request["schema"], "ds.fast-lv.request/v1");
        assert_eq!(request["jobs"][0]["config_dfs"], json!({}));
        assert_eq!(request["jobs"][0]["settings"]["delete_strange_cols"], true);
        assert!(request["jobs"][0].get("ds_project").is_none());
    }

    #[test]
    fn request_artifacts_are_create_new_and_renderer_is_bounded() {
        let root = std::env::temp_dir().join(format!(
            "ds-project-export-{}-{}",
            std::process::id(),
            sha256(b"project-export-test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("request.json");
        write_new(&path, b"{}", &PROJECT_REQUEST).unwrap();
        assert_eq!(
            ensure_absent(&path, &PROJECT_REQUEST).unwrap_err().code(),
            "fast_lv_request_output_exists"
        );

        let text = render(&json!({
            "transformer": "T-1",
            "layers": 2,
            "out": path,
            "request_sha256": "abc",
            "secret_layers": { "customers": [1, 2, 3] },
        }));
        assert!(text.contains("owner defaults"));
        assert!(!text.contains("customers"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_output_symlink_is_existing_operator_state() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "ds-project-export-symlink-{}-{}",
            std::process::id(),
            sha256(b"project-export-symlink-test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("request.json");
        symlink(root.join("absent-target"), &path).unwrap();

        assert_eq!(
            ensure_absent(&path, &PROJECT_REQUEST).unwrap_err().code(),
            "fast_lv_request_output_exists"
        );
        assert!(std::fs::symlink_metadata(&path).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }
}
