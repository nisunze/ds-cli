//! Explicit semantic coverage for the whole shipping command surface.
//!
//! Help shape alone is not enough: a newly registered command can parse and
//! still be assigned the wrong authority/effect, changing whether it can
//! reach a project or mutate durable state. This table is intentionally a
//! complete, reviewed inventory. The test first proves it neither omits nor
//! invents an id, then invokes every path's safe `--help` contract and pins
//! the meaning a caller sees in its descriptor.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::Value;

const EXPECTED: &[(&str, &str, &str)] = &[
    (
        "design.features.select",
        "local_auth_state",
        "headless_project",
    ),
    // Native auth is one local-state class because even status/list can rotate
    // a refresh credential. None of these authorities implies Desktop.
    ("auth.link.begin", "local_auth_state", "none"),
    ("auth.link.status", "read_only", "none"),
    ("auth.link.complete", "local_auth_state", "none"),
    ("auth.link.approve", "global_write", "desktop_user"),
    ("auth.device.list", "read_only", "headless_user"),
    ("auth.device.read", "read_only", "headless_user"),
    ("auth.device.revoke", "global_write", "headless_user"),
    ("auth.login", "local_auth_state", "none"),
    ("auth.logout", "local_auth_state", "none"),
    ("auth.project.list", "local_auth_state", "headless_user"),
    ("auth.project.status", "local_auth_state", "headless_user"),
    ("auth.project.use", "local_auth_state", "headless_user"),
    ("auth.status", "local_auth_state", "none"),
    // Local data preparation. `none` authority is exact: a file on the
    // operator's own disk involves no project and no principal. `convert`
    // writes one file into the operator's own workspace and publishes nothing.
    // Admin enrichment also writes locally, but its governed Rwanda boundary
    // digest is resolved from the paired project's pinned resource receipt.
    ("data.admin-bounds.attach", "local_file_write", "project"),
    // Exact hierarchy and geometry evidence use the signed-in Desktop's
    // national reference authority. The list is read-only; read optionally
    // materializes the same geometry as a Desktop-local map layer.
    ("data.admin-bounds.list", "read_only", "desktop_user"),
    ("data.admin-bounds.read", "local_ui", "desktop_user"),
    (
        "data.elevation.attach",
        "local_file_write",
        "desktop_pairing",
    ),
    // Extraction generates points from an explicit area; it is intentionally
    // separate from attachment, which enriches an existing point file. The
    // plan is a map-independent preview; extraction writes only local files
    // through the paired Desktop's governed Rwanda DEM component.
    ("data.elevation.plan", "read_only", "desktop_pairing"),
    (
        "data.elevation.extract",
        "local_file_write",
        "desktop_pairing",
    ),
    ("data.convert", "local_file_write", "none"),
    ("data.conversion-matrix", "discovery", "none"),
    ("data.inspect", "read_only", "none"),
    ("desktop.project.list", "read_only", "desktop_user"),
    ("desktop.project.switch", "local_ui", "desktop_user"),
    ("desktop.status", "discovery", "none"),
    ("design.attachment.download", "read_only", "project"),
    ("design.attachment.list", "read_only", "project"),
    ("design.attachment.publish", "global_write", "project"),
    ("design.attachment.retire", "global_write", "project"),
    ("design.comment.list", "read_only", "project"),
    ("design.comment.post", "global_write", "project"),
    ("design.comment.promote", "global_write", "project"),
    ("design.comment.read", "read_only", "project"),
    ("design.comment.resolve", "global_write", "project"),
    ("design.lv.process", "local_file_write", "none"),
    (
        "design.lv.project-export",
        "local_file_write",
        "headless_project",
    ),
    ("design.consumer-grouping.apply", "global_write", "project"),
    ("design.consumer-grouping.preview", "read_only", "project"),
    ("design.consumer-grouping.read", "read_only", "project"),
    (
        "design.consumer-grouping.archive",
        "global_write",
        "project",
    ),
    ("design.tag.enrich-preview", "read_only", "project"),
    ("design.tag.enrich-apply", "global_write", "project"),
    ("design.known-columns.list", "read_only", "project"),
    ("design.known-columns.set", "global_write", "project"),
    ("design.selection.archive", "global_write", "project"),
    ("design.selection.assign", "global_write", "project"),
    ("design.selection.list", "read_only", "project"),
    ("design.selection.read", "read_only", "project"),
    ("design.selection.save", "global_write", "project"),
    // Governed metadata-discovered groups. `apply` and `unassign` write the
    // shared project record, so both are `global_write`; `preview` proposes
    // nothing durable and `export` is a projection of what is already stored,
    // so both stay `read_only` and usable on a project the caller cannot edit.
    // Every row is `project` authority: a group is assigned to transformers in
    // the paired session's selected project, never to a named one.
    ("design.group.apply", "global_write", "project"),
    ("design.group.export", "read_only", "project"),
    ("design.group.list", "read_only", "project"),
    ("design.group.preview", "read_only", "project"),
    ("design.group.unassign", "global_write", "project"),
    ("design.tag.define", "global_write", "project"),
    ("design.tag.list", "read_only", "project"),
    ("design.tag.query", "read_only", "project"),
    ("design.tag.set", "global_write", "project"),
    ("design.transformer.download", "local_ui", "project"),
    (
        "design.transformer.inventory",
        "local_auth_state",
        "headless_project",
    ),
    (
        "design.transformer.retire",
        "global_write",
        "headless_project",
    ),
    (
        "design.transformer.restore",
        "global_write",
        "headless_project",
    ),
    ("dsgrid-exchange.convert", "local_file_write", "none"),
    ("dsgrid-exchange.inspect", "discovery", "none"),
    ("dsgrid-exchange.plan", "discovery", "none"),
    ("dsgrid.apply", "local_file_write", "none"),
    ("dsgrid.describe", "discovery", "none"),
    ("dsgrid.inspect", "discovery", "none"),
    ("dsgrid.run", "read_only", "none"),
    ("dsgrid.validate", "discovery", "none"),
    // The paired application's local model lifecycle. `desktop_pairing` is
    // the exact authority and the load-bearing half of this family's
    // contract: a local model is browser-local state, so none of these four
    // requires — or may accept — a project. `local_ui` is the effect for the
    // three that change the application's own store and occupancy; nothing
    // durable is written outside it and nothing governed is published.
    ("dsgrid.model.create-local", "local_ui", "desktop_pairing"),
    (
        "dsgrid.model.import-external",
        "local_ui",
        "desktop_pairing",
    ),
    ("dsgrid.model.list", "read_only", "desktop_pairing"),
    ("dsgrid.model.set-active", "local_ui", "desktop_pairing"),
    // The one project act, and the only command in the family that carries
    // `project` authority: it registers one immutable revision in the paired
    // session's own selected project's catalogue, so it is `global_write` and
    // confirmation-gated. It activates nothing locally.
    ("dsgrid.publish-version", "global_write", "project"),
    ("feedback.close", "global_write", "desktop_user"),
    ("feedback.list", "read_only", "desktop_user"),
    ("feedback.submit", "global_write", "desktop_user"),
    ("library.catalog", "read_only", "none"),
    ("library.global.read", "read_only", "desktop_user"),
    ("library.global.write", "global_write", "desktop_user"),
    ("library.global.fork-example", "global_write", "project"),
    ("library.global.upload", "global_write", "desktop_user"),
    (
        "library.global.publish-library",
        "global_write",
        "desktop_user",
    ),
    (
        "library.global.publish-example",
        "global_write",
        "desktop_user",
    ),
    (
        "library.global.library-lifecycle",
        "global_write",
        "desktop_user",
    ),
    (
        "library.global.example-lifecycle",
        "global_write",
        "desktop_user",
    ),
    ("library.open", "read_only", "none"),
    ("library.pack", "local_file_write", "none"),
    ("library.resolve-native", "read_only", "none"),
    ("library.seed", "artifact_write", "none"),
    ("library.prepare-publication", "artifact_write", "none"),
    ("library.unpack", "local_file_write", "none"),
    ("library.verify", "read_only", "none"),
    ("map.design.attach-print", "artifact_write", "project"),
    ("map.design.batch.process", "local_ui", "project"),
    ("map.design.batch.report", "artifact_write", "project"),
    ("map.design.batch.save", "artifact_write", "project"),
    ("map.design.create", "local_ui", "project"),
    ("map.design.delete", "local_ui", "project"),
    ("map.design.discard", "local_ui", "project"),
    ("map.design.open", "local_ui", "project"),
    ("map.design.pin", "local_ui", "project"),
    ("map.design.version.list", "read_only", "project"),
    ("map.design.version.play", "local_ui", "project"),
    ("map.design.version.compare", "local_ui", "project"),
    ("map.design.geometry", "local_ui", "project"),
    ("map.design.layer-to-local", "local_ui", "project"),
    ("map.design.list", "read_only", "project"),
    ("map.design.process", "local_ui", "project"),
    ("map.design.read", "read_only", "project"),
    ("map.design.report", "artifact_write", "project"),
    ("map.design.save", "artifact_write", "project"),
    ("map.design.select", "read_only", "project"),
    ("map.design.set", "local_ui", "project"),
    ("map.design.setup", "local_ui", "project"),
    ("map.design.upload-to-local", "local_ui", "project"),
    ("map.design.upload.inspect", "read_only", "project"),
    ("map.design.upload.stage", "local_ui", "project"),
    ("map.design.version.begin", "artifact_write", "project"),
    ("map.draw", "local_ui", "desktop_pairing"),
    // A frame lands on the operator's own disk, so the effect is the file
    // write and not the panel that was opened to compose it. Authority stays
    // `desktop_pairing`: capturing what is already on screen proves a
    // transport, never a person, and reaches no project.
    (
        "map.evidence.capture",
        "local_file_write",
        "desktop_pairing",
    ),
    ("map.layer.add", "local_ui", "desktop_pairing"),
    ("map.layer.list", "read_only", "project"),
    ("map.layer.remote-list", "read_only", "desktop_pairing"),
    ("map.layer.remove", "local_ui", "desktop_pairing"),
    ("map.layer.reorder", "global_write", "project"),
    ("map.layer.visibility", "local_ui", "desktop_pairing"),
    ("map.line-difference", "local_ui", "desktop_pairing"),
    ("map.outliers", "local_ui", "desktop_pairing"),
    ("map.points-along", "local_ui", "desktop_pairing"),
    ("map.random-points", "local_ui", "desktop_pairing"),
    ("map.remove", "local_ui", "desktop_pairing"),
    ("map.survey.download", "local_ui", "project"),
    ("map.survey.migrate.apply", "global_write", "project"),
    ("map.survey.migrate.plan", "read_only", "project"),
    ("map.ui.open", "local_ui", "desktop_pairing"),
    ("map.view", "read_only", "desktop_pairing"),
    ("map.zoom", "local_ui", "desktop_pairing"),
    ("mcp.install", "machine_write", "none"),
    ("mcp.serve", "read_only", "none"),
    ("pls.backup-create", "artifact_write", "none"),
    ("pls.compare-don", "discovery", "none"),
    ("pls.delivery-verify", "discovery", "none"),
    ("pls.deviation-labels", "local_file_write", "none"),
    ("pls.pole-capacity.read", "discovery", "none"),
    ("pls.reference-closure", "discovery", "none"),
    ("pls.section-orientation", "discovery", "none"),
    ("pls.shading-variants", "local_file_write", "none"),
    ("pls.terrain-reconcile", "local_file_write", "none"),
    ("report.bundle", "local_file_write", "none"),
    ("report.engine", "discovery", "none"),
    ("report.export", "local_file_write", "none"),
    ("report.tasks", "discovery", "none"),
    (
        "report.project.scope",
        "local_auth_state",
        "headless_project",
    ),
    (
        "report.project.compounded",
        "artifact_write",
        "headless_project",
    ),
    (
        "report.project.archives",
        "local_auth_state",
        "headless_project",
    ),
    ("shell.register", "local_file_write", "none"),
    ("shell.status", "discovery", "none"),
    ("shell.unregister", "local_file_write", "none"),
    ("solar.engine", "discovery", "none"),
    ("solar.final.import", "artifact_write", "desktop_user"),
    ("solar.final.submit", "artifact_write", "desktop_user"),
    (
        "solar.input.capture",
        "local_file_write",
        "headless_project",
    ),
    ("solar.input.prepare", "local_file_write", "none"),
    ("solar.portfolio.analysis", "read_only", "desktop_user"),
    ("solar.portfolio.create", "global_write", "desktop_user"),
    ("solar.portfolio.delete", "global_write", "desktop_user"),
    ("solar.portfolio.export", "local_file_write", "desktop_user"),
    ("solar.portfolio.list", "read_only", "desktop_user"),
    ("solar.portfolio.read", "read_only", "desktop_user"),
    ("solar.portfolio.update", "global_write", "desktop_user"),
    ("solar.prepare", "local_file_write", "desktop_user"),
    ("solar.report.export", "local_file_write", "desktop_user"),
    ("solar.report.bundle", "local_file_write", "desktop_user"),
    ("solar.result.compare", "read_only", "none"),
    ("solar.result.read", "read_only", "desktop_user"),
    ("solar.results.read", "read_only", "desktop_user"),
    ("solar.run", "local_file_write", "none"),
    ("solar.run.cancel", "local_ui", "desktop_user"),
    ("solar.run.progress", "read_only", "desktop_user"),
    ("solar.run.result", "read_only", "desktop_user"),
    ("solar.run.start", "local_file_write", "desktop_user"),
    // Seeding is a governed ds-brain copy into a project's Solar root, so
    // `apply` is `global_write` and not `artifact_write`: what changes is
    // shared project state, not a durable file. Both carry `project`
    // authority because the destination IS the paired session's selected
    // project — the CLI never names one — and preview keeps `read_only`
    // exactly as ds-brain classifies `seed_preview`, so it stays usable on a
    // read-only project.
    ("solar.seed.apply", "global_write", "project"),
    ("solar.seed.network-plan", "local_file_write", "none"),
    ("solar.seed.preview", "read_only", "project"),
    ("solar.sync.status", "read_only", "desktop_user"),
    ("solar.verify-weather", "read_only", "none"),
    ("sre.events", "read_only", "desktop_user"),
    ("sre.overview", "read_only", "desktop_user"),
    ("survey.form.create", "global_write", "desktop_user"),
    ("survey.form.lifecycle", "global_write", "desktop_user"),
    ("survey.form.read", "read_only", "desktop_user"),
    ("survey.form.types", "read_only", "desktop_user"),
    ("survey.form.update", "global_write", "desktop_user"),
    ("survey.forms.list", "read_only", "desktop_user"),
    (
        "survey.project.create-from-template",
        "global_write",
        "desktop_user",
    ),
    ("survey.project-form.editor", "read_only", "desktop_user"),
    (
        "survey.project-form.settings",
        "local_auth_state",
        "headless_project",
    ),
    ("survey.project-forms.apply", "global_write", "desktop_user"),
    (
        "survey.project-forms.list",
        "local_auth_state",
        "headless_project",
    ),
    ("survey.project-forms.plan", "proposal", "desktop_user"),
    ("survey.project-forms.read", "read_only", "desktop_user"),
    ("survey.query", "local_auth_state", "headless_project"),
    (
        "survey.entries.select",
        "local_auth_state",
        "headless_project",
    ),
    (
        "survey.entries.changes",
        "local_auth_state",
        "headless_project",
    ),
    ("survey.entries.create", "global_write", "headless_project"),
    ("survey.entries.import", "global_write", "headless_project"),
    ("survey.template.apply", "global_write", "desktop_user"),
    ("survey.template.create", "global_write", "desktop_user"),
    ("survey.template.lifecycle", "global_write", "desktop_user"),
    ("survey.template.read", "read_only", "desktop_user"),
    ("survey.templates.list", "read_only", "desktop_user"),
    ("style.appearance.plan", "read_only", "project"),
    ("style.appearance.set", "global_write", "project"),
    ("style.cartography.plan", "read_only", "project"),
    ("style.cartography.set", "global_write", "project"),
    ("style.dimension.clear", "global_write", "project"),
    ("style.dimension.plan", "read_only", "project"),
    ("style.dimension.set", "global_write", "project"),
    ("style.list", "read_only", "project"),
    ("style.read", "read_only", "project"),
    ("tile.add", "global_write", "project"),
    ("tile.generate", "global_write", "headless_project"),
    ("tile.list", "read_only", "project"),
    ("tile.plan", "local_auth_state", "headless_project"),
    ("tile.preflight", "local_auth_state", "headless_project"),
    ("tile.remove", "global_write", "project"),
    ("tile.status", "local_auth_state", "headless_project"),
    ("work.plan", "read_only", "project"),
    ("work.record.list", "read_only", "project"),
    ("work.record.read", "read_only", "project"),
    ("work.task.assign", "global_write", "project"),
    ("work.task.create", "global_write", "project"),
    ("work.task.list", "read_only", "project"),
    ("work.task.read", "read_only", "project"),
    ("work.task.respond", "global_write", "project"),
    ("work.task.update", "global_write", "project"),
    ("workstation.components", "discovery", "none"),
    ("workstation.configure", "machine_write", "none"),
    ("workstation.install", "machine_write", "none"),
    ("workstation.plan", "proposal", "none"),
    ("workstation.status", "discovery", "none"),
    ("workstation.verify", "read_only", "none"),
];

