//! Survey moments — the survey photos one machine holds for a project, and
//! every decision the gallery makes over them.
//!
//! A moment is one cached survey original: a stored object the machine holds
//! bytes for, with what it knows about the entry the photo belongs to. The
//! rows are host facts — the Server reads them from its survey-media store,
//! the browser from its photo cache — and everything downstream of the row
//! is decided here: which rows are this project's, what a row is called,
//! its clock, the filters (form, date range, sync state, file-name text),
//! the order, the page, the form roster, whether "navigate to" is possible
//! and why not, and the one rotation policy.
//!
//! The rotation policy is the properties panel's, transcribed and not
//! re-invented (owner, 2026-09-20; rotation as metadata, owner 2026-10-21): a
//! turn is offered only for a stored ORIGINAL the kernel's address grammar
//! admits for this project — a derived thumbnail, a data: URL or a foreign
//! path is refused before any byte moves; the turn is a clockwise
//! quarter-turn (90, 180 or 270; a counter-clockwise click is 270). The
//! original is never rewritten: the turn is entry metadata
//! (`survey_media_rotation`) and only the JPEG thumbnail is regenerated. One
//! rule for a second turn while one is still waiting: the quarter-turns
//! accumulate onto the waiting net (`pending_degrees`); a full circle leaves
//! nothing to publish.
//!
//! Dates: `since`/`until` are RFC 3339 instants or bare `YYYY-MM-DD` days.
//! A bare day is a UTC calendar day; a caller whose days are local (the
//! gallery) sends the day's bounds as instants with its offset. The bound is
//! inclusive on both ends, so `--until 2026-09-19` keeps a moment at 23:59:59.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime, PrimitiveDateTime, Time};

use crate::survey_media_address::{self, SurveyMediaAddress};
use crate::survey_photo_media;

pub const SCHEMA: &str = "ds.survey-moments/v1";
pub const DEFAULT_LIMIT: usize = 60;
pub const MAX_LIMIT: usize = 500;
pub const MAX_TEXT_CHARS: usize = 128;
/// How many rows a host may hand over at once. Above this the host must
/// narrow the project's store, not the kernel.
pub const MAX_ROWS: usize = 20_000;

/// Whether this machine's bytes are the published head or still on their
/// way to it. `waiting` is what the gallery calls "waiting to sync": the
/// bytes are owned by an outbox (the browser's) or a publication row (the
/// Server's) and will be published as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Waiting,
    Synced,
}

impl SyncState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Synced => "synced",
        }
    }
}

/// What a host knows about the entry a photo belongs to. `None` on a
/// [`MomentRow`] means the host does not hold the entry at all.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntryFacts {
    /// The entry's capture/creation clock, epoch milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at_ms: Option<u64>,
    /// The entry's GeoJSON geometry, when it carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<Value>,
}

impl EntryFacts {
    /// The facts a stored survey entry feature carries, read the way the
    /// browser's survey store persists it: the capture clock is
    /// `properties.metadata.created_at`, or a server row's prefix-flattened
    /// `metadata_created_at`, or `created_at` — an RFC 3339 string or epoch
    /// milliseconds — and the geometry is the feature's, when it names a
    /// type. A feature with neither is still a held entry (without a
    /// location); the host hands the feature over and decides nothing.
    pub fn from_feature(feature: &Value) -> Self {
        let props = feature
            .get("properties")
            .filter(|v| v.is_object())
            .unwrap_or(feature);
        let metadata = props.get("metadata").filter(|v| v.is_object());
        let candidates = [
            metadata.and_then(|m| m.get("created_at")),
            props.get("metadata_created_at"),
            props.get("created_at"),
        ];
        let captured_at_ms = candidates.into_iter().flatten().find_map(|raw| match raw {
            Value::String(text) => OffsetDateTime::parse(text.trim(), &Rfc3339)
                .ok()
                .and_then(|instant| ms_of(instant).ok()),
            Value::Number(number) => number
                .as_f64()
                .filter(|ms| ms.is_finite() && *ms >= 0.0)
                .map(|ms| ms as u64),
            _ => None,
        });
        let geometry = feature
            .get("geometry")
            .filter(|geometry| geometry.get("type").and_then(Value::as_str).is_some())
            .cloned();
        Self {
            captured_at_ms,
            geometry,
        }
    }
}

/// One cached survey original as a host's store keeps it. Both stores map
/// to this shape; nothing here is decided.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MomentRow {
    /// The stored object reference, as cached.
    pub path: String,
    pub media_type: String,
    pub size: u64,
    /// When this machine took the bytes, epoch milliseconds.
    pub cached_at_ms: u64,
    pub state: SyncState,
    /// Whether the store holds the JPEG companion under the thumbnail path.
    #[serde(default)]
    pub thumbnail: bool,
    /// The entry, when the host holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryFacts>,
    /// The stored entry feature itself, for a host that holds the feature
    /// and reads nothing off it: the kernel derives the facts
    /// ([`EntryFacts::from_feature`]) when `entry` is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<Value>,
    /// The storage generation these bytes equal (`synced`) or were pinned
    /// to (`waiting`), when the host recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl MomentRow {
    /// The entry facts: declared, else derived from the held feature.
    fn facts(&self) -> Option<EntryFacts> {
        self.entry
            .clone()
            .or_else(|| self.feature.as_ref().map(EntryFacts::from_feature))
    }
}

