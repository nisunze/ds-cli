//! The tree (§2.5, §2.6): user folders from ds-brain plus the system-folder
//! projection over source inventories, filtered by `query` / `link` /
//! `folder` / `depth` / `kind`, deterministic for the same inputs.
//!
//! The kernel is handed one map of source listings — the rows each client
//! already read for its own screen — and answers one `ds.assets.tree/v1`.
//! It reads nothing, caches nothing and polls nothing: §2.5's cost rule ("one
//! read per source per explicit refresh") is the host's to keep, and this
//! projection exists so that keeping it costs nothing extra.
//!
//! Every blob the project holds is indexed (owner ruling, 2026-09-09), so a
//! row a rule cannot place is never dropped: it lands under `Unclassified/`
//! with an id that names the source it came from. A row that cannot even be
//! decoded lands there too, because a malformed foreign document is still
//! evidence that something exists.
//!
//! Source rows are foreign wire documents. They are decoded with lenient
//! private structs (`serde(default)`, no `deny_unknown_fields`) that read only
//! the fields the projection needs, through deserialisers that turn a
//! surprising type into `None` instead of an error. Nothing in here panics on
//! a malformed row.
//!
//! What a projected row is *not*: it is never written to Firestore (D16), it
//! never carries a signed URL or a `download_url` (§12.11 — a URL-valued
//! source field is reduced to its path before it becomes an `origin.ref`), and
//! its `asset_id` is a deterministic `sys:` composite, not a minted id.
//!
//! Where §2.5 names a leaf but not its spelling, this module decides once and
//! says so here:
//!
//! * `attachments/<label>/<revision>` is `r<revision>` keeping the file's
//!   extension (`r2.pdf`), so a revision is distinguishable *and* its format
//!   stays recognisable from the leaf alone;
//! * `revisions/<revision>.dsgrid` uses the revision id the `sys:` composite
//!   uses, and carries `format: dsgrid` from the source's own artifact type;
//! * an export or compute `<output>` is `<output_id>.<format>` when the item
//!   declares one, otherwise the bare `output_id`;
//! * `uploads/{draft,sketch}` also renders the newer `process` slot the
//!   transformer document may carry, rather than sending it to `Unclassified`;
//! * a room (`Local data/…`) is `kind: other` with no format — a room is not a
//!   file — and a print setup is `kind: note`, both per §2.5;
//! * a Solar `report_input.json` is `kind: other`, `format: unknown`: the
//!   closed §2.3 format vocabulary has no JSON, and a guess would be worse
//!   than the honest answer;
//! * `counts` are this folder's own assets and its direct children, not a
//!   subtree total, and they are computed after filtering (§2.6).
//!
//! Format and kind otherwise come from `format::recognise` over the name and
//! the byte count — a projected row carries no bytes, so a source that
//! declares its own format (a print, an export item, a media type) offers it
//! as a *name* hint; there is no second recogniser here.

use std::collections::{BTreeMap, BTreeSet};

use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::{json, Value};

use super::{
    digest_of, format, Asset, ClassifiedBy, Counts, Dated, FolderKind, Format, Kind, Link,
    MailPart, Origin, Owner, Sensitivity, Source, SourceKind, Status, TreeFolder, TreeRequest,
    TreeResponse, ASSET_SCHEMA, TREE_SCHEMA, TREE_SOURCE_NAMES,
};

/// The system roots of §2.5, in the contract's order. A user folder may not
/// claim one of these names (`build` refuses it by name); `Unclassified` is
/// rendered only when something landed in it.
pub const SYSTEM_ROOTS: [&str; 12] = [
    "Transformers",
    "MV models",
    "Survey",
    "Project data",
    "My data",
    "Project work",
    "Correspondence",
    "Reports",
    "Solar",
    "Local data",
    "Prints",
    "Unclassified",
];

/// The root each source feeds. `pm_tasks` and `pm_records` carry no bytes of
/// their own: they are the lookup rows that give a PM attachment its `<wbs>`
/// or `<reference>` folder, so they feed `Project work` without projecting a
/// row of their own.
///
/// `Correspondence` is fed by the same `pm_records` lookup rows and by
/// `pm_parties` (party id → name): one folder per thread, listing the
/// catalogued assets the thread's records carry — never a row of its own.
const SOURCE_ROOTS: [(&str, &str); 21] = [
    ("compounded", "Reports"),
    ("compute_artifacts", "Reports"),
    ("dataset_rooms", "Local data"),
    ("design_attachments", "Transformers"),
    ("grid_exports", "MV models"),
    ("grid_models", "MV models"),
    ("local_prints", "Prints"),
    ("national_datasets", "Local data"),
    ("pm_attachments", "Project work"),
    ("pm_parties", "Correspondence"),
    ("pm_records", "Correspondence"),
    ("pm_records", "Project work"),
    ("pm_tasks", "Project work"),
    ("print_setups", "Prints"),
    ("project_data", "Project data"),
    ("report_rooms", "Local data"),
    ("solar", "Solar"),
    ("survey_media", "Survey"),
    ("transformer_versions", "Transformers"),
    ("transformers", "Transformers"),
    ("user_data", "My data"),
];

/// Sources whose rows are lookups, never assets (see [`SOURCE_ROOTS`]).
const LOOKUP_SOURCES: [&str; 3] = ["pm_parties", "pm_records", "pm_tasks"];

/// The closed reserved-transformer-document set of ds-brain's
/// `IsReservedTransformerDocName` (`internal/services/path_resolver.go`),
/// carried here by name so the two lists can be pinned equal.
///
/// `all_transformers` and `combined_transformers` are the aggregate aliases
/// [`canonical_transformer_reference_name`] folds into `combined_transformer`;
/// they are listed because ds-brain lists them. The numbered overflow chunks
/// (`collisions_2`, `collisions_3`, …) are matched by
/// [`is_reserved_transformer_name`] rather than enumerated, exactly as
/// ds-brain matches them with `collisionsChunkNameRegex`. A blank name is
/// reserved before canonicalisation, also as ds-brain does it.
pub const RESERVED_TRANSFORMER_NAMES: [&str; 5] = [
    "all_transformers",
    "collisions",
    "combined_transformer",
    "combined_transformers",
    "mv_data",
];

/// ds-brain `CanonicalTransformerReferenceName`: lowercase, `[a-z0-9_]`, no
/// leading digit, aggregate aliases folded, non-empty fallback.
pub fn canonical_transformer_reference_name(name: &str) -> String {
    let mut canonical = String::with_capacity(name.len());
    for character in name.trim().to_lowercase().chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            canonical.push(character);
        } else {
            canonical.push('_');
        }
    }
    let canonical = canonical.trim_matches('_');
    match canonical {
        "all_transformers" | "combined_transformers" | "combined_transformer" => {
            return "combined_transformer".to_string();
        }
        "" => return "transformer".to_string(),
        _ => {}
    }
    if canonical.starts_with(|character: char| character.is_ascii_digit()) {
        return format!("_{canonical}");
    }
    canonical.to_string()
}

/// ds-brain `IsReservedTransformerDocName`: a reserved document is never a
/// transformer folder (§2.5).
pub fn is_reserved_transformer_name(name: &str) -> bool {
    if name.trim().is_empty() {
        return true;
    }
    let canonical = canonical_transformer_reference_name(name);
    RESERVED_TRANSFORMER_NAMES.contains(&canonical.as_str()) || is_collisions_chunk_name(&canonical)
}

/// ds-brain `IsCollisionsChunkName`: `collisions_<N>` overflow chunks only.
fn is_collisions_chunk_name(canonical: &str) -> bool {
    match canonical.strip_prefix("collisions_") {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

/// Whether a canonical transformer reference is the combined aggregate, whose
/// artifacts land under `Reports/combined/` (§2.5).
fn is_combined_aggregate(canonical: &str) -> bool {
    canonical == "combined_transformer"
}

/// The known sources that were not supplied, in contract order. Their roots
/// render as *not loaded*, never as empty (§2.5).
pub fn not_loaded_sources(request: &TreeRequest) -> Vec<&'static str> {
    TREE_SOURCE_NAMES
        .iter()
        .copied()
        .filter(|name| !request.sources.is_loaded(name))
        .collect()
}

// ── lenient decoding of foreign rows ────────────────────────────────────────

/// Text from a field that may legitimately arrive as a number (an epoch, a
/// version) or not at all. Anything else reads as absent rather than as an
/// error: one surprising field must not cost a whole row its place.
fn lenient_text<'de, D: Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(input)? {
        Some(Value::String(text)) => Some(text),
        Some(Value::Number(number)) => Some(number.to_string()),
        Some(Value::Bool(flag)) => Some(flag.to_string()),
        _ => None,
    })
}

/// A count that may arrive as a JSON number of any flavour or as a numeric
/// string. Negative, fractional and unparseable values read as absent.
fn lenient_number<'de, D: Deserializer<'de>>(input: D) -> Result<Option<u64>, D::Error> {
    Ok(Option::<Value>::deserialize(input)?
        .as_ref()
        .and_then(value_number))
}

fn value_number(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64().or_else(|| {
            number
                .as_f64()
                .filter(|float| float.is_finite() && *float >= 0.0)
                .map(|float| float as u64)
        }),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

/// Decodes a nested foreign value, or its default. Used for every nested
/// object and list so that one bad branch of a row cannot sink the row.
fn decode<T: Default + DeserializeOwned>(value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

/// The strings of a foreign list, dropping entries that are not strings.
fn string_list(value: &Value) -> Vec<String> {
    match value.as_array() {
        Some(items) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    }
}

fn present(text: &Option<String>) -> Option<&str> {
    text.as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// The first candidate a row actually carries.
fn any_of<const N: usize>(candidates: [Option<&str>; N]) -> Option<&str> {
    candidates.into_iter().flatten().next()
}

/// One path segment from source text: never empty, never a second level.
fn segment(text: &str) -> String {
    let cleaned = text.trim().replace('/', "_");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned.to_string()
    }
}

/// A reference reduced to its path: the query string and fragment go (a
/// signature lives there), and an `http(s)://` scheme and host go with them,
/// so a signed URL never becomes an `origin.ref` (§12.11). A `gs://` locator
/// is a storage identity, not a URL, and is kept whole.
fn path_only(reference: &str) -> &str {
    let bare = reference
        .split(['?', '#'])
        .next()
        .unwrap_or(reference)
        .trim();
    let lower = bare.to_ascii_lowercase();
    for scheme in ["https://", "http://"] {
        if lower.starts_with(scheme) {
            let rest = &bare[scheme.len()..];
            return rest.find('/').map_or("", |slash| &rest[slash..]);
        }
    }
    bare
}

/// The file name of a path or URL.
fn leaf_of(reference: &str) -> String {
    let path = path_only(reference);
    match path.rsplit('/').find(|segment| !segment.is_empty()) {
        Some(leaf) => leaf.to_string(),
        None => path.to_string(),
    }
}

/// The extension of a file name, lowercased, without the dot.
fn extension_of(name: &str) -> Option<String> {
    let leaf = leaf_of(name);
    let (stem, extension) = leaf.rsplit_once('.')?;
    if stem.is_empty() || extension.is_empty() || !extension.chars().all(char::is_alphanumeric) {
        return None;
    }
    Some(extension.to_lowercase())
}

/// A digest as the model spells it (`sha256:<hex>`), or the source's own text
/// when it is something else.
fn digest_text(raw: Option<&str>) -> Option<String> {
    let raw = raw.map(str::trim).filter(|text| !text.is_empty())?;
    if raw.contains(':') {
        return Some(raw.to_string());
    }
    if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("sha256:{}", raw.to_lowercase()));
    }
    Some(raw.to_string())
}

/// Kind and format from a name and its byte count. Recognition is
/// `format::recognise`'s job and only its job; a projected row carries no
/// bytes, so the name is all the evidence there is and `unknown` is the
/// honest answer when the name says nothing (§2.3). `hint` is a format the
/// source itself declared (a print's `format`, an export item's `format`);
/// it is offered as a name, never as a second recogniser.
fn recognise_name(name: &str, bytes: u64, hint: Option<&str>) -> (Kind, Format) {
    // A source that reports no byte count says nothing about emptiness: 0
    // here means "not reported", so the name is not discarded as the name of
    // an empty file (which `recognise` would rightly call `unknown`).
    let bytes = bytes.max(1);
    let recognition = format::recognise(Some(name), &[], bytes);
    if recognition.format != Format::Unknown {
        return (recognition.kind, recognition.format);
    }
    if let Some(hint) = hint.map(str::trim).filter(|hint| !hint.is_empty()) {
        let lower = hint.to_lowercase();
        let media = match lower.split(';').next().unwrap_or("").trim() {
            "audio/mpeg" => Some(Format::Mp3),
            "audio/wav" | "audio/x-wav" => Some(Format::Wav),
            "video/mp4" => Some(Format::Mp4),
            "video/webm" => Some(Format::Webm),
            _ => None,
        };
        if let Some(media) = media {
            return (format::kind_of(media), media);
        }
        let hinted = format::recognise(Some(&format!("file.{}", lower)), &[], bytes);
        if hinted.format != Format::Unknown {
            return (hinted.kind, hinted.format);
        }
    }
    (recognition.kind, recognition.format)
}

/// A stable reference for a row that carries no usable identity: the digest of
/// its canonical JSON, so the same row keeps the same id however the list was
/// ordered.
fn row_reference(row: &Value) -> String {
    let digest = digest_of(canonical_json(row).as_bytes());
    let hex = digest.rsplit(':').next().unwrap_or(&digest);
    format!("row-{}", hex.chars().take(12).collect::<String>())
}

/// JSON with object keys in sorted order, so a digest over it does not depend
/// on the order a client happened to serialise its fields in.
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(entries) => {
            let sorted: BTreeMap<&String, &Value> = entries.iter().collect();
            let body = sorted
                .into_iter()
                .map(|(key, value)| format!("{}:{}", json!(key), canonical_json(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        Value::Array(items) => {
            let body = items
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{body}]")
        }
        other => other.to_string(),
    }
}

// ── one projected row ───────────────────────────────────────────────────────

/// A row being projected. Everything a projected asset does not get is fixed
/// here: `sensitivity: internal` (D4), `classified_by: kernel`, no container,
/// and an `origin` naming the source and the reference its read action needs
/// (§2.5).
struct Row {
    folder: String,
    name: String,
    asset_id: String,
    kind: Kind,
    format: Format,
    bytes: u64,
    digest: Option<String>,
    status: Status,
    owner: Option<String>,
    created_at: Option<String>,
    modified_at: Option<String>,
    source: SourceKind,
    detail: String,
    origin: &'static str,
    reference: Value,
    links: Vec<Link>,
}

impl Row {
    fn new(
        origin: &'static str,
        folder: impl Into<String>,
        name: impl Into<String>,
        asset_id: impl Into<String>,
        reference: Value,
    ) -> Self {
        Row {
            folder: folder.into(),
            name: name.into(),
            asset_id: asset_id.into(),
            kind: Kind::Other,
            format: Format::Unknown,
            bytes: 0,
            digest: None,
            status: Status::Fresh,
            owner: None,
            created_at: None,
            modified_at: None,
            source: SourceKind::System,
            detail: origin.to_string(),
            origin,
            reference,
            links: Vec::new(),
        }
    }

    fn recognised(mut self, bytes: u64, hint: Option<&str>) -> Self {
        let (kind, format) = recognise_name(&self.name, bytes, hint);
        self.kind = kind;
        self.format = format;
        self
    }

    fn typed(mut self, kind: Kind, format: Format) -> Self {
        self.kind = kind;
        self.format = format;
        self
    }

    fn bytes(mut self, bytes: Option<u64>) -> Self {
        self.bytes = bytes.unwrap_or(0);
        self
    }

    fn digest(mut self, digest: Option<&str>) -> Self {
        self.digest = digest_text(digest);
        self
    }

    fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    fn source(mut self, kind: SourceKind, detail: &str) -> Self {
        self.source = kind;
        self.detail = detail.to_string();
        self
    }

    fn owner(mut self, owner: Option<&str>) -> Self {
        self.owner = owner
            .map(str::trim)
            .filter(|owner| !owner.is_empty())
            .map(str::to_string);
        self
    }

    fn created(mut self, created_at: Option<&str>) -> Self {
        self.created_at = created_at
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        self
    }

    fn modified(mut self, modified_at: Option<&str>) -> Self {
        self.modified_at = modified_at
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        self
    }

    fn link(mut self, link: Link) -> Self {
        self.links.push(link);
        self
    }

    fn transformer_link(self, name: &str) -> Self {
        self.link(Link::DsObject {
            object_type: "transformer".to_string(),
            entity_id: name.to_string(),
        })
    }

    fn asset(self, project_id: &str) -> (String, Asset) {
        let asset = Asset {
            reference: None,
            schema: ASSET_SCHEMA.to_string(),
            asset_id: self.asset_id,
            project_id: project_id.to_string(),
            folder: self.folder.clone(),
            name: self.name,
            kind: self.kind,
            format: self.format,
            bytes: self.bytes,
            digest: self.digest,
            status: self.status,
            sensitivity: Sensitivity::Internal,
            dated: Dated {
                document_date: None,
                received_at: None,
                created_at: self.created_at,
                source_modified_at: self.modified_at,
            },
            owner: self.owner.map(|id| Owner {
                kind: "user".to_string(),
                id,
                display: None,
            }),
            source: Source {
                kind: self.source,
                detail: Some(self.detail),
            },
            classified_by: ClassifiedBy::Kernel,
            container: None,
            links: self.links,
            origin: Some(Origin {
                source: self.origin.to_string(),
                r#ref: self.reference,
            }),
            document: None,
            mail: None,
            external: None,
            bytes_held: true,
            folder_id: None,
            content_type: None,
            created_by: None,
            updated_at: None,
            updated_by: None,
            version: None,
        };
        (self.folder, asset)
    }
}

// ── the source projection (§2.5) ────────────────────────────────────────────

/// Walks every supplied source list once and places its rows. Nothing is
/// dropped: a row that cannot be placed by its own rule, and a row that cannot
/// be decoded at all, both land under `Unclassified/<source>/<name>`.
struct Projector<'a> {
    project_id: &'a str,
    /// PM task id → `<wbs>` (from the `pm_tasks` lookup source).
    tasks: BTreeMap<String, String>,
    /// PM record id → `<reference>` (from the `pm_records` lookup source).
    records: BTreeMap<String, String>,
    /// Party id → name (from the `pm_parties` lookup source), for the
    /// `Correspondence/<thread>` folder names.
    parties: BTreeMap<String, String>,
    /// Grid model id → `<display_name>` (from the `grid_models` source), so an
    /// export lands beside the revisions of the same model.
    models: BTreeMap<String, String>,
    placed: Vec<(String, Asset)>,
}

