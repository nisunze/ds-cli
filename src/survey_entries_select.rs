use serde::Deserialize;
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

use crate::survey_entry_create::Refusal;

const MAX_PROJECT_CHARS: usize = 200;
const MAX_IDENTIFIER_CHARS: usize = 128;
const MAX_IDENTITY_BYTES: usize = 512;
const MAX_GEOMETRY_BYTES: usize = 256 * 1024;
const MAX_ENVELOPE_TEXT_CHARS: usize = 512;

pub const SURVEY_ENTRIES_SELECT_DEFAULT_LIMIT: u16 = 100;
pub const SURVEY_ENTRIES_SELECT_MAX_LIMIT: u16 = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct SurveyEntriesSelectRequest {
    form: String,
    bbox: [f64; 4],
    limit: u16,
}

impl SurveyEntriesSelectRequest {
    pub fn new(
        form: impl Into<String>,
        bbox: [f64; 4],
        limit: Option<u16>,
    ) -> Result<Self, Refusal> {
        let mut request = Self {
            form: form.into(),
            bbox,
            limit: limit.unwrap_or(SURVEY_ENTRIES_SELECT_DEFAULT_LIMIT),
        };
        request.validate()?;
        for coordinate in &mut request.bbox {
            if *coordinate == 0.0 {
                *coordinate = 0.0;
            }
        }
        Ok(request)
    }

    pub fn form(&self) -> &str {
        &self.form
    }

    pub const fn bbox(&self) -> [f64; 4] {
        self.bbox
    }

    pub const fn limit(&self) -> u16 {
        self.limit
    }

    fn validate(&self) -> Result<(), Refusal> {
        let [west, south, east, north] = self.bbox;
        if !identifier(&self.form)
            || self.bbox.iter().any(|coordinate| !coordinate.is_finite())
            || !(-180.0..=180.0).contains(&west)
            || !(-180.0..=180.0).contains(&east)
            || !(-90.0..=90.0).contains(&south)
            || !(-90.0..=90.0).contains(&north)
            || west >= east
            || south >= north
            || !(1..=SURVEY_ENTRIES_SELECT_MAX_LIMIT).contains(&self.limit)
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
            bbox: [f64; 4],
            limit: u16,
        }

        serde_json::to_vec(&Body {
            ds_project,
            form: &self.form,
            bbox: self.bbox,
            limit: self.limit,
        })
        .expect("the closed survey entry selection request always serializes")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesSelection {
    project: String,
    form: String,
    bbox: [u64; 4],
    rows: Vec<SurveyEntriesSelectRow>,
    truncated: bool,
    complete: bool,
    selection_digest: String,
    consistency: SurveyEntriesSelectConsistency,
}

impl SurveyEntriesSelection {
    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn form(&self) -> &str {
        &self.form
    }

    pub fn bbox(&self) -> [f64; 4] {
        self.bbox.map(f64::from_bits)
    }

    pub fn rows(&self) -> &[SurveyEntriesSelectRow] {
        &self.rows
    }

    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    pub const fn complete(&self) -> bool {
        self.complete
    }

    pub fn selection_digest(&self) -> &str {
        &self.selection_digest
    }

