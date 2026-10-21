use serde::de::Deserializer;
use serde::Deserialize;
use serde_json::value::RawValue;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::survey_entry_create::Refusal;

const MAX_PROJECT_CHARS: usize = 200;
const MAX_IDENTIFIER_CHARS: usize = 128;
const MAX_IDENTITY_BYTES: usize = 512;
const MAX_GEOMETRY_BYTES: usize = 256 * 1024;
const MAX_ENVELOPE_TEXT_CHARS: usize = 512;

pub const SURVEY_ENTRIES_CHANGES_DEFAULT_LIMIT: u16 = 100;
pub const SURVEY_ENTRIES_CHANGES_MAX_LIMIT: u16 = 500;
pub const SURVEY_ENTRIES_CHANGES_MAX_CURSOR_BYTES: usize = 4096;

/// One closed request for a fenced page of coalesced Survey mirror changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesChangesRequest {
    form: String,
    updated_after: String,
    limit: u16,
    cursor: Option<String>,
}

impl SurveyEntriesChangesRequest {
    pub fn new(
        form: impl Into<String>,
        updated_after: impl Into<String>,
        limit: Option<u16>,
        cursor: Option<String>,
    ) -> Result<Self, Refusal> {
        let updated_after = canonicalize_rfc3339(&updated_after.into()).ok_or_else(invalid)?;
        let request = Self {
            form: form.into(),
            updated_after,
            limit: limit.unwrap_or(SURVEY_ENTRIES_CHANGES_DEFAULT_LIMIT),
            cursor,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn form(&self) -> &str {
        &self.form
    }

    /// Canonical UTC RFC3339Nano inclusive lower mirror clock.
    pub fn updated_after(&self) -> &str {
        &self.updated_after
    }

    pub const fn limit(&self) -> u16 {
        self.limit
    }

    /// Opaque backend-authenticated continuation; the core never decodes it.
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    fn validate(&self) -> Result<(), Refusal> {
        if !identifier(&self.form)
            || !canonical_utc_rfc3339_nano(&self.updated_after)
            || !(1..=SURVEY_ENTRIES_CHANGES_MAX_LIMIT).contains(&self.limit)
            || self
                .cursor
                .as_deref()
                .is_some_and(|cursor| !bounded_cursor(cursor))
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn body_bytes(&self, ds_project: &str) -> Vec<u8> {
        #[derive(serde::Serialize)]
        struct Body<'a> {
            ds_project: &'a str,
            form: &'a str,
            updated_after: &'a str,
            limit: u16,
            #[serde(skip_serializing_if = "Option::is_none")]
            cursor: Option<&'a str>,
        }

        serde_json::to_vec(&Body {
            ds_project,
            form: &self.form,
            updated_after: &self.updated_after,
            limit: self.limit,
            cursor: self.cursor.as_deref(),
        })
        .expect("the closed survey entry changes request always serializes")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesChanges {
    project: String,
    form: String,
    updated_after: String,
    upper_fence: String,
    rows: Vec<SurveyEntriesChangeRow>,
    next_cursor: Option<String>,
    has_more: bool,
    complete: bool,
    consistency: SurveyEntriesChangesConsistency,
}

impl SurveyEntriesChanges {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn form(&self) -> &str {
        &self.form
    }
    pub fn updated_after(&self) -> &str {
        &self.updated_after
    }
    pub fn upper_fence(&self) -> &str {
        &self.upper_fence
    }
    pub fn rows(&self) -> &[SurveyEntriesChangeRow] {
        &self.rows
    }
    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
    pub const fn complete(&self) -> bool {
        self.complete
    }
    pub const fn consistency(&self) -> &SurveyEntriesChangesConsistency {
        &self.consistency
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesChangeRow {
    doc_id: String,
    geometry: Option<String>,
    created_by: String,
    is_deleted: bool,
    firestore_updated_at: String,
}

impl SurveyEntriesChangeRow {
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }
    pub fn geometry_json(&self) -> Option<&str> {
        self.geometry.as_deref()
    }
    pub fn created_by(&self) -> &str {
        &self.created_by
    }
    pub const fn is_deleted(&self) -> bool {
        self.is_deleted
    }
    pub fn firestore_updated_at(&self) -> &str {
        &self.firestore_updated_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesChangesConsistency {
    source: String,
    sync: String,
    fence: String,
    firestore_snapshot: bool,
    immutable_page_fence: bool,
}

impl SurveyEntriesChangesConsistency {
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn sync(&self) -> &str {
        &self.sync
    }
    pub fn fence(&self) -> &str {
        &self.fence
    }
    pub const fn firestore_snapshot(&self) -> bool {
        self.firestore_snapshot
    }
    pub const fn immutable_page_fence(&self) -> bool {
        self.immutable_page_fence
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    success: bool,
    message: String,
    data: ResultWire,
    timestamp: String,
    service: String,
    version: String,
    #[serde(default)]
    request_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultWire {
    project: String,
    form: String,
    updated_after: String,
    upper_fence: String,
    rows: Vec<RowWire>,
    #[serde(default, deserialize_with = "deserialize_present")]
    next_cursor: Presence<String>,
    has_more: bool,
    complete: bool,
    consistency: ConsistencyWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowWire {
    doc_id: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    geometry: Presence<Box<RawValue>>,
    #[serde(default)]
    created_by: String,
    is_deleted: bool,
    firestore_updated_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsistencyWire {
    source: String,
    sync: String,
    fence: String,
    firestore_snapshot: bool,
    immutable_page_fence: bool,
}

#[derive(Default)]
enum Presence<T> {
    #[default]
    Missing,
    Present(T),
}

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Presence<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Presence::Present)
}

pub fn validate_project(ds_project: &str) -> Result<(), Refusal> {
    if ds_project.is_empty()
        || ds_project.trim() != ds_project
        || ds_project.chars().count() > MAX_PROJECT_CHARS
        || ds_project.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn decode(
    body: &[u8],
    response_limit: usize,
    ds_project: &str,
    request: &SurveyEntriesChangesRequest,
) -> Result<SurveyEntriesChanges, Refusal> {
    if body.len() > response_limit {
        return Err(unreadable());
    }
    let envelope: Envelope = serde_json::from_slice(body).map_err(|_| unreadable())?;
    let lower = parse_canonical_utc(&request.updated_after).ok_or_else(unreadable)?;
    let upper = parse_canonical_utc(&envelope.data.upper_fence).ok_or_else(unreadable)?;
    if !envelope.success
        || envelope.message != "Survey entries changes"
        || envelope.service != "data-solutions-backend"
        || envelope.version != "1.0.0"
        || !canonical_utc_rfc3339_nano(&envelope.timestamp)
        || (!envelope.request_id.is_empty()
            && !bounded_text(&envelope.request_id, MAX_ENVELOPE_TEXT_CHARS))
        || envelope.data.project != ds_project
        || envelope.data.form != request.form
        || envelope.data.updated_after != request.updated_after
        || upper < lower
        || envelope.data.rows.len() > usize::from(request.limit)
        || envelope.data.has_more == envelope.data.complete
        || (envelope.data.has_more && envelope.data.rows.len() != usize::from(request.limit))
        || (envelope.data.has_more
            && !matches!(
                &envelope.data.next_cursor,
                Presence::Present(cursor) if bounded_cursor(cursor)
            ))
        || (envelope.data.complete && !matches!(&envelope.data.next_cursor, Presence::Missing))
        || envelope.data.consistency.source != "survey_mirror"
        || envelope.data.consistency.sync != "ensure_synced_all"
        || envelope.data.consistency.fence != "bigquery_system_time"
        || envelope.data.consistency.firestore_snapshot
        || !envelope.data.consistency.immutable_page_fence
    {
        return Err(unreadable());
    }

    let mut rows = Vec::with_capacity(envelope.data.rows.len());
    let mut previous: Option<(OffsetDateTime, &str)> = None;
    for row in &envelope.data.rows {
        let clock = parse_canonical_utc(&row.firestore_updated_at).ok_or_else(unreadable)?;
        if !bounded_identity(&row.doc_id, false)
            || !bounded_identity(&row.created_by, true)
            || clock < lower
            || clock > upper
            || previous.is_some_and(|(time, doc_id)| {
                clock < time || (clock == time && row.doc_id.as_str() <= doc_id)
            })
        {
            return Err(unreadable());
        }
        let geometry = match &row.geometry {
            Presence::Present(raw) => {
                let raw = raw.get();
                if raw.len() > MAX_GEOMETRY_BYTES
                    || !is_compact_json(raw.as_bytes())
                    || !valid_geometry(raw)
                {
                    return Err(unreadable());
                }
                Some(raw.to_owned())
            }
            Presence::Missing => None,
        };
        previous = Some((clock, &row.doc_id));
        rows.push(SurveyEntriesChangeRow {
            doc_id: row.doc_id.clone(),
            geometry,
            created_by: row.created_by.clone(),
            is_deleted: row.is_deleted,
            firestore_updated_at: row.firestore_updated_at.clone(),
        });
    }

    Ok(SurveyEntriesChanges {
        project: envelope.data.project,
        form: envelope.data.form,
        updated_after: envelope.data.updated_after,
        upper_fence: envelope.data.upper_fence,
        rows,
        next_cursor: match envelope.data.next_cursor {
            Presence::Missing => None,
            Presence::Present(cursor) => Some(cursor),
        },
        has_more: envelope.data.has_more,
        complete: envelope.data.complete,
        consistency: SurveyEntriesChangesConsistency {
            source: envelope.data.consistency.source,
            sync: envelope.data.consistency.sync,
            fence: envelope.data.consistency.fence,
            firestore_snapshot: envelope.data.consistency.firestore_snapshot,
            immutable_page_fence: envelope.data.consistency.immutable_page_fence,
        },
    })
}

fn canonicalize_rfc3339(value: &str) -> Option<String> {
    let parsed = OffsetDateTime::parse(value.trim(), &Rfc3339).ok()?;
    parsed.to_offset(time::UtcOffset::UTC).format(&Rfc3339).ok()
}

fn parse_canonical_utc(value: &str) -> Option<OffsetDateTime> {
    if !canonical_utc_rfc3339_nano(value) {
        return None;
    }
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

pub(crate) fn canonical_utc_rfc3339_nano(value: &str) -> bool {
    !value.is_empty() && canonicalize_rfc3339(value).as_deref() == Some(value)
}

fn identifier(input: &str) -> bool {
    !input.is_empty()
        && input.len() <= MAX_IDENTIFIER_CHARS
        && input
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn bounded_cursor(input: &str) -> bool {
    !input.is_empty()
        && input.len() <= SURVEY_ENTRIES_CHANGES_MAX_CURSOR_BYTES
        && !input.bytes().any(|byte| byte.is_ascii_whitespace())
}

fn bounded_identity(input: &str, empty_allowed: bool) -> bool {
    (empty_allowed || !input.is_empty())
        && input.len() <= MAX_IDENTITY_BYTES
        && input.trim() == input
        && !input.chars().any(char::is_control)
}

fn bounded_text(input: &str, max: usize) -> bool {
    !input.is_empty()
        && input.trim() == input
        && input.chars().count() <= max
        && !input.chars().any(char::is_control)
}

fn is_compact_json(bytes: &[u8]) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte.is_ascii_whitespace() {
            return false;
        }
    }
    !in_string && !escaped
}

fn valid_geometry(raw: &str) -> bool {
    serde_json::from_str(raw)
        .ok()
        .as_ref()
        .is_some_and(valid_geometry_value)
}

fn valid_geometry_value(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(kind) = object.get("type").and_then(serde_json::Value::as_str) else {
        return false;
    };
    match kind {
        "Point" => {
            exact_keys(object, &["coordinates", "type"])
                && object.get("coordinates").is_some_and(valid_position)
        }
        "MultiPoint" => {
            exact_keys(object, &["coordinates", "type"])
                && object
                    .get("coordinates")
                    .is_some_and(|coordinates| valid_positions(coordinates, 1))
        }
        "LineString" => {
            exact_keys(object, &["coordinates", "type"])
                && object
                    .get("coordinates")
                    .is_some_and(|coordinates| valid_positions(coordinates, 2))
        }
        "MultiLineString" => {
            exact_keys(object, &["coordinates", "type"])
                && object
                    .get("coordinates")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|lines| {
                        !lines.is_empty() && lines.iter().all(|line| valid_positions(line, 2))
                    })
        }
        "Polygon" => {
            exact_keys(object, &["coordinates", "type"])
                && object.get("coordinates").is_some_and(valid_polygon)
        }
        "MultiPolygon" => {
            exact_keys(object, &["coordinates", "type"])
                && object
                    .get("coordinates")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|polygons| {
                        !polygons.is_empty() && polygons.iter().all(valid_polygon)
                    })
        }
        "GeometryCollection" => {
            exact_keys(object, &["geometries", "type"])
                && object
                    .get("geometries")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|geometries| geometries.iter().all(valid_geometry_value))
        }
        _ => false,
    }
}

fn exact_keys(object: &serde_json::Map<String, serde_json::Value>, expected: &[&str]) -> bool {
    object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
}

fn valid_position(value: &serde_json::Value) -> bool {
    value
        .as_array()
        .is_some_and(|position| position.len() >= 2 && position.iter().all(valid_coordinate))
}

fn valid_coordinate(value: &serde_json::Value) -> bool {
    value.as_f64().is_some_and(f64::is_finite)
}

fn valid_positions(value: &serde_json::Value, minimum: usize) -> bool {
    value
        .as_array()
        .is_some_and(|positions| positions.len() >= minimum && positions.iter().all(valid_position))
}

fn valid_polygon(value: &serde_json::Value) -> bool {
    value
        .as_array()
        .is_some_and(|rings| !rings.is_empty() && rings.iter().all(|ring| valid_positions(ring, 4)))
}

fn invalid() -> Refusal {
    Refusal::InvalidInput
}

fn unreadable() -> Refusal {
    Refusal::UnreadableResponse
}