impl<'a> Projector<'a> {
    fn new(project_id: &'a str) -> Self {
        Projector {
            project_id,
            tasks: BTreeMap::new(),
            records: BTreeMap::new(),
            parties: BTreeMap::new(),
            models: BTreeMap::new(),
            placed: Vec::new(),
        }
    }

    fn push(&mut self, row: Row) {
        let placed = row.asset(self.project_id);
        self.placed.push(placed);
    }

    /// §2.5's last line: every row a rule cannot place, with an id that names
    /// the source it came from.
    fn unclassified(&mut self, source: &'static str, row: &Value, name: Option<&str>) {
        let reference = name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| row_reference(row));
        let leaf = segment(&reference);
        self.push(
            Row::new(
                source,
                format!("Unclassified/{source}"),
                leaf.clone(),
                format!("sys:unclassified:{source}:{reference}"),
                json!({ "source": source, "ref": reference }),
            )
            .recognised(0, None)
            .source(SourceKind::System, source),
        );
    }

    fn run(&mut self, request: &TreeRequest) {
        self.tasks = lookup_map(request, "pm_tasks", |row| {
            let task: TaskRow = decode(row);
            let id = present(&task.id).map(str::to_string)?;
            let wbs = any_of([present(&task.wbs), present(&task.title), Some(id.as_str())])
                .map(str::to_string)?;
            Some((id, wbs))
        });
        self.parties = lookup_map(request, "pm_parties", |row| {
            let party: PartyRow = decode(row);
            let id = present(&party.id).map(str::to_string)?;
            let inner: PartyRow = decode(&party.data);
            let name = any_of([
                present(&party.name),
                present(&inner.name),
                Some(id.as_str()),
            ])
            .map(str::to_string)?;
            Some((id, name))
        });
        self.records = lookup_map(request, "pm_records", |row| {
            let record = RecordRow::flattened(row);
            let id = present(&record.id).map(str::to_string)?;
            let reference = any_of([
                present(&record.reference_number),
                present(&record.subject),
                Some(id.as_str()),
            ])
            .map(str::to_string)?;
            Some((id, reference))
        });
        self.models = lookup_map(request, "grid_models", |row| {
            let model: GridModelRow = decode(row);
            let id = present(&model.model_id).map(str::to_string)?;
            let display =
                any_of([present(&model.display_name), Some(id.as_str())]).map(str::to_string)?;
            Some((id, display))
        });

        for (source, rows) in request.sources.lists() {
            if LOOKUP_SOURCES.contains(&source) {
                continue;
            }
            let Some(source) = TREE_SOURCE_NAMES.iter().find(|known| **known == source) else {
                continue;
            };
            for row in rows {
                let before = self.placed.len();
                let handled = self.project_row(source, row);
                if !handled && self.placed.len() == before {
                    let name = row.get("name").and_then(Value::as_str);
                    self.unclassified(source, row, name);
                }
            }
        }
    }

    /// Returns true when the row was deliberately projected to nothing (a
    /// reserved transformer document is never a folder, §2.5).
    fn project_row(&mut self, source: &'static str, row: &Value) -> bool {
        match source {
            "transformers" => return self.transformer(row),
            "transformer_versions" => self.transformer_version(row),
            "design_attachments" => self.design_attachment(row),
            "grid_models" => self.grid_model(row),
            "grid_exports" => self.grid_export(row),
            "pm_attachments" => self.pm_attachment(row),
            "compounded" => self.compounded(row),
            "survey_media" => self.survey_media(row),
            "project_data" => self.upload_document(row, "project_data", "Project data"),
            "user_data" => self.upload_document(row, "user_data", "My data"),
            "compute_artifacts" => self.compute_artifact(row),
            "solar" => self.solar(row),
            "dataset_rooms" => self.dataset_room(row),
            "national_datasets" => self.national_dataset(row),
            "report_rooms" => self.report_room(row),
            "local_prints" => self.local_print(row),
            "print_setups" => self.print_setup(row),
            _ => {}
        }
        false
    }

    // ── Transformers ────────────────────────────────────────────────────────

    fn transformer(&mut self, row: &Value) -> bool {
        let transformer: TransformerRow = decode(row);
        let Some(name) = present(&transformer.name) else {
            return false;
        };
        let canonical = canonical_transformer_reference_name(name);
        let reserved = is_reserved_transformer_name(name);
        let metadata: MetadataRow = decode(&transformer.metadata);
        let folder = format!("Transformers/{}", segment(name));

        if !reserved {
            self.push(
                Row::new(
                    "transformers",
                    format!("{folder}/design/current"),
                    "current",
                    format!("sys:transformer_design:{name}"),
                    json!({ "transformer": name, "read": "design" }),
                )
                .typed(Kind::Geo, Format::Geojson)
                .status(Status::Fresh)
                .source(SourceKind::System, "live design")
                .owner(present(&metadata.updated_by))
                .modified(present(&metadata.updated_at))
                .transformer_link(name),
            );

            let uploads: UploadsRow = decode(&transformer.uploads);
            for (slot, files) in [
                ("draft", &uploads.draft),
                ("sketch", &uploads.sketch),
                ("process", &uploads.process),
            ] {
                for upload in files {
                    let Some(named) =
                        any_of([present(&upload.filename), present(&upload.blob_path)])
                    else {
                        continue;
                    };
                    let file = leaf_of(named);
                    let reference = any_of([present(&upload.blob_path), Some(file.as_str())])
                        .map(path_only)
                        .unwrap_or_default()
                        .to_string();
                    self.push(
                        Row::new(
                            "transformers",
                            format!("{folder}/uploads/{slot}"),
                            segment(&file),
                            format!("sys:upload:{name}:{reference}"),
                            json!({ "transformer": name, "slot": slot, "blob_path": reference }),
                        )
                        .recognised(upload.size_bytes.unwrap_or(0), None)
                        .bytes(upload.size_bytes)
                        .status(Status::Fresh)
                        .source(SourceKind::Upload, "transformer upload")
                        .owner(present(&upload.uploaded_by))
                        .created(present(&upload.uploaded_at))
                        .transformer_link(name),
                    );
                }
            }
        }

        // Report files, print files and report artifacts. The combined
        // aggregate is not a folder; its artifacts land under
        // `Reports/combined/` (§2.5). Any other reserved document keeps its
        // files too — under `Unclassified/`, never dropped.
        let reports_folder = if !reserved {
            format!("{folder}/reports")
        } else if is_combined_aggregate(&canonical) {
            "Reports/combined".to_string()
        } else {
            "Unclassified/transformers".to_string()
        };

        // The report-artifact channel publishes map printouts here. These
        // are projected assets, with the existing report bytes as authority.
        // Rich printout rows precede the legacy URL-only listing of the same
        // canonical identity; the first placement owns folder and provenance.
        if let Some(prints) = transformer
            .report_metadata
            .get("print_urls")
            .and_then(Value::as_array)
        {
            for print in prints {
                let artifact: ReportArtifactRow = decode(print);
                let Some(path) = print.get("gcs_path").and_then(Value::as_str) else {
                    continue;
                };
                let file = leaf_of(path);
                if file.is_empty() {
                    continue;
                }
                let family = print
                    .get("map_family")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let target = match family {
                    "custom-map" => "Prints/custom maps",
                    "mv-map" => "Reports/MV",
                    _ => reports_folder.as_str(),
                };
                let mut projected = Row::new(
                    "transformers",
                    target,
                    segment(&file),
                    format!("sys:report_file:{name}:{file}"),
                    json!({"transformer": name, "path": path_only(path),
                        "sha256": present(&artifact.sha256), "family": family,
                        "attachment_type": "printout", "paper_size": print.get("paper_size")}),
                )
                .recognised(artifact.size_bytes.unwrap_or(0), present(&artifact.format))
                .bytes(artifact.size_bytes)
                .digest(present(&artifact.sha256))
                .status(Status::Durable)
                .source(SourceKind::ReportOutput, "published printout");
                if !reserved {
                    projected = projected.transformer_link(name);
                }
                self.push(projected);
            }
        }

        for path in string_list(&transformer.report_files)
            .into_iter()
            .chain(string_list(&transformer.print_files))
        {
            let file = leaf_of(&path);
            if file.is_empty() {
                continue;
            }
            let reference = path_only(&path).to_string();
            self.push(
                Row::new(
                    "transformers",
                    reports_folder.clone(),
                    segment(&file),
                    format!("sys:report_file:{name}:{file}"),
                    json!({ "transformer": name, "path": reference }),
                )
                .recognised(0, None)
                .status(Status::Durable)
                .source(SourceKind::ReportOutput, "report file")
                .owner(present(&metadata.updated_by))
                .modified(present(&metadata.updated_at))
                .transformer_link(name),
            );
        }

        for artifact in decode::<Vec<ReportArtifactRow>>(&transformer.report_artifacts) {
            let url = present(&artifact.url).unwrap_or_default();
            let file = match leaf_of(url) {
                leaf if !leaf.is_empty() => leaf,
                _ => match present(&artifact.output_id) {
                    Some(output) => match present(&artifact.format) {
                        Some(format) => format!("{output}.{}", format.to_lowercase()),
                        None => output.to_string(),
                    },
                    None => continue,
                },
            };
            let reference = any_of([present(&artifact.sha256), Some(file.as_str())])
                .unwrap_or(file.as_str())
                .to_string();
            self.push(
                Row::new(
                    "transformers",
                    reports_folder.clone(),
                    segment(&file),
                    format!("sys:report_file:{name}:{reference}"),
                    json!({
                        "transformer": name,
                        "output_id": present(&artifact.output_id),
                        "path": path_only(url),
                    }),
                )
                .recognised(artifact.size_bytes.unwrap_or(0), present(&artifact.format))
                .bytes(artifact.size_bytes)
                .digest(present(&artifact.sha256))
                .status(Status::Durable)
                .source(SourceKind::ReportOutput, "report artifact")
                .owner(present(&metadata.updated_by))
                .modified(present(&metadata.updated_at))
                .transformer_link(name),
            );
        }
        reserved
    }

    fn transformer_version(&mut self, row: &Value) {
        let version: VersionRow = decode(row);
        let object: ObjectRow = decode(&version.object);
        let Some(name) = any_of([present(&version.transformer_id), present(&object.id)]) else {
            return;
        };
        if is_reserved_transformer_name(name) {
            return;
        }
        let Some(version_id) = present(&version.version_id) else {
            return;
        };
        let label = match present(&version.version) {
            Some(number) => format!("v{number}"),
            None => format!("v{version_id}"),
        };
        self.push(
            Row::new(
                "transformer_versions",
                format!("Transformers/{}/versions", segment(name)),
                segment(&label),
                format!("sys:transformer_version:{name}:{version_id}"),
                json!({
                    "transformer": name,
                    "version_id": version_id,
                    "playback_available": version.playback_available.unwrap_or(false),
                    "reason": present(&version.reason),
                }),
            )
            .typed(Kind::Geo, Format::Geojson)
            .bytes(version.room_bytes)
            .status(Status::Durable)
            .source(SourceKind::System, "transformer version")
            .owner(present(&version.created_by))
            .created(present(&version.created_at))
            .transformer_link(name),
        );
    }

    fn design_attachment(&mut self, row: &Value) {
        let listing: AttachmentListingRow = decode(row);
        let attachment: AttachmentRow = if listing.attachment.is_null() {
            decode(row)
        } else {
            decode(&listing.attachment)
        };
        let Some(attachment_id) = present(&attachment.attachment_id) else {
            return;
        };
        let object: ObjectRow = decode(&attachment.object);
        let key = present(&attachment.object_key)
            .and_then(|key| key.rsplit(':').next())
            .filter(|name| !name.is_empty());
        let Some(name) = any_of([present(&object.id), key]) else {
            return;
        };
        if is_reserved_transformer_name(name) {
            return;
        }
        let label = any_of([
            present(&attachment.label),
            present(&attachment.file_name),
            Some(attachment_id),
        ])
        .unwrap_or(attachment_id);
        let folder = format!(
            "Transformers/{}/attachments/{}",
            segment(name),
            segment(label)
        );
        let revisions = decode::<Vec<RevisionRow>>(&listing.revisions);
        if revisions.is_empty() {
            let revision_id = present(&attachment.latest_revision_id).unwrap_or_default();
            let leaf = revision_leaf(
                present(&attachment.latest_revision),
                revision_id,
                present(&attachment.file_name),
            );
            self.push(
                Row::new(
                    "design_attachments",
                    folder,
                    leaf,
                    format!("sys:design_attachment:{attachment_id}:{revision_id}"),
                    json!({
                        "attachment_id": attachment_id,
                        "revision_id": revision_id,
                        "transformer": name,
                    }),
                )
                .recognised(0, present(&attachment.media_type))
                .status(Status::Durable)
                .source(SourceKind::Upload, "design attachment")
                .owner(present(&attachment.created_by))
                .created(present(&attachment.created_at))
                .modified(present(&attachment.updated_at))
                .transformer_link(name),
            );
            return;
        }
        for revision in revisions {
            let revision_id = present(&revision.revision_id).unwrap_or_default();
            let latest = match present(&attachment.latest_revision_id) {
                Some(latest) => latest == revision_id,
                None => present(&revision.revision) == present(&attachment.latest_revision),
            };
            let leaf = revision_leaf(
                present(&revision.revision),
                revision_id,
                any_of([present(&revision.file_name), present(&attachment.file_name)]),
            );
            self.push(
                Row::new(
                    "design_attachments",
                    folder.clone(),
                    leaf,
                    format!("sys:design_attachment:{attachment_id}:{revision_id}"),
                    json!({
                        "attachment_id": attachment_id,
                        "revision_id": revision_id,
                        "transformer": name,
                    }),
                )
                .recognised(
                    revision.byte_size.unwrap_or(0),
                    present(&revision.media_type),
                )
                .bytes(revision.byte_size)
                .digest(present(&revision.byte_digest))
                .status(if latest {
                    Status::Durable
                } else {
                    Status::Archive
                })
                .source(SourceKind::Upload, "design attachment revision")
                .owner(present(&revision.created_by))
                .created(present(&revision.created_at))
                .transformer_link(name),
            );
        }
    }

    // ── MV models ───────────────────────────────────────────────────────────

    fn grid_model(&mut self, row: &Value) {
        let model: GridModelRow = decode(row);
        let Some(model_id) = present(&model.model_id) else {
            return;
        };
        let display = any_of([present(&model.display_name), Some(model_id)]).unwrap_or(model_id);
        let folder = format!("MV models/{}/revisions", segment(display));
        let deleted = present(&model.state) == Some("deleted");
        let mut revisions = decode::<Vec<GridVersionRow>>(&model.revisions);
        if revisions.is_empty() {
            revisions = decode::<Vec<GridVersionRow>>(&model.versions);
        }
        if revisions.is_empty() {
            let Some(head) = present(&model.head_revision_id) else {
                return;
            };
            self.push(
                Row::new(
                    "grid_models",
                    folder,
                    segment(&format!("{head}.dsgrid")),
                    format!("sys:grid_revision:{model_id}:{head}"),
                    json!({ "model_id": model_id, "revision_id": head }),
                )
                .typed(Kind::Other, Format::Dsgrid)
                .digest(present(&model.head_model_digest))
                .status(if deleted {
                    Status::Archive
                } else {
                    Status::Durable
                })
                .source(SourceKind::Upload, "grid model revision")
                .modified(present(&model.updated_at)),
            );
            return;
        }
        for revision in revisions {
            let Some(revision_id) = present(&revision.revision_id) else {
                continue;
            };
            let head = present(&model.head_revision_id) == Some(revision_id);
            self.push(
                Row::new(
                    "grid_models",
                    folder.clone(),
                    segment(&format!("{revision_id}.dsgrid")),
                    format!("sys:grid_revision:{model_id}:{revision_id}"),
                    json!({ "model_id": model_id, "revision_id": revision_id }),
                )
                .typed(Kind::Other, Format::Dsgrid)
                .status(if head && !deleted {
                    Status::Durable
                } else {
                    Status::Archive
                })
                .source(SourceKind::Upload, "grid model revision")
                .owner(present(&revision.actor))
                .created(present(&revision.created_at)),
            );
        }
    }

    fn grid_export(&mut self, row: &Value) {
        let export: GridExportRow = decode(row);
        let Some(export_id) = present(&export.export_id) else {
            return;
        };
        let Some(model_id) = present(&export.model_id) else {
            return;
        };
        let display = any_of([
            present(&export.display_name),
            self.models.get(model_id).map(String::as_str),
            Some(model_id),
        ])
        .unwrap_or(model_id);
        let folder = format!(
            "MV models/{}/exports/{}",
            segment(display),
            segment(export_id)
        );
        for item in decode::<Vec<GridExportItemRow>>(&export.items) {
            let Some(output_id) = present(&item.output_id) else {
                continue;
            };
            let leaf = match present(&item.format) {
                Some(format) => format!("{output_id}.{}", format.to_lowercase()),
                None => output_id.to_string(),
            };
            self.push(
                Row::new(
                    "grid_exports",
                    folder.clone(),
                    segment(&leaf),
                    format!("sys:grid_export:{model_id}:{export_id}:{output_id}"),
                    json!({
                        "model_id": model_id,
                        "export_id": export_id,
                        "output_id": output_id,
                    }),
                )
                .recognised(0, present(&item.format))
                .digest(present(&export.model_digest))
                .status(Status::Durable)
                .source(SourceKind::ReportOutput, "grid export")
                .created(present(&export.created_at)),
            );
        }
    }

    // ── Survey, Project data, My data ───────────────────────────────────────

    fn survey_media(&mut self, row: &Value) {
        let media: SurveyMediaRow = decode(row);
        let Some(form) = present(&media.form) else {
            return;
        };
        let Some(entry) = present(&media.entry_id) else {
            return;
        };
        let field = present(&media.field).unwrap_or_default();
        let leaf = match any_of([present(&media.name), present(&media.path), Some(field)]) {
            Some(named) if !named.is_empty() => leaf_of(named),
            _ => "media".to_string(),
        };
        self.push(
            Row::new(
                "survey_media",
                format!("Survey/{}/{}", segment(form), segment(entry)),
                segment(&leaf),
                format!("sys:survey_media:{form}:{entry}:{field}"),
                json!({
                    "form": form,
                    "entry_id": entry,
                    "field": field,
                    "path": present(&media.path).map(path_only),
                }),
            )
            .recognised(media.size_bytes.unwrap_or(0), present(&media.content_type))
            .bytes(media.size_bytes)
            .status(Status::Durable)
            .source(SourceKind::DeviceScan, "survey entry media")
            .owner(present(&media.created_by))
            .created(present(&media.captured_at)),
        );
    }

    fn upload_document(&mut self, row: &Value, source: &'static str, root: &str) {
        let upload: UploadDocumentRow = decode(row);
        let Some(upload_id) = present(&upload.upload_id) else {
            return;
        };
        let leaf = match present(&upload.file_name) {
            Some(file_name) => leaf_of(file_name),
            None => match present(&upload.blob_path) {
                Some(path) => leaf_of(path),
                None => upload_id.to_string(),
            },
        };
        self.push(
            Row::new(
                source,
                root.to_string(),
                segment(&leaf),
                format!("sys:{source}:{upload_id}"),
                json!({
                    "upload_id": upload_id,
                    "blob_path": present(&upload.blob_path).map(path_only),
                }),
            )
            .recognised(upload.total_bytes.unwrap_or(0), None)
            .bytes(upload.total_bytes)
            .status(Status::Fresh)
            .source(SourceKind::Upload, "upload")
            .owner(any_of([
                present(&upload.uploaded_by),
                present(&upload.user_email),
            ]))
            .created(present(&upload.created_at)),
        );
    }

    // ── Project work ────────────────────────────────────────────────────────

    fn pm_attachment(&mut self, row: &Value) {
        let attachment: PmAttachmentRow = decode(row);
        let Some(id) = present(&attachment.id) else {
            return;
        };
        let metadata: PmMetadataRow = decode(&attachment.metadata);
        let capture: PmCaptureRow = decode(&attachment.capture);
        let linked = present(&attachment.linked_entity_id).unwrap_or_default();
        let leaf = match present(&attachment.original_name) {
            Some(name) => leaf_of(name),
            None => match present(&attachment.storage_path) {
                Some(path) => leaf_of(path),
                None => id.to_string(),
            },
        };
        let (folder, link) = match present(&attachment.linked_entity_type) {
            Some("task") => match self.tasks.get(linked) {
                Some(wbs) => (
                    Some(format!("Project work/tasks/{}", segment(wbs))),
                    Some(Link::PmTask {
                        id: linked.to_string(),
                    }),
                ),
                None => (None, None),
            },
            Some("record") => match self.records.get(linked) {
                Some(reference) => (
                    Some(format!("Project work/records/{}", segment(reference))),
                    Some(Link::PmRecord {
                        id: linked.to_string(),
                    }),
                ),
                None => (None, None),
            },
            _ => (None, None),
        };
        let Some(folder) = folder else {
            self.unclassified("pm_attachments", row, Some(&leaf));
            return;
        };
        let status = match present(&attachment.document_state) {
            Some("issued") | Some("approved") | Some("final") => Status::Durable,
            Some("superseded") => Status::Archive,
            _ => Status::Fresh,
        };
        let mut projected = Row::new(
            "pm_attachments",
            folder,
            segment(&leaf),
            format!("sys:pm_attachment:{id}"),
            json!({
                "attachment_id": id,
                "linked_entity_type": present(&attachment.linked_entity_type),
                "linked_entity_id": linked,
                "storage_path": present(&attachment.storage_path).map(path_only),
            }),
        )
        .recognised(
            attachment.size_bytes.unwrap_or(0),
            present(&attachment.content_type),
        )
        .bytes(attachment.size_bytes)
        .digest(present(&attachment.checksum))
        .status(status)
        .source(SourceKind::TaskAttachment, "project work attachment")
        .owner(present(&metadata.created_by))
        .created(any_of([
            present(&capture.captured_at),
            present(&metadata.created_at),
        ]))
        .modified(present(&metadata.updated_at));
        if let Some(link) = link {
            projected = projected.link(link);
        }
        self.push(projected);
    }

    // ── Reports, Solar ──────────────────────────────────────────────────────

    fn compounded(&mut self, row: &Value) {
        let entry: CompoundedRow = decode(row);
        let Some(stem) = present(&entry.stem) else {
            return;
        };
        let leaf = match present(&entry.filename) {
            Some(filename) => leaf_of(filename),
            None => format!("{stem}.zip"),
        };
        self.push(
            Row::new(
                "compounded",
                "Reports/combined",
                segment(&leaf),
                format!("sys:compounded:{stem}"),
                json!({
                    "stem": stem,
                    "gcs_path": present(&entry.gcs_path).map(path_only),
                }),
            )
            .recognised(0, None)
            .status(Status::Durable)
            .source(SourceKind::ReportOutput, "compounded archive")
            .owner(present(&entry.created_by))
            .created(present(&entry.created_at)),
        );
    }

    fn compute_artifact(&mut self, row: &Value) {
        let head: ComputeHeadRow = decode(row);
        let Some(work_id) = present(&head.work_id) else {
            return;
        };
        for output in decode::<Vec<ComputeOutputRow>>(&head.outputs) {
            let Some(output_id) = present(&output.output_id) else {
                continue;
            };
            let leaf = match present(&output.format) {
                Some(format) => format!("{output_id}.{}", format.to_lowercase()),
                None => output_id.to_string(),
            };
            self.push(
                Row::new(
                    "compute_artifacts",
                    format!("Reports/compute/{}", segment(work_id)),
                    segment(&leaf),
                    format!("sys:compute_artifact:{work_id}:{output_id}"),
                    json!({
                        "work_id": work_id,
                        "output_id": output_id,
                        "engine": present(&head.engine),
                        "operation": present(&head.operation),
                        "variant": present(&head.variant),
                    }),
                )
                .recognised(output.size_bytes.unwrap_or(0), present(&output.format))
                .bytes(output.size_bytes)
                .digest(present(&output.sha256))
                .status(Status::Durable)
                .source(SourceKind::ReportOutput, "compute artifact")
                .modified(present(&head.updated_at)),
            );
        }
    }

    fn solar(&mut self, row: &Value) {
        let solar: SolarRow = decode(row);
        let Some(city) = present(&solar.city) else {
            return;
        };
        let head: SolarHeadRow = decode(&solar.head);
        if head.run_id.is_some() || head.report_input_sha256.is_some() {
            self.push(
                Row::new(
                    "solar",
                    format!("Solar/{}/results", segment(city)),
                    "report_input.json",
                    format!("sys:solar_result:{city}"),
                    json!({
                        "city": city,
                        "run_id": present(&head.run_id),
                        "report_input_sha256": present(&head.report_input_sha256),
                    }),
                )
                .typed(Kind::Other, Format::Unknown)
                .bytes(head.report_input_bytes)
                .digest(present(&head.report_input_sha256))
                .status(Status::Durable)
                .source(SourceKind::ReportOutput, "solar result")
                .created(present(&head.published_at)),
            );
        }
        for artifact in decode::<Vec<SolarArtifactRow>>(&solar.artifacts) {
            let Some(name) = present(&artifact.name) else {
                continue;
            };
            self.push(
                Row::new(
                    "solar",
                    format!("Solar/{}/artifacts", segment(city)),
                    segment(&leaf_of(name)),
                    format!("sys:solar_artifact:{city}:{name}"),
                    json!({
                        "city": city,
                        "name": name,
                        "gcs_path": present(&artifact.gcs_path).map(path_only),
                    }),
                )
                .recognised(
                    artifact.size_bytes.unwrap_or(0),
                    present(&artifact.content_type),
                )
                .bytes(artifact.size_bytes)
                .status(Status::Durable)
                .source(SourceKind::ReportOutput, "solar artifact")
                .created(present(&head.published_at)),
            );
        }
    }

    // ── Local data, Prints ──────────────────────────────────────────────────

    fn dataset_room(&mut self, row: &Value) {
        let room: DatasetRoomRow = decode(row);
        let Some(dataset) = present(&room.dataset_id) else {
            return;
        };
        self.push(
            Row::new(
                "dataset_rooms",
                "Local data/rooms",
                segment(dataset),
                format!("sys:dataset_room:{dataset}"),
                json!({
                    "dataset": dataset,
                    "rows": room.rows,
                    "cells": room.cells,
                    "freshness": present(&room.freshness),
                    "expires_at": present(&room.expires_at),
                    "pending_queries": room.pending_queries,
                }),
            )
            .typed(Kind::Other, Format::Unknown)
            .bytes(room.bytes)
            .status(Status::Fresh)
            .source(SourceKind::System, "project dataset room"),
        );
    }

    fn national_dataset(&mut self, row: &Value) {
        let dataset: NationalDatasetRow = decode(row);
        let Some(name) = present(&dataset.dataset_id) else {
            return;
        };
        self.push(
            Row::new(
                "national_datasets",
                "Local data/national",
                segment(name),
                format!("sys:national_dataset:{name}"),
                json!({
                    "dataset": name,
                    "status": present(&dataset.status),
                    "ready": dataset.ready.unwrap_or(false),
                }),
            )
            .typed(Kind::Other, Format::Unknown)
            .bytes(dataset.bytes)
            .status(Status::Fresh)
            .source(SourceKind::System, "national dataset"),
        );
    }

    fn report_room(&mut self, row: &Value) {
        let room: ReportRoomRow = decode(row);
        let Some(transformer) = present(&room.transformer) else {
            return;
        };
        self.push(
            Row::new(
                "report_rooms",
                "Local data/report rooms",
                segment(transformer),
                format!("sys:report_room:{transformer}"),
                json!({
                    "transformer": transformer,
                    "scope": present(&room.scope),
                    "freshness": present(&room.freshness),
                    "dirty": room.dirty.unwrap_or(false),
                }),
            )
            .typed(Kind::Other, Format::Unknown)
            .bytes(room.bytes)
            .status(Status::Fresh)
            .source(SourceKind::System, "reporter room")
            .transformer_link(transformer),
        );
    }

    fn local_print(&mut self, row: &Value) {
        let print: LocalPrintRow = decode(row);
        let Some(locator) = present(&print.locator) else {
            return;
        };
        let leaf = match present(&print.filename) {
            Some(filename) => leaf_of(filename),
            None => leaf_of(locator),
        };
        // A map output keeps its canonical identity across local reprints.
        // District maps remain project MV reports; custom maps are printouts.
        let (folder, id, description) = match present(&print.family) {
            Some("custom-map") => (
                "Prints/custom maps",
                format!("sys:local_print:map:{leaf}"),
                "custom map printout",
            ),
            Some("mv-map") => (
                "Reports/MV",
                format!("sys:local_print:map:{leaf}"),
                "project MV report",
            ),
            _ => (
                "Prints/local",
                format!("sys:local_print:{locator}"),
                "local print artifact",
            ),
        };
        let mut projected = Row::new(
            "local_prints",
            folder,
            segment(&leaf),
            id,
            json!({
                "locator": locator,
                "output_id": present(&print.output_id),
                "paper_size": present(&print.paper_size),
                "family": present(&print.family),
                "map_id": present(&print.map_id),
                "attachment_type": present(&print.family).map(|_| "printout"),
            }),
        )
        .recognised(print.size_bytes.unwrap_or(0), present(&print.format))
        .bytes(print.size_bytes)
        .digest(present(&print.sha256))
        .status(Status::Durable)
        .source(SourceKind::ReportOutput, description);
        if let Some(transformer) = present(&print.transformer) {
            projected = projected.transformer_link(transformer);
        }
        self.push(projected);
    }

    fn print_setup(&mut self, row: &Value) {
        let setup: PrintSetupRow = decode(row);
        let Some(id) = present(&setup.id) else {
            return;
        };
        let leaf = any_of([present(&setup.name), Some(id)]).unwrap_or(id);
        self.push(
            Row::new(
                "print_setups",
                "Prints/setups",
                segment(leaf),
                format!("sys:print_setup:{id}"),
                json!({ "layout_id": id, "scope": present(&setup.scope) }),
            )
            .typed(Kind::Note, Format::Unknown)
            .status(Status::Fresh)
            .source(SourceKind::System, "print setup")
            .owner(present(&setup.updated_by))
            .modified(present(&setup.updated_at)),
        );
    }
}

