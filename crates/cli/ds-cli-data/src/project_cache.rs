//! `ds data project-cache` — the project's own extracts of canonical datasets.
//!
//! A project holds bounded, spatially indexed extracts of the datasets its
//! workflow declares, instead of downloading a whole national layer it will
//! mostly not use. Coverage is the design's own footprint, buffered and fused,
//! so neighbouring transformers share one acquisition.
//!
//! Two commands, deliberately asymmetric:
//!
//! * `status` reads and costs nothing. It reports each dataset separately —
//!   what was requested, what actually completed, how many features, whether
//!   the index answers, and the last error.
//! * `seed` is the ONE place a geographic source is queried. It is confirmed,
//!   because it spends real money at the provider, and it acquires only the
//!   coverage the project does not already hold.
//!
//! Both run headlessly under the restored native user against an explicit
//! `--project`, on this machine's holdings — no paired Desktop. The
//! holdings, acquisition loop and bundle installation are
//! `ds-project-data`'s (the same crate the desktop shell hosts); every
//! decision is `ds-command-kernel`'s; this file declares the two commands and
//! lends the crate the governed provider door and the bundle fetch.

use ds_cli_auth::{
    DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL, DataDistributionRequest,
    REFERENCE_BUNDLE_DOWNLOAD_FAILED_REFUSAL,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_dataset_cache::{
    self as policy, CatalogEntry, Dataset, OverviewRow, Scope, SeedCandidate, both_orientations,
    buffer_policy,
};
use ds_project_data::{Acquisition, BundleReceipt, Hosts, Mode, Provider};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod local_source;
mod remote_context;
mod shared;
use shared::{SharedSeeding, shared_seed_row};

const NETWORK_ENGINE_MISSING: Refusal = Refusal {
    code: "network_engine_missing",
    when: "MV route coverage is requested but the Network executable is absent",
    remedy: "Install or activate a compatible Network engine for MV geographic projection",
};
const NETWORK_ENGINE_INCOMPATIBLE: Refusal = Refusal {
    code: "network_engine_incompatible",
    when: "MV route coverage's captured Network executable or encoding profile cannot be admitted",
    remedy: "Install or activate a compatible Network engine for MV geographic projection",
};

const DATASET_ARG: Arg = Arg::value(
    "dataset",
    "<dataset-id>",
    "Dataset id, layer or authority alias. Retired broad layers refuse. Omitted: active datasets the project declares or holds, even holding none.",
);

const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<ds-project>",
    "Exact project id; this call does not read or change the saved selection.",
)
.required();

const MV_MODEL_ARG: Arg = Arg::value(
    "mv-model",
    "<absolute.dsgrid>",
    "Local DS Grid draft whose route also needs coverage; the receipt pins its SHA-256.",
);

const LOCAL_SOURCE_ARG: Arg = Arg::value(
    "local-source",
    "<manifest.json>",
    "ds.local-analytical-source/v1 manifest (64 KiB) selecting one project asset revision and coverage; --dataset must match its id.",
);
const PUBLISHER_INPUT_ARG: Arg = Arg::value(
    "publisher-input",
    "<manifest.json>",
    "Pinned Google CSV.gz/Microsoft GeoJSONL.gz manifest; converts and seeds gaps. Exclusive with --local-source/--dataset.",
);

const REMOTE_CONTEXT_ARG: Arg = Arg::switch(
    "remote-context",
    "Retired: always refuses with native_compute_required; plain seed already replays shared revisions and acquires here.",
);

const LANE_ARG: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Deployment lane; stable is the default.");

// Never implicit: without it seed reads only gaps, so the seed after a
// refresh is warm again.
const REFRESH_ARG: Arg = Arg::switch(
    "refresh",
    "Re-acquire every planned tile from its source; buildings/contours republish as a refresh revision. Held rows stay until replaced.",
);

const SUMMARY_ARG: Arg = Arg::switch(
    "summary",
    "Compact identities, source, readiness and coverage counts, without coverage geometry or history.",
);
const QUERY_DATASET_ARG: Arg = Arg::value(
    "dataset",
    "<dataset-id|layer>",
    "One canonical dataset id or catalogue layer name from project-cache status --summary.",
)
.required();
const QUERY_BOUNDARY_ARG: Arg = Arg::value(
    "boundary",
    "<path.geojson>",
    "One WGS84 Polygon or MultiPolygon in a GeoJSON file, wholly inside the room's completed coverage.",
)
.required();
const QUERY_GEOMETRY_OUT_ARG: Arg = Arg::value(
    "geometry-out",
    "<path.geojson>",
    "Write this bounded page as a GeoJSON FeatureCollection; an existing file is never replaced.",
)
.required();
const QUERY_LIMIT_ARG: Arg = Arg::value(
    "limit",
    "<1-5000>",
    "Most features in this page; a truncated result returns the next cursor and generation.",
)
.default("5000");
const QUERY_CURSOR_ARG: Arg = Arg::value(
    "cursor",
    "<feature-id>",
    "Next cursor from a truncated response; requires that response's --generation.",
);
const QUERY_GENERATION_ARG: Arg = Arg::value(
    "generation",
    "<room-generation>",
    "Room generation from the same truncated response; rejects a changed room between pages.",
);

macro_rules! refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}

