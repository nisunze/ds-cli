//! Closed Core retirement: removed linked-engine routes refuse; native host controls remain.
//! DS Grid file commands restored through the captured Network addon
//! (DECOUPLING-003) are registered again; every other removed ID stays absent.
use serde_json::Value;
use std::process::Command;

const RETIRED_COMMANDS: &[&str] = &[
    "dsgrid-exchange.convert",
    "dsgrid-exchange.inspect",
    "dsgrid-exchange.plan",
    "dsgrid-exchange.sync",
    "dsgrid.alignment.gap.set",
    "dsgrid.alignment.gap.show",
    "dsgrid.asset.attach",
    "dsgrid.asset.detach",
    "dsgrid.asset.extract",
    "dsgrid.asset.list",
    "dsgrid.backup.preview",
    "dsgrid.feature-codes.cleared-forest-offset.set",
    "dsgrid.feature-codes.export",
    "dsgrid.feature-codes.import",
    "dsgrid.feature-codes.migrate",
    "dsgrid.import-structure",
    "dsgrid.model.combine",
    "dsgrid.profile.labels.set",
    "dsgrid.profile.labels.show",
    "dsgrid.project.asset.extract",
    "dsgrid.project.asset.list",
    "dsgrid.project.compare",
    "dsgrid.project.geojson",
    "dsgrid.publish-version",
    "dsgrid.reconcile-cable-source",
    "dsgrid.replace-structure",
    "dsgrid.structure.admin-refresh",
    "dsgrid.structure.staking-enrich",
    "dsgrid.template.apply",
    "dsgrid.template.compile",
    "dsgrid.template.inspect",
    "library.catalog",
    "library.global.download",
    "library.global.example-lifecycle",
    "library.global.fork-example",
    "library.global.library-lifecycle",
    "library.global.publish-example",
    "library.global.publish-library",
    "library.global.read",
    "library.global.resolve-member",
    "library.global.upload",
    "library.model.attach",
    "library.model.clone",
    "library.model.create",
    "library.model.detach",
    "library.model.impact-apply",
    "library.model.impact-plan",
    "library.model.match",
    "library.model.show",
    "library.open",
    "library.pack",
    "library.prepare-publication",
    "library.resolve-native",
    "library.seed",
    "library.unpack",
    "library.verify",
    "pls.backup-create",
    "pls.backup-extract",
    "pls.compare-don",
    "pls.delivery-verify",
    "pls.desktop.autosag",
    "pls.desktop.check",
    "pls.desktop.close",
    "pls.desktop.deliver",
    "pls.desktop.dialogs",
    "pls.desktop.qualify",
    "pls.desktop.reports",
    "pls.desktop.restore",
    "pls.desktop.sheets-pdf",
    "pls.desktop.toolkit",
    "pls.deviation-labels",
    "pls.pole-capacity.read",
    "pls.reference-closure",
    "pls.section-orientation",
    "pls.shading-variants",
    "pls.structure-inventory",
    "pls.structure-substitute",
    "pls.structure-translate",
    "pls.terrain-reconcile",
    "pm.task.geometry.set",
];
fn call(args: &[&str]) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    (
        output.status.success(),
        serde_json::from_slice(&output.stdout).expect("DS envelope"),
    )
}

#[test]
fn retired_engine_commands_have_no_catalogue_or_dispatch_route() {
    let unique: std::collections::BTreeSet<_> = RETIRED_COMMANDS.iter().copied().collect();
    // 122 removed at retirement; 42 DS Grid file commands are restored.
    assert_eq!(RETIRED_COMMANDS.len(), 80);
    assert_eq!(
        unique.len(),
        RETIRED_COMMANDS.len(),
        "retirement inventory must contain unique command IDs"
    );
    for id in RETIRED_COMMANDS {
        let (success, descriptor) = call(&["capabilities", id, "--output", "json"]);
        assert!(!success, "{id}");
        assert_eq!(descriptor["error"]["code"], "unknown_selector", "{id}");
        let mut path: Vec<_> = id.split('.').collect();
        path.extend(["--output", "json"]);
        let (success, refused) = call(&path);
        assert!(!success, "{id}");
        // The dsgrid and pm domains exist; a retired verb there is an unknown
        // command. The other retired domains are absent entirely.
        let code = if id.starts_with("pm.") || id.starts_with("dsgrid.") {
            "unknown_command"
        } else {
            "unknown_domain"
        };
        assert_eq!(refused["error"]["code"], code, "{id}: {refused}");
        assert_eq!(refused["error"]["retryable"], false, "{id}");
    }
}

