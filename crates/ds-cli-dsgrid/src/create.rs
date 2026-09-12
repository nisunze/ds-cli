//! Native file creation over the same blank-model authority as the WASM host.
use std::io::Read;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::GridSession;
use ds_grid_exchange::blank_model::{
    BLANK_MODEL_REVISION, BlankModelError, BlankModelRequest, BlankModelStandards,
};
use ds_grid_exchange::create_blank_model;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub static COMMAND: Command = Command {
    id: "dsgrid.create",
    path: &["dsgrid", "create"],
    contract: 1,
    summary: "Create a native .dsgrid file, optionally from standards.",
    purpose: "Creates one canonical .dsgrid package through the engine's blank-model authority. Optional .dsgrid-template bytes supply verified engineering standards and resources. No browser, desktop pairing or project is needed. The file starts at package revision zero; the receipt gives the authored revision needed by dsgrid apply. It creates no project catalogue entry and never overwrites an existing file.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "out",
            "<path>",
            "New .dsgrid output path; never overwritten.",
        )
        .required(),
        Arg::value(
            "model-id",
            "<id>",
            "Stable model identity; omit for the engine default.",
        ),
        Arg::value(
            "crs",
            "<crs>",
            "Projected metric CRS; omit for the engine default.",
        ),
        Arg::value(
            "standards",
            "<path>",
            "Verified .dsgrid-template supplying engineering definitions.",
        ),
    ],
    output: "The model identity, CRS, package and authored revisions, native snapshot fingerprint, and new artifact path, size and SHA-256. persisted:true confirms the file was written and synchronized.",
    examples: &[
        Example {
            command: "ds dsgrid create --out ./new.dsgrid --model-id mv-line --crs EPSG:32735 --output json",
            note: "Create a model for native editing without a running application.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid create --help",
            note: "Read the complete native creation contract.",
            runnable: true,
        },
    ],
    refusals: &[
        Refusal {
            code: "invalid_id",
            when: "the engine rejects the model identity",
            remedy: "supply an identity accepted by the native model contract",
        },
        Refusal {
            code: "unsupported_coordinate_system",
            when: "the engine cannot author in that CRS",
            remedy: "use a supported projected metric CRS, e.g. EPSG:32735",
        },
        Refusal {
            code: "invalid_coordinate_system",
            when: "the CRS declaration is malformed",
            remedy: "supply a valid projected metric CRS",
        },
        Refusal {
            code: "standards_unreadable",
            when: "the standards path is absent, not a regular file, or unreadable",
            remedy: "name one readable .dsgrid-template file",
        },
        Refusal {
            code: "standards_too_large",
            when: "the standards file exceeds the 512 MiB read bound",
            remedy: "use a bounded standards template",
        },
        Refusal {
            code: "standards_refused",
            when: "the engine rejects the template or its engineering/resource closure",
            remedy: "supply a verified standards-only .dsgrid-template",
        },
        Refusal {
            code: "package_emit_failed",
            when: "the engine cannot emit the canonical package",
            remedy: "report the engine error; no output was written",
        },
        Refusal {
            code: "output_exists",
            when: "the output path already exists",
            remedy: "choose a new path; create never overwrites",
        },
        Refusal {
            code: "output_parent_missing",
            when: "the output parent directory does not exist",
            remedy: "create the intended directory before retrying",
        },
        Refusal {
            code: "output_unwritable",
            when: "the output cannot be created or fully written",
            remedy: "check permissions and free space; a partial file is removed",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let out = inputs.require("out")?;
    crate::apply::validate_output_path(out)?;
    let standards = inputs.value("standards").map(read_standards).transpose()?;
    let request = BlankModelRequest {
        model_id: inputs.value("model-id"),
        coordinate_system: inputs.value("crs"),
        standards: standards
            .as_deref()
            .map_or(BlankModelStandards::Empty, BlankModelStandards::Template),
        ..Default::default()
    };
    let model = create_blank_model(&request).map_err(|error| {
        let message = error.to_string();
        match error {
            BlankModelError::Identifier { .. } => {
                Failure::invalid("invalid_id", message).remedy("supply a native model identity")
            }
            BlankModelError::UnsupportedCoordinateSystem { .. } => {
                Failure::invalid("unsupported_coordinate_system", message)
                    .remedy("use a supported projected metric CRS")
            }
            BlankModelError::InvalidCoordinateSystem { .. } => {
                Failure::invalid("invalid_coordinate_system", message)
                    .remedy("supply a valid projected metric CRS")
            }
            BlankModelError::Standards(_) => Failure::invalid("standards_refused", message)
                .remedy("use a verified standards-only .dsgrid-template"),
            BlankModelError::Package(_) => Failure::failed("package_emit_failed", message)
                .remedy("report this engine error; no output was written"),
        }
    })?;
    let fingerprint = model.snapshot.snapshot_fingerprint();
    let session = GridSession::open(model.snapshot);
    crate::apply::write_new(out, &model.bytes)?;
    Ok(json!({
        "model_id": model.model_id,
        "coordinate_system": model.coordinate_system,
        "package_revision": BLANK_MODEL_REVISION,
        "authored_revision": session.current_revision().revision_id.as_str(),
        "snapshot_fingerprint": fingerprint,
        "persisted": true,
        "artifact": { "path": out, "byte_len": model.bytes.len(), "sha256": format!("sha256:{:x}", Sha256::digest(&model.bytes)) },
    }))
}

fn read_standards(path: &str) -> Result<Vec<u8>, Failure> {
    let unreadable = |error: std::io::Error| {
        Failure::invalid(
            "standards_unreadable",
            format!("cannot read `{path}`: {error}"),
        )
        .remedy("name one readable .dsgrid-template file")
    };
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let metadata = file.metadata().map_err(unreadable)?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid("standards_unreadable", "standards must be a regular file")
                .remedy("name one readable .dsgrid-template file"),
        );
    }
    if metadata.len() > crate::package::MAX_PACKAGE_BYTES {
        return Err(
            Failure::invalid("standards_too_large", "standards exceed 512 MiB")
                .remedy("use a bounded standards template"),
        );
    }
    let mut bytes = Vec::new();
    file.take(crate::package::MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() as u64 > crate::package::MAX_PACKAGE_BYTES {
        return Err(
            Failure::invalid("standards_too_large", "standards exceed 512 MiB")
                .remedy("use a bounded standards template"),
        );
    }
    Ok(bytes)
}

pub fn render(data: &Value) -> String {
    format!(
        "created {}\n  model     {}\n  crs       {}\n  revision  {}\n",
        data["artifact"]["path"].as_str().unwrap_or("?"),
        data["model_id"].as_str().unwrap_or("?"),
        data["coordinate_system"].as_str().unwrap_or("?"),
        data["authored_revision"].as_str().unwrap_or("?")
    )
}