refusal!(
    INVALID_SCOPE,
    "project_dataset_scope_invalid",
    "the dataset is not holdable, the project declares none, or source modes conflict",
    "pass a --dataset id from `ds data project-cache status`, or omit it"
);
refusal!(
    NO_DESIGN_EXTENT,
    "project_has_no_extent",
    "the project has no active transformer design or MV route to derive coverage from",
    "save a transformer design or publish an MV model head, then seed"
);
refusal!(
    PROVIDER_UNAVAILABLE,
    "dataset_provider_unavailable",
    "the governed provider could not answer the acquisition",
    "restore the connection and retry; held data is kept, never claimed partly ready"
);
refusal!(
    ACQUISITION_FAILED,
    "project_dataset_acquisition_failed",
    "the provider or the room refused one acquisition cell; held data is intact",
    "fix the cause on the dataset's row and retry"
);
refusal!(
    LOCAL_RESOURCE_REQUIRED,
    "project_dataset_local_source_required",
    "the selected pinned local source or contour producer is missing",
    "supply the exact pinned input; no compute fallback or substitution occurs"
);
refusal!(
    PUBLISHER_INVALID,
    "publisher_input_invalid",
    "publisher manifest or gzip SHA-256 pin is invalid",
    "supply exact project/source/version/coverage/object pins"
);
refusal!(
    PUBLISHER_MALFORMED,
    "publisher_input_malformed",
    "publisher records are malformed or stable ids conflict",
    "repair the selected input; failed conversion publishes no source"
);
refusal!(
    PUBLISHER_UNSUPPORTED,
    "publisher_geometry_unsupported",
    "publisher geometry is outside supported 2D Polygon/MultiPolygon",
    "supply WGS84 polygon records without CRS/dimension overrides"
);
refusal!(
    PUBLISHER_TOO_LARGE,
    "publisher_input_too_large",
    "publisher object, record or extract exceeds a native bound",
    "select a smaller explicit extraction"
);
refusal!(
    SEED_INCOMPLETE,
    "project_dataset_seed_incomplete",
    "a selected dataset failed or has incomplete coverage",
    "inspect detail.result and acquire the missing inputs"
);
refusal!(
    BUNDLE_UNAVAILABLE,
    "reference_bundle_unavailable",
    "the catalogue row publishes no verified bundle, or it could not be installed",
    "publish the dataset's bundle, then seed again"
);
refusal!(
    REMOTE_CONTEXT_RETIRED,
    "native_compute_required",
    "--remote-context was given; buildings and contours are not computed in the cloud",
    "omit it: seed reuses shared revisions and acquires only what nobody published"
);
refusal!(
    REVISION_INVALID,
    "project_dataset_revision_invalid",
    "a published dataset revision is malformed or differs from its pins",
    "report the project and dataset; held data is kept"
);
refusal!(
    REVISION_STALE,
    "project_dataset_revision_stale",
    "other members kept publishing revisions while this seed re-planned",
    "seed again; it replays what landed and publishes only what is still missing"
);
refusal!(
    PUBLICATION_REFUSED,
    "project_dataset_publication_refused",
    "the account may not read or publish this project's shared datasets",
    "check the account's access to the project; held data is kept"
);
refusal!(
    REPORTER_MISSING,
    "reporter_engine_missing",
    "a seed needs the Reporter engine to convert or derive data and none is installed",
    "install Reporter for the selected --lane; nothing is acquired without it"
);
refusal!(
    REPORTER_INCOMPATIBLE,
    "reporter_engine_incompatible",
    "the installed Reporter does not publish the offline footprint or contour tasks",
    "update Reporter for the selected --lane"
);
refusal!(
    SURFACE_UNCOVERED,
    "contour_surface_uncovered",
    "the Rwanda DEM does not cover the project area contours were asked for",
    "seed contours only inside Rwanda's surface"
);
refusal!(
    ENGINE_REFUSED,
    "engine_refused",
    "the Reporter refused one conversion or derivation",
    "read the dataset row's error for the engine's own message"
);
refusal!(
    ENGINE_RESPONSE_INVALID,
    "engine_response_invalid",
    "the Reporter answered another request or bytes that differ from its digests",
    "repair or update Reporter for the selected --lane"
);
refusal!(
    RETIRED,
    "dataset_retired",
    "the named layer was retired and has no single authority (powerlines, elementary_school)",
    "seed the detailed alternatives the message names"
);
refusal!(
    CATALOG_UNAVAILABLE,
    "catalog_unavailable",
    "the reference catalogue could not be read or is invalid",
    "retry when connected; held rooms are still reported by status"
);
refusal!(
    STORE_FAILED,
    "project_dataset_store_failed",
    "this machine's holdings could not be read or written",
    "repair the geographic data storage root, then retry"
);
refusal!(
    CONFIRM,
    "confirmation_required",
    "--yes was not given for a command that spends provider cost and downloads bundles",
    "run `ds data project-cache status` first, then re-run with --yes"
);
refusal!(
    NATIVE_PROFILE,
    "native_profile_not_configured",
    "the exact packaged native profile is unavailable",
    "install one complete ds release"
);
refusal!(
    NATIVE_PROFILE_DIGEST,
    "native_profile_digest_mismatch",
    "the packaged catalogue differs from the build pin",
    "reinstall one complete ds release"
);
refusal!(
    NATIVE_PROFILE_UNSAFE,
    "native_profile_unsafe",
    "the packaged native catalogue is unsafe or malformed",
    "reinstall one complete ds release"
);
// The one signed-out sentence, spelled with its literal code so the source
// scan behind `refusal_coverage.rs` can follow `HEADLESS_SIGNED_OUT.code`.
const HEADLESS_SIGNED_OUT: Refusal = Refusal {
    code: "headless_signed_out",
    when: ds_cli_auth::SIGNED_OUT_REFUSAL.when,
    remedy: ds_cli_auth::SIGNED_OUT_REMEDY,
};
refusal!(
    HEADLESS_NO_PROJECT,
    "headless_project_not_selected",
    "the named project could not be established by the native provider",
    "pass one accessible exact id with --project"
);
refusal!(
    PROJECT_CONTEXT_STALE,
    "project_context_stale",
    "the native identity changed during the named-project operation",
    "retry the same --project under the signed-in identity"
);
refusal!(
    PROJECT_CONTEXT_CHANGED,
    "project_context_changed",
    "the account, audience or credential changed while consuming the project's local input",
    "retry the same project and manifest under the intended account"
);
refusal!(
    NATIVE_STATE_UNSAFE,
    "native_state_unsafe",
    "protected native state is unsafe or unreadable",
    "repair the owner-only DS config directory"
);
refusal!(
    NATIVE_STATE_UNAVAILABLE,
    "native_state_unavailable",
    "protected native state cannot be accessed",
    "repair the owner-only DS config directory"
);
refusal!(
    NATIVE_STATE_PROTECTION,
    "native_state_protection_unavailable",
    "this build has no protected-state adapter",
    "install a supported native ds build"
);
refusal!(
    NATIVE_STATE_ROOT,
    "native_state_root_invalid",
    "the configured state root is not absolute",
    "unset it or provide an absolute path"
);
refusal!(
    NATIVE_STATE_CONFLICT,
    "native_state_conflict",
    "another native operation holds the state lease",
    "retry after that operation finishes"
);
refusal!(
    NATIVE_CLEANUP,
    "native_cleanup_required",
    "revoked identity cleanup could not clear context",
    "repair protected state and run auth logout"
);
refusal!(
    AUTH_CONTEXT_MISMATCH,
    "auth_context_mismatch",
    "the protected native providers disagree on identity or named project",
    "repair the native identity before retrying the same --project"
);
refusal!(
    AUTH_INPUT,
    "auth_input_invalid",
    "ds-brain refused the request",
    "read the message; a repeated refusal is a deployment mismatch"
);
refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the gateway rejects the request, project access is missing, or the project is archived",
    "verify the account and its project access"
);
refusal!(
    AUTH_REVOKED,
    "auth_revoked",
    "the native session was permanently revoked",
    "sign in again interactively"
);
refusal!(
    AUTH_IDENTITY_MISMATCH,
    "auth_identity_mismatch",
    "the restored identity differs from the bound native session",
    "sign in again and report a repeated mismatch"
);
refusal!(
    AUTH_TRANSIENT,
    "auth_transient",
    "the service is temporarily unavailable",
    "retry without changing local state"
);
refusal!(
    AUTH_UNREADABLE,
    "auth_response_unreadable",
    "the response violates its closed bounded contract",
    "retry once, then update ds if it persists"
);
refusal!(
    NOT_FOUND,
    "transformer_not_found",
    "the service found no such project or transformer",
    "verify the exact --project id and project access"
);
refusal!(
    QUERY_INVALID,
    "project_dataset_query_invalid",
    "the boundary is malformed, limit or page token is invalid, or no exact dataset is named",
    "pass one WGS84 polygon inside completed coverage and use cursor plus generation exactly as returned"
);
refusal!(
    QUERY_NOT_HELD,
    "project_dataset_not_held",
    "the dataset room is absent or the requested boundary extends beyond its completed coverage",
    "run `ds data project-cache status --dataset <name>` and seed the required project coverage before querying"
);
refusal!(
    QUERY_PAGE_STALE,
    "project_dataset_page_stale",
    "the local dataset changed after the page cursor was issued",
    "restart the same boundary query without --cursor or --generation"
);
refusal!(
    QUERY_OUTPUT_REFUSED,
    "output_refused",
    "--geometry-out already exists or cannot be written",
    "choose a fresh writable GeoJSON output path"
);

const HEADLESS_REFUSALS: [Refusal; 20] = [
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
    HEADLESS_NO_PROJECT,
    PROJECT_CONTEXT_STALE,
    NATIVE_STATE_UNSAFE,
    NATIVE_STATE_UNAVAILABLE,
    NATIVE_STATE_PROTECTION,
    NATIVE_STATE_ROOT,
    NATIVE_STATE_CONFLICT,
    NATIVE_CLEANUP,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    AUTH_REJECTED,
    AUTH_REVOKED,
    AUTH_IDENTITY_MISMATCH,
    AUTH_TRANSIENT,
    AUTH_UNREADABLE,
    PROJECT_CONTEXT_CHANGED,
];

const STATUS_REFUSALS: &[Refusal] = &[
    NETWORK_ENGINE_MISSING,
    NETWORK_ENGINE_INCOMPATIBLE,
    INVALID_SCOPE,
    RETIRED,
    CATALOG_UNAVAILABLE,
    STORE_FAILED,
    DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
    NOT_FOUND,
    HEADLESS_REFUSALS[0],
    HEADLESS_REFUSALS[1],
    HEADLESS_REFUSALS[2],
    HEADLESS_REFUSALS[3],
    HEADLESS_REFUSALS[4],
    HEADLESS_REFUSALS[5],
    HEADLESS_REFUSALS[6],
    HEADLESS_REFUSALS[7],
    HEADLESS_REFUSALS[8],
    HEADLESS_REFUSALS[9],
    HEADLESS_REFUSALS[10],
    HEADLESS_REFUSALS[11],
    HEADLESS_REFUSALS[12],
    HEADLESS_REFUSALS[13],
    HEADLESS_REFUSALS[14],
    HEADLESS_REFUSALS[15],
    HEADLESS_REFUSALS[16],
    HEADLESS_REFUSALS[17],
    HEADLESS_REFUSALS[18],
    HEADLESS_REFUSALS[19],
];

const SEED_REFUSALS: &[Refusal] = &[
    NETWORK_ENGINE_MISSING,
    REMOTE_CONTEXT_RETIRED,
    REVISION_INVALID,
    REVISION_STALE,
    PUBLICATION_REFUSED,
    REPORTER_MISSING,
    REPORTER_INCOMPATIBLE,
    SURFACE_UNCOVERED,
    ENGINE_REFUSED,
    ENGINE_RESPONSE_INVALID,
    NETWORK_ENGINE_INCOMPATIBLE,
    INVALID_SCOPE,
    RETIRED,
    NO_DESIGN_EXTENT,
    CONFIRM,
    PROVIDER_UNAVAILABLE,
    ACQUISITION_FAILED,
    LOCAL_RESOURCE_REQUIRED,
    PUBLISHER_INVALID,
    PUBLISHER_MALFORMED,
    PUBLISHER_UNSUPPORTED,
    PUBLISHER_TOO_LARGE,
    SEED_INCOMPLETE,
    BUNDLE_UNAVAILABLE,
    CATALOG_UNAVAILABLE,
    STORE_FAILED,
    DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
    REFERENCE_BUNDLE_DOWNLOAD_FAILED_REFUSAL,
    NOT_FOUND,
    HEADLESS_REFUSALS[0],
    HEADLESS_REFUSALS[1],
    HEADLESS_REFUSALS[2],
    HEADLESS_REFUSALS[3],
    HEADLESS_REFUSALS[4],
    HEADLESS_REFUSALS[5],
    HEADLESS_REFUSALS[6],
    HEADLESS_REFUSALS[7],
    HEADLESS_REFUSALS[8],
    HEADLESS_REFUSALS[9],
    HEADLESS_REFUSALS[10],
    HEADLESS_REFUSALS[11],
    HEADLESS_REFUSALS[12],
    HEADLESS_REFUSALS[13],
    HEADLESS_REFUSALS[14],
    HEADLESS_REFUSALS[15],
    HEADLESS_REFUSALS[16],
    HEADLESS_REFUSALS[17],
    HEADLESS_REFUSALS[18],
    HEADLESS_REFUSALS[19],
];

