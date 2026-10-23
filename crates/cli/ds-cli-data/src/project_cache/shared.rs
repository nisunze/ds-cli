//! The doors a shared project dataset seed borrows on this machine.
//!
//! * The project's revision head travels on the governed compute artifact
//!   door through the native Sync Center session (one JWT, project-fenced);
//!   bytes move on Brain-minted storage sessions and generation-pinned
//!   signed reads. Brain authorizes and stores; it computes nothing.
//! * Google Open Buildings tiles are fetched here, from the publisher's own
//!   public objects, pinned by generation and verified by size and CRC32C.
//! * The Rwanda DEM is sampled through the governed elevation reader.
//! * The Reporter engine converts footprints and cuts contours by fixed
//!   request/response tasks with these local files as explicit inputs; it
//!   opens no socket.
//!
//! Which tiles, which version and which cells are the Kernel's decisions;
//! this file only performs the effects.

use super::*;
use ds_cli_exec::reporter::Task as ReporterTask;
use ds_command_kernel::project_dataset_revisions::{
    self as chain, google_open_buildings as google, tasks,
};
use ds_project_data::revisions::{
    self as shared, Head, Output, Publication, Publish, Published, SharedFailure, SourceBatch,
};
use ds_sync_runtime::{Gateway, SyncRoute};
use std::io::{Read, Write};
use std::time::Duration;

const ENGINE_TIMEOUT: Duration = Duration::from_secs(45 * 60);
const PUBLIC_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// The largest publisher tile Core reads (the densest level-6 tiles are a
/// few hundred megabytes).
const MAX_TILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const STORAGE: &str = "https://storage.googleapis.com";
/// The published Rwanda surface Core samples contours from.
pub(super) const SURFACE_UNCOVERED: &str = "contour_surface_uncovered";

fn host(code: &'static str, message: impl Into<String>) -> SharedFailure {
    SharedFailure::Host {
        code,
        message: message.into(),
    }
}

fn unavailable(message: impl Into<String>) -> SharedFailure {
    SharedFailure::Project(ds_project_data::Failure::ProviderUnavailable(
        message.into(),
    ))
}

/// One gateway refusal as the seed reads it: an outage is retried by a
/// person, a verdict is reported, a malformed answer is the door's defect.
fn door_error(error: ds_sync_runtime::GatewayError) -> SharedFailure {
    match error.status {
        Some(401 | 403) => SharedFailure::Refused(format!(
            "the project's shared datasets refused this account: {}",
            error.detail
        )),
        Some(400 | 409 | 422) => SharedFailure::Invalid(format!(
            "the shared dataset door refused the request: {}",
            error.detail
        )),
        _ => unavailable(format!(
            "the shared dataset door did not answer: {}",
            error.detail
        )),
    }
}

/// The compute artifact door, through the native user's project session.
pub(super) struct NativeRevisions {
    session: ds_cli_auth::sync::NativeSyncSession,
    scratch: PathBuf,
    /// The converting engine's release: a revision names what produced it.
    engine: Option<(String, String)>,
}

impl NativeRevisions {
    pub(super) fn open(lane: &str, project: &str, scratch: &Path) -> Result<Self, Failure> {
        let principal = ds_cli_auth::headless_identity_for_named_project(lane)?;
        let binding = ds_cli_auth::runtime_credential_binding(lane)?;
        let session = ds_cli_auth::sync::NativeSyncSession::open(
            ds_cli_auth::Lane::parse(lane)?,
            principal,
            project.to_owned(),
            binding,
        )
        .map_err(|detail| {
            Failure::unavailable(PROVIDER_UNAVAILABLE.code, detail)
                .remedy(PROVIDER_UNAVAILABLE.remedy)
        })?;
        Ok(Self {
            session,
            scratch: scratch.to_path_buf(),
            engine: None,
        })
    }

    pub(super) fn set_engine(&mut self, version: String, manifest: String) {
        self.engine = Some((version, manifest));
    }
}

