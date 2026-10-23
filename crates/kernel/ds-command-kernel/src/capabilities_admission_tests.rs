//! Row -> one export: the build gates on `export`, the source scan behind
//! them, in-page admission of a shared export, the Desktop Core door for ids
//! only installed Core declares, and the implicit desktop=core rule.
use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// The build-time compiler itself, so these gates are the ones that fail the build.
#[path = "../build_capabilities.rs"]
#[allow(dead_code)]
mod compiler;

const MANIFEST: &str = include_str!("../routing/capabilities.json");
const CARGO_TOML: &str = include_str!("../Cargo.toml");

fn exports() -> &'static BTreeMap<String, Vec<String>> {
    static EXPORTS: OnceLock<BTreeMap<String, Vec<String>>> = OnceLock::new();
    EXPORTS.get_or_init(|| {
        compiler::crate_exports(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
    })
}

/// Compile a manifest as the default build does; the refusal text on a gate.
fn compile(manifest: &Value) -> Result<String, String> {
    let text = compiler::render(&compiler::parse(&manifest.to_string()));
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compiler::compile(
            &text,
            &compiler::Environment {
                exchange: &|module| {
                    src.join("gateway_exchange").join(format!("{module}.rs")).is_file()
                        || src.join(format!("{module}.rs")).is_file()
                },
                export: &|name| exports().get(name).cloned().unwrap_or_default(),
                feature_declared: &|feature| CARGO_TOML.contains(&format!("\n{feature} = ")),
                feature_enabled: &|_| false,
            },
        )
    }))
    .map_err(|panic| {
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
            .unwrap_or_default()
    })
}

fn committed() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

fn in_page(value: &str) -> bool {
    value.starts_with("kernel:") || value == "network_unit"
}

#[test]
fn every_in_page_row_names_one_export_the_kernel_defines() {
    let manifest = committed();
    let mut bound = 0;
    for capability in CAPABILITIES {
        let row = &manifest["capabilities"][capability.id];
        let runs_in_page = PLATFORMS
            .iter()
            .any(|platform| in_page(row[*platform].as_str().unwrap()));
        assert_eq!(
            runs_in_page,
            capability.export.is_some(),
            "`{}` runs in the page exactly when it names an export",
            capability.id
        );
        if let Some(export) = capability.export {
            let files = exports().get(export).map(Vec::as_slice).unwrap_or_default();
            assert_eq!(files.len(), 1, "`{}` export `{export}`: {files:?}", capability.id);
            bound += 1;
        }
    }
    assert!(bound > 200, "only {bound} rows name an export");
    // The survey photo rows name their real owners, not the media exchange's inspect.
    let owner = |id: &str| capability(id).unwrap().export;
    assert_eq!(owner("survey.photo.capture"), Some("surveyPhotoMediaProcess"));
    assert_eq!(owner("survey.photo.rotate"), Some("surveyPhotoMediaProcess"));
    assert_eq!(owner("survey.photo.publish"), Some("surveyQueueDrain"));
    assert_eq!(owner("survey.photo.fetch"), Some("storedObjectBytes"));
    assert_eq!(owner("design.download.fetch"), Some("storedObjectBytes"));
}

#[test]
fn a_deliberately_wrong_export_name_fails_the_build() {
    assert!(compile(&committed()).is_ok());
    let mut wrong = committed();
    wrong["capabilities"]["notes.create"]["export"] = json!("notesCreateDeliberatelyWrong");
    let refusal = compile(&wrong).unwrap_err();
    assert!(
        refusal.contains("`notes.create` names the export `notesCreateDeliberatelyWrong`, which no Kernel source exports"),
        "{refusal}"
    );
}

