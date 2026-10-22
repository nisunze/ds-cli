use ds_solar_project::{
    ProjectIdentity, Workspace,
    clean::{self, Control},
};
use fs2::FileExt;
use rusqlite::Connection;

#[test]
fn clean_keeps_authored_inputs_refuses_active_compute_and_requires_recomputation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("workspace");
    let mut workspace = Workspace::initialize(
        &path,
        ProjectIdentity {
            project_id: "project".into(),
            root: "eds_project/project/eds_solar".into(),
        },
    )
    .unwrap();
    workspace
        .create_city("city_a", "Authored city", None)
        .unwrap();
    let input = workspace.network_form("city_a").unwrap();
    let database = Connection::open(path.join("solar.sqlite")).unwrap();
    database.execute_batch("INSERT INTO runs(id,status,request) VALUES('run','succeeded','broken derived data'); INSERT INTO outbox(kind,identity,digest,payload) VALUES('run','run','broken','broken');").unwrap();
    std::fs::create_dir(path.join("runs/run")).unwrap();
    std::fs::write(path.join("runs/run/result.json"), b"obsolete bytes").unwrap();
    let control = |project: &str| Control {
        workspace: path.clone(),
        project_id: project.into(),
    };
    assert!(clean::execute(control("other")).is_err());
    assert!(path.join("runs/run/result.json").exists());
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.join("compute.lock"))
        .unwrap();
    FileExt::try_lock_shared(&held).unwrap();
    assert!(
        clean::execute(control("project"))
            .unwrap_err()
            .to_string()
            .contains("active")
    );
    drop(held);
    let result = clean::execute(control("project")).unwrap();
    assert_eq!(result["cleaned"], true);
    assert_eq!(result["recompute_required"], true);
    assert!(!path.join("runs/run").exists());
    assert_eq!(workspace.network_form("city_a").unwrap(), input);
    assert_eq!(workspace.pending(20).unwrap().len(), 1);
    assert!(workspace.run_result("run").is_err());
    drop(workspace);
    assert_eq!(
        Workspace::open(&path)
            .unwrap()
            .network_form("city_a")
            .unwrap(),
        input
    );
    assert!(clean::execute(control("project")).is_ok());
}
