//! The product-wide reference publications, headlessly.
//!
//! These are the **global** tiles: one governed catalog of reference layers for
//! the whole product, gated by `global_tiles.manage` at the gateway. They share
//! `/api/v1/tiles` with a project's own tiles and nothing else: no project id,
//! no project fence, no saved selection. A caller that names a project here has
//! made a mistake this owner cannot commit, because the wire has nowhere to put
//! one.
//!
//! Until 2026-09-18 `ds tile global …` reached them through the paired desktop,
//! which held the same signed-in user and posted the same bodies to the same
//! path. The window was never part of the contract, only of the route.

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    GlobalTilesCall,
};
use serde_json::{json, Value};

/// The two domains a reference publication may belong to. Closed, because a
/// domain outside this pair is a publication nothing in the product reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    NetworkTemplate,
    SolarTemplate,
}

impl Domain {
    pub const fn token(self) -> &'static str {
        match self {
            Self::NetworkTemplate => "network_template",
            Self::SolarTemplate => "solar_template",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ClientError> {
        match value {
            "network_template" => Ok(Self::NetworkTemplate),
            "solar_template" => Ok(Self::SolarTemplate),
            _ => Err(invalid()),
        }
    }
}

/// Who may mount a reference publication. Closed, because the backend reads
/// exactly these two modes and treats every other word as "everyone" — a
/// spelling mistake would silently publish a private layer to the product.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Everyone,
    Restricted,
}

impl Visibility {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Everyone => "all",
            Self::Restricted => "restricted",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ClientError> {
        match value {
            "all" => Ok(Self::Everyone),
            "restricted" => Ok(Self::Restricted),
            _ => Err(invalid()),
        }
    }
}

/// Who a restricted publication stays visible to.
///
/// Four allowlists, exactly as the catalog stores them: signed-in addresses,
/// application roles, project roles and DS projects. A restricted publication
/// with four empty lists is still readable by a system administrator, which is
/// what makes this the non-destructive lever: the publication and its archive
/// survive, and nobody else mounts it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allowlists {
    pub users: Vec<String>,
    pub app_roles: Vec<String>,
    pub project_roles: Vec<String>,
    pub projects: Vec<String>,
}

impl Allowlists {
    fn is_empty(&self) -> bool {
        self.users.is_empty()
            && self.app_roles.is_empty()
            && self.project_roles.is_empty()
            && self.projects.is_empty()
    }
    fn lists(&self) -> [&Vec<String>; 4] {
        [
            &self.users,
            &self.app_roles,
            &self.project_roles,
            &self.projects,
        ]
    }
}

/// The bound on one allowlist. Beyond it an access policy is a membership
/// system, and membership is the gateway's job, not a tile document's.
pub const MAX_ALLOWED: usize = 50;

/// One governed action on the global reference catalog.
pub enum Command {
    /// Discover eligible geographic sources, one bounded page at a time.
    Catalog {
        domain: Domain,
        search: Option<String>,
        page_size: u16,
        page_token: Option<String>,
    },
    /// The active publications of one domain.
    List { domain: Domain },
    // Publishing a new reference archive was hosted compute and is retired
    // (development philosophy §2). It returns with an edge owner, not here.
    /// One publication job's state, by its durable tile id.
    Status { tile_id: String },
    /// Who may mount one publication, without touching its archive.
    Access {
        tile_id: String,
        visibility: Visibility,
        allowed: Allowlists,
    },
}

/// The bound the desktop adapter enforced, kept here so the answer does not
/// depend on which caller asked.
pub const MAX_PAGE_SIZE: u16 = 200;

fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "global reference publication selection violates its bounded contract",
    )
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "global reference publication response violates its envelope contract",
    )
}

fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.trim() == value
}

/// One allowlist: bounded, every entry a single token, and no entry twice — a
/// duplicate is collapsed by the catalog, so accepting it here would report a
/// policy the product does not hold.
fn bounded_allowlist(values: &[String]) -> bool {
    if values.len() > MAX_ALLOWED {
        return false;
    }
    let mut seen: Vec<String> = Vec::with_capacity(values.len());
    for value in values {
        if !text(value, 200) || value.chars().any(char::is_whitespace) {
            return false;
        }
        let key = value.to_lowercase();
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
    }
    true
}

