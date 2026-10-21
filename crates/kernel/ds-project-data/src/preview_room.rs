//! Local preview results in the existing account/project report hold. A caller
//! admits authority and resolves exact input bytes before consuming this room.
//! Navigation releases a view, never the native render or its retained result.
use ds_command_kernel::printing::retained_reads::{Demand, Failure, Host, Reads};
use ds_command_kernel::project_dataset_cache::Scope;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::OnceLock;

const SCHEMA: &str = "ds.local-print-preview/v1";
// One latest result per subject, bounded independently of the number of drafts.
// The renderer admits a 96 MiB SVG. JSON may expand one control character to
// six bytes; leave room for the accompanying native diagnostics and receipt.
const MAX_BYTES: u64 = 640 * 1024 * 1024;
static READS: OnceLock<Reads> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct Room {
    pub root: PathBuf,
    pub scope: Scope,
    pub lane: String,
    pub credential_audience_sha256: String,
}

#[derive(Debug)]
pub enum Error<E> {
    Render(E),
    Store(String),
}

struct Disk<'a, T, F> {
    path: PathBuf,
    render: Option<F>,
    marker: std::marker::PhantomData<&'a T>,
}
impl<T, E, F> Host<T> for Disk<'_, T, F>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce() -> Result<T, E>,
{
    type Error = Error<E>;
    fn retained(&mut self, key: &str) -> Option<(T, bool)> {
        if std::fs::metadata(&self.path).ok()?.len() > MAX_BYTES {
            return None;
        }
        let document: Value = serde_json::from_slice(&std::fs::read(&self.path).ok()?).ok()?;
        if document["schema"] != SCHEMA || document["input_sha256"] != key {
            return None;
        }
        let bytes = serde_json::to_vec(&document["value"]).ok()?;
        if document["value_sha256"] != format!("{:x}", Sha256::digest(bytes)) {
            return None;
        }
        serde_json::from_value(document["value"].clone())
            .ok()
            .map(|value| (value, false))
    }
    fn invalidate(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn acquire(&mut self, _key: &str) -> Result<T, Self::Error> {
        self.render.take().expect("one native preview acquisition")().map_err(Error::Render)
    }
    fn retain(&mut self, key: &str, value: &T) -> Result<(), Self::Error> {
        let value = serde_json::to_value(value).map_err(|e| Error::Store(e.to_string()))?;
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&value).map_err(|e| Error::Store(e.to_string()))?)
        );
        let bytes = serde_json::to_vec(
            &json!({"schema":SCHEMA,"input_sha256":key,"value_sha256":digest,"value":value}),
        )
        .map_err(|e| Error::Store(e.to_string()))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(Error::Store(
                "native preview result exceeds its retained bound".into(),
            ));
        }
        ds_layer_store::private::write_atomic(&self.path, bytes)
            .map_err(|e| Error::Store(e.to_string()))
    }
}

