//! Survey photo bytes for a headless reader, through the product's one
//! durable media authority.
//!
//! The browser reads a photo straight from Firebase Storage with its Firebase
//! ID token. A device-linked CLI holds a DS access token, which Storage does
//! not accept, so a headless read goes through ds-brain's `media_scope` grant
//! on `/report`: the JWT, project membership and `reports.export` are checked
//! once per project when the grant is minted, exactly as for a report's photo
//! links, and each photo then resolves through the public link resolver
//! (`/api/v1/media/link/{token}?o=`) to a short-lived signature. No
//! credential accompanies a byte fetch.
//!
//! The service mints separate scopes for the field app's
//! `projects/{project}/forms/...` layout and the browser's older
//! `{project}/forms/...` layout. Both are issued only after the same project
//! access and report-export checks.
//!
//! The route is hard-coded, not a profile route, so the credential audience
//! is unchanged.

use serde_json::{Value, json};

use crate::{
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, Transport, TransportResponse,
    authorized::Authority,
};

pub const GRANT_PATH: &str = "/report";
pub const GRANT_ACTION: &str = "media_scope";
/// Long enough for a whole form's photos, short enough to be worth nothing
/// by tomorrow.
pub const GRANT_TTL_SECONDS: u64 = 3600;
pub const GRANT_RESPONSE_LIMIT: usize = 64 * 1024;
pub const GRANT_TIMEOUT_SECONDS: u64 = 60;
/// ds-brain `maxMediaScopeProjects`.
pub const MAX_PROJECTS: usize = 32;
/// ds-brain `MediaLinkPath`.
pub const LINK_PATH: &str = "/api/v1/media/link/";
pub const MAX_PHOTO_BYTES: usize = 64 * 1024 * 1024;
pub const PHOTO_TIMEOUT_SECONDS: u64 = 180;

/// One project's grant. The token is a bearer credential for an hour, so it
/// is never printed: `Debug` leaves it out and nothing returns it whole.
#[derive(Clone, PartialEq, Eq)]
pub struct MediaGrant {
    project: String,
    bucket: String,
    prefix: String,
    expires_at: String,
    resolver_origin: String,
    token: String,
}

impl std::fmt::Debug for MediaGrant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaGrant")
            .field("project", &self.project)
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl MediaGrant {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
    pub fn expires_at(&self) -> &str {
        &self.expires_at
    }
}

/// The grants of one fetch: modern project scopes first, then each paired
/// legacy survey-forms scope when the service provides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaGrants {
    grants: Vec<MediaGrant>,
}

impl MediaGrants {
    pub fn grants(&self) -> &[MediaGrant] {
        &self.grants
    }

    /// The resolver link for one object, or `None` when no grant covers it.
    pub fn link_for(&self, object_path: &str) -> Option<String> {
        let grant = self
            .grants
            .iter()
            .find(|grant| object_path.starts_with(&grant.prefix))?;
        Some(format!(
            "{}{LINK_PATH}{}?o={}",
            grant.resolver_origin,
            grant.token,
            encode_component(object_path)
        ))
    }

    /// The grants as the document the report engine reads
    /// (`ds-network-reporter` `media_link::MediaScopeGrants`): `{grants: [...]}`.
    /// The token leaves this type only here, written into private staging for the engine.
    pub fn report_grant_document(&self) -> Result<Vec<u8>, ClientError> {
        let scope = |grant: &MediaGrant| {
            json!({
                "resolver_origin": grant.resolver_origin,
                "bucket": grant.bucket,
                "prefix": grant.prefix,
                "token": grant.token,
                "expires_at": grant.expires_at,
            })
        };
        if self.grants.is_empty() {
            return Err(invalid("a report media grant names at least one project"));
        }
        let document = json!({"grants": self.grants.iter().map(scope).collect::<Vec<_>>()});
        serde_json::to_vec(&document)
            .map_err(|_| unreadable("the media grant could not be encoded"))
    }
}

/// What a grant is minted for, which decides how long it lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantUse {
    /// Reading photo bytes now: [`GRANT_TTL_SECONDS`].
    Fetch,
    /// Links written into a delivered report. The request names no TTL, so
    /// ds-brain applies its report default (six months) — the same request
    /// the browser's report export sends, and the same links the cloud
    /// export writes.
    ReportLinks,
}

