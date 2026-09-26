use serde::Deserialize;
use serde_json::{json, Value};

use crate::project_report::{
    CompoundedArchive, CompoundedReportReceipt, CompoundedReportRequest, ExportReportsReceipt,
    ProjectReportOperation, RetirementAction, RetirementReceipt, RetirementRequest,
    TransformerInventory, TransformerSet, TransformerStatusList,
};
use crate::projects::ProjectPayload;
use crate::tiles::TileOperation;
use crate::transport::{
    AuthorizedTransport, CallAuthority, PROJECT_FORMS_RESPONSE_LIMIT,
    PROJECT_REPORT_RESPONSE_LIMIT, PROJECT_RESPONSE_LIMIT, SOLAR_SNAPSHOT_RESPONSE_LIMIT,
    SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT, SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT,
    SURVEY_ENTRY_CREATE_RESPONSE_LIMIT, SURVEY_QUERY_RESPONSE_LIMIT, TILE_RESPONSE_LIMIT,
    TRANSFORMER_CONTEXT_RESPONSE_LIMIT,
};
use crate::{
    ClientError, ClientProfile, ErrorKind, ProjectDirectory, ProjectFormEditorCall,
    ProjectFormSettingsEditor, ProjectFormsCall, ProjectFormsSnapshot, ProjectListCall,
    ProjectReportCall, ProjectReportServiceCode, SolarSnapshot, SolarSnapshotCall,
    SurveyEntriesChanges, SurveyEntriesChangesCall, SurveyEntriesChangesRequest,
    SurveyEntriesSelectCall, SurveyEntriesSelectRequest, SurveyEntriesSelection,
    SurveyEntryCreateCall, SurveyEntryCreateReceipt, SurveyEntryCreateRequest, SurveyQueryCall,
    SurveyQueryRequest, SurveyQueryResult, TileCall, TileOperationResult, TilePreflight, TileType,
    TransformerContext, TransformerContextCall, TransportError, TransportResponse,
    PROJECT_STATUS_ORDER,
};

#[derive(Clone, Copy)]
pub(crate) struct Authority<'a> {
    pub(crate) bearer_token: &'a str,
    pub(crate) uid: &'a str,
    pub(crate) email: &'a str,
    pub(crate) device_id: Option<&'a str>,
}

impl<'a> Authority<'a> {
    pub(crate) fn call(self) -> CallAuthority<'a> {
        CallAuthority {
            bearer_token: self.bearer_token,
            canonical_uid: self.uid,
            canonical_email: self.email,
            device_id: self.device_id,
        }
    }
}

pub(crate) fn sync_gateway<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    request: &crate::NativeSyncRequest,
) -> Result<Value, ClientError> {
    let response = transport.sync_gateway(crate::transport::SyncGatewayCall {
        profile,
        request,
        authority: authority.call(),
    })?;
    if !(200..300).contains(&response.status) {
        return Err(crate::session::sync_gateway_refusal(
            response.status,
            &response.body,
        ));
    }
    let value: Value = serde_json::from_slice(&response.body).map_err(|_| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "native Sync Center response is not JSON",
        )
    })?;
    value.get("data").cloned().ok_or_else(|| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "native Sync Center response has no data envelope",
        )
    })
}

pub(crate) fn list_projects<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
) -> Result<ProjectDirectory, ClientError> {
    let mut buckets = Vec::with_capacity(PROJECT_STATUS_ORDER.len());
    for status in PROJECT_STATUS_ORDER {
        let response = transport.list_projects(ProjectListCall {
            profile,
            status,
            authority: authority.call(),
        })?;
        if response.status != 200 {
            return Err(classify_project_error(response.status));
        }
        let payload: ProjectPayload = decode(response, PROJECT_RESPONSE_LIMIT)?;
        buckets.push((status, payload.bucket()?));
    }
    ProjectDirectory::from_buckets(buckets)
}

