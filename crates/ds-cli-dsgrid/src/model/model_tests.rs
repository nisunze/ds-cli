use super::*;

#[test]
fn every_declared_operation_is_listed_for_the_parity_test_to_walk() {
    let mut names: Vec<&str> = BRIDGE_OPS.iter().map(|op| op.operation).collect();
    names.sort_unstable();
    let mut unique = names.clone();
    unique.dedup();
    assert_eq!(names, unique, "an operation is declared twice");
    // Two since 2026-09-20: the working copy the application itself holds
    // open, published from the window, and the window opening one of this
    // machine's working copies in Profile.
    assert_eq!(
        names.len(),
        2,
        "the family sends exactly one operation per command"
    );
    assert!(
        names.iter().all(|name| name.starts_with("dsgrid.")),
        "every operation belongs to the application's DS Grid namespace"
    );
}

#[test]
fn no_operation_claims_a_conversion_or_a_project_revision_activation() {
    // The reverted family's mistake, in operation names. `convert` belongs
    // to the DS Grid exchange boundary, and there is no durable exclusive
    // "activate this revision for the project" authority anywhere in this
    // stack — so nothing here may be named as though there were.
    for forbidden in [
        "dsgrid.model.convert",
        "dsgrid.convert",
        "dsgrid.model.activate",
        "dsgrid.project.activate",
        "dsgrid.revision.activate",
        "dsgrid.model.register",
    ] {
        assert!(
            BRIDGE_OPS.iter().all(|op| op.operation != forbidden),
            "`{forbidden}` is exactly the conflation this family exists to keep apart"
        );
    }
}

#[test]
fn no_operation_can_carry_model_content() {
    // Bytes never travel in either direction: a source is a path or an
    // opaque id, and a receipt addresses content by digest and byte count.
    for op in BRIDGE_OPS {
        for argument in op.arguments {
            assert!(
                !matches!(*argument, "bytes" | "content" | "base64" | "data" | "blob"),
                "`{}` declares a content-carrying argument `{argument}`",
                op.operation
            );
        }
    }
}

#[test]
fn no_local_operation_can_carry_a_project() {
    // The load-bearing project-independence claim, held at the wire
    // contract rather than in prose: only `publish` may name project
    // state at all, and even it never names the project itself — the
    // application's own selected project is the destination.
    //
    // The four model-management operations that used to be checked here
    // no longer cross a wire at all: `ds dsgrid model list|create-local|
    // import-external|set-active` answer from this machine's own
    // catalogue, so their project-independence is now a fact about a
    // store that has no project field (`ds_command_kernel::local_models`).
    assert!(
        !MODEL_PUBLISH.arguments.contains(&"project"),
        "publication targets the paired session's own selected project; \
             a project id is never an argument"
    );
}

#[test]
fn an_external_source_is_absolute_and_a_dsgrid_before_the_bridge_opens() {
    let absolute = if cfg!(windows) {
        "C:\\models\\route.dsgrid"
    } else {
        "/models/route.dsgrid"
    };
    assert_eq!(
        external_dsgrid_path(absolute, "path").expect("accepted"),
        absolute
    );
    assert_eq!(
        external_dsgrid_path("route.dsgrid", "path")
            .expect_err("must refuse")
            .code(),
        "absolute_path_required"
    );
    // The exchange boundary's sources, refused here by name rather than
    // quietly becoming a second conversion verb.
    for filename in ["project.bak", "workspace", "line.don"] {
        let foreign = std::env::temp_dir().join(filename);
        let foreign = foreign.to_str().expect("test path is UTF-8");
        assert_eq!(
            external_dsgrid_path(foreign, "path")
                .expect_err("must refuse")
                .code(),
            "unsupported_model_source",
            "`{foreign}` was accepted as a DS Grid package"
        );
    }
}

#[test]
fn each_named_condition_is_reclassified_only_when_it_really_is_that_condition() {
    let refused = |detail: &str| {
        classify(Failure::failed("desktop_refused", "refused").detail(json!({ "detail": detail })))
    };
    assert_eq!(
        refused("No local DS Grid model m-9.").code(),
        "local_model_not_found"
    );
    // An unsupported coordinate system is no longer one of these: the
    // engine linked into `ds` answers `create-local` directly, so there is
    // no application prose left to reclassify.
    assert_eq!(
        refused("crs EPSG:4326 is not a supported DS Grid coordinate system.").code(),
        "desktop_refused"
    );
    assert_eq!(
        refused("The .dsgrid package exceeds the desktop eager-read limit.").code(),
        "model_too_large"
    );
    // Publication's own conditions return with `publish-version`
    // (DECOUPLING-003); until then they keep the application's refusal.
    assert_eq!(
            refused("Project model gm-3 does not exist in ds-project-7. Omit project_model to publish a new one.")
                .code(),
            "desktop_refused"
        );
    assert_eq!(
        refused("The project head moved: gm-3 is at r-9, not r-7.").code(),
        "desktop_refused"
    );
    // Still classified by the shared signed-out rule, and an ordinary
    // application refusal keeps its own identity.
    assert_eq!(
            refused("No active project. Publishing a DS Grid revision needs one; local model operations do not.")
                .code(),
            "desktop_signed_out"
        );
    assert_eq!(
        refused("the model has no rows to capture").code(),
        "desktop_refused"
    );
    assert_eq!(
        classify(Failure::unavailable("desktop_unreachable", "gone")).code(),
        "desktop_unreachable"
    );
}
