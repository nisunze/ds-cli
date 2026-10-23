//! The Compounded Report archive, decided on the edge.
//!
//! A Compounded Report is one ZIP: every participating transformer's current
//! individual report files, the overall combined data set at the root, one
//! combined set per first-level applied group when asked, and
//! `manifest.json`. ds-brain no longer composes it; it only stores the
//! published archive. This module owns every decision of the composition: the
//! closed `ds.compounded-report/v1` recipe, the archive tree (the applied
//! `report_archive` grouping's ordered levels), the bundle plan the Reporter's
//! closed `export_compounded_report` task executes, the manifest and the
//! publication identity (`export-compounded_report`). Hosts read the inputs,
//! call the Reporter and seal the result; nothing here reads a file, a clock or
//! the network.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const COMPOSITION_SCHEMA: &str = "ds.compounded-report/v1";
pub const MANIFEST_SCHEMA: &str = "ds.compounded-report-manifest/v1";
/// The publication identity: the archive is sealed as `export-compounded_report`.
pub const IDENTITY: &str = "compounded_report";
pub const OPERATION: &str = "export-compounded_report";
pub const ARCHIVE_FILENAME: &str = "compounded_reports.zip";
pub const ARCHIVE_FORMAT: &str = "zip";
pub const MAX_COMPOSITION_BYTES: usize = 64 * 1024;
pub const MAX_TRANSFORMERS: usize = 500;
pub const MAX_GROUP_DEPTH: u8 = 16;
/// The Reporter's own bound on archive members.
pub const MAX_ENTRIES: usize = 2_000;
/// The compute-artifact door admits one declared output of at most 64 MiB.
pub const MAX_PUBLISHED_BYTES: u64 = 64 << 20;
const UNASSIGNED: &str = "_unassigned";
const MAX_NAME_CHARS: usize = 200;
const MAX_SEGMENT_CHARS: usize = 120;

/// A named refusal; the host maps the code onto its declared refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

impl Refusal {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub const COMPOSITION_INVALID: &str = "compounded_composition_invalid";
pub const GROUPING_INVALID: &str = "compounded_grouping_invalid";
pub const INPUTS_INVALID: &str = "compounded_inputs_invalid";
pub const ARCHIVE_TOO_LARGE: &str = "compounded_archive_too_large";

/// The archive tree: how many applied grouping levels to descend (`None` =
/// every level), whether each transformer gets its own folder, and whether a
/// combined set is filed per first-level group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub group_depth: Option<u8>,
    pub transformer_folders: bool,
    pub combine_per_group: bool,
}

impl Default for Layout {
    /// Every applied level, one folder per transformer; flat without a plan.
    fn default() -> Self {
        Self {
            group_depth: None,
            transformer_folders: true,
            combine_per_group: false,
        }
    }
}

impl Layout {
    /// The legacy `--file-level` spelling of the same tree.
    pub fn from_file_level(level: &str, combine_per_group: bool) -> Result<Self, Refusal> {
        let (group_depth, transformer_folders) = match level {
            "transformer" => (None, true),
            "sector" => (None, false),
            "district" => (Some(1), false),
            "root" => (Some(0), false),
            _ => {
                return Err(Refusal::new(
                    COMPOSITION_INVALID,
                    format!("file level {level:?} is not transformer, sector, district or root"),
                ))
            }
        };
        Ok(Self {
            group_depth,
            transformer_folders,
            combine_per_group,
        })
    }

    /// Whether the tree reads the applied grouping at all.
    pub fn needs_grouping(&self) -> bool {
        self.group_depth != Some(0) || self.combine_per_group
    }

    /// One transformer's group segments, cut to this layout's depth.
    pub fn group_segments<'a>(&self, segments: &'a [String]) -> &'a [String] {
        let depth = self.group_depth.map_or(segments.len(), |depth| {
            segments.len().min(usize::from(depth))
        });
        &segments[..depth]
    }

    /// The folder (trailing slash, or empty at the root) one transformer's
    /// files are filed under.
    pub fn transformer_dir(&self, name: &str, segments: &[String]) -> String {
        let mut prefix = group_dir(self.group_segments(segments));
        if self.transformer_folders {
            prefix.push_str(&sanitize_folder(name));
            prefix.push('/');
        }
        prefix
    }

    /// The layout as an archive listing records it: the generic knobs plus
    /// the legacy `combine_per_district` twin older readers know.
    pub fn describe(&self) -> Value {
        json!({
            "group_depth": self.group_depth,
            "transformer_folders": self.transformer_folders,
            "combine_per_group": self.combine_per_group,
            "combine_per_district": self.combine_per_group,
        })
    }
}

