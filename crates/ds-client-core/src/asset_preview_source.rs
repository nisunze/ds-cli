//! A single source-owned PDF grant. No filesystem, catalogue, byte cache,
//! consumer URL or credential lookup is owned here.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ProjectManagementCall,
};
use ds_command_kernel::asset_preview::{normalized_sha, Request, SourceProof};
use serde_json::{json, Value};
use std::io::Write;
use zeroize::Zeroize;

pub const TIMEOUT_SECONDS: u64 = 180;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferError {
    Unreachable,
    TimedOut,
    Cancelled,
    ContextChanged,
    SourceChanged,
    WriteFailed,
}
impl From<crate::TransportError> for TransferError {
    fn from(error: crate::TransportError) -> Self {
        match error {
            crate::TransportError::TimedOut => Self::TimedOut,
            _ => Self::Unreachable,
        }
    }
}
/// A native host checks its captured authority and cancellation between IO
/// phases/chunks. This is a callback, never an authorization or refresh API.
pub trait TransferControl {
    fn check_current(&self) -> Result<(), TransferError>;
    fn progress(&self, received: u64, total: u64);
}
pub struct DownloadCall<'a> {
    url: &'a str,
    generation: &'a str,
    size: u64,
}
impl DownloadCall<'_> {
    pub fn url(&self) -> &str {
        self.url
    }
    pub fn generation(&self) -> &str {
        self.generation
    }
    pub fn response_limit(&self) -> u64 {
        self.size
    }
    pub fn timeout_seconds(&self) -> u64 {
        TIMEOUT_SECONDS
    }
}
#[derive(Debug)]
pub struct DownloadReceipt {
    pub status: u16,
    pub generation: Option<String>,
    pub size_bytes: u64,
}
/// Only this authority constructor can mint a read. URLs are private,
/// zeroized on release and never returned in a document or Debug output.
pub struct Grant {
    proof: SourceProof,
    url: String,
}
impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            .field("proof", &self.proof)
            .finish_non_exhaustive()
    }
}
impl Drop for Grant {
    fn drop(&mut self) {
        self.url.zeroize();
    }
}
impl Grant {
    pub fn proof(&self) -> &SourceProof {
        &self.proof
    }
    pub fn acquire<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        writer: &mut dyn Write,
        control: &dyn TransferControl,
    ) -> Result<(), TransferError> {
        control.check_current()?;
        let read = transport.asset_preview_bytes(
            DownloadCall {
                url: &self.url,
                generation: &self.proof.generation,
                size: self.proof.size_bytes,
            },
            writer,
            control,
        )?;
        control.check_current()?;
        if read.status != 200
            || read.generation.as_deref() != Some(self.proof.generation.as_str())
            || read.size_bytes != self.proof.size_bytes
        {
            return Err(TransferError::SourceChanged);
        }
        Ok(())
    }
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "attachment source grant is missing or differs from its exact PDF descriptor/generation",
    )
}
fn signed_generation(url: &str) -> Result<String, ClientError> {
    let u = url::Url::parse(url).map_err(|_| invalid())?;
    if url.len() > 8192
        || u.scheme() != "https"
        || u.host_str() != Some("storage.googleapis.com")
        || u.port().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
    {
        return Err(invalid());
    }
    let values = u
        .query_pairs()
        .filter(|(k, _)| k == "generation")
        .map(|(_, v)| v.into_owned())
        .collect::<Vec<_>>();
    if values.len() != 1
        || !values[0].bytes().all(|b| b.is_ascii_digit())
        || values[0].parse::<u64>().ok().is_none_or(|v| v == 0)
    {
        return Err(invalid());
    }
    Ok(values[0].clone())
}
fn text(value: &Value, key: &str) -> Result<String, ClientError> {
    value[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .ok_or_else(invalid)
}
fn positive(value: &Value, key: &str) -> Result<u64, ClientError> {
    value[key].as_u64().filter(|v| *v > 0).ok_or_else(invalid)
}
fn grant(project: &str, request: &Request, data: &Value) -> Result<Grant, ClientError> {
    let (proof, url) = if let Some(id) = request.asset_id.strip_prefix("sys:pm_attachment:") {
        let source = &data["source"];
        if source["project_id"] != project || source["attachment_id"] != id {
            return Err(invalid());
        }
        let generation = positive(source, "generation")?.to_string();
        let proof = SourceProof {
            project_id: project.into(),
            asset_id: request.asset_id.clone(),
            source_kind: "pm_attachment".into(),
            source_id: id.into(),
            revision: format!("pm-version:{}", positive(source, "version")?),
            generation,
            advertised_sha256: source["advertised_sha256"].as_str().map(str::to_owned),
            size_bytes: positive(source, "size_bytes")?,
            content_type: text(source, "content_type")?,
            name: text(source, "original_name")?,
            // The canonical PM asset projection classifies source rows as
            // Internal (assets::tree::Row::asset), not a caller-supplied class.
            sensitivity: "internal".into(),
        };
        (proof, text(data, "url")?)
    } else {
        let asset = &data["asset"];
        if asset["project_id"] != project
            || asset["asset_id"] != request.asset_id
            || asset["format"] != "pdf"
        {
            return Err(invalid());
        }
        let url = text(data, "download_url")?;
        let generation = signed_generation(&url)?;
        let advertised_sha256 = match asset["digest"].as_str() {
            None => None,
            Some(value) => Some(normalized_sha(value).ok_or_else(invalid)?.to_owned()),
        };
        (
            SourceProof {
                project_id: project.into(),
                asset_id: request.asset_id.clone(),
                source_kind: "catalogue_asset".into(),
                source_id: request.asset_id.clone(),
                revision: format!("catalogue-version:{}", positive(asset, "version")?),
                generation,
                advertised_sha256,
                size_bytes: positive(asset, "bytes")?,
                content_type: "application/pdf".into(),
                sensitivity: text(asset, "sensitivity")?,
                name: text(asset, "name")?,
            },
            url,
        )
    };
    if signed_generation(&url)? != proof.generation
        || proof.size_bytes > ds_command_kernel::asset_preview::MAX_BYTES
    {
        return Err(invalid());
    }
    Ok(Grant { proof, url })
}
/// Pure request admission shared by online/offline consumers before any
/// protected identity read or source effect. Reference grammars stay with
/// their existing catalogue and PM owners.
pub fn validate_request(project: &str, request: &Request) -> Result<(), ClientError> {
    crate::validate_project_id(project)?;
    request
        .validate()
        .map_err(|_| ClientError::new(ErrorKind::InvalidInput, "asset preview pins are invalid"))?;
    if let Some(id) = request.asset_id.strip_prefix("sys:pm_attachment:") {
        if !crate::project_management::project_note_id_valid(id) {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "asset preview needs one exact PM attachment reference",
            ));
        }
    } else {
        crate::project_assets::Command::Download {
            asset_id: request.asset_id.clone(),
        }
        .validate(project)?;
    }
    Ok(())
}
pub(crate) fn validate_online_request(project: &str, request: &Request) -> Result<(), ClientError> {
    validate_request(project, request)?;
    if request.offline {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "offline preview never requests a source grant",
        ));
    }
    Ok(())
}
pub(crate) fn authorize<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &Request,
) -> Result<Grant, ClientError> {
    validate_online_request(project, request)?;
    let data = if let Some(id) = request.asset_id.strip_prefix("sys:pm_attachment:") {
        let response = transport.project_management(ProjectManagementCall {
            profile,
            authority: authority.call(),
            payload: json!({"action":"download_attachment_url","ds_project":project,"id":id}),
        })?;
        if response.status != 200 {
            return Err(crate::project_management::refusal(
                response.status,
                &response.body,
            ));
        }
        if response.body.len() > crate::project_management::MAX_RESPONSE_BYTES {
            return Err(invalid());
        }
        let value: Value = serde_json::from_slice(&response.body).map_err(|_| invalid())?;
        if value["success"] != true {
            return Err(invalid());
        }
        value["data"].clone()
    } else {
        crate::project_assets::run(
            profile,
            transport,
            authority,
            project,
            &crate::project_assets::Command::Download {
                asset_id: request.asset_id.clone(),
            },
            None,
        )?
    };
    grant(project, request, &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::*;
    use crate::TransportError;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn request() -> Request {
        serde_json::from_value(json!({"schema":ds_command_kernel::asset_preview::REQUEST_SCHEMA,"asset_id":"sys:pm_attachment:pma_one"})).unwrap()
    }
    fn source() -> Value {
        json!({"url":"https://storage.googleapis.com/owned/source.pdf?generation=9007199254740993&X-Goog-Signature=secret-for-test","source":{"project_id":"project","attachment_id":"pma_one","version":7,"generation":9007199254740993u64,"size_bytes":4,"content_type":"application/pdf","original_name":"Meeting.pdf"}})
    }
    struct Guard {
        calls: AtomicUsize,
        cancelled: bool,
    }
    impl TransferControl for Guard {
        fn check_current(&self) -> Result<(), TransferError> {
            if self.cancelled {
                Err(TransferError::Cancelled)
            } else {
                Ok(())
            }
        }
        fn progress(&self, _: u64, _: u64) {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct Bytes {
        calls: usize,
        generation: Option<String>,
        size: u64,
    }
    impl AuthorizedTransport for Bytes {
        fn list_projects(
            &mut self,
            _call: ProjectListCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn transformer_context(
            &mut self,
            _call: TransformerContextCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn project_forms(
            &mut self,
            _call: ProjectFormsCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn project_form_editor(
            &mut self,
            _call: ProjectFormEditorCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn solar_snapshot(
            &mut self,
            _call: SolarSnapshotCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn survey_query(
            &mut self,
            _call: SurveyQueryCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn survey_entries_select(
            &mut self,
            _call: SurveyEntriesSelectCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn survey_entries_changes(
            &mut self,
            _call: SurveyEntriesChangesCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn survey_entry_create(
            &mut self,
            _call: SurveyEntryCreateCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn project_data(
            &mut self,
            _call: ProjectDataCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn upload_bytes(
            &mut self,
            _call: crate::UploadBytesCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn survey_control(
            &mut self,
            _call: SurveyControlCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn styles(&mut self, _call: StylesCall<'_>) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn layers(&mut self, _call: LayersCall<'_>) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn tiles(&mut self, _call: TileCall<'_>) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn project_report(
            &mut self,
            _call: ProjectReportCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(crate::TransportError::Unreachable)
        }
        fn asset_preview_bytes(
            &mut self,
            call: DownloadCall<'_>,
            writer: &mut dyn Write,
            control: &dyn TransferControl,
        ) -> Result<DownloadReceipt, TransferError> {
            self.calls += 1;
            assert_eq!(call.timeout_seconds(), 180);
            assert_eq!(call.generation(), "9007199254740993");
            assert_eq!(call.response_limit(), 4);
            assert!(call
                .url()
                .starts_with("https://storage.googleapis.com/owned/"));
            control.check_current()?;
            writer
                .write_all(b"test")
                .map_err(|_| TransferError::WriteFailed)?;
            control.progress(4, 4);
            Ok(DownloadReceipt {
                status: 200,
                generation: self.generation.clone(),
                size_bytes: self.size,
            })
        }
    }
    #[test]
    fn source_authority_keeps_integer_generation_and_optional_advertised_sha_exact() {
        let grant = grant("project", &request(), &source()).unwrap();
        assert_eq!(grant.proof.generation, "9007199254740993");
        assert_eq!(grant.proof.revision, "pm-version:7");
        assert_eq!(grant.proof.advertised_sha256, None);
        assert!(!format!("{grant:?}").contains("secret-for-test"));
        assert!(!format!("{grant:?}").contains("storage.googleapis.com"));
    }
    #[test]
    fn changed_project_attachment_generation_and_missing_version_refuse_grant() {
        for field in ["project_id", "attachment_id", "generation", "version"] {
            let mut data = source();
            data["source"][field] = match field {
                "project_id" | "attachment_id" => json!("other"),
                "generation" => json!(9007199254740992u64),
                _ => Value::Null,
            };
            assert!(grant("project", &request(), &data).is_err());
        }
        for url in ["https://attacker.example/source.pdf?generation=9007199254740993","http://storage.googleapis.com/x?generation=9007199254740993","https://storage.googleapis.com/x?generation=9007199254740993&generation=9007199254740993","https://storage.googleapis.com/x"]{let mut data=source();data["url"]=json!(url);assert!(grant("project",&request(),&data).is_err());}
    }
    #[test]
    fn nullable_catalogue_sha_and_server_permitted_classes_keep_original_proof() {
        let req=serde_json::from_value(json!({"schema":ds_command_kernel::asset_preview::REQUEST_SCHEMA,"asset_id":"a_7kq3nr2v0b1c"})).unwrap();
        for class in ["open", "internal", "restricted", "confidential"] {
            let data = json!({"download_url":source()["url"],"asset":{"project_id":"project","asset_id":"a_7kq3nr2v0b1c","format":"pdf","version":2,"bytes":4,"name":"Contract.pdf","sensitivity":class,"digest":null}});
            let grant = grant("project", &req, &data).unwrap();
            assert_eq!(grant.proof.advertised_sha256, None);
            assert_eq!(grant.proof.sensitivity, class);
            grant.proof.validate("project", &req).unwrap();
        }
    }
    #[test]
    fn only_exact_storage_response_and_active_consumer_admit_written_bytes() {
        let grant = grant("project", &request(), &source()).unwrap();
        let guard = Guard {
            calls: AtomicUsize::new(0),
            cancelled: false,
        };
        let mut transport = Bytes {
            calls: 0,
            generation: Some(grant.proof.generation.clone()),
            size: 4,
        };
        let mut writer = Vec::new();
        grant.acquire(&mut transport, &mut writer, &guard).unwrap();
        assert_eq!(writer, b"test");
        assert_eq!(transport.calls, 1);
        assert_eq!(guard.calls.load(Ordering::SeqCst), 1);
        for generation in [None, Some("other".into())] {
            transport.generation = generation;
            assert_eq!(
                grant.acquire(&mut transport, &mut Vec::new(), &guard),
                Err(TransferError::SourceChanged)
            );
        }
        let before = transport.calls;
        let cancelled = Guard {
            calls: AtomicUsize::new(0),
            cancelled: true,
        };
        assert_eq!(
            grant.acquire(&mut transport, &mut Vec::new(), &cancelled),
            Err(TransferError::Cancelled)
        );
        assert_eq!(transport.calls, before);
    }
    #[test]
    fn offline_and_invalid_source_references_refuse_before_authority() {
        let mut req = request();
        req.offline = true;
        req.expected_revision = Some("pm-version:7".into());
        req.expected_generation = Some("9007199254740993".into());
        req.expected_sha256 = Some("a".repeat(64));
        assert!(validate_online_request("project", &req).is_err());
        validate_request("project", &req).unwrap();
        req.offline = false;
        for id in [
            "a_wrong",
            "sys:pm_attachment:..",
            "sys:pm_attachment:parent/child",
            "sys:pm_attachment:",
        ] {
            req.asset_id = id.into();
            assert!(validate_online_request("project", &req).is_err());
        }
    }
}
