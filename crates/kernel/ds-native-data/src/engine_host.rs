//! The host half of elevation work (DECOUPLING-013).
//!
//! The Network engine interpolates and samples; this machine holds the raster
//! bytes it reads. A round names the exact Rwanda block ranges and Terrarium
//! tiles it needs; they are read from the verified block cache or fetched as
//! exact public ranges, held in the run's private directory, and the same
//! request is sent again. Final file placement is the host's too.

use crate::RequestContext;
use crate::bridge::{CommandFailure, FailureClass};
use crate::rwanda_dem::VerifiedRwandaDemRuntime;
use ds_network_client::{CapturedNetworkClient, ElevationCompleted, Needed, NetworkFailure};
use ds_network_contracts::elevation::{
    DemAccessV1, ELEVATION_REQUEST_SCHEMA_V1, ElevationRequestV1, ElevationTaskV1, HeldFileV1,
    HeldRastersV1, RwandaBlockV1, TerrariumTileV1, rwanda_block_file, terrarium_tile_file,
};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BLOCK_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TILE_BYTES: u64 = 2 * 1024 * 1024;
/// Inputs one round may ask for; the job's own bounds keep it far below.
const MAX_ROUND_INPUTS: usize = 4_096;

/// A Network failure as the native data vocabulary states it.
pub(crate) fn engine_failure(failure: NetworkFailure) -> CommandFailure {
    let (class, code, remedy) = match failure.code() {
        "network_engine_missing" | "module_removed" => (
            FailureClass::Unavailable,
            "network_engine_missing",
            "Install or activate a compatible Network engine; elevation is computed there",
        ),
        "network_engine_incompatible" => (
            FailureClass::Unavailable,
            "network_engine_incompatible",
            "Update the Network engine to one that serves elevation_v1",
        ),
        _ => (
            FailureClass::Failed,
            crate::bridge::FAILED_CODE,
            "Resolve the named native resource or file failure, then retry the same explicit request.",
        ),
    };
    let message = match failure.owner_code() {
        Some(owner) => format!("{owner}: {}", failure.message),
        None => failure.message.clone(),
    };
    CommandFailure {
        class,
        code,
        message,
        remedy: remedy.into(),
        detail: json!({}),
    }
}

/// One engine job: its client, private run directory and held inputs.
pub(crate) struct EngineJob {
    client: CapturedNetworkClient,
    run: tempfile::TempDir,
}

impl EngineJob {
    /// Capture the Network engine and a private directory beside `near`, so
    /// the artifacts it writes move into place without crossing devices.
    pub(crate) fn open(near: &Path) -> Result<Self, CommandFailure> {
        let client = CapturedNetworkClient::capture(1, &|| false).map_err(engine_failure)?;
        let run = tempfile::Builder::new()
            .prefix(".ds-elevation-")
            .tempdir_in(near)
            .map_err(|error| CommandFailure::failed(format!("create elevation run: {error}")))?;
        for dir in ["blocks", "tiles"] {
            std::fs::create_dir(run.path().join(dir)).map_err(|error| {
                CommandFailure::failed(format!("create elevation run: {error}"))
            })?;
        }
        Ok(Self { client, run })
    }

    fn blocks(&self) -> PathBuf {
        self.run.path().join("blocks")
    }

    fn tiles(&self) -> PathBuf {
        self.run.path().join("tiles")
    }

    /// The rasters this machine holds for one job.
    pub(crate) fn rasters(&self, runtime: Option<&VerifiedRwandaDemRuntime>) -> HeldRastersV1 {
        HeldRastersV1 {
            rwanda_index: runtime.map(|runtime| runtime.index_path.clone()),
            rwanda_cog: runtime.and_then(|runtime| {
                runtime.cog_path.as_ref().map(|path| HeldFileV1 {
                    path: path.clone(),
                    size_bytes: runtime.cog_size_bytes,
                })
            }),
            rwanda_blocks: Some(self.blocks()),
            terrarium_tiles: Some(self.tiles()),
            dem_access: match runtime {
                Some(runtime) if runtime.mode == "offline" => DemAccessV1::InstalledFull,
                Some(_) => DemAccessV1::DirectRange,
                None if crate::rwanda_dem::installed_full_available() => DemAccessV1::InstalledFull,
                None => DemAccessV1::DirectRange,
            },
            full_local_required_above_points: runtime
                .map(|runtime| runtime.full_local_required_above_points)
                .unwrap_or(crate::rwanda_dem::FULL_LOCAL_REQUIRED_ABOVE_POINTS),
        }
    }

