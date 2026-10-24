//! Held-first reads a person sees: the platform's transformer delete ledger,
//! a lane's latest release (or its installation manager's publication), a
//! project's Assets catalogue, `/config` document and survey pipeline status,
//! and a transformer's version history and downloaded versions. What this device holds answers at once and
//! offline (`*Held`); a refresh (`*Refresh`) fetches through the gateway
//! exchange, admits the answer and retains it in [`crate::retained_reads`].
//! A refresh that fails leaves the held value as it was; a refusal that says
//! the read is gone or no longer this account's (403, 404) forgets it.

use serde::Serialize;
use serde_json::{json, Value};

use super::retained::{self, Edge};
use super::{project_reference, Exchange, Refusal};
use crate::retained_reads::Kind;

/// A held or fresh value and when this device saved it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Shown {
    pub value: Value,
    pub saved_at_ms: u64,
}

/// What this device holds for one key, if it still reads.
pub async fn held(
    edge: &impl Edge,
    kind: Kind,
    project: Option<&str>,
    key: &str,
) -> Result<Option<Shown>, Refusal> {
    let observed = retained::read::<Value>(edge, kind, project, key).await?;
    Ok(observed.held.map(|held| Shown {
        value: held.value,
        saved_at_ms: held.saved_at_ms,
    }))
}

/// The read is gone, or no longer this account's to read.
pub fn revoked(refusal: &Refusal) -> bool {
    matches!(refusal.status, Some(403 | 404))
}

/// The identity a later write is fenced by, observed before the fetch, so an
/// answer fetched while another account signed in is never retained.
async fn owner(edge: &impl Edge, kind: Kind, project: Option<&str>, key: &str) -> Option<String> {
    retained::read::<Value>(edge, kind, project, key)
        .await
        .ok()
        .and_then(|observed| observed.owner)
}

/// Retain one fresh value; a store that cannot keep it never hides it.
async fn retain(
    edge: &impl Edge,
    owner: Option<String>,
    kind: Kind,
    project: Option<&str>,
    key: &str,
    value: Value,
) -> Shown {
    if let Some(owner) = owner {
        let _ = retained::write(edge, Some(&owner), kind, project, key, &value, None).await;
    }
    Shown {
        value,
        saved_at_ms: edge.now_ms(),
    }
}

/// A revoking refusal forgets the held value; every refusal is answered.
async fn refused(
    edge: &impl Edge,
    owner: Option<String>,
    kind: Kind,
    project: Option<&str>,
    key: &str,
    refusal: Refusal,
) -> Refusal {
    if let Some(owner) = owner.filter(|_| revoked(&refusal)) {
        let _ = retained::forget(edge, Some(&owner), kind, project, Some(key)).await;
    }
    refusal
}

/// Fetch one exchange and retain its answer.
pub async fn refresh<C>(
    edge: &impl Edge,
    kind: Kind,
    project: Option<&str>,
    key: &str,
    call: C,
) -> Result<Shown, Refusal>
where
    C: Exchange,
    C::Output: Serialize,
{
    let owner = owner(edge, kind, project, key).await;
    match edge.fetch(call).await {
        Ok(output) => {
            let value = serde_json::to_value(output)
                .map_err(|error| Refusal::new(crate::retained_reads::INVALID, error.to_string()))?;
            Ok(retain(edge, owner, kind, project, key, value).await)
        }
        Err(refusal) => Err(refused(edge, owner, kind, project, key, refusal).await),
    }
}

/// The platform's transformer delete ledger, one row per window of days.
pub mod backup_ledger {
    use super::*;
    use crate::gateway_exchange::archive::DeletedTransformers;

    pub const KIND: Kind = Kind::BackupLedger;

    pub async fn held(edge: &impl Edge, days: u32) -> Result<Option<Shown>, Refusal> {
        super::held(edge, KIND, None, &days.to_string()).await
    }

