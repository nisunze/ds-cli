//! Shared deterministic command/state semantics for DS native and web clients.
//!
//! This crate owns no transport, credential store, browser API, renderer, or
//! shell behavior. Its first bounded slice validates a Firebase login response
//! identically for the native `ds` client and the browser WASM adapter.

use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroize;

pub mod assets;
pub mod solar_dashboard;
pub mod solar_report_bundle;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarPromptBundlePlan)]
pub fn solar_prompt_bundle_plan_wasm(
    canonical: &str,
    markdown: &str,
) -> Result<String, wasm_bindgen::JsValue> {
    let plan = solar_report_bundle::plan(canonical, markdown)
        .map_err(|e| wasm_bindgen::JsValue::from_str(&e))?;
    serde_json::to_string(&plan).map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = buildSolarPromptBundle)]
pub fn build_solar_prompt_bundle_wasm(input: &[u8]) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    solar_report_bundle::build_encoded(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarReportBundleMediaName)]
pub fn solar_report_bundle_media_name_wasm(
    reference: &str,
    index: usize,
) -> Result<String, wasm_bindgen::JsValue> {
    solar_report_bundle::media_name(reference, index)
        .map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod solar_portfolio_graphs;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarPortfolioGraphAddress)]
pub fn solar_portfolio_graph_address_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    solar_portfolio_graphs::address(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod solar_portfolio_read;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarPortfolioGraphPlan)]
pub fn solar_portfolio_graph_plan_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    solar_portfolio_graphs::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = verifySolarPortfolioGraphBytes)]
pub fn verify_solar_portfolio_graph_bytes_wasm(
    bytes: &[u8],
    context: &[u8],
) -> Result<(), wasm_bindgen::JsValue> {
    solar_portfolio_graphs::verify_bytes(bytes, context)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod solar_saved_analysis;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = bindSolarHostedPortfolioData)]
pub fn bind_solar_hosted_portfolio_data_wasm(
    input: &[u8],
) -> Result<String, wasm_bindgen::JsValue> {
    solar_saved_analysis::bind_hosted(input)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarSavedAnalysis)]
pub fn solar_saved_analysis_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    solar_saved_analysis::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod solar_snapshot;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarDashboard)]
pub fn solar_dashboard_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    solar_dashboard::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = readBoundSolarPortfolioResult)]
pub fn read_bound_solar_portfolio_result_wasm(
    result: &[u8],
    context: &[u8],
) -> Result<String, wasm_bindgen::JsValue> {
    solar_portfolio_read::evaluate(result, context)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarSnapshot)]
pub fn solar_snapshot_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    solar_snapshot::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = assetsCommand)]
pub fn assets_command_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    assets::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = assetsPreview)]
pub fn assets_preview_wasm(bytes: &[u8], request: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    assets::evaluate_with_bytes(bytes, request)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod compute_artifact_sync;
pub mod compute_jobs;
pub mod execution_context;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = executionContext)]
pub fn execution_context_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    execution_context::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod desktop_instance;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = desktopInstance)]
pub fn desktop_instance_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    desktop_instance::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod connectivity;
pub mod design;
pub mod design_dashboard;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designDashboard)]
pub fn design_dashboard_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_dashboard::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod design_health;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designHealth)]
pub fn design_health_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_health::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod design_status_query;
pub mod design_status_row;
pub mod status_upload;
pub mod sync;
pub mod sync_store;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = syncStore)]
pub fn sync_store_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    sync_store::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = syncPlan)]
pub fn sync_plan_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    sync::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designStatusQuery)]
pub fn design_status_query_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_status_query::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod design_conflict;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designConflict)]
pub fn design_conflict_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_conflict::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod design_presence;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designPresence)]
pub fn design_presence_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_presence::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod design_version;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designVersion)]
pub fn design_version_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_version::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod process_settings;
pub mod profile_structure_type;
pub mod profile_supports;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = processSettings)]
pub fn process_settings_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    process_settings::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = profileStructureTypeSelection)]
pub fn profile_structure_type_selection_wasm(
    input: &[u8],
) -> Result<String, wasm_bindgen::JsValue> {
    profile_structure_type::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = profileSupports)]
pub fn profile_supports_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    profile_supports::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designStatusRow)]
pub fn design_status_row_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_status_row::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designStatusUpload)]
pub fn design_status_upload_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    status_upload::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod field_domain;
pub mod installation;
pub mod installation_inventory;
/// The whole installation inventory, as an operator must read it: grouped by
/// host, ordered by freshness, with licence and lease state said in tokens a
/// caller translates. Reached identically by `ds`, the Server and the browser.
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = installationInventory)]
pub fn installation_inventory_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    installation_inventory::evaluate_request(input)
        .and_then(|value| serde_json::to_string(&value).map_err(|error| error.to_string()))
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod map_active_view;
pub mod map_scene;
pub mod project_context;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = mapSceneCommand)]
pub fn map_scene_command_wasm(
    input: &[u8],
    artifact: bool,
) -> Result<String, wasm_bindgen::JsValue> {
    let result = if artifact {
        map_scene::validate_artifact(input)
    } else {
        map_scene::build(input)
    };
    result
        .and_then(|value| serde_json::to_string(&value).map_err(|_| "scene encoding failed".into()))
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = mapSceneRenderPlan)]
pub fn map_scene_render_plan_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    map_scene::render_plan(input)
        .and_then(|value| {
            serde_json::to_string(&value).map_err(|_| "scene plan encoding failed".into())
        })
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = mapActiveView)]
pub fn map_active_view_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    map_active_view::evaluate(input)
        .and_then(|value| {
            serde_json::to_string(&value).map_err(|_| "active map plan encoding failed".into())
        })
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = connectivityCommand)]
pub fn connectivity_command_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    connectivity::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = computeArtifactRetryAssessment)]
pub fn compute_artifact_retry_assessment_wasm(
    input: &[u8],
) -> Result<String, wasm_bindgen::JsValue> {
    compute_artifact_sync::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod compute_artifact_sanitation;
pub mod reconciliation;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = computeArtifactSanitation)]
pub fn compute_artifact_sanitation_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    compute_artifact_sanitation::evaluate(input)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = reconciliationCommand)]
pub fn reconciliation_command_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    reconciliation::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod city_vectors;
mod project_control;
pub mod project_dataset_cache;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = projectDatasetCacheCommand)]
pub fn project_dataset_cache_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    project_dataset_cache::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod project_design_extent;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = projectDesignExtentCommand)]
pub fn project_design_extent_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    project_design_extent::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod reference_cache;
pub mod report;
pub mod report_export;
pub mod solar_batch;
pub mod solar_network_form;
mod solar_network_seed;
mod solar_portfolio;

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarPortfolioBatch)]
pub fn solar_portfolio_batch_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    solar_batch::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
pub mod survey;
pub mod survey_control;
pub mod survey_entries_changes;
pub mod survey_entries_select;
pub mod survey_entry_create;
pub mod survey_entry_outbox;
pub mod survey_query;

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyEntryCreate)]
pub fn survey_entry_create_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    survey_entry_create::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyEntryGeometryValidate)]
pub fn survey_entry_geometry_validate_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    survey_entry_create::evaluate_geometry(input)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyEntryOutbox)]
pub fn survey_entry_outbox_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    survey_entry_outbox::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = resolveDesignSources)]
pub fn resolve_design_sources_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design::resolve_sources(input)
        .and_then(|v| serde_json::to_string(&v).map_err(|e| e.to_string()))
        .map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designEdit)]
