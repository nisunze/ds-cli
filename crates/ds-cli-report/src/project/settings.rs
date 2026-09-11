//! `ds report project settings` and `ds report project outputs set` — the
//! project's printing output policy, read and written with no browser.
//!
//! What a stored export setting means, where each selected output may run,
//! which settings row carries the selection and what to do when none does are
//! all `ds-command-kernel::report_formats`. These two commands are the native
//! host for that decision: they restore the native user, read the selected
//! project's configuration through the existing governed `/config/{project}`
//! door, hand the sheets to the kernel, and — for a write — hand the kernel's
//! rows to the one closed configuration change that can save them.
//!
//! No new gateway operation exists for either. The read is the configuration
//! read `design.feeder-limits.read` already makes; the write is the
//! `save_config` POST the browser's Settings page has always sent.

use std::io::Read;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
#[cfg(test)]
use ds_command_kernel::report_formats::Placement;
use ds_command_kernel::report_formats::{
    DesignOutputSelection, ReadinessMode, apply_output_selection, named_layouts, named_setup_id,
    normalize, output_setting_index, stored_output_selection, string_list,
};
use serde_json::{Value, json};

use super::LANE_ARG;

/// The authored selection, as a document. A file rather than flags because a
/// selection is a structure — named printouts with their formats, geospatial
/// and tabular outputs, and the placement matrix — and `ds` does not build
/// structures from argv.
const SELECTION_ARG: Arg = Arg::value(
    "selection",
    "<json-file>",
    "Design output selection document (ds.design-output-selection/v1).",
)
.required();

/// A selection document is small; this bound exists so an accidental path
/// cannot be read into memory, not to constrain authoring.
const MAX_SELECTION_BYTES: usize = 256 * 1024;

const SELECTION_INVALID: Refusal = Refusal {
    code: "invalid_output_selection",
    when: "the selection file is missing, oversized, not JSON, or not a valid design output selection",
    remedy: "read the schema with `ds report layout schema` and correct the document",
};
const SETTINGS_UNREADABLE: Refusal = Refusal {
    code: "project_settings_unreadable",
    when: "the project's settings sheet cannot be read as printing configuration",
    remedy: "inspect the project configuration and repair the settings sheet",
};
/// Naming a global printing setup is not adopting it. The kernel's rule is
/// right and unchanged — `report_formats` refuses an export whose selection
/// names a setup the project does not hold. What was missing was the way out:
/// the refusal arrived at export time, from a command that had no idea which
/// command adopts a setup. It is raised here instead, before the write, and
/// it names the adoption command.
const SETUP_NOT_ADOPTED: Refusal = Refusal {
    code: "print_setup_not_adopted",
    when: "the selection names a printing setup this project does not hold; naming a global template is not adoption",
    remedy: "copy the global setup into this project with `ds report layout copy`, then save the selection again",
};
const CONFIRM: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that changes saved project settings",
    remedy: "run `ds report project settings` first, then re-run with --yes",
};

/// The refusals a configuration read or write can actually answer with. The
/// transformer-scope codes of the compounded family cannot occur here, so
/// they are not advertised: a refusal list is a promise about what may happen.
const CONFIG_REFUSALS: &[Refusal] = &[
    super::NATIVE_PROFILE,
    super::NATIVE_PROFILE_DIGEST,
    super::NATIVE_PROFILE_UNSAFE,
    super::HEADLESS_SIGNED_OUT,
    super::HEADLESS_NO_PROJECT,
    super::PROJECT_CONTEXT_STALE,
    super::NATIVE_STATE_UNSAFE,
    super::NATIVE_STATE_UNAVAILABLE,
    super::NATIVE_STATE_PROTECTION,
    super::NATIVE_STATE_ROOT,
    super::NATIVE_STATE_CONFLICT,
    super::NATIVE_CLEANUP,
    super::AUTH_CONTEXT_MISMATCH,
    super::AUTH_REVOKED,
    super::AUTH_IDENTITY_MISMATCH,
    super::AUTH_REJECTED,
    super::AUTH_TRANSIENT,
    super::AUTH_UNREADABLE,
    super::NOT_FOUND,
    SETTINGS_UNREADABLE,
];

