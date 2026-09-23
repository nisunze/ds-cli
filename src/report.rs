//! Offline report command decisions. Hosts own IO; export never requires sync.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub mod activity;
pub mod artifacts;

pub const DEVELOPMENT_ENGINE: &str = "ds-network-reporter@0.1.0+development";
pub const LOCAL_ONLY_SCHEMA: u32 = 4;

fn valid_semver_core(value: &str) -> bool {
    let mut parts = value.split('.');
    (0..3).all(|_| {
        parts.next().is_some_and(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        })
    }) && parts.next().is_none()
}

/// Exact immutable Network Reporter identity: package semantic version plus
/// the producing source commit. Package upgrades must not strand retained
/// publications merely because they no longer use the current version.
pub fn valid_network_reporter_engine_identity(engine: &str) -> bool {
    let Some(identity) = engine.strip_prefix("ds-network-reporter@") else {
        return false;
    };
    let Some((version, source)) = identity.split_once('+') else {
        return false;
    };
    valid_semver_core(version)
        && source.len() == 40
        && source
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PublicationState {
    LocalOnly,
    Composed,
    Pending,
    Queued,
}

/// The one publication lifecycle a reader sees, for any producer.
///
/// The defect this exists to close: a verified artifact sitting on a PC was
/// indistinguishable from a report that never ran. Three surfaces answered the
/// same question differently — the engine said `ok`, the outbox said
/// `pending`, the cloud said `stale` — because none of them named the *stage*
/// a publication had reached. These four names are that stage, and every
/// producer (Desktop, headless CLI, Server) reports in them.
///
/// Why this is a derivation rather than a rename of
/// [`crate::sync_store::ArtifactState`]: that enum is a persisted SQLite
/// column (`artifacts.state`) and the report activity projection validates its
/// five tokens. Renaming it is a data migration that would strand every queue
/// already on disk. The stage is a *reader's* vocabulary; it is computed from
/// facts the store already holds, one-to-one and losslessly, so nothing has to
/// be migrated and nothing can drift.
///
/// `Sealed` is the state the defect made invisible: bytes are committed and
/// verified in the producer store, and no queue row exists for them yet.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStage {
    /// Committed and digest-verified on this machine; not yet a queue row.
    Sealed,
    /// A queue row exists and is waiting for its turn at the gateway.
    Queued,
    /// Bytes are moving, or have moved, and the head is not confirmed yet.
    Drained,
    /// The shared record answered with a head revision. This is done.
    Acknowledged,
    /// The record moved past what these bytes were computed on. Terminal until
    /// a human or a newer computation resolves it.
    Conflicted,
    /// The gateway refused these bytes by name. Terminal.
    Refused,
}

impl PublicationStage {
    /// The token this stage is spelled with on every surface.
    pub fn as_str(self) -> &'static str {
        match self {
            PublicationStage::Sealed => "sealed",
            PublicationStage::Queued => "queued",
            PublicationStage::Drained => "drained",
            PublicationStage::Acknowledged => "acknowledged",
            PublicationStage::Conflicted => "conflicted",
            PublicationStage::Refused => "refused",
        }
    }

    /// Whether an operator still has work owed to them for this publication.
    /// `Sealed`, `Queued` and `Drained` are all "not yet up there".
    pub fn outstanding(self) -> bool {
        matches!(
            self,
            PublicationStage::Sealed | PublicationStage::Queued | PublicationStage::Drained
        )
    }

    /// One sentence an operator reads, naming the stage and what it implies.
    pub fn sentence(self) -> &'static str {
        match self {
            PublicationStage::Sealed => {
                "computed and verified on this device; not queued for publication yet"
            }
            PublicationStage::Queued => "queued for publication; waiting for the sync pump",
            PublicationStage::Drained => "uploading; the shared record has not confirmed it yet",
            PublicationStage::Acknowledged => "published; the shared record carries it",
            PublicationStage::Conflicted => {
                "the shared record moved past what this result was computed on"
            }
            PublicationStage::Refused => "the gateway refused these bytes",
        }
    }
}

/// The stage a queue row is at, from the persisted artifact-state token.
///
/// The tokens are [`crate::sync_store::ArtifactState::as_str`]'s, read as data
/// rather than matched on the enum so that a store written by an older build
/// still reads. An unknown token is not guessed: the caller is told, because
/// inventing a stage is exactly how three surfaces came to disagree.
pub fn publication_stage(state: &str) -> Result<PublicationStage, String> {
    match state {
        "held" => Ok(PublicationStage::Queued),
        "uploading" => Ok(PublicationStage::Drained),
        "published" => Ok(PublicationStage::Acknowledged),
        "conflict" => Ok(PublicationStage::Conflicted),
        "refused" => Ok(PublicationStage::Refused),
        other => Err(format!("unknown publication state `{other}`")),
    }
}