pub(crate) fn feeder_configuration<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    change: Option<&crate::ProjectConfigurationChange>,
) -> Result<crate::FeederConfiguration, ClientError> {
    use crate::ProjectConfigurationChange;
    let bounds = match change {
        Some(ProjectConfigurationChange::FeederLimits { minimum, maximum }) => {
            Some((*minimum, *maximum))
        }
        _ => None,
    };
    if let Some((min, max)) = bounds {
        crate::project_feeders::validate(min, max)?;
    }
    let mut send = |sheet: &str, settings: Option<&Value>| {
        crate::project_feeders::decode(transport.project_configuration(
            crate::ProjectConfigurationCall {
                profile,
                project,
                settings,
                sheet,
                authority: authority.call(),
            },
        )?)
    };
    let mut document = send("project_settings", None)?;
    let settings_change = match change {
        Some(ProjectConfigurationChange::ReadSettings) => Some(None),
        Some(ProjectConfigurationChange::SaveSheet { sheet, value }) => Some(Some((
            sheet.as_str(),
            ds_command_kernel::design_config::save_sheet(&document, sheet, value),
        ))),
        Some(ProjectConfigurationChange::SetParameter {
            sheet,
            parameter,
            raw,
        }) => Some(Some((
            sheet.as_str(),
            ds_command_kernel::design_config::set_parameter(&document, sheet, parameter, raw),
        ))),
        Some(ProjectConfigurationChange::DuplicateRuleSet {
            sheet,
            source,
            target,
        }) => Some(Some((
            sheet.as_str(),
            ds_command_kernel::design_config::duplicate_rule_set(
                &document["sheets"][sheet],
                source,
                target,
                false,
            )
            .and_then(|value| {
                ds_command_kernel::design_config::save_sheet(&document, sheet, &value)
            }),
        ))),
        _ => None,
    };
    if let Some(mutation) = settings_change {
        if !document["sheets"].is_object() {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "config_document_invalid",
            ));
        }
        let mut saved = false;
        if let Some((sheet, value)) = mutation {
            let value = value.map_err(|code| ClientError::new(ErrorKind::InvalidInput, code))?;
            if !ds_command_kernel::design_config::settings_equal(&document["sheets"][sheet], &value)
            {
                send(sheet, Some(&value))?;
                document = send(sheet, None)?;
                if !ds_command_kernel::design_config::settings_equal(
                    &document["sheets"][sheet],
                    &value,
                ) {
                    return Err(ClientError::new(
                        ErrorKind::UnreadableResponse,
                        "config_save_readback_mismatch",
                    ));
                }
                saved = true;
            }
        }
        return Ok(crate::FeederConfiguration {
            document,
            summary: json!({"project":project,"saved":saved}),
        });
    }
    if let Some((min, max)) = bounds {
        let updated =
            crate::project_feeders::updated(crate::project_feeders::rows(&document)?, min, max)?;
        send("project_settings", Some(&json!(updated)))?;
        document = send("project_settings", None)?;
        let verified = crate::project_feeders::snapshot(project, document.clone(), true)?;
        let number = |key: &str| {
            verified.summary[key]
                .as_f64()
                .or_else(|| verified.summary[key].as_str()?.parse().ok())
        };
        if number("minimum") != Some(min) || number("maximum") != Some(max) {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "feeder settings were saved but fresh read-back did not match the requested bounds",
            ));
        }
    }
    if let Some(ProjectConfigurationChange::EnsureMeterType { name }) = change {
        let updated = crate::project_feeders::ensure_meter_type(&document, name)?;
        if document["sheets"]["cust_meter_type"] != json!(updated) {
            send("cust_meter_type", Some(&json!(updated)))?;
            document = send("cust_meter_type", None)?;
            if document["sheets"]["cust_meter_type"] != json!(updated) {
                return Err(ClientError::new(
                    ErrorKind::UnreadableResponse,
                    "project configuration meter types did not match the saved catalog",
                ));
            }
        }
    }
    if let Some(ProjectConfigurationChange::DesignOutputs { rows }) = change {
        crate::project_feeders::design_outputs(rows)?;
        if crate::project_feeders::rows(&document)? != rows {
            send("project_settings", Some(&json!(rows)))?;
            document = send("project_settings", None)?;
            if crate::project_feeders::rows(&document)? != rows {
                return Err(ClientError::new(
                    ErrorKind::UnreadableResponse,
                    "project settings were saved but a fresh read-back did not return the requested output rows",
                ));
            }
        }
    }
    // Every vocabulary edit resolves against the document just fetched, saves
    // only when it actually changes the sheet, and re-reads to prove what was
    // stored. A catalogue that saved and read back differently is the one way
    // a housekeeping edit could corrupt the vocabulary it was meant to repair.
    let catalog_edit = match change {
        Some(ProjectConfigurationChange::CustomerCategoryAlias { alias, category }) => Some((
            "cust_category",
            crate::project_feeders::customer_category_alias(&document, alias, category)?,
        )),
        Some(ProjectConfigurationChange::RetireCustomerCategory { name }) => Some((
            "cust_category",
            crate::project_feeders::retire_customer_category(&document, name.as_deref())?,
        )),
        Some(ProjectConfigurationChange::RenameCustomerCategory { from, to }) => Some((
            "cust_category",
            crate::project_feeders::rename_customer_category(&document, from, to)?,
        )),
        Some(ProjectConfigurationChange::UnbindCustomerAlias { alias, category }) => Some((
            "cust_category",
            crate::project_feeders::unbind_customer_alias(&document, alias, category)?,
        )),
        Some(ProjectConfigurationChange::DefaultMeterType { name }) => Some((
            "cust_meter_type",
            crate::project_feeders::default_meter_type(&document, name)?,
        )),
        _ => None,
    };
    if let Some((sheet, updated)) = catalog_edit {
        if document["sheets"][sheet] != json!(updated) {
            send(sheet, Some(&json!(updated)))?;
            document = send(sheet, None)?;
            if document["sheets"][sheet] != json!(updated) {
                return Err(ClientError::new(
                    ErrorKind::UnreadableResponse,
                    "project configuration customer categories did not match the saved catalog",
                ));
            }
        }
    }
    crate::project_feeders::snapshot(project, document, change.is_some())
}

pub(crate) fn transformer_context<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    transformer: &str,
) -> Result<TransformerContext, ClientError> {
    let response = transport.transformer_context(TransformerContextCall {
        profile,
        ds_project,
        transformer,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_transformer_context_error(response.status));
    }
    crate::transformers::decode_context(
        response,
        TRANSFORMER_CONTEXT_RESPONSE_LIMIT,
        ds_project,
        transformer,
    )
}

pub(crate) fn project_forms<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
) -> Result<ProjectFormsSnapshot, ClientError> {
    let response = transport.project_forms(ProjectFormsCall {
        profile,
        ds_project,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_project_forms_error(response.status));
    }
    crate::project_forms::decode_project_forms(
        response,
        PROJECT_FORMS_RESPONSE_LIMIT,
        ds_project,
        authority.email,
    )
}

pub(crate) fn project_form_editor<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    form_slug: &str,
) -> Result<ProjectFormSettingsEditor, ClientError> {
    let response = transport.project_form_editor(ProjectFormEditorCall {
        profile,
        ds_project,
        form_slug,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_project_forms_error(response.status));
    }
    crate::project_form_editor::decode_editor(
        response,
        PROJECT_FORMS_RESPONSE_LIMIT,
        ds_project,
        form_slug,
    )
}

pub(crate) fn solar_snapshot<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    template_id: &str,
) -> Result<SolarSnapshot, ClientError> {
    let response = transport.solar_snapshot(SolarSnapshotCall {
        profile,
        ds_project,
        template_id,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_solar_snapshot_error(response.status));
    }
    crate::solar_snapshot::decode_snapshot(
        response,
        SOLAR_SNAPSHOT_RESPONSE_LIMIT,
        ds_project,
        template_id,
    )
}

