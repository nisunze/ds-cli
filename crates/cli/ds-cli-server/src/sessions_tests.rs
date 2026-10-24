use super::*;

struct NoSessions;
impl SessionOpener for NoSessions {
    fn open(&self, _: &Path, _: &Connection, _: &str) -> Result<Arc<ServerSyncSession>, String> {
        Err("no gateway session in this test".into())
    }
}

#[test]
fn only_pump_attempts_use_the_failure_recording_opener_and_failures_are_not_cached() {
    #[derive(Default)]
    struct OpeningCounts {
        ordinary: AtomicU64,
        sync: AtomicU64,
    }
    impl SessionOpener for OpeningCounts {
        fn open(
            &self,
            _: &Path,
            _: &Connection,
            _: &str,
        ) -> Result<Arc<ServerSyncSession>, String> {
            self.ordinary.fetch_add(1, Ordering::SeqCst);
            Err("ordinary failure".into())
        }
        fn open_for_sync(
            &self,
            _: &Path,
            _: &Connection,
            _: &str,
        ) -> Result<Arc<ServerSyncSession>, String> {
            self.sync.fetch_add(1, Ordering::SeqCst);
            Err("pump failure".into())
        }
    }
    let dir = ds_cli_contract::testing::private_tempdir("core-test-");
    let original = sessions(dir.path().join("sync.sqlite"));
    let counts = Arc::new(OpeningCounts::default());
    let sessions = ServerSessions::with(
        original.connection.clone(),
        original.database.clone(),
        original.limits,
        original.identity.clone(),
        counts.clone(),
    );
    assert!(sessions.session("project-a").is_err());
    assert!(sessions.sync_session("project-a").is_err());
    assert!(sessions.sync_session("project-a").is_err());
    assert_eq!(counts.ordinary.load(Ordering::SeqCst), 1);
    assert_eq!(counts.sync.load(Ordering::SeqCst), 2);
    assert!(sessions.sessions.lock().unwrap().is_empty());
    assert!(!original.database.exists());
}

fn sessions(database: PathBuf) -> Arc<ServerSessions> {
    let identity = HostIdentity {
        owner: "owner-digest".into(),
        principal: execution_context::Principal {
            uid: "uid-a".into(),
            lane: "canary".into(),
            deployment: "https://gateway.example".into(),
            install_id: "install-1".into(),
        },
    };
    ServerSessions::with(
        Connection {
            owner: "owner-digest".into(),
            lane: "canary".into(),
            socket: crate::transport::socket_path(std::path::Path::new("/state")),
            legacy_address: None,
        },
        database,
        Limits {
            global_running: 2,
            per_project_running: 1,
            per_project_queued: 4,
            global_queued: 8,
        },
        identity,
        Arc::new(NoSessions),
    )
}

#[test]
fn a_project_named_for_the_first_time_is_admitted_with_no_directory_anywhere() {
    // Nothing lists project-a or project-b beforehand: there is no
    // directory, no snapshot and no upstream. The owner names them, and
    // that is the whole of what the Server needs.
    let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
    let now = 1_700_000_000_000;
    for project in ["project-a", "project-b", "a-project-never-seen"] {
        let context = sessions
            .admit_read("layer_read", Some(project), b"list", now)
            .expect("the owner's word is enough");
        assert_eq!(context.project, project);
        assert_eq!(context.principal_uid, "uid-a");
    }
    assert!(
        sessions.sessions.lock().unwrap().is_empty(),
        "admission opens no session"
    );
}

#[test]
fn a_read_carries_a_context_of_its_own_and_never_collides_with_a_caller_key() {
    let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
    let context = sessions
        .admit_read(
            "layer_write",
            Some("project-a"),
            b"hide poles",
            1_700_000_000_000,
        )
        .expect("admitted");
    assert_eq!(context.operation, "layer_write");
    assert_eq!(context.project, "project-a");
    assert!(context.idempotency_key.starts_with("read:layer_write:"));
    assert!(
        context.idempotency_key.contains(':'),
        "a caller key is letters, digits, _ and - only, so this can never be one"
    );
    assert_eq!(context.client, sessions.client_label());
    let second = sessions
        .admit_read(
            "layer_write",
            Some("project-a"),
            b"hide poles",
            1_700_000_000_000,
        )
        .expect("admitted");
    assert_ne!(
        context.idempotency_key, second.idempotency_key,
        "two reads are two operations, never an idempotent pair"
    );
}

