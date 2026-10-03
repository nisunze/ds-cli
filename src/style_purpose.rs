//! Authored purposes over the existing immutable style tuple authority.
//! A style table cannot admit printing layouts; required template references
//! remain explicit blockers until the print owner captures separate Templates.
use crate::printing::{standards::PageRole, Paper};
use crate::style_resolution::{self, Binding, Ink, Key, Refusal, Snapshot, Target};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const DECLARATION_SCHEMA: &str = "ds.style-purposes/v1";
pub const INDEX_SCHEMA: &str = "ds.style-purpose-index/v1";
pub const MAX_GROUPS: usize = 64;
pub const MAX_ROLES: usize = 64;
pub const MAX_LAYOUTS: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub schema: String,
    pub groups: Vec<Purpose>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    StyleEditing,
    PrintComposition,
}

/// Names, order, requirements and exact tuple choices are authored data.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Purpose {
    pub id: String,
    pub label: String,
    pub kind: Kind,
    pub target: Target,
    pub ink: Ink,
    pub required_roles: Vec<String>,
    pub roles: Vec<Role>,
    pub required_layout_roles: Vec<PageRole>,
    pub layouts: Vec<LayoutReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub id: String,
    pub label: String,
    pub key: Key,
    /// Exact authored line-dasharray value or expression, including solid.
    /// Presence of a property alone does not prove the required linetype.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_type: Option<Value>,
}

/// References, never layout bodies or invented style tuples. Later preview
/// must call Templates::document_by_id and fence both pins against this ref.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutReference {
    pub template_id: String,
    pub role: PageRole,
    pub paper: Paper,
    pub revision_id: String,
    pub content_sha256: String,
}

fn refusal(code: &str, message: impl Into<String>) -> Refusal {
    Refusal {
        code: code.into(),
        message: message.into(),
    }
}
fn invalid(message: impl Into<String>) -> Refusal {
    refusal("style_purpose_invalid", message)
}
fn bounded(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 200
        && text.trim() == text
        && !text.chars().any(char::is_control)
}
fn digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn source(binding: &Binding) -> Value {
    json!({"key":binding.key,"style_ref":binding.style_ref,
        "revision_id":binding.revision_id,"content_sha256":binding.content_sha256,
        "scope":binding.scope})
}