pub fn design_edit_wasm(snapshot: &[u8], request: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design::edit_json(snapshot, request).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyEvaluate)]
pub fn survey_evaluate_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    survey::survey_evaluate(input)
        .and_then(|v| serde_json::to_string(&v).map_err(|_| "Survey result encoding failed".into()))
        .map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub use project_control::{
    plan_project_control_artifact_download, plan_project_control_bulk_action,
    plan_project_control_report_download, plan_project_control_rooms,
    project_project_control_local_reports, ProjectControlError, RoomAvailability,
    RoomMaterializationPlan, MAX_PROJECT_CONTROL_INPUT_BYTES, MAX_PROJECT_CONTROL_OUTBOX_ROWS,
    MAX_PROJECT_CONTROL_ROOMS, PROJECT_CONTROL_ARTIFACT_DOWNLOAD_SCHEMA,
    PROJECT_CONTROL_BULK_PLAN_SCHEMA, PROJECT_CONTROL_REPORT_CATALOG_SCHEMA,
    PROJECT_CONTROL_REPORT_PLAN_SCHEMA, PROJECT_CONTROL_ROOM_PLAN_SCHEMA,
};

pub use solar_network_seed::{
    plan_solar_network_seed, solar_network_seed_request_schema, SolarNetworkSeedError,
    MAX_SOLAR_NETWORK_SEED_BYTES, MV_ALIGNMENT_FACTS_SCHEMA, SOLAR_NETWORK_SEED_PLAN_SCHEMA,
    SOLAR_NETWORK_SEED_REQUEST_SCHEMA,
};

pub use solar_portfolio::{
    plan_solar_portfolio_mutation, solar_portfolio_membership_identity,
    validate_solar_portfolio_catalog, validate_solar_portfolio_mutation_receipt, SolarPortfolio,
    SolarPortfolioError, MAX_SOLAR_PORTFOLIOS, MAX_SOLAR_PORTFOLIO_CATALOG_BYTES,
    MAX_SOLAR_PORTFOLIO_CITIES, MAX_SOLAR_PORTFOLIO_MUTATION_BYTES, SOLAR_PORTFOLIO_CATALOG_SCHEMA,
    SOLAR_PORTFOLIO_MEMBERSHIP_SCHEMA, SOLAR_PORTFOLIO_MUTATION_PLAN_SCHEMA,
};

pub const LOGIN_CONTRACT_SCHEMA: &str = "ds.auth.login-result/v1";
pub const MAX_LOGIN_RESPONSE_BYTES: usize = 64 * 1024;
pub const MAX_SOLAR_PORTFOLIO_RESULT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioReadError(String);

impl fmt::Display for PortfolioReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PortfolioReadError {}