/// A head answer, admitted by the Kernel's inventory rules.
pub(super) fn published(data: &Value) -> Result<Published, SharedFailure> {
    let head = ds_command_kernel::compute_artifact_inventory::decode_head(
        &serde_json::to_vec(data).unwrap_or_default(),
    )
    .map_err(SharedFailure::Invalid)?;
    if head.engine != chain::ENGINE {
        return Err(SharedFailure::Invalid(
            "the door answered another engine's head".into(),
        ));
    }
    Ok(Published {
        head_revision: head.head_revision,
        outputs: head
            .outputs
            .iter()
            .map(|output| Output {
                output_id: output.output_id.clone(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
            })
            .collect(),
    })
}

/// The open declaration a revision publication sends.
pub(super) fn open_body(
    publication: &Publication,
    engine: &(String, String),
) -> Result<Value, SharedFailure> {
    let outputs: Vec<Value> = publication
        .outputs
        .iter()
        .map(|output| {
            json!({"output_id": output.output_id, "format": output.format,
                "content_type": output.content_type, "sha256": output.sha256,
                "size_bytes": output.size_bytes})
        })
        .collect();
    let body = json!({
        "action": "open",
        "project_id": publication.head.project,
        "client_run_id": publication.client_run_id,
        "engine": chain::ENGINE,
        "operation": publication.head.operation,
        "variant": publication.head.variant,
        "engine_version": engine.0,
        "engine_build_manifest_sha256": engine.1,
        "input_base_fingerprint": publication.base_fingerprint,
        "outputs": outputs,
    });
    ds_command_kernel::sync::declaration::validate(&publication.head.project, &body)
        .map_err(SharedFailure::Invalid)?;
    Ok(body)
}

/// What finalize answered: published, or a typed stale verdict.
pub(super) fn finalized(answer: &Value) -> Result<Publish, SharedFailure> {
    match answer["state"].as_str() {
        Some("published") => Ok(Publish::Published {
            head_revision: answer["head_revision"]
                .as_u64()
                .filter(|revision| *revision > 0)
                .ok_or_else(|| {
                    SharedFailure::Invalid("a publication answered no head revision".into())
                })?,
        }),
        Some("stored_stale") => Ok(Publish::Stale {
            reason: answer["conflict"]
                .as_str()
                .unwrap_or("another revision landed first")
                .to_owned(),
        }),
        _ => Err(SharedFailure::Invalid(
            "the publication did not finish in a known state".into(),
        )),
    }
}

impl shared::RevisionHost for NativeRevisions {
    fn read(&mut self, head: &Head) -> Result<Option<Published>, SharedFailure> {
        let body = json!({"action": "read", "project_id": head.project, "engine": chain::ENGINE,
            "operation": head.operation, "variant": head.variant});
        match Gateway::post(&self.session, SyncRoute::ComputeArtifacts, &body) {
            Ok(data) => published(&data).map(Some),
            Err(error) if error.status == Some(404) => Ok(None),
            Err(error) => Err(door_error(error)),
        }
    }

    fn download(&mut self, head: &Head, output: &Output, dest: &Path) -> Result<(), SharedFailure> {
        let ticket = Gateway::post(
                &self.session,
                SyncRoute::ComputeArtifacts,
                &json!({"action": "download", "project_id": head.project, "engine": chain::ENGINE,
                    "operation": head.operation, "variant": head.variant, "output_id": output.output_id}),
            )
            .map_err(door_error)?;
        let url = ticket["download_url"]
            .as_str()
            .ok_or_else(|| SharedFailure::Invalid("a download ticket has no location".into()))?;
        use ds_sync_runtime::host::Reads;
        ds_sync_runtime::reads::VerifiedReads::new(self.scratch.clone())
            .download_verified(url, dest, &output.sha256, output.size_bytes)
            .map_err(unavailable)
    }

    fn publish(&mut self, publication: &Publication) -> Result<Publish, SharedFailure> {
        let engine = self.engine.clone().ok_or_else(|| {
            host(
                "reporter_engine_missing",
                "a revision names the engine that produced it; install Reporter for this lane",
            )
        })?;
        let open = open_body(publication, &engine)?;
        let answer =
            Gateway::post(&self.session, SyncRoute::ComputeArtifacts, &open).map_err(door_error)?;
        let work_id = answer["work_id"]
            .as_str()
            .ok_or_else(|| SharedFailure::Invalid("the publication opened no work".into()))?
            .to_owned();
        for upload in answer["outputs"].as_array().into_iter().flatten() {
            let id = upload["output_id"].as_str().unwrap_or_default();
            let output = publication
                .outputs
                .iter()
                .find(|output| output.output_id == id)
                .ok_or_else(|| {
                    SharedFailure::Invalid("the door opened an undeclared output".into())
                })?;
            match upload["status"].as_str() {
                Some("already_exists") => continue,
                Some("upload") => {}
                _ => {
                    return Err(SharedFailure::Invalid(
                        "the door answered an unknown upload state".into(),
                    ));
                }
            }
            let session = upload["session_uri"]
                .as_str()
                .ok_or_else(|| SharedFailure::Invalid("an upload has no storage session".into()))?;
            let file = std::fs::File::open(&output.path)
                .map_err(|error| ds_project_data::Failure::Store(error.to_string()))?;
            let receipt = ds_sync_runtime::native_transfer::transfer_verified_output(
                id,
                session,
                output.size_bytes,
                &output.sha256,
                file,
            )
            .map_err(unavailable)?;
            if !receipt.committed() {
                return Err(unavailable(format!(
                    "output {id} was not committed by storage"
                )));
            }
        }
        let answer = Gateway::post(
            &self.session,
            SyncRoute::ComputeArtifacts,
            &json!({"action": "finalize", "project_id": publication.head.project,
                    "work_id": work_id, "client_run_id": publication.client_run_id}),
        )
        .map_err(door_error)?;
        finalized(&answer)
    }
}

/// Core's own acquisition for the two derived datasets.
pub(super) struct CoreSource<'a> {
    pub(super) lane: &'a str,
    engine: Option<ds_cli_exec::reporter::Reporter>,
    /// Publisher tiles downloaded by this run.
    pub(super) tiles_fetched: usize,
    pub(super) cells_derived: usize,
    surface: Option<ds_elevation_host::RwandaSampler>,
}

