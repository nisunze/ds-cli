//! Repeatable plan/profile print variants from one checked JSON document.
//! The Rust reporter owns pagination and every drawing byte; this command
//! only binds a shared, pinned request to named ink variants.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{DS_REPORT, EXPORT_TIMEOUT};

pub static COMMAND: Command = Command {
    id: "report.plan-profile-config",
    path: &["report", "plan-profile-config"],
    contract: 4,
    summary: "Print an atomic standard MV booklet from project JSON.",
    purpose: "Bind same-revision geometry and held approved assets to the project canonical MV setup, then render into the explicit --out directory. The reusable configuration contains no output destination. All printing furniture inherits the exact adopted revision and fixed version/date. Discover the V3 configuration with report plan-profile-config schema. The complete job commits one fresh output directory only after every booklet, preview and receipt succeeds.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::project::LANE_ARG,
        Arg::value(
            "project",
            "<id>",
            "Exact project context; must match the configuration.",
        )
        .required(),
        Arg::value(
            "config",
            "<file.json>",
            "Absolute path to a ds.grid-plan-profile-print/v3 JSON configuration.",
        )
        .required(),
        Arg::value(
            "out",
            "<directory>",
            "Fresh absolute output directory for this run; never stored in the configuration.",
        )
        .required(),
    ],
    output: "One atomic receipt with configuration SHA-256, exact model revision, ordered PNG previews, layout-plan digest and PDF digest for every booklet. Failure leaves the destination absent.",
    examples: &[Example {
        command: "ds report plan-profile-config --project gisagara --config /project/prints/plan-profile.json --out /project/prints/observation --output json",
        note: "Render every named variant from one pinned configuration; see docs/reference/report.md for the schema.",
        runnable: false,
    }],
    refusals: &crate::project::joined::<{ crate::project::NATIVE_READ_REFUSALS.len() + 10 }>(&[
        crate::project::NATIVE_READ_REFUSALS,
        &[
            crate::project::mv_setup::REFUSAL,
            crate::project::mv_setup::STYLE_REFUSAL,
            Refusal {
                code: "print_config_invalid",
                when: "the JSON is unreadable, ambiguous, unsafe, or names the wrong project",
                remedy: "correct the configuration schema, project, paths, settings and variant names",
            },
            Refusal {
                code: "projection_missing",
                when: "the declared scene or plan is missing",
                remedy: "materialize both projections from the pinned model revision",
            },
            Refusal {
                code: "output_exists",
                when: "the output root already exists",
                remedy: "pass a fresh --out directory",
            },
            Refusal {
                code: "output_create_failed",
                when: "the output root cannot be created",
                remedy: "check destination permissions and free space",
            },
            Refusal {
                code: "request_write_failed",
                when: "a typed reporter request cannot be written",
                remedy: "check temporary storage permissions and free space",
            },
            Refusal {
                code: "receipt_write_failed",
                when: "the batch receipt cannot be saved",
                remedy: "check destination permissions and free space; the destination remains absent on failure",
            },
            Refusal {
                code: "engine_refused",
                when: "the Rust reporter rejects a variant's source or settings",
                remedy: "read the reporter detail and correct the pinned inputs; no part of the job was committed",
            },
            Refusal {
                code: "reporter_engine_missing",
                when: "the Rust reporter is unavailable",
                remedy: "install the matching ds-report binary",
            },
        ],
    ]),
    reference: Some("docs/reference/report.md"),
    search: &["dsgrid"],
    requires: Requires::Server,
    availability,
};

pub static SCHEMA: Command = Command {
    id: "report.plan-profile-config.schema",
    path: &["report", "plan-profile-config", "schema"],
    contract: 4,
    summary: "Describe the JSON plan/profile print configuration.",
    purpose: "Return versioned geometry/asset fields and the per-run destination rule, the exact staking workbook Description binding schema, and a complete minimal example inheriting the canonical project printing setup. This is local discovery and does not render a sheet.",
    chapter: Chapter::Reports,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "Required geometry fields, canonical inheritance rule, path rule and a JSON example.",
    examples: &[Example {
        command: "ds report plan-profile-config schema --output json",
        note: "Get a JSON configuration example before rendering.",
        runnable: true,
    }],
    refusals: &[],
    reference: Some("docs/reference/report.md"),
    search: &[
        "dsgrid",
        "variants",
        "staking description",
        "workbook column",
    ],
    requires: Requires::Server,
    availability: schema_available,
};

fn schema_available() -> Availability {
    Availability::Available
}

pub fn schema_run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Ok(schema_document())
}

