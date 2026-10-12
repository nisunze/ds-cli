//! Thin headless LV catalog command adapters. Numbering decisions stay server-owned.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_client_core::design_version_catalog::{Command as Request, Mutation};
use serde_json::Value;
const PROJECT: Arg = Arg::value(
    "project",
    "<project-id>",
    "Exact project; saved selection is unused.",
)
.required();
const OBJECT: Arg = Arg::value(
    "object",
    "<transformer-id>",
    "Exact LV transformer identity.",
)
.required();
const VERSION: Arg = Arg::value(
    "version",
    "<immutable-id>",
    "Immutable catalog identity: legacy vN or vc_<sha256>; never a display ordinal.",
);
const ORDINAL: Arg = Arg::value(
    "display-ordinal",
    "<nonnegative-integer>",
    "Display number assigned independently of immutable identity; zero is allowed.",
);
const CATALOG: Arg = Arg::value(
    "expected-catalog",
    "<sha256>",
    "Exact catalog_revision returned by catalog; moved history refuses.",
)
.required();
const SOURCE: Arg = Arg::value(
    "expected-source",
    "<RFC3339>",
    "Exact current transformer source_revision returned by status; moved content refuses.",
)
.required();
const REASON: Arg = Arg::value(
    "reason",
    "<text>",
    "Reason for the deliberate catalog mutation.",
)
.required();
const KEY: Arg = Arg::value(
    "idempotency-key",
    "<key>",
    "Stable key reused only for identical retries.",
)
.required();
const TASK: Arg = Arg::value(
    "task",
    "<task-id>",
    "Existing PM task for this revision/change; backend verifies exact project identity.",
);
const RECORD: Arg = Arg::value(
    "source-record",
    "<record-id>",
    "Existing recorded change; backend retains its verified revision/digest, without closing tasks or assigning approval.",
);
const LANE: Arg = crate::transformer::LANE_ARG;
const fn descriptor(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    args: &'static [Arg],
    effect: Effect,
) -> Command {
    Command {
        id,
        path,
        summary,
        args,
        effect,
        contract: 1,
        purpose: "LV version catalog separates immutable version/revision references from renumberable display ordinals. Legacy vN references retain their original snapshots. Publishing captures current saved transformer bytes as a new immutable revision; it never overwrites a prior snapshot. New insertion before a reviewed version shifts display labels only. This does not issue a PM submission or approve engineering. Every mutation requires exact catalog/source fences and explicit confirmation.",
        chapter: Chapter::Design,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        output: "Verified same-project/object catalog, exact immutable revision metadata for show, or fenced mutation receipt. No implicit publication, submission or engineering approval.",
        examples: &[],
        refusals: &[Refusal {
            code: "version_catalog_refused",
            when: "The server refuses catalog/source fences, occupied display ordinal, immutable identity or revision policy",
            remedy: "Read the nested server cause, refresh the named catalog/head, and review before changing the request; retry only an identical uncertain mutation",
        }],
        reference: Some("docs/reference/design.md"),
        search: &["version catalog", "display ordinal", "insert version"],
        requires: Requires::Server,
        availability: available,
    }
}
fn available() -> Availability {
    ds_cli_auth::native_availability()
}
pub static CATALOG_COMMAND: Command = descriptor(
    "design.version.catalog",
    &["design", "version", "catalog"],
    "Read stable LV version identities and current display ordinals.",
    &[PROJECT, OBJECT, LANE],
    Effect::ReadOnly,
);
pub static REVISION_SHOW: Command = descriptor(
    "design.version.revision.show",
    &["design", "version", "revision", "show"],
    "Read metadata for one exact immutable LV catalog revision.",
    &[
        PROJECT,
        OBJECT,
        VERSION.required(),
        Arg::switch(
            "snapshot",
            "Explicitly read exact immutable snapshot bytes through Bulk; omitted, Run returns metadata only.",
        ),
        Arg::value(
            "revision",
            "<immutable-revision-id>",
            "Exact vr_<sha256> or legacy vN revision identity.",
        )
        .required(),
        LANE,
    ],
    Effect::ReadOnly,
);
pub static REVISION_PUBLISH: Command = descriptor(
    "design.version.revision.publish",
    &["design", "version", "revision", "publish"],
    "Freeze saved LV content as an immutable catalog revision (needs --yes).",
    &[
        PROJECT,
        OBJECT,
        VERSION,
        ORDINAL,
        Arg::value(
            "insert-before",
            "<immutable-version-id>",
            "New identity only: explicitly insert before this occupied display ordinal, shifting labels under the catalog fence.",
        ),
        CATALOG,
        SOURCE,
        REASON,
        KEY,
        TASK,
        RECORD,
        LANE,
    ],
    Effect::GlobalWrite,
);
pub static RENUMBER: Command = descriptor(
    "design.version.renumber",
    &["design", "version", "renumber"],
    "Change an LV display ordinal without changing immutable references (needs --yes).",
    &[
        PROJECT,
        OBJECT,
        VERSION.required(),
        ORDINAL.required(),
        CATALOG,
        SOURCE,
        REASON,
        KEY,
        TASK,
        RECORD,
        LANE,
    ],
    Effect::GlobalWrite,
);
pub static REVISION_BIND: Command = descriptor(
    "design.version.revision.bind",
    &["design", "version", "revision", "bind"],
    "Bind an exact immutable revision to an issued PM record or milestone (needs --yes).",
    &[PROJECT, OBJECT, Arg::value("file", "<binding.json>", "Closed binding pins: version_id,revision_id,purpose,record_id/expected_record_revision,milestone_id,assets,expected_catalog_revision,expected_source_revision,reason,idempotency_key. Submission requires issued record and delivered assets; at most 32 asset pins.").required(), LANE],
    Effect::GlobalWrite,
);
pub static REVISION_PROMOTE: Command = Command {
    purpose: "Promote an exact immutable runtime revision through Bulk. The server retains the current source as an immutable backup, records the tracked change and validates runtime identities before replacing Latest. Exported-design and metadata-only revisions refuse. Requires exact catalog/source fences and explicit confirmation; no caller content, implicit retry, submission or engineering approval.",
    output: "Verified exact target revision, immutable original-source backup, tracked change and new working source version/digest.",
    search: &["version promote", "immutable backup", "safe restore"],
    ..descriptor(
    "design.version.revision.promote",
    &["design", "version", "revision", "promote"],
    "Promote an exact immutable runtime revision after backing up current source (needs --yes).",
    &[PROJECT, OBJECT, Arg::value("file", "<promotion.json>", "Closed promotion pins: version_id,revision_id,expected_catalog_revision,expected_source_revision,task_ids?,source_record_ids?,reason,idempotency_key. Server retains original frame backup and tracked change before replacing Latest; exported-design and metadata-only snapshots refuse.").required(), LANE],
    Effect::GlobalWrite,
)
};