impl<'a> CoreSource<'a> {
    pub(super) fn new(lane: &'a str) -> Self {
        Self {
            lane,
            engine: None,
            tiles_fetched: 0,
            cells_derived: 0,
            surface: None,
        }
    }

    /// The Reporter, captured once through the Kernel's Reporter client and
    /// only when work needs it, and checked to register the offline tasks this
    /// seed calls.
    pub(super) fn engine(&mut self) -> Result<&ds_cli_exec::reporter::Reporter, SharedFailure> {
        if self.engine.is_none() {
            let captured = ds_cli_exec::reporter::Reporter::capture_for_lane(self.lane)
                .map_err(|failure| host("reporter_engine_missing", failure.message().to_owned()))?;
            for task in [
                ReporterTask::PreparePublisherFootprints,
                ReporterTask::PlanContourLattice,
                ReporterTask::DeriveContours,
            ] {
                let entry = captured.require(task).map_err(|failure| {
                    host(
                        "reporter_engine_incompatible",
                        format!("{}; update Reporter", failure.message()),
                    )
                })?;
                if entry["network"] != "none" {
                    return Err(host(
                        "reporter_engine_incompatible",
                        format!(
                            "the installed Reporter does not publish the offline {} task; update Reporter",
                            task.name()
                        ),
                    ));
                }
            }
            self.engine = Some(captured);
        }
        Ok(self.engine.as_ref().expect("captured above"))
    }

    /// The engine release a revision is published under.
    pub(super) fn engine_release(&mut self) -> Result<(String, String), SharedFailure> {
        let engine = self.engine()?;
        let document = engine.build_info().map_err(|failure| {
            host("reporter_engine_incompatible", failure.message().to_owned())
        })?;
        let identity = ds_command_kernel::report_export::engine_identity(&document)
            .map_err(|error| host("reporter_engine_incompatible", error))?;
        Ok((identity.engine_version, identity.build_manifest_sha256))
    }