/// One fetched photo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyPhoto {
    pub object_path: String,
    pub bytes: Vec<u8>,
    /// Sniffed from the bytes: `image/jpeg`, `image/png`, `image/webp`,
    /// `image/heic`, or `None` for anything else the store held.
    pub media_type: Option<&'static str>,
}

/// The projects a set of references needs, the named one first, each once.
pub fn scope_projects<'a>(
    project: &'a str,
    references: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    let mut projects = vec![project];
    for reference in references {
        if !projects.contains(&reference) {
            projects.push(reference);
        }
    }
    projects
}

pub(crate) fn grant<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    others: &[&str],
) -> Result<MediaGrants, ClientError> {
    grant_for(
        profile,
        transport,
        authority,
        project,
        others,
        GrantUse::Fetch,
    )
}

/// The request body one grant is minted with.
fn grant_payload(project: &str, extras: &[&str], purpose: GrantUse) -> serde_json::Value {
    match purpose {
        GrantUse::Fetch => json!({
            "action": GRANT_ACTION,
            "eds_project_id": project,
            "scope_projects": extras,
            "ttl_seconds": GRANT_TTL_SECONDS,
        }),
        // Byte-identical to the browser's report export: no TTL, and no
        // `scope_projects` when there is nothing extra to ask for.
        GrantUse::ReportLinks if extras.is_empty() => json!({
            "action": GRANT_ACTION,
            "eds_project_id": project,
        }),
        GrantUse::ReportLinks => json!({
            "action": GRANT_ACTION,
            "eds_project_id": project,
            "scope_projects": extras,
        }),
    }
}

pub(crate) fn grant_for<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    others: &[&str],
    purpose: GrantUse,
) -> Result<MediaGrants, ClientError> {
    crate::authorized::validate_tiles(project)?;
    let mut requested = vec![project];
    for other in others {
        crate::authorized::validate_tiles(other)?;
        if !requested.contains(other) {
            requested.push(other);
        }
    }
    if requested.len() > MAX_PROJECTS {
        return Err(invalid(
            "the photos name more than 32 projects; fetch them in smaller groups",
        ));
    }
    let response = transport.survey_media_grant(crate::SurveyMediaGrantCall {
        profile,
        authority: authority.call(),
        payload: grant_payload(project, &requested[1..], purpose),
    })?;
    match response.status {
        200 => decode_grants(&response.body, &requested),
        403 => Err(ClientError::new(
            ErrorKind::AuthenticationRejected,
            "photo access is the report export grant: the verified user needs reports.export on every project the photos belong to",
        )),
        404 => Err(ClientError::new(
            ErrorKind::ResourceNotFound,
            "the project is not visible to this user",
        )),
        400 | 422 => Err(invalid("the media grant request was refused")),
        503 => Err(ClientError::new(
            ErrorKind::RouteUnavailable,
            "this deployment cannot mint media grants (no media link signing key)",
        )),
        408 | 425 | 429 | 500..=599 => Err(ClientError::new(
            ErrorKind::Transient,
            "the media grant service is temporarily unavailable",
        )),
        _ => Err(unreadable("the media grant answered an unexpected status")),
    }
}