    pub const fn consistency(&self) -> &SurveyEntriesSelectConsistency {
        &self.consistency
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesSelectRow {
    doc_id: String,
    geometry: String,
    created_by: String,
    firestore_updated_at: String,
}

impl SurveyEntriesSelectRow {
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }

    pub fn geometry_json(&self) -> &str {
        &self.geometry
    }

    pub fn created_by(&self) -> &str {
        &self.created_by
    }

    pub fn firestore_updated_at(&self) -> &str {
        &self.firestore_updated_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesSelectConsistency {
    source: String,
    sync: String,
    snapshot: bool,
    mutable: bool,
}

impl SurveyEntriesSelectConsistency {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn sync(&self) -> &str {
        &self.sync
    }

    pub const fn snapshot(&self) -> bool {
        self.snapshot
    }

    pub const fn mutable(&self) -> bool {
        self.mutable
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
    bbox: [f64; 4],
    rows: Vec<RowWire>,
    truncated: bool,
    complete: bool,
    selection_digest: String,
    consistency: ConsistencyWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowWire {
    doc_id: String,
    geometry: Box<RawValue>,
    #[serde(default)]
    created_by: String,
    #[serde(default)]
    firestore_updated_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsistencyWire {
    source: String,
    sync: String,
    snapshot: bool,
    mutable: bool,
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
    request: &SurveyEntriesSelectRequest,
) -> Result<SurveyEntriesSelection, Refusal> {
    if body.len() > response_limit {
        return Err(unreadable());
    }
    let envelope: Envelope = serde_json::from_slice(&body).map_err(|_| unreadable())?;
    if !envelope.success
        || envelope.message != "Survey entry selection"
        || envelope.service != "data-solutions-backend"
        || envelope.version != "1.0.0"
        || !bounded_text(&envelope.timestamp, MAX_ENVELOPE_TEXT_CHARS)
        || (!envelope.request_id.is_empty()
            && !bounded_text(&envelope.request_id, MAX_ENVELOPE_TEXT_CHARS))
        || envelope.data.project != ds_project
        || envelope.data.form != request.form
        || !same_bbox(envelope.data.bbox, request.bbox)
        || envelope.data.rows.len() > usize::from(request.limit)
        || envelope.data.truncated == envelope.data.complete
        || (envelope.data.truncated && envelope.data.rows.len() != usize::from(request.limit))
        || envelope.data.consistency.source != "survey_mirror"
        || envelope.data.consistency.sync != "ensure_synced_all"
        || envelope.data.consistency.snapshot
        || !envelope.data.consistency.mutable
        || !lower_sha256(&envelope.data.selection_digest)
    {
        return Err(unreadable());
    }

    let mut rows = Vec::with_capacity(envelope.data.rows.len());
    let mut previous_doc_id: Option<&str> = None;
    for row in &envelope.data.rows {
        if !bounded_identity(&row.doc_id, false)
            || !bounded_identity(&row.created_by, true)
            || !canonical_utc_rfc3339_nano(&row.firestore_updated_at)
            || previous_doc_id.is_some_and(|previous| previous >= row.doc_id.as_str())
        {
            return Err(unreadable());
        }
        let geometry = row.geometry.get();
        if geometry.len() > MAX_GEOMETRY_BYTES
            || !is_compact_json(geometry.as_bytes())
            || !valid_geometry(geometry)
        {
            return Err(unreadable());
        }
        previous_doc_id = Some(&row.doc_id);
        rows.push(SurveyEntriesSelectRow {
            doc_id: row.doc_id.clone(),
            geometry: geometry.to_owned(),
            created_by: row.created_by.clone(),
            firestore_updated_at: row.firestore_updated_at.clone(),
        });
    }

    let consistency = SurveyEntriesSelectConsistency {
        source: envelope.data.consistency.source,
        sync: envelope.data.consistency.sync,
        snapshot: envelope.data.consistency.snapshot,
        mutable: envelope.data.consistency.mutable,
    };
    let selection = SurveyEntriesSelection {
        project: envelope.data.project,
        form: envelope.data.form,
        bbox: envelope.data.bbox.map(f64::to_bits),
        rows,
        truncated: envelope.data.truncated,
        complete: envelope.data.complete,
        selection_digest: envelope.data.selection_digest,
        consistency,
    };
    let digest = Sha256::digest(selection_digest_bytes(request.limit, &selection));
    if selection.selection_digest != format!("{digest:x}") {
        return Err(unreadable());
    }
    Ok(selection)
}

fn selection_digest_bytes(limit: u16, result: &SurveyEntriesSelection) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"ds.survey.entries.selection/v1\n");
    append_field(&mut out, "project", &result.project);
    append_field(&mut out, "form", &result.form);
    let bbox = format!(
        "bbox {:016x},{:016x},{:016x},{:016x}\n",
        result.bbox[0], result.bbox[1], result.bbox[2], result.bbox[3]
    );
    out.extend_from_slice(bbox.as_bytes());
    out.extend_from_slice(format!("limit {limit}\n").as_bytes());
    out.extend_from_slice(format!("truncated {}\n", bool_digit(result.truncated)).as_bytes());
    out.extend_from_slice(format!("complete {}\n", bool_digit(result.complete)).as_bytes());
    append_field(&mut out, "consistency.source", &result.consistency.source);
    append_field(&mut out, "consistency.sync", &result.consistency.sync);
    out.extend_from_slice(
        format!(
            "consistency.snapshot {}\n",
            bool_digit(result.consistency.snapshot)
        )
        .as_bytes(),
    );
    out.extend_from_slice(
        format!(
            "consistency.mutable {}\n",
            bool_digit(result.consistency.mutable)
        )
        .as_bytes(),
    );
    out.extend_from_slice(format!("rows {}\n", result.rows.len()).as_bytes());
    for row in &result.rows {
        out.extend_from_slice(b"row\n");
        append_field(&mut out, "doc_id", &row.doc_id);
        append_field(&mut out, "geometry", &row.geometry);
        append_field(&mut out, "created_by", &row.created_by);
        append_field(&mut out, "firestore_updated_at", &row.firestore_updated_at);
    }
    out
}

fn append_field(out: &mut Vec<u8>, name: &str, value: &str) {
    out.extend_from_slice(format!("{name} {}:", value.len()).as_bytes());
    out.extend_from_slice(value.as_bytes());
    out.push(b'\n');
}

const fn bool_digit(value: bool) -> u8 {
    if value {
        1
    } else {
        0
    }
}

fn same_bbox(left: [f64; 4], right: [f64; 4]) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| left.to_bits() == right.to_bits())
}

fn identifier(input: &str) -> bool {
    !input.is_empty()
        && input.len() <= MAX_IDENTIFIER_CHARS
        && input
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
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

fn lower_sha256(input: &str) -> bool {
    input.len() == 64
        && input
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => return false,
    };
    valid_geometry_value(&value)
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

fn canonical_utc_rfc3339_nano(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes.last() != Some(&b'Z')
    {
        return false;
    }
    for index in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !bytes[index].is_ascii_digit() {
            return false;
        }
    }
    let year = digits(bytes, 0, 4);
    let month = digits(bytes, 5, 7);
    let day = digits(bytes, 8, 10);
    let hour = digits(bytes, 11, 13);
    let minute = digits(bytes, 14, 16);
    let second = digits(bytes, 17, 19);
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return false;
    }
    if bytes.len() == 20 {
        return true;
    }
    if bytes[19] != b'.' || !(22..=30).contains(&bytes.len()) {
        return false;
    }
    let fraction = &bytes[20..bytes.len() - 1];
    !fraction.is_empty()
        && fraction.len() <= 9
        && fraction.iter().all(u8::is_ascii_digit)
        && fraction.last() != Some(&b'0')
}

fn digits(bytes: &[u8], start: usize, end: usize) -> u32 {
    bytes[start..end]
        .iter()
        .fold(0, |value, digit| value * 10 + u32::from(digit - b'0'))
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

fn invalid() -> Refusal {
    Refusal::InvalidInput
}

fn unreadable() -> Refusal {
    Refusal::UnreadableResponse
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_backend_selection_digest_vector_matches() {
        let result = SurveyEntriesSelection {
            project: "project_a".to_owned(),
            form: "survey_points".to_owned(),
            bbox: [29.0, -2.0, 30.0, -1.0].map(f64::to_bits),
            rows: vec![
                SurveyEntriesSelectRow {
                    doc_id: "a".to_owned(),
                    geometry: r#"{"type":"Point","coordinates":[29.1,-1.1]}"#.to_owned(),
                    created_by: "a@example.test".to_owned(),
                    firestore_updated_at: "2026-08-30T01:00:00Z".to_owned(),
                },
                SurveyEntriesSelectRow {
                    doc_id: "b".to_owned(),
                    geometry: r#"{"type":"Point","coordinates":[29.2,-1.2]}"#.to_owned(),
                    created_by: "b@example.test".to_owned(),
                    firestore_updated_at: "2026-08-30T02:00:00Z".to_owned(),
                },
            ],
            truncated: true,
            complete: false,
            selection_digest: String::new(),
            consistency: SurveyEntriesSelectConsistency {
                source: "survey_mirror".to_owned(),
                sync: "ensure_synced_all".to_owned(),
                snapshot: false,
                mutable: true,
            },
        };
        let digest = Sha256::digest(selection_digest_bytes(2, &result));
        assert_eq!(
            format!("{digest:x}"),
            "9a1f4cf3f07661d30aa892fd2a005e5c4a3506a46e0cc8d06177d8f4d087c5f5"
        );
    }

    #[test]
    fn canonical_timestamp_accepts_only_go_rfc3339_nano_utc_output() {
        for value in [
            "",
            "2026-08-30T01:00:00Z",
            "2026-08-30T01:00:00.1Z",
            "2026-08-30T01:00:00.123456789Z",
            "2024-02-29T23:59:59Z",
        ] {
            assert!(canonical_utc_rfc3339_nano(value), "rejected {value}");
        }
        for value in [
            "2026-08-30T01:00:00.120Z",
            "2026-08-30T03:00:00+02:00",
            "2026-02-29T01:00:00Z",
            "2026-08-30t01:00:00z",
            "2026-08-30T01:00:60Z",
        ] {
            assert!(!canonical_utc_rfc3339_nano(value), "accepted {value}");
        }
    }
}