    pub async fn refresh(
        edge: &impl Edge,
        days: u32,
        limit: Option<u32>,
    ) -> Result<Shown, Refusal> {
        let call = DeletedTransformers {
            days: Some(days),
            limit,
        };
        super::refresh(edge, KIND, None, &days.to_string(), call).await
    }
}

/// A lane's latest release at one release URL, keyed by the URL's digest.
pub mod release {
    use super::*;

    pub const KIND: Kind = Kind::AppRelease;

    pub fn key(url: &str) -> String {
        crate::durable_store::sha256(url.as_bytes())
    }

    pub async fn held(edge: &impl Edge, url: &str) -> Result<Option<Shown>, Refusal> {
        super::held(edge, KIND, None, &key(url)).await
    }

    /// Retain a release answer's `release` member as `{ release }`.
    pub async fn retain(edge: &impl Edge, url: &str, member: Value) -> Shown {
        let owner = owner(edge, KIND, None, &key(url)).await;
        super::retain(
            edge,
            owner,
            KIND,
            None,
            &key(url),
            json!({ "release": member }),
        )
        .await
    }
}

/// A project's Assets catalogue: its declared folders and every row, walked
/// page by page.
pub mod assets_catalogue {
    use super::*;
    use crate::gateway_exchange::assets::{List, ListQuery, Tree};

    pub const KIND: Kind = Kind::AssetsCatalogue;
    /// Rows per page, and the most pages one walk reads.
    pub const PAGE_ROWS: u32 = 200;
    pub const PAGES_MAX: usize = 100;

    pub async fn held(edge: &impl Edge, project: &str) -> Result<Option<Shown>, Refusal> {
        project_reference(project)?;
        super::held(edge, KIND, Some(project), "").await
    }

    /// The folders and every row (`complete` when the walk reached the end).
    pub async fn refresh(edge: &impl Edge, project: &str) -> Result<Shown, Refusal> {
        project_reference(project)?;
        let owner = owner(edge, KIND, Some(project), "").await;
        let folders = match edge
            .fetch(Tree {
                project: project.into(),
            })
            .await
        {
            Ok(folders) => folders,
            Err(refusal) => {
                return Err(refused(edge, owner, KIND, Some(project), "", refusal).await)
            }
        };
        let mut assets = Vec::new();
        let mut cursor = None;
        let mut complete = false;
        for _ in 0..PAGES_MAX {
            let query = ListQuery {
                limit: Some(PAGE_ROWS),
                cursor: cursor.take(),
                ..ListQuery::default()
            };
            let page = match edge
                .fetch(List {
                    project: project.into(),
                    query,
                })
                .await
            {
                Ok(page) => page,
                Err(refusal) => {
                    return Err(refused(edge, owner, KIND, Some(project), "", refusal).await)
                }
            };
            assets.extend(page.assets);
            match page
                .next_cursor
                .filter(|next| page.has_more && !next.is_empty())
            {
                Some(next) => cursor = Some(next),
                None => {
                    complete = true;
                    break;
                }
            }
        }
        let value = json!({ "assets": assets, "folders": folders, "complete": complete });
        Ok(retain(edge, owner, KIND, Some(project), "", value).await)
    }
}

/// A project's `/config` document. A configuration save forgets the held
/// document, so a refresh fetched before the save is refused
/// (`retained_reads_stale`) instead of keeping the older document.
pub mod project_config {
    use super::*;
    use crate::gateway_exchange::design::DesignCall;

    pub const KIND: Kind = Kind::ProjectConfig;

    pub async fn held(edge: &impl Edge, project: &str) -> Result<Option<Shown>, Refusal> {
        project_reference(project)?;
        super::held(edge, KIND, Some(project), "").await
    }