fn group(snapshot: &Snapshot, purpose: &Purpose, owner: &Binding) -> Result<Value, Refusal> {
    if !bounded(&purpose.id)
        || !bounded(&purpose.label)
        || purpose.required_roles.is_empty()
        || purpose.required_roles.len() > MAX_ROLES
        || purpose.roles.len() > MAX_ROLES
        || purpose.layouts.len() > MAX_LAYOUTS
        || purpose.required_layout_roles.len() > MAX_LAYOUTS
        || (purpose.kind == Kind::PrintComposition
            && (purpose.target != Target::Print || purpose.required_layout_roles.is_empty()))
        || (purpose.kind == Kind::StyleEditing
            && (!purpose.required_layout_roles.is_empty() || !purpose.layouts.is_empty()))
    {
        return Err(invalid(format!(
            "purpose {} has invalid identity, kind or bounds",
            purpose.id
        )));
    }
    let mut declared = BTreeSet::new();
    for role in &purpose.roles {
        if !bounded(&role.id) || !bounded(&role.label) || !declared.insert(&role.id) {
            return Err(invalid(format!(
                "purpose {} has an invalid or duplicate role",
                purpose.id
            )));
        }
        role.key.validate()?;
        if role.key.target != purpose.target || role.key.ink != purpose.ink {
            return Err(invalid(format!(
                "purpose {} role {} changes target or ink",
                purpose.id, role.id
            )));
        }
    }
    let mut required = BTreeSet::new();
    let mut roles = Vec::new();
    for id in &purpose.required_roles {
        if !bounded(id) || !required.insert(id) {
            return Err(invalid(format!(
                "purpose {} has an invalid required role",
                purpose.id
            )));
        }
        let role = purpose
            .roles
            .iter()
            .find(|role| &role.id == id)
            .ok_or_else(|| {
                refusal(
                    "style_purpose_role_missing",
                    format!("purpose {} requires role {id}", purpose.id),
                )
            })?;
        let binding = style_resolution::resolve(snapshot, &role.key)?;
        if binding.document["type"] == "line" {
            let constraint = role
                .line_type
                .as_ref()
                .filter(|value| value.is_array())
                .ok_or_else(|| {
                    refusal(
                        "style_purpose_line_type_missing",
                        format!(
                            "purpose {} role {id} lacks an authored line-dasharray constraint",
                            purpose.id
                        ),
                    )
                })?;
            let actual = binding.document["paint"]
                .get("line-dasharray")
                .filter(|value| value.is_array())
                .ok_or_else(|| {
                    refusal(
                        "style_purpose_line_type_missing",
                        format!(
                            "purpose {} role {id} held document lacks line-dasharray",
                            purpose.id
                        ),
                    )
                })?;
            if constraint != actual {
                return Err(refusal(
                    "style_purpose_constraint_mismatch",
                    format!(
                        "purpose {} role {id} line-dasharray differs from its authored constraint",
                        purpose.id
                    ),
                ));
            }
        } else if role.line_type.is_some() {
            return Err(invalid(format!(
                "purpose {} role {id} declares linetype for a non-line document",
                purpose.id
            )));
        }
        roles.push(
            json!({"id":role.id,"label":role.label,"line_type":role.line_type,"binding":binding}),
        );
    }
    if required != declared {
        return Err(invalid(format!(
            "purpose {} has roles outside its required roster",
            purpose.id
        )));
    }
    let mut required_layouts = BTreeSet::new();
    for role in &purpose.required_layout_roles {
        let identity = serde_json::to_string(role).map_err(|error| invalid(error.to_string()))?;
        if !required_layouts.insert(identity) {
            return Err(invalid(format!(
                "purpose {} repeats a required layout role",
                purpose.id
            )));
        }
    }
    let mut declared_layouts = BTreeSet::new();
    let mut blockers = Vec::new();
    for layout in &purpose.layouts {
        let role =
            serde_json::to_string(&layout.role).map_err(|error| invalid(error.to_string()))?;
        if !bounded(&layout.template_id)
            || !digest(&layout.revision_id)
            || !digest(&layout.content_sha256)
            || !declared_layouts.insert(role)
            || matches!(layout.paper, Paper::Custom { short_mm, long_mm }
                if !short_mm.is_finite() || !long_mm.is_finite() || short_mm <= 0. || long_mm < short_mm)
        {
            return Err(invalid(format!(
                "purpose {} has an invalid layout reference",
                purpose.id
            )));
        }
        blockers.push(json!({"code":"style_purpose_layout_capture_required",
            "layout":layout,"message":"Capture separate Templates and admit the exact role, paper, project, revision and content before preview or delivery."}));
    }
    if required_layouts != declared_layouts {
        return Err(refusal(
            "style_purpose_role_missing",
            format!(
                "purpose {} required layout roster differs from its references",
                purpose.id
            ),
        ));
    }
    Ok(
        json!({"id":purpose.id,"label":purpose.label,"kind":purpose.kind,
        "target":purpose.target,"ink":purpose.ink,"declaration_source":source(owner),
        "required_roles":purpose.required_roles,"roles":roles,
        "required_layout_roles":purpose.required_layout_roles,"layouts":purpose.layouts,
        "style_roles_ready":true,"composition_ready":false,"blockers":blockers}),
    )
}

