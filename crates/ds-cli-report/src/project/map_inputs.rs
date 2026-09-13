//! Native source acquisition only; overview projection and extents are kernel-owned.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution};
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
    let inventory =
        ds_cli_auth::transformer_inventory(lane, &ds_cli_auth::TransformerSet::default())?;
    let identity = inventory.identity();
    let project = inventory.project_id();
    let config = ds_cli_auth::feeder_configuration_receipt(lane, None)?;
    if config.identity() != identity || config.project_id() != project {
        return Err(invalid("configuration scope changed"));
    }
    let receipt = InputReceipt::from_config(&config.result().document).map_err(invalid)?;
    let sheets = receipt.sheets().map_err(invalid)?;
    printing::style_overrides::preflight(&layout, &sheets["printing_styles"]).map_err(invalid)?;
    let models = super::mv_context::load(lane, identity, project)?;
    let mut sources = printing::project_sources::Sources::default();
    sources
        .collections(&super::mv_context::overview(&models)?)
        .map_err(invalid)?;
    let mut revisions = Vec::new();
    for row in inventory.result().rows() {
        if row.kind() != ds_cli_auth::TransformerKind::Transformer
            || row.lifecycle() != ds_cli_auth::TransformerLifecycle::Active
        {
            continue;
        }
        let response =
            super::export::with_weak_network(super::export::WEAK_NETWORK_DELAYS, || {
                ds_cli_auth::transformer_context(lane, row.name())
            })?;
        let snapshot = response.snapshot();
        if response.identity() != identity
            || snapshot.ds_project() != project
            || snapshot.transformer_name() != row.name()
        {
            return Err(invalid("transformer scope changed"));
        }
        sources
            .transformer(
                row.name(),
                &serde_json::to_value(snapshot.layers()).map_err(invalid)?,
            )
            .map_err(invalid)?;
        revisions.push(json!({"transformer":row.name(),"version":snapshot.metadata().version(),"content_digest":snapshot.metadata().content_digest()}));
    }
    let network = sources.layers();
    let extent =
        ds_command_kernel::project_design_extent::design_extent("mv_data", &network, 0.00001)
            .map_err(invalid)?
            .bounds
            .ok_or_else(|| invalid("Project has no geographic design"))?;
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
    let mode = if i.switch("seed") {
        ds_project_data::Mode::Acquire(&mut hosts)
    } else {
        ds_project_data::Mode::Read
    };
    let context = ds_project_data::read_print_context(
        &ds_report_host::shared_root().map_err(invalid)?,
        &scope,
        "mv_data",
        &network,
        &contexts,
        &catalog,
        mode,
    )
    .map_err(invalid)?;
    if let Some(bytes) = context.document {
        let document: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        sources.collections(&document["layers"]).map_err(invalid)?;
    }
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
