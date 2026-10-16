//! Native addon selection happens only at accepted admission. Resumes inspect
//! the retained executable, never the active lane pointer or executable override.
use super::*;
use ds_command_kernel::compute_jobs::{EngineLane, EnginePin, EnginePinSource};
use std::io::Read;

fn sha256_file(path: &Path) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("compute_engine_pin_unavailable: {e}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 512 * 1024 * 1024 {
        return Err("compute_engine_pin_invalid: executable is not a bounded ordinary file".into());
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut file = file.take(512 * 1024 * 1024 + 1);
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut chunk = [0u8; 65536];
    loop {
        let count = file.read(&mut chunk).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        length += count as u64;
        hash.update(&chunk[..count]);
    }
    if length != metadata.len() {
        return Err("compute_engine_changed_during_capture".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(crate) fn worker_share(global_running: usize) -> usize {
    (thread::available_parallelism().map_or(1, usize::from) / global_running.max(1)).max(1)
}

pub(crate) fn capture(
    job: &Job,
    mut workers: usize,
    module_base: Option<&Path>,
) -> Result<Option<EnginePin>, String> {
    if job.engine == EngineKind::TilesPrepared {
        return Ok(None);
    }
    let context = job
        .context
        .clone()
        .ok_or("compute_engine_pin_context_mismatch")?;
    let (executable, executable_sha256, identity, development_override) = match job.engine {
        EngineKind::LvProcessing => {
            let client = ds_network_client::CapturedNetworkClient::capture_for_lane_in(
                &job.lane,
                workers,
                &|| false,
                module_base,
            )
            .map_err(|error| format!("{}: {error}", error.code()))?;
            (
                client.executable().to_path_buf(),
                client.executable_sha256().to_owned(),
                serde_json::to_value(client.identity()).map_err(|e| e.to_string())?,
                client.development_override(),
            )
        }
        EngineKind::SolarPrepared => {
            // Preserve Solar's established owner limit in the admitted width.
            workers = workers.min(ds_solar_io::compute_parallelism());
            let executable = if std::env::var_os("DS_SOLAR_ENGINE_BIN").is_some() {
                ds_solar_process::installed_engine_path()?
            } else {
                let selected = match module_base {
                    Some(base) => ds_cli_exec::addons::SOLAR.capture_for_lane_in(&job.lane, base),
                    None => ds_cli_exec::addons::SOLAR.capture_for_lane(&job.lane),
                }
                .map_err(|error| format!("{}: {error}", error.code()))?;
                selected.executable().to_path_buf()
            };
            let sha = sha256_file(&executable)?;
            let identity = ds_solar_process::selected_engine_identity(&executable)
                .map_err(|e| e.to_string())?;
            if sha256_file(&executable)? != sha {
                return Err("compute_engine_changed_during_capture".into());
            }
            (
                executable,
                sha,
                serde_json::to_value(identity).map_err(|e| e.to_string())?,
                std::env::var_os("DS_SOLAR_ENGINE_BIN").is_some(),
            )
        }
        EngineKind::TilesPrepared => unreachable!(),
    };
    Ok(Some(EnginePin {
        schema: "ds.compute.engine-pin/v1".into(),
        engine: job.engine.clone(),
        owner: job.owner.clone(),
        lane: EngineLane::parse(&job.lane).map_err(str::to_owned)?,
        context,
        source: EnginePinSource::NativeLaneCapture,
        executable: executable
            .into_os_string()
            .into_string()
            .map_err(|_| "compute_engine_pin_path_invalid")?,
        executable_sha256,
        identity,
        workers,
        development_override,
    }))
}

pub(crate) fn network(
    job: &Job,
    cancel: &dyn Fn() -> bool,
) -> Result<ds_network_client::CapturedNetworkClient, String> {
    compute_jobs::validate_engine_pin(job).map_err(str::to_owned)?;
    let pin = job
        .engine_pin
        .as_ref()
        .ok_or("compute_engine_pin_required")?;
    let identity: ds_network_contracts::engine::EngineIdentity =
        serde_json::from_value(pin.identity.clone())
            .map_err(|e| format!("compute_engine_pin_identity_invalid: {e}"))?;
    if identity.runtime.internal_worker_threads != pin.workers
        || serde_json::to_value(&identity).map_err(|e| e.to_string())? != pin.identity
    {
        return Err("compute_engine_pin_identity_invalid".into());
    }
    ds_network_client::CapturedNetworkClient::from_pin(
        &ds_network_client::NetworkEnginePin {
            schema: "ds.network.captured-engine/v1".into(),
            executable: PathBuf::from(&pin.executable),
            executable_sha256: pin.executable_sha256.clone(),
            identity,
            development_override: pin.development_override,
        },
        cancel,
    )
    .map_err(|e| format!("{}: {e}", e.code()))
}

pub(crate) fn solar(
    job: &Job,
    scratch: &Path,
    cancel: &CancelToken,
) -> Result<ds_solar_process::PreparedEngine, String> {
    compute_jobs::validate_engine_pin(job).map_err(str::to_owned)?;
    let pin = job
        .engine_pin
        .as_ref()
        .ok_or("compute_engine_pin_required")?;
    let identity: ds_solar_contracts::EngineIdentity = serde_json::from_value(pin.identity.clone())
        .map_err(|e| format!("compute_engine_pin_identity_invalid: {e}"))?;
    if identity.name != "ds-solar-engine"
        || identity.version.is_empty()
        || serde_json::to_value(&identity).map_err(|e| e.to_string())? != pin.identity
    {
        return Err("compute_engine_pin_identity_invalid".into());
    }
    let executable = Path::new(&pin.executable);
    if pin.workers > ds_solar_io::compute_parallelism() {
        return Err("compute_engine_pin_budget_unavailable: restore the admitted Solar worker share; input and exact engine pin are retained".into());
    }
    if !executable.is_absolute() || sha256_file(executable)? != pin.executable_sha256 {
        return Err("compute_engine_pin_changed".into());
    }
    let engine =
        ds_solar_process::PreparedEngine::capture(executable, scratch, pin.workers, cancel)
            .map_err(|e| e.to_string())?;
    if engine.identity() != &identity || sha256_file(executable)? != pin.executable_sha256 {
        return Err("compute_engine_pin_changed".into());
    }
    Ok(engine)
}