pub fn publication_state(
    engine: &str,
    enqueue: bool,
    queued: bool,
) -> Result<PublicationState, String> {
    if engine == DEVELOPMENT_ENGINE {
        return Ok(PublicationState::LocalOnly);
    }
    if !valid_network_reporter_engine_identity(engine) {
        return Err("Report engine identity is invalid".into());
    }
    Ok(if !enqueue {
        PublicationState::Composed
    } else if queued {
        PublicationState::Queued
    } else {
        PublicationState::Pending
    })
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Activity {
        snapshot: activity::Snapshot,
    },
    MapPlan {
        request: crate::printing::map::Request,
        boundaries: Vec<crate::printing::custom::Boundary>,
        sheets: serde_json::Value,
    },
    MapRequest {
        request: crate::printing::map::Request,
    },
    MapModelRoom {
        owner: String,
        project: String,
        model: crate::printing::map::Model,
    },
    MapMvLayers {
        projection: serde_json::Value,
        model: crate::printing::map::Model,
        bounds: [f64; 4],
    },
    MapCurrent {
        rows: Vec<serde_json::Value>,
        item: serde_json::Value,
    },
    MapCatalog {
        rows: Vec<serde_json::Value>,
        item: serde_json::Value,
    },
    MapLayers {
        area: serde_json::Value,
        sources: std::collections::BTreeMap<String, serde_json::Value>,
    },
    CustomAreaPlan {
        project: String,
        id: String,
        codes: Vec<String>,
    },
    CustomArea {
        project: String,
        id: String,
        codes: Vec<String>,
        boundaries: Vec<crate::printing::custom::Boundary>,
        #[serde(default)]
        include_geometry: bool,
    },
    Export {
        project: String,
        transformer: String,
        #[serde(default)]
        force: bool,
        #[serde(default)]
        report_type: Option<String>,
        #[serde(default)]
        transformers: Vec<String>,
        #[serde(default)]
        selection: Option<crate::report_formats::DesignOutputSelection>,
        /// The project the host has open. A project overview (combined or
        /// compounded) is drawn from that project's rooms, so it must be the
        /// one asked for; a single transformer export is not so bound.
        #[serde(default)]
        active_project: Option<String>,
    },
    /// The outcome of a batch of exports, in a stable order with its failure
    /// count — the shaping every batch host repeated.
    BatchOutcome {
        results: Vec<serde_json::Value>,
    },
    FormatOptions {
        #[serde(default)]
        setups: Vec<serde_json::Value>,
    },
    Formats {
        sheets: serde_json::Value,
        combined: bool,
        #[serde(default)]
        selection: Option<crate::report_formats::DesignOutputSelection>,
    },
    ContextPreparation {
        sheets: serde_json::Value,
        online: bool,
        #[serde(default)]
        selection: Option<crate::report_formats::DesignOutputSelection>,
    },
    Publication {
        engine: String,
        enqueue: bool,
        queued: bool,
    },
    Plan {
        requested: Option<Vec<String>>,
        inventory: Option<Vec<String>>,
        rooms: Vec<Room>,
        processors: usize,
        resident_limit: usize,
    },
    Archive {
        transformers: Vec<String>,
        purpose: String,
        lifecycle: Option<String>,
        groups: Vec<Group>,
    },
    /// How a compounded archive is laid out — the level vocabulary, its legacy
    /// spelling, and the keys an operator reads it under.
    ArchiveLayout {
        #[serde(default)]
        layout: ArchiveLayout,
    },
    /// What is this artifact called? One answer for a local outbox file and for
    /// a signed storage locator.
    ArtifactName {
        #[serde(default)]
        locator: String,
    },
    ArtifactOutput {
        output_id: String,
    },
    ArtifactInventory {
        artifacts: Vec<artifacts::Artifact>,
        combined: bool,
        #[serde(default)]
        inventory: bool,
        #[serde(default)]
        require_combined_base: bool,
    },
    /// What are one version's artifacts, and in what order are they read?
    ArtifactGroups {
        #[serde(default)]
        locators: Vec<String>,
    },
    /// What is one stored version called, inside and out?
    CombinedEntryNaming {
        #[serde(default)]
        display_name: String,
        #[serde(default)]
        version_label: String,
        #[serde(default)]
        prefix: String,
    },
}

/// The archive layout as a registry entry or a dialog carries it.
///
/// Two spellings describe one choice: `file_level` is current, and
/// `transformer_grouping` is the legacy twin older archives were written with.
/// Both, plus the default, were folded in one expression inside a Svelte
/// component — so an archive written by the command line and one written by the
/// dialog could be described to an operator under different words. The fold
/// lives here now, and it answers in keys.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveLayout {
    #[serde(default)]
    pub file_level: Option<String>,
    /// Legacy spelling accepted for archives and clients written before
    /// `file_level`.
    #[serde(default)]
    pub transformer_grouping: Option<String>,
    #[serde(default)]
    pub combine_per_district: Option<bool>,
    #[serde(default)]
    pub combine_per_group: Option<bool>,
    /// How many of the applied plan's ordered levels the tree descends before
    /// files are placed. `None` means every level the plan has; `Some(0)` is the
    /// archive root. This and `transformer_folders` are the GENERIC knobs
    /// ds-brain writes onto every registry entry it records, so they arrive here
    /// on any archive written by a current brain — decoding them is what keeps
    /// `deny_unknown_fields` from rejecting the whole entry.
    #[serde(default)]
    pub group_depth: Option<u32>,
    /// Whether each transformer's files sit in their own folder under the
    /// deepest group level reached.
    #[serde(default)]
    pub transformer_folders: Option<bool>,
}

/// Every level a compounded archive may nest to, in nesting order. `flat` and
/// `district_sector` are the legacy twin's spellings of the ends of the same
/// scale.
pub const ARCHIVE_LEVELS: &[&str] = &["root", "district", "sector", "district_sector", "flat"];

/// The label key a level is read under. Two levels deliberately share one:
/// `district_sector` IS a sector-level archive under the legacy spelling, and
/// anything this module does not recognise is the default, transformer.
pub fn archive_level_key(level: &str) -> &'static str {
    match level {
        "root" => "pctl_layout_root",
        "district" => "pctl_layout_district",
        "sector" | "district_sector" => "pctl_layout_sector",
        _ => "pctl_layout_transformer",
    }
}