fn json(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds runs");
    assert!(
        output.status.success(),
        "`ds {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("one JSON envelope")
}

#[test]
fn every_shipping_command_has_a_pinned_semantic_contract_and_safe_invocation() {
    let index = json(&["capabilities", "--output", "json"]);
    let mut actual = BTreeMap::new();
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let domain_id = domain["id"].as_str().expect("domain id");
        let commands = json(&["capabilities", domain_id, "--output", "json"]);
        for command in commands["data"]["commands"].as_array().expect("commands") {
            let id = command["id"].as_str().expect("command id");
            let descriptor = json(&["capabilities", id, "--output", "json"]);
            actual.insert(id.to_string(), descriptor["data"]["command"].clone());
        }
    }

    let expected: BTreeMap<&str, (&str, &str)> = EXPECTED
        .iter()
        .map(|(id, effect, authority)| (*id, (*effect, *authority)))
        .collect();
    assert_eq!(
        EXPECTED.len(),
        expected.len(),
        "semantic inventory contains a duplicate id"
    );
    assert_eq!(
        actual.keys().cloned().collect::<BTreeSet<_>>(),
        expected.keys().map(|id| (*id).to_string()).collect(),
        "the shipping surface changed; review the new command's exact effect/authority and add it here"
    );

    for (id, descriptor) in actual {
        let (effect, authority) = expected[&*id];
        assert_eq!(
            descriptor["effect"], effect,
            "`{id}` effect changed without a reviewed semantic assertion"
        );
        assert_eq!(
            descriptor["authority"], authority,
            "`{id}` authority changed without a reviewed semantic assertion"
        );
        let path: Vec<&str> = descriptor["path"]
            .as_array()
            .expect("path")
            .iter()
            .map(|part| part.as_str().expect("path part"))
            .collect();
        let mut help = path;
        help.push("--help");
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(&help)
            .env("NO_COLOR", "1")
            .output()
            .expect("safe help runs");
        assert!(
            output.status.success(),
            "`ds {} --help` did not resolve",
            help[..help.len() - 1].join(" ")
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(descriptor["summary"].as_str().expect("summary")),
            "`{id}` help did not reach its own semantic contract"
        );
    }
}
