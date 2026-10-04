//! Thin local CLI adapter over the linked engine/exchange contract.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs, outcome::Failure};
use ds_grid_engine::composite::{LinkRequest, ReconcileRequest, SplitRequest};
use ds_grid_exchange::{
    linked_models::{self, LinkedCheckpoint, LinkedError},
    package::unpack,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const REQUEST: Arg = Arg::value(
    "request",
    "<request.json>",
    "Typed split, burst or derive request; schemas come from ds dsgrid describe --linked-models.",
);
pub const BUNDLE: Arg = Arg::value(
    "bundle",
    "<linked.dsgrid-links>",
    "One exact local linked checkpoint; opens every digest-pinned package. Omitted on reconcile, the derive request names the submodels.",
);
pub const OUT: Arg = Arg::value(
    "out",
    "<new.dsgrid-links>",
    "A new checkpoint path; an existing file is refused.",
);
pub const APPLY: Arg = Arg::switch(
    "apply",
    "Commit the candidate to --out; omitted, only plan and report.",
);
pub const PUBLICATION: Arg = Arg::value(
    "publication",
    "<project-publication.json>",
    "Explicit project, graph generation and every participant's project-model/head binding; --apply --yes publishes all versions atomically after local commit.",
);
pub const LANE: Arg = Arg::value(
    "lane",
    "<lane>",
    "Authenticated DS lane used only for project publication or project status.",
)
.choices(&["stable", "canary"])
.default("stable");
pub const EDIT: Arg = Arg::repeated(
    "edited",
    "<model-id=package.dsgrid>",
    "Replace a participant in the candidate with an exact edited .dsgrid package; never overwrites the baseline.",
);

const OWN_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "composite_publication_invalid",
        when: "a project binding, graph fence or split retirement disagrees with the exact checkpoint",
        remedy: "capture the explicit project and every participant/head, then review the dry-run publication request",
    },
    Refusal {
        code: "composite_invalid",
        when: "a typed request, ownership or canonical snapshot is invalid",
        remedy: "read the engine schema and correct the named input",
    },
    Refusal {
        code: "composite_conflict",
        when: "both owner and composite changed the same feature or source rows differ",
        remedy: "resolve the exact named features against the common baseline",
    },
    Refusal {
        code: "composite_owner_required",
        when: "a shared or new feature has no explicit owner",
        remedy: "name one source part in the request's ownership map",
    },
    Refusal {
        code: "composite_boundary_read_only",
        when: "a nonowner changed a boundary mirror",
        remedy: "make the edit in its owner or in the composite",
    },
    Refusal {
        code: "composite_generation_conflict",
        when: "the expected graph generation moved",
        remedy: "read status and prepare a fresh request; do not retry blindly",
    },
    Refusal {
        code: "composite_burst_limit",
        when: "the affected dependency closure exceeds the explicit budget",
        remedy: "review the affected features and authorize a larger bounded request",
    },
    Refusal {
        code: "linked_package_invalid",
        when: "a package, checkpoint or source CRS violates its attestation",
        remedy: "use exact compatible DS Grid packages and a verified checkpoint",
    },
    Refusal {
        code: "linked_file_unavailable",
        when: "a bounded local input cannot be read or output cannot be committed",
        remedy: "check the absolute paths, permissions and file sizes",
    },
    Refusal {
        code: "linked_output_exists",
        when: "--out already exists",
        remedy: "choose a new path; the prior generation remains available",
    },
];
pub const REFUSALS: &[Refusal] = &refusals();
const fn refusals() -> [Refusal; OWN_REFUSALS.len() + crate::project::SHARED] {
    let mut result = [OWN_REFUSALS[0]; OWN_REFUSALS.len() + crate::project::SHARED];
    let mut i = 0;
    while i < OWN_REFUSALS.len() {
        result[i] = OWN_REFUSALS[i];
        i += 1;
    }
    crate::project::with_shared(result, OWN_REFUSALS.len())
}