    fn task<R: serde::Serialize, T: serde::de::DeserializeOwned>(
        &mut self,
        task: ReporterTask,
        request: &R,
        scratch: &Path,
        name: &str,
    ) -> Result<T, SharedFailure> {
        let subcommand = task.subcommand();
        let request_path = scratch.join(format!("{name}-request.json"));
        let result_path = scratch.join(format!("{name}-result.json"));
        std::fs::write(
            &request_path,
            serde_json::to_vec(request)
                .map_err(|error| ds_project_data::Failure::Store(error.to_string()))?,
        )
        .map_err(|error| ds_project_data::Failure::Store(error.to_string()))?;
        let completed = self
            .engine()?
            .run(task, &request_path, &result_path, ENGINE_TIMEOUT)
            .map_err(|failure| host("engine_refused", failure.message().to_owned()))?;
        let _ = std::fs::remove_file(&request_path);
        if !completed.succeeded() {
            let message = completed.stderr().trim().to_owned();
            let code = [
                "publisher_input_invalid",
                "publisher_input_malformed",
                "publisher_geometry_unsupported",
                "publisher_input_too_large",
                SURFACE_UNCOVERED,
            ]
            .into_iter()
            .find(|code| message.contains(code))
            .unwrap_or("engine_refused");
            return Err(host(code, format!("{subcommand}: {message}")));
        }
        let bytes = std::fs::read(&result_path)
            .map_err(|error| host("engine_response_invalid", error.to_string()))?;
        let _ = std::fs::remove_file(&result_path);
        serde_json::from_slice(&bytes)
            .map_err(|error| host("engine_response_invalid", error.to_string()))
    }

    fn footprints(
        &mut self,
        dataset: &Dataset,
        cells: &[[f64; 4]],
        scratch: &Path,
    ) -> Result<SourceBatch, SharedFailure> {
        let tokens = google::tiles(&policy::Coverage {
            cells: cells.to_vec(),
        })
        .map_err(|error| {
            SharedFailure::Project(ds_project_data::Failure::AcquisitionFailed(error))
        })?;
        // Capture the engine before any download: a missing Reporter costs
        // no publisher bytes.
        self.engine()?;
        let mut objects = Vec::new();
        let mut tiles = Vec::new();
        for token in &tokens {
            let object = google::object(token).map_err(|error| {
                SharedFailure::Project(ds_project_data::Failure::AcquisitionFailed(error))
            })?;
            let (meta, path) = fetch_public_tile(&object, token, scratch)?;
            self.tiles_fetched += 1;
            tiles.push(
                json!({"token": token, "object": object, "generation": meta.generation,
                "bytes": meta.size, "crc32c": meta.crc32c, "sha256": meta.sha256}),
            );
            objects.push(tasks::PublisherObject {
                object_id: token.clone(),
                path,
                sha256: meta.sha256,
                size_bytes: meta.size,
            });
        }
        let out_dir = scratch.join(format!("footprint-pages-{}", self.tiles_fetched));
        let request = tasks::FootprintsRequest {
            schema: tasks::FOOTPRINTS_REQUEST.into(),
            dataset_id: dataset.id.clone(),
            provider: dataset.provider.clone(),
            publisher: tasks::Publisher::GoogleOpenBuildingsV3,
            objects: objects.clone(),
            cells: cells.to_vec(),
            out_dir: out_dir.clone(),
        };
        let result: tasks::FootprintsResult = self.task(
            ReporterTask::PreparePublisherFootprints,
            &request,
            scratch,
            "footprints",
        )?;
        for object in &objects {
            let _ = std::fs::remove_file(&object.path);
        }
        if result.schema != tasks::FOOTPRINTS_RESULT
            || result.conversion != tasks::FOOTPRINT_CONVERSION
            || result.pages.len() != cells.len()
            || result
                .pages
                .iter()
                .zip(cells)
                .any(|(page, cell)| page.cell != *cell || !page.path.starts_with(&out_dir))
        {
            return Err(host(
                "engine_response_invalid",
                "the Reporter answered another footprint request",
            ));
        }
        Ok(SourceBatch {
            source_version: google::VERSION.into(),
            receipt: json!({"publisher": "google_open_buildings", "version": google::VERSION,
                "tiles": tiles, "conversion": result.conversion, "raw_rows": result.raw_rows,
                "network_requests_by_engine": 0}),
            pages: result
                .pages
                .into_iter()
                .map(|page| (page.cell, page.path))
                .collect(),
        })
    }

