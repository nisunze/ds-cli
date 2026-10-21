//! Durable media links for exported reports: Brain grants one project's
//! media tree to the signed-in account for months, and an export derives
//! every photo link under it. A grant is held per project for offline
//! exports and renewed a day before it lapses.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{data, Admitted, Exchange, Plan, Refusal, Request};
#[cfg(feature = "durable-store")]
use {
    super::retained::{self, Edge},
    crate::retained_reads::Kind,
};

/// Renew a held grant this long before it lapses, so a long export never
/// starts with an authority that expires mid-run.
const RENEW_BEFORE_MS: i128 = 24 * 60 * 60 * 1000;
pub const INVALID: &str = "media_grant_invalid";

/// One project's media tree, granted for durable report links.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct MediaGrant {
    /// Where the links resolve: Brain's own public origin. Brain writes
    /// `resolver_origin`; the page and the held copy read `resolverOrigin`.
    #[serde(alias = "resolver_origin")]
    pub resolver_origin: String,
    pub bucket: String,
    /// The object prefix every derived link lies under.
    pub prefix: String,
    pub token: String,
    /// RFC 3339.
    #[serde(alias = "expires_at")]
    pub expires_at: String,
}

impl MediaGrant {
    fn expires_ms(&self) -> Option<i128> {
        OffsetDateTime::parse(&self.expires_at, &Rfc3339)
            .ok()
            .map(|at| at.unix_timestamp_nanos() / 1_000_000)
    }

    /// Whether the grant still holds `margin_ms` after `now_ms`.
    fn lives(&self, now_ms: u64, margin_ms: i128) -> bool {
        self.expires_ms()
            .is_some_and(|expires| expires - margin_ms > i128::from(now_ms))
    }

    fn usable(&self) -> bool {
        self.resolver_origin.starts_with("https://")
            && !self.bucket.is_empty()
            && !self.prefix.is_empty()
            && self.token.split('.').count() == 2
            && self.expires_ms().is_some()
    }

    /// Whether this grant covers `project`'s own media tree.
    fn covers(&self, project: &str) -> bool {
        let prefix = self.prefix.trim();
        prefix == format!("projects/{project}/") || prefix == format!("{project}/")
    }
}

/// The grant of one project's media tree, read from Brain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaGrantFetch {
    pub project: String,
}

impl Exchange for MediaGrantFetch {
    type Output = MediaGrant;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"action": "media_scope", "eds_project_id": self.project});
        Ok(Request::post("/report", body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let grants: Vec<MediaGrant> = answer
            .get("grants")
            .and_then(|grants| Vec::<MediaGrant>::deserialize(grants).ok())
            .unwrap_or_default();
        grants
            .into_iter()
            .find(|grant| grant.covers(&self.project))
            .filter(MediaGrant::usable)
            .map(Admitted::Done)
            .ok_or_else(|| {
                Refusal::new(
                    INVALID,
                    "Brain returned no usable media scope for this project.",
                )
            })
    }
}

/// The grant this device holds for `project`, while it has not lapsed; no
/// transport.
#[cfg(feature = "durable-store")]
pub async fn held(edge: &impl Edge, project: &str) -> Result<Option<MediaGrant>, Refusal> {
    let observed = retained::read::<MediaGrant>(edge, Kind::MediaGrant, None, project).await?;
    let now = edge.now_ms();
    Ok(observed
        .held
        .map(|held| held.value)
        .filter(|grant| grant.lives(now, 0)))
}

/// A grant for `project` that holds for at least another day: the held one,
/// or a new one from Brain, then held under the account its read observed.
#[cfg(feature = "durable-store")]
pub async fn grant(edge: &impl Edge, project: &str) -> Result<MediaGrant, Refusal> {
    let observed = retained::read::<MediaGrant>(edge, Kind::MediaGrant, None, project)
        .await
        .ok();
    let now = edge.now_ms();
    if let Some(held) = observed
        .as_ref()
        .and_then(|observed| observed.held.as_ref())
        .map(|held| &held.value)
        .filter(|grant| grant.lives(now, RENEW_BEFORE_MS))
    {
        return Ok(held.clone());
    }
    let fresh = edge
        .fetch(MediaGrantFetch {
            project: project.into(),
        })
        .await?;
    if let Some(observed) = observed {
        let owner = observed.owner.as_deref();
        let generation = Some(observed.generation);
        let kind = Kind::MediaGrant;
        let _ = retained::write(edge, owner, kind, None, project, &fresh, generation).await;
    }
    Ok(fresh)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use wasm_bindgen::prelude::*;

    use super::{grant, held};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;

    /// The media grant this device holds for `project`, unexpired; no
    /// transport.
    #[wasm_bindgen(js_name = mediaGrantHeld, unchecked_return_type = "MediaGrant | undefined")]
    pub async fn media_grant_held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(
            &held(&edge, &project)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// A media grant for `project` that holds for at least another day.
    #[wasm_bindgen(js_name = mediaGrant, unchecked_return_type = "MediaGrant")]
    pub async fn media_grant(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(
            &grant(&edge, &project)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }
}

#[cfg(test)]
#[path = "media_grants_tests.rs"]
mod tests;