/// Validate and project one sealed Solar portfolio result through the Solar
/// domain contract used by both native `ds` and browser WASM hosts.
pub fn validate_solar_portfolio_result(
    result_json: &[u8],
    expected_run_id: &str,
) -> Result<String, PortfolioReadError> {
    if result_json.is_empty() || result_json.len() > MAX_SOLAR_PORTFOLIO_RESULT_BYTES {
        return Err(PortfolioReadError(
            "the Solar portfolio result is outside the 4 MiB shared read bound".to_string(),
        ));
    }
    let document = serde_json::from_slice(result_json).map_err(|_| {
        PortfolioReadError("the Solar portfolio result is not valid JSON".to_string())
    })?;
    let validated = ds_solar_contracts::validate_portfolio_result_read(document, expected_run_id)
        .map_err(|error| PortfolioReadError(error.to_string()))?;
    serde_json::to_string(&validated).map_err(|_| {
        PortfolioReadError("the validated Solar portfolio result could not be encoded".to_string())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginErrorKind {
    InvalidInput,
    UnreadableResponse,
    IdentityMismatch,
}

/// Bounded, credential-free classification of a failed Firebase password
/// exchange. Hosts use this only to select stable guidance; response text and
/// credential material never cross the kernel boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirebaseAuthFailureKind {
    InvalidCredentials,
    PasswordSignInUnavailable,
    AccountDisabled,
    Transient,
    Rejected,
    UnreadableResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginError {
    kind: LoginErrorKind,
    message: &'static str,
}

impl LoginError {
    const fn new(kind: LoginErrorKind, message: &'static str) -> Self {
        Self { kind, message }
    }

    pub const fn kind(&self) -> LoginErrorKind {
        self.kind
    }
}

impl fmt::Display for LoginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for LoginError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginPrincipal {
    uid: String,
    email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    photo_url: Option<String>,
}

impl LoginPrincipal {
    pub fn uid(&self) -> &str {
        &self.uid
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    pub fn photo_url(&self) -> Option<&str> {
        self.photo_url.as_deref()
    }
}

/// A validated login response. Token fields remain private and are zeroized on
/// drop. Hosts may borrow or take them only to install their protected session.
#[derive(Debug)]
pub struct ValidatedLogin {
    principal: LoginPrincipal,
    id_token: String,
    refresh_token: String,
    expires_at_epoch_seconds: u64,
}

impl ValidatedLogin {
    pub fn principal(&self) -> &LoginPrincipal {
        &self.principal
    }

    pub fn id_token(&self) -> &str {
        &self.id_token
    }

    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    pub const fn expires_at_epoch_seconds(&self) -> u64 {
        self.expires_at_epoch_seconds
    }

    pub fn take_id_token(&mut self) -> String {
        std::mem::take(&mut self.id_token)
    }

    pub fn take_refresh_token(&mut self) -> String {
        std::mem::take(&mut self.refresh_token)
    }

    /// Bounded browser handoff. It is deliberately not a command receipt and
    /// must never be traced or logged because it contains credential material.
    pub fn to_browser_json(&self) -> Result<String, LoginError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct BrowserLogin<'a> {
            schema: &'static str,
            user: &'a LoginPrincipal,
            id_token: &'a str,
            refresh_token: &'a str,
            expires_at_epoch_seconds: u64,
        }

        serde_json::to_string(&BrowserLogin {
            schema: LOGIN_CONTRACT_SCHEMA,
            user: &self.principal,
            id_token: &self.id_token,
            refresh_token: &self.refresh_token,
            expires_at_epoch_seconds: self.expires_at_epoch_seconds,
        })
        .map_err(|_| unreadable_response())
    }
}

impl Drop for ValidatedLogin {
    fn drop(&mut self) {
        self.id_token.zeroize();
        self.refresh_token.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirebaseLoginWire {
    local_id: String,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    uid: Option<String>,
    email: String,
    id_token: String,
    refresh_token: String,
    expires_in: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    photo_url: String,
    #[serde(default)]
    provider_user_info: Vec<ProviderUserInfo>,
    #[serde(default)]
    raw_user_info: String,
}

impl Drop for FirebaseLoginWire {
    fn drop(&mut self) {
        self.id_token.zeroize();
        self.refresh_token.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUserInfo {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    photo_url: String,
}

pub fn validate_firebase_login_response(
    response: &[u8],
    expected_email: Option<&str>,
    firebase_project_id: &str,
    now_epoch_seconds: u64,
) -> Result<ValidatedLogin, LoginError> {
    validate_audience(firebase_project_id)?;
    if response.len() > MAX_LOGIN_RESPONSE_BYTES {
        return Err(unreadable_response());
    }
    let mut wire: FirebaseLoginWire =
        serde_json::from_slice(response).map_err(|_| unreadable_response())?;
    if [wire.user_id.as_deref(), wire.uid.as_deref()]
        .into_iter()
        .flatten()
        .any(|alias| alias != wire.local_id)
    {
        return Err(identity_mismatch());
    }
    let principal = validate_identity(
        &wire.local_id,
        &wire.email,
        expected_email.unwrap_or(&wire.email),
        first_nonempty([
            wire.display_name.as_str(),
            wire.provider_user_info
                .iter()
                .find_map(|provider| clean_optional(&provider.display_name))
                .unwrap_or_default(),
            raw_user_info_field(&wire.raw_user_info, &["name", "displayName"])
                .as_deref()
                .unwrap_or_default(),
        ]),
        first_nonempty([
            wire.photo_url.as_str(),
            wire.provider_user_info
                .iter()
                .find_map(|provider| clean_optional(&provider.photo_url))
                .unwrap_or_default(),
            raw_user_info_field(&wire.raw_user_info, &["picture", "photoUrl", "photoURL"])
                .as_deref()
                .unwrap_or_default(),
        ]),
    )?;
    let claim_expiry = validate_firebase_id_token(
        &wire.id_token,
        principal.uid(),
        principal.email(),
        firebase_project_id,
        now_epoch_seconds,
    )?;
    validate_secret(&wire.refresh_token)?;
    let expires_in = parse_expiry(&wire.expires_in)?;
    Ok(ValidatedLogin {
        principal,
        id_token: std::mem::take(&mut wire.id_token),
        refresh_token: std::mem::take(&mut wire.refresh_token),
        expires_at_epoch_seconds: claim_expiry.min(now_epoch_seconds.saturating_add(expires_in)),
    })
}

pub fn classify_firebase_auth_failure(status: u16, response: &[u8]) -> FirebaseAuthFailureKind {
    if response.len() > MAX_LOGIN_RESPONSE_BYTES {
        return FirebaseAuthFailureKind::UnreadableResponse;
    }
    if matches!(status, 408 | 425 | 429 | 500..=599) {
        return FirebaseAuthFailureKind::Transient;
    }
    let Some(code) = serde_json::from_slice::<Value>(response)
        .ok()
        .and_then(|body| body["error"]["message"].as_str().map(str::to_owned))
        .and_then(|message| {
            message
                .split([':', ' '])
                .next()
                .filter(|code| !code.is_empty())
                .map(str::to_owned)
        })
    else {
        return FirebaseAuthFailureKind::UnreadableResponse;
    };
    match code.as_str() {
        "INVALID_LOGIN_CREDENTIALS"
        | "EMAIL_NOT_FOUND"
        | "INVALID_PASSWORD"
        | "MISSING_PASSWORD" => FirebaseAuthFailureKind::InvalidCredentials,
        "OPERATION_NOT_ALLOWED" => FirebaseAuthFailureKind::PasswordSignInUnavailable,
        "USER_DISABLED" => FirebaseAuthFailureKind::AccountDisabled,
        "TOO_MANY_ATTEMPTS_TRY_LATER" => FirebaseAuthFailureKind::Transient,
        _ => FirebaseAuthFailureKind::Rejected,
    }
}

/// Validate the identity-binding claims on an ID token obtained directly from
/// Firebase's fixed authentication endpoint. The caller remains responsible
/// for cryptographic verification at the governing API boundary.
pub fn validate_firebase_id_token(
    token: &str,
    uid: &str,
    email: &str,
    firebase_project_id: &str,
    now_epoch_seconds: u64,
) -> Result<u64, LoginError> {
    validate_audience(firebase_project_id)?;
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(identity_mismatch());
    };
    if header.is_empty() || payload.is_empty() || signature.is_empty() {
        return Err(identity_mismatch());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| identity_mismatch())?;
    let claims: Value = serde_json::from_slice(&bytes).map_err(|_| identity_mismatch())?;
    let subject = exact_claim(&claims, "sub")?;
    let claim_email = exact_claim(&claims, "email")?;
    let claim_audience = exact_claim(&claims, "aud")?;
    let issuer = exact_claim(&claims, "iss")?;
    let exp = claims
        .get("exp")
        .and_then(Value::as_u64)
        .filter(|exp| *exp > now_epoch_seconds)
        .ok_or_else(identity_mismatch)?;
    let expected_issuer = format!("https://securetoken.google.com/{firebase_project_id}");
    if subject != uid
        || claim_email != email
        || claim_audience != firebase_project_id
        || issuer != expected_issuer
    {
        return Err(identity_mismatch());
    }
    Ok(exp)
}

fn validate_identity(
    uid: &str,
    response_email: &str,
    expected_email: &str,
    display_name: Option<String>,
    photo_url: Option<String>,
) -> Result<LoginPrincipal, LoginError> {
    if uid.is_empty()
        || uid.trim() != uid
        || uid.chars().count() > 128
        || uid.chars().any(char::is_control)
        || response_email.trim() != response_email
        || !response_email.eq_ignore_ascii_case(expected_email)
        || !valid_email(response_email)
    {
        return Err(identity_mismatch());
    }
    Ok(LoginPrincipal {
        uid: uid.to_owned(),
        email: response_email.to_owned(),
        display_name,
        photo_url,
    })
}

fn valid_email(email: &str) -> bool {
    !email.is_empty()
        && email.trim() == email
        && email.chars().count() <= 320
        && !email.chars().any(char::is_control)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        })
}

fn validate_audience(audience: &str) -> Result<(), LoginError> {
    if audience.is_empty()
        || audience.trim() != audience
        || audience.len() > 128
        || audience.chars().any(char::is_control)
    {
        return Err(LoginError::new(
            LoginErrorKind::InvalidInput,
            "Firebase project identity is outside the closed login contract",
        ));
    }
    Ok(())
}

fn validate_secret(secret: &str) -> Result<(), LoginError> {
    if secret.is_empty() || secret.len() > 16 * 1024 || secret.contains(['\r', '\n']) {
        return Err(unreadable_response());
    }
    Ok(())
}

fn parse_expiry(value: &str) -> Result<u64, LoginError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| (60..=86_400).contains(seconds))
        .ok_or_else(unreadable_response)
}

fn exact_claim<'a>(claims: &'a Value, key: &str) -> Result<&'a str, LoginError> {
    claims
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.trim() == *value)
        .ok_or_else(identity_mismatch)
}

