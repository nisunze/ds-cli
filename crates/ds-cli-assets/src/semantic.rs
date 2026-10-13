//! Thin declarations and argument binding for native document semantic derivatives.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::{
    asset_preview::{Request, Retention, REQUEST_SCHEMA},
    assets::semantic::{self, Extractor},
};
use serde_json::{json, Value};
const MANIFEST: Arg = Arg::value(
    "manifest",
    "<native-export.json>",
    "Unchanged typed manifest written by semantic export; do not assemble receipts.",
)
.required();
const OUT: Arg = Arg::value(
    "out",
    "<absolute-file>",
    "New local output file; existing files are preserved.",
);
const VERSION: Arg = Arg::value(
    "extractor-version",
    "<installed-version>",
    "Actual external MarkItDown version used for this selected document.",
)
.required();
const OPTIONS: Arg = Arg::value(
    "options-sha256",
    "<sha256>",
    "Actual options digest; omitted means compact JSON {} (no extractor options).",
);
const OWN: [Refusal; 7] = [
    Refusal {
        code: "asset_semantic_invalid",
        when: "pins, native manifest, source or external Markdown file are malformed, changed or unsupported",
        remedy: "use unchanged semantic-export JSON and selected document/nonempty UTF-8 Markdown; no other format is admitted",
    },
    Refusal {
        code: "asset_semantic_offline_absent",
        when: "the exact derivative is absent, expired or revoked locally",
        remedy: "explicitly import then read the selected derivative online",
    },
    Refusal {
        code: "asset_semantic_retained_invalid",
        when: "the retained native source/derivative room cannot prove its exact scope and bytes",
        remedy: "use the selected source/export again under the same protected account; preserve foreign files",
    },
    Refusal {
        code: "print_preview_artifact_expired",
        when: "the selected retained file expired",
        remedy: "read the exact selected derivative online again",
    },
    Refusal {
        code: "report_output_exists",
        when: "the chosen local output already exists",
        remedy: "choose a new absolute output path; existing files are preserved",
    },
    crate::CONFIRMATION_REQUIRED,
    crate::ASSETS_UNREADABLE,
];
const EXTRA: [Refusal; 23] = {
    let mut out = [OWN[0]; 23];
    let mut i = 0;
    while i < 16 {
        out[i] = super::retained_preview::REFUSALS[i];
        i += 1;
    }
    let mut j = 0;
    while j < 7 {
        out[i + j] = OWN[j];
        j += 1;
    }
    out
};
const REFUSALS: [Refusal; 45] = crate::refusals::<45>(&EXTRA);
const fn leaf(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    purpose: &'static str,
    effect: Effect,
    args: &'static [Arg],
    output: &'static str,
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose,
        chapter: Chapter::Assets,
        effect,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args,
        output,
        examples: &[],
        refusals: &REFUSALS,
        reference: None,
        search: &[
            "document extraction",
            "extract once",
            "MarkItDown",
            "document Markdown",
        ],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    }
}
pub static EXPORT: Command = leaf(
    "assets.semantic.export",
    &["assets", "semantic", "export"],
    "Hand one exact PDF, DOCX or XLSX document to external MarkItDown with a native manifest.",
    "Acquires or reuses the selected whole document in the protected native room (PDF at most 32 MiB; DOCX/XLSX at most 8 MiB). Writes one new document and a typed JSON manifest carrying measured source/version/generation and actual extractor pins. Run MarkItDown externally on that document using the declared version/options, then use semantic import. DS never runs an extractor. Offline requires all retained source pins and makes zero source requests.",
    Effect::LocalFileWrite,
    &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        Arg::value(
            "asset",
            "<asset-id>",
            "Catalogue PDF/DOCX/XLSX or sys:pm_attachment:<id>.",
        )
        .required(),
        OUT.required(),
        MANIFEST,
        VERSION,
        OPTIONS,
        super::retained_preview::OFFLINE,
        super::retained_preview::REVISION,
        super::retained_preview::GENERATION,
        super::retained_preview::SHA,
        super::retained_preview::RETENTION,
    ],
    "selection, document path and reusable native manifest path; original source is unchanged.",
);
pub static LOOKUP: Command = leaf(
    "assets.semantic.lookup",
    &["assets", "semantic", "lookup"],
    "Discover whether this exact PDF, DOCX or XLSX document already has an extracted derivative.",
    "Reads the selected export manifest and one exact source-authorized derivative key. found:false means no selected derivative; uploading remains partial and unreadable. Unsupported/denied/stale sources refuse truthfully. No bytes are downloaded and no extraction or upload occurs.",
    Effect::ReadOnly,
    &[crate::PROJECT_ARG, crate::LANE_ARG, MANIFEST],
    "Exact found/state metadata; no document bytes or grants.",
);
pub static READ: Command = leaf(
    "assets.semantic.read",
    &["assets", "semantic", "read"],
    "Read this document's checked Markdown, reusing the retained derivative.",
    "Consumes the native export manifest and exact scoped retained derivative first. Offline makes zero source requests and refuses absent/expired/revoked data. Explicit refresh asks current source authority once and downloads the immutable committed generation, verifies digest/size/UTF-8, and retains it in the shared native room. A capture is not proof of current membership/cloud freshness. Inline text is bounded to4096 characters; --out writes the full unchanged Markdown to a new .md file. No extraction occurs.",
    Effect::LocalFileWrite,
    &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        MANIFEST,
        OUT,
        Arg::switch(
            "offline",
            "Use only the exact retained derivative; no cloud acquisition.",
        ),
        Arg::switch(
            "refresh",
            "Explicitly re-authorize and acquire this derivative online.",
        ),
        super::retained_preview::RETENTION,
    ],
    "Ready receipt, retained artifact, reused flag, bounded Markdown, more flag and optional full output.",
);
pub static IMPORT: Command = leaf(
    "assets.semantic.import",
    &["assets", "semantic", "import"],
    "Retain externally extracted Markdown against its exact native document.",
    "Reads the unchanged native export manifest and one selected absolute .md file (nonempty UTF-8, at most16MiB). Proves the retained original document receipt, measures output SHA/bytes, prepares through the source's authority, uploads through its issued grant and commits the exact Bulk manifest. Ready replay does not upload again. Requires --yes. No extractor execution or source/catalogue version mutation occurs.",
    Effect::GlobalWrite,
    &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        MANIFEST,
        Arg::value(
            "markdown",
            "<absolute.md>",
            "Selected output from the pinned external extractor; no URL or command.",
        )
        .required(),
        super::retained_preview::RETENTION,
    ],
    "Committed/replayed ready derivative receipt and exact retained Markdown artifact.",
);
fn retention(inputs: &Inputs) -> Retention {
    if inputs.value("preview-retention") == Some("7d") {
        Retention::Days7
    } else {
        Retention::Hours24
    }
}
fn lane(inputs: &Inputs) -> &str {
    inputs.value("lane").unwrap_or("stable")
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("asset_semantic_invalid", message).remedy(OWN[0].remedy)
}
pub fn export(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let request:Request=serde_json::from_value(json!({"schema":REQUEST_SCHEMA,"asset_id":inputs.require("asset")?,"expected_revision":inputs.value("expected-revision"),"expected_generation":inputs.value("expected-generation"),"expected_sha256":inputs.value("expected-sha256"),"offline":inputs.switch("offline"),"retention":inputs.value("preview-retention").unwrap_or("24h")})).map_err(|_|invalid("Selected document request is malformed."))?;
    let extractor = Extractor {
        name: "markitdown".into(),
        version: inputs.require("extractor-version")?.into(),
        options_sha256: inputs
            .value("options-sha256")
            .map(str::to_owned)
            .unwrap_or_else(|| semantic::digest(b"{}")),
    };
    ds_cli_auth::document_semantic::export(
        lane(inputs),
        inputs.require("project")?,
        &request,
        extractor,
        inputs.require("out")?,
        inputs.require("manifest")?,
    )
}
pub fn lookup(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_auth::document_semantic::lookup(
        lane(inputs),
        inputs.require("project")?,
        inputs.require("manifest")?,
    )
}
pub fn read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_auth::document_semantic::read(
        lane(inputs),
        inputs.require("project")?,
        inputs.require("manifest")?,
        inputs.switch("offline"),
        inputs.switch("refresh"),
        inputs.value("out"),
        retention(inputs),
    )
}
pub fn import(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if !context.confirmed {
        return Err(
            Failure::invalid("confirmation_required", "Semantic import requires --yes.")
                .remedy(crate::CONFIRMATION_REQUIRED.remedy),
        );
    }
    ds_cli_auth::document_semantic::import(
        lane(inputs),
        inputs.require("project")?,
        inputs.require("manifest")?,
        inputs.require("markdown")?,
        retention(inputs),
    )
}
pub fn render(value: &Value) -> String {
    format!("{value}\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_leaves_have_explicit_effects_and_no_executor_inputs() {
        for command in [&EXPORT, &LOOKUP, &READ, &IMPORT] {
            assert_eq!(command.authority, Authority::HeadlessProject);
            assert!(command.args.iter().any(|arg| arg.name == "manifest"));
            assert!(!command
                .args
                .iter()
                .any(|arg| matches!(arg.name, "executable" | "command" | "url")));
        }
        assert_eq!(EXPORT.effect, Effect::LocalFileWrite);
        assert_eq!(IMPORT.effect, Effect::GlobalWrite);
    }
    #[test]
    fn malformed_manifest_and_unconfirmed_import_stop_before_auth() {
        let tokens = ["--project", "project", "--manifest", "relative.json"].map(str::to_owned);
        let input = ds_cli_contract::parse(&LOOKUP, &tokens).unwrap();
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        assert_eq!(
            lookup(&input, &context).unwrap_err().code(),
            "asset_semantic_invalid"
        );
        let tokens = [
            "--project",
            "project",
            "--manifest",
            "relative.json",
            "--markdown",
            "relative.md",
        ]
        .map(str::to_owned);
        let input = ds_cli_contract::parse(&IMPORT, &tokens).unwrap();
        assert_eq!(
            import(&input, &context).unwrap_err().code(),
            "confirmation_required"
        );
    }
}