pub(crate) fn survey_query<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    query: &SurveyQueryRequest,
) -> Result<SurveyQueryResult, ClientError> {
    let response = transport.survey_query(SurveyQueryCall {
        profile,
        ds_project,
        query,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        if let Some(error) =
            crate::survey_form_read::decode_service_error(&response, SURVEY_QUERY_RESPONSE_LIMIT)
        {
            return Err(error);
        }
        if let Some(error) =
            crate::survey_query::decode_service_error(&response, SURVEY_QUERY_RESPONSE_LIMIT)
        {
            return Err(error);
        }
        return Err(classify_survey_query_error(&response));
    }
    crate::survey_query::decode(response, SURVEY_QUERY_RESPONSE_LIMIT, query)
}

pub(crate) fn survey_entries_select<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    request: &SurveyEntriesSelectRequest,
) -> Result<SurveyEntriesSelection, ClientError> {
    let response = transport.survey_entries_select(SurveyEntriesSelectCall {
        profile,
        ds_project,
        request,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        if let Some(error) = crate::survey_form_read::decode_service_error(
            &response,
            SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT,
        ) {
            return Err(error);
        }
        return Err(crate::survey_entries_select::decode_service_error(
            &response,
            SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT,
        )
        .unwrap_or_else(|| classify_survey_entries_select_error(&response)));
    }
    crate::survey_entries_select::decode(
        response,
        SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT,
        ds_project,
        request,
    )
}

pub(crate) fn survey_entries_changes<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    request: &SurveyEntriesChangesRequest,
) -> Result<SurveyEntriesChanges, ClientError> {
    let response = transport.survey_entries_changes(SurveyEntriesChangesCall {
        profile,
        ds_project,
        request,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        if let Some(error) = crate::survey_form_read::decode_service_error(
            &response,
            SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT,
        ) {
            return Err(error);
        }
        return Err(crate::survey_entries_changes::decode_service_error(
            &response,
            SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT,
        )
        .unwrap_or_else(|| classify_survey_entries_changes_error(&response)));
    }
    crate::survey_entries_changes::decode(
        response,
        SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT,
        ds_project,
        request,
    )
}

pub(crate) fn survey_entry_create<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project_id: &str,
    request: &SurveyEntryCreateRequest,
) -> Result<SurveyEntryCreateReceipt, ClientError> {
    let response = transport.survey_entry_create(SurveyEntryCreateCall {
        profile,
        project_id,
        request,
        authority: authority.call(),
    })?;
    if response.status != 201 {
        return Err(crate::survey_entry_create::decode_service_error(
            &response,
            SURVEY_ENTRY_CREATE_RESPONSE_LIMIT,
        )
        .unwrap_or_else(|| classify_survey_entry_create_error(response.status)));
    }
    crate::survey_entry_create::decode(
        response,
        SURVEY_ENTRY_CREATE_RESPONSE_LIMIT,
        project_id,
        request,
    )
}

pub(crate) fn tile_status<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    tile_type: TileType,
) -> Result<TileOperationResult, ClientError> {
    let response = transport.tiles(TileCall {
        profile,
        ds_project,
        operation: TileOperation::Status(tile_type),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::tiles::decode_operation(response, TILE_RESPONSE_LIMIT, ds_project, tile_type, false)
}

pub(crate) fn tile_preflight<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    tile_type: TileType,
) -> Result<TilePreflight, ClientError> {
    let response = transport.tiles(TileCall {
        profile,
        ds_project,
        operation: TileOperation::Preflight(tile_type),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::tiles::decode_preflight(response, TILE_RESPONSE_LIMIT, ds_project, tile_type)
}

pub(crate) fn tile_generate<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    tile_type: TileType,
    force: bool,
) -> Result<TileOperationResult, ClientError> {
    let response = transport.tiles(TileCall {
        profile,
        ds_project,
        operation: TileOperation::Generate { tile_type, force },
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::tiles::decode_operation(response, TILE_RESPONSE_LIMIT, ds_project, tile_type, true)
}

pub(crate) fn compounded_report<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    request: &CompoundedReportRequest,
) -> Result<CompoundedReportReceipt, ClientError> {
    let response = transport.project_report(ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::Compounded(request),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_project_report_response(&response));
    }
    crate::project_report::decode_compounded(response, PROJECT_REPORT_RESPONSE_LIMIT, ds_project)
}

/// One preview-pinned pole-material catalogue repair through the `/report`
/// action gate. `request` is the kernel's fenced request
/// (`design_config::material_propagation_request`); the receipt is returned
/// whole for the kernel to judge (`material_propagation_receipt`), so a
/// headless caller gets the same verdict the browser got.
pub(crate) fn material_propagation<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    request: &serde_json::Value,
) -> Result<serde_json::Value, ClientError> {
    if !request.is_object() || request.to_string().len() > 512 * 1024 {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "a material propagation request is one kernel-built object of at most 512 KiB",
        ));
    }
    let response = transport.project_report(ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::MaterialPropagation(request),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        // The gate answers `{"detail": …}` for a missing capability and the
        // service its own sentence; both cross as the route's bounded refusal.
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap_or_default();
        let sentence = body["detail"]
            .as_str()
            .or_else(|| body["error"]["message"].as_str())
            .or_else(|| body["error"].as_str());
        let code = body["error_code"]
            .as_str()
            .or_else(|| body["error"]["code"].as_str())
            .map(str::to_ascii_lowercase);
        return Err(ClientError::with_service_refusal(
            classify_project_report_error(response.status).kind(),
            "the material propagation route refused the request",
            crate::ServiceRefusal::new(response.status, code.as_deref(), sentence),
        ));
    }
    if response.body.len() > PROJECT_REPORT_RESPONSE_LIMIT {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the material propagation receipt exceeds its bound",
        ));
    }
    serde_json::from_slice(&response.body).map_err(|_| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "the material propagation receipt is not a JSON document",
        )
    })
}

/// Ask ds-brain to compute the individual reports of exact transformers in
/// the cloud and publish them to the project. The route requires at least
/// one name — an empty list is not "every active" there but a fall-through
/// to the retired generic proxy — so the door refuses it before any bytes.
pub(crate) fn export_reports<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    request: &TransformerSet,
) -> Result<ExportReportsReceipt, ClientError> {
    if request.is_empty() {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "a cloud report computation names at least one transformer",
        ));
    }
    let response = transport.project_report(ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::ExportReports(request),
        authority: authority.call(),
    })?;
    // 207 is this route's own partial verdict, decoded like a 200; every
    // other status is a refusal.
    if !matches!(response.status, 200 | 207) {
        return Err(classify_export_reports_response(&response));
    }
    crate::project_report::decode_export_reports(
        response,
        PROJECT_REPORT_RESPONSE_LIMIT,
        ds_project,
        request,
    )
}

pub(crate) fn compounded_report_list<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
) -> Result<Vec<CompoundedArchive>, ClientError> {
    let response = transport.project_report(ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::CompoundedList,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_project_report_response(&response));
    }
    crate::project_report::decode_compounded_list(response, PROJECT_REPORT_RESPONSE_LIMIT)
}

pub(crate) fn transformer_inventory<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    requested: &TransformerSet,
) -> Result<TransformerInventory, ClientError> {
    let response = transport.project_report(ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::Inventory(requested),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_project_report_response(&response));
    }
    crate::project_report::decode_inventory(
        response,
        PROJECT_REPORT_RESPONSE_LIMIT,
        ds_project,
        requested,
    )
}