const WRITE_REFUSALS: &[Refusal] = &[
    SELECTION_INVALID,
    SETUP_NOT_ADOPTED,
    CONFIRM,
    super::NATIVE_PROFILE,
    super::NATIVE_PROFILE_DIGEST,
    super::NATIVE_PROFILE_UNSAFE,
    super::HEADLESS_SIGNED_OUT,
    super::HEADLESS_NO_PROJECT,
    super::PROJECT_CONTEXT_STALE,
    super::NATIVE_STATE_UNSAFE,
    super::NATIVE_STATE_UNAVAILABLE,
    super::NATIVE_STATE_PROTECTION,
    super::NATIVE_STATE_ROOT,
    super::NATIVE_STATE_CONFLICT,
    super::NATIVE_CLEANUP,
    super::AUTH_CONTEXT_MISMATCH,
    super::AUTH_REVOKED,
    super::AUTH_IDENTITY_MISMATCH,
    super::AUTH_REJECTED,
    super::AUTH_TRANSIENT,
    super::AUTH_UNREADABLE,
    super::NOT_FOUND,
    SETTINGS_UNREADABLE,
];

pub static COMMAND: Command = Command {
    id: "report.project.settings",
    path: &["report", "project", "settings"],
    contract: 1,
    summary: "Read the project's printing outputs and whether they are ready.",
    purpose: "\
Restores the native user and reads its audience-fenced selected project's \
fresh configuration, then asks ds-command-kernel what that project's export \
setting means: the outputs it will produce, the paper each named printout \
prints on, whether the selected printing setups are actually held, and — when \
they are not — the refusal by message key, so `ds` and the GUI refuse in the \
same words. Reads whatever shape the setting was stored in, including every \
legacy one. Nothing is generated and nothing is saved. No project, Desktop \
descriptor, URL, body or action override is accepted.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG],
    output: "\
Lane and selected-project identity, the settings `source` (the project's own \
row or the report defaults), the stored `setting` row, the resolved `outputs` \
with their formats and suffixes, the `papers` of the named printouts, \
`ready`, any `issues`, and the `refusal` with its code, message key and mode.",
    examples: &[Example {
        command: "ds report project settings --output json",
        note: "`.data.refusal` names why an unready project cannot export, by key.",
        runnable: false,
    }],
    refusals: CONFIG_REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};

pub static OUTPUTS_SET: Command = Command {
    id: "report.project.outputs.set",
    path: &["report", "project", "outputs", "set"],
    contract: 1,
    summary: "Save the project's design output selection.",
    purpose: "\
Validates the selection document against the kernel's closed schema before \
any network call, reads the selected project's fresh configuration, and lets \
ds-command-kernel write the selection into the settings sheet — under \
whichever of the five export-row aliases the project already uses, or a new \
`design_export_format` row when it has none. The patched sheet is saved \
through the governed configuration owner and verified with a fresh read-back; \
every other settings row is preserved exactly. Placement (`execution`) is \
saved as authored and remains what ds-brain admits an export against. Requires \
--yes. Produces no report.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[SELECTION_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity, the export row `parameter` the selection \
was written to, whether the row was created, the saved `selection`, the \
outputs it resolves to and their placements, and `saved`.",
    examples: &[Example {
        command: "ds report project outputs set --selection outputs.json --yes --output json",
        note: "`.data.parameter` names the row the project actually stores its selection in.",
        runnable: false,
    }],
    refusals: WRITE_REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};

fn unreadable(message: &'static str) -> Failure {
    Failure::unavailable(SETTINGS_UNREADABLE.code, message).remedy(SETTINGS_UNREADABLE.remedy)
}

fn invalid_selection(cause: impl std::fmt::Display) -> Failure {
    Failure::invalid(SELECTION_INVALID.code, cause.to_string()).remedy(SELECTION_INVALID.remedy)
}