    fn surface(&mut self) -> Result<&mut ds_elevation_host::RwandaSampler, SharedFailure> {
        if self.surface.is_none() {
            self.surface = Some(ds_elevation_host::RwandaSampler::for_lane(self.lane).map_err(|error| {
                unavailable(format!("the Rwanda DEM could not be opened: {error}"))
            })?);
        }
        Ok(self.surface.as_mut().expect("opened above"))
    }

    fn contours(
        &mut self,
        dataset: &Dataset,
        cells: &[[f64; 4]],
        scratch: &Path,
    ) -> Result<SourceBatch, SharedFailure> {
        let settings = ds_project_data::datasets::contour_settings(dataset).ok_or_else(|| {
            SharedFailure::Invalid("the contours dataset names no intervals".into())
        })?;
        self.engine()?;
        let evidence = ds_elevation_host::RwandaSampler::source_evidence();
        let mut pages = Vec::with_capacity(cells.len());
        let mut sampled = 0_u64;
        for (index, cell) in cells.iter().enumerate() {
            let points_path = scratch.join(format!("contour-points-{index}.json"));
            let plan: tasks::ContourPlanResult = self.task(
                ReporterTask::PlanContourLattice,
                &tasks::ContourPlanRequest {
                    schema: tasks::CONTOUR_PLAN_REQUEST.into(),
                    cell: *cell,
                    sample_spacing_m: settings.sample_spacing_m,
                    out_points: points_path.clone(),
                },
                scratch,
                &format!("contour-plan-{index}"),
            )?;
            let points: Vec<[f64; 2]> = serde_json::from_slice(
                &std::fs::read(&points_path)
                    .map_err(|error| host("engine_response_invalid", error.to_string()))?,
            )
            .map_err(|error| host("engine_response_invalid", error.to_string()))?;
            if points.len() != plan.lattice.columns * plan.lattice.rows {
                return Err(host(
                    "engine_response_invalid",
                    "the lattice points do not match their plan",
                ));
            }
            let mut samples = Vec::with_capacity(points.len());
            for chunk in points.chunks(ds_elevation_host::RWANDA_BATCH_POINT_LIMIT) {
                samples.extend(self.surface()?.sample_wgs84(chunk).map_err(|error| {
                    unavailable(format!("the Rwanda DEM could not be read: {error}"))
                })?);
            }
            if samples.iter().any(Option::is_none) {
                return Err(host(
                    SURFACE_UNCOVERED,
                    "the Rwanda DEM does not cover this project area; no other surface is admitted for contours yet",
                ));
            }
            sampled += samples.len() as u64;
            let samples_path = scratch.join(format!("contour-samples-{index}.json"));
            std::fs::write(
                &samples_path,
                serde_json::to_vec(&samples).unwrap_or_default(),
            )
            .map_err(|error| ds_project_data::Failure::Store(error.to_string()))?;
            let out = scratch.join(format!("contour-page-{index}.geojsonl"));
            let result: tasks::ContoursResult = self.task(
                ReporterTask::DeriveContours,
                &tasks::ContoursRequest {
                    schema: tasks::CONTOURS_REQUEST.into(),
                    dataset_id: dataset.id.clone(),
                    provider: dataset.provider.clone(),
                    cell: *cell,
                    lattice: plan.lattice,
                    samples: samples_path.clone(),
                    minor_interval_m: settings.minor_interval_m,
                    index_interval_m: settings.index_interval_m,
                    surface: serde_json::to_value(&evidence).unwrap_or_default(),
                    out: out.clone(),
                },
                scratch,
                &format!("contours-{index}"),
            )?;
            let _ = std::fs::remove_file(&points_path);
            let _ = std::fs::remove_file(&samples_path);
            if result.schema != tasks::CONTOURS_RESULT
                || result.page.cell != *cell
                || result.page.path != out
            {
                return Err(host(
                    "engine_response_invalid",
                    "the Reporter answered another contour request",
                ));
            }
            self.cells_derived += 1;
            pages.push((*cell, out));
        }
        Ok(SourceBatch {
            source_version: surface_version(),
            receipt: json!({"surface": evidence, "version": surface_version(),
                "minor_interval_m": settings.minor_interval_m, "index_interval_m": settings.index_interval_m,
                "sample_spacing_m": settings.sample_spacing_m, "sampled_points": sampled,
                "contour_tile_m": tasks::CONTOUR_TILE_M}),
            pages,
        })
    }
}

