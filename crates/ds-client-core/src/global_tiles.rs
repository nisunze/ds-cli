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

/// One BigQuery table a publication is built from, as the catalog names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub gcp_project: String,
    pub dataset: String,
    pub table: String,
}

impl Source {
    /// Parse `project.dataset.table` exactly as the catalog prints it.
    ///
    /// The shape is the backend's own: a GCP project id, then two BigQuery
    /// identifiers. Anything else is refused here rather than sent, because a
    /// malformed source would be discovered only after a publication job had
    /// started.
    pub fn parse(value: &str) -> Result<Self, ClientError> {
        let parts: Vec<&str> = value.split('.').collect();
        let [project, dataset, table] = parts[..] else {
            return Err(invalid());
        };
        let gcp = project.len() >= 6
            && project.len() <= 63
            && project.starts_with(|c: char| c.is_ascii_lowercase())
            && project
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        let identifier = |value: &str| {
            !value.is_empty()
                && value.len() <= 1024
                && value.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if !gcp || !identifier(dataset) || !identifier(table) {
            return Err(invalid());
        }
        Ok(Self {
            gcp_project: project.to_owned(),
            dataset: dataset.to_owned(),
            table: table.to_owned(),
        })
    }
}

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
    /// Publish a new reference archive from exact catalog sources.
    Generate {
        domain: Domain,
        name: String,
        country: String,
        sources: Vec<Source>,
        maxzoom: u8,
    },
    /// One publication job's state, by its durable tile id.
    Status { tile_id: String },
}

/// The bound the desktop adapter enforced, kept here so the answer does not
/// depend on which caller asked.
pub const MAX_SOURCES: usize = 50;
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

impl Command {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Catalog { .. } => "global_catalog",
            Self::List { .. } => "global_list",
            Self::Generate { .. } => "global_generate",
            Self::Status { .. } => "global_status",
        }
    }

    pub const fn writes(&self) -> bool {
        matches!(self, Self::Generate { .. })
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
            Self::Generate {
                name,
                country,
                sources,
                maxzoom,
                ..
            } => {
                text(name, 200)
                    && text(country, 200)
                    && (1..=MAX_SOURCES).contains(&sources.len())
                    && (1..=22).contains(maxzoom)
                    // Two sources whose table names collide would publish two
                    // layers under one name, so the publication is refused
                    // before it starts rather than after.
                    && {
                        let mut names: Vec<&str> =
                            sources.iter().map(|source| source.table.as_str()).collect();
                        names.sort_unstable();
                        let total = names.len();
                        names.dedup();
                        names.len() == total
                    }
            }
            Self::Status { tile_id } => text(tile_id, 200),
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
            Self::Generate {
                domain,
                name,
                country,
                sources,
                maxzoom,
            } => {
                // The layer name is the source table's, which is why two
                // sources may not share one. `basezoom` follows `maxzoom` and
                // the lower bounds are the publication defaults the desktop
                // sent; they are stated here rather than left to the backend.
                let layers: Vec<Value> = sources
                    .iter()
                    .map(|source| {
                        json!({
                            "gcp_project": source.gcp_project,
                            "dataset": source.dataset,
                            "table": source.table,
                            "layer_name": source.table,
                        })
                    })
                    .collect();
                json!({
                    "action": action,
                    "domain": domain.token(),
                    "name": name,
                    "country": country,
                    "layers": layers,
                    "minzoom": 0,
                    "maxzoom": maxzoom,
                    "basezoom": maxzoom,
                    "point_minzoom": 0,
                    "drop_densest_as_needed": false,
                })
            }
            Self::Status { tile_id } => json!({ "action": action, "tile_id": tile_id }),
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
mod tests {
    use super::*;

    #[test]
    fn a_source_is_an_exact_catalog_identity() {
        let source = Source::parse("engineering-data-solutions.reference.rw_roads")
            .expect("a catalog identity parses");
        assert_eq!(source.gcp_project, "engineering-data-solutions");
        assert_eq!(source.dataset, "reference");
        assert_eq!(source.table, "rw_roads");
        for bad in [
            "",
            "one.two",
            "one.two.three.four",
            "Upper.two.three",
            "-leading.two.three",
            "short.two.three",
            "engineering-data-solutions..three",
            "engineering-data-solutions.2digit.three",
            "engineering-data-solutions.two.3digit",
        ] {
            assert!(Source::parse(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_publication_names_each_layer_once() {
        let source = |table: &str| Source {
            gcp_project: "engineering-data-solutions".into(),
            dataset: "reference".into(),
            table: table.into(),
        };
        let generate = |sources: Vec<Source>| Command::Generate {
            domain: Domain::NetworkTemplate,
            name: "Rwanda roads".into(),
            country: "RW".into(),
            sources,
            maxzoom: 12,
        };
        assert!(generate(vec![source("roads"), source("rivers")])
            .validate()
            .is_ok());
        assert!(
            generate(vec![source("roads"), source("roads")])
                .validate()
                .is_err(),
            "two sources published one layer name"
        );
        assert!(generate(vec![]).validate().is_err(), "an empty publication");
        assert!(
            generate(
                (0..=MAX_SOURCES)
                    .map(|n| source(&format!("t{n}")))
                    .collect()
            )
            .validate()
            .is_err(),
            "a publication past its bound"
        );
    }

    #[test]
    fn every_action_sends_the_keys_the_backend_reads() {
        let catalog = Command::Catalog {
            domain: Domain::SolarTemplate,
            search: Some("roads".into()),
            page_size: 50,
            page_token: None,
        };
        let payload = catalog.payload();
        assert_eq!(payload["action"], "global_catalog");
        assert_eq!(payload["domain"], "solar_template");
        assert_eq!(payload["gcp_project"], DEFAULT_GCP_PROJECT);
        assert_eq!(payload["page_size"], 50);
        assert_eq!(payload["search"], "roads");
        assert!(payload["page_token"].is_null());

        let generate = Command::Generate {
            domain: Domain::NetworkTemplate,
            name: "Rwanda roads".into(),
            country: "RW".into(),
            sources: vec![Source::parse("engineering-data-solutions.reference.rw_roads").unwrap()],
            maxzoom: 12,
        };
        let payload = generate.payload();
        assert_eq!(payload["action"], "global_generate");
        assert_eq!(payload["layers"][0]["layer_name"], "rw_roads");
        assert_eq!(payload["layers"][0]["dataset"], "reference");
        assert_eq!(payload["maxzoom"], 12);
        assert_eq!(payload["basezoom"], 12, "basezoom follows maxzoom");
        assert_eq!(payload["minzoom"], 0);
        assert_eq!(payload["drop_densest_as_needed"], false);

        // A global action never carries a project: the catalog has none, and a
        // project id on this wire would be a fence nothing enforces.
        for command in [
            &catalog,
            &generate,
            &Command::List {
                domain: Domain::NetworkTemplate,
            },
            &Command::Status {
                tile_id: "tile-1".into(),
            },
        ] {
            let payload = command.payload();
            let object = payload.as_object().expect("payload is an object");
            assert!(
                !object.contains_key("eds_project_id") && !object.contains_key("project_id"),
                "{} carries a project",
                command.action()
            );
        }
    }

    #[test]
    fn only_publishing_writes() {
        assert!(!Command::List {
            domain: Domain::NetworkTemplate
        }
        .writes());
        assert!(!Command::Status {
            tile_id: "t".into()
        }
        .writes());
        assert!(Command::Generate {
            domain: Domain::NetworkTemplate,
            name: "n".into(),
            country: "RW".into(),
            sources: vec![],
            maxzoom: 1,
        }
        .writes());
    }
}
