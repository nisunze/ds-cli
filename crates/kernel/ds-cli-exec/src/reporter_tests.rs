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
            "ds-reporter-client-{}-{}",
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

/// A fixture Reporter: a catalogue it reads from `catalogue.json` beside it
/// (counted per read), a `started` marker for any task process, and one
/// registered task that writes its result. Unknown commands exit 2.
#[cfg(unix)]
fn fixture(catalogue: Value) -> (Scratch, Reporter) {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new();
    let root = dir.path();
    std::fs::write(
        root.join("catalogue.json"),
        serde_json::to_vec(&catalogue).unwrap(),
    )
    .unwrap();
    let script = format!(
        "#!/bin/sh\nroot='{}'\ncase \"$1\" in\n\
         build-info) printf '%s\\n' '{{\"service\":\"ds-network-reporter\",\"binary\":\"ds-report\",\"added\":true}}';;\n\
         task-schemas) printf x >> \"$root/catalogue-reads\"; cat \"$root/catalogue.json\";;\n\
         render-print-layout) printf started > \"$root/started\"; printf '{{\"status\":\"completed\"}}' > \"$5\";;\n\
         render-mv-frontmatter) printf started > \"$root/started\"; echo 'frontmatter refused: logo missing' >&2; exit 3;;\n\
         *) exit 2;;\nesac\n",
        root.display()
    );
    let path = root.join("reporter");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let reporter = Reporter::capture_pinned(&path).unwrap();
    (dir, reporter)
}

#[cfg(unix)]
fn registered(tasks: &[Task]) -> Value {
    json!({
        "service": "ds-network-reporter",
        "added_top_level": {"newer": "member"},
        "tasks": tasks.iter().map(|task| json!({
            "name": task.name(),
            "subcommand": task.subcommand(),
            "effect": "writes-files",
            "network": "none",
            "a_newer_member": [1, 2, 3],
        })).collect::<Vec<_>>(),
    })
}

#[cfg(unix)]
#[test]
fn a_registered_task_runs_over_typed_files_and_the_catalogue_is_read_once() {
    let (dir, reporter) = fixture(registered(&[Task::RenderPrintLayout]));
    let request = dir.path().join("request.json");
    std::fs::write(&request, b"{}").unwrap();
    for attempt in 0..2 {
        let result = dir.path().join(format!("result-{attempt}.json"));
        let reply = reporter
            .run(
                Task::RenderPrintLayout,
                &request,
                &result,
                Duration::from_secs(20),
            )
            .unwrap();
        assert!(reply.succeeded());
        assert_eq!(reply.task(), Task::RenderPrintLayout);
        assert_eq!(
            std::fs::read_to_string(&result).unwrap(),
            r#"{"status":"completed"}"#
        );
    }
    // Additive identity and catalogue members are accepted; one read per run.
    assert_eq!(reporter.build_info().unwrap()["added"], true);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("catalogue-reads")).unwrap(),
        "x"
    );
}

#[cfg(unix)]
#[test]
fn an_unregistered_task_is_refused_by_name_before_any_process_starts() {
    let (dir, reporter) = fixture(registered(&[Task::RenderPrintLayout]));
    let result = dir.path().join("result.json");
    let refused = reporter
        .run(
            Task::RenderMvFrontmatter,
            &dir.path().join("request.json"),
            &result,
            Duration::from_secs(20),
        )
        .err()
        .unwrap();
    assert_eq!(refused.code(), TASK_UNSUPPORTED);
    assert_eq!(
        refused.detail_value().unwrap()["task"],
        "render_mv_frontmatter"
    );
    assert!(!dir.path().join("started").exists());
    assert!(!result.exists());
    // A launch is admitted the same way.
    let refused = reporter
        .launch(Task::RenderGridPlanProfile, &dir.path().join("r"), &result)
        .unwrap_err();
    assert_eq!(refused.code(), TASK_UNSUPPORTED);
    assert!(!dir.path().join("started").exists());
}

