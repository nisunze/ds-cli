//! Native model/library lifecycle. Cloud project writes remain the existing
//! explicit-project publish-version operation with its expected-head fence.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::model_library;
use ds_grid_exchange::{bundle_digest, unpack_library};
use ds_grid_model::{DependencyPin, EntityId};
use serde_json::{Value, json};
use std::path::Path;

const MODEL: Arg = Arg::value(
    "model",
    "<path.dsgrid>",
    "Exact source model package; no active-model assumption.",
)
.required();
const RELEASE: Arg = Arg::value(
    "release",
    "<path.dsgrid-library>",
    "Exact verified immutable library artifact.",
)
.required();
const EXPECTED: Arg = Arg::value(
    "expected-sha256",
    "<sha256:hex>",
    "SHA-256 of the exact source package or clone source; refuses stale bytes.",
)
.required();
const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "New artifact path; existing files are never overwritten.",
)
.required();
const ID: Arg = Arg::value("library-id", "<id>", "Stable library identity.").required();
const VERSION: Arg = Arg::value(
    "library-version",
    "<id>",
    "Exact immutable release identity.",
)
.required();
const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "digest_conflict",
        when: "source or release SHA-256 differs from its fence",
        remedy: "inspect the intended exact source and pass its verified digest",
    },
    Refusal {
        code: "no_cloud_equivalent",
        when: "a selected definition is absent or differs in the exact release",
        remedy: "review a separate native replacement or choose an exact equivalent; never substitute by name",
    },
    Refusal {
        code: "library_already_pinned",
        when: "the model already pins this library identity",
        remedy: "inspect and deliberately detach that exact pin before changing the release",
    },
    Refusal {
        code: "library_pin_conflict",
        when: "detachment names a different release or content root",
        remedy: "show this exact model and use its complete immutable pin",
    },
    Refusal {
        code: "library_identity_conflict",
        when: "a clone reuses its source artifact identity",
        remedy: "choose a distinct library identity for the deliberate clone",
    },
    Refusal {
        code: "library_selection_invalid",
        when: "identity, element selection or paging is invalid or unbounded",
        remedy: "use valid identities, 1..5000 elements and paging within 0..5000",
    },
    Refusal {
        code: "library_resolution_required",
        when: "the release has unresolved dependencies",
        remedy: "acquire and verify its exact dependencies before adoption",
    },
    Refusal {
        code: "library_kind_invalid",
        when: "a defaults template is offered as an asset library",
        remedy: "create an asset-bundle release from the reviewed model",
    },
    Refusal {
        code: "native_bytes_missing",
        when: "a required native resource has no exact bytes",
        remedy: "recover the digest-pinned native member and its dependency closure",
    },
    Refusal {
        code: "library_operation_failed",
        when: "native package or library verification refuses the operation",
        remedy: "validate the source and inspect the returned native refusal; nothing was published",
    },
    Refusal {
        code: "library_path_not_found",
        when: "an explicit input path is unreadable",
        remedy: "use the intended existing local artifact",
    },
    Refusal {
        code: "library_path_not_file",
        when: "an input is not a file",
        remedy: "pass the exact model or release file",
    },
    Refusal {
        code: "library_file_too_large",
        when: "an input exceeds the local read bound",
        remedy: "use a bounded portable model/library artifact",
    },
    Refusal {
        code: "library_read_failed",
        when: "local input reading fails",
        remedy: "check access to the explicit artifact",
    },
    Refusal {
        code: "output_exists",
        when: "the output path already exists",
        remedy: "choose a new immutable output path",
    },
    Refusal {
        code: "output_unwritable",
        when: "the output cannot be created",
        remedy: "choose an accessible new output path",
    },
];

const SEARCH: &[&str] = &[
    "interoperability",
    "provider",
    "model library",
    "cloud equivalence",
];
/// A declared term must say what the id or summary does not already say.
const SEARCH_WHERE_SUMMARY_SAYS_INTEROPERABILITY: &[&str] =
    &["provider", "model library", "cloud equivalence"];