pub(crate) fn transformer_status<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    requested: &TransformerSet,
) -> Result<TransformerStatusList, ClientError> {
    let response = transport.project_report(ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::ListStatus(requested),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_project_report_response(&response));
    }
    crate::project_report::decode_status(response, PROJECT_REPORT_RESPONSE_LIMIT)
}

pub(crate) fn transformer_retirement<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    action: RetirementAction,
    request: &RetirementRequest,
) -> Result<RetirementReceipt, ClientError> {
    let operation = match action {
        RetirementAction::Retire => ProjectReportOperation::Retire(request),
        RetirementAction::Restore => ProjectReportOperation::Restore(request),
    };
    let response = transport.project_report(ProjectReportCall {
        profile,
        ds_project,
        operation,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_project_report_response(&response));
    }
    crate::project_report::decode_retirement_receipt(
        response,
        PROJECT_REPORT_RESPONSE_LIMIT,
        ds_project,
        action,
        request,
    )
}

pub(crate) fn validate_transformer_context(
    ds_project: &str,
    transformer: &str,
) -> Result<(), ClientError> {
    crate::transformers::validate_context_input(ds_project, transformer)
}

pub(crate) fn validate_project_forms(ds_project: &str) -> Result<(), ClientError> {
    if ds_project.is_empty()
        || ds_project.trim() != ds_project
        || ds_project.chars().count() > 200
        || ds_project.chars().any(char::is_control)
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "the selected project id is empty, untrimmed, or outside its 200-character bound",
        ));
    }
    Ok(())
}

pub(crate) fn validate_project_form_editor(
    ds_project: &str,
    form_slug: &str,
) -> Result<(), ClientError> {
    crate::project_form_editor::validate_editor_input(ds_project, form_slug)
}

pub(crate) fn validate_solar_snapshot(
    ds_project: &str,
    template_id: &str,
) -> Result<(), ClientError> {
    crate::solar_snapshot::validate_input(ds_project, template_id)
}

pub(crate) fn validate_survey_query(ds_project: &str) -> Result<(), ClientError> {
    crate::survey_query::validate_project(ds_project)
}

pub(crate) fn validate_survey_entries_select(ds_project: &str) -> Result<(), ClientError> {
    crate::survey_entries_select::validate_project(ds_project)
}

pub(crate) fn validate_survey_entries_changes(ds_project: &str) -> Result<(), ClientError> {
    crate::survey_entries_changes::validate_project(ds_project)
}

pub(crate) fn validate_survey_entry_create(project_id: &str) -> Result<(), ClientError> {
    crate::survey_entry_create::validate_project(project_id)
}

pub(crate) fn validate_tiles(ds_project: &str) -> Result<(), ClientError> {
    crate::tiles::validate_project(ds_project)
}

pub(crate) fn validate_project_report(ds_project: &str) -> Result<(), ClientError> {
    crate::tiles::validate_project(ds_project)
}

fn decode<T: for<'de> Deserialize<'de>>(
    response: TransportResponse,
    limit: usize,
) -> Result<T, ClientError> {
    if response.body.len() > limit {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the service response exceeded its operation bound",
        ));
    }
    serde_json::from_slice(&response.body).map_err(|_| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "the service response is not its closed JSON contract",
        )
    })
}

fn classify_project_error(status: u16) -> ClientError {
    classified(
        status,
        "the project directory is temporarily unavailable",
        "the project directory refused the verified user",
    )
}
fn classify_transformer_context_error(status: u16) -> ClientError {
    match status {
        401 => {
            return ClientError::new(
                ErrorKind::AuthenticationRejected,
                "the transformer context route rejected authentication (HTTP 401)",
            );
        }
        403 => {
            return ClientError::new(
                ErrorKind::AuthenticationRejected,
                "the transformer context route denied access (HTTP 403)",
            );
        }
        404 => {
            return ClientError::new(
                ErrorKind::ResourceNotFound,
                "the transformer context route is unavailable (HTTP 404); individual missing transformers are returned inside a successful batch response",
            );
        }
        _ => {}
    }
    classified(
        status,
        "the transformer context service is temporarily unavailable",
        "the transformer context service refused the verified user or selected transformer",
    )
}
fn classify_project_forms_error(status: u16) -> ClientError {
    classified(
        status,
        "the project-forms service is temporarily unavailable",
        "the project-forms service refused the verified user or selected project",
    )
}
fn classify_solar_snapshot_error(status: u16) -> ClientError {
    if status == 404 {
        ClientError::new(
            ErrorKind::ResourceNotFound,
            "the selected project has no live Solar city with that id",
        )
    } else {
        classified(
            status,
            "the governed Solar snapshot service is temporarily unavailable",
            "the governed Solar snapshot service refused the verified user or selected city",
        )
    }
}
fn classify_survey_query_error(response: &TransportResponse) -> ClientError {
    classify_survey(
        response,
        &[400, 409],
        "the governed survey aggregate service is temporarily unavailable",
        "the governed survey aggregate service refused the typed question",
        "the governed survey aggregate service refused the verified user",
    )
}
fn classify_survey_entries_select_error(response: &TransportResponse) -> ClientError {
    classify_survey(
        response,
        &[400, 409, 413],
        "the governed survey entry selection service is temporarily unavailable",
        "the governed survey entry selection service refused the bounded selection",
        "the governed survey entry selection service refused the verified user",
    )
}
fn classify_survey_entries_changes_error(response: &TransportResponse) -> ClientError {
    classify_survey(
        response,
        &[400, 409, 410, 413],
        "the governed survey entry changes service is temporarily unavailable",
        "the governed survey entry changes service refused the bounded page",
        "the governed survey entry changes service refused the verified user",
    )
}
fn classify_survey_entry_create_error(status: u16) -> ClientError {
    if status == 404 {
        ClientError::new(
            ErrorKind::ResourceNotFound,
            "the selected project, Survey form, or context ancestor is not available",
        )
    } else if matches!(status, 408 | 425 | 429 | 500..=599) {
        ClientError::new(
            ErrorKind::Transient,
            "the governed Survey entry create service is temporarily unavailable",
        )
    } else if matches!(status, 400 | 409 | 413) {
        ClientError::new(
            ErrorKind::InvalidInput,
            "the governed Survey entry create service refused the bounded request",
        )
    } else {
        ClientError::new(
            ErrorKind::AuthenticationRejected,
            "the governed Survey entry create service refused the verified user",
        )
    }
}

