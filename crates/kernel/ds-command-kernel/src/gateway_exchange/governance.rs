//! Platform governance: the role definitions and user registry the
//! governance page shows, and the three changes it makes to one account.
//! Brain decides who may read the registry and make each change.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};
#[cfg(feature = "durable-store")]
use {
    super::retained::{self, Edge},
    crate::retained_reads::Kind,
};

const PATH: &str = "/api/v1/governance";
/// Role definitions and a platform's user registry.
const PAGE_CAP: u64 = 4 * 1024 * 1024;
/// Removing an account from every project runs in batches on Brain.
const REMOVAL_TIMEOUT_MS: u64 = 120_000;
pub const INVALID: &str = "governance_answer_invalid";

/// Go writes an empty list as `null`.
fn list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(input: D) -> Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(input)?.unwrap_or_default())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct RoleDef {
    pub name: String,
    pub description: String,
    #[serde(deserialize_with = "list")]
    pub capabilities: Vec<String>,
    /// Platform roles only: the screens the role opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_access: Option<Vec<String>>,
    pub rank: i64,
    #[serde(deserialize_with = "list")]
    pub grantable_by: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct UserSummary {
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photo_url: Option<String>,
    #[serde(deserialize_with = "list")]
    pub roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sign_in: Option<String>,
    pub login_count: u64,
    pub is_active: bool,
    /// `active`, `suspended` or `blocked`.
    pub account_status: String,
}

/// Everything the governance page shows. `users` is empty unless the caller
/// manages users.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GovernancePage {
    #[serde(deserialize_with = "list")]
    pub app_role_defs: Vec<RoleDef>,
    #[serde(deserialize_with = "list")]
    pub project_role_defs: Vec<RoleDef>,
    #[serde(deserialize_with = "list")]
    pub users: Vec<UserSummary>,
}

/// What removing an account from every project changed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct RemovedFromProjects {
    pub email: String,
    pub matched_project_count: u64,
    pub removed_project_count: u64,
    #[serde(deserialize_with = "list")]
    pub project_ids: Vec<String>,
}

/// An account's access after a change.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct AccessState {
    pub email: String,
    pub account_status: String,
    pub is_active: bool,
}

fn answer<C: Exchange>(body: &Value) -> Result<Admitted<C>, Refusal>
where
    C::Output: for<'de> Deserialize<'de>,
{
    C::Output::deserialize(data(body))
        .map(Admitted::Done)
        .map_err(|_| {
            Refusal::new(
                INVALID,
                "The governance service answered an invalid response.",
            )
        })
}

fn post(action: &str, data: Value) -> Request {
    Request::post(PATH, json!({"action": action, "data": data}))
}

/// The governance page, as Brain answers it to this caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageData;

impl Exchange for PageData {
    type Output = GovernancePage;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(post("page_data", json!({})).response_cap(PAGE_CAP).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        answer(&body)
    }
}

/// Give one account its platform roles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetAppRoles {
    pub email: String,
    pub roles: Vec<String>,
}

impl Exchange for SetAppRoles {
    type Output = ();

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(post(
            "set_app_roles",
            json!({"email": self.email, "roles": self.roles}),
        )
        .into())
    }

    fn admit(self, _status: u16, _body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(()))
    }
}

/// Remove an account from every project; its account and platform roles
/// stay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveFromAllProjects {
    pub email: String,
}

impl Exchange for RemoveFromAllProjects {
    type Output = RemovedFromProjects;

    fn plan(&self) -> Result<Plan, Refusal> {
        let request = post(
            "remove_user_from_all_projects",
            json!({"email": self.email}),
        );
        Ok(request.timeout_ms(REMOVAL_TIMEOUT_MS).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        answer(&body)
    }
}

/// Change an account's login access without deleting it, its roles or its
/// memberships.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetAccessState {
    pub email: String,
    pub account_status: String,
    pub reason: String,
}

impl Exchange for SetAccessState {
    type Output = AccessState;

    fn plan(&self) -> Result<Plan, Refusal> {
        let data = json!({"email": self.email, "account_status": self.account_status,
            "reason": self.reason});
        Ok(post("set_user_access_state", data).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        answer(&body)
    }
}

/// The page this device holds, with no transport.
#[cfg(feature = "durable-store")]
pub async fn held(edge: &impl Edge) -> Result<Option<GovernancePage>, Refusal> {
    let observed = retained::read(edge, Kind::GovernancePage, None, "").await?;
    Ok(observed.held.map(|held| held.value))
}

/// The page from Brain, retained under the account its read observed.
/// Retention is a cache: a store that cannot keep the page never hides it.
#[cfg(feature = "durable-store")]
pub async fn refresh(edge: &impl Edge) -> Result<GovernancePage, Refusal> {
    let observed = retained::read::<GovernancePage>(edge, Kind::GovernancePage, None, "")
        .await
        .ok();
    let page = edge.fetch(PageData).await?;
    if let Some(observed) = observed {
        let owner = observed.owner.as_deref();
        let generation = Some(observed.generation);
        let kind = Kind::GovernancePage;
        let _ = retained::write(edge, owner, kind, None, "", &page, generation).await;
    }
    Ok(page)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{held, refresh, RemoveFromAllProjects, SetAccessState, SetAppRoles};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    /// The page this device holds, at once and with no transport.
    #[wasm_bindgen(
        js_name = governanceHeldPage,
        unchecked_return_type = "GovernancePage | undefined"
    )]
    pub async fn governance_held_page(
        host: GatewayHost,
        store: StoreHost,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(&held(&edge).await.map_err(|refusal| rejection(&refusal))?)
    }

    /// The role definitions, and the user registry when the caller manages
    /// users, read from Brain and held for the next open.
    #[wasm_bindgen(js_name = governancePage, unchecked_return_type = "GovernancePage")]
    pub async fn governance_page(host: GatewayHost, store: StoreHost) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(
            &refresh(&edge)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// Give one account its platform roles.
    #[wasm_bindgen(js_name = governanceSetAppRoles)]
    pub async fn governance_set_app_roles(
        host: GatewayHost,
        email: String,
        roles: Vec<String>,
    ) -> Result<(), JsValue> {
        run(&host, SetAppRoles { email, roles }).await
    }

    /// Remove one account from every project.
    #[wasm_bindgen(
        js_name = governanceRemoveFromAllProjects,
        unchecked_return_type = "RemovedFromProjects"
    )]
    pub async fn governance_remove_from_all_projects(
        host: GatewayHost,
        email: String,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, RemoveFromAllProjects { email }).await?)
    }

    /// Set one account's access: `active`, `suspended` or `blocked`.
    #[wasm_bindgen(js_name = governanceSetAccessState, unchecked_return_type = "AccessState")]
    pub async fn governance_set_access_state(
        host: GatewayHost,
        email: String,
        account_status: String,
        reason: String,
    ) -> Result<JsValue, JsValue> {
        let call = SetAccessState {
            email,
            account_status,
            reason,
        };
        typed(&run(&host, call).await?)
    }
}

#[cfg(test)]
#[path = "governance_tests.rs"]
mod tests;