/// A design-attachment revision leaf: the revision is the leaf (§2.5), keeping
/// the file's extension so its format stays recognisable.
fn revision_leaf(revision: Option<&str>, revision_id: &str, file_name: Option<&str>) -> String {
    let stem = match revision {
        Some(number) => format!("r{number}"),
        None if !revision_id.is_empty() => revision_id.to_string(),
        None => "r0".to_string(),
    };
    match file_name.and_then(extension_of) {
        Some(extension) => format!("{stem}.{extension}"),
        None => stem,
    }
}

/// Builds one lookup map from a source list, keeping the first row per id so
/// the map does not depend on list order.
fn lookup_map(
    request: &TreeRequest,
    source: &str,
    read: impl Fn(&Value) -> Option<(String, String)>,
) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(rows) = request.sources.get(source) else {
        return map;
    };
    for row in rows {
        if let Some((key, value)) = read(row) {
            map.entry(key).or_insert(value);
        }
    }
    map
}

// ── Correspondence (correspondence.md §Asset, owner ruling 2026-09-20) ──────

/// `Correspondence/<thread>`: every correspondence record and what it carries,
/// indexed in the project's assets as one system folder per thread.
///
/// A projection over rows the project already holds — the `pm_records`
/// lookup rows, the `pm_parties` names and the catalogued `assets` — rebuilt
/// at read time and never written (D15/D16). A thread folder lists the
/// catalogued asset each of its records was authored against
/// (`source_asset_id`, `meeting_note_asset_id`), the registered documents it
/// names (`document_ids`), every catalogued asset linked `pm_record:<id>` to
/// one of its records, and, under a mail container, the MIME parts the row
/// carries. Each listing is the catalogued row itself under a second path —
/// the same `asset_id`, so `read`/`preview --member` work from here exactly
/// as from the declared folder — with an `origin` naming the thread and the
/// role the asset plays. A record the caller may not read has no row and so
/// no folder; a thread whose assets the caller may not read has no folder
/// either: nothing here is a placeholder for what was redacted.
mod correspondence {
    use super::*;

    /// One thread as the lookup rows describe it.
    struct Thread {
        thread_id: String,
        root_subject: String,
        root_date: String,
        party_ids: Vec<String>,
        record_ids: Vec<String>,
        /// (asset id, role, record id) in record order.
        members: Vec<(String, &'static str, String)>,
    }