/// The report route's refusal body, read only for its typed code.
///
/// Two shapes, because the route emits two: the 424 names its cause in `code`
/// while the 409s name theirs in `status`. Both are bounded and neither is
/// echoed to the caller — only the closed enum below crosses, so a server
/// string can never become CLI output.
#[derive(serde::Deserialize)]
struct ProjectReportRefusalWire {
    #[serde(default)]
    code: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    missing_individual_artifact_count: Option<i64>,
    #[serde(default)]
    missing_individual_artifact_causes: Vec<MissingArtifactCauseWire>,
}

#[derive(Deserialize)]
struct MissingArtifactCauseWire {
    #[serde(default)]
    transformer: String,
    #[serde(default)]
    code: String,
}

/// The rooms a combined run could not package, as `name:code` pairs: each name
/// a bounded identifier and each code the route's closed token. The route's
/// free-text detail never crosses; a pair outside the bounds is dropped, and
/// the list stops before the refusal's detail budget.
fn missing_rooms_details(
    wire: &ProjectReportRefusalWire,
) -> serde_json::Map<String, serde_json::Value> {
    const MAX_ROOMS_BYTES: usize = 1800;
    let token = |value: &str, max: usize, extra: &[u8]| {
        !value.is_empty()
            && value.len() <= max
            && value.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || extra.contains(&b)
            })
    };
    let mut rooms = String::new();
    let mut shown = 0_i64;
    for cause in &wire.missing_individual_artifact_causes {
        let name = cause.transformer.trim();
        let code = cause.code.trim();
        if !token(name, 128, b"-.") || !token(code, 64, b"") {
            continue;
        }
        let pair = format!("{name}:{code}");
        if rooms.len() + pair.len() + 2 > MAX_ROOMS_BYTES {
            break;
        }
        if !rooms.is_empty() {
            rooms.push_str(", ");
        }
        rooms.push_str(&pair);
        shown += 1;
    }
    let mut details = serde_json::Map::new();
    if let Some(count) = wire
        .missing_individual_artifact_count
        .filter(|count| *count >= 0)
    {
        details.insert("missing_individual_artifact_count".into(), count.into());
    }
    if shown > 0 {
        details.insert("missing_rooms".into(), rooms.into());
        details.insert("missing_rooms_shown".into(), shown.into());
    }
    details
}

/// Read the route's own refusal code out of a non-200 body.
///
/// Returns `None` for anything unrecognised, so an unknown or malformed body
/// falls back to the status-only classification below rather than inventing a
/// cause. The body is bounded before parsing: a refusal is small, and this runs
/// on a response that has already failed.
fn project_report_service_code(response: &TransportResponse) -> Option<ProjectReportServiceCode> {
    const MAX_REFUSAL_BODY: usize = 64 * 1024;
    if response.body.is_empty() || response.body.len() > MAX_REFUSAL_BODY {
        return None;
    }
    let wire: ProjectReportRefusalWire = serde_json::from_slice(&response.body).ok()?;
    match (response.status, wire.code.as_str(), wire.status.as_str()) {
        (424, "no_individual_transformer_artifacts", _) => {
            Some(ProjectReportServiceCode::NoIndividualArtifacts)
        }
        (409, _, "grouping_stale") => Some(ProjectReportServiceCode::GroupingStale),
        (409, _, "grouping_incomplete") => Some(ProjectReportServiceCode::GroupingIncomplete),
        _ => None,
    }
}

/// Classify a failed report response, preferring the cause the route named.
///
/// The status alone cannot separate these: `409` carries two unrelated causes
/// and `424` is the one a whole compounded run ends on, yet all of
/// `400|409|422|423|424` used to collapse into a single sentence with no
/// remedy. The body is consulted first and the status remains the fallback.
fn classify_project_report_response(response: &TransportResponse) -> ClientError {
    if let Some(code) = project_report_service_code(response) {
        let (kind, message) = match code {
            ProjectReportServiceCode::NoIndividualArtifacts => (
                ErrorKind::InvalidInput,
                "no selected transformer had an individual report to package",
            ),
            ProjectReportServiceCode::GroupingStale => (
                ErrorKind::InvalidInput,
                "the applied report grouping changed after this request pinned it",
            ),
            ProjectReportServiceCode::GroupingIncomplete => (
                ErrorKind::InvalidInput,
                "the applied report grouping does not cover every requested transformer",
            ),
        };
        if code == ProjectReportServiceCode::NoIndividualArtifacts {
            // The body was bounded and parsed once already to name the code.
            if let Ok(wire) = serde_json::from_slice::<ProjectReportRefusalWire>(&response.body) {
                let details = missing_rooms_details(&wire);
                let refusal = crate::ServiceRefusal::with_details(
                    response.status,
                    Some("no_individual_transformer_artifacts"),
                    None,
                    Some(&details),
                );
                return ClientError::with_project_report_refusal(kind, message, code, refusal);
            }
        }
        return ClientError::with_project_report_service_code(kind, message, code);
    }
    classify_project_report_error(response.status)
}

/// The write-governance refusal of a cloud report computation.
///
/// ds-brain gates `export_reports_only` on transformer-write governance
/// before it proxies, and answers `423 {"error_code": TRANSFORMER_EDIT_LOCKED
/// | TRANSFORMER_FINAL_LOCKED, "detail": …}` — the detail names the holder
/// of the lease, which is the one fact the operator needs. Both cross as a
/// bounded [`crate::ServiceRefusal`] (`transformer_edit_locked` /
/// `transformer_final_locked`); no new [`ProjectReportServiceCode`] variant
/// is minted for a refusal one action emits, and everything else classifies
/// as the report route always has.
fn classify_export_reports_response(response: &TransportResponse) -> ClientError {
    #[derive(serde::Deserialize)]
    struct GovernanceRefusalWire {
        error_code: String,
        #[serde(default)]
        detail: String,
    }
    const MAX_REFUSAL_BODY: usize = 64 * 1024;
    if response.status == 423
        && !response.body.is_empty()
        && response.body.len() <= MAX_REFUSAL_BODY
    {
        if let Ok(wire) = serde_json::from_slice::<GovernanceRefusalWire>(&response.body) {
            // The code that crosses is this crate's spelling, as every typed
            // refusal's is; the service's upper-case token is not echoed.
            let code = match wire.error_code.as_str() {
                "TRANSFORMER_EDIT_LOCKED" => Some("transformer_edit_locked"),
                "TRANSFORMER_FINAL_LOCKED" => Some("transformer_final_locked"),
                _ => None,
            };
            if let Some(code) = code {
                return ClientError::with_service_refusal(
                    ErrorKind::InvalidInput,
                    "the governed report service refused a locked transformer",
                    crate::ServiceRefusal::new(423, Some(code), Some(&wire.detail)),
                );
            }
        }
    }
    classify_project_report_response(response)
}

