//! The hosted Web's durable store host: the same [`super::execute`] on OPFS,
//! inside the one dedicated storage worker. rusqlite runs over sqlite-wasm-rs
//! and the sahpool VFS; the worker's TypeScript only holds the cross-tab Web
//! Lock ([`LOCK`]) around one call and relays bytes in and out.
//!
//! The pool lives in its own OPFS directory under its own VFS name. The older
//! JavaScript pool (`/ds-durable-sqlite-v1`) is never opened here: that SAHPool
//! rewrites slot headers it cannot verify, and it held only two caches whose
//! contracts declare them reconstructible, so they rebuild on first refresh.
//!
//! Before-effects complete before the transaction and after-effects after its
//! commit, all under the caller's lock, so a crash in between fails closed.
use super::{effects_before, execute, Call, Effect, Identity, INVALID};
use js_sys::Uint8Array;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use sqlite_wasm_vfs::sahpool::{install, OpfsSAHPoolCfgBuilder, OpfsSAHPoolUtil};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DedicatedWorkerGlobalScope, FileSystemDirectoryHandle, FileSystemFileHandle,
    FileSystemGetDirectoryOptions, FileSystemGetFileOptions, FileSystemSyncAccessHandle,
};

pub const VFS: &str = "ds-kernel-store-v1";
pub const DIRECTORY: &str = ".ds-kernel-store-v1";
pub const DATABASE: &str = "/kernel.sqlite";
/// The cross-tab Web Lock the storage worker holds around every call.
pub const LOCK: &str = "ds-kernel-store-v1";
pub const UNAVAILABLE: &str = "durable_store_storage_unavailable";

thread_local! {
    static POOL: RefCell<Option<Rc<OpfsSAHPoolUtil>>> = const { RefCell::new(None) };
}

#[derive(Serialize)]
struct Failure {
    error: String,
    /// The storage itself failed: the worker is replaced before the next call.
    reset: bool,
}
fn failure(error: String, reset: bool) -> JsValue {
    JsValue::from_str(
        &serde_json::to_string(&Failure { error, reset }).unwrap_or_else(|_| "{}".into()),
    )
}
fn js(error: JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            error
                .dyn_ref::<js_sys::Error>()
                .map(|error| String::from(error.message()))
        })
        .unwrap_or_else(|| "a browser storage call failed".into())
}

/// Install the pool once per worker, or reacquire its handles after a pause.
async fn pool() -> Result<Rc<OpfsSAHPoolUtil>, String> {
    if let Some(pool) = POOL.with(|held| held.borrow().clone()) {
        if pool.is_paused() {
            pool.unpause_vfs().await.map_err(|e| e.to_string())?;
        }
        return Ok(pool);
    }
    let options = OpfsSAHPoolCfgBuilder::new()
        .vfs_name(VFS)
        .directory(DIRECTORY)
        .build();
    let pool = Rc::new(
        install::<sqlite_wasm_rs::WasmOsCallback>(&options, false)
            .await
            .map_err(|e| e.to_string())?,
    );
    POOL.with(|held| *held.borrow_mut() = Some(pool.clone()));
    Ok(pool)
}