fn first_nonempty<const N: usize>(values: [&str; N]) -> Option<String> {
    values
        .into_iter()
        .find_map(clean_optional)
        .map(str::to_owned)
}

fn clean_optional(value: &str) -> Option<&str> {
    (!value.is_empty() && value.trim() == value && !value.chars().any(char::is_control))
        .then_some(value)
}

fn raw_user_info_field(raw: &str, keys: &[&str]) -> Option<String> {
    if raw.is_empty() || raw.len() > 16 * 1024 {
        return None;
    }
    let value: Value = serde_json::from_str(raw).ok()?;
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .and_then(clean_optional)
            .map(str::to_owned)
    })
}

fn unreadable_response() -> LoginError {
    LoginError::new(
        LoginErrorKind::UnreadableResponse,
        "the Firebase response is outside the closed login contract",
    )
}

fn identity_mismatch() -> LoginError {
    LoginError::new(
        LoginErrorKind::IdentityMismatch,
        "Firebase UID, email, JWT subject, audience, issuer, or expiry did not remain bound",
    )
}

#[cfg(feature = "wasm")]
mod wasm {
    use wasm_bindgen::prelude::*;

    use super::{
        plan_project_control_artifact_download, plan_project_control_bulk_action,
        plan_project_control_report_download, plan_project_control_rooms, plan_solar_network_seed,
        plan_solar_portfolio_mutation, project_project_control_local_reports,
        solar_portfolio_membership_identity, validate_firebase_id_token,
        validate_firebase_login_response, validate_solar_portfolio_catalog,
        validate_solar_portfolio_mutation_receipt, validate_solar_portfolio_result,
    };

    #[wasm_bindgen(js_name = solarReportImages)]
    pub fn solar_report_images(markdown: &str) -> Result<String, JsValue> {
        let images = ds_solar_contracts::report_media::images(markdown)
            .map_err(|error| JsValue::from_str(&error.message))?;
        serde_json::to_string(&images).map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = projectSolarReportImages)]
    pub fn project_solar_report_images(
        markdown: &str,
        resolved_json: &str,
    ) -> Result<String, JsValue> {
        if resolved_json.len() > 8 * 1024 * 1024 {
            return Err(JsValue::from_str(
                "Solar media projection exceeds its byte bound",
            ));
        }
        let resolved: std::collections::BTreeMap<String, String> =
            serde_json::from_str(resolved_json)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
        if resolved.len() > 512 {
            return Err(JsValue::from_str(
                "Solar media projection exceeds 512 images",
            ));
        }
        ds_solar_contracts::report_media::project(markdown, &resolved)
            .map_err(|error| JsValue::from_str(&error.message))
    }

    #[wasm_bindgen(js_name = planProjectControlRooms)]
    pub fn project_control_rooms(input_json: &[u8]) -> Result<String, JsValue> {
        plan_project_control_rooms(input_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = planProjectControlReportDownload)]
    pub fn project_control_report_download(input_json: &[u8]) -> Result<String, JsValue> {
        plan_project_control_report_download(input_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = planProjectControlBulkAction)]
    pub fn project_control_bulk_action(input_json: &[u8]) -> Result<String, JsValue> {
        plan_project_control_bulk_action(input_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = planProjectControlArtifactDownload)]
    pub fn project_control_artifact_download(input_json: &[u8]) -> Result<String, JsValue> {
        plan_project_control_artifact_download(input_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = projectProjectControlLocalReports)]
    pub fn project_control_local_reports(input_json: &[u8]) -> Result<String, JsValue> {
        project_project_control_local_reports(input_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = validateFirebaseLogin)]
    pub fn validate_firebase_login(
        response_json: &[u8],
        expected_email: Option<String>,
        firebase_project_id: &str,
        now_epoch_seconds: u64,
    ) -> Result<String, JsValue> {
        validate_firebase_login_response(
            response_json,
            expected_email.as_deref(),
            firebase_project_id,
            now_epoch_seconds,
        )
        .and_then(|login| login.to_browser_json())
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = validateFirebaseIdToken)]
    pub fn validate_firebase_token(
        token: &str,
        uid: &str,
        email: &str,
        firebase_project_id: &str,
        now_epoch_seconds: u64,
    ) -> Result<u64, JsValue> {
        validate_firebase_id_token(token, uid, email, firebase_project_id, now_epoch_seconds)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = validateSolarPortfolioResult)]
    pub fn validate_solar_portfolio(
        result_json: &[u8],
        expected_run_id: &str,
    ) -> Result<String, JsValue> {
        validate_solar_portfolio_result(result_json, expected_run_id)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = validateSolarPortfolioCatalog)]
    pub fn validate_solar_portfolios(catalog_json: &[u8]) -> Result<String, JsValue> {
        validate_solar_portfolio_catalog(catalog_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = planSolarPortfolioMutation)]
    pub fn plan_solar_portfolio(input_json: &[u8]) -> Result<String, JsValue> {
        plan_solar_portfolio_mutation(input_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = solarPortfolioMembershipIdentity)]
    pub fn solar_portfolio_membership(input_json: &[u8]) -> Result<String, JsValue> {
        solar_portfolio_membership_identity(input_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = validateSolarPortfolioMutationReceipt)]
    pub fn solar_portfolio_mutation_receipt(input_json: &[u8]) -> Result<String, JsValue> {
        validate_solar_portfolio_mutation_receipt(input_json)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = planSolarNetworkSeed)]
    pub fn solar_network_seed(input_json: &[u8]) -> Result<String, JsValue> {
        plan_solar_network_seed(input_json).map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = solarNetworkForm)]
    pub fn solar_network_form(input_json: &[u8]) -> Result<String, JsValue> {
        crate::solar_network_form::evaluate(input_json).map_err(|error| JsValue::from_str(&error))
    }
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = styleChannels)]
pub fn wasm_style_channels(kind: &str) -> String {
    serde_json::to_string(&style_authoring::style_channels(kind))
        .expect("static channel definitions")
}

