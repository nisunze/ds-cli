//! Intentional document source acquisition over the common native preview files.
//! Source authorization and transport are supplied by the closed native client.
//! This owner keeps no URL, credential, second byte cache or editable source.
use crate::{
    preview_artifact::{self, Artifact, RequestReceipt, Retention},
    HostFailure,
};
use ds_project_data::preview_room::Room;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub use ds_command_kernel::asset_preview::{
    Request, SourceProof, MAX_BYTES, RECEIPT_SCHEMA, REQUEST_SCHEMA,
};
const METHOD: &str = "native-pdf-metadata/v1";
fn admission(error: ds_command_kernel::asset_preview::Failure) -> HostFailure {
    HostFailure::new(error.code, error.message)
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Authorizing,
    Waiting,
    Acquiring,
    Decoding,
    Ready,
}
/// Host-owned scope and cancellation facts, never a new authorization source.
pub trait Control {
    fn check_current(&self) -> Result<(), HostFailure>;
    fn progress(&self, phase: Phase, received: u64, total: Option<u64>);
}
pub struct SourceFailure {
    pub failure: HostFailure,
    /// Only a typed source denial/deletion may revoke retained bytes.
    pub retained_forbidden: bool,
}
impl From<HostFailure> for SourceFailure {
    fn from(failure: HostFailure) -> Self {
        Self {
            failure,
            retained_forbidden: false,
        }
    }
}
/// The provider retains its private signed generation grant internally. A
/// public request cannot choose a URL, source path, headers or credentials.
pub trait SourceProvider {
    fn authorize(
        &mut self,
        request: &Request,
        control: &dyn Control,
    ) -> Result<SourceProof, SourceFailure>;
    fn acquire(
        &mut self,
        source: &SourceProof,
        writer: &mut File,
        control: &dyn Control,
    ) -> Result<(), HostFailure>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub source: SourceProof,
    pub measured_sha256: String,
    pub document: Value,
    pub artifact: Artifact,
    pub cache: RequestReceipt,
}
fn fail(code: &'static str, message: impl Into<String>) -> HostFailure {
    HostFailure::new(code, message)
}
fn inputs(request: &Request, revision: &str, generation: &str) -> Value {
    // A first source may advertise no SHA. This exact source identity joins the
    // later fully pinned consumer; measured SHA is proven by the retained file.
    json!({"schema":REQUEST_SCHEMA,"asset_id":request.asset_id,"revision":revision,"generation":generation,"method":METHOD,"decoder_schema":ds_command_kernel::assets::PREVIEW_DOC_SCHEMA})
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Retained {
    schema: String,
    source: SourceProof,
    measured_sha256: String,
    document: Value,
    artifact: Artifact,
}
impl Retained {
    fn receipt(self, cache: RequestReceipt) -> Receipt {
        Receipt {
            schema: self.schema,
            source: self.source,
            measured_sha256: self.measured_sha256,
            document: self.document,
            artifact: self.artifact,
            cache,
        }
    }
}
/// No construction starts IO. An explicit consumer authorizes online, then
/// joins the existing exact disk lease before source acquisition or decoding.
pub fn acquire(
    room: &Room,
    request: &Request,
    provider: &mut dyn SourceProvider,
    control: &dyn Control,
) -> Result<Receipt, HostFailure> {
    request.validate().map_err(admission)?;
    control.check_current()?;
    control.progress(Phase::Waiting, 0, None);
    // Serialize admission and denial for this exact source through the SAME
    // native request lock owner. This lease is never a cached grant: nothing
    // is committed to it, and every online consumer still re-authorizes.
    // A denial cannot race an earlier producer into republishing held bytes.
    let _source_lease = preview_artifact::begin_request_cancellable(
        room,
        json!({"schema":"ds.asset-preview.source-admission/v1","asset_id":request.asset_id}),
        Retention::Hours24,
        || control.check_current(),
    )?;
    let source = if request.offline {
        None
    } else {
        control.progress(Phase::Authorizing, 0, None);
        match provider.authorize(request, control) {
            Ok(source) => {
                control.check_current()?;
                source
                    .validate(&room.scope.project, request)
                    .map_err(admission)?;
                Some(source)
            }
            Err(error) => {
                if error.retained_forbidden {
                    preview_artifact::forget_asset_requests(room, &request.asset_id)?;
                }
                return Err(error.failure);
            }
        }
    };
    let revision = source
        .as_ref()
        .map(|s| s.revision.as_str())
        .or(request.expected_revision.as_deref())
        .ok_or_else(|| {
            fail(
                "asset_preview_pins_invalid",
                "The exact source revision is missing.",
            )
        })?;
    let generation = source
        .as_ref()
        .map(|s| s.generation.as_str())
        .or(request.expected_generation.as_deref())
        .ok_or_else(|| {
            fail(
                "asset_preview_pins_invalid",
                "The exact source generation is missing.",
            )
        })?;
    control.progress(Phase::Waiting, 0, None);
    let mut lease = preview_artifact::begin_request_cancellable(
        room,
        inputs(request, revision, generation),
        request.retention,
        || control.check_current(),
    )?;
    control.check_current()?;
    if let Some(metadata) = lease.retained() {
        let receipt: Retained = serde_json::from_value(metadata.clone()).map_err(|_| {
            fail(
                "asset_preview_retained_invalid",
                "Retained preview metadata is not its native receipt.",
            )
        })?;
        receipt
            .source
            .validate(&room.scope.project, request)
            .map_err(admission)?;
        if source
            .as_ref()
            .is_some_and(|fresh| fresh != &receipt.source)
        {
            return Err(fail(
                "asset_preview_source_changed",
                "Source metadata changed; retained bytes are not admitted as the current preview.",
            ));
        }
        if receipt.schema != RECEIPT_SCHEMA
            || receipt.document["schema"] != ds_command_kernel::assets::PREVIEW_DOC_SCHEMA
            || receipt.document["format"].as_str()
                != ds_command_kernel::assets::semantic::source_format(&receipt.source.content_type)
            || receipt.artifact.content_type != receipt.source.content_type
        {
            return Err(fail(
                "asset_preview_retained_invalid",
                "Retained preview has no exact native document.",
            ));
        }
        receipt
            .source
            .validate_measured(request, &receipt.measured_sha256)
            .map_err(admission)?;
        if receipt.artifact.sha256 != receipt.measured_sha256
            || receipt.artifact.size_bytes != receipt.source.size_bytes
        {
            return Err(fail(
                "asset_preview_retained_invalid",
                "Retained document differs from its exact source byte proof.",
            ));
        }
        preview_artifact::admit_receipt(room, &receipt.artifact)?;
        control.check_current()?;
        control.progress(
            Phase::Ready,
            receipt.source.size_bytes,
            Some(receipt.source.size_bytes),
        );
        return Ok(receipt.receipt(lease.receipt(true)?));
    }
    let source = source.ok_or_else(|| {
        fail(
            "asset_preview_offline_absent",
            "This exact document revision is not retained under the current native authority.",
        )
    })?;
    let mut staging = Stage::new(room)?;
    control.progress(Phase::Acquiring, 0, Some(source.size_bytes));
    provider.acquire(&source, staging.file(), control)?;
    control.check_current()?;
    staging
        .file()
        .flush()
        .map_err(|e| fail("asset_preview_acquire_failed", e.to_string()))?;
    if staging
        .file()
        .metadata()
        .map_err(|e| fail("asset_preview_acquire_failed", e.to_string()))?
        .len()
        != source.size_bytes
    {
        return Err(fail(
            "asset_preview_source_invalid",
            "Acquired bytes differ from the exact source size.",
        ));
    }
    staging
        .file()
        .rewind()
        .map_err(|e| fail("asset_preview_acquire_failed", e.to_string()))?;
    let mut bytes = Vec::new();
    Read::by_ref(staging.file())
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| fail("asset_preview_acquire_failed", e.to_string()))?;
    let measured_sha256 = format!("{:x}", Sha256::digest(&bytes));
    source
        .validate_measured(request, &measured_sha256)
        .map_err(admission)?;
    if bytes.len() as u64 != source.size_bytes {
        return Err(fail(
            "asset_preview_source_invalid",
            "Acquired bytes differ from the exact source size.",
        ));
    }
    control.progress(Phase::Decoding, source.size_bytes, Some(source.size_bytes));
    control.check_current()?;
    let document=ds_command_kernel::assets::evaluate_with_bytes(&bytes,&serde_json::to_vec(&json!({"schema":ds_command_kernel::assets::REQUEST_SCHEMA,"action":"preview","format":ds_command_kernel::assets::semantic::source_format(&source.content_type),"name":source.name})).map_err(|e|fail("asset_preview_decode_failed",e.to_string()))?).map_err(|e|fail("asset_preview_decode_failed",e))?;
    let document: Value = serde_json::from_str(&document)
        .map_err(|e| fail("asset_preview_decode_failed", e.to_string()))?;
    control.check_current()?;
    let artifact = preview_artifact::materialize_document_retained(
        room,
        staging.file(),
        source.size_bytes,
        &measured_sha256,
        &source.content_type,
        json!({"schema":REQUEST_SCHEMA,"source":source,"method":METHOD}),
        request.retention,
    )?;
    control.check_current()?;
    let retained = Retained {
        schema: RECEIPT_SCHEMA.into(),
        source,
        measured_sha256,
        document,
        artifact,
    };
    lease.commit(
        serde_json::to_value(&retained)
            .map_err(|e| fail("asset_preview_retained_invalid", e.to_string()))?,
        &retained.artifact,
    )?;
    control.check_current()?;
    control.progress(
        Phase::Ready,
        retained.source.size_bytes,
        Some(retained.source.size_bytes),
    );
    Ok(retained.receipt(lease.receipt(false)?))
}
static STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Stage {
    file: Option<File>,
    path: PathBuf,
}
impl Stage {
    fn new(room: &Room) -> Result<Self, HostFailure> {
        let parent = room
            .root
            .join("preview-acquire-stage")
            .join(preview_artifact::authority(room)?);
        for ancestor in parent.ancestors().take_while(|p| p.starts_with(&room.root)) {
            match std::fs::symlink_metadata(ancestor) {
                Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(fail(
                        "asset_preview_acquire_failed",
                        "Preview staging root is crossed.",
                    ))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(fail("asset_preview_acquire_failed", e.to_string())),
            }
        }
        ds_layer_store::private::create_dir_all(&parent)
            .map_err(|e| fail("asset_preview_acquire_failed", e.to_string()))?;
        let instant = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| fail("asset_preview_acquire_failed", e.to_string()))?
            .as_nanos();
        let path = parent.join(format!(
            "{}-{instant}-{}.part",
            std::process::id(),
            STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let file = ds_layer_store::private::open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| fail("asset_preview_acquire_failed", e.to_string()))?;
        Ok(Self {
            file: Some(file),
            path,
        })
    }
    fn file(&mut self) -> &mut File {
        self.file.as_mut().expect("live native staging handle")
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize},
        Arc, Barrier,
    };
    use std::time::{Duration, Instant};
    const PDF: &[u8] = b"%PDF-1.7 exact preview bytes\n";
    #[derive(Default)]
    struct Counts {
        authorizations: AtomicUsize,
        acquisitions: AtomicUsize,
        decodes: AtomicUsize,
    }
    struct Guard {
        counts: Arc<Counts>,
        cancelled: AtomicBool,
        cancel_on: Option<Phase>,
    }
    impl Control for Guard {
        fn check_current(&self) -> Result<(), HostFailure> {
            if self.cancelled.load(Ordering::SeqCst) {
                Err(fail("cancelled", "Captured owner or view was released."))
            } else {
                Ok(())
            }
        }
        fn progress(&self, phase: Phase, _: u64, _: Option<u64>) {
            if matches!(phase, Phase::Decoding) {
                self.counts.decodes.fetch_add(1, Ordering::SeqCst);
            }
            if self
                .cancel_on
                .is_some_and(|p| std::mem::discriminant(&p) == std::mem::discriminant(&phase))
            {
                self.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }
    fn guard(counts: Arc<Counts>) -> Guard {
        Guard {
            counts,
            cancelled: AtomicBool::new(false),
            cancel_on: None,
        }
    }
    struct Provider {
        counts: Arc<Counts>,
        source: SourceProof,
        delay: Duration,
        denied: bool,
        corrupt: bool,
    }
    impl SourceProvider for Provider {
        fn authorize(
            &mut self,
            _: &Request,
            control: &dyn Control,
        ) -> Result<SourceProof, SourceFailure> {
            self.counts.authorizations.fetch_add(1, Ordering::SeqCst);
            control.check_current()?;
            if self.denied {
                return Err(SourceFailure {
                    failure: fail("asset_denied", "Source authority denied the attachment."),
                    retained_forbidden: true,
                });
            }
            Ok(self.source.clone())
        }
        fn acquire(
            &mut self,
            _: &SourceProof,
            writer: &mut File,
            control: &dyn Control,
        ) -> Result<(), HostFailure> {
            self.counts.acquisitions.fetch_add(1, Ordering::SeqCst);
            let started = Instant::now();
            while started.elapsed() < self.delay {
                control.check_current()?;
                std::thread::sleep(Duration::from_millis(20));
            }
            control.check_current()?;
            let mut bytes = PDF.to_vec();
            if self.corrupt {
                bytes[10] ^= 1;
            }
            writer
                .write_all(&bytes)
                .map_err(|e| fail("source_transport_failed", e.to_string()))?;
            control.progress(
                Phase::Acquiring,
                bytes.len() as u64,
                Some(bytes.len() as u64),
            );
            Ok(())
        }
    }
    fn room(root: PathBuf) -> Room {
        Room {
            root,
            scope: ds_command_kernel::project_dataset_cache::Scope {
                principal: "owner".into(),
                project: "project".into(),
            },
            lane: "run".into(),
            credential_audience_sha256: "a".repeat(64),
        }
    }
    fn request() -> Request {
        Request {
            schema: REQUEST_SCHEMA.into(),
            asset_id: "sys:pm_attachment:pdf-one".into(),
            expected_revision: None,
            expected_generation: None,
            expected_sha256: None,
            offline: false,
            retention: Retention::Hours24,
        }
    }
    fn provider(counts: Arc<Counts>) -> Provider {
        Provider {
            counts,
            source: SourceProof {
                project_id: "project".into(),
                asset_id: request().asset_id,
                source_kind: "pm_attachment".into(),
                source_id: "pdf-one".into(),
                revision: "actual-version-7".into(),
                generation: "1234567890123456".into(),
                advertised_sha256: None,
                size_bytes: PDF.len() as u64,
                content_type: "application/pdf".into(),
                sensitivity: "internal".into(),
                name: "Meeting.pdf".into(),
            },
            delay: Duration::ZERO,
            denied: false,
            corrupt: false,
        }
    }
    fn offline(receipt: &Receipt) -> Request {
        Request {
            expected_revision: Some(receipt.source.revision.clone()),
            expected_generation: Some(receipt.source.generation.clone()),
            expected_sha256: Some(receipt.measured_sha256.clone()),
            offline: true,
            ..request()
        }
    }
    #[test]
    fn initial_generation_pin_without_advertised_sha_reuses_same_exact_native_file_offline() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let mut p = provider(c.clone());
        let g = guard(c.clone());
        let first = acquire(&room, &request(), &mut p, &g).unwrap();
        assert_eq!(first.source.advertised_sha256, None);
        assert_eq!(first.measured_sha256, format!("{:x}", Sha256::digest(PDF)));
        assert!(!first.cache.reused);
        let second = acquire(&room, &request(), &mut p, &g).unwrap();
        assert!(second.cache.reused);
        assert_eq!(first.artifact, second.artifact);
        p.denied = true;
        let held = acquire(&room, &offline(&first), &mut p, &g).unwrap();
        assert_eq!(held.artifact, first.artifact);
        assert_eq!(c.authorizations.load(Ordering::SeqCst), 2);
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 1);
        assert_eq!(c.decodes.load(Ordering::SeqCst), 1);
        assert_eq!(preview_artifact::read(&room, &held.artifact).unwrap(), PDF);
    }
    #[test]
    fn permitted_restricted_and_confidential_grants_preserve_native_preview_capability() {
        for class in ["restricted", "confidential"] {
            let dir = tempfile::tempdir().unwrap();
            let room = room(dir.path().into());
            let c = Arc::new(Counts::default());
            let mut p = provider(c.clone());
            p.source.sensitivity = class.into();
            let g = guard(c.clone());
            let receipt = acquire(&room, &request(), &mut p, &g).unwrap();
            assert_eq!(receipt.source.sensitivity, class);
            assert!(
                acquire(&room, &offline(&receipt), &mut p, &g)
                    .unwrap()
                    .cache
                    .reused
            );
            p.denied = true;
            assert_eq!(
                acquire(&room, &request(), &mut p, &g).unwrap_err().code,
                "asset_denied"
            );
            assert!(preview_artifact::read(&room, &receipt.artifact).is_err());
            assert_eq!(c.acquisitions.load(Ordering::SeqCst), 1);
        }
    }
    #[test]
    fn concurrent_consumers_authorize_but_join_one_byte_acquire_and_decode() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let gate = Arc::new(Barrier::new(9));
        let threads = (0..8)
            .map(|_| {
                let room = room.clone();
                let c = c.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    let mut p = provider(c.clone());
                    p.delay = Duration::from_millis(120);
                    let g = guard(c);
                    gate.wait();
                    acquire(&room, &request(), &mut p, &g).unwrap()
                })
            })
            .collect::<Vec<_>>();
        gate.wait();
        let receipts = threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(c.authorizations.load(Ordering::SeqCst), 8);
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 1);
        assert_eq!(c.decodes.load(Ordering::SeqCst), 1);
        assert!(receipts.iter().all(|r| r.artifact == receipts[0].artifact));
        assert_eq!(receipts.iter().filter(|r| !r.cache.reused).count(), 1);
    }
    #[test]
    fn cancelled_waiter_does_not_restart_or_cancel_another_consumers_acquisition() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let (sender, receiver) = std::sync::mpsc::channel();
        struct ActiveGuard {
            inner: Guard,
            sender: std::sync::mpsc::Sender<()>,
        }
        impl Control for ActiveGuard {
            fn check_current(&self) -> Result<(), HostFailure> {
                self.inner.check_current()
            }
            fn progress(&self, phase: Phase, received: u64, total: Option<u64>) {
                self.inner.progress(phase, received, total);
                if matches!(phase, Phase::Acquiring) {
                    let _ = self.sender.send(());
                }
            }
        }
        let active_room = room.clone();
        let active_counts = c.clone();
        let active = std::thread::spawn(move || {
            let mut p = provider(active_counts.clone());
            p.delay = Duration::from_millis(180);
            let g = ActiveGuard {
                inner: guard(active_counts),
                sender,
            };
            acquire(&active_room, &request(), &mut p, &g).unwrap()
        });
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut p = provider(c.clone());
        let cancelled = Guard {
            cancel_on: Some(Phase::Waiting),
            ..guard(c.clone())
        };
        assert_eq!(
            acquire(&room, &request(), &mut p, &cancelled)
                .unwrap_err()
                .code,
            "cancelled"
        );
        let finished = active.join().unwrap();
        let g = guard(c.clone());
        assert!(
            acquire(&room, &offline(&finished), &mut p, &g)
                .unwrap()
                .cache
                .reused
        );
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 1);
        assert_eq!(c.decodes.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn known_source_denial_invalidates_old_native_receipts_and_offline_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let mut p = provider(c.clone());
        let g = guard(c.clone());
        let first = acquire(&room, &request(), &mut p, &g).unwrap();
        p.denied = true;
        assert_eq!(
            acquire(&room, &request(), &mut p, &g).unwrap_err().code,
            "asset_denied"
        );
        assert!(preview_artifact::read(&room, &first.artifact).is_err());
        assert_eq!(
            acquire(&room, &offline(&first), &mut p, &g)
                .unwrap_err()
                .code,
            "asset_preview_offline_absent"
        );
        assert_eq!(c.authorizations.load(Ordering::SeqCst), 2);
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn source_denial_refuses_old_receipt_when_foreign_files_block_cleanup_and_only_fresh_bytes_readmit(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let mut p = provider(c.clone());
        let g = guard(c.clone());
        let first = acquire(&room, &request(), &mut p, &g).unwrap();
        let folder = PathBuf::from(&first.artifact.path)
            .parent()
            .unwrap()
            .to_path_buf();
        let foreign = folder.join("foreign.txt");
        std::fs::write(&foreign, b"leave untouched").unwrap();
        p.denied = true;
        assert!(acquire(&room, &request(), &mut p, &g).is_err());
        assert!(preview_artifact::read(&room, &first.artifact).is_err());
        assert_eq!(std::fs::read(&foreign).unwrap(), b"leave untouched");
        assert_eq!(
            acquire(&room, &offline(&first), &mut p, &g)
                .unwrap_err()
                .code,
            "asset_preview_offline_absent"
        );
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 1);
        p.denied = false;
        let fresh = acquire(&room, &request(), &mut p, &g).unwrap();
        assert!(!fresh.cache.reused);
        assert_ne!(fresh.artifact.path, first.artifact.path);
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 2);
        assert_eq!(preview_artifact::read(&room, &fresh.artifact).unwrap(), PDF);
        assert!(preview_artifact::read(&room, &first.artifact).is_err());
        assert_eq!(std::fs::read(&foreign).unwrap(), b"leave untouched");
    }
    #[test]
    fn changed_revision_generation_sha_class_and_scope_refuse_before_source_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let mut p = provider(c.clone());
        let g = guard(c.clone());
        let first = acquire(&room, &request(), &mut p, &g).unwrap();
        let mut pin = offline(&first);
        pin.offline = false;
        for kind in 0..4 {
            let mut p = provider(c.clone());
            match kind {
                0 => p.source.revision = "new-version".into(),
                1 => p.source.generation = "new-generation".into(),
                2 => p.source.advertised_sha256 = Some("b".repeat(64)),
                _ => p.source.sensitivity = "undeclared".into(),
            };
            assert!(acquire(&room, &pin, &mut p, &g).is_err());
        }
        let other = Room {
            scope: ds_command_kernel::project_dataset_cache::Scope {
                principal: "other".into(),
                ..room.scope.clone()
            },
            ..room.clone()
        };
        assert_eq!(
            acquire(&other, &offline(&first), &mut p, &g)
                .unwrap_err()
                .code,
            "asset_preview_offline_absent"
        );
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn advertised_and_selected_sha_are_verified_against_acquired_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let mut p = provider(c.clone());
        p.source.advertised_sha256 = Some(format!("{:x}", Sha256::digest(PDF)));
        p.corrupt = true;
        let g = guard(c.clone());
        assert_eq!(
            acquire(&room, &request(), &mut p, &g).unwrap_err().code,
            "asset_preview_source_changed"
        );
        assert_eq!(c.decodes.load(Ordering::SeqCst), 0);
        let offline = Request {
            expected_revision: Some(p.source.revision.clone()),
            expected_generation: Some(p.source.generation.clone()),
            expected_sha256: p.source.advertised_sha256.clone(),
            offline: true,
            ..request()
        };
        assert_eq!(
            acquire(&room, &offline, &mut p, &g).unwrap_err().code,
            "asset_preview_offline_absent"
        );
    }
    #[test]
    fn cancellation_before_publish_leaves_no_retained_preview_and_staging_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let mut p = provider(c.clone());
        let g = Guard {
            cancel_on: Some(Phase::Decoding),
            ..guard(c.clone())
        };
        assert_eq!(
            acquire(&room, &request(), &mut p, &g).unwrap_err().code,
            "cancelled"
        );
        let stage = room
            .root
            .join("preview-acquire-stage")
            .join(preview_artifact::authority(&room).unwrap());
        assert_eq!(std::fs::read_dir(stage).unwrap().count(), 0);
        let g = guard(c);
        let receipt = acquire(&room, &request(), &mut p, &g).unwrap();
        assert!(!receipt.cache.reused);
    }
    #[test]
    fn unused_and_invalid_offline_consumers_have_zero_provider_calls() {
        let c = Arc::new(Counts::default());
        let mut p = provider(c.clone());
        let g = guard(c.clone());
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        assert_eq!(c.authorizations.load(Ordering::SeqCst), 0);
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 0);
        let invalid = Request {
            offline: true,
            ..request()
        };
        assert_eq!(
            acquire(&room, &invalid, &mut p, &g).unwrap_err().code,
            "asset_preview_pins_invalid"
        );
        assert_eq!(c.authorizations.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn intentional_slow_preview_succeeds_beyond_thirty_seconds_without_restart() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let c = Arc::new(Counts::default());
        let mut p = provider(c.clone());
        p.delay = Duration::from_secs(31);
        let g = guard(c.clone());
        let began = Instant::now();
        let result = acquire(&room, &request(), &mut p, &g).unwrap();
        assert!(began.elapsed() > Duration::from_secs(30));
        assert!(!result.cache.reused);
        assert_eq!(c.acquisitions.load(Ordering::SeqCst), 1);
    }
}
