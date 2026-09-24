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
    purpose: "Reads all active LV transformers and exact current MV models, preserving their revisions and geometry. Applies an authored layout to held geographic context and writes a replayable report.layout.render request. No design writes or publication. --seed explicitly acquires missing context. Missing context is named in the receipt; inspect it before rendering. Detailed poles and customers are omitted from the overview.",
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
            "Optional local DS Grid draft included in the print context and receipt with its exact digest.",
        ),
        Arg::value(
            "focus-bounds",
            "<west,south,east,north>",
            "Optional WGS84 plan-page bounds for acquiring and reading geographic context around one MV sheet.",
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
    output: "Project, transformer count, exact MV model provenance, source revisions, omitted context, and render request path.",
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
pub fn run(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(i.require("layout")?)
        .map_err(invalid)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid("layout exceeds 16 MiB"));
    }
    let layout: printing::Layout = serde_json::from_slice(&bytes).map_err(invalid)?;
    printing::validate(&layout).map_err(invalid)?;
    let out = PathBuf::from(i.require("out-dir")?);
    if out.exists() {
        return Err(Failure::invalid(
            "report_output_exists",
            "map output directory already exists",
        )
        .remedy("Choose a fresh --out-dir"));
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
    let contexts = ds_cli_auth::transformer_contexts_for_project(lane, project, &active)?;
    if contexts.identity() != identity || contexts.project_id() != project {
        return Err(invalid("transformer context scope changed"));
    }
    for (name, response) in active.iter().zip(contexts.into_result()) {
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
    let network = sources.layers();
    // Context coverage belongs to the physical page, while the overview's
    // complete design remains available to the shared map painter. An MV
    // route can span a district; its union rectangle is not a sheet extent.
    let context_network = if let Some(raw) = i.value("focus-bounds") {
        let values = raw
            .split(',')
            .map(|part| part.trim().parse::<f64>().map_err(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        if values.len() != 4
            || values.iter().any(|v| !v.is_finite())
            || !(-180.0..=180.0).contains(&values[0])
            || !(-90.0..=90.0).contains(&values[1])
            || !(-180.0..=180.0).contains(&values[2])
            || !(-90.0..=90.0).contains(&values[3])
            || values[2] <= values[0]
            || values[3] <= values[1]
            || values[2] - values[0] > 0.05
            || values[3] - values[1] > 0.05
        {
            return Err(invalid(
                "--focus-bounds needs a valid WGS84 page rectangle no wider than 0.05 degrees",
            ));
        }
        json!({"mv_sheet": {"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[values[0],values[1]],[values[2],values[3]]]}}]}})
    } else {
        network.clone()
    };
    let catalog = ds_project_data::validate_resources(&ds_cli_auth::data_distribution(
        lane,
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
    let mut provider = ds_cli_data::project_cache::CliProvider { lane };
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
    let extent = sources.overview_extent().map_err(invalid)?;
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
    let render = json!({"schema":"ds.print-layout-export/v1","render":{"layout":layout,"layers":sources.vectors(),"extent":extent,"focus_extent":extent,"print_styles":sheets["printing_styles"],"symbol_assets":sheets.get("printing_symbol_assets").cloned().unwrap_or_else(||json!({})),"text":{"project":project,"transformer":layout.name}},"formats":["pdf","png"],"dpi":300,"out_dir":out.join("rendered")});
    let path = out.join("render-request.json");
    let data = serde_json::to_vec(&render).map_err(invalid)?;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(invalid)?
        .write_all(&data)
        .map_err(invalid)?;
    let result = json!({"project":project,"transformer_count":revisions.len(),"sources":revisions,"mv_models":super::mv_context::provenance(&models),"omitted":context.omitted.iter().map(|o|json!({"layer":o.layer,"reason":o.reason})).collect::<Vec<_>>(),"warnings":context.warnings,"request":path,"sha256":ds_command_kernel::report_export::sha256_hex(&data)});
    std::fs::write(
        out.join("sources.json"),
        serde_json::to_vec_pretty(&result).map_err(invalid)?,
    )
    .map_err(invalid)?;
    Ok(result)
}