pub mod overlays;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name=editOverlays)]
pub fn wasm_edit_overlays(input: &[u8]) -> Result<String, String> {
    if input.len() > 1024 * 1024 {
        return Err("overlay request exceeds 1 MiB".into());
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        registry: overlays::OverlayRegistry,
        edit: overlays::OverlayEdit,
    }
    let mut r: Request = serde_json::from_slice(input).map_err(|_| "invalid overlay request")?;
    let result = r.registry.apply(r.edit)?;
    serde_json::to_string(&serde_json::json!({"registry":r.registry,"result":result}))
        .map_err(|_| "invalid overlay result".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_700_000_000;
    const PROJECT: &str = "ds-testing";
    const UID: &str = "Case-Sensitive UID/01";
    const EMAIL: &str = "operator@example.com";

    fn token(uid: &str, email: &str, audience: &str, issuer: &str, exp: u64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "sub": uid,
                "email": email,
                "aud": audience,
                "iss": issuer,
                "exp": exp,
            }))
            .unwrap(),
        );
        format!("{header}.{claims}.signature")
    }

    fn response(overrides: Value) -> Vec<u8> {
        let mut value = serde_json::json!({
            "localId": UID,
            "email": EMAIL,
            "idToken": token(
                UID,
                EMAIL,
                PROJECT,
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW + 3_600,
            ),
            "refreshToken": "protected-refresh",
            "expiresIn": "3600",
            "providerUserInfo": [{
                "displayName": "DS Operator",
                "photoUrl": "https://images.example/operator.png"
            }]
        });
        if let (Some(target), Some(source)) = (value.as_object_mut(), overrides.as_object()) {
            target.extend(source.clone());
        }
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn validates_one_bounded_login_for_native_and_browser_projection() {
        let login =
            validate_firebase_login_response(&response(Value::Null), Some(EMAIL), PROJECT, NOW)
                .unwrap();
        assert_eq!(login.principal().uid(), UID);
        assert_eq!(login.principal().display_name(), Some("DS Operator"));
        assert_eq!(login.expires_at_epoch_seconds(), NOW + 3_600);
        let browser: Value = serde_json::from_str(&login.to_browser_json().unwrap()).unwrap();
        assert_eq!(browser["schema"], LOGIN_CONTRACT_SCHEMA);
        assert_eq!(browser["user"]["uid"], UID);
        assert_eq!(browser["refreshToken"], "protected-refresh");
    }

    #[test]
    fn fails_closed_on_identity_audience_issuer_and_expiry_mismatch() {
        let cases = [
            token(
                "another-uid",
                EMAIL,
                PROJECT,
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW + 3_600,
            ),
            token(
                UID,
                "other@example.com",
                PROJECT,
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW + 3_600,
            ),
            token(
                UID,
                EMAIL,
                "other-project",
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW + 3_600,
            ),
            token(
                UID,
                EMAIL,
                PROJECT,
                "https://attacker.example/ds-testing",
                NOW + 3_600,
            ),
            token(
                UID,
                EMAIL,
                PROJECT,
                &format!("https://securetoken.google.com/{PROJECT}"),
                NOW,
            ),
        ];
        for id_token in cases {
            let error = validate_firebase_login_response(
                &response(serde_json::json!({ "idToken": id_token })),
                Some(EMAIL),
                PROJECT,
                NOW,
            )
            .unwrap_err();
            assert_eq!(error.kind(), LoginErrorKind::IdentityMismatch);
        }
    }

    #[test]
    fn rejects_unbounded_or_malformed_login_envelopes() {
        assert_eq!(
            validate_firebase_login_response(
                &vec![b'x'; MAX_LOGIN_RESPONSE_BYTES + 1],
                Some(EMAIL),
                PROJECT,
                NOW,
            )
            .unwrap_err()
            .kind(),
            LoginErrorKind::UnreadableResponse,
        );
        assert_eq!(
            validate_firebase_login_response(b"{}", Some(EMAIL), PROJECT, NOW)
                .unwrap_err()
                .kind(),
            LoginErrorKind::UnreadableResponse,
        );
        assert_eq!(
            validate_firebase_login_response(
                &response(serde_json::json!({ "userId": "another-uid" })),
                Some(EMAIL),
                PROJECT,
                NOW,
            )
            .unwrap_err()
            .kind(),
            LoginErrorKind::IdentityMismatch,
        );
    }

    #[test]
    fn classifies_failed_password_exchange_without_leaking_response_text() {
        let response = |message: &str| {
            serde_json::to_vec(&serde_json::json!({ "error": { "message": message } })).unwrap()
        };
        assert_eq!(
            classify_firebase_auth_failure(400, &response("INVALID_LOGIN_CREDENTIALS")),
            FirebaseAuthFailureKind::InvalidCredentials,
        );
        assert_eq!(
            classify_firebase_auth_failure(400, &response("OPERATION_NOT_ALLOWED")),
            FirebaseAuthFailureKind::PasswordSignInUnavailable,
        );
        assert_eq!(
            classify_firebase_auth_failure(400, &response("USER_DISABLED")),
            FirebaseAuthFailureKind::AccountDisabled,
        );
        assert_eq!(
            classify_firebase_auth_failure(429, b"not exposed"),
            FirebaseAuthFailureKind::Transient,
        );
        assert_eq!(
            classify_firebase_auth_failure(400, b"not-json"),
            FirebaseAuthFailureKind::UnreadableResponse,
        );
    }

    #[test]
    fn native_portfolio_read_uses_the_solar_contract_projection() {
        let document = portfolio_v2_fixture();
        let result = validate_solar_portfolio_result(&document, "run-1").unwrap();
        let result: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["trace"]["city_ids"], serde_json::json!(["a", "b"]));
        assert_eq!(result["trace"]["representative_city"], "a");
    }

    fn portfolio_v2_fixture() -> Vec<u8> {
        let mut sections = ds_solar_contracts::portfolio::PORTFOLIO_RESULT_SECTIONS
            .iter()
            .map(|name| ((*name).to_string(), serde_json::json!({ "value": 1 })))
            .collect::<serde_json::Map<_, _>>();
        sections.insert(
            "representative_graphs".to_string(),
            serde_json::json!({ "_strategy": "first" }),
        );
        let city = |city_id: &str, selected_system: &str, customers: i64, energy: f64| {
            serde_json::json!({
                "city_id": city_id,
                "display_name": city_id.to_ascii_uppercase(),
                "city_run_id": format!("run-{city_id}"),
                "input_digest": format!("sha256:{}", "d".repeat(64)),
                "result_digest": format!("sha256:{}", "e".repeat(64)),
                "contribution_digest": format!("sha256:{}", "f".repeat(64)),
                "calculation_projection_digest": format!("sha256:{}", "1".repeat(64)),
                "selected_system": selected_system,
                "inverter_count": 1,
                "pv_capacity_mw": 1.0,
                "battery_capacity_mwh": 1.0,
                "generator_count": 1.0,
                "diesel_capacity_mw": 1.0,
                "total_customers": customers,
                "annual_energy_kwh": energy,
            })
        };
        serde_json::to_vec(&serde_json::json!({
            "schema_version": ds_solar_contracts::PORTFOLIO_SCHEMA_VERSION_V2,
            "engine": { "name": "ds-solar", "version": "0.1.0" },
            "project_id": "project",
            "root": "root",
            "portfolio_id": "portfolio",
            "portfolio_name": "Portfolio",
            "membership_revision": format!("sha256:{}", "a".repeat(64)),
            "run_id": "run-1",
            "input_digest": format!("sha256:{}", "b".repeat(64)),
            "assumptions": { "currency": "XAF", "project_years": 25, "discount_rate": 0.1 },
            "representative_city_id": "a",
            "cities": [city("a", "hybrid", 10, 100.0), city("b", "solar_battery", 20, 200.0)],
            "city_count": 2,
            "total_customers": 30,
            "annual_energy_kwh": 300.0,
            "systems": {
                "hybrid": {
                    "cities": 1, "inverter_count": 1, "pv_capacity_mw": 1.0,
                    "battery_capacity_mwh": 1.0, "generator_count": 1.0,
                    "diesel_capacity_mw": 1.0
                },
                "solar_battery": {
                    "cities": 1, "inverter_count": 1, "pv_capacity_mw": 1.0,
                    "battery_capacity_mwh": 1.0, "generator_count": 1.0,
                    "diesel_capacity_mw": 1.0
                }
            },
            "sections": sections,
            "portfolio_digest": format!("sha256:{}", "c".repeat(64)),
        }))
        .unwrap()
    }
}