const QUERY_OWN_REFUSALS: [Refusal; 9] = [
    INVALID_SCOPE,
    RETIRED,
    QUERY_INVALID,
    QUERY_NOT_HELD,
    QUERY_PAGE_STALE,
    QUERY_OUTPUT_REFUSED,
    CATALOG_UNAVAILABLE,
    STORE_FAILED,
    DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
];
const fn query_refusals() -> [Refusal; 9 + HEADLESS_REFUSALS.len() + 1] {
    let mut all = [QUERY_INVALID; 9 + HEADLESS_REFUSALS.len() + 1];
    let mut index = 0;
    while index < QUERY_OWN_REFUSALS.len() {
        all[index] = QUERY_OWN_REFUSALS[index];
        index += 1;
    }
    index = 0;
    while index < HEADLESS_REFUSALS.len() {
        all[QUERY_OWN_REFUSALS.len() + index] = HEADLESS_REFUSALS[index];
        index += 1;
    }
    all[QUERY_OWN_REFUSALS.len() + HEADLESS_REFUSALS.len()] = NOT_FOUND;
    all
}
const QUERY_REFUSAL_SET: [Refusal; 9 + HEADLESS_REFUSALS.len() + 1] = query_refusals();

pub static STATUS_COMMAND: Command = Command {
    id: "data.project-cache.status",
    path: &["data", "project-cache", "status"],
    contract: 1,
    summary: "Report the project's held extracts of canonical geographic datasets.",
    purpose: "Reports each dataset's requested/completed coverage, features, index, source versions and errors under the explicit project. --local-source reads its exact principal/project/revision room offline. Otherwise reads the catalogue; held rooms still report when it is unavailable. Missing data is never acquired.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        DATASET_ARG,
        LOCAL_SOURCE_ARG,
        SUMMARY_ARG,
        LANE_ARG,
    ],
    output: "Coverage and history by default; --summary keeps identity, source, readiness, coverage counts and seed/read commands. Includes `seeded`, `available`, `catalog`; --local-source adds `selected_source` provenance.",
    examples: &[
        Example {
            command: "ds data project-cache status --project gisagara --output json",
            note: "Reads every dataset the named project declares or holds. Costs nothing.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache status --project gisagara --local-source ./source.json --summary --output json",
            note: "Inspect one pinned local analytical source's held room, offline and without catalogue discovery.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache status --project gisagara --summary --output json",
            note: "Find usable datasets and their bounded read commands without acquisition history.",
            runnable: false,
        },
    ],
    refusals: STATUS_REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static SEED_COMMAND: Command = Command {
    id: "data.project-cache.seed",
    path: &["data", "project-cache", "seed"],
    contract: 1,
    summary: "Acquire the geographic datasets this project's design needs.",
    purpose: "Seeds design/MV coverage under --project. Buildings and contours are seeded once per project: published revisions replay first, Core fetches only uncovered Google Open Buildings tiles or samples the Rwanda DEM, the Reporter converts offline, and the delta is published as the next revision. Seeds declared datasets plus what it holds. --local-source/--publisher-input select pinned inputs instead. --refresh re-acquires and republishes. Incomplete results refuse with detail.result.",
    chapter: Chapter::Data,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        DATASET_ARG,
        LOCAL_SOURCE_ARG,
        PUBLISHER_INPUT_ARG,
        REMOTE_CONTEXT_ARG,
        MV_MODEL_ARG,
        REFRESH_ARG,
        LANE_ARG,
    ],
    output: "Per dataset: acquired cells, coverage, features, holding, warnings/errors, optional `selected_source` pins, and for buildings/contours `shared` (head revision, replayed and published revisions, source cells, freshness); plus `refresh`, `failed`, `complete` and captured transformer/MV sources.",
    examples: &[
        Example {
            command: "ds data project-cache seed --project gisagara --dataset google_open_buildings --yes --output json",
            note: "Seed building footprints once for the project: replay published revisions, fetch only uncovered tiles, publish the delta.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache seed --project gisagara --publisher-input ./publisher.json --yes --output json",
            note: "Convert pinned inputs and seed; reuse returned source_manifest offline.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache seed --project gisagara --local-source ./source.json --yes --output json",
            note: "Authorize the project and hold missing local analytical coverage.",
            runnable: false,
        },
    ],
    refusals: SEED_REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &["install", "footprints"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static QUERY_COMMAND: Command = Command {
    id: "data.project-cache.query",
    path: &["data", "project-cache", "query"],
    contract: 1,
    summary: "Export held project datasets inside one polygon.",
    purpose: "Exports one bounded GeoJSON page of actual intersections wholly inside completed project coverage. --local-source selects its exact retained principal/project/revision room offline, without catalogue, provider, input artifact or freshness reads; --dataset must match its native id. Missing coverage refuses. A truncated response supplies cursor and generation; a changed room refuses continuation. Use status --summary to find datasets and readiness.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT_ARG,
        QUERY_DATASET_ARG,
        LOCAL_SOURCE_ARG,
        QUERY_BOUNDARY_ARG,
        QUERY_GEOMETRY_OUT_ARG,
        QUERY_LIMIT_ARG,
        QUERY_CURSOR_ARG,
        QUERY_GENERATION_ARG,
        LANE_ARG,
    ],
    output: "Canonical dataset id and layer, source version, query boundary, rows_returned, truncated, generation, next cursor when truncated, and written GeoJSON path. The output file is one page; continue only with its cursor and generation.",
    examples: &[
        Example {
            command: "ds data project-cache query --project gisagara --dataset mv_line --boundary ./corridor.geojson --geometry-out ./held-mv.geojson --output json",
            note: "Export held MV features intersecting a covered corridor.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache query --project gisagara --local-source ./source.json --dataset <source-id> --boundary ./area.geojson --geometry-out ./footprints.geojson --output json",
            note: "Read one page from the exact pinned local source's completed room without upstream access.",
            runnable: false,
        },
    ],
    refusals: &QUERY_REFUSAL_SET,
    reference: Some("docs/reference/data.md"),
    search: &[
        "geojson",
        "extract",
        "spatial",
        "intersect",
        "village",
        "lv",
        "mv",
        "hv",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The crate's failure, as the refusal this command declared for it.
fn refused(error: ds_project_data::Failure) -> Failure {
    use ds_project_data::Failure as Cause;
    let message = error.message().to_owned();
    match error {
        // Neither command reads a print context, so these three cannot occur
        // here; they are named as a scope refusal rather than left unmapped.
        Cause::NotHeld(_) | Cause::Unsupported(_) | Cause::TooLarge(_) => {
            Failure::invalid(INVALID_SCOPE.code, message).remedy(INVALID_SCOPE.remedy)
        }
        Cause::AcquisitionFailed(_) => {
            Failure::failed(ACQUISITION_FAILED.code, message).remedy(ACQUISITION_FAILED.remedy)
        }
        Cause::ProviderUnavailable(_) => Failure::unavailable(PROVIDER_UNAVAILABLE.code, message)
            .remedy(PROVIDER_UNAVAILABLE.remedy),
        Cause::LocalResourceRequired(_) => Failure::conflict(LOCAL_RESOURCE_REQUIRED.code, message)
            .remedy(LOCAL_RESOURCE_REQUIRED.remedy),
        Cause::BundleUnavailable(_) => {
            Failure::unavailable(BUNDLE_UNAVAILABLE.code, message).remedy(BUNDLE_UNAVAILABLE.remedy)
        }
        Cause::CatalogInvalid(_) => Failure::unavailable(CATALOG_UNAVAILABLE.code, message)
            .remedy(CATALOG_UNAVAILABLE.remedy),
        Cause::Store(_) => {
            Failure::unavailable(STORE_FAILED.code, message).remedy(STORE_FAILED.remedy)
        }
        Cause::Retired(_) => Failure::invalid(RETIRED.code, message).remedy(RETIRED.remedy),
    }
}

fn explicit_dataset(inputs: &Inputs) -> Result<String, Failure> {
    let Some(dataset) = inputs.value("dataset") else {
        return Ok(String::new());
    };
    let dataset = dataset.trim();
    if dataset.is_empty() || dataset.len() > 128 {
        return Err(Failure::invalid(
            INVALID_SCOPE.code,
            "`--dataset` must be one exact canonical dataset id",
        )
        .remedy(INVALID_SCOPE.remedy));
    }
    Ok(dataset.to_owned())
}

/// This machine's holdings root: the same shared root the desktop and the
/// headless export read the reference assets from.
fn holdings_root() -> Result<PathBuf, Failure> {
    ds_layer_store::reference_cache::configured_root()
        .map_err(|error| Failure::unavailable(STORE_FAILED.code, error).remedy(STORE_FAILED.remedy))
}

/// The catalogue, read once per invocation and never cached: the reference
/// catalogue is ds-brain's, and a stale copy would declare a bundle that is no
/// longer published.
fn catalogue(
    lane: &str,
    project: &str,
) -> Result<Vec<ds_project_data::ReferenceResource>, Failure> {
    let rows =
        ds_cli_auth::data_distribution(lane, project, &DataDistributionRequest::ListDatasets {})?;
    ds_project_data::validate_resources(&rows).map_err(refused)
}

/// The room a held dataset lives in, as the kernel's overview reads it.
fn held_rooms(root: &Path, scope: &Scope) -> Result<BTreeMap<String, Value>, Failure> {
    let inventory = ds_layer_store::project_dataset_cache::project_inventory(
        root,
        &scope.principal,
        &scope.project,
    )
    .map_err(|error| Failure::unavailable(STORE_FAILED.code, error).remedy(STORE_FAILED.remedy))?;
    let mut rooms = BTreeMap::new();
    if let Some(map) = inventory["datasets"].as_object() {
        for (id, room) in map {
            rooms.insert(id.clone(), room.clone());
        }
    }
    Ok(rooms)
}

fn catalog_entry(dataset: &Dataset, label: &str, unpublished: bool) -> CatalogEntry {
    CatalogEntry {
        id: dataset.id.clone(),
        label: label.to_owned(),
        provider: dataset.provider.clone(),
        quality: serde_json::to_value(dataset.quality)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
        parameters: dataset.parameters.clone(),
        residency: "bundle".into(),
        unpublished,
    }
}

/// A catalogue row that is never held here: cloud-resident, queried on demand
/// (`docs/contracts/foundation-datasets.md` R2). Listed so an operator reads
/// where the dataset lives instead of a silence.
fn cloud_entry(resource: &ds_project_data::ReferenceResource) -> CatalogEntry {
    CatalogEntry {
        id: resource.id.clone(),
        label: resource.label.clone(),
        provider: "bigquery".into(),
        quality: "source".into(),
        parameters: BTreeMap::from([
            ("layer".to_owned(), json!(resource.layer)),
            ("country".to_owned(), json!(resource.country)),
            ("resource_id".to_owned(), json!(resource.id)),
        ]),
        residency: "cloud".into(),
        unpublished: false,
    }
}

/// A room's own identity, for a held dataset nobody declares any more.
fn room_entry(id: &str, room: &Value) -> CatalogEntry {
    CatalogEntry {
        id: id.to_owned(),
        label: id.to_owned(),
        provider: room["provider"].as_str().unwrap_or("").to_owned(),
        quality: room["quality"].as_str().unwrap_or("").to_owned(),
        parameters: room["parameters"]
            .as_object()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        residency: "bundle".into(),
        unpublished: false,
    }
}

/// Resolve `--dataset` against the catalogue: an exact id, a layer name, or a
/// retired alias answers as its authority row's id, with `answered_as` when
/// the name differed; a retired broad layer refuses with its alternatives.
fn resolve_explicit(
    explicit: &str,
    resources: &[ds_project_data::ReferenceResource],
    rooms: &BTreeMap<String, Value>,
) -> Result<(String, Option<Value>), Failure> {
    if explicit.is_empty() {
        return Ok((String::new(), None));
    }
    let found = ds_project_data::named(resources, explicit);
    let dataset = rooms
        .get(explicit)
        .map(|room| room_entry(explicit, room))
        .or_else(|| found.as_ref().map(|found| cloud_entry(found.resource)));
    let retired_layer = match dataset.as_ref() {
        Some(dataset) => policy::retired_dataset_layer(dataset),
        None => {
            ds_command_kernel::printing::context::retired_alternatives(explicit).map(|_| explicit)
        }
    };
    if let Some(layer) = retired_layer {
        let alternatives = ds_command_kernel::printing::context::retired_alternatives(layer)
            .expect("kernel identifies only retired foundation layers");
        return Err(Failure::invalid(
            RETIRED.code,
            format!(
                "{explicit} identifies retired layer {layer}; its detailed alternatives are {}",
                alternatives.join(", ")
            ),
        )
        .remedy(RETIRED.remedy));
    }
    match found {
        Some(found) => Ok((
            found.resource.id.clone(),
            found.answered_as.map(|requested| {
                json!({
                    "requested": requested,
                    "authority": {"id": found.resource.id, "layer": found.resource.layer},
                })
            }),
        )),
        None => Ok((explicit.to_owned(), None)),
    }
}

pub fn run_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    // A local read of the durable providers: no network, no token output.
    let Some(identity) = ds_cli_auth::probe_headless_identity_for_named_project(lane)? else {
        return Err(Failure::conflict(
            HEADLESS_SIGNED_OUT.code,
            "no native user is signed in for this lane",
        )
        .remedy(HEADLESS_SIGNED_OUT.remedy));
    };
    let scope = Scope {
        principal: identity.uid().to_owned(),
        project: project.to_owned(),
    };
    if !ds_command_kernel::execution_context::valid_project(project) {
        return Err(Failure::invalid(
            INVALID_SCOPE.code,
            "--project must be one exact DS project id",
        ));
    }
    let root = holdings_root()?;
    local_source::dispatch(
        inputs,
        &scope,
        |entry| {
            let fence = ds_cli_auth::capture_layer_scope_fence_for_project(lane, project)?;
            local_source::status(
                &root,
                &scope,
                lane,
                &entry,
                inputs.switch("summary"),
                || {
                    ds_cli_auth::verify_layer_scope_fence_for_project(
                        lane,
                        &fence,
                        &scope.principal,
                        project,
                    )
                },
            )
        },
        || catalog_status(inputs, &root, &scope, lane, project),
    )
}

fn catalog_status(
    inputs: &Inputs,
    root: &Path,
    scope: &Scope,
    lane: &str,
    project: &str,
) -> Result<Value, Failure> {
    let explicit = explicit_dataset(inputs)?;
    let rooms = held_rooms(root, scope)?;
    let (declared, resources, catalog) = match catalogue(lane, project) {
        Ok(resources) => {
            let declared = ds_project_data::declared(&resources).map_err(refused)?;
            let count = resources.len();
            (
                declared,
                resources,
                json!({"read": true, "resources": count}),
            )
        }
        Err(error) => (
            Vec::new(),
            Vec::new(),
            json!({"read": false, "code": error.code(), "reason": error.message()}),
        ),
    };
    let (explicit, answered_as) = resolve_explicit(&explicit, &resources, &rooms)?;
    let policy = buffer_policy(&[]).map_err(|error| {
        Failure::unavailable(STORE_FAILED.code, error).remedy(STORE_FAILED.remedy)
    })?;
    let mut rows: Vec<OverviewRow> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for entry in &declared {
        if !explicit.is_empty() && entry.dataset.id != explicit {
            continue;
        }
        seen.insert(entry.dataset.id.clone());
        let unpublished = match &entry.source {
            ds_project_data::DeclaredSource::Catalog(resource) => resource.is_unpublished(),
            _ => false,
        };
        rows.push(OverviewRow {
            dataset: catalog_entry(&entry.dataset, &entry.candidate.label, unpublished),
            held: rooms.get(&entry.dataset.id).cloned().unwrap_or(Value::Null),
        });
    }
    for (id, room) in &rooms {
        if seen.contains(id) || (!explicit.is_empty() && id != &explicit) {
            continue;
        }
        seen.insert(id.clone());
        // A seeded cloud room keeps its catalogue identity (label, cloud
        // residency); the room itself records only the digest.
        let dataset = match resources
            .iter()
            .find(|r| &r.id == id && r.is_cloud_resident())
        {
            Some(resource) => cloud_entry(resource),
            None => room_entry(id, room),
        };
        rows.push(OverviewRow {
            dataset,
            held: room.clone(),
        });
    }
    // Cloud-resident rows not seeded here are listed so the answer says
    // where they live. A named bundle row nobody declared or seeded is
    // listed once, unpublished or not seeded, rather than refused.
    for resource in &resources {
        let listed =
            resource.is_cloud_resident() || (!explicit.is_empty() && resource.id == explicit);
        if !listed
            || seen.contains(&resource.id)
            || (!explicit.is_empty() && resource.id != explicit)
        {
            continue;
        }
        seen.insert(resource.id.clone());
        let dataset = if resource.is_cloud_resident() {
            cloud_entry(resource)
        } else {
            catalog_entry(
                &ds_project_data::catalog_dataset(resource),
                &resource.label,
                resource.is_unpublished(),
            )
        };
        rows.push(OverviewRow {
            dataset,
            held: rooms.get(&resource.id).cloned().unwrap_or(Value::Null),
        });
    }
    if !explicit.is_empty() && rows.is_empty() {
        return Err(Failure::invalid(
            INVALID_SCOPE.code,
            format!("{explicit} is neither declared by this project nor held on this machine"),
        )
        .remedy(INVALID_SCOPE.remedy));
    }
    let mut overview = policy::overview(project, &policy, &rows);
    overview["lane"] = json!(lane);
    overview["catalog"] = catalog;
    if let Some(answered_as) = answered_as {
        overview["answered_as"] = answered_as;
    }
    if inputs.switch("summary") {
        Ok(compact_status(overview))
    } else {
        Ok(overview)
    }
}

/// Agent-sized discovery over the same kernel overview. The full response
/// remains the default for diagnosing one room's coverage and acquisitions.
fn compact_status(mut overview: Value) -> Value {
    let datasets = overview["datasets"].as_array().cloned().unwrap_or_default();
    overview["view"] = json!("summary");
    overview["datasets"] = Value::Array(
        datasets
            .iter()
            .map(|row| {
                let layer = row["parameters"]["layer"].as_str().unwrap_or("");
                let cloud = row["residency"] == "cloud";
                let read_command = match layer {
                    "rwanda_upi_parcels" => Some("data.parcels.query"),
                    "edcl_customers" => Some("data.customers.query"),
                    _ => None,
                };
                let seed_available = (cloud && read_command.is_some())
                    || (!cloud && row["ready_reason"] != "bundle_unpublished");
                let cells = |name: &str| row[name]["cells"].as_array().map(Vec::len).unwrap_or(0);
                json!({
                    "dataset_id": row["dataset_id"],
                    "layer": if layer.is_empty() { Value::Null } else { json!(layer) },
                    "label": row["label"],
                    "source": row["provider"],
                    "residency": row["residency"],
                    "read_command": read_command,
                    "held_read_command": if row["seeded"] == true { json!("data.project-cache.query") } else { Value::Null },
                    "seed_available": seed_available,
                    "seed_command": if seed_available { json!("data.project-cache.seed") } else { Value::Null },
                    "row_cap": if read_command.is_some() || row["seeded"] == true { json!(5000) } else { Value::Null },
                    "seeded": row["seeded"],
                    "ready": row["ready"],
                    "ready_reason": row["ready_reason"],
                    "local_holding": row["local_holding"],
                    "feature_count": row["feature_count"],
                    "index_state": row["index_state"],
                    "source_version": row["source_version"],
                    "available_version": row["available_version"],
                    "fresh": row["fresh"],
                    "requested_cells": cells("requested"),
                    "completed_cells": cells("completed"),
                    "stale_cells": cells("stale"),
                    "pending_queries": row["pending_queries"],
                    "expired_queries": row["expired_queries"],
                    "last_error": row["last_error"],
                })
            })
            .collect(),
    );
    overview
}

fn page_number(inputs: &Inputs, name: &str) -> Result<Option<i64>, Failure> {
    inputs
        .value(name)
        .map(|raw| {
            raw.parse::<i64>()
                .ok()
                .filter(|value| *value >= 0)
                .ok_or_else(|| {
                    Failure::invalid(
                        QUERY_INVALID.code,
                        format!("--{name} must be a nonnegative integer from a previous page"),
                    )
                    .remedy(QUERY_INVALID.remedy)
                })
        })
        .transpose()
}

fn query_page(inputs: &Inputs) -> Result<(u64, Option<i64>, Option<i64>), Failure> {
    let limit = inputs
        .require("limit")?
        .parse::<u64>()
        .ok()
        .filter(|value| (1..=5000).contains(value))
        .ok_or_else(|| {
            Failure::invalid(QUERY_INVALID.code, "--limit must be between 1 and 5000")
                .remedy(QUERY_INVALID.remedy)
        })?;
    let cursor = page_number(inputs, "cursor")?;
    let generation = page_number(inputs, "generation")?;
    if cursor.is_some() != generation.is_some() {
        return Err(Failure::invalid(
            QUERY_INVALID.code,
            "--cursor and --generation must be passed together from one truncated result",
        )
        .remedy(QUERY_INVALID.remedy));
    }
    Ok((limit, cursor, generation))
}

fn query_store_error(message: String) -> Failure {
    if message.contains("has not been seeded")
        || message.contains("has not been opened")
        || message.contains("has not been acquired")
        || message.contains("extends beyond completed coverage")
    {
        Failure::conflict(QUERY_NOT_HELD.code, message).remedy(QUERY_NOT_HELD.remedy)
    } else if message.contains("changed between pages") {
        Failure::conflict(QUERY_PAGE_STALE.code, message).remedy(QUERY_PAGE_STALE.remedy)
    } else {
        Failure::unavailable(STORE_FAILED.code, message).remedy(STORE_FAILED.remedy)
    }
}

pub fn run_query(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    if !ds_command_kernel::execution_context::valid_project(project) {
        return Err(Failure::invalid(
            INVALID_SCOPE.code,
            "--project must be one exact DS project id",
        )
        .remedy(INVALID_SCOPE.remedy));
    }
    let explicit = explicit_dataset(inputs)?;
    if explicit.is_empty() {
        return Err(
            Failure::invalid(QUERY_INVALID.code, "--dataset is required")
                .remedy(QUERY_INVALID.remedy),
        );
    }
    let (limit, cursor, generation) = query_page(inputs)?;
    let boundary = crate::foundation::read_boundary(Path::new(inputs.require("boundary")?))
        .map_err(|error| {
            Failure::invalid(QUERY_INVALID.code, error.message().to_owned())
                .remedy(QUERY_INVALID.remedy)
        })?;
    ds_command_kernel::reference_cache::validate_area(&boundary).map_err(|error| {
        Failure::invalid(QUERY_INVALID.code, error).remedy(QUERY_INVALID.remedy)
    })?;
    let out = crate::foundation::geometry_out(Path::new(inputs.require("geometry-out")?))?;

    let Some(identity) = ds_cli_auth::probe_headless_identity_for_named_project(lane)? else {
        return Err(Failure::conflict(
            HEADLESS_SIGNED_OUT.code,
            "no native user is signed in for this lane",
        )
        .remedy(HEADLESS_SIGNED_OUT.remedy));
    };
    let scope = Scope {
        principal: identity.uid().to_owned(),
        project: project.to_owned(),
    };
    let (resolved, fence) = local_source::dispatch(
        inputs,
        &scope,
        |entry| {
            let fence = ds_cli_auth::capture_layer_scope_fence_for_project(lane, project)?;
            Ok((entry, Some(fence)))
        },
        || {
            let resources = catalogue(lane, project)?;
            ds_project_data::declared::resolve(&resources, &explicit)
                .map_err(refused)?
                .ok_or_else(|| {
                    Failure::invalid(
                        INVALID_SCOPE.code,
                        format!("{explicit} is not a canonical dataset of this project"),
                    )
                    .remedy(INVALID_SCOPE.remedy)
                })
                .map(|entry| (entry, None))
        },
    )?;
    let root = holdings_root()?;
    let mut answer = query_dataset(
        &root,
        &scope,
        &resolved.dataset,
        &boundary,
        &out,
        (limit, cursor, generation),
        || match &fence {
            Some(fence) => ds_cli_auth::verify_layer_scope_fence_for_project(
                lane,
                fence,
                &scope.principal,
                project,
            ),
            None => Ok(()),
        },
    )?;
    if matches!(
        resolved.source,
        ds_project_data::DeclaredSource::LocalAnalytical(_)
    ) {
        answer["selected_source"] = local_source::metadata(&resolved);
    }
    Ok(answer)
}

fn query_dataset(
    root: &Path,
    scope: &Scope,
    dataset: &Dataset,
    boundary: &Value,
    out: &Path,
    pagination: (u64, Option<i64>, Option<i64>),
    mut verify_scope: impl FnMut() -> Result<(), Failure>,
) -> Result<Value, Failure> {
    verify_scope()?;
    let (limit, cursor, generation) = pagination;
    let dataset_key = policy::dataset_key(scope, dataset);
    let page = ds_layer_store::project_dataset_cache::execute_at(
        root,
        json!({
            "action": "query",
            "dataset_key": dataset_key,
            "principal": scope.principal,
            "project": scope.project,
            "area": boundary,
            "limit": limit,
            "after_id": cursor,
            "generation": generation,
        }),
    )
    .map_err(query_store_error)?;
    let features = page["features"].as_array().ok_or_else(|| {
        Failure::unavailable(STORE_FAILED.code, "project room returned no feature array")
            .remedy(STORE_FAILED.remedy)
    })?;
    verify_scope()?;
    crate::foundation::write_geometry(out, features)?;
    let truncated = page["truncated"] == true;
    let next = if truncated {
        json!({
            "cursor": page["next_after_id"],
            "generation": page["generation"],
        })
    } else {
        Value::Null
    };
    Ok(json!({
        "project": scope.project,
        "dataset_id": page["dataset_id"],
        "layer": dataset.parameters.get("layer"),
        "source_version": page["source_version"],
        "boundary": {"bounds": ds_command_kernel::reference_cache::area_bounds(&boundary).map_err(|error| Failure::internal(STORE_FAILED.code, error))?},
        "rows_returned": features.len(),
        "truncated": truncated,
        "generation": page["generation"],
        "next": next,
        "geometry": {"written_to": out.to_string_lossy(), "features": features.len()},
    }))
}

/// Bounded canonical cloud reads remain governed. Footprints and contours
/// are seeded through shared project revisions ([`run_seed`]), never here.
pub struct CliProvider<'a> {
    pub lane: &'a str,
    pub project: &'a str,
}