    pub(super) fn project(
        request: &TreeRequest,
        parties: &BTreeMap<String, String>,
        roots: &mut BTreeMap<String, Node>,
    ) {
        let Some(rows) = request.sources.get("pm_records") else {
            return;
        };
        let mut threads: BTreeMap<String, Thread> = BTreeMap::new();
        let mut records: Vec<RecordRow> = rows.iter().map(RecordRow::flattened).collect();
        // Time order decides which record is the root of a thread whose root
        // was not loaded, and the order the members are listed in.
        records.sort_by(|left, right| {
            present(&left.happened_at)
                .unwrap_or("")
                .cmp(present(&right.happened_at).unwrap_or(""))
                .then_with(|| present(&left.id).cmp(&present(&right.id)))
        });
        for record in &records {
            let Some(id) = present(&record.id) else {
                continue;
            };
            let thread_id = present(&record.thread_id).unwrap_or(id).to_string();
            let thread = threads.entry(thread_id.clone()).or_insert_with(|| Thread {
                thread_id: thread_id.clone(),
                root_subject: String::new(),
                root_date: String::new(),
                party_ids: Vec::new(),
                record_ids: Vec::new(),
                members: Vec::new(),
            });
            // The thread's own record names it; failing that, its earliest.
            if id == thread_id || thread.root_subject.is_empty() {
                thread.root_subject = present(&record.subject).unwrap_or("").to_string();
                thread.root_date = present(&record.happened_at)
                    .map(|stamp| stamp.chars().take(10).collect())
                    .unwrap_or_default();
            }
            for party in string_list(&record.external_party_ids) {
                if !thread.party_ids.contains(&party) {
                    thread.party_ids.push(party);
                }
            }
            thread.record_ids.push(id.to_string());
            if let Some(source) = present(&record.source_asset_id) {
                thread
                    .members
                    .push((source.to_string(), "source", id.to_string()));
            }
            if let Some(note) = present(&record.meeting_note_asset_id) {
                thread
                    .members
                    .push((note.to_string(), "meeting_note", id.to_string()));
            }
            for document in string_list(&record.document_ids) {
                thread.members.push((document, "document", id.to_string()));
            }
        }
        // Assets linked `pm_record:<id>` join the thread of that record.
        let mut record_thread: BTreeMap<&str, &str> = BTreeMap::new();
        for thread in threads.values() {
            for record in &thread.record_ids {
                record_thread.insert(record.as_str(), thread.thread_id.as_str());
            }
        }
        let mut linked: Vec<(String, String, String)> = Vec::new();
        for asset in &request.assets {
            for link in &asset.links {
                if let Link::PmRecord { id } = link {
                    if let Some(thread_id) = record_thread.get(id.as_str()) {
                        linked.push(((*thread_id).to_string(), asset.asset_id.clone(), id.clone()));
                    }
                }
            }
        }
        for (thread_id, asset_id, record_id) in linked {
            if let Some(thread) = threads.get_mut(&thread_id) {
                thread.members.push((asset_id, "attachment", record_id));
            }
        }

        let catalogue: BTreeMap<&str, &Asset> = request
            .assets
            .iter()
            .map(|asset| (asset.asset_id.as_str(), asset))
            .collect();
        for thread in threads.values() {
            let folder = format!("Correspondence/{}", segment(&folder_name(thread, parties)));
            let mut listed: BTreeSet<&str> = BTreeSet::new();
            for (asset_id, role, record_id) in &thread.members {
                // One listing per asset per thread, under its first role.
                if !listed.insert(asset_id.as_str()) {
                    continue;
                }
                let Some(row) = catalogue.get(asset_id.as_str()) else {
                    // Named by the record but not in the catalogue the caller
                    // may read: no row, no placeholder.
                    continue;
                };
                let mut asset = (*row).clone();
                asset.folder = folder.clone();
                asset.origin = Some(Origin {
                    source: "correspondence".to_string(),
                    r#ref: json!({
                        "thread_id": thread.thread_id,
                        "record_id": record_id,
                        "role": role,
                        "asset_id": asset_id,
                    }),
                });
                let parts = asset.mail.as_ref().map(|mail| mail.parts.clone());
                place(roots, &folder, asset, FolderKind::System);
                if let Some(parts) = parts {
                    let container_folder = format!("{folder}/{}", segment(&row.name));
                    for part in parts {
                        place(
                            roots,
                            &container_folder,
                            mail_part_row(row, &thread.thread_id, &part),
                            FolderKind::System,
                        );
                    }
                }
            }
        }
    }

    /// `<party names> — <root subject> (<root date>)`, each piece present only
    /// when the thread carries it; never empty.
    fn folder_name(thread: &Thread, parties: &BTreeMap<String, String>) -> String {
        let names: Vec<&str> = thread
            .party_ids
            .iter()
            .take(2)
            .map(|id| parties.get(id).map_or(id.as_str(), String::as_str))
            .collect();
        let mut name = names.join(" + ");
        if !thread.root_subject.is_empty() {
            if !name.is_empty() {
                name.push_str(" — ");
            }
            name.push_str(&thread.root_subject);
        }
        if !thread.root_date.is_empty() {
            name.push_str(&format!(" ({})", thread.root_date));
        }
        if name.trim().is_empty() {
            name = thread.thread_id.clone();
        }
        name
    }

    /// One MIME part of a mail container as a row of its own: the member
    /// name, the decoded size, the kernel's reading of its kind, and an origin
    /// that says exactly how to read it (`read --asset <id> --member <path>`).
    fn mail_part_row(container: &Asset, thread_id: &str, part: &MailPart) -> Asset {
        let (kind, format) = recognise_name(&part.path, part.bytes, Some(&part.content_type));
        Asset {
            reference: None,
            schema: ASSET_SCHEMA.to_string(),
            asset_id: format!("sys:mail_part:{}:{}", container.asset_id, part.path),
            project_id: container.project_id.clone(),
            folder: String::new(),
            name: part.path.clone(),
            kind,
            format,
            bytes: part.bytes,
            digest: None,
            status: container.status,
            sensitivity: container.sensitivity,
            dated: container.dated.clone(),
            owner: container.owner.clone(),
            source: Source {
                kind: SourceKind::Container,
                detail: Some(format!("mail part {} of {}", part.section, container.name)),
            },
            classified_by: ClassifiedBy::Kernel,
            container: Some(container.asset_id.clone()),
            links: Vec::new(),
            origin: Some(Origin {
                source: "correspondence".to_string(),
                r#ref: json!({
                    "thread_id": thread_id,
                    "asset_id": container.asset_id,
                    "member": part.path,
                    "section": part.section,
                    "disposition": part.disposition,
                    "content_type": part.content_type,
                }),
            }),
            document: None,
            mail: None,
            external: None,
            bytes_held: container.bytes_held,
            folder_id: None,
            content_type: Some(part.content_type.clone()),
            created_by: None,
            updated_at: None,
            updated_by: None,
            version: None,
        }
    }
}

// ── the foreign wire rows, read leniently ───────────────────────────────────
//
// Each struct below reads only the fields the projection needs from one
// client's wire document (ds-web `src/lib/api/*`, and the host-derived shapes
// §2.5 names). None of them denies unknown fields: they are foreign documents
// and will grow fields that are none of the kernel's business.