#[cfg(all(test, target_arch = "wasm32", feature = "wasm"))]
mod wasm_tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::Value;
    use wasm_bindgen_test::*;

    use super::wasm::{
        project_control_rooms, solar_portfolio_membership, validate_firebase_login,
        validate_solar_portfolio,
    };

    #[wasm_bindgen_test]
    fn assets_native_and_wasm_share_results_and_refusals() {
        let recognise = serde_json::to_vec(&serde_json::json!({
            "schema": "ds.assets.request/v1", "action": "recognise",
            "name": "notes.txt", "bytes_len": 5, "head_b64": "aGVsbG8="
        }))
        .unwrap();
        let native = crate::assets::evaluate(&recognise).unwrap();
        let browser = super::assets_command_wasm(&recognise).unwrap();
        assert_eq!(browser, native);
        let preview = serde_json::to_vec(&serde_json::json!({
            "schema": "ds.assets.request/v1", "action": "preview", "format": "pdf"
        }))
        .unwrap();
        let native = crate::assets::evaluate_with_bytes(b"%PDF-1.7", &preview).unwrap_err();
        let browser = super::assets_preview_wasm(b"%PDF-1.7", &preview).unwrap_err();
        assert_eq!(browser.as_string().unwrap(), native);
    }

    #[wasm_bindgen_test]
    fn assets_preview_native_and_wasm_share_a_csv_grid() {
        let csv = b"id,name,kv\nTX-104,Kabeza,11\nTX-105,Gikondo,33\n";
        let preview = serde_json::to_vec(&serde_json::json!({
            "schema": "ds.assets.request/v1", "action": "preview", "format": "csv", "rows": 1
        }))
        .unwrap();
        let native = crate::assets::evaluate_with_bytes(csv, &preview).unwrap();
        let browser = super::assets_preview_wasm(csv, &preview).unwrap();
        assert_eq!(browser, native);
        let doc: Value = serde_json::from_str(&browser).unwrap();
        assert_eq!(doc["schema"], "ds.assets.preview_doc/v1");
        assert_eq!(doc["blocks"][0]["type"], "grid");
        assert_eq!(doc["blocks"][0]["rows_total"], 2);
        assert_eq!(doc["blocks"][0]["rows_shown"], 1);
    }

    #[wasm_bindgen_test]
    fn survey_native_and_wasm_share_validation_factory_and_conflict_policy() {
        for request in [
            serde_json::json!({"operation":"fields","fields":[{"key":"n","required":true,"type":"number","validation":{"min":1}}],"values":{"n":0}}),
            serde_json::json!({"operation":"new_field","kind":"photos","id":"id1","metadata":{"type":"photos","label":"Photos"}}),
            serde_json::json!({"operation":"policy","mode":"accuracy","input":{"enforceAccuracy":true,"requiredAccuracyM":10,"accuracyM":20}}),
            serde_json::json!({"operation":"draft","mode":"conflict","cached":{"pole":{"settings":{"x":2},"baseSettings":{"x":1},"editorVersion":1}},"slug":"pole","revision":2,"latest":{"x":3},"now":0}),
        ] {
            let bytes = serde_json::to_vec(&request).unwrap();
            let native = crate::survey::survey_evaluate(&bytes).unwrap();
            let browser: Value =
                serde_json::from_str(&super::survey_evaluate_wasm(&bytes).unwrap()).unwrap();
            assert_eq!(browser, native);
        }
    }

    #[wasm_bindgen_test]
    fn browser_project_control_uses_the_native_room_plan() {
        let input = serde_json::to_vec(&serde_json::json!({
            "project": "project",
            "heads": [{ "name": "clean", "version": 2 }, { "name": "dirty", "version": 3 }],
            "rooms": [
                { "name": "clean", "server_known": true, "server_version": 2 },
                { "name": "dirty", "server_known": true, "server_version": 2, "local_dirty": true }
            ],
            "requested": ["clean", "dirty"],
            "force": true,
            "resident_cap": 8,
            "max_parallel_requests": 4
        }))
        .unwrap();
        let plan: Value = serde_json::from_str(&project_control_rooms(&input).unwrap()).unwrap();
        assert_eq!(plan["schema"], "ds.project-control.room-plan/v1");
        assert_eq!(plan["fetch"], serde_json::json!(["clean"]));
        assert_eq!(plan["reuse"][0]["name"], "dirty");
        assert_eq!(plan["reuse"][0]["provenance"], "local");
    }

    #[wasm_bindgen_test]
    fn browser_membership_identity_uses_the_native_rust_contract() {
        let input = serde_json::to_vec(&serde_json::json!({
            "portfolio_id": "pf_north",
            "cities": ["rw_kigali", "rw_musanze"]
        }))
        .unwrap();
        let identity: Value =
            serde_json::from_str(&solar_portfolio_membership(&input).unwrap()).unwrap();
        assert_eq!(identity["schema"], "ds-solar.portfolio-membership/v1");
        assert_eq!(
            identity["fingerprint"],
            "a28725fa6d460cf2ed3fdda79a2fd9a712df29f8421b2609bdf74a716e59d92c"
        );
    }

    #[wasm_bindgen_test]
    fn browser_export_uses_the_native_login_contract() {
        let now = 1_700_000_000_u64;
        let project = "ds-testing";
        let email = "operator@example.com";
        let uid = "uid-1";
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "sub": uid,
                "email": email,
                "aud": project,
                "iss": format!("https://securetoken.google.com/{project}"),
                "exp": now + 3600,
            }))
            .unwrap(),
        );
        let response = serde_json::to_vec(&serde_json::json!({
            "localId": uid,
            "email": email,
            "idToken": format!("{header}.{claims}.signature"),
            "refreshToken": "refresh",
            "expiresIn": "3600",
        }))
        .unwrap();
        let result =
            validate_firebase_login(&response, Some(email.to_owned()), project, now).unwrap();
        let result: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["user"]["uid"], uid);
    }

    #[wasm_bindgen_test]
    fn browser_portfolio_export_uses_the_same_solar_projection() {
        let mut sections = ds_solar_contracts::portfolio::PORTFOLIO_RESULT_SECTIONS
            .iter()
            .map(|name| ((*name).to_string(), serde_json::json!({ "value": 1 })))
            .collect::<serde_json::Map<_, _>>();
        sections.insert(
            "representative_graphs".to_string(),
            serde_json::json!({ "_strategy": "first" }),
        );
        let city = |city_id: &str, selected_system: &str, customers: i64, energy: f64| {
            serde_json::json!({
                "city_id": city_id,
                "display_name": city_id.to_ascii_uppercase(),
                "city_run_id": format!("run-{city_id}"),
                "input_digest": format!("sha256:{}", "d".repeat(64)),
                "result_digest": format!("sha256:{}", "e".repeat(64)),
                "contribution_digest": format!("sha256:{}", "f".repeat(64)),
                "calculation_projection_digest": format!("sha256:{}", "1".repeat(64)),
                "selected_system": selected_system,
                "inverter_count": 1,
                "pv_capacity_mw": 1.0,
                "battery_capacity_mwh": 1.0,
                "generator_count": 1.0,
                "diesel_capacity_mw": 1.0,
                "total_customers": customers,
                "annual_energy_kwh": energy,
            })
        };
        let document = serde_json::to_vec(&serde_json::json!({
            "schema_version": ds_solar_contracts::PORTFOLIO_SCHEMA_VERSION_V2,
            "engine": { "name": "ds-solar", "version": "0.1.0" },
            "project_id": "project",
            "root": "root",
            "portfolio_id": "portfolio",
            "portfolio_name": "Portfolio",
            "membership_revision": format!("sha256:{}", "a".repeat(64)),
            "run_id": "run-1",
            "input_digest": format!("sha256:{}", "b".repeat(64)),
            "assumptions": { "currency": "XAF", "project_years": 25, "discount_rate": 0.1 },
            "representative_city_id": "a",
            "cities": [city("a", "hybrid", 10, 100.0), city("b", "solar_battery", 20, 200.0)],
            "city_count": 2,
            "total_customers": 30,
            "annual_energy_kwh": 300.0,
            "systems": {
                "hybrid": {
                    "cities": 1, "inverter_count": 1, "pv_capacity_mw": 1.0,
                    "battery_capacity_mwh": 1.0, "generator_count": 1.0,
                    "diesel_capacity_mw": 1.0
                },
                "solar_battery": {
                    "cities": 1, "inverter_count": 1, "pv_capacity_mw": 1.0,
                    "battery_capacity_mwh": 1.0, "generator_count": 1.0,
                    "diesel_capacity_mw": 1.0
                }
            },
            "sections": sections,
            "portfolio_digest": format!("sha256:{}", "c".repeat(64)),
        }))
        .unwrap();
        let result = validate_solar_portfolio(&document, "run-1").unwrap();
        let result: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["trace"]["city_ids"], serde_json::json!(["a", "b"]));
    }
}

