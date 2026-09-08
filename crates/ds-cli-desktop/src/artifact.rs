//! Receipt-verified access to committed local Network Reporter outputs.
//! Callers name one project, transformer and output id; the paired application
//! resolves the opaque locator from its owner-bound native inventory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::ops;

const READ_OP: ops::BridgeOp = ops::BridgeOp {
    operation: "printing.artifact.read",
    arguments: &["project", "transformer", "outputId"],
};
const COPY_OP: ops::BridgeOp = ops::BridgeOp {
    operation: "printing.artifact.copy",
    arguments: &["project", "transformer", "outputId", "destination"],
};
const TIMEOUT: Duration = Duration::from_secs(120);

const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Exact project whose owner-bound local report inventory is inspected; never switches the map.",
)
.required();
const TRANSFORMER_ARG: Arg = Arg::value(
    "transformer",
    "<name>",
    "Canonical transformer name used by the committed report batch.",
)
.required();
const OUTPUT_ARG: Arg = Arg::value(
    "output-id",
    "<output-id>",
    "Exact outputId returned by desktop printing export or artifact read.",
)
.required();
const DESCRIPTOR_ARG: Arg = Arg::value(
    "desktop-descriptor",
    "<path>",
    "Use this bridge descriptor instead of discovering one.",
);

const REFUSALS: &[Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::SIGNED_OUT,
    Refusal {
        code: "printing_artifact_invalid",
        when: "project, transformer or output identity is malformed",
        remedy: "copy the exact project, transformer and outputId from the printing export receipt",
    },
    Refusal {
        code: "printing_artifact_unavailable",
        when: "the owner-bound committed output is absent or its receipt/bytes fail verification",
        remedy: "run desktop printing export for that exact project and transformer, then retry",
    },
];

