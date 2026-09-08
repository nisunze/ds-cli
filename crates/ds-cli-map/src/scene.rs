//! Headless scene preparation; deterministic composition belongs to the kernel.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;

pub static COMMAND: Command = Command {
    id: "map.scene.build",
    path: &["map", "scene", "build"],
    contract: 1,
    summary: "Build a portable Cesium display scene without opening a map.",
    purpose: "Validates and composes an explicit local scene through ds-command-kernel. The request carries a geographic view, embedded GeoJSON, PMTiles or raster references, Styles API layer definitions and optional 3D Tiles. Writes a digest-pinned scene artifact atomically without overwriting. No network fetch, login, UI project switch or engineering computation occurs. Provider credentials and signed URLs are refused; Google binds through the viewer's authenticated config at runtime.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("request", "<json-file>", "Local ds.map.scene.request/v1 document, at most 32 MiB; see the scene contract in docs/reference/map.md.").required(),
        Arg::value("out", "<absolute-file>", "New .scene.json artifact path; the parent must exist and the output must not.").required(),
    ],
    output: "Artifact path, scene SHA-256, byte size, layer/source/embedded-feature counts and network_fetched false. No scene payload or credentials.",
    examples: &[Example { command: "ds map scene build --request scene-request.json --out /work/site.scene.json --output json", note: "Prepare a scene headlessly, then open the artifact in the main map's Cesium view.", runnable: false }],
    refusals: &[
        Refusal { code: "scene_request_unreadable", when: "the local request file cannot be read", remedy: "pass a readable bounded local JSON file" },
        Refusal { code: "scene_request_invalid", when: "the kernel refuses schema, bounds, source URLs or layer references", remedy: "correct the request using the scene contract; do not embed credentials or signed URLs" },
        Refusal { code: "scene_output_unwritable", when: "the output is relative, already exists, or cannot be written atomically", remedy: "choose a new absolute output path in an existing writable directory" },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::layer::local_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = inputs.require("request")?;
    let output = inputs.require("out")?;
    let unreadable = |_| {
        Failure::invalid(
            "scene_request_unreadable",
            "could not read the scene request",
        )
        .remedy("pass a readable bounded local JSON file")
    };
    let mut bytes = Vec::new();
    std::fs::File::open(request)
        .map_err(unreadable)?
        .take(ds_command_kernel::map_scene::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    let artifact = ds_command_kernel::map_scene::build(&bytes).map_err(|error| {
        Failure::invalid("scene_request_invalid", error).remedy(
            "correct the request using the scene contract; do not embed credentials or signed URLs",
        )
    })?;
    let unwritable = || {
        Failure::invalid(
            "scene_output_unwritable",
            "scene output must be a new absolute file in a writable directory",
        )
        .remedy("choose a new absolute output path in an existing writable directory")
    };
    let target = Path::new(output);
    if !target.is_absolute() || target.exists() {
        return Err(unwritable());
    }
    let encoded = serde_json::to_vec(&artifact).map_err(|_| unwritable())?;
    let mut staging = tempfile::NamedTempFile::new_in(target.parent().ok_or_else(unwritable)?)
        .map_err(|_| unwritable())?;
    staging.write_all(&encoded).map_err(|_| unwritable())?;
    staging.as_file().sync_all().map_err(|_| unwritable())?;
    staging
        .persist_noclobber(target)
        .map_err(|_| unwritable())?;
    Ok(
        json!({"path":output,"scene_sha256":artifact["sha256"],"bytes":encoded.len(),"summary":artifact["summary"],"network_fetched":false}),
    )
}
pub fn render(data: &Value) -> String {
    format!(
        "{} · scene {} · {} bytes · prepared headlessly\n",
        data["path"].as_str().unwrap_or(""),
        data["scene_sha256"].as_str().unwrap_or(""),
        data["bytes"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::{Format, Output, parse};
    #[test]
    fn scene_build_writes_verified_artifact_and_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let request = dir.path().join("request.json");
        let output = dir.path().join("site.scene.json");
        std::fs::write(&request, serde_json::to_vec(&json!({
            "schema":"ds.map.scene.request/v1","name":"headless", "view":{"center":[30,-2],"zoom":14,"pitch":50,"bearing":0},
            "sources":{"g":{"type":"google-3d"}}, "layers":[{"id":"context","source":"g","type":"3d-tiles"}]
        })).unwrap()).unwrap();
        let inputs = parse(
            &COMMAND,
            &[
                "--request".into(),
                request.to_str().unwrap().into(),
                "--out".into(),
                output.to_str().unwrap().into(),
            ],
        )
        .unwrap();
        let context = Context {
            confirmed: false,
            output: Output::resolve(Format::Json, false, true),
        };
        let receipt = run(&inputs, &context).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        let scene = ds_command_kernel::map_scene::validate_artifact(&bytes).unwrap();
        assert_eq!(receipt["scene_sha256"], scene["sha256"]);
        assert_eq!(receipt["network_fetched"], false);
        assert_eq!(receipt["summary"]["layers"], 1);
        assert_eq!(
            run(&inputs, &context).unwrap_err().code(),
            "scene_output_unwritable"
        );
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
    }
}