/// Why "navigate to" is not on offer for a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigateRefusal {
    /// The entry the photo belongs to is not held by this machine.
    EntryNotLocal,
    /// The entry is held but carries no location.
    EntryWithoutLocation,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Navigate {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<NavigateRefusal>,
}

/// One moment as the gallery, the viewer and the "i" panel show it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Moment {
    pub path: String,
    pub file: String,
    pub project: String,
    pub form: String,
    pub entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub thumbnail_path: String,
    pub thumbnail: bool,
    pub media_type: String,
    pub size: u64,
    pub state: SyncState,
    pub cached_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at_ms: Option<u64>,
    /// The moment's clock: the entry's capture time when the entry is held,
    /// else when this machine cached the bytes. Sorting and the date filter
    /// read this.
    pub when_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Value>,
    pub navigate: Navigate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// The gallery's filter. Every criterion composes with AND; an absent
/// criterion is no bound.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    /// Any of these forms; composes with `form` as one set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncState>,
    /// Case-insensitive substring of the file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// The filter as it was applied — bounds resolved to instants, text trimmed.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedFilter {
    /// The forms kept, any of; empty is every form.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub limit: usize,
    pub empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormCount {
    pub form: String,
    pub count: usize,
}

/// The list answer: N of M, newest first.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListAnswer {
    pub schema: String,
    pub project: String,
    pub filter: AppliedFilter,
    pub moments: Vec<Moment>,
    /// Rows answered.
    pub count: usize,
    /// Rows the filter kept.
    pub matched: usize,
    /// This project's rows the machine holds, before the filter.
    pub total: usize,
    pub more: bool,
    pub waiting: usize,
    pub synced: usize,
    /// The forms present among the project's rows, most frequent first.
    pub forms: Vec<FormCount>,
}

/// A typed refusal a host relays as it is.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub remedy: &'static str,
}

impl Refusal {
    fn new(code: &'static str, message: impl Into<String>, remedy: &'static str) -> Self {
        Self {
            code,
            message: message.into(),
            remedy,
        }
    }
}

pub const INVALID_FILTER: &str = "invalid_filter";
pub const NOT_SURVEY_MEDIA: &str = "not_survey_media";
pub const PROJECT_MISMATCH: &str = "project_mismatch";
pub const DERIVED_THUMBNAIL: &str = "derived_thumbnail";
pub const INVALID_TURN: &str = "invalid_turn";
pub const MOMENT_NOT_HELD: &str = "moment_not_held";

const REMEDY_FILTER: &str =
    "pass a form slug, RFC 3339 instants or YYYY-MM-DD days, waiting|synced, text up to 128 characters and a limit from 1 through 500";
const REMEDY_ADDRESS: &str =
    "pass the canonical original object path of a survey photo in this project, not a URL or thumbnail";
const REMEDY_TURN: &str = "choose 90, 180 or 270 clockwise degrees";
const REMEDY_HELD: &str = "list the moments this machine holds with survey moments list";

fn file_name_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The moment's clock: the entry's capture time when held, else the cache time.
pub fn when_ms(row: &MomentRow) -> u64 {
    row.facts()
        .and_then(|entry| entry.captured_at_ms)
        .unwrap_or(row.cached_at_ms)
}

fn navigate(row: &MomentRow) -> Navigate {
    match &row.facts() {
        None => Navigate {
            enabled: false,
            reason: Some(NavigateRefusal::EntryNotLocal),
        },
        Some(entry) => match &entry.geometry {
            Some(geometry) if geometry.get("type").and_then(Value::as_str).is_some() => Navigate {
                enabled: true,
                reason: None,
            },
            _ => Navigate {
                enabled: false,
                reason: Some(NavigateRefusal::EntryWithoutLocation),
            },
        },
    }
}

/// Describe one held row. `None` when the row is not a survey original of
/// this project — a derived thumbnail, another project's photo, a key the
/// address grammar refuses — which the gallery never shows.
pub fn describe(project: &str, row: &MomentRow) -> Option<Moment> {
    let address = survey_media_address::parse(&row.path).ok()?;
    if address.project != project || address.object_path == address.thumbnail_object_path {
        return None;
    }
    let facts = row.facts();
    let location = facts.as_ref().and_then(|entry| {
        entry
            .geometry
            .as_ref()
            .filter(|geometry| geometry.get("type").and_then(Value::as_str).is_some())
            .cloned()
    });
    Some(Moment {
        path: row.path.clone(),
        file: if address.file.is_empty() {
            file_name_of(&row.path).to_string()
        } else {
            address.file.clone()
        },
        project: address.project,
        form: address.form,
        entry: address.entry,
        field: address.field,
        thumbnail_path: address.thumbnail_object_path,
        thumbnail: row.thumbnail,
        media_type: row.media_type.clone(),
        size: row.size,
        state: row.state,
        cached_at_ms: row.cached_at_ms,
        captured_at_ms: facts.as_ref().and_then(|entry| entry.captured_at_ms),
        when_ms: when_ms(row),
        location,
        navigate: navigate(row),
        generation: row.generation.clone(),
        sha256: row.sha256.clone(),
    })
}