/// One closed composition: the explicit scope (empty = every active saved
/// transformer), the tree, and the recorded force bit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composition {
    pub transformers: Vec<String>,
    pub archive_layout: Layout,
    pub force: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireLayout {
    #[serde(default)]
    group_depth: Option<i64>,
    transformer_folders: Option<bool>,
    combine_per_group: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireComposition {
    schema: String,
    transformers: Option<Vec<String>>,
    archive_layout: Option<WireLayout>,
    force: Option<bool>,
}

impl Composition {
    /// Admit one `ds.compounded-report/v1` object: every member required,
    /// unknown members refused, at most 500 distinct real transformer names.
    pub fn parse(value: &Value) -> Result<Self, Refusal> {
        let invalid = |message: String| Refusal::new(COMPOSITION_INVALID, message);
        if !value.is_object() || value.to_string().len() > MAX_COMPOSITION_BYTES {
            return Err(invalid(
                "composition must be one JSON object of at most 64 KiB".into(),
            ));
        }
        let wire: WireComposition = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("invalid compounded composition: {error}")))?;
        let (Some(transformers), Some(layout), Some(force)) =
            (wire.transformers, wire.archive_layout, wire.force)
        else {
            return Err(invalid(format!(
                "composition requires schema {COMPOSITION_SCHEMA}, transformers, archive_layout and force"
            )));
        };
        if wire.schema != COMPOSITION_SCHEMA {
            return Err(invalid(format!(
                "composition schema must be {COMPOSITION_SCHEMA}"
            )));
        }
        let (Some(transformer_folders), Some(combine_per_group)) =
            (layout.transformer_folders, layout.combine_per_group)
        else {
            return Err(invalid(
                "archive_layout requires transformer_folders and combine_per_group".into(),
            ));
        };
        let group_depth = match layout.group_depth {
            None => None,
            Some(depth) if (0..=i64::from(MAX_GROUP_DEPTH)).contains(&depth) => Some(depth as u8),
            Some(_) => {
                return Err(invalid(
                    "group_depth must be null or an integer from 0 to 16".into(),
                ))
            }
        };
        if transformers.len() > MAX_TRANSFORMERS {
            return Err(invalid(format!(
                "composition exceeds {MAX_TRANSFORMERS} transformers"
            )));
        }
        let mut seen = BTreeSet::new();
        for name in &transformers {
            let canonical = crate::report::transformers::canonical_transformer_reference(name);
            if name.is_empty()
                || name.trim() != name
                || name.chars().count() > MAX_NAME_CHARS
                || crate::report::is_reserved_report_identity(&canonical)
                || canonical == IDENTITY
                || !seen.insert(canonical)
            {
                return Err(invalid(format!(
                    "composition requires distinct bounded real transformer identities: {name:?}"
                )));
            }
        }
        Ok(Self {
            transformers,
            archive_layout: Layout {
                group_depth,
                transformer_folders,
                combine_per_group,
            },
            force,
        })
    }

    /// The composition this run used, with the resolved scope.
    pub fn recorded(&self, resolved: &[String]) -> Value {
        json!({
            "schema": COMPOSITION_SCHEMA,
            "transformers": resolved,
            "archive_layout": {
                "group_depth": self.archive_layout.group_depth,
                "transformer_folders": self.archive_layout.transformer_folders,
                "combine_per_group": self.archive_layout.combine_per_group,
            },
            "force": self.force,
        })
    }
}

/// Keep a readable group label but make it a member name the Reporter's
/// portable ZIP grammar admits; an empty value is the `_unassigned` bucket.
pub fn sanitize_folder(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            c if c.is_control() => '-',
            '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*' => '-',
            c => c,
        })
        .take(MAX_SEGMENT_CHARS)
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return UNASSIGNED.to_owned();
    }
    let stem = cleaned
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && ('1'..='9').contains(&suffix.chars().next().unwrap_or('0'))
            })
        });
    if reserved {
        format!("_{cleaned}")
    } else {
        cleaned
    }
}

/// The folder (trailing slash) of one ordered group path.
pub fn group_dir(segments: &[String]) -> String {
    segments
        .iter()
        .map(|segment| format!("{}/", sanitize_folder(segment)))
        .collect()
}

/// The applied `report_archive` grouping as each transformer's ordered
/// display segments, plus the plan's identity for the manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupPaths {
    pub by_transformer: BTreeMap<String, Vec<String>>,
    pub identity: Option<Value>,
}

