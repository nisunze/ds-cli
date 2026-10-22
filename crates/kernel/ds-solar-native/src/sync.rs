//! Solar closes native outputs; the common runtime owns sealing and delivery.
use ds_sync_runtime::{LocalOutput, LocalRow, rows};
use ds_sync_store::PublicationBytes;
use serde_json::{Value, json};

pub struct Publication {
    pub project: String,
    pub row: LocalRow,
    pub declaration: Value,
    pub outputs: Vec<PublicationBytes>,
}

pub struct Source<'a> {
    pub project: &'a str,
    pub run: &'a str,
    pub operation: &'a str,
    pub variant: &'a str,
    pub resource: &'a str,
    pub release: &'a str,
    pub manifest: &'a str,
    pub input_base: &'a str,
}

/// Persist the trusted preparation receipt alongside its exact input digest.
/// Decoding uses the same admitted handoff as headless prepared computation.
pub fn capture_claim(
    prepared: &ds_solar_contracts::PreparedSolarCityInput,
    captured: &crate::CapturedPublication,
) -> Result<Value, String> {
    let claim = json!({
        "schema_version":"ds-solar.prepared-publication-claim/v1",
        "claim_kind":"server-revalidated-publication-handoff",
        "project_id":prepared.city.identity.project_id,
        "city_id":prepared.city.identity.city_id,
        "city_content_digest":prepared.city.content_digest,
        "prepared_input_digest":prepared.input_digest,
        "source_snapshot_sha256":captured.source_snapshot_sha256,
        "input_base_fingerprint":prepared.city.revision.revision,
        "snapshot_receipt_id":captured.snapshot_receipt_id,
        "snapshot_receipt_expires_at":captured.snapshot_receipt_expires_at,
    });
    claim_provenance(prepared, &claim)?;
    Ok(claim)
}

pub fn claim_provenance(
    prepared: &ds_solar_contracts::PreparedSolarCityInput,
    claim: &Value,
) -> Result<ds_compute_runtime::SolarSnapshotProvenance, String> {
    let bytes = ds_compute_runtime::encode_solar_server_submission(
        &serde_json::to_vec(prepared).map_err(|e| e.to_string())?,
        &serde_json::to_vec(claim).map_err(|e| e.to_string())?,
    )?;
    ds_compute_runtime::decode_solar_server_submission(&bytes).map(|(_, provenance)| provenance)
}

/// Build the one native row and declaration from a closed owner inventory.
/// Output metadata is admitted by the common kernel declaration validator.
pub fn close(
    source: Source<'_>,
    mut outputs: Vec<(LocalOutput, Vec<u8>)>,
) -> Result<Publication, String> {
    outputs.sort_by(|a, b| a.0.output_id.cmp(&b.0.output_id));
    let declarations = outputs
        .iter()
        .map(|(declaration, _)| declaration.clone())
        .collect::<Vec<_>>();
    let mut inventory = declarations
        .iter()
        .map(|output| (output.output_id.clone(), output.sha256.clone()))
        .collect::<Vec<_>>();
    let identity = json!({"project":source.project,"source_run":source.run,"operation":source.operation,"variant":source.variant,
        "release":source.release,"manifest":source.manifest,"input_base":source.input_base,"outputs":declarations});
    let key = format!(
        "publication-{}",
        ds_solar_contracts::digest::hex_sha256(
            &serde_json::to_vec(&identity).map_err(|e| e.to_string())?
        )
    );
    let row = LocalRow {
        identity: ds_command_kernel::sync::Identity {
            engine: ds_sync_runtime::solar::ENGINE.into(),
            operation: source.operation.into(),
            variant: source.variant.into(),
        },
        sha256: rows::inventory_digest(&mut inventory),
        size_bytes: declarations
            .iter()
            .try_fold(0u64, |total, output| total.checked_add(output.size_bytes))
            .ok_or("publication size overflow")?,
        produced_at_ms: rows::now_ms(),
        base_revision: None,
        input_base_fingerprint: Some(source.input_base.into()),
        readable: true,
        engine_release: source.release.into(),
        engine_build_manifest_sha256: source.manifest.into(),
        resource: source.resource.into(),
        client_publish_id: key.clone(),
        outputs: declarations,
        bytes_locator: ds_sync_runtime::retained_publication::locator(&key),
    };
    let declaration = rows::open_declaration(source.project, &row);
    Ok(Publication {
        project: source.project.into(),
        row,
        declaration,
        outputs: outputs
            .into_iter()
            .map(|(output, bytes)| PublicationBytes {
                output_id: output.output_id,
                bytes,
            })
            .collect(),
    })
}

