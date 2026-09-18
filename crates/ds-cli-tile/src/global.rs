//! Governed global reference publication, through the native user.
//!
//! These publications belong to the product, not to a project and not to a
//! window: `global_tiles.manage` at the gateway is the whole of their
//! authority. Until 2026-09-18 they were reached through the paired
//! application, which held the same signed-in user and posted the same bodies
//! to `/api/v1/tiles`. The transport moved to `ds-client-core::global_tiles`;
//! the wire did not change.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_client_core::global_tiles::{Allowlists, Command as Global, Domain, Source, Visibility};
use serde_json::Value;

const DOMAIN: Arg = Arg::value("domain", "<domain>", "Governed Reference Layers domain.")
    .required()
    .choices(&["network_template", "solar_template"]);
const INVALID_NUMBER: Refusal = Refusal {
    code: "invalid_number",
    when: "limit or maxzoom is outside its declared integer range",
    remedy: "use limit 1..200 and maxzoom 1..22",
};
const INVALID_SELECTION: Refusal = Refusal {
    code: "global_tile_selection_invalid",
    when: "a source is not an exact project.dataset.table identity, two sources share a table name, an allowed reader is repeated or is not one token, or a required text is empty",
    remedy: "take each source from `ds tile global catalog`, give the publication a name and country, and name each allowed reader by the address or role it signs in with",
};
/// This domain's own refusals, then every refusal the native user path can
/// return, composed so a new one reaches these commands rather than going
/// undocumented.
const fn refusals() -> [Refusal; 2 + ds_cli_auth::PROJECT_LIST_COMMAND.refusals.len()] {
    let mut all = [INVALID_NUMBER; 2 + ds_cli_auth::PROJECT_LIST_COMMAND.refusals.len()];
    all[1] = INVALID_SELECTION;
    let mut index = 0;
    while index < ds_cli_auth::PROJECT_LIST_COMMAND.refusals.len() {
        all[2 + index] = ds_cli_auth::PROJECT_LIST_COMMAND.refusals[index];
        index += 1;
    }
    all
}
const REFUSAL_SET: [Refusal; 2 + ds_cli_auth::PROJECT_LIST_COMMAND.refusals.len()] = refusals();
const REFUSALS: &[Refusal] = &REFUSAL_SET;
const LANE: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);

pub static CATALOG: Command = Command {
    id: "tile.global.catalog",
    path: &["tile", "global", "catalog"],
    contract: 1,
    summary: "Discover geographic sources for global reference publication.",
    purpose: "Reads one bounded page of the same source catalog as Reference Layers. The server enforces global_tiles.manage and the selected domain allowlist. This includes eligible sources that have not yet been published and therefore are absent from the Desktop data catalog.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        DOMAIN,
        Arg::value(
            "search",
            "<text>",
            "Filter source names; at most 200 characters.",
        ),
        Arg::value("limit", "<n>", "Maximum tables per page, 1..200.").default("50"),
        Arg::value(
            "page-token",
            "<token>",
            "Continue the preceding page; at most 2048 characters.",
        ),
        LANE,
    ],
    output: "Catalog datasets and exact source table identities, row counts, geography columns, and has_more/next_page_token when another page exists.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/tile.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static LIST: Command = Command {
    id: "tile.global.list",
    path: &["tile", "global", "list"],
    contract: 1,
    summary: "List active governed reference publications.",
    purpose: "Reads the same active global publication list as Reference Layers for one domain. It preserves each durable tile identity and its published layer metadata, so a caller can recover the exact tile_id after an ambiguous generation response without starting or retrying a write.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[DOMAIN, LANE],
    output: "All active publications visible in the selected domain, preserving exact tile ids, names, layer names, feature counts, archive locations and access policy.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/tile.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static GENERATE: Command = Command {
    id: "tile.global.generate",
    path: &["tile", "global", "generate"],
    contract: 1,
    summary: "Publish a new global reference tile archive and Desktop datasets.",
    purpose: "Starts the existing governed reference worker for 1..50 explicit catalog project.dataset.table sources. Creates a new publication; does not replace an existing tile or change its access policy. Layer names are the source table names and must be distinct. Uses minzoom 0, explicit maxzoom/basezoom and no density dropping. The server validates source eligibility and global_tiles.manage. Follow the returned tile_id with status; started is not completed.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        DOMAIN,
        Arg::value(
            "name",
            "<text>",
            "Publication name, at most 200 characters.",
        )
        .required(),
        Arg::value(
            "country",
            "<name>",
            "Catalog country, at most 200 characters.",
        )
        .required(),
        Arg::repeated(
            "source",
            "<project.dataset.table>",
            "Exact source from global catalog; repeat 1..50 times.",
        )
        .required(),
        Arg::value("maxzoom", "<zoom>", "Maximum and base zoom, integer 1..22.").required(),
        LANE,
    ],
    output: "Publication tile_id, status, phase and message. Poll global status for completion or failure before reconciling the Desktop catalog.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/tile.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static STATUS: Command = Command {
    id: "tile.global.status",
    path: &["tile", "global", "status"],
    contract: 1,
    summary: "Read one governed reference publication job.",
    purpose: "Reads the durable job state through Reference Layers, without starting or retrying publication. Returns the actual failure if the worker could not publish.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "tile",
            "<tile-id>",
            "Exact tile_id returned by global generate.",
        )
        .required(),
        LANE,
    ],
    output: "Status, phase, tile_id, feature count and completed artifact locations, or last_error.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/tile.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static ACCESS: Command = Command {
    id: "tile.global.access",
    path: &["tile", "global", "access"],
    contract: 1,
    summary: "Set who may mount one governed reference publication.",
    purpose: "Replaces one publication's whole access policy through the same governed action Reference Layers uses, without touching its archive, its layers or its styles. `all` admits every member of a project whose country the publication matches; `restricted` admits only the readers named here, plus a system administrator. Restricted with nobody named is how a publication stops being mounted product-wide while it is still recoverable — the reversible alternative to removing it. What this call carries becomes the entire policy: a reader kept out of it loses the grant.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "tile",
            "<tile-id>",
            "Exact tile_id, as `ds tile global list` reports it.",
        )
        .required(),
        Arg::value(
            "mode",
            "<all|restricted>",
            "Who reads the publication: everybody the country admits, or only the readers named below.",
        )
        .required()
        .choices(&["all", "restricted"]),
        Arg::repeated(
            "allow-user",
            "<email>",
            "Sign-in address admitted under restricted; repeat up to 50 times.",
        ),
        Arg::repeated(
            "allow-app-role",
            "<role>",
            "Application role admitted under restricted; repeat up to 50 times.",
        ),
        Arg::repeated(
            "allow-project-role",
            "<role>",
            "Project role admitted under restricted; repeat up to 50 times.",
        ),
        Arg::repeated(
            "allow-project",
            "<ds-project-id>",
            "DS project whose members are admitted under restricted; repeat up to 50 times.",
        ),
        LANE,
    ],
    output: "tile_id and the access policy now stored: mode and the four allowlists exactly as the catalog holds them.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/tile.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn selection(error: ds_client_core::ClientError) -> Failure {
    Failure::invalid(INVALID_SELECTION.code, error.to_string()).remedy(INVALID_SELECTION.remedy)
}