#[cfg(unix)]
#[test]
fn a_catalogue_naming_the_task_under_another_subcommand_does_not_register_it() {
    let mut catalogue = registered(&[Task::RenderPrintLayout]);
    catalogue["tasks"][0]["subcommand"] = json!("render-print-layout-v2");
    let (dir, reporter) = fixture(catalogue);
    let refused = reporter
        .run(
            Task::RenderPrintLayout,
            &dir.path().join("request.json"),
            &dir.path().join("result.json"),
            Duration::from_secs(20),
        )
        .err()
        .unwrap();
    assert_eq!(refused.code(), TASK_UNSUPPORTED);
    assert!(!dir.path().join("started").exists());

    // Another task's name on this subcommand does not register it either.
    let mut catalogue = registered(&[Task::RenderPrintLayout]);
    catalogue["tasks"][0]["name"] = json!("render_print_layout_v2");
    let (dir, reporter) = fixture(catalogue);
    assert_eq!(
        reporter
            .require(Task::RenderPrintLayout)
            .unwrap_err()
            .code(),
        TASK_UNSUPPORTED
    );
    assert!(!dir.path().join("started").exists());
}

#[cfg(unix)]
#[test]
fn an_entry_without_a_name_registers_its_subcommand() {
    let mut catalogue = registered(&[Task::RenderPrintLayout]);
    catalogue["tasks"][0]
        .as_object_mut()
        .unwrap()
        .remove("name");
    let (dir, reporter) = fixture(catalogue);
    let result = dir.path().join("result.json");
    let reply = reporter
        .run(
            Task::RenderPrintLayout,
            &dir.path().join("request.json"),
            &result,
            Duration::from_secs(20),
        )
        .unwrap();
    assert!(reply.succeeded());
    assert!(result.exists());
}

#[cfg(unix)]
#[test]
fn a_reporter_without_a_catalogue_cannot_run_tasks() {
    let (dir, reporter) = fixture(json!({"service": "ds-network-reporter"}));
    let refused = reporter.require(Task::RenderPrintLayout).unwrap_err();
    assert_eq!(refused.code(), "callee_contract_mismatch");
    assert!(!dir.path().join("started").exists());
}

#[cfg(unix)]
#[test]
fn the_reporters_own_refusal_names_the_task_and_carries_its_message() {
    let (dir, reporter) = fixture(registered(&[Task::RenderMvFrontmatter]));
    let reply = reporter
        .run(
            Task::RenderMvFrontmatter,
            &dir.path().join("request.json"),
            &dir.path().join("result.json"),
            Duration::from_secs(20),
        )
        .unwrap();
    assert!(!reply.succeeded());
    assert_eq!(reply.status(), Some(3));
    let refusal = reply.refusal();
    assert_eq!(refusal.code(), "engine_refused");
    let detail = refusal.detail_value().unwrap();
    assert_eq!(detail["subcommand"], "render-mv-frontmatter");
    assert!(detail["engine"].as_str().unwrap().contains("logo missing"));
}

#[test]
fn every_task_has_one_catalogue_name_and_one_fixed_subcommand() {
    let all = [
        Task::ExportTransformerReport,
        Task::ExportCombinedTransformerReport,
        Task::DiscoverCombinedMediaReferences,
        Task::ExportCompoundedReport,
        Task::ExportSpatialSectorCounts,
        Task::ExportLvStandard,
        Task::RenderPrintLayout,
        Task::RenderGridPlanProfile,
        Task::RenderGridPlanProfileModel,
        Task::RenderMvFrontmatter,
        Task::RenderVoltageDropResult,
        Task::RenderVoltageDropCombined,
        Task::PlanSolarNetworkSeed,
        Task::DetectCollisions,
        Task::PreparePublisherFootprints,
        Task::PlanContourLattice,
        Task::DeriveContours,
        Task::BindPrintText,
    ];
    let names: std::collections::BTreeSet<_> = all.iter().map(|task| task.name()).collect();
    assert_eq!(names.len(), all.len());
    for task in all {
        // The Reporter's catalogue spells a task's subcommand as its name in kebab case.
        assert_eq!(task.subcommand(), task.name().replace('_', "-"));
    }
}