    pub async fn refresh(edge: &impl Edge, project: &str) -> Result<Shown, Refusal> {
        project_reference(project)?;
        let observed = retained::read::<Value>(edge, KIND, Some(project), "").await?;
        let owner = observed.owner.ok_or_else(|| {
            Refusal::new(
                crate::durable_store::INVALID,
                "Project configuration retention requires the bound store owner.",
            )
        })?;
        let value = match edge
            .fetch(DesignCall::ProjectConfig {
                project: project.into(),
            })
            .await
        {
            Ok(value) => value,
            Err(refusal) => {
                return Err(refused(edge, Some(owner), KIND, Some(project), "", refusal).await)
            }
        };
        retained::write(
            edge,
            Some(&owner),
            KIND,
            Some(project),
            "",
            &value,
            Some(observed.generation),
        )
        .await?;
        Ok(Shown {
            value,
            saved_at_ms: edge.now_ms(),
        })
    }

    pub async fn forget(edge: &impl Edge, project: &str) -> Result<(), Refusal> {
        project_reference(project)?;
        retained::forget(edge, None, KIND, Some(project), None).await
    }

    /// Held-first acquisition: the held document answers without a fetch;
    /// only a project with nothing held performs one explicit read and
    /// retains it. The answer names which of the two produced the document.
    pub async fn acquire(
        edge: &impl Edge,
        project: &str,
    ) -> Result<
        (
            Shown,
            crate::gateway_exchange::design::ProjectConfigurationRead,
        ),
        Refusal,
    > {
        use crate::gateway_exchange::design::ProjectConfigurationRead as Source;
        if let Some(held) = held(edge, project).await? {
            return Ok((held, Source::Held));
        }
        Ok((refresh(edge, project).await?, Source::Refresh))
    }
}

/// A project's survey sync and publication status.
pub mod pipeline_status {
    use super::*;
    use crate::gateway_exchange::pipeline::PipelineCall;

    pub const KIND: Kind = Kind::PipelineStatus;

    pub async fn held(edge: &impl Edge, project: &str) -> Result<Option<Shown>, Refusal> {
        project_reference(project)?;
        super::held(edge, KIND, Some(project), "").await
    }

    pub async fn refresh(edge: &impl Edge, project: &str) -> Result<Shown, Refusal> {
        project_reference(project)?;
        super::refresh(
            edge,
            KIND,
            Some(project),
            "",
            PipelineCall::Status {
                project: project.into(),
            },
        )
        .await
    }
}

/// A transformer's governed version history as the version authority admits
/// it, and the versions a person downloaded for playback.
pub mod transformer_versions {
    use super::*;
    use crate::gateway_exchange::design_collab::{DesignCollab, Surface, INVALID};

    pub const KIND: Kind = Kind::TransformerVersions;
    pub const DOWNLOAD: Kind = Kind::TransformerVersion;

    fn object(transformer: &str) -> Value {
        json!({"kind": "lv_transformer", "id": transformer})
    }

    fn call(request: Value) -> DesignCollab {
        DesignCollab {
            surface: Surface::Versions,
            request: request.as_object().cloned().unwrap_or_default(),
        }
    }

    /// The version authority's admission of one answer.
    fn accept(
        project: &str,
        transformer: &str,
        operation: &str,
        data: Value,
    ) -> Result<Value, Refusal> {
        let request = json!({"action": "accept", "operation": operation, "project": project,
            "object": object(transformer), "response": {"success": true, "data": data}});
        let bytes = serde_json::to_vec(&request)
            .map_err(|error| Refusal::new(INVALID, error.to_string()))?;
        crate::design_versions_headless::authority_command(&bytes)
            .map_err(|message| Refusal::new(INVALID, message))
    }

    pub fn download_key(transformer: &str, version: &str) -> String {
        serde_json::to_string(&(transformer, version)).unwrap_or_default()
    }

    pub async fn held(
        edge: &impl Edge,
        project: &str,
        transformer: &str,
    ) -> Result<Option<Shown>, Refusal> {
        project_reference(project)?;
        super::held(edge, KIND, Some(project), transformer).await
    }

