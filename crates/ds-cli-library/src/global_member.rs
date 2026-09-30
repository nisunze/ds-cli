use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

const MAX_METADATA_BYTES: usize = 8192;
const NATIVE: &[Refusal] = ds_cli_auth::PROJECT_LIST_COMMAND.refusals;
const MEMBER: &[Refusal] = &ds_cli_auth::grid_library_member::REFUSALS;
const BOUND: Refusal = Refusal {
    code: "catalog_member_metadata_too_large",
    when: "one resolved member's metadata exceeds the 8 KiB projection bound",
    remedy: "report the oversized inventory record through ds feedback submit; use a release with bounded metadata",
};
const fn refusals() -> [Refusal; NATIVE.len() + MEMBER.len() + 1] {
    let mut all = [BOUND; NATIVE.len() + MEMBER.len() + 1];
    let mut i = 0;
    while i < NATIVE.len() {
        all[i] = NATIVE[i];
        i += 1;
    }
    let mut j = 0;
    while j < MEMBER.len() {
        all[i + j] = MEMBER[j];
        j += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "library.global.resolve-member",
    path: &["library", "global", "resolve-member"],
    contract: 1,
    summary: "Read bounded metadata for one exact governed global library member.",
    purpose: "Require explicit library/release ids, the case-sensitive inventory relative path and expected SHA-256. The native gateway authorizes the restored user/device in the selected lane and verifies the stored object before resolving the pin. Discover pins in library global read's exact release assets inventory. No head fallback, project selection, member byte preview/download, local file write or solver approval is provided by this metadata read.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("library-id", "<id>", "Exact governed global library id.").required(),
        Arg::value(
            "release-id",
            "<id>",
            "Exact immutable release id; never defaults to the head.",
        )
        .required(),
        Arg::value(
            "relative-path",
            "<path>",
            "Exact case-sensitive path from release.assets; not a local file or object URL.",
        )
        .required(),
        Arg::value(
            "expected-digest",
            "<sha256>",
            "Exact inventory artifact.digest: 64 lowercase hexadecimal characters.",
        )
        .required(),
        Arg::value(
            "lane",
            "<stable|canary>",
            "Native authentication lane and account/device context.",
        )
        .default("stable")
        .choices(&["stable", "canary"]),
    ],
    output: "One exact library_id/release_id/member metadata record (at most 8 KiB), including artifact digest and byte_length, provenance and optional external_definition. Signed delivery URLs are excluded; bytes_fetched=false. The server verified object integrity; this call does not verify locally downloaded bytes.",
    examples: &[Example {
        command: "ds library global resolve-member --library-id library_1 --release-id release_1 --relative-path pls-cadd/criteria/Base.CRI --expected-digest aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --lane stable --output json",
        note: "Replace the illustrative pin with one exact authorized release.assets record.",
        runnable: false,
    }],
    refusals: &refusals(),
    reference: Some("docs/reference/library.md"),
    search: &["criteria", "structures"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let resolved = ds_cli_auth::grid_library_member::resolve(
        inputs.require("lane")?,
        inputs.require("library-id")?,
        inputs.require("release-id")?,
        inputs.require("relative-path")?,
        inputs.require("expected-digest")?,
    )?;
    project_metadata(resolved)
}

fn project_metadata(resolved: Value) -> Result<Value, Failure> {
    // Core already checked the response and all four selection pins. Return the
    // inventory evidence verbatim, without spending or exposing its signed URL.
    let projected = json!({
        "library_id": resolved["library_id"],
        "release_id": resolved["release_id"],
        "member": resolved["member"],
        "bytes_fetched": false,
    });
    if serde_json::to_vec(&projected)
        .expect("JSON value serializes")
        .len()
        > MAX_METADATA_BYTES
    {
        return Err(Failure::unavailable(
            "catalog_member_metadata_too_large",
            "the resolved member metadata exceeds 8 KiB",
        )
        .remedy(BOUND.remedy));
    }
    Ok(projected)
}

pub fn render(data: &Value) -> String {
    format!(
        "{}/{} {} — {} bytes, SHA-256 {} (metadata only)",
        data["library_id"].as_str().unwrap_or_default(),
        data["release_id"].as_str().unwrap_or_default(),
        data["member"]["relative_path"].as_str().unwrap_or_default(),
        data["member"]["artifact"]["byte_length"],
        data["member"]["artifact"]["digest"]
            .as_str()
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_keeps_exact_inventory_evidence_and_excludes_delivery_credentials() {
        let member = json!({"relative_path": "pls-cadd/criteria/Base.CRI", "class": "criteria",
            "artifact": {"digest": "a".repeat(64), "byte_length": 17},
            "provenance": {"kind": "source_project", "reference": "approved-source"},
            "external_definition": {"system": "pls-cadd", "definition_id": "Base"}});
        let result = project_metadata(json!({"library_id": "library_1", "release_id": "release_1",
            "member": member, "download_url": "signed-secret", "expires_at": "unused"}))
        .unwrap();
        assert_eq!(result["member"], member);
        assert_eq!(result["bytes_fetched"], false);
        assert!(result.get("download_url").is_none());
        assert!(render(&result).contains("17 bytes"));
    }

    #[test]
    fn oversized_metadata_refuses_without_silent_truncation() {
        let error = project_metadata(
            json!({"member": {"provenance": {"reference": "x".repeat(MAX_METADATA_BYTES)}}}),
        )
        .unwrap_err();
        assert_eq!(error.code(), "catalog_member_metadata_too_large");
    }
}
