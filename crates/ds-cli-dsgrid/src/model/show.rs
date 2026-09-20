//! `ds dsgrid model show` — one working copy in full: its catalogue row,
//! the identity its package declares, and the PLS-CADD workspace it is
//! linked to.
//!
//! `list` is the index; this is the record. It opens the package through the
//! engine so what it prints about revision and coordinate system is what the
//! bytes say now, beside what the row recorded when the copy was acquired —
//! a copy edited in place shows both.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
    Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::model::{pls_source, workspace};

const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<model-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The working copy, by the id `ds dsgrid model list` reports.",
};

const SHOW_OWN: [Refusal; 1] = [Refusal {
    code: "not_a_dsgrid_package",
    when: "the store holds bytes this build's engine cannot open as a package",
    remedy: "re-import the package; a package written by a newer schema is refused, not guessed",
}];
const REFUSALS: &[Refusal; SHOW_OWN.len() + workspace::REFUSALS.len()] = &refusals();
const fn refusals() -> [Refusal; SHOW_OWN.len() + workspace::REFUSALS.len()] {
    let mut all = [SHOW_OWN[0]; SHOW_OWN.len() + workspace::REFUSALS.len()];
    let mut shared = 0;
    while shared < workspace::REFUSALS.len() {
        all[SHOW_OWN.len() + shared] = workspace::REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.model.show",
    path: &["dsgrid", "model", "show"],
    contract: 1,
    summary: "Show one working copy: row, package identity, PLS-CADD link.",
    purpose: "\
Prints one of this machine's working copies in full: the catalogue row \
(name, origin, revision, digest, project pin, head revision), what its \
package declares now (model id, package revision, coordinate system, table \
counts), and the PLS-CADD workspace it is linked to with the member versions \
the link pinned. Reads the package through the engine; writes nothing.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[MODEL_ARG, workspace::LANE_ARG, workspace::ACCOUNT_ARG],
    output: "\
The `model` row with `pls_source`, and `package` {model_id, package_revision, \
crs, structures, structure_types, tension_sections, terrain_points, bytes, \
sha256, path}.",
    examples: &[Example {
        command: "ds dsgrid model show --model local-5ff16cd0a3d6416b --account <uid> --output json",
        note: "Read .data.model.pls_source to see where a sync would write.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["working copy", "linked workspace", "details"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = inputs.require("model")?.trim().to_owned();
    let opened = pls_source::open(inputs, &id)?;
    let snapshot = &opened.package.snapshot;
    Ok(json!({
        "model": workspace::row(&opened.row, opened.active.as_deref()),
        "package": {
            "model_id": opened.package.manifest.model.model_id.as_str(),
            "package_revision": opened.package.manifest.model.model_revision,
            "crs": opened.package.manifest.model.coordinate_system.to_string(),
            "structures": snapshot.structures.len(),
            "structure_types": snapshot.structure_types.len(),
            "tension_sections": snapshot.tension_sections.len(),
            "terrain_points": snapshot.terrain_points.len(),
            "bytes": opened.package_bytes.len(),
            "sha256": format!("sha256:{}", ds_io::pls_cadd_native::sha256_hex_digest(&opened.package_bytes)),
            "path": opened.package_path.to_string_lossy(),
        },
    }))
}

pub fn render(data: &Value) -> String {
    let model = &data["model"];
    let package = &data["package"];
    let mut out = format!(
        "{}{} · {}\n  origin     {}\n  revision   {}   head {}\n  crs        {}\n  package    rev {} · {} bytes · {}\n  content    {} structures · {} types · {} sections · {} terrain points\n",
        if model["active"].as_bool().unwrap_or(false) {
            "* "
        } else {
            ""
        },
        model["model"].as_str().unwrap_or("?"),
        model["name"].as_str().unwrap_or(""),
        model["origin"].as_str().unwrap_or("?"),
        model["revision"],
        model["head_revision"].as_str().unwrap_or("—"),
        package["crs"].as_str().unwrap_or("?"),
        package["package_revision"],
        package["bytes"],
        package["path"].as_str().unwrap_or("?"),
        package["structures"],
        package["structure_types"],
        package["tension_sections"],
        package["terrain_points"],
    );
    match model["pls_source"].as_object() {
        Some(link) => {
            let versions = link
                .get("member_versions")
                .and_then(Value::as_object)
                .map(|versions| {
                    versions
                        .iter()
                        .map(|(family, version)| {
                            format!("{family} {}", version.as_str().unwrap_or("?"))
                        })
                        .collect::<Vec<_>>()
                        .join(" · ")
                })
                .unwrap_or_default();
            out.push_str(&format!(
                "  linked     {}\n             PLS-CADD {} · {} members · {}\n             {}\n",
                link.get("path").and_then(Value::as_str).unwrap_or("?"),
                link.get("pls_version")
                    .and_then(Value::as_str)
                    .unwrap_or("?"),
                link.get("member_count").cloned().unwrap_or(Value::Null),
                versions,
                link.get("digest").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        None => {
            out.push_str("  linked     none — `ds dsgrid model link` to pin a PLS-CADD workspace\n")
        }
    }
    out
}
