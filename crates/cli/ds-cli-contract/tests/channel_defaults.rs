//! A channel binary defaults `--lane` to its own channel, and a command with
//! several omitted required inputs names them all at once. One test function:
//! the channel lane is process-wide and set once.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, LANE, Requires,
    set_channel_lane,
};
use ds_cli_contract::{args, help};

static ARGS: &[Arg] = &[
    Arg::value("view", "<projects|store>", "View.").required(),
    LANE,
    Arg::value("out", "<absolute-path>", "Output.").required(),
];

static COMMAND: Command = Command {
    id: "test.channel",
    path: &["test", "channel"],
    contract: 1,
    chapter: Chapter::Catalog,
    summary: "Fixture.",
    purpose: "Fixture for channel defaults.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: ARGS,
    output: "Nothing.",
    examples: &[],
    refusals: &[],
    reference: None,
    search: &[],
    requires: Requires::Server,
    availability: || Availability::Available,
};

fn parse(tokens: &[&str]) -> Result<ds_cli_contract::Inputs, ds_cli_contract::Failure> {
    args::parse(&COMMAND, &tokens.iter().map(|t| t.to_string()).collect::<Vec<_>>())
}

#[test]
fn channel_lane_default_and_all_missing_inputs() {
    // Unset process: the declaration's stable default.
    assert_eq!(parse(&["--view", "store", "--out", "/x"]).unwrap().value("lane"), Some("stable"));

    set_channel_lane("canary");
    let omitted = parse(&["--view", "store", "--out", "/x"]).unwrap();
    assert_eq!(omitted.value("lane"), Some("canary"));
    let explicit = parse(&["--view", "store", "--out", "/x", "--lane", "stable"]).unwrap();
    assert_eq!(explicit.value("lane"), Some("stable"));
    assert!(help::command(&COMMAND).contains("(default: canary)"));
    let summary = LANE.summary("Deployment lane; stable is the default.");
    assert_eq!(summary.display_summary(), "Deployment lane; canary is the default.");
    assert_eq!(LANE.summary("Native lane; defaults to stable.").display_summary(), "Native lane; defaults to canary.");

    // Several omissions are reported together; one keeps its own sentence.
    let both = parse(&[]).unwrap_err();
    assert_eq!(both.code(), "missing_input");
    assert_eq!(both.message(), "`--view` and `--out` are required");
    assert_eq!(
        both.detail_value().unwrap()["missing"],
        serde_json::json!(["view", "out"])
    );
    assert_eq!(
        both.remedy_text(),
        Some("pass `--view <projects|store>` and `--out <absolute-path>`")
    );
    let one = parse(&["--view", "store"]).unwrap_err();
    assert_eq!(one.message(), "`--out` is required");
    assert!(one.detail_value().is_none());
}