/// Every requested project needs its modern grant. A paired legacy forms
/// grant is accepted only for the same requested project and HTTPS resolver.
/// Old services that issue modern grants alone remain readable.
fn decode_grants(body: &[u8], requested: &[&str]) -> Result<MediaGrants, ClientError> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| unreadable("the media grant is not JSON"))?;
    let rows = value
        .get("grants")
        .and_then(Value::as_array)
        .ok_or_else(|| unreadable("the media grant carries no grants"))?;
    let text = |row: &Value, key: &str| {
        row.get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| unreadable("a media grant is missing a field"))
    };
    let mut grants = Vec::with_capacity(requested.len() * 2);
    let mut decode = |project: &str, prefix: String| -> Result<(), ClientError> {
        let row = rows
            .iter()
            .find(|row| row.get("prefix").and_then(Value::as_str) == Some(prefix.as_str()))
            .ok_or_else(|| unreadable("the media grant does not cover a requested project"))?;
        let resolver_origin = text(row, "resolver_origin")?;
        let origin = url::Url::parse(&resolver_origin)
            .map_err(|_| unreadable("the media grant's resolver is not a URL"))?;
        if origin.scheme() != "https" || origin.path() != "/" || origin.query().is_some() {
            return Err(unreadable(
                "the media grant's resolver is not an https origin",
            ));
        }
        let token = text(row, "token")?;
        if !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(unreadable("the media grant's token is not one URL segment"));
        }
        grants.push(MediaGrant {
            project: project.to_owned(),
            bucket: text(row, "bucket")?,
            prefix,
            expires_at: text(row, "expires_at")?,
            resolver_origin: resolver_origin.trim_end_matches('/').to_owned(),
            token,
        });
        Ok(())
    };
    for project in requested {
        decode(project, format!("projects/{project}/"))?;
    }
    // A mixed response must not silently leave one source project's old
    // survey layout ungranted. The backend issues legacy scopes as a set.
    let has_legacy = requested.iter().any(|project| {
        let prefix = format!("{project}/forms/");
        rows.iter()
            .any(|row| row.get("prefix").and_then(Value::as_str) == Some(prefix.as_str()))
    });
    if has_legacy {
        for project in requested {
            decode(project, format!("{project}/forms/"))?;
        }
    }
    Ok(MediaGrants { grants })
}

/// Fetch one photo's bytes under the grants. Needs no credential: the link
/// is the authority, and the transport sends nothing else.
pub fn fetch<T: Transport + ?Sized>(
    transport: &mut T,
    grants: &MediaGrants,
    object_path: &str,
) -> Result<SurveyPhoto, ClientError> {
    let (object_path, link) = prepare(grants, object_path)?;
    let response = transport.survey_media_bytes(crate::SurveyMediaBytesCall { url: &link })?;
    accept(object_path, response)
}

/// A photo's thumbnail, and whether it had to be made here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyThumbnail {
    pub photo: SurveyPhoto,
    /// True when the store held no thumbnail (the field app does not always
    /// upload one) and it was made from the original by the product's own
    /// thumbnail procedure, `survey_photo_media::thumbnail`: the same bytes
    /// the browser and a rotation make.
    pub generated: bool,
}

/// Fetch the thumbnail companion of one original, or make it from the
/// original when the store never received one.
pub fn fetch_thumbnail<T: Transport + ?Sized>(
    transport: &mut T,
    grants: &MediaGrants,
    object_path: &str,
) -> Result<SurveyThumbnail, ClientError> {
    fetch_thumbnail_turned(transport, grants, object_path, 0)
}

/// The thumbnail showing the `quarter_turns` the photo's entry records. An
/// unturned photo reads its stored companion (or makes one when the store
/// never received it); a turned photo's thumbnail is made from the original
/// here, by the one Kernel procedure, so a companion that predates the turn
/// is never shown.
pub fn fetch_thumbnail_turned<T: Transport + ?Sized>(
    transport: &mut T,
    grants: &MediaGrants,
    object_path: &str,
    quarter_turns: u8,
) -> Result<SurveyThumbnail, ClientError> {
    let address = ds_command_kernel::survey_media_address::parse(object_path)
        .map_err(|_| invalid("the reference is not a survey media address"))?;
    let rotation =
        ds_command_kernel::survey_photo_media::Rotation::from_quarter_turns(quarter_turns)
            .map_err(|_| invalid("a rotation is 0 to 3 clockwise quarter-turns"))?;
    let make = |transport: &mut T| -> Result<SurveyThumbnail, ClientError> {
        let original = fetch(transport, grants, &address.object_path)?;
        let made = ds_command_kernel::survey_photo_media::thumbnail(&original.bytes, rotation)
            .map_err(|_| {
                unreadable("the original photo could not be decoded to make its thumbnail")
            })?;
        Ok(SurveyThumbnail {
            photo: SurveyPhoto {
                object_path: address.thumbnail_object_path.clone(),
                bytes: made.bytes,
                media_type: Some(made.media_type),
            },
            generated: true,
        })
    };
    if quarter_turns != 0 && address.thumbnail_object_path != address.object_path {
        return make(transport);
    }
    match fetch(transport, grants, &address.thumbnail_object_path) {
        Ok(photo) => Ok(SurveyThumbnail {
            photo,
            generated: false,
        }),
        Err(error)
            if error.kind() == ErrorKind::ResourceNotFound
                && address.thumbnail_object_path != address.object_path =>
        {
            make(transport)
        }
        Err(error) => Err(error),
    }
}

