//! Restart-safe local runs over captured, prepared inputs. The workspace owns
//! capture, locks and closed-batch verification; the caller supplies the
//! calculation, so this application crate never links a Solar engine.
use crate::{MAX_CITIES, Workspace, files, identifier, timestamp};
use anyhow::{Result, ensure};
use ds_solar_contracts::{
    PreparedSolarCityInput, SolarArtifactBatch, SolarCityInput, digest,
    network_form::ResolvedInformation,
};
use ds_solar_io::{cache::DirectoryReferenceCache, output::GeneratedDocument};
use ds_solar_reference_host::ReferencePreparer;
use fs2::FileExt;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub run_id: String,
    pub cities: Vec<String>,
    pub concurrency: usize,
    pub charts: bool,
    pub drafts: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct CapturedRun {
    request: RunRequest,
    inputs: Vec<SolarCityInput>,
    #[serde(default)]
    prepared_digests: BTreeMap<String, String>,
}

/// One explicit calculation of a captured run's prepared inputs. The calculator
/// commits one closed batch at `destination`, which does not exist yet; the
/// workspace then verifies that batch against the captured prepared inputs.
pub struct LocalCalculation<'a> {
    pub run_id: &'a str,
    pub project_id: &'a str,
    pub root: &'a str,
    pub prepared: &'a [PreparedSolarCityInput],
    /// Locally held input maps per city id, keyed by their immutable source URI.
    pub input_maps: BTreeMap<String, BTreeMap<String, GeneratedDocument>>,
    pub concurrency: usize,
    pub charts: bool,
    pub drafts: &'a [String],
    pub destination: &'a Path,
}

/// A host's calculation of one captured run; only engine-linking tooling has one.
pub type LocalCalculator<'a> = dyn Fn(LocalCalculation<'_>) -> Result<()> + 'a;