    /// Run `task` to completion, holding what each round asks for.
    pub(crate) fn run(
        &self,
        context: &RequestContext,
        runtime: Option<&VerifiedRwandaDemRuntime>,
        task: ElevationTaskV1,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ElevationCompleted, CommandFailure> {
        let mut request = ElevationRequestV1 {
            schema: ELEVATION_REQUEST_SCHEMA_V1.into(),
            execution_host: context.execution_host().into(),
            rasters: self.rasters(runtime),
            task,
        };
        let mut failure = None;
        let completed =
            self.client
                .elevation(
                    &mut request,
                    self.run.path(),
                    cancel,
                    &mut |_, needed| match acquire(
                        context,
                        runtime,
                        needed,
                        &self.blocks(),
                        &self.tiles(),
                    ) {
                        Ok(()) => Ok(()),
                        Err(error) => {
                            failure = Some(error);
                            Err(NetworkFailure::acquisition(
                                "the host could not hold a raster the engine named",
                            ))
                        }
                    },
                );
        match completed {
            Ok(completed) => Ok(completed),
            Err(error) => Err(failure.take().unwrap_or_else(|| engine_failure(error))),
        }
    }
}

/// Hold exactly what one round named.
fn acquire(
    context: &RequestContext,
    runtime: Option<&VerifiedRwandaDemRuntime>,
    needed: &Needed,
    blocks: &Path,
    tiles: &Path,
) -> Result<(), CommandFailure> {
    if needed.rwanda_blocks.len() + needed.terrarium_tiles.len() > MAX_ROUND_INPUTS {
        return Err(CommandFailure::failed(
            "an elevation round asked for more rasters than one job may hold".into(),
        ));
    }
    let runtime = match runtime {
        Some(runtime) => runtime,
        None => {
            return Err(CommandFailure::failed(
                "the elevation job reads the Rwanda DEM but no verified resource is held".into(),
            ));
        }
    };
    if needed.rwanda_index {
        return Err(CommandFailure::failed(
            "the Network engine asked for the Rwanda index this host already supplied".into(),
        ));
    }
    if !needed.rwanda_blocks.is_empty() {
        let mut provider = CachedRangeProvider {
            url: runtime.cog_url.to_string(),
            total: runtime.cog_size_bytes,
            cache: runtime.cache_path.clone(),
            max_bytes: runtime.cache_max_bytes,
            max_entries: runtime.cache_max_entries,
            fetcher: HttpRangeFetcher::new(context.check_online),
        };
        for block in &needed.rwanda_blocks {
            let bytes = provider.read(block).map_err(CommandFailure::failed)?;
            crate::elevation::write_atomic(&blocks.join(rwanda_block_file(block)), &bytes)
                .map_err(CommandFailure::failed)?;
        }
    }
    if !needed.terrarium_tiles.is_empty() {
        let agent = agent();
        for tile in &needed.terrarium_tiles {
            let bytes = fetch_tile(&agent, context, runtime.terrarium_url_template, tile)
                .map_err(CommandFailure::failed)?;
            crate::elevation::write_atomic(&tiles.join(terrarium_tile_file(tile)), &bytes)
                .map_err(CommandFailure::failed)?;
        }
    }
    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .timeout_per_call(Some(HTTP_TIMEOUT))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(HTTP_TIMEOUT))
        .timeout_recv_body(Some(HTTP_TIMEOUT))
        .build()
        .new_agent()
}

fn fetch_tile(
    agent: &ureq::Agent,
    context: &RequestContext,
    template: &str,
    tile: &TerrariumTileV1,
) -> Result<Vec<u8>, String> {
    let url = template
        .replace("{z}", &tile.z.to_string())
        .replace("{x}", &tile.x.to_string())
        .replace("{y}", &tile.y.to_string());
    (context.check_online)()?;
    let response = agent
        .get(&url)
        .call()
        .map_err(|error| format!("Terrarium transport failed: {error}"))?;
    if response.status().as_u16() != 200 {
        return Err(format!("Terrarium returned HTTP {}", response.status()));
    }
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_TILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read Terrarium tile: {error}"))?;
    if bytes.len() as u64 > MAX_TILE_BYTES {
        return Err("Terrarium tile exceeded the bounded response size".to_string());
    }
    Ok(bytes)
}

/// Move one engine artifact to its final place without replacing anything.
pub(crate) fn place(artifact: &Path, destination: &Path) -> Result<(), String> {
    std::fs::hard_link(artifact, destination)
        .or_else(|_| {
            // A filesystem without links: copy into a fresh file.
            let mut source = std::fs::File::open(artifact)?;
            let mut target = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)?;
            std::io::copy(&mut source, &mut target)?;
            target.sync_all()
        })
        .map_err(|error| format!("place {}: {error}", destination.display()))?;
    let _ = std::fs::remove_file(artifact);
    Ok(())
}

pub(crate) trait RangeFetcher {
    fn fetch(&mut self, url: &str, offset: u64, length: u64, total: u64)
    -> Result<Vec<u8>, String>;
}

pub(crate) struct HttpRangeFetcher {
    agent: ureq::Agent,
    check_online: crate::OnlineCheck,
}

impl HttpRangeFetcher {
    pub(crate) fn new(check_online: crate::OnlineCheck) -> Self {
        Self {
            check_online,
            agent: agent(),
        }
    }
}

pub(crate) fn validate_range_response(
    status: u16,
    content_range: Option<&str>,
    offset: u64,
    length: u64,
    total: u64,
) -> Result<(), String> {
    let end = offset
        .checked_add(length)
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| "Rwanda DEM byte range overflowed".to_string())?;
    let expected = format!("bytes {offset}-{end}/{total}");
    if status != 206 {
        return Err(format!(
            "Rwanda DEM range server returned HTTP {status}; exact HTTP 206 is required"
        ));
    }
    if content_range != Some(expected.as_str()) {
        return Err(format!(
            "Rwanda DEM range server returned a mismatched Content-Range; expected {expected}"
        ));
    }
    Ok(())
}