/// The bundle byte transfer a seeding host lends `ds-project-data`: the
/// pinned, credential-free fetch of ds-cli-auth, fenced to a signed-in lane.
pub fn bundle_fetch(lane: &str) -> impl FnMut(&BundleReceipt, &Path) -> Result<(), String> + '_ {
    move |receipt: &BundleReceipt, dest: &Path| -> Result<(), String> {
        ds_cli_auth::download_reference_bundle(
            lane,
            &receipt.url,
            &receipt.bundle_sha256,
            receipt.compressed_bytes,
            dest,
        )
        .map_err(|error| format!("{}: {}", error.code(), error.message()))
    }
}

fn decoded_page(decoded: Value) -> Acquisition {
    Acquisition {
        features: decoded["features"].as_array().cloned().unwrap_or_default(),
        source_version: decoded["sourceVersion"].as_str().unwrap_or("").to_owned(),
        source_receipt: decoded["sourceReceipt"].clone(),
    }
}

impl Provider for CliProvider<'_> {
    fn acquire(
        &mut self,
        dataset: &Dataset,
        area: &Value,
    ) -> Result<Acquisition, ds_project_data::Failure> {
        self.acquire_with(dataset, area, |request| {
            ds_cli_auth::data_distribution(self.lane, self.project, request)
        })
    }
}