#[test]
fn ordinary_tasks_keep_confirmation_and_reject_removed_geometry_inputs() {
    for id in ["pm.task.create", "pm.task.checklist.edit"] {
        let (success, descriptor) = call(&["capabilities", id, "--output", "json"]);
        assert!(success);
        let command = &descriptor["data"]["command"];
        assert_eq!(command["effect"], "global_write");
        assert_eq!(command["confirmation_required"], true);
        let inputs = command["inputs"].as_array().unwrap();
        for name in ["geometry-from", "package", "buffer-m"] {
            assert!(
                inputs.iter().all(|input| input["name"] != name),
                "{id}: {name}"
            );
            let flag = format!("--{name}");
            let mut args: Vec<_> = id.split('.').collect();
            args.extend([&flag, "unused", "--output", "json"]);
            let (success, refusal) = call(&args);
            assert!(!success);
            assert_eq!(refusal["error"]["code"], "unknown_flag", "{id}: {refusal}");
        }
    }
    let (success, described) = call(&["capabilities", "pm.task.create", "--output", "json"]);
    assert!(success);
    assert!(
        described["data"]["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["name"] == "dry-run")
    );
}

#[test]
fn retained_profile_map_and_task_controls_are_discoverable() {
    for id in [
        "map.profile.view",
        "map.profile.set",
        "map.profile.edit",
        "map.profile.save",
        "pm.task.create",
        "pm.task.geometry.read",
        "pm.task.geometry.clear",
        "pm.task.checklist.read",
        "pm.task.checklist.edit",
        "pm.task.checklist.promote",
    ] {
        let (success, descriptor) = call(&["capabilities", id, "--output", "json"]);
        assert!(success, "{id}: {descriptor}");
        assert_eq!(descriptor["data"]["command"]["id"], id);
    }
    // The DS Grid file profiles return with their restored commands; the
    // PLS-CADD and library profiles stay retired with theirs.
    for token in [
        "grid",
        "grid-native",
        "grid-corrections",
        "grid-local-model",
        "clearance",
    ] {
        assert!(
            ds_cli_mcp::surface::Profile::from_token(token).is_some(),
            "{token}"
        );
    }
    for token in ["pls", "pls-desktop", "pls-library", "library-governance"] {
        assert!(
            ds_cli_mcp::surface::Profile::from_token(token).is_none(),
            "{token}"
        );
    }
    assert!(ds_cli_mcp::surface::Profile::from_token("grid-profile").is_some());
}

#[test]
fn retired_chapters_have_no_public_catalogue_or_mcp_router() {
    use ds_cli_contract::spec::Chapter;
    use ds_cli_mcp::surface::{Exposure, Surface};
    // The Grid model chapter is routed again for the restored file commands.
    assert_eq!(Chapter::from_token("grid-model"), Some(Chapter::GridModel));
    assert!(Chapter::from_token("pls-cadd").is_none());
    // Independent domain owners retain their enum identities without exposing Core routes.
    assert_eq!(Chapter::GridModel.token(), "grid-model");
    assert_eq!(Chapter::PlsCadd.token(), "pls-cadd");
    let surface = Surface::new(Exposure::Chapters, None, Vec::new()).unwrap();
    let tools = surface.tool_list();
    assert!(tools.iter().any(|tool| tool["name"] == "ds_grid_model"));
    assert!(tools.iter().all(|tool| tool["name"] != "ds_pls_cadd"));
    assert!(
        tools
            .iter()
            .any(|tool| tool["name"] == "ds_map_presentation")
    );
}