pub static READ_COMMAND: Command = Command {
    id: "desktop.printing.artifact.read",
    path: &["desktop", "printing", "artifact", "read"],
    contract: 1,
    summary: "Verify one committed local print artifact and read its evidence.",
    purpose: "Resolves one exact output through the signed-in owner's native committed report inventory, then reopens and verifies its complete bytes against the sealed receipt. The locator remains opaque and no map project is opened or switched.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, TRANSFORMER_ARG, OUTPUT_ARG, DESCRIPTOR_ARG],
    output: "Exact project, transformer, output id, filename, format, recorded layout/paper/orientation/dimensions when available, content type, byte count, SHA-256 and opaque locator with verified=true.",
    examples: &[Example {
        command: "ds desktop printing artifact read --project survey_test --transformer agasharu --output-id pdf__a3 --output json",
        note: "Verify the locally committed A3 PDF before copying it.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};

pub static COPY_COMMAND: Command = Command {
    id: "desktop.printing.artifact.copy",
    path: &["desktop", "printing", "artifact", "copy"],
    contract: 1,
    summary: "Copy one verified local print artifact to a new file.",
    purpose: "Resolves and receipt-verifies one exact owner/project/transformer output inside the paired desktop, then creates the explicit --out file once. It never derives a report cache path, overwrites an existing destination, opens a map or changes the GUI project.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        TRANSFORMER_ARG,
        OUTPUT_ARG,
        Arg::value("out", "<file>", "New destination file; never overwritten.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "Destination plus the exact project, transformer, output id, filename, recorded layout/paper/orientation/dimensions when available, content type, byte count and SHA-256 copied.",
    examples: &[Example {
        command: "ds desktop printing artifact copy --project survey_test --transformer agasharu --output-id pdf__a3 --out ./agasharu-a3.pdf --output json",
        note: "Create one verified local copy for inspection or Drive synchronization.",
        runnable: false,
    }],
    refusals: &[
        REFUSALS[0],
        REFUSALS[1],
        REFUSALS[2],
        REFUSALS[3],
        REFUSALS[4],
        REFUSALS[5],
        REFUSALS[6],
        REFUSALS[7],
        REFUSALS[8],
        REFUSALS[9],
        Refusal {
            code: "printing_artifact_path_exists",
            when: "--out already exists",
            remedy: "choose a new destination; artifact copies never overwrite files",
        },
        Refusal {
            code: "printing_artifact_unwritable",
            when: "--out is not an absolute, new file under an existing writable directory",
            remedy: "choose a new writable destination and retry",
        },
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};

fn bounded(value: &str, label: &str, max: usize) -> Result<String, Failure> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > max
        || value.chars().any(char::is_control)
    {
        return Err(
            Failure::invalid("printing_artifact_invalid", format!("{label} is invalid"))
                .remedy("use the exact identity returned by desktop printing export"),
        );
    }
    Ok(value.to_string())
}

fn selector(inputs: &Inputs) -> Result<Value, Failure> {
    Ok(json!({
        "project": bounded(inputs.require("project")?, "project", 160)?,
        "transformer": bounded(inputs.require("transformer")?, "transformer", 160)?,
        "outputId": bounded(inputs.require("output-id")?, "output-id", 160)?,
    }))
}

fn absolute_new_destination(raw: &str) -> Result<PathBuf, Failure> {
    let raw = bounded(raw, "out", 4096)?;
    let path = std::path::absolute(Path::new(&raw)).map_err(|_| {
        Failure::invalid(
            "printing_artifact_unwritable",
            "--out could not be resolved",
        )
        .remedy("choose a new writable destination")
    })?;
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(Failure::failed(
            "printing_artifact_path_exists",
            format!("`{}` already exists", path.display()),
        )
        .remedy("choose a new destination; artifact copies never overwrite files"));
    }
    let parent = path.parent().ok_or_else(|| {
        Failure::invalid(
            "printing_artifact_unwritable",
            "--out has no parent directory",
        )
    })?;
    let metadata = std::fs::metadata(parent).map_err(|_| {
        Failure::invalid(
            "printing_artifact_unwritable",
            "--out parent directory does not exist",
        )
        .remedy("create the destination directory, then retry")
    })?;
    if !metadata.is_dir() {
        return Err(Failure::invalid(
            "printing_artifact_unwritable",
            "--out parent is not a directory",
        ));
    }
    Ok(path)
}

fn verified_result(expected: &Value, result: Value) -> Result<Value, Failure> {
    let valid_sha = result["sha256"].as_str().is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if result["project"] != expected["project"]
        || result["transformer"] != expected["transformer"]
        || result["outputId"] != expected["outputId"]
        || result["verified"].as_bool() != Some(true)
        || !valid_sha
        || !result["sizeBytes"]
            .as_u64()
            .is_some_and(|size| size > 0 && size <= 64 * 1024 * 1024)
    {
        return Err(Failure::unavailable(
            "desktop_contract_mismatch",
            "the paired application returned invalid print artifact evidence",
        )
        .remedy("update DS GridDesign and ds to matching builds"));
    }
    Ok(result)
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let selector = selector(inputs)?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    let result = ops::invoke(&descriptor, &READ_OP, selector.clone(), TIMEOUT)
        .map_err(ops::classify_signed_out)?;
    verified_result(&selector, result)
}

pub fn copy(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut selector = selector(inputs)?;
    let destination = absolute_new_destination(inputs.require("out")?)?;
    selector["destination"] = json!(destination.to_string_lossy());
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    let result = ops::invoke(&descriptor, &COPY_OP, selector.clone(), TIMEOUT)
        .map_err(ops::classify_signed_out)?;
    let result = verified_result(&selector, result)?;
    if result["out"] != selector["destination"] || result["copied"].as_bool() != Some(true) {
        return Err(Failure::unavailable(
            "desktop_contract_mismatch",
            "the paired application returned invalid artifact copy evidence",
        ));
    }
    Ok(result)
}

pub fn render(data: &Value) -> String {
    if let Some(out) = data["out"].as_str() {
        format!(
            "copied {}\nfile     {out}\nbytes    {}\nsha256   {}\n",
            data["outputId"], data["sizeBytes"], data["sha256"]
        )
    } else {
        format!(
            "verified {}\nfile     {}\nbytes    {}\nsha256   {}\n",
            data["outputId"], data["filename"], data["sizeBytes"], data["sha256"]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_contract_has_only_exact_selector_and_destination() {
        assert_eq!(READ_OP.arguments, ["project", "transformer", "outputId"]);
        assert_eq!(
            COPY_OP.arguments,
            ["project", "transformer", "outputId", "destination"]
        );
    }

    #[test]
    fn copy_refuses_an_existing_destination_before_pairing() {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        let error = absolute_new_destination(temporary.path().to_str().unwrap()).unwrap_err();
        assert_eq!(error.code(), "printing_artifact_path_exists");
    }
}