pub fn portfolio(
    result: &ds_solar_contracts::SolarPortfolioResult,
    bytes: Vec<u8>,
    files: Vec<crate::PortfolioPublicationFile>,
) -> Result<Publication, String> {
    portfolio_with_manifest(result, bytes, files, &result.engine.build_manifest_sha256)
}

pub(crate) fn portfolio_with_manifest(
    result: &ds_solar_contracts::SolarPortfolioResult,
    bytes: Vec<u8>,
    files: Vec<crate::PortfolioPublicationFile>,
    manifest: &str,
) -> Result<Publication, String> {
    if !result.engine.build_manifest_sha256.is_empty()
        && result.engine.build_manifest_sha256 != manifest
    {
        return Err("Portfolio bytes belong to another producer build".into());
    }
    result.validate().map_err(|e| e.to_string())?;
    let decoded: ds_solar_contracts::SolarPortfolioResult = serde_json::from_slice(&bytes)
        .map_err(|_| "portfolio publication has invalid result bytes")?;
    if decoded != *result {
        return Err("portfolio publication bytes differ from their closed result".into());
    }
    let metadata =
        |id: String, format: String, content_type: String, sha256: String, size_bytes: u64| {
            LocalOutput {
                filename: None,
                paper_size: None,
                presentation: None,
                output_id: id,
                format,
                content_type,
                sha256,
                size_bytes,
            }
        };
    let mut outputs = vec![(
        metadata(
            "portfolio-result".into(),
            "json".into(),
            "application/json".into(),
            ds_solar_contracts::digest::hex_sha256(&bytes),
            bytes.len() as u64,
        ),
        bytes,
    )];
    for file in files {
        let d = file.declaration;
        outputs.push((
            metadata(
                d.output_id,
                d.format,
                d.content_type,
                d.sha256,
                d.size_bytes,
            ),
            file.bytes,
        ));
    }
    let operation = format!("portfolio-{}", result.portfolio_id);
    let release = format!("{}@{}", result.engine.name, result.engine.version);
    let base = result
        .membership_revision
        .strip_prefix("sha256:")
        .ok_or("portfolio membership has no exact digest")?;
    let publication = close(
        Source {
            project: &result.project_id,
            run: &result.run_id,
            operation: &operation,
            variant: "aggregate-v3",
            resource: &result.portfolio_id,
            release: &release,
            manifest,
            input_base: base,
        },
        outputs,
    )?;
    ds_command_kernel::sync::declaration::validate(&publication.project, &publication.declaration)?;
    Ok(publication)
}

/// The local queue receipt is a projection of the kernel's state, not delivery.
pub fn receipt(
    project: &str,
    requested: &str,
    key: &str,
    state: ds_command_kernel::sync_store::ArtifactState,
) -> Value {
    json!({"project_id":project,"requested_publish_id":requested,"client_publish_id":key,"state":state.as_str()})
}

/// Hosts capture the fence and authorize locally; this one handoff records all
/// native Solar publication kinds and projects the common store's answer.
pub fn retain(
    store: &ds_sync_runtime::SharedStore,
    fence: &ds_command_kernel::sync_store::Fence,
    publication: Publication,
    free: &dyn Fn(&ds_command_kernel::sync_store::ArtifactRow) -> Result<u64, String>,
) -> Result<Value, String> {
    use ds_sync_runtime::seal::Recorded;
    let requested = publication.row.client_publish_id.clone();
    let sealed = ds_sync_runtime::retained_publication::seal(
        store,
        fence,
        &publication.project,
        &publication.row,
        publication.declaration,
        publication.outputs,
        free,
    )?;
    let (key, state) = match sealed {
        Recorded::Recorded { row, .. } => (row.client_publish_id, row.state),
        Recorded::AlreadyRecorded { replay_key, state } => (replay_key, state),
        Recorded::Refused(refusal) => return Err(format!("{}: {}", refusal.code, refusal.detail)),
    };
    Ok(receipt(&publication.project, &requested, &key, state))
}

