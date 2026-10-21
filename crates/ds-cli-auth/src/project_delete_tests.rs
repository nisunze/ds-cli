use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

const PID: &str = "zz_delete_fixture";
const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn plan() -> Value {
    json!({
        "plan": {
            "project_id": PID,
            "plan_digest": DIGEST,
            "groups": [
                {"store":"firestore","location":"eds_project/{pid}","disposition":"archive","count":1},
                {"store":"firestore","location":"eds_project/{pid}/forms/*/entries","disposition":"archive","count":3},
                {"store":"firestore","location":"eds_project/{pid}/docs/sync","disposition":"delete_only","count":1},
                {"store":"firestore","location":"eds_project/deleted_entries/forms/*/entries","disposition":"archive","count":1},
                {"store":"messaging","location":"conversations/{conversation}","disposition":"archive","count":1},
                {"store":"storage","location":"gs://b/projects/{pid}/assets/","disposition":"archive","count":2},
                {"store":"storage","location":"gs://b/projects/{pid}/forms/","disposition":"retained","count":1},
                {"store":"bigquery","location":"bq:{dataset}_survey","disposition":"delete_only","count":1}
            ],
            "totals": {"archive":{"count":9,"bytes":7},"delete_only":{"count":2,"bytes":0},"retained":{"count":1,"bytes":5},"blocked":{"count":0,"bytes":0}},
            "blockers": [],
            "archive_bucket": "deleted",
            "archive_object": format!("{PID}/0123.zip"),
            "retained_objects": ["gs://b/projects/zz_delete_fixture/forms/f/e/a.jpg"],
        },
        "record": null,
    })
}

fn doc(name: &str) -> Value {
    json!({"name": format!("projects/p/databases/(default)/documents/{name}"), "fields": {"x": {"stringValue": "y"}}})
}

#[derive(Default)]
struct World {
    calls: Vec<String>,
    opened: u32,
    sessions_used: Vec<String>,
    uploaded: Option<Vec<u8>>,
    fail_next_upload: bool,
    remote_exists: bool,
    corrupt_readback: bool,
    receipt: Option<Value>,
    applies: Vec<Value>,
}

#[derive(Clone)]
struct Fake(Rc<RefCell<World>>);

impl Fake {
    fn new() -> Self {
        Self(Rc::new(RefCell::new(World::default())))
    }
    fn calls(&self, action: &str) -> usize {
        self.0
            .borrow()
            .calls
            .iter()
            .filter(|c| c.as_str() == action)
            .count()
    }
}

fn page(items: Vec<Value>) -> Value {
    json!({"items": items, "done": true, "next_cursor": ""})
}