/// The details the viewer's "i" panel shows: the described row, or the one
/// refusal when the machine does not hold it as a survey original of this
/// project.
pub fn read(project: &str, path: &str, row: Option<&MomentRow>) -> Result<Moment, Refusal> {
    let row = row.filter(|row| row.path == path).ok_or_else(|| {
        Refusal::new(
            MOMENT_NOT_HELD,
            format!("this machine holds no survey photo at {path} for project {project}"),
            REMEDY_HELD,
        )
    })?;
    describe(project, row).ok_or_else(|| {
        Refusal::new(
            NOT_SURVEY_MEDIA,
            format!("{path} is not a survey original of project {project}"),
            REMEDY_ADDRESS,
        )
    })
}

fn ms_of(instant: OffsetDateTime) -> Result<u64, ()> {
    let nanos = instant.unix_timestamp_nanos();
    if nanos < 0 {
        return Err(());
    }
    u64::try_from(nanos / 1_000_000).map_err(|_| ())
}

/// A bound: an RFC 3339 instant, or a bare UTC day (its first or last
/// millisecond, by `end`).
fn bound_ms(raw: &str, end: bool) -> Option<u64> {
    let value = raw.trim();
    if let Ok(instant) = OffsetDateTime::parse(value, &Rfc3339) {
        return ms_of(instant).ok();
    }
    let format = time::format_description::parse("[year]-[month]-[day]").ok()?;
    let day = Date::parse(value, &format).ok()?;
    let time = if end {
        Time::from_hms_milli(23, 59, 59, 999).ok()?
    } else {
        Time::MIDNIGHT
    };
    ms_of(PrimitiveDateTime::new(day, time).assume_utc()).ok()
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= survey_media_address::MAX_SEGMENT_CHARS
        && value.trim() == value
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Resolve the filter once: bounds to instants, text trimmed and lowered,
/// limit bounded. A malformed criterion is one typed refusal, never an empty
/// grid and never "no bound".
pub fn resolve(filter: &Filter) -> Result<AppliedFilter, Refusal> {
    let invalid = |what: &str| Refusal::new(INVALID_FILTER, what.to_string(), REMEDY_FILTER);
    let mut forms: Vec<String> = Vec::new();
    for slug in filter.form.iter().chain(filter.forms.iter()) {
        let slug = slug.trim();
        if slug.is_empty() {
            continue;
        }
        if !valid_slug(slug) {
            return Err(invalid(&format!("form `{slug}` is not a form slug")));
        }
        if !forms.iter().any(|kept| kept == slug) {
            forms.push(slug.to_string());
        }
    }
    let since_ms = match filter.since.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(bound_ms(raw, false).ok_or_else(|| {
            invalid(&format!(
                "since `{raw}` is not an RFC 3339 instant or a YYYY-MM-DD day"
            ))
        })?),
    };
    let until_ms = match filter.until.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(bound_ms(raw, true).ok_or_else(|| {
            invalid(&format!(
                "until `{raw}` is not an RFC 3339 instant or a YYYY-MM-DD day"
            ))
        })?),
    };
    if let (Some(since), Some(until)) = (since_ms, until_ms) {
        if since > until {
            return Err(invalid("since is after until"));
        }
    }
    let text = match filter.text.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(text) if text.chars().count() <= MAX_TEXT_CHARS => Some(text.to_lowercase()),
        Some(_) => return Err(invalid("text exceeds 128 characters")),
    };
    let limit = match filter.limit {
        None => DEFAULT_LIMIT,
        Some(limit) if (1..=MAX_LIMIT).contains(&limit) => limit,
        Some(limit) => return Err(invalid(&format!("limit {limit} is outside 1 through 500"))),
    };
    Ok(AppliedFilter {
        empty: forms.is_empty()
            && since_ms.is_none()
            && until_ms.is_none()
            && filter.sync.is_none()
            && text.is_none(),
        forms,
        since_ms,
        until_ms,
        sync: filter.sync,
        text,
        limit,
    })
}

fn keeps(filter: &AppliedFilter, moment: &Moment) -> bool {
    if filter.since_ms.is_some_and(|since| moment.when_ms < since) {
        return false;
    }
    if filter.until_ms.is_some_and(|until| moment.when_ms > until) {
        return false;
    }
    if !filter.forms.is_empty() && !filter.forms.contains(&moment.form) {
        return false;
    }
    if filter.sync.is_some_and(|sync| moment.state != sync) {
        return false;
    }
    if filter
        .text
        .as_deref()
        .is_some_and(|text| !moment.file.to_lowercase().contains(text))
    {
        return false;
    }
    true
}

/// The forms present among the moments, most frequent first, then by slug.
pub fn form_counts(moments: &[Moment]) -> Vec<FormCount> {
    let mut counts: Vec<FormCount> = Vec::new();
    for moment in moments {
        match counts.iter_mut().find(|entry| entry.form == moment.form) {
            Some(entry) => entry.count += 1,
            None => counts.push(FormCount {
                form: moment.form.clone(),
                count: 1,
            }),
        }
    }
    counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.form.cmp(&b.form)));
    counts
}

