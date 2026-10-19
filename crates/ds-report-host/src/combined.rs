//! The native host of one local Combined Report run.
//!
//! The decisions are the kernel's (`report_export::combined`); this module is
//! the IO between them and a filesystem. A host hands it saved rooms one at a
//! time, so a project is never held in memory: each room is projected onto
//! the report layers, written to private staging and dropped. The run then
//! asks the engine adapter for the read-only media scope when the host needs
//! it, runs the one combined task, proves the bytes the engine declared are
//! the bytes on disk and places the artifacts under the operator's output
//! root beside their receipt. Staging is removed on every path.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ds_command_kernel::report_export::combined::{
    self as decisions, CombinedRefusal, FileReceipt, MediaScope, RoomPin, Scope, Staged,
    VerifiedCombined,
};
use ds_command_kernel::report_export::{blocker_summary, InputReceipt, RUN_RECEIPT_FILE};
use serde_json::{json, Value};

use super::{
    create_private_dir, create_staging_root, engine_identity_now, now_ms, place_artifact,
    promote_report, random_run_id, read_bounded, staging_failed, verify_staged_artifacts,
    write_new_private, AdminBoundsAsset, HostFailure, ReportEngine, RunOutcome, Staging,
    CODE_ADMIN_BOUNDS_UNAVAILABLE, CODE_ENGINE_IDENTITY_CHANGED, CODE_ENGINE_REFUSED,
    CODE_EXPORT_BLOCKED, CODE_INPUTS_INVALID, CODE_OUTPUT_EXISTS, CODE_RESULT_INVALID,
};

const CONFIG_FILE: &str = "config.json";
const ROOMS_DIRECTORY: &str = "rooms";
const OUT_DIRECTORY: &str = "out";
const MEDIA_GRANT_FILE: &str = "media-grant.json";
const MEDIA_REQUEST_FILE: &str = "media-request.json";
const MEDIA_RESULT_FILE: &str = "media-result.json";
const REQUEST_FILE: &str = "request.json";
const RESULT_FILE: &str = "result.json";

fn invalid(message: impl Into<String>) -> HostFailure {
    HostFailure::new(CODE_INPUTS_INVALID, message)
}

/// What one combined run shares with its host.
#[derive(Debug, Clone, Copy)]
pub struct CombinedSettings<'a> {
    pub project_id: &'a str,
    pub receipt: &'a InputReceipt,
    pub admin_bounds: Option<&'a AdminBoundsAsset>,
    /// Private scratch; the run creates one directory below it.
    pub staging_root: &'a Path,
    /// The operator's output root; the run places `<out_root>/combined_transformer/`.
    pub out_root: &'a Path,
}

/// What a run carries beyond its rooms. The default carries none.
#[derive(Debug, Clone, Copy, Default)]
pub struct CombinedExtras<'a> {
    /// One media grant document in the engine's `{grants:[…]}` schema.
    pub media_grant: Option<&'a [u8]>,
    /// The governed transformer natures the host's authority resolved.
    pub transformer_natures: Option<&'a BTreeMap<String, String>>,
}

/// A completed, verified and placed combined run. `run` carries the members
/// a publication seal reads, exactly as an individual run does.
#[derive(Debug, Clone)]
pub struct CombinedOutcome {
    pub run: RunOutcome,
    pub pins: Vec<RoomPin>,
    pub result: VerifiedCombined,
    pub input_base_fingerprint: String,
    pub room_content_sha256: String,
}

/// One run's private staging, filled one saved room at a time.
pub struct CombinedStaging {
    scope: Scope,
    staging: Staging,
    out_root: PathBuf,
    config: FileReceipt,
    admin_bounds: Option<(PathBuf, FileReceipt)>,
    rooms: Vec<(PathBuf, RoomPin)>,
    empty: Vec<String>,
    staged_bytes: u64,
    client_run_id: String,
}

