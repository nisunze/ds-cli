use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::{bundle_digest, verify_release};
use serde_json::{Value, json};

use crate::{engine_failure, read};

pub static COMMAND: Command = Command {
    id: "library.verify",
    path: &["library", "verify"],
    contract: 1,
    summary: "Verify an immutable library release and its optional transport digest.",
    purpose: "Authenticates every declared release member, schema fingerprint and content root. If --digest is omitted, the exact supplied bytes define the transport digest; no catalogue or newest-version fallback is consulted.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("release", "<path>", "The .dsgrid-library release.").required(),
        Arg::value(
            "digest",
            "<sha256:hex>",
            "Optional exact transport digest to require.",
        ),
    ],
    output: "Verified artifact id, immutable version, content root, bundle digest, object count and engine capabilities.",
    examples: &[Example {
        command: "ds library verify --release ./library.dsgrid-library --output json",
        note: "Verify one local release without network access.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "library_verify_failed",
        when: "the container, digest, schema or content root does not verify",
        remedy: "obtain the exact immutable release named by the model/catalogue",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = inputs.require("release")?;
    let bytes = read(path)?;
    let digest = inputs
        .value("digest")
        .map(ToString::to_string)
        .unwrap_or_else(|| bundle_digest(&bytes));
    let release = verify_release(&bytes, &digest)
        .map_err(|error| engine_failure("library_verify_failed", error))?;
    Ok(json!({
        "path": path,
        "bundle_digest": digest,
        "artifact_id": release.manifest.artifact_id,
        "version": release.manifest.revision_id,
        "content_root_digest": release.manifest.content_root_digest,
        "object_count": release.manifest.objects.len(),
        "required_engine_capabilities": release.manifest.required_engine_capabilities,
        "execution_owner": "ds",
        "deterministic_completion": "every declared member, schema fingerprint, content root and requested transport digest verified",
        "pls_cadd_ui_handoff": { "required": false, "condition": "Verification never opens PLS-CADD; only a native solver/check or explicit visual acceptance requires it.", "artifact": path, "digest": digest, "post_ui_reimport": "Re-import any native-saved workspace as a new authority candidate." },
        "engineer_decision": "Engineer decides authority, applicability and certification scope; verification proves bytes, not engineering adequacy."
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "verified {}/{}\n{}",
        data["artifact_id"],
        data["version"],
        data["bundle_digest"].as_str().unwrap_or("")
    )
}