/// Read the stored plan (`design.consumer-grouping.read --purpose
/// report_archive`). An absent or archived plan is a flat archive.
pub fn group_paths(plan: Option<&Value>) -> Result<GroupPaths, Refusal> {
    let invalid = |message: &str| Refusal::new(GROUPING_INVALID, message.to_owned());
    let Some(plan) = plan.filter(|plan| !plan.is_null()) else {
        return Ok(GroupPaths::default());
    };
    if plan["lifecycle"].as_str() == Some("archived") {
        return Ok(GroupPaths::default());
    }
    let groups = plan["groups"]
        .as_array()
        .ok_or_else(|| invalid("the applied report_archive grouping has no groups"))?;
    let levels = plan["definition_ids"].as_array().map_or(0, Vec::len);
    let mut by_transformer = BTreeMap::new();
    for group in groups {
        let segments: Vec<String> = if group["unassigned"] == true {
            vec![String::new(); levels.max(1)]
        } else {
            group["values"]
                .as_array()
                .ok_or_else(|| invalid("a grouping group has no ordered values"))?
                .iter()
                .map(|value| {
                    let pick = |key: &str| -> Vec<String> {
                        value[key]
                            .as_array()
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .filter(|item| !item.trim().is_empty())
                                    .map(str::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default()
                    };
                    let labels = pick("labels");
                    if labels.is_empty() {
                        pick("values").join(" + ")
                    } else {
                        labels.join(" + ")
                    }
                })
                .collect()
        };
        for name in group["transformers"]
            .as_array()
            .ok_or_else(|| invalid("a grouping group has no transformer list"))?
        {
            let name = name
                .as_str()
                .ok_or_else(|| invalid("a grouping member is not a name"))?;
            if by_transformer
                .insert(name.to_owned(), segments.clone())
                .is_some()
            {
                return Err(invalid("a transformer belongs to two grouping groups"));
            }
        }
    }
    Ok(GroupPaths {
        by_transformer,
        identity: Some(json!({
            "applied": true,
            "purpose": plan["purpose"],
            "definition_ids": plan["definition_ids"],
            "plan_digest": plan["plan_digest"],
            "revision": plan["revision"],
        })),
    })
}

/// One file the archive carries, already staged by the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedFile {
    pub filename: String,
    pub source: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// One transformer's current individual report set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub transformer: String,
    pub revision: i64,
    pub batch_id: String,
    pub files: Vec<StagedFile>,
}

/// One combined data set: the root set (`group` empty) or one first-level
/// group's set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombinedSet {
    pub group: Option<String>,
    pub transformers: Vec<String>,
    pub files: Vec<StagedFile>,
}

pub struct PlanInput<'a> {
    pub project: &'a str,
    pub composition: &'a Composition,
    pub participating: &'a [String],
    pub members: &'a [Member],
    pub groups: &'a GroupPaths,
    pub combined: &'a [CombinedSet],
    pub output: &'a str,
}

/// The Reporter request, the manifest it embeds, and the publication facts.
#[derive(Clone, Debug, PartialEq)]
pub struct BundlePlan {
    pub request: Value,
    pub manifest: Value,
    pub input_fingerprint: String,
    pub prefix: String,
    pub entry_count: usize,
}

fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.trim() == value
        && !value.contains(['/', '\\'])
        && !value.chars().any(char::is_control)
        && value != "."
        && value != ".."
}

/// The first-level groups a per-group combined set is filed for.
pub fn first_level_groups(
    groups: &GroupPaths,
    participating: &[String],
) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in participating {
        let first = groups
            .by_transformer
            .get(name)
            .and_then(|segments| segments.first())
            .cloned()
            .unwrap_or_default();
        out.entry(first).or_default().push(name.clone());
    }
    out
}

