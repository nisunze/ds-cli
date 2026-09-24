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
    ContourParameters, DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL, DataDistributionRequest,
    PrintContextKind, REFERENCE_BUNDLE_DOWNLOAD_FAILED_REFUSAL,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_dataset_cache::{
    self as policy, BUILDINGS_DATASET_ID, CONTOURS_DATASET_ID, CatalogEntry, Dataset, OverviewRow,
    Scope, SeedCandidate, both_orientations, buffer_policy,
};
use ds_project_data::{Acquisition, BundleReceipt, Hosts, Mode, Provider};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const DATASET_ARG: Arg = Arg::value(
    "dataset",
    "<dataset-id>",
    "One canonical dataset id, catalogue layer name, or retired alias (answered as its authority). Omitted: every dataset this project declares plus any it holds, even holding none yet.",
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
    "Optional exact local DS Grid draft whose route also needs geographic coverage; the receipt pins its SHA-256. Seed still authorizes --project.",
);

const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

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
    "the named dataset is not one this client can hold, or the project declares no dataset at all",
    "pass a --dataset id listed by `ds data project-cache status`, or omit it to seed what the project declares"
);
refusal!(
    NO_DESIGN_EXTENT,
    "project_has_no_extent",
    "the project holds no active transformer design or governed MV route to derive coverage from",
    "save a transformer design or publish an MV model head, then seed"
);
refusal!(
    PROVIDER_UNAVAILABLE,
    "dataset_provider_unavailable",
    "the governed provider could not answer the acquisition",
    "restore the connection and retry; previously held data is kept and never partially claimed as ready"
);
refusal!(
    ACQUISITION_FAILED,
    "project_dataset_acquisition_failed",
    "the provider or the room refused one acquisition cell; held data is intact",
    "fix the cause named on the dataset's row and retry; only the missing coverage is acquired again"
);
refusal!(
    BUNDLE_UNAVAILABLE,
    "reference_bundle_unavailable",
    "the catalogue row publishes no verified bundle, or it could not be installed",
    "publish the dataset's bundle, then seed again"
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
    "ds-brain's reference catalogue could not be read or is not a valid published catalogue",
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
    "read the message; the area is the kernel's own plan, so a repeated refusal is a deployment mismatch"
);
refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the gateway rejects the verified request, the user lacks project access, or the project is archived",
    "verify the account and its project access; device credentials need the bulk lane's deployed verifier"
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

const HEADLESS_REFUSALS: [Refusal; 19] = [
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
];

const STATUS_REFUSALS: &[Refusal] = &[
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
];

const SEED_REFUSALS: &[Refusal] = &[
    INVALID_SCOPE,
    RETIRED,
    NO_DESIGN_EXTENT,
    CONFIRM,
    PROVIDER_UNAVAILABLE,
    ACQUISITION_FAILED,
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
];

