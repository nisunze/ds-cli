//! Disk-only MV sample answers shared by Server, CLI and Desktop. The native
//! preview room owns exact render reuse; consumers receive only file receipts.
use crate::{mv_preview, preview_artifact, HostFailure, ReportEngine};
use ds_project_data::preview_room::Room;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SCHEMA: &str = "ds.mv-preview-artifacts/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    pub source_sheet_number: u64,
    pub alignment_id: String,
    pub from_station_m: f64,
    pub to_station_m: f64,
    pub page: preview_artifact::Artifact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub schema: String,
    pub project: String,
    pub layout_id: String,
    pub model_revision: String,
    pub setup_sha256: String,
    pub page_count: u64,
    pub full_page_count: u64,
    pub sheet: Sheet,
}

impl Answer {
    pub fn to_value(&self) -> Value {
        json!({"schema":self.schema,"project":self.project,"layout_id":self.layout_id,
            "model_revision":self.model_revision,"setup_sha256":self.setup_sha256,
            "page_count":self.page_count,"full_page_count":self.full_page_count,
            "sheet":self.sheet,"pages":[self.sheet],
            "progress":{"completed":1,"total":1,"stage":"complete"}})
    }
}

pub fn execute_retained(
    engine: &dyn ReportEngine,
    inputs: &mv_preview::MvPreviewInputs<'_>,
    room: &Room,
    retention: preview_artifact::Retention,
) -> Result<Answer, mv_preview::MvPreviewRefusal> {
    execute_bound(engine, inputs, room, retention, None)
}

pub fn execute_bound(
    engine: &dyn ReportEngine,
    inputs: &mv_preview::MvPreviewInputs<'_>,
    room: &Room,
    retention: preview_artifact::Retention,
    source: Option<&ds_command_kernel::printing::mv_models::SourceBinding>,
) -> Result<Answer, mv_preview::MvPreviewRefusal> {
    mv_preview::validate_source(inputs, source)?;
    if room.scope.project != inputs.project_id {
        return Err(HostFailure::new(
            crate::CODE_INPUTS_INVALID,
            "the MV preview artifact room belongs to another project",
        )
        .into());
    }
    let identity = crate::engine_identity_now(engine)?;
    let rendered = mv_preview::execute_retained_bound(engine, inputs, Some(room), source)?;
    if crate::engine_identity_now(engine)? != identity {
        return Err(HostFailure::new(
            crate::CODE_ENGINE_IDENTITY_CHANGED,
            "the reporter build changed while materializing an MV preview",
        )
        .into());
    }
    let provenance = json!({"source_binding":source,"schema":SCHEMA,"project":inputs.project_id,
        "layout":inputs.layout,"sheets":inputs.sheets,"styles":inputs.table,
        "project_crs":inputs.project_crs,"model_crs":inputs.model_crs,"engine":identity,
        "scene_sha256":ds_command_kernel::report_export::sha256_hex(inputs.scene),
        "plan_sha256":ds_command_kernel::report_export::sha256_hex(inputs.plan),
        "model_revision":rendered.model_revision,"setup_sha256":rendered.setup_sha256,
        "page_count":rendered.page_count,"full_page_count":rendered.full_page_count,
        "source_sheet_number":rendered.sheet.source_sheet_number,
        "alignment_id":rendered.sheet.alignment_id,
        "from_station_m":rendered.sheet.from_station_m,"to_station_m":rendered.sheet.to_station_m});
    let page = preview_artifact::materialize_svg_retained(
        room,
        &rendered.sheet.svg,
        provenance,
        retention,
    )?;
    if crate::engine_identity_now(engine)? != identity {
        return Err(HostFailure::new(
            crate::CODE_ENGINE_IDENTITY_CHANGED,
            "the reporter build changed while persisting an MV preview",
        )
        .into());
    }
    Ok(Answer {
        schema: SCHEMA.into(),
        project: rendered.project_id,
        layout_id: rendered.layout_id,
        model_revision: rendered.model_revision,
        setup_sha256: rendered.setup_sha256,
        page_count: rendered.page_count,
        full_page_count: rendered.full_page_count,
        sheet: Sheet {
            source_sheet_number: rendered.sheet.source_sheet_number,
            alignment_id: rendered.sheet.alignment_id,
            from_station_m: rendered.sheet.from_station_m,
            to_station_m: rendered.sheet.to_station_m,
            page,
        },
    })
}