/// The canonical object path and the resolver link that reads it.
fn prepare(grants: &MediaGrants, object_path: &str) -> Result<(String, String), ClientError> {
    let address = ds_command_kernel::survey_media_address::parse(object_path)
        .map_err(|_| invalid("the reference is not a survey media address"))?;
    let link = grants.link_for(&address.object_path).ok_or_else(|| {
        invalid("the photo belongs to a project or survey layout outside this fetch's grants")
    })?;
    Ok((address.object_path, link))
}

fn accept(
    object_path: String,
    mut response: TransportResponse,
) -> Result<SurveyPhoto, ClientError> {
    match response.status {
        200 if !response.body.is_empty() => {
            let bytes = std::mem::take(&mut response.body);
            Ok(SurveyPhoto {
                media_type: sniff(&bytes),
                object_path,
                bytes,
            })
        }
        200 => Err(unreadable("the photo store returned no bytes")),
        404 => Err(ClientError::new(
            ErrorKind::ResourceNotFound,
            "the photo is not stored",
        )),
        410 => Err(ClientError::new(
            ErrorKind::Transient,
            "the media grant expired; run the fetch again",
        )),
        408 | 425 | 429 | 500..=599 => Err(ClientError::new(
            ErrorKind::Transient,
            "the photo store is temporarily unavailable",
        )),
        _ => Err(unreadable("the photo link answered an unexpected status")),
    }
}

fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some("image/webp"),
        [
            _,
            _,
            _,
            _,
            b'f',
            b't',
            b'y',
            b'p',
            b'h',
            b'e',
            b'i',
            b'c' | b'x',
            ..,
        ] => Some("image/heic"),
        _ => None,
    }
}

