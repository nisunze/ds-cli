//! Explicit machine-global immutable recipe files. Definitions contain no
//! project holdings or credentials; each invocation binds its own sources.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_command_kernel::workflow_documents::{self, Definition};
use ds_network::vector::workflow::{self, WorkflowOperations};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
const LIBRARY: Arg = Arg::value(
    "library",
    "<directory>",
    "Reusable local global recipe library; no credentials or source holdings.",
)
.required();
const NAME: Arg = Arg::value("name", "<name>", "Plain reusable recipe name.").required();
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    args: &'static [Arg],
    effect: Effect,
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "Store or inspect exact immutable definitions for the existing Rust workflow DAG. Save pins operation versions and refuses embedded acquisition/credential fields and document/layer defaults. Bind exact sources on each run; local global reuse grants no project authority.",
        chapter: Chapter::Data,
        effect,
        authority: Authority::None,
        execution: Execution::Sync,
        args,
        output: "Verified immutable definition and digest, or bounded library entries. No project computation or publication.",
        examples: &[],
        refusals: &[
            Refusal {
                code: "workflow_document_invalid",
                when: "A document, identifier or reusable recipe is invalid.",
                remedy: "Read the workflow contract and bind source holdings per run.",
            },
            Refusal {
                code: "workflow_source_changed",
                when: "An immutable library entry no longer matches its digest.",
                remedy: "Restore the exact immutable entry; never mutate it in place.",
            },
            Refusal {
                code: "workflow_op_version_unsupported",
                when: "A pinned operation contract is unavailable.",
                remedy: "Use an installed matching native command contract or save a reviewed new revision.",
            },
            Refusal {
                code: "workflow_budget_exceeded",
                when: "Definition or library listing exceeds bounds.",
                remedy: "Split the definition or select one exact recipe.",
            },
            crate::UNREADABLE,
            crate::OUTPUT_REFUSED,
        ],
        reference: Some("docs/reference/data.md"),
        search: &["workflow", "global", "recipe", "reuse", "revision"],
        requires: Requires::Server,
        availability: crate::available,
    }
}
pub static SAVE: Command = command(
    "data.vector.workflow.save",
    &["data", "vector", "workflow", "save"],
    "Save an immutable reusable native recipe.",
    &[
        LIBRARY,
        NAME,
        Arg::value("file", "<workflow.json>", "Draft recipe JSON.").required(),
        Arg::value("note", "<text>", "Bounded change note."),
    ],
    Effect::LocalFileWrite,
);
pub static SHOW: Command = command(
    "data.vector.workflow.show",
    &["data", "vector", "workflow", "show"],
    "Read one exact immutable global recipe.",
    &[
        LIBRARY,
        NAME,
        Arg::value("digest", "<sha256>", "Exact recipe digest.").required(),
    ],
    Effect::ReadOnly,
);
pub static LIST: Command = command(
    "data.vector.workflow.list",
    &["data", "vector", "workflow", "list"],
    "List bounded immutable global recipe revisions.",
    &[LIBRARY],
    Effect::ReadOnly,
);
fn failure(message: String) -> Failure {
    let code = message
        .split(':')
        .next()
        .filter(|c| c.starts_with("workflow_"))
        .unwrap_or("workflow_document_invalid")
        .to_owned();
    match code.as_str() {
        "workflow_source_changed"
        | "workflow_authority_missing"
        | "workflow_budget_exceeded"
        | "workflow_op_version_unsupported"
        | "workflow_document_invalid" => Failure::invalid(code, message),
        _ => Failure::invalid("workflow_document_invalid", message),
    }
}
fn read(path: &Path) -> Result<Value, Failure> {
    let b = ds_design_workspace::read_file(path, workflow_documents::MAX_BYTES)
        .map_err(|e| Failure::invalid(crate::UNREADABLE.code, e.to_string()))?;
    serde_json::from_slice(&b).map_err(|e| failure(e.to_string()))
}
fn versions(document: &Value) -> Result<BTreeMap<String, Value>, Failure> {
    let mut versions = BTreeMap::new();
    for step in document["steps"]
        .as_array()
        .ok_or_else(|| failure("workflow_document_invalid: steps missing".into()))?
    {
        let id = step["tool"]
            .as_str()
            .ok_or_else(|| failure("workflow_document_invalid: tool missing".into()))?;
        let op = super::workflow_operations::NativeOperations
            .descriptor(id)
            .or_else(|| ds_network::vector::describe(Some(id)).ok())
            .ok_or_else(|| failure("workflow_op_version_unsupported: unknown operation".into()))?;
        if op.get("version").is_none() {
            return Err(failure(
                "workflow_op_version_unsupported: unversioned operation".into(),
            ));
        }
        versions.insert(id.into(), op["version"].clone());
    }
    Ok(versions)
}
pub fn save(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let document = read(Path::new(i.require("file")?))?;
    workflow::validate_with_operations(
        document.clone(),
        json!({}),
        BTreeMap::new(),
        &super::workflow_operations::NativeOperations,
    )
    .map_err(|e| {
        failure(format!(
            "workflow_document_invalid: {} at {}",
            e.message, e.path
        ))
    })?;
    let definition = workflow_documents::capture(
        i.require("name")?,
        i.value("note").unwrap_or(""),
        document.clone(),
        versions(&document)?,
    )
    .map_err(failure)?;
    let path = Path::new(i.require("library")?)
        .join(&definition.name)
        .join(format!("{}.json", definition.digest));
    let bytes = serde_json::to_vec(&definition).unwrap();
    if path.exists() {
        let existing: Definition =
            serde_json::from_value(read(&path)?).map_err(|e| failure(e.to_string()))?;
        workflow_documents::verify(&existing).map_err(failure)?;
        if serde_json::to_value(&existing).unwrap() != serde_json::to_value(&definition).unwrap() {
            return Err(failure(
                "workflow_source_changed: existing immutable entry differs".into(),
            ));
        }
    } else {
        std::fs::create_dir_all(path.parent().unwrap())
            .map_err(|e| Failure::failed(crate::OUTPUT_REFUSED.code, e.to_string()))?;
        ds_design_workspace::write_new(&path, &bytes)
            .map_err(|e| Failure::failed(crate::OUTPUT_REFUSED.code, e.to_string()))?;
    }
    Ok(
        json!({"path":path,"definition":definition,"scope":"machine-global","source_bindings_retained":false}),
    )
}
pub fn show(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_command_kernel::design::identifier(i.require("name")?).map_err(failure)?;
    let digest = i.require("digest")?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(failure(
            "workflow_document_invalid: digest must be exact SHA-256".into(),
        ));
    }
    let path = Path::new(i.require("library")?)
        .join(i.require("name")?)
        .join(format!("{digest}.json"));
    let definition: Definition =
        serde_json::from_value(read(&path)?).map_err(|e| failure(e.to_string()))?;
    workflow_documents::verify(&definition).map_err(failure)?;
    if definition.digest != digest || definition.name != i.require("name")? {
        return Err(failure(
            "workflow_source_changed: definition name/digest differs".into(),
        ));
    }
    Ok(serde_json::to_value(definition).unwrap())
}
pub fn list(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut entries = vec![];
    for dir in std::fs::read_dir(i.require("library")?)
        .map_err(|e| Failure::invalid(crate::UNREADABLE.code, e.to_string()))?
    {
        let dir = dir.map_err(|e| failure(e.to_string()))?;
        if !dir
            .file_type()
            .map_err(|e| failure(e.to_string()))?
            .is_dir()
        {
            continue;
        }
        for file in std::fs::read_dir(dir.path()).map_err(|e| failure(e.to_string()))? {
            let path = file.map_err(|e| failure(e.to_string()))?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if entries.len() >= 1000 {
                return Err(failure(
                    "workflow_budget_exceeded: library listing exceeds 1000 revisions".into(),
                ));
            }
            let definition: Definition =
                serde_json::from_value(read(&path)?).map_err(|e| failure(e.to_string()))?;
            workflow_documents::verify(&definition).map_err(failure)?;
            entries.push(json!({"name":definition.name,"digest":definition.digest,"note":definition.note,"path":path}));
        }
    }
    entries.sort_by_key(|v| {
        (
            v["name"].as_str().unwrap_or("").to_owned(),
            v["digest"].as_str().unwrap_or("").to_owned(),
        )
    });
    Ok(json!({"scope":"machine-global","revisions":entries}))
}