/// Reuse one exact result, or continue this intentional native preview through
/// the existing retained-read owner. No provider, freshness probe or cloud read
/// exists here. The file lock also joins another process rendering this subject.
pub fn consume<T, E>(
    room: &Room,
    subject: &str,
    inputs: &Value,
    render: impl FnOnce() -> Result<T, E>,
) -> Result<T, Error<E>>
where
    T: Serialize + DeserializeOwned,
{
    if room.scope.principal.trim().is_empty()
        || room.scope.project.trim().is_empty()
        || room.lane.trim().is_empty()
        || subject.trim().is_empty()
        || room.credential_audience_sha256.len() != 64
        || !room
            .credential_audience_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error::Store(
            "preview room requires exact account, project, lane and audience authority".into(),
        ));
    }
    let authority = json!([
        SCHEMA,
        room.scope.principal,
        room.scope.project,
        room.lane,
        room.credential_audience_sha256
    ]);
    let scope = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&authority).map_err(|e| Error::Store(e.to_string()))?)
    );
    let subject = format!("{:x}", Sha256::digest(subject.as_bytes()));
    let root = super::room_hold::room_dir(&room.root, &room.scope)
        .join("previews")
        .join(&scope);
    let _flight = ds_layer_store::exclusive_lock(&root.join(format!("{subject}.lock")))
        .map_err(|e| Error::Store(e.to_string()))?;
    let key = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(inputs).map_err(|e| Error::Store(e.to_string()))?)
    );
    let mut host = Disk {
        path: root.join(format!("{subject}.json")),
        render: Some(render),
        marker: std::marker::PhantomData,
    };
    READS
        .get_or_init(Reads::default)
        .read(
            &format!("{scope}/{subject}"),
            &key,
            Demand {
                online: true,
                acquire: true,
                refresh: false,
            },
            &mut host,
        )
        .map_err(|error| match error {
            Failure::Unavailable(error) => error,
            _ => Error::Store("native preview result room is unavailable".into()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    fn room(root: PathBuf) -> Room {
        Room {
            root,
            scope: Scope {
                principal: "owner".into(),
                project: "project".into(),
            },
            lane: "canary".into(),
            credential_audience_sha256: "a".repeat(64),
        }
    }
    #[test]
    fn navigation_and_concurrent_transformer_consumers_join_one_exact_render() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(4));
        std::thread::scope(|threads| {
            for _ in 0..4 {
                let room = &room;
                let calls = calls.clone();
                let barrier = barrier.clone();
                threads.spawn(move || {
                    barrier.wait();
                    let result = consume::<Value, ()>(
                        room,
                        "transformer/tx",
                        &json!({"layout":7,"room_revision":4}),
                        || {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Ok(json!({"svg":"<svg/>"}))
                        },
                    )
                    .unwrap();
                    assert_eq!(result["svg"], "<svg/>");
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        consume::<Value, ()>(
            &room,
            "transformer/tx",
            &json!({"layout":7,"room_revision":4}),
            || panic!("navigation rendered again"),
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn exact_inputs_and_every_authority_dimension_are_separate_and_unused_rooms_do_no_work() {
        let dir = tempfile::tempdir().unwrap();
        let base = room(dir.path().into());
        let calls = AtomicUsize::new(0);
        let run = |room: &Room, subject: &str, inputs: Value| {
            consume::<Value, ()>(room, subject, &inputs, || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"svg":"<svg/>"}))
            })
            .unwrap()
        };
        // Constructing an unused room cannot render or even inspect freshness.
        let _unused = base.clone();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        run(
            &base,
            "transformer/tx",
            json!({"layout":7,"revision":4,"engine":"a"}),
        );
        for input in [
            json!({"layout":8,"revision":4,"engine":"a"}),
            json!({"layout":8,"revision":5,"engine":"a"}),
            json!({"layout":8,"revision":5,"engine":"b"}),
        ] {
            run(&base, "transformer/tx", input);
        }
        for changed in [
            Room {
                scope: Scope {
                    principal: "other".into(),
                    ..base.scope.clone()
                },
                ..base.clone()
            },
            Room {
                scope: Scope {
                    project: "other".into(),
                    ..base.scope.clone()
                },
                ..base.clone()
            },
            Room {
                lane: "stable".into(),
                ..base.clone()
            },
            Room {
                credential_audience_sha256: "b".repeat(64),
                ..base.clone()
            },
        ] {
            run(
                &changed,
                "transformer/tx",
                json!({"layout":8,"revision":5,"engine":"b"}),
            );
        }
        run(
            &base,
            "mv/model",
            json!({"layout":8,"revision":5,"engine":"b"}),
        );
        assert_eq!(calls.load(Ordering::SeqCst), 9);
    }
    #[test]
    fn failed_replacement_keeps_the_previous_result_and_corrupt_bytes_cannot_be_reused() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        consume::<Value, ()>(&room, "mv/model", &json!(1), || Ok(json!({"svg":"<svg/>"}))).unwrap();
        assert!(matches!(
            consume::<Value, ()>(&room, "mv/model", &json!(2), || Err(())),
            Err(Error::Render(()))
        ));
        consume::<Value, ()>(&room, "mv/model", &json!(1), || {
            panic!("failed render replaced held result")
        })
        .unwrap();
        let root = super::super::room_hold::room_dir(&room.root, &room.scope).join("previews");
        let authority = std::fs::read_dir(root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let file = std::fs::read_dir(authority)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|s| s == "json"))
            .unwrap();
        let mut document: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        document["value"]["svg"] = json!("wrong");
        std::fs::write(file, serde_json::to_vec(&document).unwrap()).unwrap();
        let result = consume::<Value, ()>(&room, "mv/model", &json!(1), || {
            Ok(json!({"svg":"<svg repaired/>"}))
        })
        .unwrap();
        assert_eq!(result["svg"], "<svg repaired/>");
    }
}
