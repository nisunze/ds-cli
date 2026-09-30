//! Native source acquisition only; overview projection and extents are kernel-owned.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
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
    contract: 1,
    summary: "Prepare a district MV overview for headless PDF/PNG printing.",
    purpose: "Read active LV transformers and exact current MV models with revisions, geometry and saved print styles. Bound emitted vectors with --focus-bounds or --area-bounds without straightening crossing lines. Apply an authored layout to held context and write a replayable report.layout.render request. No design write or publication. --seed acquires missing context; the receipt names any omission.",
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
        super::LANE_ARG,
    ],
    output: "Project, transformer count, new LV line, pole, service cable and customer feature counts, exact MV model provenance, source revisions, omitted context, and render request path.",
    examples: &[],
    refusals: super::export::REFUSALS,
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
fn parse_bounds(raw: &str, name: &str, max_span: f64) -> Result<[f64; 4], Failure> {
    let values = raw
        .split(',')
        .map(|part| part.trim().parse::<f64>().map_err(invalid))
        .collect::<Result<Vec<_>, _>>()?;
    let bounds: [f64; 4] = values.try_into().map_err(|_| {
        invalid(format!(
            "--{name} needs four WGS84 coordinates: west,south,east,north"
        ))
    })?;
    if bounds.iter().any(|value| !value.is_finite())
        || !(-180.0..=180.0).contains(&bounds[0])
        || !(-90.0..=90.0).contains(&bounds[1])
        || !(-180.0..=180.0).contains(&bounds[2])
        || !(-90.0..=90.0).contains(&bounds[3])
        || bounds[2] <= bounds[0]
        || bounds[3] <= bounds[1]
        || bounds[2] - bounds[0] > max_span
        || bounds[3] - bounds[1] > max_span
    {
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
    let requested = ds_cli_auth::TransformerSet::default();
    let inventory =
        ds_cli_auth::transformer_inventory_for_project(lane, i.require("project")?, &requested)?;
    let identity = inventory.identity();
    let project = inventory.project_id();
    let config = ds_cli_auth::feeder_configuration_for_project(lane, project)?;
    if config.identity() != identity || config.project_id() != project {
        return Err(invalid("configuration scope changed"));
    }
    let receipt = InputReceipt::from_config(&config.result().document).map_err(invalid)?;
    let receipt = super::export::complete_proof_print_styles(
        lane,
        project,
        receipt,
        std::slice::from_ref(&layout),
    )?;
    let sheets = receipt.sheets().map_err(invalid)?;
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
    // A large district can contain hundreds of active transformers. Refresh
    // the same fenced native project context between bounded groups so a
    // long acquisition does not expire its authentication lease mid-batch.
    for names in active.chunks(16) {
        let contexts = read_context_batch(
            || ds_cli_auth::transformer_contexts_for_project(lane, project, names),
            std::thread::sleep,
        )?;
        if contexts.identity() != identity || contexts.project_id() != project {
            return Err(invalid("transformer context scope changed"));
        }
        for (name, response) in names.iter().zip(contexts.into_result()) {
            let snapshot = &response;
            if snapshot.ds_project() != project || snapshot.transformer_name() != name {
                return Err(invalid("transformer scope changed"));
            }
            sources
                .transformer(
                    name,
                    &serde_json::to_value(snapshot.layers()).map_err(invalid)?,
                )
                .map_err(invalid)?;
            revisions.push(json!({"transformer":name,"version":snapshot.metadata().version(),"content_digest":snapshot.metadata().content_digest()}));
        }
    }
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
    std::fs::create_dir_all(&out).map_err(invalid)?;
    let out = out.canonicalize().map_err(invalid)?;
    let render_layers = if let Some([w, s, e, n]) = focus_bounds.or(area_bounds) {
        let margin = 0.002;
        sources
            .vectors_in_area([w - margin, s - margin, e + margin, n + margin])
            .map_err(invalid)?
    } else {
        sources.vectors()
    };
    let render = json!({"schema":"ds.print-layout-export/v1","render":{"layout":layout,"layers":render_layers,"extent":extent,"focus_extent":extent,"print_styles":sheets["printing_styles"],"symbol_assets":sheets.get("printing_symbol_assets").cloned().unwrap_or_else(||json!({})),"text":{"project":project,"transformer":layout.name}},"formats":["pdf","png"],"dpi":300,"out_dir":out.join("rendered")});
    let path = out.join("render-request.json");
    let data = serde_json::to_vec(&render).map_err(invalid)?;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(invalid)?
        .write_all(&data)
        .map_err(invalid)?;
    let result = json!({"project":project,"transformer_count":revisions.len(),"design_layer_counts":design_layer_counts,"sources":revisions,"mv_models":super::mv_context::provenance(&models),"area_bounds":area_bounds,"render_extent":extent,"omitted":context.omitted.iter().map(|o|json!({"layer":o.layer,"reason":o.reason})).collect::<Vec<_>>(),"warnings":context.warnings,"request":path,"sha256":ds_command_kernel::report_export::sha256_hex(&data)});
    std::fs::write(
        out.join("sources.json"),
        serde_json::to_vec_pretty(&result).map_err(invalid)?,
    )
    .map_err(invalid)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{parse_bounds, read_context_batch, read_layout};
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
    fn district_area_accepts_authority_extent_while_sheet_focus_stays_bounded() {
        let nyamagabe = "29.2646534136055,-2.60032940893701,29.6660934836079,-2.19979084366594";
        assert!(parse_bounds(nyamagabe, "area-bounds", 0.5).is_ok());
        assert!(parse_bounds(nyamagabe, "focus-bounds", 0.05).is_err());
        assert!(parse_bounds("29,-3,28,-2", "area-bounds", 0.5).is_err());
        assert!(parse_bounds("29,-3,30,-2", "area-bounds", 0.5).is_err());
    }
}
