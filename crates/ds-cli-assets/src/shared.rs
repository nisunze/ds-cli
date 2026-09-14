//! Shared producer references; no artifact upload or copy.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::assets::{Link, ReportReference};
use serde_json::Value;

const EXTRA: Refusal = Refusal {
    code: "asset_scope",
    when: "a tag or transformer scope is incomplete or mixed",
    remedy: "use tag-definition and tag-value together, or transformer alone",
};
const INPUT_INVALID: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "the catalogue limit is outside 1..200 or the shared asset request is invalid",
    remedy: "use a limit from 1 through 200 and the declared scope fields",
};
const fn refusals() -> [Refusal; 2 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()] {
    let mut out = [EXTRA; 2 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()];
    out[1] = INPUT_INVALID;
    let mut i = 0;
    while i < ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() {
        out[i + 2] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals[i];
        i += 1;
    }
    out
}
const ARGS: &[Arg] = &[
    Arg::value(
        "lane",
        "<stable|canary>",
        "Native credential lane; default stable.",
    )
    .choices(&["stable", "canary"]),
    Arg::value(
        "role",
        "<network_information|city_map|transformer_map>",
        "Shared artifact role.",
    )
    .choices(&["network_information", "city_map", "transformer_map"])
    .required(),
    Arg::value(
        "tag-definition",
        "<id>",
        "Exact project tag definition; pair with --tag-value.",
    ),
    Arg::value(
        "tag-value",
        "<value>",
        "Exact tag value, e.g. a city; pair with --tag-definition.",
    ),
    Arg::value(
        "transformer",
        "<name>",
        "Transformer identity instead of a tag.",
    ),
];
const REFERENCE_ARGS: &[Arg] = &[
    ARGS[0],
    ARGS[1],
    ARGS[2],
    ARGS[3],
    ARGS[4],
    Arg::value("work", "<id>", "Published network reporter work id.").required(),
    Arg::value(
        "output-id",
        "<id>",
        "Exact verified output id from that work.",
    )
    .required(),
];
pub static RESOLVE: Command = Command {
    id: "assets.resolve",
    path: &["assets", "resolve"],
    contract: 1,
    summary: "Resolve a shared report table or map by tag or transformer.",
    purpose: "Returns a shared asset reference or a missing/ambiguous/incomplete state with manual_entry_allowed=true. Missing geographic data is normal: Solar seeds editable manual inputs. No source bytes are copied or uploaded. A city is an exact project tag definition/value, never a display-name guess.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: ARGS,
    output: "Resolution status, project asset metadata when available, manual_entry_allowed=true and storage_copied=false.",
    examples: &[],
    refusals: &refusals(),
    reference: Some("docs/reference/assets.md"),
    availability: ds_cli_auth::native_availability,
};
pub static MAPS: Command = Command {
    id: "assets.maps",
    path: &["assets", "maps"],
    contract: 1,
    summary: "List custom maps, project-wide maps and maps grouped by tags.",
    purpose: "Reads one authorized asset catalogue page and indexes maps by exact tag definition and value. A city is one tag group. Custom map documents exist independently of tags; untagged maps remain visible. Multiple memberships reference the same asset and bytes. No rendering, data acquisition or copying occurs. Follow next_cursor while more is true; a page without maps may still have later matches. Creation and composition remain CLI/MCP workflows.",
    chapter: Chapter::Assets,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ARGS[0],
        Arg::value(
            "limit",
            "<count>",
            "Asset rows inspected in this page, 1..200.",
        )
        .default("50"),
        Arg::value(
            "cursor",
            "<opaque>",
            "Unchanged next_cursor from the preceding index page.",
        ),
    ],
    output: "Project, tag-group-map family, unique map assets, optional producer references, tag groups and ungrouped_asset_ids, more, truncated and next_cursor. Only published catalogue assets are included; local or queued printouts are not online evidence.",
    examples: &[],
    refusals: &refusals(),
    reference: Some("docs/reference/assets.md"),
    availability: ds_cli_auth::native_availability,
};
pub static PUBLISH_MAP: Command = Command {
    id: "assets.map.publish",
    path: &["assets", "map", "publish"],
    contract: 1,
    summary: "Publish a custom map into Project Control and its tag groups.",
    purpose: "Upload one operator-declared PDF/PNG/JPEG/WebP map through existing Project Assets, classify it as a durable geographic document and attach exact tags. No Desktop is required. Untagged maps appear as project-wide maps. Repeating the same name and bytes reuses the asset; changed bytes create a new asset. Inputs are bounded to 32 MiB and 16 tags. Requires assets ingest, classify and attach permissions. Partial failures retain the uploaded asset; retry the same declaration.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ARGS[0],
        Arg::value(
            "file",
            "<path>",
            "Existing local PDF/PNG/JPEG/WebP map, at most 32 MiB.",
        )
        .required(),
        Arg::value(
            "name",
            "<filename>",
            "Meaningful catalogue filename; default is the local filename.",
        ),
        Arg::repeated(
            "tag",
            "<definition=value>",
            "Exact project tag membership; repeat up to 16 times, e.g. city=gagal.",
        ),
    ],
    output: "Published asset, verified name/digest/size, map index entry, reused and bytes_uploaded. No upload URL or credentials.",
    examples: &[],
    refusals: &refusals(),
    reference: Some("docs/reference/assets.md"),
    availability: ds_cli_auth::native_availability,
};
pub fn publish_map(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    use std::io::Read;
    let failure = || {
        Failure::invalid(
            "auth_input_invalid",
            "Use a readable map file up to 32 MiB, a filename and at most 16 definition=value tags",
        )
    };
    let path = std::path::Path::new(i.require("file")?);
    let name = i
        .value("name")
        .or_else(|| path.file_name().and_then(|n| n.to_str()))
        .ok_or_else(failure)?;
    let file = std::fs::File::open(path).map_err(|_| failure())?;
    if !file.metadata().map_err(|_| failure())?.is_file() {
        return Err(failure());
    }
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    let tags = i
        .repeated("tag")
        .iter()
        .map(|raw| {
            let (definition_id, value) = raw.split_once('=').ok_or_else(failure)?;
            Ok(Link::Tag {
                definition_id: definition_id.into(),
                value: value.into(),
            })
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(ds_cli_auth::shared_assets(
        i.value("lane").unwrap_or("stable"),
        &ds_cli_auth::SharedAssetsCommand::PublishMap {
            name: name.into(),
            bytes,
            tags,
        },
    )?
    .into_result())
}
pub fn maps(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = i
        .require("limit")?
        .parse::<u16>()
        .ok()
        .filter(|n| (1..=200).contains(n))
        .ok_or_else(|| {
            Failure::invalid("auth_input_invalid", "limit must be 1..200")
                .remedy("Choose one bounded catalogue page.")
        })?;
    Ok(ds_cli_auth::shared_assets(
        i.value("lane").unwrap_or("stable"),
        &ds_cli_auth::SharedAssetsCommand::Maps {
            limit,
            cursor: i.value("cursor").map(str::to_string),
        },
    )?
    .into_result())
}
pub static REFERENCE: Command = Command {
    id: "assets.reference",
    path: &["assets", "reference"],
    contract: 1,
    summary: "Link an existing verified report artifact to a tag or transformer.",
    purpose: "Registers metadata pointing to the producer's existing object and generation. Repeating the same work/output reuses its asset. City maps require a tag; transformer maps require a transformer. Uses project assets.attach authority and creates no storage object.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: REFERENCE_ARGS,
    output: "Shared project asset identity, reference, tag/object links and storage_copied=false.",
    examples: &[],
    refusals: &refusals(),
    reference: Some("docs/reference/assets.md"),
    availability: ds_cli_auth::native_availability,
};
pub fn link(inputs: &Inputs) -> Result<Link, Failure> {
    match (
        inputs.value("tag-definition"),
        inputs.value("tag-value"),
        inputs.value("transformer"),
    ) {
        (Some(definition_id), Some(value), None) => Ok(Link::Tag {
            definition_id: definition_id.into(),
            value: value.into(),
        }),
        (None, None, Some(name)) => Ok(Link::DsObject {
            object_type: "transformer".into(),
            entity_id: name.into(),
        }),
        _ => Err(Failure::invalid(
            "asset_scope",
            "Use tag-definition with tag-value, or one transformer.",
        )),
    }
}
pub fn resolve(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    Ok(ds_cli_auth::shared_assets(
        i.value("lane").unwrap_or("stable"),
        &ds_cli_auth::SharedAssetsCommand::Resolve {
            link: link(i)?,
            role: i.require("role")?.into(),
        },
    )?
    .into_result())
}
pub fn reference(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    Ok(ds_cli_auth::shared_assets(
        i.value("lane").unwrap_or("stable"),
        &ds_cli_auth::SharedAssetsCommand::Reference {
            link: link(i)?,
            reference: ReportReference {
                work_id: i.require("work")?.into(),
                output_id: i.require("output-id")?.into(),
                role: i.require("role")?.into(),
            },
        },
    )?
    .into_result())
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
