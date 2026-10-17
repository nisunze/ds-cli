//! Closed Core retirement: removed linked-engine routes refuse; native host controls remain.
use serde_json::Value;
use std::process::Command;

const RETIRED_COMMANDS: &[&str] = &[
    "dsgrid-exchange.convert",
    "dsgrid-exchange.inspect",
    "dsgrid-exchange.plan",
    "dsgrid-exchange.sync",
    "dsgrid.alignment.gap.set",
    "dsgrid.alignment.gap.show",
    "dsgrid.analyse.clearance",
    "dsgrid.apply",
    "dsgrid.apply-batch",
    "dsgrid.apply-correction",
    "dsgrid.asset.attach",
    "dsgrid.asset.detach",
    "dsgrid.asset.extract",
    "dsgrid.asset.list",
    "dsgrid.backup.preview",
    "dsgrid.create",
    "dsgrid.criteria.clearance.set",
    "dsgrid.criteria.show",
    "dsgrid.describe",
    "dsgrid.feature-codes.cleared-forest-offset.set",
    "dsgrid.feature-codes.export",
    "dsgrid.feature-codes.import",
    "dsgrid.feature-codes.migrate",
    "dsgrid.feature-codes.report",
    "dsgrid.import-structure",
    "dsgrid.inspect",
    "dsgrid.model.combine",
    "dsgrid.model.create-local",
    "dsgrid.model.forget",
    "dsgrid.model.import-external",
    "dsgrid.model.link",
    "dsgrid.model.list",
    "dsgrid.model.prepare-project",
    "dsgrid.model.set-active",
    "dsgrid.model.show",
    "dsgrid.model.unlink",
    "dsgrid.package.migrate",
    "dsgrid.profile.labels.set",
    "dsgrid.profile.labels.show",
    "dsgrid.profile.open",
    "dsgrid.project.asset.extract",
    "dsgrid.project.asset.list",
    "dsgrid.project.backup.download",
    "dsgrid.project.bump-version",
    "dsgrid.project.compare",
    "dsgrid.project.download",
    "dsgrid.project.exports.download",
    "dsgrid.project.exports.list",
    "dsgrid.project.exports.publish",
    "dsgrid.project.geojson",
    "dsgrid.project.list",
    "dsgrid.project.restore",
    "dsgrid.project.retire",
    "dsgrid.project.set-approval",
    "dsgrid.project.show",
    "dsgrid.project.update",
    "dsgrid.project.versions",
    "dsgrid.publish-version",
    "dsgrid.reconcile-cable-source",
    "dsgrid.replace-structure",
    "dsgrid.report.staking",
    "dsgrid.report.structures",
    "dsgrid.run",
    "dsgrid.spotting.apply-receipt",
    "dsgrid.spotting.preview-receipt",
    "dsgrid.structure.admin-refresh",
    "dsgrid.structure.describe",
    "dsgrid.structure.retype",
    "dsgrid.structure.staking-enrich",
    "dsgrid.template.apply",
    "dsgrid.template.compile",
    "dsgrid.template.inspect",
    "dsgrid.validate",
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
    assert_eq!(RETIRED_COMMANDS.len(), 122);
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
        let code = if id.starts_with("pm.") {
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
    for token in [
        "grid",
        "grid-native",
        "grid-corrections",
        "grid-local-model",
        "clearance",
        "pls",
        "pls-desktop",
        "pls-library",
        "library-governance",
    ] {
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
    for token in ["grid-model", "pls-cadd"] {
        assert!(Chapter::from_token(token).is_none(), "{token}");
    }
    // Independent domain owners retain their enum identities without exposing Core routes.
    assert_eq!(Chapter::GridModel.token(), "grid-model");
    assert_eq!(Chapter::PlsCadd.token(), "pls-cadd");
    let surface = Surface::new(Exposure::Chapters, None, Vec::new()).unwrap();
    let tools = surface.tool_list();
    for retired in ["ds_grid_model", "ds_pls_cadd"] {
        assert!(
            tools.iter().all(|tool| tool["name"] != retired),
            "{retired}"
        );
    }
    assert!(
        tools
            .iter()
            .any(|tool| tool["name"] == "ds_map_presentation")
    );
}