/// Read only: an old closed batch with no completion seal is never adopted.
pub fn recorded(
    store: &ds_sync_runtime::SharedStore,
    fence: &ds_command_kernel::sync_store::Fence,
    publication: &Publication,
) -> Result<Value, String> {
    let row = ds_sync_runtime::seal::standing(
        store,
        fence,
        &publication.project,
        &publication.row.identity,
    )?
    .ok_or("publication_not_sealed: run this portfolio again before publishing it")?;
    let expected = &publication.row;
    let same_completion = row.client_publish_id == expected.client_publish_id
        && row.sha256 == expected.sha256
        && row.publication_input_base() == expected.input_base_fingerprint.as_deref()
        && row.engine_release == expected.engine_release
        && row.engine_build_manifest_sha256.as_deref().unwrap_or("")
            == expected.engine_build_manifest_sha256;
    if !(same_completion
        || row.matches_publication(
            &expected.sha256,
            expected.input_base_fingerprint.as_deref().unwrap_or(""),
            &expected.engine_release,
            &expected.engine_build_manifest_sha256,
        ))
        || ds_sync_runtime::rows::local_row_from_store(&row).outputs != expected.outputs
    {
        return Err(
            "publication_replaced: this batch is not the sealed publication; run it again".into(),
        );
    }
    Ok(receipt(
        &publication.project,
        &expected.client_publish_id,
        &row.client_publish_id,
        row.state,
    ))
}

/// The one completion retention every host binds: the Server, and the Desktop
/// that wraps it. A closed native publication seals into the host's common
/// store under its captured identity, and the row it displaces frees its bytes
/// through the shared Solar producer, whichever storage holds them. The host
/// supplies only its authority recheck (the Server's owner fence, the
/// Desktop's session fence) and an optional wake for its publication pass.
/// Retention makes no gateway call, so an offline completion is retained and
/// a reconnect publishes it without recomputation.
pub struct CommonRetention {
    database: std::path::PathBuf,
    identity: ds_compute_runtime::HostIdentity,
    project: String,
    authorize: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    completed: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl CommonRetention {
    pub fn new(
        database: std::path::PathBuf,
        identity: ds_compute_runtime::HostIdentity,
        project: &str,
        authorize: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) -> Self {
        Self {
            database,
            identity,
            project: project.to_owned(),
            authorize,
            completed: None,
        }
    }
    /// Wake the host's publication pass after a retention or observation.
    pub fn with_completion(mut self, completed: std::sync::Arc<dyn Fn() + Send + Sync>) -> Self {
        self.completed = Some(completed);
        self
    }
    fn captured(&self, publication: &Publication) -> Result<(), String> {
        (self.authorize)()?;
        if publication.project != self.project {
            return Err("Solar publication crosses its captured project".into());
        }
        Ok(())
    }
}

impl crate::PublicationProvider for CommonRetention {
    fn retain(&self, publication: Publication) -> Result<Value, String> {
        use ds_sync_runtime::Producer;
        self.captured(&publication)?;
        let store = ds_sync_runtime::open_store(&self.database)?;
        let fence = ds_sync_runtime::solar_producer::fence_of(&self.identity);
        let producer = ds_sync_runtime::solar_producer::SolarProducer {
            database: &self.database,
            identity: &self.identity,
            project: self.project.clone(),
            failure: None,
        };
        let receipt = retain(&store, &fence, publication, &|previous| {
            producer.reclaim(&self.project, &rows::local_row_from_store(previous))
        })?;
        if let Some(completed) = &self.completed {
            completed();
        }
        (self.authorize)()?;
        Ok(receipt)
    }
    fn observe(&self, publication: &Publication) -> Result<Value, String> {
        self.captured(publication)?;
        let store = ds_sync_runtime::open_store(&self.database)?;
        let receipt = recorded(
            &store,
            &ds_sync_runtime::solar_producer::fence_of(&self.identity),
            publication,
        )?;
        if let Some(completed) = &self.completed {
            completed();
        }
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests;
