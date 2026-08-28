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
    ("desktop.project.list", "read_only", "desktop_user"),
    ("desktop.project.switch", "local_ui", "desktop_user"),
    ("desktop.status", "discovery", "none"),
    ("dsgrid-exchange.convert", "local_file_write", "none"),
    ("dsgrid-exchange.inspect", "discovery", "none"),
    ("dsgrid-exchange.plan", "discovery", "none"),
    ("dsgrid.apply", "local_file_write", "none"),
    ("dsgrid.describe", "discovery", "none"),
    ("dsgrid.inspect", "discovery", "none"),
    ("dsgrid.validate", "discovery", "none"),
    ("feedback.close", "global_write", "desktop_user"),
    ("feedback.list", "read_only", "desktop_user"),
    ("feedback.submit", "global_write", "desktop_user"),
    ("library.catalog", "read_only", "none"),
    ("library.open", "read_only", "none"),
    ("library.pack", "local_file_write", "none"),
    ("library.resolve-native", "read_only", "none"),
    ("library.seed", "artifact_write", "none"),
    ("library.unpack", "local_file_write", "none"),
    ("library.verify", "read_only", "none"),
    ("map.design.attach-print", "artifact_write", "project"),
    ("map.design.batch.process", "local_ui", "project"),
    ("map.design.batch.report", "artifact_write", "project"),
    ("map.design.batch.save", "artifact_write", "project"),
    ("map.design.create", "local_ui", "project"),
    ("map.design.delete", "local_ui", "project"),
    ("map.design.discard", "local_ui", "project"),
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
    ("pls.compare-don", "discovery", "none"),
    ("pls.delivery-verify", "discovery", "none"),
    ("pls.deviation-labels", "local_file_write", "none"),
    ("pls.pole-capacity.read", "discovery", "none"),
    ("pls.reference-closure", "discovery", "none"),
    ("pls.section-orientation", "discovery", "none"),
    ("pls.terrain-reconcile", "local_file_write", "none"),
    ("report.bundle", "local_file_write", "none"),
    ("report.engine", "discovery", "none"),
    ("report.export", "local_file_write", "none"),
    ("report.tasks", "discovery", "none"),
    ("shell.register", "local_file_write", "none"),
    ("shell.status", "discovery", "none"),
    ("shell.unregister", "local_file_write", "none"),
    ("solar.engine", "discovery", "none"),
    ("solar.final.import", "artifact_write", "desktop_user"),
    ("solar.final.submit", "artifact_write", "desktop_user"),
    ("solar.portfolio.export", "local_file_write", "desktop_user"),
    ("solar.portfolio.list", "read_only", "desktop_user"),
    ("solar.portfolio.read", "read_only", "desktop_user"),
    ("solar.prepare", "local_file_write", "desktop_user"),
    ("solar.report.export", "local_file_write", "desktop_user"),
    ("solar.result.read", "read_only", "desktop_user"),
    ("solar.results.read", "read_only", "desktop_user"),
    ("solar.run", "local_file_write", "none"),
    ("solar.run.cancel", "local_ui", "desktop_user"),
    ("solar.run.progress", "read_only", "desktop_user"),
    ("solar.run.result", "read_only", "desktop_user"),
    ("solar.run.start", "local_file_write", "desktop_user"),
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
    ("survey.project-forms.apply", "global_write", "desktop_user"),
    ("survey.project-forms.plan", "proposal", "desktop_user"),
    ("survey.project-forms.read", "read_only", "desktop_user"),
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
    ("tile.generate", "global_write", "project"),
    ("tile.list", "read_only", "project"),
    ("tile.plan", "read_only", "project"),
    ("tile.preflight", "read_only", "project"),
    ("tile.remove", "global_write", "project"),
    ("tile.status", "read_only", "project"),
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