/// The gallery: this project's survey originals among `rows`, filtered,
/// newest first, bounded. Rows that are not this project's originals are
/// not counted anywhere.
pub fn list(project: &str, rows: &[MomentRow], filter: &Filter) -> Result<ListAnswer, Refusal> {
    if rows.len() > MAX_ROWS {
        return Err(Refusal::new(
            INVALID_FILTER,
            format!("{} rows exceed the {MAX_ROWS}-row bound", rows.len()),
            "narrow the store the host reads before asking the kernel",
        ));
    }
    let applied = resolve(filter)?;
    let mut all: Vec<Moment> = rows
        .iter()
        .filter_map(|row| describe(project, row))
        .collect();
    all.sort_by(|a, b| b.when_ms.cmp(&a.when_ms).then_with(|| a.path.cmp(&b.path)));
    let total = all.len();
    let waiting = all.iter().filter(|m| m.state == SyncState::Waiting).count();
    let synced = total - waiting;
    let forms = form_counts(&all);
    let kept: Vec<Moment> = all.into_iter().filter(|m| keeps(&applied, m)).collect();
    let matched = kept.len();
    let more = matched > applied.limit;
    let moments: Vec<Moment> = kept.into_iter().take(applied.limit).collect();
    Ok(ListAnswer {
        schema: SCHEMA.into(),
        project: project.into(),
        count: moments.len(),
        moments,
        matched,
        total,
        more,
        waiting,
        synced,
        forms,
        filter: applied,
    })
}

pub const RECORD_SCHEMA: &str = "ds.survey-media-record/v1";

/// One survey original as the Server's media store keeps it: the durable
/// record behind a [`MomentRow`], beside the bytes on disk. A record is
/// written when a rotation is prepared (`waiting`) and settled when it is
/// published (`synced`); the bytes are the rotation bundle
/// (`original.bin` — the camera original, byte-identical to the stored
/// object — `thumbnail.jpeg`, `manifest.json`) under
/// `survey-media/<project>/<key>/`, digest-pinned by this record. `degrees`
/// is the net turn the entry metadata records once published.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MediaRecord {
    pub schema: String,
    pub project: String,
    pub path: String,
    pub thumbnail_path: String,
    pub media_type: String,
    pub size: u64,
    pub sha256: String,
    pub thumbnail_size: u64,
    pub thumbnail_sha256: String,
    pub width: u32,
    pub height: u32,
    pub cached_at_ms: u64,
    pub updated_at_ms: u64,
    pub state: SyncState,
    /// The net clockwise turn the entry records for the original at
    /// `source_generation` (its bytes are never turned).
    pub degrees: u16,
    /// The published generation the bytes were derived from — the
    /// precondition a publication replaces.
    pub source_generation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_thumbnail_generation: Option<String>,
    /// The account that prepared the bytes.
    pub account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryFacts>,
}

impl MediaRecord {
    /// The gallery row this record is.
    pub fn moment_row(&self) -> MomentRow {
        MomentRow {
            path: self.path.clone(),
            media_type: self.media_type.clone(),
            size: self.size,
            cached_at_ms: self.cached_at_ms,
            state: self.state,
            thumbnail: true,
            entry: self.entry.clone(),
            feature: None,
            generation: match self.state {
                SyncState::Synced => self.published_generation.clone(),
                SyncState::Waiting => Some(self.source_generation.clone()),
            },
            sha256: Some(self.sha256.clone()),
        }
    }
}

/// The directory name one object's bytes live under: a digest of the
/// object path, so any path the address grammar admits is one safe segment.
pub fn media_key(path: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(path.as_bytes()))
}

/// The one root every host keeps survey media under, below its state root.
pub const MEDIA_ROOT: &str = "survey-media";

/// One admitted turn: the object, the clockwise turn asked for and the net
/// turn the entry will record once any waiting turn is added.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rotation {
    pub project: String,
    pub path: String,
    pub thumbnail_path: String,
    /// The clockwise turn asked for: 90, 180 or 270.
    pub degrees: u16,
    /// `pending_degrees + degrees`, modulo a full circle: what publishing
    /// records. 0 means the turns cancel and nothing is left to publish.
    pub net_degrees: u16,
}

