//! Authenticated, bounded project model reads. Signed locators never leave this owner.
use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, GridModelsCall,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
pub const MAX_BYTES: usize = 256 * 1024 * 1024;
pub mod publication;
pub enum Command {
    List {
        limit: u16,
        cursor: Option<String>,
    },
    Download {
        model: String,
        revision: String,
    },
    Publish {
        intent: ds_command_kernel::grid_publication::Intent,
        bytes: Vec<u8>,
    },
}
pub struct Receipt {
    pub data: Value,
    pub bytes: Option<Vec<u8>>,
}
pub struct DownloadCall<'a> {
    url: &'a str,
    size: usize,
}
impl DownloadCall<'_> {
    pub fn url(&self) -> &str {
        self.url
    }
    pub fn response_limit(&self) -> usize {
        self.size
    }
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
        "invalid project model selection or page bound",
    )
}
fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "model response, storage locator or exact package bytes failed verification",
    )
}
impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !id(project) {
            return Err(invalid());
        }
        match self {
            Self::List { limit, cursor }
                if (1..=100).contains(limit)
                    && cursor.as_ref().is_none_or(|c| {
                        !c.is_empty() && c.len() <= 4096 && !c.chars().any(char::is_control)
                    }) =>
            {
                Ok(())
            }
            Self::Download { model, revision } if id(model) && id(revision) => Ok(()),
            Self::Publish { intent, bytes } => intent
                .plan(project, bytes)
                .map(|_| ())
                .map_err(|message| ClientError::new(ErrorKind::InvalidInput, message)),
            _ => Err(invalid()),
        }
    }
}
fn locator<'a>(url: &'a str, object: &str, size: usize) -> Result<DownloadCall<'a>, ClientError> {
    let u = url::Url::parse(url).map_err(|_| bad())?;
    if url.len() > 8192
        || u.scheme() != "https"
        || u.host_str() != Some("storage.googleapis.com")
        || u.port().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || size == 0
        || size > MAX_BYTES
    {
        return Err(bad());
    }
    let path = u.path().trim_start_matches('/');
    let (_, tail) = path.split_once('/').ok_or_else(bad)?;
    if tail != object
        || !object.starts_with("ds-grid/objects/sha256/")
        || object
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(bad());
    }
    Ok(DownloadCall { url, size })
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Receipt, ClientError> {
    command.validate(project)?;
    if let Command::Publish { intent, bytes } = command {
        return publication::run(profile, transport, authority, project, intent, bytes);
    }
    let payload = match command {
        Command::List { limit, cursor } => {
            json!({"action":"list_models","project_id":project,"page_size":limit,"cursor":cursor.as_deref().unwrap_or(""),"include_deleted":false})
        }
        Command::Download { model, revision } => {
            json!({"action":"get_version","project_id":project,"model_id":model,"revision_id":revision})
        }
        Command::Publish { .. } => unreachable!("publication dispatched before reads"),
    };
    let response = transport.grid_models(GridModelsCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(bad());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if body["success"] != true {
        return Err(bad());
    }
    let data = &body["data"];
    match command {
        Command::Publish { .. } => unreachable!("publication dispatched before reads"),
        Command::List { limit, .. } => {
            let rows = if data["models"].is_null() {
                vec![]
            } else {
                data["models"].as_array().ok_or_else(bad)?.clone()
            };
            let more = data["has_more"].as_bool().ok_or_else(bad)?;
            let cursor = data["next_cursor"].as_str().unwrap_or("");
            if rows.len() > usize::from(*limit)
                || cursor.len() > 4096
                || (more && cursor.is_empty())
            {
                return Err(bad());
            }
            let mut ids = std::collections::HashSet::new();
            let mut models = Vec::new();
            for row in rows {
                let model = row["model_id"].as_str().filter(|v| id(v)).ok_or_else(bad)?;
                if !ids.insert(model.to_owned()) || row["project_id"] != project {
                    return Err(bad());
                }
                models.push(json!({"model_id":model,"display_name":row["display_name"],"model_kind":row["model_kind"],"head_revision_id":row["head_revision_id"],"head_model_digest":row["head_model_digest"],"state":row["state"],"lifecycle":row["lifecycle"]}));
            }
            Ok(Receipt {
                data: json!({"project":project,"models":models,"more":more,"next_cursor":cursor}),
                bytes: None,
            })
        }
        Command::Download { model, revision } => {
            let v = data
                .get("revision")
                .or_else(|| data.get("version"))
                .ok_or_else(bad)?;
            if v["project_id"] != project
                || v["model_id"] != *model
                || v["revision_id"] != *revision
            {
                return Err(bad());
            }
            let digest = v["model"]["digest"]
                .as_str()
                .ok_or_else(bad)?
                .trim_start_matches("sha256:");
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(bad());
            }
            let size = v["model"]["byte_length"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= MAX_BYTES as u64)
                .ok_or_else(bad)? as usize;
            let object = v["model"]["object"].as_str().ok_or_else(bad)?;
            let url = data["download_url"].as_str().ok_or_else(bad)?;
            let mut response = transport.grid_model_bytes(locator(url, object, size)?)?;
            if response.status != 200
                || response.body.len() != size
                || format!("{:x}", Sha256::digest(&response.body)) != digest
            {
                return Err(bad());
            }
            Ok(Receipt {
                data: json!({"project":project,"model_id":model,"revision_id":revision,"sha256":digest,"bytes":size,"verified":true}),
                bytes: Some(std::mem::take(&mut response.body)),
            })
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_locator_cannot_redirect_credentials_or_substitute_objects() {
        let object = "ds-grid/objects/sha256/ab/model.dsgrid";
        assert!(locator(
            &format!("https://storage.googleapis.com/bucket/{object}?X-Goog-Signature=issued"),
            object,
            12
        )
        .is_ok());
        for url in [
            "http://storage.googleapis.com/bucket/x",
            "https://evil.example/x",
            "https://storage.googleapis.com/bucket/wrong",
            "https://user@storage.googleapis.com/bucket/x",
        ] {
            assert!(locator(url, object, 12).is_err());
        }
        assert!(Command::List {
            limit: 101,
            cursor: None
        }
        .validate("project")
        .is_err());
        assert!(Command::Download {
            model: "../x".into(),
            revision: "r".into()
        }
        .validate("project")
        .is_err());
    }
}