impl Gateway for Fake {
    fn call(&mut self, command: &Door) -> Result<Value, Failure> {
        let mut world = self.0.borrow_mut();
        world.calls.push(command.action().to_owned());
        Ok(match command {
            Door::Plan { .. } => {
                let mut planned = plan();
                if let Some(receipt) = &world.receipt {
                    planned["record"] = json!({"state": "archived", "archive": receipt});
                }
                planned
            }
            Door::Export { source, cursor, .. } => match source.as_str() {
                "firestore" if cursor.is_none() => json!({
                    "items": [
                        {"store":"firestore","group":"eds_project/{pid}","document":doc("eds_project/zz_delete_fixture")},
                        {"store":"firestore","group":"eds_project/{pid}/forms/*/entries","document":doc("eds_project/zz_delete_fixture/forms/a/entries/1")},
                    ],
                    "done": false, "next_cursor": "c1",
                }),
                "firestore" => page(vec![
                    json!({"store":"firestore","group":"eds_project/{pid}/forms/*/entries","document":doc("eds_project/zz_delete_fixture/forms/a/entries/2")}),
                    json!({"store":"firestore","group":"eds_project/{pid}/forms/*/entries","document":doc("eds_project/zz_delete_fixture/forms/b/entries/3")}),
                ]),
                "trash" => page(vec![
                    json!({"store":"firestore","group":"eds_project/deleted_entries/forms/*/entries","document":doc("eds_project/deleted_entries/forms/a/entries/9")}),
                ]),
                "messaging" => page(vec![
                    json!({"store":"messaging","group":"conversations/{conversation}","document":doc("conversations/pg_1")}),
                ]),
                _ => page(vec![
                    json!({"store":"storage","group":"gs://b/projects/{pid}/assets/","object":{"bucket":"b","name":"projects/zz_delete_fixture/assets/x/a.pdf","generation":5,"size":3,"url":"https://signed/a"}}),
                    json!({"store":"storage","group":"gs://b/projects/{pid}/assets/","object":{"bucket":"b","name":"projects/zz_delete_fixture/assets/x/b.md","generation":6,"size":4,"url":"https://signed/b"}}),
                    json!({"store":"storage","group":"gs://b/projects/{pid}/forms/","object":{"bucket":"b","name":"projects/zz_delete_fixture/forms/f/e/a.jpg","generation":7,"size":5,"retained":true,"reason":"retained:survey_media_standalone"}}),
                ]),
            },
            Door::ArchiveOpen { .. } => {
                world.opened += 1;
                if world.remote_exists {
                    json!({"exists": true})
                } else {
                    json!({"exists": false, "session_uri": format!("https://storage.googleapis.com/upload/s{}", world.opened)})
                }
            }
            Door::ArchiveRead { .. } => {
                let size = world.uploaded.as_ref().map_or(0, Vec::len);
                json!({"attrs": {"generation": 77, "size": size}, "url": "https://signed/archive"})
            }
            Door::ArchiveReceipt {
                generation, sha256, ..
            } => {
                let receipt =
                    json!({"plan_digest": DIGEST, "generation": generation, "sha256": sha256});
                world.receipt = Some(receipt.clone());
                json!({"state": "archived", "archive": receipt})
            }
            Door::Apply { .. } => {
                if world.receipt.is_none() {
                    return Err(Failure::conflict(ARCHIVE_REQUIRED.code, "no receipt"));
                }
                if world.applies.is_empty() {
                    json!({"state":"deleted","complete":true,"deleted":{"firestore":5},"residuals":[]})
                } else {
                    world.applies.remove(0)
                }
            }
        })
    }
}

impl Bytes for Fake {
    fn download(&mut self, url: &str, limit: u64, sink: &mut dyn Write) -> Result<u64, Failure> {
        let world = self.0.borrow();
        let body: Vec<u8> = match url {
            "https://signed/a" => b"pdf".to_vec(),
            "https://signed/b" => b"note".to_vec(),
            "https://signed/archive" => {
                let mut bytes = world.uploaded.clone().unwrap_or_default();
                if world.corrupt_readback {
                    bytes.push(0);
                }
                bytes
            }
            other => panic!("unexpected download {other}"),
        };
        assert!(body.len() as u64 <= limit + 1);
        sink.write_all(&body).unwrap();
        Ok(body.len() as u64)
    }

    fn upload(
        &mut self,
        session: &str,
        path: &Path,
        _size: u64,
        _sha256: &str,
    ) -> Result<bool, Failure> {
        let mut world = self.0.borrow_mut();
        world.sessions_used.push(session.to_owned());
        if world.fail_next_upload {
            world.fail_next_upload = false;
            return Err(Failure::unavailable(UPLOAD_FAILED.code, "connection reset"));
        }
        world.uploaded = Some(fs::read(path).unwrap());
        Ok(true)
    }
}