fn schema_document() -> Value {
    json!({"schema":"ds.grid-plan-profile-print/v3",
        "required":["schema","project_id","scene_path","plan_path"],
        "optional":["preview_only","variants","sample_pages","side_profiles_path","notes_path","structure_descriptions_path","model_crs","context_page_files","model_fields","publication_assets"],
        "alignment_context":ds_command_kernel::printing::mv_context::schema(),
        "structure_description_binding":ds_command_kernel::printing::structure_descriptions::schema(),
        "override_rule":"Declarative changes belong in the existing revision-fenced project layout (mv.settings, pages and ordinary title furniture), validated by report.layout.schema; this job refuses transient settings. The mv_standardize layout intent canonizes approved cover/naming with standard index/key-plan/notes and A3 defaults.",
        "settings_rule":"All text, logos, layout, scales, fonts, ink and fixed version/date inherit the project's canonical adopted MV setup. This configuration only binds geometry, held approved assets and destinations. V1 transient settings are refused.",
        "path_rule":"Source paths resolve beside the configuration. The destination is a fresh absolute --out directory supplied for each run; stored output_root is refused.",
        "example":{"schema":"ds.grid-plan-profile-print/v3","project_id":"project-id","scene_path":"sources/profile-scene.json","plan_path":"sources/plan.json"}
    })
}

pub fn schema_render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}

fn availability() -> Availability {
    DS_REPORT.availability()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintConfig {
    schema: String,
    project_id: String,
    scene_path: PathBuf,
    plan_path: PathBuf,
    #[serde(default)]
    preview_only: bool,
    #[serde(default)]
    side_profiles_path: Option<PathBuf>,
    #[serde(default)]
    notes_path: Option<PathBuf>,
    #[serde(default)]
    structure_descriptions_path: Option<PathBuf>,
    #[serde(default)]
    sample_pages: Option<usize>,
    #[serde(default)]
    model_crs: Option<String>,
    #[serde(default)]
    context_page_files: Vec<PathBuf>,
    #[serde(default)]
    model_fields: BTreeMap<ds_command_kernel::printing::mv::ModelField, String>,
    #[serde(default)]
    publication_assets: BTreeMap<String, PathBuf>,
    #[serde(default = "standard_variant")]
    variants: Vec<PrintVariant>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintVariant {
    name: String,
}

fn standard_variant() -> Vec<PrintVariant> {
    vec![PrintVariant {
        name: "booklet".into(),
    }]
}

fn decode_config(bytes: &[u8]) -> Result<PrintConfig, Failure> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| Failure::invalid("print_config_invalid", e.to_string()))?;
    serde_json::from_value(value)
        .map_err(|e| Failure::invalid("print_config_invalid", e.to_string()))
}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}