macro_rules! command {
    ($name:ident, $id:literal, $verb:literal, $effect:expr, $summary:literal, $purpose:literal, $args:expr, $example:literal) => {
        pub static $name: Command = Command {
            id: $id, path: &["dsgrid", "model", $verb], contract: 1,
            summary: $summary, purpose: $purpose, chapter: Chapter::GridModel,
            effect: $effect, authority: Authority::HeadlessProject, execution: Execution::Sync,
            args: $args, output: "Dry-run/apply status, composite/parts, generation, canonical authored digest, affected counts and exact candidate digest. --publication adds the reviewable atomic project request; applying returns its verified immutable version vector. Source files are immutable.",
            examples: &[Example { command: $example, note: "Dry run by default; add --apply --out <new-path> to commit a local candidate.", runnable: false }],
            refusals: REFUSALS, reference: None, search: &["composite", "submodels", "reconciliation", "split", "linked"], requires: Requires::Server,
            availability: || Availability::Available,
        };
    }
}
command!(
    SPLIT,
    "dsgrid.model.split",
    "split",
    Effect::GlobalWrite,
    "Partition a submodel into linked owner parts with a digest proof.",
    "Select canonical features, planar points in a polygon, or an exact scalar attribute through the engine's SplitRequest. The combined model of the parts is automatic: its identity derives from the part identities and nobody creates, combines or deletes it. The original policy explicitly retires the original identity or retains it as one part. Reference ties are read-only mirrors. Plans by default; --apply writes one complete local checkpoint. --publication captures every explicit project binding and head fence; --apply then publishes one verified atomic generation, including optional retirement of the split original.",
    &[
        Arg::value(
            "package",
            "<source.dsgrid>",
            "Exact source .dsgrid package."
        )
        .required(),
        REQUEST.required(),
        OUT,
        APPLY,
        PUBLICATION,
        LANE
    ],
    "ds dsgrid model split --package /work/original.dsgrid --request /work/split.json --output json"
);
command!(
    RECONCILE,
    "dsgrid.model.reconcile",
    "reconcile",
    Effect::GlobalWrite,
    "Keep the automatic combined model in step in one bounded burst.",
    "The combined model is automatic: it is derived from its submodels and nobody creates, combines or deletes it. With --bundle, the burst request pins expected_generation and max_affected_features; edits are compared with the saved baseline, all owner/combined conflicts are named, span/corridor and section calculations are localized, and unavailable sag/clearance inputs are reported. Without --bundle, the derive request names 2..100 exact submodel packages and explicit owners for shared features, and derives generation zero of their combined model. Default is a dry run. --apply commits graph and every package together to a new local checkpoint; --publication additionally stages exact bytes and publishes one atomic project version vector.",
    &[
        BUNDLE,
        REQUEST.required(),
        EDIT,
        OUT,
        APPLY,
        PUBLICATION,
        LANE
    ],
    "ds dsgrid model reconcile --bundle /work/linked.dsgrid-links --request /work/burst.json --edited north=/work/north.dsgrid --apply --out /work/generation-2.dsgrid-links --output json"
);
command!(
    STATUS,
    "dsgrid.model.status",
    "status",
    Effect::ReadOnly,
    "Verify a linked checkpoint and show its coherent generation.",
    "Reads one attested local checkpoint and every package, verifies ownership and boundary mirrors against the common baseline, and reports generation, composite, parts, digest and bounded counts. With --edited and --request, inspect a pending burst without committing it. Alternatively --project --graph --generation reads and verifies one immutable published vector. Does not write or publish anything.",
    &[
        BUNDLE,
        REQUEST,
        EDIT,
        Arg::value(
            "project",
            "<project-id>",
            "Read one immutable published graph generation in this explicit project."
        ),
        Arg::value(
            "graph",
            "<graph-id>",
            "Exact published graph identity; required with --project."
        ),
        Arg::value(
            "generation",
            "<integer>",
            "Exact immutable graph generation, including zero; required with --project."
        ),
        LANE
    ],
    "ds dsgrid model status --bundle /work/linked.dsgrid-links --output json"
);

fn failure(error: LinkedError) -> Failure {
    match error {
        LinkedError::Engine(error) => {
            let failure = if matches!(
                error.code.as_str(),
                "composite_conflict" | "composite_generation_conflict"
            ) {
                Failure::conflict(&error.code, &error.message)
            } else {
                Failure::invalid(&error.code, &error.message)
            };
            failure.detail(json!({"features": error.features})).remedy(
                "resolve the named features or input using the engine's linked model schema",
            )
        }
        error => Failure::invalid("linked_package_invalid", error.to_string())
            .remedy("use exact compatible packages and a verified checkpoint"),
    }
}

pub(super) fn read(path: &str, max: u64) -> Result<Vec<u8>, Failure> {
    use std::io::Read;
    let file = std::fs::File::open(path)
        .map_err(|e| Failure::invalid("linked_file_unavailable", format!("{path}: {e}")))?;
    if !file
        .metadata()
        .map_err(|e| Failure::invalid("linked_file_unavailable", e.to_string()))?
        .is_file()
    {
        return Err(Failure::invalid(
            "linked_file_unavailable",
            "input must be a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Failure::invalid("linked_file_unavailable", e.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(Failure::invalid(
            "linked_file_unavailable",
            "input exceeds its read bound",
        ));
    }
    Ok(bytes)
}

