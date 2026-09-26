//! Repeatable plan/profile print variants from one checked JSON document.
//! The Rust reporter owns pagination and every drawing byte; this command
//! only binds a shared, pinned request to named ink variants.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{DS_REPORT, EXPORT_TIMEOUT};

pub static COMMAND: Command = Command {
    id: "report.plan-profile-config",
    path: &["report", "plan-profile-config"],
    contract: 1,
    summary: "Print plan/profile variants from a JSON configuration.",
    purpose: "Read a project-pinned ds.grid-plan-profile-print/v1 JSON configuration, then run the Rust reporter once for each named variant. Discover the JSON shape with `ds report plan-profile-config schema`. Shared scene, plan, context and drawing settings are declared once. Every variant gets a fresh output directory and PDF; a batch receipt records source and PDF digests. Existing print files are never replaced.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<id>",
            "Exact project context; must match the configuration.",
        )
        .required(),
        Arg::value(
            "config",
            "<file.json>",
            "Absolute path to a ds.grid-plan-profile-print/v1 JSON configuration.",
        )
        .required(),
    ],
    output: "One receipt with the configuration SHA-256, model revision, page count and digest-pinned PDF for every completed variant; a partial receipt if a later variant fails.",
    examples: &[Example {
        command: "ds report plan-profile-config --project gisagara --config /project/prints/plan-profile.json --output json",
        note: "Render every named variant from one pinned configuration; see docs/reference/report.md for the schema.",
        runnable: false,
    }],
    refusals: &[
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
            remedy: "choose a fresh output_root in the configuration",
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
            remedy: "check destination permissions and free space; preserve completed variant directories",
        },
        Refusal {
            code: "engine_refused",
            when: "the Rust reporter rejects a variant's source or settings",
            remedy: "read the partial batch receipt and reporter detail; correct the pinned inputs",
        },
        Refusal {
            code: "reporter_engine_missing",
            when: "the Rust reporter is unavailable",
            remedy: "install the matching ds-report binary",
        },
    ],
    reference: Some("docs/reference/report.md"),
    search: &["dsgrid"],
    requires: Requires::Server,
    availability,
};

