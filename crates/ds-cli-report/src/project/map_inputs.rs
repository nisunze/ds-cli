//! Native source acquisition only; overview projection and extents are kernel-owned.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::{printing, report_export::InputReceipt};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

pub static COMMAND: Command = Command {
    id: "report.project.map-inputs",
    path: &["report", "project", "map-inputs"],
    contract: 2,
    summary: "Capture governed project maps for headless printing.",
    purpose: "Capture active LV transformers and exact current MV models, provenance, styles, layout, held context and renderer policy in a replayable report.layout.render request. Omit --transformer for all active transformers. Vectors retain crossing geometry. No design write or publication; omissions are reported.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<id>",
            "Exact project id; saved active-project selection is ignored.",
        )
        .required(),
        super::TRANSFORMER_ARG,
        Arg::value(
            "mv-model",
            "<absolute.dsgrid>",
            "Local DS Grid draft with its exact digest in the print receipt.",
        ),
        Arg::value(
            "focus-bounds",
            "<west,south,east,north>",
            "WGS84 sheet bounds up to 0.05 degrees; acquire context and nearby vectors without changing print extent.",
        ),
        Arg::value(
            "area-bounds",
            "<west,south,east,north>",
            "WGS84 map rectangle (up to 0.5 degrees); acquires context, clips vectors and sets print extent. Excludes --focus-bounds.",
        ),
        Arg::value(
            "layout",
            "<json-file>",
            "Validated authored layout; context and pens remain editable without code.",
        )
        .required(),
        Arg::value(
            "out-dir",
            "<path>",
            "Fresh directory for pinned render inputs and subsequent PDF/PNG outputs.",
        )
        .required(),
        Arg::switch(
            "seed",
            "Acquire missing map context through the dataset owner; may incur provider cost.",
        ),
        Arg::value(
            "reuse-capture",
            "<out-dir>",
            "Earlier capture of this project whose pinned transformer sources are reused.",
        ),
        super::LANE_ARG,
    ],
    output: "Project, transformer count, new LV line, pole, service cable and customer feature counts, exact MV model provenance, source revisions, omitted context, and render request path.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("report_inputs_invalid", e.to_string())
        .remedy("Correct the authored layout or reported source; use a fresh output directory")
}

const CONTEXT_BATCH_BACKOFF: [std::time::Duration; 2] = [
    std::time::Duration::from_secs(2),
    std::time::Duration::from_secs(5),
];

fn read_context_batch<T>(
    mut read: impl FnMut() -> Result<T, Failure>,
    mut pause: impl FnMut(std::time::Duration),
) -> Result<T, Failure> {
    for delay in CONTEXT_BATCH_BACKOFF {
        match read() {
            Ok(value) => return Ok(value),
            Err(error)
                if error.class() == ds_cli_contract::outcome::ExitClass::Unavailable
                    && matches!(error.code(), "auth_transient" | "device_auth_transient") =>
            {
                pause(delay);
            }
            Err(error) => return Err(error),
        }
    }
    read()
}

/// A native project read that answers nothing used to leave the capture idle
/// for many minutes with no JSON. Each acquisition step now has a bound and a
/// stalled step is refused by name; the abandoned read ends with the process.
const ACQUISITION_STEP_BOUND: std::time::Duration = std::time::Duration::from_secs(180);
const ACQUISITION_STALLED: Refusal = Refusal {
    code: "report_source_acquisition_stalled",
    when: "a native project read answered nothing within 180 seconds",
    remedy: "retry once the store answers, or pass --reuse-capture",
};
const REUSED_CAPTURE_INVALID: Refusal = Refusal {
    code: "report_reused_capture_invalid",
    when: "--reuse-capture is not a digest-verified capture of this project and principal",
    remedy: "pass an earlier --out-dir of this project, or omit --reuse-capture",
};
const REUSED_CAPTURE_STALE: Refusal = Refusal {
    code: "report_reused_capture_stale",
    when: "the active transformers differ from the pinned capture",
    remedy: "capture once without --reuse-capture, then reuse it",
};
const REFUSALS: &[Refusal] = &super::joined::<{ super::export::REFUSALS.len() + 3 }>(&[
    super::export::REFUSALS,
    &[
        ACQUISITION_STALLED,
        REUSED_CAPTURE_INVALID,
        REUSED_CAPTURE_STALE,
    ],
]);