pub static STATUS_COMMAND: Command = Command {
    id: "data.project-cache.status",
    path: &["data", "project-cache", "status"],
    contract: 1,
    summary: "Report the project's held extracts of canonical geographic datasets.",
    purpose: "Reads what the explicitly named project holds on this machine, per dataset: requested and completed coverage (kept separate, so a failed acquisition never reads as a holding), feature count, index state, source versions, buffer policy and last error. Stale coverage is reported, never deleted; abandoned acquisitions read as expired. Declared datasets held nowhere yet are listed as not seeded. No provider, no cost: only the reference catalogue is read, and held rooms are reported even when it cannot be. Saved active-project selection is ignored.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, DATASET_ARG, LANE_ARG],
    output: "Per dataset: identity, seeded, ready, local_holding, source versions, stale coverage, buffer policy, requested and completed coverage, feature count, index state, pending and expired acquisitions and last error; plus `seeded`, `available` and `catalog`.",
    examples: &[
        Example {
            command: "ds data project-cache status --project gisagara --output json",
            note: "Reads every dataset the named project declares or holds. Costs nothing.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache status --project gisagara --dataset google_open_buildings --output json",
            note: "Reads one dataset's own coverage and readiness.",
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
    purpose: "Derives coverage from the explicitly named project's active transformer designs, exact governed MV route segments, and an optional local DS Grid draft supplied with --mv-model. It buffers and fuses them, then acquires only missing data; a re-run over unchanged design acquires nothing. With no --dataset it seeds what this project declares plus what it holds. Confirmed, because it queries a source. Saved active-project selection is ignored. Held data survives a failure, and a partial acquisition is never ready.",
    chapter: Chapter::Data,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, DATASET_ARG, MV_MODEL_ARG, LANE_ARG],
    output: "Per dataset: clusters, cells acquired this run, coverage, feature count, local holding, warnings and its own failure cause; plus `failed` and `complete`.",
    examples: &[
        Example {
            command: "ds data project-cache seed --project gisagara --dataset google_open_buildings --yes --output json",
            note: "Seeds building footprints for the whole project's fused coverage.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache seed --project gisagara --yes --output json",
            note: "Seeds every dataset this project declares, holding none too.",
            runnable: false,
        },
    ],
    refusals: SEED_REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &["install", "footprints"],
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
fn catalogue(lane: &str) -> Result<Vec<ds_project_data::ReferenceResource>, Failure> {
    let rows = ds_cli_auth::data_distribution(lane, &DataDistributionRequest::ListDatasets {})?;
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
) -> Result<(String, Option<Value>), Failure> {
    if explicit.is_empty() {
        return Ok((String::new(), None));
    }
    match ds_project_data::named(resources, explicit) {
        Some(found) => Ok((
            found.resource.id.clone(),
            found.answered_as.map(|requested| {
                json!({
                    "requested": requested,
                    "authority": {"id": found.resource.id, "layer": found.resource.layer},
                })
            }),
        )),
        None => {
            if let Some(alternatives) =
                ds_command_kernel::printing::context::retired_alternatives(explicit)
            {
                return Err(Failure::invalid(
                    RETIRED.code,
                    format!(
                        "{explicit} is retired; its detailed alternatives are {}",
                        alternatives.join(", ")
                    ),
                )
                .remedy(RETIRED.remedy));
            }
            Ok((explicit.to_owned(), None))
        }
    }
}

pub fn run_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let explicit = explicit_dataset(inputs)?;
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
    let rooms = held_rooms(&root, &scope)?;
    let (declared, resources, catalog) = match catalogue(lane) {
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
    let (explicit, answered_as) = resolve_explicit(&explicit, &resources)?;
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
    Ok(overview)
}

/// The host's door to ds-brain's `query_print_context`, decoded by the kernel.
/// Shared with `ds report project export --seed`, which seeds the printed
/// transformer through the same door.
pub struct CliProvider<'a> {
    pub lane: &'a str,
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

fn contour_parameters(dataset: &Dataset) -> Option<ContourParameters> {
    Some(ContourParameters {
        minor_interval_m: dataset.parameters.get("minor_interval_m")?.as_u64()? as u16,
        index_interval_m: dataset.parameters.get("index_interval_m")?.as_u64()? as u16,
        sample_spacing_m: dataset.parameters.get("sample_spacing_m")?.as_f64()?,
    })
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
            let response =
                ds_cli_auth::data_distribution(self.lane, &request).map_err(|error| {
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
        let (kind, contour_parameters) = match dataset.id.as_str() {
            BUILDINGS_DATASET_ID => (PrintContextKind::GoogleOpenBuildings, None),
            CONTOURS_DATASET_ID => (
                PrintContextKind::ElevationContours,
                Some(contour_parameters(dataset).ok_or_else(|| {
                    Cause::AcquisitionFailed(
                        "the contour dataset carries no authored intervals".into(),
                    )
                })?),
            ),
            other => {
                return Err(Cause::AcquisitionFailed(format!(
                    "{other} has no governed provider door"
                )));
            }
        };
        let request = DataDistributionRequest::QueryPrintContext {
            context_kind: kind,
            area: area.clone(),
            contour_parameters,
        };
        let response = ds_cli_auth::data_distribution(self.lane, &request).map_err(|error| {
            let message = format!("{}: {}", error.code(), error.message());
            match error.code() {
                "data_distribution_unavailable" | "auth_transient" => {
                    Cause::ProviderUnavailable(message)
                }
                _ => Cause::AcquisitionFailed(message),
            }
        })?;
        let decoded = match kind {
            PrintContextKind::GoogleOpenBuildings => policy::decode_buildings(&response, area),
            PrintContextKind::ElevationContours => policy::decode_contours(&response, area),
        }
        .map_err(Cause::AcquisitionFailed)?;
        Ok(decoded_page(decoded))
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
        let routes = ds_project_data::mv_projection::route_extents(&bytes, &revision, &id)
            .map_err(|error| Failure::failed(INVALID_SCOPE.code, error))?;
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

fn local_mv_route_extents(path: &str) -> Result<(Vec<policy::Extent>, Value), Failure> {
    let local = ds_project_data::mv_projection::load_local(Path::new(path))
        .map_err(|e| Failure::invalid(INVALID_SCOPE.code, format!("--mv-model: {e}")))?;
    let source = json!({"kind":"local_draft","path":path,
        "sha256":format!("sha256:{}",local.sha256),
        "revision_id":local.revision_id,"route_segments":local.route_extents.len()});
    Ok((local.route_extents, source))
}

pub fn run_seed(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let explicit = explicit_dataset(inputs)?;
    let requested = ds_cli_auth::TransformerSet::new(Vec::<String>::new())
        .map_err(|error| Failure::invalid(INVALID_SCOPE.code, error.to_string()))?;
    let inventory = ds_cli_auth::transformer_inventory_for_project(lane, project, &requested)?;
    let project = inventory.project_id().to_owned();
    let scope = Scope {
        principal: inventory.identity().uid().to_owned(),
        project: project.clone(),
    };
    let root = holdings_root()?;
    let resources = catalogue(lane)?;
    let (explicit, answered_as) = resolve_explicit(&explicit, &resources)?;
    let declared = ds_project_data::declared(&resources).map_err(refused)?;
    let rooms = held_rooms(&root, &scope)?;
    let held: Vec<SeedCandidate> = rooms
        .keys()
        .map(|id| SeedCandidate {
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
    let mut extents = design_extents(lane, &inventory)?;
    let transformer_extent_count = extents.len();
    let (mut mv_extents, mut mv_sources) = mv_route_extents(lane, &project, inventory.identity())?;
    if let Some(path) = inputs.value("mv-model") {
        let (draft, source) = local_mv_route_extents(path)?;
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
    let mut provider = CliProvider { lane };
    let mut fetch = bundle_fetch(lane);
    let mut rows = Vec::new();
    let mut failed = 0_usize;
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
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        match ds_project_data::ensure(
            &root,
            &scope,
            &entry.dataset,
            &entry.source,
            &policy,
            &extents,
            &[],
            Mode::Acquire(&mut hosts),
        ) {
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
        "failed": failed,
        "complete": failed == 0,
        "transformer_extent_count": transformer_extent_count,
        "mv_route_extent_count": mv_extent_count,
        "mv_sources": mv_sources,
    });
    if let Some(answered_as) = answered_as {
        receipt["answered_as"] = answered_as;
    }
    Ok(receipt)
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
        out.push_str(&dataset_lines(dataset));
        out.push('\n');
    }
    out
}

pub fn render_seed(data: &Value) -> String {
    let datasets = data["datasets"].as_array().cloned().unwrap_or_default();
    let mut out = format!(
        "seeded {} dataset(s) for {}\n",
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
mod tests {
    use super::*;

    #[test]
    fn reading_is_free_and_acquiring_is_confirmed() {
        // Status must never be able to spend provider cost, and seeding must
        // never be able to run without an explicit human decision.
        assert_eq!(STATUS_COMMAND.effect, Effect::ReadOnly);
        assert!(!STATUS_COMMAND.effect.needs_confirmation());
        assert_eq!(SEED_COMMAND.effect, Effect::ArtifactWrite);
        assert!(SEED_COMMAND.effect.needs_confirmation());
        assert!(
            SEED_COMMAND
                .refusals
                .iter()
                .any(|r| r.code == "confirmation_required")
        );
    }

    /// Both commands are background project work under an explicit address.
    /// The gateway still decides membership; the saved selection is unused.
    #[test]
    fn project_address_is_required_without_a_map_app() {
        for command in [&STATUS_COMMAND, &SEED_COMMAND] {
            assert_eq!(command.authority, Authority::HeadlessProject);
            let names: Vec<&str> = command.args.iter().map(|arg| arg.name).collect();
            assert!(names.contains(&"lane"), "{} lost its lane", command.id);
            assert!(
                command
                    .args
                    .iter()
                    .any(|arg| arg.name == "project" && arg.required)
            );
            for forbidden in ["desktop-descriptor", "url", "action", "body"] {
                assert!(
                    !names.contains(&forbidden),
                    "{} accepts a {forbidden} override",
                    command.id
                );
            }
        }
    }

    /// The descriptor and the executor say the same thing about an omitted
    /// `--dataset`: first use on any computer seeds what the project declares.
    #[test]
    fn an_omitted_dataset_declares_the_first_use_behaviour() {
        let dataset = SEED_COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "dataset")
            .expect("seed declares its dataset argument");
        assert!(!dataset.required, "the dataset is optional by contract");
        let summary = dataset.summary.to_lowercase();
        assert!(summary.contains("declares"), "{}", dataset.summary);
        assert!(
            summary.contains("holding none") || summary.contains("holds none"),
            "the first-use case is the one an operator hits first: {}",
            dataset.summary,
        );
        assert!(SEED_COMMAND.purpose.contains("plus what it holds"));
        assert!(!SEED_COMMAND.purpose.contains("everything published"));
    }

    /// Every failure the holdings crate can answer with maps onto a refusal
    /// the seed command declares, under the crate's own code.
    #[test]
    fn every_holdings_failure_is_a_declared_seed_refusal() {
        use ds_project_data::Failure as Cause;
        for cause in [
            Cause::NotHeld("x".into()),
            Cause::AcquisitionFailed("x".into()),
            Cause::ProviderUnavailable("x".into()),
            Cause::BundleUnavailable("x".into()),
            Cause::Unsupported("x".into()),
            Cause::TooLarge("x".into()),
            Cause::CatalogInvalid("x".into()),
            Cause::Store("x".into()),
        ] {
            let failure = refused(cause.clone());
            let declared = SEED_REFUSALS.iter().any(|r| r.code == failure.code());
            let mapped_to_scope = failure.code() == cause.code()
                || failure.code() == INVALID_SCOPE.code
                || failure.code() == CATALOG_UNAVAILABLE.code;
            assert!(
                declared || mapped_to_scope,
                "{} → {}",
                cause.code(),
                failure.code()
            );
        }
    }

    /// Contour intervals travel from the dataset document the room was opened
    /// with — never from a default of this host's own.
    #[test]
    fn contour_parameters_are_the_datasets_own() {
        let dataset = ds_project_data::contours_dataset(ds_project_data::ContourSettings {
            minor_interval_m: 10,
            index_interval_m: 50,
            sample_spacing_m: 10.0,
        });
        let parameters = contour_parameters(&dataset).expect("authored intervals");
        assert_eq!(parameters.minor_interval_m, 10);
        assert_eq!(parameters.index_interval_m, 50);
        assert_eq!(parameters.sample_spacing_m, 10.0);
        assert!(contour_parameters(&ds_project_data::buildings_dataset()).is_none());
    }

    /// A seed that could not finish every dataset renders as partial.
    #[test]
    fn a_partial_seed_never_renders_as_a_complete_one() {
        let data = serde_json::json!({
            "project": "p",
            "failed": 1,
            "complete": false,
            "datasets": [
                {"dataset_id":"google_open_buildings","clusters":1,"acquired":1,"feature_count":12,
                 "local_holding":true,"warnings":[]},
                {"dataset_id":"id-roads","clusters":0,"acquired":0,"feature_count":0,
                 "local_holding":false,"warnings":[],
                 "error":"This resource is not installed on this computer."},
            ],
        });
        let rendered = render_seed(&data);
        assert!(rendered.contains("held on this computer"), "{rendered}");
        assert!(
            rendered.contains("not acquired: This resource is not installed on this computer."),
            "{rendered}",
        );
        assert!(
            rendered.contains("1 dataset(s) did not complete"),
            "{rendered}"
        );
        assert!(rendered.contains("google_open_buildings"), "{rendered}");
    }

    #[test]
    fn status_render_separates_requested_from_completed() {
        let rendered = render_status(&json!({
            "project": "p",
            "seeded": 1,
            "catalog": {"read": true},
            "datasets": [{
                "dataset_id": "google_open_buildings",
                "seeded": true,
                "feature_count": 12,
                "index_state": "ready",
                "requested": {"cells": [[0,0,1,1],[2,2,3,3]]},
                "completed": {"cells": [[0,0,1,1]]},
                "source_version": "v1",
                "buffer_policy": {"design_buffer_m": 500.0, "isolated_buffer_m": 1000.0,
                    "isolated_rule": "point_only", "provisional": true},
                "last_error": "the provider was unreachable"
            }]
        }));
        assert!(rendered.contains("1 covered area(s) of 2 requested"));
        assert!(rendered.contains("(provisional)"));
        assert!(rendered.contains("last error: the provider was unreachable"));
    }

    #[test]
    fn status_render_names_obsolete_coverage_and_unread_catalogues() {
        let rendered = render_status(&json!({
            "project": "p",
            "seeded": 1,
            "catalog": {"read": false, "reason": "data_distribution_unavailable"},
            "datasets": [{
                "dataset_id": "google_open_buildings",
                "seeded": true,
                "feature_count": 12,
                "index_state": "ready",
                "requested": {"cells": [[0,0,1,1]]},
                "completed": {"cells": [[0,0,1,1]]},
                "source_version": "v1",
                "available_version": "v2",
                "stale": {"cells": [[0,0,1,1]]},
                "expired_queries": 2,
            }, {
                "dataset_id": "roads", "seeded": false, "feature_count": 0, "index_state": "absent",
                "requested": {"cells": []}, "completed": {"cells": []},
            }]
        }));
        assert!(rendered.contains("1 held area(s) completed under a superseded version"));
        assert!(rendered.contains("provider now publishes v2"));
        assert!(rendered.contains("2 abandoned acquisition(s) expired"));
        assert!(rendered.contains("catalogue not read (data_distribution_unavailable)"));
        assert!(rendered.contains("not seeded on this computer"));
    }
}
