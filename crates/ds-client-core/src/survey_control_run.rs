use crate::survey_control::*;
use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile,
};
use serde_json::{json, Value};

fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    route: SurveyRoute,
    payload: Value,
) -> Result<Value, ClientError> {
    let response = transport.survey_control(crate::SurveyControlCall {
        profile,
        route,
        payload,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::survey_control::response(response)
}
fn checked_form(value: Value, slug: Option<&str>) -> Result<Value, ClientError> {
    if !value.is_object()
        || !value["slug"].is_string()
        || !value["fields"].is_array()
        || value["version"].as_u64().is_none()
        || slug.is_some_and(|s| value["slug"] != s)
    {
        return Err(unreadable());
    }
    Ok(value)
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: Option<&str>,
    command: &SurveyControlCommand,
) -> Result<Value, ClientError> {
    command.validate()?;
    // A hint that is not the selection is a selection refusal, never an
    // invalid one: `invalid()` stays for ids that are genuinely malformed.
    if command
        .project_hint()
        .is_some_and(|hint| Some(hint) != project)
    {
        return Err(project_not_selected());
    }
    let mut send = |route, payload| call(profile, transport, authority, route, payload);
    use SurveyControlCommand::*;
    match command {
        FormsList(q) => {
            let mut body = json!({"action":"list"});
            if let Some(status) = q.status.as_deref().filter(|s| *s != "all") {
                body["status"] = json!(status);
            }
            let raw = send(SurveyRoute::Factory, body)?;
            let rows = raw["forms"].as_array().ok_or_else(unreadable)?;
            let query = q.query.as_deref().unwrap_or("").to_lowercase();
            let mut rows: Vec<_> = rows
                .iter()
                .filter(|r| {
                    ["slug", "display_name", "description"]
                        .iter()
                        .any(|k| r[*k].as_str().unwrap_or("").to_lowercase().contains(&query))
                })
                .collect();
            rows.sort_by_key(|r| r["slug"].as_str().unwrap_or(""));
            if rows.iter().any(|r| !r["slug"].is_string()) {
                return Err(unreadable());
            }
            Ok(
                json!({"status":q.status,"query":q.query,"total":rows.len(),"forms":rows.iter().take(q.limit).map(|r|form_summary(r,q.detail)).collect::<Vec<_>>(),"more":{"omitted":rows.len().saturating_sub(q.limit)}}),
            )
        }
        FormRead(q) => {
            let result = send(SurveyRoute::Factory, json!({"action":"get","slug":q.slug}))?;
            page_form(checked_form(result["form"].clone(), Some(&q.slug))?, q)
        }
        FormTypes(_) => {
            let result = send(SurveyRoute::Factory, json!({"action":"get_field_types"}))?;
            if !result["field_types"].is_array()
                || !result["condition_operators"].is_array()
                || !result["type_aliases"].is_object()
            {
                return Err(unreadable());
            }
            Ok(
                json!({"fieldTypes":result["field_types"],"conditionOperators":result["condition_operators"],"typeAliases":result["type_aliases"]}),
            )
        }
        FormCreate(v) => {
            let result = send(
                SurveyRoute::Factory,
                json!({"action":"create","schema_data":v.schema}),
            )?;
            Ok(json!({"form":checked_form(result["form"].clone(),v.schema["slug"].as_str())?}))
        }
        FormUpdate(v) => {
            let result = send(
                SurveyRoute::Factory,
                json!({"action":"update","slug":v.slug,"version":v.expected_version,"schema_data":v.schema}),
            )?;
            Ok(json!({"form":checked_form(result["form"].clone(),Some(&v.slug))?}))
        }
        FormLifecycle(v) => {
            let action = serde_json::to_value(&v.action).expect("enum");
            let mut body = json!({"action":action,"slug":v.slug});
            match v.action {
                FormAction::Duplicate => {
                    body["new_display_name"] = json!(v.new_display_name);
                    body["confirm_duplicate"] = json!(true);
                }
                FormAction::Publish | FormAction::Unpublish => {
                    let current =
                        send(SurveyRoute::Factory, json!({"action":"get","slug":v.slug}))?;
                    let current = checked_form(current["form"].clone(), Some(&v.slug))?;
                    let version = current["version"].as_u64().ok_or_else(unreadable)?;
                    if v.expected_version.is_some_and(|n| n != version) {
                        return Err(invalid());
                    }
                    body["version"] = json!(version);
                }
                FormAction::Archive => {
                    body["confirm_archive"] = json!(true);
                    body["force_delete"] = json!(v.force);
                }
                FormAction::Delete => {
                    body["confirm_delete"] = json!(true);
                    body["force_delete"] = json!(v.force);
                }
                FormAction::Restore => {}
            }
            let result = send(SurveyRoute::Factory, body)?;
            Ok(
                json!({"action":action,"slug":v.slug,"result":result.get("form").unwrap_or(&result)}),
            )
        }
        ProjectFormsRead(v) => {
            let raw = send(
                SurveyRoute::ProjectForms,
                json!({"action":"activate","project_id":v.project}),
            )?;
            validate_activation(&raw, &v.project)?;
            let query = v.query.as_deref().unwrap_or("").to_lowercase();
            let forms = raw["forms"].as_array().ok_or_else(unreadable)?;
            let mut rows: Vec<_> = forms
                .iter()
                .filter(|f| {
                    (v.status == "all"
                        || (v.status == "enabled" && f["enabled"] == true)
                        || (v.status == "disabled" && f["enabled"] != true))
                        && ["slug", "display_name", "description"]
                            .iter()
                            .any(|k| f[*k].as_str().unwrap_or("").to_lowercase().contains(&query))
                })
                .cloned()
                .collect();
            rows.sort_by_key(|r| r["slug"].as_str().unwrap_or("").to_owned());
            let empty = vec![];
            let orphans: Vec<_> = raw["orphaned_project_forms"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .filter(|f| {
                    matches!(v.status.as_str(), "all" | "unavailable")
                        && ["form_slug", "reason"]
                            .iter()
                            .any(|k| f[*k].as_str().unwrap_or("").to_lowercase().contains(&query))
                })
                .collect();
            let total = rows.len() + orphans.len();
            Ok(
                json!({"project":v.project,"status":v.status,"total":total,"available_total":rows.len(),"unavailable_total":orphans.len(),"forms":rows.iter().take(v.limit).map(|f|{let mut r=form_summary(f,v.detail);for k in ["enabled","settings_revision","project_settings"]{r[k]=f[k].clone();}r}).collect::<Vec<_>>(),"orphaned_project_forms":orphans.iter().take(v.limit.saturating_sub(rows.len())).collect::<Vec<_>>(),"more":{"omitted":total.saturating_sub(v.limit)}}),
            )
        }
        ProjectFormEditor(v) => {
            let result = send(
                SurveyRoute::ProjectForms,
                json!({"action":"settings_editor","project_id":v.project,"form_slug":v.form}),
            )?;
            if result["project_id"] != v.project
                || result["form_slug"] != v.form
                || result["editor"]["form"]["slug"] != v.form
            {
                return Err(unreadable());
            }
            Ok(json!({"project":v.project,"form":v.form,"editor":result["editor"]}))
        }
        ProjectFormsPlan(v) | ProjectFormsApply(v) => {
            let mut snapshot = send(
                SurveyRoute::ProjectForms,
                json!({"action":"activate","project_id":v.project}),
            )?;
            validate_activation(&snapshot, &v.project)?;
            if !snapshot["settings_editors"].is_object() {
                snapshot["settings_editors"] = json!({});
            }
            for row in &v.changes {
                if row.settings.is_some() {
                    let result = send(
                        SurveyRoute::ProjectForms,
                        json!({"action":"settings_editor","project_id":v.project,"form_slug":row.form_slug}),
                    )?;
                    if result["project_id"] != v.project || result["form_slug"] != row.form_slug {
                        return Err(unreadable());
                    }
                    snapshot["settings_editors"][&row.form_slug] = result["editor"].clone();
                }
            }
            let plan =
                ds_command_kernel::survey::project_forms::plan(&v.project, &snapshot, &v.changes)
                    .map_err(|_| invalid())?;
            if matches!(command, ProjectFormsPlan(_)) {
                return Ok(plan);
            }
            if plan["ready"] != true {
                return Err(invalid());
            }
            let result = send(
                SurveyRoute::ProjectForms,
                json!({"action":"bulk_save","project_id":v.project,"forms":v.changes}),
            )?;
            if result["saved_count"].as_u64() != Some(v.changes.len() as u64)
                || result["project_id"] != v.project
                || !result["saved"].as_array().is_some_and(|saved| {
                    saved.len() == v.changes.len()
                        && v.changes.iter().all(|change| {
                            saved
                                .iter()
                                .filter(|row| row["form_slug"] == change.form_slug)
                                .count()
                                == 1
                        })
                })
            {
                return Err(unreadable());
            }
            Ok(
                json!({"project":v.project,"applied":result["saved_count"],"saved":result["saved"],"authoritative_refresh":result["authoritative_refresh"],"forms":result["forms"],"orphaned_project_forms":result["orphaned_project_forms"]}),
            )
        }
        TemplatesList(q) => {
            let result = send(
                SurveyRoute::Templates,
                json!({"action":"list","data":{"limit":500}}),
            )?;
            let rows = result["templates"].as_array().ok_or_else(unreadable)?;
            let query = q.query.as_deref().unwrap_or("").to_lowercase();
            let mut rows: Vec<_> = rows
                .iter()
                .filter(|r| {
                    ["slug", "name", "description"]
                        .iter()
                        .any(|k| r[*k].as_str().unwrap_or("").to_lowercase().contains(&query))
                })
                .collect();
            rows.sort_by_key(|r| r["slug"].as_str().unwrap_or(""));
            let more = result["total"]
                .as_u64()
                .unwrap_or(rows.len() as u64)
                .saturating_sub(rows.len() as u64);
            Ok(
                json!({"total":rows.len(),"templates":rows.iter().take(q.limit).map(|r| { let mut summary=json!({"slug":r["slug"],"name":r["name"],"visibility":r["visibility"],"form_count":r["config"]["forms"].as_object().map_or(0,|m|m.len())}); if q.detail {for key in ["description","category","created_at","updated_at"] {summary[key]=r[key].clone();}} summary }).collect::<Vec<_>>(),"more":{"omitted":rows.len().saturating_sub(q.limit),"catalogue_omitted":more}}),
            )
        }
        TemplateRead(v) => {
            let mut template = send(SurveyRoute::Template(v.template.clone()), Value::Null)?;
            if template["slug"] != v.template {
                return Err(unreadable());
            }
            let forms = template["config"]["forms"]
                .as_object()
                .ok_or_else(unreadable)?;
            let total = forms.len();
            let mut ordered: Vec<_> = forms.iter().collect();
            ordered.sort_by_key(|(key, _)| *key);
            let page: serde_json::Map<_, _> = ordered
                .into_iter()
                .skip(v.form_offset)
                .take(v.form_limit)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let count = page.len();
            template["config"]["forms"] = json!(page);
            Ok(
                json!({"template":template,"formTotal":total,"formOffset":v.form_offset,"formReturned":count,"nextFormOffset":v.form_offset+count,"complete":v.form_offset+count>=total}),
            )
        }
        TemplateCreate(v) => {
            let slug = v.slug.clone().unwrap_or_else(|| {
                v.name
                    .to_lowercase()
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect::<String>()
                    .trim_matches('_')
                    .to_owned()
            });
            if slug.is_empty() {
                return Err(invalid());
            }
            let mut data = json!({"source_project_id":v.project,"template_name":v.name,"template_slug":slug,"visibility":v.visibility});
            if let Some(s) = &v.description {
                data["description"] = json!(s);
            }
            if let Some(s) = &v.category {
                data["category"] = json!(s);
            }
            let result = send(
                SurveyRoute::Templates,
                json!({"action":"create","data":data}),
            )?;
            Ok(json!({"action":"create_template_from_project","result":result}))
        }
        TemplateApply(v) => {
            let result = send(
                SurveyRoute::Projects,
                json!({"action":"apply_template","data":{"project_id":v.project,"template_slug":v.template,"merge_strategy":v.merge_strategy}}),
            )?;
            Ok(
                json!({"action":"apply_template_to_existing_project","project":v.project,"template":v.template,"merge_strategy":v.merge_strategy,"result":result}),
            )
        }
        TemplateLifecycle(v) => {
            let body = match v.action {
                TemplateAction::Delete => {
                    json!({"action":"delete","data":{"template_slug":v.template}})
                }
                _ => {
                    json!({"action":"set_public","data":{"template_slug":v.template,"is_public":matches!(v.action,TemplateAction::Publish)}})
                }
            };
            let result = send(SurveyRoute::Templates, body)?;
            Ok(json!({"action":v.action,"template":v.template,"result":result}))
        }
        ProjectCreate(v) => {
            let mut body = json!({"template_slug":v.template,"project_name":v.project_name});
            if let Some(id) = &v.project_id {
                body["project_id"] = json!(id);
            }
            let result = send(SurveyRoute::FromTemplate, body)?;
            Ok(
                json!({"action":"create_new_project_from_template","template":v.template,"project_name":v.project_name,"result":result}),
            )
        }
    }
}
fn validate_activation(snapshot: &Value, project: &str) -> Result<(), ClientError> {
    if snapshot["project_id"] != project || !snapshot["forms"].is_array() {
        Err(unreadable())
    } else {
        Ok(())
    }
}