fn request<T: serde::de::DeserializeOwned>(inputs: &Inputs) -> Result<T, Failure> {
    let bytes = read(inputs.require("request")?, 8 * 1024 * 1024)?;
    serde_json::from_slice(&bytes).map_err(|e| Failure::invalid("composite_invalid", e.to_string()))
}

pub fn split(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let bytes = read(
        inputs.require("package")?,
        linked_models::MAX_LINKED_BYTES as u64,
    )?;
    let checkpoint =
        linked_models::split_package(&bytes, &request::<SplitRequest>(inputs)?).map_err(failure)?;
    finish(inputs, checkpoint, None, Some(&bytes))
}

/// Derive generation zero of the automatic combined model from exact
/// submodel packages. Nobody names the combined model: the engine derives
/// its identity from the part identities.
fn derive(inputs: &Inputs) -> Result<Value, Failure> {
    if !inputs.repeated("edited").is_empty() {
        return Err(Failure::invalid(
            "composite_invalid",
            "--edited requires an existing --bundle",
        ));
    }
    let request: LinkRequest = request(inputs)?;
    if !(2..=100).contains(&request.sources.len()) {
        return Err(Failure::invalid(
            "composite_invalid",
            "the derive request needs 2..100 submodel sources",
        ));
    }
    let mut packages = BTreeMap::new();
    for path in request.sources {
        let bytes = read(&path, linked_models::MAX_LINKED_BYTES as u64)?;
        let package = unpack(&bytes).map_err(|e| failure(LinkedError::Package(e.to_string())))?;
        let id = package.manifest.model.model_id.to_string();
        if packages.insert(id, package).is_some() {
            return Err(Failure::invalid(
                "composite_invalid",
                "duplicate source model identity",
            ));
        }
    }
    let checkpoint =
        linked_models::link_packages(packages, &request.shared_owners).map_err(failure)?;
    finish(inputs, checkpoint, None, None)
}

fn candidate(inputs: &Inputs) -> Result<LinkedCheckpoint, Failure> {
    let bytes = read(
        inputs.require("bundle")?,
        linked_models::MAX_LINKED_BYTES as u64,
    )?;
    let mut checkpoint = linked_models::decode(&bytes).map_err(failure)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut edits = Vec::new();
    for edited in inputs.repeated("edited") {
        let (id, path) = edited.split_once('=').ok_or_else(|| {
            Failure::invalid("composite_invalid", "--edited requires model-id=path")
        })?;
        if !checkpoint.state.models.contains_key(id) || !seen.insert(id) {
            return Err(Failure::invalid(
                "composite_invalid",
                "edited model is absent or repeated",
            ));
        }
        let bytes = read(path, linked_models::MAX_LINKED_BYTES as u64)?;
        let package = unpack(&bytes).map_err(|e| failure(LinkedError::Package(e.to_string())))?;
        if package.manifest.model.model_id.as_str() != id {
            return Err(Failure::invalid(
                "linked_package_invalid",
                "edited participant identity differs from --edited",
            ));
        }
        edits.push(bytes);
    }
    linked_models::stage_edits(&mut checkpoint, &edits).map_err(failure)?;
    Ok(checkpoint)
}

pub fn reconcile(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    if inputs.value("bundle").is_none() {
        return derive(inputs);
    }
    let checkpoint = candidate(inputs)?;
    let (checkpoint, report) =
        linked_models::reconcile_checkpoint(&checkpoint, &request::<ReconcileRequest>(inputs)?)
            .map_err(failure)?;
    finish(inputs, checkpoint, Some(report), None)
}