/// The surface version contour revisions record.
pub(super) fn surface_version() -> String {
    let evidence = ds_elevation_host::RwandaSampler::source_evidence();
    format!("{}/{}", evidence.id, evidence.dataset)
}

impl shared::SourceHost for CoreSource<'_> {
    fn current_version(&self, dataset: &Dataset) -> Option<String> {
        match dataset.id.as_str() {
            policy::BUILDINGS_DATASET_ID => Some(google::VERSION.into()),
            policy::CONTOURS_DATASET_ID => Some(surface_version()),
            _ => None,
        }
    }

    fn acquire(
        &mut self,
        dataset: &Dataset,
        cells: &[[f64; 4]],
        scratch: &Path,
    ) -> Result<SourceBatch, SharedFailure> {
        match dataset.id.as_str() {
            policy::BUILDINGS_DATASET_ID => self.footprints(dataset, cells, scratch),
            policy::CONTOURS_DATASET_ID => self.contours(dataset, cells, scratch),
            other => Err(SharedFailure::Invalid(format!(
                "{other} has no Core acquisition owner"
            ))),
        }
    }
}

/// What the publisher's object metadata pins, and what the bytes proved.
pub(super) struct TileMeta {
    pub generation: String,
    pub size: u64,
    pub crc32c: String,
    pub sha256: String,
}

