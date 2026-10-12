//! Private retained project-head metadata, shared by native CLI and Desktop.
//! Invalidation writes a marker only. The consuming reader coalesces and may
//! acquire missing/invalidated metadata; offline reads retain exact old pins.
use ds_command_kernel::{
    printing::mv_models,
    printing::retained_reads::{Demand, Failure as ReadFailure, Host, Reads},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const SCHEMA: &str = "ds.grid-project-catalogue/v1";
const MAX_BYTES: u64 = 8 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub principal: String,
    pub project: String,
    pub lane: String,
    pub audience_sha256: String,
}
impl Scope {
    fn key(&self) -> Result<String, String> {
        if self.principal.is_empty()
            || self.principal.len() > 256
            || self.project.is_empty()
            || self.project.len() > 256
            || self.lane.is_empty()
            || !ds_command_kernel::printing::preview_progress::digest(&self.audience_sha256)
        {
            return Err("invalid catalogue scope".into());
        }
        Ok(hash(&serde_json::to_vec(self).map_err(|e| e.to_string())?))
    }
}
#[derive(Debug)]
pub enum Failure<E> {
    Admission(&'static str),
    AcquisitionRequired,
    AccessRevoked,
    Storage(String),
    Remote(E),
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn directory(root: &Path, scope: &Scope) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err("catalogue root must be absolute".into());
    }
    Ok(root.join("grid-project-catalogue").join(scope.key()?))
}
fn bytes(path: &Path) -> Option<Vec<u8>> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    crate::private::open_options()
        .read(true)
        .open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_BYTES).then_some(bytes)
}
fn write(path: &Path, value: &[u8]) -> Result<(), String> {
    crate::private::write_atomic(path, value).map_err(|e| e.to_string())
}
fn owned_directory(directory: &Path) -> Result<(), String> {
    let parent = directory
        .parent()
        .ok_or("catalogue has no private parent")?;
    for path in [parent, directory] {
        if fs::symlink_metadata(path).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink()) {
            return Err("catalogue directory is foreign or a link".into());
        }
        crate::private::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    let lock = directory.join("catalogue.lock");
    if fs::symlink_metadata(&lock).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err("catalogue lock is foreign or a link".into());
    }
    Ok(())
}
struct Store<'a, E, L, A> {
    directory: PathBuf,
    scope: &'a Scope,
    gate: Option<fs::File>,
    lock_error: Option<String>,
    load: Option<L>,
    unauthorized: A,
    _error: std::marker::PhantomData<E>,
}
impl<E, L, A> Store<'_, E, L, A> {
    fn ensure_lock(&mut self) -> Result<(), String> {
        if self.gate.is_none() {
            owned_directory(&self.directory)?;
            self.gate = Some(crate::exclusive_lock(
                &self.directory.join("catalogue.lock"),
            )?);
        }
        Ok(())
    }
    fn generation(&self) -> Vec<u8> {
        bytes(&self.directory.join("invalidation")).unwrap_or_default()
    }
    fn revoked(&self) -> bool {
        self.directory.join("revoked").exists()
    }
}
impl<E, L: FnOnce() -> Result<Value, E>, A: Fn(&E) -> bool> Host<Value> for Store<'_, E, L, A> {
    type Error = Failure<E>;
    fn retained(&mut self, _key: &str) -> Option<(Value, bool)> {
        if let Err(e) = self.ensure_lock() {
            self.lock_error = Some(e);
            return None;
        }
        if self.revoked() {
            return None;
        }
        let held: Value =
            serde_json::from_slice(&bytes(&self.directory.join("catalogue.json"))?).ok()?;
        let data = held["data"].clone();
        if held["schema"] != SCHEMA
            || held["scope"] != serde_json::to_value(self.scope).ok()?
            || held["sha256"] != hash(&serde_json::to_vec(&data).ok()?)
            || mv_models::project(&self.scope.project, &self.scope.principal, &data, None).is_err()
        {
            return None;
        }
        Some((data, held["generation"] != json!(self.generation())))
    }
    fn invalidate(&mut self) -> Result<(), Self::Error> {
        self.ensure_lock().map_err(Failure::Storage)?;
        write(
            &self.directory.join("invalidation"),
            uuid::Uuid::new_v4().as_bytes(),
        )
        .map_err(Failure::Storage)
    }
    fn acquire(&mut self, _key: &str) -> Result<Value, Self::Error> {
        if let Some(error) = self.lock_error.take() {
            return Err(Failure::Storage(error));
        }
        self.ensure_lock().map_err(Failure::Storage)?;
        match self.load.take().expect("one consuming acquisition")() {
            Ok(value) => Ok(value),
            Err(error) => {
                if (self.unauthorized)(&error) {
                    write(&self.directory.join("revoked"), b"access refused")
                        .map_err(Failure::Storage)?;
                }
                Err(Failure::Remote(error))
            }
        }
    }
    fn retain(&mut self, _key: &str, data: &Value) -> Result<(), Self::Error> {
        mv_models::project(&self.scope.project, &self.scope.principal, data, None)
            .map_err(|e| Failure::Storage(e.into()))?;
        let held = json!({"schema":SCHEMA,"scope":self.scope,"data":data,"generation":self.generation(),"sha256":hash(&serde_json::to_vec(data).map_err(|e|Failure::Storage(e.to_string()))?)});
        let held = serde_json::to_vec(&held).map_err(|e| Failure::Storage(e.to_string()))?;
        if held.len() as u64 > MAX_BYTES {
            return Err(Failure::Storage(
                "bounded project catalogue exceeded8MiB".into(),
            ));
        }
        write(&self.directory.join("catalogue.json"), &held).map_err(Failure::Storage)?;
        if self.revoked() {
            fs::remove_file(self.directory.join("revoked"))
                .map_err(|e| Failure::Storage(e.to_string()))?;
        }
        Ok(())
    }
}
static READS: OnceLock<Reads> = OnceLock::new();
pub fn read<E>(
    root: &Path,
    scope: &Scope,
    demand: Demand,
    load: impl FnOnce() -> Result<Value, E>,
    unauthorized: impl Fn(&E) -> bool,
) -> Result<Value, Failure<E>> {
    let directory = directory(root, scope).map_err(Failure::Storage)?;
    let mut store = Store {
        directory,
        scope,
        gate: None,
        lock_error: None,
        load: Some(load),
        unauthorized,
        _error: std::marker::PhantomData,
    };
    if store.revoked() && !(demand.refresh && demand.online && demand.acquire) {
        return Err(Failure::AccessRevoked);
    }
    READS
        .get_or_init(Reads::default)
        .read(
            &hash(store.directory.as_os_str().as_encoded_bytes()),
            "heads",
            demand,
            &mut store,
        )
        .map_err(|e| match e {
            ReadFailure::AcquisitionRequired => Failure::AcquisitionRequired,
            ReadFailure::GateUnavailable => {
                Failure::Storage("catalogue coordinator unavailable".into())
            }
            ReadFailure::Unavailable(e) => e,
        })
}
pub fn invalidate(root: &Path, scope: &Scope) -> Result<(), String> {
    let directory = directory(root, scope)?;
    owned_directory(&directory)?;
    let _gate = crate::exclusive_lock(&directory.join("catalogue.lock"))?;
    write(
        &directory.join("invalidation"),
        uuid::Uuid::new_v4().as_bytes(),
    )
}
/// One consuming catalogue/control path for both native hosts. Context and
/// selected-source admission precede cache or transport access.
pub fn consume<E>(
    root: &Path,
    scope: &Scope,
    demand: Demand,
    inventory: Option<ds_command_kernel::grid_held_models::Inventory>,
    source: Option<mv_models::SourceBinding>,
    load: impl FnOnce() -> Result<Value, E>,
    unauthorized: impl Fn(&E) -> bool,
) -> Result<Value, Failure<E>> {
    if let Some(source) = &source {
        mv_models::admit_context(&scope.project, source).map_err(Failure::Admission)?;
    }
    let inventory = inventory
        .map(|inventory| {
            ds_command_kernel::grid_held_models::admit(
                serde_json::to_value(inventory)
                    .map_err(|_| Failure::Admission(mv_models::INVALID))?,
            )
            .map_err(|_| Failure::Admission(mv_models::STALE))
        })
        .transpose()?;
    if inventory.as_ref().is_some_and(|v| {
        v.context.project.as_deref() != Some(&scope.project)
            || v.context.account.as_deref() != Some(&scope.principal)
    }) {
        return Err(Failure::Admission(mv_models::STALE));
    }
    let listing = read(root, scope, demand, load, unauthorized)?;
    let mut catalogue = mv_models::project(&scope.project, &scope.principal, &listing, inventory)
        .map_err(Failure::Admission)?;
    if let Some(source) = source {
        mv_models::admit_source(&scope.project, &catalogue, &source, &source.worker_revision)
            .map_err(Failure::Admission)?;
        catalogue["selected_source"] =
            serde_json::to_value(source).map_err(|_| Failure::Admission(mv_models::INVALID))?;
    }
    Ok(catalogue)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    fn scope(project: &str) -> Scope {
        Scope {
            principal: "owner".into(),
            project: project.into(),
            lane: "local".into(),
            audience_sha256: "a".repeat(64),
        }
    }
    fn answer(project: &str) -> Value {
        json!({"project":project,"models":[],"more":false})
    }
    fn demand() -> Demand {
        Demand {
            online: true,
            acquire: true,
            refresh: false,
        }
    }
    #[test]
    fn real_adapter_coalesces_warm_offline_and_unused_invalidation_without_remote_reads() {
        let root = tempfile::tempdir().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let room = scope("template");
        std::thread::scope(|threads| {
            for _ in 0..6 {
                let requests = requests.clone();
                let room = room.clone();
                let path = root.path();
                threads.spawn(move || {
                    read(
                        path,
                        &room,
                        demand(),
                        || {
                            requests.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(20));
                            Ok::<_, &str>(answer("template"))
                        },
                        |_| false,
                    )
                    .unwrap()
                });
            }
        });
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        invalidate(root.path(), &room).unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        for online in [false, false] {
            read(
                root.path(),
                &room,
                Demand {
                    online,
                    acquire: false,
                    refresh: false,
                },
                || panic!("offline performs no transport"),
                |_: &&str| false,
            )
            .unwrap();
        }
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        read(
            root.path(),
            &room,
            demand(),
            || {
                requests.fetch_add(1, Ordering::SeqCst);
                Ok::<_, &str>(answer("template"))
            },
            |_| false,
        )
        .unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert!(matches!(
            read(
                root.path(),
                &scope("foreign"),
                Demand {
                    online: false,
                    acquire: false,
                    refresh: false
                },
                || panic!("missing offline project does not acquire"),
                |_: &&str| false
            ),
            Err(Failure::AcquisitionRequired)
        ));
    }
    #[test]
    fn foreign_selected_source_refuses_before_cache_or_transport_access() {
        let root = tempfile::tempdir().unwrap();
        let scope = scope("template");
        let source = mv_models::SourceBinding {
            project_id: "foreign".into(),
            model_id: "head".into(),
            revision_id: "rev".into(),
            content_sha256: "a".repeat(64),
            version: 1,
            worker_id: "local".into(),
            worker_model_id: "native".into(),
            worker_revision: "rev:1".into(),
            history_pin: "pin".into(),
            coordinate_system: "EPSG:32736".into(),
            read_only: false,
        };
        let calls = AtomicUsize::new(0);
        assert!(matches!(
            consume(
                root.path(),
                &scope,
                demand(),
                None,
                Some(source),
                || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, &str>(answer("template"))
                },
                |_| false
            ),
            Err(Failure::Admission(mv_models::STALE))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!root.path().join("grid-project-catalogue").exists());
    }
    #[cfg(unix)]
    #[test]
    fn foreign_scope_links_are_not_followed_or_written() {
        let root = tempfile::tempdir().unwrap();
        let foreign = tempfile::tempdir().unwrap();
        let scope = scope("template");
        let owned = directory(root.path(), &scope).unwrap();
        fs::create_dir_all(owned.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(foreign.path(), &owned).unwrap();
        assert!(matches!(
            read(
                root.path(),
                &scope,
                demand(),
                || panic!("foreign path does not acquire"),
                |_: &&str| false
            ),
            Err(Failure::Storage(_))
        ));
        assert_eq!(fs::read_dir(foreign.path()).unwrap().count(), 0);
    }
    #[test]
    fn revoked_or_crossed_scope_never_reuses_a_previous_success() {
        let root = tempfile::tempdir().unwrap();
        let room = scope("template");
        read(
            root.path(),
            &room,
            demand(),
            || Ok::<_, &str>(answer("template")),
            |_| false,
        )
        .unwrap();
        assert!(matches!(
            read(
                root.path(),
                &room,
                Demand {
                    refresh: true,
                    ..demand()
                },
                || Err("unauthorized"),
                |e| *e == "unauthorized"
            ),
            Err(Failure::Remote(_))
        ));
        assert!(matches!(
            read(
                root.path(),
                &room,
                Demand {
                    online: false,
                    acquire: false,
                    refresh: false
                },
                || panic!("revoked does not acquire"),
                |_: &&str| false
            ),
            Err(Failure::AccessRevoked)
        ));
        for changed in 0..3 {
            let mut other = room.clone();
            match changed {
                0 => other.principal = "peer".into(),
                1 => other.lane = "other".into(),
                _ => other.audience_sha256 = "b".repeat(64),
            };
            assert!(matches!(
                read(
                    root.path(),
                    &other,
                    Demand {
                        online: false,
                        acquire: false,
                        refresh: false
                    },
                    || panic!("scope does not acquire"),
                    |_: &&str| false
                ),
                Err(Failure::AcquisitionRequired)
            ));
        }
    }
}