async fn root() -> Result<FileSystemDirectoryHandle, String> {
    let global: DedicatedWorkerGlobalScope = js_sys::global()
        .dyn_into()
        .map_err(|_| "durable storage effects run only in the storage worker".to_string())?;
    JsFuture::from(global.navigator().storage().get_directory())
        .await
        .map_err(js)?
        .dyn_into()
        .map_err(|_| "the OPFS root is not a directory".to_string())
}
/// The file's parent directory and name, creating the directory when asked.
async fn parent(path: &str, create: bool) -> Result<Option<(FileSystemDirectoryHandle, String)>, String> {
    let mut directory = root().await?;
    let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let name = parts.pop().ok_or("an effect file has a name")?.to_string();
    for part in parts {
        let options = FileSystemGetDirectoryOptions::new();
        options.set_create(create);
        match JsFuture::from(directory.get_directory_handle_with_options(part, &options)).await {
            Ok(handle) => directory = handle.dyn_into().map_err(|_| "not a directory".to_string())?,
            Err(_) if !create => return Ok(None),
            Err(error) => return Err(js(error)),
        }
    }
    Ok(Some((directory, name)))
}
async fn write_file(path: &str, contents: &str) -> Result<(), String> {
    let (directory, name) = parent(path, true).await?.ok_or("effect directory missing")?;
    let options = FileSystemGetFileOptions::new();
    options.set_create(true);
    let file: FileSystemFileHandle = JsFuture::from(directory.get_file_handle_with_options(&name, &options))
        .await
        .map_err(js)?
        .dyn_into()
        .map_err(|_| "not a file".to_string())?;
    let handle: FileSystemSyncAccessHandle = JsFuture::from(file.create_sync_access_handle())
        .await
        .map_err(js)?
        .dyn_into()
        .map_err(|_| "no sync access handle".to_string())?;
    let written = (|| {
        handle.truncate_with_u32(0).map_err(js)?;
        handle.write_with_u8_array(contents.as_bytes()).map_err(js)?;
        handle.flush().map_err(js)
    })();
    handle.close();
    written
}
async fn delete_file(path: &str) -> Result<(), String> {
    let Some((directory, name)) = parent(path, false).await? else {
        return Ok(());
    };
    match JsFuture::from(directory.remove_entry(&name)).await {
        Ok(_) => Ok(()),
        Err(error)
            if error
                .dyn_ref::<web_sys::DomException>()
                .is_some_and(|exception| exception.name() == "NotFoundError") =>
        {
            Ok(())
        }
        Err(error) => Err(js(error)),
    }
}
async fn run(effects: &[Effect]) -> Result<(), String> {
    for effect in effects {
        match effect {
            Effect::WriteFile { file, contents } => write_file(file.path(), contents).await?,
            Effect::DeleteFile { file } => delete_file(file.path()).await?,
        }
    }
    Ok(())
}

/// The worker's name for its one cross-tab lock, so its TypeScript keeps no
/// storage identity of its own.
#[wasm_bindgen(js_name = durableStoreLock)]
pub fn lock() -> String {
    LOCK.into()
}

/// One answer across the worker boundary: the answer JSON and its bytes.
#[wasm_bindgen]
pub struct DurableStoreReply {
    answer: String,
    bytes: Option<Vec<u8>>,
}
#[wasm_bindgen]
impl DurableStoreReply {
    #[wasm_bindgen(getter)]
    pub fn answer(&self) -> String {
        self.answer.clone()
    }
    /// The answer's payload bytes, or `undefined`.
    #[wasm_bindgen(getter)]
    pub fn bytes(&self) -> Option<Uint8Array> {
        self.bytes.as_deref().map(Uint8Array::from)
    }
}

/// One call in one transaction on OPFS. The caller holds [`LOCK`]; the pool
/// releases its OPFS handles before this returns, so another tab's worker can
/// take the lock next. `call` is the Call JSON, `identity` the Identity JSON
/// the host binds (absent for the statement-protocol stores). An error is the
/// JSON `{error, reset}`.
#[wasm_bindgen(js_name = durableStoreTransaction)]
pub async fn transaction(
    call: String,
    identity: Option<String>,
    bytes: Option<Vec<u8>>,
) -> Result<DurableStoreReply, JsValue> {
    let call: Call =
        serde_json::from_str(&call).map_err(|e| failure(format!("{INVALID}: {e}"), false))?;
    let identity = identity
        .map(|text| serde_json::from_str::<Identity>(&text))
        .transpose()
        .map_err(|e| failure(format!("{INVALID}: {e}"), false))?;
    let pool = pool()
        .await
        .map_err(|e| failure(format!("{UNAVAILABLE}: {e}"), true))?;
    let before = effects_before(&call);
    let answer = match run(&before).await {
        Err(error) => Err((format!("{UNAVAILABLE}: {error}"), false)),
        Ok(()) => Connection::open_with_flags_and_vfs(
            DATABASE,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            VFS,
        )
        .map_err(|e| (format!("{UNAVAILABLE}: {e}"), true))
        .and_then(|mut connection| {
            execute(&mut connection, identity.as_ref(), call, bytes).map_err(|e| (e, false))
        }),
    };
    // The connection closed when the closure returned; release every handle.
    let paused = pool.pause_vfs();
    let answer = match (answer, paused) {
        (_, Err(error)) => return Err(failure(format!("{UNAVAILABLE}: {error}"), true)),
        (Err((error, reset)), Ok(())) => return Err(failure(error, reset)),
        (Ok(answer), Ok(())) => answer,
    };
    // Committed: the after-effects run under the same lock.
    run(&answer.effects)
        .await
        .map_err(|e| failure(format!("{UNAVAILABLE}: {e}"), false))?;
    Ok(DurableStoreReply {
        answer: serde_json::to_string(&answer).map_err(|e| failure(e.to_string(), false))?,
        bytes: answer.bytes,
    })
}