fn encode_path(object: &str) -> String {
    object
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn public_get(url: &str) -> Result<ureq::http::Response<ureq::Body>, SharedFailure> {
    let response = ureq::get(url)
        .header(
            "User-Agent",
            "DataSolutions-project-datasets/1.0 (datasolutions.rw)",
        )
        .config()
        .max_redirects(0)
        .https_only(true)
        .timeout_global(Some(PUBLIC_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|_| {
            unavailable("the building publisher could not be reached; held data is kept")
        })?;
    if !response.status().is_success() {
        return Err(unavailable(format!(
            "the building publisher answered {}",
            response.status()
        )));
    }
    Ok(response)
}

/// A base64 CRC32C as GCS publishes it, decoded.
pub(super) fn crc32c_of(text: &str) -> Option<u32> {
    let value = |byte: u8| -> Option<u32> {
        Some(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes = text.as_bytes();
    if bytes.len() != 8 || &bytes[6..] != b"==" {
        return None;
    }
    let mut bits = 0_u64;
    for byte in &bytes[..6] {
        bits = bits << 6 | u64::from(value(*byte)?);
    }
    // 36 bits carry 32 bits of value and 4 bits of padding.
    Some((bits >> 4) as u32)
}

/// Download one publisher tile, pinned to the generation its metadata names
/// and proven by its size and CRC32C before the engine reads it.
fn fetch_public_tile(
    object: &str,
    token: &str,
    scratch: &Path,
) -> Result<(TileMeta, PathBuf), SharedFailure> {
    let encoded = encode_path(object);
    let mut response = public_get(&format!(
        "{STORAGE}/storage/v1/b/{}/o/{encoded}?fields=name,size,crc32c,generation",
        google::BUCKET
    ))?;
    let mut text = String::new();
    response
        .body_mut()
        .as_reader()
        .take(64 * 1024)
        .read_to_string(&mut text)
        .map_err(|_| unavailable("the publisher's tile metadata was interrupted"))?;
    let meta: Value = serde_json::from_str(&text)
        .map_err(|_| SharedFailure::Invalid("the publisher's tile metadata is not JSON".into()))?;
    let size = meta["size"].as_str().and_then(|s| s.parse::<u64>().ok());
    let generation = meta["generation"]
        .as_str()
        .filter(|g| !g.is_empty() && g.len() <= 20 && g.bytes().all(|b| b.is_ascii_digit()));
    let crc = meta["crc32c"].as_str().and_then(crc32c_of);
    let (Some(size), Some(generation), Some(crc)) = (size, generation, crc) else {
        return Err(SharedFailure::Invalid(
            "the publisher's tile metadata names no size, generation or CRC32C".into(),
        ));
    };
    if meta["name"] != object || size == 0 || size > MAX_TILE_BYTES {
        return Err(SharedFailure::Invalid(format!(
            "the publisher answered another object or an unbounded tile for {token}"
        )));
    }
    let path = scratch.join(format!("{token}-{generation}.csv.gz"));
    let mut response = public_get(&format!(
        "{STORAGE}/download/storage/v1/b/{}/o/{encoded}?generation={generation}&alt=media",
        google::BUCKET
    ))?;
    let mut file = std::fs::File::create(&path)
        .map_err(|error| ds_project_data::Failure::Store(error.to_string()))?;
    let mut reader = response.body_mut().as_reader().take(size + 1);
    let mut buffer = vec![0_u8; 1 << 16];
    let (mut bytes, mut crc32c, mut sha) = (0_u64, 0_u32, <sha2::Sha256 as sha2::Digest>::new());
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| unavailable(format!("the download of tile {token} was interrupted")))?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        crc32c = crc32c::crc32c_append(crc32c, &buffer[..read]);
        sha2::Digest::update(&mut sha, &buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|error| ds_project_data::Failure::Store(error.to_string()))?;
    }
    file.sync_all()
        .map_err(|error| ds_project_data::Failure::Store(error.to_string()))?;
    if bytes != size || crc32c != crc {
        let _ = std::fs::remove_file(&path);
        return Err(SharedFailure::Invalid(format!(
            "tile {token} generation {generation} is not the publisher's bytes ({bytes} of {size} bytes, CRC32C mismatch: {})",
            crc32c != crc
        )));
    }
    Ok((
        TileMeta {
            generation: generation.into(),
            size,
            crc32c: meta["crc32c"].as_str().unwrap_or_default().into(),
            sha256: format!("{:x}", sha2::Digest::finalize(sha)),
        },
        path,
    ))
}

/// One run's doors for shared datasets, opened on first use.
pub(super) struct SharedSeeding<'a> {
    revisions: NativeRevisions,
    source: CoreSource<'a>,
    scratch: tempfile::TempDir,
    run_id: String,
}

fn open_seeding<'a>(
    root: &Path,
    lane: &'a str,
    project: &str,
) -> Result<SharedSeeding<'a>, Failure> {
    let base = root.join("project-dataset-seed");
    ds_layer_store::private::create_dir_all(&base).map_err(|error| {
        Failure::unavailable(STORE_FAILED.code, error.to_string()).remedy(STORE_FAILED.remedy)
    })?;
    let scratch = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(&base)
        .map_err(|error| {
            Failure::unavailable(STORE_FAILED.code, error.to_string()).remedy(STORE_FAILED.remedy)
        })?;
    let mut revisions = NativeRevisions::open(lane, project, scratch.path())?;
    let mut source = CoreSource::new(lane);
    // A revision names the engine release that produced it. Without an
    // installed Reporter a seed can still replay what others published; it
    // refuses only when it must convert or publish.
    if let Ok((version, manifest)) = source.engine_release() {
        revisions.set_engine(version, manifest);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    Ok(SharedSeeding {
        revisions,
        source,
        scratch,
        run_id: format!("seed-{}-{stamp}", std::process::id()),
    })
}

