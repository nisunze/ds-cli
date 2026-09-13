//! Explicitly refresh independent Solar inputs from project-owned sources.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::assets::Link;
use serde_json::{Value, json};

const FORM_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "form_file",
        when: "the form file is unreadable, oversized or invalid JSON",
        remedy: "provide a readable reference form JSON of at most 16 MiB",
    },
    Refusal {
        code: "network_form",
        when: "the form or owner receipt violates the shared network contract",
        remedy: "start with solar.network.resolve and edit only its declared manual fields or references",
    },
    Refusal {
        code: "asset_project",
        when: "the selected asset project differs from the Solar workspace",
        remedy: "select the workspace project with auth.project.use",
    },
    Refusal {
        code: "city_tag",
        when: "only part of the city tag is provided",
        remedy: "provide tag-definition and tag-value together, or omit both for manual entry",
    },
];
const fn form_refusals() -> [Refusal;
    FORM_REFUSALS.len()
        + crate::project::SEED.refusals.len()
        + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()] {
    let mut out = [FORM_REFUSALS[0];
        FORM_REFUSALS.len()
            + crate::project::SEED.refusals.len()
            + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()];
    let parts = [
        FORM_REFUSALS,
        crate::project::SEED.refusals,
        ds_cli_auth::PROJECT_STATUS_COMMAND.refusals,
    ];
    let mut i = 0;
    let mut offset = 0;
    while i < parts.len() {
        let mut j = 0;
        while j < parts[i].len() {
            out[offset] = parts[i][j];
            offset += 1;
            j += 1;
        }
        i += 1;
    }
    out
}
pub static COMMAND: Command = Command {
    id: "solar.network.resolve",
    path: &["solar", "network", "resolve"],
    contract: 1,
    summary: "Copy network data into editable independent Solar inputs.",
    purpose: "Resolve the classified sizing table and city map by an exact project tag, then transformer maps by identity. Missing artifacts seed manual entry and never block the city. Copies classified values into Solar and queues normal sync. Map bytes are copied through the verified Solar media service. Source identities record provenance only; future runs use the owned inputs. Repeating this command explicitly refreshes sources while preserving manual overrides. Omit the tag to use the form's saved binding or seed a wholly manual network.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value("workspace", "<dir>", "Existing local Solar workspace.").required(),
        Arg::value(
            "city",
            "<id>",
            "Existing city to seed; no geographic data required.",
        )
        .required(),
        Arg::value(
            "lane",
            "<stable|canary>",
            "Credential lane for shared asset reads; default stable.",
        )
        .choices(&["stable", "canary"]),
        Arg::value(
            "tag-definition",
            "<id>",
            "Exact city tag definition; pair with tag-value.",
        ),
        Arg::value(
            "tag-value",
            "<value>",
            "Exact city tag value; pair with tag-definition.",
        ),
    ],
    output: "Solar-owned input snapshot, editable effective values, source notices and sync receipt. Manual entry remains enabled.",
    examples: &[],
    refusals: &form_refusals(),
    reference: Some("docs/reference/solar.md"),
    availability: || crate::DS_SOLAR.availability(),
};

pub fn read_information(lane: &str, project: &str, reference: &Value) -> Result<Value, Failure> {
    let receipt = ds_cli_auth::shared_assets(
        lane,
        &ds_cli_auth::SharedAssetsCommand::Read {
            asset_id: reference["asset_id"].as_str().unwrap_or_default().into(),
            digest: reference["digest"].as_str().unwrap_or_default().into(),
        },
    )?;
    if receipt.project_id() != project {
        return Err(Failure::invalid(
            "asset_project",
            "Select the Solar workspace's project before resolving its shared assets.",
        ));
    }
    let information = json!({"reference":reference,"base64":receipt.result()["base64"]});
    let request = serde_json::to_vec(
        &json!({"operation":"information","project_id":project,"information":information}),
    )
    .map_err(|e| Failure::invalid("network_form", e.to_string()))?;
    ds_command_kernel::solar_network_form::evaluate(&request)
        .map_err(|e| Failure::invalid("network_form", e))?;
    Ok(information)
}