/// Run one acquisition step on its own thread and refuse it by name when it
/// answers nothing within `bound`.
fn bounded<T: Send + 'static>(
    step: &str,
    bound: std::time::Duration,
    read: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    let stalled = |what: &str| {
        Failure::unavailable(ACQUISITION_STALLED.code, format!("{step} {what}"))
            .remedy(ACQUISITION_STALLED.remedy)
    };
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("map-inputs-acquisition".into())
        .spawn(move || {
            let _ = send.send(read());
        })
        .map_err(|error| stalled(&format!("could not start: {error}")))?;
    match receive.recv_timeout(bound) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(stalled(&format!(
            "answered nothing within {} s",
            bound.as_secs()
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(stalled("ended without an answer"))
        }
    }
}

/// The transformer sources a fresh capture read, pinned beside its request so
/// a later sheet of the same delivery reuses them instead of rereading every
/// room. `sources.json` names this file and its SHA-256.
const CONTEXTS_FILE: &str = "transformer-contexts.json";
const CONTEXTS_SCHEMA: &str = "ds.report-map-inputs.transformer-contexts/v1";
const CAPTURE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn principal_sha256(uid: &str) -> String {
    ds_command_kernel::report_export::sha256_hex(uid.as_bytes())
}

fn reuse_invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid(REUSED_CAPTURE_INVALID.code, message.to_string())
        .remedy(REUSED_CAPTURE_INVALID.remedy)
}

fn read_bounded(path: &std::path::Path) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| reuse_invalid(format!("open `{}`: {error}", path.display())))?
        .take(CAPTURE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| reuse_invalid(format!("read `{}`: {error}", path.display())))?;
    if bytes.len() as u64 > CAPTURE_MAX_BYTES {
        return Err(reuse_invalid(format!("`{}` exceeds 2 GiB", path.display())));
    }
    Ok(bytes)
}

