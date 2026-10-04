//! Thin native adapter for the canonical project selection in the shared print library.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::printing::mv::{self, ModelField, Resolved, Selection};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Read;

pub const REFUSAL: Refusal = Refusal {
    code: "mv_print_setup_refused",
    when: "the canonical MV selection, adopted revision, required text or allowed model differences are incomplete or changed",
    remedy: "read the keyed refusal in detail; inspect ds report project settings and the adopted ds report layout get document, then intentionally repair or select its exact revision",
};
pub const STYLE_REFUSAL: Refusal = Refusal {
    code: "mv_print_style_unresolved",
    when: "the project API has not supplied the governed MV paper binding",
    remedy: "integrate the print-styles resolver for (mv_booklet, project_model, print, project); adopt governed defaults through the API",
};
pub static SET: Command = Command {
    id:"report.project.mv-setup.set", path:&["report","project","mv-setup","set"], contract:1,
    summary:"Select the project's one canonical MV printing revision.",
    purpose:"Save one ds.mv-print-selection/v1 selection in the existing project settings sheet, behind an exact-base concurrent-write fence. The project must hold an MV layout copied from an exact global revision. This never publishes a template, alters geometry, advances an issue date or prints; missing model bindings or held PDF assets remain explicit print-time refusals. Discover selection and layout shapes through report layout schema.",
    chapter:Chapter::Reports, effect:Effect::GlobalWrite, authority:Authority::HeadlessProject,
    execution:Execution::Sync,
    args:&[super::PROJECT_ARG,super::LANE_ARG,Arg::value("selection","<json-file>","ds.mv-print-selection/v1: layout_id, exact revision, external_version and fixed issue_date.").required()],
    output:"Exact project and saved canonical selection. Printing readiness must be checked with the resolved model and held assets.",
    examples:&[], refusals:&super::joined::<{super::NATIVE_WRITE_REFUSALS.len()+1}>(&[super::NATIVE_WRITE_REFUSALS,&[REFUSAL]]),
    reference:Some("docs/reference/report.md"),search:&["front matter","template adoption"], requires:Requires::Server,availability:ds_cli_auth::native_availability,
};
pub static RESOLVE: Command = Command {
    id: "report.project.mv-setup.resolve",
    path: &["report", "project", "mv-setup", "resolve"],
    contract: 1,
    summary: "Resolve canonical MV setup for one model's allowed title differences.",
    purpose: "Read the named project's adopted printing revision when selected; otherwise resolve the exact approved global MV template pinned by governed printing defaults. The kernel resolves all approved project furniture and fixed version/date, accepting only model identity/title fields the template permits. The receipt is the same input Desktop and ds-report consume. This resolves documents only: actual geometry and held approved PDF assets must still be validated by the local reporter before output.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        super::PROJECT_ARG,
        super::LANE_ARG,
        Arg::value(
            "model-identity",
            "<text>",
            "Explicit model identity, only when permitted by the adopted template.",
        ),
        Arg::value(
            "model-title",
            "<text>",
            "Explicit model drawing title, only when permitted by the adopted template.",
        ),
    ],
    output: "Exact resolved setup, approved text/settings/page order, project or global lineage and canonical receipt SHA-256; rendering readiness is not asserted.",
    examples: &[],
    refusals: &super::joined::<{ super::NATIVE_READ_REFUSALS.len() + 1 }>(&[
        super::NATIVE_READ_REFUSALS,
        &[REFUSAL],
    ]),
    reference: Some("docs/reference/report.md"),
    search: &["front matter", "template adoption"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn resolve(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut fields = BTreeMap::new();
    for (arg, field) in [
        ("model-identity", ModelField::Identity),
        ("model-title", ModelField::Title),
    ] {
        if let Some(value) = inputs.value(arg) {
            fields.insert(field, value.into());
        }
    }
    Ok(
        json!({"resolved":resolve_project(inputs.require("lane")?,inputs.require("project")?,fields)?,"ready":false}),
    )
}
pub fn render_resolve(value: &Value) -> String {
    format!(
        "project {} · MV setup {} · revision {}\nreceipt {}",
        value["resolved"]["project_id"],
        value["resolved"]["selection"]["layout_id"],
        value["resolved"]["selection"]["revision"],
        value["resolved"]["sha256"]
    )
}

pub fn failure(error: mv::Refusal) -> Failure {
    Failure::invalid(REFUSAL.code, error.to_string())
        .remedy(error.remedy.clone())
        .detail(json!({"refusal":error}))
}
pub(crate) fn resolve_project(
    lane: &str,
    project: &str,
    fields: BTreeMap<ModelField, String>,
) -> Result<Resolved, Failure> {
    let configuration = ds_cli_auth::feeder_configuration_for_project(lane, project)?.into_result();
    let sheets = printing_inputs(lane, project, &configuration.document["sheets"])?;
    mv::resolve(&sheets, project, fields).map_err(failure)
}

fn printing_inputs(lane: &str, project: &str, sheets: &Value) -> Result<Value, Failure> {
    if !mv::uses_global_default(sheets) {
        return super::settings::sheets_with_printing_catalogue(lane, project, sheets, None);
    }
    let selection = mv::effective_selection(sheets).map_err(failure)?;
    let setup = ds_cli_auth::printing(
        lane,
        true,
        None,
        &ds_cli_auth::PrintingRequest::Get {
            id: selection.layout_id,
        },
    )?;
    let mut sheets = sheets.clone();
    sheets["global_printing_setups"] = json!([setup]);
    Ok(sheets)
}
/// Resolver acquisition seam for print-styles integration. The authorized
/// project API materializes this exact binding; the CLI never searches a
/// catalogue, constructs an id, or supplies a packaged paint default.
pub(crate) fn resolve_project_print(
    lane: &str,
    project: &str,
    fields: BTreeMap<ModelField, String>,
) -> Result<
    (
        Resolved,
        Value,
        Value,
        Value,
        Option<ds_command_kernel::printing::project_crs::Capture>,
    ),
    Failure,
> {
    let scoped = ds_cli_auth::feeder_configuration_for_project(lane, project)?;
    let directory = ds_cli_auth::project_directory(lane)?;
    if scoped.project_id() != project || scoped.identity() != directory.identity() {
        return Err(Failure::invalid(
            "print_project_crs_context_mismatch",
            "print project CRS/configuration context differs",
        ));
    }
    let identity = scoped.identity();
    let project_crs = directory
        .project_params(project)
        .filter(|params| params["crs"].is_object())
        .map(|params| {
            ds_command_kernel::printing::project_crs::Capture::new(
                project,
                lane,
                identity.uid(),
                identity.credential_audience_sha256(),
                params.clone(),
            )
        })
        .transpose()
        .map_err(|e| Failure::invalid("print_project_crs_invalid", e))?;
    let configuration = scoped.into_result();
    let sheets = printing_inputs(lane, project, &configuration.document["sheets"])?;
    let setup = mv::resolve(&sheets, project, fields).map_err(failure)?;
    let table = ds_cli_auth::style_governance(
        lane,
        project,
        &ds_command_kernel::style_governance::Command::Table,
    )?;
    let snapshot: ds_command_kernel::style_resolution::Snapshot = serde_json::from_value(table)
        .map_err(|error| {
            Failure::failed(STYLE_REFUSAL.code, error.to_string()).remedy(STYLE_REFUSAL.remedy)
        })?;
    let (paper, renderer) = mv::resolve_print_bindings(&setup, &snapshot).map_err(|error| {
        Failure::failed(STYLE_REFUSAL.code, error.to_string()).remedy(STYLE_REFUSAL.remedy)
    })?;
    Ok((setup, json!(paper), json!(renderer), sheets, project_crs))
}

pub fn set(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(inputs.require("selection")?)
        .and_then(|f| f.take(64 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|e| Failure::invalid(REFUSAL.code, e.to_string()).remedy(REFUSAL.remedy))?;
    if bytes.len() > 64 * 1024 {
        return Err(
            Failure::invalid(REFUSAL.code, "selection exceeds 64 KiB").remedy(REFUSAL.remedy)
        );
    }
    let selected: Selection = serde_json::from_slice(&bytes)
        .map_err(|e| Failure::invalid(REFUSAL.code, e.to_string()).remedy(REFUSAL.remedy))?;
    selected.validate().map_err(failure)?;
    let project = inputs.require("project")?;
    let lane = inputs.require("lane")?;
    let configuration = ds_cli_auth::feeder_configuration_for_project(lane, project)?.into_result();
    let expected_rows = configuration.document["sheets"]["project_settings"]
        .as_array()
        .cloned()
        .ok_or_else(|| {
            Failure::invalid(REFUSAL.code, "project_settings is not a parameter sheet")
                .remedy(REFUSAL.remedy)
        })?;
    let rows = mv::patch_selection(&expected_rows, &selected).map_err(failure)?;
    let mut sheets = configuration.document["sheets"].clone();
    sheets["project_settings"] = json!(rows);
    let sheets = super::settings::sheets_with_printing_catalogue(lane, project, &sheets, None)?;
    // A model-specific required field is intentionally supplied only by that
    // model at print time. All structural/adoption errors still refuse here.
    if let Err(error) = mv::resolve(&sheets, project, BTreeMap::new())
        && (!matches!(
            error.code.as_str(),
            "mv_print_text_missing" | "mv_print_model_binding_missing"
        ) || !matches!(error.field.as_str(), "model_identity" | "model_title"))
    {
        return Err(failure(error));
    }
    let saved = ds_cli_auth::mv_printing_selection_for_project(
        lane,
        project,
        selected.clone(),
        expected_rows,
    )?;
    let actual = mv::selection(&saved.document["sheets"]).map_err(failure)?;
    if actual != selected {
        return Err(
            Failure::failed(REFUSAL.code, "saved MV selection differs from readback")
                .remedy(REFUSAL.remedy),
        );
    }
    Ok(json!({"project":project,"lane":lane,"selection":actual,"saved":true,"ready":false}))
}
pub fn render(value: &Value) -> String {
    format!(
        "project {} · MV setup {} · revision {} · saved {}",
        value["project"],
        value["selection"]["layout_id"],
        value["selection"]["revision"],
        value["saved"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptors_declare_project_identity_mutation_confirmation_and_discovery() {
        assert!(SET.arg("project").unwrap().required);
        assert!(SET.effect.needs_confirmation());
        assert_eq!(RESOLVE.authority, Authority::HeadlessProject);
        assert!(!RESOLVE.effect.needs_confirmation());
        assert!(SET.search.contains(&"front matter"));
        assert!(RESOLVE.search.contains(&"front matter"));
    }
    #[test]
    fn kernel_refusal_is_preserved_with_field_message_key_and_remedy() {
        let error = mv::resolve(&json!({}), "project_a", BTreeMap::new()).unwrap_err();
        let expected = error.clone();
        let failure = failure(error);
        assert_eq!(failure.code(), REFUSAL.code);
        let value = serde_json::to_value(&expected).unwrap();
        assert_eq!(value["message_key"], "mv_print_setup_missing");
        assert_eq!(failure.remedy_text(), Some(expected.remedy.as_str()));
    }
}