impl CliProvider<'_> {
    fn acquire_with(
        &self,
        dataset: &Dataset,
        area: &Value,
        mut read: impl FnMut(&DataDistributionRequest) -> Result<Value, Failure>,
    ) -> Result<Acquisition, ds_project_data::Failure> {
        use ds_project_data::Failure as Cause;
        // A cloud-resident dataset seeds through its bounded read: the cell is
        // the boundary, and only a complete cell extends coverage.
        if dataset.provider == ds_project_data::declared::CLOUD_READ_PROVIDER {
            let layer = dataset
                .parameters
                .get("layer")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let request = match layer.as_str() {
                "rwanda_upi_parcels" => DataDistributionRequest::ParcelsQuery {
                    village_code: None,
                    cell_code: None,
                    bbox: None,
                    boundary: Some(area.clone()),
                    limit: None,
                },
                "edcl_customers" => DataDistributionRequest::CustomersQuery {
                    village_code: None,
                    cell_code: None,
                    bbox: None,
                    boundary: Some(area.clone()),
                    limit: None,
                },
                other => {
                    return Err(Cause::AcquisitionFailed(format!(
                        "{other} is cloud-resident but has no bounded read to seed from"
                    )));
                }
            };
            let response = read(&request).map_err(|error| {
                let message = format!("{}: {}", error.code(), error.message());
                match error.code() {
                    "data_distribution_unavailable" | "auth_transient" => {
                        Cause::ProviderUnavailable(message)
                    }
                    _ => Cause::AcquisitionFailed(message),
                }
            })?;
            let decoded = policy::decode_cloud_read(&response, area, &layer)
                .map_err(Cause::AcquisitionFailed)?;
            return Ok(decoded_page(decoded));
        }
        // Footprints and contours are seeded once per project through its
        // shared revisions (`data.project-cache.seed`); a print, which lends
        // only this door, reads what this machine already holds.
        Err(match dataset.id.as_str() {
            policy::BUILDINGS_DATASET_ID | policy::CONTOURS_DATASET_ID => {
                let source = if dataset.id == policy::BUILDINGS_DATASET_ID {
                    "Google Open Buildings footprints"
                } else {
                    "Rwanda DEM contours"
                };
                Cause::LocalResourceRequired(format!(
                    "{source} ({}, parameters {}) are seeded once per project through its shared revisions: run `ds data project-cache seed --project {} --dataset {} --yes`; this read uses what the machine holds",
                    dataset.id,
                    serde_json::to_string(&dataset.parameters).unwrap_or_default(),
                    self.project,
                    dataset.id
                ))
            }
            _ => Cause::AcquisitionFailed(format!(
                "{} has no admitted acquisition owner on this door",
                dataset.id
            )),
        })
    }
}

