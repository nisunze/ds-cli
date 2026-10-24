use super::*;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};

/// A private directory removed when the test ends; no extra dependency.
#[cfg(unix)]
struct Scratch(PathBuf);
#[cfg(unix)]
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ds-solar-client-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
#[cfg(unix)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fixture addon: an identity with the given catalogue (counted per read),
/// a `started` marker for any task process; calculate-prepared refuses with
/// its typed stdout refusal. Unknown commands exit 2.
#[cfg(unix)]
fn fixture(schemas: &[&str]) -> (Scratch, Solar) {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new();
    let root = dir.path();
    let identity = json!({"name": "ds-solar-engine", "version": "9.9.9", "schemas": schemas,
        "a_newer_member": {"additive": true}});
    std::fs::write(
        root.join("identity.json"),
        serde_json::to_vec(&identity).unwrap(),
    )
    .unwrap();
    let script = format!(
        "#!/bin/sh\nroot='{}'\ncase \"$1\" in\n\
         build-info) printf x >> \"$root/identity-reads\"; cat \"$root/identity.json\";;\n\
         calculate-prepared) printf started > \"$root/started\"; \
         printf '%s' '{{\"status\":\"error\",\"error\":{{\"code\":\"invalid_input\",\"message\":\"prepared input refused\"}}}}'; exit 3;;\n\
         *) exit 2;;\nesac\n",
        root.display()
    );
    let path = root.join("solar");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let solar = Solar::capture_pinned(&path).unwrap();
    (dir, solar)
}

#[test]
fn every_task_names_one_subcommand_and_one_catalogue_schema() {
    let tasks = [
        Task::CalculatePrepared,
        Task::CalculateBatch,
        Task::CalculatePortfolio,
        Task::PrepareReference,
        Task::EncodeOutputs,
        Task::FinalLint,
    ];
    let subcommands: std::collections::BTreeSet<_> =
        tasks.iter().map(|task| task.subcommand()).collect();
    let schemas: std::collections::BTreeSet<_> =
        tasks.iter().map(|task| task.catalogue_schema()).collect();
    assert_eq!(subcommands.len(), tasks.len());
    assert_eq!(schemas.len(), tasks.len());
    assert!(schemas.iter().all(|schema| schema.starts_with("ds-solar.")));
}

#[cfg(unix)]
#[test]
fn an_undeclared_task_refuses_before_any_process_and_the_identity_is_read_once() {
    let (dir, solar) = fixture(&[Task::CalculateBatch.catalogue_schema()]);
    assert!(solar.require(Task::CalculateBatch).is_ok());
    for task in [Task::CalculatePrepared, Task::CalculatePortfolio] {
        let refused = solar
            .run(
                task,
                &dir.path().join("request.json"),
                &dir.path().join("out"),
                DISCOVERY_TIMEOUT,
            )
            .err()
            .expect("an undeclared task refuses");
        assert_eq!(refused.code(), TASK_UNSUPPORTED);
        assert_eq!(refused.detail_value().unwrap()["task"], task.name());
        let transport = solar.with_task_executable(task, |_| unreachable!("no transport runs"));
        assert_eq!(transport.err().unwrap().code(), TASK_UNSUPPORTED);
    }
    assert!(
        !dir.path().join("started").exists(),
        "no task process started"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("identity-reads")).unwrap(),
        "x",
        "one identity read serves every check"
    );
    let ran = solar
        .with_task_executable(Task::CalculateBatch, |path| path.to_path_buf())
        .unwrap();
    assert_eq!(ran, solar.executable());
}

#[cfg(unix)]
#[test]
fn a_declared_task_runs_and_its_typed_refusal_is_kept() {
    let (dir, solar) = fixture(&[Task::CalculatePrepared.catalogue_schema()]);
    let reply = solar
        .run(
            Task::CalculatePrepared,
            &dir.path().join("request.json"),
            &dir.path().join("out"),
            DISCOVERY_TIMEOUT,
        )
        .unwrap();
    assert!(dir.path().join("started").exists());
    assert!(!reply.succeeded());
    let refusal = reply.refusal();
    assert_eq!(refusal.code(), "engine_refused");
    let detail = refusal.detail_value().unwrap();
    assert_eq!(detail["subcommand"], "calculate-prepared");
    assert_eq!(detail["native"]["code"], "invalid_input");
}

#[cfg(unix)]
#[test]
fn an_identity_that_is_not_the_solar_engine_is_a_contract_mismatch() {
    let (dir, solar) = fixture(&[]);
    std::fs::write(
        dir.path().join("identity.json"),
        br#"{"name":"something-else","schemas":[]}"#,
    )
    .unwrap();
    assert_eq!(
        solar.build_info().err().unwrap().code(),
        "callee_contract_mismatch"
    );
}

#[cfg(unix)]
#[test]
fn new_tasks_require_update_only_at_the_named_boundary_and_accept_typed_future_families() {
    let (dir, solar) = fixture(&[Task::CalculatePrepared.catalogue_schema()]);
    assert!(solar.require(Task::CalculatePrepared).is_ok());
    for task in [Task::PrepareReference, Task::EncodeOutputs, Task::FinalLint] {
        let error = solar
            .with_task_executable(task, |_| unreachable!("unsupported task started"))
            .unwrap_err();
        assert_eq!(error.code(), "solar_engine_update_required");
        assert_eq!(error.detail_value().unwrap()["task"], task.name());
    }
    assert!(!dir.path().join("started").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("identity-reads")).unwrap(),
        "x"
    );
    let (_dir, future) = fixture(&[
        "ds-solar.reference-preparation-request/v2",
        "ds-solar.output-encoding-request/v2",
        "ds-solar.final-lint-request/v2",
    ]);
    assert!(future.require(Task::PrepareReference).is_ok());
    assert!(future.require(Task::EncodeOutputs).is_ok());
    assert!(future.require(Task::FinalLint).is_ok());
}