/// The authored selection, read and validated before anything is restored or
/// requested. An invalid document must cost no network call.
fn selection(inputs: &Inputs) -> Result<DesignOutputSelection, Failure> {
    let path = inputs.require("selection")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(invalid_selection)?
        .take(MAX_SELECTION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid_selection)?;
    if bytes.len() > MAX_SELECTION_BYTES {
        return Err(invalid_selection("the selection document exceeds 256 KiB"));
    }
    let selection: DesignOutputSelection =
        serde_json::from_slice(&bytes).map_err(invalid_selection)?;
    // The kernel's own bounds, applied here so a refusal happens locally.
    selection.tokens().map_err(invalid_selection)?;
    Ok(selection)
}

fn receipt(lane: &str, summary: &Value) -> Value {
    json!({"lane":lane,"project":summary["project"]})
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let configuration = ds_cli_auth::feeder_configuration(lane, None)?;
    let sheets = sheets_with_printing_catalogue(lane, &configuration.document["sheets"], None)?;
    // The native read always refreshes: this client substitutes no cached
    // configuration, so the mode the kernel names its refusal with is not a
    // guess.
    let mut output = ds_command_kernel::report_formats::inspect_project_settings(
        &sheets,
        Some(ReadinessMode::Refreshed),
    )
    .map_err(|_| {
        unreadable("the project's settings sheet is not readable printing configuration")
    })?;
    let receipt = receipt(lane, &configuration.summary);
    output["lane"] = receipt["lane"].clone();
    output["project"] = receipt["project"].clone();
    Ok(output)
}

/// The configuration sheets, completed with the printing setups the selection
/// names under the kernel's `printing_setups` sheet.
///
/// The configuration read serves the settings rows; the printing setups a
/// project holds live in the printing library. Only the reporter's sealed
/// input receipt ever carried that sheet, and it is derived from the saved
/// selection — so before this read, a headless adoption check and the
/// `papers` projection looked for a sheet that never existed at write time and
/// refused every project setup as "not held". The printing list is the
/// catalogue's identity (its rows carry no document); each named setup the
/// project holds is then read exactly, in the `{id, revision, layout}` shape
/// the kernel's `named_layouts` reads. A setup the selection names but the
/// catalogue lacks is left out, so the kernel refuses it by its own rule. A
/// sheet the configuration does serve is kept as served.
fn sheets_with_printing_catalogue(
    lane: &str,
    sheets: &Value,
    selection: Option<&DesignOutputSelection>,
) -> Result<Value, Failure> {
    if sheets.get("printing_setups").is_some() {
        return Ok(sheets.clone());
    }
    let wanted = match selection {
        Some(selection) => selection
            .prints
            .iter()
            .filter(|print| print.enabled)
            .map(|print| print.layout_id.clone())
            .collect::<Vec<_>>(),
        None => stored_setup_ids(sheets),
    };
    if wanted.is_empty() {
        return Ok(with_printing_setups(sheets, Vec::new()));
    }
    let catalogue = ds_cli_auth::printing(lane, false, &ds_cli_auth::PrintingRequest::List {})?;
    let held = catalogue["setups"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row["id"].as_str().map(str::to_owned))
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut setups = Vec::new();
    for id in wanted {
        if !held.contains(&id) {
            continue;
        }
        let setup = ds_cli_auth::printing(
            lane,
            false,
            &ds_cli_auth::PrintingRequest::Get { id: id.clone() },
        )?;
        setups
            .push(json!({"id":setup["id"],"revision":setup["revision"],"layout":setup["layout"]}));
    }
    Ok(with_printing_setups(sheets, setups))
}