pub static SAVE: Command = Command {
    id:"solar.network.save", path:&["solar","network","save"], contract:1,
    summary:"Save a Solar network's copied seed and manual overrides.",
    purpose:"Save a complete ds-solar.network-form/v1 document with sources and object overrides. An empty sources object is valid: enter customers, proposed transformers and sizing assumptions manually. Per-transformer overrides are keyed by exact name: each new row needs name, phase (for example I), kva, bt_km (LV length in kilometres), and numeric customer-category columns (for example Pauvre). Put minimum_load_profile and consider_inst in overrides; MV length is overrides.proposed.total_mv_length_km. Uses the expected digest returned by solar.network.resolve to protect concurrent edits. Copied inputs and operator edits enter the workspace and normal sync outbox; source refresh is always explicit.",
    chapter:Chapter::Solar,effect:Effect::LocalFileWrite,authority:Authority::None,execution:Execution::Sync,
    args:&[
        Arg::value("workspace","<dir>","Existing Solar workspace.").required(),
        Arg::value("city","<id>","Existing city; geography is optional.").required(),
        Arg::value("expected","<digest>","Current city digest returned by solar.network.resolve.").required(),
        Arg::value("form","<file>","Complete reference form JSON, up to 16 MiB. Start with the document returned by solar.network.resolve; edit overrides or transformer_overrides.").required(),
    ],
    output:"Saved Solar-owned input document, expected digest for the next edit, manual effective values and pending sync receipt.",
    examples:&[],refusals:&form_refusals(),reference:Some("docs/reference/solar.md"),availability:||crate::DS_SOLAR.availability(),
};
pub fn save(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    use std::io::Read;
    let path = i.require("form")?;
    let file =
        std::fs::File::open(path).map_err(|e| Failure::invalid("form_file", e.to_string()))?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Failure::invalid("form_file", e.to_string()))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(Failure::invalid(
            "form_file",
            "Network form exceeds 16 MiB.",
        ));
    }
    let document: Value =
        serde_json::from_slice(&bytes).map_err(|e| Failure::invalid("form_file", e.to_string()))?;
    crate::project::invoke(
        json!({"operation":"network_form_write","workspace":i.require("workspace")?,"city":i.require("city")?,"expected":i.require("expected")?,"document":document}),
    )
}
pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let workspace = i.require("workspace")?;
    let city = i.require("city")?;
    let held = crate::project::invoke(
        json!({"operation":"network_form_read","workspace":workspace,"city":city}),
    )?;
    let project = held["project_id"]
        .as_str()
        .ok_or_else(|| Failure::failed("network_form", "Workspace omitted project identity"))?;
    let mut document = held["document"].clone();
    match (i.value("tag-definition"), i.value("tag-value")) {
        (Some(definition), Some(value)) => {
            document["city_tag"] = json!({"definition_id":definition,"value":value})
        }
        (None, None) => {}
        _ => {
            return Err(Failure::invalid(
                "city_tag",
                "Use tag-definition and tag-value together.",
            ));
        }
    }
    let lane = i.value("lane").unwrap_or("stable");
    let mut notices = Vec::new();
    let mut information = Value::Null;
    let mut information_available = false;
    if let (Some(definition), Some(value)) = (
        document["city_tag"]["definition_id"].as_str(),
        document["city_tag"]["value"].as_str(),
    ) {
        let link = Link::Tag {
            definition_id: definition.into(),
            value: value.into(),
        };
        for (key, role) in [
            ("information", "network_information"),
            ("city_map", "city_map"),
        ] {
            let result = ds_cli_auth::shared_assets(
                lane,
                &ds_cli_auth::SharedAssetsCommand::Resolve {
                    link: link.clone(),
                    role: role.into(),
                },
            );
            match result {
                Ok(receipt) if receipt.project_id() == project => {
                    let resolution = &receipt.result()["resolution"];
                    if resolution["status"] == "resolved" {
                        let asset = &resolution["asset"];
                        document["sources"][key] = json!({"project_id":project,"asset_id":asset["asset_id"],"digest":asset["digest"]});
                        if key == "information" { information_available = true; }
                    } else {
                        notices.push(json!({"role":role,"status":resolution["status"],"manual_entry_allowed":true}));
                    }
                },
                Ok(_) => return Err(Failure::invalid("asset_project", "Select the Solar workspace's project before resolving its assets.")),
                Err(error) => notices.push(json!({"role":role,"status":"unavailable","message":error.to_string(),"manual_entry_allowed":true})),
            }
        }
        if information_available {
            match read_information(lane, project, &document["sources"]["information"]) {
                Ok(value) => information = value,
                Err(error) => notices.push(json!({"role":"network_information","status":"unavailable","message":error.to_string(),"manual_entry_allowed":true})),
            }
        }
    }
    if held["legacy"] == true && !information.is_null() {
        let request=serde_json::to_vec(&json!({"operation":"adopt","project_id":project,"document":document,"information":information})).map_err(|e|Failure::invalid("network_form",e.to_string()))?;
        let adopted = ds_command_kernel::solar_network_form::evaluate(&request)
            .map_err(|e| Failure::invalid("network_form", e))?;
        document = serde_json::from_str(&adopted)
            .map_err(|e| Failure::invalid("network_form", e.to_string()))?;
    }
    let seed_request = json!({"operation":"seed","project_id":project,"document":document,"information":information});
    let seed = ds_command_kernel::solar_network_form::evaluate(
        &serde_json::to_vec(&seed_request).unwrap(),
    );
    let values: Value = match seed {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|_| Failure::failed("network_form", "Invalid form seed"))?,
        Err(error) => {
            notices.push(json!({"role":"network_information","status":"unavailable","message":error,"manual_entry_allowed":true}));
            information = Value::Null;
            serde_json::from_str(
                &ds_command_kernel::solar_network_form::evaluate(
                    &serde_json::to_vec(
                        &json!({"operation":"seed","project_id":project,"document":document}),
                    )
                    .unwrap(),
                )
                .map_err(|e| Failure::invalid("network_form", e))?,
            )
            .unwrap()
        }
    };
    if let Some(transformers) = values["proposed"]["transformers"]
        .as_array()
        .filter(|_| !document["city_tag"].is_null())
    {
        let mut maps = document["sources"]["transformer_maps"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for transformer in transformers {
            let Some(name) = transformer["name"].as_str() else {
                continue;
            };
            if let Ok(receipt) = ds_cli_auth::shared_assets(
                lane,
                &ds_cli_auth::SharedAssetsCommand::Resolve {
                    link: Link::DsObject {
                        object_type: "transformer".into(),
                        entity_id: name.into(),
                    },
                    role: "transformer_map".into(),
                },
            ) {
                if receipt.project_id() != project {
                    return Err(Failure::invalid(
                        "asset_project",
                        "Project changed while resolving maps.",
                    ));
                }
                if receipt.result()["resolution"]["status"] == "resolved" {
                    let asset = &receipt.result()["resolution"]["asset"];
                    maps.insert(name.into(), json!({"project_id":project,"asset_id":asset["asset_id"],"digest":asset["digest"]}));
                }
            }
        }
        document["sources"]["transformer_maps"] = Value::Object(maps);
    }
    let mut copies = Vec::new();
    if document["sources"]["city_map"].is_object() {
        copies.push((
            "network.map:reseau_propose".to_string(),
            document["sources"]["city_map"].clone(),
        ));
    }
    if let Some(maps) = document["sources"]["transformer_maps"].as_object() {
        for (name, reference) in maps {
            copies.push((format!("network.transformer:{name}"), reference.clone()));
        }
    }
    for (role, reference) in copies {
        let (kind, key) = role.split_once(':').expect("constructed role");
        let (old_source, old_map) = if kind == "network.map" {
            (
                &held["document"]["sources"]["city_map"],
                &held["document"]["seeded"]["maps"][key],
            )
        } else {
            (
                &held["document"]["sources"]["transformer_maps"][key],
                &held["document"]["seeded"]["maps"]["transformers"][key],
            )
        };
        if old_source == &reference && old_map["kind"] == "solar_network_media" {
            continue;
        }
        match copy_map(lane, project, city, &role, &reference, workspace) {
            Ok(copied) => {
                let (kind, key) = role.split_once(':').expect("constructed role");
                if kind == "network.map" {
                    document["seeded"]["maps"][key] = copied;
                    if held["legacy"] == true && let Some(m) = document["overrides"]["maps"].as_object_mut() { m.remove(key); }
                } else {
                    document["seeded"]["maps"]["transformers"][key] = copied;
                    if held["legacy"] == true && let Some(m) = document["overrides"]["maps"]["transformers"].as_object_mut() { m.remove(key); }
                }
            }
            Err(error) => notices.push(json!({"role":role,"status":"copy_unavailable","message":error.to_string(),"manual_entry_allowed":true})),
        }
    }
    let mut result = crate::project::invoke(
        json!({"operation":"network_form_write","workspace":workspace,"city":city,"expected":held["expected"],"document":document,"information":information}),
    )?;
    result["source_notices"] = json!(notices);
    Ok(result)
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn copy_map(
    lane: &str,
    project: &str,
    city: &str,
    role: &str,
    reference: &Value,
    workspace: &str,
) -> Result<Value, Failure> {
    use base64::Engine;
    let receipt = ds_cli_auth::shared_assets(
        lane,
        &ds_cli_auth::SharedAssetsCommand::Read {
            asset_id: reference["asset_id"].as_str().unwrap_or_default().into(),
            digest: reference["digest"].as_str().unwrap_or_default().into(),
        },
    )?;
    if receipt.project_id() != project {
        return Err(Failure::invalid(
            "asset_project",
            "Project changed while copying a map.",
        ));
    }
    let encoded = receipt.result()["base64"]
        .as_str()
        .ok_or_else(|| Failure::invalid("network_form", "Map read omitted bytes"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| Failure::invalid("network_form", e.to_string()))?;
    let mut session = ds_cli_auth::solar_project_session(lane)?;
    if session.binding()["project"] != project {
        return Err(Failure::invalid(
            "asset_project",
            "Project changed while copying a map.",
        ));
    }
    let result = session.execute(&ds_cli_auth::SolarProjectCommand::ImportMap {
        city: city.into(),
        role: role.into(),
        file_name: format!("{}.png", reference["asset_id"].as_str().unwrap_or("map")),
        bytes,
    })?;
    crate::project::invoke(
        json!({"operation":"network_media_cache","workspace":workspace,"reference":result["reference"],"base64":encoded}),
    )?;
    Ok(result["reference"].clone())
}
