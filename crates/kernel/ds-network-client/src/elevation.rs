//! The Network engine's `elevation` family (DECOUPLING-013): Core sends one
//! closed request naming the raster files it holds; the engine computes and
//! either completes or names the exact Rwanda blocks and Terrarium tiles it
//! still needs. The host acquires those and asks again, a bounded number of
//! times. Raster acquisition, confirmation and final file placement stay
//! with the host; interpolation and sampling run only in the engine.
use super::*;
use ds_network_contracts::elevation::*;

/// Rounds one job may take: rasters first, then fallback tiles, then the
/// rare block a lazy reader met. More means the engine and host disagree.
pub const MAX_ELEVATION_ROUNDS: usize = 8;

/// What one round asked the host to hold before it asks again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Needed {
    pub rwanda_index: bool,
    pub rwanda_blocks: Vec<RwandaBlockV1>,
    pub terrarium_tiles: Vec<TerrariumTileV1>,
}

/// A completed task and the private directory its artifacts are in.
#[derive(Debug)]
pub struct ElevationCompleted {
    pub result: ElevationResultV1,
    pub out: tempfile::TempDir,
    pub identity: EngineIdentity,
    pub executable_sha256: String,
}

impl ElevationCompleted {
    /// One named artifact of the engine's output directory.
    pub fn artifact(&self, name: &str) -> PathBuf {
        self.out.path().join("output").join(name)
    }
}

impl NetworkFailure {
    /// A host that could not hold an input the engine named; the host keeps
    /// its own typed cause and reports that instead.
    pub fn acquisition(message: impl Into<String>) -> Self {
        Self::new("elevation_acquisition_failed", message)
    }
}

impl CapturedNetworkClient {
    /// One round: the engine's answer and the directory it wrote into.
    pub fn elevation_round(
        &self,
        request: &ElevationRequestV1,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(ElevationReplyV1, tempfile::TempDir), NetworkFailure> {
        if !self
            .identity
            .capabilities
            .contains(&NetworkCapability::ElevationV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "selected Network engine has no ElevationV1 capability",
            ));
        }
        validate_elevation_request(request)
            .map_err(|e| NetworkFailure::new("elevation_request_invalid", e))?;
        let input = serde_json::to_vec(request)
            .map_err(|e| NetworkFailure::new("elevation_request_invalid", e.to_string()))?;
        if input.len() > MAX_ELEVATION_REQUEST_BYTES {
            return Err(NetworkFailure::new(
                "elevation_request_invalid",
                "elevation request exceeds its admitted input bound",
            ));
        }
        let metadata = fs::symlink_metadata(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(NetworkFailure::new(
                "network_scratch_invalid",
                "scratch must be an ordinary directory",
            ));
        }
        let run = tempfile::Builder::new()
            .prefix("ds-network-elevation-")
            .tempdir_in(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(run.path(), fs::Permissions::from_mode(0o700))
                .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        }
        let request_path = run.path().join("request.json");
        write_request(&request_path, &input)?;
        let out = run.path().join("output");
        create_output(&out)?;
        let width = self.identity.runtime.internal_worker_threads;
        let args: Vec<OsString> = vec![
            "--request".into(),
            request_path.as_os_str().to_owned(),
            "--out".into(),
            out.as_os_str().to_owned(),
            "--workers".into(),
            width.to_string().into(),
        ];
        let completed = self
            .engine
            .call_cancelled(ELEVATION_SUBCOMMAND, &args, TASK_TIMEOUT, width, cancel)
            .map_err(|e| NetworkFailure::new(static_code(e.code()), e.to_string()))?;
        if completed.truncated || completed.stdout.len() > MAX_ELEVATION_REPLY_BYTES {
            return Err(NetworkFailure::new(
                "network_receipt_bounded",
                "elevation task exceeded its reply bound",
            ));
        }
        if !completed.succeeded() {
            return Err(read_refusal(&completed.stdout, &self.identity)?);
        }
        let envelope: ElevationReplyEnvelopeV1 = serde_json::from_str(&completed.stdout)
            .map_err(|e| NetworkFailure::new("network_receipt_invalid", e.to_string()))?;
        if !crate::same_family(&envelope.schema, ELEVATION_REPLY_SCHEMA_V1)
            || envelope.input_sha256 != digest_bytes(&input)
            || !same_identity(&envelope.engine_identity, &self.identity)?
        {
            return Err(NetworkFailure::new(
                "network_receipt_invalid",
                "the elevation reply answers another request or engine",
            ));
        }
        Ok((envelope.reply, run))
    }

    /// The whole job: rounds until the engine completes. `acquire` holds
    /// exactly the rasters a round named (and may update the request, for
    /// instance with the acquisition evidence the next round records).
    pub fn elevation(
        &self,
        request: &mut ElevationRequestV1,
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
        acquire: &mut dyn FnMut(&mut ElevationRequestV1, &Needed) -> Result<(), NetworkFailure>,
    ) -> Result<ElevationCompleted, NetworkFailure> {
        let mut asked = std::collections::BTreeSet::new();
        for _ in 0..MAX_ELEVATION_ROUNDS {
            let (reply, run) = self.elevation_round(request, scratch, cancel)?;
            match reply {
                ElevationReplyV1::Completed { result } => {
                    return Ok(ElevationCompleted {
                        result,
                        out: run,
                        identity: self.identity.clone(),
                        executable_sha256: self.executable_sha256.clone(),
                    });
                }
                ElevationReplyV1::InputsRequired {
                    rwanda_index,
                    rwanda_blocks,
                    terrarium_tiles,
                } => {
                    // An input asked for twice was not held as asked: the
                    // engine and host disagree, so stop instead of looping.
                    let fresh = rwanda_index
                        .then(|| "rwanda_index".to_string())
                        .into_iter()
                        .chain(rwanda_blocks.iter().map(|b| format!("{b:?}")))
                        .chain(terrarium_tiles.iter().map(|t| format!("{t:?}")));
                    let mut any = false;
                    for key in fresh {
                        any = true;
                        if !asked.insert(key) {
                            return Err(NetworkFailure::new(
                                "elevation_inputs_unsettled",
                                "the engine asked again for a raster input the host held",
                            ));
                        }
                    }
                    if !any {
                        return Err(NetworkFailure::new(
                            "network_receipt_invalid",
                            "the engine asked for no input and completed nothing",
                        ));
                    }
                    acquire(
                        request,
                        &Needed {
                            rwanda_index,
                            rwanda_blocks,
                            terrarium_tiles,
                        },
                    )?;
                }
            }
        }
        Err(NetworkFailure::new(
            "elevation_inputs_unsettled",
            "the elevation job needed more acquisition rounds than its bound",
        ))
    }
}