impl Command {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Catalog { .. } => "global_catalog",
            Self::List { .. } => "global_list",
            Self::Status { .. } => "global_status",
            Self::Access { .. } => "global_access",
        }
    }

    pub const fn writes(&self) -> bool {
        matches!(self, Self::Access { .. })
    }

    pub fn validate(&self) -> Result<(), ClientError> {
        let ok = match self {
            Self::Catalog {
                search,
                page_size,
                page_token,
                ..
            } => {
                (1..=MAX_PAGE_SIZE).contains(page_size)
                    && search.as_deref().is_none_or(|value| text(value, 200))
                    && page_token.as_deref().is_none_or(|value| text(value, 2048))
            }
            Self::List { .. } => true,
            Self::Status { tile_id } => text(tile_id, 200),
            Self::Access {
                tile_id,
                visibility,
                allowed,
            } => {
                text(tile_id, 200)
                    && allowed.lists().iter().all(|list| bounded_allowlist(list))
                    && allowed
                        .users
                        .iter()
                        // The catalog matches a user by the address it signed
                        // in with, so a bare name would be an allowlist entry
                        // that can never match anybody.
                        .all(|user| user.contains('@'))
                    // An allowlist under "everyone" is recorded and never
                    // read: the caller would believe it had restricted a
                    // publication that the whole product still mounts.
                    && (*visibility == Visibility::Restricted || allowed.is_empty())
            }
        };
        if ok {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    fn payload(&self) -> Value {
        let action = self.action();
        match self {
            Self::Catalog {
                domain,
                search,
                page_size,
                page_token,
            } => {
                let mut body = json!({
                    "action": action,
                    "domain": domain.token(),
                    "gcp_project": DEFAULT_GCP_PROJECT,
                    "page_size": page_size,
                });
                if let Some(search) = search {
                    body["search"] = json!(search);
                }
                if let Some(token) = page_token {
                    body["page_token"] = json!(token);
                }
                body
            }
            Self::List { domain } => json!({ "action": action, "domain": domain.token() }),
            Self::Status { tile_id } => json!({ "action": action, "tile_id": tile_id }),
            Self::Access {
                tile_id,
                visibility,
                allowed,
            } => json!({
                "action": action,
                "tile_id": tile_id,
                // Every list is sent, empty included: a key the backend does
                // not receive keeps its stored value, so an omitted list
                // would silently preserve the grant the caller just dropped.
                "access_policy": {
                    "mode": visibility.token(),
                    "users": allowed.users,
                    "app_roles": allowed.app_roles,
                    "project_roles": allowed.project_roles,
                    "projects": allowed.projects,
                },
            }),
        }
    }
}

/// The catalog's own GCP project. One value, sent explicitly, because the
/// backend treats an absent one as a different query.
const DEFAULT_GCP_PROJECT: &str = "engineering-data-solutions";

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate()?;
    let response = transport.global_tiles(GlobalTilesCall {
        profile,
        authority: authority.call(),
        payload: command.payload(),
    })?;
    match response.status {
        200 => {}
        404 => {
            return Err(ClientError::new(
                ErrorKind::ResourceNotFound,
                "no governed reference publication carries this id",
            ));
        }
        400 | 409 => {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "the reference catalog refused this selection",
            ));
        }
        408 | 425 | 429 | 500..=599 => {
            return Err(ClientError::new(
                ErrorKind::Transient,
                "the reference publication service is temporarily unavailable",
            ));
        }
        _ => {
            return Err(ClientError::new(
                ErrorKind::AuthenticationRejected,
                "the reference catalog refused the verified user; publishing needs global_tiles.manage",
            ));
        }
    }
    if response.body.len() > crate::transport::TILE_RESPONSE_LIMIT {
        return Err(unreadable());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if body["success"] != true {
        return Err(unreadable());
    }
    body.get("data")
        .cloned()
        .filter(|data| !data.is_null())
        .ok_or_else(unreadable)
}

#[cfg(test)]
#[path = "global_tiles_tests.rs"]
mod tests;