fn classify_project_report_error(status: u16) -> ClientError {
    if matches!(status, 408 | 425 | 429 | 500..=599) {
        ClientError::new(
            ErrorKind::Transient,
            "the governed report service is temporarily unavailable",
        )
    } else if status == 404 {
        ClientError::new(
            ErrorKind::ResourceNotFound,
            "the governed report service found no such project or transformer",
        )
    } else if matches!(status, 400 | 409 | 422 | 423 | 424) {
        ClientError::new(
            ErrorKind::InvalidInput,
            "the governed report service refused the bounded operation",
        )
    } else {
        ClientError::new(
            ErrorKind::AuthenticationRejected,
            "the governed report service refused the verified user or selected project",
        )
    }
}

pub(crate) fn classify_tile_error(status: u16) -> ClientError {
    if matches!(status, 408 | 425 | 429 | 500..=599) {
        ClientError::new(
            ErrorKind::Transient,
            "the managed tile service is temporarily unavailable",
        )
    } else if matches!(status, 400 | 409) {
        ClientError::new(
            ErrorKind::InvalidInput,
            "the managed tile service refused the bounded operation",
        )
    } else {
        ClientError::new(
            ErrorKind::AuthenticationRejected,
            "the managed tile service refused the verified user or selected project",
        )
    }
}

fn classified(status: u16, transient: &'static str, rejected: &'static str) -> ClientError {
    if matches!(status, 408 | 425 | 429 | 500..=599) {
        ClientError::new(ErrorKind::Transient, transient)
    } else {
        ClientError::new(ErrorKind::AuthenticationRejected, rejected)
    }
}

/// The longest body this boundary will read only to learn who wrote it.
const MAX_AUTHORSHIP_BODY_BYTES: usize = 64 * 1024;

/// Whether ds-brain authored this refusal at all.
///
/// Every ds-brain answer, refusal included, is the DS envelope, and the DS
/// envelope always carries `success`. API Gateway, asked for a path its spec
/// was never taught, answers its own `{"message": …, "code": 404}` and
/// ds-brain is never reached. The two carry the same status and share nothing
/// else, so the presence of `success` is the whole discriminator — and it has
/// to be presence rather than a well-formed code, because a malformed code
/// from the service is still the service. Nothing from the body crosses this
/// boundary; only this one boolean does.
fn service_authored(response: &TransportResponse) -> bool {
    #[derive(serde::Deserialize)]
    struct Envelope {
        success: Option<bool>,
    }
    if response.body.len() > MAX_AUTHORSHIP_BODY_BYTES {
        // Far too large to be the Gateway's own one-sentence refusal.
        return true;
    }
    serde_json::from_slice::<Envelope>(&response.body)
        .is_ok_and(|envelope| envelope.success.is_some())
}

fn classify_survey(
    response: &TransportResponse,
    invalid_statuses: &[u16],
    transient: &'static str,
    invalid: &'static str,
    rejected: &'static str,
) -> ClientError {
    let status = response.status;
    if status == 404 && !service_authored(response) {
        ClientError::new(
            ErrorKind::RouteUnavailable,
            "this deployment does not serve the governed Survey read route",
        )
    } else if status == 404 {
        ClientError::new(
            ErrorKind::ResourceNotFound,
            "the selected project or survey form is not available to this user",
        )
    } else if matches!(status, 408 | 425 | 429 | 500..=599) {
        ClientError::new(ErrorKind::Transient, transient)
    } else if invalid_statuses.contains(&status) {
        ClientError::new(ErrorKind::InvalidInput, invalid)
    } else {
        ClientError::new(ErrorKind::AuthenticationRejected, rejected)
    }
}

