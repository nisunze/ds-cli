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
    PlanBinding {
        key: crate::style_resolution::Key,
        style_ref: String,
    },
    CreateBinding {
        key: crate::style_resolution::Key,
        style_ref: String,
        expected_manifest_revision: String,
        expected_plan_sha256: String,
    },
    PlanA4,
    CreateA4 {
        expected_definition_sha256: String,
        expected_plan_sha256: String,
    },
    PlanA4Migration,
    ApplyA4Migration {
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
            Self::PlanBinding { .. } => "plan_style_binding",
            Self::CreateBinding { .. } => "create_style_binding",
            Self::PlanA4 => "plan_printing_a4",
            Self::CreateA4 { .. } => "create_printing_a4",
            Self::PlanA4Migration => "plan_a4_migration",
            Self::ApplyA4Migration { .. } => "apply_a4_migration",
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
        Command::PlanBinding { key, style_ref } | Command::CreateBinding { key, style_ref, .. } => {
            validate_binding(key, style_ref)?;
            data["key"] = json!(key);
            data["style_ref"] = json!(style_ref);
            if let Command::CreateBinding {
                expected_manifest_revision,
                expected_plan_sha256,
                ..
            } = &request.command
            {
                if !digest(expected_manifest_revision) || !digest(expected_plan_sha256) {
                    return Err("style_governance_invalid: binding create needs its reviewed manifest and plan digests".into());
                }
                data["expected_manifest_revision"] = json!(expected_manifest_revision);
                data["expected_plan_sha256"] = json!(expected_plan_sha256);
            }
        }
        Command::CreateA4 {
            expected_definition_sha256,
            expected_plan_sha256,
        }
        | Command::ApplyA4Migration {
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
        Command::PlanBinding { key, style_ref } => {
            admit_binding_plan(request, key, style_ref, data)?;
        }
        Command::CreateBinding {
            key,
            style_ref,
            expected_manifest_revision,
            expected_plan_sha256,
        } => {
            if data.as_object().is_none_or(|object| object.len() != 6)
                || data["schema"] != "ds.style-binding-result/v1"
                || data["applied"] != true
            {
                return Err("style_governance_invalid: incomplete binding create receipt".into());
            }
            let plan = admit_binding_plan(request, key, style_ref, &data["plan"])?;
            if plan.manifest_revision != *expected_manifest_revision
                || plan.plan_sha256 != *expected_plan_sha256
                || data["created"] != (plan.creates == 1)
                || data["preserved"] != (plan.preserves == 1)
                || data["manifest_revision"] != plan.next_manifest_revision
            {
                return Err("style_governance_invalid: binding create differs from its reviewed manifest or plan".into());
            }
        }
        Command::PlanA4 => {
            admit_a4_plan(request, data)?;
        }
        Command::PlanA4Migration => {
            admit_a4_migration_plan(request, data)?;
        }
        Command::ApplyA4Migration {
            expected_definition_sha256,
            expected_plan_sha256,
        } => {
            if data.as_object().is_none_or(|object| object.len() != 6)
                || data["schema"] != "ds.print-a4-migration-result/v1"
                || data["applied"] != true
            {
                return Err("style_governance_invalid: incomplete A4 migration receipt".into());
            }
            let plan = admit_a4_migration_plan(request, &data["plan"])?;
            if !plan.apply_allowed
                || plan.definition_sha256 != *expected_definition_sha256
                || plan.plan_sha256 != *expected_plan_sha256
                || data["updated"] != (plan.updates == 1)
                || data["retained"] != (plan.retains == 1)
                || data["preserved"] != (plan.preserves == 1)
            {
                return Err("style_governance_invalid: A4 migration differs from its reviewed source, destination or retained version".into());
            }
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

fn validate_binding(key: &crate::style_resolution::Key, style_ref: &str) -> Result<(), String> {
    key.validate()
        .map_err(|error| format!("style_governance_invalid: {error}"))?;
    if key.target != crate::style_resolution::Target::Print
        || !exact(style_ref, 500)
        || !style_ref.ends_with("_print")
    {
        return Err("style_governance_invalid: binding requires an exact print key and existing _print style ref".into());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingPlan {
    schema: String,
    project_id: String,
    principal: String,
    key: crate::style_resolution::Key,
    style_ref: String,
    manifest_revision: String,
    style_head_sha256: String,
    style_content_sha256: String,
    style_update_time: String,
    next_manifest_revision: String,
    adds_document: bool,
    creates: u8,
    preserves: u8,
    apply_allowed: bool,
    plan_sha256: String,
}

fn admit_binding_plan(
    request: &Request,
    key: &crate::style_resolution::Key,
    style_ref: &str,
    data: &Value,
) -> Result<BindingPlan, String> {
    validate_binding(key, style_ref)?;
    let plan: BindingPlan = serde_json::from_value(data.clone())
        .map_err(|error| format!("style_governance_invalid: binding plan {error}"))?;
    if plan.project_id != request.project {
        return Err(
            "project_context_changed: binding plan belongs to another captured project".into(),
        );
    }
    let state_valid = match (plan.creates, plan.preserves) {
        (1, 0) => plan.next_manifest_revision != plan.manifest_revision,
        (0, 1) => !plan.adds_document && plan.next_manifest_revision == plan.manifest_revision,
        _ => false,
    };
    let mut unhashed = data.clone();
    unhashed["plan_sha256"] = json!("");
    if plan.schema != "ds.style-binding-plan/v1"
        || plan.key != *key
        || plan.style_ref != style_ref
        || !exact(&plan.principal, 320)
        || !digest(&plan.manifest_revision)
        || !digest(&plan.style_head_sha256)
        || !digest(&plan.style_content_sha256)
        || !digest(&plan.next_manifest_revision)
        || !digest(&plan.plan_sha256)
        || !state_valid
        || !plan.apply_allowed
        || time::OffsetDateTime::parse(
            &plan.style_update_time,
            &time::format_description::well_known::Rfc3339,
        )
        .is_err()
        || plan.plan_sha256 != crate::style_plan::style_digest(&unhashed)
    {
        return Err("style_governance_invalid: binding scope, exact key/style, state or plan digest differs".into());
    }
    Ok(plan)
}
// This remains a fixed reference; neither host supplies an arbitrary path/body.
fn a4_document_path(project: &str) -> String {
    format!("eds_project/{project}/printing_a4_documents/voltage-drop-a4-v1")
}
const A4_BASELINE_DEFINITION: &str =
    "93a01522c92cf7a597d14a9f4dcb678f36aa81216b3ac41170fd9c9c9e17409e";
const A4_BASELINE_CONTENT: &str =
    "bdbe3df0b4e4154127b59070626d9298ca6faeaba0b5cabcb13268d5ef33344d";
const A4_BASELINE_HTML: &str = "f8475303b1fb7b2cf9c613cb51f5f8d6fc7b843805fda247c7d281911a94fc3d";
// Reviewed fitting revision immediately before the farthest-customer metric.
const A4_FITTING_DEFINITION: &str =
    "967ebb76ab66af38f3ad20ac04844da2383feea3df6fe725ea8870393de0d1f0";
const A4_FITTING_CONTENT: &str = "736e564abc947dbde216dd0bd73569d900522f005ad51553fe9f69ceaaebc863";
const A4_FITTING_HTML: &str = "124a3e83edb483ee9c776d30a4994e25c2f707f79c2536cd0d0a5a99aef3015d";

fn a4_retained_hashes(definition: &str) -> Option<(&'static str, &'static str)> {
    match definition {
        A4_BASELINE_DEFINITION => Some((A4_BASELINE_CONTENT, A4_BASELINE_HTML)),
        A4_FITTING_DEFINITION => Some((A4_FITTING_CONTENT, A4_FITTING_HTML)),
        _ => None,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct A4MigrationPlan {
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
    retention_path: String,
    previous_content_sha256: String,
    previous_html_sha256: String,
    updates: u8,
    retains: u8,
}
fn admit_a4_migration_plan(request: &Request, data: &Value) -> Result<A4MigrationPlan, String> {
    let plan: A4MigrationPlan = serde_json::from_value(data.clone())
        .map_err(|error| format!("style_governance_invalid: A4 migration plan {error}"))?;
    if plan.project_id != request.project {
        return Err("project_context_changed: A4 migration belongs to another project".into());
    }
    let expected_retention = format!(
        "{}/revisions/{}",
        a4_document_path(&request.project),
        plan.existing_sha256
    );
    let without_retention = plan.retention_path.is_empty()
        && plan.previous_content_sha256.is_empty()
        && plan.previous_html_sha256.is_empty()
        && plan.updates == 0
        && plan.retains == 0;
    let state_valid = match plan.existing_state.as_str() {
        "baseline" => a4_retained_hashes(&plan.existing_sha256).is_some_and(|(content, html)| {
            plan.definition_sha256 != plan.existing_sha256
                && exact(&plan.existing_update_time, 64)
                && plan.retention_path == expected_retention
                && plan.previous_content_sha256 == content
                && plan.previous_html_sha256 == html
                && plan.updates == 1
                && plan.retains == 1
                && plan.preserves == 0
                && plan.apply_allowed
        }),
        "exact" => {
            without_retention
                && plan.existing_sha256 == plan.definition_sha256
                && exact(&plan.existing_update_time, 64)
                && plan.preserves == 1
                && plan.apply_allowed
        }
        "missing" => {
            without_retention
                && plan.existing_sha256.is_empty()
                && plan.existing_update_time.is_empty()
                && plan.preserves == 0
                && !plan.apply_allowed
        }
        "conflict" => {
            without_retention
                && digest(&plan.existing_sha256)
                && plan.existing_sha256 != plan.definition_sha256
                && a4_retained_hashes(&plan.existing_sha256).is_none()
                && exact(&plan.existing_update_time, 64)
                && plan.preserves == 0
                && !plan.apply_allowed
        }
        _ => false,
    };
    let definition = &plan.definition;
    let definition_valid = definition
        .as_object()
        .is_some_and(|object| object.len() == 5)
        && definition["schema"] == "ds.print-a4-default/v1"
        && definition["html_sha256"] == plan.html_sha256
        && definition["content_sha256"] == plan.content_sha256
        && definition["status_layout"]["paper"]["size"] == "A4"
        && definition["status_layout"]["paper"]["orientation"] == "portrait"
        && definition["logo_slots"]
            == json!([
                "governed-logo-0",
                "governed-logo-1",
                "governed-logo-2",
                "governed-logo-3",
                "governed-logo-4",
                "governed-logo-5"
            ]);
    let mut unhashed = data.clone();
    unhashed["plan_sha256"] = json!("");
    if plan.schema != "ds.print-a4-migration-plan/v1"
        || plan.document_path != a4_document_path(&request.project)
        || !exact(&plan.principal, 320)
        || plan.creates != 0
        || !state_valid
        || !definition_valid
        || !digest(&plan.html_sha256)
        || !digest(&plan.content_sha256)
        || plan.definition_sha256 != crate::style_plan::style_digest(definition)
        || plan.plan_sha256 != crate::style_plan::style_digest(&unhashed)
    {
        return Err("style_governance_invalid: A4 migration source, retained version, scope or plan digest differs".into());
    }
    Ok(plan)
}
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
        || plan.document_path != a4_document_path(&request.project)
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
    fn binding_key() -> crate::style_resolution::Key {
        crate::style_resolution::Key {
            entity_class: "survey_existing_poles".into(),
            source_kind: "live_geojson".into(),
            target: crate::style_resolution::Target::Print,
            role: "project".into(),
            ink: crate::style_resolution::Ink::Colour,
        }
    }
    fn binding_fixture_plan() -> Value {
        let mut plan = json!({
            "schema":"ds.style-binding-plan/v1", "project_id":"project_a",
            "principal":"user@example.com", "key":binding_key(),
            "style_ref":"lv_poles_as_built_print", "manifest_revision":"a".repeat(64),
            "style_head_sha256":"b".repeat(64), "style_content_sha256":"c".repeat(64),
            "style_update_time":"2026-10-05T00:00:00.123456789Z",
            "next_manifest_revision":"d".repeat(64), "adds_document":false,
            "creates":1, "preserves":0, "apply_allowed":true, "plan_sha256":""
        });
        plan["plan_sha256"] = json!(crate::style_plan::style_digest(&plan));
        plan
    }
    #[test]
    fn binding_authority_is_print_only_and_refuses_unrequested_plan_effects() {
        let request = Request {
            project: "project_a".into(),
            command: Command::PlanBinding {
                key: binding_key(),
                style_ref: "lv_poles_as_built_print".into(),
            },
        };
        let body = request_body(&request).unwrap();
        assert_eq!(body["action"], "plan_style_binding");
        assert_eq!(body["data"].as_object().unwrap().len(), 3);
        assert!(body["data"]["key"].get("ink").is_none());
        let plan = binding_fixture_plan();
        assert!(admit(&request, &json!({"success":true,"data":plan})).is_ok());
        for mutation in [
            "project",
            "key",
            "style",
            "default",
            "count",
            "timestamp",
            "hash",
        ] {
            let mut bad = plan.clone();
            match mutation {
                "project" => bad["project_id"] = json!("project_b"),
                "key" => bad["key"]["source_kind"] = json!("saved_design"),
                "style" => bad["style_ref"] = json!("other_print"),
                "default" => bad["default_pointer"] = json!("other"),
                "count" => bad["creates"] = json!(2),
                "timestamp" => bad["style_update_time"] = json!("not-a-timestamp"),
                _ => {}
            }
            bad["plan_sha256"] = json!("");
            bad["plan_sha256"] = json!(crate::style_plan::style_digest(&bad));
            if mutation == "hash" {
                bad["style_head_sha256"] = json!("e".repeat(64));
            }
            assert!(
                admit(&request, &json!({"success":true,"data":bad})).is_err(),
                "{mutation}"
            );
        }
        let mut key = binding_key();
        key.target = crate::style_resolution::Target::Screen;
        assert!(request_body(&Request {
            project: "project_a".into(),
            command: Command::PlanBinding {
                key,
                style_ref: "lv_poles_as_built_print".into()
            },
        })
        .is_err());
        assert!(serde_json::from_value::<Command>(json!({
            "operation":"plan_binding", "key":binding_key(),
            "style_ref":"lv_poles_as_built_print", "document":{}
        }))
        .is_err());
    }
    #[test]
    fn binding_create_preserves_existing_tuple_and_checks_both_reviewed_fences() {
        let mut plan = binding_fixture_plan();
        for preserve in [false, true] {
            if preserve {
                plan["creates"] = json!(0);
                plan["preserves"] = json!(1);
                plan["next_manifest_revision"] = plan["manifest_revision"].clone();
                plan["plan_sha256"] = json!("");
                plan["plan_sha256"] = json!(crate::style_plan::style_digest(&plan));
            }
            let mut request = Request {
                project: "project_a".into(),
                command: Command::CreateBinding {
                    key: binding_key(),
                    style_ref: "lv_poles_as_built_print".into(),
                    expected_manifest_revision: plan["manifest_revision"].as_str().unwrap().into(),
                    expected_plan_sha256: plan["plan_sha256"].as_str().unwrap().into(),
                },
            };
            let body = request_body(&request).unwrap();
            assert_eq!(body["action"], "create_style_binding");
            assert_eq!(body["data"].as_object().unwrap().len(), 5);
            let receipt = json!({"success":true,"data":{
                "schema":"ds.style-binding-result/v1", "applied":true,
                "created":!preserve,"preserved":preserve,"plan":plan,
                "manifest_revision":plan["next_manifest_revision"]
            }});
            assert!(admit(&request, &receipt).is_ok());
            if let Command::CreateBinding {
                expected_manifest_revision,
                ..
            } = &mut request.command
            {
                *expected_manifest_revision = "f".repeat(64);
            }
            assert!(admit(&request, &receipt).is_err());
            if let Command::CreateBinding {
                expected_manifest_revision,
                expected_plan_sha256,
                ..
            } = &mut request.command
            {
                *expected_manifest_revision = plan["manifest_revision"].as_str().unwrap().into();
                *expected_plan_sha256 = "f".repeat(64);
            }
            assert!(admit(&request, &receipt).is_err());
        }
    }
    #[test]
    #[ignore = "requires exact fresh Go binding-plan bytes; mandatory separate owning gate"]
    fn genuine_go_binding_plan_has_the_same_native_digest_and_scope() {
        let path = std::env::var("DS_STYLE_BINDING_PLAN_INPUT")
            .expect("fresh owning Go producer capture is required");
        let response: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let plan = &response["data"];
        let request = Request {
            project: plan["project_id"].as_str().unwrap().into(),
            command: Command::PlanBinding {
                key: serde_json::from_value(plan["key"].clone()).unwrap(),
                style_ref: plan["style_ref"].as_str().unwrap().into(),
            },
        };
        let admitted = admit(&request, &response).unwrap();
        assert_eq!(admitted["creates"], 1);
        assert_eq!(admitted["preserves"], 0);
    }
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
        let mut plan = json!({"schema":"ds.print-a4-seed-plan/v1","project_id":project,"principal":"user@example.com","document_path":a4_document_path(project),"definition_sha256":crate::style_plan::style_digest(&definition),"content_sha256":"b".repeat(64),"html_sha256":"a".repeat(64),"definition":definition,"existing_state":"missing","creates":1,"preserves":0,"apply_allowed":true,"plan_sha256":""});
        plan["plan_sha256"] = json!(crate::style_plan::style_digest(&plan));
        plan
    }
    #[test]
    fn a4_migration_retains_only_the_known_baseline_and_refuses_rehashed_wrong_authority() {
        let mut plan = a4_fixture_plan("project_a");
        plan["schema"] = json!("ds.print-a4-migration-plan/v1");
        plan["existing_state"] = json!("baseline");
        plan["existing_sha256"] = json!(A4_BASELINE_DEFINITION);
        plan["existing_update_time"] = json!("2026-10-05T00:00:00Z");
        plan["creates"] = json!(0);
        plan["retention_path"] = json!(format!(
            "{}/revisions/{A4_BASELINE_DEFINITION}",
            a4_document_path("project_a")
        ));
        plan["previous_content_sha256"] = json!(A4_BASELINE_CONTENT);
        plan["previous_html_sha256"] = json!(A4_BASELINE_HTML);
        plan["updates"] = json!(1);
        plan["retains"] = json!(1);
        plan["plan_sha256"] = json!("");
        plan["plan_sha256"] = json!(crate::style_plan::style_digest(&plan));
        let request = Request {
            project: "project_a".into(),
            command: Command::PlanA4Migration,
        };
        assert!(admit(&request, &json!({"success":true,"data":plan})).is_ok());
        for field in [
            "retention_path",
            "existing_sha256",
            "previous_content_sha256",
            "previous_html_sha256",
            "project_id",
        ] {
            let mut bad = plan.clone();
            bad[field] = json!("a".repeat(64));
            bad["plan_sha256"] = json!("");
            bad["plan_sha256"] = json!(crate::style_plan::style_digest(&bad));
            assert!(
                admit(&request, &json!({"success":true,"data":bad})).is_err(),
                "{field}"
            );
        }
        let request = Request {
            project: "project_a".into(),
            command: Command::ApplyA4Migration {
                expected_definition_sha256: plan["definition_sha256"].as_str().unwrap().into(),
                expected_plan_sha256: plan["plan_sha256"].as_str().unwrap().into(),
            },
        };
        let body = request_body(&request).unwrap();
        assert_eq!(body["action"], "apply_a4_migration");
        assert_eq!(body["data"].as_object().unwrap().len(), 3);
        let mut response = json!({"success":true,"data":{"schema":"ds.print-a4-migration-result/v1","applied":true,"updated":true,"preserved":false,"retained":true,"plan":plan}});
        assert!(admit(&request, &response).is_ok());
        response["data"]["retained"] = json!(false);
        assert!(admit(&request, &response).is_err());
    }
    #[test]
    fn genuine_go_a4_migration_plan_has_the_same_native_digest_and_retention() {
        let response: Value =
            serde_json::from_str(include_str!("../tests/fixtures/a4-migration-go-plan.json"))
                .unwrap();
        let request = Request {
            project: "project_a".into(),
            command: Command::PlanA4Migration,
        };
        let plan = admit(&request, &response).unwrap();
        assert_eq!(plan["updates"], 1);
        assert_eq!(plan["retains"], 1);
        assert_eq!(plan["creates"], 0);
        assert_eq!(plan["existing_sha256"], A4_BASELINE_DEFINITION);
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
        for mutation in [
            "project",
            "path",
            "other_project_path",
            "global_path",
            "body",
            "count",
            "state",
            "extra",
            "plan",
        ] {
            let mut bad = response.clone();
            match mutation {
                "project" => bad["data"]["project_id"] = json!("project_b"),
                "path" => {
                    bad["data"]["document_path"] =
                        json!("eds_apps/ds_griddesign/printing_defaults/v1")
                }
                "other_project_path" => {
                    bad["data"]["document_path"] = json!(a4_document_path("project_b"))
                }
                "global_path" => {
                    bad["data"]["document_path"] =
                        json!("eds_apps/ds_griddesign/printing_a4_documents/voltage-drop-a4-v1")
                }
                "body" => bad["data"]["definition"]["content_sha256"] = json!("c".repeat(64)),
                "count" => bad["data"]["creates"] = json!(2),
                "state" => bad["data"]["existing_state"] = json!("overwrite"),
                "extra" => bad["data"]["default_pointer"] = json!("other"),
                _ => bad["data"]["plan_sha256"] = json!("0".repeat(64)),
            }
            if matches!(mutation, "other_project_path" | "global_path") {
                bad["data"]["plan_sha256"] = json!("");
                bad["data"]["plan_sha256"] = json!(crate::style_plan::style_digest(&bad["data"]));
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
            "f8475303b1fb7b2cf9c613cb51f5f8d6fc7b843805fda247c7d281911a94fc3d"
        );
    }
}