/// Every active transformer's design extent, from the service's saved rooms.
fn design_extents(
    lane: &str,
    inventory: &ds_cli_auth::HeadlessNamedProject<ds_cli_auth::TransformerInventory>,
) -> Result<Vec<policy::Extent>, Failure> {
    use ds_cli_auth::{TransformerKind, TransformerLifecycle};
    let names: Vec<String> = inventory
        .result()
        .rows()
        .iter()
        .filter(|row| {
            row.kind() == TransformerKind::Transformer
                && row.lifecycle() == TransformerLifecycle::Active
        })
        .map(|row| row.name().to_owned())
        .collect();
    // One restored session for the whole inventory: a restore per
    // transformer is a token refresh per transformer.
    let contexts =
        ds_cli_auth::transformer_contexts_for_project(lane, inventory.project_id(), &names)?;
    if contexts.identity() != inventory.identity()
        || contexts.project_id() != inventory.project_id()
    {
        return Err(Failure::conflict(
            "auth_context_mismatch",
            "project or native identity changed while reading design extents",
        ));
    }
    let mut extents = Vec::with_capacity(names.len());
    for (name, context) in names.iter().zip(contexts.into_result()) {
        let layers = serde_json::to_value(context.layers()).unwrap_or(Value::Null);
        extents.push(ds_project_data::extents::extent_of(name, &layers).map_err(refused)?);
    }
    Ok(extents)
}

/// Governed active MV heads contribute narrow route-segment coverage to the
/// same project room as transformer designs. Every page and download names the
/// project explicitly; no saved project selection participates.
fn mv_route_extents(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
) -> Result<(Vec<policy::Extent>, Vec<Value>), Failure> {
    let mut cursor = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut models = Vec::new();
    for page in 0..10 {
        let response = ds_cli_auth::grid_models_for_project(
            lane,
            project,
            &ds_cli_auth::GridModelsCommand::List {
                limit: 100,
                cursor: cursor.clone(),
                include_deleted: false,
            },
        )?;
        let data = response.data;
        models.extend(
            data["models"]
                .as_array()
                .ok_or_else(|| Failure::failed(INVALID_SCOPE.code, "MV catalog has no model rows"))?
                .iter()
                .cloned(),
        );
        if models.len() > 100 {
            return Err(Failure::invalid(
                INVALID_SCOPE.code,
                "project has over 100 MV model heads; seed a bounded model scope",
            ));
        }
        if data["more"] == false {
            break;
        }
        let next = data["next_cursor"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Failure::failed(INVALID_SCOPE.code, "MV catalog pagination is incomplete")
            })?
            .to_owned();
        if !seen.insert(next.clone()) || page == 9 {
            return Err(Failure::failed(
                INVALID_SCOPE.code,
                "MV catalog repeated a cursor or exceeded ten pages",
            ));
        }
        cursor = Some(next);
    }
    let mut extents = Vec::new();
    let mut sources = Vec::new();
    let mut projection_session = None;
    for row in models {
        if row["model_kind"] != "mv_line" || row["state"] != "active" {
            continue;
        }
        let text = |key: &str| -> Result<String, Failure> {
            row[key]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    Failure::failed(INVALID_SCOPE.code, format!("MV catalog lacks {key}"))
                })
        };
        let id = text("model_id")?;
        let revision = text("head_revision_id")?;
        let digest = text("head_model_digest")?;
        let receipt = ds_cli_auth::grid_models_for_project(
            lane,
            project,
            &ds_cli_auth::GridModelsCommand::Download {
                model: id.clone(),
                revision: revision.clone(),
            },
        )?;
        if receipt.data["verified"] != true || receipt.data["sha256"] != digest {
            return Err(Failure::failed(
                INVALID_SCOPE.code,
                format!("MV model {id} differs from its governed head"),
            ));
        }
        let bytes = receipt.bytes.ok_or_else(|| {
            Failure::failed(
                INVALID_SCOPE.code,
                format!("MV model {id} has no verified bytes"),
            )
        })?;
        let session = projection_session
            .get_or_insert_with(|| {
                ds_project_data::mv_projection::ProjectionSession::capture_for_lane(lane)
            })
            .as_ref()
            .map_err(mv_projection_fail)?;
        let routes = session
            .route_extents(&bytes, &revision, &id)
            .map_err(|error| mv_projection_fail(&error))?;
        sources.push(json!({"model_id":id,"revision_id":revision,"sha256":digest,"route_segments":routes.len()}));
        extents.extend(routes);
        if extents.len() > policy::MAX_EXTENTS {
            return Err(Failure::invalid(
                INVALID_SCOPE.code,
                "MV route coverage exceeds 5,000 acquisition segments; split the governed model scope",
            ));
        }
    }
    let current =
        ds_cli_auth::probe_headless_identity_for_named_project(lane)?.ok_or_else(|| {
            Failure::conflict(
                HEADLESS_SIGNED_OUT.code,
                "native user signed out during MV acquisition",
            )
        })?;
    if &current != identity {
        return Err(Failure::conflict(
            AUTH_CONTEXT_MISMATCH.code,
            "native identity changed during MV acquisition",
        ));
    }
    Ok((extents, sources))
}