/// RFC 3986 unreserved characters pass; every other byte is escaped, `/`
/// included, as Go's `url.PathEscape` does for the resolver's own links.
fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant_body(projects: &[&str]) -> Vec<u8> {
        let grants: Vec<Value> = projects
            .iter()
            .map(|project| {
                json!({"resolver_origin":"https://brain.example","bucket":"media-bucket",
                       "prefix":format!("projects/{project}/"),"token":format!("tok-{project}"),
                       "expires_at":"2026-09-25T02:00:00Z"})
            })
            .collect();
        json!({"grant": grants[0], "grants": grants, "ttl_seconds": 3600})
            .to_string()
            .into_bytes()
    }

    fn paired_grant_body(projects: &[&str]) -> Vec<u8> {
        let mut value: Value = serde_json::from_slice(&grant_body(projects)).unwrap();
        let grants = value["grants"].as_array_mut().unwrap();
        for project in projects {
            grants.push(json!({
                "resolver_origin": "https://brain.example",
                "bucket": "media-bucket",
                "prefix": format!("{project}/forms/"),
                "token": format!("legacy-{project}"),
                "expires_at": "2026-09-25T02:00:00Z"
            }));
        }
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn a_report_grant_asks_for_the_default_life_and_becomes_the_engine_document() {
        // A report's links live as long as the browser's: no TTL is named.
        assert_eq!(
            grant_payload("p1", &[], GrantUse::ReportLinks),
            json!({"action": "media_scope", "eds_project_id": "p1"})
        );
        assert_eq!(
            grant_payload("p1", &["p0"], GrantUse::ReportLinks)["scope_projects"],
            json!(["p0"])
        );
        assert_eq!(
            grant_payload("p1", &[], GrantUse::Fetch)["ttl_seconds"],
            json!(GRANT_TTL_SECONDS)
        );
        let single = decode_grants(&grant_body(&["p1"]), &["p1"]).unwrap();
        let document: Value =
            serde_json::from_slice(&single.report_grant_document().unwrap()).unwrap();
        assert_eq!(
            document,
            json!({"grants":[{"resolver_origin":"https://brain.example","bucket":"media-bucket",
                   "prefix":"projects/p1/","token":"tok-p1","expires_at":"2026-09-25T02:00:00Z"}]})
        );
        let several = decode_grants(&grant_body(&["p1", "p0"]), &["p1", "p0"]).unwrap();
        let document: Value =
            serde_json::from_slice(&several.report_grant_document().unwrap()).unwrap();
        assert_eq!(document["grants"][1]["prefix"], "projects/p0/");
    }

    #[test]
    fn grants_cover_exactly_the_requested_projects_and_build_resolver_links() {
        let grants = decode_grants(&grant_body(&["p1", "p0"]), &["p1", "p0"]).unwrap();
        assert_eq!(
            grants
                .link_for("projects/p0/forms/pole/e1/photo/a b.jpg")
                .unwrap(),
            "https://brain.example/api/v1/media/link/tok-p0?o=projects%2Fp0%2Fforms%2Fpole%2Fe1%2Fphoto%2Fa%20b.jpg"
        );
        assert!(grants.link_for("p1/forms/pole/e1/a.jpg").is_none());
        assert!(!format!("{grants:?}").contains("tok-"));
        // A grant set missing a requested project, or resolving off https, is refused.
        assert!(decode_grants(&grant_body(&["p1"]), &["p1", "p0"]).is_err());
        let insecure = String::from_utf8(grant_body(&["p1"]))
            .unwrap()
            .replace("https://", "http://");
        assert!(decode_grants(insecure.as_bytes(), &["p1"]).is_err());
    }

    #[test]
    fn paired_legacy_grants_cover_only_requested_projects_and_reach_the_report() {
        let grants = decode_grants(&paired_grant_body(&["p1", "p0"]), &["p1", "p0"]).unwrap();
        let legacy = "p1/forms/field_notes/e1/DSC_0570.JPG";
        assert_eq!(
            grants.link_for(legacy).unwrap(),
            "https://brain.example/api/v1/media/link/legacy-p1?o=p1%2Fforms%2Ffield_notes%2Fe1%2FDSC_0570.JPG"
        );
        assert!(
            grants
                .link_for("p9/forms/field_notes/e1/secret.jpg")
                .is_none()
        );
        assert!(
            grants
                .link_for("p1/assets/confidential/contract.pdf")
                .is_none()
        );
        assert_eq!(prepare(&grants, legacy).unwrap().0, legacy);
        let document: Value =
            serde_json::from_slice(&grants.report_grant_document().unwrap()).unwrap();
        assert_eq!(document["grants"].as_array().unwrap().len(), 4);
        assert_eq!(document["grants"][2]["prefix"], "p1/forms/");

        let mut partial: Value = serde_json::from_slice(&paired_grant_body(&["p1", "p0"])).unwrap();
        partial["grants"].as_array_mut().unwrap().pop();
        assert!(decode_grants(&serde_json::to_vec(&partial).unwrap(), &["p1", "p0"]).is_err());
    }

    #[test]
    fn a_fetch_reads_only_granted_trees_and_names_what_it_got() {
        let grants = decode_grants(&grant_body(&["p1"]), &["p1"]).unwrap();
        let (object_path, link) =
            prepare(&grants, "/projects/p1/forms/pole/e1/photo/a.jpg").unwrap();
        assert_eq!(object_path, "projects/p1/forms/pole/e1/photo/a.jpg");
        assert!(link.starts_with("https://brain.example/api/v1/media/link/tok-p1?o="));
        let photo = accept(
            object_path.clone(),
            TransportResponse::new(200, vec![0xFF, 0xD8, 0xFF, 0xE0]),
        )
        .unwrap();
        assert_eq!(photo.media_type, Some("image/jpeg"));
        let missing = accept(object_path, TransportResponse::new(404, Vec::new()));
        assert_eq!(missing.unwrap_err().kind(), ErrorKind::ResourceNotFound);
        for outside in ["p1/forms/pole/e1/a.jpg", "projects/p9/forms/pole/e1/a.jpg"] {
            assert_eq!(
                prepare(&grants, outside).unwrap_err().kind(),
                ErrorKind::InvalidInput
            );
        }
    }
}
