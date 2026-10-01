//! Headless read of one form's survey entries WITH their field values.
//!
//! The map's survey loader reads a form through `/api/v1/data`
//! `query_entries` under the signed-in user's JWT; this is that same read for a
//! host with no window (owner ruling 2026-09-25: what the UI can do, the CLI
//! does, headlessly; feedback ee3f371f). ds-brain authorizes it exactly as it
//! authorizes the UI: project membership and the caller's form scope.
//!
//! The route streams one GeoJSON Feature per line and closes with a
//! `__type: "summary"` trailer carrying the form count, per-form errors and
//! the feature total. The trailer is load-bearing: a stream without it was cut
//! off, a trailer naming a form error failed part-way, and a total that
//! disagrees with the lines received lost rows. Each is a refusal, because a
//! partial read returned as a whole one is how a report silently misses
//! entries. The server has no row limit, so the bound is this client's: the
//! whole verified stream is read (under a byte bound) and at most `limit`
//! entries are returned, with the true total beside them.
//!
//! Each entry names the survey media references its properties carry, as the
//! kernel's `survey_media_address` grammar reads them, so a caller can fetch
//! the photos an entry points at without re-deriving any storage layout.

use ds_command_kernel::survey_media_address;
use serde_json::{json, Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};

/// The route and action, fixed here and not in the client profile: a route
/// hard-coded in the transport never enters the credential audience.
pub const SURVEY_ENTRIES_READ_PATH: &str = "/api/v1/data";
pub const SURVEY_ENTRIES_READ_ACTION: &str = "query_entries";
pub const SURVEY_ENTRIES_READ_DEFAULT_LIMIT: usize = 500;
pub const SURVEY_ENTRIES_READ_MAX_LIMIT: usize = 5_000;
/// The largest stream this client reads before refusing. A form whose entries
/// exceed it must be narrowed (`updated_after`, `bbox`), never truncated.
pub const SURVEY_ENTRIES_READ_RESPONSE_LIMIT: usize = 64 * 1024 * 1024;
pub const SURVEY_ENTRIES_READ_TIMEOUT_SECONDS: u64 = 300;

/// One bounded read of one form.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyEntriesReadRequest {
    form: String,
    updated_after: Option<String>,
    bbox: Option<[f64; 4]>,
    include_deleted: bool,
    limit: usize,
}

impl SurveyEntriesReadRequest {
    pub fn new(
        form: impl Into<String>,
        updated_after: Option<&str>,
        bbox: Option<[f64; 4]>,
        include_deleted: bool,
        limit: Option<usize>,
    ) -> Result<Self, ClientError> {
        let form = form.into();
        if !ds_command_kernel::survey::hold::valid_form_slug(&form) {
            return Err(invalid("the form is not one exact survey form slug"));
        }
        let updated_after = updated_after
            .map(|value| {
                canonical_rfc3339(value)
                    .ok_or_else(|| invalid("updated_after is not an RFC 3339 time"))
            })
            .transpose()?;
        if bbox.is_some_and(|[west, south, east, north]| {
            [west, south, east, north]
                .iter()
                .any(|value| !value.is_finite())
                || !(-180.0..=180.0).contains(&west)
                || !(-180.0..=180.0).contains(&east)
                || !(-90.0..=90.0).contains(&south)
                || !(-90.0..=90.0).contains(&north)
                || west >= east
                || south >= north
        }) {
            return Err(invalid(
                "bbox is not one WGS84 west,south,east,north rectangle",
            ));
        }
        let limit = limit.unwrap_or(SURVEY_ENTRIES_READ_DEFAULT_LIMIT);
        if !(1..=SURVEY_ENTRIES_READ_MAX_LIMIT).contains(&limit) {
            return Err(invalid("limit must be from 1 through 5000"));
        }
        Ok(Self {
            form,
            updated_after,
            bbox,
            include_deleted,
            limit,
        })
    }