/// The seed row for one shared dataset: its outcome, or its own error.
/// One dataset failing never ends the run.
#[allow(clippy::too_many_arguments)]
pub(super) fn shared_seed_row<'a>(
    seeding: &mut Option<SharedSeeding<'a>>,
    lane: &'a str,
    root: &Path,
    scope: &Scope,
    entry: &ds_project_data::Declared,
    policy: &policy::BufferPolicy,
    extents: &[policy::Extent],
    refresh: bool,
    held: bool,
) -> Value {
    let failed_row = |code: &str, message: &str| {
        json!({
            "dataset_id": entry.dataset.id,
            "label": entry.candidate.label,
            "clusters": 0, "acquired": 0, "feature_count": 0,
            "local_holding": held,
            "warnings": [],
            "code": code,
            "error": format!("{code}: {message}"),
        })
    };
    if seeding.is_none() {
        match open_seeding(root, lane, &scope.project) {
            Ok(opened) => *seeding = Some(opened),
            Err(failure) => return failed_row(failure.code(), failure.message()),
        }
    }
    let state = seeding.as_mut().expect("opened above");
    let (tiles_before, cells_before) = (state.source.tiles_fetched, state.source.cells_derived);
    let mut shared_seed = ds_project_data::revisions::SharedSeed {
        revisions: &mut state.revisions,
        source: &mut state.source,
        scratch: state.scratch.path(),
        run_id: &state.run_id,
    };
    let settled = ds_project_data::revisions::seed(
        root,
        scope,
        &entry.dataset,
        &entry.source,
        policy,
        extents,
        &[],
        refresh,
        &mut shared_seed,
    );
    match settled {
        Ok(outcome) => {
            let mut row = json!({
                "dataset_id": entry.dataset.id,
                "label": entry.candidate.label,
                "dataset_key": outcome.ensure.dataset_key,
                "clusters": outcome.ensure.clusters,
                "acquired": outcome.ensure.acquired,
                "skipped": outcome.ensure.skipped,
                "covered": outcome.ensure.covered,
                "feature_count": outcome.ensure.features,
                "local_holding": true,
                "shared": {
                    "head_revision": outcome.head_revision,
                    "replayed_revisions": outcome.replayed,
                    "source_cells": outcome.source_cells,
                    "publisher_tiles_fetched": state.source.tiles_fetched - tiles_before,
                    "surface_cells_derived": state.source.cells_derived - cells_before,
                    "published": outcome.published,
                    "stale_retries": outcome.stale_retries,
                    "chain": outcome.chain,
                    "freshness": outcome.freshness,
                },
                "coverage": outcome.ensure.coverage,
                "warnings": outcome.ensure.warnings,
            });
            if !outcome.ensure.covered {
                row["error"] = json!(
                    "coverage is not complete; another acquisition may still be preparing this area"
                );
            }
            row
        }
        Err(error) => failed_row(error.code(), error.message()),
    }
}

/// The Reporter conversion a `--publisher-input` seed lends the producer:
/// the same offline task, the same typed refusals.
pub(super) fn reporter_converter(
    lane: &str,
) -> impl FnMut(
    &tasks::FootprintsRequest,
) -> Result<tasks::FootprintsResult, ds_project_data::publisher_footprints::Error>
+ '_ {
    use ds_project_data::publisher_footprints::Error;
    let mut source = CoreSource::new(lane);
    move |request| {
        let scratch = request
            .out_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| Error::Store("the conversion has no staging directory".into()))?;
        source
            .task(
                ReporterTask::PreparePublisherFootprints,
                request,
                &scratch,
                "publisher-input",
            )
            .map_err(|failure| {
                let message = failure.message().to_owned();
                match failure.code() {
                    "publisher_input_invalid" => Error::Invalid(message),
                    "publisher_input_malformed" => Error::Malformed(message),
                    "publisher_geometry_unsupported" => Error::Unsupported(message),
                    "publisher_input_too_large" => Error::TooLarge(message),
                    code => Error::Engine(code, message),
                }
            })
    }
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
