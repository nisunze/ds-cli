//! Native Solar application operations. Hosts supply UI effects separately.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub mod application;
pub mod dashboard;
pub mod native;
pub mod pipeline;
pub mod reference;
pub mod settings;
pub mod sync;
pub mod workspace;
pub mod workspace_selection;
pub use ds_solar_contracts::{BundleBytes, SolarError, SolarErrorCode, SolarResult};
pub use ds_solar_io::delivery::bundle_from_delivery;
pub use ds_solar_reference_host::provider::{ReferenceBundleProvider, ReferenceRequest};
/// Private producer handoff; no signed URLs or credentials enter authored requests.
pub struct CapturedCitySnapshot {
    pub snapshot_json: String,
    pub input_base_fingerprint: String,
    pub captured_at: String,
    pub publication: Option<CapturedPublication>,
}
/// Returned by the authenticated capture, never accepted from authored input.
pub struct CapturedPublication {
    pub source_snapshot_sha256: String,
    pub snapshot_receipt_id: String,
    pub snapshot_receipt_expires_at: String,
}
pub trait SnapshotProvider: Send + Sync {
    fn capture(&self, city: &str) -> Result<CapturedCitySnapshot, String>;
}
/// Captured project authority supplies bytes only for an immutable governed reference.
pub trait MediaProvider: Send + Sync {
    fn fetch(&self, city: &str, reference: &str) -> Result<Vec<u8>, String>;
}
/// Closed portfolio output bytes supplied by the native owner.
pub struct PortfolioPublicationFile {
    pub declaration: ds_solar_contracts::portfolio_publication::PortfolioPublicationOutput,
    pub bytes: Vec<u8>,
}
/// Captured local publication authority. Completion retains bytes without a
/// gateway call; the common sync runtime publishes them on reconnect.
pub trait PublicationProvider: Send + Sync {
    fn retain(&self, publication: sync::Publication) -> Result<serde_json::Value, String>;
    /// Observe only an already sealed row. A read cannot adopt an old batch.
    fn observe(&self, publication: &sync::Publication) -> Result<serde_json::Value, String>;
}
#[derive(Debug, Clone, Copy, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReportFormat {
    Docx,
    Pdf,
}
pub trait ReportRenderer: Send + Sync {
    fn render(
        &self,
        copy: ds_command_kernel::solar_report_bundle::RenderCopy,
        format: ReportFormat,
    ) -> Result<Vec<u8>, String>;
}
pub trait PlatformEffects: Send + Sync {
    fn open(&self, _path: &Path, _label: &str) -> Result<(), String> {
        Err("Opening an operating-system application is unavailable on this host".into())
    }
    fn save(&self, _bytes: &[u8], _name: &str, _label: &str) -> Result<bool, String> {
        Err("Choose an explicit native export destination on this host".into())
    }
    fn choose_final(&self) -> Result<Option<PathBuf>, String> {
        Err("An explicit Markdown source path is required on this host".into())
    }
}
pub struct HeadlessEffects;
impl PlatformEffects for HeadlessEffects {}
#[derive(Clone)]
pub struct NativeHost {
    data: PathBuf,
    engine_lane: Option<String>,
    state: Arc<native::SolarNativeState>,
    pub(crate) platform: Arc<dyn PlatformEffects>,
    pub(crate) references: Option<Arc<dyn ReferenceBundleProvider>>,
    pub(crate) media: Option<Arc<dyn MediaProvider>>,
    pub(crate) snapshots: Option<Arc<dyn SnapshotProvider>>,
    pub(crate) publication: Option<Arc<dyn PublicationProvider>>,
    pub(crate) renderer: Option<Arc<dyn ReportRenderer>>,
    pub(crate) workspace_selection: Option<Arc<dyn workspace_selection::Provider>>,
}
impl NativeHost {
    pub fn new(
        data: PathBuf,
        state: Arc<native::SolarNativeState>,
        platform: Arc<dyn PlatformEffects>,
    ) -> Result<Self, String> {
        workspace::secure_private_directory(&data)?;
        Ok(Self {
            data,
            engine_lane: None,
            state,
            platform,
            references: None,
            media: None,
            snapshots: None,
            publication: None,
            renderer: None,
            workspace_selection: None,
        })
    }
    pub fn headless(data: PathBuf) -> Result<Self, String> {
        Self::new(
            data,
            Arc::new(native::SolarNativeState::default()),
            Arc::new(HeadlessEffects),
        )
    }
    pub fn data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.data.clone())
    }
    /// Captured native profile/compiled host lane, never an authored request field.
    /// Standalone development hosts retain their explicit unmanaged selection.
    pub fn with_engine_lane(mut self, lane: &str) -> Result<Self, String> {
        if !matches!(lane, "stable" | "canary") {
            return Err("module_lane_invalid: capture exactly stable or canary".into());
        }
        self.engine_lane = Some(lane.to_owned());
        Ok(self)
    }
    pub(crate) fn capture_engine(&self) -> Result<ds_cli_exec::CapturedExternal, String> {
        self.capture_engine_from(None)
    }
    /// Privileged native host support for an explicitly relocated owned module
    /// store. This diagnostic path is not a captured execution authority.
    pub fn selected_engine_path_in(&self, module_base: &Path) -> Result<PathBuf, String> {
        Ok(self
            .capture_engine_from(Some(module_base))?
            .executable()
            .to_owned())
    }
    fn capture_engine_from(
        &self,
        module_base: Option<&Path>,
    ) -> Result<ds_cli_exec::CapturedExternal, String> {
        // Solar's unmanaged policy remains absolute override/sibling only.
        // A managed migration must not introduce ambient PATH discovery.
        if std::env::var_os(ds_cli_exec::addons::SOLAR.env_override).is_some()
            || self.engine_lane.is_none()
        {
            return ds_cli_exec::addons::SOLAR
                .capture_pinned(&ds_solar_process::installed_engine_path()?)
                .map_err(|error| format!("{}: {error}", error.code()));
        }
        let lane = self.engine_lane.as_deref().expect("managed host lane");
        match module_base {
            Some(base) => ds_cli_exec::addons::SOLAR.capture_for_lane_in(lane, base),
            None => ds_cli_exec::addons::SOLAR.capture_for_lane(lane),
        }
        .map_err(|error| format!("{}: {error}", error.code()))
    }
    pub fn with_workspace_selection_provider(
        mut self,
        provider: Arc<dyn workspace_selection::Provider>,
    ) -> Self {
        self.workspace_selection = Some(provider);
        self
    }
    pub fn with_report_renderer(mut self, renderer: Arc<dyn ReportRenderer>) -> Self {
        self.renderer = Some(renderer);
        self
    }
    pub fn state(&self) -> Arc<native::SolarNativeState> {
        self.state.clone()
    }
    pub fn with_snapshot_provider(mut self, provider: Arc<dyn SnapshotProvider>) -> Self {
        self.snapshots = Some(provider);
        self
    }
    pub fn with_publication_provider(mut self, provider: Arc<dyn PublicationProvider>) -> Self {
        self.publication = Some(provider);
        self
    }
    pub fn with_media_provider(mut self, provider: Arc<dyn MediaProvider>) -> Self {
        self.media = Some(provider);
        self
    }
    pub fn with_reference_provider(mut self, provider: Arc<dyn ReferenceBundleProvider>) -> Self {
        self.references = Some(provider);
        self
    }
}