    pub fn form(&self) -> &str {
        &self.form
    }
    pub fn updated_after(&self) -> Option<&str> {
        self.updated_after.as_deref()
    }
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// The exact body the map's loader sends, for one project and one form.
    pub(crate) fn payload(&self, project: &str) -> Value {
        let mut body = json!({
            "action": SURVEY_ENTRIES_READ_ACTION,
            "eds_project_id": project,
            "form_slugs": [self.form],
            "exclude_deleted": !self.include_deleted,
        });
        if let Some(after) = &self.updated_after {
            body["updated_after"] = json!(after);
        }
        if let Some(bbox) = self.bbox {
            body["bbox"] = json!(bbox);
        }
        body
    }
}

/// A verified read: the entries (at most the requested limit), the true total,
/// and the media references each returned entry carries.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyEntriesRead {
    pub project: String,
    pub form: String,
    pub total: u64,
    pub truncated: bool,
    pub entries: Vec<SurveyEntry>,
    /// The trailer's `sync` object as the route sent it, when present.
    pub sync: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurveyEntry {
    /// The GeoJSON Feature exactly as the route streamed it.
    pub feature: Value,
    /// Every survey media reference in the entry's properties: the property
    /// it was found in, and the canonical object path the kernel resolved.
    pub media: Vec<SurveyEntryMedia>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntryMedia {
    pub property: String,
    pub reference: String,
    /// The bucket the reference itself names; a bare object path names none.
    pub bucket: Option<String>,
    pub object_path: String,
    pub thumbnail_object_path: String,
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &SurveyEntriesReadRequest,
) -> Result<SurveyEntriesRead, ClientError> {
    crate::authorized::validate_tiles(project)?;
    let response = transport.survey_entries_read(crate::SurveyEntriesReadCall {
        profile,
        authority: authority.call(),
        payload: request.payload(project),
    })?;
    match response.status {
        200 => decode(&response.body, project, request),
        status => Err(refusal(status)),
    }
}

/// Send one `query_entries` body the core's survey hold built
/// (`ds_command_kernel::survey::hold::decide`) and hand back the route's
/// NDJSON whole. The hold verifies the stream line by line; this only checks
/// the body asks for this project's entries and bounds the answer.
pub(crate) fn query<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    body: &Value,
) -> Result<Vec<u8>, ClientError> {
    crate::authorized::validate_tiles(project)?;
    if body["action"] != SURVEY_ENTRIES_READ_ACTION
        || body["eds_project_id"] != project
        || body["form_slugs"]
            .as_array()
            .is_none_or(|forms| forms.len() != 1)
    {
        return Err(invalid(
            "the survey query is not one query_entries request for this project's form",
        ));
    }
    let mut response = transport.survey_entries_read(crate::SurveyEntriesReadCall {
        profile,
        authority: authority.call(),
        payload: body.clone(),
    })?;
    match response.status {
        200 => Ok(std::mem::take(&mut response.body)),
        status => Err(refusal(status)),
    }
}

fn refusal(status: u16) -> ClientError {
    match status {
        403 => ClientError::new(
            ErrorKind::AuthenticationRejected,
            "the verified user may not read this project's survey form",
        ),
        404 => ClientError::new(
            ErrorKind::ResourceNotFound,
            "the survey form or project is not visible to this user",
        ),
        400 | 422 => invalid("the survey read request was refused"),
        408 | 425 | 429 | 500..=599 => ClientError::new(
            ErrorKind::Transient,
            "the survey read service is temporarily unavailable",
        ),
        _ => unreadable("the survey read answered an unexpected status"),
    }
}

/// The survey media references one feature carries, as [`decode`] names them.
pub fn entry_media(feature: &Value) -> Vec<SurveyEntryMedia> {
    media_of(feature)
}