fn domain(inputs: &Inputs) -> Result<Domain, Failure> {
    Domain::parse(inputs.require("domain")?).map_err(selection)
}

fn run(inputs: &Inputs, command: Global) -> Result<Value, Failure> {
    // The owner's own bounds, applied before a native user is restored: an
    // unusable selection is the caller's mistake and naming it here costs no
    // session, no round trip and no half-applied governed write.
    command.validate().map_err(selection)?;
    ds_cli_auth::global_tiles(inputs.require("lane")?, &command)
}

pub fn catalog(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = ds_cli_contract::args::integer(inputs.require("limit")?, "limit", 1, 200)?;
    run(
        inputs,
        Global::Catalog {
            domain: domain(inputs)?,
            search: inputs.value("search").map(str::to_owned),
            page_size: limit as u16,
            page_token: inputs.value("page-token").map(str::to_owned),
        },
    )
}

pub fn list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run(
        inputs,
        Global::List {
            domain: domain(inputs)?,
        },
    )
}

pub fn generate(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let maxzoom = ds_cli_contract::args::integer(inputs.require("maxzoom")?, "maxzoom", 1, 22)?;
    // Each source is an exact catalog identity, and its table name becomes the
    // published layer's, so two sources sharing one table name are refused
    // here rather than after a publication job has started.
    let sources = inputs
        .repeated("source")
        .iter()
        .map(|value| Source::parse(value).map_err(selection))
        .collect::<Result<Vec<_>, Failure>>()?;
    run(
        inputs,
        Global::Generate {
            domain: domain(inputs)?,
            name: inputs.require("name")?.to_owned(),
            country: inputs.require("country")?.to_owned(),
            sources,
            maxzoom: maxzoom as u8,
        },
    )
}

pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run(
        inputs,
        Global::Status {
            tile_id: inputs.require("tile")?.to_owned(),
        },
    )
}

/// The non-destructive lever. An allowlist is only meaningful under
/// `restricted`, so one given under `all` is refused by the owner rather than
/// recorded where nothing reads it.
pub fn access(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run(
        inputs,
        Global::Access {
            tile_id: inputs.require("tile")?.to_owned(),
            visibility: Visibility::parse(inputs.require("mode")?).map_err(selection)?,
            allowed: Allowlists {
                users: inputs.repeated("allow-user").to_vec(),
                app_roles: inputs.repeated("allow-app-role").to_vec(),
                project_roles: inputs.repeated("allow-project-role").to_vec(),
                projects: inputs.repeated("allow-project").to_vec(),
            },
        },
    )
}

pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