pub(crate) fn tile_list<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    include_global: bool,
) -> Result<crate::TileCatalog, ClientError> {
    validate_tiles(ds_project)?;

    let response = transport.tiles(TileCall {
        profile,
        ds_project,
        operation: TileOperation::List { include_global },
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::tiles::decode_catalog(response, TILE_RESPONSE_LIMIT, ds_project, include_global)
}

pub(crate) fn tile_add<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    tile_type: TileType,
    source_project: &str,
) -> Result<crate::TileMutation, ClientError> {
    validate_tiles(ds_project)?;
    crate::tiles::validate_catalog_id(source_project)?;
    let response = transport.tiles(TileCall {
        profile,
        ds_project,
        operation: TileOperation::Add {
            tile_type,
            source_project,
        },
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::tiles::decode_mutation(
        response,
        TILE_RESPONSE_LIMIT,
        Some(&format!("{}_{}", source_project, tile_type.token())),
        crate::TileScope::Project,
    )
}

pub(crate) fn tile_remove<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    tile_id: &str,
    scope: crate::TileScope,
) -> Result<crate::TileMutation, ClientError> {
    validate_tiles(ds_project)?;
    crate::tiles::validate_catalog_id(tile_id)?;
    let response = transport.tiles(TileCall {
        profile,
        ds_project,
        operation: TileOperation::Remove { tile_id, scope },
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::tiles::decode_mutation(response, TILE_RESPONSE_LIMIT, Some(tile_id), scope)
}

pub(crate) fn layer_config<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    refresh: bool,
) -> Result<crate::LayerSnapshot, ClientError> {
    validate_tiles(ds_project)?;
    let response = transport.layers(crate::LayersCall {
        profile,
        ds_project,
        operation: crate::layers::LayerOperation::Read { refresh },
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::layers::decode_snapshot(response, ds_project)
}

pub(crate) fn style_catalog<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
) -> Result<crate::StyleSnapshot, ClientError> {
    validate_tiles(ds_project)?;
    let response = transport.layers(crate::LayersCall {
        profile,
        ds_project,
        operation: crate::layers::LayerOperation::StyleCatalogRead,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::layers::decode_style_snapshot(response, ds_project)
}

pub(crate) fn layer_reorder<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    orders: &[crate::LayerOrder],
) -> Result<crate::LayerOrderReceipt, ClientError> {
    validate_tiles(ds_project)?;
    crate::layers::validate_orders(orders)?;
    let snapshot = layer_config(profile, transport, authority, ds_project, false)?;
    let known = snapshot.canonical_ids();
    if orders
        .iter()
        .any(|row| !known.contains(row.layer_id.as_str()))
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "unknown canonical layer identity",
        ));
    }
    let response = transport.layers(crate::LayersCall {
        profile,
        ds_project,
        operation: crate::layers::LayerOperation::Reorder(orders),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::layers::decode_order(response, ds_project, orders.len())
}

pub(crate) fn layer_default_visibility<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    defaults: &[crate::LayerVisibilityDefault],
) -> Result<crate::LayerVisibilityDefaultReceipt, ClientError> {
    validate_tiles(ds_project)?;
    crate::layers::validate_visibility_defaults(defaults)?;
    let snapshot = layer_config(profile, transport, authority, ds_project, false)?;
    let known = snapshot.canonical_ids();
    if defaults
        .iter()
        .any(|row| !known.contains(row.layer_id.as_str()))
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "unknown canonical layer identity",
        ));
    }
    let response = transport.layers(crate::LayersCall {
        profile,
        ds_project,
        operation: crate::layers::LayerOperation::DefaultVisibility(defaults),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    crate::layers::decode_visibility_default(response, ds_project, defaults.len())
}

pub(crate) fn style_edit<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    ds_project: &str,
    reference: &str,
    instruction: &crate::StyleInstruction,
    apply: bool,
) -> Result<crate::StyleEditReceipt, ClientError> {
    validate_tiles(ds_project)?;
    if reference.is_empty() || reference.len() > 500 {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "invalid style reference",
        ));
    }
    let snapshot = style_catalog(profile, transport, authority, ds_project)?;
    let plan =
        ds_command_kernel::style_plan::plan_style_edit(snapshot.document(), reference, instruction)
            .map_err(|_| {
                ClientError::new(
            ErrorKind::InvalidInput,
            "style instruction violates the published field, type, range or routing contract",
        )
            })?;
    // The kernel owns what a publish may carry — the closed field set, the
    // typed identities and the 2 MiB bound — so the body it builds for the
    // call below is the payload it already admitted.
    ds_command_kernel::style_document::validate_publish_payload(&plan.payload).map_err(|_| {
        ClientError::new(
            ErrorKind::InvalidInput,
            "style payload violates the published style write contract",
        )
    })?;
    let mut data = plan.receipt;
    data.as_object_mut()
        .expect("style planner receipt")
        .entry("warnings")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .expect("style planner warnings")
        .extend(
            snapshot
                .warnings()
                .iter()
                .map(|warning| serde_json::Value::String(warning.clone())),
        );
    if apply {
        let response = transport.styles(crate::StylesCall {
            profile,
            payload: &plan.payload,
            authority: authority.call(),
        })?;
        if response.status != 200 {
            return Err(classify_tile_error(response.status));
        }
        // Whether the receipt is the style that was authored, and what its
        // warnings say, is the kernel's answer — the same one the browser
        // reads through the `styleDocument` export.
        let published = crate::styles::admit_publish(&plan.payload, response)?;
        data["document"] = published["document"].clone();
        data.as_object_mut()
            .expect("style planner receipt")
            .get_mut("warnings")
            .expect("inserted style planner warnings")
            .as_array_mut()
            .expect("style planner warnings")
            .extend(
                published["warnings"]
                    .as_array()
                    .expect("admitted style warnings")
                    .iter()
                    .cloned(),
            );
        data["published"] = serde_json::json!(true);
        data["dryRun"] = serde_json::json!(false);
    }
    Ok(crate::StyleEditReceipt { data })
}

pub(crate) fn project_data<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: crate::ProjectDataCommand<'_>,
) -> Result<crate::ProjectDataReceipt, ClientError> {
    validate_tiles(project)?;
    command.validate()?;
    let send = |transport: &mut T,
                payload: &serde_json::Value|
     -> Result<serde_json::Value, ClientError> {
        let response = transport.project_data(crate::ProjectDataCall {
            profile,
            payload,
            authority: authority.call(),
        })?;
        if response.status != 200 {
            return Err(classify_tile_error(response.status));
        }
        crate::layers::data(response)
    };
    let mut data = match command {
        crate::ProjectDataCommand::List => crate::project_data::list(send(
            transport,
            &serde_json::json!({"action":"list","project_id":project}),
        )?)?,
        crate::ProjectDataCommand::Remove { upload_id } => {
            let data = send(
                transport,
                &serde_json::json!({"action":"delete","project_id":project,"upload_id":upload_id}),
            )?;
            if data["upload_id"] != upload_id {
                return Err(crate::project_data::bad());
            }
            serde_json::json!({"upload_id":upload_id,"removed":true})
        }
        crate::ProjectDataCommand::Upload {
            file_name,
            size,
            reader,
        } => {
            let start = send(
                transport,
                &serde_json::json!({"action":"upload_start","project_id":project,"file_name":file_name,"size":size,"content_type":"application/octet-stream"}),
            )?;
            let issued = crate::project_data::issued(&start, project)?;
            let response = transport.upload_bytes(crate::UploadBytesCall {
                uri: &issued.uri,
                size,
                reader,
            })?;
            crate::project_data::uploaded(response)?;
            let done = send(
                transport,
                &serde_json::json!({"action":"upload","project_id":project,"blob_path":issued.blob,"file_name":file_name}),
            )?;
            if done["upload_id"] != issued.id
                || done["tile_status"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() < 100)
                    .is_none()
            {
                return Err(crate::project_data::bad());
            }
            serde_json::json!({"upload_id":issued.id,"file_name":file_name,"bytes":size,"registered":true,"tile_status":done["tile_status"],"ready":matches!(done["tile_status"].as_str(),Some("success"|"ready"))})
        }
    };
    data["project"] = serde_json::json!(project);
    Ok(crate::ProjectDataReceipt { data })
}

pub(crate) fn status_processing<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: crate::StatusProcessingCommand<'_>,
) -> Result<crate::StatusProcessingReceipt, ClientError> {
    crate::status_processing::run(profile, transport, authority, project, command)
}

/// One saved-selection operation. Membership, versioning and every capability
/// check stay ds-brain's; this validates the request grammar and bounds the
/// answer.
pub(crate) fn design_selections<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &crate::DesignSelectionRequest,
) -> Result<crate::DesignSelectionAnswer, ClientError> {
    validate_tiles(project)?;
    request.validate()?;
    let response = transport.design_selections(crate::DesignSelectionsCall {
        profile,
        ds_project: project,
        operation: request,
        authority: authority.call(),
    })?;
    crate::design_selections::decode(response, request)
}

