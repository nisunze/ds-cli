//! Prepared tiling jobs: retain the source bytes with the admitted project.
//! Server and Desktop call this owner; neither interprets tiling algorithms.
use super::*;
use ds_tile_runtime::{Cancel, Request, Runtime, Scope, Tools};
use std::collections::BTreeMap;
use std::future::Future;

pub const INPUT_TAG: &str = "ds.tiles.prepared/v1";
const MAX_BYTES: usize = 64 * 1024 * 1024;
pub use ds_tile_runtime::Prepared;
fn isolated_runtime<T, F, Fut>(work: F) -> Result<T, String>
where
    T: Send,
    F: FnOnce() -> Fut + Send,
    Fut: Future<Output = Result<T, String>> + Send,
{
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?
                    .block_on(work())
            })
            .join()
            .unwrap_or_else(|_| Err("tile runtime panicked; input retained for recovery".into()))
    })
}
fn decode(input: &[u8]) -> Result<Prepared, String> {
    if input.len() > MAX_BYTES {
        return Err("prepared tile input exceeds 64 MiB".into());
    }
    ds_tile_runtime::Prepared::decode(input).map_err(|e| e.to_string())
}
pub fn submit(path: &Path, admission: &Admission<'_>, input: &[u8]) -> Result<Job, SubmitError> {
    let request = decode(input).map_err(SubmitError::Invalid)?;
    submit_validated(
        path,
        admission,
        input,
        EngineKind::TilesPrepared,
        INPUT_TAG,
        execution_context::TILE_PROCESSING,
        Some(&request.project),
    )
}

pub(crate) fn execute(
    database: &Path,
    input: &[u8],
    job: &Job,
    cancelled: &CancelToken,
) -> Result<Vec<u8>, String> {
    if job.input_tag != INPUT_TAG {
        return Err("durable tile input tag mismatch".into());
    }
    let request = decode(input)?;
    let context = job
        .context
        .as_ref()
        .ok_or("tile job has no execution context")?;
    if request.project != context.project {
        return Err("tile project differs from admitted context".into());
    }
    let state = database.parent().ok_or("tile state directory is missing")?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    // Linux packaging installs the executable in /usr/bin and its resources
    // under the compile-time product root. Never infer /usr/bin/tiling-tools.
    let resources = option_env!("DS_NATIVE_CLIENT_PRODUCT_ROOT")
        .map(PathBuf::from)
        .or_else(|| executable.parent().map(Path::to_path_buf))
        .ok_or("native resource directory is missing")?;
    isolated_runtime(|| async {
        let cancel = Cancel::default();
        let compute = async {
            let tools = Tools::resolve(&resources, &cancel).await?;
            let runtime = Runtime::new(state.join("tile-cache"), tools)?;
            let work = tempfile::tempdir_in(state)?;
            let mut layers = BTreeMap::new();
            for (name, bytes) in &request.layers {
                let path = work.path().join(format!("{name}.geojsonl"));
                std::fs::write(&path, bytes)?;
                layers.insert(name.clone(), path);
            }
            let result = runtime
                .build(
                    &Request {
                        scope: Scope {
                            authority: format!("{}:{}", job.lane, job.owner),
                            project: context.project.clone(),
                        },
                        layers,
                        options: request.options,
                        force: request.force,
                    },
                    &cancel,
                )
                .await?;
            // Local-only receipt: there is no implied online publication. The
            // artifact can be read through this job's scoped result endpoint.
            match result {
                Some(artifact) => {
                    let bytes = std::fs::read(&artifact.path)?;
                    if digest(&bytes) != artifact.receipt.sha256 {
                        anyhow::bail!("cached tile archive changed before result retention");
                    }
                    Ok(serde_json::to_vec(&serde_json::json!({
                        "schema":"ds.tiles.result/v1", "project": context.project,
                        "status":"ready_local", "cache_hit":artifact.cache_hit,
                        "artifact":artifact.receipt,
                        "pmtiles_base64":base64::engine::general_purpose::STANDARD.encode(bytes)
                    }))?)
                }
                None => Ok(serde_json::to_vec(
                    &serde_json::json!({"schema":"ds.tiles.result/v1", "project":context.project,"status":"empty_local"}),
                )?),
            }
        };
        let monitor = async {
            loop {
                if cancelled.is_cancelled() {
                    cancel.cancel();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        tokio::pin!(compute);
        tokio::select! {
            result = &mut compute => result.map_err(|e: anyhow::Error| e.to_string()),
            _ = monitor => compute.await.map_err(|e: anyhow::Error| e.to_string()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_runtime_is_safe_when_called_from_a_tokio_worker() {
        let caller = std::thread::current().id();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("outer runtime");
        let worker = runtime
            .block_on(async { isolated_runtime(|| async { Ok(std::thread::current().id()) }) })
            .expect("the isolated runtime completes");
        assert_ne!(caller, worker);
    }

    #[test]
    fn tile_runtime_contains_a_worker_panic() {
        let error = isolated_runtime(|| async {
            panic!("fixture panic");
            #[allow(unreachable_code)]
            Ok::<(), String>(())
        })
        .expect_err("the panic is contained");
        assert_eq!(error, "tile runtime panicked; input retained for recovery");
    }
}
