//! Project-owned report references. No Solar storage, uploader or media cache.
use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, SharedAssetsCall,
};
use base64::Engine;
use ds_command_kernel::assets::{Link, ReportReference};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub enum Command {
    /// Register bytes held outside DS — a Drive link with its digest — as a
    /// catalogue row (`reference`, external form, `assets.ingest`). DS never
    /// fetches the URL; the row's `read` is its link.
    External {
        url: String,
        sha256: String,
        size: u64,
        kind: String,
        format: Option<String>,
        folder_id: Option<String>,
        sensitivity: Option<String>,
        provider: Option<String>,
        name: Option<String>,
    },
    /// One link from an asset to the work it belongs to, or its removal.
    Attach {
        asset_id: String,
        link: Link,
        detach: bool,
    },
    /// One catalogue row by id, under the caller's class authority.
    Get {
        asset_id: String,
    },
    PublishMap {
        name: String,
        bytes: Vec<u8>,
        tags: Vec<Link>,
        replaces: Vec<String>,
    },
    Maps {
        limit: u16,
        cursor: Option<String>,
    },
    Reference {
        reference: ReportReference,
        link: Link,
    },
    Resolve {
        link: Link,
        role: String,
    },
    Read {
        asset_id: String,
        digest: String,
    },
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
const MAX_BYTES: usize = 32 * 1024 * 1024;
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid shared asset reference, tag identity or digest",
    )
}
fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "shared asset identity or content digest failed verification",
    )
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn sha(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|d| {
        d.len() == 64
            && d.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn role(s: &str) -> bool {
    matches!(s, "network_information" | "city_map" | "transformer_map")
}
fn link_valid(link: &Link) -> bool {
    match link {
        Link::Tag {
            definition_id,
            value,
        } => {
            token(definition_id)
                && !value.is_empty()
                && value.len() <= 256
                && value.trim() == value
                && !value.chars().any(char::is_control)
        }
        Link::DsObject {
            object_type,
            entity_id,
        } => object_type == "transformer" && token(entity_id),
        Link::PmTask { id } => token(id),
        Link::PmRecord { id } => token(id),
    }
}

/// A catalogue asset id as ds-brain mints one: `a_` and twelve of its
/// alphabet.
fn asset_id(id: &str) -> bool {
    id.len() == 14
        && id.starts_with("a_")
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// An https URL with a host and no credentials, bounded as ds-brain bounds it.
fn external_url(raw: &str) -> bool {
    if raw.trim() != raw || raw.is_empty() || raw.len() > 2048 || raw.chars().any(char::is_control)
    {
        return false;
    }
    let Ok(parsed) = url::Url::parse(raw) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none()
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn optional_token(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(token)
}
impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !token(project) {
            return Err(invalid());
        }
        let valid = match self {
            Self::External {
                url,
                sha256,
                size,
                kind,
                format,
                folder_id,
                sensitivity,
                provider,
                name,
            } => {
                external_url(url)
                    && hex_digest(sha256)
                    && *size > 0
                    && matches!(
                        kind.as_str(),
                        "doc" | "sheet" | "mail" | "note" | "geo" | "image" | "pack" | "other"
                    )
                    && optional_token(format)
                    && optional_token(folder_id)
                    && sensitivity.as_deref().is_none_or(|class| {
                        matches!(class, "open" | "internal" | "restricted" | "confidential")
                    })
                    && provider
                        .as_deref()
                        .is_none_or(|provider| matches!(provider, "google_drive" | "other"))
                    && optional_token(name)
            }
            Self::Attach {
                asset_id: id, link, ..
            } => asset_id(id) && link_valid(link),
            Self::Get { asset_id: id } => asset_id(id),
            Self::PublishMap {
                name,
                bytes,
                tags,
                replaces,
            } => {
                token(name)
                    && bytes.len() <= MAX_BYTES
                    && !bytes.is_empty()
                    && tags.len() <= 16
                    && tags
                        .iter()
                        .all(|link| matches!(link, Link::Tag { .. }) && link_valid(link))
                    && replaces.len() <= 16
                    && replaces.iter().all(|id| {
                        id.len() == 14
                            && id.starts_with("a_")
                            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    })
                    && replaces
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == replaces.len()
                    && super::shared_map_publish::format(name, bytes).is_ok()
            }
            Self::Maps { limit, cursor } => {
                (1..=200).contains(limit)
                    && cursor.as_ref().is_none_or(|s| {
                        !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
                    })
            }
            Self::Reference { reference, link } => {
                token(&reference.work_id)
                    && !reference.output_id.is_empty()
                    && reference.output_id.len() <= 256
                    && role(&reference.role)
                    && link_valid(link)
            }
            Self::Resolve { link, role: r } => link_valid(link) && role(r),
            Self::Read { asset_id, digest } => {
                asset_id.starts_with("a_") && asset_id.len() == 14 && token(asset_id) && sha(digest)
            }
        };
        if valid {
            Ok(())
        } else {
            Err(invalid())
        }
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
    if let Command::PublishMap {
        name,
        bytes,
        tags,
        replaces,
    } = command
    {
        return super::shared_map_publish::publish(
            profile, transport, authority, project, name, bytes, tags, replaces,
        );
    }
    let payload = match command {
        Command::PublishMap { .. } => {
            unreachable!("map publication is a closed upload transaction")
        }
        Command::External {
            url,
            sha256,
            size,
            kind,
            format,
            folder_id,
            sensitivity,
            provider,
            name,
        } => {
            let mut external = json!({"url": url, "sha256": sha256, "size": size, "kind": kind});
            for (key, value) in [
                ("format", format),
                ("folder_id", folder_id),
                ("sensitivity", sensitivity),
                ("provider", provider),
                ("name", name),
            ] {
                if let Some(value) = value {
                    external[key] = json!(value);
                }
            }
            json!({"action":"reference","ds_project":project,"external":external})
        }
        Command::Attach {
            asset_id,
            link,
            detach,
        } => {
            json!({"action": if *detach { "detach" } else { "attach" }, "ds_project":project,"asset_id":asset_id,"link":link})
        }
        Command::Get { asset_id } => {
            json!({"action":"get","ds_project":project,"asset_id":asset_id})
        }
        Command::Maps { limit, cursor } => {
            let mut value = json!({"action":"list","ds_project":project,"limit":limit});
            if let Some(cursor) = cursor {
                value["cursor"] = json!(cursor);
            }
            value
        }
        Command::Reference { reference, link } => {
            json!({"action":"reference","ds_project":project,"reference":{"reference":reference,"link":link}})
        }
        Command::Resolve { link, role } => {
            json!({"action":"resolve","ds_project":project,"link":link,"role":role})
        }
        Command::Read { asset_id, .. } => {
            json!({"action":"download","ds_project":project,"asset_id":asset_id})
        }
    };
    let response = transport.shared_assets(SharedAssetsCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    if response.status != 200 {
        // The catalogue's own refusals name their rule: `ASSET_REFUSED` with
        // `details.reason` (`asset_bytes_not_held`, `invalid_external_reference`,
        // …), `CONFLICT` for the same reference under different facts,
        // `ASSET_CLASS_FORBIDDEN` naming the class. Those cross as the code
        // and the details the rule named; the class stays the status's.
        let served = crate::project_correspondence::envelope(response.status, &response.body);
        let coarse = classify_tile_error(response.status);
        return Err(match served {
            Some(served) => ClientError::with_service_refusal(
                coarse.kind(),
                "the assets catalogue refused this request",
                served,
            ),
            None => coarse,
        });
    }
    if response.body.len() > 1024 * 1024 {
        return Err(bad());
    }
    let v: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if v["success"] != true {
        return Err(bad());
    }
    let data = &v["data"];
    if let Command::Maps { limit, .. } = command {
        if data["assets"]
            .as_array()
            .is_none_or(|rows| rows.len() > usize::from(*limit))
        {
            return Err(bad());
        }
        return ds_command_kernel::assets::tag_maps::index(project, data).map_err(|_| bad());
    }
    if matches!(command, Command::Resolve { .. }) {
        if data["project_id"] != project || data["manual_entry_allowed"] != true {
            return Err(bad());
        }
        match data["status"].as_str() {
            Some("missing" | "ambiguous" | "incomplete") => {
                if !data["asset"].is_null() {
                    return Err(bad());
                }
                if data["candidates"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|row| row["project_id"] != project))
                {
                    return Err(bad());
                }
                return Ok(json!({"project":project,"resolution":data,"storage_copied":false}));
            }
            Some("resolved") => {}
            _ => return Err(bad()),
        }
    }
    let asset = if matches!(command, Command::Read { .. } | Command::Resolve { .. }) {
        &data["asset"]
    } else {
        data
    };
    if asset["project_id"] != project
        || asset["schema"] != "ds.assets.asset/v1"
        || !asset["digest"].as_str().is_some_and(sha)
    {
        return Err(bad());
    }
    match command {
        Command::External { url, sha256, .. } => {
            // The row is the reference the caller declared, or it is not the
            // row: same digest, the link kept, no bytes held here.
            if asset["digest"] != format!("sha256:{sha256}")
                || asset["source"]["kind"] != "external_reference"
                || asset["bytes_held"] != false
                || asset["external"]["url"].as_str().is_none_or(|held| {
                    url::Url::parse(held).ok().map(|u| u.to_string())
                        != url::Url::parse(url).ok().map(|u| u.to_string())
                })
            {
                return Err(bad());
            }
            return Ok(
                json!({"asset":asset,"project":project,"storage_copied":false,"bytes_held":false}),
            );
        }
        Command::Attach {
            asset_id,
            link,
            detach,
        } => {
            let held = asset["links"]
                .as_array()
                .is_some_and(|links| links.contains(&json!(link)));
            if asset["asset_id"] != *asset_id || held == *detach {
                return Err(bad());
            }
            return Ok(json!({"asset":asset,"project":project,"storage_copied":false}));
        }
        Command::Get { asset_id } => {
            if asset["asset_id"] != *asset_id {
                return Err(bad());
            }
            return Ok(json!({"asset":asset,"project":project,"storage_copied":false}));
        }
        Command::PublishMap { .. } => {
            unreachable!("map publication returned before reference reads")
        }
        Command::Maps { .. } => {
            unreachable!("catalogue pages return before individual asset validation")
        }
        Command::Reference { reference, link } => {
            if asset["reference"] != json!(reference)
                || !asset["links"]
                    .as_array()
                    .is_some_and(|links| links.contains(&json!(link)))
            {
                return Err(bad());
            }
        }
        Command::Resolve { link, role } => {
            if asset["reference"]["role"] != *role
                || !asset["links"]
                    .as_array()
                    .is_some_and(|links| links.contains(&json!(link)))
            {
                return Err(bad());
            }
        }
        Command::Read { asset_id, digest } => {
            if asset["asset_id"] != *asset_id || asset["digest"] != *digest {
                return Err(bad());
            }
            let size = asset["bytes"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= MAX_BYTES as u64)
                .ok_or_else(bad)? as usize;
            let url = data["download_url"].as_str().ok_or_else(bad)?;
            let u = url::Url::parse(url).map_err(|_| bad())?;
            if url.len() > 8192
                || u.scheme() != "https"
                || u.host_str() != Some("storage.googleapis.com")
                || u.port().is_some()
                || !u.username().is_empty()
                || u.password().is_some()
                || u.fragment().is_some()
                || !u.query_pairs().any(|(k, _)| k == "generation")
            {
                return Err(bad());
            }
            let bytes = transport.shared_asset_bytes(DownloadCall { url, size })?;
            if bytes.status != 200
                || bytes.body.len() != size
                || format!("sha256:{:x}", Sha256::digest(&bytes.body)) != *digest
            {
                return Err(bad());
            }
            return Ok(
                json!({"asset":asset,"verified":true,"base64":base64::engine::general_purpose::STANDARD.encode(&bytes.body)}),
            );
        }
    }
    if matches!(command, Command::Resolve { .. }) {
        return Ok(json!({"project":project,"resolution":data,"storage_copied":false}));
    }
    Ok(json!({"asset":asset,"project":project,"storage_copied":false}))
}