/// One data-distribution action for the selected project: the catalogue, or
/// one bounded derived print-context acquisition. Membership is ds-brain's;
/// this validates the request grammar and bounds the answer.
pub(crate) fn data_distribution<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &crate::DataDistributionRequest,
) -> Result<serde_json::Value, ClientError> {
    data_distribution_over(
        profile,
        transport,
        authority,
        project,
        request,
        &crate::WeakNetwork::FIELD,
    )
}

/// [`data_distribution`] under an explicit retry schedule. A read that is
/// safe to send again (every seeding read: the catalogue, a print-context
/// acquisition, a bounded parcel or customer read) is retried when the link
/// blinks; a billed spatial execution is sent once.
pub(crate) fn data_distribution_over<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &crate::DataDistributionRequest,
    weak_network: &crate::WeakNetwork,
) -> Result<serde_json::Value, ClientError> {
    validate_tiles(project)?;
    request.validate()?;
    let weak_network = if request.resendable() {
        *weak_network
    } else {
        crate::WeakNetwork::new(&[])
    };
    let (outcome, retries) = weak_network.run(
        || {
            transport.data_distribution(crate::DataDistributionCall {
                profile,
                ds_project: project,
                operation: request,
                authority: authority.call(),
            })
        },
        |outcome| match outcome {
            Err(_) => true,
            Ok(response) => crate::weak_network::transient_status(response.status),
        },
    );
    let response = outcome.map_err(|error| {
        match error {
            TransportError::TimedOut => ClientError::new(
                ErrorKind::Transient,
                "Data distribution did not answer within the gateway deadline",
            ),
            TransportError::Unreachable => {
                ClientError::new(ErrorKind::Transient, "Data distribution is unreachable")
            }
        }
        .after_retries(retries)
    })?;
    crate::data_distribution::decode(response, request)
        .map_err(|error| error.after_retries(retries))
}

pub(crate) fn printing<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &crate::PrintingRequest,
) -> Result<serde_json::Value, ClientError> {
    if !project.is_empty() {
        validate_tiles(project)?;
    }
    request.validate()?;
    if project.is_empty() && request.needs_project(true) {
        return Err(ClientError::new(
            crate::ErrorKind::InvalidInput,
            "Selected project is required for printing",
        ));
    }
    let response = transport.printing(crate::PrintingCall {
        profile,
        ds_project: project,
        operation: request,
        authority: authority.call(),
    })?;
    crate::printing::decode(response, request)
}

#[cfg(test)]
mod project_report_refusal_tests {
    use super::*;

    fn refusal(status: u16, body: &str) -> ClientError {
        classify_project_report_response(&TransportResponse::new(status, body.as_bytes().to_vec()))
    }

    /// The cause a compounded run actually ends on. Before this it arrived as
    /// the same `InvalidInput` sentence as four unrelated statuses, which is
    /// what made a blocked reporting session unreadable headlessly.
    #[test]
    fn the_route_s_own_cause_survives_instead_of_the_status_alone() {
        let error = refusal(
            424,
            r#"{"status":"failed","code":"no_individual_transformer_artifacts","detail":"x"}"#,
        );
        assert_eq!(
            error.project_report_service_code(),
            Some(ProjectReportServiceCode::NoIndividualArtifacts)
        );
    }

    /// 409 carries two unrelated causes; the status cannot tell them apart.
    #[test]
    fn one_status_carrying_two_causes_still_separates_them() {
        assert_eq!(
            refusal(409, r#"{"status":"grouping_stale"}"#).project_report_service_code(),
            Some(ProjectReportServiceCode::GroupingStale)
        );
        assert_eq!(
            refusal(409, r#"{"status":"grouping_incomplete"}"#).project_report_service_code(),
            Some(ProjectReportServiceCode::GroupingIncomplete)
        );
    }

    /// An unknown, empty, oversized or malformed body must not invent a cause —
    /// it falls back to the status classification that was there before.
    #[test]
    fn an_unreadable_body_falls_back_rather_than_guessing() {
        for (status, body) in [
            (424, String::from("")),
            (424, String::from("not json")),
            (409, String::from(r#"{"status":"something_new"}"#)),
            (
                422,
                String::from(r#"{"code":"no_individual_transformer_artifacts"}"#),
            ),
            (424, format!(r#"{{"code":"{}"}}"#, "x".repeat(70_000))),
        ] {
            let error = refusal(status, &body);
            assert_eq!(error.project_report_service_code(), None, "status {status}");
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
        }
    }

    /// A combined run that packaged nothing names its rooms and each closed
    /// cause, so a forced run killed for memory reads as that, per room. The
    /// route's free-text detail and a malformed name never cross.
    #[test]
    fn the_rooms_a_combined_run_could_not_package_cross_as_bounded_pairs() {
        let error = refusal(
            424,
            r#"{"code":"no_individual_transformer_artifacts","detail":"SECRET-SERVER-TEXT",
               "missing_individual_artifact_count":3,
               "missing_individual_artifact_causes":[
                 {"transformer":"ruhuha","code":"reporter_container_terminated","detail":"SECRET"},
                 {"transformer":"zihare","code":"reporter_request_failed","detail":"x"},
                 {"transformer":"bad name!","code":"x","detail":"x"}]}"#,
        );
        assert_eq!(
            error.project_report_service_code(),
            Some(ProjectReportServiceCode::NoIndividualArtifacts)
        );
        let refusal = error.service_refusal().expect("the rooms are carried");
        assert_eq!(
            refusal.detail_integer("missing_individual_artifact_count"),
            Some(3)
        );
        assert_eq!(
            refusal.detail_text("missing_rooms"),
            Some("ruhuha:reporter_container_terminated, zihare:reporter_request_failed")
        );
        assert_eq!(refusal.detail_integer("missing_rooms_shown"), Some(2));
        assert_eq!(refusal.message(), None);
        assert!(!format!("{refusal:?}").contains("SECRET"));
    }

    /// The body is read for its code only; no server string reaches the caller.
    #[test]
    fn no_server_text_crosses_the_boundary() {
        let error = refusal(
            424,
            r#"{"code":"no_individual_transformer_artifacts","detail":"SECRET-SERVER-TEXT"}"#,
        );
        assert!(!error.to_string().contains("SECRET-SERVER-TEXT"));
    }
}
