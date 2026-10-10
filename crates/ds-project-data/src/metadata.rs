//! Last authenticated project observations. A read is not a freshness probe
//! or an authorization grant. Navigation/Refresh replaces an observation;
//! writes and push invalidations remove it. No timer, TTL or background IO.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "ds.project-metadata/v1";
const MAX_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scope {
    pub principal: String,
    pub lane: String,
    pub audience: String,
    pub project: String,
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Styles,
    TransformerStatus,
}
impl Kind {
    fn file(self) -> &'static str {
        match self {
            Self::Styles => "styles.json",
            Self::TransformerStatus => "transformer-status.json",
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    schema: String,
    scope: Scope,
    sha256: String,
    document: Value,
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_vec(value)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|_| "project metadata could not be encoded".into())
}
fn path(root: &Path, scope: &Scope, kind: Kind) -> Result<PathBuf, String> {
    if [
        &scope.principal,
        &scope.lane,
        &scope.audience,
        &scope.project,
    ]
    .iter()
    .any(|s| s.is_empty() || s.len() > 500 || s.trim() != *s)
    {
        return Err("project metadata needs a complete identity and project fence".into());
    }
    Ok(root
        .join("project-metadata/v1")
        .join(digest(&(&scope.principal, &scope.lane, &scope.audience))?)
        .join(digest(&scope.project)?)
        .join(kind.file()))
}
fn read(path: &Path, scope: &Scope) -> Result<Option<Value>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("project metadata could not be read".into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "project metadata could not be read")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_slice::<Observation>(&bytes) else {
        return Ok(None);
    };
    if value.schema != SCHEMA || value.scope != *scope || value.sha256 != digest(&value.document)? {
        return Ok(None);
    }
    Ok(Some(value.document))
}
fn lock(path: &Path) -> Result<fs::File, String> {
    ds_layer_store::exclusive_lock(&path.with_extension("lock"))
}

#[derive(Debug)]
pub enum Error<E> {
    Read(E),
    Store(String),
}

/// Share a cold read across processes. Explicit Refresh always reads again.
/// A failed explicit read never falls back to, or leaves admissible, old data.
pub fn acquire<E>(
    root: &Path,
    scope: &Scope,
    kind: Kind,
    refresh: bool,
    fetch: impl FnOnce() -> Result<Value, E>,
) -> Result<(Value, bool), Error<E>> {
    let path = path(root, scope, kind).map_err(Error::Store)?;
    let _lock = lock(&path).map_err(Error::Store)?;
    if !refresh {
        if let Some(value) = read(&path, scope).map_err(Error::Store)? {
            return Ok((value, true));
        }
    }
    let document = match fetch() {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&path);
            return Err(Error::Read(error));
        }
    };
    write(&path, scope, &document).map_err(Error::Store)?;
    Ok((document, false))
}

fn write(path: &Path, scope: &Scope, document: &Value) -> Result<(), String> {
    let observation = Observation {
        schema: SCHEMA.into(),
        scope: scope.clone(),
        sha256: digest(document)?,
        document: document.clone(),
    };
    let bytes =
        serde_json::to_vec(&observation).map_err(|_| "project metadata could not be encoded")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("project metadata exceeds its byte bound".into());
    }
    ds_layer_store::private::write_atomic(path, bytes)
        .map_err(|_| "project metadata could not be kept".into())
}

/// Record an already authenticated full observation. Targeted status reads
/// cannot establish whole-project completeness and must not use this method.
pub fn observe(root: &Path, scope: &Scope, kind: Kind, document: &Value) -> Result<(), String> {
    let path = path(root, scope, kind)?;
    let _lock = lock(&path)?;
    write(&path, scope, document)
}

pub fn invalidate(root: &Path, scope: &Scope, kind: Kind) -> Result<(), String> {
    let path = path(root, scope, kind)?;
    let _lock = lock(&path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("project metadata could not be invalidated".into()),
    }
}