/// Plan one archive. Every participating transformer must arrive with its
/// current set; a missing member is the host's readiness refusal, never a
/// silently smaller archive.
pub fn plan(input: &PlanInput<'_>) -> Result<BundlePlan, Refusal> {
    let invalid = |message: String| Refusal::new(INPUTS_INVALID, message);
    if input.participating.is_empty() {
        return Err(invalid("no transformer participates in the archive".into()));
    }
    let members: BTreeMap<&str, &Member> = input
        .members
        .iter()
        .map(|member| (member.transformer.as_str(), member))
        .collect();
    if members.len() != input.members.len()
        || input.participating.len() != members.len()
        || input
            .participating
            .iter()
            .any(|name| !members.contains_key(name.as_str()))
    {
        return Err(invalid(
            "every participating transformer needs exactly one current set".into(),
        ));
    }
    if !input
        .combined
        .iter()
        .any(|set| set.group.is_none() && !set.files.is_empty())
    {
        return Err(invalid("the overall combined set is missing".into()));
    }
    let layout = &input.composition.archive_layout;
    let empty = Vec::new();
    let mut entries = Vec::new();
    let mut paths = BTreeSet::new();
    let mut add = |path: String, file: &StagedFile| -> Result<Value, Refusal> {
        if !file_name(&file.filename) || !hex(&file.sha256) || file.size_bytes == 0 {
            return Err(invalid(format!(
                "unsafe or unpinned file {:?}",
                file.filename
            )));
        }
        if path == "manifest.json" || !paths.insert(path.to_ascii_lowercase()) {
            return Err(invalid(format!("two files share the archive path {path}")));
        }
        entries.push(json!({"source": file.source, "archive_path": path, "sha256": file.sha256}));
        Ok(json!({"path": path, "sha256": file.sha256, "size_bytes": file.size_bytes}))
    };
    let mut transformers = Vec::new();
    for name in input.participating {
        let member = members[name.as_str()];
        if member.files.is_empty() || member.revision < 1 {
            return Err(invalid(format!("{name} has no current individual report")));
        }
        let segments = input.groups.by_transformer.get(name).unwrap_or(&empty);
        let dir = layout.transformer_dir(name, segments);
        let files = member
            .files
            .iter()
            .map(|file| add(format!("{dir}{}", file.filename), file))
            .collect::<Result<Vec<_>, _>>()?;
        transformers.push(json!({
            "name": name,
            "revision": member.revision,
            "batch_id": member.batch_id,
            "group": layout.group_segments(segments),
            "files": files,
        }));
    }
    let mut combined = Vec::new();
    for set in input.combined {
        let dir = set
            .group
            .as_deref()
            .map(|group| group_dir(&[group.to_owned()]));
        let files = set
            .files
            .iter()
            .map(|file| match &dir {
                None => add(file.filename.clone(), file),
                Some(dir) => add(
                    format!("{dir}{}_{}", dir.trim_end_matches('/'), file.filename),
                    file,
                ),
            })
            .collect::<Result<Vec<_>, _>>()?;
        combined
            .push(json!({"group": set.group, "transformers": set.transformers, "files": files}));
    }
    if entries.len() + 1 > MAX_ENTRIES {
        return Err(invalid(format!(
            "{} files exceed the archive's {MAX_ENTRIES} members",
            entries.len()
        )));
    }
    let mut manifest = json!({
        "schema": MANIFEST_SCHEMA,
        "project": input.project,
        "composition": input.composition.recorded(input.participating),
        "archive_layout": layout.describe(),
        "grouping": input.groups.identity.clone().unwrap_or_else(|| json!({"applied": false})),
        "transformers": transformers,
        "combined": combined,
    });
    manifest.sort_all_objects();
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&manifest).expect("a manifest encodes"))
    );
    let Value::Object(manifest_map) = manifest.clone() else {
        unreachable!("the manifest is an object")
    };
    let entry_count = entries.len() + 1;
    Ok(BundlePlan {
        request: Value::Object(Map::from_iter([
            ("entries".to_owned(), Value::Array(entries)),
            ("pdf_collections".to_owned(), json!([])),
            ("manifest".to_owned(), Value::Object(manifest_map)),
            ("output".to_owned(), json!(input.output)),
        ])),
        manifest,
        prefix: format!("compounded-{}", &fingerprint[..16]),
        input_fingerprint: fingerprint,
        entry_count,
    })
}

/// The Reporter's bundle receipt, admitted against the plan.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct BundleReceipt {
    pub output: String,
    pub bytes: u64,
    pub sha256: String,
    pub entries: usize,
}

pub fn admit_receipt(plan: &BundlePlan, document: &Value) -> Result<BundleReceipt, Refusal> {
    let invalid = |message: &str| Refusal::new(INPUTS_INVALID, message.to_owned());
    if document["task"] != "export_compounded_report" || document["status"] != "completed" {
        return Err(invalid(
            "the Reporter did not complete the compounded bundle task",
        ));
    }
    let receipt: BundleReceipt = serde_json::from_value(document.clone())
        .map_err(|_| invalid("the Reporter bundle receipt is malformed"))?;
    if receipt.output != plan.request["output"].as_str().unwrap_or_default()
        || receipt.entries != plan.entry_count
        || !hex(&receipt.sha256)
        || receipt.bytes == 0
    {
        return Err(invalid(
            "the Reporter bundle receipt is not this plan's archive",
        ));
    }
    if receipt.bytes > MAX_PUBLISHED_BYTES {
        return Err(Refusal::new(
            ARCHIVE_TOO_LARGE,
            format!(
                "the archive is {} bytes; the project publication door admits at most {MAX_PUBLISHED_BYTES}",
                receipt.bytes
            ),
        ));
    }
    Ok(receipt)
}

#[cfg(test)]
#[path = "compounded_tests.rs"]
mod tests;