impl CombinedStaging {
    /// Plan the run from the project's receipt and prepare its staging.
    pub fn begin(settings: &CombinedSettings<'_>) -> Result<Self, HostFailure> {
        let scope = decisions::scope(settings.project_id, settings.receipt).map_err(invalid)?;
        let admin_bounds = match (scope.requires_admin_bounds, settings.admin_bounds) {
            (true, None) => return Err(HostFailure::new(
                CODE_ADMIN_BOUNDS_UNAVAILABLE,
                "a Rwanda combined report requires the installed administrative reference asset",
            )),
            (false, Some(_)) => {
                return Err(invalid(
                    "only a Rwanda combined report stages the administrative reference",
                ))
            }
            (_, asset) => asset.map(|asset| {
                (
                    asset.path.clone(),
                    FileReceipt {
                        bytes: asset.size_bytes,
                        sha256: asset.sha256.clone(),
                    },
                )
            }),
        };
        let target = settings.out_root.join(decisions::IDENTITY);
        if fs::symlink_metadata(&target).is_ok() {
            return Err(HostFailure::new(
                CODE_OUTPUT_EXISTS,
                format!("{} already exists", target.display()),
            ));
        }
        let config_bytes = settings.receipt.network_config_bytes().map_err(invalid)?;
        let client_run_id = random_run_id();
        create_staging_root(settings.staging_root)?;
        let staging = Staging {
            path: settings.staging_root.join(&client_run_id),
        };
        create_private_dir(&staging.path)?;
        create_private_dir(&staging.path.join(ROOMS_DIRECTORY))?;
        create_private_dir(&staging.path.join(OUT_DIRECTORY))?;
        write_new_private(&staging.path.join(CONFIG_FILE), &config_bytes)?;
        let config = FileReceipt::of(&config_bytes);
        let staged_bytes = config.bytes
            + admin_bounds
                .as_ref()
                .map_or(0, |(_, receipt)| receipt.bytes);
        Ok(Self {
            scope,
            staging,
            out_root: settings.out_root.to_path_buf(),
            config,
            admin_bounds,
            rooms: Vec::new(),
            empty: Vec::new(),
            staged_bytes,
            client_run_id,
        })
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn client_run_id(&self) -> &str {
        &self.client_run_id
    }

    /// The rooms staged so far.
    pub fn staged_rooms(&self) -> usize {
        self.rooms.len()
    }

    /// Rooms that had no feature in any report layer: named, never staged.
    pub fn empty_rooms(&self) -> &[String] {
        &self.empty
    }

    /// Stage one saved room. `Ok(false)` is a room with nothing to
    /// aggregate; it is named in [`Self::empty_rooms`] instead.
    pub fn add_room(
        &mut self,
        transformer: &str,
        revision: i64,
        content_digest: Option<&str>,
        layers: &BTreeMap<String, Value>,
    ) -> Result<bool, HostFailure> {
        if self
            .rooms
            .iter()
            .any(|(_, pin)| pin.transformer == transformer)
            || self.empty.iter().any(|name| name == transformer)
        {
            return Err(invalid(format!("{transformer} is staged twice")));
        }
        if self.rooms.len() + self.empty.len() >= decisions::MAX_TRANSFORMERS {
            return Err(invalid(format!(
                "a combined report stages at most {} rooms",
                decisions::MAX_TRANSFORMERS
            )));
        }
        let Some(staged) = decisions::stage_room(transformer, revision, content_digest, layers)
            .map_err(invalid)?
        else {
            self.empty.push(transformer.to_owned());
            return Ok(false);
        };
        let total = self
            .staged_bytes
            .checked_add(staged.pin.source_bytes)
            .filter(|total| *total <= decisions::MAX_TOTAL_INPUT_BYTES)
            .ok_or_else(|| {
                invalid(format!(
                    "the project's rooms exceed the Reporter's {}-byte combined input bound; narrow the scope with --transformer",
                    decisions::MAX_TOTAL_INPUT_BYTES
                ))
            })?;
        let path = self
            .staging
            .path
            .join(ROOMS_DIRECTORY)
            .join(format!("room-{:05}.json", self.rooms.len()));
        write_new_private(&path, &staged.document)?;
        self.staged_bytes = total;
        self.rooms.push((path, staged.pin));
        Ok(true)
    }

    fn sort(&mut self) -> Result<(), HostFailure> {
        self.rooms
            .sort_by(|(_, left), (_, right)| left.transformer.cmp(&right.transformer));
        decisions::sorted_pins(self.pins()).map_err(invalid)?;
        Ok(())
    }

    fn pins(&self) -> Vec<RoomPin> {
        self.rooms.iter().map(|(_, pin)| pin.clone()).collect()
    }

    fn out_dir(&self) -> PathBuf {
        self.staging.path.join(OUT_DIRECTORY)
    }

    fn config_path(&self) -> PathBuf {
        self.staging.path.join(CONFIG_FILE)
    }

    /// The media the staged rooms reference, from the engine's read-only
    /// task over exactly these files.
    pub fn media_scope(&mut self, engine: &dyn ReportEngine) -> Result<MediaScope, HostFailure> {
        self.sort()?;
        let config_path = self.config_path();
        let out_dir = self.out_dir();
        let staged = Staged {
            network_config: (&config_path, &self.config),
            rooms: &self.rooms,
            out_dir: &out_dir,
            admin_bounds: self
                .admin_bounds
                .as_ref()
                .map(|(path, receipt)| (path.as_path(), receipt)),
            media_grant: None,
            transformer_natures: None,
        };
        let request = decisions::media_request(&self.scope, &staged).map_err(invalid)?;
        let request_path = self.staging.path.join(MEDIA_REQUEST_FILE);
        let result_path = self.staging.path.join(MEDIA_RESULT_FILE);
        write_new_private(
            &request_path,
            &serde_json::to_vec(&request).map_err(|error| staging_failed(error.to_string()))?,
        )?;
        let exit = engine.discover_combined_media_references(&request_path, &result_path)?;
        let document = read_bounded(&result_path, decisions::MAX_MEDIA_RESULT_BYTES)?;
        let _ = fs::remove_file(&request_path);
        let _ = fs::remove_file(&result_path);
        if !exit.succeeded {
            return Err(HostFailure::new(
                CODE_ENGINE_REFUSED,
                "the reporter did not complete the combined media scope",
            )
            .with_detail(json!({ "engine": exit.summary })));
        }
        let document = document.ok_or_else(|| {
            HostFailure::new(
                CODE_RESULT_INVALID,
                "the reporter exited successfully without writing its media scope",
            )
        })?;
        decisions::verify_media_scope(&document, self.rooms.len())
            .map_err(|error| HostFailure::new(CODE_RESULT_INVALID, error))
    }

    /// Run the combined task over the staged rooms, prove its answer and
    /// place it. Nothing is left under the output root unless every step
    /// held; the staging directory is removed on every path.
    pub fn run(
        mut self,
        engine: &dyn ReportEngine,
        extras: &CombinedExtras<'_>,
    ) -> Result<CombinedOutcome, HostFailure> {
        self.sort()?;
        let media = match extras.media_grant {
            Some(grant) => {
                if grant.is_empty() || grant.len() as u64 > decisions::MAX_MEDIA_GRANT_BYTES {
                    return Err(invalid(format!(
                        "a media grant is a document of 1 through {} bytes",
                        decisions::MAX_MEDIA_GRANT_BYTES
                    )));
                }
                let path = self.staging.path.join(MEDIA_GRANT_FILE);
                write_new_private(&path, grant)?;
                Some((path, FileReceipt::of(grant)))
            }
            None => None,
        };
        let config_path = self.config_path();
        let out_dir = self.out_dir();
        let staged = Staged {
            network_config: (&config_path, &self.config),
            rooms: &self.rooms,
            out_dir: &out_dir,
            admin_bounds: self
                .admin_bounds
                .as_ref()
                .map(|(path, receipt)| (path.as_path(), receipt)),
            media_grant: media
                .as_ref()
                .map(|(path, receipt)| (path.as_path(), receipt)),
            transformer_natures: extras.transformer_natures,
        };
        let request = decisions::engine_request(&self.scope, &staged).map_err(invalid)?;
        let request_path = self.staging.path.join(REQUEST_FILE);
        let result_path = self.staging.path.join(RESULT_FILE);
        write_new_private(
            &request_path,
            &serde_json::to_vec_pretty(&request)
                .map_err(|error| staging_failed(error.to_string()))?,
        )?;

        let captured = engine.capture_for_run()?;
        let engine = captured.as_deref().unwrap_or(engine);
        let identity_before = engine_identity_now(engine)?;
        let exit = engine.export_combined_transformer_report(&request_path, &result_path)?;
        let document = read_bounded(&result_path, decisions::MAX_RESULT_BYTES)?;
        let verified = match (exit.succeeded, document) {
            (succeeded, Some(document)) => {
                match decisions::verify_result(&document, &self.scope, &staged, &identity_before) {
                    Ok(verified) if succeeded => verified,
                    Ok(_) => {
                        return Err(HostFailure::new(
                            CODE_RESULT_INVALID,
                            "the reporter exited unsuccessfully beside a complete combined result",
                        ))
                    }
                    Err(CombinedRefusal::Incomplete {
                        status,
                        blockers,
                        artifacts,
                        engineering,
                    }) => {
                        return Err(incomplete(
                            &status,
                            &blockers,
                            artifacts,
                            self.scope.outputs.len(),
                            engineering,
                        ))
                    }
                    Err(CombinedRefusal::Invalid { message }) if succeeded => {
                        return Err(HostFailure::new(CODE_RESULT_INVALID, message))
                    }
                    Err(CombinedRefusal::Invalid { .. }) => {
                        return Err(engine_refused(&exit.summary))
                    }
                }
            }
            (false, None) => return Err(engine_refused(&exit.summary)),
            (true, None) => {
                return Err(HostFailure::new(
                    CODE_RESULT_INVALID,
                    "the reporter exited successfully without writing its combined result",
                ))
            }
        };
        verify_staged_artifacts(&out_dir, &verified.artifacts)?;
        if engine_identity_now(engine)? != identity_before {
            return Err(HostFailure::new(
                CODE_ENGINE_IDENTITY_CHANGED,
                "the reporter's build identity changed while the combined report was running",
            ));
        }

        let pins = self.pins();
        let input_base_fingerprint =
            decisions::input_base_fingerprint(&self.scope, &pins, extras.transformer_natures)
                .map_err(invalid)?;
        let room_content_sha256 = decisions::room_content_sha256(&pins).map_err(invalid)?;
        fs::create_dir_all(&self.out_root).map_err(|error| {
            staging_failed(format!(
                "could not create {}: {error}",
                self.out_root.display()
            ))
        })?;
        let artifact_dir = self.out_root.join(decisions::IDENTITY);
        let delivery = Staging {
            path: self
                .out_root
                .join(format!(".{}.partial", self.client_run_id)),
        };
        create_private_dir(&delivery.path)?;
        for artifact in &verified.artifacts {
            place_artifact(
                &out_dir.join(&artifact.filename),
                &delivery.path.join(&artifact.filename),
                artifact.size_bytes,
                &artifact.sha256,
            )?;
        }
        let media_sha256 = media.as_ref().map(|(_, receipt)| receipt.sha256.clone());
        let receipt = decisions::run_receipt(&decisions::ReceiptFacts {
            scope: &self.scope,
            pins: &pins,
            client_run_id: &self.client_run_id,
            engine: &identity_before,
            result: &verified,
            artifact_dir: decisions::IDENTITY,
            now_ms: now_ms(),
            transformer_natures: extras.transformer_natures,
            media_grant_sha256: media_sha256.as_deref(),
        })
        .map_err(|error| HostFailure::new(CODE_RESULT_INVALID, error))?;
        write_new_private(
            &delivery.path.join(RUN_RECEIPT_FILE),
            &serde_json::to_vec_pretty(&receipt)
                .map_err(|error| staging_failed(error.to_string()))?,
        )?;
        promote_report(&delivery.path, &artifact_dir)?;
        Ok(CombinedOutcome {
            run: RunOutcome {
                transformer: decisions::IDENTITY.to_owned(),
                client_run_id: self.client_run_id.clone(),
                receipt_path: artifact_dir.join(RUN_RECEIPT_FILE),
                artifact_dir,
                receipt,
                artifacts: verified.artifacts.clone(),
                failed: Vec::new(),
                warnings: Vec::new(),
                survey_layers_omitted: None,
                survey_layers: Vec::new(),
                engine: identity_before,
            },
            pins,
            result: verified,
            input_base_fingerprint,
            room_content_sha256,
        })
    }
}

/// A combined run that built less than its plan: every planned format is part
/// of the deliverable. Engineering gaps are named per transformer, because
/// they are what an operator fixes before the workbook can publish quantities.
fn incomplete(
    status: &str,
    blockers: &[ds_command_kernel::report_export::Blocker],
    artifacts: usize,
    planned: usize,
    engineering: Option<decisions::EngineeringGaps>,
) -> HostFailure {
    let cause = match &engineering {
        Some(gaps) => {
            let named = gaps
                .transformers
                .iter()
                .take(5)
                .map(|gap| format!("{} ({})", gap.transformer, gap.fields.join(", ")))
                .collect::<Vec<_>>()
                .join("; ");
            let more = gaps.transformers.len().saturating_sub(5);
            let more = if more > 0 {
                format!("; {more} more transformer(s)")
            } else {
                String::new()
            };
            let unitemized = if gaps.omitted_details > 0 {
                format!(
                    "; {} more fact(s) the Reporter counted without itemizing",
                    gaps.omitted_details
                )
            } else {
                String::new()
            };
            format!(
                "the workbook refused to publish quantities: {} missing or invalid engineering fact(s); itemized: {named}{more}{unitemized}",
                gaps.blockers,
            )
        }
        None => blocker_summary(blockers),
    };
    let bounded: Vec<serde_json::Value> = blockers
        .iter()
        .map(|blocker| {
            json!({
                "code": blocker.code,
                "format": blocker.format,
                "message": blocker.message.chars().take(1000).collect::<String>(),
            })
        })
        .collect();
    HostFailure::new(
        CODE_EXPORT_BLOCKED,
        format!(
            "the Reporter built {artifacts} of {planned} planned combined format(s) (status {status}); nothing is sealed because every planned format is part of the Combined Report — {cause}"
        ),
    )
    .with_detail(json!({
        "status": status,
        "artifacts": artifacts,
        "planned": planned,
        "blockers": bounded,
        "engineering": engineering,
    }))
}

fn engine_refused(summary: &str) -> HostFailure {
    HostFailure::new(
        CODE_ENGINE_REFUSED,
        "the reporter did not complete the combined report",
    )
    .with_detail(json!({ "engine": summary }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineExit, HostFailure};
    use sha2::{Digest, Sha256};
    use std::sync::Mutex;

    fn receipt(country: &str) -> InputReceipt {
        let sheets = r#"{"project_settings":[{"parameter":"report_formats","value":"gpkg"}]}"#;
        InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: country.into(),
            sheets_json: sheets.into(),
            sheets_sha256: format!("{:x}", Sha256::digest(sheets.as_bytes())),
            reference_semantic_sha256: "7".repeat(64),
        }
    }

    fn room(features: usize) -> BTreeMap<String, Value> {
        let feature = json!({"type":"Feature","properties":{"pole_number":"P1"},"geometry":{"type":"Point","coordinates":[30.06,-1.94]}});
        serde_json::from_value(json!({
            "lv_poles": {"type": "FeatureCollection", "features": vec![feature; features]},
            "voltage_drop": {"type": "FeatureCollection", "features": []},
        }))
        .unwrap()
    }

    fn build_info() -> Value {
        json!({
            "schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report",
            "package_version": "0.1.0", "source_sha": "a".repeat(40),
            "engine_version": format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
            "cargo_lock_sha256": "d".repeat(64), "features": [], "rustc": "rustc 1.95.0",
            "target": "x86_64-unknown-linux-gnu", "profile": "release",
            "schemas": ["ds.engine-build/v1"], "build_manifest_sha256": "f".repeat(64)
        })
    }

    /// Answers like `ds-report`: reads the request, writes every requested
    /// artifact into `out_dir` and writes its result document.
    #[derive(Default)]
    struct FakeCombined {
        behaviour: &'static str,
        requests: Mutex<Vec<Value>>,
        media: Mutex<Option<Value>>,
    }

    impl ReportEngine for FakeCombined {
        fn build_info(&self) -> Result<Value, HostFailure> {
            Ok(build_info())
        }
        fn export_transformer_report(&self, _: &Path, _: &Path) -> Result<EngineExit, HostFailure> {
            unreachable!("a combined run never exports one transformer")
        }
        fn discover_combined_media_references(
            &self,
            request: &Path,
            result: &Path,
        ) -> Result<EngineExit, HostFailure> {
            let request: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
            let rooms = request["transformers"].as_array().unwrap().len();
            *self.media.lock().unwrap() = Some(request);
            fs::write(result, serde_json::to_vec(&json!({"schema_version":2,"task":decisions::MEDIA_TASK,"status":"completed","engine":build_info(),"reference_count":2,"source_projects":["project-a"],"transformers_scanned":rooms,"features_scanned":3,"total_input_bytes":1,"elapsed_ms":1})).unwrap()).unwrap();
            Ok(EngineExit {
                succeeded: true,
                summary: String::new(),
            })
        }
        fn export_combined_transformer_report(
            &self,
            request: &Path,
            result: &Path,
        ) -> Result<EngineExit, HostFailure> {
            let request: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
            self.requests.lock().unwrap().push(request.clone());
            if self.behaviour == "refuse" {
                return Ok(EngineExit {
                    succeeded: false,
                    summary: "combined_media_requires_supplied_links: grant needed".into(),
                });
            }
            let out = PathBuf::from(request["out_dir"].as_str().unwrap());
            let file = |path: &str| {
                let bytes = fs::read(path).unwrap();
                json!({"bytes": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes))})
            };
            let rooms: Vec<Value> = request["transformers"].as_array().unwrap().iter().map(|room| {
                json!({"transformer": room["transformer"], "source": file(room["layers"].as_str().unwrap()), "layers_decoded": 1, "features_decoded": 1, "geometry_changed": false, "admin_features_requested": 0, "admin_features_matched": 0})
            }).collect();
            let mut total = file(request["network_config"].as_str().unwrap())["bytes"]
                .as_u64()
                .unwrap();
            total += rooms
                .iter()
                .map(|room| room["source"]["bytes"].as_u64().unwrap())
                .sum::<u64>();
            let media = request
                .get("media_grant")
                .map(|path| file(path.as_str().unwrap()));
            total += media
                .as_ref()
                .map_or(0, |receipt| receipt["bytes"].as_u64().unwrap());
            let mut artifacts = Vec::new();
            let formats: Vec<String> = serde_json::from_value(request["formats"].clone()).unwrap();
            for format in &formats {
                let filename =
                    ds_command_kernel::report_formats::report_filename(decisions::IDENTITY, format)
                        .unwrap();
                let path = out.join(&filename);
                fs::write(&path, format!("{format} bytes")).unwrap();
                if self.behaviour == "tamper" && format == "xlsx" {
                    fs::write(&path, "tampered").unwrap();
                    artifacts.push(json!({"format": format, "filename": filename, "path": path, "bytes": 10, "sha256": format!("{:x}", Sha256::digest(b"xlsx bytes"))}));
                    continue;
                }
                let bytes = fs::read(&path).unwrap();
                artifacts.push(json!({"format": format, "filename": filename, "path": path, "bytes": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes))}));
            }
            let mut status = "completed";
            let mut blockers = json!([]);
            if self.behaviour == "partial" {
                status = "partial";
                let removed = artifacts.pop().unwrap();
                blockers = json!([{"code": "artifact_stage_failed", "message": "disk full", "format": removed["format"]}]);
            }
            let document = json!({
                "schema_version": 4, "task": decisions::TASK, "status": status, "engine": build_info(),
                "prefix": decisions::IDENTITY, "country": request["country"],
                "upstream_admission_requirements": [{"code": decisions::REVIEWS_REQUIREMENT, "signed_work_grant_field": "transformer_reviews_absent", "required_value": true, "must_bind_input_fingerprint": true, "forwarded_to_task": false, "message": "m"}],
                "input_receipt": {"network_config": file(request["network_config"].as_str().unwrap()), "media_grant": media, "transformers": rooms, "total_input_bytes": total},
                "layers_aggregated": 1, "features_aggregated": 2, "geometry_changed": false,
                "admin_bounds": {"configured": false, "villages_loaded": 0, "features_matched": 0, "features_requested": 0},
                "solar": {"configured": false}, "formats_requested": formats, "artifacts": artifacts,
                "tiles": {}, "blockers": blockers,
                "phase_timings": {"input_preparation_ms": 1, "format_compute_ms": 2, "artifact_finalize_ms": 3}, "elapsed_ms": 6
            });
            fs::write(result, serde_json::to_vec(&document).unwrap()).unwrap();
            Ok(EngineExit {
                succeeded: status != "failed",
                summary: String::new(),
            })
        }
    }

    fn begin(root: &Path, receipt: &InputReceipt) -> CombinedStaging {
        CombinedStaging::begin(&CombinedSettings {
            project_id: "project-a",
            receipt,
            admin_bounds: None,
            staging_root: &root.join("staging"),
            out_root: &root.join("out"),
        })
        .unwrap()
    }

    #[test]
    fn rooms_stage_one_at_a_time_and_the_run_places_the_verified_package() {
        let root = tempfile::tempdir().unwrap();
        let receipt = receipt("Burundi");
        let mut staging = begin(root.path(), &receipt);
        assert!(staging.add_room("tx_b", 2, None, &room(2)).unwrap());
        assert!(!staging.add_room("tx_empty", 1, None, &room(0)).unwrap());
        assert!(staging.add_room("tx_a", 5, None, &room(1)).unwrap());
        assert!(staging.add_room("tx_a", 5, None, &room(1)).is_err());
        assert_eq!(staging.empty_rooms(), ["tx_empty"]);
        let engine = FakeCombined::default();
        let scope = staging.media_scope(&engine).unwrap();
        assert_eq!(scope.reference_count, 2);
        let media = engine.media.lock().unwrap().clone().unwrap();
        assert_eq!(media["transformers"][0]["transformer"], "tx_a");
        let natures = BTreeMap::from([("tx_a".to_string(), "fill_in".to_string())]);
        let grant = br#"{"grants":[]}"#;
        let staging_root = root.path().join("staging");
        let outcome = staging
            .run(
                &engine,
                &CombinedExtras {
                    media_grant: Some(grant),
                    transformer_natures: Some(&natures),
                },
            )
            .unwrap();
        let request = engine.requests.lock().unwrap()[0].clone();
        assert_eq!(
            request["formats"],
            json!(["shp", "xlsx", "geojsonseq", "gpkg"])
        );
        assert_eq!(request["transformer_natures"]["tx_a"], "fill_in");
        assert!(request.get("admin_bounds_asset").is_none());
        // Staging is gone once the run ends.
        assert!(!Path::new(request["transformers"][0]["layers"].as_str().unwrap()).exists());
        assert_eq!(fs::read_dir(&staging_root).unwrap().count(), 0);
        let placed = root.path().join("out").join("combined_transformer");
        for artifact in &outcome.run.artifacts {
            assert!(
                placed.join(&artifact.filename).is_file(),
                "{}",
                artifact.filename
            );
        }
        assert_eq!(outcome.run.transformer, "combined_transformer");
        assert_eq!(outcome.run.receipt["transformer_revision"], 0);
        assert_eq!(
            outcome.run.receipt["input_base_fingerprint"],
            json!(outcome.input_base_fingerprint)
        );
        assert_eq!(
            outcome
                .pins
                .iter()
                .map(|pin| pin.transformer.as_str())
                .collect::<Vec<_>>(),
            ["tx_a", "tx_b"]
        );
        assert!(placed.join(RUN_RECEIPT_FILE).is_file());
    }

    #[test]
    fn partial_tampered_or_refused_runs_leave_nothing_under_the_output_root() {
        for behaviour in ["partial", "tamper", "refuse"] {
            let root = tempfile::tempdir().unwrap();
            let receipt = receipt("Burundi");
            let mut staging = begin(root.path(), &receipt);
            staging.add_room("tx_a", 1, None, &room(1)).unwrap();
            let engine = FakeCombined {
                behaviour,
                ..Default::default()
            };
            let error = staging
                .run(&engine, &CombinedExtras::default())
                .unwrap_err();
            let expected = match behaviour {
                "partial" => CODE_EXPORT_BLOCKED,
                "tamper" => CODE_RESULT_INVALID,
                _ => CODE_ENGINE_REFUSED,
            };
            assert_eq!(error.code, expected, "{behaviour}: {error}");
            assert!(
                !root
                    .path()
                    .join("out")
                    .join("combined_transformer")
                    .exists(),
                "{behaviour}"
            );
            assert_eq!(
                fs::read_dir(root.path().join("staging")).unwrap().count(),
                0,
                "{behaviour}"
            );
        }
    }

    #[test]
    fn rwanda_needs_the_reference_and_an_existing_package_is_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let rwanda = receipt("Rwanda");
        let settings = CombinedSettings {
            project_id: "project-a",
            receipt: &rwanda,
            admin_bounds: None,
            staging_root: &root.path().join("staging"),
            out_root: &root.path().join("out"),
        };
        assert_eq!(
            CombinedStaging::begin(&settings).err().unwrap().code,
            CODE_ADMIN_BOUNDS_UNAVAILABLE
        );
        let other = receipt("Burundi");
        fs::create_dir_all(root.path().join("out").join("combined_transformer")).unwrap();
        let existing = CombinedSettings {
            receipt: &other,
            ..settings
        };
        assert_eq!(
            CombinedStaging::begin(&existing).err().unwrap().code,
            CODE_OUTPUT_EXISTS
        );
        let mut staging = CombinedStaging::begin(&CombinedSettings {
            out_root: &root.path().join("fresh"),
            ..existing
        })
        .unwrap();
        let digest = "0".repeat(64);
        assert_eq!(
            staging
                .add_room("tx_a", 1, Some(&digest), &room(1))
                .unwrap_err()
                .code,
            CODE_INPUTS_INVALID
        );
        assert_eq!(
            staging
                .add_room("collisions", 1, None, &room(1))
                .unwrap_err()
                .code,
            CODE_INPUTS_INVALID
        );
    }
}
