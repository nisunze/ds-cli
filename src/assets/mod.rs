//! Project assets: the shared model, format recognition, classification, the
//! ZIP central-directory walk, bounded `PreviewDoc` decoding and the
//! system-folder tree projection (ds-web `docs/project-assets-contract.md`).
//!
//! The kernel owns the model and every decision made over bytes it is handed.
//! Hosts own byte acquisition (signed reads, uploads, drag-drop), PDF
//! rendering (`pdf.js`, decision D8), geo feature decoding (`ds-io`, decision
//! D20), catalogue storage (ds-brain) and the worker dispatch above
//! [`MAX_MAIN_THREAD_BYTES`] (§4.4). Nothing here touches a filesystem, a
//! socket or a browser API, and no preview ever references remote content as
//! fetchable (§4.2).
//!
//! Two entry points exist because a 32 MiB document must never be base64
//! inflated inside a JSON request (§4.4):
//!
//! * [`evaluate`] — one JSON request (`ds.assets.request/v1`) carrying
//!   `recognise`, `classify` or `tree`;
//! * [`evaluate_with_bytes`] — the document bytes plus one JSON request
//!   carrying `container_walk`, `extract_member` or `preview`.
//!
//! Every bound below is a refusal that names the bound and the actual number.
//! A bound is never a silent truncation; where a document is legitimately cut
//! at a display bound (a 200-row grid of a 5,000-row sheet) the result says
//! so through `truncated` and its totals.

use std::fmt;

use base64::{
    alphabet,
    engine::{general_purpose, DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub mod classify;
pub mod container;
pub mod format;
pub mod mail_container;
pub mod preview;
pub mod tag_maps;
pub mod tree;

pub use classify::{capability_for, may_loosen, ClassifyOutcome};
pub use container::{extract_member, resolve_companions, walk};
pub use format::{format_cap_bytes, kind_of, previewable, recognise, Recognition};
pub use preview::preview;
pub use tree::build;

pub const REQUEST_SCHEMA: &str = "ds.assets.request/v1";
pub const ASSET_SCHEMA: &str = "ds.assets.asset/v1";
pub const FOLDER_SCHEMA: &str = "ds.assets.folder/v1";
pub const CONTAINER_SCHEMA: &str = "ds.assets.container/v1";
pub const PREVIEW_DOC_SCHEMA: &str = "ds.assets.preview_doc/v1";
pub const TREE_SCHEMA: &str = "ds.assets.tree/v1";

const MIB: u64 = 1024 * 1024;

/// Small command envelope; document bytes travel beside it.
/// Tree inventory metadata has a separate budget below.
pub const MAX_REQUEST_BYTES: usize = MIB as usize;
/// Project-wide inventory metadata, still constrained to 20,000 source rows.
/// Real local print receipts exceed 1 MiB for only 200 transformer rooms.
/// This is not a document/PDF size limit.
pub const MAX_TREE_REQUEST_BYTES: usize = 64 * MIB as usize;
/// Largest document either entry point accepts at all (§4.2, the pdf fetch
/// ceiling); every per-format cap below is at or under it.
pub const MAX_PREVIEW_INPUT_BYTES: u64 = 32 * MIB;
/// §4.2 pdf: recognised and bounded here, decoded by `pdf.js` in the host.
pub const MAX_PDF_BYTES: u64 = 32 * MIB;
/// §4.2 docx / odt (doc and rtf share it: recognised, never decoded).
pub const MAX_DOCUMENT_BYTES: u64 = 8 * MIB;
/// §4.2 xlsx / ods (xls shares it: recognised, never decoded).
pub const MAX_WORKBOOK_BYTES: u64 = 8 * MIB;
/// §4.2 csv / tsv.
pub const MAX_DELIMITED_BYTES: u64 = 8 * MIB;
/// §4.2 eml / msg.
pub const MAX_MAIL_BYTES: u64 = 4 * MIB;
/// §4.2 md / txt.
pub const MAX_TEXT_BYTES: u64 = MIB;
/// §4.2 jpg / png / webp / tif.
pub const MAX_IMAGE_BYTES: u64 = 12 * MIB;
/// §4.2 geojson / kml / kmz / gpkg / shp.
pub const MAX_GEO_BYTES: u64 = 20 * MIB;
/// §3.4: members listed by one walk; more reports `truncated: true`.
pub const MAX_CONTAINER_MEMBERS: usize = 5_000;
/// §3.4: directory levels a member path may have.
pub const MAX_CONTAINER_DEPTH: usize = 8;
/// §3.4: archives inside the archive that are walked; deeper ones are
/// reported as `pack` without walking.
pub const MAX_NESTED_ARCHIVE_LEVELS: usize = 1;
/// §4.2: features a geo preview may carry (matches the `ds map` scene bound);
/// above it the preview is refused and promotion is the honest alternative.
pub const MAX_GEO_FEATURES: u64 = 20_000;
/// §4.2: rows shown per grid (sheet, csv, tsv).
pub const MAX_GRID_ROWS: usize = 200;
/// §4.2: columns shown per grid.
pub const MAX_GRID_COLUMNS: usize = 40;
/// §4.2: blocks in a decoded document (docx / odt).
pub const MAX_DOC_BLOCKS: usize = 200;
/// §4.2: bytes of one inline image carried in a document preview.
pub const MAX_INLINE_IMAGE_BYTES: u64 = MIB;
/// §4.2: inline images carried in one document preview.
pub const MAX_INLINE_IMAGES: usize = 20;
/// §4.2: bytes of a mail text body.
pub const MAX_MAIL_BODY_BYTES: usize = 200_000;
/// §4.2: pdf pages a host renders eagerly; the rest render on scroll.
pub const MAX_PDF_EAGER_PAGES: u32 = 5;
/// §4.2: longest edge of an image thumbnail.
pub const MAX_THUMBNAIL_PX: u32 = 2048;
/// §4.4: inputs at or under this size may run on the main-thread kernel;
/// larger ones go through the compute worker pool. A host rule, published
/// here so both hosts read one number.
pub const MAX_MAIN_THREAD_BYTES: u64 = MIB;
/// §2.5: rows accepted per tree source list (and for `folders` / `assets`).
pub const MAX_TREE_SOURCE_ROWS: usize = 20_000;
/// §2.6: characters in a tree `query`.
pub const MAX_QUERY_CHARS: usize = 200;
/// How many leading bytes a host should send with `recognise`. More is
/// accepted up to [`MAX_REQUEST_BYTES`]; fewer may leave a ZIP-based format
/// (docx / xlsx / kmz) recognised only as `zip`.
pub const RECOGNITION_HEAD_BYTES: usize = 64 * 1024;

macro_rules! closed_vocabulary {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire spelling (identical to the serde name).
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

/// §2.3 `kind`: the short vocabulary a narrow tab and a phone row can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Doc,
    Sheet,
    Mail,
    Note,
    Geo,
    Image,
    Pack,
    #[default]
    Other,
}

closed_vocabulary!(Kind {
    Doc => "doc",
    Sheet => "sheet",
    Mail => "mail",
    Note => "note",
    Geo => "geo",
    Image => "image",
    Pack => "pack",
    Other => "other",
});

/// §2.3 `status`: exactly these three words (RULED). Metadata only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Fresh,
    Durable,
    Archive,
}

closed_vocabulary!(Status {
    Fresh => "fresh",
    Durable => "durable",
    Archive => "archive",
});

/// §2.4 `sensitivity`, declared from open to closed so the derived order is
/// the governance order: `open < internal < restricted < confidential`.
/// The default is `internal` (D4): an unclassified document is a closed one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Open,
    #[default]
    Internal,
    Restricted,
    Confidential,
}

closed_vocabulary!(Sensitivity {
    Open => "open",
    Internal => "internal",
    Restricted => "restricted",
    Confidential => "confidential",
});

impl Sensitivity {
    /// 0 for `open` up to 3 for `confidential`.
    pub fn rank(self) -> u8 {
        match self {
            Sensitivity::Open => 0,
            Sensitivity::Internal => 1,
            Sensitivity::Restricted => 2,
            Sensitivity::Confidential => 3,
        }
    }

    /// True when moving from `self` to `to` exposes more (§2.2 loosening).
    pub fn loosens_to(self, to: Sensitivity) -> bool {
        to < self
    }

    /// The stricter of the two (folder-default inheritance keeps this one).
    pub fn stricter(self, other: Sensitivity) -> Sensitivity {
        self.max(other)
    }
}

/// §2.3 `format`: what the kernel actually found, never a guess. `unknown`
/// is the honest answer for anything else, and `.bak` is not a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Pdf,
    Docx,
    Doc,
    Odt,
    Rtf,
    Xlsx,
    Xls,
    Ods,
    Csv,
    Tsv,
    Eml,
    Msg,
    Md,
    Html,
    Txt,
    Shp,
    Kmz,
    Kml,
    Gpkg,
    Geojson,
    Jpg,
    Png,
    Webp,
    Tif,
    Zip,
    #[serde(rename = "7z")]
    SevenZ,
    #[serde(rename = "tar.gz")]
    TarGz,
    Dsgrid,
    #[default]
    Unknown,
}