impl ArchiveLayout {
    /// The level this layout names: the generic knobs when they were authored,
    /// then the current spelling, then the legacy twin, then the default. One
    /// expression, one answer.
    ///
    /// The generic pair wins first because that is the order ds-brain's own
    /// `shape()` resolves in: once a depth or a folder flag is authored it
    /// clears the legacy spellings, so an entry carrying both is describing its
    /// tree with the generic pair and merely keeping the legacy words for older
    /// readers.
    pub fn level(&self) -> &str {
        if let Some(level) = self.generic_level() {
            return level;
        }
        self.file_level
            .as_deref()
            .filter(|value| !value.is_empty())
            .or(self
                .transformer_grouping
                .as_deref()
                .filter(|value| !value.is_empty()))
            .unwrap_or("transformer")
    }

    /// The level the generic depth/folder pair names, or `None` when neither
    /// knob was authored. This is `compoundedLegacyShape` read backwards: depth
    /// 0 is the root, depth 1 is the first grouping level, anything deeper is
    /// the sector end of the scale, and "every level" is a per-transformer tree
    /// exactly when files sit in transformer folders.
    fn generic_level(&self) -> Option<&'static str> {
        if self.group_depth.is_none() && self.transformer_folders.is_none() {
            return None;
        }
        let folders = self.transformer_folders.unwrap_or(true);
        Some(match (self.group_depth, folders) {
            // Depth 0 with transformer folders is the legacy `flat` tree: one
            // folder per transformer at the archive root.
            (Some(0), true) => "flat",
            (Some(0), false) => "root",
            (Some(1), _) => "district",
            (Some(_), _) => "sector",
            (None, true) => "transformer",
            (None, false) => "sector",
        })
    }

    pub fn describe(&self) -> serde_json::Value {
        let level = self.level();
        let level_key = archive_level_key(level);
        let combine_per_district = self.combine_per_district == Some(true);
        serde_json::json!({
            "level": level,
            "level_key": level_key,
            "combine_per_district": combine_per_district,
            "combine_per_group": self.combine_per_group == Some(true),
            // The phrase the operator reads: the level alone, or the
            // per-district phrase taking that level as its parameter.
            "label_key": if combine_per_district { "pctl_layout_per_district" } else { level_key },
            "label_level_key": if combine_per_district { Some(level_key) } else { None },
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Room {
    name: String,
    available: bool,
    dirty: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Group {
    group_key: String,
    transformers: Vec<String>,
}
/// Names a report *produces*: the collision document and the combined set
/// under any of its spellings. They are output, never a participant, so no
/// lane may take one as a requested transformer. One vocabulary decides it —
/// `design::row_allows(name).reportable` — so an alias or a cased spelling
/// cannot smuggle one in and the register cannot disagree about it.
pub fn is_reserved_report_identity(name: &str) -> bool {
    !crate::design::row_allows(name).reportable
}
fn names(values: Vec<String>) -> Result<BTreeSet<String>, String> {
    if values.len() > 2000 {
        return Err("Report scope exceeds 2000 rooms".into());
    }
    let mut result = BTreeSet::new();
    for name in values {
        if name.is_empty()
            || name.len() > 121
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || is_reserved_report_identity(&name)
        {
            return Err(format!("Invalid report transformer: {name}"));
        }
        result.insert(name);
    }
    Ok(result)
}
/// Input batches are ordered newest first. Preserve every unselected canonical
/// output in its original batch; never copy its provenance into a newer run.
pub fn current_output_indices(batches: &[Vec<String>]) -> Vec<Vec<usize>> {
    let mut seen = BTreeSet::new();
    batches
        .iter()
        .map(|outputs| {
            outputs
                .iter()
                .enumerate()
                .filter_map(|(index, key)| seen.insert(key).then_some(index))
                .collect()
        })
        .collect()
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 64 * 1024 * 1024 {
        return Err("Report command exceeds 64 MiB".into());
    }
    let command: Command = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let result = match command {
        Command::Activity { snapshot } => serde_json::to_value(activity::project(&snapshot)?)
            .map_err(|error| error.to_string())?,
        Command::MapModelRoom {
            owner,
            project,
            model,
        } => crate::printing::map::model_room(&owner, &project, model)?,
        Command::MapMvLayers {
            projection,
            model,
            bounds,
        } => crate::printing::map::mv_layers(projection, model, bounds)?,
        Command::MapCurrent { rows, item } => {
            serde_json::json!(crate::printing::map::current(&rows, &item))
        }
        Command::MapCatalog { rows, item } => crate::printing::map::catalog(rows, item)?,
        Command::MapRequest { request } => {
            crate::printing::map::validate_request(&request)?;
            serde_json::to_value(request).map_err(|e| e.to_string())?
        }
        Command::MapPlan {
            request,
            boundaries,
            sheets,
        } => crate::printing::map::plan(&request, boundaries, &sheets)?,
        Command::MapLayers { area, sources } => {
            crate::printing::map::select_layers(&area, &sources)?
        }
        Command::CustomAreaPlan { project, id, codes } => {
            crate::printing::custom::plan_area(&project, &id, &codes)?
        }
        Command::CustomArea {
            project,
            id,
            codes,
            boundaries,
            include_geometry,
        } => crate::printing::custom::area_result(
            crate::printing::custom::prepare_area(&project, &id, &codes, boundaries)?,
            include_geometry,
        ),
        Command::BatchOutcome { results } => {
            let mut results = results;
            results.sort_by(|a, b| {
                crate::design_status_query::locale_cmp(
                    a["transformer"].as_str().unwrap_or(""),
                    b["transformer"].as_str().unwrap_or(""),
                )
            });
            let failed = results
                .iter()
                .filter(|row| row["status"].as_str() == Some("error"))
                .count();
            serde_json::json!({"results": results, "failed": failed})
        }
        Command::Export {
            project,
            transformer,
            force,
            report_type,
            transformers,
            selection,
            active_project,
        } => {
            if project.is_empty()
                || project.len() > 128
                || project.trim() != project
                || !project
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err("Report export requires one exact project id.".into());
            }
            let kind = if transformer == "combined_transformer" {
                match report_type.as_deref() {
                    None | Some("combined") => "combined",
                    Some("compounded_reports") => "compounded",
                    _ => return Err("Unsupported report type.".into()),
                }
            } else {
                names(vec![transformer.clone()])?;
                if report_type.is_some() || !transformers.is_empty() {
                    return Err("A transformer export cannot also name a combined scope.".into());
                }
                "transformer"
            };
            if names(transformers.clone())?.len() != transformers.len() {
                return Err("Report scope repeats a transformer.".into());
            }
            if kind != "transformer" && active_project.as_deref() != Some(project.as_str()) {
                return Err("Project overview requires the matching active project.".into());
            }
            let label = match kind {
                "combined" => "Combined report".into(),
                "compounded" => "Compounded report".into(),
                _ => format!("Report · {transformer}"),
            };
            let mut plan = serde_json::json!({"project":project,"transformer":transformer,"force":force,"transformers":transformers,"kind":kind,"label":label,"reuse_prepared":true});
            if let Some(selection) = selection {
                if kind != "transformer" {
                    return Err("Local output overrides require individual transformers".into());
                }
                let tokens = selection.tokens()?;
                if tokens.is_empty() || tokens.len() > 32 {
                    return Err("Local output selection must contain 1..32 outputs".into());
                }
                plan["selection"] = serde_json::to_value(selection).map_err(|e| e.to_string())?;
            }
            plan
        }
        Command::FormatOptions { setups } => {
            serde_json::json!({"options": crate::report_formats::options(&setups)?})
        }
        Command::Formats {
            sheets,
            combined,
            selection,
        } => {
            serde_json::json!({"outputs": crate::report_formats::selected_plan(&sheets, combined, selection.as_ref())?})
        }
        Command::ContextPreparation {
            sheets,
            online,
            selection,
        } => {
            let sheets = sheets.get("sheets").unwrap_or(&sheets);
            let outputs = crate::report_formats::selected_plan(sheets, false, selection.as_ref())?;
            let layouts = crate::report_formats::named_layouts(sheets)?;
            let mut seen = BTreeSet::new();
            let mut contexts = Vec::new();
            let mut hidden = Vec::new();
            for output in outputs {
                if let Some((_, id)) = crate::report_formats::named_print_output(&output.format) {
                    let layout = &layouts[id];
                    for context in &layout.context_layers {
                        // A source the layout hides entirely prints nothing:
                        // it is named, never acquired and never counted.
                        if !crate::printing::context::prints_in(layout, context) {
                            hidden.push(crate::printing::context::hidden_context(id, context));
                            continue;
                        }
                        let value = serde_json::to_value(context).map_err(|e| e.to_string())?;
                        if seen.insert(serde_json::to_string(&value).map_err(|e| e.to_string())?) {
                            contexts.push(value);
                        }
                    }
                }
            }
            // Every export ensures its selected inputs. Acquisition happens
            // before the renderer; offline hosts can only inspect held data.
            serde_json::json!({"contexts": contexts, "acquire_missing": online, "hidden": hidden})
        }
        Command::Publication {
            engine,
            enqueue,
            queued,
        } => serde_json::json!({"state": publication_state(&engine, enqueue, queued)?}),
        Command::Plan {
            requested,
            inventory,
            rooms,
            processors,
            resident_limit,
        } => {
            if processors == 0 || resident_limit == 0 || resident_limit > 64 {
                return Err("Invalid report concurrency budget".into());
            }
            if rooms.len() > 2000 {
                return Err("Report room inventory exceeds 2000".into());
            }
            let scope = match requested {
                Some(values) if !values.is_empty() => names(values)?,
                _ => {
                    let mut scope = names(inventory.ok_or("Prepare the complete project inventory before an all-project offline report")?)?;
                    scope.extend(names(
                        rooms
                            .iter()
                            .filter(|r| r.dirty)
                            .map(|r| r.name.clone())
                            .collect(),
                    )?);
                    scope
                }
            };
            let available: BTreeSet<_> = rooms
                .iter()
                .filter(|r| r.available)
                .map(|r| r.name.as_str())
                .collect();
            let missing: Vec<_> = scope
                .iter()
                .filter(|n| !available.contains(n.as_str()))
                .cloned()
                .collect();
            let concurrency = scope.len().min(processors).min(resident_limit);
            serde_json::json!({"names":scope,"missing":missing,"concurrency":concurrency})
        }
        Command::Archive {
            transformers,
            purpose,
            lifecycle,
            groups,
        } => {
            if purpose != "report_archive" || lifecycle.as_deref() == Some("archived") {
                return Err(
                    "Apply one active report_archive consumer grouping before compounding reports"
                        .into(),
                );
            }
            let expected = names(transformers)?;
            let mut seen = BTreeSet::new();
            let mut output = Vec::new();
            for (index, group) in groups.into_iter().enumerate() {
                for name in &group.transformers {
                    if !expected.contains(name) || !seen.insert(name.clone()) {
                        return Err("The report_archive grouping must cover each compounded transformer exactly once".into());
                    }
                }
                output.push(serde_json::json!({"folder":format!("groups/group-{:03}",index+1),"group_key":group.group_key,"transformers":group.transformers}));
            }
            if seen != expected {
                return Err(
                    "The report_archive grouping is stale for the compounded transformer inventory"
                        .into(),
                );
            }
            serde_json::json!({"groups":output})
        }
        Command::ArchiveLayout { layout } => layout.describe(),
        Command::ArtifactOutput { output_id } => {
            serde_json::json!({"output": artifacts::output(&output_id)})
        }
        Command::ArtifactInventory {
            artifacts: items,
            combined,
            inventory,
            require_combined_base,
        } => {
            serde_json::json!({"artifacts": artifacts::validate(items, combined, inventory, require_combined_base)?})
        }
        Command::ArtifactName { locator } => {
            let filename = crate::report_formats::artifact_filename(&locator);
            let opens_as_pdf = crate::report_formats::artifact_opens_as_pdf(&filename);
            let kind = crate::report_formats::artifact_kind(&filename);
            serde_json::json!({"filename": filename, "opens_as_pdf": opens_as_pdf, "kind": kind})
        }
        Command::ArtifactGroups { locators } => {
            let groups: Vec<serde_json::Value> = crate::report_formats::artifact_groups(&locators)
                .into_iter()
                .map(|(kind, items)| serde_json::json!({"kind": kind, "items": items}))
                .collect();
            serde_json::json!({ "groups": groups })
        }
        Command::CombinedEntryNaming {
            display_name,
            version_label,
            prefix,
        } => {
            let (label, folder, archive_name) = crate::report_formats::combined_entry_naming(
                &display_name,
                &version_label,
                &prefix,
            );
            serde_json::json!({"label": label, "folder": folder, "archive_name": archive_name})
        }
    };
    serde_json::to_string(&serde_json::json!({"schema":"ds.report.command/v1","result":result}))
        .map_err(|e| e.to_string())
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = reportCommand)]
pub fn report_command_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    evaluate(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The whole persisted queue vocabulary maps onto a stage, and nothing is
    /// invented. This matters because the blocker was three surfaces answering
    /// "is this room's report current?" differently: if one token here had no
    /// stage, or an unknown token guessed one, that disagreement would be back.
    #[test]
    fn every_persisted_queue_token_names_exactly_one_stage() {
        let table = [
            ("held", PublicationStage::Queued),
            ("uploading", PublicationStage::Drained),
            ("published", PublicationStage::Acknowledged),
            ("conflict", PublicationStage::Conflicted),
            ("refused", PublicationStage::Refused),
        ];
        for (token, expected) in table {
            assert_eq!(publication_stage(token).unwrap(), expected, "{token}");
        }
        assert!(publication_stage("drained").is_err());
        assert!(publication_stage("").is_err());
    }

    /// `sealed` is the stage the defect erased: 593 verified artifacts on one
    /// PC read as "never ran". It is outstanding work and must never be
    /// reachable from a queue token, because a sealed batch is precisely one
    /// that has no queue row.
    #[test]
    fn sealed_is_outstanding_and_is_not_a_queue_token() {
        assert!(PublicationStage::Sealed.outstanding());
        assert!(PublicationStage::Queued.outstanding());
        assert!(PublicationStage::Drained.outstanding());
        assert!(!PublicationStage::Acknowledged.outstanding());
        assert!(!PublicationStage::Conflicted.outstanding());
        assert!(!PublicationStage::Refused.outstanding());
        for token in ["held", "uploading", "published", "conflict", "refused"] {
            assert_ne!(publication_stage(token).unwrap(), PublicationStage::Sealed);
        }
        assert_eq!(
            serde_json::to_value(PublicationStage::Sealed).unwrap(),
            serde_json::json!("sealed")
        );
        assert_eq!(PublicationStage::Sealed.as_str(), "sealed");
    }

    /// Five level tokens, four labels, one legacy twin and one default — the
    /// whole vocabulary, asserted as a table so a new spelling has to be added
    /// here before it can be read anywhere.
    #[test]
    fn the_archive_layout_vocabulary_is_five_tokens_and_four_labels() {
        let describe = |value: serde_json::Value| -> serde_json::Value {
            let request = serde_json::json!({"command":"archive_layout","layout":value});
            let reply: serde_json::Value =
                serde_json::from_str(&evaluate(request.to_string().as_bytes()).unwrap()).unwrap();
            reply["result"].clone()
        };
        for (layout, level, key) in [
            (
                serde_json::json!({"file_level":"root"}),
                "root",
                "pctl_layout_root",
            ),
            (
                serde_json::json!({"file_level":"district"}),
                "district",
                "pctl_layout_district",
            ),
            (
                serde_json::json!({"file_level":"sector"}),
                "sector",
                "pctl_layout_sector",
            ),
            (
                serde_json::json!({"transformer_grouping":"district_sector"}),
                "district_sector",
                "pctl_layout_sector",
            ),
            (
                serde_json::json!({"file_level":"transformer"}),
                "transformer",
                "pctl_layout_transformer",
            ),
            // The legacy twin's own flat spelling and the empty layout both
            // land on the default, exactly as the archive registry reads them.
            (
                serde_json::json!({"transformer_grouping":"flat"}),
                "flat",
                "pctl_layout_transformer",
            ),
            (
                serde_json::json!({}),
                "transformer",
                "pctl_layout_transformer",
            ),
            // The generic depth/folder pair ds-brain authors, read back as the
            // same levels their legacy spellings named.
            (
                serde_json::json!({"group_depth":0,"transformer_folders":false}),
                "root",
                "pctl_layout_root",
            ),
            (
                serde_json::json!({"group_depth":0,"transformer_folders":true}),
                "flat",
                "pctl_layout_transformer",
            ),
            (
                serde_json::json!({"group_depth":1,"transformer_folders":false}),
                "district",
                "pctl_layout_district",
            ),
            (
                serde_json::json!({"group_depth":2,"transformer_folders":false}),
                "sector",
                "pctl_layout_sector",
            ),
            (
                serde_json::json!({"transformer_folders":false}),
                "sector",
                "pctl_layout_sector",
            ),
            (
                serde_json::json!({"transformer_folders":true}),
                "transformer",
                "pctl_layout_transformer",
            ),
        ] {
            let answer = describe(layout);
            assert_eq!(answer["level"], level);
            assert_eq!(answer["level_key"], key);
            assert_eq!(answer["label_key"], key);
            assert_eq!(answer["label_level_key"], serde_json::Value::Null);
        }
        // A registry entry exactly as ds-brain's AsRegistryField writes it:
        // BOTH spellings on one document. This is the shape that made the app
        // fail closed with "unknown field `group_depth`" — every field must
        // decode, and the generic pair must win over the legacy words it
        // deliberately keeps alongside for older readers.
        let from_brain = describe(serde_json::json!({
            "group_depth": 1,
            "transformer_folders": false,
            "combine_per_group": false,
            "combine_per_district": false,
            "file_level": "transformer",
            "transformer_grouping": "district_sector",
        }));
        assert_eq!(from_brain["level"], "district");
        assert_eq!(from_brain["level_key"], "pctl_layout_district");
        // And the same entry with no depth: "every level the plan has", which is
        // the per-transformer tree when files sit in transformer folders.
        let every_level = describe(serde_json::json!({
            "transformer_folders": true,
            "combine_per_group": true,
            "combine_per_district": true,
            "transformer_grouping": "district_sector",
        }));
        assert_eq!(every_level["level"], "transformer");
        assert_eq!(every_level["combine_per_group"], true);
        // The current spelling wins over the legacy twin when both are present.
        let both = describe(
            serde_json::json!({"file_level":"root","transformer_grouping":"district_sector"}),
        );
        assert_eq!(both["level"], "root");
        // Combining per district wraps the level rather than replacing it.
        let combined =
            describe(serde_json::json!({"file_level":"sector","combine_per_district":true}));
        assert_eq!(combined["label_key"], "pctl_layout_per_district");
        assert_eq!(combined["label_level_key"], "pctl_layout_sector");
        assert_eq!(combined["combine_per_district"], true);
        assert!(ARCHIVE_LEVELS.contains(&"district_sector"));
        // The artifact name rides the same command surface, so a card and a
        // headless caller ask one question of one owner.
        let reply: serde_json::Value = serde_json::from_str(
            &evaluate(
                serde_json::json!({"command":"artifact_name","locator":"https://h/v0/b/x/o/a%2Fb.xlsx?alt=media"})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(reply["result"]["filename"], "b.xlsx");
        assert_eq!(reply["result"]["opens_as_pdf"], false);
        let reply: serde_json::Value = serde_json::from_str(
            &evaluate(
                serde_json::json!({"command":"artifact_name","locator":"https://h/v0/b/x/o/a%2FB.PDF?alt=media"})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(reply["result"]["filename"], "B.PDF");
        assert_eq!(reply["result"]["opens_as_pdf"], true);
        assert_eq!(reply["result"]["kind"], "other");
    }

    #[test]
    fn a_version_reads_its_files_under_one_vocabulary_in_one_order() {
        let command = |value: serde_json::Value| -> serde_json::Value {
            serde_json::from_str(&evaluate(value.to_string().as_bytes()).unwrap()).unwrap()
        };
        // `.shp.zip` is a shapefile; a plain `.zip` is an archive. The browser
        // drew that line itself, beside the colours it drew it for.
        for (locator, kind) in [
            ("a/b.xlsx", "excel"),
            ("a/b.XLS", "excel"),
            ("a/b.shp.zip", "shapefile"),
            ("a/b.shp.7z", "shapefile"),
            ("a/b.kmz", "kmz"),
            ("a/b.gpkg", "gpkg"),
            ("a/b.zip", "zip"),
            ("a/b.pdf", "other"),
            ("a/b", "other"),
        ] {
            let reply = command(serde_json::json!({"command":"artifact_name","locator":locator}));
            assert_eq!(reply["result"]["kind"], kind, "{locator}");
        }
        // Groups are positions, in reading order, and a kind nothing matched
        // is not an empty badge.
        let reply = command(serde_json::json!({"command":"artifact_groups","locators":[
            "p/one.zip", "p/two.xlsx", "p/three.shp.zip", "p/four.xlsx", "p/five.txt",
        ]}));
        assert_eq!(
            reply["result"]["groups"],
            serde_json::json!([
                {"kind":"excel","items":[1,3]},
                {"kind":"shapefile","items":[2]},
                {"kind":"zip","items":[0]},
                {"kind":"other","items":[4]},
            ])
        );
        assert_eq!(
            command(serde_json::json!({"command":"artifact_groups"}))["result"]["groups"],
            serde_json::json!([])
        );
    }

    #[test]
    fn a_stored_version_is_named_once_inside_and_out() {
        let naming = |value: serde_json::Value| -> serde_json::Value {
            let mut request = value;
            request["command"] = "combined_entry_naming".into();
            serde_json::from_str::<serde_json::Value>(
                &evaluate(request.to_string().as_bytes()).unwrap(),
            )
            .unwrap()["result"]
                .clone()
        };
        // Display name, then the published label, then the prefix.
        let named =
            naming(serde_json::json!({"display_name":"Rev B","version_label":"v2","prefix":"p1"}));
        assert_eq!(named["label"], "Rev B");
        assert_eq!(named["folder"], "Rev B");
        assert_eq!(named["archive_name"], "p1.zip");
        assert_eq!(
            naming(serde_json::json!({"version_label":"v2","prefix":"p1"}))["label"],
            "v2"
        );
        assert_eq!(naming(serde_json::json!({"prefix":"p1"}))["label"], "p1");
        // A run of characters a filesystem refuses is ONE separator.
        let refused = naming(serde_json::json!({"display_name":"a//b:*c","prefix":"p1"}));
        assert_eq!(refused["label"], "a//b:*c");
        assert_eq!(refused["folder"], "a_b_c");
        // A name that is nothing but refused characters is the prefix again,
        // never an archive rooted at `/`.
        assert_eq!(
            naming(serde_json::json!({"display_name":"//","prefix":"p1"}))["folder"],
            "_"
        );
        assert_eq!(
            naming(serde_json::json!({"display_name":"","version_label":"","prefix":""}))["folder"],
            ""
        );
    }
    #[test]
    fn export_and_publication_are_separate() {
        let release = format!("ds-network-reporter@0.1.0+{}", "a".repeat(40));
        assert_eq!(
            publication_state(&release, true, false).unwrap(),
            PublicationState::Pending
        );
        assert_eq!(
            publication_state(&release, true, true).unwrap(),
            PublicationState::Queued
        );
        assert_eq!(
            publication_state(&release, false, false).unwrap(),
            PublicationState::Composed
        );
        assert_eq!(
            publication_state(DEVELOPMENT_ENGINE, true, true).unwrap(),
            PublicationState::LocalOnly
        );
        assert_eq!(
            publication_state(
                &format!("ds-network-reporter@0.2.1+{}", "b".repeat(40)),
                true,
                false,
            )
            .unwrap(),
            PublicationState::Pending,
        );
        assert!(publication_state("unknown", true, false).is_err());
        assert!(publication_state(
            &format!("ds-network-reporter@01.2.1+{}", "b".repeat(40)),
            true,
            false,
        )
        .is_err());
    }
    #[test]
    fn computed_identities_are_reserved_under_any_spelling() {
        assert!(is_reserved_report_identity("Collisions"));
        assert!(is_reserved_report_identity("all_transformers"));
        assert!(is_reserved_report_identity(" Combined-Transformers "));
        assert!(!is_reserved_report_identity("tx_a"));
        let input = serde_json::json!({"command":"plan","requested":["collisions"],"inventory":null,"rooms":[],"processors":32,"resident_limit":4});
        assert!(evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
    }
    #[test]
    fn offline_inventory_never_silently_shrinks() {
        let input = serde_json::json!({"command":"plan","requested":null,"inventory":["tx_a","tx_b"],"rooms":[{"name":"tx_a","available":true,"dirty":false},{"name":"tx_c","available":true,"dirty":true}],"processors":32,"resident_limit":4});
        let result: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
        assert_eq!(
            result["result"]["names"],
            serde_json::json!(["tx_a", "tx_b", "tx_c"])
        );
        assert_eq!(result["result"]["missing"], serde_json::json!(["tx_b"]));
        assert_eq!(result["result"]["concurrency"], 3);
        let mut absent = input;
        absent["inventory"] = serde_json::Value::Null;
        assert!(evaluate(&serde_json::to_vec(&absent).unwrap()).is_err());
        absent["requested"] = serde_json::json!(["tx_a"]);
        assert!(evaluate(&serde_json::to_vec(&absent).unwrap()).is_ok());
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn iterations_overwrite_only_selected_outputs_preserving_original_batches() {
        let batches = vec![
            vec!["pdf__a3".into()],
            vec!["xlsx".into()],
            vec!["pdf__a0".into(), "pdf__a3".into(), "xlsx".into()],
            vec!["pdf__a3".into()],
        ];
        assert_eq!(
            current_output_indices(&batches),
            vec![vec![0], vec![0], vec![0], vec![]]
        );
    }

    #[test]
    fn printing_ensures_selected_context_once_and_acquires_only_online() {
        let mut first = crate::printing::default_layout();
        first.id = "a3".into();
        first
            .context_layers
            .push(crate::printing::PrintContextLayer {
                id: "roads".into(),
                label: "Roads".into(),
                source: crate::printing::PrintContextSource::Catalog {
                    country: "Rwanda".into(),
                    layer: "roads".into(),
                },
            });
        let mut second = first.clone();
        second.id = "a0".into();
        let mut unused = first.clone();
        unused.id = "unused".into();
        unused.context_layers[0].id = "not-selected".into();
        let setups = [first, second, unused]
            .map(|layout| json!({"id":layout.id,"revision":"a".repeat(64),"layout":layout}));
        let sheets = json!({"project_settings":[{"parameter":"design_export_format","value":["pdf__a3","png__a3","pdf__a0"]}], "printing_setups":setups});
        for online in [true, false] {
            let output: serde_json::Value = serde_json::from_str(
                &evaluate(
                    &serde_json::to_vec(
                        &json!({"command":"context_preparation","sheets":sheets,"online":online}),
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(output["result"]["acquire_missing"], online);
            assert_eq!(output["result"]["contexts"].as_array().unwrap().len(), 1);
            assert_eq!(output["result"]["contexts"][0]["id"], "roads");
        }
        let mut missing = sheets;
        missing["printing_setups"] = json!([]);
        assert!(evaluate(
            &serde_json::to_vec(
                &json!({"command":"context_preparation","sheets":missing,"online":true})
            )
            .unwrap()
        )
        .is_err());
    }
    /// The published Gisagara CJIC samples select elevation contours and hide
    /// both layers they print as. Every project that copied them acquired
    /// contours, counted them as printed, and drew nothing. A source the
    /// layout hides entirely is named and never acquired; a layout that shows
    /// it elsewhere still gets it.
    #[test]
    fn a_context_the_layout_hides_entirely_is_named_never_acquired() {
        let contours = crate::printing::context::derived_layers().remove(2);
        let hidden_style = crate::printing::PrintStyleOverride {
            visible: Some(false),
            ..Default::default()
        };
        let mut hiding = crate::printing::context::toggle(
            crate::printing::default_layout(),
            contours.clone(),
            true,
            None,
        )
        .unwrap();
        hiding.id = "a0".into();
        for id in ["contours_index", "contours_intermediate"] {
            hiding
                .style_refs
                .insert(id.into(), format!("print_context/{id}_print"));
            hiding
                .style_overrides
                .insert(id.into(), hidden_style.clone());
        }
        let prepare = |layouts: Vec<crate::printing::Layout>, formats: serde_json::Value| {
            let setups: Vec<serde_json::Value> = layouts
                .into_iter()
                .map(|layout| json!({"id":layout.id,"revision":"a".repeat(64),"layout":layout}))
                .collect();
            let sheets = json!({"project_settings":[{"parameter":"design_export_format","value":formats}], "printing_setups":setups});
            let output: serde_json::Value = serde_json::from_str(
                &evaluate(
                    &serde_json::to_vec(
                        &json!({"command":"context_preparation","sheets":sheets,"online":true}),
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            output["result"].clone()
        };
        let result = prepare(vec![hiding.clone()], json!(["pdf__a0"]));
        assert_eq!(
            result["contexts"],
            json!([]),
            "a hidden source is not acquired"
        );
        assert_eq!(result["hidden"][0]["context"], "elevation_contours");
        assert_eq!(result["hidden"][0]["layout"], "a0");
        assert_eq!(
            result["hidden"][0]["hidden_layers"],
            json!(["contours_intermediate", "contours_index"])
        );

        // One layer still shown is enough: the source prints.
        let mut index_only = hiding.clone();
        index_only.style_overrides.remove("contours_index");
        let result = prepare(vec![index_only], json!(["pdf__a0"]));
        assert_eq!(result["contexts"][0]["id"], "elevation_contours");
        assert_eq!(result["hidden"], json!([]));

        // Hidden on one selected sheet, shown on another: acquired once.
        let mut showing = crate::printing::context::toggle(
            crate::printing::default_layout(),
            contours,
            true,
            None,
        )
        .unwrap();
        showing.id = "a3".into();
        let result = prepare(vec![hiding, showing], json!(["pdf__a0", "pdf__a3"]));
        assert_eq!(result["contexts"].as_array().unwrap().len(), 1);
        assert_eq!(result["hidden"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn gui_and_cli_export_selection_use_the_same_closed_command() {
        let value = json!({"command":"export","project":"p","transformer":"agatare"});
        let output: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&value).unwrap()).unwrap()).unwrap();
        assert_eq!(output["result"]["kind"], "transformer");
        assert_eq!(output["result"]["reuse_prepared"], true);
        for bad in [
            json!({"transformer":"collisions"}),
            json!({"report_type":"compounded_reports"}),
            json!({"transformers":["another"]}),
            json!({"project":"../p"}),
        ] {
            let mut changed = value.clone();
            for (k, v) in bad.as_object().unwrap() {
                changed[k] = v.clone();
            }
            assert!(evaluate(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
    }

    #[test]
    fn a_project_overview_needs_the_matching_open_project_and_a_batch_reports_in_order() {
        let overview = |active: serde_json::Value| {
            evaluate(&serde_json::to_vec(&serde_json::json!({"command":"export","project":"p1","transformer":"combined_transformer","transformers":["a","b"],"active_project":active})).unwrap())
        };
        assert!(overview(serde_json::json!("p1")).is_ok());
        assert_eq!(
            overview(serde_json::json!("p2")).unwrap_err(),
            "Project overview requires the matching active project."
        );
        assert!(overview(serde_json::Value::Null).is_err());
        // A single transformer export is not bound to the open project.
        assert!(evaluate(br#"{"command":"export","project":"p1","transformer":"tx_a"}"#).is_ok());
        let out = evaluate(br#"{"command":"batch_outcome","results":[{"transformer":"tx_b","status":"error","error":"x"},{"transformer":"tx_a","status":"ok"},{"transformer":"TX_C","status":"error"}]}"#).unwrap();
        let out: serde_json::Value = serde_json::from_str(&out).unwrap();
        let out = &out["result"];
        assert_eq!(out["failed"], 2);
        assert_eq!(out["results"][0]["transformer"], "tx_a");
        assert_eq!(out["results"][1]["transformer"], "tx_b");
        assert_eq!(out["results"][2]["transformer"], "TX_C");
    }
}