/// The printing setups the stored output selection names, read with the
/// kernel's own readers: the export row under any of its aliases, the
/// versioned document or the legacy token list, and the `pdf__<id>` tokens.
fn stored_setup_ids(sheets: &Value) -> Vec<String> {
    let Some(rows) = sheets["project_settings"].as_array() else {
        return Vec::new();
    };
    let Some(index) = output_setting_index(rows) else {
        return Vec::new();
    };
    let value = &rows[index]["value"];
    let tokens = stored_output_selection(value)
        .and_then(|selection| selection.tokens())
        .unwrap_or_else(|_| string_list(value));
    normalize(&tokens)
        .iter()
        .filter_map(|token| named_setup_id(token).map(str::to_owned))
        .collect()
}

/// Pure half of the completion: the served sheets plus the resolved rows,
/// unless the configuration already served the sheet.
fn with_printing_setups(sheets: &Value, setups: Vec<Value>) -> Value {
    let mut sheets = sheets.clone();
    if sheets.get("printing_setups").is_none() {
        sheets["printing_setups"] = Value::Array(setups);
    }
    sheets
}

/// Refuse a selection naming a printing setup this project does not hold.
///
/// The facts are the kernel's: `named_layouts` is the project's printing
/// catalogue, and `enabled` is what `tokens()` uses to decide a print
/// is actually produced. No rule is restated here — only the moment it is
/// applied moves, from the export that would have failed to the write that
/// would have guaranteed it.
fn adoption(sheets: &Value, selection: &DesignOutputSelection) -> Result<(), Failure> {
    let layouts = named_layouts(sheets)
        .map_err(|_| unreadable("the project's sealed printing setup catalogue is not readable"))?;
    let missing = selection
        .prints
        .iter()
        .filter(|print| print.enabled && !layouts.contains_key(&print.layout_id))
        .map(|print| print.layout_id.as_str())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(
        Failure::invalid(
            SETUP_NOT_ADOPTED.code,
            format!(
                "this project holds no printing setup named {}; naming a global template does not adopt it",
                missing.join(", ")
            ),
        )
        .remedy(SETUP_NOT_ADOPTED.remedy)
        .next("ds report layout copy --request <copy.json> --output json"),
    )
}

pub fn set(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let selection = selection(inputs)?;
    let lane = inputs.require("lane")?;
    let configuration = ds_cli_auth::feeder_configuration(lane, None)?;
    let sheets =
        sheets_with_printing_catalogue(lane, &configuration.document["sheets"], Some(&selection))?;
    // A selection that cannot execute must not be saved as if it could.
    adoption(&sheets, &selection)?;
    let mut rows = configuration.document["sheets"]["project_settings"]
        .as_array()
        .cloned()
        .ok_or_else(|| unreadable("the project has no settings sheet to write the selection to"))?;
    let held = rows.len();
    // Which row, under which alias, and whether one must be created is the
    // kernel's answer; this host only carries it to the save.
    let parameter = apply_output_selection(&mut rows, &selection)
        .map_err(|_| unreadable("the project's settings rows cannot carry an output selection"))?;
    let created = rows.len() > held;
    let saved = ds_cli_auth::design_output_rows(lane, rows)?;
    let mut output = receipt(lane, &saved.summary);
    output["parameter"] = json!(parameter);
    output["created"] = json!(created);
    output["selection"] = serde_json::to_value(&selection).unwrap_or(Value::Null);
    output["placements"] = selection
        .placement_map()
        .map(|map| serde_json::to_value(map).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    output["saved"] = json!(true);
    Ok(output)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} · {} · source {}\n",
        data["project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["source"].as_str().unwrap_or("?"),
    );
    if let Some(outputs) = data["outputs"].as_array() {
        for output in outputs {
            out.push_str(&format!(
                "  {:<28} {}\n",
                output["outputId"].as_str().unwrap_or("?"),
                output["suffix"].as_str().unwrap_or(""),
            ));
        }
    }
    if data["ready"] == Value::Bool(true) {
        out.push_str("ready\n");
        return out;
    }
    out.push_str(&format!(
        "refusal {} ({})\n",
        data["refusal"]["code"].as_str().unwrap_or("?"),
        data["refusal"]["mode"].as_str().unwrap_or("?"),
    ));
    if let Some(issues) = data["issues"].as_array() {
        for issue in issues {
            out.push_str(&format!("  {}\n", issue.as_str().unwrap_or("?")));
        }
    }
    out
}