closed_vocabulary!(Format {
    Pdf => "pdf",
    Docx => "docx",
    Doc => "doc",
    Odt => "odt",
    Rtf => "rtf",
    Xlsx => "xlsx",
    Xls => "xls",
    Ods => "ods",
    Csv => "csv",
    Tsv => "tsv",
    Eml => "eml",
    Msg => "msg",
    Md => "md",
    Html => "html",
    Txt => "txt",
    Shp => "shp",
    Kmz => "kmz",
    Kml => "kml",
    Gpkg => "gpkg",
    Geojson => "geojson",
    Jpg => "jpg",
    Png => "png",
    Webp => "webp",
    Tif => "tif",
    Zip => "zip",
    SevenZ => "7z",
    TarGz => "tar.gz",
    Dsgrid => "dsgrid",
    Unknown => "unknown",
});

/// §2.3 `source.kind`, plus `system` for rows the tree projects (§2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Upload,
    TaskAttachment,
    MailImport,
    ReportOutput,
    Container,
    DeviceScan,
    System,
    /// Bytes held outside DS — a Drive link with a digest — registered by
    /// `assets.reference --url` (correspondence.md §Asset). Never served.
    ExternalReference,
}

closed_vocabulary!(SourceKind {
    Upload => "upload",
    TaskAttachment => "task_attachment",
    MailImport => "mail_import",
    ReportOutput => "report_output",
    Container => "container",
    DeviceScan => "device_scan",
    System => "system",
    ExternalReference => "external_reference",
});

/// §2.1 `classified_by`: a user override is never re-inferred away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifiedBy {
    #[default]
    Kernel,
    User,
}

closed_vocabulary!(ClassifiedBy {
    Kernel => "kernel",
    User => "user",
});

/// §2.5 folder kinds: declared by a person, or projected from inventories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderKind {
    #[default]
    User,
    System,
}

closed_vocabulary!(FolderKind {
    User => "user",
    System => "system",
});

/// §2.1 `dated`: every date is the source's own text, never re-parsed here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dated {
    #[serde(default)]
    pub document_date: Option<String>,
    #[serde(default)]
    pub received_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub source_modified_at: Option<String>,
}

/// §2.3 `owner`: the accountable person or team, not the uploader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub display: Option<String>,
}

/// §2.3 `source`: how the asset entered the catalogue; the uploader lives here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub kind: SourceKind,
    #[serde(default)]
    pub detail: Option<String>,
}

/// §2.1 `links`: one-way pointers from an asset to a task or a DS object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Link {
    /// A project vocabulary value, never a guessed city/display name.
    Tag {
        definition_id: String,
        value: String,
    },
    PmTask {
        id: String,
    },
    /// A correspondence record (ds-brain `docs/contracts/correspondence.md`):
    /// the asset is authored against, or attached to, one record.
    PmRecord {
        id: String,
    },
    DsObject {
        object_type: String,
        entity_id: String,
    },
}

/// A registered document (`classify --document`): the number, revision and
/// state a submission or transmittal names it by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRegistration {
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub revision_label: String,
    #[serde(default)]
    pub state: String,
}

/// One MIME part of a `mail` container as ds-brain listed it at finalize;
/// `path` is the member name the kernel's own walk derives by the same rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailPart {
    pub path: String,
    #[serde(default)]
    pub section: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub disposition: String,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub named: bool,
}

/// The header and structure metadata of a `mail` container (never a body).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub happened_at: Option<String>,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub parts: Vec<MailPart>,
    #[serde(default)]
    pub parts_total: u64,
    #[serde(default)]
    pub truncated: bool,
}

/// Where an external reference's bytes live. `read` answers this link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReference {
    pub url: String,
    #[serde(default)]
    pub provider: String,
}

/// §2.5 `origin`: which source inventory a projected row came from and the
/// reference its own read action needs. Never written to Firestore (D16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub source: String,
    pub r#ref: Value,
}

/// §2.1 asset document. `schema` defaults to [`ASSET_SCHEMA`] so a row can be
/// built without it, and is validated when present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Metadata pointer to an existing verified report output; never new bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ReportReference>,
    #[serde(default = "asset_schema")]
    pub schema: String,
    pub asset_id: String,
    pub project_id: String,
    pub folder: String,
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub format: Format,
    pub bytes: u64,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub sensitivity: Sensitivity,
    #[serde(default)]
    pub dated: Dated,
    #[serde(default)]
    pub owner: Option<Owner>,
    pub source: Source,
    #[serde(default)]
    pub classified_by: ClassifiedBy,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Registered-document identity (correspondence.md §Asset); absent on an
    /// unregistered row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentRegistration>,
    /// Header and MIME structure of a `mail` container, from the row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mail: Option<MailMetadata>,
    /// The link of an `external_reference` row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalReference>,
    /// Whether DS can serve the bytes. A row written before the field reads
    /// `true`, which is what every row then was: an external reference is
    /// the only row that carries `false`, and it always carries the field.
    #[serde(
        default = "bytes_held_default",
        skip_serializing_if = "bytes_held_is_default"
    )]
    pub bytes_held: bool,
    // ds-brain's row envelope beyond the §2.1 model. Read so a catalogued row
    // decodes as the server sends it — the tree refused every ingested row
    // (`unknown field created_by`) before these were read — and republished
    // only when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
}

fn bytes_held_default() -> bool {
    true
}