pub fn promote(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = i.require("file")?;
    let metadata =
        std::fs::metadata(path).map_err(|e| Failure::invalid("invalid_input", e.to_string()))?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err(Failure::invalid(
            "invalid_input",
            "promotion input must be a bounded regular file",
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| Failure::invalid("invalid_input", e.to_string()))?;
    let promotion =
        serde_json::from_slice::<ds_command_kernel::design_version_promotion::Promotion>(&bytes)
            .map_err(|e| Failure::invalid("invalid_input", e.to_string()))?;
    ask(
        i,
        Request::Promote {
            object_id: i.require("object")?.into(),
            promotion,
        },
    )
}

pub fn bind(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = i.require("file")?;
    let metadata =
        std::fs::metadata(path).map_err(|e| Failure::invalid("invalid_input", e.to_string()))?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err(Failure::invalid(
            "invalid_input",
            "binding input must be a bounded regular file",
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| Failure::invalid("invalid_input", e.to_string()))?;
    let binding =
        serde_json::from_slice::<ds_command_kernel::design_version_binding::Binding>(&bytes)
            .map_err(|e| Failure::invalid("invalid_input", e.to_string()))?;
    ask(
        i,
        Request::Bind {
            object_id: i.require("object")?.into(),
            binding,
        },
    )
}

fn mutation(i: &Inputs) -> Result<Mutation, Failure> {
    Ok(Mutation {
        task_id: i.value("task").map(str::to_owned),
        source_record_id: i.value("source-record").map(str::to_owned),
        version_id: i.value("version").map(str::to_owned),
        display_ordinal: i
            .value("display-ordinal")
            .map(|s| {
                s.parse().map_err(|_| {
                    Failure::invalid(
                        "invalid_input",
                        "display-ordinal must be a nonnegative integer",
                    )
                })
            })
            .transpose()?,
        insert_before_version_id: i.value("insert-before").map(str::to_owned),
        expected_catalog_revision: i.require("expected-catalog")?.into(),
        expected_source_revision: i.require("expected-source")?.into(),
        reason: i.require("reason")?.into(),
        idempotency_key: i.require("idempotency-key")?.into(),
    })
}
fn ask(i: &Inputs, c: Request) -> Result<Value, Failure> {
    ds_cli_auth::design_version_catalog_for_project(i.require("lane")?, i.require("project")?, &c)
        .map_err(|e| {
            Failure::failed("version_catalog_refused", e.to_string())
                .detail(serde_json::json!({"cause":e.code(),"detail":e.detail_value()}))
                .remedy(
                    e.remedy_text()
                        .unwrap_or("Inspect the exact catalog and source fences before retrying"),
                )
        })
}
pub fn catalog(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Catalog {
            object_id: i.require("object")?.into(),
        },
    )
}
pub fn show(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let object_id = i.require("object")?.into();
    let version_id = i.require("version")?.into();
    let revision_id = i.require("revision")?.into();
    let request = if i.switch("snapshot") {
        Request::Snapshot {
            object_id,
            version_id,
            revision_id,
        }
    } else {
        Request::Revision {
            object_id,
            version_id,
            revision_id,
        }
    };
    ask(i, request)
}

pub fn publish(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Publish {
            object_id: i.require("object")?.into(),
            mutation: mutation(i)?,
        },
    )
}
pub fn renumber(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        Request::Renumber {
            object_id: i.require("object")?.into(),
            mutation: mutation(i)?,
        },
    )
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