#[allow(clippy::too_many_arguments)]
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    purpose: &'static str,
    args: &'static [Arg],
    examples: &'static [Example],
    effect: Effect,
    search: &'static [&'static str],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose,
        chapter: Chapter::GridModel,
        effect,
        authority: Authority::None,
        execution: Execution::Sync,
        args,
        output: "Exact model/library digests and immutable pins, retained-byte evidence and explicit solver-approval status. Local artifacts require separate authorized cloud publication.",
        examples,
        refusals: REFUSALS,
        reference: Some("docs/reference/library.md"),
        search,
        requires: Requires::Server,
        availability: || Availability::Available,
    }
}
pub static CREATE: Command = command(
    "library.model.create",
    &["library", "model", "create"],
    "Create an interoperability library from an exact model revision.",
    "Captures reusable definitions and exact native resource bytes without project routes or unrelated customer files. Provider-neutral: PLS-CADD is an exchange adapter, not an impersonated solver. Records source revision evidence; never certifies strength or case applicability.",
    &[MODEL, EXPECTED, ID, VERSION, OUT],
    &[Example {
        command: "ds library model create --model ./reviewed.dsgrid --expected-sha256 sha256:<exact> --library-id reusable --library-version release-1 --out ./reusable.dsgrid-library --output json",
        note: "Replace the illustrative digest with this model's exact SHA-256.",
        runnable: false,
    }],
    Effect::LocalFileWrite,
    SEARCH_WHERE_SUMMARY_SAYS_INTEROPERABILITY,
);
pub static ATTACH: Command = command(
    "library.model.attach",
    &["library", "model", "attach"],
    "Pin exact library equivalents and retain a model's native evidence.",
    "Fences both artifacts, checks exact semantic definitions, pins the immutable release and carries a verified offline cache. Keeps authored model definitions and native bytes unchanged. A different same-named member refuses no_cloud_equivalent. Publish the new model separately with explicit project and expected head.",
    &[
        MODEL,
        EXPECTED,
        RELEASE,
        Arg::value(
            "expected-library-sha256",
            "<sha256:hex>",
            "Exact library bundle SHA-256.",
        )
        .required(),
        Arg::repeated(
            "element",
            "<entity-id>",
            "Adopt these exact reusable elements; omitted selects all extractable release elements.",
        ),
        OUT,
    ],
    &[Example {
        command: "ds library model attach --model ./reviewed.dsgrid --expected-sha256 sha256:<model> --release ./reusable.dsgrid-library --expected-library-sha256 sha256:<release> --out ./pinned.dsgrid --output json",
        note: "Use exact artifact digests; no cloud head changes.",
        runnable: false,
    }],
    Effect::LocalFileWrite,
    SEARCH,
);
pub static DETACH: Command = command(
    "library.model.detach",
    &["library", "model", "detach"],
    "Detach an exact library pin while retaining native evidence.",
    "Requires the source package digest and complete artifact/revision/content-root pin. Produces a new model revision, retains cached releases and native evidence, and changes no project history. Cloud publication remains separately authorized and head-fenced.",
    &[
        MODEL,
        EXPECTED,
        ID,
        VERSION,
        Arg::value(
            "content-root",
            "<sha256:hex>",
            "Exact pinned library content root.",
        )
        .required(),
        OUT,
    ],
    &[Example {
        command: "ds library model detach --model ./pinned.dsgrid --expected-sha256 sha256:<model> --library-id reusable --library-version release-1 --content-root sha256:<root> --out ./detached.dsgrid --output json",
        note: "Take the complete pin from model show.",
        runnable: false,
    }],
    Effect::LocalFileWrite,
    SEARCH,
);
pub static CLONE: Command = command(
    "library.model.clone",
    &["library", "model", "clone"],
    "Clone an exact interoperability library under a new identity.",
    "Preserves reusable definitions and native bytes, records the source release and bundle digest, and requires a distinct library identity. No customer project is selected or authorized by cloning; adoption/publication in another project must use its own explicit authorization.",
    &[RELEASE, EXPECTED, ID, VERSION, OUT],
    &[Example {
        command: "ds library model clone --release ./reusable.dsgrid-library --expected-sha256 sha256:<release> --library-id deliberate-clone --library-version release-1 --out ./clone.dsgrid-library --output json",
        note: "Exact clone evidence without a cloud write.",
        runnable: false,
    }],
    Effect::LocalFileWrite,
    SEARCH_WHERE_SUMMARY_SAYS_INTEROPERABILITY,
);
pub static SHOW: Command = command(
    "library.model.show",
    &["library", "model", "show"],
    "Read model library pins, native equivalence and export admission.",
    "Verifies the exact package and its immutable cached releases, then reports native definition usage, exact resource IDs, affected placed supports/sections, capacity basis pins and unresolved external dependencies alongside per-resource equivalence and retained release history. Retained native bytes expose the export owner's unsourced-loads or DS-authored-capacity marker without qualification. Definition and resource collections share the declared paging window and have separate totals/cursors. Unused definitions are not authorization to prune; byte equivalence never implies solver approval or engineering acceptance.",
    &[
        MODEL,
        EXPECTED,
        Arg::value(
            "offset",
            "<n>",
            "First resource and definition row, 0..5000.",
        )
        .default("0"),
        Arg::value(
            "limit",
            "<n>",
            "Rows in each resource/definition collection, 1..5000.",
        )
        .default("25"),
    ],
    &[Example {
        command: "ds library model show --model ./pinned.dsgrid --expected-sha256 sha256:<model> --output json",
        note: "Inspect immutable pins and the member coverage before exporting.",
        runnable: false,
    }],
    Effect::ReadOnly,
    SEARCH,
);