fn bytes_held_is_default(held: &bool) -> bool {
    *held
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportReference {
    pub work_id: String,
    pub output_id: String,
    pub role: String,
}

fn asset_schema() -> String {
    ASSET_SCHEMA.to_string()
}

impl Asset {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ASSET_SCHEMA {
            return Err(format!("asset {} must use {ASSET_SCHEMA}", self.asset_id));
        }
        if self.asset_id.is_empty() || self.project_id.is_empty() || self.name.is_empty() {
            return Err("asset asset_id, project_id and name must not be empty".into());
        }
        Ok(())
    }

    /// One catalogue row as ds-brain answers it (`AssetRow`), read into the
    /// kernel's model — the members the projection needs, the rest left on
    /// the wire. A translation of `decodeAssetRow` (ds-web
    /// `api/assets-decode.ts:59-91`): a row missing its id, name, kind,
    /// status, sensitivity, owner, source or byte count is not a row (`None`);
    /// a link of a kind the model does not know is dropped, not the row.
    pub fn from_wire(value: &Value) -> Option<Self> {
        let row = value.as_object()?;
        let text = |key: &str| {
            row.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let nested = |key: &str| row.get(key).and_then(Value::as_object);
        let word = |value: Option<&Value>| value.cloned();
        let owner = nested("owner")?;
        let source = nested("source")?;
        let dated = nested("dated");
        let reference = nested("reference").and_then(|reference| {
            Some(ReportReference {
                work_id: reference.get("work_id")?.as_str()?.to_owned(),
                output_id: reference.get("output_id")?.as_str()?.to_owned(),
                role: reference.get("role")?.as_str()?.to_owned(),
            })
        });
        Some(Self {
            reference,
            schema: text("schema").unwrap_or_else(asset_schema),
            asset_id: text("asset_id")?,
            project_id: text("project_id").unwrap_or_default(),
            folder: text("folder").unwrap_or_default(),
            name: text("name")?,
            kind: serde_json::from_value(word(row.get("kind"))?).ok()?,
            format: serde_json::from_value(word(row.get("format"))?).unwrap_or_default(),
            bytes: row.get("bytes")?.as_u64()?,
            digest: text("digest"),
            status: serde_json::from_value(word(row.get("status"))?).ok()?,
            sensitivity: serde_json::from_value(word(row.get("sensitivity"))?).ok()?,
            dated: Dated {
                document_date: dated
                    .and_then(|d| d.get("document_date"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
                received_at: dated
                    .and_then(|d| d.get("received_at"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
                created_at: dated
                    .and_then(|d| d.get("created_at"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
                source_modified_at: dated
                    .and_then(|d| d.get("source_modified_at"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            },
            owner: Some(Owner {
                kind: owner
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                id: owner
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                display: owner
                    .get("display")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            }),
            source: Source {
                kind: serde_json::from_value(word(source.get("kind"))?).ok()?,
                detail: source
                    .get("detail")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            },
            classified_by: serde_json::from_value(word(row.get("classified_by"))?).ok()?,
            container: text("container"),
            links: row
                .get("links")
                .and_then(Value::as_array)
                .map(|links| {
                    links
                        .iter()
                        .filter_map(|link| serde_json::from_value(link.clone()).ok())
                        .collect()
                })
                .unwrap_or_default(),
            origin: None,
            document: row
                .get("document")
                .and_then(|value| serde_json::from_value(value.clone()).ok()),
            mail: row
                .get("mail")
                .and_then(|value| serde_json::from_value(value.clone()).ok()),
            external: row
                .get("external")
                .and_then(|value| serde_json::from_value(value.clone()).ok()),
            bytes_held: row
                .get("bytes_held")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            folder_id: text("folder_id"),
            content_type: text("content_type"),
            created_by: text("created_by"),
            updated_at: text("updated_at"),
            updated_by: text("updated_by"),
            version: row.get("version").and_then(Value::as_i64),
        })
    }
}

/// §2.2 folder counts, computed per caller (D6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    #[serde(default)]
    pub assets: u64,
    #[serde(default)]
    pub folders: u64,
}

/// §2.2 folder document: a declared row (`user`) or a projected node
/// (`system`). `kind` defaults to `user`, which is what ds-brain stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Folder {
    #[serde(default = "folder_schema")]
    pub schema: String,
    pub folder_id: String,
    pub project_id: String,
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub kind: FolderKind,
    #[serde(default)]
    pub default_sensitivity: Option<Sensitivity>,
    #[serde(default)]
    pub default_status: Option<Status>,
    #[serde(default)]
    pub counts: Counts,
    // ds-brain's folder envelope beyond the §2.2 model, read so a declared
    // folder decodes as the server sends it and republished only when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_assets: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
}

fn folder_schema() -> String {
    FOLDER_SCHEMA.to_string()
}

impl Folder {
    /// One declared folder as ds-brain answers it (`AssetFolderRow`), read
    /// into the kernel's model. A translation of `decodeFolderRow` (ds-web
    /// `api/assets-decode.ts:93-110`): a row missing its id or path is not a
    /// folder; `kind` defaults to `user`, which is what ds-brain stores.
    pub fn from_wire(value: &Value) -> Option<Self> {
        let row = value.as_object()?;
        let text = |key: &str| {
            row.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let path = text("path")?;
        let counts = row.get("counts").and_then(Value::as_object);
        Some(Self {
            schema: text("schema").unwrap_or_else(folder_schema),
            folder_id: text("folder_id")?,
            project_id: text("project_id").unwrap_or_default(),
            name: text("name")
                .or_else(|| path.rsplit('/').next().map(str::to_owned))
                .unwrap_or_default(),
            path,
            parent: text("parent"),
            kind: row
                .get("kind")
                .cloned()
                .and_then(|kind| serde_json::from_value(kind).ok())
                .unwrap_or_default(),
            default_sensitivity: row
                .get("default_sensitivity")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            default_status: row
                .get("default_status")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            counts: Counts {
                assets: counts
                    .and_then(|c| c.get("assets"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                folders: counts
                    .and_then(|c| c.get("folders"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            },
            hidden_assets: row.get("hidden_assets").and_then(Value::as_u64),
            created_at: text("created_at"),
            updated_at: text("updated_at"),
            version: row.get("version").and_then(Value::as_i64),
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FOLDER_SCHEMA {
            return Err(format!(
                "folder {} must use {FOLDER_SCHEMA}",
                self.folder_id
            ));
        }
        if self.folder_id.is_empty() || self.project_id.is_empty() {
            return Err("folder folder_id and project_id must not be empty".into());
        }
        Ok(())
    }
}

/// §3.4 one row of a walked container. `companions` are the sibling files a
/// shapefile needs; `nested` marks an archive member reported without walking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerMember {
    pub path: String,
    pub bytes: u64,
    pub kind: Kind,
    pub format: Format,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub companions: Vec<String>,
    #[serde(default)]
    pub nested: bool,
}

/// §3.4 `ds.assets.container/v1`: a central-directory listing, never an
/// extraction. `asset_id` is the host's to fill; the kernel does not know it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Container {
    pub schema: String,
    #[serde(default)]
    pub asset_id: Option<String>,
    pub format: Format,
    pub walked: bool,
    pub truncated: bool,
    pub members: Vec<ContainerMember>,
    pub member_count_total: u64,
}

impl Container {
    /// An empty, walked, untruncated listing for `format`.
    pub fn new(format: Format) -> Self {
        Container {
            schema: CONTAINER_SCHEMA.to_string(),
            asset_id: None,
            format,
            walked: true,
            truncated: false,
            members: Vec::new(),
            member_count_total: 0,
        }
    }
}

/// One attachment named by a mail preview (never its bytes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailAttachment {
    pub name: String,
    pub mime: String,
    pub bytes: u64,
}

/// §4 `PreviewDoc` blocks. Svelte renders these dumbly; nothing in a block
/// is a URL to fetch, a script, or a stylesheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        text: String,
    },
    List {
        items: Vec<String>,
        ordered: bool,
    },
    /// A bounded grid: `rows_shown ≤ rows_total` and `columns.len() ≤
    /// columns_total`; the totals are how truncation is told, never hidden.
    Grid {
        title: Option<String>,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        rows_total: u64,
        rows_shown: u64,
        columns_total: u64,
    },
    /// An inline image: bytes are carried (`bytes_b64`) or absent, never
    /// referenced by a remote URL.
    Image {
        index: u32,
        mime: String,
        width: u32,
        height: u32,
        bytes_b64: Option<String>,
    },
    Mail {
        headers: Vec<(String, String)>,
        text: String,
        attachments: Vec<MailAttachment>,
    },
    /// A geo plan (D20): `decoder` is `"kernel"` for geojson counted here or
    /// `"ds-io"` for shp / kml / kmz / gpkg decoded by the host's existing
    /// decoder; `refused` names a bound or a missing reader by name.
    Geo {
        decoder: String,
        member: Option<String>,
        companions: Vec<String>,
        feature_count: Option<u64>,
        bbox: Option<[f64; 4]>,
        refused: Option<String>,
    },
    Meta {
        entries: Vec<(String, String)>,
    },
}

/// §4 `ds.assets.preview_doc/v1`. `bounded` says every §4.2 bound was applied;
/// `truncated` says at least one block was cut at a bound and reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewDoc {
    pub schema: String,
    pub format: Format,
    pub kind: Kind,
    pub bounded: bool,
    pub truncated: bool,
    pub blocks: Vec<Block>,
    pub notes: Vec<String>,
}

impl PreviewDoc {
    /// An empty bounded document for `format`, its kind from [`kind_of`].
    pub fn new(format: Format) -> Self {
        PreviewDoc {
            schema: PREVIEW_DOC_SCHEMA.to_string(),
            format,
            kind: kind_of(format),
            bounded: true,
            truncated: false,
            blocks: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// `preview` request (two-slice). `member` selects one container member and
/// is always a path inside the bytes; `sheet` selects one worksheet of a
/// workbook by name (the first sheet when absent) and is never a path, so
/// "sheet `Data` of the xlsx at `Lot3/boq.xlsx` inside this zip" is one
/// request; `rows` and `pages` are display hints bounded by [`MAX_GRID_ROWS`]
/// and [`MAX_PDF_EAGER_PAGES`]; `name` is the display name a note may cite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub schema: String,
    pub format: Format,
    #[serde(default)]
    pub member: Option<String>,
    #[serde(default)]
    pub sheet: Option<String>,
    #[serde(default)]
    pub pages: Option<u32>,
    #[serde(default)]
    pub rows: Option<u32>,
    #[serde(default)]
    pub name: Option<String>,
}

impl PreviewRequest {
    pub fn validate(&self) -> Result<(), String> {
        require_schema(&self.schema)?;
        if let Some(rows) = self.rows {
            if rows as usize > MAX_GRID_ROWS {
                return Err(format!(
                    "preview asks for {rows} rows; the grid bound is {MAX_GRID_ROWS}"
                ));
            }
        }
        if let Some(pages) = self.pages {
            if pages > MAX_PDF_EAGER_PAGES {
                return Err(format!(
                    "preview asks for {pages} pages; the eager page bound is {MAX_PDF_EAGER_PAGES}"
                ));
            }
        }
        if let Some(member) = &self.member {
            if member.is_empty() {
                return Err("preview member must name a container path".into());
            }
        }
        if let Some(sheet) = &self.sheet {
            if sheet.trim().is_empty() {
                return Err("preview sheet must name a worksheet".into());
            }
        }
        Ok(())
    }
}

/// `recognise` request: the file name (a hint only), the true length and the
/// leading bytes ([`RECOGNITION_HEAD_BYTES`] recommended) as base64.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecogniseRequest {
    pub schema: String,
    #[serde(default)]
    pub name: Option<String>,
    pub bytes_len: u64,
    pub head_b64: String,
}

/// §2.3 the fields a person may set; `reason` is the audit text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifyOverride {
    #[serde(default)]
    pub kind: Option<Kind>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub sensitivity: Option<Sensitivity>,
    #[serde(default)]
    pub owner: Option<Owner>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `classify` request: the asset as the host holds it, its folder for default
/// inheritance (§2.2), an optional override and the caller's capabilities
/// (§6.1) so loosening can be refused here rather than discovered later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifyRequest {
    pub schema: String,
    pub asset: Asset,
    #[serde(default)]
    pub folder: Option<Folder>,
    #[serde(default)]
    pub r#override: Option<ClassifyOverride>,
    #[serde(default)]
    pub caller_capabilities: Vec<String>,
}

impl ClassifyRequest {
    pub fn validate(&self) -> Result<(), String> {
        require_schema(&self.schema)?;
        self.asset.validate()?;
        if let Some(folder) = &self.folder {
            folder.validate()?;
        }
        Ok(())
    }
}

/// Source lists keyed by source name (contract §2.5). A key that is absent
/// means "not loaded" (its root renders as not loaded, never as empty); an
/// unknown key is refused by name by `tree::build`. Rows are the client's wire
/// documents exactly as received; only `tree.rs` decodes them, leniently.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreeSources(pub std::collections::BTreeMap<String, Vec<Value>>);

/// The closed set of source names the projection understands (contract §2.5).
pub const TREE_SOURCE_NAMES: [&str; 20] = [
    "transformers",
    "transformer_versions",
    "design_attachments",
    "grid_models",
    "grid_exports",
    "pm_attachments",
    "pm_tasks",
    "pm_records",
    "pm_parties",
    "compounded",
    "survey_media",
    "project_data",
    "user_data",
    "compute_artifacts",
    "solar",
    "dataset_rooms",
    "national_datasets",
    "report_rooms",
    "local_prints",
    "print_setups",
];

impl TreeSources {
    /// (name, rows) for every supplied source list, in name order.
    pub fn lists(&self) -> impl Iterator<Item = (&str, &Vec<Value>)> {
        self.0.iter().map(|(name, rows)| (name.as_str(), rows))
    }

    /// The rows of one source, or `None` when that source was not loaded.
    pub fn get(&self, name: &str) -> Option<&Vec<Value>> {
        self.0.get(name)
    }

    /// Source names the projection does not understand, in name order.
    pub fn unknown_names(&self) -> Vec<&str> {
        self.0
            .keys()
            .map(String::as_str)
            .filter(|name| !TREE_SOURCE_NAMES.contains(name))
            .collect()
    }

    /// Whether a known source was supplied at all.
    pub fn is_loaded(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }
}

/// `tree` request: sources to project, the declared folders and catalogued
/// assets, and the §2.6 filters. `depth` limits the returned nesting,
/// `folder` roots the answer at one path, `kind` keeps one folder kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeRequest {
    pub schema: String,
    #[serde(default)]
    pub sources: TreeSources,
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub link: Option<Link>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default)]
    pub kind: Option<FolderKind>,
}

impl TreeRequest {
    pub fn validate(&self) -> Result<(), String> {
        require_schema(&self.schema)?;
        for (name, rows) in self.sources.lists() {
            check_rows(&format!("tree source {name}"), rows.len())?;
        }
        check_rows("tree folders", self.folders.len())?;
        check_rows("tree assets", self.assets.len())?;
        if let Some(query) = &self.query {
            let chars = query.chars().count();
            if chars > MAX_QUERY_CHARS {
                return Err(format!(
                    "tree query is {chars} characters; the query bound is {MAX_QUERY_CHARS}"
                ));
            }
        }
        for folder in &self.folders {
            folder.validate()?;
        }
        for asset in &self.assets {
            asset.validate()?;
        }
        Ok(())
    }
}

fn check_rows(what: &str, rows: usize) -> Result<(), String> {
    if rows > MAX_TREE_SOURCE_ROWS {
        return Err(format!(
            "{what} has {} rows; the per-source bound is {}",
            thousands(rows as u64),
            thousands(MAX_TREE_SOURCE_ROWS as u64)
        ));
    }
    Ok(())
}

/// One node of `ds.assets.tree/v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeFolder {
    pub kind: FolderKind,
    pub path: String,
    pub name: String,
    pub counts: Counts,
    pub children: Vec<TreeFolder>,
    pub assets: Vec<Asset>,
}

/// `ds.assets.tree/v1`: one deterministic tree over user folders and the
/// system projection. `truncated` reports a `depth` cut or a bound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeResponse {
    pub schema: String,
    pub folders: Vec<TreeFolder>,
    pub total_assets: u64,
    pub total_folders: u64,
    pub truncated: bool,
}

/// The single-slice request grammar of [`evaluate`].
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    TagMaps {
        schema: String,
        project: String,
        page: Value,
    },
    Recognise(RecogniseRequest),
    Classify(Box<ClassifyRequest>),
    Tree(Box<TreeRequest>),
}

/// The two-slice request grammar of [`evaluate_with_bytes`].
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum BytesRequest {
    ContainerWalk {
        schema: String,
        #[serde(default)]
        format: Option<Format>,
    },
    ExtractMember {
        schema: String,
        member: String,
        #[serde(default)]
        max_bytes: Option<u64>,
    },
    Preview(PreviewRequest),
}

pub fn require_schema(schema: &str) -> Result<(), String> {
    if schema != REQUEST_SCHEMA {
        return Err(format!("assets request must use {REQUEST_SCHEMA}"));
    }
    Ok(())
}

/// One JSON request: `recognise`, `classify` or `tree`. The request bound is
/// checked before anything is parsed.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    check_bytes(
        "assets request",
        input.len() as u64,
        "request",
        MAX_TREE_REQUEST_BYTES as u64,
    )?;
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid assets request: {error}"))?;
    if !matches!(&request, Request::Tree(_)) {
        check_bytes(
            "assets request",
            input.len() as u64,
            "request",
            MAX_REQUEST_BYTES as u64,
        )?;
    }
    let value = match request {
        Request::TagMaps {
            schema,
            project,
            page,
        } => {
            require_schema(&schema)?;
            tag_maps::index(&project, &page)?
        }
        Request::Recognise(request) => {
            require_schema(&request.schema)?;
            let head = decode_base64("head_b64", &request.head_b64)?;
            if head.len() as u64 > request.bytes_len {
                return Err(format!(
                    "recognise head is {} bytes but bytes_len says the file is {} bytes",
                    thousands(head.len() as u64),
                    thousands(request.bytes_len)
                ));
            }
            let recognition = recognise(request.name.as_deref(), &head, request.bytes_len);
            with_action("recognise", serde_json::to_value(recognition))?
        }
        Request::Classify(request) => {
            request.validate()?;
            let outcome = classify::classify(&request)?;
            with_action("classify", serde_json::to_value(outcome))?
        }
        Request::Tree(request) => {
            request.validate()?;
            // `build_value` is `build` plus the §2.5 "not loaded" markers the
            // shared tree types have no field for; the wire form carries them.
            with_action("tree", Ok(tree::build_value(&request)?))?
        }
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

/// The document bytes beside one JSON request: `container_walk`,
/// `extract_member` or `preview`. Bounds are checked in this order, each a
/// named refusal: the request, the input, then the per-format cap.
pub fn evaluate_with_bytes(bytes: &[u8], request: &[u8]) -> Result<String, String> {
    check_bytes(
        "assets request",
        request.len() as u64,
        "request",
        MAX_REQUEST_BYTES as u64,
    )?;
    check_bytes(
        "preview input",
        bytes.len() as u64,
        "preview input",
        MAX_PREVIEW_INPUT_BYTES,
    )?;
    let request: BytesRequest = serde_json::from_slice(request)
        .map_err(|error| format!("invalid assets request: {error}"))?;
    let value = match request {
        BytesRequest::ContainerWalk { schema, format } => {
            require_schema(&schema)?;
            let declared = format.unwrap_or(Format::Zip);
            // A mail container is walked by its MIME structure, whether the
            // host declared `eml` or handed the bytes over under the default.
            if declared == Format::Eml || (format.is_none() && mail_container::is_message(bytes)) {
                let container = mail_container::walk(bytes)?;
                return serde_json::to_string(&with_action(
                    "container_walk",
                    serde_json::to_value(container),
                )?)
                .map_err(|error| error.to_string());
            }
            if !is_zip_container(declared) {
                return Err(match declared {
                    Format::SevenZ | Format::TarGz => format!(
                        "container walk reads zip central directories only; {declared} is reported as a pack and not walked"
                    ),
                    other => format!(
                        "{other} is not a container; container_walk lists zip archives and the zip-based docx, xlsx, odt, ods, kmz and dsgrid packages"
                    ),
                });
            }
            let mut container = walk(bytes)?;
            container.format = declared;
            with_action("container_walk", serde_json::to_value(container))?
        }
        BytesRequest::ExtractMember {
            schema,
            member,
            max_bytes,
        } => {
            require_schema(&schema)?;
            let cap = max_bytes.map_or(MAX_PREVIEW_INPUT_BYTES, |requested| {
                requested.min(MAX_PREVIEW_INPUT_BYTES)
            });
            let extracted = extract_member(bytes, &member, cap)?;
            json!({
                "schema": REQUEST_SCHEMA,
                "action": "extract_member",
                "member": member,
                "bytes": extracted.len(),
                "digest": digest_of(&extracted),
                "bytes_b64": general_purpose::STANDARD.encode(&extracted),
            })
        }
        BytesRequest::Preview(request) => {
            request.validate()?;
            if request.member.is_none() {
                check_format_cap(bytes.len() as u64, request.format)?;
            }
            with_action("preview", serde_json::to_value(preview(bytes, &request)?))?
        }
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

/// The formats whose bytes are one ZIP archive, so `container_walk` can list
/// their central directory (§3.4): the plain pack and every ZIP-based
/// document, model and geo package. `7z` and `tar.gz` are packs the kernel
/// reports without walking; nothing else is a container.
pub fn is_zip_container(format: Format) -> bool {
    matches!(
        format,
        Format::Zip
            | Format::Kmz
            | Format::Dsgrid
            | Format::Docx
            | Format::Xlsx
            | Format::Odt
            | Format::Ods
    )
}

/// Stamps `action` on a result object and gives it [`REQUEST_SCHEMA`] when the
/// document carries no schema of its own.
fn with_action(action: &str, value: serde_json::Result<Value>) -> Result<Value, String> {
    let mut value = value.map_err(|error| error.to_string())?;
    let Some(map) = value.as_object_mut() else {
        return Err(format!("assets {action} result must be an object"));
    };
    map.insert("action".into(), json!(action));
    if !map.contains_key("schema") {
        map.insert("schema".into(), json!(REQUEST_SCHEMA));
    }
    Ok(value)
}

/// Refuses `actual` over `bound`, naming both: "preview input is 34,603,008
/// bytes; the pdf bound is 32 MiB".
pub fn check_bytes(what: &str, actual: u64, bound_label: &str, bound: u64) -> Result<(), String> {
    if actual > bound {
        return Err(format!(
            "{what} is {} bytes; the {bound_label} bound is {}",
            thousands(actual),
            describe_bytes(bound)
        ));
    }
    Ok(())
}

/// The §4.2 per-format cap as a named refusal.
pub fn check_format_cap(len: u64, format: Format) -> Result<(), String> {
    check_bytes(
        "preview input",
        len,
        format.as_str(),
        format_cap_bytes(format),
    )
}

/// "32 MiB", "64 KiB" or "200,000 bytes" — whichever is exact.
pub fn describe_bytes(bytes: u64) -> String {
    if bytes > 0 && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else if bytes > 0 && bytes.is_multiple_of(1024) {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{} bytes", thousands(bytes))
    }
}

/// Decimal digits grouped by thousands: 34603008 → "34,603,008".
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Standard alphabet, padding optional: what `btoa` and `Buffer` both emit.
const BASE64_INPUT: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Decodes one base64 request field; malformed text is a named refusal.
pub fn decode_base64(field: &str, text: &str) -> Result<Vec<u8>, String> {
    BASE64_INPUT
        .decode(text.trim())
        .map_err(|error| format!("{field} is not valid base64: {error}"))
}

/// `sha256:<hex>` over the exact bytes — the digest convention of §2.1.
pub fn digest_of(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {

    /// A catalogue row as ds-brain answers it carries `folder_id`, `version`,
    /// `created_by`, `bytes_held`, `mail` … beside the model's members; the
    /// reader takes what the projection needs and leaves the rest on the
    /// wire, and a link kind it does not know drops the link, not the row.
    #[test]
    fn a_brain_row_reads_into_the_model_and_leaves_the_rest_on_the_wire() {
        let row = json!({
            "schema": "ds.assets.asset/v1", "asset_id": "a_7kq3nr2v0b1c", "project_id": "p",
            "folder_id": "f_1", "folder": "contracts", "name": "lot3.pdf", "kind": "doc",
            "format": "pdf", "bytes": 10, "digest": "sha256:ab", "status": "fresh",
            "sensitivity": "internal", "dated": {"created_at": "2026-09-20T00:00:00Z"},
            "owner": {"kind": "user", "id": "lead@ds.rw"}, "source": {"kind": "upload"},
            "classified_by": "kernel", "container": null,
            "links": [{"kind": "pm_task", "id": "t1"}, {"kind": "pm_record", "id": "R1"},
                      {"kind": "something_new", "id": "x"}],
            "bytes_held": true, "created_by": "lead@ds.rw", "updated_at": "2026-09-20T00:00:00Z",
            "updated_by": "lead@ds.rw", "version": 3, "mail": {"subject": "hi"}
        });
        let asset = Asset::from_wire(&row).expect("a row");
        assert_eq!(asset.folder, "contracts");
        assert_eq!(asset.kind, Kind::Doc);
        assert_eq!(asset.links.len(), 2);
        assert!(asset.links.contains(&Link::PmRecord { id: "R1".into() }));
        assert!(asset.validate().is_ok());
        assert!(Asset::from_wire(&json!({"asset_id": "a_x", "name": "n"})).is_none());

        let folder = Folder::from_wire(&json!({
            "schema": "ds.assets.folder/v1", "folder_id": "f_1", "project_id": "p", "kind": "user",
            "path": "contracts/2026", "name": "", "parent": "f_0", "default_sensitivity": "confidential",
            "default_status": "fresh", "counts": {"assets": 4, "folders": 1}, "hidden_assets": 2,
            "created_at": "2026-09-20T00:00:00Z", "updated_at": "2026-09-20T00:00:00Z", "version": 2
        }))
        .expect("a folder");
        assert_eq!(folder.name, "2026");
        assert_eq!(folder.counts.assets, 4);
        assert_eq!(folder.default_sensitivity, Some(Sensitivity::Confidential));
        assert!(folder.validate().is_ok());
    }
    use super::*;

    fn request(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }

    fn contract_asset() -> Value {
        json!({
            "schema": "ds.assets.asset/v1",
            "asset_id": "a_7Kq3nR2v",
            "project_id": "arjgpydw_survey_test",
            "folder": "contracts/2026/epc",
            "name": "EPC-Lot3-signed.pdf",
            "kind": "doc",
            "format": "pdf",
            "bytes": 2847113,
            "digest": "sha256:0b9c",
            "status": "durable",
            "sensitivity": "confidential",
            "dated": {
                "document_date": "2026-03-14",
                "received_at": "2026-03-16T08:12:00Z",
                "created_at": "2026-03-16T08:12:04Z",
                "source_modified_at": "2026-03-14T17:02:11Z"
            },
            "owner": { "kind": "user", "id": "nisunze@gmail.com", "display": "Nisunze" },
            "source": { "kind": "upload", "detail": "assets tab" },
            "classified_by": "kernel",
            "container": null,
            "links": [
                { "kind": "pm_task", "id": "t_4812" },
                { "kind": "ds_object", "object_type": "transformer", "entity_id": "TX-104" }
            ]
        })
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn request_bound_is_a_named_refusal() {
        let oversized = vec![b' '; MAX_TREE_REQUEST_BYTES + 1];
        let error = evaluate(&oversized).unwrap_err();
        assert!(error.contains("request bound is 64 MiB"), "{error}");
        let small_envelope = vec![b' '; MAX_REQUEST_BYTES + 1];
        let error = evaluate_with_bytes(b"", &small_envelope).unwrap_err();
        assert!(error.contains("1,048,577 bytes"), "{error}");
    }

    #[test]
    fn valid_inventory_above_small_command_budget_is_projected() {
        let source = json!({"name":"TX", "metadata":{"ignored_inventory_metadata":"x".repeat(MAX_REQUEST_BYTES)}});
        let request =
            json!({"schema": REQUEST_SCHEMA, "action":"tree", "sources":{"transformers":[source]}});
        let output = evaluate(&serde_json::to_vec(&request).unwrap()).unwrap();
        assert!(output.contains("sys:transformer_design:TX"));
        let envelope = json!({"schema": REQUEST_SCHEMA, "action":"recognise", "name":"x".repeat(MAX_REQUEST_BYTES), "bytes_len":0, "head_b64":""});
        assert!(evaluate(&serde_json::to_vec(&envelope).unwrap())
            .unwrap_err()
            .contains("request bound is 1 MiB"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn schema_action_and_unknown_fields_are_refused() {
        let wrong_schema = request(json!({
            "schema": "ds.assets.request/v0", "action": "recognise", "bytes_len": 0, "head_b64": ""
        }));
        assert!(evaluate(&wrong_schema)
            .unwrap_err()
            .contains(REQUEST_SCHEMA));
        let unknown_action = request(json!({"schema": REQUEST_SCHEMA, "action": "delete"}));
        assert!(evaluate(&unknown_action)
            .unwrap_err()
            .starts_with("invalid assets request"));
        let extra_field = request(json!({
            "schema": REQUEST_SCHEMA, "action": "recognise", "bytes_len": 0, "head_b64": "", "path": "/x"
        }));
        assert!(evaluate(&extra_field)
            .unwrap_err()
            .contains("unknown field"));
        let extra_field = request(json!({
            "schema": REQUEST_SCHEMA, "action": "preview", "format": "txt", "url": "http://x"
        }));
        assert!(evaluate_with_bytes(b"hi", &extra_field)
            .unwrap_err()
            .contains("unknown field"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn recognise_dispatches_and_checks_the_head_against_bytes_len() {
        let short = request(json!({
            "schema": REQUEST_SCHEMA, "action": "recognise", "name": "a.txt",
            "bytes_len": 2, "head_b64": "aGVsbG8="
        }));
        let error = evaluate(&short).unwrap_err();
        assert!(
            error.contains("5 bytes") && error.contains("2 bytes"),
            "{error}"
        );
        let ok = request(json!({
            "schema": REQUEST_SCHEMA, "action": "recognise", "name": "a.txt",
            "bytes_len": 5, "head_b64": "aGVsbG8"
        }));
        let result: Value = serde_json::from_str(&evaluate(&ok).unwrap()).unwrap();
        assert_eq!(result["action"], "recognise");
        assert_eq!(result["schema"], REQUEST_SCHEMA);
        assert!(result.get("format").is_some() && result.get("kind").is_some());
        let bad = request(json!({
            "schema": REQUEST_SCHEMA, "action": "recognise", "bytes_len": 5, "head_b64": "!!"
        }));
        assert!(evaluate(&bad).unwrap_err().contains("head_b64"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn preview_input_and_per_format_caps_are_named_refusals() {
        let preview =
            request(json!({"schema": REQUEST_SCHEMA, "action": "preview", "format": "txt"}));
        let oversized = vec![b'a'; MAX_PREVIEW_INPUT_BYTES as usize + 1];
        assert_eq!(
            evaluate_with_bytes(&oversized, &preview).unwrap_err(),
            "preview input is 33,554,433 bytes; the preview input bound is 32 MiB"
        );
        let over_text = vec![b'a'; MAX_TEXT_BYTES as usize + 1];
        assert_eq!(
            evaluate_with_bytes(&over_text, &preview).unwrap_err(),
            "preview input is 1,048,577 bytes; the txt bound is 1 MiB"
        );
        let rows = request(json!({
            "schema": REQUEST_SCHEMA, "action": "preview", "format": "csv", "rows": 201
        }));
        assert_eq!(
            evaluate_with_bytes(b"a,b", &rows).unwrap_err(),
            "preview asks for 201 rows; the grid bound is 200"
        );
        let walk = request(json!({
            "schema": REQUEST_SCHEMA, "action": "container_walk", "format": "7z"
        }));
        assert!(evaluate_with_bytes(b"7z", &walk)
            .unwrap_err()
            .contains("7z is reported as a pack"));
        let walk = request(json!({
            "schema": REQUEST_SCHEMA, "action": "container_walk", "format": "pdf"
        }));
        assert!(evaluate_with_bytes(b"%PDF", &walk)
            .unwrap_err()
            .contains("pdf is not a container"));
        let sheet = request(json!({
            "schema": REQUEST_SCHEMA, "action": "preview", "format": "xlsx", "sheet": " "
        }));
        assert_eq!(
            evaluate_with_bytes(b"PK", &sheet).unwrap_err(),
            "preview sheet must name a worksheet"
        );
    }

    fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::{Cursor, Write};
        let mut writer = zip::write::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn container_walk_lists_every_zip_based_format_under_its_own_name() {
        let kmz = zip_of(&[("doc.kml", b"<kml/>")]);
        for format in ["zip", "kmz", "dsgrid", "docx", "xlsx", "odt", "ods"] {
            let walk = request(json!({
                "schema": REQUEST_SCHEMA, "action": "container_walk", "format": format
            }));
            let result: Value =
                serde_json::from_str(&evaluate_with_bytes(&kmz, &walk).unwrap()).unwrap();
            assert_eq!(result["action"], "container_walk");
            assert_eq!(result["schema"], CONTAINER_SCHEMA);
            assert_eq!(result["format"], format, "{format}");
            assert_eq!(result["walked"], true);
            assert_eq!(result["members"][0]["path"], "doc.kml");
            assert_eq!(result["members"][0]["format"], "kml");
            assert_eq!(result["member_count_total"], 1);
        }
        let unnamed = request(json!({"schema": REQUEST_SCHEMA, "action": "container_walk"}));
        let result: Value =
            serde_json::from_str(&evaluate_with_bytes(&kmz, &unnamed).unwrap()).unwrap();
        assert_eq!(result["format"], "zip");
        assert!(Format::ALL
            .iter()
            .filter(|format| is_zip_container(**format))
            .all(|format| kind_of(*format) != Kind::Image));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn extract_member_answers_bytes_digest_and_base64() {
        let archive = zip_of(&[("Lot3/notes.txt", b"hello")]);
        let extract = request(json!({
            "schema": REQUEST_SCHEMA, "action": "extract_member", "member": "Lot3/notes.txt"
        }));
        let result: Value =
            serde_json::from_str(&evaluate_with_bytes(&archive, &extract).unwrap()).unwrap();
        assert_eq!(result["action"], "extract_member");
        assert_eq!(result["member"], "Lot3/notes.txt");
        assert_eq!(result["bytes"], 5);
        assert_eq!(result["digest"], digest_of(b"hello"));
        assert_eq!(result["bytes_b64"], "aGVsbG8=");
        let capped = request(json!({
            "schema": REQUEST_SCHEMA, "action": "extract_member",
            "member": "Lot3/notes.txt", "max_bytes": 4
        }));
        let error = evaluate_with_bytes(&archive, &capped).unwrap_err();
        assert!(
            error.contains("5 bytes") && error.contains("4 bytes"),
            "{error}"
        );
        let missing = request(json!({
            "schema": REQUEST_SCHEMA, "action": "extract_member", "member": "Lot3/absent.txt"
        }));
        assert!(evaluate_with_bytes(&archive, &missing)
            .unwrap_err()
            .contains("Lot3/absent.txt"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn tree_answers_carry_the_not_loaded_markers_on_the_wire() {
        let tree = request(json!({
            "schema": REQUEST_SCHEMA, "action": "tree",
            "sources": {"transformers": [{"name": "TX-1"}]}
        }));
        let result: Value = serde_json::from_str(&evaluate(&tree).unwrap()).unwrap();
        assert_eq!(result["action"], "tree");
        assert_eq!(result["schema"], TREE_SCHEMA);
        let missing = result["not_loaded_sources"]
            .as_array()
            .expect("not_loaded_sources");
        assert!(missing.contains(&json!("solar")) && !missing.contains(&json!("transformers")));
        let solar = result["folders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|folder| folder["path"] == "Solar")
            .expect("Solar root is rendered, not dropped");
        assert_eq!(solar["not_loaded"], true);
        assert_eq!(solar["not_loaded_sources"], json!(["solar"]));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn preview_dispatch_refuses_host_owned_and_undecodable_formats_by_name() {
        let refusal = |format: &str| {
            let preview =
                request(json!({"schema": REQUEST_SCHEMA, "action": "preview", "format": format}));
            evaluate_with_bytes(b"%PDF-1.7 or anything", &preview).unwrap_err()
        };
        assert!(refusal("pdf").contains("pdf.js"));
        assert!(refusal("msg").contains("slice 2"));
        assert!(refusal("zip").contains("container_walk"));
        assert!(refusal("unknown").contains("metadata card"));
        assert!(refusal("doc").contains("doc has no kernel decoder"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn tree_bounds_are_named_refusals() {
        let query = request(json!({
            "schema": REQUEST_SCHEMA, "action": "tree", "query": "q".repeat(MAX_QUERY_CHARS + 1)
        }));
        assert_eq!(
            evaluate(&query).unwrap_err(),
            "tree query is 201 characters; the query bound is 200"
        );
        let rows = request(json!({
            "schema": REQUEST_SCHEMA, "action": "tree",
            "sources": {"pm_tasks": vec![json!({}); MAX_TREE_SOURCE_ROWS + 1]}
        }));
        assert_eq!(
            evaluate(&rows).unwrap_err(),
            "tree source pm_tasks has 20,001 rows; the per-source bound is 20,000"
        );
        let foreign_row = request(json!({
            "schema": REQUEST_SCHEMA, "action": "tree",
            "sources": {"transformers": [{"name": "TX-1", "anything": {"nested": true}}]}
        }));
        let parsed: Request = serde_json::from_slice(&foreign_row).unwrap();
        assert!(
            matches!(parsed, Request::Tree(tree) if tree.sources.get("transformers").map(Vec::len) == Some(1))
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn vocabularies_round_trip_their_wire_names() {
        for format in Format::ALL {
            let wire = serde_json::to_value(format).unwrap();
            assert_eq!(wire, json!(format.as_str()));
            let back: Format = serde_json::from_value(wire).unwrap();
            assert_eq!(back, *format);
        }
        assert_eq!(Format::ALL.len(), 28);
        assert_eq!(Format::SevenZ.as_str(), "7z");
        assert_eq!(Format::TarGz.to_string(), "tar.gz");
        for kind in Kind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        }
        for source in SourceKind::ALL {
            assert_eq!(
                serde_json::to_value(source).unwrap(),
                json!(source.as_str())
            );
        }
        assert_eq!(
            serde_json::to_value(Status::Archive).unwrap(),
            json!("archive")
        );
        assert_eq!(
            serde_json::to_value(ClassifiedBy::User).unwrap(),
            json!("user")
        );
        assert_eq!(
            serde_json::to_value(FolderKind::System).unwrap(),
            json!("system")
        );
        assert!(serde_json::from_value::<Kind>(json!("folder")).is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn sensitivity_orders_open_to_confidential() {
        let order = [
            Sensitivity::Open,
            Sensitivity::Internal,
            Sensitivity::Restricted,
            Sensitivity::Confidential,
        ];
        assert_eq!(Sensitivity::ALL, &order);
        for pair in order.windows(2) {
            assert!(pair[0] < pair[1]);
            assert!(pair[1].loosens_to(pair[0]));
            assert!(!pair[0].loosens_to(pair[1]));
            assert_eq!(pair[0].stricter(pair[1]), pair[1]);
        }
        assert_eq!(Sensitivity::Confidential.rank(), 3);
        assert_eq!(Sensitivity::default(), Sensitivity::Internal);
        assert_eq!(Status::default(), Status::Fresh);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn asset_round_trips_the_contract_document() {
        let asset: Asset = serde_json::from_value(contract_asset()).unwrap();
        assert_eq!(serde_json::to_value(&asset).unwrap(), contract_asset());
        assert_eq!(asset.sensitivity, Sensitivity::Confidential);
        assert_eq!(
            asset.links[1],
            Link::DsObject {
                object_type: "transformer".into(),
                entity_id: "TX-104".into()
            }
        );
        let minimal: Asset = serde_json::from_value(json!({
            "asset_id": "a_1", "project_id": "p", "folder": "", "name": "x.bin",
            "bytes": 1, "source": {"kind": "upload"}
        }))
        .unwrap();
        assert_eq!(minimal.schema, ASSET_SCHEMA);
        assert_eq!(minimal.kind, Kind::Other);
        assert_eq!(minimal.format, Format::Unknown);
        assert_eq!(minimal.sensitivity, Sensitivity::Internal);
        assert_eq!(minimal.classified_by, ClassifiedBy::Kernel);
        assert!(minimal.validate().is_ok());
        let mut extra = contract_asset();
        extra["download_url"] = json!("https://storage/x");
        assert!(serde_json::from_value::<Asset>(extra).is_err());
        let folder: Folder = serde_json::from_value(json!({
            "schema": "ds.assets.folder/v1", "folder_id": "f_contracts_2026_epc",
            "project_id": "p", "path": "contracts/2026/epc", "name": "epc",
            "parent": "f_contracts_2026", "default_sensitivity": "confidential",
            "default_status": "durable", "counts": {"assets": 14, "folders": 0}
        }))
        .unwrap();
        assert_eq!(folder.kind, FolderKind::User);
        assert_eq!(folder.counts.assets, 14);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn classify_validates_the_asset_before_dispatch() {
        let mut asset = contract_asset();
        asset["schema"] = json!("ds.assets.asset/v0");
        let classify = request(json!({
            "schema": REQUEST_SCHEMA, "action": "classify", "asset": asset,
            "caller_capabilities": ["assets.classify"]
        }));
        assert!(evaluate(&classify).unwrap_err().contains(ASSET_SCHEMA));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn blocks_and_documents_serialise_with_snake_case_tags() {
        let mut doc = PreviewDoc::new(Format::Md);
        doc.blocks.push(Block::Heading {
            level: 1,
            text: "Title".into(),
        });
        doc.blocks.push(Block::Geo {
            decoder: "ds-io".into(),
            member: Some("gis/poles.shp".into()),
            companions: vec!["gis/poles.dbf".into()],
            feature_count: None,
            bbox: None,
            refused: None,
        });
        let value = serde_json::to_value(&doc).unwrap();
        assert_eq!(value["schema"], PREVIEW_DOC_SCHEMA);
        assert_eq!(value["kind"], "note");
        assert_eq!(value["blocks"][0]["type"], "heading");
        assert_eq!(value["blocks"][1]["type"], "geo");
        let back: PreviewDoc = serde_json::from_value(value).unwrap();
        assert_eq!(back, doc);
        let container = serde_json::to_value(Container::new(Format::Zip)).unwrap();
        assert_eq!(container["schema"], CONTAINER_SCHEMA);
        assert_eq!(container["walked"], true);
        let member = serde_json::to_value(ContainerMember {
            path: "a.pdf".into(),
            bytes: 3,
            kind: Kind::Doc,
            format: Format::Pdf,
            companions: vec![],
            nested: false,
        })
        .unwrap();
        assert!(member.get("companions").is_none());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn digest_and_number_helpers_are_exact() {
        assert_eq!(
            digest_of(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(describe_bytes(32 * MIB), "32 MiB");
        assert_eq!(describe_bytes(64 * 1024), "64 KiB");
        assert_eq!(describe_bytes(200_000), "200,000 bytes");
        assert_eq!(describe_bytes(0), "0 bytes");
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(34_603_008), "34,603,008");
        assert_eq!(decode_base64("x", "aGk=").unwrap(), b"hi");
        assert_eq!(decode_base64("x", "aGk").unwrap(), b"hi");
        assert!(decode_base64("x", "a").is_err());
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_every_format_cap_is_refused_before_decoding() {
        // Garbage one byte past each §4.2 cap: the refusal names the format,
        // the actual number and the bound, and no decoder ran — the bytes are
        // not the format they claim, so a decoder would have said something
        // else entirely.
        for format in Format::ALL {
            let cap = format_cap_bytes(*format);
            if cap >= MAX_PREVIEW_INPUT_BYTES {
                continue;
            }
            let oversized = vec![b'x'; cap as usize + 1];
            let ask = request(json!({
                "schema": REQUEST_SCHEMA, "action": "preview", "format": format.as_str()
            }));
            let error = evaluate_with_bytes(&oversized, &ask).unwrap_err();
            assert_eq!(
                error,
                format!(
                    "preview input is {} bytes; the {format} bound is {}",
                    thousands(cap + 1),
                    describe_bytes(cap)
                ),
                "{format}"
            );
        }
        let oversized = vec![b'x'; MAX_PREVIEW_INPUT_BYTES as usize + 1];
        for format in ["pdf", "zip", "dsgrid", "unknown"] {
            let ask = request(json!({
                "schema": REQUEST_SCHEMA, "action": "preview", "format": format
            }));
            assert_eq!(
                evaluate_with_bytes(&oversized, &ask).unwrap_err(),
                "preview input is 33,554,433 bytes; the preview input bound is 32 MiB"
            );
        }
        let ask = request(json!({"schema": REQUEST_SCHEMA, "action": "container_walk"}));
        assert!(evaluate_with_bytes(&oversized, &ask)
            .unwrap_err()
            .contains("33,554,433"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_pages_hint_is_bounded_like_rows() {
        let ask = request(json!({
            "schema": REQUEST_SCHEMA, "action": "preview", "format": "txt", "pages": 6
        }));
        assert_eq!(
            evaluate_with_bytes(b"hello", &ask).unwrap_err(),
            "preview asks for 6 pages; the eager page bound is 5"
        );
        let ask = request(json!({
            "schema": REQUEST_SCHEMA, "action": "preview", "format": "txt", "pages": 5, "rows": 200
        }));
        assert!(evaluate_with_bytes(b"hello", &ask).is_ok());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_no_filesystem_or_network_anywhere_in_assets() {
        // Needles are assembled from fragments so this test is not itself the
        // thing it forbids; the sources are read at compile time.
        let sources: [(&str, &str); 13] = [
            ("mod.rs", include_str!("mod.rs")),
            ("classify.rs", include_str!("classify.rs")),
            ("container.rs", include_str!("container.rs")),
            ("format.rs", include_str!("format.rs")),
            ("tree.rs", include_str!("tree.rs")),
            ("preview/mod.rs", include_str!("preview/mod.rs")),
            ("preview/text.rs", include_str!("preview/text.rs")),
            ("preview/tabular.rs", include_str!("preview/tabular.rs")),
            ("preview/image.rs", include_str!("preview/image.rs")),
            ("preview/eml.rs", include_str!("preview/eml.rs")),
            ("preview/xlsx.rs", include_str!("preview/xlsx.rs")),
            ("preview/docx.rs", include_str!("preview/docx.rs")),
            ("preview/geo.rs", include_str!("preview/geo.rs")),
        ];
        let needles = [
            concat!("std::", "fs"),
            concat!("std::", "net"),
            concat!("std::", "process"),
            concat!("File::", "open"),
            concat!("File::", "create"),
            concat!("fs::", "read"),
            concat!("fs::", "write"),
            concat!("req", "west"),
            concat!("ur", "eq::"),
            concat!("Tcp", "Stream"),
            concat!("web_", "sys"),
            concat!("js_", "sys"),
            concat!("Indexed", "DB"),
            concat!("extract_to", "_directory"),
        ];
        for (name, source) in sources {
            for needle in needles {
                assert!(
                    !source.contains(needle),
                    "src/assets/{name} reaches outside the kernel: it contains {needle}"
                );
            }
        }
    }

    fn classified(body: Value) -> Result<Value, String> {
        let mut body = body;
        body["schema"] = json!(REQUEST_SCHEMA);
        body["action"] = json!("classify");
        evaluate(&request(body)).map(|text| serde_json::from_str(&text).unwrap())
    }

    fn bare_asset(fields: Value) -> Value {
        let mut asset = json!({
            "asset_id": "a_1", "project_id": "p", "folder": "contracts", "name": "x.pdf",
            "format": "pdf", "bytes": 1, "source": {"kind": "upload"}
        });
        if let (Some(base), Some(extra)) = (asset.as_object_mut(), fields.as_object()) {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        asset
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_default_sensitivity_is_internal_and_never_open() {
        // No class at all → internal.
        let out = classified(json!({"asset": bare_asset(json!({}))})).unwrap();
        assert_eq!(out["asset"]["sensitivity"], "internal");
        // A kernel-classified row that arrives `open` is closed to internal.
        let out = classified(json!({"asset": bare_asset(json!({"sensitivity": "open"}))})).unwrap();
        assert_eq!(out["asset"]["sensitivity"], "internal");
        assert!(out["changed"]
            .as_array()
            .unwrap()
            .contains(&json!("sensitivity")));
        // A folder whose default is `open` cannot open anything.
        let out = classified(json!({
            "asset": bare_asset(json!({})),
            "folder": {"folder_id": "f", "project_id": "p", "path": "contracts", "name": "contracts",
                       "default_sensitivity": "open"}
        }))
        .unwrap();
        assert_eq!(out["asset"]["sensitivity"], "internal");
        // Only a capable person opens a document, and that is audited.
        let out = classified(json!({
            "asset": bare_asset(json!({})),
            "override": {"sensitivity": "open"},
            "caller_capabilities": ["assets.classify", "assets.read"]
        }))
        .unwrap();
        assert_eq!(out["asset"]["sensitivity"], "open");
        assert_eq!(out["asset"]["classified_by"], "user");
        assert_eq!(out["audit_required"], true);
        let error = classified(json!({
            "asset": bare_asset(json!({})),
            "override": {"sensitivity": "open"},
            "caller_capabilities": ["assets.classify"]
        }))
        .unwrap_err();
        assert!(error.contains("assets.read"), "{error}");
        // Projected rows are internal too.
        let tree: Value = serde_json::from_str(
            &evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "tree",
                "sources": {"compounded": [{"stem": "lot3"}]}
            })))
            .unwrap(),
        )
        .unwrap();
        let reports = tree["folders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|folder| folder["path"] == "Reports")
            .unwrap();
        assert_eq!(
            reports["children"][0]["assets"][0]["sensitivity"],
            "internal"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_user_override_is_never_reinferred() {
        // A person said `sheet` / `archive` / `restricted` about a pdf; the
        // kernel would infer `doc` / the folder's `durable` / internal. None
        // of that runs over a person's decision.
        let out = classified(json!({
            "asset": bare_asset(json!({
                "kind": "sheet", "status": "archive", "sensitivity": "restricted",
                "classified_by": "user"
            })),
            "folder": {"folder_id": "f", "project_id": "p", "path": "contracts", "name": "contracts",
                       "default_status": "durable", "default_sensitivity": "internal"}
        }))
        .unwrap();
        assert_eq!(out["asset"]["kind"], "sheet");
        assert_eq!(out["asset"]["status"], "archive");
        assert_eq!(out["asset"]["sensitivity"], "restricted");
        assert_eq!(out["asset"]["classified_by"], "user");
        assert_eq!(out["changed"], json!([]));
        assert_eq!(out["audit_required"], false);
        // The same row classified by the kernel is inferred.
        let out = classified(json!({
            "asset": bare_asset(json!({"kind": "sheet", "status": "fresh"})),
            "folder": {"folder_id": "f", "project_id": "p", "path": "contracts", "name": "contracts",
                       "default_status": "durable"}
        }))
        .unwrap();
        assert_eq!(out["asset"]["kind"], "doc");
        assert_eq!(out["asset"]["status"], "durable");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_pdf_is_refused_with_the_pdf_js_sentence() {
        let ask = request(json!({"schema": REQUEST_SCHEMA, "action": "preview", "format": "pdf"}));
        let error = evaluate_with_bytes(b"%PDF-1.7 tiny", &ask).unwrap_err();
        assert_eq!(
            error,
            "pdf is rendered by pdf.js in the host (decision D8); the kernel recognises and bounds pdf but does not decode it"
        );
        let recognised: Value = serde_json::from_str(
            &evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "recognise", "name": "EPC.pdf",
                "bytes_len": 13,
                "head_b64": general_purpose::STANDARD.encode(b"%PDF-1.7 tiny")
            })))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(recognised["format"], "pdf");
        assert_eq!(recognised["previewable"], true);
        assert!(recognised["reason"].as_str().unwrap().contains("pdf.js"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_unsupported_formats_are_refused_with_the_honest_alternative() {
        for (format, bytes, alternative) in [
            ("doc", b"\xD0\xCF\x11\xE0".as_slice(), "docx"),
            ("rtf", b"{\\rtf1".as_slice(), "docx"),
            ("xls", b"\xD0\xCF\x11\xE0".as_slice(), "xlsx"),
            ("msg", b"\xD0\xCF\x11\xE0".as_slice(), "eml"),
            ("odt", b"PK\x03\x04".as_slice(), "docx"),
            ("ods", b"PK\x03\x04".as_slice(), "xlsx"),
            ("dsgrid", b"PK\x03\x04".as_slice(), "DS Grid"),
            ("zip", b"PK\x03\x04".as_slice(), "container_walk"),
            ("7z", b"7z".as_slice(), "container_walk"),
            ("tar.gz", b"\x1f\x8b".as_slice(), "container_walk"),
            ("unknown", b"?".as_slice(), "metadata card"),
        ] {
            let ask = request(json!({
                "schema": REQUEST_SCHEMA, "action": "preview", "format": format
            }));
            let error = evaluate_with_bytes(bytes, &ask).unwrap_err();
            assert!(
                error.contains(alternative),
                "{format}: the refusal must offer {alternative}: {error}"
            );
            assert!(
                error.starts_with(format) || error.contains(format),
                "{format}: {error}"
            );
        }
        // gpkg is a refusal inside the document, by name (§4.1).
        let ask = request(json!({"schema": REQUEST_SCHEMA, "action": "preview", "format": "gpkg"}));
        let doc: Value =
            serde_json::from_str(&evaluate_with_bytes(b"SQLite format 3\0", &ask).unwrap())
                .unwrap();
        assert!(doc["blocks"][0]["refused"]
            .as_str()
            .unwrap()
            .contains("no SQLite reader in WASM"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_unknown_fields_refused_on_requests_but_tolerated_on_source_rows() {
        let refusals: Vec<Result<String, String>> = vec![
            evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "classify",
                "asset": bare_asset(json!({})), "surprise": 1
            }))),
            evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "classify",
                "asset": bare_asset(json!({"download_url": "https://x"}))
            }))),
            evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "classify",
                "asset": bare_asset(json!({})), "override": {"format": "pdf"}
            }))),
            evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "classify",
                "asset": bare_asset(json!({})),
                "folder": {"folder_id": "f", "project_id": "p", "path": "contracts",
                           "name": "contracts", "colour": "red"}
            }))),
            evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "tree", "sources": {}, "poll": true
            }))),
            evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "recognise", "bytes_len": 0,
                "head_b64": "", "path": "/etc/passwd"
            }))),
            evaluate_with_bytes(
                b"PK",
                &request(json!({
                    "schema": REQUEST_SCHEMA, "action": "container_walk", "extract": "/tmp"
                })),
            ),
            evaluate_with_bytes(
                b"PK",
                &request(json!({
                    "schema": REQUEST_SCHEMA, "action": "extract_member", "member": "a",
                    "to": "/tmp"
                })),
            ),
            evaluate_with_bytes(
                b"x",
                &request(json!({
                    "schema": REQUEST_SCHEMA, "action": "preview", "format": "txt",
                    "fetch": "https://x"
                })),
            ),
        ];
        for outcome in refusals {
            let error = outcome.expect_err("an unknown request field is refused");
            assert!(
                error.contains("unknown field"),
                "the refusal names the unknown field: {error}"
            );
        }
        // A foreign source row grows fields that are none of the kernel's
        // business; it still projects.
        let tree: Value = serde_json::from_str(
            &evaluate(&request(json!({
                "schema": REQUEST_SCHEMA, "action": "tree",
                "sources": {"compounded": [{
                    "stem": "lot3", "download_url": "https://signed", "zzz": [1, 2, 3],
                    "nested": {"deep": {"deeper": true}}
                }]}
            })))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(tree["total_assets"], 1);
        assert!(!serde_json::to_string(&tree).unwrap().contains("signed"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_document_and_request_schemas_are_validated() {
        let error =
            classified(json!({"asset": bare_asset(json!({"schema": "ds.assets.asset/v0"}))}))
                .unwrap_err();
        assert!(error.contains(ASSET_SCHEMA), "{error}");
        let error = classified(json!({
            "asset": bare_asset(json!({})),
            "folder": {"schema": "ds.assets.folder/v2", "folder_id": "f", "project_id": "p",
                       "path": "contracts", "name": "contracts"}
        }))
        .unwrap_err();
        assert!(error.contains(FOLDER_SCHEMA), "{error}");
        for body in [
            json!({"schema": "ds.assets.request/v2", "action": "container_walk"}),
            json!({"schema": "ds.assets.request/v2", "action": "extract_member", "member": "a"}),
            json!({"schema": "ds.assets.request/v2", "action": "preview", "format": "txt"}),
            json!({"schema": "ds.assets.request/v2", "action": "tree"}),
            json!({"schema": "ds.assets.request/v2", "action": "classify", "asset": bare_asset(json!({}))}),
        ] {
            let ask = request(body.clone());
            let two_slice = evaluate_with_bytes(b"x", &ask).expect_err("refused");
            let one_slice = evaluate(&ask).expect_err("refused");
            assert!(
                two_slice.contains(REQUEST_SCHEMA) || one_slice.contains(REQUEST_SCHEMA),
                "{body}: {two_slice} / {one_slice}"
            );
        }
        // Results carry their own schema and the action they answered.
        let ask = request(json!({"schema": REQUEST_SCHEMA, "action": "preview", "format": "txt"}));
        let doc: Value = serde_json::from_str(&evaluate_with_bytes(b"hi", &ask).unwrap()).unwrap();
        assert_eq!(doc["schema"], PREVIEW_DOC_SCHEMA);
        assert_eq!(doc["action"], "preview");
    }
}