/// Verify one NDJSON stream and keep at most `limit` entries.
///
/// Public because it is pure, and because what it refuses is the point.
pub fn decode(
    body: &[u8],
    project: &str,
    request: &SurveyEntriesReadRequest,
) -> Result<SurveyEntriesRead, ClientError> {
    let text =
        std::str::from_utf8(body).map_err(|_| unreadable("the survey stream is not text"))?;
    let mut entries: Vec<SurveyEntry> = Vec::new();
    let mut received: u64 = 0;
    let mut summary: Option<Map<String, Value>> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if summary.is_some() {
            return Err(unreadable(
                "the survey stream continued after its closing summary",
            ));
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|_| unreadable("the survey stream contains a line that is not JSON"))?;
        let Value::Object(object) = value else {
            return Err(unreadable(
                "the survey stream contains a line that is not an object",
            ));
        };
        if object.get("__type") == Some(&json!("summary")) {
            summary = Some(object);
            continue;
        }
        if object.get("type") != Some(&json!("Feature")) {
            return Err(unreadable(
                "the survey stream contains a row that is not a Feature",
            ));
        }
        received += 1;
        if entries.len() < request.limit {
            let feature = Value::Object(object);
            let media = media_of(&feature);
            entries.push(SurveyEntry { feature, media });
        }
    }
    let Some(summary) = summary else {
        return Err(unreadable(
            "the survey stream ended without its closing summary; it was cut off",
        ));
    };
    if summary
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(ClientError::new(
            ErrorKind::Transient,
            "the survey service failed part-way through this form; nothing partial is returned",
        ));
    }
    let total = summary
        .get("total_features")
        .and_then(Value::as_u64)
        .ok_or_else(|| unreadable("the survey stream summary carries no feature total"))?;
    if total != received {
        return Err(unreadable(
            "the survey stream summary disagrees with the rows received; rows were lost",
        ));
    }
    Ok(SurveyEntriesRead {
        project: project.to_owned(),
        form: request.form.clone(),
        total,
        truncated: received > entries.len() as u64,
        entries,
        sync: summary.get("sync").cloned(),
    })
}

/// The survey media references one entry carries, found in string
/// properties and in arrays or objects of them (a multi-photo field). A
/// string may hold several references separated as the map separates them
/// (newline, `,`, `;`, `|`), each a bare object path, a `gs://` reference or
/// a Cloud Storage URL; one is admitted only when the kernel resolves it to a
/// survey media address. The project and form it names are not filtered: a
/// migrated entry keeps the references it was captured with, and the map
/// shows them all.
fn media_of(feature: &Value) -> Vec<SurveyEntryMedia> {
    let mut found: Vec<SurveyEntryMedia> = Vec::new();
    let Some(properties) = feature.get("properties").and_then(Value::as_object) else {
        return found;
    };
    let mut candidates: Vec<(String, &str)> = Vec::new();
    for (key, value) in properties {
        collect_strings(key.clone(), value, &mut candidates, 0);
    }
    for (property, text) in &candidates {
        for token in text
            .split(['\n', '\r', ',', ';', '|'])
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            let Some((address, bucket)) = media_address(token) else {
                continue;
            };
            if !found
                .iter()
                .any(|seen| seen.object_path == address.object_path)
            {
                found.push(SurveyEntryMedia {
                    property: property.to_owned(),
                    reference: token.to_owned(),
                    bucket,
                    object_path: address.object_path,
                    thumbnail_object_path: address.thumbnail_object_path,
                });
            }
        }
    }
    found
}

/// The survey media address one stored reference names, and the bucket when
/// the reference itself names one: a bare object path, a `gs://` reference or
/// a Cloud Storage URL, read as the map reads it.
pub fn media_address(
    reference: &str,
) -> Option<(survey_media_address::SurveyMediaAddress, Option<String>)> {
    let (path, bucket) = object_reference(reference.trim())?;
    survey_media_address::parse(&path)
        .ok()
        .map(|address| (address, bucket))
}

