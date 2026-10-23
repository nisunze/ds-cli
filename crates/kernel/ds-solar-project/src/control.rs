//! Closed local project controls shared by native CLI and the legacy file adapter.
//! This boundary carries no credential, HTTP client or implicit reference acquisition,
//! and links no Solar engine: a local run's calculation is an explicit host input.
use crate::{
    ProjectIdentity, Workspace, files,
    run::{LocalCalculator, RunRequest},
};
use anyhow::{Result, ensure};
use ds_solar_contracts::SolarCityInput;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Clean {
        command: crate::clean::Control,
    },
    PortfolioDashboardSource {
        source: PathBuf,
        project: String,
        run: String,
        portfolio: String,
        membership_revision: String,
    },
    DashboardSource {
        source: PathBuf,
        project: String,
        run: String,
        city: String,
    },
    PortfolioCalculate {
        request: crate::portfolio::Request,
    },
    CityCreate {
        workspace: PathBuf,
        city: String,
        display_name: String,
        input: Option<PathBuf>,
    },
    NetworkFormRead {
        workspace: PathBuf,
        city: String,
    },
    NetworkFormWrite {
        workspace: PathBuf,
        city: String,
        expected: String,
        document: Value,
        #[serde(default)]
        information: Option<ds_solar_contracts::network_form::ResolvedInformation>,
    },
    NetworkMapSave {
        workspace: PathBuf,
        city: String,
        expected: String,
        reference: Value,
        #[serde(default)]
        file: Option<PathBuf>,
    },
    NetworkMediaCache {
        workspace: PathBuf,
        reference: Value,
        base64: String,
    },
    NetworkSources {
        workspace: PathBuf,
        cities: Vec<String>,
    },
    ReferencePlan {
        workspace: PathBuf,
        city: String,
        cache: PathBuf,
    },
    ReferenceImport {
        workspace: PathBuf,
        city: String,
        cache: PathBuf,
        expected: String,
        bundle: Value,
    },
    ReferencePlanFile {
        workspace: PathBuf,
        project: String,
        city: String,
        cache: PathBuf,
        out: PathBuf,
    },
    ReferenceImportFiles {
        workspace: PathBuf,
        project: String,
        city: String,
        cache: PathBuf,
        expected: String,
        bundle: PathBuf,
    },
    Initialize {
        workspace: PathBuf,
        identity: ProjectIdentity,
    },
    Seed {
        workspace: PathBuf,
        inputs: Vec<PathBuf>,
        #[serde(default)]
        expected: BTreeMap<String, String>,
    },
    CityRead {
        workspace: PathBuf,
        city: String,
        out: PathBuf,
    },
    CityWrite {
        workspace: PathBuf,
        city: String,
        snapshot: PathBuf,
        expected: Option<String>,
    },
    Status {
        workspace: PathBuf,
    },
    Run {
        workspace: PathBuf,
        cache: PathBuf,
        request: RunRequest,
        #[serde(default)]
        network: BTreeMap<String, ds_solar_contracts::network_form::ResolvedInformation>,
    },
    Result {
        workspace: PathBuf,
        run_id: String,
    },
    SyncBind {
        workspace: PathBuf,
        project: String,
        lane: String,
        principal: String,
        audience: String,
    },
    SyncRebase {
        workspace: PathBuf,
        sequence: i64,
        expected_cloud: String,
    },
    SyncNext {
        workspace: PathBuf,
        #[serde(default)]
        inputs_only: bool,
        #[serde(default)]
        run_id: Option<String>,
    },
    SyncAck {
        workspace: PathBuf,
        sequence: i64,
        digest: String,
        receipt: Value,
        #[serde(default)]
        inputs_only: bool,
        #[serde(default)]
        run_id: Option<String>,
    },
    SyncFailed {
        workspace: PathBuf,
        sequence: i64,
        digest: String,
        code: String,
        #[serde(default)]
        inputs_only: bool,
        #[serde(default)]
        run_id: Option<String>,
    },
    Outbox {
        workspace: PathBuf,
        #[serde(default = "page_limit")]
        limit: usize,
    },
}
fn page_limit() -> usize {
    20
}