impl RangeFetcher for HttpRangeFetcher {
    fn fetch(
        &mut self,
        url: &str,
        offset: u64,
        length: u64,
        total: u64,
    ) -> Result<Vec<u8>, String> {
        (self.check_online)()?;
        let end = offset
            .checked_add(length)
            .and_then(|value| value.checked_sub(1))
            .ok_or_else(|| "Rwanda DEM byte range overflowed".to_string())?;
        let response = self
            .agent
            .get(url)
            .header("Range", &format!("bytes={offset}-{end}"))
            .call()
            .map_err(|error| format!("Rwanda DEM public range transport failed: {error}"))?;
        let status = response.status().as_u16();
        let content_range = response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok());
        validate_range_response(status, content_range, offset, length, total)?;
        let mut reader = response.into_body().into_reader().take(length + 1);
        let mut bytes = Vec::with_capacity(length as usize);
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| format!("read Rwanda DEM public byte range: {error}"))?;
        if bytes.len() != length as usize {
            return Err(format!(
                "Rwanda DEM range returned {} bytes; expected {length}",
                bytes.len()
            ));
        }
        Ok(bytes)
    }
}

/// Exact compressed ranges, persisted in the verified resource's block cache.
pub(crate) struct CachedRangeProvider<F> {
    pub(crate) url: String,
    pub(crate) total: u64,
    pub(crate) cache: PathBuf,
    pub(crate) max_bytes: u64,
    pub(crate) max_entries: u32,
    pub(crate) fetcher: F,
}

impl<F: RangeFetcher> CachedRangeProvider<F> {
    fn cache_path(&self, block: &RwandaBlockV1) -> PathBuf {
        self.cache.join(rwanda_block_file(block))
    }

    fn read_cache(path: &Path, expected: u64) -> Result<Option<Vec<u8>>, String> {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            return Ok(None);
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != expected {
            std::fs::remove_file(path)
                .map_err(|error| format!("remove invalid Rwanda DEM cache entry: {error}"))?;
            return Ok(None);
        }
        std::fs::read(path)
            .map(Some)
            .map_err(|error| format!("read Rwanda DEM block cache: {error}"))
    }

    fn trim_cache(&self, incoming: u64) -> Result<(), String> {
        let mut entries = Vec::<(PathBuf, u64, SystemTime)>::new();
        for entry in std::fs::read_dir(&self.cache)
            .map_err(|error| format!("inspect Rwanda DEM block cache: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("inspect Rwanda DEM cache entry: {error}"))?;
            let metadata = entry
                .metadata()
                .map_err(|error| format!("inspect Rwanda DEM cache metadata: {error}"))?;
            if metadata.is_file() {
                entries.push((
                    entry.path(),
                    metadata.len(),
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                ));
            }
        }
        entries.sort_by_key(|entry| entry.2);
        let mut bytes = entries.iter().map(|entry| entry.1).sum::<u64>();
        let mut count = entries.len() as u32;
        for (path, size, _) in entries {
            if bytes.saturating_add(incoming) <= self.max_bytes && count < self.max_entries {
                break;
            }
            std::fs::remove_file(&path)
                .map_err(|error| format!("evict Rwanda DEM cache entry: {error}"))?;
            bytes = bytes.saturating_sub(size);
            count = count.saturating_sub(1);
        }
        if incoming > self.max_bytes || self.max_entries == 0 {
            return Err("Rwanda DEM block exceeds the governed cache bound".to_string());
        }
        Ok(())
    }

    fn write_cache(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.trim_cache(bytes.len() as u64)?;
        let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut file = ds_layer_store::private::open_options()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("create Rwanda DEM cache entry: {error}"))?;
        let result = (|| {
            file.write_all(bytes)
                .map_err(|error| format!("write Rwanda DEM cache entry: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("sync Rwanda DEM cache entry: {error}"))?;
            std::fs::rename(&temp, path)
                .map_err(|error| format!("admit Rwanda DEM cache entry: {error}"))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }

    /// One exact block: from the cache, else one public range read.
    pub(crate) fn read(&mut self, block: &RwandaBlockV1) -> Result<Vec<u8>, String> {
        if block.length == 0
            || block.length > MAX_BLOCK_BYTES
            || block.offset.saturating_add(block.length) > self.total
        {
            return Err("the Network engine asked for a range outside the Rwanda COG".into());
        }
        let path = self.cache_path(block);
        if let Some(bytes) = Self::read_cache(&path, block.length)? {
            return Ok(bytes);
        }
        let bytes = self
            .fetcher
            .fetch(&self.url, block.offset, block.length, self.total)?;
        self.write_cache(&path, &bytes)?;
        Ok(bytes)
    }
}

#[cfg(test)]
#[path = "engine_host_tests.rs"]
mod tests;