/// Read only declared purposes from the authorized held table. Hosts must
/// capture that table under the explicit project, as the native CLI does.
/// This performs no transport, seed adoption, layout capture or mutation.
pub fn index(
    project: &str,
    snapshot: &Snapshot,
    target: Target,
    ink: Ink,
) -> Result<Value, Refusal> {
    style_resolution::validate_snapshot(snapshot)?;
    if project != snapshot.project_id {
        return Err(refusal(
            "project_context_changed",
            "purpose table belongs to another project",
        ));
    }
    if target == Target::Screen && ink != Ink::Colour {
        return Err(refusal(
            "style_resolution_invalid",
            "ink=monochrome requires target=print",
        ));
    }
    let size = serde_json::to_vec(snapshot)
        .map_err(|error| invalid(error.to_string()))?
        .len();
    if size > style_resolution::MAX_BYTES {
        return Err(invalid("purpose source table exceeds 32 MiB"));
    }
    let mut sources = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut groups = Vec::new();
    let mut authored_groups = 0_usize;
    for owner in &snapshot.bindings {
        let Some(value) = owner.document["metadata"].get("style_purposes") else {
            continue;
        };
        style_resolution::resolve(snapshot, &owner.key)?;
        // One immutable document may bind several exact tuples. Its authored
        // declaration is read once, without collapsing different revisions.
        if !sources.insert((
            &owner.style_ref,
            &owner.scope,
            &owner.revision_id,
            &owner.content_sha256,
        )) {
            continue;
        }
        let declaration: Declaration = serde_json::from_value(value.clone()).map_err(|error| {
            invalid(format!("{} purpose declaration: {error}", owner.style_ref))
        })?;
        if declaration.schema != DECLARATION_SCHEMA || declaration.groups.is_empty() {
            return Err(invalid(format!(
                "{} has invalid purpose schema or empty groups",
                owner.style_ref
            )));
        }
        authored_groups += declaration.groups.len();
        if authored_groups > MAX_GROUPS {
            return Err(invalid("purpose declarations exceed 64 groups"));
        }
        for purpose in declaration.groups {
            if purpose.target != target || purpose.ink != ink {
                continue;
            }
            if !identities.insert(purpose.id.clone()) {
                return Err(refusal(
                    "style_purpose_ambiguous",
                    format!("several held declarations name purpose {}", purpose.id),
                ));
            }
            groups.push(group(snapshot, &purpose, owner)?);
        }
    }
    if groups.is_empty() {
        return Err(refusal(
            "style_purpose_missing",
            "no held governed document declares a purpose for the exact target and ink",
        ));
    }
    let result = json!({"schema":INDEX_SCHEMA,"project_id":project,
        "resolution_revision":snapshot.revision_id,"target":target,"ink":ink,"groups":groups});
    if serde_json::to_vec(&result)
        .map_err(|error| invalid(error.to_string()))?
        .len()
        > style_resolution::MAX_BYTES
    {
        return Err(invalid("purpose index exceeds 32 MiB"));
    }
    Ok(result)
}

