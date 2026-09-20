//! One machine-local preference file per KIND of choice, keyed by lane, DS
//! account and project — the store family every per-host, per-project operator
//! choice lives in (the drawer's remembered visibility, the working area's
//! form selection). A new choice is a new [`Kind`] over this file shape, never
//! a new mechanism.
//!
//! What every kind gets, once: one file, one lock, atomic replacement, an
//! absent file reads as empty and creates nothing, a malformed file is refused
//! rather than repaired, a read-decide-write under one exclusive lock so two
//! concurrent transitions never replace each other's edits, an identical write
//! is not a revision, and a fence that a canary project never inherits a
//! stable project's choice and two accounts on one host never share theirs.
use fs2::FileExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub const MAX_STORE_BYTES: usize = 4 * 1024 * 1024;

/// One kind of choice: which file holds it, how its entries are bounded and
/// what its receipt calls the size of one entry.
pub struct Kind<T> {
    pub file: &'static str,
    pub lock: &'static str,
    /// The noun a refusal uses: "layer visibility", "working-area forms".
    pub noun: &'static str,
    /// Refuse an entry that exceeds the kind's bound before it is written.
    pub validate: fn(&T) -> Result<(), String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Store<T> {
    pub schema: u32,
    pub revision: u64,
    /// `<lane>/<uid>/<project>` → the entry.
    pub projects: BTreeMap<String, T>,
}
impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            schema: 1,
            revision: 0,
            projects: BTreeMap::new(),
        }
    }
}

/// The lane tokens a scope may carry: the two release lanes `ds` and the
/// installed desktop share, and `local` — the unprovisioned debug desktop's
/// own lane, which pairs with no `ds` and so never inherits or leaks a
/// release lane's choices.
pub const LANES: [&str; 3] = ["stable", "canary", "local"];

pub fn scope(noun: &str, lane: &str, uid: &str, project: &str) -> Result<String, String> {
    let ok = |value: &str| {
        !value.is_empty()
            && value.trim() == value
            && value.chars().count() <= 500
            && !value.chars().any(|c| c.is_control() || c == '/')
    };
    if !LANES.contains(&lane) || !ok(uid) || !ok(project) {
        return Err(format!(
            "{noun} needs a lane, a bounded account uid and a bounded project id"
        ));
    }
    Ok(format!("{lane}/{uid}/{project}"))
}

fn io_error(noun: &str) -> String {
    format!("local {noun} store could not be read or persisted")
}

pub fn load<T: DeserializeOwned>(kind: &Kind<T>, path: &Path) -> Result<Store<T>, String> {
    match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_STORE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| io_error(kind.noun))?;
            if bytes.len() > MAX_STORE_BYTES {
                return Err(format!("local {} store exceeds 4 MiB", kind.noun));
            }
            let store: Store<T> = serde_json::from_slice(&bytes).map_err(|_| {
                format!(
                    "local {} store is malformed; repair it before editing",
                    kind.noun
                )
            })?;
            if store.schema != 1 {
                return Err(format!(
                    "local {} store has an unsupported schema",
                    kind.noun
                ));
            }
            Ok(store)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(_) => Err(io_error(kind.noun)),
    }
}

/// The entry for one lane, account and project; `None` when none was recorded.
pub fn read_at<T: DeserializeOwned>(
    kind: &Kind<T>,
    root: &Path,
    lane: &str,
    uid: &str,
    project: &str,
) -> Result<Option<T>, String> {
    let key = scope(kind.noun, lane, uid, project)?;
    let path = root.join(kind.file);
    if !path.try_exists().map_err(|_| io_error(kind.noun))? {
        return Ok(None);
    }
    let lock = OpenOptions::new()
        .read(true)
        .open(root.join(kind.lock))
        .ok();
    if let Some(lock) = &lock {
        lock.lock_shared().map_err(|_| io_error(kind.noun))?;
    }
    Ok(load(kind, &path)?.projects.remove(&key))
}

#[derive(Debug)]
pub enum UpdateError<E> {
    Store(String),
    Transition(E),
}

/// Read, decide and persist under one lock. The transition sees the entry as
/// it is (`None` = none recorded) and returns the entry as it should be
/// (`None` = remove it) plus whatever the caller wants back. The receipt names
/// the scope, whether anything changed and the store revision after the write.
pub fn update_at<T, U, E>(
    kind: &Kind<T>,
    root: &Path,
    lane: &str,
    uid: &str,
    project: &str,
    update: impl FnOnce(Option<&T>) -> Result<(Option<T>, U), E>,
) -> Result<(Value, U), UpdateError<E>>
where
    T: Serialize + DeserializeOwned + PartialEq + Clone,
{
    let store_error = |_: std::io::Error| UpdateError::Store(io_error(kind.noun));
    let any_error = |_: &dyn std::fmt::Display| UpdateError::Store(io_error(kind.noun));
    let key = scope(kind.noun, lane, uid, project).map_err(UpdateError::Store)?;
    fs::create_dir_all(root).map_err(store_error)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(kind.lock))
        .map_err(store_error)?;
    lock.lock_exclusive().map_err(store_error)?;
    let path = root.join(kind.file);
    let mut store = load(kind, &path).map_err(UpdateError::Store)?;
    let previous = store.projects.get(&key);
    let (next, output) = update(previous).map_err(UpdateError::Transition)?;
    if let Some(entry) = &next {
        (kind.validate)(entry).map_err(UpdateError::Store)?;
    }
    let changed = store.projects.get(&key) != next.as_ref();
    if changed {
        match next {
            Some(entry) => {
                store.projects.insert(key.clone(), entry);
            }
            None => {
                store.projects.remove(&key);
            }
        }
        store.revision = store
            .revision
            .checked_add(1)
            .ok_or_else(|| UpdateError::Store(format!("{} revision exhausted", kind.noun)))?;
        let bytes = serde_json::to_vec(&store).map_err(|error| any_error(&error))?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(UpdateError::Store(format!(
                "local {} store exceeds 4 MiB",
                kind.noun
            )));
        }
        let mut staged = tempfile::NamedTempFile::new_in(root).map_err(store_error)?;
        staged.write_all(&bytes).map_err(store_error)?;
        staged.flush().map_err(store_error)?;
        staged.as_file().sync_all().map_err(store_error)?;
        staged.persist(&path).map_err(|error| any_error(&error))?;
        #[cfg(unix)]
        File::open(root)
            .and_then(|f| f.sync_all())
            .map_err(store_error)?;
    }
    Ok((
        json!({
            "scope": key,
            "changed": changed,
            "revision": store.revision,
            "persisted": "native_local",
        }),
        output,
    ))
}