pub mod layer_document;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = layerDocument)]
pub fn layer_document_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    layer_document::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod style_document;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = styleDocument)]
pub fn style_document_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    style_document::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod layer_state;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = layerState)]
pub fn layer_state_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    layer_state::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

mod layer_catalog;
pub use layer_catalog::{project_layer_catalog, MAX_LAYER_CATALOG_BYTES};
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = projectLayerCatalog)]
pub fn project_layer_catalog_wasm(
    input: &[u8],
    limit: usize,
) -> Result<String, wasm_bindgen::JsValue> {
    project_layer_catalog(input, limit).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod style_authoring;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = transformStyle)]
pub fn transform_style_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    style_authoring::transform_style(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod style_label;
pub mod style_plan;

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = stylePlan)]
pub fn style_plan_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    style_plan::plan_style_request(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = styleCatalog)]
pub fn style_catalog_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    style_plan::style_catalog_request(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

/// What fields a layer has, what values they carry and what scalar type each
/// one is — over canonical project data, never over what a renderer painted.
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = fieldDomain)]
pub fn field_domain_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    field_domain::evaluate(input)
        .and_then(|value| {
            serde_json::to_string(&value).map_err(|_| "field-domain encoding failed".into())
        })
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod report_formats;

pub mod design_versions;
pub mod design_versions_headless;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designVersionAuthorityCommand)]
pub fn design_version_authority_command_wasm(
    input: &[u8],
) -> Result<String, wasm_bindgen::JsValue> {
    design_versions_headless::authority_command(input)
        .and_then(|value| serde_json::to_string(&value).map_err(|error| error.to_string()))
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designVersionCommand)]
pub fn design_version_command_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_versions::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod design_config;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designConfig)]
pub fn design_config_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_config::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designConfigInspect)]
pub fn design_config_inspect_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_config::inspect(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod autoprocess;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designAutoprocess)]
pub fn design_autoprocess_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    autoprocess::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod force_gate;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designForceGate)]
pub fn design_force_gate_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    force_gate::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod backup_ledger;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = backupLedgerCommand)]
pub fn backup_ledger_command(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    backup_ledger::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod canvas;

pub mod print_scaling;
pub mod printing;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = printingCommand)]
pub fn printing_command(input: &[u8]) -> Result<String, String> {
    printing::evaluate(input)
}