pub fn declaration_schema() -> Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(Declaration)).expect("closed purpose schema");
    schema["properties"]["schema"]["const"] = json!(DECLARATION_SCHEMA);
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(role: &str) -> Key {
        Key {
            entity_class: "lv_line".into(),
            source_kind: "saved_design".into(),
            target: Target::Print,
            role: role.into(),
            ink: Ink::Colour,
        }
    }
    fn fixture() -> Snapshot {
        let roles = ["focused", "context"]
            .into_iter()
            .map(|role| json!({"id":role,"label":role,"key":key(role),"line_type":[4,2]}))
            .collect::<Vec<_>>();
        let declaration = json!({"schema":DECLARATION_SCHEMA,"groups":[{
            "id":"transformer_a0","label":"Transformer on A0","kind":"print_composition",
            "target":"print","ink":"colour","required_roles":["focused","context"],"roles":roles,
            "required_layout_roles":["network"],"layouts":[{"template_id":"authored-transformer-a0",
                "role":"network","paper":"A0","revision_id":"c".repeat(64),"content_sha256":"d".repeat(64)}]}]});
        let mut bindings = Vec::new();
        for role in ["focused", "context"] {
            let mut document = json!({"type":"line","paint":{"line-width":0.6,
                "line-color":"#64748B","line-dasharray":[4,2]}});
            if role == "focused" {
                document["metadata"] = json!({"style_purposes":declaration});
            }
            bindings.push(Binding {
                key: key(role),
                style_ref: format!("fixture/{role}"),
                revision_id: "a".repeat(64),
                content_sha256: crate::style_plan::style_digest(&document),
                scope: "projects/fixture".into(),
                document,
                presentation_tier: None,
            });
        }
        Snapshot {
            schema: style_resolution::SCHEMA.into(),
            project_id: "fixture".into(),
            revision_id: "b".repeat(64),
            seed_version: None,
            style_presets: None,
            missing_style_refs: Vec::new(),
            bindings,
        }
    }
    fn refresh(snapshot: &mut Snapshot) {
        for binding in &mut snapshot.bindings {
            binding.content_sha256 = crate::style_plan::style_digest(&binding.document);
        }
    }
    fn read(snapshot: &Snapshot) -> Result<Value, Refusal> {
        index("fixture", snapshot, Target::Print, Ink::Colour)
    }
    fn purpose(snapshot: &mut Snapshot) -> &mut Value {
        &mut snapshot.bindings[0].document["metadata"]["style_purposes"]["groups"][0]
    }

    #[test]
    fn authored_order_exact_tuples_and_separate_layout_blockers_survive() {
        let table = fixture();
        let before = serde_json::to_vec(&table).unwrap();
        let result = read(&table).unwrap();
        assert_eq!(result["schema"], INDEX_SCHEMA);
        assert_eq!(result["project_id"], "fixture");
        assert_eq!(result["resolution_revision"], table.revision_id);
        let group = &result["groups"][0];
        assert_eq!(group["label"], "Transformer on A0");
        assert_eq!(
            group["roles"][0]["binding"],
            serde_json::to_value(&table.bindings[0]).unwrap()
        );
        assert_eq!(
            group["roles"][1]["binding"],
            serde_json::to_value(&table.bindings[1]).unwrap()
        );
        assert_eq!(
            group["declaration_source"]["content_sha256"],
            table.bindings[0].content_sha256
        );
        assert_eq!(group["composition_ready"], false);
        assert_eq!(group["layouts"][0]["paper"], "A0");
        assert!(group["layouts"][0].get("layout").is_none());
        assert_eq!(
            group["blockers"][0]["code"],
            "style_purpose_layout_capture_required"
        );
        assert_eq!(before, serde_json::to_vec(&table).unwrap());
    }

    #[test]
    fn required_roster_and_linetype_have_no_defaults() {
        let mut table = fixture();
        purpose(&mut table)["roles"].as_array_mut().unwrap().pop();
        refresh(&mut table);
        assert_eq!(read(&table).unwrap_err().code, "style_purpose_role_missing");
        let mut table = fixture();
        purpose(&mut table)["roles"][1]
            .as_object_mut()
            .unwrap()
            .remove("line_type");
        refresh(&mut table);
        assert_eq!(
            read(&table).unwrap_err().code,
            "style_purpose_line_type_missing"
        );
        let mut table = fixture();
        table.bindings[1].document["paint"]
            .as_object_mut()
            .unwrap()
            .remove("line-dasharray");
        refresh(&mut table);
        assert_eq!(
            read(&table).unwrap_err().code,
            "style_purpose_line_type_missing"
        );
        let mut table = fixture();
        table.bindings[1].document["paint"]["line-dasharray"] = json!([2, 4]);
        refresh(&mut table);
        assert_eq!(
            read(&table).unwrap_err().code,
            "style_purpose_constraint_mismatch"
        );
    }

    #[test]
    fn project_source_role_and_content_pins_are_fenced() {
        let table = fixture();
        assert_eq!(
            index("other", &table, Target::Print, Ink::Colour)
                .unwrap_err()
                .code,
            "project_context_changed"
        );
        for dimension in ["source_kind", "role", "entity_class"] {
            let mut table = fixture();
            purpose(&mut table)["roles"][1]["key"][dimension] = json!("different");
            refresh(&mut table);
            assert_eq!(read(&table).unwrap_err().code, "style_resolution_unknown");
        }
        let mut table = fixture();
        table.bindings[1].document["paint"]["line-color"] = json!("#000000");
        assert_eq!(
            read(&table).unwrap_err().code,
            "style_resolution_revision_mismatch"
        );
        let mut table = fixture();
        purpose(&mut table)["label"] = json!("unheld edit");
        assert_eq!(
            read(&table).unwrap_err().code,
            "style_resolution_revision_mismatch"
        );
    }

    #[test]
    fn exact_authored_linetype_expressions_are_compared_without_evaluation() {
        let mut table = fixture();
        let expression = json!([
            "match",
            ["get", "cable_size"],
            "35mm",
            ["literal", [4, 2]],
            ["literal", []]
        ]);
        purpose(&mut table)["roles"][1]["line_type"] = expression.clone();
        table.bindings[1].document["paint"]["line-dasharray"] = expression;
        refresh(&mut table);
        assert!(read(&table).is_ok());
        table.bindings[1].document["paint"]["line-dasharray"][3] = json!(["literal", [2, 4]]);
        refresh(&mut table);
        assert_eq!(
            read(&table).unwrap_err().code,
            "style_purpose_constraint_mismatch"
        );
    }

    #[test]
    fn target_and_ink_never_choose_a_neighbouring_purpose() {
        let table = fixture();
        assert_eq!(
            index("fixture", &table, Target::Screen, Ink::Colour)
                .unwrap_err()
                .code,
            "style_purpose_missing"
        );
        assert_eq!(
            index("fixture", &table, Target::Print, Ink::Monochrome)
                .unwrap_err()
                .code,
            "style_purpose_missing"
        );
        assert_eq!(
            index("fixture", &table, Target::Screen, Ink::Monochrome)
                .unwrap_err()
                .code,
            "style_resolution_invalid"
        );
        let mut table = fixture();
        purpose(&mut table)["roles"][1]["key"]["target"] = json!("screen");
        refresh(&mut table);
        assert_eq!(read(&table).unwrap_err().code, "style_purpose_invalid");
    }

    #[test]
    fn duplicate_declarations_and_layout_gaps_refuse_without_body_admission() {
        let mut table = fixture();
        table.bindings[1].document["metadata"] = table.bindings[0].document["metadata"].clone();
        refresh(&mut table);
        assert_eq!(read(&table).unwrap_err().code, "style_purpose_ambiguous");
        let mut table = fixture();
        purpose(&mut table)["layouts"] = json!([]);
        refresh(&mut table);
        assert_eq!(read(&table).unwrap_err().code, "style_purpose_role_missing");
        let mut table = fixture();
        purpose(&mut table)["layouts"][0]["revision_id"] = json!("missing");
        refresh(&mut table);
        assert_eq!(read(&table).unwrap_err().code, "style_purpose_invalid");
        let mut table = fixture();
        purpose(&mut table)["layouts"][0]["layout"] = json!({});
        refresh(&mut table);
        assert_eq!(read(&table).unwrap_err().code, "style_purpose_invalid");
    }

    #[test]
    fn style_editing_is_explicit_and_the_declaration_schema_is_closed() {
        let mut table = fixture();
        purpose(&mut table)["kind"] = json!("style_editing");
        purpose(&mut table)["required_layout_roles"] = json!([]);
        purpose(&mut table)["layouts"] = json!([]);
        refresh(&mut table);
        assert_eq!(
            read(&table).unwrap()["groups"][0]["composition_ready"],
            false
        );
        assert_eq!(
            read(&table).unwrap()["groups"][0]["style_roles_ready"],
            true
        );
        let schema = declaration_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["schema"]["const"], DECLARATION_SCHEMA);
        let mut table = fixture();
        table.bindings[0].document["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("style_purposes");
        refresh(&mut table);
        assert_eq!(read(&table).unwrap_err().code, "style_purpose_missing");
    }
}