const MATCH_REFUSALS: &[Refusal] = &[
    REFUSALS[0],
    REFUSALS[5],
    REFUSALS[9],
    REFUSALS[10],
    REFUSALS[11],
    REFUSALS[12],
    REFUSALS[13],
    Refusal {
        code: "source_symlink_refused",
        when: "the native library folder contains a symlink",
        remedy: "point --native-dir at a folder of real files",
    },
];

/// Native files one folder match reads; the whole folder is read before matching.
const MAX_NATIVE_FILES: usize = 20_000;
const MAX_NATIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub static MATCH: Command = Command {
    id: "library.model.match",
    path: &["library", "model", "match"],
    contract: 1,
    summary: "Name the library members holding a model's exact native bytes.",
    purpose: "Resolves each native definition a model carries (for example a project PLS type such as S190_1p_strain_12.012) to the members with identical bytes, whatever their names, in one exact release (--release) or in a native library folder such as a canonical structure library (--native-dir): exact_member names the canonical member and what it backs, ambiguous_members lists several without choosing, no_exact_member says nothing carries those bytes, and project_evidence marks route, terrain and settings. Names never map anything; byte identity is not engineering equivalence or approval.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        MODEL,
        EXPECTED,
        Arg::value(
            "release",
            "<path.dsgrid-library>",
            "One exact library release (.dsgrid-library or .dsgrid-template); or use --native-dir.",
        ),
        Arg::value(
            "expected-library-sha256",
            "<sha256:hex>",
            "Exact release bundle SHA-256; required with --release.",
        ),
        Arg::value(
            "native-dir",
            "<dir>",
            "A folder of native library files, read whole (at most 20000 files, 2 GiB); or use --release.",
        ),
        Arg::value("offset", "<n>", "First resource row, 0..5000.").default("0"),
        Arg::value("limit", "<n>", "Resource rows, 1..5000.").default("25"),
    ],
    output: "Per model resource its status, the matching members (release member or folder file name and path) and the elements it backs; exact, ambiguous, unmatched and project_evidence counts; the release pin or the folder inventory digest compared. solver_approval is always false.",
    examples: &[
        Example {
            command: "ds library model match --model ./project-head.dsgrid --expected-sha256 sha256:<model> --release ./canonical.dsgrid-template --expected-library-sha256 sha256:<release> --output json",
            note: "Cite only exact_member rows as verified membership.",
            runnable: false,
        },
        Example {
            command: "ds library model match --model ./project-head.dsgrid --expected-sha256 sha256:<model> --native-dir ./canonical/structures --output json",
            note: "The same exact-byte rule against a folder of canonical native files.",
            runnable: false,
        },
    ],
    refusals: MATCH_REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: SEARCH_FOR_MATCH,
    requires: Requires::Server,
    availability: || Availability::Available,
};

/// A declared term must say what the id or summary does not already say.
const SEARCH_FOR_MATCH: &[&str] = &["canonical code", "member code", "membership"];

fn page(inputs: &Inputs) -> Result<(usize, usize), Failure> {
    let number = |key: &str, default: &str| {
        inputs
            .value(key)
            .unwrap_or(default)
            .parse::<usize>()
            .map_err(|_| {
                Failure::invalid("library_selection_invalid", "paging must be a whole number")
            })
    };
    let offset = number("offset", "0")?;
    let limit = number("limit", "25")?;
    if offset > 5000 || !(1..=5000).contains(&limit) {
        return Err(Failure::invalid(
            "library_selection_invalid",
            "paging is outside 0..5000",
        ));
    }
    Ok((offset, limit))
}