fn local_mv_route_extents(path: &str, lane: &str) -> Result<(Vec<policy::Extent>, Value), Failure> {
    let local =
        ds_project_data::mv_projection::load_local(Path::new(path), lane).map_err(|error| {
            if error.unavailable() {
                mv_projection_fail(&error)
            } else {
                Failure::invalid(INVALID_SCOPE.code, format!("--mv-model: {error}"))
            }
        })?;
    let source = json!({"kind":"local_draft","path":path,
        "sha256":format!("sha256:{}",local.sha256),
        "revision_id":local.revision_id,"route_segments":local.route_extents.len()});
    Ok((local.route_extents, source))
}
fn mv_projection_fail(error: &ds_project_data::mv_projection::ProjectionError) -> Failure {
    if error.unavailable() {
        if error.code() == "network_engine_missing" {
            Failure::unavailable(NETWORK_ENGINE_MISSING.code, error.to_string())
        } else {
            Failure::unavailable(NETWORK_ENGINE_INCOMPATIBLE.code, error.to_string())
        }
        .remedy("Install or activate a compatible Network engine for MV geographic projection")
    } else {
        Failure::failed(INVALID_SCOPE.code, error.to_string())
    }
}

pub fn run_seed(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    remote_context::validate_source_mode(inputs)?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    explicit_dataset(inputs)?;
    let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
    let selected = local_source::select_seed(
        inputs,
        &Scope {
            principal: identity.uid().to_owned(),
            project: project.to_owned(),
        },
    )?;
    let requested = ds_cli_auth::TransformerSet::new(Vec::<String>::new())
        .map_err(|error| Failure::invalid(INVALID_SCOPE.code, error.to_string()))?;
    let inventory = ds_cli_auth::transformer_inventory_for_project(lane, project, &requested)?;
    if inventory.identity() != &identity {
        return Err(Failure::conflict(
            AUTH_CONTEXT_MISMATCH.code,
            "the native identity changed while authorizing the selected source's project",
        ));
    }
    let scope = Scope {
        principal: inventory.identity().uid().to_owned(),
        project: inventory.project_id().to_owned(),
    };
    match selected {
        Some(entry) => local_seed(inputs, &inventory, &scope, entry),
        None => catalog_seed(inputs, &inventory, &scope),
    }
}

fn local_seed(
    inputs: &Inputs,
    inventory: &ds_cli_auth::HeadlessNamedProject<ds_cli_auth::TransformerInventory>,
    scope: &Scope,
    selected: local_source::Selected,
) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let mut extents = design_extents(lane, inventory)?;
    let transformer_extent_count = extents.len();
    let (mut mv_extents, mut mv_sources) =
        mv_route_extents(lane, &scope.project, inventory.identity())?;
    if let Some(path) = inputs.value("mv-model") {
        let (draft, source) = local_mv_route_extents(path, lane)?;
        mv_extents.extend(draft);
        mv_sources.push(source);
    }
    let mv_extent_count = mv_extents.len();
    extents.extend(mv_extents);
    if extents.len() > policy::MAX_EXTENTS {
        return Err(Failure::invalid(
            INVALID_SCOPE.code,
            "combined transformer and MV route coverage exceeds 5,000 acquisition extents",
        ));
    }
    if extents.iter().all(|extent| extent.bounds.is_none()) {
        return Err(Failure::conflict(
            NO_DESIGN_EXTENT.code,
            "no active transformer design or governed MV route has an extent to derive coverage from",
        )
        .remedy(NO_DESIGN_EXTENT.remedy));
    }
    let root = holdings_root()?;
    let refresh = inputs.switch("refresh");
    let fence = ds_cli_auth::capture_layer_scope_fence_for_project(lane, &scope.project)?;
    let verify = || {
        ds_cli_auth::verify_layer_scope_fence_for_project(
            lane,
            &fence,
            &scope.principal,
            &scope.project,
        )
    };
    verify()?;
    let mut convert = shared::reporter_converter(lane);
    let (entry, producer) = local_source::materialize(&root, scope, selected, &mut convert)?;
    verify()?;
    let row = local_source::seed(&root, scope, &entry, &extents, refresh, verify)?;
    let complete = row["covered"] == true;
    finish_seed(
        json!({"lane":lane,"project":scope.project,"datasets":[row],"seeded":1,
        "refresh":refresh,"failed":usize::from(!complete),"complete":complete,
        "transformer_extent_count":transformer_extent_count,"mv_route_extent_count":mv_extent_count,
        "mv_sources":mv_sources,"publisher_source":producer}),
    )
}

fn catalog_seed(
    inputs: &Inputs,
    inventory: &ds_cli_auth::HeadlessNamedProject<ds_cli_auth::TransformerInventory>,
    scope: &Scope,
) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inventory.project_id().to_owned();
    let explicit = explicit_dataset(inputs)?;
    let refresh = inputs.switch("refresh");
    let root = holdings_root()?;
    let resources = catalogue(lane, &project)?;
    let rooms = held_rooms(&root, scope)?;
    let (explicit, answered_as) = resolve_explicit(&explicit, &resources, &rooms)?;
    let declared = ds_project_data::declared(&resources).map_err(refused)?;
    let held: Vec<SeedCandidate> = rooms
        .iter()
        .filter(|(id, room)| policy::retired_dataset_layer(&room_entry(id, room)).is_none())
        .map(|(id, _)| SeedCandidate {
            id: id.clone(),
            label: String::new(),
            seeded: true,
        })
        .collect();
    let candidates: Vec<SeedCandidate> = declared.iter().map(|d| d.candidate.clone()).collect();
    let targets = policy::seed_targets(&candidates, &held, &explicit).map_err(|error| {
        Failure::invalid(INVALID_SCOPE.code, if error == policy::SEED_SCOPE_REFUSAL {
            "this project declares no dataset and holds none; name the one to seed with --dataset".to_owned()
        } else {
            error
        })
        .remedy(INVALID_SCOPE.remedy)
    })?;
    let mut extents = design_extents(lane, inventory)?;
    let transformer_extent_count = extents.len();
    let (mut mv_extents, mut mv_sources) = mv_route_extents(lane, &project, inventory.identity())?;
    if let Some(path) = inputs.value("mv-model") {
        let (draft, source) = local_mv_route_extents(path, lane)?;
        mv_extents.extend(draft);
        mv_sources.push(source);
    }
    let mv_extent_count = mv_extents.len();
    extents.extend(mv_extents);
    if extents.len() > policy::MAX_EXTENTS {
        return Err(Failure::invalid(
            INVALID_SCOPE.code,
            "combined transformer and MV route coverage exceeds 5,000 acquisition extents",
        ));
    }
    if extents.iter().all(|extent| extent.bounds.is_none()) {
        return Err(Failure::conflict(
            NO_DESIGN_EXTENT.code,
            "no active transformer design or governed MV route has an extent to derive coverage from",
        )
        .remedy(NO_DESIGN_EXTENT.remedy));
    }
    // What is seeded here is printed: the capture holds what a landscape
    // or portrait sheet shows around the design, not a metre count.
    let mut policy = buffer_policy(&[]).map_err(|error| {
        Failure::unavailable(STORE_FAILED.code, error).remedy(STORE_FAILED.remedy)
    })?;
    policy.sheets = both_orientations();
    let mut provider = CliProvider {
        lane,
        project: &project,
    };
    let mut fetch = bundle_fetch(lane);
    let mut rows = Vec::new();
    let mut failed = 0_usize;
    // Opened only when a shared dataset is seeded, removed with the run.
    let mut seeding: Option<SharedSeeding> = None;
    for target in targets {
        let resolved = match declared.iter().find(|d| d.dataset.id == target) {
            Some(found) => Some(found.clone()),
            None => ds_project_data::declared::resolve(&resources, &target).map_err(refused)?,
        };
        let Some(entry) = resolved else {
            failed += 1;
            rows.push(json!({
                "dataset_id": target,
                "clusters": 0, "acquired": 0, "feature_count": 0, "local_holding": false,
                "warnings": [],
                "error": format!("{target} is not a dataset this client can hold"),
            }));
            continue;
        };
        if matches!(
            entry.source,
            ds_project_data::DeclaredSource::Buildings
                | ds_project_data::DeclaredSource::Contours(_)
        ) {
            let row = shared_seed_row(
                &mut seeding,
                lane,
                &root,
                scope,
                &entry,
                &policy,
                &extents,
                refresh,
                rooms.contains_key(&entry.dataset.id),
            );
            if row.get("error").is_some() {
                failed += 1;
            }
            rows.push(row);
            continue;
        }
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        // `--refresh` is the only door to the crate's refresh: it re-reads
        // the whole plan, where an ordinary seed acquires only the gaps.
        let settled = if refresh {
            ds_project_data::refresh(
                &root,
                scope,
                &entry.dataset,
                &entry.source,
                &policy,
                &extents,
                &[],
                &mut hosts,
            )
        } else {
            ds_project_data::ensure(
                &root,
                scope,
                &entry.dataset,
                &entry.source,
                &policy,
                &extents,
                &[],
                Mode::Acquire(&mut hosts),
            )
        };
        match settled {
            Ok(outcome) => {
                let mut row = json!({
                    "dataset_id": entry.dataset.id,
                    "label": entry.candidate.label,
                    "dataset_key": outcome.dataset_key,
                    "clusters": outcome.clusters,
                    "acquired": outcome.acquired,
                    "skipped": outcome.skipped,
                    "covered": outcome.covered,
                    "feature_count": outcome.features,
                    "local_holding": true,
                    "shared": false,
                    "coverage": outcome.coverage,
                    "warnings": outcome.warnings,
                });
                if !outcome.covered {
                    failed += 1;
                    row["error"] = json!(
                        "coverage is not complete; another acquisition may still be preparing this area"
                    );
                }
                rows.push(row);
            }
            Err(error) => {
                failed += 1;
                rows.push(json!({
                    "dataset_id": entry.dataset.id,
                    "label": entry.candidate.label,
                    "clusters": 0, "acquired": 0, "feature_count": 0,
                    "local_holding": rooms.contains_key(&entry.dataset.id),
                    "warnings": [],
                    "code": error.code(),
                    "error": format!("{}: {}", error.code(), error.message()),
                }));
            }
        }
    }
    let mut receipt = json!({
        "lane": lane,
        "project": project,
        "datasets": rows,
        "seeded": rows.len(),
        "refresh": refresh,
        "failed": failed,
        "complete": failed == 0,
        "transformer_extent_count": transformer_extent_count,
        "mv_route_extent_count": mv_extent_count,
        "mv_sources": mv_sources,
    });
    if let Some(answered_as) = answered_as {
        receipt["answered_as"] = answered_as;
    }
    finish_seed(receipt)
}

