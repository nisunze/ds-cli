//! Thin transport for one revision- and source-fenced native cable export.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::pls_cadd_cable_export::{
    CableMemberExportError, CableMemberExportRequest, export_cable_member_package,
};
use serde_json::{Value, json};

const OWN: &[Refusal] = &[
    Refusal {
        code: "request_invalid",
        when: "a cable/resource/revision identity is malformed",
        remedy: "use exact identities from dsgrid inspect and project_table",
    },
    Refusal {
        code: "revision_conflict",
        when: "the authored revision differs from --revision",
        remedy: "inspect this package's authored revision and deliberately refresh the fence",
    },
    Refusal {
        code: "cable_not_found",
        when: "the selected cable is absent",
        remedy: "read the cables table and select an exact cable id",
    },
    Refusal {
        code: "source_unavailable",
        when: "the selected resource is absent or is not an embedded cable definition",
        remedy: "read the resources table and select the exact retained cable resource",
    },
    Refusal {
        code: "source_digest_mismatch",
        when: "the selected source differs from --expect-source-digest",
        remedy: "review its current resource digest; never substitute a same-named member",
    },
    Refusal {
        code: "source_bytes_unavailable",
        when: "no unique retained payload matches the resource's digest and size",
        remedy: "recover the exact retained resource before exporting",
    },
    Refusal {
        code: "cable_member_unrepresentable",
        when: "native units, mechanics, identity or readback cannot represent this cable",
        remedy: "read the exact native field refusal; review nominal/strand moduli together, or preview dsgrid reconcile-cable-source for a linked source",
    },
    Refusal {
        code: "cable_member_package_invalid",
        when: "the package cannot be verified and decoded",
        remedy: "run ds dsgrid validate on the source package",
    },
    Refusal {
        code: "output_required",
        when: "a write has no --out",
        remedy: "pass a new member path or --dry-run",
    },
    Refusal {
        code: "output_member_identity_mismatch",
        when: "--out has a different leaf than the source member",
        remedy: "use the exact filename reported by --dry-run inside your cables folder",
    },
    Refusal {
        code: "output_exists",
        when: "--out already exists",
        remedy: "choose a new directory; member export never overwrites",
    },
    Refusal {
        code: "output_parent_missing",
        when: "the output directory does not exist",
        remedy: "create the intended cables directory",
    },
    Refusal {
        code: "output_unwritable",
        when: "the member cannot be written and synchronized",
        remedy: "check permissions and free space; a partial file is removed",
    },
    Refusal {
        code: "invalid_limit",
        when: "--limit is outside 1..5000",
        remedy: "pass a whole number from 1 to 5000",
    },
];
const REFUSALS: &[Refusal; OWN.len() + crate::package::SHARED_REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + crate::package::SHARED_REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + crate::package::SHARED_REFUSALS.len()];
    let mut i = 0;
    while i < OWN.len() {
        all[i] = OWN[i];
        i += 1;
    }
    let mut j = 0;
    while j < crate::package::SHARED_REFUSALS.len() {
        all[i + j] = crate::package::SHARED_REFUSALS[j];
        j += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.cable.export",
    path: &["dsgrid", "cable", "export"],
    contract: 1,
    summary: "Export one edited cable member without whole-model earthing or criteria gates.",
    purpose: "Write one reviewed cable definition over an explicitly selected, digest-pinned retained native member. Unchanged members remain exact; characterized mechanics and creep edits have field/old/new/owner-rule receipts and native readback. The source resource selection remains explicit after a generic cable edit severs its link. This is independent edited-member emission, not whole-workspace conversion or source extraction. No project, model or source file changes. PLS-CADD Cable Data comparison remains a separate native acceptance gate.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<file.dsgrid>", "The exact local package to read.").required(),
        Arg::value(
            "cable-id",
            "<id>",
            "The cable to export, from project_table cables.",
        )
        .required(),
        Arg::value("revision", "<rev:...>", "The observed authored revision.").required(),
        Arg::value(
            "source-resource-id",
            "<id>",
            "The exact retained native cable resource, from project_table resources.",
        )
        .required(),
        Arg::value(
            "expect-source-digest",
            "<sha256:digest>",
            "The selected resource's content_digest.",
        )
        .required(),
        Arg::value(
            "out",
            "<cables/member>",
            "New output path with the exact source-member leaf; required unless --dry-run.",
        ),
        Arg::switch(
            "dry-run",
            "Validate the final bytes and return patch/preservation evidence without writing.",
        ),
        Arg::value(
            "limit",
            "<n>",
            "Cap receipt collections; every truncation is reported.",
        )
        .default("50"),
    ],
    output: "Package/revision and source identities, exact filename, source/emitted SHA-256, native version, cable_data_readback in native and SI units, per-field patches, native_cable_data_accepted:false, and dry_run/persisted. A write adds output path and size. more.truncated identifies every capped collection.",
    examples: &[Example {
        command: "ds dsgrid cable export --model ./reviewed.dsgrid --cable-id <id> --revision rev:<digest> --source-resource-id <id> --expect-source-digest sha256:<digest> --dry-run --output json",
        note: "Validate one retained member and review its field patches before choosing an output path.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "edited cable",
        "cables folder",
        "creep",
        "PLS-CADD Cable Data",
        "member export",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

fn owner_failure(error: CableMemberExportError) -> Failure {
    let message = error.to_string();
    match error {
        CableMemberExportError::PackageTooLarge => Failure::invalid("model_too_large", message)
            .remedy("use a bounded .dsgrid package"),
        CableMemberExportError::Package(_) => Failure::invalid("cable_member_package_invalid", message)
            .remedy("run ds dsgrid validate on this package"),
        CableMemberExportError::RevisionConflict { .. } => Failure::invalid("revision_conflict", message)
            .remedy("inspect the exact authored revision and deliberately refresh the fence"),
        CableMemberExportError::CableNotFound { .. } => Failure::invalid("cable_not_found", message)
            .remedy("read project_table cables and select an exact cable id"),
        CableMemberExportError::SourceUnavailable { .. } => Failure::invalid("source_unavailable", message)
            .remedy("read project_table resources and select the retained embedded cable resource"),
        CableMemberExportError::SourceDigestMismatch { .. } => Failure::invalid("source_digest_mismatch", message)
            .remedy("review the selected resource digest; never substitute a same-named member"),
        CableMemberExportError::SourceBytesUnavailable { .. } => Failure::invalid("source_bytes_unavailable", message)
            .remedy("recover the exact retained source bytes"),
        CableMemberExportError::NativeRefusal { .. } => Failure::invalid("cable_member_unrepresentable", message)
            .remedy("review the exact cable fields named by the native owner; a linked source can be previewed through dsgrid reconcile-cable-source"),
    }
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let dry_run = inputs.switch("dry-run");
    let limit = crate::package::parse_limit(inputs.value("limit"))?;
    let request: CableMemberExportRequest = serde_json::from_value(json!({
        "cable_id": inputs.require("cable-id")?, "expected_revision": inputs.require("revision")?,
        "source_resource_id": inputs.require("source-resource-id")?, "expected_source_digest": inputs.require("expect-source-digest")?,
    })).map_err(|error| Failure::invalid("request_invalid", error.to_string()).remedy("use exact cable/resource/revision identities from this package"))?;
    let out = if dry_run {
        None
    } else {
        let path = inputs.value("out").ok_or_else(|| {
            Failure::invalid("output_required", "member export needs an output path")
                .remedy("pass --out or --dry-run")
        })?;
        crate::apply::validate_output_path(path)?;
        Some(path)
    };
    let bytes = crate::package::read_bytes(inputs.require("model")?)?;
    let outcome = export_cable_member_package(&bytes, &request).map_err(owner_failure)?;
    if let Some(path) = out {
        if std::path::Path::new(path)
            .file_name()
            .and_then(|leaf| leaf.to_str())
            != Some(outcome.filename.as_str())
        {
            return Err(Failure::invalid(
                "output_member_identity_mismatch",
                format!("output leaf must be {:?}", outcome.filename),
            )
            .remedy("use this exact source-member leaf inside a new cables directory"));
        }
        crate::apply::write_new(path, &outcome.bytes)?;
    }
    let mut receipt = serde_json::to_value(&outcome).expect("native receipt serializes");
    let mut truncated = Vec::new();
    crate::run::bound_value(&mut receipt["patches"], "patches", limit, &mut truncated);
    crate::run::bound_value(
        &mut receipt["cable_data_readback"],
        "cable_data_readback",
        limit,
        &mut truncated,
    );
    receipt["dry_run"] = json!(dry_run);
    receipt["persisted"] = json!(!dry_run);
    receipt["byte_len"] = json!(outcome.bytes.len());
    if let Some(path) = out {
        receipt["out"] = json!(path);
    }
    receipt["more"] = json!({"truncated": truncated});
    Ok(receipt)
}

pub fn render(data: &Value) -> String {
    format!(
        "{} {} (native cable v{}, {} bytes)\nsource {}\nemitted {}\npatches {}; native Cable Data acceptance pending\n",
        if data["dry_run"] == true {
            "preview"
        } else {
            "written"
        },
        data["filename"].as_str().unwrap_or(""),
        data["native_version"],
        data["byte_len"],
        data["source_digest"].as_str().unwrap_or(""),
        data["emitted_digest"].as_str().unwrap_or(""),
        data["patches"].as_array().map_or(0, Vec::len)
    )
}