fn options(staging: &Path) -> Options {
    Options {
        project: PID.into(),
        dry_run: false,
        plan_digest: Some(DIGEST.into()),
        archive_only: true,
        archive_out: None,
        staging: staging.to_path_buf(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ds-project-delete-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn read_index(zip_path: &Path) -> (Value, Vec<String>) {
    let mut archive = zip::ZipArchive::new(File::open(zip_path).unwrap()).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_owned())
        .collect();
    let mut text = String::new();
    archive
        .by_name("index.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    (serde_json::from_str(&text).unwrap(), names)
}

#[test]
fn the_archive_index_covers_every_archive_member_of_the_plan() {
    let dir = scratch("index");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    let (zip_path, facts) = build_archive(&mut fake, &mut bytes, &dir, &plan(), DIGEST).unwrap();
    let (index, names) = read_index(&zip_path);
    assert_eq!(names[0], "index.json", "index.json is the first member");
    assert_eq!(index["plan_digest"], DIGEST);
    let mut documents: BTreeMap<String, u64> = BTreeMap::new();
    let mut objects: BTreeMap<String, u64> = BTreeMap::new();
    for member in index["members"].as_array().unwrap() {
        let path = member["path"].as_str().unwrap();
        assert!(
            names.iter().any(|n| n == path),
            "index names {path} but the zip lacks it"
        );
        let group = member["group"].as_str().unwrap().to_owned();
        match member["documents"].as_u64() {
            Some(n) => *documents.entry(group).or_default() += n,
            None => *objects.entry(group).or_default() += 1,
        }
    }
    for group in plan()["plan"]["groups"].as_array().unwrap() {
        let location = group["location"].as_str().unwrap();
        let count = group["count"].as_u64().unwrap();
        match (
            group["disposition"].as_str().unwrap(),
            group["store"].as_str().unwrap(),
        ) {
            ("archive", "storage") => assert_eq!(objects.get(location), Some(&count), "{location}"),
            ("archive", _) => assert_eq!(documents.get(location), Some(&count), "{location}"),
            ("retained", _) => {
                let listed = index["retained"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| r["group"] == location)
                    .count();
                assert_eq!(listed as u64, count, "{location}");
            }
            _ => {
                assert!(
                    !documents.contains_key(location) && !objects.contains_key(location),
                    "{location} is not archived"
                );
            }
        }
    }
    assert_eq!(
        facts["members"],
        json!(index["members"].as_array().unwrap().len())
    );
    // The NDJSON members hold the native documents exactly.
    let mut archive = zip::ZipArchive::new(File::open(&zip_path).unwrap()).unwrap();
    let entry = archive
        .by_name("firestore/eds_project.pid.forms._.entries.ndjson.gz")
        .unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(entry)
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text.lines().count(), 3);
    assert!(text.contains(r#""stringValue":"y""#));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dry_run_reads_only_the_plan() {
    let dir = scratch("dry");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    let mut opts = options(&dir);
    opts.dry_run = true;
    opts.plan_digest = None;
    let answer = execute(&mut fake, &mut bytes, &opts).unwrap();
    assert_eq!(answer["plan"]["plan_digest"], DIGEST);
    assert_eq!(fake.0.borrow().calls, vec!["delete_plan"]);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn archiving_needs_the_reviewed_digest() {
    let dir = scratch("digest");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    let mut opts = options(&dir);
    opts.plan_digest = None;
    assert_eq!(
        execute(&mut fake, &mut bytes, &opts).unwrap_err().code(),
        DIGEST_REQUIRED.code
    );
    opts.plan_digest = Some(format!("sha256:{}", "a".repeat(64)));
    assert_eq!(
        execute(&mut fake, &mut bytes, &opts).unwrap_err().code(),
        PLAN_CHANGED.code
    );
    assert_eq!(fake.calls("delete_export"), 0);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn archive_only_verifies_and_receipts_without_deleting() {
    let dir = scratch("archive-only");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    let answer = execute(&mut fake, &mut bytes, &options(&dir)).unwrap();
    assert_eq!(answer["mode"], "archived");
    assert_eq!(answer["archive_receipt"]["generation"], 77);
    assert_eq!(fake.calls("delete_archive_receipt"), 1);
    assert_eq!(fake.calls("delete_apply"), 0);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_interrupted_upload_resumes_on_its_session_without_a_second_archive() {
    let dir = scratch("resume");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    fake.0.borrow_mut().fail_next_upload = true;
    let first = execute(&mut fake, &mut bytes, &options(&dir)).unwrap_err();
    assert_eq!(first.code(), UPLOAD_FAILED.code);
    let exports_after_first = fake.calls("delete_export");
    let answer = execute(&mut fake, &mut bytes, &options(&dir)).unwrap();
    assert_eq!(answer["mode"], "archived");
    let world = fake.0.borrow();
    assert_eq!(
        world.opened, 1,
        "the resumed upload reused its minted session"
    );
    assert_eq!(world.sessions_used.len(), 2);
    assert_eq!(world.sessions_used[0], world.sessions_used[1]);
    drop(world);
    assert_eq!(
        fake.calls("delete_export"),
        exports_after_first,
        "the built archive was reused"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_existing_archive_object_is_never_uploaded_again() {
    let dir = scratch("exists");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    // The object was stored by an earlier run whose local state was kept.
    let (built, _) = build_archive(&mut fake, &mut bytes, &dir, &plan(), DIGEST).unwrap();
    {
        let mut world = fake.0.borrow_mut();
        world.remote_exists = true;
        world.uploaded = Some(fs::read(&built).unwrap());
    }
    execute(&mut fake, &mut bytes, &options(&dir)).unwrap();
    assert!(fake.0.borrow().sessions_used.is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_stored_archive_that_differs_is_never_receipted() {
    let dir = scratch("verify");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    fake.0.borrow_mut().corrupt_readback = true;
    let error = execute(&mut fake, &mut bytes, &options(&dir)).unwrap_err();
    assert_eq!(error.code(), VERIFY_FAILED.code);
    assert_eq!(fake.calls("delete_archive_receipt"), 0);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn apply_runs_after_the_receipt_and_resumes_until_complete() {
    let dir = scratch("apply");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    fake.0.borrow_mut().applies = vec![
        json!({"state":"applying","complete":false,"deleted":{"firestore":3},"residuals":[]}),
        json!({"state":"deleted","complete":true,"deleted":{"firestore":5},"residuals":[]}),
    ];
    let mut opts = options(&dir);
    opts.archive_only = false;
    let answer = execute(&mut fake, &mut bytes, &opts).unwrap();
    assert_eq!(answer["apply"]["complete"], true);
    assert_eq!(fake.calls("delete_apply"), 2);
    let calls = fake.0.borrow().calls.clone();
    let receipt = calls
        .iter()
        .position(|c| c == "delete_archive_receipt")
        .unwrap();
    let apply = calls.iter().position(|c| c == "delete_apply").unwrap();
    assert!(receipt < apply, "nothing is deleted before the receipt");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn apply_that_stalls_on_residuals_refuses() {
    let dir = scratch("stall");
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    let stuck = json!({"state":"applying","complete":false,"deleted":{"firestore":3},"residuals":["permission denied deleting dataset x"]});
    fake.0.borrow_mut().applies = vec![stuck.clone(), stuck];
    let mut opts = options(&dir);
    opts.archive_only = false;
    assert_eq!(
        execute(&mut fake, &mut bytes, &opts).unwrap_err().code(),
        INCOMPLETE.code
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn archive_out_never_overwrites_a_different_file() {
    let dir = scratch("out");
    let out = dir.join("kept.zip");
    fs::write(&out, b"something else").unwrap();
    let mut fake = Fake::new();
    let mut bytes = fake.clone();
    let mut opts = options(&dir);
    opts.archive_out = Some(out.clone());
    assert_eq!(
        execute(&mut fake, &mut bytes, &opts).unwrap_err().code(),
        ARCHIVE_OUT_EXISTS.code
    );
    assert_eq!(fs::read(&out).unwrap(), b"something else");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn object_names_cannot_escape_the_archive() {
    assert_eq!(
        object_path("b", "projects/p/a.pdf").as_deref(),
        Some("objects/b/projects/p/a.pdf")
    );
    for name in ["../x", "a/../b", "/abs", "a//b", "a\\b", ""] {
        assert!(object_path("b", name).is_none(), "{name}");
    }
}

#[test]
fn route_refusals_keep_their_declared_codes() {
    let forbidden = map_error(error_with(403, "project_delete_forbidden"));
    assert_eq!(forbidden.code(), FORBIDDEN.code);
    let changed = map_error(error_with(409, "project_delete_plan_changed"));
    assert_eq!(changed.code(), PLAN_CHANGED.code);
    assert!(REFUSALS.iter().all(|r| !r.remedy.is_empty()));
}

fn error_with(status: u16, code: &str) -> ClientError {
    use crate::test_support::{FixtureTransport, NOW, SIGN_IN, signed_in};
    let body = serde_json::to_vec(
        &json!({"success": false, "error": {"code": code, "message": "refused"}}),
    )
    .unwrap();
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_deletion(status, &body);
    let mut client = signed_in(transport);
    client
        .project_deletion(
            &Door::Plan {
                project_id: PID.into(),
            },
            NOW,
        )
        .unwrap_err()
}
