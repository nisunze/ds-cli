//! Compile any supported engineering standards source through the shared
//! template compiler used by model creation and the browser host.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::{
    ArtifactKind, LibraryReleaseOptions, compile_model_template_source, unpack_model_template,
};
use ds_grid_model::EntityId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

pub static COMMAND: Command = Command {
    id: "dsgrid.template.compile",
    path: &["dsgrid", "template", "compile"],
    contract: 1,
    summary: "Compile a standards source into one verified .dsgrid-template.",
    purpose: "Uses the same standards compiler as New DS Grid model. A complete .dsgrid, PLS-CADD backup, supported native engineering library, or existing template becomes a standards-only .dsgrid-template. Route, terrain, placements, results and workspace state are excluded. The template is the exact input to dsgrid create --standards; no separate model semantics are authored.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("source", "<path>", "One supported standards source file.").required(),
        Arg::value(
            "out",
            "<path>",
            "New .dsgrid-template output; never overwritten.",
        )
        .required(),
    ],
    output: "Verified template digest, source digest, engineering table counts, and persisted artifact path.",
    examples: &[Example {
        command: "ds dsgrid template compile --source ./reference.dsgrid --out ./standards.dsgrid-template --output json",
        note: "Compile standards through the shared model-template authority.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "source_unreadable",
            when: "the source cannot be read as a bounded regular file",
            remedy: "name one readable supported source file",
        },
        Refusal {
            code: "template_refused",
            when: "the compiler rejects the source or standards/resource closure",
            remedy: "inspect the named source and its dependencies",
        },
        Refusal {
            code: "output_exists",
            when: "the output path exists",
            remedy: "choose a new output path",
        },
        Refusal {
            code: "output_parent_missing",
            when: "the output parent does not exist",
            remedy: "create the intended directory",
        },
        Refusal {
            code: "output_unwritable",
            when: "the output cannot be persisted",
            remedy: "check permissions and free space",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &["standards", "template", "model creation"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = inputs.require("source")?;
    let out = inputs.require("out")?;
    crate::apply::validate_output_path(out)?;
    let source_bytes = crate::package::read_bytes(source).map_err(|error| {
        Failure::invalid("source_unreadable", error.to_string())
            .remedy("name one readable supported source file")
    })?;
    let source_name = Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Failure::invalid("source_unreadable", "source needs a UTF-8 filename"))?;
    let source_sha = format!("{:x}", Sha256::digest(&source_bytes));
    let short = &source_sha[..16];
    let options = LibraryReleaseOptions {
        kind: ArtifactKind::Template,
        artifact_id: EntityId::new(format!("standards-{short}")).expect("digest identity"),
        revision_id: EntityId::new(format!("standards-revision-{short}")).expect("digest identity"),
        parent_revision_ids: Vec::new(),
        dependency_pins: Vec::new(),
        assets: Vec::new(),
    };
    let template =
        compile_model_template_source(source_name, &source_bytes, &options).map_err(|error| {
            Failure::invalid("template_refused", error.to_string())
                .remedy("inspect the source and its engineering/resource dependencies")
        })?;
    let release = unpack_model_template(&template)
        .map_err(|error| Failure::failed("template_refused", error.to_string()))?;
    crate::apply::write_new(out, &template)?;
    Ok(json!({
        "source": {"path": source, "sha256": format!("sha256:{source_sha}")},
        "template": {
            "artifact_kind": "template",
            "structure_types": release.snapshot.structure_types.len(),
            "available_structures": release.snapshot.available_structures.len(),
            "cables": release.snapshot.cables.len(),
            "criterion_sets": release.snapshot.criterion_sets.len(),
            "feature_codes": release.snapshot.feature_codes.len(),
            "resources": release.snapshot.resources.len(),
            "assets": release.assets.len(),
        },
        "persisted": true,
        "artifact": {"path": out, "byte_len": template.len(), "sha256": format!("sha256:{:x}", Sha256::digest(&template))},
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "compiled {} from {}\n",
        data["artifact"]["path"].as_str().unwrap_or("?"),
        data["source"]["path"].as_str().unwrap_or("?")
    )
}

pub static APPLY: Command = Command {
    id: "dsgrid.template.apply",
    path: &["dsgrid", "template", "apply"],
    contract: 1,
    summary: "Apply a verified structure standards template to a cleared model.",
    purpose: "Replaces only structure definitions, analytical strength tables, exact native resources, and the ordered spotting catalog in a cleared .dsgrid working model. Preserves its route, terrain, criteria, policy, duty profiles and identity. Requires one .dsgrid-template from the shared standards compiler and an exact authored revision. Writes a new package revision.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "Cleared source .dsgrid package.").required(),
        Arg::value("template", "<path>", "Verified canonical .dsgrid-template.").required(),
        Arg::value("revision", "<rev>", "Expected authored source revision.").required(),
        Arg::value("out", "<path>", "New .dsgrid output; never overwritten.").required(),
    ],
    output: "New model revision, template digest, structure catalog counts, and preserved design counts.",
    examples: &[Example {
        command: "ds dsgrid template apply --model ./cleared.dsgrid --template ./standards.dsgrid-template --revision rev:... --out ./working.dsgrid --output json",
        note: "Update a cleared route from the same template format used for model creation.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "revision_conflict",
            when: "the model head differs from --revision",
            remedy: "reinspect the exact working model",
        },
        Refusal {
            code: "model_not_cleared",
            when: "placements or stringing remain",
            remedy: "clear the working model before changing structure standards",
        },
        Refusal {
            code: "template_refused",
            when: "the template is invalid or has incompatible structure/resource IDs",
            remedy: "compile compatible server standards through dsgrid template compile",
        },
        Refusal {
            code: "model_apply_failed",
            when: "the resulting model or resource closure fails validation",
            remedy: "inspect the named validation issue; the source is unchanged",
        },
        Refusal {
            code: "output_exists",
            when: "the output path exists",
            remedy: "choose a new output path",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &["standards", "template", "spotting catalog"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn apply(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let out = inputs.require("out")?;
    crate::apply::validate_output_path(out)?;
    let model = crate::package::read_bytes(inputs.require("model")?)?;
    let template = crate::package::read_bytes(inputs.require("template")?)?;
    let outcome = ds_grid_exchange::standards_apply::apply_structure_standards_template(
        &model,
        &template,
        inputs.require("revision")?,
    )
    .map_err(|detail| {
        let code = if detail.starts_with("revision_conflict:") {
            "revision_conflict"
        } else if detail.starts_with("model_not_cleared:") {
            "model_not_cleared"
        } else if detail.starts_with("template_") || detail.starts_with("structure_") {
            "template_refused"
        } else {
            "model_apply_failed"
        };
        Failure::invalid(code, detail).remedy("inspect the model and verified standards template")
    })?;
    crate::apply::write_new(out, &outcome.bytes)?;
    let mut receipt = serde_json::to_value(&outcome).expect("typed receipt serializes");
    receipt["persisted"] = json!(true);
    receipt["artifact"] = json!({"path": out, "byte_len": outcome.bytes.len(), "sha256": format!("sha256:{:x}", Sha256::digest(&outcome.bytes))});
    Ok(receipt)
}