/// The properties panel's rotation gate, in one place. `pending_degrees` is
/// the net turn this machine already holds waiting for `path` (0 if none):
/// a further turn accumulates onto it, never refused and never re-encoded.
pub fn rotation(
    project: &str,
    path: &str,
    degrees: i64,
    pending_degrees: u16,
) -> Result<Rotation, Refusal> {
    let address: SurveyMediaAddress = survey_media_address::parse(path).map_err(|_| {
        Refusal::new(
            NOT_SURVEY_MEDIA,
            format!("{path} is not a survey media address"),
            REMEDY_ADDRESS,
        )
    })?;
    if address.project != project {
        return Err(Refusal::new(
            PROJECT_MISMATCH,
            format!(
                "{path} belongs to project {}, not {project}",
                address.project
            ),
            REMEDY_ADDRESS,
        ));
    }
    if address.object_path == address.thumbnail_object_path {
        return Err(Refusal::new(
            DERIVED_THUMBNAIL,
            "Thumbnail images cannot be rotated",
            REMEDY_ADDRESS,
        ));
    }
    if address.object_path != path {
        return Err(Refusal::new(
            NOT_SURVEY_MEDIA,
            format!(
                "{path} is not the canonical object path {}",
                address.object_path
            ),
            REMEDY_ADDRESS,
        ));
    }
    let turn = survey_photo_media::normalize_rotation(degrees as f64)
        .map_err(|_| Refusal::new(INVALID_TURN, "rotation must be finite", REMEDY_TURN))?;
    if !matches!(degrees, 90 | 180 | 270 | -90 | -180 | -270) || turn == 0 {
        return Err(Refusal::new(
            INVALID_TURN,
            format!("{degrees} is not a net quarter-turn"),
            REMEDY_TURN,
        ));
    }
    if !matches!(pending_degrees, 0 | 90 | 180 | 270) {
        return Err(Refusal::new(
            INVALID_TURN,
            format!("{pending_degrees} is not a waiting quarter-turn"),
            REMEDY_TURN,
        ));
    }
    Ok(Rotation {
        project: project.into(),
        path: address.object_path,
        thumbnail_path: address.thumbnail_object_path,
        degrees: turn,
        net_degrees: (pending_degrees + turn) % 360,
    })
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    List {
        project: String,
        rows: Vec<MomentRow>,
        #[serde(default)]
        filter: Filter,
    },
    Read {
        project: String,
        path: String,
        #[serde(default)]
        row: Option<MomentRow>,
    },
    Rotation {
        project: String,
        path: String,
        degrees: i64,
        /// Retired: a waiting turn is now named by `pending_degrees`.
        #[serde(default)]
        waiting: bool,
        /// The net turn this host already holds waiting for `path`.
        #[serde(default)]
        pending_degrees: u16,
    },
}