/// A boolean that may arrive as a string or a number.
fn lenient_bool<'de, D: Deserializer<'de>>(input: D) -> Result<Option<bool>, D::Error> {
    Ok(match Option::<Value>::deserialize(input)? {
        Some(Value::Bool(flag)) => Some(flag),
        Some(Value::String(text)) => match text.trim().to_lowercase().as_str() {
            "true" | "yes" => Some(true),
            "false" | "no" => Some(false),
            _ => None,
        },
        Some(Value::Number(number)) => number.as_u64().map(|number| number != 0),
        _ => None,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TransformerRow {
    #[serde(deserialize_with = "lenient_text")]
    name: Option<String>,
    metadata: Value,
    uploads: Value,
    report_files: Value,
    print_files: Value,
    report_artifacts: Value,
    report_metadata: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MetadataRow {
    #[serde(deserialize_with = "lenient_text")]
    updated_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    updated_by: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UploadsRow {
    draft: Vec<UploadRow>,
    sketch: Vec<UploadRow>,
    process: Vec<UploadRow>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UploadRow {
    #[serde(deserialize_with = "lenient_text")]
    filename: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    blob_path: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    uploaded_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    uploaded_by: Option<String>,
    #[serde(alias = "bytes", deserialize_with = "lenient_number")]
    size_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReportArtifactRow {
    #[serde(deserialize_with = "lenient_text")]
    url: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    output_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    format: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    sha256: Option<String>,
    #[serde(alias = "bytes", deserialize_with = "lenient_number")]
    size_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ObjectRow {
    #[serde(deserialize_with = "lenient_text")]
    id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VersionRow {
    #[serde(deserialize_with = "lenient_text")]
    transformer_id: Option<String>,
    object: Value,
    #[serde(deserialize_with = "lenient_text")]
    version_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    version: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    reason: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_by: Option<String>,
    #[serde(deserialize_with = "lenient_bool")]
    playback_available: Option<bool>,
    #[serde(deserialize_with = "lenient_number")]
    room_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AttachmentListingRow {
    attachment: Value,
    revisions: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AttachmentRow {
    #[serde(deserialize_with = "lenient_text")]
    attachment_id: Option<String>,
    object: Value,
    #[serde(deserialize_with = "lenient_text")]
    object_key: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    label: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    file_name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    media_type: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    latest_revision_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    latest_revision: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_by: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    updated_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RevisionRow {
    #[serde(deserialize_with = "lenient_text")]
    revision_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    revision: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    file_name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    media_type: Option<String>,
    #[serde(deserialize_with = "lenient_number")]
    byte_size: Option<u64>,
    #[serde(deserialize_with = "lenient_text")]
    byte_digest: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_by: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GridModelRow {
    #[serde(deserialize_with = "lenient_text")]
    model_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    display_name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    head_revision_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    head_model_digest: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    state: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    updated_at: Option<String>,
    revisions: Value,
    versions: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GridVersionRow {
    #[serde(deserialize_with = "lenient_text")]
    revision_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    actor: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GridExportRow {
    #[serde(deserialize_with = "lenient_text")]
    model_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    display_name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    export_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    model_digest: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_at: Option<String>,
    items: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GridExportItemRow {
    #[serde(deserialize_with = "lenient_text")]
    output_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    format: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TaskRow {
    #[serde(deserialize_with = "lenient_text")]
    id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    wbs: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    title: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RecordRow {
    #[serde(deserialize_with = "lenient_text")]
    id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    reference_number: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    subject: Option<String>,
    // The correspondence fields (ds-brain correspondence.md §Record), read
    // from the raw `data` shape ds-brain answers and the camelCase shape a
    // browser decodes, so the thread folder is the same whichever host fed
    // the rows.
    #[serde(deserialize_with = "lenient_text", alias = "thread_id")]
    thread_id: Option<String>,
    #[serde(deserialize_with = "lenient_text", alias = "happened_at")]
    happened_at: Option<String>,
    #[serde(deserialize_with = "lenient_text", alias = "source_asset_id")]
    source_asset_id: Option<String>,
    #[serde(deserialize_with = "lenient_text", alias = "meeting_note_asset_id")]
    meeting_note_asset_id: Option<String>,
    #[serde(default, alias = "document_ids")]
    document_ids: Value,
    #[serde(default, alias = "external_party_ids")]
    external_party_ids: Value,
    #[serde(default)]
    data: Value,
}

impl RecordRow {
    /// A raw ds-brain row keeps its authored fields under `data`; a decoded
    /// row carries them at the top. Whichever arrived, one reader.
    fn flattened(row: &Value) -> RecordRow {
        let mut decoded: RecordRow = decode(row);
        if decoded.data.is_object() {
            let inner: RecordRow = decode(&decoded.data);
            let take = |outer: &Option<String>, inner: &Option<String>| {
                present(outer)
                    .map(str::to_string)
                    .or_else(|| present(inner).map(str::to_string))
            };
            decoded.reference_number = take(&decoded.reference_number, &inner.reference_number);
            decoded.subject = take(&decoded.subject, &inner.subject);
            decoded.thread_id = take(&decoded.thread_id, &inner.thread_id);
            decoded.happened_at = take(&decoded.happened_at, &inner.happened_at);
            decoded.source_asset_id = take(&decoded.source_asset_id, &inner.source_asset_id);
            decoded.meeting_note_asset_id =
                take(&decoded.meeting_note_asset_id, &inner.meeting_note_asset_id);
            if decoded.document_ids.is_null() {
                decoded.document_ids = inner.document_ids;
            }
            if decoded.external_party_ids.is_null() {
                decoded.external_party_ids = inner.external_party_ids;
            }
        }
        decoded
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PartyRow {
    #[serde(deserialize_with = "lenient_text")]
    id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    name: Option<String>,
    #[serde(default)]
    data: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PmAttachmentRow {
    #[serde(deserialize_with = "lenient_text")]
    id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    linked_entity_type: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    linked_entity_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    storage_path: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    original_name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    content_type: Option<String>,
    #[serde(deserialize_with = "lenient_number")]
    size_bytes: Option<u64>,
    #[serde(deserialize_with = "lenient_text")]
    checksum: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    document_state: Option<String>,
    metadata: Value,
    capture: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PmMetadataRow {
    #[serde(deserialize_with = "lenient_text")]
    created_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_by: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    updated_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PmCaptureRow {
    #[serde(deserialize_with = "lenient_text")]
    captured_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CompoundedRow {
    #[serde(deserialize_with = "lenient_text")]
    stem: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    filename: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    gcs_path: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_by: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SurveyMediaRow {
    #[serde(deserialize_with = "lenient_text")]
    form: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    entry_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    field: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    path: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    content_type: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    captured_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_by: Option<String>,
    #[serde(alias = "bytes", deserialize_with = "lenient_number")]
    size_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UploadDocumentRow {
    #[serde(deserialize_with = "lenient_text")]
    upload_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    file_name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    created_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    uploaded_by: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    user_email: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    blob_path: Option<String>,
    #[serde(
        alias = "bytes",
        alias = "size_bytes",
        deserialize_with = "lenient_number"
    )]
    total_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ComputeHeadRow {
    #[serde(deserialize_with = "lenient_text")]
    work_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    engine: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    operation: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    variant: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    updated_at: Option<String>,
    outputs: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ComputeOutputRow {
    #[serde(deserialize_with = "lenient_text")]
    output_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    format: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    sha256: Option<String>,
    #[serde(alias = "bytes", deserialize_with = "lenient_number")]
    size_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SolarRow {
    #[serde(deserialize_with = "lenient_text")]
    city: Option<String>,
    head: Value,
    artifacts: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SolarHeadRow {
    #[serde(deserialize_with = "lenient_text")]
    run_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    report_input_sha256: Option<String>,
    #[serde(deserialize_with = "lenient_number")]
    report_input_bytes: Option<u64>,
    #[serde(deserialize_with = "lenient_text")]
    published_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SolarArtifactRow {
    #[serde(deserialize_with = "lenient_text")]
    name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    gcs_path: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    content_type: Option<String>,
    #[serde(alias = "bytes", deserialize_with = "lenient_number")]
    size_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DatasetRoomRow {
    #[serde(deserialize_with = "lenient_text")]
    dataset_id: Option<String>,
    #[serde(deserialize_with = "lenient_number")]
    rows: Option<u64>,
    #[serde(deserialize_with = "lenient_number")]
    bytes: Option<u64>,
    #[serde(deserialize_with = "lenient_number")]
    cells: Option<u64>,
    #[serde(deserialize_with = "lenient_text")]
    freshness: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    expires_at: Option<String>,
    #[serde(deserialize_with = "lenient_number")]
    pending_queries: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct NationalDatasetRow {
    #[serde(deserialize_with = "lenient_text")]
    dataset_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    status: Option<String>,
    #[serde(deserialize_with = "lenient_number")]
    bytes: Option<u64>,
    #[serde(deserialize_with = "lenient_bool")]
    ready: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReportRoomRow {
    #[serde(deserialize_with = "lenient_text")]
    transformer: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    scope: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    freshness: Option<String>,
    #[serde(deserialize_with = "lenient_bool")]
    dirty: Option<bool>,
    #[serde(deserialize_with = "lenient_number")]
    bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LocalPrintRow {
    #[serde(deserialize_with = "lenient_text")]
    family: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    map_id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    locator: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    filename: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    format: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    paper_size: Option<String>,
    #[serde(deserialize_with = "lenient_number")]
    size_bytes: Option<u64>,
    #[serde(deserialize_with = "lenient_text")]
    sha256: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    transformer: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    output_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PrintSetupRow {
    #[serde(deserialize_with = "lenient_text")]
    id: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    name: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    scope: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    updated_at: Option<String>,
    #[serde(deserialize_with = "lenient_text")]
    updated_by: Option<String>,
}

// ── assembling and filtering the tree ───────────────────────────────────────

/// One folder while the tree is being built. Children live in a `BTreeMap` so
/// the answer is ordered by name whatever order the rows arrived in.
#[derive(Debug)]
struct Node {
    kind: FolderKind,
    children: BTreeMap<String, Node>,
    assets: Vec<Asset>,
}

impl Node {
    fn new(kind: FolderKind) -> Self {
        Node {
            kind,
            children: BTreeMap::new(),
            assets: Vec::new(),
        }
    }
}

fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// The folder named by `names`, creating what is missing with `kind`. An empty
/// path addresses no folder and answers `None`.
fn ensure<'a>(
    level: &'a mut BTreeMap<String, Node>,
    names: &[&str],
    kind: FolderKind,
) -> Option<&'a mut Node> {
    let (head, rest) = names.split_first()?;
    let entry = level
        .entry((*head).to_string())
        .or_insert_with(|| Node::new(kind));
    if rest.is_empty() {
        Some(entry)
    } else {
        ensure(&mut entry.children, rest, kind)
    }
}

/// Creates the folder at `path` (and its parents) if it does not exist.
fn ensure_path(roots: &mut BTreeMap<String, Node>, path: &str, kind: FolderKind) {
    let names: Vec<&str> = segments(path).collect();
    ensure(roots, &names, kind);
}

fn place(roots: &mut BTreeMap<String, Node>, path: &str, asset: Asset, kind: FolderKind) {
    let names: Vec<&str> = segments(path).collect();
    if let Some(node) = ensure(roots, &names, kind) {
        node.assets.push(asset);
    }
}

/// The §2.6 filters over the loaded tree: `query` is a case-insensitive
/// substring of the asset's own words, `link` is exact.
struct Filter<'a> {
    query: Option<String>,
    link: Option<&'a Link>,
}

impl Filter<'_> {
    fn active(&self) -> bool {
        self.query.is_some() || self.link.is_some()
    }

    fn keeps(&self, asset: &Asset, path: &str) -> bool {
        if let Some(link) = self.link {
            if !asset.links.iter().any(|held| held == link) {
                return false;
            }
        }
        let Some(query) = &self.query else {
            return true;
        };
        let owner_id = asset.owner.as_ref().map(|owner| owner.id.as_str());
        let owner_display = asset
            .owner
            .as_ref()
            .and_then(|owner| owner.display.as_deref());
        [
            Some(asset.name.as_str()),
            Some(path),
            Some(asset.folder.as_str()),
            Some(asset.kind.as_str()),
            Some(asset.format.as_str()),
            Some(asset.status.as_str()),
            owner_display,
            owner_id,
        ]
        .into_iter()
        .flatten()
        .any(|field| field.to_lowercase().contains(query))
    }
}

/// Filters a subtree in place; returns whether anything survived.
fn prune(node: &mut Node, path: &str, filter: &Filter) -> bool {
    node.assets.retain(|asset| filter.keeps(asset, path));
    let children = std::mem::take(&mut node.children);
    for (name, mut child) in children {
        let child_path = join(path, &name);
        if prune(&mut child, &child_path, filter) {
            node.children.insert(name, child);
        }
    }
    !node.assets.is_empty() || !node.children.is_empty()
}

/// Renders one folder. `depth` is how many levels including this one may be
/// returned; children beyond it are a `truncated` cut, never a silent one.
fn render(
    name: &str,
    path: &str,
    node: Node,
    depth: Option<u32>,
    truncated: &mut bool,
) -> TreeFolder {
    let child_depth = depth.map(|depth| depth.saturating_sub(1));
    // The count is the folder's own, whether or not `depth` lets its
    // children be rendered: a cut is reported by `truncated`, never by a
    // count that says zero.
    let folders_count = node.children.len() as u64;
    let mut children = Vec::new();
    if child_depth == Some(0) {
        if !node.children.is_empty() {
            *truncated = true;
        }
    } else {
        for (child_name, child) in node.children {
            let child_path = join(path, &child_name);
            children.push(render(
                &child_name,
                &child_path,
                child,
                child_depth,
                truncated,
            ));
        }
    }
    let mut assets = node.assets;
    assets.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.asset_id.cmp(&right.asset_id))
            .then_with(|| {
                canonical_json(&serde_json::to_value(left).unwrap_or(Value::Null)).cmp(
                    &canonical_json(&serde_json::to_value(right).unwrap_or(Value::Null)),
                )
            })
    });
    TreeFolder {
        kind: node.kind,
        path: path.to_string(),
        name: name.to_string(),
        counts: Counts {
            assets: assets.len() as u64,
            folders: folders_count,
        },
        children,
        assets,
    }
}

fn total_assets(folder: &TreeFolder) -> u64 {
    folder.assets.len() as u64 + folder.children.iter().map(total_assets).sum::<u64>()
}

fn total_folders(folder: &TreeFolder) -> u64 {
    1 + folder.children.iter().map(total_folders).sum::<u64>()
}

// ── bounds, collisions and the entry points ─────────────────────────────────

fn check_sources(request: &TreeRequest) -> Result<(), String> {
    let unknown = request.sources.unknown_names();
    let Some(first) = unknown.first() else {
        return Ok(());
    };
    Err(format!(
        "unknown tree source \"{first}\"; the sources are {}",
        TREE_SOURCE_NAMES.join(", ")
    ))
}

/// The first path segment of a folder path, if it has one.
fn root_segment(path: &str) -> Option<&str> {
    segments(path).next()
}

/// The system root a name collides with, compared without case because two
/// roots called `Reports` and `reports` would be one confusion, not two
/// folders.
fn system_root_named(name: &str) -> Option<&'static str> {
    SYSTEM_ROOTS
        .iter()
        .copied()
        .find(|root| root.eq_ignore_ascii_case(name))
}

/// The one system root a declared folder may share: `correspondence/…` is the
/// natural name for the folder a project files its letters in — the
/// contract's own acceptance declares one — so refusing it would refuse the
/// project's whole tree the day the index landed. A declared folder rooted
/// there is filed under the same root as the thread index, spelled as the
/// root is spelled, and the two coexist.
const SHARED_ROOT: &str = "Correspondence";

/// A declared path with a `correspondence` root respelled to the system root.
fn respell_shared_root(path: &str) -> String {
    let mut parts: Vec<&str> = segments(path).collect();
    if let Some(first) = parts.first_mut() {
        if first.eq_ignore_ascii_case(SHARED_ROOT) {
            *first = SHARED_ROOT;
        }
    }
    parts.join("/")
}

fn check_user_folders(request: &TreeRequest) -> Result<(), String> {
    for folder in &request.folders {
        if let Some(root) = root_segment(&folder.path)
            .and_then(system_root_named)
            .filter(|root| *root != SHARED_ROOT)
        {
            return Err(format!(
                "user folder \"{}\" collides with the system root {root}; system roots are projected from the sources, not declared — rename the folder",
                folder.path
            ));
        }
    }
    for asset in &request.assets {
        if let Some(root) = root_segment(&asset.folder)
            .and_then(system_root_named)
            .filter(|root| *root != SHARED_ROOT)
        {
            return Err(format!(
                "asset \"{}\" is filed in \"{}\", which collides with the system root {root}; system roots are projected from the sources, not declared — file it elsewhere",
                asset.name, asset.folder
            ));
        }
    }
    Ok(())
}

/// The project the tree is for. `TreeRequest` carries no `project_id` of its
/// own, so a projected row inherits the one the declared folders and assets
/// already agree on (empty when the request carries neither).
fn project_id_of(request: &TreeRequest) -> String {
    request
        .assets
        .iter()
        .map(|asset| asset.project_id.as_str())
        .chain(
            request
                .folders
                .iter()
                .map(|folder| folder.project_id.as_str()),
        )
        .map(str::trim)
        .find(|id| !id.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// User folders and catalogued assets, merged at the root beside the system
/// roots (§2.5). A declared folder keeps its declared kind; a folder that only
/// an asset's `folder` path implies is materialised so the asset is never
/// dropped, and an asset filed nowhere lands under `Unclassified/assets`.
fn merge_declared(request: &TreeRequest, roots: &mut BTreeMap<String, Node>) {
    for folder in &request.folders {
        if root_segment(&folder.path).is_some() {
            ensure_path(roots, &respell_shared_root(&folder.path), folder.kind);
        }
    }
    for asset in &request.assets {
        match root_segment(&asset.folder) {
            Some(_) => {
                let mut asset = asset.clone();
                asset.folder = respell_shared_root(&asset.folder);
                let folder = asset.folder.clone();
                place(roots, &folder, asset, FolderKind::User)
            }
            None => {
                let is_tag_map = asset
                    .reference
                    .as_ref()
                    .is_some_and(|r| super::tag_maps::is_group_map(&r.role));
                let tags = super::tag_maps::tags(&asset.links);
                let folder = if is_tag_map && !tags.is_empty() {
                    // One placement, even when several tag groups reuse this asset.
                    format!(
                        "Prints/tag groups/{}/{}",
                        segment(&tags[0].definition_id),
                        segment(&tags[0].value)
                    )
                } else {
                    "Unclassified/assets".to_string()
                };
                place(roots, &folder, asset.clone(), FolderKind::System)
            }
        }
    }
}

/// `ds.assets.tree/v1`: the system-folder projection of §2.5 merged with the
/// declared folders and assets, filtered by §2.6.
///
/// Deterministic by construction: folders are held in a `BTreeMap` keyed by
/// name and assets are ordered by name, then `asset_id`, then their own
/// canonical JSON, so the same rows in any order give byte-identical output.
pub fn build(request: &TreeRequest) -> Result<TreeResponse, String> {
    request.validate()?;
    check_sources(request)?;
    check_user_folders(request)?;

    let project_id = project_id_of(request);
    let mut projector = Projector::new(&project_id);
    projector.run(request);

    let mut roots: BTreeMap<String, Node> = BTreeMap::new();
    for root in SYSTEM_ROOTS {
        if root != "Unclassified" {
            ensure_path(&mut roots, root, FolderKind::System);
        }
    }
    // A source read twice (an overlapping page, a retried request) hands the
    // same row twice; one `sys:` id is one asset, so the first placement wins
    // and a duplicate never doubles a folder count.
    // Coalesce proven identical printouts while keeping the local preview.
    // Different digests remain distinct: publication may still be pending.
    let mut placed = std::mem::take(&mut projector.placed);
    let print_key = |asset: &Asset| -> Option<(String, String, String, u64)> {
        let origin = asset.origin.as_ref()?;
        let family = origin.r#ref.get("family")?.as_str()?;
        if !matches!(family, "custom-map" | "mv-map") {
            return None;
        }
        let digest = asset.digest.as_ref()?.strip_prefix("sha256:")?;
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some((
            family.into(),
            asset.name.clone(),
            digest.to_lowercase(),
            asset.bytes,
        ))
    };
    let locals: BTreeMap<_, _> = placed
        .iter()
        .enumerate()
        .filter_map(|(i, (_, a))| {
            if a.origin.as_ref()?.source != "local_prints" {
                return None;
            }
            print_key(a).map(|key| (key, i))
        })
        .collect();
    let mut duplicates = BTreeSet::new();
    for i in 0..placed.len() {
        let asset = &placed[i].1;
        if asset
            .origin
            .as_ref()
            .is_none_or(|o| o.source != "transformers")
        {
            continue;
        }
        let Some(local) = print_key(asset).and_then(|key| locals.get(&key).copied()) else {
            continue;
        };
        if let Some(path) = asset.origin.as_ref().unwrap().r#ref.get("path").cloned() {
            placed[local].1.origin.as_mut().unwrap().r#ref["published_path"] = path;
            placed[local].1.source.detail = Some("local and published printout".into());
            duplicates.insert(placed[i].1.asset_id.clone());
        }
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (path, asset) in placed {
        if duplicates.contains(&asset.asset_id) {
            continue;
        }
        if !seen.insert(asset.asset_id.clone()) {
            continue;
        }
        place(&mut roots, &path, asset, FolderKind::System);
    }
    merge_declared(request, &mut roots);
    correspondence::project(request, &projector.parties, &mut roots);

    // `folder` roots the answer at one path; a path nothing declared or
    // projected answers with no folders rather than an error.
    let mut selected: Vec<(String, String, Node)> = Vec::new();
    if let Some(path) = request
        .folder
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let mut level = roots;
        let mut found: Option<(String, String, Node)> = None;
        let names: Vec<&str> = segments(path).collect();
        for (index, name) in names.iter().enumerate() {
            let Some(node) = level.remove(*name) else {
                found = None;
                break;
            };
            if index + 1 == names.len() {
                let full = names.join("/");
                found = Some(((*name).to_string(), full, node));
                break;
            }
            level = node.children;
        }
        if let Some(entry) = found {
            selected.push(entry);
        }
    } else {
        for (name, node) in roots {
            let path = name.clone();
            selected.push((name, path, node));
        }
    }

    if let Some(kind) = request.kind {
        selected.retain(|(_, _, node)| node.kind == kind);
    }

    let filter = Filter {
        query: request
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_lowercase),
        link: request.link.as_ref(),
    };
    if filter.active() {
        let mut kept = Vec::new();
        for (name, path, mut node) in selected {
            if prune(&mut node, &path, &filter) {
                kept.push((name, path, node));
            }
        }
        selected = kept;
    }

    let mut truncated = false;
    let mut folders = Vec::new();
    if request.depth == Some(0) {
        truncated = !selected.is_empty();
    } else {
        for (name, path, node) in selected {
            folders.push(render(&name, &path, node, request.depth, &mut truncated));
        }
    }

    Ok(TreeResponse {
        schema: TREE_SCHEMA.to_string(),
        total_assets: folders.iter().map(total_assets).sum(),
        total_folders: folders.iter().map(total_folders).sum(),
        folders,
        truncated,
    })
}

/// [`build`] rendered as JSON with the *not loaded* markers of §2.5 that the
/// shared `TreeFolder` / `TreeResponse` types have no field for: a system root
/// whose sources were all absent carries `not_loaded: true`, a root missing
/// some of its sources carries `not_loaded_sources: [...]`, and the response
/// carries `not_loaded_sources` for the whole request. An absent source is
/// never rendered as an empty root.
pub fn build_value(request: &TreeRequest) -> Result<Value, String> {
    let response = build(request)?;
    let mut value = serde_json::to_value(&response).map_err(|error| error.to_string())?;
    let absent: BTreeSet<&str> = not_loaded_sources(request).into_iter().collect();

    if let Some(folders) = value.get_mut("folders").and_then(Value::as_array_mut) {
        for folder in folders.iter_mut() {
            let Some(folder) = folder.as_object_mut() else {
                continue;
            };
            let Some(path) = folder.get("path").and_then(Value::as_str) else {
                continue;
            };
            let path = path.to_string();
            let feeding: Vec<&str> = SOURCE_ROOTS
                .iter()
                .filter(|(_, root)| *root == path)
                .map(|(source, _)| *source)
                .collect();
            if feeding.is_empty() {
                continue;
            }
            let missing: Vec<&str> = feeding
                .iter()
                .copied()
                .filter(|source| absent.contains(source))
                .collect();
            if missing.is_empty() {
                continue;
            }
            if missing.len() == feeding.len() {
                folder.insert("not_loaded".to_string(), json!(true));
            }
            folder.insert("not_loaded_sources".to_string(), json!(missing));
        }
    }
    if let Some(response) = value.as_object_mut() {
        let missing: Vec<&str> = absent.into_iter().collect();
        response.insert("not_loaded_sources".to_string(), json!(missing));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{evaluate, Status, MAX_QUERY_CHARS, MAX_TREE_SOURCE_ROWS, REQUEST_SCHEMA};

    fn tree_request(body: Value) -> TreeRequest {
        let mut request = match body {
            Value::Object(map) => map,
            other => panic!("fixture must be an object, got {other}"),
        };
        request.insert("schema".to_string(), json!(REQUEST_SCHEMA));
        serde_json::from_value(Value::Object(request)).expect("fixture decodes as a TreeRequest")
    }

    /// (folder path, asset name, asset id) for every asset in the answer.
    fn rows(response: &TreeResponse) -> Vec<(String, String, String)> {
        fn walk(folder: &TreeFolder, out: &mut Vec<(String, String, String)>) {
            for asset in &folder.assets {
                out.push((
                    folder.path.clone(),
                    asset.name.clone(),
                    asset.asset_id.clone(),
                ));
            }
            for child in &folder.children {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        for folder in &response.folders {
            walk(folder, &mut out);
        }
        out
    }

    fn assets_of(response: &TreeResponse) -> Vec<Asset> {
        fn walk(folder: &TreeFolder, out: &mut Vec<Asset>) {
            out.extend(folder.assets.iter().cloned());
            for child in &folder.children {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        for folder in &response.folders {
            walk(folder, &mut out);
        }
        out
    }

    fn asset_named(response: &TreeResponse, asset_id: &str) -> Asset {
        assets_of(response)
            .into_iter()
            .find(|asset| asset.asset_id == asset_id)
            .unwrap_or_else(|| panic!("no asset {asset_id} in {:?}", rows(response)))
    }

    fn folder_at<'a>(response: &'a TreeResponse, path: &str) -> &'a TreeFolder {
        fn walk<'a>(folder: &'a TreeFolder, path: &str) -> Option<&'a TreeFolder> {
            if folder.path == path {
                return Some(folder);
            }
            folder.children.iter().find_map(|child| walk(child, path))
        }
        response
            .folders
            .iter()
            .find_map(|folder| walk(folder, path))
            .unwrap_or_else(|| panic!("no folder {path}"))
    }

    #[test]
    fn projected_media_names_and_mime_hints_are_narrow() {
        for (name, mime, expected) in [
            ("clip.mp4", "", Format::Mp4),
            ("download.bin", "audio/mpeg", Format::Mp3),
            ("download.bin", "audio/x-wav", Format::Wav),
            ("download.bin", "video/webm; codecs=vp9", Format::Webm),
        ] {
            assert_eq!(recognise_name(name, 0, Some(mime)), (Kind::Other, expected));
        }
        assert_eq!(
            recognise_name("review.pdf", 0, Some("video/mp4")),
            (Kind::Doc, Format::Pdf)
        );
        assert_eq!(
            recognise_name("download.bin", 0, Some("video/unknown")),
            (Kind::Other, Format::Unknown)
        );
    }

    #[test]
    fn map_printouts_keep_canonical_identity_and_mv_ownership() {
        let response = build(&tree_request(json!({"sources": {"local_prints": [
            {"locator":"old-path", "filename":"huye.a3-landscape.pdf", "family":"custom-map", "map_id":"huye", "format":"pdf", "size_bytes":200},
            {"locator":"mv-path", "filename":"mv_data.a3-landscape.pdf", "family":"mv-map", "format":"pdf"}
        ]}}))).unwrap();
        assert!(rows(&response).contains(&(
            "Prints/custom maps".into(),
            "huye.a3-landscape.pdf".into(),
            "sys:local_print:map:huye.a3-landscape.pdf".into()
        )));
        assert!(rows(&response)
            .iter()
            .any(|(folder, _, _)| folder == "Reports/MV"));
        let asset = asset_named(&response, "sys:local_print:map:huye.a3-landscape.pdf");
        assert_eq!(asset.bytes, 200);
        assert_eq!(asset.origin.unwrap().r#ref["attachment_type"], "printout");
    }

    #[test]
    fn published_maps_are_projected_from_existing_report_channel() {
        let response = build(&tree_request(json!({"sources": {"transformers": [
            {"name":"combined_transformer", "print_files":["gs://reports/huye.a0-landscape.pdf"], "report_metadata":{"print_urls":[
                {"gcs_path":"gs://reports/huye.a0-landscape.pdf", "map_family":"custom-map", "size_bytes":500, "sha256":"abc"}
            ]}},
            {"name":"mv_data", "print_files":["gs://reports/mv_data.a3-landscape.pdf"], "report_metadata":{"print_urls":[
                {"gcs_path":"gs://reports/mv_data.a3-landscape.pdf", "map_family":"mv-map", "size_bytes":15000000, "sha256":"abc"}
            ]}}
        ]}}))).unwrap();
        assert!(rows(&response)
            .iter()
            .any(|(folder, name, _)| folder == "Prints/custom maps"
                && name == "huye.a0-landscape.pdf"));
        assert!(rows(&response)
            .iter()
            .any(|(folder, name, _)| folder == "Reports/MV" && name == "mv_data.a3-landscape.pdf"));
        let mv = asset_named(
            &response,
            "sys:report_file:mv_data:mv_data.a3-landscape.pdf",
        );
        assert_eq!(mv.bytes, 15000000);
        assert_eq!(mv.origin.unwrap().r#ref["attachment_type"], "printout");
        assert_eq!(rows(&response).len(), 2);
    }

    /// One row per source key of §2.5 — all nineteen.
    fn every_source() -> Value {
        json!({
            "transformers": [{
                "name": "TX-1",
                "metadata": {"updated_at": "2026-03-01T00:00:00Z", "updated_by": "lead@ds.rw"},
                "uploads": {"draft": [{
                    "filename": "survey.csv", "blob_path": "uploads/TX-1/survey.csv",
                    "uploaded_at": "2026-02-01T00:00:00Z", "uploaded_by": "field@ds.rw"
                }]},
                "report_files": ["gs://reports/TX-1/report.xlsx"],
                "report_artifacts": [{
                    "url": "https://storage.example/reports/TX-1-map.pdf?X-Goog-Signature=secret",
                    "output_id": "map", "format": "pdf", "size_bytes": 2048
                }]
            }],
            "transformer_versions": [{
                "transformer_id": "TX-1", "version_id": "ver_9", "version": 3,
                "reason": "as-built", "created_at": "2026-03-02T00:00:00Z",
                "created_by": "lead@ds.rw", "playback_available": true, "room_bytes": 4096
            }],
            "design_attachments": [{
                "attachment": {
                    "attachment_id": "att_1",
                    "object": {"kind": "lv_transformer", "id": "TX-1"},
                    "object_key": "lv_transformer:TX-1",
                    "label": "permits", "file_name": "permit.pdf",
                    "latest_revision_id": "rev_2", "latest_revision": 2,
                    "created_by": "legal@ds.rw"
                },
                "revisions": [
                    {"revision_id": "rev_1", "revision": 1, "file_name": "permit.pdf", "byte_size": 10},
                    {"revision_id": "rev_2", "revision": 2, "file_name": "permit.pdf", "byte_size": 20}
                ]
            }],
            "grid_models": [{
                "model_id": "m_1", "display_name": "Feeder A", "head_revision_id": "r_2",
                "revisions": [{"revision_id": "r_1"}, {"revision_id": "r_2"}]
            }],
            "grid_exports": [{
                "model_id": "m_1", "export_id": "exp_1",
                "items": [{"output_id": "out_1", "format": "gpkg"}]
            }],
            "pm_tasks": [{"id": "t_1", "wbs": "1.2.3", "title": "Survey"}],
            "pm_records": [{"id": "rec_1", "referenceNumber": "RFI-004", "subject": "Access"}],
            "pm_attachments": [
                {
                    "id": "pa_1", "linkedEntityType": "task", "linkedEntityId": "t_1",
                    "originalName": "note.docx", "sizeBytes": 12, "documentState": "issued",
                    "metadata": {"createdBy": "pm@ds.rw", "createdAt": 1772323200000_u64}
                },
                {
                    "id": "pa_2", "linkedEntityType": "record", "linkedEntityId": "rec_1",
                    "originalName": "reply.pdf", "documentState": "superseded", "metadata": {}
                }
            ],
            "compounded": [{
                "stem": "lot3", "filename": "lot3.zip", "gcs_path": "gs://reports/lot3.zip",
                "created_at": "2026-03-03T00:00:00Z", "created_by": "lead@ds.rw"
            }],
            "survey_media": [{
                "form": "poles", "entry_id": "e_1", "field": "photo", "name": "pole.jpg",
                "path": "survey/poles/e_1/pole.jpg", "content_type": "image/jpeg",
                "captured_at": "2026-01-09T07:00:00Z", "created_by": "field@ds.rw"
            }],
            "project_data": [{
                "upload_id": "u_1", "file_name": "boundaries.geojson",
                "created_at": "2026-01-10T00:00:00Z", "uploaded_by": "gis@ds.rw",
                "blob_path": "project_data/boundaries.geojson"
            }],
            "user_data": [{
                "upload_id": "u_2", "file_name": "mine.csv", "created_at": "2026-01-11T00:00:00Z",
                "user_email": "me@ds.rw", "blob_path": "user_data/mine.csv"
            }],
            "compute_artifacts": [{
                "work_id": "w_1", "engine": "solar", "operation": "report", "variant": "en",
                "updated_at": "2026-03-04T00:00:00Z",
                "outputs": [{"output_id": "o_1", "format": "pdf", "size_bytes": 5}]
            }],
            "solar": [{
                "city": "Kigali",
                "head": {
                    "run_id": "run_1", "report_input_bytes": 9,
                    "report_input_sha256": "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
                    "published_at": "2026-03-05T00:00:00Z"
                },
                "artifacts": [{
                    "name": "chart.png", "gcs_path": "gs://solar/Kigali/chart.png",
                    "size_bytes": 3, "content_type": "image/png"
                }]
            }],
            "dataset_rooms": [{
                "dataset_id": "buildings", "rows": 10, "bytes": 20, "cells": 30,
                "freshness": "fresh", "expires_at": "2026-04-01T00:00:00Z", "pending_queries": 0
            }],
            "national_datasets": [{
                "dataset_id": "rwanda_admin", "status": "ready", "bytes": 40, "ready": true
            }],
            "report_rooms": [{
                "transformer": "TX-1", "scope": "report", "freshness": "fresh",
                "dirty": false, "bytes": 50
            }],
            "local_prints": [{
                "locator": "print_1", "filename": "map.pdf", "format": "pdf",
                "paper_size": "A1", "size_bytes": 60, "transformer": "TX-1", "output_id": "out"
            }],
            "print_setups": [{
                "id": "layout_1", "name": "A1 landscape", "scope": "project",
                "updated_at": "2026-03-06T00:00:00Z", "updated_by": "lead@ds.rw"
            }]
        })
    }

    #[test]
    fn identical_printout_copies_share_preview_but_pending_revisions_remain_distinct() {
        let digest = "a".repeat(64);
        let mut inputs = json!({"sources": {
            "local_prints": [{"locator":"local-map", "filename":"mv_data.a3-landscape.pdf", "family":"mv-map", "format":"pdf", "size_bytes":15000, "sha256":digest}],
            "transformers": [{"name":"mv_data", "print_files":["gs://reports/mv_data.a3-landscape.pdf"], "report_metadata":{"print_urls":[{"gcs_path":"gs://reports/mv_data.a3-landscape.pdf", "map_family":"mv-map", "size_bytes":15000, "sha256":digest}]}}]
        }});
        let response = build(&tree_request(inputs.clone())).unwrap();
        let maps: Vec<_> = assets_of(&response)
            .into_iter()
            .filter(|a| a.name == "mv_data.a3-landscape.pdf")
            .collect();
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].origin.as_ref().unwrap().source, "local_prints");
        assert!(maps[0].origin.as_ref().unwrap().r#ref["published_path"].is_string());
        inputs["sources"]["local_prints"][0]["sha256"] = json!("b".repeat(64));
        let response = build(&tree_request(inputs)).unwrap();
        assert_eq!(
            assets_of(&response)
                .iter()
                .filter(|a| a.name == "mv_data.a3-landscape.pdf")
                .count(),
            2
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_every_source_projects() {
        let request = tree_request(json!({ "sources": every_source() }));
        let response = build(&request).expect("tree");
        let placed = rows(&response);
        let expected = [
            (
                "Transformers/TX-1/design/current",
                "current",
                "sys:transformer_design:TX-1",
            ),
            (
                "Transformers/TX-1/uploads/draft",
                "survey.csv",
                "sys:upload:TX-1:uploads/TX-1/survey.csv",
            ),
            (
                "Transformers/TX-1/reports",
                "report.xlsx",
                "sys:report_file:TX-1:report.xlsx",
            ),
            (
                "Transformers/TX-1/reports",
                "TX-1-map.pdf",
                "sys:report_file:TX-1:TX-1-map.pdf",
            ),
            (
                "Transformers/TX-1/versions",
                "v3",
                "sys:transformer_version:TX-1:ver_9",
            ),
            (
                "Transformers/TX-1/attachments/permits",
                "r1.pdf",
                "sys:design_attachment:att_1:rev_1",
            ),
            (
                "Transformers/TX-1/attachments/permits",
                "r2.pdf",
                "sys:design_attachment:att_1:rev_2",
            ),
            (
                "MV models/Feeder A/revisions",
                "r_1.dsgrid",
                "sys:grid_revision:m_1:r_1",
            ),
            (
                "MV models/Feeder A/revisions",
                "r_2.dsgrid",
                "sys:grid_revision:m_1:r_2",
            ),
            (
                "MV models/Feeder A/exports/exp_1",
                "out_1.gpkg",
                "sys:grid_export:m_1:exp_1:out_1",
            ),
            (
                "Project work/tasks/1.2.3",
                "note.docx",
                "sys:pm_attachment:pa_1",
            ),
            (
                "Project work/records/RFI-004",
                "reply.pdf",
                "sys:pm_attachment:pa_2",
            ),
            ("Reports/combined", "lot3.zip", "sys:compounded:lot3"),
            (
                "Reports/compute/w_1",
                "o_1.pdf",
                "sys:compute_artifact:w_1:o_1",
            ),
            (
                "Survey/poles/e_1",
                "pole.jpg",
                "sys:survey_media:poles:e_1:photo",
            ),
            ("Project data", "boundaries.geojson", "sys:project_data:u_1"),
            ("My data", "mine.csv", "sys:user_data:u_2"),
            (
                "Solar/Kigali/results",
                "report_input.json",
                "sys:solar_result:Kigali",
            ),
            (
                "Solar/Kigali/artifacts",
                "chart.png",
                "sys:solar_artifact:Kigali:chart.png",
            ),
            (
                "Local data/rooms",
                "buildings",
                "sys:dataset_room:buildings",
            ),
            (
                "Local data/national",
                "rwanda_admin",
                "sys:national_dataset:rwanda_admin",
            ),
            ("Local data/report rooms", "TX-1", "sys:report_room:TX-1"),
            ("Prints/local", "map.pdf", "sys:local_print:print_1"),
            ("Prints/setups", "A1 landscape", "sys:print_setup:layout_1"),
        ];
        for (path, name, asset_id) in expected {
            assert!(
                placed.contains(&(path.to_string(), name.to_string(), asset_id.to_string())),
                "{path}/{name} ({asset_id}) is missing from {placed:#?}"
            );
        }
        assert_eq!(placed.len(), expected.len(), "{placed:#?}");
        // Nothing landed in Unclassified, and every root of §2.5 is present.
        assert!(!placed
            .iter()
            .any(|(path, _, _)| path.starts_with("Unclassified")));
        for root in SYSTEM_ROOTS {
            if root != "Unclassified" {
                assert_eq!(folder_at(&response, root).kind, FolderKind::System);
            }
        }
        // A signed URL never becomes an origin reference (§12.11).
        let artifact = asset_named(&response, "sys:report_file:TX-1:TX-1-map.pdf");
        let origin = artifact.origin.expect("origin").r#ref.to_string();
        assert!(!origin.contains("X-Goog-Signature"), "{origin}");

        // The same answer through the request grammar.
        let wire = serde_json::to_vec(&json!({
            "schema": REQUEST_SCHEMA, "action": "tree", "sources": every_source()
        }))
        .expect("json");
        let answered: Value =
            serde_json::from_str(&evaluate(&wire).expect("tree")).expect("json answer");
        assert_eq!(answered["action"], json!("tree"));
        assert_eq!(answered["schema"], json!(TREE_SCHEMA));
        assert_eq!(answered["total_assets"], json!(expected.len()));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_status_rules() {
        let request = tree_request(json!({ "sources": every_source() }));
        let response = build(&request).expect("tree");
        let status = |asset_id: &str| asset_named(&response, asset_id).status;
        // §2.5, every rule it states.
        assert_eq!(status("sys:design_attachment:att_1:rev_2"), Status::Durable);
        assert_eq!(status("sys:design_attachment:att_1:rev_1"), Status::Archive);
        assert_eq!(status("sys:pm_attachment:pa_1"), Status::Durable);
        assert_eq!(status("sys:pm_attachment:pa_2"), Status::Archive);
        assert_eq!(status("sys:report_file:TX-1:TX-1-map.pdf"), Status::Durable);
        assert_eq!(status("sys:report_file:TX-1:report.xlsx"), Status::Durable);
        assert_eq!(status("sys:compounded:lot3"), Status::Durable);
        assert_eq!(status("sys:local_print:print_1"), Status::Durable);
        assert_eq!(status("sys:grid_revision:m_1:r_2"), Status::Durable);
        assert_eq!(status("sys:grid_revision:m_1:r_1"), Status::Archive);
        assert_eq!(
            status("sys:upload:TX-1:uploads/TX-1/survey.csv"),
            Status::Fresh
        );
        assert_eq!(status("sys:transformer_design:TX-1"), Status::Fresh);
        assert_eq!(
            status("sys:transformer_version:TX-1:ver_9"),
            Status::Durable
        );
        assert_eq!(status("sys:survey_media:poles:e_1:photo"), Status::Durable);
        assert_eq!(status("sys:grid_export:m_1:exp_1:out_1"), Status::Durable);
        assert_eq!(status("sys:compute_artifact:w_1:o_1"), Status::Durable);
        assert_eq!(status("sys:solar_result:Kigali"), Status::Durable);
        assert_eq!(
            status("sys:solar_artifact:Kigali:chart.png"),
            Status::Durable
        );
        assert_eq!(status("sys:project_data:u_1"), Status::Fresh);
        assert_eq!(status("sys:user_data:u_2"), Status::Fresh);
        // A PM attachment that is neither issued/approved/final nor superseded.
        let drafting = tree_request(json!({ "sources": {
            "pm_tasks": [{"id": "t_1", "wbs": "1.2.3"}],
            "pm_attachments": [{
                "id": "pa_3", "linkedEntityType": "task", "linkedEntityId": "t_1",
                "originalName": "draft.docx", "documentState": "draft"
            }]
        }}));
        let drafts = build(&drafting).expect("tree");
        assert_eq!(
            asset_named(&drafts, "sys:pm_attachment:pa_3").status,
            Status::Fresh
        );
        // Every projected row is internal and kernel-classified (D4).
        for asset in assets_of(&response) {
            assert_eq!(
                asset.sensitivity,
                Sensitivity::Internal,
                "{}",
                asset.asset_id
            );
            assert_eq!(
                asset.classified_by,
                ClassifiedBy::Kernel,
                "{}",
                asset.asset_id
            );
            assert!(asset.origin.is_some(), "{}", asset.asset_id);
        }
        // Owner and dates come from the source row.
        let upload = asset_named(&response, "sys:upload:TX-1:uploads/TX-1/survey.csv");
        assert_eq!(upload.owner.expect("owner").id, "field@ds.rw");
        assert_eq!(
            upload.dated.created_at.as_deref(),
            Some("2026-02-01T00:00:00Z")
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_versions_are_viewable_geo_assets() {
        let request = tree_request(json!({ "sources": every_source() }));
        let response = build(&request).expect("tree");
        let version = asset_named(&response, "sys:transformer_version:TX-1:ver_9");
        assert_eq!(version.kind, Kind::Geo);
        assert_eq!(version.format, Format::Geojson);
        assert_eq!(version.status, Status::Durable);
        assert_eq!(version.name, "v3");
        assert_eq!(version.bytes, 4096);
        let origin = version.origin.expect("origin");
        assert_eq!(origin.source, "transformer_versions");
        assert_eq!(origin.r#ref["version_id"], json!("ver_9"));
        assert_eq!(origin.r#ref["playback_available"], json!(true));
        assert!(version.links.contains(&Link::DsObject {
            object_type: "transformer".to_string(),
            entity_id: "TX-1".to_string(),
        }));
        // The live design is a viewable geo asset too, and it is fresh.
        let design = asset_named(&response, "sys:transformer_design:TX-1");
        assert_eq!((design.kind, design.format), (Kind::Geo, Format::Geojson));
        assert_eq!(design.status, Status::Fresh);
        // Versions are their own folder under the transformer.
        assert_eq!(
            folder_at(&response, "Transformers/TX-1/versions")
                .counts
                .assets,
            1
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_reserved_transformers_filtered() {
        // The closed list, pinned by name against ds-brain's
        // IsReservedTransformerDocName / CanonicalTransformerReferenceName.
        assert_eq!(
            RESERVED_TRANSFORMER_NAMES,
            [
                "all_transformers",
                "collisions",
                "combined_transformer",
                "combined_transformers",
                "mv_data"
            ]
        );
        for reserved in [
            "mv_data",
            "MV Data",
            "combined_transformer",
            "all_transformers",
            "combined_transformers",
            "collisions",
            "collisions_2",
            "collisions_17",
            "  ",
            "",
        ] {
            assert!(is_reserved_transformer_name(reserved), "{reserved}");
        }
        for ordinary in [
            "TX-1",
            "collisions_lv_lines",
            "collisions_service_cables",
            "mv_data_2",
        ] {
            assert!(!is_reserved_transformer_name(ordinary), "{ordinary}");
        }
        assert_eq!(canonical_transformer_reference_name("TX 1/A"), "tx_1_a");
        assert_eq!(canonical_transformer_reference_name("9-lines"), "_9_lines");
        assert_eq!(canonical_transformer_reference_name("###"), "transformer");

        let request = tree_request(json!({ "sources": {
            "transformers": [
                {"name": "TX-1"},
                {"name": "mv_data", "report_files": ["gs://x/mv_data/rows.csv"]},
                {"name": "collisions_2"},
                {"name": "combined_transformer",
                 "report_files": ["gs://reports/combined/lot3-combined.xlsx"]}
            ],
            "transformer_versions": [
                {"transformer_id": "mv_data", "version_id": "v1", "version": 1}
            ],
            "design_attachments": [
                {"attachment_id": "att_x", "object": {"id": "combined_transformer"},
                 "label": "sheet", "file_name": "x.pdf", "latest_revision_id": "r1"}
            ]
        }}));
        let response = build(&request).expect("tree");
        let placed = rows(&response);
        let paths: BTreeSet<&str> = placed.iter().map(|(path, _, _)| path.as_str()).collect();
        assert!(paths.contains("Transformers/TX-1/design/current"));
        for reserved in ["mv_data", "collisions_2", "combined_transformer"] {
            assert!(
                !paths
                    .iter()
                    .any(|path| path.starts_with(&format!("Transformers/{reserved}"))),
                "{reserved} became a folder: {paths:?}"
            );
        }
        // The combined aggregate's artifacts land under Reports/combined (§2.5).
        assert!(placed.contains(&(
            "Reports/combined".to_string(),
            "lot3-combined.xlsx".to_string(),
            "sys:report_file:combined_transformer:lot3-combined.xlsx".to_string()
        )));
        // Another reserved document's files are kept, never dropped.
        assert!(placed
            .iter()
            .any(|(path, name, _)| path == "Unclassified/transformers" && name == "rows.csv"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_projection_is_deterministic() {
        let sources = every_source();
        let folders = json!([
            {"folder_id": "f_1", "project_id": "p", "path": "contracts/2026", "name": "2026"},
            {"folder_id": "f_2", "project_id": "p", "path": "contracts", "name": "contracts"}
        ]);
        let assets = json!([
            {"asset_id": "a_2", "project_id": "p", "folder": "contracts/2026",
             "name": "epc.pdf", "bytes": 10, "source": {"kind": "upload"}},
            {"asset_id": "a_1", "project_id": "p", "folder": "contracts/2026",
             "name": "epc.pdf", "bytes": 11, "source": {"kind": "upload"}}
        ]);
        let request = tree_request(json!({
            "sources": sources.clone(), "folders": folders.clone(), "assets": assets.clone()
        }));
        let once = serde_json::to_string(&build(&request).expect("tree")).expect("json");
        let twice = serde_json::to_string(&build(&request).expect("tree")).expect("json");
        assert_eq!(once, twice, "the same request answered differently twice");

        // The same rows in a different order answer identically.
        let mut shuffled = match sources {
            Value::Object(map) => map,
            other => panic!("{other}"),
        };
        for (_, rows) in shuffled.iter_mut() {
            if let Some(list) = rows.as_array_mut() {
                list.reverse();
                list.push(list.first().cloned().unwrap_or(Value::Null));
                list.pop();
            }
        }
        let reversed_folders = match folders {
            Value::Array(mut list) => {
                list.reverse();
                Value::Array(list)
            }
            other => other,
        };
        let reversed_assets = match assets {
            Value::Array(mut list) => {
                list.reverse();
                Value::Array(list)
            }
            other => other,
        };
        let shuffled = tree_request(json!({
            "sources": Value::Object(shuffled),
            "folders": reversed_folders,
            "assets": reversed_assets
        }));
        let out = serde_json::to_string(&build(&shuffled).expect("tree")).expect("json");
        assert_eq!(once, out, "row order changed the answer");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_query_filters() {
        let base = json!({
            "sources": every_source(),
            "folders": [{"folder_id": "f_1", "project_id": "p", "path": "contracts",
                         "name": "contracts"}],
            "assets": [{"asset_id": "a_1", "project_id": "p", "folder": "contracts",
                        "name": "EPC-Lot3-signed.pdf", "bytes": 10,
                        "source": {"kind": "upload"},
                        "owner": {"kind": "user", "id": "commercial@ds.rw", "display": "Aline"}}]
        });
        let with = |extra: Value| {
            let mut body = match base.clone() {
                Value::Object(map) => map,
                other => panic!("{other}"),
            };
            if let Value::Object(extra) = extra {
                for (key, value) in extra {
                    body.insert(key, value);
                }
            }
            tree_request(Value::Object(body))
        };

        // Substring of the name, case-insensitively.
        let response = build(&with(json!({"query": "PERMIT"}))).expect("tree");
        let names: Vec<String> = rows(&response)
            .into_iter()
            .map(|(_, name, _)| name)
            .collect();
        assert_eq!(names, ["r1.pdf", "r2.pdf"]);
        // Folders with nothing left are not returned at all.
        assert!(!response.folders.iter().any(|folder| folder.path == "Solar"));

        // Substring of the folder path.
        let response = build(&with(json!({"query": "solar/kigali"}))).expect("tree");
        assert_eq!(rows(&response).len(), 2);

        // Status, kind and owner display are matched too (§2.6).
        // rev_1, grid r_1 and the superseded PM attachment.
        assert_eq!(
            build(&with(json!({"query": "archive"})))
                .expect("tree")
                .total_assets,
            3
        );
        assert_eq!(
            build(&with(json!({"query": "aline"})))
                .expect("tree")
                .total_assets,
            1
        );
        // The two live geo rows, the gpkg export (geo by its name, though the
        // source reports no size) and the upload whose name carries the word.
        assert_eq!(
            build(&with(json!({"query": "geo"})))
                .expect("tree")
                .total_assets,
            4
        );

        // The link filter of the Project work mount.
        let by_task =
            build(&with(json!({"link": {"kind": "pm_task", "id": "t_1"}}))).expect("tree");
        assert_eq!(
            rows(&by_task),
            [(
                "Project work/tasks/1.2.3".to_string(),
                "note.docx".to_string(),
                "sys:pm_attachment:pa_1".to_string()
            )]
        );
        let by_object = build(&with(json!({
            "link": {"kind": "ds_object", "object_type": "transformer", "entity_id": "TX-1"}
        })))
        .expect("tree");
        assert!(by_object.total_assets >= 6, "{}", by_object.total_assets);
        assert!(rows(&by_object)
            .iter()
            .all(|(path, _, _)| path.starts_with("Transformers")
                || path.starts_with("Local data")
                || path.starts_with("Prints")));

        // folder roots the answer; depth cuts it and says so; kind keeps one side.
        let rooted = build(&with(json!({"folder": "Transformers/TX-1"}))).expect("tree");
        assert_eq!(rooted.folders.len(), 1);
        assert_eq!(rooted.folders[0].path, "Transformers/TX-1");
        assert_eq!(rooted.folders[0].name, "TX-1");
        assert!(build(&with(json!({"folder": "nowhere"})))
            .expect("tree")
            .folders
            .is_empty());

        let deep = build(&with(json!({"folder": "Transformers/TX-1/versions"}))).expect("tree");
        assert_eq!(deep.folders.len(), 1);
        assert_eq!(deep.folders[0].path, "Transformers/TX-1/versions");
        assert_eq!(deep.total_assets, 1);

        let none = build(&with(json!({"depth": 0}))).expect("tree");
        assert!(none.folders.is_empty());
        assert!(none.truncated);

        let shallow = build(&with(json!({"depth": 1}))).expect("tree");
        assert!(shallow.truncated);
        assert!(shallow
            .folders
            .iter()
            .all(|folder| folder.children.is_empty()));
        assert_eq!(shallow.total_folders, shallow.folders.len() as u64);

        let user_only = build(&with(json!({"kind": "user"}))).expect("tree");
        assert_eq!(
            user_only
                .folders
                .iter()
                .map(|folder| folder.path.as_str())
                .collect::<Vec<_>>(),
            ["contracts"]
        );
        assert_eq!(user_only.folders[0].counts.assets, 1);
        assert_eq!(user_only.total_assets, 1);

        // Counts are computed after filtering.
        let counted = build(&with(json!({"query": "permit"}))).expect("tree");
        assert_eq!(
            folder_at(&counted, "Transformers/TX-1/attachments/permits").counts,
            Counts {
                assets: 2,
                folders: 0
            }
        );
        assert_eq!(folder_at(&counted, "Transformers/TX-1").counts.folders, 1);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_user_folder_collision_refused() {
        for path in [
            "Reports",
            "reports/2026",
            "Local data/mine",
            "unclassified/x",
        ] {
            let request = tree_request(json!({"folders": [{
                "folder_id": "f_1", "project_id": "p", "path": path, "name": "x"
            }]}));
            let refusal = build(&request).expect_err("collision must be refused");
            assert!(refusal.contains(path), "{refusal}");
            assert!(refusal.contains("system root"), "{refusal}");
        }
        // An asset filed under a system root is refused the same way.
        let request = tree_request(json!({"assets": [{
            "asset_id": "a_1", "project_id": "p", "folder": "Transformers/TX-1",
            "name": "sneaky.pdf", "bytes": 1, "source": {"kind": "upload"}
        }]}));
        let refusal = build(&request).expect_err("collision must be refused");
        assert!(refusal.contains("sneaky.pdf"), "{refusal}");
        assert!(refusal.contains("Transformers"), "{refusal}");
        // A user folder that does not collide is merged beside the system roots.
        let request = tree_request(json!({"folders": [{
            "folder_id": "f_1", "project_id": "p", "path": "contracts/2026/epc", "name": "epc",
            "default_sensitivity": "confidential"
        }]}));
        let response = build(&request).expect("tree");
        assert_eq!(folder_at(&response, "contracts").kind, FolderKind::User);
        assert_eq!(folder_at(&response, "contracts/2026/epc").name, "epc");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_absent_source_is_not_loaded_not_empty() {
        let request = tree_request(json!({"sources": {
            "transformers": [{"name": "TX-1"}],
            "survey_media": []
        }}));
        let response = build(&request).expect("tree");
        // Every root is still rendered — an absent source is not an empty root.
        for root in SYSTEM_ROOTS {
            if root != "Unclassified" {
                assert_eq!(folder_at(&response, root).path, root);
            }
        }
        let value = build_value(&request).expect("tree value");
        let root = |path: &str| {
            value["folders"]
                .as_array()
                .expect("folders")
                .iter()
                .find(|folder| folder["path"] == json!(path))
                .cloned()
                .unwrap_or_else(|| panic!("no root {path}"))
        };
        // Loaded and empty is not "not loaded".
        assert_eq!(root("Survey").get("not_loaded"), None);
        assert_eq!(root("Survey").get("not_loaded_sources"), None);
        // Nothing loaded for these roots at all.
        assert_eq!(root("Solar")["not_loaded"], json!(true));
        assert_eq!(root("Solar")["not_loaded_sources"], json!(["solar"]));
        assert_eq!(root("Prints")["not_loaded"], json!(true));
        // Partly loaded: the root is real, the missing sources are named.
        assert_eq!(root("Transformers").get("not_loaded"), None);
        assert_eq!(
            root("Transformers")["not_loaded_sources"],
            json!(["design_attachments", "transformer_versions"])
        );
        assert_eq!(
            value["not_loaded_sources"],
            json!([
                "compounded",
                "compute_artifacts",
                "dataset_rooms",
                "design_attachments",
                "grid_exports",
                "grid_models",
                "local_prints",
                "national_datasets",
                "pm_attachments",
                "pm_parties",
                "pm_records",
                "pm_tasks",
                "print_setups",
                "project_data",
                "report_rooms",
                "solar",
                "transformer_versions",
                "user_data"
            ])
        );
        assert_eq!(
            not_loaded_sources(&tree_request(json!({}))).len(),
            TREE_SOURCE_NAMES.len()
        );
    }

    /// Owner ruling 2026-09-20: everything about a thread is under
    /// `Assets › Correspondence`. Two records of one thread (raw ds-brain
    /// shape) and one in another (browser camelCase shape); the catalogue
    /// holds the .eml with its parts, a registered document, an external
    /// reference linked `pm_record`, and one asset the record names that the
    /// caller may not read.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_indexes_every_thread_under_correspondence() {
        let row = |id: &str, name: &str, kind: &str, extra: Value| {
            let mut asset = json!({
                "schema": "ds.assets.asset/v1", "asset_id": id, "project_id": "p", "folder": "correspondence/2026-09",
                "name": name, "kind": kind, "format": "unknown", "bytes": 10, "digest": "sha256:ab", "status": "fresh",
                "sensitivity": "internal", "dated": {}, "owner": {"kind": "user", "id": "u"},
                "source": {"kind": "upload"}, "classified_by": "user", "links": [],
                "created_by": "u", "updated_at": "2026-09-20T00:00:00Z", "updated_by": "u", "version": 1,
            });
            for (key, value) in extra.as_object().expect("extra") {
                asset[key] = value.clone();
            }
            asset
        };
        let request = tree_request(json!({
            "sources": {
                "pm_parties": [
                    {"id": "p_acme", "data": {"name": "Acme Consultancy", "kind": "organisation"}},
                    {"id": "p_zed", "name": "Zed Ltd"},
                ],
                "pm_records": [
                    {"id": "R1", "data": {"subject": "Review of the MV design", "thread_id": "R1", "happened_at": "2026-09-14T09:00:00Z",
                     "external_party_ids": ["p_acme", "p_zed"], "source_asset_id": "a_mail", "document_ids": ["a_doc", "a_hidden"]}},
                    {"id": "R2", "data": {"subject": "Re: Review", "thread_id": "R1", "happened_at": "2026-09-18T09:00:00Z",
                     "external_party_ids": ["p_zed"], "source_asset_id": "a_mail"}},
                    {"id": "R3", "subject": "Site note", "threadId": "R3", "happenedAt": "2026-09-19T09:00:00Z",
                     "externalParticipantNames": ["nobody"]},
                    {"id": "R4", "subject": "Letter", "threadId": "R4", "happenedAt": "2026-09-19T09:00:00Z", "sourceAssetId": "a_gone"},
                ],
            },
            "assets": [
                row("a_mail", "review.eml", "mail", json!({"format": "eml", "mail": {
                    "from": "x@example.com", "to": ["y@example.com"], "cc": [], "subject": "Review", "message_id": "<m@example.com>",
                    "parts": [
                        {"path": "part-1.txt", "section": "1", "content_type": "text/plain", "disposition": "body", "bytes": 12, "kind": "doc", "format": "txt", "named": false},
                        {"path": "drawing.pdf", "section": "2", "content_type": "application/pdf", "disposition": "attachment", "bytes": 3000, "kind": "doc", "format": "pdf", "named": true},
                    ], "parts_total": 2, "truncated": false}})),
                row("a_doc", "GTP-001.pdf", "doc", json!({"format": "pdf", "document": {"number": "GTP-001", "revision_label": "B", "state": "issued"}})),
                row("a_ref", "package.zip", "pack", json!({"source": {"kind": "external_reference"}, "bytes_held": false,
                    "external": {"url": "https://drive.google.com/file/d/abc", "provider": "google_drive"},
                    "links": [{"kind": "pm_record", "id": "R2"}]})),
                row("a_note", "note.md", "note", json!({"links": [{"kind": "pm_record", "id": "R3"}]})),
            ],
        }));
        let response = build(&request).expect("tree");
        let listing = rows(&response);
        let thread =
            "Correspondence/Acme Consultancy + Zed Ltd — Review of the MV design (2026-09-14)";
        let under = |folder: &str| -> Vec<&str> {
            listing
                .iter()
                .filter(|(path, _, _)| path == folder)
                .map(|(_, _, id)| id.as_str())
                .collect()
        };
        // Rows sit in name order, as everywhere in the tree.
        assert_eq!(
            under(thread),
            vec!["a_doc", "a_ref", "a_mail"],
            "the source mail, the registered document and the linked reference — the hidden document has no row and no placeholder"
        );
        let parts = under(&format!("{thread}/review.eml"));
        assert_eq!(
            parts,
            vec![
                "sys:mail_part:a_mail:drawing.pdf",
                "sys:mail_part:a_mail:part-1.txt"
            ]
        );
        let pdf = asset_named(&response, "sys:mail_part:a_mail:drawing.pdf");
        assert_eq!(pdf.format, Format::Pdf);
        assert_eq!(pdf.bytes, 3000);
        assert_eq!(pdf.container.as_deref(), Some("a_mail"));
        assert_eq!(
            pdf.origin.as_ref().expect("origin").r#ref["member"],
            "drawing.pdf"
        );
        let mail = assets_of(&response)
            .into_iter()
            .find(|asset| asset.asset_id == "a_mail" && asset.folder == thread)
            .expect("the indexed listing of the mail");
        assert_eq!(
            mail.origin.as_ref().expect("origin").r#ref["role"],
            "source"
        );
        assert_eq!(
            mail.origin.as_ref().expect("origin").r#ref["thread_id"],
            "R1"
        );
        // The catalogued row keeps its declared folder too: the index is a
        // second path to the same asset, never a move.
        assert!(
            listing.iter().any(|(path, _, id)| path == "Correspondence/2026-09" && id == "a_mail"),
            "a declared `correspondence/…` folder shares the root, respelled, rather than refusing the tree"
        );
        // R3 carries a linked note; R4 names an asset the caller cannot see,
        // so it has no folder at all.
        assert_eq!(
            under("Correspondence/Site note (2026-09-19)"),
            vec!["a_note"]
        );
        assert!(
            !listing
                .iter()
                .any(|(path, _, _)| path.starts_with("Correspondence/Letter")),
            "a thread with nothing readable has no folder"
        );
        assert_eq!(
            folder_at(&response, "Correspondence").kind,
            FolderKind::System
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_unknown_source_refused() {
        let request = tree_request(json!({"sources": {"transformer_uploads": []}}));
        let refusal = build(&request).expect_err("an unknown source must be refused");
        assert!(refusal.contains("transformer_uploads"), "{refusal}");
        assert!(
            refusal.contains("transformers, transformer_versions"),
            "{refusal}"
        );
        // Through the request grammar as well.
        let wire = serde_json::to_vec(&json!({
            "schema": REQUEST_SCHEMA, "action": "tree", "sources": {"nope": []}
        }))
        .expect("json");
        assert!(evaluate(&wire).expect_err("refusal").contains("nope"));
        // The row bound is a refusal that names the number.
        let request = tree_request(json!({
            "sources": {"pm_tasks": vec![json!({}); MAX_TREE_SOURCE_ROWS + 1]}
        }));
        assert_eq!(
            build(&request).expect_err("bound"),
            "tree source pm_tasks has 20,001 rows; the per-source bound is 20,000"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_unplaceable_rows_land_in_unclassified() {
        let request = tree_request(json!({"sources": {
            "transformers": [{"name": ""}],
            "pm_attachments": [
                {"id": "pa_c", "linkedEntityType": "comment", "linkedEntityId": "c_1",
                 "originalName": "aside.txt"},
                {"id": "pa_orphan", "linkedEntityType": "task", "linkedEntityId": "t_missing",
                 "originalName": "orphan.pdf"}
            ],
            "grid_exports": [{"export_id": "exp_2", "items": [{"output_id": "o"}]}],
            "design_attachments": [{"label": "no id", "file_name": "x.pdf"}],
            "solar": [{"artifacts": [{"name": "orphan.png"}]}]
        }}));
        let response = build(&request).expect("tree");
        let placed = rows(&response);
        for (source, name) in [
            ("pm_attachments", "aside.txt"),
            ("pm_attachments", "orphan.pdf"),
            ("transformers", ""),
            ("grid_exports", ""),
            ("design_attachments", ""),
            ("solar", ""),
        ] {
            assert!(
                placed.iter().any(|(path, leaf, asset_id)| path
                    == &format!("Unclassified/{source}")
                    && asset_id.starts_with(&format!("sys:unclassified:{source}:"))
                    && (name.is_empty() || leaf == name)),
                "{source}/{name} did not land in Unclassified: {placed:#?}"
            );
        }
        // Six rows in, six rows out: nothing was dropped.
        assert_eq!(placed.len(), 6);
        assert_eq!(
            folder_at(&response, "Unclassified").kind,
            FolderKind::System
        );
        // An asset the caller filed nowhere is kept too.
        let request = tree_request(json!({"assets": [{
            "asset_id": "a_1", "project_id": "p", "folder": "", "name": "loose.pdf",
            "bytes": 1, "source": {"kind": "upload"}
        }]}));
        let response = build(&request).expect("tree");
        assert_eq!(
            rows(&response),
            [(
                "Unclassified/assets".to_string(),
                "loose.pdf".to_string(),
                "a_1".to_string()
            )]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tree_malformed_rows_are_tolerated() {
        let request = tree_request(json!({"sources": {
            "transformers": ["oops", 12, [], null, {"name": 7, "uploads": "not an object",
                                                    "report_files": {"a": 1}}],
            "transformer_versions": [{"transformer_id": ["TX-1"], "version_id": "v"}],
            "solar": [{"city": "Kigali", "head": "not an object", "artifacts": 3}],
            "pm_tasks": ["not a task"],
            "pm_records": [42],
            "local_prints": [{"locator": "p_1", "size_bytes": "60", "filename": true}],
            "dataset_rooms": [{"dataset_id": "buildings", "rows": -1, "bytes": 12.0}]
        }}));
        let response = build(&request).expect("malformed rows must not refuse the whole tree");
        let placed = rows(&response);
        // The five undecodable transformer rows are kept as unclassified rows,
        // and the row whose name is a number is placed under that number.
        assert_eq!(
            placed
                .iter()
                .filter(|(path, _, _)| path == "Unclassified/transformers")
                .count(),
            4
        );
        assert!(placed
            .iter()
            .any(|(path, _, _)| path == "Transformers/7/design/current"));
        // A version row whose transformer is a list has no transformer: it is kept.
        assert!(placed
            .iter()
            .any(|(path, _, _)| path == "Unclassified/transformer_versions"));
        // A city with an unreadable head still yields its (empty) row, not a panic.
        assert!(placed
            .iter()
            .any(|(path, _, _)| path == "Unclassified/solar"));
        // Numbers that arrive as strings are read; impossible ones are dropped.
        assert_eq!(asset_named(&response, "sys:local_print:p_1").bytes, 60);
        let room = asset_named(&response, "sys:dataset_room:buildings");
        assert_eq!(room.bytes, 12);
        assert_eq!(
            room.origin.expect("origin").r#ref["rows"],
            Value::Null,
            "a negative count is absent, never a panic"
        );
        // An empty request is a tree of empty system roots.
        let empty = build(&tree_request(json!({}))).expect("tree");
        assert_eq!(empty.total_assets, 0);
        assert_eq!(empty.total_folders, (SYSTEM_ROOTS.len() - 1) as u64);
        assert!(!empty.truncated);
        assert_eq!(empty.schema, TREE_SCHEMA);
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_tree_duplicate_rows_are_one_asset() {
        // An overlapping page hands the same row twice, in two sources.
        let sources = every_source();
        let doubled: serde_json::Map<String, Value> = sources
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, rows)| {
                let mut twice = rows.as_array().unwrap().clone();
                twice.extend(rows.as_array().unwrap().iter().cloned());
                (name.clone(), Value::Array(twice))
            })
            .collect();
        let once = build(&tree_request(json!({"sources": sources}))).expect("tree");
        let twice = build(&tree_request(json!({"sources": Value::Object(doubled)}))).expect("tree");
        assert_eq!(
            serde_json::to_string(&once).unwrap(),
            serde_json::to_string(&twice).unwrap(),
            "a duplicated row is one asset, and no folder count doubles"
        );
        let ids: Vec<String> = rows(&twice).into_iter().map(|(_, _, id)| id).collect();
        let unique: BTreeSet<&String> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len());
        assert_eq!(
            folder_at(&twice, "Transformers/TX-1/attachments/permits")
                .counts
                .assets,
            2
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_tree_reserved_names_in_any_case_are_never_folders() {
        let request = tree_request(json!({"sources": {
            "transformers": [
                {"name": "MV_DATA", "report_files": ["gs://x/rows.csv"]},
                {"name": "Combined Transformer",
                 "report_files": ["gs://reports/combined/lot3-combined.xlsx"],
                 "report_artifacts": [{"url": "gs://reports/combined/lot3.pdf", "sha256": "abc"}]},
                {"name": "COLLISIONS_3"},
                {"name": " all transformers "},
                {"name": "TX-2"}
            ],
            "transformer_versions": [{"transformer_id": "Mv_Data", "version_id": "v1"}],
            "design_attachments": [{"attachment_id": "att_r", "object": {"id": "COLLISIONS"},
                                    "label": "x", "file_name": "x.pdf", "latest_revision_id": "r1"}]
        }}));
        let response = build(&request).expect("tree");
        let placed = rows(&response);
        for (path, _, _) in &placed {
            assert!(
                !path.starts_with("Transformers/") || path.starts_with("Transformers/TX-2"),
                "a reserved document became a folder: {path}"
            );
        }
        assert!(placed.contains(&(
            "Reports/combined".to_string(),
            "lot3-combined.xlsx".to_string(),
            "sys:report_file:Combined Transformer:lot3-combined.xlsx".to_string()
        )));
        assert!(placed.contains(&(
            "Reports/combined".to_string(),
            "lot3.pdf".to_string(),
            "sys:report_file:Combined Transformer:abc".to_string()
        )));
        // The other reserved rows are kept under Unclassified, never dropped.
        assert!(placed
            .iter()
            .any(|(path, name, _)| path == "Unclassified/transformers" && name == "rows.csv"));
        assert!(placed
            .iter()
            .any(|(path, _, _)| path == "Unclassified/transformer_versions"));
        assert!(placed
            .iter()
            .any(|(path, _, _)| path == "Unclassified/design_attachments"));
        assert_eq!(folder_at(&response, "Transformers").counts.folders, 1);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_tree_attachment_of_an_unlisted_transformer_is_still_under_transformers() {
        // No `transformers` source at all; the attachment's object_key alone
        // names the transformer, and that is where it lands.
        let request = tree_request(json!({"sources": {
            "design_attachments": [{
                "attachment_id": "att_g", "object_key": "lv_transformer:TX-ghost",
                "label": "permits", "file_name": "permit.pdf", "latest_revision_id": "r1",
                "latest_revision": 1
            }]
        }}));
        let response = build(&request).expect("tree");
        assert_eq!(
            rows(&response),
            [(
                "Transformers/TX-ghost/attachments/permits".to_string(),
                "r1.pdf".to_string(),
                "sys:design_attachment:att_g:r1".to_string()
            )]
        );
        let asset = asset_named(&response, "sys:design_attachment:att_g:r1");
        assert_eq!(asset.status, Status::Durable);
        assert_eq!(asset.format, Format::Pdf);
        assert_eq!(
            asset.links,
            vec![Link::DsObject {
                object_type: "transformer".into(),
                entity_id: "TX-ghost".into()
            }]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_tree_orphans_land_in_unclassified_with_source_named_ids() {
        let request = tree_request(json!({"sources": {
            "pm_attachments": [
                {"id": "pa_none", "originalName": "loose.pdf"},
                {"id": "pa_blank", "linkedEntityType": "", "linkedEntityId": "", "originalName": "blank.pdf"}
            ],
            "compounded": [{"filename": "nostem.zip", "gcs_path": "gs://x/nostem.zip"}, {}],
            "pm_tasks": [{"id": "t_1"}]
        }}));
        let response = build(&request).expect("tree");
        let placed = rows(&response);
        assert_eq!(placed.len(), 4, "four rows in, four rows out: {placed:?}");
        assert!(placed.contains(&(
            "Unclassified/pm_attachments".to_string(),
            "loose.pdf".to_string(),
            "sys:unclassified:pm_attachments:loose.pdf".to_string()
        )));
        assert!(placed.contains(&(
            "Unclassified/pm_attachments".to_string(),
            "blank.pdf".to_string(),
            "sys:unclassified:pm_attachments:blank.pdf".to_string()
        )));
        let compounded: Vec<&(String, String, String)> = placed
            .iter()
            .filter(|(path, _, _)| path == "Unclassified/compounded")
            .collect();
        assert_eq!(compounded.len(), 2);
        assert!(compounded
            .iter()
            .all(|(_, _, id)| id.starts_with("sys:unclassified:compounded:")));
        // A lookup row is never an asset.
        assert!(!placed.iter().any(|(path, _, _)| path.contains("pm_tasks")));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_tree_query_is_a_literal_case_insensitive_substring() {
        let assets = json!([
            {"asset_id": "a_1", "project_id": "p", "folder": "contracts", "name": "Résumé (final).pdf",
             "bytes": 1, "source": {"kind": "upload"}},
            {"asset_id": "a_2", "project_id": "p", "folder": "contracts", "name": "a.*b[1]+?.txt",
             "bytes": 1, "source": {"kind": "upload"}},
            {"asset_id": "a_3", "project_id": "p", "folder": "contracts", "name": "plain.txt",
             "bytes": 1, "source": {"kind": "upload"}}
        ]);
        let ask = |query: &str| {
            build(&tree_request(json!({"assets": assets, "query": query}))).expect("tree")
        };
        let names = |response: &TreeResponse| -> Vec<String> {
            rows(response)
                .into_iter()
                .map(|(_, name, _)| name)
                .collect()
        };
        assert_eq!(names(&ask("RÉSUMÉ (FINAL)")), vec!["Résumé (final).pdf"]);
        assert_eq!(names(&ask(".*b[1]+?")), vec!["a.*b[1]+?.txt"]);
        assert_eq!(names(&ask("^")), Vec::<String>::new());
        assert_eq!(names(&ask("\\")), Vec::<String>::new());
        assert_eq!(names(&ask("(")), vec!["Résumé (final).pdf"]);
        let both = ask(".txt");
        assert_eq!(names(&both), vec!["a.*b[1]+?.txt", "plain.txt"]);
        assert_eq!(folder_at(&both, "contracts").counts.assets, 2);
        assert_eq!(both.total_assets, 2);
        // Whitespace-only and empty queries filter nothing.
        assert_eq!(ask("   ").total_assets, 3);
        let overlong = "x".repeat(MAX_QUERY_CHARS + 1);
        let error = build(&tree_request(json!({"assets": assets, "query": overlong}))).unwrap_err();
        assert!(error.contains("201 characters"), "{error}");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_tree_missing_folder_filter_and_depth_cuts() {
        let sources = every_source();
        let missing = build(&tree_request(json!({
            "sources": sources, "folder": "Nowhere/at/all"
        })))
        .expect("a missing path is an empty tree, not an error");
        assert!(missing.folders.is_empty());
        assert_eq!(missing.total_assets, 0);
        assert_eq!(missing.total_folders, 0);
        assert!(!missing.truncated);
        // Case matters: a folder path is a name, not a query.
        let missing = build(&tree_request(
            json!({"sources": sources, "folder": "transformers"}),
        ))
        .expect("tree");
        assert!(missing.folders.is_empty());

        let zero = build(&tree_request(json!({"sources": sources, "depth": 0}))).expect("tree");
        assert!(zero.folders.is_empty());
        assert!(zero.truncated, "depth 0 cut something and says so");
        assert_eq!(zero.total_assets, 0);

        let one = build(&tree_request(json!({"sources": sources, "depth": 1}))).expect("tree");
        assert!(one.truncated);
        let transformers = folder_at(&one, "Transformers");
        assert!(transformers.children.is_empty());
        assert_eq!(
            transformers.counts.folders, 1,
            "the count is the folder's own even when depth hides its children"
        );
        assert_eq!(
            one.total_assets, 2,
            "Project data and My data hold their uploads at the root"
        );

        let two = build(&tree_request(json!({
            "sources": sources, "folder": "Transformers/TX-1", "depth": 2
        })))
        .expect("tree");
        assert_eq!(two.folders.len(), 1);
        assert_eq!(two.folders[0].path, "Transformers/TX-1");
        let names: Vec<&str> = two.folders[0]
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["attachments", "design", "reports", "uploads", "versions"]
        );
        assert!(two.truncated, "attachments/permits is below depth 2");
        assert_eq!(
            folder_at(&two, "Transformers/TX-1/attachments")
                .counts
                .folders,
            1
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_tree_sys_ids_and_statuses_match_the_contract_table() {
        let response = build(&tree_request(json!({"sources": every_source()}))).expect("tree");
        let expected: Vec<(&str, &str, Status)> = vec![
            (
                "Transformers/TX-1/design/current",
                "sys:transformer_design:TX-1",
                Status::Fresh,
            ),
            (
                "Transformers/TX-1/uploads/draft",
                "sys:upload:TX-1:uploads/TX-1/survey.csv",
                Status::Fresh,
            ),
            (
                "Transformers/TX-1/reports",
                "sys:report_file:TX-1:report.xlsx",
                Status::Durable,
            ),
            (
                "Transformers/TX-1/reports",
                "sys:report_file:TX-1:TX-1-map.pdf",
                Status::Durable,
            ),
            (
                "Transformers/TX-1/versions",
                "sys:transformer_version:TX-1:ver_9",
                Status::Durable,
            ),
            (
                "Transformers/TX-1/attachments/permits",
                "sys:design_attachment:att_1:rev_1",
                Status::Archive,
            ),
            (
                "Transformers/TX-1/attachments/permits",
                "sys:design_attachment:att_1:rev_2",
                Status::Durable,
            ),
            (
                "MV models/Feeder A/revisions",
                "sys:grid_revision:m_1:r_1",
                Status::Archive,
            ),
            (
                "MV models/Feeder A/revisions",
                "sys:grid_revision:m_1:r_2",
                Status::Durable,
            ),
            (
                "MV models/Feeder A/exports/exp_1",
                "sys:grid_export:m_1:exp_1:out_1",
                Status::Durable,
            ),
            (
                "Survey/poles/e_1",
                "sys:survey_media:poles:e_1:photo",
                Status::Durable,
            ),
            ("Project data", "sys:project_data:u_1", Status::Fresh),
            ("My data", "sys:user_data:u_2", Status::Fresh),
            (
                "Project work/tasks/1.2.3",
                "sys:pm_attachment:pa_1",
                Status::Durable,
            ),
            (
                "Project work/records/RFI-004",
                "sys:pm_attachment:pa_2",
                Status::Archive,
            ),
            ("Reports/combined", "sys:compounded:lot3", Status::Durable),
            (
                "Reports/compute/w_1",
                "sys:compute_artifact:w_1:o_1",
                Status::Durable,
            ),
            (
                "Solar/Kigali/results",
                "sys:solar_result:Kigali",
                Status::Durable,
            ),
            (
                "Solar/Kigali/artifacts",
                "sys:solar_artifact:Kigali:chart.png",
                Status::Durable,
            ),
            (
                "Local data/rooms",
                "sys:dataset_room:buildings",
                Status::Fresh,
            ),
            (
                "Local data/national",
                "sys:national_dataset:rwanda_admin",
                Status::Fresh,
            ),
            (
                "Local data/report rooms",
                "sys:report_room:TX-1",
                Status::Fresh,
            ),
            ("Prints/local", "sys:local_print:print_1", Status::Durable),
            ("Prints/setups", "sys:print_setup:layout_1", Status::Fresh),
        ];
        let placed = rows(&response);
        assert_eq!(placed.len(), expected.len(), "{placed:#?}");
        for (path, id, status) in expected {
            assert!(
                placed
                    .iter()
                    .any(|(at, _, found)| at == path && found == id),
                "{id} is not at {path}: {placed:#?}"
            );
            let asset = asset_named(&response, id);
            assert_eq!(asset.status, status, "{id}");
            assert_eq!(asset.sensitivity, Sensitivity::Internal, "{id}");
            assert_eq!(asset.classified_by, ClassifiedBy::Kernel, "{id}");
            assert_eq!(asset.container, None, "{id}");
            let origin = asset
                .origin
                .as_ref()
                .unwrap_or_else(|| panic!("{id} has no origin"));
            assert!(
                !TREE_SOURCE_NAMES.iter().all(|name| *name != origin.source),
                "{id}: origin names a source"
            );
            let reference = origin.r#ref.to_string();
            assert!(
                !reference.contains("Signature") && !reference.contains("https://"),
                "{id}: a signed URL never becomes an origin ref: {reference}"
            );
        }
        assert!(!rows(&response)
            .iter()
            .any(|(path, _, _)| path.starts_with("Unclassified")));
        // A source that reports no byte count still gets its format from the
        // name (0 means "not reported", not "empty").
        for (id, format, kind) in [
            (
                "sys:report_file:TX-1:report.xlsx",
                Format::Xlsx,
                Kind::Sheet,
            ),
            ("sys:report_file:TX-1:TX-1-map.pdf", Format::Pdf, Kind::Doc),
            ("sys:compounded:lot3", Format::Zip, Kind::Pack),
            ("sys:grid_export:m_1:exp_1:out_1", Format::Gpkg, Kind::Geo),
            (
                "sys:upload:TX-1:uploads/TX-1/survey.csv",
                Format::Csv,
                Kind::Sheet,
            ),
            ("sys:design_attachment:att_1:rev_2", Format::Pdf, Kind::Doc),
            ("sys:pm_attachment:pa_2", Format::Pdf, Kind::Doc),
        ] {
            let asset = asset_named(&response, id);
            assert_eq!((asset.format, asset.kind), (format, kind), "{id}");
        }
        // The https artifact URL is reduced to its path; a gs:// locator stays.
        let artifact = asset_named(&response, "sys:report_file:TX-1:TX-1-map.pdf");
        assert_eq!(
            artifact.origin.unwrap().r#ref["path"],
            "/reports/TX-1-map.pdf"
        );
        let compounded = asset_named(&response, "sys:compounded:lot3");
        assert_eq!(
            compounded.origin.unwrap().r#ref["gcs_path"],
            "gs://reports/lot3.zip"
        );
    }
}
