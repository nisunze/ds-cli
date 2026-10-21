//! Complete bounded registry receipts for native Core clients.
use ds_cli_contract::{Context, Failure, Inputs, spec::*};
use ds_layer_store::OverlayEdit;
use serde_json::Value;
use std::io::Read;

pub static READ: Command = Command {
    id: "map.layer.registry",
    path: &["map", "layer", "registry"],
    contract: 1,
    summary: "Read the complete machine-local overlay registry receipt.",
    purpose: "Reads at most 100 retained XYZ/raster PMTiles references, including the registry revision and persistence receipt. No sign-in, map, project acquisition or network access is required. An absent registry is returned empty without creating a store.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "Complete layers, revision, receipt, changed=false and persisted=native_local; at most 100 validated layers and 1 MiB stored input.",
    examples: &[Example {
        command: "ds map layer registry --output json",
        note: "Read retained overlays without opening Desktop.",
        runnable: false,
    }],
    refusals: &[super::LOCAL_STORE_REFUSAL],
    reference: Some("docs/reference/map.md"),
    search: &[],
    requires: Requires::Server,
    availability: super::local_availability,
};
pub static EDIT: Command = Command {
    id: "map.layer.registry-edit",
    path: &["map", "layer", "registry-edit"],
    contract: 1,
    summary: "Apply one typed machine-local overlay registry edit.",
    purpose: "Reads one JSON OverlayEdit file bounded to 1 MiB: operation add with layer, remove with id, visibility with id/visible, or one-time import with layers. Unknown fields and list operations refuse. The shared native owner validates HTTP(S) references, serializes concurrent edits and atomically persists at most 100 layers. This does not fetch URLs, modify project data or require sign-in. Import preserves existing IDs and cannot resurrect removed legacy entries after its first successful application.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "file",
        "<path>",
        "Typed OverlayEdit JSON file; at most 1 MiB.",
    )
    .required()],
    output: "Complete layers, revision, operation receipt, changed and persisted=native_local; refused edits preserve existing registry bytes.",
    examples: &[Example {
        command: "ds map layer registry-edit --file overlay-edit.json --output json",
        note: "Use map layer add/remove/visibility for flag-based single edits.",
        runnable: false,
    }],
    refusals: &[super::LOCAL_STORE_REFUSAL],
    reference: Some("docs/reference/map.md"),
    search: &[],
    requires: Requires::Server,
    availability: super::local_availability,
};
fn refused(message: impl Into<String>) -> Failure {
    Failure::invalid("local_layer_refused", message).remedy(super::LOCAL_STORE_REFUSAL.remedy)
}
pub fn read(_: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_layer_store::execute(OverlayEdit::List).map_err(refused)
}
pub fn decode(bytes: &[u8]) -> Result<OverlayEdit, Failure> {
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err(refused("overlay edit must contain 1..1048576 bytes"));
    }
    let edit: OverlayEdit =
        serde_json::from_slice(bytes).map_err(|_| refused("malformed typed overlay edit"))?;
    if matches!(edit, OverlayEdit::List) {
        return Err(refused("use map layer registry for a read-only list"));
    }
    Ok(edit)
}
pub fn edit(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = inputs.require("file")?;
    if !std::fs::symlink_metadata(path)
        .map_err(|_| refused("overlay edit file is unavailable"))?
        .is_file()
    {
        return Err(refused("overlay edit must be an ordinary file"));
    }
    let file =
        std::fs::File::open(path).map_err(|_| refused("overlay edit file is unavailable"))?;
    if !file
        .metadata()
        .map_err(|_| refused("overlay edit file is unavailable"))?
        .is_file()
    {
        return Err(refused("overlay edit must be an ordinary file"));
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refused("overlay edit file could not be read"))?;
    ds_layer_store::execute(decode(&bytes)?).map_err(refused)
}
pub fn render(data: &Value) -> String {
    format!(
        "{} local overlays · revision {} · changed {}\n",
        data["layers"].as_array().map_or(0, Vec::len),
        data["revision"],
        data["changed"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_edit_contract_refuses_before_store_access() {
        for bytes in [
            b"{}".as_slice(),
            b"{\"operation\":\"list\"}",
            b"{\"operation\":\"remove\",\"id\":\"x\",\"sql\":\"bad\"}",
        ] {
            assert!(decode(bytes).is_err());
        }
        assert!(decode(&vec![b' '; 1024 * 1024 + 1]).is_err());
        assert!(matches!(
            decode(br#"{"operation":"visibility","id":"x","visible":false}"#).unwrap(),
            OverlayEdit::Visibility { visible: false, .. }
        ));
        assert_eq!(READ.effect, Effect::ReadOnly);
        assert_eq!(EDIT.effect, Effect::LocalFileWrite);
    }
    #[test]
    fn registered_handlers_preserve_retained_rows_and_refuse_without_replacing_bytes() {
        let root = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DS_LAYER_HOME");
        // This crate's other tests call pure decoding only. The real owner is
        // exercised with an isolated root, never an installed user's registry.
        unsafe {
            std::env::set_var("DS_LAYER_HOME", root.path());
        }
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        let input = root.path().join("request.json");
        let inputs = ds_cli_contract::parse(
            &EDIT,
            &["--file".into(), input.to_string_lossy().into_owned()],
        )
        .unwrap();
        let row = serde_json::json!({"id":"retained","name":"Retained","kind":"xyz","url":"https://example.invalid/{z}/{x}/{y}","tileSize":256,"visible":true});
        std::fs::write(
            &input,
            serde_json::to_vec(&serde_json::json!({"operation":"add","layer":row})).unwrap(),
        )
        .unwrap();
        let first = edit(&inputs, &context).unwrap();
        assert_eq!(first["revision"], 1);
        assert_eq!(first["layers"][0]["id"], "retained");
        let store = root.path().join("overlays.json");
        let retained = std::fs::read(&store).unwrap();
        for packet in [
            serde_json::json!({"operation":"remove","id":"missing"}),
            serde_json::json!({"operation":"visibility","id":"retained","visible":false,"extra":true}),
        ] {
            std::fs::write(&input, serde_json::to_vec(&packet).unwrap()).unwrap();
            assert!(edit(&inputs, &context).is_err());
            assert_eq!(std::fs::read(&store).unwrap(), retained);
        }
        std::fs::write(
            &input,
            br#"{"operation":"visibility","id":"retained","visible":false}"#,
        )
        .unwrap();
        assert_eq!(
            edit(&inputs, &context).unwrap()["layers"][0]["visible"],
            false
        );
        let current = read(&Inputs::default(), &context).unwrap();
        assert_eq!(current["revision"], 2);
        assert_eq!(current["changed"], false);
        assert_eq!(current["persisted"], "native_local");
        if let Some(previous) = previous {
            unsafe {
                std::env::set_var("DS_LAYER_HOME", previous);
            }
        } else {
            unsafe {
                std::env::remove_var("DS_LAYER_HOME");
            }
        }
    }
}
