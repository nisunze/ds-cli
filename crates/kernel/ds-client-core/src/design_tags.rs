//! Read-only tag vocabulary and exact digest-pinned projection under project membership.
use crate::{
    authorized::{classify_tile_error, service_authored, Authority},
    AuthorizedTransport, ClientError, ClientProfile, DesignTagsCall, ErrorKind,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
pub enum Command {
    Definitions,
    Batch {
        group: String,
        entries: Vec<(String, String)>,
        digest: Option<String>,
    },
    Projection {
        transformers: Vec<String>,
        definitions: Vec<String>,
    },
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid explicit design tag projection scope",
    )
}
fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "design tag projection identity or byte digest failed verification",
    )
}
impl Command {
    /// Whether this request names at least one tag definition by id, so a
    /// service-authored 404 can only be about that definition.
    fn names_definition(&self) -> bool {
        match self {
            Self::Definitions => false,
            Self::Batch { .. } => true,
            Self::Projection { definitions, .. } => !definitions.is_empty(),
        }
    }
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !id(project) {
            return Err(invalid());
        }
        if let Self::Projection {
            transformers,
            definitions,
        } = self
        {
            for (rows, max) in [(transformers, 2000), (definitions, 64)] {
                if rows.len() > max
                    || rows.iter().any(|s| !id(s))
                    || rows.iter().collect::<std::collections::BTreeSet<_>>().len() != rows.len()
                {
                    return Err(invalid());
                }
            }
            if transformers.is_empty() {
                return Err(invalid());
            }
        }
        if let Self::Batch {
            group,
            entries,
            digest,
        } = self
        {
            if !id(group)
                || entries.is_empty()
                || entries.len() > 200
                || entries.iter().any(|(name, value)| !id(name) || !id(value))
                || entries
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != entries.len()
                || digest.as_ref().is_some_and(|d| {
                    d.len() != 64
                        || !d
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate(project)?;
    let payload = match command {
        Command::Batch {
            group,
            entries,
            digest,
        } => {
            json!({"action":if digest.is_some() {"apply_tag_group"} else {"preview_tag_group"},"project_id":project,"group_batch":{"group":group,"entries":entries.iter().map(|(name,value)|json!({"transformer":name,"value":value})).collect::<Vec<_>>(),"expected_plan_digest":digest.as_deref().unwrap_or("")}})
        }
        Command::Definitions => {
            json!({"action":"list_tag_definitions","project_id":project,"include_archived":false})
        }
        Command::Projection {
            transformers,
            definitions,
        } => {
            json!({"action":"export_tag_projection","project_id":project,"transformers":transformers,"definition_ids":definitions})
        }
    };
    let response = transport.design_tags(DesignTagsCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    if response.status != 200 {
        let body = if response.body.len() <= 64 * 1024 {
            serde_json::from_slice::<Value>(&response.body).ok()
        } else {
            None
        };
        let error = body.as_ref().and_then(|v| v.get("error"));
        let message = error
            .and_then(|v| v.get("message"))
            .and_then(Value::as_str)
            .or_else(|| {
                body.as_ref()
                    .and_then(|v| v.get("message"))
                    .and_then(Value::as_str)
            });
        let code = error.and_then(|v| v.get("code")).and_then(Value::as_str);
        // The tag service answers a definition id it does not hold as active
        // with its own 404. That is a fact about the named definition, not
        // about the caller's authority, and reporting it as a rejected
        // credential sent operators to check access they already had. Only a
        // 404 ds-brain itself authored, for a request that names a
        // definition, is read this way: a Gateway 404 for an unrouted path
        // and every 401/403 keep the classification they always had.
        let (kind, owner_message) = if response.status == 404
            && command.names_definition()
            && service_authored(&response)
        {
            (
                ErrorKind::ResourceNotFound,
                "the design tag service holds no active tag definition by the id this request named",
            )
        } else {
            (
                classify_tile_error(response.status).kind(),
                "the design tag service refused the project operation",
            )
        };
        return Err(ClientError::with_service_refusal(
            kind,
            owner_message,
            crate::ServiceRefusal::new(response.status, code, message),
        ));
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(bad());
    }
    let v: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if v["success"] != true {
        return Err(bad());
    }
    match command {
        Command::Batch { group, entries, .. } => {
            let plan = &v["data"]["plan"];
            if plan["project_id"] != project
                || plan["group"] != *group
                || plan["operation"] != "assign"
                || plan["outcomes"]
                    .as_array()
                    .is_none_or(|a| a.len() != entries.len())
                || plan["plan_digest"].as_str().is_none_or(|d| {
                    d.len() != 64
                        || !d
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            {
                return Err(bad());
            }
            for (outcome, (name, _)) in plan["outcomes"]
                .as_array()
                .ok_or_else(bad)?
                .iter()
                .zip(entries)
            {
                if outcome["transformer"] != *name {
                    return Err(bad());
                }
            }
            Ok(plan.clone())
        }
        Command::Definitions => {
            let rows = if v["data"]["definitions"].is_null() {
                vec![]
            } else {
                v["data"]["definitions"].as_array().ok_or_else(bad)?.clone()
            };
            if rows.len() > 1000 {
                return Err(bad());
            }
            Ok(json!({"project":project,"definitions":rows,"more":false}))
        }
        Command::Projection { definitions, .. } => {
            let p = &v["data"]["projection"];
            let doc = p["document"].as_str().ok_or_else(bad)?;
            if p["project_id"] != project
                || p["definition_ids"] != json!(definitions)
                || p["bytes"].as_u64() != Some(doc.len() as u64)
                || p["sha256"] != format!("{:x}", Sha256::digest(doc.as_bytes()))
            {
                return Err(bad());
            }
            let parsed: Value = serde_json::from_str(doc).map_err(|_| bad())?;
            if parsed["project_id"] != project
                || parsed["schema_version"] != "ds-report.design-tags/v3"
            {
                return Err(bad());
            }
            Ok(p.clone())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_scope_is_explicit_and_unique() {
        assert!(Command::Projection {
            transformers: vec![],
            definitions: vec![]
        }
        .validate("p")
        .is_err());
        assert!(Command::Projection {
            transformers: vec!["a".into(), "a".into()],
            definitions: vec![]
        }
        .validate("p")
        .is_err());
        assert!(Command::Projection {
            transformers: vec!["a".into()],
            definitions: vec![]
        }
        .validate("p")
        .is_ok());
    }
}