    /// The version records, newest first, retained.
    pub async fn refresh(
        edge: &impl Edge,
        project: &str,
        transformer: &str,
    ) -> Result<Shown, Refusal> {
        project_reference(project)?;
        let owner = owner(edge, KIND, Some(project), transformer).await;
        let request = json!({"action": "list_versions", "project_id": project, "object": object(transformer)});
        let answer = match edge.fetch(call(request)).await {
            Ok(answer) => answer,
            Err(refusal) => {
                return Err(refused(edge, owner, KIND, Some(project), transformer, refusal).await)
            }
        };
        let accepted = accept(project, transformer, "list_versions", answer)?;
        Ok(retain(
            edge,
            owner,
            KIND,
            Some(project),
            transformer,
            accepted["versions"].clone(),
        )
        .await)
    }

    pub async fn downloaded(
        edge: &impl Edge,
        project: &str,
        transformer: &str,
        version: &str,
    ) -> Result<Option<Shown>, Refusal> {
        project_reference(project)?;
        super::held(
            edge,
            DOWNLOAD,
            Some(project),
            &download_key(transformer, version),
        )
        .await
    }

    /// One version as `{ version, snapshot }`, retained for playback offline.
    pub async fn download(
        edge: &impl Edge,
        project: &str,
        transformer: &str,
        version: &str,
    ) -> Result<Shown, Refusal> {
        project_reference(project)?;
        let key = download_key(transformer, version);
        let owner = owner(edge, DOWNLOAD, Some(project), &key).await;
        let request = json!({"action": "get_version", "project_id": project, "object": object(transformer), "version_id": version});
        let answer = match edge.fetch(call(request)).await {
            Ok(answer) => answer,
            Err(refusal) => {
                return Err(refused(edge, owner, DOWNLOAD, Some(project), &key, refusal).await)
            }
        };
        let accepted = accept(project, transformer, "get_version", answer)?;
        if accepted["version"]["version_id"] != version {
            return Err(Refusal::new(
                INVALID,
                "The version service answered another version.",
            ));
        }
        if accepted["version"]["playback_available"] != true {
            return Err(Refusal::new(
                INVALID,
                "This version has no playback snapshot.",
            ));
        }
        crate::design_versions::layers(&accepted["snapshot"])
            .map_err(|message| Refusal::new(INVALID, message))?;
        let value = json!({"version": accepted["version"], "snapshot": accepted.get("snapshot").cloned().unwrap_or(Value::Null)});
        Ok(retain(edge, owner, DOWNLOAD, Some(project), &key, value).await)
    }

    /// How many downloaded versions this device holds for the project.
    pub async fn downloads(edge: &impl Edge, project: &str) -> Result<u64, Refusal> {
        project_reference(project)?;
        retained::count(edge, DOWNLOAD, Some(project)).await
    }

    pub async fn forget_downloads(edge: &impl Edge, project: &str) -> Result<(), Refusal> {
        project_reference(project)?;
        retained::forget(edge, None, DOWNLOAD, Some(project), None).await
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{
        assets_catalogue, backup_ledger, pipeline_status, project_config, release,
        transformer_versions,
    };
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::releases::{
        exports::read, installation_manager, release_member, ANSWER_CAP, MANAGER_CAP,
    };
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(typescript_custom_section)]
    const TYPES: &'static str = r#"
/** A value this device holds, and when it saved it. */
export interface HeldShown<T> { value: T; saved_at_ms: number }
/** A project's Assets catalogue: its folders and every row (`complete` when the walk reached the end). */
export interface AssetsCatalogue { assets: Record<string, unknown>[]; folders: Record<string, unknown>[]; complete: boolean }
"#;

    fn edge(host: GatewayHost, store: StoreHost) -> BrowserEdge {
        BrowserEdge::new(host, store)
    }

