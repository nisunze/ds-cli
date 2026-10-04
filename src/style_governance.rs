//! Closed Styles API operations. Seeding and retirement remain server effects;
//! hosts neither construct style ids nor edit inventory documents themselves.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Table,
    Manifest,
    PlanSeed,
    PlanA4,
    CreateA4 {
        expected_definition_sha256: String,
        expected_plan_sha256: String,
    },
    Seed {
        expected_manifest_revision: String,
        expected_plan_sha256: String,
    },
    Inventory {
        #[serde(default)]
        all_projects: bool,
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default = "page_size")]
        limit: u16,
    },
    Backup {
        expected_inventory_sha256: String,
        #[serde(default)]
        all_projects: bool,
    },
    ReadBackup {
        backup_id: String,
        #[serde(default)]
        all_projects: bool,
    },
    PlanRetirement {
        expected_inventory_sha256: String,
        backup_id: String,
        #[serde(default)]
        all_projects: bool,
    },
}
fn page_size() -> u16 {
    100
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub project: String,
    pub command: Command,
}
fn exact(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text.trim() == text
        && !text.chars().any(char::is_control)
}
fn digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|c| c.is_ascii_hexdigit())
}
impl Command {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Table => "get_style_table",
            Self::Manifest => "get_seed_manifest",
            Self::PlanSeed => "plan_seed_manifest",
            Self::PlanA4 => "plan_printing_a4",
            Self::CreateA4 { .. } => "create_printing_a4",
            Self::Seed { .. } => "seed_manifest",
            Self::Inventory { .. } => "inventory_styles",
            Self::Backup { .. } => "backup_inventory",
            Self::ReadBackup { .. } => "read_inventory_backup",
            Self::PlanRetirement { .. } => "plan_retirement",
        }
    }
}
pub fn request_body(request: &Request) -> Result<Value, String> {
    if !exact(&request.project, 200) {
        return Err("style_governance_invalid: one exact project is required".into());
    }
    let mut data = json!({"project_id":request.project});
    match &request.command {
        Command::CreateA4 {
            expected_definition_sha256,
            expected_plan_sha256,
        } => {
            if !digest(expected_definition_sha256) || !digest(expected_plan_sha256) {
                return Err("style_governance_invalid: A4 create needs its reviewed definition and plan digests".into());
            }
            data["expected_definition_sha256"] = json!(expected_definition_sha256);
            data["expected_plan_sha256"] = json!(expected_plan_sha256);
        }
        Command::Seed {
            expected_manifest_revision,
            expected_plan_sha256,
        } => {
            if !digest(expected_manifest_revision) || !digest(expected_plan_sha256) {
                return Err(
                    "style_governance_invalid: seed needs the reviewed manifest and plan digests"
                        .into(),
                );
            }
            data["expected_manifest_revision"] = json!(expected_manifest_revision);
            data["expected_plan_sha256"] = json!(expected_plan_sha256);
        }
        Command::Inventory { cursor, limit, .. } => {
            if !(1..=200).contains(limit) || cursor.as_ref().is_some_and(|c| !exact(c, 2000)) {
                return Err("style_governance_invalid: inventory requires limit 1..200 and an exact opaque cursor".into());
            }
            data["limit"] = json!(limit);
            if let Some(cursor) = cursor {
                data["cursor"] = json!(cursor);
            }
        }
        Command::Backup {
            expected_inventory_sha256,
            ..
        }
        | Command::PlanRetirement {
            expected_inventory_sha256,
            ..
        } => {
            if !digest(expected_inventory_sha256) {
                return Err(
                    "style_governance_invalid: inventory digest must be the reviewed 64hex value"
                        .into(),
                );
            }
            data["expected_inventory_sha256"] = json!(expected_inventory_sha256);
        }
        _ => {}
    }
    if let Command::Inventory { all_projects, .. }
    | Command::Backup { all_projects, .. }
    | Command::ReadBackup { all_projects, .. }
    | Command::PlanRetirement { all_projects, .. } = &request.command
    {
        data["all_projects"] = json!(all_projects);
    }
    if let Command::ReadBackup { backup_id, .. } | Command::PlanRetirement { backup_id, .. } =
        &request.command
    {
        if !exact(backup_id, 200) {
            return Err("style_governance_invalid: one exact backup id is required".into());
        }
        data["backup_id"] = json!(backup_id);
    }
    Ok(json!({"action":request.command.action(),"data":data}))
}
pub fn admit(request: &Request, response: &Value) -> Result<Value, String> {
    if response["success"] != true || !response["data"].is_object() {
        return Err("style_governance_invalid: missing governed API receipt".into());
    }
    let data = &response["data"];
    let requested_scope = match &request.command {
        Command::Inventory { all_projects, .. }
        | Command::Backup { all_projects, .. }
        | Command::ReadBackup { all_projects, .. }
        | Command::PlanRetirement { all_projects, .. } => Some(*all_projects),
        _ => None,
    };
    if let Some(all_projects) = requested_scope {
        if data["scope"]["project_id"] != request.project.as_str()
            || data["scope"]["all_projects"] != all_projects
        {
            return Err(
                "project_context_changed: inventory or backup belongs to another captured scope"
                    .into(),
            );
        }
    }
    match &request.command {
        Command::PlanA4 => {
            admit_a4_plan(request, data)?;
        }
        Command::CreateA4 {
            expected_definition_sha256,
            expected_plan_sha256,
        } => {
            if data.as_object().is_none_or(|object| object.len() != 5)
                || data["schema"] != "ds.print-a4-seed-result/v1"
                || data["applied"] != true
            {
                return Err("style_governance_invalid: incomplete fixed A4 create receipt".into());
            }
            let plan = admit_a4_plan(request, &data["plan"])?;
            if !plan.apply_allowed
                || plan.definition_sha256 != *expected_definition_sha256
                || plan.plan_sha256 != *expected_plan_sha256
                || data["created"] != (plan.creates == 1)
                || data["preserved"] != (plan.preserves == 1)
            {
                return Err(
                    "style_governance_invalid: A4 create differs from the reviewed source or plan"
                        .into(),
                );
            }
        }
        Command::Table => {
            let table: crate::style_resolution::Snapshot = serde_json::from_value(data.clone())
                .map_err(|e| format!("style_resolution_invalid: {e}"))?;
            if table.project_id != request.project {
                return Err(
                    "project_context_changed: style table belongs to another project".into(),
                );
            }
            crate::style_resolution::validate_snapshot(&table).map_err(|e| e.to_string())?;
            if table.bindings.len() > crate::style_resolution::MAX_BINDINGS
                || table.missing_style_refs.len() > crate::style_resolution::MAX_BINDINGS
            {
                return Err("style_resolution_invalid: table exceeds its declared bound".into());
            }
            for binding in &table.bindings {
                crate::style_resolution::resolve(&table, &binding.key)
                    .map_err(|e| e.to_string())?;
            }
        }
        Command::Manifest => {
            if data["schema"] != "ds.style-seed-manifest/v1"
                || !data["revision_id"].as_str().is_some_and(digest)
                || !data["documents"].is_array()
            {
                return Err("style_governance_invalid: manifest authority is incomplete".into());
            }
        }
        Command::PlanSeed | Command::Seed { .. } => {
            if !data["manifest_revision"].as_str().is_some_and(digest)
                || !data["plan_sha256"].as_str().is_some_and(digest)
                || !data["creates"].is_array()
                || !data["preserves"].is_array()
                || !data["apply_allowed"].is_boolean()
            {
                return Err("style_governance_invalid: seed plan authority is incomplete".into());
            }
            if let Some(migrations) = data.get("migrations") {
                let rows = migrations
                    .as_array()
                    .ok_or("style_governance_invalid: seed migrations must be an array")?;
                if rows.len() > crate::style_resolution::MAX_BINDINGS {
                    return Err(
                        "style_governance_invalid: seed migrations exceed their bound".into(),
                    );
                }
                for row in rows {
                    if !row["style_ref"].as_str().is_some_and(|id| exact(id, 500))
                        || !row["expected_head"].as_str().is_some_and(digest)
                        || !row["expected_content_sha256"].as_str().is_some_and(digest)
                        || !row["content_sha256"].as_str().is_some_and(digest)
                        || !row["document"].is_object()
                        || row["content_sha256"]
                            != crate::style_plan::style_digest(&row["document"])
                    {
                        return Err("style_governance_invalid: seed migration lacks exact original and destination authority".into());
                    }
                }
            }
            if let Command::Seed {
                expected_manifest_revision,
                expected_plan_sha256,
            } = &request.command
            {
                if data["manifest_revision"] != expected_manifest_revision.as_str()
                    || data["plan_sha256"] != expected_plan_sha256.as_str()
                    || data["applied"] != true
                {
                    return Err("style_governance_invalid: applied seed receipt mismatches the reviewed plan".into());
                }
            }
        }
        Command::Inventory { limit, .. } => {
            if data["schema"] != "ds.style-inventory/v1"
                || data["entries"]
                    .as_array()
                    .is_none_or(|rows| rows.len() > usize::from(*limit))
                || !data["inventory_sha256"].as_str().is_some_and(digest)
                || !data["complete"].is_boolean()
            {
                return Err(
                    "style_governance_invalid: inventory receipt is incomplete or unbounded".into(),
                );
            }
        }
        Command::Backup {
            expected_inventory_sha256,
            ..
        } => {
            if data["inventory_sha256"] != expected_inventory_sha256.as_str()
                || !data["backup_id"].as_str().is_some_and(|id| exact(id, 200))
                || !data["entries"].is_array()
            {
                return Err(
                    "style_governance_invalid: immutable backup mismatches the reviewed inventory"
                        .into(),
                );
            }
        }
        Command::ReadBackup { backup_id, .. } => {
            if data["backup_id"] != backup_id.as_str()
                || !data["entries"].is_array()
                || !data["inventory_sha256"].as_str().is_some_and(digest)
            {
                return Err("style_governance_invalid: exact backup receipt mismatches".into());
            }
        }
        Command::PlanRetirement {
            expected_inventory_sha256,
            backup_id,
            ..
        } => {
            if data["dry_run"] != true
                || data["deletion_executable"] != false
                || !data["candidates"].is_array()
                || !data["blocked"].is_array()
                || data["inventory_sha256"] != expected_inventory_sha256.as_str()
                || data["backup_id"] != backup_id.as_str()
            {
                return Err("style_governance_invalid: retirement needs an exact backup and a deletion-disabled dry run".into());
            }
        }
    }
    Ok(data.clone())
}
// This remains a fixed reference; neither host supplies an arbitrary path/body.
const A4_DOCUMENT_PATH: &str = "eds_apps/ds_griddesign/printing_a4_documents/voltage-drop-a4-v1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct A4SeedPlan {
    schema: String,
    project_id: String,
    principal: String,
    document_path: String,
    definition_sha256: String,
    content_sha256: String,
    html_sha256: String,
    definition: Value,
    existing_state: String,
    #[serde(default)]
    existing_sha256: String,
    #[serde(default)]
    existing_update_time: String,
    creates: u8,
    preserves: u8,
    apply_allowed: bool,
    plan_sha256: String,
}
fn admit_a4_plan(request: &Request, data: &Value) -> Result<A4SeedPlan, String> {
    let plan: A4SeedPlan = serde_json::from_value(data.clone())
        .map_err(|error| format!("style_governance_invalid: fixed A4 plan {error}"))?;
    if plan.project_id != request.project {
        return Err("project_context_changed: A4 plan belongs to another captured project".into());
    }
    let definition = &plan.definition;
    let definition_valid = definition
        .as_object()
        .is_some_and(|object| object.len() == 5)
        && definition["schema"] == "ds.print-a4-default/v1"
        && definition["html_sha256"] == plan.html_sha256
        && definition["content_sha256"] == plan.content_sha256
        && definition["status_layout"].is_object()
        && definition["logo_slots"]
            == json!([
                "governed-logo-0",
                "governed-logo-1",
                "governed-logo-2",
                "governed-logo-3",
                "governed-logo-4",
                "governed-logo-5"
            ]);
    let state_valid = match plan.existing_state.as_str() {
        "missing" => {
            plan.existing_sha256.is_empty()
                && plan.existing_update_time.is_empty()
                && plan.creates == 1
                && plan.preserves == 0
                && plan.apply_allowed
        }
        "exact" => {
            plan.existing_sha256 == plan.definition_sha256
                && exact(&plan.existing_update_time, 64)
                && plan.creates == 0
                && plan.preserves == 1
                && plan.apply_allowed
        }
        "conflict" => {
            digest(&plan.existing_sha256)
                && plan.existing_sha256 != plan.definition_sha256
                && exact(&plan.existing_update_time, 64)
                && plan.creates == 0
                && plan.preserves == 0
                && !plan.apply_allowed
        }
        _ => false,
    };
    let mut unhashed = data.clone();
    unhashed["plan_sha256"] = json!("");
    if plan.schema != "ds.print-a4-seed-plan/v1"
        || plan.document_path != A4_DOCUMENT_PATH
        || !exact(&plan.principal, 320)
        || !definition_valid
        || !state_valid
        || !digest(&plan.html_sha256)
        || !digest(&plan.content_sha256)
        || !digest(&plan.definition_sha256)
        || plan.definition_sha256 != crate::style_plan::style_digest(definition)
        || plan.plan_sha256 != crate::style_plan::style_digest(&unhashed)
    {
        return Err(
            "style_governance_invalid: fixed A4 scope, exact definition or plan digest differs"
                .into(),
        );
    }
    Ok(plan)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effects_are_closed_and_fenced_without_a_deletion_command() {
        let mut request = Request {
            project: "fixture".into(),
            command: Command::Seed {
                expected_manifest_revision: "a".repeat(64),
                expected_plan_sha256: "b".repeat(64),
            },
        };
        let body = request_body(&request).unwrap();
        assert_eq!(body["action"], "seed_manifest");
        assert_eq!(body["data"]["project_id"], "fixture");
        request.command = Command::PlanRetirement {
            expected_inventory_sha256: "bad".into(),
            backup_id: "snapshot".into(),
            all_projects: false,
        };
        assert!(request_body(&request)
            .unwrap_err()
            .contains("inventory digest"));
        assert!(serde_json::from_value::<Command>(json!({"operation":"delete"})).is_err());
        request.command = Command::Inventory {
            cursor: None,
            limit: 201,
            all_projects: false,
        };
        assert!(request_body(&request).is_err());
    }
    #[test]
    fn declared_scaling_migrations_require_exact_heads_and_destination_bytes() {
        let request = Request {
            project: "fixture".into(),
            command: Command::PlanSeed,
        };
        let document = json!({"type":"line","paint":{"line-width":2},"metadata":{"print_scaling":{"reference_paper":"A3","units":"css_px","factors":{"A0":1,"A1":1,"A2":1,"A3":1,"A4":1,"A5":1,"Custom":1}}}});
        let mut receipt = json!({"success":true,"data":{"manifest_revision":"a".repeat(64),"plan_sha256":"b".repeat(64),"creates":[],"preserves":[],"apply_allowed":true,"migrations":[{"style_ref":"fixture/line_print","expected_head":"c".repeat(64),"expected_content_sha256":"d".repeat(64),"content_sha256":crate::style_plan::style_digest(&document),"document":document}]}});
        assert!(admit(&request, &receipt).is_ok());
        receipt["data"]["migrations"][0]["document"]["paint"]["line-width"] = json!(3);
        assert!(admit(&request, &receipt)
            .unwrap_err()
            .contains("destination authority"));
    }
    #[test]
    fn retirement_admission_never_accepts_an_executable_deletion() {
        let request = Request {
            project: "fixture".into(),
            command: Command::PlanRetirement {
                expected_inventory_sha256: "a".repeat(64),
                backup_id: "snapshot".into(),
                all_projects: true,
            },
        };
        let mut receipt = json!({"success":true,"data":{"dry_run":true,"deletion_executable":false,"inventory_sha256":"a".repeat(64),"backup_id":"snapshot","scope":{"project_id":"fixture","all_projects":true},"candidates":[],"blocked":[]}});
        assert!(admit(&request, &receipt).is_ok());
        let mut other_project = receipt.clone();
        other_project["data"]["scope"]["project_id"] = json!("other");
        assert!(admit(&request, &other_project)
            .unwrap_err()
            .contains("project_context_changed"));
        receipt["data"]["deletion_executable"] = json!(true);
        assert!(admit(&request, &receipt).is_err());
    }

    fn a4_fixture_plan(project: &str) -> Value {
        let definition = json!({"schema":"ds.print-a4-default/v1","html_sha256":"a".repeat(64),"content_sha256":"b".repeat(64),"status_layout":{"paper":{"size":"A4","orientation":"portrait"}},"logo_slots":["governed-logo-0","governed-logo-1","governed-logo-2","governed-logo-3","governed-logo-4","governed-logo-5"]});
        let mut plan = json!({"schema":"ds.print-a4-seed-plan/v1","project_id":project,"principal":"user@example.com","document_path":A4_DOCUMENT_PATH,"definition_sha256":crate::style_plan::style_digest(&definition),"content_sha256":"b".repeat(64),"html_sha256":"a".repeat(64),"definition":definition,"existing_state":"missing","creates":1,"preserves":0,"apply_allowed":true,"plan_sha256":""});
        plan["plan_sha256"] = json!(crate::style_plan::style_digest(&plan));
        plan
    }
    #[test]
    fn a4_seed_requires_exact_definition_scope_and_consistent_create_receipt() {
        let plan = a4_fixture_plan("project_a");
        let request = Request {
            project: "project_a".into(),
            command: Command::PlanA4,
        };
        let response = json!({"success":true,"data":plan});
        assert!(admit(&request, &response).is_ok());
        for mutation in ["project", "path", "body", "count", "state", "extra", "plan"] {
            let mut bad = response.clone();
            match mutation {
                "project" => bad["data"]["project_id"] = json!("project_b"),
                "path" => {
                    bad["data"]["document_path"] =
                        json!("eds_apps/ds_griddesign/printing_defaults/v1")
                }
                "body" => bad["data"]["definition"]["content_sha256"] = json!("c".repeat(64)),
                "count" => bad["data"]["creates"] = json!(2),
                "state" => bad["data"]["existing_state"] = json!("overwrite"),
                "extra" => bad["data"]["default_pointer"] = json!("other"),
                _ => bad["data"]["plan_sha256"] = json!("0".repeat(64)),
            }
            assert!(admit(&request, &bad).is_err(), "{mutation}");
        }
        let request = Request {
            project: "project_a".into(),
            command: Command::CreateA4 {
                expected_definition_sha256: plan["definition_sha256"].as_str().unwrap().into(),
                expected_plan_sha256: plan["plan_sha256"].as_str().unwrap().into(),
            },
        };
        let body = request_body(&request).unwrap();
        assert_eq!(body["action"], "create_printing_a4");
        assert_eq!(body["data"].as_object().unwrap().len(), 3);
        let mut response = json!({"success":true,"data":{"schema":"ds.print-a4-seed-result/v1","plan":plan,"applied":true,"created":true,"preserved":false}});
        assert!(admit(&request, &response).is_ok());
        response["data"]["created"] = json!(false);
        assert!(admit(&request, &response).is_err());
        let invalid = Request {
            project: "project_a".into(),
            command: Command::CreateA4 {
                expected_definition_sha256: "bad".into(),
                expected_plan_sha256: "b".repeat(64),
            },
        };
        assert!(request_body(&invalid).is_err());
    }
    #[test]
    #[ignore = "requires exact fresh Go isolated-emulator A4 seed-plan bytes; mandatory separate owning gate"]
    fn genuine_go_a4_create_only_plan_has_the_same_native_digest_and_scope() {
        let path = std::env::var("DS_A4_SEED_PLAN_INPUT")
            .expect("fresh owning Go producer capture is required");
        let plan: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let request = Request {
            project: "project_a".into(),
            command: Command::PlanA4,
        };
        let admitted = admit(&request, &json!({"success":true,"data":plan})).unwrap();
        assert_eq!(admitted["creates"], 1);
        assert_eq!(admitted["preserves"], 0);
        assert_eq!(
            admitted["html_sha256"],
            "6df34e11fb5e4b16654dcd7c7490e8d93487c08ca6a27227c80fda383303c78d"
        );
    }
}
