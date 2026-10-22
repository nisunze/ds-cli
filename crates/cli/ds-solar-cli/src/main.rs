//! `ds-solar` — local Solar execution and parity tooling.
//!
//! The CLI is one of three adapters over the same runtime, and it models the
//! same prepared-input flow as the product. Local `prepare` reads verified
//! cache entries or frozen fixtures; acquisition belongs to the Kernel native
//! application. `run` receives complete prepared inputs. Besides its frozen
//! parity corpus, the CLI accepts a canonical `SolarCityInput` exported by an
//! approved client. It never reads browser storage itself.

use ds_solar_reference_host::{
    FixtureReferenceProvider, ReferenceBundleProvider, ReferencePreparer, Refresh,
};
mod fixtures;
use ds_solar_addon::prepared_task;
use ds_solar_contracts::result_comparison::comparison_receipt;
mod project;
use ds_solar_project::intake::{
    GovernedCityIntake, GovernedIntakeAuthority, GovernedSnapshot, governed_intake,
};

use ds_solar_project::files::read_bounded;
use ds_solar_project::preparation::{
    load_prepared, load_prepared_for_run, prepared_publication_claim, validate_governed_intake,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use ds_solar_contracts::{
    BatchArtifact, PortfolioCalculationAssumptions, PortfolioDocumentFormat,
    PortfolioDocumentOptions, PortfolioIdentity, PortfolioLanguage, PortfolioMember,
    PortfolioReportIntent, PortfolioRepresentativeStrategy, PortfolioRunInput, ReferencePin,
    SolarArtifactBatch, SolarCalculationResult, SolarCityInput, WeatherDataset,
};
use ds_solar_runtime::{
    CalculationOptions, CollectingProgressSink, Concurrency, DirectoryDebugSink,
    DirectoryReferenceCache, SolarRuntime, run_cities, run_portfolio,
};
use sha2::{Digest, Sha256};

/// JSON and artifact bounds are enforced before allocation. The batch schema
/// permits 25,000 long logical descriptors, so its manifest gets a larger
/// allowance than either bounded city projection. A chart is the largest
/// source artifact admitted by the portfolio graph contract.
const MAX_BATCH_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXTERNAL_JSON_BYTES: u64 = 64 * 1024 * 1024;
// Matches ds-brain's bounded Solar response lane. The outer JSON may be much
// larger than snapshot_json because the latter is escaped and the response can
// carry up to 512 short-lived media download entries.
const MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;
// The intake drops signed media URLs but pretty-prints a normalised city. Keep
// this a distinct contract boundary even though it currently shares the lane.
const MAX_GOVERNED_INTAKE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PORTFOLIO_CONTRIBUTION_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PORTFOLIO_CALCULATION_BYTES: u64 = 32 * 1024 * 1024;
const MAX_RESULT_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SOURCE_ARTIFACT_BYTES: u64 = 100 * 1024 * 1024;

/// Local Solar execution and parity tooling.
#[derive(Debug, Parser)]
#[command(name = "ds-solar", version, about)]
struct Cli {
    /// Fixture directory holding `city/`, `weather/` and `golden/`.
    #[arg(long, global = true, default_value = "fixtures")]
    fixtures: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Durable offline project inputs, runs, drafts and pending uploads.
    Project(project::Args),
    /// Print the immutable ds.engine-build/v1 identity as JSON.
    BuildInfo,
    /// Calculate one complete prepared city into a fresh native artifact directory.
    CalculatePrepared(prepared_task::Args),
    /// Validate one ds-brain governed snapshot into a durable native intake.
    Intake(IntakeArgs),
    /// Prepare cities from verified local cache or frozen fixtures.
    Prepare(PrepareArgs),
    /// Run prepared cities. Performs no intake or network call of any kind.
    Run(RunArgs),
    /// Aggregate one exact verified city batch into a closed portfolio run.
    Portfolio(PortfolioArgs),
    /// Re-derive a weather dataset's content digest and validate it.
    VerifyWeather(VerifyWeatherArgs),
    /// Compare the engine against the frozen Python summary for one city.
    Parity(ParityArgs),
    /// Compare two result documents by canonical digest.
    Compare(CompareArgs),
    /// Measure a release run: wall time, per-city time, peak memory, bytes.
    Bench(BenchArgs),
}

#[derive(Debug, clap::Args)]
struct IntakeArgs {
    /// Exact `data` object returned by the fixed ds-brain `desktop_snapshot`
    /// Solar action, or `-` to read that object from bounded stdin. The
    /// response envelope is not accepted.
    #[arg(long)]
    snapshot: PathBuf,
    /// Fresh file that receives `ds-solar.governed-city-intake/v1`.
    #[arg(long, short)]
    out: PathBuf,
}

#[derive(Debug, clap::Args)]
struct PrepareArgs {
    /// Canonical `ds-solar.city-input/v1` document exported by an approved
    /// client. Repeat for a multi-city batch. The file carries its project,
    /// root and provenance; this CLI never opens IndexedDB or Firestore.
    #[arg(long = "city-input", value_name = "file", conflicts_with = "city")]
    city_inputs: Vec<PathBuf>,
    /// Governed intake written by `ds-solar intake`. Repeat for a multi-city
    /// batch. Publication authority remains in this sidecar while the sealed
    /// city input enters preparation.
    #[arg(
        long = "governed-intake",
        value_name = "file",
        conflicts_with_all = ["city", "city_inputs"]
    )]
    governed_intakes: Vec<PathBuf>,
    /// Fixture city context ids. Omit to prepare every city in the fixture set.
    #[arg(long, conflicts_with = "city_inputs")]
    city: Vec<String>,
    /// Project id recorded in *fixture* prepared inputs. A canonical city
    /// input already carries its own project identity.
    #[arg(long)]
    project_id: Option<String>,
    /// Root collection recorded in *fixture* prepared inputs. A canonical
    /// city input already carries its own root identity.
    #[arg(long)]
    root: Option<String>,
    /// Where to write prepared inputs.
    #[arg(long, short)]
    out: PathBuf,
    /// Versioned reference-bundle cache directory.
    #[arg(long)]
    cache: Option<PathBuf>,
    /// Rebuild fixture-backed cache entries instead of reusing held entries.
    /// Real project inputs require an existing verified local cache entry.
    #[arg(long)]
    overwrite: bool,
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    /// Directory of prepared inputs, as written by `prepare`.
    #[arg(long)]
    prepared: PathBuf,
    /// City context ids to run. Omit to run every prepared input.
    #[arg(long)]
    city: Vec<String>,
    /// Directory to write results, the batch and any charts into.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Render chart artifacts.
    #[arg(long)]
    charts: bool,
    /// Cities to run at once. Defaults to available CPUs; 1 runs one city at a time.
    #[arg(long, default_value_t = ds_solar_runtime::compute::compute_parallelism())]
    concurrency: usize,
    /// Run id echoed into results. Excluded from result digests.
    #[arg(long)]
    run_id: Option<String>,
    /// Write diagnostic stage snapshots into this directory. Off by default;
    /// when off, no debug work of any kind is performed.
    #[arg(long)]
    debug_artifacts: Option<PathBuf>,
    /// Also write human-readable CSV renderings of debug tables.
    #[arg(long, requires = "debug_artifacts")]
    debug_csv: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum PortfolioLanguageArg {
    Fr,
    En,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum PortfolioReportArg {
    Apd,
    Network,
    Plant,
    Financial,
}

#[derive(Debug, clap::Args)]
struct PortfolioArgs {
    /// Exact locally committed city-run directory containing `batch.json`.
    #[arg(long)]
    source: PathBuf,
    /// Exact source run id expected in the source batch receipt.
    #[arg(long)]
    source_run_id: String,
    /// Ordered portfolio member city ids. Must exactly equal source-batch order.
    #[arg(long, required = true)]
    city: Vec<String>,
    /// Stable project-owned portfolio id.
    #[arg(long)]
    portfolio_id: String,
    /// Human-facing name sealed into this membership revision.
    #[arg(long)]
    portfolio_name: String,
    /// Exact collaborative membership revision.
    #[arg(long)]
    membership_revision: String,
    /// Unique portfolio calculation run id.
    #[arg(long)]
    run_id: String,
    /// Explicit uppercase three-letter presentation currency.
    #[arg(long)]
    currency: String,
    /// Explicit project horizon in years (1..=100).
    #[arg(long)]
    project_years: u32,
    /// Explicit decimal discount rate in [0, 1).
    #[arg(long)]
    discount_rate: f64,
    /// Explicit representative city; must be one of the ordered members.
    #[arg(long)]
    representative_city: String,
    /// Report language.
    #[arg(long, value_enum)]
    language: PortfolioLanguageArg,
    /// Report intent. Repeat to request multiple deterministic Markdown files.
    #[arg(long, value_enum, required = true)]
    report: Vec<PortfolioReportArg>,
    /// Fresh directory to atomically publish the portfolio run into.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, clap::Args)]
struct VerifyWeatherArgs {
    /// Weather dataset JSON to check.
    #[arg(long)]
    file: PathBuf,
}

#[derive(Debug, clap::Args)]
struct ParityArgs {
    /// City context id. Omit to check every city in the fixture set.
    #[arg(long)]
    city: Option<String>,
}

#[derive(Debug, clap::Args)]
struct CompareArgs {
    /// A result document.
    #[arg(long)]
    left: PathBuf,
    /// Another result document.
    #[arg(long)]
    right: PathBuf,
    /// Emit one bounded machine-readable comparison receipt.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, clap::Args)]
struct BenchArgs {
    /// Directory of prepared inputs.
    #[arg(long)]
    prepared: PathBuf,
    /// How many times to repeat the whole run.
    #[arg(long, default_value_t = 3)]
    repeat: usize,
    /// Cities to run at once.
    #[arg(long, default_value_t = ds_solar_runtime::compute::compute_parallelism())]
    concurrency: usize,
    /// Render chart artifacts, which dominates the measurement when on.
    #[arg(long)]
    charts: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let logging = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false);
    if matches!(&cli.command, Command::CalculatePrepared(_)) {
        logging.with_writer(std::io::stderr).init();
    } else {
        logging.init();
    }
    let set = fixtures::FixtureSet::new(&cli.fixtures);
    match cli.command {
        Command::Project(args) => project::execute(args),
        Command::BuildInfo => build_info(),
        Command::CalculatePrepared(args) => prepared_task::run(args),
        Command::Intake(args) => intake(args),
        Command::Prepare(args) => prepare(&set, args),
        Command::Run(args) => run(args),
        Command::Portfolio(args) => portfolio(args),
        Command::VerifyWeather(args) => verify_weather(args),
        Command::Parity(args) => parity(&set, args),
        Command::Compare(args) => compare(args),
        Command::Bench(args) => bench(args),
    }
}

fn intake(args: IntakeArgs) -> Result<()> {
    let governed: GovernedSnapshot = if args.snapshot == Path::new("-") {
        let stdin = std::io::stdin();
        read_json_stream_bounded(
            stdin.lock(),
            MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES,
            "governed Solar snapshot on stdin",
        )?
    } else {
        read_json_bounded(
            &args.snapshot,
            MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES,
            "governed Solar snapshot",
        )?
    };
    let intake = governed_intake(governed)?;
    write_json_new(&args.out, &intake)?;
    println!(
        "intake {} {} {}",
        intake.city_input.identity.project_id,
        intake.city_input.identity.city_id,
        intake.city_input.content_digest
    );
    Ok(())
}

fn build_info() -> Result<()> {
    ds_solar_addon::build_info()
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn prepare(set: &fixtures::FixtureSet, args: PrepareArgs) -> Result<()> {
    let fixture_mode = args.city_inputs.is_empty() && args.governed_intakes.is_empty();
    if !fixture_mode && (args.project_id.is_some() || args.root.is_some()) {
        bail!(
            "--project-id and --root are fixture-only; each --city-input carries its own identity"
        );
    }
    if !fixture_mode && args.cache.is_none() {
        bail!(
            "--city-input/--governed-intake requires --cache; the sealed weather/reference bundle must be retained locally"
        );
    }

    let (inputs, governed_authorities) = if fixture_mode {
        let cities = if args.city.is_empty() {
            set.city_ids()?
        } else {
            args.city.clone()
        };
        let project_id = args.project_id.as_deref().unwrap_or("ds-solar-fixtures");
        let root = args.root.as_deref().unwrap_or("solar");
        (
            cities
                .iter()
                .map(|city| set.city_input(city, project_id, root))
                .collect::<Result<Vec<_>>>()?,
            BTreeMap::new(),
        )
    } else if !args.city_inputs.is_empty() {
        (load_city_inputs(&args.city_inputs)?, BTreeMap::new())
    } else {
        load_governed_intakes(&args.governed_intakes)?
    };

    // Frozen bundles remain fixture-only. Real inputs prepare from their
    // verified local cache; Kernel native preparation owns acquisition.
    let fixture_provider = if fixture_mode {
        Some(FixtureReferenceProvider::new(
            set.all_reference_bundles()?,
            ReferencePin::current(),
        )?)
    } else {
        None
    };
    let provider: Option<&dyn ReferenceBundleProvider> = fixture_provider
        .as_ref()
        .map(|provider| provider as &dyn ReferenceBundleProvider);
    let cache = args.cache.as_ref().map(DirectoryReferenceCache::new);

    let mut preparer = ReferencePreparer::new().with_refresh(if args.overwrite {
        Refresh::Overwrite
    } else {
        Refresh::UseCache
    });
    if let Some(provider) = provider {
        preparer = preparer.with_provider(provider);
    }
    if let Some(cache) = &cache {
        preparer = preparer.with_cache(cache);
    }

    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;
    // Check the whole governed batch before any preparation or publication.
    // This prevents a stale claim for a later city from leaving earlier cities
    // as a newly published partial batch.
    for input in &inputs {
        if governed_authorities.contains_key(&input.identity.city_id) {
            let city = &input.identity.city_id;
            let prepared_path = args.out.join(format!("{city}.prepared.json"));
            let claim_path = args
                .out
                .join(format!("{city}.prepared-publication-claim.json"));
            require_create_new_paths([prepared_path.as_path(), claim_path.as_path()])?;
        }
    }
    let input_count = inputs.len();
    for input in inputs {
        let city = input.identity.city_id.clone();
        let path = args.out.join(format!("{city}.prepared.json"));
        let claim_path = args
            .out
            .join(format!("{city}.prepared-publication-claim.json"));
        let governed_authority = governed_authorities.get(&city);
        let prepared = preparer.prepare(input, now())?;
        let publication_claim = governed_authority
            .map(|authority| prepared_publication_claim(&prepared.input, authority))
            .transpose()?;
        if let Some(claim) = publication_claim {
            // A governed preparation is a closed two-document handoff. Making
            // the prepared input create-new prevents a retry from replacing
            // bytes already named by an existing publication claim.
            write_json_new(&path, &prepared.input)?;
            write_json_new(&claim_path, &claim)?;
        } else {
            write_json(&path, &prepared.input)?;
        }
        println!(
            "{city:<16} {:<8} {}  {}  {}",
            prepared.resolution.as_str(),
            &prepared.input.input_digest[..23],
            &prepared.input.reference_unit.receipt.source_key[..23],
            prepared.input.weather_key.cache_entry()
        );
    }
    println!("{input_count} prepared into {}", args.out.display());
    Ok(())
}

fn require_create_new_paths<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Result<()> {
    for path in paths {
        match std::fs::symlink_metadata(path) {
            Ok(_) => bail!("refusing to replace {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("checking {}", path.display()));
            }
        }
    }
    Ok(())
}

/// Read explicit city inputs that an approved application exported.
///
/// An external caller cannot point this adapter at a browser cache: the
/// document is the intentional boundary.  We require a sealed digest and a
/// safe, unique city id before it becomes an output file name.
fn load_city_inputs(paths: &[PathBuf]) -> Result<Vec<SolarCityInput>> {
    let mut inputs = Vec::with_capacity(paths.len());
    let mut city_ids = BTreeSet::new();
    for path in paths {
        let input: SolarCityInput =
            read_json_bounded(path, MAX_EXTERNAL_JSON_BYTES, "Solar city input")?;
        if input.content_digest.is_empty() {
            bail!(
                "{} is not a sealed ds-solar.city-input/v1 document; export it from DS GridDesign first",
                path.display()
            );
        }
        input
            .validate()
            .with_context(|| format!("validating city input {}", path.display()))?;
        ensure_safe_city_id(&input.identity.city_id)?;
        if !city_ids.insert(input.identity.city_id.clone()) {
            bail!(
                "city input documents repeat city id {:?}; prepare each city once per batch",
                input.identity.city_id
            );
        }
        inputs.push(input);
    }
    inputs.sort_by(|left, right| left.identity.city_id.cmp(&right.identity.city_id));
    Ok(inputs)
}

fn load_governed_intakes(
    paths: &[PathBuf],
) -> Result<(
    Vec<SolarCityInput>,
    BTreeMap<String, GovernedIntakeAuthority>,
)> {
    let mut inputs = Vec::with_capacity(paths.len());
    let mut city_ids = BTreeSet::new();
    let mut authorities = BTreeMap::new();
    for path in paths {
        let intake: GovernedCityIntake =
            read_json_bounded(path, MAX_GOVERNED_INTAKE_BYTES, "governed Solar intake")?;
        validate_governed_intake(&intake)
            .with_context(|| format!("validating governed intake {}", path.display()))?;
        if !city_ids.insert(intake.city_input.identity.city_id.clone()) {
            bail!(
                "governed intake documents repeat city id {:?}",
                intake.city_input.identity.city_id
            );
        }
        authorities.insert(intake.city_input.identity.city_id.clone(), intake.authority);
        inputs.push(intake.city_input);
    }
    inputs.sort_by(|left, right| left.identity.city_id.cmp(&right.identity.city_id));
    Ok((inputs, authorities))
}

fn ensure_safe_city_id(city_id: &str) -> Result<()> {
    if !ds_solar_contracts::is_solar_id(city_id) {
        bail!(
            "city input id {:?} is not a safe single path segment",
            city_id
        );
    }
    Ok(())
}

fn run(args: RunArgs) -> Result<()> {
    let prepared = load_prepared_for_run(&args.prepared, &args.city)?;

    let progress = CollectingProgressSink::new();
    let debug = args
        .debug_artifacts
        .as_ref()
        .map(|root| DirectoryDebugSink::new(root).with_csv(args.debug_csv));

    let mut runtime = SolarRuntime::new().with_progress(&progress);
    if let Some(sink) = &debug {
        runtime = runtime.with_debug(sink);
    }

    // Charts and documents move together: the APD is what the charts are for,
    // and "Skip plots" produces a document with no image references rather than
    // a document with broken ones.
    let mut options = CalculationOptions::default()
        .with_charts(args.charts)
        .with_documents(true);
    if let Some(run_id) = &args.run_id {
        options = options.with_run_id(run_id.clone());
    }

    let identity = &prepared[0].city.identity;
    let started = std::time::Instant::now();
    let output = run_cities(
        &runtime,
        &prepared,
        &options,
        Concurrency::new(args.concurrency),
        &identity.project_id,
        &identity.root,
    )?;
    let elapsed = started.elapsed();

    for city in &output.cities {
        match &city.outcome {
            // Three phases, printed separately because they fail and scale
            // separately: a slow pipeline is an engine problem, slow charts a
            // rendering one, and slow documents a composition one.
            Ok(outcome) => println!(
                "ok    {:<16} {}  pipeline {} ms · charts {} ms · documents {} ms",
                city.city_id,
                outcome.result.result_digest,
                outcome.timings.pipeline_ms,
                outcome.timings.charts_ms,
                outcome.timings.documents_ms
            ),
            Err(error) => println!(
                "FAIL  {:<16} {}: {}",
                city.city_id,
                error.code.as_str(),
                error.message
            ),
        }
        if let Some(Err(error)) = &city.portfolio_projection {
            eprintln!(
                "portfolio-ineligible {:<16} {}: {}",
                city.city_id,
                error.code.as_str(),
                error.message
            );
        }
    }
    println!(
        "batch {}  {} artifacts  {} bytes  {:?}",
        output.batch.batch_id,
        output.batch.artifacts.len(),
        output.batch.total_bytes(),
        elapsed
    );

    if let Some(out) = &args.out {
        write_run(out, &output, &progress)?;
        println!("written {}", out.display());
    }
    if let Some(sink) = &debug {
        let index = sink.finish()?;
        println!("debug   {}", index.display());
    }

    let failures = output.failures().len();
    if failures > 0 {
        bail!("{failures} of {} cities failed", output.cities.len());
    }
    Ok(())
}

fn portfolio(args: PortfolioArgs) -> Result<()> {
    let source_batch: SolarArtifactBatch = read_json_bounded(
        &args.source.join("batch.json"),
        MAX_BATCH_MANIFEST_BYTES,
        "Solar batch manifest",
    )?;
    source_batch.validate()?;
    verify_batch_files(&args.source, &source_batch)?;
    if source_batch.run.run_id != args.source_run_id {
        bail!(
            "source batch run id {:?} does not match explicit --source-run-id {:?}",
            source_batch.run.run_id,
            args.source_run_id
        );
    }

    if !(2..=200).contains(&args.city.len()) {
        bail!(
            "portfolio membership must contain 2..=200 repeated --city values; found {}",
            args.city.len()
        );
    }
    let mut declared = BTreeSet::new();
    for city_id in &args.city {
        ensure_safe_city_id(city_id)?;
        if !declared.insert(city_id.as_str()) {
            bail!("portfolio member city {city_id:?} appears more than once");
        }
    }
    let source_city_ids = source_batch
        .run
        .cities
        .iter()
        .map(|city| city.city_id.as_str())
        .collect::<Vec<_>>();
    let requested_city_ids = args.city.iter().map(String::as_str).collect::<Vec<_>>();
    if source_city_ids != requested_city_ids {
        bail!(
            "ordered --city membership must exactly equal source batch order; source is [{}], requested [{}]",
            source_city_ids.join(", "),
            requested_city_ids.join(", ")
        );
    }
    if !declared.contains(args.representative_city.as_str()) {
        bail!(
            "--representative-city {:?} is not an ordered portfolio member",
            args.representative_city
        );
    }

    let mut contributions = Vec::with_capacity(args.city.len());
    let mut calculations = Vec::with_capacity(args.city.len());
    for city_id in &args.city {
        let city_dir = args.source.join("cities").join(city_id);
        contributions.push(read_json_bounded::<
            ds_solar_contracts::PortfolioCityContribution,
        >(
            &city_dir.join(ds_solar_runtime::PORTFOLIO_CONTRIBUTION_NAME),
            MAX_PORTFOLIO_CONTRIBUTION_BYTES,
            "portfolio city contribution",
        )?);
        calculations.push(read_json_bounded::<
            ds_solar_runtime::PortfolioCityCalculation,
        >(
            &city_dir.join(ds_solar_runtime::PORTFOLIO_CALCULATION_NAME),
            MAX_PORTFOLIO_CALCULATION_BYTES,
            "portfolio city calculation",
        )?);
    }
    let members = contributions
        .iter()
        .zip(&calculations)
        .map(|(contribution, calculation)| {
            PortfolioMember::bind(
                contribution,
                calculation.projection_digest.clone(),
                source_batch.batch_id.clone(),
                source_batch.batch_digest.clone(),
            )
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let language = match args.language {
        PortfolioLanguageArg::Fr => PortfolioLanguage::Fr,
        PortfolioLanguageArg::En => PortfolioLanguage::En,
    };
    let report_intents = args
        .report
        .iter()
        .map(|intent| match intent {
            PortfolioReportArg::Apd => PortfolioReportIntent::Apd,
            PortfolioReportArg::Network => PortfolioReportIntent::Network,
            PortfolioReportArg::Plant => PortfolioReportIntent::Plant,
            PortfolioReportArg::Financial => PortfolioReportIntent::Financial,
        })
        .collect();
    let input = PortfolioRunInput::close(
        PortfolioIdentity {
            project_id: source_batch.run.project_id.clone(),
            root: source_batch.run.root.clone(),
            portfolio_id: args.portfolio_id,
            portfolio_name: args.portfolio_name,
            membership_revision: args.membership_revision,
        },
        PortfolioCalculationAssumptions::new(
            args.currency,
            args.project_years,
            args.discount_rate,
        )?,
        args.run_id,
        members,
        PortfolioRepresentativeStrategy::Explicit {
            city_id: args.representative_city,
        },
        language,
        PortfolioDocumentOptions {
            report_intents,
            formats: vec![PortfolioDocumentFormat::Markdown],
        },
    )?;
    let output = run_portfolio(
        &input,
        &contributions,
        &calculations,
        std::slice::from_ref(&source_batch),
    )?;
    write_portfolio_run(&args.out, &output)?;
    println!(
        "portfolio {} revision {}  {} cities  batch {}  {}",
        output.result.portfolio_id,
        output.result.membership_revision,
        output.result.city_count,
        output.batch.batch_id,
        args.out.display()
    );
    Ok(())
}

fn write_portfolio_run(out: &Path, output: &ds_solar_runtime::PortfolioRunOutput) -> Result<()> {
    output.batch.validate()?;
    if out.exists() {
        bail!(
            "refusing to replace existing portfolio directory {}; choose a fresh --out path",
            out.display()
        );
    }
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating output parent {}", parent.display()))?;
    let leaf = out
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .context("--out must name a concrete directory")?;
    let staging = parent.join(format!(
        ".{leaf}.{}.{}.staging",
        output.batch.batch_id,
        std::process::id()
    ));
    if staging.exists() {
        bail!(
            "staging directory already exists at {}; remove or inspect it before retrying",
            staging.display()
        );
    }
    std::fs::create_dir(&staging)
        .with_context(|| format!("creating staging directory {}", staging.display()))?;
    // Consolidated figures are written with the documents that reference them.
    // `verify_batch_files` then refuses a batch whose report points at a chart
    // nobody wrote, which is how a missing figure becomes a failed export
    // rather than a broken image in a delivered report.
    for artifact in std::iter::once(&output.input_artifact)
        .chain(std::iter::once(&output.result_artifact))
        .chain(output.documents.iter())
        .chain(output.charts.iter())
    {
        write_bytes(&staging.join(artifact.name()), &artifact.bytes)?;
    }
    verify_batch_files(&staging, &output.batch)?;
    write_json(&staging.join("batch.json"), &output.batch)?;
    write_json(
        &staging.join("sync-intent.json"),
        &output.batch.sync_intent(now()),
    )?;
    std::fs::rename(&staging, out).with_context(|| {
        format!(
            "atomically publishing {} as {}",
            staging.display(),
            out.display()
        )
    })?;
    Ok(())
}

fn write_run(
    out: &Path,
    output: &ds_solar_runtime::RunOutput,
    progress: &CollectingProgressSink,
) -> Result<()> {
    ds_solar_project::files::commit_run_with_progress(
        out,
        output,
        Some(&serde_json::to_value(progress.events())?),
    )
}

fn verify_batch_files(root: &Path, batch: &ds_solar_contracts::SolarArtifactBatch) -> Result<()> {
    for artifact in &batch.artifacts {
        verify_batch_artifact(root, artifact, MAX_SOURCE_ARTIFACT_BYTES)?;
    }
    Ok(())
}

/// Stream one declared artifact through a fixed buffer. Both the descriptor and
/// the opened file handle must fit the cap before hashing begins; `take(max+1)`
/// also catches a file that grows after its metadata was inspected.
fn verify_batch_artifact(root: &Path, artifact: &BatchArtifact, max_bytes: u64) -> Result<()> {
    if artifact.byte_count > max_bytes {
        bail!(
            "batch artifact {} declares {} bytes, exceeding the {max_bytes}-byte source artifact bound",
            artifact.name,
            artifact.byte_count
        );
    }
    let path = root.join(&artifact.name);
    let file = File::open(&path).with_context(|| {
        format!(
            "batch artifact {} is missing from staged output {}",
            artifact.name,
            path.display()
        )
    })?;
    let metadata = file
        .metadata()
        .with_context(|| format!("inspecting batch artifact {}", path.display()))?;
    if !metadata.is_file() {
        bail!("batch artifact {} is not a regular file", artifact.name);
    }
    if metadata.len() > max_bytes {
        bail!(
            "batch artifact {} has {} bytes on disk, exceeding the {max_bytes}-byte source artifact bound",
            artifact.name,
            metadata.len()
        );
    }
    if metadata.len() != artifact.byte_count {
        bail!(
            "batch artifact {} has {} bytes on disk, expected {}",
            artifact.name,
            metadata.len(),
            artifact.byte_count
        );
    }

    let read_limit = artifact
        .byte_count
        .checked_add(1)
        .context("batch artifact read bound overflowed")?;
    let mut reader = BufReader::new(file).take(read_limit);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes_read = 0_u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .with_context(|| format!("reading batch artifact {}", path.display()))?;
        if count == 0 {
            break;
        }
        bytes_read = bytes_read
            .checked_add(count as u64)
            .context("batch artifact byte count overflowed")?;
        if bytes_read > artifact.byte_count {
            bail!(
                "batch artifact {} grew beyond its declared {} bytes while it was read",
                artifact.name,
                artifact.byte_count
            );
        }
        hasher.update(&buffer[..count]);
    }
    if bytes_read != artifact.byte_count {
        bail!(
            "batch artifact {} yielded {bytes_read} bytes, expected {}",
            artifact.name,
            artifact.byte_count
        );
    }
    let digest = format!("sha256:{:x}", hasher.finalize());
    if digest != artifact.content_digest {
        bail!(
            "batch artifact {} digest {} does not match {}",
            artifact.name,
            digest,
            artifact.content_digest
        );
    }
    Ok(())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn read_json_bounded<T: serde::de::DeserializeOwned>(
    path: &Path,
    max_bytes: u64,
    label: &str,
) -> Result<T> {
    let bytes = read_bounded(path, max_bytes, label)?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

/// Decode one bounded stream without logging or formatting its raw bytes.
///
/// This is intentionally used only for governed `intake --snapshot -`.
/// Every pathname reader continues through `read_bounded`, including its
/// opened-handle regular-file and size-stability checks.
fn read_json_stream_bounded<T: serde::de::DeserializeOwned>(
    reader: impl Read,
    max_bytes: u64,
    label: &str,
) -> Result<T> {
    let read_limit = max_bytes
        .checked_add(1)
        .context("stream read bound overflowed")?;
    let initial_capacity = usize::try_from(max_bytes.min(64 * 1024))
        .context("stream read bound does not fit this platform's address space")?;
    let mut bytes = Vec::with_capacity(initial_capacity);
    reader
        .take(read_limit)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {label}"))?;
    if bytes.len() as u64 > max_bytes {
        bail!("{label} exceeds the {max_bytes}-byte read bound");
    }
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {label}"))
}

fn verify_weather(args: VerifyWeatherArgs) -> Result<()> {
    let dataset: WeatherDataset =
        read_json_bounded(&args.file, MAX_EXTERNAL_JSON_BYTES, "Solar weather dataset")?;
    dataset.validate()?;
    let key = ds_solar_contracts::WeatherKey::of_dataset(&dataset)?;
    println!("schema   {}", dataset.schema_version);
    println!("key      {}", key.cache_entry());
    println!("records  {}", dataset.record_count());
    println!("digest   {}", dataset.content_digest);
    println!("valid    yes");
    Ok(())
}

fn parity(set: &fixtures::FixtureSet, args: ParityArgs) -> Result<()> {
    let cities = match &args.city {
        Some(city) => vec![city.clone()],
        None => set.city_ids()?,
    };
    let mut failures = 0;
    for city in &cities {
        let weather = set.weather(city)?;
        let golden = set.golden(city)?;
        let summary = ds_solar_engine::stages::site_weather::run(&weather)?;
        let actual = serde_json::to_value(&summary)?;
        let expected = &golden["weather_summary"];
        if &actual == expected {
            println!("ok    {city}");
        } else {
            failures += 1;
            println!("FAIL  {city}");
            println!("  rust   {actual}");
            println!("  python {expected}");
        }
    }
    if failures > 0 {
        bail!(
            "{failures} of {} cities differ from the Python oracle",
            cities.len()
        );
    }
    println!("{} cities match the Python oracle exactly", cities.len());
    Ok(())
}

fn compare(args: CompareArgs) -> Result<()> {
    let left: SolarCalculationResult =
        read_json_bounded(&args.left, MAX_RESULT_JSON_BYTES, "Solar result")?;
    let right: SolarCalculationResult =
        read_json_bounded(&args.right, MAX_RESULT_JSON_BYTES, "Solar result")?;
    if args.json {
        let receipt = comparison_receipt(&left, &right)?;
        println!("{}", serde_json::to_string(&receipt)?);
        return Ok(());
    }
    left.validate()?;
    right.validate()?;
    println!("{}  {}", left.result_digest, args.left.display());
    println!("{}  {}", right.result_digest, args.right.display());
    if left.result_digest != right.result_digest {
        bail!("the two results have different canonical digests");
    }
    println!("identical");
    Ok(())
}

fn bench(args: BenchArgs) -> Result<()> {
    let prepared = load_prepared(&args.prepared, &[])?;
    let runtime = SolarRuntime::new();
    let options = CalculationOptions::default().with_charts(args.charts);
    let identity = &prepared[0].city.identity;

    println!(
        "cities {}  concurrency {}  charts {}  repeats {}",
        prepared.len(),
        args.concurrency,
        args.charts,
        args.repeat
    );
    if cfg!(debug_assertions) {
        println!("WARNING: debug build — these numbers do not describe the shipped binary");
    }

    let mut digests: Option<Vec<String>> = None;
    let mut wall = Vec::with_capacity(args.repeat);
    for round in 1..=args.repeat {
        let started = std::time::Instant::now();
        let output = run_cities(
            &runtime,
            &prepared,
            &options,
            Concurrency::new(args.concurrency),
            &identity.project_id,
            &identity.root,
        )?;
        let elapsed = started.elapsed();
        wall.push(elapsed);

        let round_digests: Vec<String> = output
            .cities
            .iter()
            .map(|city| match &city.outcome {
                Ok(outcome) => outcome.result.result_digest.clone(),
                Err(error) => format!("error:{}", error.code.as_str()),
            })
            .collect();
        match &digests {
            None => digests = Some(round_digests),
            Some(first) => {
                if first != &round_digests {
                    bail!("round {round} produced different results from round 1");
                }
            }
        }
        println!(
            "round {round}  {:?}  batch {}",
            elapsed, output.batch.batch_id
        );
    }

    let total: std::time::Duration = wall.iter().sum();
    let mean = total / wall.len() as u32;
    let best = wall.iter().min().copied().unwrap_or_default();
    println!(
        "mean {:?}  best {:?}  per-city {:?}",
        mean,
        best,
        best / prepared.len() as u32
    );
    if let Some(peak) = peak_rss_kb() {
        println!("peak RSS {} MiB", peak / 1024);
    }
    println!("bytes written 0 — the run path writes nothing unless --out is given");
    Ok(())
}

/// Peak resident set size in KiB, on platforms that report it.
fn peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut encoded = serde_json::to_vec_pretty(value)?;
    encoded.push(b'\n');
    std::fs::write(path, encoded).with_context(|| format!("writing {}", path.display()))
}

fn write_json_new<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut encoded = serde_json::to_vec_pretty(value)?;
    encoded.push(b'\n');
    durable_create_new(path, &encoded)
}

static STAGED_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Publish complete bytes without ever exposing a partial destination path.
///
/// The staged inode is written and synced in the destination directory. A
/// hard link then creates the requested path atomically and fails if any
/// directory entry already occupies it, including a dangling symbolic link.
fn durable_create_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|candidate| !candidate.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let file_name = path
        .file_name()
        .context("create-new output path must name a file")?
        .to_string_lossy();

    let (staged_path, mut staged_file) = (0..128)
        .find_map(|_| {
            let sequence = STAGED_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!(
                ".{file_name}.stage.{}.{}",
                std::process::id(),
                sequence
            ));
            match open_private_create_new(&candidate) {
                Ok(file) => Some(Ok((candidate, file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error)),
            }
        })
        .transpose()
        .with_context(|| format!("staging {}", path.display()))?
        .context("could not allocate a same-directory staged output")?;

    let stage_result = (|| -> Result<()> {
        staged_file
            .write_all(bytes)
            .with_context(|| format!("staging {}", path.display()))?;
        staged_file
            .sync_all()
            .with_context(|| format!("syncing staged {}", path.display()))?;
        drop(staged_file);

        std::fs::hard_link(&staged_path, path)
            .with_context(|| format!("refusing to replace {}", path.display()))?;
        // Publication is the commit point. The linked destination names the
        // already-fsynced complete inode, so cleanup cannot turn success into
        // a false failure that an idempotent retry cannot repair.
        let _ = File::open(parent).and_then(|directory| directory.sync_all());
        let _ = std::fs::remove_file(&staged_path);
        let _ = File::open(parent).and_then(|directory| directory.sync_all());
        Ok(())
    })();

    if stage_result.is_err() {
        // Best-effort cleanup only removes our exact create-new staging path.
        // If the hard link succeeded, the destination remains a complete inode.
        let _ = std::fs::remove_file(&staged_path);
    }
    stage_result
}

fn open_private_create_new(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Governed intakes retain an actor-bound publication receipt. The
        // staged inode and its hard-linked destination are therefore private
        // even under a permissive process umask.
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