#[test]
fn each_export_gate_refuses_the_manifest_that_breaks_it() {
    let mut missing = committed();
    missing["capabilities"]["notes.create"]
        .as_object_mut()
        .unwrap()
        .remove("export");
    let refusal = compile(&missing).unwrap_err();
    assert!(refusal.contains("`notes.create` runs in the page on web but names no `export`"), "{refusal}");

    let mut stray = committed();
    stray["capabilities"]["pm.party.list"]["export"] = json!("projectWork");
    let refusal = compile(&stray).unwrap_err();
    assert!(refusal.contains("but no platform runs it in the page"), "{refusal}");

    let mut spelled = committed();
    spelled["capabilities"]["notes.create"]["export"] = json!("notes-create");
    assert!(compile(&spelled).unwrap_err().contains("is not a JS identifier"));

    let mut member = committed();
    member["platforms"][3]["id"] = json!("export");
    assert!(compile(&member).unwrap_err().contains("a row member"));

    let mut module = committed();
    module["capabilities"]["notes.create"]["web"] = json!("kernel:nowhere_module");
    assert!(compile(&module).unwrap_err().contains("which src/gateway_exchange does not have, nor src"));
}

#[test]
fn the_scanner_reads_module_level_exports_only() {
    let source = r##"
//! A doc brace { that never opens a block.
#[wasm_bindgen(js_name = alpha)]
pub fn alpha_wasm() {}
#[wasm_bindgen]
pub fn bare_name() {}
#[wasm_bindgen::prelude::wasm_bindgen(js_name = "Quoted")]
pub async fn quoted() {}
#[cfg_attr(
    feature = "wasm",
    wasm_bindgen::prelude::wasm_bindgen(js_name = gated)
)]
pub(crate) fn gated_wasm() {}
#[wasm_bindgen]
pub struct Handle { inner: u8 }
#[wasm_bindgen]
impl Handle {
    #[wasm_bindgen(js_name = method)]
    pub fn method(&self) -> u8 { self.inner }
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log(text: &str);
}
#[wasm_bindgen(typescript_custom_section)]
const TS: &str = r#"export interface X { a: { b: string } }"#;
#[wasm_bindgen(start)]
pub fn start() {}
pub(crate) mod exports {
    use super::*;
    /// A brace in a char '{', a string "}" and a lifetime 'a.
    #[wasm_bindgen(js_name = nested, unchecked_return_type = "{ a: string }")]
    pub fn nested(
        #[wasm_bindgen(unchecked_param_type = "string")] value: JsValue,
        f: fn(u8) -> u8,
    ) -> char {
        let _ = "}";
        '}'
    }
}
fn not_exported() { let _ = ('\'', b'{', "\"{"); }
#[wasm_bindgen_test]
fn test_only() {}
"##;
    let names: Vec<String> = compiler::wasm_exports(source)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["alpha", "bare_name", "Quoted", "gated", "Handle", "nested"]);
    // The real crate: the export behind a shared dispatcher is found once, in its module.
    assert_eq!(
        exports().get("designCollab").map(Vec::as_slice),
        Some(&["gateway_exchange/design_collab.rs".to_owned()][..])
    );
}

#[test]
fn a_shared_export_runs_only_what_a_row_declares_here() {
    let admit = |platform: &str, id: &str, export: &str, exchange: &str| {
        admit_export_on(platform, id, export, exchange).map(|capability| capability.id)
    };
    let refused = |platform: &str, id: &str, export: &str, exchange: &str| {
        admit_export_on(platform, id, export, exchange).unwrap_err().code
    };
    // The Web runs it on the Kernel through the export its row names.
    assert_eq!(
        admit("web", "design.comment.post", "designCollab", "design_collab"),
        Ok("design.comment.post")
    );
    // The Desktop page runs a Core row in-page through the same owner.
    assert!(admit("desktop", "design.comment.post", "designCollab", "design_collab").is_ok());
    // Headless Core has no page.
    assert_eq!(refused("cli", "design.comment.post", "designCollab", "design_collab"), CODE_NOT_IN_PAGE);
    // Another row's export, a row with no in-page owner, another exchange.
    assert_eq!(refused("web", "notes.create", "designCollab", "design_collab"), CODE_EXPORT_MISMATCH);
    assert_eq!(refused("desktop", "design.group.apply", "designCollab", "design_collab"), CODE_EXPORT_MISMATCH);
    assert_eq!(refused("web", "design.comment.post", "designCollab", "design"), CODE_NOT_IN_PAGE);
    // The row's own refusal where it is not offered.
    assert_eq!(refused("android", "design.comment.post", "designCollab", "design_collab"), CODE_DISCARDED);
    assert_eq!(refused("web", "data.reference.catalog", "referenceCatalogue", "reference_data"), CODE_PENDING);
    assert_eq!(refused("web", "nope.never", "designCollab", "design_collab"), CODE_UNKNOWN);
}