/// The JSON door every host uses: one request in, one answer or one typed
/// refusal out.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 64 * 1024 * 1024 {
        return Err("survey moments request exceeds its byte bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid survey moments request: {error}"))?;
    let answer = match request {
        Request::List {
            project,
            rows,
            filter,
        } => list(&project, &rows, &filter).map(|answer| json!(answer)),
        Request::Read { project, path, row } => {
            read(&project, &path, row.as_ref()).map(|moment| json!(moment))
        }
        Request::Rotation {
            project,
            path,
            degrees,
            waiting: _,
            pending_degrees,
        } => rotation(&project, &path, degrees, pending_degrees).map(|rotation| json!(rotation)),
    };
    let value = match answer {
        Ok(data) => json!({"status": "ok", "data": data}),
        Err(refusal) => json!({"status": "refused", "refusal": refusal}),
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "agct38sq_sample";

    fn day(y: i32, m: u8, d: u8, h: u8) -> u64 {
        let date = Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap();
        let time = Time::from_hms(h, 0, 0).unwrap();
        ms_of(PrimitiveDateTime::new(date, time).assume_utc()).unwrap()
    }

    fn row(form: &str, entry: &str, file: &str, waiting: bool, when: u64, held: bool) -> MomentRow {
        MomentRow {
            path: format!("{PROJECT}/forms/{form}/{entry}/{file}"),
            media_type: "image/jpeg".into(),
            size: 1234,
            cached_at_ms: when,
            state: if waiting {
                SyncState::Waiting
            } else {
                SyncState::Synced
            },
            thumbnail: true,
            entry: held.then(|| EntryFacts {
                captured_at_ms: Some(when),
                geometry: Some(json!({"type": "Point", "coordinates": [30.1, -1.9]})),
            }),
            feature: None,
            generation: None,
            sha256: None,
        }
    }

    /// The gallery's five rows, transcribed from `survey-moments-filters.test.ts`.
    fn rows() -> Vec<MomentRow> {
        vec![
            row(
                "poles",
                "e1",
                "IMG_0001.jpg",
                true,
                day(2026, 9, 20, 9),
                true,
            ),
            row(
                "lines",
                "e2",
                "span_0002.jpg",
                false,
                day(2026, 9, 19, 12),
                true,
            ),
            row(
                "poles",
                "e3",
                "IMG_0003.jpg",
                false,
                day(2026, 9, 15, 12),
                true,
            ),
            row(
                "meters",
                "e4",
                "meter_0004.jpg",
                true,
                day(2026, 8, 25, 12),
                true,
            ),
            row("scan", "e5", "scan.jpg", false, day(2026, 1, 1, 12), false),
        ]
    }

    fn files(answer: &ListAnswer) -> Vec<&str> {
        answer.moments.iter().map(|m| m.file.as_str()).collect()
    }

    fn filter(patch: impl FnOnce(&mut Filter)) -> Filter {
        let mut filter = Filter::default();
        patch(&mut filter);
        filter
    }

    #[test]
    fn an_empty_filter_keeps_every_row_newest_first_and_reads_as_empty() {
        let answer = list(PROJECT, &rows(), &Filter::default()).unwrap();
        assert_eq!(
            files(&answer),
            [
                "IMG_0001.jpg",
                "span_0002.jpg",
                "IMG_0003.jpg",
                "meter_0004.jpg",
                "scan.jpg"
            ]
        );
        assert!(answer.filter.empty);
        assert_eq!(
            (answer.count, answer.matched, answer.total, answer.more),
            (5, 5, 5, false)
        );
        assert_eq!((answer.waiting, answer.synced), (2, 3));
        assert!(!resolve(&filter(|f| f.text = Some("  ".into())))
            .unwrap()
            .text
            .is_some());
        assert!(
            !resolve(&filter(|f| f.sync = Some(SyncState::Waiting)))
                .unwrap()
                .empty
        );
        assert!(
            !resolve(&filter(|f| f.since = Some("2026-09-01".into())))
                .unwrap()
                .empty
        );
    }

    #[test]
    fn the_date_range_is_inclusive_on_both_days() {
        let today = filter(|f| {
            f.since = Some("2026-09-20".into());
            f.until = Some("2026-09-20".into());
        });
        assert_eq!(
            files(&list(PROJECT, &rows(), &today).unwrap()),
            ["IMG_0001.jpg"]
        );
        let week = filter(|f| {
            f.since = Some("2026-09-14".into());
            f.until = Some("2026-09-20".into());
        });
        assert_eq!(
            files(&list(PROJECT, &rows(), &week).unwrap()),
            ["IMG_0001.jpg", "span_0002.jpg", "IMG_0003.jpg"]
        );
        let month = filter(|f| {
            f.since = Some("2026-08-22".into());
            f.until = Some("2026-09-20".into());
        });
        assert_eq!(list(PROJECT, &rows(), &month).unwrap().count, 4);
        // A moment at noon on the `until` day is inside; the day after is not.
        let until = filter(|f| f.until = Some("2026-09-19".into()));
        assert_eq!(
            files(&list(PROJECT, &rows(), &until).unwrap()),
            [
                "span_0002.jpg",
                "IMG_0003.jpg",
                "meter_0004.jpg",
                "scan.jpg"
            ]
        );
        let since = filter(|f| f.since = Some("2026-09-19".into()));
        assert_eq!(
            files(&list(PROJECT, &rows(), &since).unwrap()),
            ["IMG_0001.jpg", "span_0002.jpg"]
        );
        // An instant with an offset is honoured as an instant.
        let instant = filter(|f| f.since = Some("2026-09-20T11:00:00+02:00".into()));
        assert_eq!(
            files(&list(PROJECT, &rows(), &instant).unwrap()),
            ["IMG_0001.jpg"]
        );
        let instant = filter(|f| f.since = Some("2026-09-20T11:00:00+01:00".into()));
        assert_eq!(
            files(&list(PROJECT, &rows(), &instant).unwrap()),
            [] as [&str; 0]
        );
    }

    #[test]
    fn a_malformed_bound_is_one_typed_refusal_not_no_bound() {
        let bad = filter(|f| f.since = Some("19/09/2026".into()));
        let refusal = list(PROJECT, &rows(), &bad).unwrap_err();
        assert_eq!(refusal.code, INVALID_FILTER);
        assert!(refusal.message.contains("19/09/2026"));
        let crossed = filter(|f| {
            f.since = Some("2026-09-20".into());
            f.until = Some("2026-09-19".into());
        });
        assert_eq!(
            list(PROJECT, &rows(), &crossed).unwrap_err().code,
            INVALID_FILTER
        );
        assert_eq!(
            list(PROJECT, &rows(), &filter(|f| f.limit = Some(0)))
                .unwrap_err()
                .code,
            INVALID_FILTER
        );
        assert_eq!(
            list(PROJECT, &rows(), &filter(|f| f.limit = Some(501)))
                .unwrap_err()
                .code,
            INVALID_FILTER
        );
        assert_eq!(
            list(
                PROJECT,
                &rows(),
                &filter(|f| f.form = Some("no slug".into()))
            )
            .unwrap_err()
            .code,
            INVALID_FILTER
        );
    }

    #[test]
    fn filters_by_form_sync_state_and_file_name_case_insensitively() {
        let by_form = filter(|f| f.form = Some("poles".into()));
        assert_eq!(
            files(&list(PROJECT, &rows(), &by_form).unwrap()),
            ["IMG_0001.jpg", "IMG_0003.jpg"]
        );
        // Several forms are one set, any of; a repeated slug counts once.
        let any_of = filter(|f| {
            f.form = Some("poles".into());
            f.forms = vec!["lines".into(), "poles".into()];
        });
        let answer = list(PROJECT, &rows(), &any_of).unwrap();
        assert_eq!(
            files(&answer),
            ["IMG_0001.jpg", "span_0002.jpg", "IMG_0003.jpg"]
        );
        assert_eq!(answer.filter.forms, ["poles", "lines"]);
        let waiting = filter(|f| f.sync = Some(SyncState::Waiting));
        assert_eq!(
            files(&list(PROJECT, &rows(), &waiting).unwrap()),
            ["IMG_0001.jpg", "meter_0004.jpg"]
        );
        let synced = filter(|f| f.sync = Some(SyncState::Synced));
        assert_eq!(
            files(&list(PROJECT, &rows(), &synced).unwrap()),
            ["span_0002.jpg", "IMG_0003.jpg", "scan.jpg"]
        );
        let text = filter(|f| f.text = Some("img".into()));
        assert_eq!(
            files(&list(PROJECT, &rows(), &text).unwrap()),
            ["IMG_0001.jpg", "IMG_0003.jpg"]
        );
        let padded = filter(|f| f.text = Some(" 0004 ".into()));
        assert_eq!(
            files(&list(PROJECT, &rows(), &padded).unwrap()),
            ["meter_0004.jpg"]
        );
    }

    #[test]
    fn composes_every_criterion_with_and() {
        let all = filter(|f| {
            f.since = Some("2026-09-14".into());
            f.until = Some("2026-09-20".into());
            f.form = Some("poles".into());
            f.sync = Some(SyncState::Synced);
            f.text = Some("img".into());
        });
        assert_eq!(
            files(&list(PROJECT, &rows(), &all).unwrap()),
            ["IMG_0003.jpg"]
        );
        let none = filter(|f| {
            f.form = Some("poles".into());
            f.sync = Some(SyncState::Waiting);
            f.text = Some("span".into());
        });
        let answer = list(PROJECT, &rows(), &none).unwrap();
        assert_eq!(answer.count, 0);
        assert_eq!(
            answer.total, 5,
            "the total is the project's holdings, not the page"
        );
    }

    #[test]
    fn offers_the_forms_present_most_frequent_first_and_pages_n_of_m() {
        let answer = list(PROJECT, &rows(), &filter(|f| f.limit = Some(2))).unwrap();
        assert_eq!(
            answer.forms,
            [
                FormCount {
                    form: "poles".into(),
                    count: 2
                },
                FormCount {
                    form: "lines".into(),
                    count: 1
                },
                FormCount {
                    form: "meters".into(),
                    count: 1
                },
                FormCount {
                    form: "scan".into(),
                    count: 1
                },
            ]
        );
        assert_eq!(
            (answer.count, answer.matched, answer.total, answer.more),
            (2, 5, 5, true)
        );
        assert_eq!(list(PROJECT, &[], &Filter::default()).unwrap().forms, []);
    }

    #[test]
    fn only_this_projects_originals_are_moments() {
        let mut rows = rows();
        rows.push(row(
            "poles",
            "e9",
            "IMG_0009.jpg",
            false,
            day(2026, 9, 21, 9),
            true,
        ));
        rows.last_mut().unwrap().path = "other_project/forms/poles/e9/IMG_0009.jpg".to_string();
        rows.push(MomentRow {
            path: format!("{PROJECT}/forms/poles/e1/IMG_0001_thunder.jpeg"),
            ..row("poles", "e1", "x", false, day(2026, 9, 22, 9), true)
        });
        rows.push(MomentRow {
            path: "data:image/png;base64,AAAA".into(),
            ..row("poles", "e1", "x", false, day(2026, 9, 23, 9), true)
        });
        let answer = list(PROJECT, &rows, &Filter::default()).unwrap();
        assert_eq!(answer.total, 5);
        assert_eq!(files(&answer)[0], "IMG_0001.jpg");
    }

    #[test]
    fn a_moment_is_described_with_its_clock_location_and_navigate_verdict() {
        let held = &rows()[0];
        let moment = describe(PROJECT, held).unwrap();
        assert_eq!(
            (moment.form.as_str(), moment.entry.as_str()),
            ("poles", "e1")
        );
        assert_eq!(
            moment.thumbnail_path,
            format!("{PROJECT}/forms/poles/e1/IMG_0001_thunder.jpeg")
        );
        assert_eq!(moment.when_ms, moment.captured_at_ms.unwrap());
        assert!(moment.navigate.enabled && moment.location.is_some());

        let not_local = describe(PROJECT, &rows()[4]).unwrap();
        assert_eq!(
            not_local.navigate.reason,
            Some(NavigateRefusal::EntryNotLocal)
        );
        assert_eq!(
            not_local.when_ms, not_local.cached_at_ms,
            "the cache clock stands in"
        );

        let mut without_location = rows()[1].clone();
        without_location.entry = Some(EntryFacts {
            captured_at_ms: Some(7),
            geometry: None,
        });
        let described = describe(PROJECT, &without_location).unwrap();
        assert_eq!(
            described.navigate.reason,
            Some(NavigateRefusal::EntryWithoutLocation)
        );
        assert_eq!(described.when_ms, 7);
    }

    #[test]
    fn entry_facts_are_read_off_the_stored_feature_in_either_spelling() {
        let nested = json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.0, -2.0]},
            "properties": {"metadata": {"created_at": "2026-09-20T09:00:00Z"}}});
        let facts = EntryFacts::from_feature(&nested);
        assert_eq!(facts.captured_at_ms, Some(day(2026, 9, 20, 9)));
        assert!(facts.geometry.is_some());
        let flattened = json!({"geometry": null, "properties": {"metadata_created_at": "2026-09-19T12:00:00+02:00"}});
        assert_eq!(
            EntryFacts::from_feature(&flattened).captured_at_ms,
            Some(day(2026, 9, 19, 10))
        );
        assert_eq!(EntryFacts::from_feature(&flattened).geometry, None);
        let epoch = json!({"properties": {"created_at": 1_789_900_000_000u64}});
        assert_eq!(
            EntryFacts::from_feature(&epoch).captured_at_ms,
            Some(1_789_900_000_000)
        );
        let bare = json!({"created_at": "not a date", "geometry": {"coordinates": [1, 2]}});
        let facts = EntryFacts::from_feature(&bare);
        assert_eq!((facts.captured_at_ms, facts.geometry), (None, None));

        // A row that hands the feature over is described exactly as one that
        // declared the facts; a feature without a location refuses navigate
        // with the location reason, not "not local".
        let mut row = rows()[4].clone();
        assert_eq!(
            describe(PROJECT, &row).unwrap().navigate.reason,
            Some(NavigateRefusal::EntryNotLocal)
        );
        row.feature = Some(nested.clone());
        let moment = describe(PROJECT, &row).unwrap();
        assert!(moment.navigate.enabled);
        assert_eq!(
            moment.when_ms,
            day(2026, 9, 20, 9),
            "the entry clock outranks the cache clock"
        );
        row.feature = Some(flattened);
        assert_eq!(
            describe(PROJECT, &row).unwrap().navigate.reason,
            Some(NavigateRefusal::EntryWithoutLocation)
        );
    }

    #[test]
    fn read_answers_the_held_row_or_one_refusal() {
        let held = rows();
        let path = held[0].path.clone();
        assert_eq!(
            read(PROJECT, &path, Some(&held[0])).unwrap().file,
            "IMG_0001.jpg"
        );
        assert_eq!(
            read(PROJECT, &path, None).unwrap_err().code,
            MOMENT_NOT_HELD
        );
        assert_eq!(
            read(PROJECT, &path, Some(&held[1])).unwrap_err().code,
            MOMENT_NOT_HELD
        );
        let thumb = MomentRow {
            path: format!("{PROJECT}/forms/poles/e1/IMG_0001_thunder.jpeg"),
            ..held[0].clone()
        };
        assert_eq!(
            read(PROJECT, &thumb.path, Some(&thumb)).unwrap_err().code,
            NOT_SURVEY_MEDIA
        );
    }

    #[test]
    fn the_rotation_gate_is_the_properties_panels() {
        let path = format!("{PROJECT}/forms/poles/e1/IMG_0001.jpg");
        let admitted = rotation(PROJECT, &path, 90, 0).unwrap();
        assert_eq!((admitted.degrees, admitted.net_degrees), (90, 90));
        assert_eq!(
            admitted.thumbnail_path,
            format!("{PROJECT}/forms/poles/e1/IMG_0001_thunder.jpeg")
        );
        // A counter-clockwise click is the turn 270; a turn while another is
        // waiting accumulates onto it, and four quarter-turns cancel.
        assert_eq!(rotation(PROJECT, &path, -90, 0).unwrap().degrees, 270);
        assert_eq!(rotation(PROJECT, &path, -90, 90).unwrap().net_degrees, 0);
        assert_eq!(rotation(PROJECT, &path, 90, 180).unwrap().net_degrees, 270);
        assert_eq!(
            rotation(PROJECT, &path, 90, 45).unwrap_err().code,
            INVALID_TURN
        );
        assert_eq!(rotation(PROJECT, &path, 180, 0).unwrap().degrees, 180);
        // The refusals, before any byte moves.
        assert_eq!(
            rotation(PROJECT, &path, 0, 0).unwrap_err().code,
            INVALID_TURN
        );
        assert_eq!(
            rotation(PROJECT, &path, 360, 0).unwrap_err().code,
            INVALID_TURN
        );
        assert_eq!(
            rotation(PROJECT, &path, 45, 0).unwrap_err().code,
            INVALID_TURN
        );
        let thumb = format!("{PROJECT}/forms/poles/e1/IMG_0001_thunder.jpeg");
        assert_eq!(
            rotation(PROJECT, &thumb, 90, 0).unwrap_err().code,
            DERIVED_THUMBNAIL
        );
        assert_eq!(
            rotation("other", &path, 90, 0).unwrap_err().code,
            PROJECT_MISMATCH
        );
        assert_eq!(
            rotation(PROJECT, "data:image/png;base64,AAAA", 90, 0)
                .unwrap_err()
                .code,
            NOT_SURVEY_MEDIA
        );
        assert_eq!(
            rotation(PROJECT, "https://example.test/x.jpg", 90, 0)
                .unwrap_err()
                .code,
            NOT_SURVEY_MEDIA
        );
    }

    #[test]
    fn the_json_door_answers_ok_or_refused() {
        let input =
            json!({"op": "list", "project": PROJECT, "rows": rows(), "filter": {"form": "poles"}});
        let answer: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
        assert_eq!(answer["status"], "ok");
        assert_eq!(answer["data"]["count"], 2);
        let input = json!({"op": "rotation", "project": PROJECT, "path": "nope", "degrees": 90});
        let answer: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
        assert_eq!(answer["status"], "refused");
        assert_eq!(answer["refusal"]["code"], NOT_SURVEY_MEDIA);
        assert!(evaluate(b"{\"op\":\"nope\"}").is_err());
    }
}