pub static SCHEMA: Command = Command {
    id: "report.plan-profile-config.schema",
    path: &["report", "plan-profile-config", "schema"],
    contract: 1,
    summary: "Describe the JSON plan/profile print configuration.",
    purpose: "Return the versioned configuration fields, path rules and a complete minimal example for color and monochrome variants. This is local discovery and does not render a sheet.",
    chapter: Chapter::Reports,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "Required fields, accepted variant inks, relative path rule and a JSON example.",
    examples: &[Example {
        command: "ds report plan-profile-config schema --output json",
        note: "Get a JSON configuration example before rendering.",
        runnable: true,
    }],
    refusals: &[],
    reference: Some("docs/reference/report.md"),
    search: &["dsgrid", "variants"],
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
    json!({
        "schema":"ds.grid-plan-profile-print/v1",
        "required":["schema","project_id","scene_path","plan_path","output_root","settings","variants"],
        "optional":["sample_pages","side_profiles_path","notes_path","model_crs","context_page_files","logo_files"],
        "sample_pages_rule":"Optional positive integer at the top level. Omit it to print the entire project; never put it inside settings.",
        "path_rule":"Relative paths resolve beside the configuration file; output_root must not exist and its parent must exist.",
        "settings_rule":"Typed Rust SheetSettings. Required: format, project_title and sheet_title. Put ink_mode in each variant; see report.plan-profile for drawing controls. Feature-code labels are off by default; set show_feature_codes=true and feature_label_style with codes, orientation, font_size_pt and placement to opt in.",
        "variant_ink_modes":["reference_accents","monochrome"],
        "example":{
            "schema":"ds.grid-plan-profile-print/v1",
            "project_id":"project-id",
            "scene_path":"sources/profile-scene.json",
            "plan_path":"sources/plan.json",
            "output_root":"v0-output",
            "settings":{"format":"advanced","project_title":"Project name","sheet_title":"MV plan and profile","horizontal_scale":1500,"vertical_scale":800,"plan_scale":1500,"show_feature_codes":false},
            "variants":[{"name":"color","ink_mode":"reference_accents"},{"name":"monochrome","ink_mode":"monochrome"}]
        }
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
    output_root: PathBuf,
    #[serde(default)]
    side_profiles_path: Option<PathBuf>,
    #[serde(default)]
    notes_path: Option<PathBuf>,
    #[serde(default)]
    sample_pages: Option<usize>,
    #[serde(default)]
    model_crs: Option<String>,
    #[serde(default)]
    context_page_files: Vec<PathBuf>,
    #[serde(default)]
    logo_files: Vec<PathBuf>,
    settings: Map<String, Value>,
    variants: Vec<PrintVariant>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintVariant {
    name: String,
    ink_mode: InkMode,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum InkMode {
    Monochrome,
    ReferenceAccents,
}

impl InkMode {
    fn name(self) -> &'static str {
        match self {
            Self::Monochrome => "monochrome",
            Self::ReferenceAccents => "reference_accents",
        }
    }
}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}

fn validate(config: &PrintConfig, project: &str, base: &Path) -> Result<PathBuf, Failure> {
    if config.schema != "ds.grid-plan-profile-print/v1" || config.project_id != project {
        return Err(Failure::invalid(
            "print_config_invalid",
            "schema or project_id does not match the requested print",
        ));
    }
    if config.settings.contains_key("ink_mode")
        || !config.settings.contains_key("format")
        || !config.settings.contains_key("project_title")
        || !config.settings.contains_key("sheet_title")
        || config.settings.contains_key("sample_pages")
    {
        return Err(Failure::invalid(
            "print_config_invalid",
            "settings needs format, project_title and sheet_title; sample_pages belongs at the top level and ink_mode belongs to each variant",
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
    let output_root = resolve(base, &config.output_root);
    if output_root.symlink_metadata().is_ok() {
        return Err(Failure::invalid(
            "output_exists",
            "output_root already exists; print variants require a fresh destination",
        ));
    }
    Ok(output_root)
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
    let config: PrintConfig = serde_json::from_slice(&bytes)
        .map_err(|e| Failure::invalid("print_config_invalid", e.to_string()))?;
    let base = config_path.parent().expect("absolute file has parent");
    let output_root = validate(&config, project, base)?;
    std::fs::create_dir(&output_root)
        .map_err(|e| Failure::failed("output_create_failed", e.to_string()))?;

    let config_sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
    let receipt_path = output_root.join("print-receipt.json");
    let mut variants = Vec::<Value>::new();
    save_receipt(&receipt_path, &config_sha256, project, "running", &variants)?;
    for variant in &config.variants {
        let mut settings = config.settings.clone();
        settings.insert("ink_mode".into(), json!(variant.ink_mode.name()));
        let out_dir = output_root.join(&variant.name);
        let request = json!({
            "project_id": config.project_id,
            "scene_path": resolve(base, &config.scene_path),
            "plan_path": resolve(base, &config.plan_path),
            "side_profiles_path": config.side_profiles_path.as_deref().map(|p| resolve(base, p)),
            "notes_path": config.notes_path.as_deref().map(|p| resolve(base, p)),
            "out_dir": out_dir,
            "settings": settings,
            "sample_pages": config.sample_pages,
            "model_crs": config.model_crs,
            "context_page_files": config.context_page_files.iter().map(|p| resolve(base, p)).collect::<Vec<_>>(),
            "logo_files": config.logo_files.iter().map(|p| resolve(base, p)).collect::<Vec<_>>()
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
                save_receipt(&receipt_path, &config_sha256, project, "partial", &variants)?;
                return Err(error.detail(json!({"variant":variant.name,"receipt_path":receipt_path,"completed_variants":variants})));
            }
        };
        let document = std::fs::read(&result_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        if !completed.succeeded() || document.is_none() {
            save_receipt(&receipt_path, &config_sha256, project, "partial", &variants)?;
            return Err(Failure::failed(
                "engine_refused",
                format!(
                    "variant '{}' failed; completed variants remain in {}",
                    variant.name,
                    output_root.display()
                ),
            )
            .detail(
                json!({"variant":variant.name,"receipt_path":receipt_path,"engine":document}),
            ));
        }
        let document = document.expect("checked above");
        variants.push(json!({
            "name":variant.name,
            "ink_mode":variant.ink_mode.name(),
            "model_revision":document["model_revision"],
            "page_count":document["page_count"],
            "full_page_count":document["full_page_count"],
            "pdf":document["pdf"],
            "pdf_sha256":document["pdf_sha256"],
            "context_pages":document["context_pages"]
        }));
        save_receipt(&receipt_path, &config_sha256, project, "partial", &variants)?;
    }
    save_receipt(&receipt_path, &config_sha256, project, "ok", &variants)?;
    Ok(
        json!({"config_sha256":config_sha256,"project_id":project,"status":"ok","receipt_path":receipt_path,"variants":variants}),
    )
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
    fn variant_names_cannot_escape_or_collide() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("scene.json"), "{}").unwrap();
        std::fs::write(source.path().join("plan.json"), "{}").unwrap();
        let make = |names: &[&str]| PrintConfig {
            schema: "ds.grid-plan-profile-print/v1".into(),
            project_id: "p".into(),
            scene_path: "scene.json".into(),
            plan_path: "plan.json".into(),
            output_root: "out".into(),
            side_profiles_path: None,
            notes_path: None,
            sample_pages: Some(1),
            model_crs: None,
            context_page_files: vec![],
            logo_files: vec![],
            settings: serde_json::from_value(
                json!({"format":"advanced","project_title":"P","sheet_title":"S"}),
            )
            .unwrap(),
            variants: names
                .iter()
                .map(|name| PrintVariant {
                    name: (*name).into(),
                    ink_mode: InkMode::Monochrome,
                })
                .collect(),
        };
        assert!(validate(&make(&["color", "mono"]), "p", source.path()).is_ok());
        assert_eq!(
            validate(&make(&["../escape"]), "p", source.path())
                .unwrap_err()
                .code(),
            "print_config_invalid"
        );
        assert_eq!(
            validate(&make(&["color", "color"]), "p", source.path())
                .unwrap_err()
                .code(),
            "print_config_invalid"
        );
        assert_eq!(
            validate(&make(&["color"]), "other", source.path())
                .unwrap_err()
                .code(),
            "print_config_invalid"
        );
        let mut misplaced = make(&["color"]);
        misplaced.settings.insert("sample_pages".into(), json!(1));
        assert_eq!(
            validate(&misplaced, "p", source.path()).unwrap_err().code(),
            "print_config_invalid"
        );
    }

    #[test]
    fn discovery_example_parses_as_the_live_configuration() {
        let example = schema_document()["example"].clone();
        let config: PrintConfig = serde_json::from_value(example).unwrap();
        assert_eq!(config.schema, "ds.grid-plan-profile-print/v1");
        assert_eq!(config.variants.len(), 2);
        assert_eq!(config.variants[0].ink_mode.name(), "reference_accents");
    }
}
