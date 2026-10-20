//! Held-first reads a person sees: the platform's transformer delete ledger,
//! a lane's latest release and a project's Assets catalogue. What this device holds answers at once and
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
    let _ = retained::write(edge, owner.as_deref(), kind, project, key, &value, None).await;
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
    if revoked(&refusal) {
        let _ = retained::forget(edge, owner.as_deref(), kind, project, Some(key)).await;
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

    pub async fn refresh(edge: &impl Edge, days: u32, limit: Option<u32>) -> Result<Shown, Refusal> {
        let call = DeletedTransformers { days: Some(days), limit };
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
        super::retain(edge, owner, KIND, None, &key(url), json!({ "release": member })).await
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
        let folders = match edge.fetch(Tree { project: project.into() }).await {
            Ok(folders) => folders,
            Err(refusal) => return Err(refused(edge, owner, KIND, Some(project), "", refusal).await),
        };
        let mut assets = Vec::new();
        let mut cursor = None;
        let mut complete = false;
        for _ in 0..PAGES_MAX {
            let query = ListQuery { limit: Some(PAGE_ROWS), cursor: cursor.take(), ..ListQuery::default() };
            let page = match edge.fetch(List { project: project.into(), query }).await {
                Ok(page) => page,
                Err(refusal) => return Err(refused(edge, owner, KIND, Some(project), "", refusal).await),
            };
            assets.extend(page.assets);
            match page.next_cursor.filter(|next| page.has_more && !next.is_empty()) {
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

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{assets_catalogue, backup_ledger, release};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::releases::{exports::read as read_release, release_member};
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
    pub async fn backup_ledger_held(host: GatewayHost, store: StoreHost, days: u32) -> Result<JsValue, JsValue> {
        let held = backup_ledger::held(&edge(host, store), days).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// One window of the delete ledger, newest deletion first, retained.
    #[wasm_bindgen(js_name = backupLedgerRefresh, unchecked_return_type = "HeldShown<{ rows: Record<string, unknown>[]; count: number }>")]
    pub async fn backup_ledger_refresh(host: GatewayHost, store: StoreHost, days: u32, limit: Option<u32>) -> Result<JsValue, JsValue> {
        let fresh = backup_ledger::refresh(&edge(host, store), days, limit).await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }

    /// The release this device holds for a release URL, as `{ release }`, or undefined.
    #[wasm_bindgen(js_name = releaseHeld, unchecked_return_type = "{ release: unknown } | undefined")]
    pub async fn release_held(host: GatewayHost, store: StoreHost, url: String) -> Result<JsValue, JsValue> {
        match release::held(&edge(host, store), &url).await {
            Ok(Some(held)) => typed(&held.value),
            _ => Ok(JsValue::UNDEFINED),
        }
    }

    /// The latest release at `url`, read anonymously and retained, as
    /// `{ release }`; undefined when there is none or the read failed.
    #[wasm_bindgen(js_name = releaseRefresh, unchecked_return_type = "{ release: unknown } | undefined")]
    pub async fn release_refresh(host: GatewayHost, store: StoreHost, url: String) -> Result<JsValue, JsValue> {
        match read_release(&url).await.as_ref().map(release_member) {
            Some(Some(member)) => typed(&release::retain(&edge(host, store), &url, member).await.value),
            _ => Ok(JsValue::UNDEFINED),
        }
    }

    /// The Assets catalogue this device holds for a project, or null.
    #[wasm_bindgen(js_name = assetsCatalogueHeld, unchecked_return_type = "HeldShown<AssetsCatalogue> | null")]
    pub async fn assets_catalogue_held(host: GatewayHost, store: StoreHost, project: String) -> Result<JsValue, JsValue> {
        let held = assets_catalogue::held(&edge(host, store), &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// The project's folders and every catalogue row, retained.
    #[wasm_bindgen(js_name = assetsCatalogueRefresh, unchecked_return_type = "HeldShown<AssetsCatalogue>")]
    pub async fn assets_catalogue_refresh(host: GatewayHost, store: StoreHost, project: String) -> Result<JsValue, JsValue> {
        let fresh = assets_catalogue::refresh(&edge(host, store), &project).await;
        typed(&fresh.map_err(|refusal| rejection(&refusal))?)
    }
}

#[cfg(test)]
#[path = "held_reads_tests.rs"]
mod tests;