/// Shared style edits can affect every project catalogue for this account.
pub fn invalidate_all_styles(root: &Path, scope: &Scope) -> Result<(), String> {
    let example = path(root, scope, Kind::Styles)?;
    let parent = example
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid metadata path")?;
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("metadata observations could not be invalidated".into()),
    };
    for (index, entry) in entries.enumerate() {
        if index >= 10_000 {
            return Err("too many style observations to invalidate in one operation".into());
        }
        let entry = entry.map_err(|_| "metadata observation could not be invalidated")?;
        if !entry
            .file_type()
            .map_err(|_| "metadata observation could not be invalidated")?
            .is_dir()
        {
            continue;
        }
        let path = entry.path().join(Kind::Styles.file());
        let _lock = lock(&path)?;
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("style observation could not be invalidated".into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    fn scope() -> Scope {
        Scope {
            principal: "u".into(),
            lane: "canary".into(),
            audience: "a".into(),
            project: "p".into(),
        }
    }
    #[test]
    fn warm_reads_do_no_network_and_refresh_and_invalidation_fetch_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        let mut reads = 0;
        for (refresh, expected_reads, expected_cached) in [
            (false, 1, false),
            (false, 1, true),
            (true, 2, false),
            (false, 2, true),
        ] {
            let (_, cached) = acquire(dir.path(), &scope, Kind::Styles, refresh, || {
                reads += 1;
                Ok::<_, ()>(json!({"revision":reads}))
            })
            .unwrap();
            assert_eq!((reads, cached), (expected_reads, expected_cached));
        }
        invalidate(dir.path(), &scope, Kind::Styles).unwrap();
        let (_, cached) = acquire(dir.path(), &scope, Kind::Styles, false, || {
            reads += 1;
            Ok::<_, ()>(json!({"revision":reads}))
        })
        .unwrap();
        assert_eq!((reads, cached), (3, false));
    }
    #[test]
    fn scopes_and_kinds_are_isolated_and_empty_observations_are_reused() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        observe(dir.path(), &scope, Kind::TransformerStatus, &json!([])).unwrap();
        assert_eq!(
            acquire(
                dir.path(),
                &scope,
                Kind::TransformerStatus,
                false,
                || Err::<Value, _>(())
            )
            .unwrap(),
            (json!([]), true)
        );
        for other in [
            Scope {
                principal: "v".into(),
                ..scope.clone()
            },
            Scope {
                lane: "stable".into(),
                ..scope.clone()
            },
            Scope {
                audience: "b".into(),
                ..scope.clone()
            },
            Scope {
                project: "q".into(),
                ..scope.clone()
            },
        ] {
            assert!(matches!(
                acquire(
                    dir.path(),
                    &other,
                    Kind::TransformerStatus,
                    false,
                    || Err::<Value, _>(())
                ),
                Err(Error::Read(()))
            ));
        }
        assert!(matches!(
            acquire(dir.path(), &scope, Kind::Styles, false, || Err::<Value, _>(
                ()
            )),
            Err(Error::Read(()))
        ));
    }
    #[test]
    fn changed_style_invalidates_all_projects_but_keeps_status_and_other_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        let second = Scope {
            project: "q".into(),
            ..scope.clone()
        };
        let other = Scope {
            principal: "other".into(),
            ..scope.clone()
        };
        for scope in [&scope, &second, &other] {
            observe(dir.path(), scope, Kind::Styles, &json!({"revision":1})).unwrap();
        }
        observe(dir.path(), &scope, Kind::TransformerStatus, &json!([])).unwrap();
        invalidate_all_styles(dir.path(), &scope).unwrap();
        for scope in [&scope, &second] {
            assert!(matches!(
                acquire(dir.path(), scope, Kind::Styles, false, || Err::<Value, _>(
                    ()
                )),
                Err(Error::Read(()))
            ));
        }
        assert!(
            acquire(dir.path(), &other, Kind::Styles, false, || Err::<Value, _>(
                ()
            ))
            .unwrap()
            .1
        );
        assert!(
            acquire(
                dir.path(),
                &scope,
                Kind::TransformerStatus,
                false,
                || Err::<Value, _>(())
            )
            .unwrap()
            .1
        );
    }
    #[test]
    fn corrupt_observations_refresh_instead_of_blessing_unverified_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        observe(dir.path(), &scope, Kind::Styles, &json!({"revision":1})).unwrap();
        let path = path(dir.path(), &scope, Kind::Styles).unwrap();
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["document"]["revision"] = json!(2);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            acquire(dir.path(), &scope, Kind::Styles, false, || Ok::<_, ()>(
                json!({"revision":3})
            ))
            .unwrap(),
            (json!({"revision":3}), false)
        );
    }
    #[test]
    fn concurrent_consumers_share_one_fetch_and_failures_do_not_serve_stale() {
        let dir = tempfile::tempdir().unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let reads = reads.clone();
                let root = dir.path();
                threads.spawn(move || {
                    acquire(root, &scope(), Kind::Styles, false, || {
                        reads.fetch_add(1, Ordering::SeqCst);
                        Ok::<_, ()>(json!({"revision":1}))
                    })
                    .unwrap();
                });
            }
        });
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(matches!(
            acquire(
                dir.path(),
                &scope(),
                Kind::Styles,
                true,
                || Err::<Value, _>("revoked")
            ),
            Err(Error::Read("revoked"))
        ));
        assert!(matches!(
            acquire(
                dir.path(),
                &scope(),
                Kind::Styles,
                false,
                || Err::<Value, _>("offline")
            ),
            Err(Error::Read("offline"))
        ));
    }
}