pub mod compute_artifact_inventory;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = computeArtifactInventory)]
pub fn compute_artifact_inventory_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    compute_artifact_inventory::evaluate(input)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod compute_artifact_engine;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = computeArtifactTransition)]
pub fn compute_artifact_transition_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    compute_artifact_engine::evaluate(input)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = computeArtifactFailure)]
pub fn compute_artifact_failure_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    compute_artifact_engine::failure::evaluate(input)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod compute_artifact_queue;

/// The resumable byte-transfer protocol, owned once for every host.
///
/// Pure: it performs no IO and holds no HTTP client, so the same source
/// compiles into the browser WASM package and the native shell. Hosts report
/// observed facts and execute the single returned action; they never decide
/// what an acknowledgement meant. See
/// `docs/contracts/ds-transport-authority.md`.
pub mod transfer;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = transferStep)]
pub fn transfer_step_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    transfer::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

/// Upload admission: identity, order, the concurrency bound, and what a
/// cancellation settles.
///
/// `transfer` owns what happens inside one transfer; this owns what happens
/// between them. A host contributes dispatch and rendering only, so a browser
/// tab, an installed shell and a headless run admit and order uploads the same
/// way. See `docs/contracts/ds-transport-authority.md`.
pub mod upload_queue;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = uploadQueueStep)]
pub fn upload_queue_step_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    upload_queue::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod compute_artifact_cache;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = computeArtifactCache)]
pub fn compute_artifact_cache_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    compute_artifact_cache::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

/// What one HTTP outcome means: transient or terminal, retry and when, worth
/// an SRE report, and what the operator is told. Hosts report facts and
/// execute the decision; the browser, the shell and `ds` cannot answer a
/// status or rewrite a backend message differently.
pub mod response_policy;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = responsePolicy)]
pub fn response_policy_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    response_policy::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

/// Durable lifecycle decisions for host-held survey-photo bytes.
pub mod survey_photo_outbox;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyPhotoOutboxStep)]
pub fn survey_photo_outbox_step_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    survey_photo_outbox::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

/// Pixel processing for survey capture and import. The byte-oriented API is
/// separate from the JSON state machines so photo payloads never become base64
/// command fields.
pub mod survey_photo_media;

pub mod survey_photo_transport;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyPhotoTransport)]
pub fn survey_photo_transport_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    survey_photo_transport::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyPhotoMediaProcess)]
pub fn survey_photo_media_process_wasm(
    input: &[u8],
    rotation_degrees: u16,
) -> Result<survey_photo_media::SurveyPhotoBundleWasm, wasm_bindgen::JsValue> {
    survey_photo_media::process_wasm(input, rotation_degrees)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyPhotoThumbnailPath)]
pub fn survey_photo_thumbnail_path_wasm(path: &str) -> Result<String, wasm_bindgen::JsValue> {
    survey_photo_outbox::thumbnail_path_for(path)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

/// Which stored object a survey media reference names — across the browser's,
/// ds-web's and the field app's three path layouts — and the body a host
/// sends to address it. No host re-derives the grammar.
pub mod survey_media_address;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyMediaAddress)]
pub fn survey_media_address_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    survey_media_address::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

pub mod project_config;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = projectConfigAdmit)]
pub fn project_config_admit_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    project_config::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

/// The one rule for reading a count off a host-supplied value, so no caller
/// clamps on its way in. Declared in the module.
pub mod host_counts;

pub mod design_tags;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designTags)]
pub fn design_tags_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    design_tags::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod collisions;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = collisions)]
pub fn collisions_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    collisions::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod local_layers;
pub mod local_models;
pub mod vector_ops;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = localLayers)]
pub fn local_layers_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    local_layers::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod application_settings;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = applicationSettings)]
pub fn application_settings_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    application_settings::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod grid_publication;
pub mod solar_seed;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = solarSeed)]
pub fn solar_seed_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    solar_seed::evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

pub mod report_consistency;

/// The DS envelope v1, the one answer shape `ds` and the desktop shell give
/// their external clients. Spoken between processes; no WASM export.
pub mod envelope;

/// Where vector-tile compute runs for web, Windows Desktop, Linux Desktop and
/// Linux Server. Source reads and publication remain separate service calls.
pub mod tiling_placement;
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = tilingPlacement)]
pub fn tiling_placement_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    tiling_placement::evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = computeArtifactHead)]
pub fn compute_artifact_head_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    compute_artifact_inventory::evaluate_head(input)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