impl Workspace {
    /// A restarted command resumes the captured input, never today's mutable
    /// input. Holding only the job lock lets another process seed or synchronize.
    /// Shared assets are transient inputs supplied by the authorized host.
    /// Captured city records and the outbox retain references and overrides only.
    pub fn execute_with_network(
        &mut self,
        request: &RunRequest,
        reference_cache: &Path,
        network: &BTreeMap<String, ResolvedInformation>,
        calculate: impl FnOnce(LocalCalculation<'_>) -> Result<()>,
    ) -> Result<Value> {
        ensure!(identifier(&request.run_id), "invalid Solar run id");
        let _compute_lock = crate::clean::lock(&self.path, false)?;
        ensure!(
            !request.cities.is_empty() && request.cities.len() <= MAX_CITIES,
            "run requires an explicit city selection"
        );
        ensure!(
            (1..=32).contains(&request.concurrency),
            "concurrency must be 1..32"
        );
        ensure!(
            !request.drafts.is_empty()
                && request
                    .drafts
                    .iter()
                    .all(|s| matches!(s.as_str(), "apd" | "network" | "plant" | "financial")),
            "choose at least one supported draft"
        );
        let locks = self.path.join("jobs");
        if !locks.exists() {
            match files::private_dir(&locks) {
                Ok(()) => (),
                Err(e) if locks.is_dir() => {
                    let _ = e;
                }
                Err(e) => return Err(e),
            }
        }
        files::ordinary_dir(&locks)?;
        let lock_path = locks.join(format!("{}.lock", request.run_id));
        if fs::symlink_metadata(&lock_path).is_ok() {
            files::ordinary_file(&lock_path)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path)?;
        lock.try_lock_exclusive()
            .map_err(|_| anyhow::anyhow!("Solar run is already executing"))?;
        let prior: Option<String> = self
            .db
            .query_row(
                "SELECT request FROM runs WHERE id=?1",
                [&request.run_id],
                |r| r.get(0),
            )
            .optional()?;
        let mut captured = if let Some(prior) = prior {
            let captured: CapturedRun = serde_json::from_str(&prior)?;
            ensure!(
                digest::canonical_json_digest(&captured.request)?
                    == digest::canonical_json_digest(request)?,
                "run id already names a different request"
            );
            captured
        } else {
            let inputs = self.cities(&request.cities)?;
            let captured = CapturedRun {
                request: request.clone(),
                inputs,
                prepared_digests: Default::default(),
            };
            self.db.execute(
                "INSERT INTO runs(id,status,request) VALUES(?1,'running',?2)",
                params![request.run_id, serde_json::to_string(&captured)?],
            )?;
            captured
        };
        let destination = self.path.join("runs").join(&request.run_id);
        if destination.exists() {
            return self.finish_run(request, &destination);
        }
        self.db.execute(
            "UPDATE runs SET status='running',error=NULL WHERE id=?1",
            [&request.run_id],
        )?;
        let result = (|| {
            let cache = DirectoryReferenceCache::new(reference_cache);
            let preparer = ReferencePreparer::new().with_cache(&cache);
            let mut prepared = Vec::new();
            for input in &captured.inputs {
                let resolved = ds_solar_contracts::network_form::prepare_input(
                    input.clone(),
                    network.get(&input.identity.city_id),
                )?;
                prepared.push(preparer.prepare(resolved, timestamp())?.input);
            }
            captured.prepared_digests = prepared
                .iter()
                .map(|p| (p.city.identity.city_id.clone(), p.input_digest.clone()))
                .collect();
            self.db.execute(
                "UPDATE runs SET request=?1 WHERE id=?2",
                params![serde_json::to_string(&captured)?, request.run_id],
            )?;
            let mut input_maps = BTreeMap::new();
            for input in &prepared {
                input_maps.insert(
                    input.city.identity.city_id.clone(),
                    self.input_maps(&input.city)?,
                );
            }
            calculate(LocalCalculation {
                run_id: &request.run_id,
                project_id: &self.identity.project_id,
                root: &self.identity.root,
                prepared: &prepared,
                input_maps,
                concurrency: request.concurrency,
                charts: request.charts,
                drafts: &request.drafts,
                destination: &destination,
            })?;
            self.finish_run(request, &destination)
        })();
        if result.is_err() {
            self.db.execute(
                "UPDATE runs SET status='interrupted',error='local_run_failed' WHERE id=?1",
                [&request.run_id],
            )?;
        }
        result
    }

    fn finish_run(&mut self, request: &RunRequest, directory: &Path) -> Result<Value> {
        let batch: SolarArtifactBatch = serde_json::from_slice(&files::read(
            &directory.join("batch.json"),
            8 * 1024 * 1024,
        )?)?;
        files::verify(directory, &batch)?;
        ensure!(
            batch.run.project_id == self.identity.project_id
                && batch.run.root == self.identity.root
                && batch.run.run_id == request.run_id,
            "closed run identity mismatch"
        );
        let raw: String = self.db.query_row(
            "SELECT request FROM runs WHERE id=?1",
            [&request.run_id],
            |r| r.get(0),
        )?;
        let captured: CapturedRun = serde_json::from_str(&raw)?;
        for city in &batch.run.cities {
            ensure!(
                captured.prepared_digests.get(&city.city_id) == Some(&city.input_digest),
                "closed run is not the captured prepared input"
            );
        }
        let selected: std::collections::BTreeSet<_> = request.cities.iter().cloned().collect();
        let completed: std::collections::BTreeSet<_> =
            batch.run.cities.iter().map(|c| c.city_id.clone()).collect();
        let status = if completed.is_empty() {
            "failed"
        } else if completed == selected {
            "succeeded"
        } else {
            "partial"
        };
        ensure!(
            completed.is_subset(&selected),
            "closed run includes an unselected city"
        );
        let failures_path = directory.join("failures.json");
        let failures: Value = if failures_path.exists() {
            serde_json::from_slice(&files::read(&failures_path, 8 * 1024 * 1024)?)?
        } else {
            json!([])
        };
        let receipt = json!({"run_id":request.run_id,"status":status,"batch_id":batch.batch_id,"batch_digest":batch.batch_digest,"path":directory,"cities":batch.run.cities,"artifacts":batch.artifacts,"failures":failures,"publication":if completed.is_empty() {"not_applicable"} else {"local_only"}});
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE runs SET status=?1,receipt=?2,error=NULL WHERE id=?3",
            params![status, serde_json::to_string(&receipt)?, request.run_id],
        )?;
        tx.commit()?;
        self.run_result(&request.run_id)
    }

    pub fn run_result(&self, run_id: &str) -> Result<Value> {
        ensure!(identifier(run_id), "invalid Solar run id");
        let raw: Option<String> = self
            .db
            .query_row("SELECT receipt FROM runs WHERE id=?1", [run_id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        let raw = raw.ok_or_else(|| anyhow::anyhow!("Solar run has no committed result"))?;
        let batch: SolarArtifactBatch = serde_json::from_slice(&files::read(
            &self.path.join("runs").join(run_id).join("batch.json"),
            8 * 1024 * 1024,
        )?)?;
        files::verify(&self.path.join("runs").join(run_id), &batch)?;
        let mut receipt: Value = serde_json::from_str(&raw)?;
        ensure!(
            receipt["batch_digest"] == batch.batch_digest
                && batch.run.project_id == self.identity.project_id
                && batch.run.root == self.identity.root
                && batch.run.run_id == run_id,
            "stored receipt and batch disagree"
        );
        receipt["publication"] = json!(if batch.artifacts.is_empty() {
            "not_applicable"
        } else {
            "local_only"
        });
        Ok(receipt)
    }
}