/// Read a prior capture's pinned transformer sources: its `sources.json`
/// names the file and SHA-256, the bytes must match, and the capture must
/// belong to this project and principal and hold exactly `active`.
fn reused_contexts(
    dir: &std::path::Path,
    project: &str,
    uid: &str,
    active: &[String],
) -> Result<(Value, PathBuf, String), Failure> {
    let receipt: Value = serde_json::from_slice(&read_bounded(&dir.join("sources.json"))?)
        .map_err(|error| reuse_invalid(format!("sources.json: {error}")))?;
    let pinned = &receipt["transformer_contexts"];
    let (Some(path), Some(sha256)) = (pinned["path"].as_str(), pinned["sha256"].as_str()) else {
        return Err(reuse_invalid(
            "sources.json names no pinned transformer capture; capture once without --reuse-capture",
        ));
    };
    let path = PathBuf::from(path);
    let bytes = read_bounded(&path)?;
    if ds_command_kernel::report_export::sha256_hex(&bytes) != sha256 {
        return Err(reuse_invalid(format!(
            "`{}` no longer has the SHA-256 its capture recorded",
            path.display()
        )));
    }
    let document: Value = serde_json::from_slice(&bytes).map_err(reuse_invalid)?;
    if document["schema"] != CONTEXTS_SCHEMA
        || document["project"] != project
        || receipt["project"] != project
    {
        return Err(reuse_invalid(format!(
            "the pinned capture is not a {CONTEXTS_SCHEMA} capture of project {project}"
        )));
    }
    if document["principal_sha256"] != principal_sha256(uid).as_str() {
        return Err(reuse_invalid(
            "the pinned capture was read by another principal",
        ));
    }
    let mut pinned_names = document["transformers"]
        .as_array()
        .ok_or_else(|| reuse_invalid("the pinned capture lists no transformers"))?
        .iter()
        .map(|row| row["transformer"].as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| reuse_invalid("a pinned transformer row has no name"))?;
    let mut current = active.to_vec();
    pinned_names.sort();
    current.sort();
    if pinned_names != current {
        let added = current
            .iter()
            .filter(|name| pinned_names.binary_search(name).is_err())
            .count();
        let removed = pinned_names
            .iter()
            .filter(|name| current.binary_search(name).is_err())
            .count();
        return Err(Failure::conflict(
            REUSED_CAPTURE_STALE.code,
            format!("{added} active transformers are not in the pinned capture and {removed} pinned transformers are no longer active"),
        )
        .remedy(REUSED_CAPTURE_STALE.remedy));
    }
    Ok((document, path, sha256.to_owned()))
}
fn parse_bounds(raw: &str, name: &str, max_span: f64) -> Result<[f64; 4], Failure> {
    let bounds = ds_cli_contract::args::bbox(raw).map_err(|error| invalid(error.message()))?;
    if bounds[2] - bounds[0] > max_span || bounds[3] - bounds[1] > max_span {
        return Err(invalid(format!(
            "--{name} needs a valid WGS84 rectangle no wider or taller than {max_span} degrees"
        )));
    }
    Ok(bounds)
}
fn read_layout(path: &str) -> Result<printing::Layout, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| invalid(format!("open --layout `{path}`: {error}")))?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(format!("read --layout `{path}`: {error}")))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid("layout exceeds 16 MiB"));
    }
    let layout: printing::Layout = serde_json::from_slice(&bytes).map_err(invalid)?;
    printing::validate(&layout).map_err(invalid)?;
    Ok(layout)
}
pub fn run(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let layout = read_layout(i.require("layout")?)?;
    let out = PathBuf::from(i.require("out-dir")?);
    if out.exists() {
        return Err(Failure::invalid(
            "report_output_exists",
            "map output directory already exists",
        )
        .remedy("Choose a fresh --out-dir"));
    }
    let focus_bounds = i
        .value("focus-bounds")
        .map(|raw| parse_bounds(raw, "focus-bounds", 0.05))
        .transpose()?;
    let area_bounds = i
        .value("area-bounds")
        .map(|raw| parse_bounds(raw, "area-bounds", 0.5))
        .transpose()?;
    if focus_bounds.is_some() && area_bounds.is_some() {
        return Err(invalid(
            "--focus-bounds and --area-bounds select different map purposes; pass one",
        ));
    }
    let lane = i.require("lane")?;
    let requested = super::transformer_set(i)?;
    let inventory = {
        let (lane, project) = (lane.to_owned(), i.require("project")?.to_owned());
        bounded(
            "the transformer inventory read",
            ACQUISITION_STEP_BOUND,
            move || ds_cli_auth::transformer_inventory_for_project(&lane, &project, &requested),
        )?
    };
    let identity = inventory.identity();
    let project = inventory.project_id();
    let config = ds_cli_auth::feeder_configuration_for_project(lane, project)?;
    if config.identity() != identity || config.project_id() != project {
        return Err(invalid("configuration scope changed"));
    }
    let directory = ds_cli_auth::project_directory(lane)?;
    if directory.identity() != identity {
        return Err(invalid("project CRS discovery scope changed"));
    }
    let project_crs = directory
        .project_params(project)
        .filter(|params| params["crs"].is_object())
        .map(|params| {
            printing::project_crs::Capture::new(
                project,
                lane,
                identity.uid(),
                identity.credential_audience_sha256(),
                params.clone(),
            )
        })
        .transpose()
        .map_err(invalid)?;
    let receipt = InputReceipt::from_config(&config.result().document).map_err(invalid)?;
    let server_sheets_sha256 = receipt.sheets_sha256.clone();
    let styles = ds_cli_auth::style_governance(
        lane,
        project,
        &ds_command_kernel::style_governance::Command::Table,
    )?;
    let snapshot: ds_command_kernel::style_resolution::Snapshot =
        serde_json::from_value(styles).map_err(invalid)?;
    if snapshot.project_id != project {
        return Err(invalid("renderer policy scope changed"));
    }
    // Project templates persist semantic keys after migration. Resolve their
    // exact API documents before the ref-based physical-pen preflight, just
    // as the interactive and report renderers do.
    let (layout, _) = printing::resolve_style_documents(layout, &snapshot).map_err(invalid)?;
    let receipt = super::export::complete_proof_print_styles(
        lane,
        project,
        receipt,
        std::slice::from_ref(&layout),
    )?;
    let sheets = receipt.sheets().map_err(invalid)?;
    let renderer_defaults = printing::renderer_defaults::resolve(&snapshot).map_err(invalid)?;
    printing::style_overrides::preflight(&layout, &sheets["printing_styles"]).map_err(invalid)?;
    let mut models = super::mv_context::load(lane, identity, project)?;
    if let Some(path) = i.value("mv-model") {
        models.push(super::mv_context::load_local(path)?);
    }
    let mut sources = printing::project_sources::Sources::default();
    sources
        .collections(&super::mv_context::overview(&models)?)
        .map_err(invalid)?;
    let mut revisions = Vec::new();
    let active = inventory
        .result()
        .rows()
        .iter()
        .filter(|row| {
            row.kind() == ds_cli_auth::TransformerKind::Transformer
                && row.lifecycle() == ds_cli_auth::TransformerLifecycle::Active
        })
        .map(|row| row.name().to_owned())
        .collect::<Vec<_>>();
    // A fresh capture reads every active room and pins those sources beside
    // its request; `--reuse-capture` reuses a pinned capture of the same
    // project and principal, verified by SHA-256 and by the live active set.
    let (contexts, contexts_pin) = if let Some(dir) = i.value("reuse-capture") {
        let (document, path, sha256) =
            reused_contexts(std::path::Path::new(dir), project, identity.uid(), &active)?;
        (document, Some((path, sha256)))
    } else {
        let mut rows = Vec::with_capacity(active.len());
        // A large district can contain hundreds of active transformers. Refresh
        // the same fenced native project context between bounded groups so a
        // long acquisition does not expire its authentication lease mid-batch.
        for names in active.chunks(16) {
            let contexts = {
                let (lane, project, names) = (lane.to_owned(), project.to_owned(), names.to_vec());
                bounded(
                    "a transformer context read",
                    ACQUISITION_STEP_BOUND,
                    move || {
                        read_context_batch(
                            || {
                                ds_cli_auth::transformer_contexts_for_project(
                                    &lane, &project, &names,
                                )
                            },
                            std::thread::sleep,
                        )
                    },
                )?
            };
            if contexts.identity() != identity || contexts.project_id() != project {
                return Err(invalid("transformer context scope changed"));
            }
            for (name, response) in names.iter().zip(contexts.into_result()) {
                let snapshot = &response;
                if snapshot.ds_project() != project || snapshot.transformer_name() != name {
                    return Err(invalid("transformer scope changed"));
                }
                rows.push(json!({"transformer":name,"version":snapshot.metadata().version(),"content_digest":snapshot.metadata().content_digest(),"layers":serde_json::to_value(snapshot.layers()).map_err(invalid)?}));
            }
        }
        (
            json!({"schema":CONTEXTS_SCHEMA,"project":project,"principal_sha256":principal_sha256(identity.uid()),"transformers":rows}),
            None,
        )
    };
    for row in contexts["transformers"].as_array().into_iter().flatten() {
        let name = row["transformer"].as_str().unwrap_or_default();
        sources.transformer(name, &row["layers"]).map_err(invalid)?;
        revisions.push(json!({"transformer":name,"version":row["version"],"content_digest":row["content_digest"]}));
    }
    let contexts_bytes = match contexts_pin {
        Some(_) => None,
        None => Some(serde_json::to_vec(&contexts).map_err(invalid)?),
    };
    drop(contexts);
    let network = sources.layers();
    let design_layer_counts = ["tr", "lv_lines", "lv_poles", "service_cables", "customers"]
        .into_iter()
        .map(|id| {
            (
                id.to_owned(),
                json!(network[id]["features"].as_array().map_or(0, Vec::len)),
            )
        })
        .collect::<serde_json::Map<String, Value>>();
    // Context coverage belongs to the physical page, while the overview's
    // complete design remains available to the shared map painter. An MV
    // route can span a district; its union rectangle is not a sheet extent.
    let context_network = if let Some(bounds) = area_bounds.or(focus_bounds) {
        json!({"mv_sheet": {"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[bounds[0],bounds[1]],[bounds[2],bounds[3]]]}}]}})
    } else {
        network.clone()
    };
    let catalog = ds_project_data::validate_resources(&ds_cli_auth::data_distribution(
        lane,
        project,
        &ds_cli_auth::DataDistributionRequest::ListDatasets {},
    )?)
    .map_err(invalid)?;
    let scope = ds_command_kernel::project_dataset_cache::Scope {
        principal: identity.uid().into(),
        project: project.into(),
    };
    let contexts = layout
        .context_layers
        .iter()
        .filter(|c| {
            !matches!(
                c.source,
                printing::PrintContextSource::ProjectDsgridMv { .. }
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut provider = ds_cli_data::project_cache::CliProvider { lane, project };
    let mut fetch = ds_cli_data::project_cache::bundle_fetch(lane);
    let mut hosts = ds_project_data::Hosts {
        provider: &mut provider,
        fetch: &mut fetch,
    };
    let boundary_contexts = contexts
        .iter()
        .filter(|c| c.id == "district_boundaries")
        .cloned()
        .collect::<Vec<_>>();
    let boundary_context = ds_project_data::read_print_context(
        &ds_report_host::shared_root().map_err(invalid)?,
        &scope,
        "mv_data",
        &context_network,
        &boundary_contexts,
        &catalog,
        if i.switch("seed") {
            ds_project_data::Mode::Acquire(&mut hosts)
        } else {
            ds_project_data::Mode::Read
        },
    )
    .map_err(invalid)?;
    if let Some(bytes) = &boundary_context.document {
        let document: Value = serde_json::from_slice(bytes).map_err(invalid)?;
        sources.collections(&document["layers"]).map_err(invalid)?;
    }
    let extent = match area_bounds {
        Some(bounds) => bounds,
        None => sources.overview_extent().map_err(invalid)?,
    };
    let contexts = contexts
        .into_iter()
        .filter(|c| c.id != "district_boundaries")
        .collect::<Vec<_>>();
    let mode = if i.switch("seed") {
        ds_project_data::Mode::Acquire(&mut hosts)
    } else {
        ds_project_data::Mode::Read
    };
    let mut context = ds_project_data::read_print_context(
        &ds_report_host::shared_root().map_err(invalid)?,
        &scope,
        "mv_data",
        &context_network,
        &contexts,
        &catalog,
        mode,
    )
    .map_err(invalid)?;
    if let Some(bytes) = context.document {
        let document: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        sources.collections(&document["layers"]).map_err(invalid)?;
    }
    context.omitted.extend(boundary_context.omitted);
    context.warnings.extend(boundary_context.warnings);
    let admin_reference = if layout
        .elements
        .iter()
        .any(|element| element.text.contains("{admin_subtitle}"))
    {
        let points=network["tr"]["features"].as_array().filter(|features|features.len()==1)
            .ok_or_else(||invalid("print_admin_subtitle_unavailable: one exact focused transformer Point is required; an overview cannot guess its administrative identity"))?;
        let root = ds_report_host::shared_root().map_err(invalid)?;
        let path =
            ds_report_host::installed_admin_bounds_path(&root, &receipt.reference_semantic_sha256);
        let asset =
            ds_report_host::verify_admin_bounds_asset(&path, &receipt.reference_semantic_sha256)
                .map_err(|error| invalid(error.message))?;
        Some(
            printing::admin_reference::Capture::new(
                printing::admin_reference::Asset {
                    country: receipt.country.clone(),
                    path: asset.path,
                    sha256: asset.sha256,
                },
                &points[0],
            )
            .map_err(invalid)?,
        )
    } else {
        None
    };
    std::fs::create_dir_all(&out).map_err(invalid)?;
    let out = out.canonicalize().map_err(invalid)?;
    let transformer_contexts = match (contexts_bytes, contexts_pin) {
        (_, Some((path, sha256))) => json!({"path":path,"sha256":sha256,"reused":true}),
        (bytes, None) => {
            let bytes = bytes.unwrap_or_default();
            let path = out.join(CONTEXTS_FILE);
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map_err(invalid)?
                .write_all(&bytes)
                .map_err(invalid)?;
            json!({"path":path,"sha256":ds_command_kernel::report_export::sha256_hex(&bytes),"reused":false})
        }
    };
    let render_layers = if let Some([w, s, e, n]) = focus_bounds.or(area_bounds) {
        let margin = 0.002;
        sources
            .vectors_in_area([w - margin, s - margin, e + margin, n + margin])
            .map_err(invalid)?
    } else {
        sources.vectors()
    };
    let current = ds_cli_auth::headless_identity_for_named_project(lane)?;
    if &current != identity {
        return Err(invalid("authenticated style capture scope changed"));
    }
    let style_capture = ds_command_kernel::printing::style_capture::Capture::new(
        &snapshot,
        project,
        lane,
        identity.uid(),
        identity.credential_audience_sha256(),
        &server_sheets_sha256,
        &sheets,
        std::slice::from_ref(&layout),
    )
    .map_err(invalid)?;
    let report_config = json!({"sheets":sheets,"printing_style_capture":style_capture});
    let render = json!({"schema":"ds.print-layout-export/v1","renderer_defaults":renderer_defaults,"render":{"admin_reference":admin_reference,"project_crs":project_crs,"report_config":report_config,"renderer_defaults":renderer_defaults,"layout":layout,"layers":render_layers,"extent":extent,"focus_extent":extent,"print_styles":sheets["printing_styles"],"symbol_assets":sheets.get("printing_symbol_assets").cloned().unwrap_or_else(||json!({})),"text":{"project":project,"transformer":layout.name}},"formats":["pdf","png"],"dpi":300,"out_dir":out.join("rendered")});
    let path = out.join("render-request.json");
    let data = serde_json::to_vec(&render).map_err(invalid)?;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(invalid)?
        .write_all(&data)
        .map_err(invalid)?;
    let result = json!({"project":project,"transformer_count":revisions.len(),"design_layer_counts":design_layer_counts,"sources":revisions,"transformer_contexts":transformer_contexts,"mv_models":super::mv_context::provenance(&models),"area_bounds":area_bounds,"render_extent":extent,"omitted":context.omitted.iter().map(|o|json!({"layer":o.layer,"reason":o.reason})).collect::<Vec<_>>(),"warnings":context.warnings,"request":path,"sha256":ds_command_kernel::report_export::sha256_hex(&data)});
    std::fs::write(
        out.join("sources.json"),
        serde_json::to_vec_pretty(&result).map_err(invalid)?,
    )
    .map_err(invalid)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        ACQUISITION_STALLED, CONTEXTS_SCHEMA, REFUSALS, REUSED_CAPTURE_INVALID,
        REUSED_CAPTURE_STALE, bounded, parse_bounds, principal_sha256, read_context_batch,
        read_layout, reused_contexts,
    };
    use ds_cli_contract::outcome::Failure;

    #[test]
    fn missing_layout_refusal_names_the_exact_authored_file() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("maps/layout-rulindo.json");
        let error = read_layout(missing.to_str().unwrap()).unwrap_err();
        assert_eq!(error.code(), "report_inputs_invalid");
        assert!(error.message().contains("open --layout"), "{error:?}");
        assert!(
            error
                .message()
                .contains(&missing.to_string_lossy().to_string()),
            "{error:?}"
        );
    }

    #[test]
    fn transient_context_batch_is_retried_then_read() {
        let mut calls = 0;
        let mut sleeps = Vec::new();
        let result = read_context_batch(
            || {
                calls += 1;
                match calls {
                    1 => Err(Failure::unavailable("auth_transient", "temporary")),
                    2 => Err(Failure::unavailable("device_auth_transient", "temporary")),
                    _ => Ok(42),
                }
            },
            |delay| sleeps.push(delay.as_secs()),
        );
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls, 3);
        assert_eq!(sleeps, [2, 5]);
    }

    #[test]
    fn permanent_context_refusal_does_not_retry() {
        let mut calls = 0;
        let mut sleeps = Vec::new();
        let result = read_context_batch::<()>(
            || {
                calls += 1;
                Err(Failure::unauthorized("auth_rejected", "denied"))
            },
            |delay| sleeps.push(delay),
        );
        assert_eq!(result.unwrap_err().code(), "auth_rejected");
        assert_eq!(calls, 1);
        assert!(sleeps.is_empty());
    }

    #[test]
    fn transient_context_batch_exhausts_after_three_reads() {
        let mut calls = 0;
        let result = read_context_batch::<()>(
            || {
                calls += 1;
                Err(Failure::unavailable("auth_transient", "temporary"))
            },
            |_| {},
        );
        assert_eq!(result.unwrap_err().code(), "auth_transient");
        assert_eq!(calls, 3);
    }

    #[test]
    fn a_stalled_acquisition_step_is_refused_by_name_and_documented() {
        // f48bd93c: a read that never answers used to hang the capture with
        // no JSON. It is refused by name within its bound instead.
        let error = bounded(
            "the inventory read",
            std::time::Duration::from_millis(50),
            || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), ACQUISITION_STALLED.code);
        assert!(
            error
                .message()
                .contains("the inventory read answered nothing"),
            "{error:?}"
        );
        assert!(error.remedy_text().is_some());
        let answered = bounded("a read", std::time::Duration::from_secs(5), || Ok(7)).unwrap();
        assert_eq!(answered, 7);
        let refused = bounded::<()>("a read", std::time::Duration::from_secs(5), || {
            Err(Failure::unauthorized("auth_rejected", "denied"))
        })
        .unwrap_err();
        assert_eq!(refused.code(), "auth_rejected");
        for code in [
            ACQUISITION_STALLED.code,
            REUSED_CAPTURE_INVALID.code,
            REUSED_CAPTURE_STALE.code,
            "report_inputs_invalid",
        ] {
            assert!(
                REFUSALS.iter().any(|refusal| refusal.code == code),
                "{code}"
            );
        }
    }

    #[test]
    fn a_reused_capture_is_digest_pinned_to_its_project_principal_and_active_rooms() {
        let root = tempfile::tempdir().unwrap();
        let capture = root.path().join("sheet-001");
        std::fs::create_dir(&capture).unwrap();
        let write = |document: &serde_json::Value| {
            let bytes = serde_json::to_vec(document).unwrap();
            let path = capture.join("transformer-contexts.json");
            std::fs::write(&path, &bytes).unwrap();
            let sha256 = ds_command_kernel::report_export::sha256_hex(&bytes);
            std::fs::write(
                capture.join("sources.json"),
                serde_json::to_vec(&serde_json::json!({"project":"p1","transformer_contexts":{"path":path,"sha256":sha256,"reused":false}})).unwrap(),
            )
            .unwrap();
            path
        };
        let document = serde_json::json!({"schema":CONTEXTS_SCHEMA,"project":"p1","principal_sha256":principal_sha256("uid-a"),"transformers":[
            {"transformer":"tr-b","version":3,"content_digest":"d-b","layers":{}},
            {"transformer":"tr-a","version":1,"content_digest":"d-a","layers":{}}]});
        let path = write(&document);
        let active = ["tr-a".to_owned(), "tr-b".to_owned()];
        let (reused, pinned, _) = reused_contexts(&capture, "p1", "uid-a", &active).unwrap();
        assert_eq!(pinned, path);
        assert_eq!(reused["transformers"][0]["content_digest"], "d-b");
        // Another project, another principal or a changed room set is refused.
        assert_eq!(
            reused_contexts(&capture, "p2", "uid-a", &active)
                .unwrap_err()
                .code(),
            REUSED_CAPTURE_INVALID.code
        );
        assert_eq!(
            reused_contexts(&capture, "p1", "uid-b", &active)
                .unwrap_err()
                .code(),
            REUSED_CAPTURE_INVALID.code
        );
        let grown = ["tr-a".to_owned(), "tr-b".to_owned(), "tr-c".to_owned()];
        let stale = reused_contexts(&capture, "p1", "uid-a", &grown).unwrap_err();
        assert_eq!(stale.code(), REUSED_CAPTURE_STALE.code);
        assert!(
            stale.message().starts_with("1 active transformers"),
            "{stale:?}"
        );
        // Bytes that no longer match the recorded digest are not reused.
        std::fs::write(&path, b"{}").unwrap();
        let tampered = reused_contexts(&capture, "p1", "uid-a", &active).unwrap_err();
        assert_eq!(tampered.code(), REUSED_CAPTURE_INVALID.code);
        assert!(tampered.message().contains("SHA-256"), "{tampered:?}");
        // A directory that never pinned a capture names the repair.
        let empty = root.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert_eq!(
            reused_contexts(&empty, "p1", "uid-a", &active)
                .unwrap_err()
                .code(),
            REUSED_CAPTURE_INVALID.code
        );
    }

    #[test]
    fn district_area_accepts_authority_extent_while_sheet_focus_stays_bounded() {
        let nyamagabe = "29.2646534136055,-2.60032940893701,29.6660934836079,-2.19979084366594";
        assert!(parse_bounds(nyamagabe, "area-bounds", 0.5).is_ok());
        assert!(parse_bounds(nyamagabe, "focus-bounds", 0.05).is_err());
        assert!(parse_bounds("29,-3,28,-2", "area-bounds", 0.5).is_err());
        assert!(parse_bounds("29,-3,30,-2", "area-bounds", 0.5).is_err());
    }
}