fn validate(
    config: &PrintConfig,
    project: &str,
    base: &Path,
    output_root: &Path,
) -> Result<PathBuf, Failure> {
    if config.schema != "ds.grid-plan-profile-print/v3" || config.project_id != project {
        return Err(Failure::invalid(
            "print_config_invalid",
            "schema or project_id does not match the requested print",
        ));
    }
    if config.sample_pages == Some(0) {
        return Err(Failure::invalid(
            "print_config_invalid",
            "sample_pages must be positive; omit it for a complete project print",
        ));
    }
    if config.variants.is_empty() || config.variants.len() > 8 {
        return Err(Failure::invalid(
            "print_config_invalid",
            "variants must contain 1..8 named prints",
        ));
    }
    let mut names = BTreeSet::new();
    for variant in &config.variants {
        let name = variant.name.as_str();
        if name.is_empty()
            || name.len() > 48
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !names.insert(name)
        {
            return Err(Failure::invalid(
                "print_config_invalid",
                "variant names must be unique safe directory names of 1..48 ASCII letters, digits, '-' or '_'",
            ));
        }
    }
    for (name, path) in [
        ("scene_path", &config.scene_path),
        ("plan_path", &config.plan_path),
    ] {
        if !resolve(base, path).is_file() {
            return Err(Failure::invalid(
                "projection_missing",
                format!("{name} does not name an existing file"),
            ));
        }
    }
    if !output_root.is_absolute() {
        return Err(Failure::invalid(
            "print_config_invalid",
            "--out must be an absolute directory",
        ));
    }
    if output_root.symlink_metadata().is_ok() {
        return Err(Failure::invalid(
            "output_exists",
            "--out already exists; print variants require a fresh destination",
        ));
    }
    Ok(output_root.to_owned())
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let config_path = PathBuf::from(inputs.require("config")?);
    if !config_path.is_absolute() || !config_path.is_file() {
        return Err(Failure::invalid(
            "print_config_invalid",
            "--config must name an existing absolute JSON file",
        ));
    }
    let bytes = std::fs::read(&config_path)
        .map_err(|e| Failure::invalid("print_config_invalid", e.to_string()))?;
    let config = decode_config(&bytes)?;
    let base = config_path.parent().expect("absolute file has parent");
    let output_root = validate(&config, project, base, Path::new(inputs.require("out")?))?;
    let (resolved, style_resolution, renderer_defaults) =
        crate::project::mv_setup::resolve_project_print(
            inputs.require("lane")?,
            project,
            config.model_fields.clone(),
        )?;
    let staging =
        tempfile::tempdir_in(output_root.parent().ok_or_else(|| {
            Failure::invalid("print_config_invalid", "output_root has no parent")
        })?)
        .map_err(|e| Failure::failed("output_create_failed", e.to_string()))?;

    let config_sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
    let receipt_path = output_root.join("print-receipt.json");
    let mut variants = Vec::<Value>::new();

    for variant in &config.variants {
        let out_dir = staging.path().join(&variant.name);
        let request = json!({
            "preview_only":config.preview_only,
            "project_id": config.project_id,
            "scene_path": resolve(base, &config.scene_path),
            "plan_path": resolve(base, &config.plan_path),
            "side_profiles_path": config.side_profiles_path.as_deref().map(|p| resolve(base, p)),
            "notes_path": config.notes_path.as_deref().map(|p| resolve(base, p)),
            "structure_descriptions_path": config.structure_descriptions_path.as_deref().map(|p| resolve(base, p)),
            "out_dir": out_dir,
            "settings": resolved.settings,
            "style_resolution": style_resolution,
            "renderer_defaults": renderer_defaults,
            "mv_setup": resolved,
            "publication_assets": config.publication_assets.iter().map(|(id,p)|(id.clone(),resolve(base,p))).collect::<BTreeMap<_,_>>(),
            "sample_pages": config.sample_pages,
            "model_crs": config.model_crs,
            "context_page_files": config.context_page_files.iter().map(|p| resolve(base, p)).collect::<Vec<_>>(),
            "logo_files": []
        });
        let temp = tempfile::tempdir()
            .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
        let request_path = temp.path().join("request.json");
        let result_path = temp.path().join("result.json");
        let request_bytes = serde_json::to_vec(&request)
            .map_err(|e| Failure::internal("request_write_failed", e.to_string()))?;
        ds_layer_store::private::write(&request_path, request_bytes)
            .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
        let args = vec![
            OsString::from("--request"),
            request_path.into(),
            OsString::from("--result"),
            result_path.clone().into(),
        ];
        let completed = match DS_REPORT.call("render-grid-plan-profile", &args, EXPORT_TIMEOUT) {
            Ok(completed) => completed,
            Err(error) => {
                return Err(error.detail(json!({"variant":variant.name,"committed":false})));
            }
        };
        let document = std::fs::read(&result_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        if !completed.succeeded() || document.is_none() {
            return Err(Failure::failed(
                "engine_refused",
                format!(
                    "variant '{}' failed; the atomic destination was not committed: {}",
                    variant.name,
                    output_root.display()
                ),
            )
            .detail(
                json!({"variant":variant.name,"receipt_path":receipt_path,"engine":document}),
            ));
        }
        let mut document = document.expect("checked above");
        relocate_paths(&mut document, staging.path(), &output_root);
        ds_layer_store::private::write_atomic(
            &staging.path().join(&variant.name).join("manifest.json"),
            serde_json::to_vec_pretty(&document)
                .map_err(|e| Failure::internal("receipt_write_failed", e.to_string()))?,
        )
        .map_err(|e| Failure::failed("receipt_write_failed", e.to_string()))?;
        variants.push(json!({
            "name":variant.name,
            "mv_setup_sha256":document["mv_setup_sha256"],
            "publication_page_count":document["publication_page_count"],
            "model_revision":document["model_revision"],
            "page_count":document["page_count"],
            "full_page_count":document["full_page_count"],
            "pdf":document["pdf"],
            "pdf_sha256":document["pdf_sha256"],
            "context_pages":document["context_pages"],
            "booklet_plan":document["booklet_plan"],
            "booklet_previews":document["booklet_previews"]
        }));
    }
    save_receipt(
        &staging.path().join("print-receipt.json"),
        &config_sha256,
        project,
        "ok",
        &variants,
    )?;
    commit_job(staging, &output_root)?;
    Ok(
        json!({"config_sha256":config_sha256,"project_id":project,"status":"ok","receipt_path":receipt_path,"variants":variants}),
    )
}

fn commit_job(staging: tempfile::TempDir, destination: &Path) -> Result<(), Failure> {
    // A completed directory is the sole commit boundary. Linux renameat2
    // refuses a concurrent creator, including an empty directory.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let source = std::ffi::CString::new(staging.path().as_os_str().as_bytes())
            .map_err(|e| Failure::invalid("print_config_invalid", e.to_string()))?;
        let target = std::ffi::CString::new(destination.as_os_str().as_bytes())
            .map_err(|e| Failure::invalid("print_config_invalid", e.to_string()))?;
        // SAFETY: both C strings remain alive for the synchronous syscall.
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(Failure::failed(
                "output_create_failed",
                std::io::Error::last_os_error().to_string(),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        if destination.symlink_metadata().is_ok() {
            return Err(Failure::invalid("output_exists", "destination exists"));
        }
        std::fs::rename(staging.path(), destination)
            .map_err(|e| Failure::failed("output_create_failed", e.to_string()))
    }
}

fn relocate_paths(value: &mut Value, from: &Path, to: &Path) {
    match value {
        Value::String(text) => {
            if let Ok(relative) = Path::new(text.as_str()).strip_prefix(from) {
                *text = to.join(relative).display().to_string();
            }
        }
        Value::Array(values) => {
            for item in values {
                relocate_paths(item, from, to);
            }
        }
        Value::Object(values) => {
            for item in values.values_mut() {
                relocate_paths(item, from, to);
            }
        }
        _ => {}
    }
}

fn save_receipt(
    path: &Path,
    config_sha256: &str,
    project: &str,
    status: &str,
    variants: &[Value],
) -> Result<(), Failure> {
    let bytes = serde_json::to_vec_pretty(&json!({"schema":"ds.grid-plan-profile-print-receipt/v1","config_sha256":config_sha256,"project_id":project,"status":status,"variants":variants}))
        .map_err(|e| Failure::internal("receipt_write_failed", e.to_string()))?;
    ds_layer_store::private::write_atomic(path, bytes)
        .map_err(|e| Failure::failed("receipt_write_failed", e.to_string()))
}

pub fn render(data: &Value) -> String {
    format!(
        "{} plan/profile variants\nreceipt {}",
        data["variants"].as_array().map_or(0, Vec::len),
        data["receipt_path"].as_str().unwrap_or("?")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_job_commit_refuses_existing_delivery_and_moves_one_complete_directory() {
        let parent = tempfile::tempdir().unwrap();
        let destination = parent.path().join("delivery");
        std::fs::create_dir(&destination).unwrap();
        let staging = tempfile::tempdir_in(parent.path()).unwrap();
        std::fs::write(staging.path().join("receipt.json"), b"complete").unwrap();
        assert!(commit_job(staging, &destination).is_err());
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);
        std::fs::remove_dir(&destination).unwrap();
        let staging = tempfile::tempdir_in(parent.path()).unwrap();
        std::fs::write(staging.path().join("receipt.json"), b"complete").unwrap();
        commit_job(staging, &destination).unwrap();
        assert_eq!(
            std::fs::read(destination.join("receipt.json")).unwrap(),
            b"complete"
        );
    }

    #[test]
    fn variant_names_cannot_escape_or_collide() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("scene.json"), "{}").unwrap();
        std::fs::write(source.path().join("plan.json"), "{}").unwrap();
        let make = |names: &[&str]| PrintConfig {
            schema: "ds.grid-plan-profile-print/v3".into(),
            preview_only: false,
            project_id: "p".into(),
            scene_path: "scene.json".into(),
            plan_path: "plan.json".into(),
            side_profiles_path: None,
            notes_path: None,
            structure_descriptions_path: None,
            sample_pages: Some(1),
            model_crs: None,
            context_page_files: vec![],
            model_fields: BTreeMap::new(),
            publication_assets: BTreeMap::new(),
            variants: names
                .iter()
                .map(|name| PrintVariant {
                    name: (*name).into(),
                })
                .collect(),
        };
        let out = source.path().join("out");
        assert!(validate(&make(&["color", "mono"]), "p", source.path(), &out).is_ok());
        assert_eq!(
            validate(&make(&["../escape"]), "p", source.path(), &out)
                .unwrap_err()
                .code(),
            "print_config_invalid"
        );
        assert_eq!(
            validate(&make(&["color", "color"]), "p", source.path(), &out)
                .unwrap_err()
                .code(),
            "print_config_invalid"
        );
        assert_eq!(
            validate(&make(&["color"]), "other", source.path(), &out)
                .unwrap_err()
                .code(),
            "print_config_invalid"
        );
    }

    #[test]
    fn discovery_example_parses_as_the_live_configuration() {
        let example = schema_document()["example"].clone();
        let config: PrintConfig = serde_json::from_value(example).unwrap();
        assert_eq!(config.schema, "ds.grid-plan-profile-print/v3");
        assert_eq!(config.variants.len(), 1);
        assert_eq!(config.variants[0].name, "booklet");
    }

    #[test]
    fn reusable_config_refuses_a_saved_destination() {
        let mut example = schema_document()["example"].clone();
        example["output_root"] = json!("C:\\External\\prints");
        assert!(decode_config(&serde_json::to_vec(&example).unwrap()).is_err());
    }
}