pub fn status(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if let Some(project) = inputs.value("project") {
        if inputs.value("bundle").is_some()
            || inputs.value("request").is_some()
            || !inputs.repeated("edited").is_empty()
        {
            return Err(Failure::invalid(
                "composite_publication_invalid",
                "project status requires an exact graph/generation and cannot mix local candidate arguments",
            ));
        }
        let graph = inputs.value("graph").ok_or_else(|| {
            Failure::invalid(
                "composite_publication_invalid",
                "--project requires --graph and --generation",
            )
        })?;
        let generation = inputs
            .value("generation")
            .and_then(|n| n.parse::<u64>().ok())
            .ok_or_else(|| {
                Failure::invalid(
                    "composite_publication_invalid",
                    "--generation must name an explicit nonnegative integer",
                )
            })?;
        let mut data = ds_cli_auth::grid_models_for_project(
            inputs.value("lane").unwrap_or("stable"),
            project,
            &ds_cli_auth::GridModelsCommand::LinkedGeneration {
                graph: graph.into(),
                generation,
            },
        )?
        .data;
        data["status"] = json!("verified");
        data["scope"] = json!("project_linked_generation");
        data["composite"] = data["composite_model_id"].clone();
        data["parts"] = data["part_model_ids"].clone();
        return Ok(data);
    }
    if inputs.value("graph").is_some()
        || inputs.value("generation").is_some()
        || inputs.value("bundle").is_none()
    {
        return Err(Failure::invalid(
            "composite_publication_invalid",
            "status requires --bundle, or --project --graph --generation",
        ));
    }
    if inputs.value("request").is_some() {
        return reconcile(inputs, context);
    }
    if !inputs.repeated("edited").is_empty() {
        return Err(Failure::invalid(
            "composite_invalid",
            "pending edits need an explicit reconciliation request",
        ));
    }
    let mut result = finish(inputs, candidate(inputs)?, None, None)?;
    result["status"] = json!("verified");
    Ok(result)
}

fn finish(
    inputs: &Inputs,
    checkpoint: LinkedCheckpoint,
    report: Option<ds_grid_engine::composite::BatchReport>,
    origin: Option<&[u8]>,
) -> Result<Value, Failure> {
    let graph = &checkpoint.state.graph;
    let mut result = json!({"status": "dry_run", "scope": "local_checkpoint", "composite": graph.composite,
        "parts": graph.parts, "generation": graph.generation, "canonical_digest": graph.canonical_digest,
        "features": graph.features.len(), "project_published": false,
        "counts": report.as_ref().map(|r| json!({"dirty": r.dirty.len(), "affected": r.affected.len(), "sections": r.sections.len(), "alignments": r.alignments.len(), "spans": r.spans.len(), "calculations_pending": r.calculations_pending})),
        "calculation_findings": checkpoint.calculations.as_ref().map(|c| json!({"sag_unavailable": c.sag_tension.iter().filter(|s| s.unavailable.is_some()).count(), "clearance_unavailable": c.clearance_unavailable, "sections_unavailable": c.clearance.as_ref().map(|r| r.sections_unavailable.len())}))});
    // Encoding/reopening proves that a dry-run candidate is concretely
    // reviewable, including package validation and resource attestations.
    let bytes = linked_models::encode(&checkpoint).map_err(failure)?;
    linked_models::decode(&bytes).map_err(failure)?;
    result["candidate_sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
    result["candidate_byte_length"] = json!(bytes.len());
    let publication = super::composite_publication::prepare(inputs, &bytes, origin)?;
    if let Some((_, _, plan)) = &publication {
        result["publication"] = plan.clone();
    }
    if inputs.switch("apply") {
        let path = std::path::Path::new(
            inputs
                .value("out")
                .ok_or_else(|| Failure::invalid("composite_invalid", "--apply requires --out"))?,
        );
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let mut staging = tempfile::NamedTempFile::new_in(parent)
            .map_err(|e| Failure::invalid("linked_file_unavailable", e.to_string()))?;
        use std::io::Write;
        staging
            .write_all(&bytes)
            .and_then(|_| staging.as_file().sync_all())
            .map_err(|e| Failure::invalid("linked_file_unavailable", e.to_string()))?;
        staging.persist_noclobber(path).map_err(|e| {
            Failure::invalid(
                if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                    "linked_output_exists"
                } else {
                    "linked_file_unavailable"
                },
                e.to_string(),
            )
        })?;
        result["status"] = json!("applied");
        result["out"] = json!(path);
        if let Some((project, command, _)) = publication {
            let receipt = ds_cli_auth::grid_models_for_project(
                inputs.value("lane").unwrap_or("stable"),
                &project,
                &command,
            )
            .map_err(|error| {
                let publication_refusal = error.detail_value().cloned();
                error.detail(json!({
                    "local_checkpoint": path,
                    "candidate_sha256": result["candidate_sha256"],
                    "project_published": false,
                    "publication_refusal": publication_refusal,
                }))
            })?;
            result["publication"] = receipt.data;
            result["scope"] = json!("project_linked_generation");
            result["project_published"] = json!(true);
        }
    }
    Ok(result)
}

pub fn render(data: &Value) -> String {
    let mut text = format!(
        "{}: composite {}, generation {}",
        data["status"], data["composite"], data["generation"]
    );
    if let Some(count) = data["features"].as_u64() {
        text.push_str(&format!(", {count} features"));
    }
    text
}
