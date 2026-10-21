//! Complete saved-room acquisition shared by native consumers. The host supplies
//! authenticated transport and already-observed heads; this owner never probes.
use super::*;
use ds_command_kernel::pinned_context::{self, PlanRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomSource {
    Held,
    Fetched,
}

#[derive(Debug)]
pub struct AcquiredRoom {
    pub room: Room,
    pub source: RoomSource,
    /// A read may still feed a report if its durable cache write failed.
    pub persistence_error: Option<String>,
}

#[derive(Debug)]
pub enum AcquisitionError<E> {
    Read(E),
    Unavailable,
    Invalid(String),
    Superseded,
    Store(String),
}

fn digest_matches(room: &Room) -> bool {
    room.content_digest.as_deref().is_some_and(|expected| {
        ds_command_kernel::report_export::jcs::layers_content_digest(&room.layers)
            .is_ok_and(|actual| actual == expected)
    })
}

fn needs_read(
    name: &str,
    room: Option<&Room>,
    heads: &[Value],
    force: bool,
) -> Result<bool, String> {
    let held: Vec<Value> = room
        .into_iter()
        .map(|room| {
            json!({
                "name": name, "version": room.version, "complete": true,
                "layers": layer_counts(&room.layers),
            })
        })
        .collect();
    let request: PlanRequest = serde_json::from_value(json!({
        "schema": pinned_context::SCHEMA, "pins": [name],
        "held": held, "heads": heads, "force": force,
    }))
    .map_err(|error| error.to_string())?;
    Ok(!pinned_context::plan(request)?.fetch.is_empty())
}

/// Consume a saved room under explicit authority. The caller admits project
/// access before calling and retains its identity/project fence in `read`.
/// `None` from transport means unavailable, never permission to use stale bytes.
/// Per-room file locking shares overlapping native reads, including across
/// processes. The final room/index lock compares the captured copy atomically.
pub fn acquire<E>(
    root: &Path,
    scope: &Scope,
    name: &str,
    heads: &[Value],
    force: bool,
    read: impl FnOnce() -> Result<Option<Room>, E>,
) -> Result<AcquiredRoom, AcquisitionError<E>> {
    if scope.principal.trim().is_empty()
        || scope.project.trim().is_empty()
        || name.trim().is_empty()
    {
        return Err(AcquisitionError::Invalid(
            "room acquisition requires principal, project and transformer".into(),
        ));
    }
    let captured = super::room(root, scope, name).ok().flatten();
    let path = room_dir(root, scope).join(format!(
        ".acquire-{:x}.lock",
        Sha256::digest(name.as_bytes())
    ));
    let _flight = ds_layer_store::exclusive_lock(&path)
        .map_err(|error| AcquisitionError::Store(error.to_string()))?;
    let current = super::room(root, scope, name).ok().flatten();
    // A force caller which waited for another acquisition joins its new room;
    // a subsequent explicit Refresh still reads again.
    let force = force && current == captured;
    if let Some(room) = current.as_ref().filter(|room| digest_matches(room)) {
        if !needs_read(name, Some(room), heads, force).map_err(AcquisitionError::Invalid)? {
            return Ok(AcquiredRoom {
                room: room.clone(),
                source: RoomSource::Held,
                persistence_error: None,
            });
        }
    }
    let room = read()
        .map_err(AcquisitionError::Read)?
        .ok_or(AcquisitionError::Unavailable)?;
    if room.transformer != name || room.content_digest.is_some() && !digest_matches(&room) {
        return Err(AcquisitionError::Invalid(
            "saved room identity or content digest does not match its layers".into(),
        ));
    }
    if needs_read(name, Some(&room), heads, false).map_err(AcquisitionError::Invalid)? {
        return Err(AcquisitionError::Superseded);
    }
    let persistence_error = match lock(root, scope) {
        Ok(_lock) => {
            if super::room(root, scope, name).ok().flatten() != current {
                return Err(AcquisitionError::Superseded);
            }
            hold_room_locked(root, scope, &room).err()
        }
        Err(error) => Some(error),
    };
    Ok(AcquiredRoom {
        room,
        source: RoomSource::Fetched,
        persistence_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn saved(version: i64) -> Room {
        let mut room = Room {
            transformer: "tx".into(),
            version: Some(version),
            content_digest: None,
            layers: BTreeMap::from([(
                "lv_poles".into(),
                json!({"features":[{"properties":{"nested":{"kept":true}}}]}),
            )]),
        };
        room.content_digest = Some(
            ds_command_kernel::report_export::jcs::layers_content_digest(&room.layers).unwrap(),
        );
        room
    }
    fn scope() -> Scope {
        Scope {
            principal: "owner".into(),
            project: "project".into(),
        }
    }
    #[test]
    fn cold_warm_and_changed_head_read_only_what_changed() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        let cold =
            acquire::<()>(dir.path(), &scope, "tx", &[], false, || Ok(Some(saved(7)))).unwrap();
        assert_eq!(cold.source, RoomSource::Fetched);
        let warm = acquire::<()>(dir.path(), &scope, "tx", &[], false, || {
            panic!("unchanged room downloaded")
        })
        .unwrap();
        assert_eq!(warm.source, RoomSource::Held);
        assert_eq!(warm.room, cold.room);
        let changed = acquire::<()>(
            dir.path(),
            &scope,
            "tx",
            &[json!({"name":"tx","version":8})],
            false,
            || Ok(Some(saved(8))),
        )
        .unwrap();
        assert_eq!(changed.source, RoomSource::Fetched);
    }
    #[test]
    fn rejects_old_answers_and_concurrent_room_changes() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        assert!(matches!(
            acquire::<()>(
                dir.path(),
                &scope,
                "tx",
                &[json!({"name":"tx","version":8})],
                false,
                || Ok(Some(saved(7)))
            ),
            Err(AcquisitionError::Superseded)
        ));
        assert!(room(dir.path(), &scope, "tx").unwrap().is_none());
        let result = acquire::<()>(dir.path(), &scope, "tx", &[], false, || {
            hold_room(dir.path(), &scope, &saved(9)).unwrap();
            Ok(Some(saved(8)))
        });
        assert!(matches!(result, Err(AcquisitionError::Superseded)));
        assert_eq!(
            room(dir.path(), &scope, "tx").unwrap().unwrap().version,
            Some(9)
        );
    }
    #[test]
    fn invalid_and_unavailable_reads_never_bless_or_replace_old_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let scope = scope();
        hold_room(dir.path(), &scope, &saved(7)).unwrap();
        let heads = [json!({"name":"tx","version":8})];
        assert!(matches!(
            acquire::<()>(dir.path(), &scope, "tx", &heads, false, || Ok(None)),
            Err(AcquisitionError::Unavailable)
        ));
        let mut invalid = saved(8);
        invalid.content_digest = Some("incorrect".into());
        assert!(matches!(
            acquire::<()>(dir.path(), &scope, "tx", &heads, false, || Ok(Some(
                invalid
            ))),
            Err(AcquisitionError::Invalid(_))
        ));
        assert_eq!(room(dir.path(), &scope, "tx").unwrap().unwrap(), saved(7));
    }
    #[test]
    fn project_and_principal_isolation_and_class_inventory_are_not_bypassed() {
        let dir = tempfile::tempdir().unwrap();
        let owner = scope();
        hold_room(dir.path(), &owner, &saved(7)).unwrap();
        for isolated in [
            Scope {
                principal: "other".into(),
                ..scope()
            },
            Scope {
                project: "other".into(),
                ..scope()
            },
        ] {
            assert!(matches!(
                acquire::<()>(dir.path(), &isolated, "tx", &[], false, || Ok(None)),
                Err(AcquisitionError::Unavailable)
            ));
        }
        let head = [json!({"name":"tx","version":7,"layers":{"lv_poles":2}})];
        assert!(matches!(
            acquire::<()>(dir.path(), &owner, "tx", &head, false, || Ok(None)),
            Err(AcquisitionError::Unavailable)
        ));
        assert_eq!(room(dir.path(), &owner, "tx").unwrap().unwrap(), saved(7));
    }
    #[test]
    fn overlapping_native_consumers_share_one_read() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Barrier,
        };
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(2));
        std::thread::scope(|threads| {
            for _ in 0..2 {
                let calls = calls.clone();
                let barrier = barrier.clone();
                let path = dir.path();
                threads.spawn(move || {
                    barrier.wait();
                    let room = acquire::<()>(path, &scope(), "tx", &[], false, || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(Some(saved(7)))
                    })
                    .unwrap();
                    assert_eq!(room.room, saved(7));
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