#[test]
fn the_route_answer_names_the_in_page_owner_where_the_page_runs_it() {
    let export = |platform: &str, id: &str| route_on(platform, id).unwrap().export;
    assert_eq!(export("web", "notes.create"), Some("notesCreate"));
    assert_eq!(export("desktop", "notes.create"), Some("notesCreate"));
    assert_eq!(export("cli", "notes.create"), None);
    assert_eq!(export("web", "pm.party.list"), None);
    assert_eq!(export("web", "grid.session"), Some("gridEngineRequest"));
    assert_eq!(export("desktop", "grid.session"), None);
    let wire = serde_json::to_value(route_on("web", "notes.create").unwrap()).unwrap();
    assert_eq!(wire["export"], "notesCreate");
    assert!(serde_json::to_value(route_on("cli", "notes.create").unwrap()).unwrap().get("export").is_none());
}

#[test]
fn the_desktop_core_door_forwards_what_installed_core_declares() {
    let unknown = "desktop.printing.future-sheet";
    assert!(capability(unknown).is_err(), "the control id must be unknown to this table");
    let declared = json!({"id": unknown, "contract": 1, "availability": "available", "inputs": []});
    let other = json!({"id": "desktop.printing.other", "contract": 1, "availability": "available", "inputs": []});
    // The page beside Core leaves an unknown id to Core's own discovery...
    assert!(matches!(core_door_on("desktop", unknown), Ok(CoreDoor::Installed)));
    assert_eq!(command::core_refusal_on("desktop", unknown), None);
    // ...admits it when installed Core declares exactly it...
    assert_eq!(command::admit_on("desktop", unknown, &declared, &json!({}), "p"), None);
    // ...and refuses it, typed, when Core does not.
    let refusal = command::admit_on("desktop", unknown, &other, &json!({}), "p").unwrap();
    assert_eq!(refusal.code, CODE_UNKNOWN);
    assert!(refusal.message.contains("Neither this client's capability manifest nor installed DS Core"));
    // Core still validates a declared run's inputs.
    let refusal = command::admit_on("desktop", unknown, &declared, &json!({"extra": 1}), "p").unwrap();
    assert_eq!(refusal.code, "inputs_invalid");
    // Known ids keep the manifest's answer on the Desktop.
    assert!(matches!(core_door_on("desktop", "design.lv.process"), Ok(CoreDoor::Declared(_))));
    assert!(core_door_on("desktop", "survey.entries.create").unwrap_err().starts_with("capability_not_on_core"));
    assert_eq!(command::core_refusal_on("desktop", "account.connect").unwrap().code, CODE_DISCARDED);
    // The Web and headless Core are unaffected.
    for platform in ["web", "cli"] {
        assert!(core_door_on(platform, unknown).unwrap_err().starts_with(CODE_UNKNOWN), "{platform}");
        let refusal = command::admit_on(platform, unknown, &declared, &json!({}), "p").unwrap();
        assert_eq!(refusal.code, CODE_UNKNOWN, "{platform}");
    }
    assert!(beside_core("desktop") && !beside_core("web") && !beside_core("cli"));
}

/// Rows a platform other than cli runs on Core, which DS Core does not run.
fn on_core_without_core(manifest: &Value) -> Vec<String> {
    let rows = manifest["capabilities"].as_object().unwrap();
    rows.iter()
        .filter(|(_, row)| {
            row.as_object().unwrap().iter().any(|(platform, value)| platform != "cli" && value == "core")
                && row["cli"] != "core"
        })
        .map(|(id, _)| id.clone())
        .collect()
}

#[test]
fn a_platform_runs_on_core_only_what_core_has() {
    // desktop=core means the Desktop forwards it to installed Core, which
    // therefore has the command: the row is core on cli too.
    assert_eq!(on_core_without_core(&committed()), Vec::<String>::new());
    let mut broken = committed();
    broken["capabilities"]["survey.entries.restore"]["desktop"] = json!("core");
    assert_eq!(on_core_without_core(&broken), ["survey.entries.restore"]);
}