pub fn render_set(data: &Value) -> String {
    let outputs = data["placements"]
        .as_object()
        .map(|map| map.len())
        .unwrap_or(0);
    format!(
        "project {} · {} · {} → {}{} · {} output(s) · saved={}\n",
        data["project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        "design output selection",
        data["parameter"].as_str().unwrap_or("?"),
        if data["created"] == Value::Bool(true) {
            " (row created)"
        } else {
            ""
        },
        outputs,
        data["saved"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One catalogue row in the shape the printing list serves and the
    /// kernel's `named_layouts` reads.
    fn layout(id: &str) -> Value {
        let mut layout = ds_command_kernel::printing::default_layout();
        layout.id = id.to_owned();
        json!({"id":id,"revision":"a".repeat(64),"layout":layout})
    }

    /// A selection enabling (or not) one named PDF print.
    fn select(id: &str, enabled: bool) -> DesignOutputSelection {
        DesignOutputSelection {
            schema: "ds.design-output-selection/v1".to_owned(),
            prints: vec![ds_command_kernel::report_formats::NamedPrintSelection {
                layout_id: id.to_owned(),
                enabled,
                formats: vec![ds_command_kernel::report_formats::PrintArtifactFormat::Pdf],
            }],
            geospatial: Vec::new(),
            tabular: Vec::new(),
            execution: Default::default(),
        }
    }

    /// Both commands are background project work: no map, no room, no Desktop
    /// descriptor, and no project override — the selected project is the one
    /// the native user holds. The write is a write and says so.
    #[test]
    fn both_commands_declare_background_project_authority_and_no_override() {
        for command in [&COMMAND, &OUTPUTS_SET] {
            assert_eq!(command.authority, Authority::HeadlessProject);
            assert_eq!(command.chapter, Chapter::Reports);
            assert!(matches!(command.execution, Execution::Sync));
            let names = command.args.iter().map(|arg| arg.name).collect::<Vec<_>>();
            assert!(names.contains(&"lane"), "{} lost its lane", command.id);
            for forbidden in ["project", "desktop-descriptor", "url", "action", "body"] {
                assert!(
                    !names.contains(&forbidden),
                    "{} accepts a {forbidden} override",
                    command.id
                );
            }
            assert_eq!(command.reference, Some("docs/reference/report.md"));
        }
        assert!(matches!(COMMAND.effect, Effect::LocalAuthState));
        assert!(matches!(OUTPUTS_SET.effect, Effect::GlobalWrite));
        assert_eq!(OUTPUTS_SET.args[0].name, "selection");
    }

    /// A refusal list is a promise about what may happen. Neither command can
    /// answer with a transformer-scope code, so neither advertises one; the
    /// write advertises the two only it can reach.
    #[test]
    fn each_command_advertises_only_refusals_it_can_reach() {
        let codes = |refusals: &[Refusal]| {
            refusals
                .iter()
                .map(|refusal| refusal.code)
                .collect::<Vec<_>>()
        };
        for refusals in [CONFIG_REFUSALS, WRITE_REFUSALS] {
            let codes = codes(refusals);
            assert!(codes.contains(&"headless_project_not_selected"));
            assert!(codes.contains(&SETTINGS_UNREADABLE.code));
            for phantom in [
                "invalid_transformer_scope",
                "reserved_transformer_identity",
                "report_no_individual_artifacts",
            ] {
                assert!(!codes.contains(&phantom), "{phantom} cannot happen here");
            }
        }
        assert!(!codes(CONFIG_REFUSALS).contains(&"confirmation_required"));
        assert!(codes(WRITE_REFUSALS).contains(&"confirmation_required"));
        assert!(codes(WRITE_REFUSALS).contains(&SELECTION_INVALID.code));
        // The write's confirmation remedy sends the operator to the read, not
        // to the compounded family's scope command.
        assert!(CONFIRM.remedy.contains("ds report project settings"));
    }

    /// The served configuration carries no printing catalogue; the resolved
    /// rows complete it in the sealed sheet's shape, the stored selection's
    /// setup ids are read by the kernel's own readers under every stored
    /// shape, and a sheet the configuration does serve is never replaced.
    #[test]
    fn served_sheets_are_completed_with_the_named_printing_setups() {
        let served = json!({"project_settings":[{"parameter":"design_export_format","value":["pdf__project_a3","xlsx"]}]});
        assert_eq!(stored_setup_ids(&served), vec!["project_a3".to_owned()]);
        let versioned = json!({"project_settings":[{"parameter":"tr_export_formats","value":{
            "schema":"ds.design-output-selection/v1",
            "prints":[{"layout_id":"project_a0","enabled":true,"formats":["pdf"]},
                      {"layout_id":"project_a3","enabled":false,"formats":["pdf"]}],
            "geospatial":["kmz"],"tabular":[]}}]});
        assert_eq!(stored_setup_ids(&versioned), vec!["project_a0".to_owned()]);
        assert!(stored_setup_ids(&json!({"project_settings":[]})).is_empty());
        assert!(stored_setup_ids(&json!({})).is_empty());

        let completed = with_printing_setups(&served, vec![layout("project_a3")]);
        assert_eq!(completed["project_settings"], served["project_settings"]);
        assert_eq!(
            completed["printing_setups"].as_array().map(Vec::len),
            Some(1)
        );
        adoption(&completed, &select("project_a3", true)).expect("a held setup is selectable");
        let refusal = adoption(&completed, &select("a3_landscape_rwanda_project_lv", true))
            .expect_err("a setup outside the catalogue is not held");
        assert!(format!("{refusal:?}").contains(SETUP_NOT_ADOPTED.code));
        // Nothing resolved: the sheet is present and empty, so the kernel
        // reads "holds none" rather than "unreadable".
        assert_eq!(
            with_printing_setups(&served, Vec::new())["printing_setups"],
            json!([])
        );
        // A served sheet stays as served.
        let sealed = json!({"printing_setups":[layout("project_a3")]});
        assert_eq!(
            with_printing_setups(&sealed, Vec::new())["printing_setups"],
            sealed["printing_setups"]
        );
    }

    /// An invalid selection costs no network call, because it is refused
    /// before anything is restored. The kernel's bounds are the bounds.
    #[test]
    fn an_invalid_selection_is_refused_locally_by_the_kernels_own_rule() {
        let dir = tempfile::tempdir().expect("temp dir");
        let write = |name: &str, body: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, body).expect("fixture written");
            path.to_string_lossy().into_owned()
        };
        let inputs = |path: &str| {
            let tokens = vec!["--selection".to_owned(), path.to_owned()];
            ds_cli_contract::parse(&OUTPUTS_SET, &tokens).expect("declared inputs")
        };
        let valid = write(
            "valid.json",
            r#"{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"],
                "execution":{"gpkg":["web"]}}"#,
        );
        let read = selection(&inputs(&valid)).expect("a valid selection is accepted");
        assert_eq!(read.placements("gpkg"), [Placement::Web]);
        // Selecting nothing is a state the Settings page can save by
        // unticking every box, so `ds` is not stricter than the surface it
        // replaces: the kernel's bounds are the bounds, and an empty
        // selection is within them.
        let empty = write(
            "empty.json",
            r#"{"schema":"ds.design-output-selection/v1"}"#,
        );
        assert!(
            selection(&inputs(&empty))
                .expect("an empty selection is a selection")
                .tokens()
                .expect("bounds hold")
                .is_empty()
        );
        for (name, body) in [
            (
                "wrong-schema.json",
                r#"{"schema":"ds.something-else/v1","geospatial":["gpkg"]}"#,
            ),
            (
                "unknown-field.json",
                r#"{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"],"outputs":[]}"#,
            ),
            (
                "bad-lane.json",
                r#"{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"],"execution":{"gpkg":["cloud"]}}"#,
            ),
            ("not-json.json", "gpkg,xlsx"),
        ] {
            let path = write(name, body);
            let failure = selection(&inputs(&path)).expect_err(name);
            assert_eq!(failure.code(), SELECTION_INVALID.code, "{name}");
        }
        let missing = dir.path().join("absent.json");
        assert_eq!(
            selection(&inputs(&missing.to_string_lossy()))
                .expect_err("a missing file is refused")
                .code(),
            SELECTION_INVALID.code
        );
    }

    /// The human view is a projection of the machine result: the outputs, then
    /// one word about readiness — and when it is not ready, the kernel's code,
    /// the mode it was read in, and its own findings. No second English
    /// ending is composed here; the GUI resolves the message key instead.
    #[test]
    fn the_human_view_shows_the_outputs_then_ready_or_the_kernels_refusal() {
        let ready = json!({"project":"aderm_loc7","lane":"stable","source":"project_settings",
            "ready":true,"outputs":[{"outputId":"gpkg","suffix":".gpkg"}],"refusal":Value::Null});
        let text = render(&ready);
        assert!(text.contains("aderm_loc7"), "{text}");
        assert!(text.contains("gpkg"), "{text}");
        assert!(text.trim_end().ends_with("ready"), "{text}");
        let refused = json!({"project":"aderm_loc7","lane":"stable","source":"project_settings",
            "ready":false,"outputs":[],"issues":["Selected printing setup a0-review is missing"],
            "refusal":{"code":"printing_inputs_incomplete",
                "message_key":"printing_inputs_incomplete_refreshed","mode":"refreshed"}});
        let text = render(&refused);
        assert!(
            text.contains("refusal printing_inputs_incomplete (refreshed)"),
            "{text}"
        );
        assert!(text.contains("a0-review"), "{text}");
        assert!(!text.contains("ready\n"), "{text}");
        let saved = json!({"project":"aderm_loc7","lane":"stable","parameter":"tr_export_formats",
            "created":true,"placements":{"gpkg":["web"]},"saved":true});
        let text = render_set(&saved);
        assert!(text.contains("tr_export_formats"), "{text}");
        assert!(text.contains("row created"), "{text}");
        assert!(text.contains("saved=true"), "{text}");
    }

    /// The whole production failure in one test: a project selects a global
    /// printout it never adopted, and every surface that could have said so
    /// said something else. The write is refused before it happens, and the
    /// refusal names the command that fixes it.
    #[test]
    fn a_selection_naming_an_unadopted_setup_is_refused_with_the_adoption_command() {
        let sheets = json!({"printing_setups":[layout("project_a3")]});

        // The setup the project actually holds is accepted.
        adoption(&sheets, &select("project_a3", true)).expect("an adopted setup is selectable");
        // A global template the project has merely NAMED is not.
        let refusal = adoption(&sheets, &select("a3_landscape_rwanda_project_lv", true))
            .expect_err("naming a global template is not adoption");
        let rendered = format!("{refusal:?}");
        assert!(rendered.contains("print_setup_not_adopted"), "{rendered}");
        assert!(
            rendered.contains("a3_landscape_rwanda_project_lv"),
            "{rendered}"
        );
        assert!(rendered.contains("ds report layout copy"), "{rendered}");
        assert_eq!(refusal.remedy_text(), Some(SETUP_NOT_ADOPTED.remedy));
        assert!(
            refusal
                .next_commands()
                .iter()
                .any(|command| command.starts_with("ds report layout copy")),
            "{rendered}"
        );

        // A print the operator switched off produces nothing, so it cannot
        // make an export fail and must not block the save.
        adoption(&sheets, &select("a3_landscape_rwanda_project_lv", false))
            .expect("a disabled print is not selected");
        // A project with no sealed catalogue at all refuses the same way.
        assert!(adoption(&json!({}), &select("project_a3", true)).is_err());
    }
}
