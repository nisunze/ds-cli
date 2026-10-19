//! Local reference files, governed by the existing Solar project owner.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

const WORKSPACE: Arg =
    Arg::value("workspace", "<dir>", "Private local Solar workspace.").required();
const PROJECT: Arg = Arg::value(
    "project",
    "<id>",
    "Exact workspace project; grants no cloud authority.",
)
.required();
const CITY: Arg = Arg::value("city", "<id>", "One exact captured local city.").required();
const CACHE: Arg = Arg::value("cache", "<dir>", "Native verified reference cache.").required();

pub static PLAN: Command = Command {
    id: "solar.reference.plan",
    path: &["solar", "reference", "plan"],
    contract: 1,
    summary: "Export one city's native weather/PV reference request.",
    purpose: "The Solar owner checks the exact local workspace/project and retained cache, then writes its selected city's complete reference request to a fresh file. Hand that file to the optional external reference producer. No sign-in, provider, API, Python or calculation runs here; a missing cache remains an explicit preparation need.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        WORKSPACE,
        PROJECT,
        CITY,
        CACHE,
        Arg::value("out", "<path>", "Fresh request JSON file.").required(),
    ],
    output: "Project, city, input digest, retained-cache readiness and exact request-file path. Use this input digest as the later import's expected fence.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/solar.md"),
    search: &["pvlib", "offline"],
    requires: Requires::Server,
    availability: crate::project::availability,
};

pub static IMPORT: Command = Command {
    id: "solar.reference.import",
    path: &["solar", "reference", "import"],
    contract: 1,
    summary: "Verify and retain four local weather/PV reference artifacts.",
    purpose: "The Solar owner admits the explicit local project and expected city-input digest before reading four declared regular, bounded files. It verifies manifest/bytes and site/equipment, commits through its existing native reference cache, and verifies readback. Identical admitted bytes reuse the retained generation. No provider, cloud acquisition, Python, calculation or publication runs; local project attribution grants no cloud access.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[WORKSPACE, PROJECT, CITY, CACHE,
        Arg::value("expected", "<digest>", "Unchanged city input digest from reference plan.").required(),
        Arg::value("bundle", "<dir>", "Directory holding weather.json, reference-unit.json, reference-unit.parquet and manifest.json.").required()],
    output: "Project/city/input digest, verified readiness, cache location, byte count, prepared-input digest and whether the exact retained generation was reused.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/solar.md"),
    search: &["pvlib", "offline"],
    requires: Requires::Server,
    availability: crate::project::availability,
};

pub fn plan(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    plan_with(i, crate::project::invoke)
}
fn plan_with(
    i: &Inputs,
    invoke: impl FnOnce(Value) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    invoke(
        json!({"operation":"reference_plan_file", "workspace":i.require("workspace")?,
        "project":i.require("project")?, "city":i.require("city")?, "cache":i.require("cache")?,
        "out":i.require("out")?}),
    )
}
pub fn import(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    import_with(i, crate::project::invoke)
}
fn import_with(
    i: &Inputs,
    invoke: impl FnOnce(Value) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    invoke(
        json!({"operation":"reference_import_files", "workspace":i.require("workspace")?,
        "project":i.require("project")?, "city":i.require("city")?, "cache":i.require("cache")?,
        "expected":i.require("expected")?, "bundle":i.require("bundle")?}),
    )
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

const REFUSALS: &[Refusal] = &[Refusal {
    code: "engine_refused",
    when: "project, city revision, artifact, path or cache admission refused",
    remedy: "read owner detail; preserve the source bytes and current city, then plan again or correct the named input",
}];

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::parse;

    fn inputs(command: &'static Command, more: &[&str]) -> Inputs {
        let mut tokens = vec![
            "--workspace",
            "/local/work",
            "--project",
            "p",
            "--city",
            "c",
            "--cache",
            "/local/cache",
        ];
        tokens.extend_from_slice(more);
        parse(
            command,
            &tokens.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn plan_transports_exact_scope_and_fresh_output_in_one_owner_call() {
        let result = plan_with(&inputs(&PLAN, &["--out", "/local/request.json"]), |request| {
            assert_eq!(request, json!({"operation":"reference_plan_file","workspace":"/local/work","project":"p","city":"c","cache":"/local/cache","out":"/local/request.json"}));
            Ok(json!({"ready":false}))
        }).unwrap();
        assert_eq!(result["ready"], false);
    }
    #[test]
    fn import_transports_authored_revision_and_local_files_in_one_owner_call() {
        let result = import_with(&inputs(&IMPORT, &["--expected", "sha256:held", "--bundle", "/local/sealed"]), |request| {
            assert_eq!(request, json!({"operation":"reference_import_files","workspace":"/local/work","project":"p","city":"c","cache":"/local/cache","expected":"sha256:held","bundle":"/local/sealed"}));
            Ok(json!({"ready":true,"reused":true}))
        }).unwrap();
        assert_eq!(result["reused"], true);
    }
    #[test]
    fn local_reference_controls_expose_no_remote_acquisition_or_launcher() {
        for command in [&PLAN, &IMPORT] {
            assert!(matches!(command.authority, Authority::None));
            let names = command.args.iter().map(|arg| arg.name).collect::<Vec<_>>();
            for absent in [
                "lane", "token", "api-key", "url", "command", "argv", "python",
            ] {
                assert!(!names.contains(&absent), "{} exposed {absent}", command.id);
            }
        }
    }
}