/// One page of resource rows plus the counts every match reports.
fn match_page(rows: Vec<Value>, offset: usize, limit: usize, mut head: Value) -> Value {
    let total = rows.len();
    let end = offset.saturating_add(limit).min(total);
    head["resources"] = json!(
        rows.into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>()
    );
    head["total_resources"] = json!(total);
    head["solver_approval"] = json!(false);
    head["more"] = if end < total {
        json!({ "offset": end })
    } else {
        Value::Null
    };
    head
}

/// Every regular file below `dir`, by `/`-separated relative path, bounded.
fn native_folder(dir: &str) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let root = Path::new(dir);
    if !root.is_dir() {
        return Err(Failure::invalid(
            "library_path_not_found",
            format!("`{dir}` is not a readable folder"),
        )
        .remedy("use the intended existing native library folder"));
    }
    let mut files = Vec::new();
    crate::seed::collect_tree(root, root, "", &mut files)?;
    let bytes = files
        .iter()
        .map(|(_, bytes)| bytes.len() as u64)
        .sum::<u64>();
    if files.len() > MAX_NATIVE_FILES || bytes > MAX_NATIVE_BYTES {
        return Err(Failure::invalid(
            "library_file_too_large",
            format!(
                "`{dir}` holds {} files and {bytes} bytes; a match reads at most {MAX_NATIVE_FILES} files and {MAX_NATIVE_BYTES} bytes",
                files.len()
            ),
        )
        .remedy("point --native-dir at the library folder itself, not a whole drive"));
    }
    Ok(files)
}

pub fn match_members(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (offset, limit) = page(inputs)?;
    let source = (
        inputs.value("release"),
        inputs.value("expected-library-sha256"),
        inputs.value("native-dir"),
    );
    let model = crate::read(inputs.require("model")?)?;
    let expected = inputs.require("expected-sha256")?;
    match source {
        (Some(release), Some(release_digest), None) => {
            let release = crate::read(release)?;
            let report = ds_grid_exchange::library_match::match_release_members(
                &model,
                expected,
                &release,
                release_digest,
            )
            .map_err(owner_error)?;
            Ok(match_page(
                report.resources.iter().map(|row| json!(row)).collect(),
                offset,
                limit,
                json!({
                    "model_digest": report.model_digest,
                    "release": report.release,
                    "exact": report.exact,
                    "ambiguous": report.ambiguous,
                    "unmatched": report.unmatched,
                    "project_evidence": report.project_evidence,
                }),
            ))
        }
        (None, None, Some(dir)) => {
            let files = native_folder(dir)?;
            let report =
                ds_grid_exchange::library_match::match_native_members(&model, expected, &files)
                    .map_err(owner_error)?;
            Ok(match_page(
                report.resources.iter().map(|row| json!(row)).collect(),
                offset,
                limit,
                json!({
                    "model_digest": report.model_digest,
                    "native_dir": dir,
                    "native_files": report.native_files,
                    "native_inventory_digest": report.native_inventory_digest,
                    "exact": report.exact,
                    "ambiguous": report.ambiguous,
                    "unmatched": report.unmatched,
                    "project_evidence": report.project_evidence,
                }),
            ))
        }
        _ => Err(Failure::invalid(
            "library_selection_invalid",
            "name exactly one source: --release with --expected-library-sha256, or --native-dir",
        )
        .remedy("pass --release and --expected-library-sha256, or --native-dir alone")),
    }
}