fn finish_seed(receipt: Value) -> Result<Value, Failure> {
    if receipt["complete"] == true && receipt["failed"] == 0 {
        return Ok(receipt);
    }
    Err(Failure::failed(SEED_INCOMPLETE.code,
        "project dataset acquisition is incomplete; inspect the per-dataset errors in result detail")
        .remedy(SEED_INCOMPLETE.remedy).detail(json!({"result":receipt})))
}

fn coverage_cells(value: &Value) -> usize {
    value["cells"].as_array().map(Vec::len).unwrap_or(0)
}

fn dataset_lines(dataset: &Value) -> String {
    let held = coverage_cells(&dataset["completed"]);
    let asked = coverage_cells(&dataset["requested"]);
    if dataset["residency"] == "cloud" && held == 0 {
        return format!(
            "  {} · in the cloud ({}) — read on demand, or seeded for this project's extents; not installed nationally",
            dataset["dataset_id"].as_str().unwrap_or("?"),
            dataset["label"].as_str().unwrap_or("?"),
        );
    }
    let mut line = format!(
        "  {} · {} feature(s) · index {} · {} covered area(s) of {} requested{}{}",
        dataset["dataset_id"].as_str().unwrap_or("?"),
        dataset["feature_count"].as_u64().unwrap_or(0),
        dataset["index_state"].as_str().unwrap_or("?"),
        held,
        asked,
        if dataset["seeded"] == Value::Bool(false) {
            " · not seeded on this computer"
        } else {
            ""
        },
        match dataset["ready_reason"].as_str() {
            Some(reason) if dataset["seeded"] == Value::Bool(true) =>
                format!(" · not ready: {reason}"),
            _ => String::new(),
        },
    );
    if let Some(version) = dataset["source_version"].as_str().filter(|v| !v.is_empty()) {
        line.push_str(&format!("\n    source version {version}"));
    }
    // Freshness is reported, never acted on. Naming the stale areas is what
    // lets an operator decide to spend money on a refresh; saying nothing
    // would let obsolete rows pass for current ones.
    let stale = coverage_cells(&dataset["stale"]);
    if stale > 0 {
        let available = dataset["available_version"].as_str().unwrap_or("");
        line.push_str(&format!(
            "\n    {stale} held area(s) completed under a superseded version{}; refresh to re-acquire, nothing is removed until it lands",
            if available.is_empty() {
                String::new()
            } else {
                format!(" (provider now publishes {available})")
            },
        ));
    }
    let expired = dataset["expired_queries"].as_u64().unwrap_or(0);
    if expired > 0 {
        line.push_str(&format!(
            "\n    {expired} abandoned acquisition(s) expired and no longer count as pending"
        ));
    }
    if let Some(policy) = dataset["buffer_policy"].as_object() {
        let sheets = policy
            .get("sheets")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        line.push_str(&format!(
            "\n    buffers {} m design / {} m isolated · rule {}{} · {}",
            policy["design_buffer_m"].as_f64().unwrap_or(0.0),
            policy["isolated_buffer_m"].as_f64().unwrap_or(0.0),
            policy["isolated_rule"].as_str().unwrap_or("?"),
            if policy["provisional"] == Value::Bool(true) {
                " (provisional)"
            } else {
                ""
            },
            match sheets {
                0 => "metre buffer only".to_owned(),
                n => format!("capture fills {n} sheet shape(s)"),
            },
        ));
    }
    if let Some(error) = dataset["last_error"].as_str() {
        line.push_str(&format!("\n    last error: {error}"));
    }
    line
}

pub fn render_status(data: &Value) -> String {
    let datasets = data["datasets"].as_array().cloned().unwrap_or_default();
    if datasets.is_empty() {
        return format!(
            "{} holds no project dataset extracts yet\n",
            data["project"].as_str().unwrap_or("this project")
        );
    }
    let mut out = format!(
        "{} · {} dataset(s), {} seeded on this computer\n",
        data["project"].as_str().unwrap_or("?"),
        datasets.len(),
        data["seeded"].as_u64().unwrap_or(0),
    );
    if data["catalog"]["read"] == Value::Bool(false) {
        out.push_str(&format!(
            "  catalogue not read ({}): declared datasets are not listed\n",
            data["catalog"]["reason"].as_str().unwrap_or("?")
        ));
    }
    for dataset in &datasets {
        if data["view"] == "summary" {
            out.push_str(&format!(
                "  {} · {} · {} · {} feature(s) · {} / {} covered area(s){}\n",
                dataset["parameters"]["layer"]
                    .as_str()
                    .or_else(|| dataset["layer"].as_str())
                    .or_else(|| dataset["dataset_id"].as_str())
                    .unwrap_or("?"),
                dataset["residency"].as_str().unwrap_or("?"),
                if dataset["ready"] == true {
                    "ready"
                } else {
                    dataset["ready_reason"].as_str().unwrap_or("not ready")
                },
                dataset["feature_count"].as_u64().unwrap_or(0),
                dataset["completed_cells"].as_u64().unwrap_or(0),
                dataset["requested_cells"].as_u64().unwrap_or(0),
                dataset["read_command"]
                    .as_str()
                    .map(|command| format!(" · {command}"))
                    .unwrap_or_default(),
            ));
            continue;
        }
        out.push_str(&dataset_lines(dataset));
        out.push('\n');
    }
    out
}

pub fn render_query(data: &Value) -> String {
    let mut out = format!(
        "{} · {} intersecting feature(s) written to {}\n",
        data["layer"].as_str().unwrap_or("dataset"),
        data["rows_returned"].as_u64().unwrap_or(0),
        data["geometry"]["written_to"].as_str().unwrap_or("?"),
    );
    if data["truncated"] == true {
        out.push_str(&format!(
            "  More features remain: --cursor {} --generation {} (write the next page to a new file)\n",
            data["next"]["cursor"], data["next"]["generation"],
        ));
    }
    out
}

pub fn render_seed(data: &Value) -> String {
    let datasets = data["datasets"].as_array().cloned().unwrap_or_default();
    let mut out = format!(
        "{} {} dataset(s) for {}\n",
        if data["refresh"] == Value::Bool(true) {
            "refreshed"
        } else {
            "seeded"
        },
        datasets.len(),
        data["project"].as_str().unwrap_or("?")
    );
    for dataset in &datasets {
        out.push_str(&format!(
            "  {} · {} cluster(s) · {} acquisition(s) this run · {} feature(s) held{}\n",
            dataset["dataset_id"].as_str().unwrap_or("?"),
            dataset["clusters"].as_u64().unwrap_or(0),
            dataset["acquired"].as_u64().unwrap_or(0),
            dataset["feature_count"].as_u64().unwrap_or(0),
            if dataset["local_holding"] == Value::Bool(true) {
                " · held on this computer"
            } else {
                ""
            },
        ));
        // A dataset that failed says so on its own line. Reporting only the
        // total would let a run that acquired nothing read as a seed.
        if let Some(error) = dataset["error"].as_str() {
            out.push_str(&format!("    not acquired: {error}\n"));
        }
        for warning in dataset["warnings"].as_array().cloned().unwrap_or_default() {
            if let Some(text) = warning.as_str() {
                out.push_str(&format!("    note: {text}\n"));
            }
        }
    }
    let failed = data["failed"].as_u64().unwrap_or(0);
    if failed > 0 {
        out.push_str(&format!(
            "{failed} dataset(s) did not complete; held data is kept and retrying acquires only what is still missing\n"
        ));
    }
    out
}

#[cfg(test)]
#[path = "project_cache_tests.rs"]
mod tests;