#[test]
fn an_unnamed_project_is_required_and_an_unbounded_one_is_named_not_guessed() {
    let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
    let now = 1_700_000_000_000;
    assert_eq!(
        sessions
            .admit_read("layer_read", None, b"list", now)
            .expect_err("no project")
            .code(),
        "project_required"
    );
    let padded = sessions
        .admit_read("layer_read", Some(" padded"), b"list", now)
        .expect_err("not a bounded project id");
    assert_eq!(padded.code(), "context_corrupt");
    assert_eq!(padded.class(), ExitClass::InvalidInput);
}

#[test]
fn a_narrowing_project_is_the_write_doors_refusal_or_nothing_at_all() {
    // No name is no narrowing: the host defaults nothing on a read.
    assert_eq!(narrowing_project(None).expect("nothing named"), None);
    assert_eq!(
        narrowing_project(Some("project-a")).expect("one segment"),
        Some("project-a".to_owned())
    );
    // A path expression is refused by the same rule, the same code, the
    // same class, the same sentence and the same remedy a submission gets.
    let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
    let written = sessions
        .admit_read("layer_read", Some("A/../B"), b"list", 1_700_000_000_000)
        .expect_err("not a segment at a write door");
    let read = narrowing_project(Some("A/../B")).expect_err("not a segment on a read");
    assert_eq!(read.code(), written.code());
    assert_eq!(read.class(), written.class());
    assert_eq!(read.message(), written.message());
    assert_eq!(read.remedy_text(), written.remedy_text());
    assert_eq!(read.code(), "context_corrupt");
    assert!(
        read.message().contains("one path segment"),
        "{}",
        read.message()
    );
}

#[test]
fn the_projects_this_host_knows_are_the_ones_its_owner_handed_it_work_for() {
    // A queue with nothing in it names no project; once the owner has
    // handed the Server work for two projects those two are the answer,
    // read from the rows on disk with no upstream anywhere. It is an
    // answer about this host's own work, not a gate on anything.
    let dir = ds_cli_contract::testing::private_tempdir("core-test-");
    let database = dir.path().join("store.sqlite");
    let sessions = sessions(database.clone());
    assert_eq!(sessions.durable_projects().unwrap(), Vec::<String>::new());
    let batch = |name: &str| -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"schema":"ds.fast-lv.request/v1","jobs":[{"transformer_name":name,"gdfs":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr-1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":name,"names":name}}]},"lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line-1","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]},"customers":{"type":"FeatureCollection","features":[]}},"settings":{}}]})).unwrap()
    };
    let now = 1_700_000_000_000;
    for (key, project) in [
        ("k-a", "project-a"),
        ("k-b", "project-b"),
        ("k-a2", "project-a"),
    ] {
        let client = sessions.client_label();
        let job = runtime::submit(
            &database,
            &runtime::Admission {
                identity: sessions.identity(),
                client: &client,
                key,
                requested_project: Some(project),
                limits: sessions.limits(),
                now_ms: now,
            },
            &batch(key),
        )
        .expect("admitted on the owner's word");
        assert_eq!(job.context.unwrap().project, project);
    }
    let mut projects = sessions.durable_projects().unwrap();
    projects.sort();
    assert_eq!(
        projects,
        vec!["project-a".to_owned(), "project-b".to_owned()]
    );
}

#[test]
fn one_key_in_two_projects_is_two_ids_and_the_same_key_twice_is_one() {
    // The durable id digests the project, so the owner's daily key in two
    // projects is two pieces of work rather than a refusal or a collision.
    let sessions = sessions(PathBuf::from("/tmp/does-not-exist/store.sqlite"));
    let now = 1_700_000_000_000;
    let admit = |project: &str| {
        sessions
            .admit(&Request {
                operation: "transformer_processing",
                key: "daily",
                input_sha256: digest(b"same bytes"),
                requested_project: Some(project),
                sealed_project: None,
                client: "cli:1",
                now_ms: now,
            })
            .expect("admitted")
    };
    let a = admit("project-a");
    let b = admit("project-b");
    assert_ne!(a.job_id, b.job_id);
    assert_eq!(a.idempotency_key, b.idempotency_key);
    assert_eq!(admit("project-a").job_id, a.job_id);
}