const NO_ENGINE: &str = "Local project calculation is available through solar.application; the native host does not link the Solar engine";

/// Decode the owner's closed request before any local filesystem effect.
/// Calculation requests refuse before any effect: native hosts calculate
/// through `solar.application` and the independently released Solar addon.
pub fn execute(request: Value) -> Result<Value> {
    execute_with(request, None)
}

/// [`execute`] with a host-supplied local run calculation. Only local tooling
/// that links an engine supplies one. Portfolio aggregation is never local.
pub fn execute_with(request: Value, calculate: Option<&LocalCalculator<'_>>) -> Result<Value> {
    let bytes = serde_json::to_vec(&request)?;
    ensure!(
        bytes.len() <= 32 * 1024 * 1024,
        "Solar project request exceeds 32 MiB"
    );
    let request: Request = serde_json::from_slice(&bytes)?;
    let result = match request {
        Request::Clean { command } => crate::clean::execute(command)?,
        Request::PortfolioDashboardSource {
            source,
            project,
            run,
            portfolio,
            membership_revision,
        } => crate::dashboard::portfolio_source(
            &source,
            &project,
            &run,
            &portfolio,
            &membership_revision,
        )?,
        Request::DashboardSource {
            source,
            project,
            run,
            city,
        } => crate::dashboard::source(&source, &project, &run, &city)?,
        Request::PortfolioCalculate { request } => {
            let _ = request;
            anyhow::bail!(NO_ENGINE)
        }
        Request::CityCreate {
            workspace,
            city,
            display_name,
            input,
        } => {
            let snapshot = input
                .map(|path| -> Result<Value> {
                    Ok(serde_json::from_slice(&files::read(
                        &path,
                        16 * 1024 * 1024,
                    )?)?)
                })
                .transpose()?;
            Workspace::open(&workspace)?.create_city(&city, &display_name, snapshot)?
        }
        Request::NetworkFormRead { workspace, city } => {
            Workspace::open(&workspace)?.network_form(&city)?
        }
        Request::NetworkFormWrite {
            workspace,
            city,
            expected,
            document,
            information,
        } => Workspace::open(&workspace)?.save_network_form(
            &city,
            &expected,
            document,
            information.as_ref(),
        )?,
        Request::NetworkMapSave {
            workspace,
            city,
            expected,
            reference,
            file,
        } => Workspace::open(&workspace)?.save_network_map(
            &city,
            &expected,
            reference,
            file.as_deref(),
        )?,
        Request::NetworkMediaCache {
            workspace,
            reference,
            base64,
        } => {
            Workspace::open(&workspace)?.cache_network_media_encoded(&reference, &base64)?;
            json!({"cached":true})
        }
        Request::NetworkSources { workspace, cities } => {
            Workspace::open(&workspace)?.network_sources(&cities)?
        }
        Request::ReferencePlan {
            workspace,
            city,
            cache,
        } => crate::reference::plan(&workspace, &city, &cache)?,
        Request::ReferenceImport {
            workspace,
            city,
            cache,
            expected,
            bundle,
        } => crate::reference::import(&workspace, &city, &cache, &expected, &bundle)?,
        Request::ReferencePlanFile {
            workspace,
            project,
            city,
            cache,
            out,
        } => crate::reference::plan_file(&workspace, &project, &city, &cache, &out)?,
        Request::ReferenceImportFiles {
            workspace,
            project,
            city,
            cache,
            expected,
            bundle,
        } => {
            crate::reference::import_files(&workspace, &project, &city, &cache, &expected, &bundle)?
        }
        Request::Initialize {
            workspace,
            identity,
        } => Workspace::initialize(&workspace, identity)?.status()?,
        Request::CityRead {
            workspace,
            city,
            out,
        } => {
            let receipt = Workspace::open(&workspace)?.city_snapshot(&city)?;
            files::json(&out, &receipt["snapshot"])?;
            json!({"city_id":city,"content_digest":receipt["content_digest"],"snapshot":out})
        }
        Request::CityWrite {
            workspace,
            city,
            snapshot,
            expected,
        } => {
            let mut ws = Workspace::open(&workspace)?;
            let snapshot: Value =
                serde_json::from_slice(&files::read(&snapshot, 16 * 1024 * 1024)?)?;
            ws.write_city_snapshot(&city, snapshot, expected.as_deref())?
        }

        Request::Status { workspace } => Workspace::open(&workspace)?.status()?,
        Request::Seed {
            workspace,
            inputs,
            expected,
        } => {
            ensure!(
                !inputs.is_empty() && inputs.len() <= 64,
                "seed requires 1..64 input files"
            );
            let cities = inputs
                .iter()
                .map(|path| -> Result<SolarCityInput> {
                    let raw = files::read(path, 32 * 1024 * 1024)?;
                    let value: Value = serde_json::from_slice(&raw)?;
                    // The existing governed intake is an approved export. Its
                    // receipt is not reused as permission for an offline edit.
                    let city = if value["schema_version"] == "ds-solar.governed-city-intake/v1" {
                        value["city_input"].clone()
                    } else {
                        value
                    };
                    Ok(serde_json::from_value(city)?)
                })
                .collect::<Result<Vec<_>>>()?;
            Workspace::open(&workspace)?.seed(&cities, &expected)?
        }
        Request::Run {
            workspace,
            cache,
            request,
            network,
        } => {
            let Some(calculate) = calculate else {
                anyhow::bail!(NO_ENGINE)
            };
            Workspace::open(&workspace)?
                .execute_with_network(&request, &cache, &network, calculate)?
        }
        Request::Result { workspace, run_id } => {
            Workspace::open(&workspace)?.run_result(&run_id)?
        }
        Request::SyncBind {
            workspace,
            project,
            lane,
            principal,
            audience,
        } => {
            Workspace::open(&workspace)?.bind_sender(&project, &lane, &principal, &audience)?;
            json!({"bound":true})
        }
        Request::SyncRebase {
            workspace,
            sequence,
            expected_cloud,
        } => {
            Workspace::open(&workspace)?.rebase_pending_city(sequence, &expected_cloud)?;
            json!({"sequence":sequence,"expected_cloud":expected_cloud,"publication":"pending"})
        }
        Request::SyncNext {
            workspace,
            inputs_only,
            run_id,
        } => Workspace::open(&workspace)?.sync_next_selected(inputs_only, run_id.as_deref())?,
        Request::SyncAck {
            workspace,
            sequence,
            digest,
            receipt,
            inputs_only,
            run_id,
        } => {
            Workspace::open(&workspace)?.sync_ack_selected(
                sequence,
                &digest,
                &receipt,
                inputs_only,
                run_id.as_deref(),
            )?;
            json!({"acknowledged":sequence})
        }
        Request::SyncFailed {
            workspace,
            sequence,
            digest,
            code,
            inputs_only,
            run_id,
        } => {
            let ws = Workspace::open(&workspace)?;
            let row = ws
                .pending_selected(1, inputs_only, run_id.as_deref())?
                .pop()
                .ok_or_else(|| anyhow::anyhow!("no pending upload"))?;
            ensure!(
                row.sequence == sequence && row.digest == digest,
                "pending upload moved"
            );
            ws.failed(&row, &code)?;
            json!({"pending":sequence})
        }
        Request::Outbox { workspace, limit } => {
            let rows = Workspace::open(&workspace)?.pending(limit)?;
            json!({"pending":rows.into_iter().map(|r|json!({"sequence":r.sequence,"kind":r.kind,"identity":r.identity,"digest":r.digest,"attempts":r.attempts,"error":r.error})).collect::<Vec<_>>()})
        }
    };
    ensure!(
        serde_json::to_vec(&result)?.len() <= 32 * 1024 * 1024,
        "Solar project result exceeds 32 MiB"
    );
    Ok(result)
}