/// The object path (and the bucket, when the reference names one) of one
/// media token, read the way the map reads it (`extractPhotoPath`).
fn object_reference(token: &str) -> Option<(String, Option<String>)> {
    let (bucket, path) = if let Some(rest) = token.strip_prefix("gs://") {
        let (bucket, path) = rest.split_once('/')?;
        (Some(bucket.to_owned()), percent_decode(path)?)
    } else if token.starts_with("https://") || token.starts_with("http://") {
        let url = url::Url::parse(token).ok()?;
        let host = url.host_str()?.to_ascii_lowercase();
        let segments: Vec<&str> = url.path_segments()?.collect();
        if host == "firebasestorage.googleapis.com" {
            let at = segments
                .windows(3)
                .position(|window| window[0] == "b" && window[2] == "o")?;
            let bucket = percent_decode(segments[at + 1])?;
            let rest = segments.get(at + 3..).filter(|rest| !rest.is_empty())?;
            (Some(bucket), percent_decode(&rest.join("/"))?)
        } else if host == "storage.googleapis.com" || host == "storage.cloud.google.com" {
            let (bucket, rest) = segments.split_first()?;
            if rest.is_empty() {
                return None;
            }
            (
                Some(percent_decode(bucket)?),
                percent_decode(&rest.join("/"))?,
            )
        } else {
            return None;
        }
    } else {
        (None, token.to_owned())
    };
    let path = path.split(['?', '#']).next()?.replace('\\', "/");
    Some((path, bucket.filter(|bucket| !bucket.is_empty())))
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Every string under one property, with its dotted field path (`data.photo`,
/// `photos.0`): survey values nest the form's own fields under `data`.
fn collect_strings<'a>(
    path: String,
    value: &'a Value,
    out: &mut Vec<(String, &'a str)>,
    depth: usize,
) {
    if depth > 3 {
        return;
    }
    match value {
        Value::String(text) => out.push((path, text)),
        Value::Array(items) => items.iter().enumerate().for_each(|(index, item)| {
            collect_strings(format!("{path}.{index}"), item, out, depth + 1)
        }),
        Value::Object(fields) => fields
            .iter()
            .for_each(|(key, item)| collect_strings(format!("{path}.{key}"), item, out, depth + 1)),
        _ => {}
    }
}

fn canonical_rfc3339(value: &str) -> Option<String> {
    let parsed = OffsetDateTime::parse(value.trim(), &Rfc3339).ok()?;
    parsed.to_offset(time::UtcOffset::UTC).format(&Rfc3339).ok()
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

    fn request(limit: usize) -> SurveyEntriesReadRequest {
        SurveyEntriesReadRequest::new("transformer_survey", None, None, false, Some(limit)).unwrap()
    }

    fn stream(rows: &[Value], summary: Value) -> Vec<u8> {
        let mut out = String::new();
        for row in rows {
            out.push_str(&row.to_string());
            out.push('\n');
        }
        out.push_str(&summary.to_string());
        out.push('\n');
        out.into_bytes()
    }

    fn entry(doc: &str, photo: &str) -> Value {
        json!({"type":"Feature","geometry":{"type":"Point","coordinates":[29.7,-2.6]},
               "properties":{"doc_id":doc,"transformer_name":"ruhuha","photo":photo,"comment":"ok"}})
    }

    #[test]
    fn a_listed_hyphenated_form_is_read_without_changing_its_slug() {
        for form in ["field_notes_-_areas", "field_notes", "f", "_legacy_"] {
            let request = SurveyEntriesReadRequest::new(form, None, None, false, Some(1))
                .expect("exact form slug");
            assert_eq!(request.form(), form);
            assert_eq!(
                request.payload("czgmdwth_gisagara")["form_slugs"],
                json!([form])
            );
        }
        assert!(SurveyEntriesReadRequest::new("f".repeat(128), None, None, false, None).is_ok());
    }

    #[test]
    fn labels_paths_and_out_of_bounds_forms_remain_invalid() {
        for form in [
            "",
            "Field Notes - Areas",
            "Field_notes_-_areas",
            " field_notes",
            "field_notes ",
            "field_notes\n",
            "field_notes\0",
            ".",
            "..",
            "../field_notes",
            "field_notes/areas",
            "field_notes\\areas",
            "field_notes..areas",
            "field_notes%2fareas",
            "field_notés",
            &"f".repeat(129),
        ] {
            let error = SurveyEntriesReadRequest::new(form, None, None, false, Some(1))
                .expect_err("invalid form remains refused");
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{form:?}");
        }
    }

    #[test]
    fn the_body_is_the_map_loaders_read_for_one_form() {
        let request = SurveyEntriesReadRequest::new(
            "transformer_survey",
            Some("2026-09-01T00:00:00+02:00"),
            Some([29.0, -3.0, 30.0, -2.0]),
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            request.payload("p1"),
            json!({"action":"query_entries","eds_project_id":"p1","form_slugs":["transformer_survey"],
                   "exclude_deleted":true,"updated_after":"2026-08-31T22:00:00Z","bbox":[29.0,-3.0,30.0,-2.0]})
        );
        assert!(SurveyEntriesReadRequest::new("Bad-Form", None, None, false, None).is_err());
        assert!(SurveyEntriesReadRequest::new("f", Some("yesterday"), None, false, None).is_err());
        assert!(SurveyEntriesReadRequest::new("f", None, None, false, Some(0)).is_err());
        assert!(SurveyEntriesReadRequest::new(
            "f",
            None,
            Some([30.0, -2.0, 29.0, -3.0]),
            false,
            None
        )
        .is_err());
    }

    #[test]
    fn a_verified_stream_keeps_values_names_media_and_bounds_rows() {
        let photo = "p1/forms/transformer_survey/doc1/photo_1.jpg";
        let rows = [
            entry("doc1", photo),
            entry("doc2", "not a media path"),
            entry("doc3", "other/forms/x/d/f.jpg"),
        ];
        let read = decode(
            &stream(
                &rows,
                json!({"__type":"summary","form_count":1,"errors":[],"total_features":3}),
            ),
            "p1",
            &request(2),
        )
        .unwrap();
        assert_eq!(read.total, 3);
        assert!(read.truncated);
        assert_eq!(read.entries.len(), 2);
        assert_eq!(read.entries[0].feature["properties"]["comment"], "ok");
        assert_eq!(read.entries[0].media.len(), 1);
        assert_eq!(read.entries[0].media[0].property, "photo");
        assert!(read.entries[1].media.is_empty());
    }

    #[test]
    fn a_partial_stream_is_refused_rather_than_returned_whole() {
        let rows = [entry("doc1", "x")];
        // Cut off: no trailer.
        let cut = rows[0].to_string().into_bytes();
        assert!(decode(&cut, "p1", &request(10)).is_err());
        // A form failed part-way.
        let failed = stream(
            &rows,
            json!({"__type":"summary","form_count":1,"errors":[{"form":"transformer_survey","error":"x"}],"total_features":1}),
        );
        assert!(decode(&failed, "p1", &request(10)).is_err());
        // Rows lost: the total disagrees.
        let lost = stream(
            &rows,
            json!({"__type":"summary","form_count":1,"errors":[],"total_features":2}),
        );
        assert!(decode(&lost, "p1", &request(10)).is_err());
        // Anything after the trailer.
        let mut after = stream(
            &rows,
            json!({"__type":"summary","form_count":1,"errors":[],"total_features":1}),
        );
        after.extend_from_slice(b"{\"type\":\"Feature\"}\n");
        assert!(decode(&after, "p1", &request(10)).is_err());
    }

    #[test]
    fn media_references_are_read_in_every_form_the_map_reads() {
        let feature = json!({"type":"Feature","geometry":null,"properties":{
            "photos": "https://firebasestorage.googleapis.com/v0/b/electrification-rural.firebasestorage.app/o/projects%2Fp1%2Fforms%2Fpole%2Fe1%2Fphoto%2Fa.jpg?alt=media&token=t; gs://bucket-b/p1/forms/pole/e1/b.jpg",
            "more": ["projects/p1/forms/pole/e1/photo/a.jpg", {"url": "p0/forms/pole/e9/c.jpg"}],
            "remark": "see photo, comma separated, not a path",
            "link": "https://example.com/p1/forms/pole/e1/d.jpg"
        }});
        let media = media_of(&feature);
        let mut paths: Vec<(&str, Option<&str>, &str)> = media
            .iter()
            .map(|m| {
                (
                    m.property.as_str(),
                    m.bucket.as_deref(),
                    m.object_path.as_str(),
                )
            })
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            [
                ("more.1.url", None, "p0/forms/pole/e9/c.jpg"),
                ("photos", Some("bucket-b"), "p1/forms/pole/e1/b.jpg"),
                (
                    "photos",
                    Some("electrification-rural.firebasestorage.app"),
                    "projects/p1/forms/pole/e1/photo/a.jpg"
                ),
            ],
            "{media:?}"
        );
    }
}
