//! The project's known-column visibility — which fields of which layer the
//! tables show — on `/config/{project}/known-columns`.
//!
//! Until 2026-09-20 `ds design known-columns list|set` relayed through the
//! paired desktop to `getProjectKnownColumns` / `setProjectKnownColumn`
//! (ds-web `api/design.ts`). ds-brain's `ConfigHandler` authenticates from
//! the bearer and gates the write on project config edit; the window was
//! habit. One `GET` reads the revisioned document; one `PATCH` sets one
//! field's visibility against the revision it was read at.

use serde_json::{json, Value};

use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, KnownColumnsCall,
};

fn id(s: &str) -> bool {
    !s.is_empty()
        && s.trim() == s
        && s.chars().count() <= 128
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// The document: `authority`, `revision` and `columns`.
    List,
    /// One field of one layer, shown or hidden, against the revision read.
    Set {
        layer: String,
        field: String,
        visible: bool,
        expected_revision: i64,
    },
}

impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        let ok = id(project)
            && match self {
                Self::List => true,
                Self::Set {
                    layer,
                    field,
                    expected_revision,
                    ..
                } => id(layer) && id(field) && *expected_revision >= 0,
            };
        if ok {
            Ok(())
        } else {
            Err(ClientError::new(
                ErrorKind::InvalidInput,
                "invalid known-columns request: a layer, a field or a revision is outside its bound",
            ))
        }
    }
}

fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "known columns did not answer in a shape this build understands",
    )
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate(project)?;
    let patch = match command {
        Command::List => None,
        Command::Set {
            layer,
            field,
            visible,
            expected_revision,
        } => Some(json!({
            "layer": layer,
            "field": field,
            "visible": visible,
            "expected_revision": expected_revision,
        })),
    };
    let response = transport.known_columns(KnownColumnsCall {
        profile,
        project,
        patch: patch.as_ref(),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        // The handler answers `{"detail": …}`; the gateway answers its own
        // 404 when the route is unpublished, which is a different fact.
        let service_authored = serde_json::from_slice::<Value>(&response.body)
            .ok()
            .is_some_and(|body| body.get("detail").is_some() || body.get("success").is_some());
        if response.status == 404 && !service_authored {
            return Err(ClientError::new(
                ErrorKind::RouteUnavailable,
                "this deployment does not serve the known-columns route",
            ));
        }
        let body: Value = serde_json::from_slice(&response.body).unwrap_or_default();
        let sentence = body["detail"]
            .as_str()
            .or_else(|| body["error"]["message"].as_str());
        let kind = match response.status {
            404 => ErrorKind::ResourceNotFound,
            409 | 412 => ErrorKind::InvalidInput,
            _ => classify_tile_error(response.status).kind(),
        };
        return Err(ClientError::with_service_refusal(
            kind,
            "known columns refused the request",
            crate::ServiceRefusal::new(response.status, None, sentence),
        ));
    }
    if response.body.len() > 4 * 1024 * 1024 {
        return Err(bad());
    }
    let document: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if !document.is_object() || document.get("revision").is_none() {
        return Err(bad());
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_is_the_browsers_and_a_bound_is_refused_first() {
        let set = Command::Set {
            layer: "poles".into(),
            field: "height_m".into(),
            visible: false,
            expected_revision: 4,
        };
        assert!(set.validate("p").is_ok());
        assert_eq!(
            Command::Set {
                layer: "".into(),
                field: "x".into(),
                visible: true,
                expected_revision: 0
            }
            .validate("p")
            .expect_err("refused")
            .kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            Command::List.validate("").expect_err("refused").kind(),
            ErrorKind::InvalidInput
        );
    }
}