    /// The delete ledger window this device holds, or null.
    #[wasm_bindgen(js_name = backupLedgerHeld, unchecked_return_type = "HeldShown<{ rows: Record<string, unknown>[]; count: number }> | null")]
    pub async fn backup_ledger_held(
        host: GatewayHost,
        store: StoreHost,
        days: u32,
    ) -> Result<JsValue, JsValue> {
        let held = backup_ledger::held(&edge(host, store), days).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// One window of the delete ledger, newest deletion first, retained.
    #[wasm_bindgen(js_name = backupLedgerRefresh, unchecked_return_type = "HeldShown<{ rows: Record<string, unknown>[]; count: number }>")]
    pub async fn backup_ledger_refresh(
        host: GatewayHost,
        store: StoreHost,
        days: u32,
        limit: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let fresh = backup_ledger::refresh(&edge(host, store), days, limit).await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }

    /// The release this device holds for a release URL, as `{ release }`, or undefined.
    #[wasm_bindgen(js_name = releaseHeld, unchecked_return_type = "{ release: unknown } | undefined")]
    pub async fn release_held(
        host: GatewayHost,
        store: StoreHost,
        url: String,
    ) -> Result<JsValue, JsValue> {
        match release::held(&edge(host, store), &url).await {
            Ok(Some(held)) => typed(&held.value),
            _ => Ok(JsValue::UNDEFINED),
        }
    }

    /// The latest release at `url`, read anonymously and retained, as
    /// `{ release }`; undefined when there is none or the read failed.
    #[wasm_bindgen(js_name = releaseRefresh, unchecked_return_type = "{ release: unknown } | undefined")]
    pub async fn release_refresh(
        host: GatewayHost,
        store: StoreHost,
        url: String,
    ) -> Result<JsValue, JsValue> {
        match read(&host, &url, ANSWER_CAP)
            .await
            .as_ref()
            .map(release_member)
        {
            Some(Some(member)) => typed(
                &release::retain(&edge(host, store), &url, member)
                    .await
                    .value,
            ),
            _ => Ok(JsValue::UNDEFINED),
        }
    }

    /// The installation manager's publication for `channel` at `url`, read
    /// anonymously, admitted and retained, as `{ release }` (`releaseHeld`
    /// answers it offline); undefined when there is none or it is not exactly
    /// what the publisher writes.
    #[wasm_bindgen(js_name = installationManagerRefresh, unchecked_return_type = "{ release: unknown } | undefined")]
    pub async fn installation_manager_refresh(
        host: GatewayHost,
        store: StoreHost,
        url: String,
        channel: String,
    ) -> Result<JsValue, JsValue> {
        if !matches!(channel.as_str(), "stable" | "canary") {
            return Ok(JsValue::UNDEFINED);
        }
        match read(&host, &url, MANAGER_CAP)
            .await
            .and_then(|body| installation_manager(&body, &channel))
        {
            Some(publication) => typed(
                &release::retain(&edge(host, store), &url, publication)
                    .await
                    .value,
            ),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// The Assets catalogue this device holds for a project, or null.
    #[wasm_bindgen(js_name = assetsCatalogueHeld, unchecked_return_type = "HeldShown<AssetsCatalogue> | null")]
    pub async fn assets_catalogue_held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let held = assets_catalogue::held(&edge(host, store), &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// The project's folders and every catalogue row, retained.
    #[wasm_bindgen(js_name = assetsCatalogueRefresh, unchecked_return_type = "HeldShown<AssetsCatalogue>")]
    pub async fn assets_catalogue_refresh(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let fresh = assets_catalogue::refresh(&edge(host, store), &project).await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }

    /// The `/config` document this device holds for a project, or null.
    #[wasm_bindgen(js_name = projectConfigHeld, unchecked_return_type = "HeldShown<Record<string, unknown>> | null")]
    pub async fn project_config_held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let held = project_config::held(&edge(host, store), &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// The project's `/config` document, retained unless a save forgot the
    /// held one meanwhile (`retained_reads_stale`).
    #[wasm_bindgen(js_name = projectConfigRefresh, unchecked_return_type = "HeldShown<Record<string, unknown>>")]
    pub async fn project_config_refresh(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let fresh = project_config::refresh(&edge(host, store), &project).await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }

    /// Held-first: the held `/config` document, or one explicit read when
    /// nothing is held. The reply's observation names its actual source.
    #[wasm_bindgen(js_name = projectConfigAcquire, unchecked_return_type = "ProjectConfigurationReply")]
    pub async fn project_config_acquire(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let (shown, source) = project_config::acquire(&edge(host, store), &project)
            .await
            .map_err(|refusal| rejection(&refusal))?;
        let reply = crate::gateway_exchange::design::admit_project_configuration_reply(
            &project,
            crate::gateway_exchange::design::ProjectConfigurationRead::Acquire,
            serde_json::json!({"project": project, "document": shown.value,
                "observation": {"source": source, "saved_at_ms": shown.saved_at_ms}}),
        )
        .map_err(|refusal| rejection(&refusal))?;
        typed(&reply)
    }

    /// Forget the held `/config` document: a save changed it.
    #[wasm_bindgen(js_name = projectConfigForget)]
    pub async fn project_config_forget(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<(), JsValue> {
        project_config::forget(&edge(host, store), &project)
            .await
            .map_err(|refusal| rejection(&refusal))
    }

    /// The survey pipeline status this device holds for a project, or null.
    #[wasm_bindgen(js_name = pipelineStatusHeld, unchecked_return_type = "HeldShown<Record<string, unknown>> | null")]
    pub async fn pipeline_status_held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let held = pipeline_status::held(&edge(host, store), &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// The project's survey pipeline status, retained.
    #[wasm_bindgen(js_name = pipelineStatusRefresh, unchecked_return_type = "HeldShown<Record<string, unknown>>")]
    pub async fn pipeline_status_refresh(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let fresh = pipeline_status::refresh(&edge(host, store), &project).await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }

    /// A transformer's version records this device holds, or null.
    #[wasm_bindgen(js_name = transformerVersionsHeld, unchecked_return_type = "HeldShown<Record<string, unknown>[]> | null")]
    pub async fn transformer_versions_held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        transformer: String,
    ) -> Result<JsValue, JsValue> {
        let held = transformer_versions::held(&edge(host, store), &project, &transformer).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// A transformer's version records as the version authority admits them, retained.
    #[wasm_bindgen(js_name = transformerVersionsRefresh, unchecked_return_type = "HeldShown<Record<string, unknown>[]>")]
    pub async fn transformer_versions_refresh(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        transformer: String,
    ) -> Result<JsValue, JsValue> {
        let fresh = transformer_versions::refresh(&edge(host, store), &project, &transformer).await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }

    /// A downloaded version this device holds, `{ version, snapshot }`, or null.
    #[wasm_bindgen(js_name = transformerVersionDownloaded, unchecked_return_type = "HeldShown<{ version: Record<string, unknown>; snapshot: unknown }> | null")]
    pub async fn transformer_version_downloaded(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        transformer: String,
        version: String,
    ) -> Result<JsValue, JsValue> {
        let held =
            transformer_versions::downloaded(&edge(host, store), &project, &transformer, &version)
                .await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// Download one governed version for playback offline, retained.
    #[wasm_bindgen(js_name = transformerVersionDownload, unchecked_return_type = "HeldShown<{ version: Record<string, unknown>; snapshot: unknown }>")]
    pub async fn transformer_version_download(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        transformer: String,
        version: String,
    ) -> Result<JsValue, JsValue> {
        let fresh =
            transformer_versions::download(&edge(host, store), &project, &transformer, &version)
                .await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }

    /// How many downloaded versions this device holds for a project.
    #[wasm_bindgen(js_name = transformerVersionDownloads)]
    pub async fn transformer_version_downloads(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<f64, JsValue> {
        let count = transformer_versions::downloads(&edge(host, store), &project).await;
        count
            .map(|count| count as f64)
            .map_err(|refusal| rejection(&refusal))
    }

    /// Forget every downloaded version this device holds for a project.
    #[wasm_bindgen(js_name = transformerVersionDownloadsForget)]
    pub async fn transformer_version_downloads_forget(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<(), JsValue> {
        transformer_versions::forget_downloads(&edge(host, store), &project)
            .await
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "held_reads_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "held_reads_acquire_tests.rs"]
mod acquire_tests;
