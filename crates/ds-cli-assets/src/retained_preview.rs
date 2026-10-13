//! Argument/terminal binding only for the native source-preview owner.
use ds_cli_contract::spec::{Arg, Refusal};
use ds_cli_contract::{Failure, Inputs};
use ds_command_kernel::asset_preview::{Request, REQUEST_SCHEMA};
use serde_json::{json, Value};
pub const RETAIN: Arg = Arg::switch(
    "retain",
    "Acquire one whole PDF, DOCX or XLSX document through the shared native file room (PDF 32 MiB; Office 8 MiB); return exact source/file pins.",
);
pub const OFFLINE:Arg=Arg::switch("offline","Consume retained document only; requires --retain and all three expected source pins; zero source/auth refresh calls.");
pub const REVISION: Arg = Arg::value(
    "expected-revision",
    "<native-revision>",
    "Exact retained/native source revision, not a timestamp or filename.",
);
pub const GENERATION: Arg = Arg::value(
    "expected-generation",
    "<storage-generation>",
    "Exact immutable storage generation from a native preview receipt.",
);
pub const SHA: Arg = Arg::value(
    "expected-sha256",
    "<sha256>",
    "Exact measured SHA from a native receipt; optional first online read.",
);
pub const RETENTION: Arg = Arg::value(
    "preview-retention",
    "<24h|7d>",
    "Native retained-file expiry; defaults to 24h.",
)
.choices(&["24h", "7d"]);
pub const REFUSALS: [Refusal; 16] = [
    Refusal {
        code: "asset_preview_pins_invalid",
        when: "retained/offline arguments or source pins are invalid",
        remedy: "use --retain; offline needs exact revision, generation and SHA",
    },
    Refusal {
        code: "asset_preview_source_invalid",
        when: "source proof is not an exact bounded PDF, DOCX or XLSX document in this project",
        remedy: "re-read the selected source or use its governed download",
    },
    Refusal {
        code: "asset_preview_source_changed",
        when: "source revision, generation, SHA or metadata differs",
        remedy: "re-read the source row and explicitly select the current source",
    },
    Refusal {
        code: "asset_preview_retained_invalid",
        when: "native retained receipt does not prove its document/file",
        remedy: "read the same source online; never substitute another file",
    },
    Refusal {
        code: "asset_preview_offline_absent",
        when: "the exact source is absent, expired or revoked locally",
        remedy: "open that exact document online under this native account",
    },
    Refusal {
        code: "asset_preview_acquire_failed",
        when: "bounded native source read could not complete",
        remedy: "retry the same selected source; no partial preview is admitted",
    },
    Refusal {
        code: "asset_preview_decode_failed",
        when: "the kernel refuses the exact document bytes",
        remedy: "review or download a valid admitted document through its source",
    },
    Refusal {
        code: "asset_preview_cancelled",
        when: "the native consumer explicitly cancelled",
        remedy: "open the selected source again when wanted",
    },
    Refusal {
        code: "project_context_changed",
        when: "native owner, project, audience or credential binding changed",
        remedy: "repeat under the intended current native account and project",
    },
    Refusal {
        code: "report_inputs_invalid",
        when: "the shared native file authority is malformed",
        remedy: "restore the current protected native context; do not edit a receipt",
    },
    Refusal {
        code: "print_preview_artifact_unavailable",
        when: "exact native file/manifest is unsafe, changed, expired or revoked",
        remedy: "retry the same source online; preserve foreign files",
    },
    Refusal {
        code: "work_not_permitted",
        when: "PM source authority denies this member the selected attachment",
        remedy: "ask a project admin for source read access",
    },
    Refusal {
        code: "project_not_visible",
        when: "PM source authority cannot expose the named project/source",
        remedy: "choose an exact visible project and attachment",
    },
    ds_cli_auth::PM_REFUSED_REFUSAL,
    Refusal {
        code: "context_corrupt",
        when: "the explicit project is not a valid native project reference",
        remedy: "copy the exact project ID from the authenticated directory",
    },
    Refusal {
        code: "auth_context_unreadable",
        when: "protected native provider identity is malformed",
        remedy: "repair protected native account state without copying credentials",
    },
];
pub fn request(inputs: &Inputs) -> Result<Option<Request>, Failure> {
    let selected = inputs.switch("retain");
    let has_options = inputs.switch("offline")
        || [
            "expected-revision",
            "expected-generation",
            "expected-sha256",
            "preview-retention",
        ]
        .iter()
        .any(|name| inputs.value(name).is_some());
    if !selected {
        if has_options {
            return Err(invalid(
                "Retained source pins/offline options require --retain.",
            ));
        }
        return Ok(None);
    }
    if ["member", "sheet", "pages", "rows", "pdf-text-pages"]
        .iter()
        .any(|name| inputs.value(name).is_some())
    {
        return Err(invalid(
            "--retain selects one whole PDF, DOCX or XLSX document without member/sheet/text/page/grid options.",
        ));
    }
    let request:Request=serde_json::from_value(json!({"schema":REQUEST_SCHEMA,"asset_id":inputs.require("asset")?,"expected_revision":inputs.value("expected-revision"),"expected_generation":inputs.value("expected-generation"),"expected_sha256":inputs.value("expected-sha256"),"offline":inputs.switch("offline"),"retention":inputs.value("preview-retention").unwrap_or("24h")})).map_err(|_|invalid("Retained document arguments are invalid."))?;
    request.validate().map_err(|e| {
        Failure::invalid("asset_preview_pins_invalid", e.message).remedy(REFUSALS[0].remedy)
    })?;
    Ok(Some(request))
}
fn invalid(message: &str) -> Failure {
    Failure::invalid("asset_preview_pins_invalid", message).remedy(REFUSALS[0].remedy)
}
pub fn run(inputs: &Inputs, request: &Request) -> Result<Value, Failure> {
    let result = ds_cli_auth::asset_preview_source::acquire_for_named_project(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        request,
    )?;
    serde_json::to_value(result).map_err(|_| {
        Failure::internal(
            "assets_unreadable",
            "The native document receipt could not be encoded.",
        )
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input(flags: &[&str]) -> Inputs {
        let mut tokens = vec![
            "--project".into(),
            "project".into(),
            "--asset".into(),
            "sys:pm_attachment:pma_pdf".into(),
        ];
        tokens.extend(flags.iter().map(|v| v.to_string()));
        ds_cli_contract::parse(&super::super::preview::COMMAND, &tokens).unwrap()
    }
    #[test]
    fn dormant_preview_retains_original_arguments_and_requests_nothing() {
        assert!(request(&input(&[])).unwrap().is_none());
    }
    #[test]
    fn first_online_retained_pdf_accepts_no_advertised_sha() {
        let req = request(&input(&["--retain"])).unwrap().unwrap();
        assert!(!req.offline);
        assert!(req.expected_sha256.is_none());
    }
    #[test]
    fn offline_requires_all_native_pins_and_rejects_mixed_consumption() {
        assert_eq!(
            request(&input(&["--retain", "--offline"]))
                .unwrap_err()
                .code(),
            "asset_preview_pins_invalid"
        );
        assert!(request(&input(&["--offline"])).is_err());
        assert!(request(&input(&["--retain", "--member", "x.pdf"])).is_err());
        assert!(request(&input(&["--retain", "--pages", "5"])).is_err());
        assert!(request(&input(&["--retain", "--rows", "200"])).is_err());
        let digest = "a".repeat(64);
        assert!(
            request(&input(&[
                "--retain",
                "--offline",
                "--expected-revision",
                "pm-version:7",
                "--expected-generation",
                "1234",
                "--expected-sha256",
                &digest
            ]))
            .unwrap()
            .unwrap()
            .offline
        );
    }
}