fn owner_error(message: String) -> Failure {
    let prefix = message.split(':').next().unwrap_or("");
    match prefix {
        "digest_conflict" => Failure::failed("digest_conflict", message),
        "no_cloud_equivalent" => Failure::failed("no_cloud_equivalent", message),
        "library_already_pinned" => Failure::failed("library_already_pinned", message),
        "library_pin_conflict" => Failure::failed("library_pin_conflict", message),
        "library_identity_conflict" => Failure::failed("library_identity_conflict", message),
        "library_selection_invalid" => Failure::failed("library_selection_invalid", message),
        "library_resolution_required" => Failure::failed("library_resolution_required", message),
        "library_kind_invalid" => Failure::failed("library_kind_invalid", message),
        "native_bytes_missing" => Failure::failed("native_bytes_missing", message),
        _ => Failure::failed("library_operation_failed", message),
    }
}
fn entity(inputs: &Inputs, key: &str) -> Result<EntityId, Failure> {
    EntityId::new(inputs.require(key)?)
        .map_err(|error| Failure::invalid("library_selection_invalid", error.to_string()))
}
fn library_output(bytes: Vec<u8>, out: &str) -> Result<Value, Failure> {
    let release = unpack_library(&bytes).map_err(|error| owner_error(error.to_string()))?;
    crate::write_new(Path::new(out), &bytes)?;
    Ok(
        json!({"written":out,"bundle_digest":bundle_digest(&bytes),"byte_length":bytes.len(),"library_pin":{"artifact_id":release.manifest.artifact_id,"revision_id":release.manifest.revision_id,"content_root_digest":release.manifest.content_root_digest},"cloud_write":false,"solver_approval":false}),
    )
}
pub fn create(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let source = crate::read(inputs.require("model")?)?;
    library_output(
        model_library::create_model_library(
            &source,
            inputs.require("expected-sha256")?,
            entity(inputs, "library-id")?,
            entity(inputs, "library-version")?,
        )
        .map_err(owner_error)?,
        inputs.require("out")?,
    )
}
pub fn clone(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let source = crate::read(inputs.require("release")?)?;
    library_output(
        model_library::clone_model_library(
            &source,
            inputs.require("expected-sha256")?,
            entity(inputs, "library-id")?,
            entity(inputs, "library-version")?,
        )
        .map_err(owner_error)?,
        inputs.require("out")?,
    )
}
fn model_output(outcome: model_library::ModelLibraryOutcome, out: &str) -> Result<Value, Failure> {
    crate::write_new(Path::new(out), &outcome.bytes)?;
    let mut value = serde_json::to_value(&outcome)
        .map_err(|error| Failure::internal("library_operation_failed", error.to_string()))?;
    value["written"] = json!(out);
    value["cloud_write"] = json!(false);
    value["byte_length"] = json!(outcome.bytes.len());
    Ok(value)
}
pub fn attach(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let source = crate::read(inputs.require("model")?)?;
    let release = crate::read(inputs.require("release")?)?;
    let elements = inputs
        .repeated("element")
        .iter()
        .map(|value| {
            EntityId::new(value.as_str())
                .map_err(|error| Failure::invalid("library_selection_invalid", error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    model_output(
        model_library::attach_model_library(
            &source,
            inputs.require("expected-sha256")?,
            &release,
            inputs.require("expected-library-sha256")?,
            &elements,
        )
        .map_err(owner_error)?,
        inputs.require("out")?,
    )
}
pub fn detach(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let source = crate::read(inputs.require("model")?)?;
    let pin = DependencyPin {
        artifact_id: entity(inputs, "library-id")?,
        revision_id: entity(inputs, "library-version")?,
        content_root_digest: inputs.require("content-root")?.into(),
    };
    model_output(
        model_library::detach_model_library(&source, inputs.require("expected-sha256")?, &pin)
            .map_err(owner_error)?,
        inputs.require("out")?,
    )
}
pub fn show(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let source = crate::read(inputs.require("model")?)?;
    let report =
        model_library::inspect_model_libraries(&source, inputs.require("expected-sha256")?)
            .map_err(owner_error)?;
    let number = |key: &str, default: &str| {
        inputs
            .value(key)
            .unwrap_or(default)
            .parse::<usize>()
            .map_err(|_| {
                Failure::invalid("library_selection_invalid", "paging must be a whole number")
            })
    };
    let offset = number("offset", "0")?;
    let limit = number("limit", "25")?;
    if offset > 5000 || !(1..=5000).contains(&limit) {
        return Err(Failure::invalid(
            "library_selection_invalid",
            "paging is outside 0..5000",
        ));
    }
    let total = report.members.len();
    let end = offset.saturating_add(limit).min(total);
    let total_definitions = report.definitions.len();
    let definition_end = offset.saturating_add(limit).min(total_definitions);
    let definitions = report
        .definitions
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let members = report
        .members
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    Ok(
        json!({"model_digest":report.model_digest,"pins":report.pins,"releases":report.releases,"members":members,"total_members":total,"definitions":definitions,"total_definitions":total_definitions,"definitions_more":if definition_end<total_definitions {json!({"offset":definition_end})} else {Value::Null},"managed_export_allowed":report.managed_export_allowed,"solver_approval":false,"more":if end<total {json!({"offset":end})} else {Value::Null}}),
    )
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}
